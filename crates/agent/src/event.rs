use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{RequestId, SessionId, ToolCallId};

/// What a session tells its caller, through the [`EventSink`](crate::EventSink)
/// given to [`Agent::open`](crate::Agent::open).
///
/// # Order
///
/// - `Started` comes exactly once, before any turn. It is the first event,
///   except that on a resumed session `TitleChanged` can arrive before it. It
///   may arrive before `open` has returned.
/// - Then turns: `TurnStarted`, the turn's text, thinking, tool and request
///   events, and one `TurnFinished` or `TurnFailed`. A turn the user stopped
///   ends in `TurnFinished` with `interrupted: true`, and its open text or
///   thinking block may end without a `BlockFinished`.
/// - `UsageLimits`, `RemoteAccess`, `TitleChanged`, `ModelChanged`,
///   `PermissionModeChanged` and `CommandFailed` can come between turns or
///   inside one.
/// - `Exited` comes last, exactly once, also after a normal `Close`. It ends any
///   open turn and every request still waiting for the user; no separate
///   `TurnFailed` or `RequestWithdrawn` is sent for them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SessionEvent {
    Started(SessionInfo),
    TurnStarted,
    TextDelta(String),
    ThinkingDelta(String),
    /// The current text or thinking block ended.
    BlockFinished,
    ToolStarted {
        id: ToolCallId,
        name: String,
        detail: ToolDetail,
    },
    ToolFinished {
        id: ToolCallId,
        outcome: ToolOutcome,
    },
    NeedsUser(UserRequest),
    /// The agent cancelled a request the user had not answered yet. An
    /// `Answer` to it sent after this returns `AgentError::Protocol`.
    RequestWithdrawn {
        id: RequestId,
    },
    TurnFinished(TurnSummary),
    TurnFailed {
        reason: String,
    },
    UsageLimits(UsageLimits),
    RemoteAccess(RemoteAccess),
    TitleChanged(String),
    ModelChanged(String),
    PermissionModeChanged(PermissionMode),
    /// A SetModel, Rename etc. was refused.
    CommandFailed {
        what: String,
        reason: String,
    },
    /// The session ended. Always the last event; see the order above.
    Exited {
        clean: bool,
        detail: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: SessionId,
    pub folder: PathBuf,
    /// The full id of the model in use.
    pub model: String,
    pub permission_mode: PermissionMode,
    pub agent_version: String,
    /// The models this session can switch to. Empty when the agent did not say.
    pub models: Vec<ModelOption>,
}

/// One model a session can switch to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelOption {
    /// What `SessionCommand::SetModel` takes, such as an alias.
    pub id: String,
    /// The name to show.
    pub name: String,
    /// The full model id `id` stands for now.
    pub resolved: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PermissionMode {
    Ask,
    AcceptEdits,
    Plan,
    Bypass,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToolDetail {
    Command {
        command: String,
    },
    FileEdit {
        path: String,
        old_text: String,
        new_text: String,
    },
    FileWrite {
        path: String,
        content: String,
    },
    FileRead {
        path: String,
    },
    Search {
        query: String,
    },
    Other {
        input_json: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToolOutcome {
    Ok { output: String },
    Failed { output: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserRequest {
    pub id: RequestId,
    pub kind: UserRequestKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum UserRequestKind {
    Permission {
        tool: String,
        detail: ToolDetail,
        always_allow_label: Option<String>,
    },
    Question {
        questions: Vec<Question>,
    },
    PlanApproval {
        plan_markdown: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub prompt: String,
    pub options: Vec<String>,
    pub multi_select: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurnSummary {
    /// The running total for the session's process so far, not the cost of
    /// this turn alone.
    pub cost_usd: f64,
    /// Fraction of the context window in use, when known.
    pub context_used: Option<f32>,
    pub duration_ms: u64,
    /// True when the user stopped the turn.
    pub interrupted: bool,
}

/// A `None` window means "not reported this time", not zero use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageLimits {
    pub session: Option<LimitWindow>,
    pub weekly: Option<LimitWindow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LimitWindow {
    /// Fraction used, 0.0 ..= 1.0.
    pub used: f32,
    pub resets_at_unix: i64,
}

impl LimitWindow {
    /// `used` as a whole percentage, rounded to nearest and clamped to 0..=100.
    pub fn percent(&self) -> u8 {
        (self.used * 100.0).round().clamp(0.0, 100.0) as u8
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RemoteAccess {
    Off,
    Starting,
    On { url: Option<String> },
    Failed { reason: String, needs_login: bool },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_survive_a_json_round_trip() {
        let e = SessionEvent::UsageLimits(UsageLimits {
            session: Some(LimitWindow {
                used: 0.25,
                resets_at_unix: 1794385200,
            }),
            weekly: Some(LimitWindow {
                used: 0.45,
                resets_at_unix: 1794776400,
            }),
        });
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<SessionEvent>(&json).unwrap(), e);
    }

    #[test]
    fn a_limit_window_reports_a_whole_percentage() {
        let pct = |used| {
            LimitWindow {
                used,
                resets_at_unix: 0,
            }
            .percent()
        };
        assert_eq!(pct(0.08), 8);
        assert_eq!(pct(0.456), 46);
        assert_eq!(pct(1.2), 100);
        assert_eq!(pct(-0.1), 0);
    }
}
