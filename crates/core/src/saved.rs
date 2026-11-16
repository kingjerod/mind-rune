//! What is kept between runs: the groups, where sessions were placed, the folders new sessions
//! started in, the tiles on screen, the sidebar's width, the names given to sessions, whether
//! new sessions start with Remote Control, and the right panel's width and whether it shows. It
//! is one value, read once at start and written whole.

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
    /// The columns of tiles, left to right. `shown` is the focused tile.
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
    /// New sessions start with Remote Control wanted. Anything but `true` reads as false.
    #[serde(deserialize_with = "true_or_false")]
    pub remote_for_new: bool,
    /// The right panel's width in points; the window's own when none or not a number.
    #[serde(deserialize_with = "number_or_none")]
    pub panel_width: Option<f32>,
    /// The right panel shows. Anything that is not `true` reads as false.
    #[serde(deserialize_with = "true_or_false")]
    pub panel_open: bool,
}

/// One column of tiles as kept between runs: its tile, or the upper of its two, and the lower.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedTile {
    pub id: SessionId,
    /// The column's part of the row against the others'. One that is missing or not a number
    /// reads as 1, so the rest of the state is still read.
    #[serde(default = "one", deserialize_with = "share_or_one")]
    pub share: f32,
    /// The lower tile, when the column has two. One that cannot be read reads as none, so the
    /// rest of the state is still read.
    #[serde(default, deserialize_with = "below_that_reads")]
    pub below: Option<SavedBelow>,
}

/// The lower tile of a column as kept between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedBelow {
    pub id: SessionId,
    /// The upper tile's part of the column's height. Missing or not a number reads as 0.5.
    #[serde(default = "half", deserialize_with = "split_or_half")]
    pub split: f32,
}

fn one() -> f32 {
    1.0
}

fn half() -> f32 {
    0.5
}

fn share_or_one<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    number_or(deserializer, one())
}

fn split_or_half<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    number_or(deserializer, half())
}

/// A number, or `or` for anything else.
fn number_or<'de, D: serde::Deserializer<'de>>(deserializer: D, or: f32) -> Result<f32, D::Error> {
    Ok(number_or_none(deserializer)?.unwrap_or(or))
}

/// A number, or none for anything else: JSON has no NaN, and serde_json writes one as `null`.
fn number_or_none<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<f32>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Number {
        Number(f32),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Number::deserialize(deserializer)? {
        Number::Number(number) => Some(number),
        Number::Other(_) => None,
    })
}

/// A lower tile with an id; none for anything else.
fn below_that_reads<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<SavedBelow>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Below {
        Below(SavedBelow),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Below::deserialize(deserializer)? {
        Below::Below(below) => Some(below),
        Below::Other(_) => None,
    })
}

/// True for `true`, false for anything else.
fn true_or_false<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Flag {
        Bool(bool),
        Other(serde::de::IgnoredAny),
    }
    Ok(matches!(Flag::deserialize(deserializer)?, Flag::Bool(true)))
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
    /// columns, the first of two tiles, its upper one focused, a name kept for the second
    /// column's tile, Remote Control wanted for new sessions, and the right panel open at 360
    /// points.
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
                    below: Some(SavedBelow {
                        id: SessionId("a2".into()),
                        split: 0.25,
                    }),
                },
                SavedTile {
                    id: SessionId("b1".into()),
                    share: 0.5,
                    below: None,
                },
            ],
            sidebar_width: Some(300.0),
            sidebar_collapsed: false,
            closed_by_app: vec![(SessionId("a1".into()), 1_800_000_000)],
            names: vec![(SessionId("b1".into()), "Garden".into())],
            remote_for_new: true,
            panel_width: Some(360.0),
            panel_open: true,
        }
    }

    /// How [`sample`] is written, as the window keeps it under its settings key.
    const SAMPLE_JSON: &str = r#"{"version":1,"groups":[{"id":{"Custom":2},"name":"Side projects","collapsed":false,"hidden":false},{"id":{"Folder":"/home/alex/project"},"name":"Parser","collapsed":true,"hidden":false}],"placed":[["a1",{"Custom":2}]],"next_custom":3,"recent_folders":["/home/alex/project","/home/alex/notes"],"shown":"a1","tiles":[{"id":"a1","share":1.5,"below":{"id":"a2","split":0.25}},{"id":"b1","share":0.5,"below":null}],"sidebar_width":300.0,"sidebar_collapsed":false,"closed_by_app":[["a1",1800000000]],"names":[["b1","Garden"]],"remote_for_new":true,"panel_width":360.0,"panel_open":true}"#;

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

    /// The columns of `read` as ids: each upper tile's, and its lower tile's when it has one.
    fn columns(read: &Saved) -> Vec<(&str, Option<&str>)> {
        read.tiles
            .iter()
            .map(|tile| {
                let below = tile.below.as_ref().map(|below| below.id.0.as_str());
                (tile.id.0.as_str(), below)
            })
            .collect()
    }

    #[test]
    fn a_tile_saved_before_stacks_reads_as_a_column_of_one() {
        let json = r#"{"version":1,"shown":"a1","tiles":[{"id":"a1","share":1.5},{"id":"b1","share":0.5}]}"#;
        let read: Saved = serde_json::from_str(json).unwrap();
        assert_eq!(columns(&read), vec![("a1", None), ("b1", None)]);
        let shares: Vec<f32> = read.tiles.iter().map(|tile| tile.share).collect();
        assert_eq!(shares, vec![1.5, 0.5]);
        assert_eq!(read.shown, Some(SessionId("a1".into())));
    }

    #[test]
    fn a_below_that_cannot_be_read_is_left_out() {
        for below in [
            "5",
            r#"{"split":0.3}"#,
            r#"{"id":7,"split":0.3}"#,
            r#""a2""#,
            "null",
        ] {
            let json = format!(
                r#"{{"version":1,"next_custom":3,"tiles":[{{"id":"a1","share":2.0,"below":{below}}},{{"id":"b1","below":{{"id":"b2","split":0.3}}}}]}}"#
            );
            let read: Saved =
                serde_json::from_str(&json).unwrap_or_else(|error| panic!("{below}: {error}"));
            assert_eq!(
                columns(&read),
                vec![("a1", None), ("b1", Some("b2"))],
                "{below}"
            );
            assert_eq!(read.tiles[0].share, 2.0, "{below}");
            assert_eq!(read.next_custom, 3, "{below}");
        }
    }

    #[test]
    fn a_split_that_is_not_a_number_reads_as_a_half() {
        for split in [
            "",
            r#","split":null"#,
            r#","split":"x""#,
            r#","split":[0.3]"#,
        ] {
            let json = format!(
                r#"{{"version":1,"next_custom":3,"tiles":[{{"id":"a1","below":{{"id":"a2"{split}}}}}]}}"#
            );
            let read: Saved =
                serde_json::from_str(&json).unwrap_or_else(|error| panic!("{split}: {error}"));
            assert_eq!(
                read.tiles[0].below,
                Some(SavedBelow {
                    id: SessionId("a2".into()),
                    split: 0.5,
                }),
                "{split}"
            );
            assert_eq!(read.next_custom, 3, "{split}");
        }
    }

    #[test]
    fn a_column_of_two_survives_a_round_trip() {
        let tiles = vec![
            SavedTile {
                id: SessionId("a1".into()),
                share: 0.75,
                below: Some(SavedBelow {
                    id: SessionId("a2".into()),
                    split: 0.3,
                }),
            },
            SavedTile {
                id: SessionId("b1".into()),
                share: 1.25,
                below: None,
            },
        ];
        let saved = Saved {
            version: SAVED_VERSION,
            tiles,
            ..Saved::default()
        };
        let read: Saved = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(read, saved);
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
    fn a_preference_that_cannot_be_read_is_off_and_the_rest_is_read() {
        for value in ["false", "null", "1", r#""true""#, "[true]", "{}"] {
            let json = format!(r#"{{"version":1,"next_custom":3,"remote_for_new":{value}}}"#);
            let read: Saved =
                serde_json::from_str(&json).unwrap_or_else(|error| panic!("{value}: {error}"));
            assert!(!read.remote_for_new, "{value}");
            assert_eq!(read.next_custom, 3, "{value}");
        }
        let read: Saved =
            serde_json::from_str(r#"{"version":1,"next_custom":3,"remote_for_new":true}"#).unwrap();
        assert!(read.remote_for_new);
        assert_eq!(read.next_custom, 3);
    }

    #[test]
    fn a_panel_width_that_is_not_a_number_reads_as_none_and_the_rest_is_read() {
        for value in ["null", r#""x""#, "true", "[300]", "{}"] {
            let json = format!(
                r#"{{"version":1,"next_custom":3,"panel_width":{value},"panel_open":true}}"#
            );
            let read: Saved =
                serde_json::from_str(&json).unwrap_or_else(|error| panic!("{value}: {error}"));
            assert_eq!(read.panel_width, None, "{value}");
            assert!(read.panel_open, "{value}");
            assert_eq!(read.next_custom, 3, "{value}");
        }
        let read: Saved =
            serde_json::from_str(r#"{"version":1,"panel_width":320.5,"panel_open":1}"#).unwrap();
        assert_eq!(read.panel_width, Some(320.5));
        assert!(!read.panel_open, "anything but true is false");
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
