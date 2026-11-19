use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::command::{ImageData, SessionCommand};
use crate::event::{PermissionMode, SessionEvent, ToolDetail, ToolOutcome};
use crate::ids::{SessionId, ToolCallId};

/// Receives one session's events, in the order [`SessionEvent`] describes.
///
/// It is called on a background thread the adapter owns, never on the
/// caller's. The caller makes it cheap and thread-safe, typically by pushing
/// onto a channel. It may call [`SessionHandle::send`] on its own session.
pub type EventSink = Box<dyn Fn(SessionEvent) + Send + 'static>;

/// An agent program the UI can run sessions of.
///
/// `open` and `account` may block for seconds (they start programs and wait
/// for them), and so may reading stored sessions: call them off the UI thread.
pub trait Agent: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    /// Whether and how the user is signed in. May block for seconds.
    fn account(&self) -> Account;
    /// The stored sessions, those at least as new as `modified_since_unix` or all of them.
    /// A store that could not be found or read is an error, never an empty list: the caller
    /// takes a complete listing for everything there is.
    fn list_sessions(
        &self,
        modified_since_unix: Option<i64>,
    ) -> Result<Vec<StoredSession>, AgentError>;
    /// The end of a stored conversation: at most its last `max_items` items,
    /// with the model and mode it last had.
    fn read_session(&self, id: &SessionId, max_items: usize) -> Result<StoredHistory, AgentError>;
    /// When the stored session last changed, in Unix seconds; `None` when nothing is stored
    /// for it. Reads no content.
    fn last_active(&self, id: &SessionId) -> Result<Option<i64>, AgentError>;
    /// One image of one stored message, read again: `index` counts the message's images from 0.
    /// May block while a large file is read.
    fn read_image(
        &self,
        id: &SessionId,
        message: &str,
        index: usize,
    ) -> Result<ImageData, AgentError> {
        let _ = (id, message, index);
        Err(AgentError::Protocol(
            "this agent cannot read an image again".into(),
        ))
    }
    /// Removes a stored session for good, unless it changed after `unless_after_unix`.
    /// `Missing` only when the store was looked through and does not hold it: a store that
    /// could not be found is an error. May block while files are removed.
    fn remove_session(
        &self,
        id: &SessionId,
        unless_after_unix: i64,
    ) -> Result<Removal, AgentError> {
        let _ = (id, unless_after_unix);
        Err(AgentError::Protocol(
            "this agent cannot remove a session".into(),
        ))
    }
    /// Starts a session. May block for seconds. `Started` comes at most once
    /// and `Exited` exactly once, in the order [`SessionEvent`] describes. If
    /// the program exits before it is ready (a session id that cannot be
    /// resumed, a bad flag), `Exited` is the only event, and this may also
    /// return an error. The first events may reach `sink` before this returns.
    fn open(&self, spec: StartSpec, sink: EventSink) -> Result<Box<dyn SessionHandle>, AgentError>;
}

/// The caller's side of one open session.
pub trait SessionHandle: Send {
    /// Hands a command to the session. Results arrive as events, not here: an
    /// `Ok` means the command was passed on, not that it succeeded.
    ///
    /// May block while a large message is written, so call it off the UI
    /// thread. Returns `AgentError::Closed` once the session has ended,
    /// including after `Close`, and `AgentError::Protocol` for an `Answer` to a
    /// request that is not waiting, such as one just withdrawn.
    fn send(&self, command: SessionCommand) -> Result<(), AgentError>;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartSpec {
    pub folder: PathBuf,
    pub resume: Option<SessionId>,
    pub fork: bool,
    pub model: Option<String>,
    pub permission_mode: Option<PermissionMode>,
    /// The effort level to start with; the agent's own default when none.
    pub effort: Option<String>,
}

impl StartSpec {
    pub fn new(folder: impl Into<PathBuf>) -> Self {
        Self {
            folder: folder.into(),
            resume: None,
            fork: false,
            model: None,
            permission_mode: None,
            effort: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub remote_access: bool,
    pub plan_mode: bool,
    pub usage_limits: bool,
    pub images: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Account {
    Missing,
    SignedOut,
    SignedIn {
        name: String,
        plan: Option<String>,
        full_login: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredSession {
    pub id: SessionId,
    pub folder: PathBuf,
    pub title: String,
    pub last_active_unix: i64,
}

/// The end of a stored conversation, as [`Agent::read_session`] gives it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredHistory {
    pub items: Vec<HistoryItem>,
    /// The session holds more than `items`, which are its end.
    pub earlier_left_out: bool,
    /// The full id of the model that last answered, when the history names one.
    pub model: Option<String>,
    /// The mode the session was last in, when the history names one.
    pub permission_mode: Option<PermissionMode>,
    /// The effort level of its last reply, when the store says.
    pub effort: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HistoryItem {
    User {
        text: String,
        image_count: usize,
        /// The files the message was sent with.
        files: Vec<PathBuf>,
        /// The bytes of each image, in the message's order: one entry per image counted,
        /// `None` for one whose bytes the store could not give.
        images: Vec<Option<ImageData>>,
        /// The message's own id in the store, when it has one.
        stored: Option<String>,
    },
    AssistantText(String),
    Thinking(String),
    Tool {
        id: ToolCallId,
        name: String,
        detail: ToolDetail,
        outcome: Option<ToolOutcome>,
    },
    /// The agent summarised everything above this point to save room.
    Compacted,
}

/// What became of a stored session that was to be removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    Removed,
    Missing,
    Changed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AgentError {
    NotInstalled,
    Closed,
    Io(String),
    Protocol(String),
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentError::NotInstalled => write!(f, "the agent is not installed"),
            AgentError::Closed => write!(f, "the session is closed"),
            AgentError::Io(detail) => write!(f, "i/o error: {detail}"),
            AgentError::Protocol(detail) => write!(f, "protocol error: {detail}"),
        }
    }
}

impl std::error::Error for AgentError {}
