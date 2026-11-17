use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
    Image { media_type: String, data: ImageData },
    File { path: PathBuf },
}

/// The bytes of an image, shared and never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageData(Arc<[u8]>);

impl ImageData {
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Says how many bytes there are, never what they are.
impl fmt::Debug for ImageData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ImageData({} bytes)", self.len())
    }
}

/// Written as one base64 string.
impl Serialize for ImageData {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for ImageData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD
            .decode(text)
            .map(Self::new)
            .map_err(serde::de::Error::custom)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_bytes_are_never_printed() {
        let image = Attachment::Image {
            media_type: "image/png".into(),
            data: ImageData::new(vec![7u8; 1000]),
        };
        let printed = format!("{image:?}");
        assert!(printed.contains("1000 bytes"), "{printed}");
        assert!(printed.len() < 120, "{} characters", printed.len());
    }

    #[test]
    fn image_bytes_are_written_as_base64() {
        let data = ImageData::new(vec![1u8, 2, 3]);
        let json = serde_json::to_string(&data).unwrap();
        assert_eq!(json, "\"AQID\"");
        let back: ImageData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn a_clone_shares_the_bytes() {
        let data = ImageData::new(vec![1u8, 2, 3]);
        let copy = data.clone();
        assert_eq!(data.as_bytes().as_ptr(), copy.as_bytes().as_ptr());
    }
}
