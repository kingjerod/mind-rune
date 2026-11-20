//! The agent-neutral vocabulary: events an agent emits, commands it accepts,
//! and the traits an adapter implements. Nothing here knows which agent is on
//! the other side or which UI framework is on this one.

mod agent;
mod choices;
mod command;
mod event;
mod fake;
mod ids;
mod tasks;
mod text;

pub use agent::{
    Account, Agent, AgentError, Capabilities, EventSink, HistoryItem, Removal, SessionHandle,
    StartSpec, StoredHistory, StoredSession,
};
pub use choices::{Choice, Choices, MAX_CHOICES, MAX_LABEL, MAX_MESSAGE, MAX_QUESTION};
pub use command::{Answer, Attachment, ImageData, QuestionAnswer, SessionCommand};
pub use event::{
    CommandKind, ContextBreakdown, ContextKind, ContextPart, LimitWindow, ModelOption,
    PermissionMode, Question, QuestionOption, RemoteAccess, RemoteFailure, SessionEvent,
    SessionInfo, ToolDetail, ToolOutcome, TurnSummary, UsageLimits, UserRequest, UserRequestKind,
    percent,
};
pub use fake::{FakeAgent, FakeStep};
pub use ids::{RequestId, SessionId, ToolCallId};
pub use tasks::{MAX_STEP_TEXT, MAX_STEPS, TaskChange, TaskList, TaskStatus, TaskStep, step_text};
