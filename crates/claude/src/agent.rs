//! [`ClaudeAgent`]: the [`Agent`] implementation that runs the `claude` program.
//!
//! One session is one child process. Its reader thread turns each output line
//! into events with a [`Translator`] and hands them to the sink, then sends the
//! requests the translator asks for; the session handle writes commands to the
//! child. The translator and the counter of request ids are the only state both
//! sides touch, and the translator's lock is never held while writing to the
//! child or calling the sink.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use mr_agent::{
    Account, Agent, AgentError, Capabilities, EventSink, ImageData, Removal, SessionCommand,
    SessionEvent, SessionHandle, SessionId, StartSpec, StoredHistory, StoredSession,
};
use serde_json::Value;

use crate::account::detect;
use crate::args::build_args;
use crate::encode::{
    control_request, generate_session_title, get_usage, initialize, interrupt, permission_reply,
    remote_control, rename_session, set_effort, set_model, set_permission_mode, unsupported_reply,
    user_message,
};
use crate::env::child_env;
use crate::history::{DescriptionTable, find_session, modified_unix, read_image, stored_sessions};
use crate::history_tail::{TAIL_BYTES, read_tail};
use crate::process::{ClaudeProcess, ExitInfo, SpawnConfig};
use crate::remove::{no_store, remove, root_from};
use crate::translate::{SessionStart, Tracked, Translator};
use crate::wire::{Incoming, parse_line};

/// How long a closed session may take to exit before it is killed.
const CLOSE_GRACE: Duration = Duration::from_secs(2);
/// How often a closing session checks whether the child has exited.
const CLOSE_POLL: Duration = Duration::from_millis(10);

/// Runs `claude` sessions.
pub struct ClaudeAgent {
    program: PathBuf,
    /// Claude Code's folder, where sessions are listed, read and removed; none when it is
    /// not known, and then listing and removing answer that the store could not be found.
    root: Option<PathBuf>,
    projects_dir: Option<PathBuf>,
    parent_env: Vec<(String, String)>,
    /// Whether the last sign-in check found a saved full login.
    full_login: Mutex<Option<bool>>,
    /// What each stored session file said about itself, so an unchanged one is not read again.
    descriptions: Mutex<DescriptionTable>,
}

impl ClaudeAgent {
    /// The `claude` found on the `PATH`, the stored sessions in Claude Code's own folder
    /// (`CLAUDE_CONFIG_DIR`, else `~/.claude`) and this process's environment. Without an
    /// absolute folder there it has no store: listing and removing are errors.
    pub fn from_environment() -> Self {
        let root = root_from(
            std::env::var_os("CLAUDE_CONFIG_DIR").as_deref(),
            std::env::home_dir().as_deref(),
        );
        Self::with_store(PathBuf::from("claude"), root, std::env::vars().collect())
    }

    /// `root` is Claude Code's folder; the stored sessions are in its `projects`. A root that
    /// is not absolute is no store. A bare `program` name is looked up on the `PATH` in
    /// `parent_env`.
    pub fn with_paths(program: PathBuf, root: PathBuf, parent_env: Vec<(String, String)>) -> Self {
        Self::with_store(program, Some(root), parent_env)
    }

    fn with_store(
        program: PathBuf,
        root: Option<PathBuf>,
        parent_env: Vec<(String, String)>,
    ) -> Self {
        let root = root.filter(|root| root.is_absolute());
        Self {
            program: resolve_program(program, &parent_env),
            projects_dir: root.as_ref().map(|root| root.join("projects")),
            root,
            parent_env,
            full_login: Mutex::new(None),
            descriptions: Mutex::new(DescriptionTable::default()),
        }
    }

    /// The file of stored session `id`, when there is a store and it holds one.
    fn find_session(&self, id: &SessionId) -> Option<PathBuf> {
        find_session(self.projects_dir.as_deref()?, id)
    }

    fn has_full_login(&self) -> bool {
        let cached = *lock(&self.full_login);
        match cached {
            Some(known) => known,
            None => {
                self.account();
                lock(&self.full_login).unwrap_or(false)
            }
        }
    }
}

impl Agent for ClaudeAgent {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            remote_access: true,
            plan_mode: true,
            usage_limits: true,
            images: true,
        }
    }

    fn account(&self) -> Account {
        let account = detect(&self.program, self.parent_env.clone());
        let full_login = matches!(
            account,
            Account::SignedIn {
                full_login: true,
                ..
            }
        );
        *lock(&self.full_login) = Some(full_login);
        account
    }

    fn list_sessions(
        &self,
        modified_since_unix: Option<i64>,
    ) -> Result<Vec<StoredSession>, AgentError> {
        // Not an empty list: that would say every stored session is gone.
        let Some(projects_dir) = &self.projects_dir else {
            return Err(no_store());
        };
        stored_sessions(
            projects_dir,
            modified_since_unix,
            &mut lock(&self.descriptions),
        )
    }

    fn read_session(&self, id: &SessionId, max_items: usize) -> Result<StoredHistory, AgentError> {
        let path = self
            .find_session(id)
            .ok_or_else(|| AgentError::Protocol(format!("no stored session {}", id.0)))?;
        read_tail(&path, max_items, TAIL_BYTES)
    }

    fn read_image(
        &self,
        id: &SessionId,
        message: &str,
        index: usize,
    ) -> Result<ImageData, AgentError> {
        let Some(projects_dir) = &self.projects_dir else {
            return Err(AgentError::Io("the session is not in the store".into()));
        };
        read_image(projects_dir, id, message, index)
    }

    fn last_active(&self, id: &SessionId) -> Result<Option<i64>, AgentError> {
        match self.find_session(id) {
            // A file that vanished since it was found is as good as none.
            Some(path) => modified_unix(&path),
            None => Ok(None),
        }
    }

    fn remove_session(
        &self,
        id: &SessionId,
        unless_after_unix: i64,
    ) -> Result<Removal, AgentError> {
        match &self.root {
            Some(root) => remove(root, id, unless_after_unix),
            // Not `Missing`: the session is not known to be gone, so it stays deleted.
            None => Err(no_store()),
        }
    }

    /// The session id is always chosen here: a new one for a new or forked
    /// session, the old one for a plain resume. `Started` is given at the
    /// reply to `initialize`, before any turn.
    fn open(&self, spec: StartSpec, sink: EventSink) -> Result<Box<dyn SessionHandle>, AgentError> {
        let full_login = self.has_full_login();
        let id = match &spec.resume {
            Some(old) if !spec.fork => old.clone(),
            _ => SessionId(uuid::Uuid::new_v4().to_string()),
        };
        let start = SessionStart {
            id: id.clone(),
            folder: spec.folder.clone(),
            model: spec.model.clone(),
            permission_mode: spec.permission_mode.clone(),
        };
        let config = SpawnConfig {
            program: self.program.clone(),
            args: build_args(&spec, Some(&id)),
            cwd: spec.folder,
            env: child_env(self.parent_env.clone(), full_login),
        };

        let translator = Arc::new(Mutex::new(Translator::for_session(start)));
        let next_request = Arc::new(AtomicU64::new(1));
        let sink = Arc::new(Mutex::new(sink));
        let exited = Arc::new(AtomicBool::new(false));
        // The reader needs the process to answer requests, and the process
        // needs the reader's callback to exist, so the link is set afterwards.
        // It is weak: the reader must not keep the child alive.
        let process_link: Arc<OnceLock<Weak<ClaudeProcess>>> = Arc::new(OnceLock::new());

        let reader = Reader {
            translator: Arc::clone(&translator),
            next_request: Arc::clone(&next_request),
            sink: Arc::clone(&sink),
            process: Arc::clone(&process_link),
            ignored: Mutex::new(HashSet::new()),
        };
        let process = ClaudeProcess::spawn(config, move |line| reader.line(&line), {
            let exited = Arc::clone(&exited);
            move |info| {
                exited.store(true, Ordering::SeqCst);
                emit(&sink, exit_event(info));
            }
        })?;
        let process = Arc::new(process);
        let _ = process_link.set(Arc::downgrade(&process));

        let session = ClaudeSession {
            process,
            translator,
            next_request,
            exited,
            closing: AtomicBool::new(false),
        };
        session.request(Some(Tracked::Initialize), initialize())?;
        Ok(Box::new(session))
    }
}

/// Everything the thread reading the child's output needs.
struct Reader {
    translator: Arc<Mutex<Translator>>,
    /// Shared with the session handle, so the requests of both have their own ids.
    next_request: Arc<AtomicU64>,
    sink: Arc<Mutex<EventSink>>,
    process: Arc<OnceLock<Weak<ClaudeProcess>>>,
    /// The kinds of unrecognised line already reported on stderr.
    ignored: Mutex<HashSet<String>>,
}

impl Reader {
    fn line(&self, line: &str) {
        let message = parse_line(line);
        match &message {
            Incoming::Ignored { kind } => self.note_ignored(kind),
            Incoming::ControlRequest {
                request_id,
                subtype,
                ..
            } if subtype != "can_use_tool" => {
                self.refuse(request_id);
                return;
            }
            _ => {}
        }
        // The lock ends with this block, before the sink is called. What is read is never
        // printed or kept here: the reply to `get_settings` holds the user's settings, and the
        // one to `get_context_usage` names their memory files.
        let (events, requests) = {
            let mut translator = lock(&self.translator);
            (translator.push(message), translator.take_requests())
        };
        for event in events {
            emit(&self.sink, event);
        }
        self.send(requests);
    }

    /// Queues the requests the translator asked for, as the session handle queues its own. The
    /// reader never waits for a write.
    fn send(&self, requests: Vec<(Tracked, Value)>) {
        if requests.is_empty() {
            return;
        }
        let Some(process) = self.process.get().and_then(Weak::upgrade) else {
            return;
        };
        for (tracked, body) in requests {
            // A request that cannot be queued is dropped: the session is ending.
            let _ = request(
                &self.translator,
                &self.next_request,
                &process,
                Some(tracked),
                body,
            );
        }
    }

    /// Reports a kind of line this adapter does not read, once per kind, unless it is known to
    /// be harmless.
    fn note_ignored(&self, kind: &str) {
        if worth_reporting(kind) && lock(&self.ignored).insert(kind.to_string()) {
            eprintln!("mindrune: ignored a `{kind}` line from claude");
        }
    }

    /// Tells `claude` this adapter does not handle a request, so it stops waiting.
    fn refuse(&self, request_id: &str) {
        let Some(process) = self.process.get().and_then(Weak::upgrade) else {
            return;
        };
        // A refused line means the child is going away, which the exit reports.
        let _ = process.write_line(&unsupported_reply(request_id));
    }
}

/// Kinds of line `claude` sends that the adapter has no use for and that are known to be
/// harmless: hooks starting and answering, and a status line that carries no permission mode
/// (one that carries it is read, so it is never ignored).
const HARMLESS_KINDS: [&str; 3] = [
    "system/hook_started",
    "system/hook_response",
    "system/status",
];

/// Whether an ignored line of this kind is worth a note on stderr: it is not known to be harmless.
fn worth_reporting(kind: &str) -> bool {
    !HARMLESS_KINDS.contains(&kind)
}

fn exit_event(info: ExitInfo) -> SessionEvent {
    SessionEvent::Exited {
        clean: info.code == Some(0),
        detail: info.stderr_tail.trim().to_string(),
    }
}

/// Calls the sink with no other lock held.
fn emit(sink: &Mutex<EventSink>, event: SessionEvent) {
    (lock(sink))(event);
}

/// One running session, as the caller sees it.
struct ClaudeSession {
    process: Arc<ClaudeProcess>,
    translator: Arc<Mutex<Translator>>,
    next_request: Arc<AtomicU64>,
    exited: Arc<AtomicBool>,
    closing: AtomicBool,
}

/// Queues a control request under the next id, `rc_1`, `rc_2`, …. What it asks
/// for is tracked first, so the reply cannot arrive before it is expected.
fn request(
    translator: &Mutex<Translator>,
    next_request: &AtomicU64,
    process: &ClaudeProcess,
    tracked: Option<Tracked>,
    body: Value,
) -> Result<(), AgentError> {
    let id = format!("rc_{}", next_request.fetch_add(1, Ordering::SeqCst));
    if let Some(tracked) = tracked {
        lock(translator).track(&id, tracked);
    }
    process.write_line(&control_request(&id, body))
}

impl ClaudeSession {
    fn request(&self, tracked: Option<Tracked>, body: Value) -> Result<(), AgentError> {
        request(
            &self.translator,
            &self.next_request,
            &self.process,
            tracked,
            body,
        )
    }

    /// Closes the child's input and returns at once. A child still running after
    /// [`CLOSE_GRACE`] is killed.
    fn close(&self) -> Result<(), AgentError> {
        if self.exited.load(Ordering::SeqCst) {
            return Err(AgentError::Closed);
        }
        if self.closing.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.process.close_input();
        let process = Arc::clone(&self.process);
        let exited = Arc::clone(&self.exited);
        thread::spawn(move || {
            let deadline = Instant::now() + CLOSE_GRACE;
            while !exited.load(Ordering::SeqCst) {
                if Instant::now() >= deadline {
                    process.kill();
                    return;
                }
                thread::sleep(CLOSE_POLL);
            }
        });
        Ok(())
    }
}

impl SessionHandle for ClaudeSession {
    fn send(&self, command: SessionCommand) -> Result<(), AgentError> {
        match command {
            SessionCommand::Send { text, attachments } => {
                // Only queued: a write that fails later is heard as the session's end.
                self.process.write_line(&user_message(&text, &attachments))
            }
            SessionCommand::Answer { id, answer } => {
                // The lock ends with this statement, before anything is written.
                let reply = lock(&self.translator).answer(&id, answer)?;
                self.process.write_line(&permission_reply(&id.0, &reply))
            }
            SessionCommand::Stop => self.request(None, interrupt()),
            SessionCommand::SetModel(model) => {
                self.request(Some(Tracked::SetModel(model.clone())), set_model(&model))
            }
            SessionCommand::SetPermissionMode(mode) => self.request(
                Some(Tracked::SetMode(mode.clone())),
                set_permission_mode(&mode),
            ),
            SessionCommand::SetRemoteAccess { on } => {
                let tracked = if on {
                    Tracked::RemoteOn
                } else {
                    Tracked::RemoteOff
                };
                self.request(Some(tracked), remote_control(on, None))
            }
            SessionCommand::Rename(title) => {
                self.request(Some(Tracked::Rename(title.clone())), rename_session(&title))
            }
            SessionCommand::SuggestTitle { from } => {
                self.request(Some(Tracked::SuggestTitle), generate_session_title(&from))
            }
            SessionCommand::RefreshUsage => self.request(Some(Tracked::Usage), get_usage()),
            SessionCommand::SetEffort(level) => {
                self.request(Some(Tracked::SetEffort(level.clone())), set_effort(&level))
            }
            SessionCommand::Close => self.close(),
        }
    }
}

/// A bare program name becomes the first match on the `PATH` in `env`. A name
/// with a directory, or one found nowhere, is kept; starting it then fails the
/// usual way.
fn resolve_program(program: PathBuf, env: &[(String, String)]) -> PathBuf {
    let has_directory = program
        .parent()
        .is_some_and(|dir| !dir.as_os_str().is_empty());
    if has_directory {
        return program;
    }
    let Some((_, path)) = env.iter().find(|(key, _)| key == "PATH") else {
        return program;
    };
    std::env::split_paths(path)
        .map(|dir| dir.join(&program))
        .find(|candidate| is_executable(candidate))
        .unwrap_or(program)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unknown_kinds_of_line_are_reported() {
        for harmless in [
            "system/hook_started",
            "system/hook_response",
            "system/status",
        ] {
            assert!(!worth_reporting(harmless), "{harmless}");
        }
        for unknown in ["system/brand_new", "not-json", "user", "system/hook"] {
            assert!(worth_reporting(unknown), "{unknown}");
        }
        // A status line that carries a permission mode is read, never ignored.
        let status =
            r#"{"type":"system","subtype":"status","status":null,"permissionMode":"plan"}"#;
        assert!(!matches!(parse_line(status), Incoming::Ignored { .. }));
        // A record of background tasks is read, whatever its list holds. With an empty list it
        // was seen live (twice in `v1-remote.jsonl`); never with a task in it.
        for tasks in [r#","tasks":[]"#, r#","tasks":"x""#, ""] {
            let record = format!(
                r#"{{"type":"system","subtype":"background_tasks_changed"{tasks},"uuid":"u"}}"#
            );
            assert!(
                !matches!(parse_line(&record), Incoming::Ignored { .. }),
                "{record}"
            );
        }
    }

    #[test]
    fn a_stored_session_knows_when_it_was_last_active() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("projects/-home-alex-proj");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("s1.jsonl"),
            r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"content":"hello"}}"#,
        )
        .unwrap();
        let agent = ClaudeAgent::with_paths("claude".into(), root.path().to_path_buf(), vec![]);
        assert!(matches!(
            agent.last_active(&SessionId("s1".into())),
            Ok(Some(unix)) if unix > 0
        ));
        assert_eq!(agent.last_active(&SessionId("s2".into())), Ok(None));
    }

    #[test]
    fn the_agent_removes_from_the_store_it_reads() {
        let root = tempfile::tempdir().unwrap();
        let id = SessionId("0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5f".into());
        let folder = root.path().join("projects/-home-alex-proj");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(format!("{}.jsonl", id.0)), "{}\n").unwrap();
        let agent = ClaudeAgent::with_paths("claude".into(), root.path().to_path_buf(), vec![]);
        assert!(matches!(agent.last_active(&id), Ok(Some(_))));
        assert_eq!(agent.remove_session(&id, i64::MAX), Ok(Removal::Removed));
        assert_eq!(agent.last_active(&id), Ok(None));
        assert!(folder.is_dir());
    }

    #[test]
    fn without_a_home_there_is_no_store() {
        let agent = ClaudeAgent::with_store("claude".into(), root_from(None, None), vec![]);
        let id = SessionId("0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5f".into());
        // An error, not an empty list: an empty list would say every session is gone.
        assert_eq!(agent.list_sessions(None), Err(no_store()));
        assert_eq!(agent.list_sessions(Some(0)), Err(no_store()));
        assert_eq!(agent.last_active(&id), Ok(None));
        assert!(agent.read_session(&id, 10).is_err());
        assert!(agent.read_image(&id, "m1", 0).is_err());
        assert_eq!(
            agent.remove_session(&id, i64::MAX),
            Err(AgentError::Io("the store could not be found".into()))
        );
        assert!(agent.root.is_none());
    }

    #[test]
    fn a_relative_root_given_to_the_agent_is_no_store() {
        let agent = ClaudeAgent::with_paths("claude".into(), "relative-store".into(), vec![]);
        let id = SessionId("0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5f".into());
        assert!(agent.root.is_none());
        assert_eq!(agent.list_sessions(None), Err(no_store()));
        assert_eq!(
            agent.remove_session(&id, i64::MAX),
            Err(AgentError::Io("the store could not be found".into()))
        );
    }

    #[test]
    fn a_root_that_is_not_there_lists_as_an_error() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("not-made-yet");
        let agent = ClaudeAgent::with_paths("claude".into(), root.clone(), vec![]);
        let id = SessionId("0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5f".into());
        let no_store = AgentError::Io("the store could not be found".into());
        assert_eq!(agent.list_sessions(None), Err(no_store.clone()));
        assert_eq!(agent.list_sessions(Some(0)), Err(no_store.clone()));
        assert_eq!(agent.remove_session(&id, i64::MAX), Err(no_store));
        assert!(!root.exists(), "nothing is made there");
    }

    #[cfg(unix)]
    #[test]
    fn projects_linked_to_nowhere_lists_as_an_error() {
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(root.path().join("elsewhere"), root.path().join("projects"))
            .unwrap();
        let agent = ClaudeAgent::with_paths("claude".into(), root.path().to_path_buf(), vec![]);
        let id = SessionId("0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5f".into());
        let no_store = AgentError::Io("the store could not be found".into());
        assert_eq!(agent.list_sessions(None), Err(no_store.clone()));
        assert_eq!(agent.remove_session(&id, i64::MAX), Err(no_store));
    }

    #[test]
    fn a_store_with_no_projects_yet_lists_as_empty() {
        // A fresh install: Claude Code's folder is there and nothing was stored in it yet.
        let root = tempfile::tempdir().unwrap();
        let agent = ClaudeAgent::with_paths("claude".into(), root.path().to_path_buf(), vec![]);
        assert_eq!(agent.list_sessions(None), Ok(Vec::new()));
        assert_eq!(agent.list_sessions(Some(0)), Ok(Vec::new()));
        // Removing is another matter: where nothing is stored, no session is known to be gone.
        let id = SessionId("0b5f3c2e-6a1d-4c8e-9f20-7d3b1a2c4e5f".into());
        assert_eq!(agent.remove_session(&id, i64::MAX), Err(no_store()));
    }
}
