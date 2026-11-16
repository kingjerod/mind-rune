use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{RequestId, SessionId, ToolCallId};

/// What a session tells its caller, through the [`EventSink`](crate::EventSink)
/// given to [`Agent::open`](crate::Agent::open).
///
/// # Order
///
/// - `Started` comes at most once, before any turn. It is the first event,
///   except that on a resumed session `TitleChanged` can arrive before it. It
///   may arrive before `open` has returned, and `EffortChanged` follows it. If
///   the program exits before it is ready (a session id that cannot be
///   resumed, a bad flag), `Exited` is the only event, and `open` may also have
///   returned an error.
/// - Then turns: `TurnStarted`, the turn's text, thinking, tool and request
///   events, and one `TurnFinished` or `TurnFailed`. A turn the user stopped
///   ends in `TurnFinished` with `interrupted: true`, and its open text or
///   thinking block may end without a `BlockFinished`. A thinking phase
///   starts with `ThinkingStarted`, before any `ThinkingDelta`, and
///   `BlockFinished` ends it, whether or not any thinking text came.
/// - `UsageLimits`, `RemoteAccess`, `TitleChanged`, `ModelChanged`,
///   `EffortChanged`, `PermissionModeChanged`, `ContextBreakdown` and
///   `CommandFailed` can come between turns or inside one.
/// - `Exited` comes last, exactly once, also after a normal `Close`. It ends any
///   open turn and every request still waiting for the user; no separate
///   `TurnFailed` or `RequestWithdrawn` is sent for them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SessionEvent {
    Started(SessionInfo),
    TurnStarted,
    TextDelta(String),
    ThinkingDelta(String),
    /// A thinking phase began. `BlockFinished` ends it, also when no thinking
    /// text was sent, except in a turn the user stopped.
    ThinkingStarted,
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
    /// The effort level the next message will use; none when the model takes none.
    /// It follows `Started`, and comes again whenever the level the agent reads differs from
    /// the last one it gave, as after a `SessionCommand::SetEffort` or a change of model.
    /// After a `SetEffort` that was refused or not taken, a
    /// `CommandFailed { what: CommandKind::SetEffort, .. }` says so, with no `EffortChanged`
    /// when the level in use did not change.
    EffortChanged(Option<String>),
    PermissionModeChanged(PermissionMode),
    /// What the context is made of, as the program last counted it. Given whenever the program
    /// answers, after `Started` and after a turn ends; never an error when it does not.
    ContextBreakdown(ContextBreakdown),
    /// A SetModel, Rename etc. was refused.
    CommandFailed {
        what: CommandKind,
        reason: String,
    },
    /// The session ended. Always the last event; see the order above.
    Exited {
        clean: bool,
        detail: String,
    },
}

/// The commands whose refusal is reported by `SessionEvent::CommandFailed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandKind {
    SetModel,
    SetPermissionMode,
    Rename,
    SetRemoteAccess,
    RefreshUsage,
    SuggestTitle,
    SetEffort,
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
    /// The name of the model this choice stands for now, set only when `name` does not
    /// already say it (a name such as "Default (recommended)").
    pub stands_for: Option<String>,
    /// Whether the `Auto` permission mode can be used with this model.
    pub auto_mode: bool,
    /// The effort levels this model takes, lowest first; empty when it takes none.
    pub effort_levels: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PermissionMode {
    Ask,
    AcceptEdits,
    Plan,
    /// The agent decides which actions need the user. Offered only with a model that supports
    /// it (`ModelOption::auto_mode`); the agent may still refuse it.
    Auto,
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
    pub header: Option<String>,
    pub prompt: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
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

/// What the context window holds, part by part, in the program's order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextBreakdown {
    pub parts: Vec<ContextPart>,
    /// The size of the window, in tokens.
    pub window: u64,
}

/// One part of the context, such as the system prompt or the messages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextPart {
    pub name: String,
    pub tokens: u64,
    pub kind: ContextKind,
}

/// What a part of the context is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextKind {
    /// It fills the window.
    Used,
    /// The rest of the window.
    Free,
    /// The room kept for compacting.
    Buffer,
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
        percent(self.used)
    }
}

/// A fraction as a whole percentage, rounded to nearest and clamped to 0..=100.
pub fn percent(fraction: f32) -> u8 {
    (fraction * 100.0).round().clamp(0.0, 100.0) as u8
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RemoteAccess {
    Off,
    Starting,
    On {
        url: Option<String>,
    },
    /// The connection dropped and is being made again.
    Reconnecting,
    Failed {
        reason: String,
        why: RemoteFailure,
    },
}

/// Why Remote Control was refused or ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteFailure {
    /// The sign-in is not a full claude.ai login.
    Login,
    /// The organisation's policy turns it off.
    Policy,
    /// It was ended for this session from another place.
    Elsewhere,
    Other,
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

    #[test]
    fn the_new_words_survive_a_round_trip_through_json() {
        let events = vec![
            SessionEvent::ThinkingStarted,
            SessionEvent::ContextBreakdown(ContextBreakdown {
                parts: vec![ContextPart {
                    name: "Messages".into(),
                    tokens: 8_000,
                    kind: ContextKind::Used,
                }],
                window: 200_000,
            }),
            SessionEvent::CommandFailed {
                what: CommandKind::SetModel,
                reason: "no such model".into(),
            },
            SessionEvent::NeedsUser(UserRequest {
                id: RequestId("r1".into()),
                kind: UserRequestKind::Question {
                    questions: vec![Question {
                        header: Some("Colour".into()),
                        prompt: "Which colour?".into(),
                        options: vec![QuestionOption {
                            label: "Red".into(),
                            description: Some("Warm".into()),
                        }],
                        multi_select: false,
                    }],
                },
            }),
        ];
        let text = serde_json::to_string(&events).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<SessionEvent>>(&text).unwrap(),
            events
        );
        let command = crate::SessionCommand::RefreshUsage;
        let text = serde_json::to_string(&command).unwrap();
        assert_eq!(
            serde_json::from_str::<crate::SessionCommand>(&text).unwrap(),
            command
        );
    }
}
