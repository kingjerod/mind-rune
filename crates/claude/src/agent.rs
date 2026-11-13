//! [`ClaudeAgent`]: the [`Agent`] implementation that runs the `claude` program.
//!
//! One session is one child process. Its reader thread turns each output line
//! into events with a [`Translator`] and hands them to the sink; the session
//! handle writes commands to the child. The translator is the only state both
//! sides touch, and its lock is never held while writing to the child or
//! calling the sink.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use mr_agent::{
    Account, Agent, AgentError, Capabilities, EventSink, HistoryItem, SessionCommand, SessionEvent,
    SessionHandle, SessionId, StartSpec, StoredSession,
};
use serde_json::Value;

use crate::account::detect;
use crate::args::build_args;
use crate::encode::{
    control_request, initialize, interrupt, permission_reply, remote_control, rename_session,
    set_model, set_permission_mode, unsupported_reply, user_message,
};
use crate::env::child_env;
use crate::history::{find_session, read_history, stored_sessions};
use crate::process::{ClaudeProcess, ExitInfo, SpawnConfig};
use crate::translate::{SessionStart, Tracked, Translator};
use crate::wire::{Incoming, parse_line};

/// How long a closed session may take to exit before it is killed.
const CLOSE_GRACE: Duration = Duration::from_secs(2);
/// How often a closing session checks whether the child has exited.
const CLOSE_POLL: Duration = Duration::from_millis(10);

/// Runs `claude` sessions.
pub struct ClaudeAgent {
    program: PathBuf,
    projects_dir: PathBuf,
    parent_env: Vec<(String, String)>,
    /// Whether the last sign-in check found a saved full login.
    full_login: Mutex<Option<bool>>,
}

impl ClaudeAgent {
    /// The `claude` found on the `PATH`, the stored sessions in
    /// `~/.claude/projects` and this process's environment.
    pub fn from_environment() -> Self {
        let projects_dir = std::env::home_dir()
            .unwrap_or_default()
            .join(".claude")
            .join("projects");
        Self::with_paths(
            PathBuf::from("claude"),
            projects_dir,
            std::env::vars().collect(),
        )
    }

    /// A bare `program` name is looked up on the `PATH` in `parent_env`.
    pub fn with_paths(
        program: PathBuf,
        projects_dir: PathBuf,
        parent_env: Vec<(String, String)>,
    ) -> Self {
        Self {
            program: resolve_program(program, &parent_env),
            projects_dir,
            parent_env,
            full_login: Mutex::new(None),
        }
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
        Ok(stored_sessions(&self.projects_dir, modified_since_unix))
    }

    fn read_session(&self, id: &SessionId) -> Result<Vec<HistoryItem>, AgentError> {
        let path = find_session(&self.projects_dir, id)
            .ok_or_else(|| AgentError::Protocol(format!("no stored session {}", id.0)))?;
        read_history(&path)
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
        let sink = Arc::new(Mutex::new(sink));
        let exited = Arc::new(AtomicBool::new(false));
        // The reader needs the process to answer requests, and the process
        // needs the reader's callback to exist, so the link is set afterwards.
        // It is weak: the reader must not keep the child alive.
        let process_link: Arc<OnceLock<Weak<ClaudeProcess>>> = Arc::new(OnceLock::new());

        let reader = Reader {
            translator: Arc::clone(&translator),
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
            next_request: AtomicU64::new(1),
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
        // The lock ends with this statement, before the sink is called.
        let events = lock(&self.translator).push(message);
        for event in events {
            emit(&self.sink, event);
        }
    }

    fn note_ignored(&self, kind: &str) {
        if lock(&self.ignored).insert(kind.to_string()) {
            eprintln!("mindrune: ignored a `{kind}` line from claude");
        }
    }

    /// Tells `claude` this adapter does not handle a request, so it stops waiting.
    fn refuse(&self, request_id: &str) {
        let Some(process) = self.process.get().and_then(Weak::upgrade) else {
            return;
        };
        // A failed write means the child is going away, which the exit reports.
        let _ = process.write_line(&unsupported_reply(request_id));
    }
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
    next_request: AtomicU64,
    exited: Arc<AtomicBool>,
    closing: AtomicBool,
}

impl ClaudeSession {
    /// Sends a control request under the next id, `rc_1`, `rc_2`, …. What it asks
    /// for is tracked first, so the reply cannot arrive before it is expected.
    fn request(&self, tracked: Option<Tracked>, body: Value) -> Result<(), AgentError> {
        let id = format!("rc_{}", self.next_request.fetch_add(1, Ordering::SeqCst));
        if let Some(tracked) = tracked {
            lock(&self.translator).track(&id, tracked);
        }
        self.process.write_line(&control_request(&id, body))
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
