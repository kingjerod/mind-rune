//! What is kept between runs: the groups, where sessions were placed, the folders new sessions
//! started in, the session on screen and the sidebar's width. It is one value, read once at
//! start and written whole.

use std::path::{Path, PathBuf};

use mr_agent::SessionId;
use serde::{Deserialize, Serialize};

use crate::groups::{Group, GroupId};

/// The shape of [`Saved`] this build reads and writes. A saved state of another version is
/// not read.
pub const SAVED_VERSION: u32 = 1;
/// The most folders [`Saved::recent_folders`] holds.
pub const RECENT_FOLDERS: usize = 12;

/// Everything kept between runs. A field that was not written reads as its default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub version: u32,
    /// Every custom group, and each folder group with something to remember.
    pub groups: Vec<Group>,
    /// Where sessions were placed, by the agent's id.
    pub placed: Vec<(SessionId, GroupId)>,
    /// The number the next custom group gets.
    pub next_custom: u64,
    /// The folders new sessions started in, the latest first.
    pub recent_folders: Vec<PathBuf>,
    /// The session on screen, by the agent's id.
    pub shown: Option<SessionId>,
    /// The sidebar's width in points; the window's own default when `None`.
    pub sidebar_width: Option<f32>,
    pub sidebar_collapsed: bool,
    /// The sessions whose program a run of this app ran, by the agent's id, each with when
    /// that run closed, in Unix seconds: a change in the store up to then is this app's own.
    /// Only those recent enough to matter are kept (see `App::saved`).
    pub closed_by_app: Vec<(SessionId, i64)>,
}

impl Saved {
    /// What was read, or the default when it is of another version.
    pub fn checked(self) -> Saved {
        if self.version == SAVED_VERSION {
            self
        } else {
            Saved::default()
        }
    }
}

/// Puts `folder` at the front of `folders`, once, keeping at most [`RECENT_FOLDERS`]. True when
/// that changed `folders`.
pub(crate) fn remember_folder(folders: &mut Vec<PathBuf>, folder: &Path) -> bool {
    if folders.first().is_some_and(|first| first == folder) {
        return false;
    }
    folders.retain(|known| known != folder);
    folders.insert(0, folder.to_path_buf());
    folders.truncate(RECENT_FOLDERS);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A saved state with a folder group, a custom group, a placement, two recent folders and a
    /// shown session.
    fn sample() -> Saved {
        Saved {
            version: SAVED_VERSION,
            groups: vec![
                Group {
                    id: GroupId::Custom(2),
                    name: Some("Side projects".into()),
                    collapsed: false,
                    hidden: false,
                },
                Group {
                    id: GroupId::Folder("/home/alex/project".into()),
                    name: Some("Parser".into()),
                    collapsed: true,
                    hidden: false,
                },
            ],
            placed: vec![(SessionId("a1".into()), GroupId::Custom(2))],
            next_custom: 3,
            recent_folders: vec!["/home/alex/project".into(), "/home/alex/notes".into()],
            shown: Some(SessionId("a1".into())),
            sidebar_width: Some(300.0),
            sidebar_collapsed: false,
            closed_by_app: vec![(SessionId("a1".into()), 1_800_000_000)],
        }
    }

    /// How [`sample`] is written, as the window keeps it under its settings key.
    const SAMPLE_JSON: &str = r#"{"version":1,"groups":[{"id":{"Custom":2},"name":"Side projects","collapsed":false,"hidden":false},{"id":{"Folder":"/home/alex/project"},"name":"Parser","collapsed":true,"hidden":false}],"placed":[["a1",{"Custom":2}]],"next_custom":3,"recent_folders":["/home/alex/project","/home/alex/notes"],"shown":"a1","sidebar_width":300.0,"sidebar_collapsed":false,"closed_by_app":[["a1",1800000000]]}"#;

    #[test]
    fn a_saved_state_survives_a_round_trip() {
        let saved = sample();
        let json = serde_json::to_string(&saved).unwrap();
        assert_eq!(json, SAMPLE_JSON);
        let read: Saved = serde_json::from_str(&json).unwrap();
        assert_eq!(read, saved);
        assert_eq!(read.checked(), saved);
    }

    #[test]
    fn an_older_or_newer_version_counts_as_none() {
        for version in [0, SAVED_VERSION + 1] {
            let saved = Saved {
                version,
                ..sample()
            };
            let read: Saved = serde_json::from_str(&serde_json::to_string(&saved).unwrap())
                .unwrap_or_else(|error| panic!("{version}: {error}"));
            assert_eq!(read.checked(), Saved::default(), "version {version}");
        }
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let read: Saved = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert_eq!(
            read,
            Saved {
                version: 1,
                ..Saved::default()
            }
        );
        assert_eq!(read.clone().checked(), read);
        let empty: Saved = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.checked(), Saved::default(), "no version is none");
    }
}
