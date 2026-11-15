//! What is kept between runs: the groups, where sessions were placed, the folders new sessions
//! started in, the tiles on screen, the sidebar's width and the names given to sessions. It is one value, read once at
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
    /// The focused session, by the agent's id.
    pub shown: Option<SessionId>,
    /// The tiles, left to right. `shown` is the focused one.
    pub tiles: Vec<SavedTile>,
    /// The sidebar's width in points; the window's own default when `None`.
    pub sidebar_width: Option<f32>,
    pub sidebar_collapsed: bool,
    /// The sessions whose program a run of this app ran, by the agent's id, each with when
    /// that run closed, in Unix seconds: a change in the store up to then is this app's own.
    /// Only those recent enough to matter are kept (see `App::saved`).
    pub closed_by_app: Vec<(SessionId, i64)>,
    /// Names the user gave that the program of that session has not confirmed, by the agent's
    /// id. An entry that is not a pair of an id and a name is left out, so the rest is still
    /// read.
    #[serde(deserialize_with = "names_that_read")]
    pub names: Vec<(SessionId, String)>,
}

/// One tile as kept between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedTile {
    pub id: SessionId,
    /// Its part of the row against the others'. One that is missing or not a number reads as
    /// 1, so the rest of the state is still read.
    #[serde(default = "one", deserialize_with = "share_or_one")]
    pub share: f32,
}

fn one() -> f32 {
    1.0
}

/// A number, or 1 for anything else: JSON has no NaN, and serde_json writes one as `null`.
fn share_or_one<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Share {
        Number(f32),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Share::deserialize(deserializer)? {
        Share::Number(share) => share,
        Share::Other(_) => one(),
    })
}

/// The entries that are a pair of an id and a name; none when `names` is not a list.
fn names_that_read<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<(SessionId, String)>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Name {
        Pair(SessionId, String),
        Other(serde::de::IgnoredAny),
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Names {
        List(Vec<Name>),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Names::deserialize(deserializer)? {
        Names::List(names) => names
            .into_iter()
            .filter_map(|name| match name {
                Name::Pair(id, name) => Some((id, name)),
                Name::Other(_) => None,
            })
            .collect(),
        Names::Other(_) => Vec::new(),
    })
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

    /// A saved state with a folder group, a custom group, a placement, two recent folders, two
    /// tiles, the first focused, and a name kept for the second.
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
            tiles: vec![
                SavedTile {
                    id: SessionId("a1".into()),
                    share: 1.5,
                },
                SavedTile {
                    id: SessionId("b1".into()),
                    share: 0.5,
                },
            ],
            sidebar_width: Some(300.0),
            sidebar_collapsed: false,
            closed_by_app: vec![(SessionId("a1".into()), 1_800_000_000)],
            names: vec![(SessionId("b1".into()), "Garden".into())],
        }
    }

    /// How [`sample`] is written, as the window keeps it under its settings key.
    const SAMPLE_JSON: &str = r#"{"version":1,"groups":[{"id":{"Custom":2},"name":"Side projects","collapsed":false,"hidden":false},{"id":{"Folder":"/home/alex/project"},"name":"Parser","collapsed":true,"hidden":false}],"placed":[["a1",{"Custom":2}]],"next_custom":3,"recent_folders":["/home/alex/project","/home/alex/notes"],"shown":"a1","tiles":[{"id":"a1","share":1.5},{"id":"b1","share":0.5}],"sidebar_width":300.0,"sidebar_collapsed":false,"closed_by_app":[["a1",1800000000]],"names":[["b1","Garden"]]}"#;

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
    fn a_bad_share_keeps_the_tile_and_the_rest() {
        let json = r#"{"version":1,"groups":[{"id":{"Custom":2},"name":"Side projects","collapsed":false,"hidden":false}],"next_custom":3,"tiles":[{"id":"a1","share":null},{"id":"a2","share":"x"},{"id":"b1"},{"id":"c1","share":2.5}]}"#;
        let read: Saved = serde_json::from_str(json).unwrap();
        let ids: Vec<&str> = read.tiles.iter().map(|tile| tile.id.0.as_str()).collect();
        assert_eq!(ids, vec!["a1", "a2", "b1", "c1"]);
        let shares: Vec<f32> = read.tiles.iter().map(|tile| tile.share).collect();
        assert_eq!(shares, vec![1.0, 1.0, 1.0, 2.5]);
        assert_eq!(read.groups, sample().groups[..1]);
        assert_eq!(read.next_custom, 3);
    }

    #[test]
    fn a_bad_name_entry_keeps_the_rest() {
        let json = r#"{"version":1,"next_custom":3,"names":[["a1","Garden"],["a2"],["a3","x","y"],["a4",7],null,"a5",["b1","Orchard"]]}"#;
        let read: Saved = serde_json::from_str(json).unwrap();
        assert_eq!(
            read.names,
            vec![
                (SessionId("a1".into()), "Garden".into()),
                (SessionId("b1".into()), "Orchard".into())
            ]
        );
        assert_eq!(read.next_custom, 3);
        for names in ["null", r#""x""#, "{}"] {
            let json = format!(r#"{{"version":1,"next_custom":3,"names":{names}}}"#);
            let read: Saved =
                serde_json::from_str(&json).unwrap_or_else(|error| panic!("{names}: {error}"));
            assert!(read.names.is_empty(), "{names}");
            assert_eq!(read.next_custom, 3, "{names}");
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
