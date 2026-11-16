use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::event::PermissionMode;
use crate::ids::RequestId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SessionCommand {
    Send {
        text: String,
        attachments: Vec<Attachment>,
    },
    Answer {
        id: RequestId,
        answer: Answer,
    },
    Stop,
    SetModel(String),
    SetPermissionMode(PermissionMode),
    SetRemoteAccess {
        on: bool,
    },
    /// Names the session. The answer is a `TitleChanged` event with that name; a refusal is
    /// `CommandFailed { what: CommandKind::Rename, .. }`.
    Rename(String),
    /// Asks the agent to name the session from this text. The answer is a `TitleChanged`
    /// event; a refusal is `CommandFailed { what: CommandKind::SuggestTitle, .. }`. An agent
    /// that has no title to offer may answer nothing at all.
    SuggestTitle {
        from: String,
    },
    /// Ask for the usage limits now. The answer is a `UsageLimits` event.
    RefreshUsage,
    /// Sets the effort level of the next messages. An `EffortChanged` event comes when the
    /// level the agent reads afterwards differs from the last one it gave. When the change was
    /// refused or not taken, a `CommandFailed { what: CommandKind::SetEffort, .. }` says so,
    /// with no `EffortChanged` when the level in use did not change.
    SetEffort(String),
    Close,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Attachment {
    Image { media_type: String, data: Vec<u8> },
    File { path: PathBuf },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Answer {
    Allow { always: bool },
    Deny { message: Option<String> },
    Answers(Vec<QuestionAnswer>),
    ApprovePlan,
    KeepPlanning { feedback: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuestionAnswer {
    pub prompt: String,
    pub chosen: Vec<String>,
    pub free_text: Option<String>,
}
