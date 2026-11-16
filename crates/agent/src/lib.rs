//! The agent-neutral vocabulary: events an agent emits, commands it accepts,
//! and the traits an adapter implements. Nothing here knows which agent is on
//! the other side or which UI framework is on this one.

mod agent;
mod command;
mod event;
mod fake;
mod ids;

pub use agent::{
    Account, Agent, AgentError, Capabilities, EventSink, HistoryItem, SessionHandle, StartSpec,
    StoredHistory, StoredSession,
};
pub use command::{Answer, Attachment, QuestionAnswer, SessionCommand};
pub use event::{
    CommandKind, ContextBreakdown, ContextKind, ContextPart, LimitWindow, ModelOption,
    PermissionMode, Question, QuestionOption, RemoteAccess, RemoteFailure, SessionEvent,
    SessionInfo, ToolDetail, ToolOutcome, TurnSummary, UsageLimits, UserRequest, UserRequestKind,
    percent,
};
pub use fake::{FakeAgent, FakeStep};
pub use ids::{RequestId, SessionId, ToolCallId};
