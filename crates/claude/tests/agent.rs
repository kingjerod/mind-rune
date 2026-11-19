use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use mr_agent::{
    Account, Agent, AgentError, Answer, ContextBreakdown, ContextKind, ContextPart, LimitWindow,
    ModelOption, PermissionMode, RemoteAccess, RemoteFailure, RequestId, SessionCommand,
    SessionEvent, SessionHandle, SessionId, SessionInfo, StartSpec, ToolDetail, UsageLimits,
    UserRequestKind,
};
use mr_claude::ClaudeAgent;

const WAIT: Duration = Duration::from_secs(5);

/// Keeps the temporary directories alive for as long as the session handle.
struct WithDirs {
    session: Box<dyn SessionHandle>,
    _dirs: [tempfile::TempDir; 2],
}

impl SessionHandle for WithDirs {
    fn send(&self, command: SessionCommand) -> Result<(), AgentError> {
        self.session.send(command)
    }
}

fn stub_program() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/stub/fake_claude.sh")
}

/// Opens a session of the stub. `edit` adjusts the spec; `extra_env` is added
/// to the parent environment, which otherwise holds only `PATH`.
fn open_with(
    edit: impl FnOnce(&mut StartSpec),
    extra_env: &[(&str, &str)],
) -> (Box<dyn SessionHandle>, Receiver<SessionEvent>) {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let mut parent_env = vec![("PATH".to_string(), std::env::var("PATH").unwrap())];
    parent_env.extend(
        extra_env
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string())),
    );
    let agent = ClaudeAgent::with_paths(stub_program(), root.path().to_path_buf(), parent_env);
    let mut spec = StartSpec::new(folder.path());
    edit(&mut spec);
    let (tx, rx) = channel();
    let handle = agent
        .open(
            spec,
            Box::new(move |event| {
                let _ = tx.send(event);
            }),
        )
        .expect("the stub starts");
    let handle = WithDirs {
        session: handle,
        _dirs: [root, folder],
    };
    (Box::new(handle), rx)
}

/// Opens a stub session and waits for its `Started`, the effort level read after it, which is
/// the stub's `medium`, and the breakdown of its context.
fn open_started_with(
    edit: impl FnOnce(&mut StartSpec),
    extra_env: &[(&str, &str)],
) -> (Box<dyn SessionHandle>, Receiver<SessionEvent>, SessionInfo) {
    let (handle, rx) = open_with(edit, extra_env);
    let info = match next(&rx) {
        SessionEvent::Started(info) => info,
        other => panic!("expected Started first, got {other:?}"),
    };
    assert_eq!(
        next(&rx),
        SessionEvent::EffortChanged(Some("medium".into()))
    );
    assert_eq!(next(&rx), stub_breakdown());
    (handle, rx, info)
}

/// What the stub says its context is made of: its deferred row is left out.
fn stub_breakdown() -> SessionEvent {
    let part = |name: &str, tokens: u64, kind: ContextKind| ContextPart {
        name: name.into(),
        tokens,
        kind,
    };
    SessionEvent::ContextBreakdown(ContextBreakdown {
        parts: vec![
            part("System prompt", 3_000, ContextKind::Used),
            part("Messages", 46_000, ContextKind::Used),
            part("Autocompact buffer", 33_000, ContextKind::Buffer),
            part("Free space", 918_000, ContextKind::Free),
        ],
        window: 1_000_000,
    })
}

fn open_stub() -> (Box<dyn SessionHandle>, Receiver<SessionEvent>) {
    let (handle, rx, _) = open_started_with(|_| {}, &[]);
    (handle, rx)
}

fn next(rx: &Receiver<SessionEvent>) -> SessionEvent {
    rx.recv_timeout(WAIT).expect("an event within 5 seconds")
}

fn send_text(handle: &dyn SessionHandle, text: &str) {
    handle
        .send(SessionCommand::Send {
            text: text.into(),
            attachments: vec![],
        })
        .unwrap();
}

#[test]
fn a_session_starts_before_any_turn_with_the_id_it_was_given() {
    let (_handle, rx, info) = open_started_with(|_| {}, &[]);
    assert!(uuid::Uuid::parse_str(&info.id.0).is_ok(), "{:?}", info.id);
    assert_eq!(info.model, "claude-haiku-5-5");
    assert_eq!(info.permission_mode, PermissionMode::Ask);
    assert_eq!(info.agent_version, "2.1.319");
    assert_eq!(
        info.models[1],
        ModelOption {
            id: "sonnet".into(),
            name: "Sonnet".into(),
            resolved: "claude-sonnet-5-5".into(),
            stands_for: None,
            auto_mode: false,
            effort_levels: Vec::new(),
        }
    );
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn a_plain_resume_keeps_its_id_and_a_fork_gets_a_new_one() {
    let old = SessionId("11111111-1111-4111-8111-111111111111".into());
    let (_resumed, _rx, info) = open_started_with(|spec| spec.resume = Some(old.clone()), &[]);
    assert_eq!(info.id, old);
    let (_forked, _rx, info) = open_started_with(
        |spec| {
            spec.resume = Some(old.clone());
            spec.fork = true;
        },
        &[],
    );
    assert_ne!(info.id, old);
    assert!(uuid::Uuid::parse_str(&info.id.0).is_ok(), "{:?}", info.id);
}

#[test]
fn a_turn_arrives_as_agent_events() {
    let (handle, rx) = open_stub();
    send_text(&*handle, "hi");
    // The turn's init repeats what Started said, so it gives nothing.
    assert_eq!(next(&rx), SessionEvent::TurnStarted);
    assert_eq!(next(&rx), SessionEvent::TextDelta("ok".into()));
    assert_eq!(next(&rx), SessionEvent::BlockFinished);
    assert!(
        matches!(next(&rx), SessionEvent::UsageLimits(u) if u.session.as_ref().unwrap().percent() == 8)
    );
    // 49000 of 1000000: the 1000 output tokens are not context.
    assert!(matches!(next(&rx), SessionEvent::TurnFinished(s) if s.context_used == Some(0.049)));
    // The level read after the turn is unchanged and says nothing; the breakdown is given again.
    assert_eq!(next(&rx), stub_breakdown());
    // The stub holds the turn's result back until the hook request is refused,
    // so TurnFinished above proves the unsupported reply reached it.
}

#[test]
fn a_refused_remote_control_request_comes_back_as_a_failure() {
    let (handle, rx) = open_stub();
    handle
        .send(SessionCommand::SetRemoteAccess { on: true })
        .unwrap();
    assert_eq!(
        next(&rx),
        SessionEvent::RemoteAccess(RemoteAccess::Failed {
            reason: "Claude.ai login was rejected".into(),
            why: RemoteFailure::Login
        })
    );
}

#[test]
fn answering_a_request_that_does_not_exist_is_an_error_and_writes_nothing() {
    let (handle, _rx) = open_stub();
    let bad = SessionCommand::Answer {
        id: RequestId("nope".into()),
        answer: Answer::Allow { always: false },
    };
    assert!(matches!(handle.send(bad), Err(AgentError::Protocol(_))));
}

#[test]
fn closing_ends_the_session_cleanly() {
    let (handle, rx) = open_stub();
    handle.send(SessionCommand::Close).unwrap();
    assert_eq!(
        next(&rx),
        SessionEvent::Exited {
            clean: true,
            detail: String::new()
        }
    );
    assert!(matches!(
        handle.send(SessionCommand::Stop),
        Err(AgentError::Closed)
    ));
}

#[test]
fn a_dying_process_reports_why_and_closes_the_handle() {
    let (handle, rx) = open_stub();
    send_text(&*handle, "please-die");
    assert!(
        matches!(next(&rx), SessionEvent::Exited { clean: false, detail } if detail.contains("stub died"))
    );
    assert!(matches!(
        handle.send(SessionCommand::Stop),
        Err(AgentError::Closed)
    ));
}

#[test]
fn a_missing_claude_cannot_open_a_session() {
    let agent = ClaudeAgent::with_paths("/nonexistent/claude".into(), std::env::temp_dir(), vec![]);
    assert!(matches!(
        agent.open(StartSpec::new(std::env::temp_dir()), Box::new(|_| {})),
        Err(AgentError::NotInstalled)
    ));
    assert_eq!(agent.account(), Account::Missing);
}

#[test]
fn a_session_opened_inside_a_claude_session_does_not_see_its_variables() {
    // The stub exits at once with "leak" if ANTHROPIC_CANARY reaches it.
    let (handle, rx, _info) =
        open_started_with(|_| {}, &[("CLAUDECODE", "1"), ("ANTHROPIC_CANARY", "1")]);
    handle.send(SessionCommand::Close).unwrap();
    assert_eq!(
        next(&rx),
        SessionEvent::Exited {
            clean: true,
            detail: String::new()
        }
    );
}

/// Waits for the first event `pick` accepts, failing on an exit or a timeout.
fn wait_for<T>(rx: &Receiver<SessionEvent>, mut pick: impl FnMut(&SessionEvent) -> Option<T>) -> T {
    loop {
        let event = next(rx);
        if let Some(found) = pick(&event) {
            return found;
        }
        assert!(
            !matches!(event, SessionEvent::Exited { .. }),
            "the stub exited: {event:?}"
        );
    }
}

#[test]
fn a_permission_request_is_answered_and_the_turn_goes_on() {
    let (handle, rx) = open_stub();
    send_text(&*handle, "need-permission");
    let id = wait_for(&rx, |event| match event {
        SessionEvent::NeedsUser(request) => {
            assert_eq!(
                request.kind,
                UserRequestKind::Permission {
                    tool: "Bash".into(),
                    detail: ToolDetail::Command {
                        command: "ls".into()
                    },
                    always_allow_label: None
                }
            );
            Some(request.id.clone())
        }
        _ => None,
    });
    assert_eq!(id, RequestId("req_p".into()));
    handle
        .send(SessionCommand::Answer {
            id,
            answer: Answer::Allow { always: false },
        })
        .unwrap();
    // The stub finishes the turn only after it read the allow reply.
    wait_for(&rx, |event| match event {
        SessionEvent::TurnFinished(summary) => Some(summary.clone()),
        SessionEvent::TurnFailed { reason } => panic!("the turn failed: {reason}"),
        _ => None,
    });
}

#[test]
fn a_model_change_is_sent_and_confirmed() {
    let (handle, rx) = open_stub();
    handle
        .send(SessionCommand::SetModel("sonnet".into()))
        .unwrap();
    assert_eq!(
        next(&rx),
        SessionEvent::ModelChanged("claude-sonnet-5-5".into())
    );
}

#[test]
fn a_title_is_asked_for_and_reported_once() {
    let (handle, rx) = open_stub();
    handle
        .send(SessionCommand::SuggestTitle {
            from: "What is in this folder?".into(),
        })
        .unwrap();
    assert_eq!(
        next(&rx),
        SessionEvent::TitleChanged("Listing the folder".into())
    );
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "the reply repeats the title, which is not reported again"
    );
}

#[test]
fn the_usage_limits_are_asked_for_and_reported() {
    let (handle, rx) = open_stub();
    handle.send(SessionCommand::RefreshUsage).unwrap();
    assert_eq!(
        next(&rx),
        SessionEvent::UsageLimits(UsageLimits {
            session: Some(LimitWindow {
                used: 0.37,
                resets_at_unix: 1794385199
            }),
            weekly: Some(LimitWindow {
                used: 0.10,
                resets_at_unix: 1794776399
            }),
        })
    );
}

#[test]
fn a_change_of_effort_goes_out_and_comes_back() {
    let (handle, rx) = open_stub();
    handle
        .send(SessionCommand::SetEffort("low".into()))
        .unwrap();
    assert_eq!(next(&rx), SessionEvent::EffortChanged(Some("low".into())));
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
}

/// Whether a process with this id still exists.
fn process_exists(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn read_pid(folder: &Path) -> String {
    let file = folder.join("stub.pid");
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(pid) = std::fs::read_to_string(&file)
            && !pid.trim().is_empty()
        {
            return pid.trim().to_string();
        }
        assert!(Instant::now() < deadline, "the stub wrote no pid");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// How long a child that ignores the end of its input may take to be gone, and its exit
/// reported, after `Close`: the adapter kills it after its own 2 s grace, and this leaves a
/// loaded machine room for the kill and the exit to come through.
const KILLED_WITHIN: Duration = Duration::from_secs(5);

#[test]
fn a_child_that_ignores_close_is_killed() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().to_path_buf();
    let (handle, rx, _info) = open_started_with(|spec| spec.folder = path, &[]);
    send_text(&*handle, "ignore-eof");
    let pid = read_pid(folder.path());
    let closed_at = Instant::now();
    handle.send(SessionCommand::Close).unwrap();
    let exit = rx
        .recv_timeout(KILLED_WITHIN)
        .expect("Exited within 5 s of Close");
    assert!(
        matches!(exit, SessionEvent::Exited { clean: false, .. }),
        "{exit:?}"
    );
    assert!(closed_at.elapsed() < KILLED_WITHIN);
    assert!(!process_exists(&pid), "the stub {pid} is still running");
}
