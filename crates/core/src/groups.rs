//! The groups sessions are listed under, and what the sidebar shows: its sections, their rows
//! and marks, and the flat list of lines it is drawn from.
//!
//! Every session is in its folder's group until the user places it in another. A folder's group
//! needs no record of its own; one is kept only while it has something to remember (a name, or
//! being collapsed or hidden). A custom group is the user's own and always has a record.

use std::cmp::{Ordering, Reverse};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use mr_agent::SessionId;
use serde::{Deserialize, Serialize};

use crate::app::RECENT_DAYS;
use crate::saved::Saved;
use crate::sessions::{Entry, Mark, SessionKey, Sessions};
use crate::tiles::Tiles;

/// A group: a folder's own, or one the user created.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GroupId {
    Folder(PathBuf),
    Custom(u64),
}

impl GroupId {
    /// For element keys: "c3" for a custom group, "f" and 16 hex digits for a folder.
    ///
    /// A folder's digits are the FNV-1a 64 hash of its path's components, each followed by a
    /// zero byte: the same for paths that compare equal, and the same in every run.
    pub fn key_text(&self) -> String {
        match self {
            GroupId::Custom(number) => format!("c{number}"),
            GroupId::Folder(path) => {
                let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
                let bytes = path.components().flat_map(|component| {
                    let bytes = component.as_os_str().as_encoded_bytes();
                    bytes.iter().copied().chain([0])
                });
                for byte in bytes {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
                format!("f{hash:016x}")
            }
        }
    }
}

/// What is remembered about a group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    /// The user's name for it; `None` for a folder group named after its folder.
    pub name: Option<String>,
    pub collapsed: bool,
    pub hidden: bool,
}

impl Group {
    /// A folder group with nothing of its own to remember needs no record.
    fn worth_keeping(&self) -> bool {
        matches!(self.id, GroupId::Custom(_))
            || self.name.is_some()
            || self.collapsed
            || self.hidden
    }
}

/// The groups, and where the user placed sessions: by the agent's id, or by key while a session
/// has none yet (see [`Groups::adopt`]).
#[derive(Clone, Debug, Default)]
pub struct Groups {
    /// Every custom group, and each folder group with something to remember.
    records: HashMap<GroupId, Group>,
    /// Where sessions were placed, by the agent's id.
    placed: HashMap<SessionId, GroupId>,
    /// Where sessions with no id yet were placed, by key.
    placed_by_key: HashMap<SessionKey, GroupId>,
    /// The groups asked to list their old sessions too, in this run.
    showing_older: HashSet<GroupId>,
    older_groups_open: bool,
    /// The number of the latest custom group: each new one gets the next.
    last_custom: u64,
    /// Goes up with every change: see [`Groups::version`].
    version: u64,
}

impl Groups {
    /// The groups `saved` kept, and the placements it kept into groups that exist: a custom
    /// group it does not hold is gone. A new custom group gets a number none had before.
    pub fn restored(saved: &Saved) -> Groups {
        let mut groups = Groups::default();
        for group in saved.groups.iter().filter(|group| group.worth_keeping()) {
            groups.records.insert(group.id.clone(), group.clone());
        }
        groups.placed = saved
            .placed
            .iter()
            .filter(|(_, group)| groups.exists(group))
            .cloned()
            .collect();
        let highest = groups
            .records
            .keys()
            .filter_map(|id| match id {
                GroupId::Custom(number) => Some(*number),
                GroupId::Folder(_) => None,
            })
            .max()
            .unwrap_or(0);
        groups.last_custom = saved.next_custom.saturating_sub(1).max(highest);
        groups
    }

    /// Writes what is kept of the groups between runs into `saved`: the records, custom groups
    /// first by number and then folder groups by path; the placements by id, ordered by id; and
    /// the number the next custom group gets. Placements by key, and what is asked for only in
    /// this run (old sessions listed, older groups open), are not kept.
    pub fn save_into(&self, saved: &mut Saved) {
        let mut records: Vec<Group> = self.records.values().cloned().collect();
        records.sort_by(|a, b| by_id(&a.id, &b.id));
        let mut placed: Vec<(SessionId, GroupId)> = self
            .placed
            .iter()
            .map(|(id, group)| (id.clone(), group.clone()))
            .collect();
        placed.sort_by(|(a, _), (b, _)| a.0.cmp(&b.0));
        saved.groups = records;
        saved.placed = placed;
        saved.next_custom = self.last_custom + 1;
    }

    /// Where `entry` is: the group it was placed in while that group exists, else its
    /// folder's. Once a session has an id it is found by its id alone.
    pub fn group_of(&self, entry: &Entry) -> GroupId {
        let placed = match &entry.session.id {
            Some(id) => self.placed.get(id),
            None => self.placed_by_key.get(&entry.key),
        };
        match placed {
            Some(group) if self.exists(group) => group.clone(),
            _ => GroupId::Folder(entry.session.folder.clone()),
        }
    }

    /// Its own name, or for a folder group its folder's.
    pub fn name(&self, id: &GroupId) -> String {
        if let Some(name) = self.records.get(id).and_then(|group| group.name.clone()) {
            return name;
        }
        match id {
            GroupId::Folder(path) => folder_name(path),
            GroupId::Custom(_) => NEW_GROUP.to_string(),
        }
    }

    /// Names `id` `name`, trimmed. An empty name gives a folder group its folder's name back
    /// and leaves a custom group's as it was. No folder on disk is touched.
    pub fn rename(&mut self, id: &GroupId, name: &str) {
        self.version += 1;
        let name = name.trim();
        match id {
            GroupId::Custom(_) if name.is_empty() => {}
            GroupId::Custom(_) => {
                if let Some(group) = self.records.get_mut(id) {
                    group.name = Some(name.to_string());
                }
            }
            GroupId::Folder(_) => {
                let name = Some(name.to_string()).filter(|name| !name.is_empty());
                self.change(id, |group| group.name = name);
            }
        }
    }

    /// A new custom group named `name`, trimmed, or "New group" when that is empty.
    pub fn create(&mut self, name: &str) -> GroupId {
        self.version += 1;
        self.last_custom += 1;
        let id = GroupId::Custom(self.last_custom);
        let name = match name.trim() {
            "" => NEW_GROUP,
            name => name,
        };
        let group = Group {
            id: id.clone(),
            name: Some(name.to_string()),
            collapsed: false,
            hidden: false,
        };
        self.records.insert(id.clone(), group);
        id
    }

    /// A custom group goes, and its sessions return to their folders' groups. A folder group
    /// loses its name, its collapsed and hidden state and the sessions placed in it; its own
    /// sessions stay in it.
    pub fn delete(&mut self, id: &GroupId) {
        self.version += 1;
        self.records.remove(id);
        self.placed.retain(|_, group| group != id);
        self.placed_by_key.retain(|_, group| group != id);
        if matches!(id, GroupId::Custom(_)) {
            self.showing_older.remove(id);
        }
    }

    /// Puts `entry`'s session in `group`, when that group exists. In its own folder's group it
    /// needs no placement.
    pub fn place(&mut self, entry: &Entry, group: &GroupId) {
        self.version += 1;
        if !self.exists(group) {
            return;
        }
        self.placed_by_key.remove(&entry.key);
        let home = matches!(group, GroupId::Folder(path) if *path == entry.session.folder);
        match (&entry.session.id, home) {
            (Some(id), true) => {
                self.placed.remove(id);
            }
            (Some(id), false) => {
                self.placed.insert(id.clone(), group.clone());
            }
            (None, true) => {}
            (None, false) => {
                self.placed_by_key.insert(entry.key, group.clone());
            }
        }
    }

    /// A session that had no id when it was placed has one now.
    pub fn adopt(&mut self, key: SessionKey, id: &SessionId) {
        self.version += 1;
        if let Some(group) = self.placed_by_key.remove(&key) {
            self.placed.insert(id.clone(), group);
        }
    }

    /// The session known by `old` is known by `new` now: its placement follows it.
    pub fn follow(&mut self, old: &SessionId, new: &SessionId) {
        self.version += 1;
        if let Some(group) = self.placed.remove(old) {
            self.placed.insert(new.clone(), group);
        }
    }

    pub fn set_collapsed(&mut self, id: &GroupId, collapsed: bool) {
        self.version += 1;
        self.change(id, |group| group.collapsed = collapsed);
    }

    pub fn set_hidden(&mut self, id: &GroupId, hidden: bool) {
        self.version += 1;
        self.change(id, |group| group.hidden = hidden);
    }

    /// Lists `id`'s old sessions too, for the rest of this run.
    pub fn show_older(&mut self, id: &GroupId) {
        self.version += 1;
        self.showing_older.insert(id.clone());
    }

    /// Lists the older groups, or stops listing them.
    pub fn toggle_older_groups(&mut self) {
        self.version += 1;
        self.older_groups_open = !self.older_groups_open;
    }

    /// Goes up with every call that may change something, so a caller can tell that nothing
    /// changed without comparing.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// A custom group exists while it has a record. A folder group always can: it holds at least
    /// the sessions of its folder, and those placed in it.
    fn exists(&self, id: &GroupId) -> bool {
        match id {
            GroupId::Folder(_) => true,
            GroupId::Custom(_) => self.records.contains_key(id),
        }
    }

    /// Changes `id`'s record, making one for a folder group that has none, and drops a folder
    /// group's record once nothing is left in it to remember. A custom group that does not
    /// exist is left alone.
    fn change(&mut self, id: &GroupId, change: impl FnOnce(&mut Group)) {
        let group = match id {
            GroupId::Custom(_) => match self.records.get_mut(id) {
                Some(group) => group,
                None => return,
            },
            GroupId::Folder(_) => self.records.entry(id.clone()).or_insert_with(|| Group {
                id: id.clone(),
                name: None,
                collapsed: false,
                hidden: false,
            }),
        };
        change(group);
        if !group.worth_keeping() {
            self.records.remove(id);
        }
    }
}

/// Custom groups first, by number, then folder groups, by path.
fn by_id(a: &GroupId, b: &GroupId) -> Ordering {
    match (a, b) {
        (GroupId::Custom(a), GroupId::Custom(b)) => a.cmp(b),
        (GroupId::Custom(_), GroupId::Folder(_)) => Ordering::Less,
        (GroupId::Folder(_), GroupId::Custom(_)) => Ordering::Greater,
        (GroupId::Folder(a), GroupId::Folder(b)) => a.cmp(b),
    }
}

/// The name of a custom group created without one.
const NEW_GROUP: &str = "New group";
/// The title of a row whose session has none yet.
const NEW_SESSION: &str = "New session";

/// A folder's last component, or the whole path when it has none.
fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// One session's row in the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: SessionKey,
    pub title: String,
    /// The session's folder, when the row is not in that folder's group.
    pub folder_label: Option<String>,
    pub mark: Mark,
    /// The session is in a tile.
    pub shown: bool,
    /// The session is in the focused tile.
    pub focused: bool,
}

/// One group as the sidebar lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub id: GroupId,
    pub name: String,
    pub custom: bool,
    pub collapsed: bool,
    pub rows: Vec<Row>,
    /// Rows left out for their age.
    pub older: usize,
    /// The strongest mark among its rows.
    pub mark: Mark,
    /// A custom group, or a folder group with a name of its own or a session from another
    /// folder.
    pub can_delete: bool,
}

/// One line of the sidebar's list, top to bottom. The numbers index `sections` and its rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Header(usize),
    Row(usize, usize),
    ShowOlder(usize),
    Loading,
    NewGroup,
    HiddenGroups,
    OlderGroups,
}

/// What the sidebar shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SidebarModel {
    pub sections: Vec<Section>,
    pub lines: Vec<Line>,
    /// The hidden groups, with their names, in the order they would be listed.
    pub hidden: Vec<(GroupId, String)>,
    /// Folder groups listed only on request: nothing of theirs is recent and they have no
    /// record of their own.
    pub older_groups: usize,
    pub older_groups_open: bool,
    /// The first listing of stored sessions has not arrived.
    pub loading: bool,
}

/// What the sidebar shows of `sessions` in `groups`, with `tiles` the tiles the window shows
/// sessions in, at `now_unix`, `loading` while the first listing has not arrived.
pub fn sidebar_model(
    sessions: &Sessions,
    groups: &Groups,
    tiles: &Tiles,
    now_unix: i64,
    loading: bool,
) -> SidebarModel {
    let mut building: HashMap<GroupId, Building> = HashMap::new();
    for entry in sessions.iter() {
        let id = groups.group_of(entry);
        let section = building
            .entry(id)
            .or_insert_with_key(|id| Building::new(id, groups));
        section.add(entry, tiles, now_unix);
    }
    for id in groups.records.keys() {
        if matches!(id, GroupId::Custom(_)) && !building.contains_key(id) {
            building.insert(id.clone(), Building::new(id, groups));
        }
    }
    let mut building: Vec<Building> = building.into_values().collect();
    building.sort_by(Building::order);

    let mut model = SidebarModel {
        older_groups_open: groups.older_groups_open,
        loading,
        ..SidebarModel::default()
    };
    let mut older_groups = Vec::new();
    for section in building {
        let section = section.finish(groups);
        let record = groups.records.get(&section.id);
        let present = section
            .rows
            .iter()
            .any(|row| row.shown || row.mark != Mark::None);
        if record.is_some_and(|group| group.hidden) && !present {
            model.hidden.push((section.id, section.name));
        } else if section.rows.is_empty()
            && section.older > 0
            && record.is_none()
            && matches!(section.id, GroupId::Folder(_))
        {
            model.older_groups += 1;
            if groups.older_groups_open {
                older_groups.push(section);
            }
        } else {
            model.sections.push(section);
        }
    }
    model.sections.extend(older_groups);
    model.lines = lines(&model);
    model
}

/// The list as it is drawn, top to bottom.
fn lines(model: &SidebarModel) -> Vec<Line> {
    let mut lines = Vec::new();
    for (s, section) in model.sections.iter().enumerate() {
        lines.push(Line::Header(s));
        if section.collapsed {
            continue;
        }
        lines.extend((0..section.rows.len()).map(|r| Line::Row(s, r)));
        if section.older > 0 {
            lines.push(Line::ShowOlder(s));
        }
    }
    if model.loading {
        lines.push(Line::Loading);
    }
    lines.push(Line::NewGroup);
    if !model.hidden.is_empty() {
        lines.push(Line::HiddenGroups);
    }
    if model.older_groups > 0 {
        lines.push(Line::OlderGroups);
    }
    lines
}

/// A section while the entries are walked.
struct Building {
    id: GroupId,
    /// Its rows, each with its session's `last_active_unix`.
    rows: Vec<(i64, Row)>,
    older: usize,
    /// The newest `last_active_unix` among its entries.
    newest: Option<i64>,
    /// It holds a session from another folder, as a folder group.
    guests: bool,
    showing_older: bool,
}

impl Building {
    fn new(id: &GroupId, groups: &Groups) -> Building {
        Building {
            id: id.clone(),
            rows: Vec::new(),
            older: 0,
            newest: None,
            guests: false,
            showing_older: groups.showing_older.contains(id),
        }
    }

    /// Counts `entry` in, with a row when it is live, in a tile, touched in this run, recent,
    /// or its group lists old sessions too.
    fn add(&mut self, entry: &Entry, tiles: &Tiles, now_unix: i64) {
        self.newest = self.newest.max(Some(entry.last_active_unix));
        let home = match &self.id {
            GroupId::Folder(path) => *path == entry.session.folder,
            GroupId::Custom(_) => false,
        };
        self.guests |= !home && matches!(self.id, GroupId::Folder(_));
        let shown = tiles.contains(entry.key);
        let listed = entry.live()
            || shown
            || entry.touched
            || recent(entry.last_active_unix, now_unix)
            || self.showing_older;
        if !listed {
            self.older += 1;
            return;
        }
        let row = Row {
            key: entry.key,
            title: entry
                .session
                .title
                .clone()
                .unwrap_or_else(|| NEW_SESSION.to_string()),
            folder_label: (!home).then(|| folder_name(&entry.session.folder)),
            mark: entry.mark(),
            shown,
            focused: tiles.focused() == Some(entry.key),
        };
        self.rows.push((entry.last_active_unix, row));
    }

    /// Newest first; a section with no entry before all others, the newest created first.
    fn order(a: &Building, b: &Building) -> Ordering {
        let by_time = match (a.newest, b.newest) {
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (a, b) => b.cmp(&a),
        };
        by_time.then_with(|| match (&a.id, &b.id) {
            (GroupId::Custom(a), GroupId::Custom(b)) => b.cmp(a),
            (GroupId::Custom(_), GroupId::Folder(_)) => Ordering::Less,
            (GroupId::Folder(_), GroupId::Custom(_)) => Ordering::Greater,
            (GroupId::Folder(a), GroupId::Folder(b)) => a.cmp(b),
        })
    }

    fn finish(mut self, groups: &Groups) -> Section {
        self.rows
            .sort_by_key(|(last_active_unix, row)| (Reverse(*last_active_unix), row.key));
        let rows: Vec<Row> = self.rows.into_iter().map(|(_, row)| row).collect();
        let mark = rows
            .iter()
            .map(|row| row.mark)
            .max_by_key(|mark| strength(*mark))
            .unwrap_or(Mark::None);
        let record = groups.records.get(&self.id);
        let custom = matches!(self.id, GroupId::Custom(_));
        let named = record.is_some_and(|group| group.name.is_some());
        Section {
            name: groups.name(&self.id),
            custom,
            collapsed: record.is_some_and(|group| group.collapsed),
            rows,
            older: self.older,
            mark,
            can_delete: custom || named || self.guests,
            id: self.id,
        }
    }
}

/// How strongly a mark asks for the user.
fn strength(mark: Mark) -> u8 {
    match mark {
        Mark::None => 0,
        Mark::Finished => 1,
        Mark::Working => 2,
        Mark::NeedsYou => 3,
    }
}

/// Active within [`RECENT_DAYS`] of `now_unix`.
fn recent(last_active_unix: i64, now_unix: i64) -> bool {
    now_unix <= last_active_unix + RECENT_DAYS * 86_400
}

/// The clock times over which no entry turns from recent to old or back: every `now_unix` with
/// `after < now_unix <= until`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Recency {
    after: i64,
    until: i64,
}

impl Recency {
    pub(crate) fn of(sessions: &Sessions, now_unix: i64) -> Recency {
        let mut recency = Recency {
            after: i64::MIN,
            until: i64::MAX,
        };
        for entry in sessions.iter() {
            let turns = entry.last_active_unix + RECENT_DAYS * 86_400;
            if recent(entry.last_active_unix, now_unix) {
                recency.until = recency.until.min(turns);
            } else {
                recency.after = recency.after.max(turns);
            }
        }
        recency
    }

    pub(crate) fn holds_at(&self, now_unix: i64) -> bool {
        self.after < now_unix && now_unix <= self.until
    }
}

#[cfg(test)]
mod tests {
    use mr_agent::StoredSession;

    use super::*;
    use crate::session::{Phase, Session};
    use crate::tiles::{Place, RowSize};

    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;
    const MINDRUNE: &str = "/home/alex/rune_claude";
    const NOTES: &str = "/home/alex/notes";

    fn folder(path: &str) -> GroupId {
        GroupId::Folder(path.into())
    }

    /// Adds a stored session `id`, titled "About {id}", in `folder`, last active `ago` seconds
    /// before `NOW`: resting and not touched.
    fn stored(sessions: &mut Sessions, id: &str, folder: &str, ago: i64) -> SessionKey {
        let stored = StoredSession {
            id: SessionId(id.into()),
            folder: folder.into(),
            title: format!("About {id}"),
            last_active_unix: NOW - ago,
        };
        sessions.add(Session::resting(&stored), NOW - ago)
    }

    fn entry(sessions: &Sessions, key: SessionKey) -> &Entry {
        sessions.get(key).unwrap()
    }

    fn model(sessions: &Sessions, groups: &Groups) -> SidebarModel {
        sidebar_model(sessions, groups, &Tiles::default(), NOW, false)
    }

    /// A row of tiles holding `keys`, left to right, the last one focused.
    fn tiles(keys: &[SessionKey]) -> Tiles {
        let mut tiles = Tiles::default();
        for &key in keys {
            tiles.put(key, Place::Side, RowSize::default());
        }
        tiles
    }

    fn names(model: &SidebarModel) -> Vec<&str> {
        model.sections.iter().map(|s| s.name.as_str()).collect()
    }

    fn section<'a>(model: &'a SidebarModel, id: &GroupId) -> &'a Section {
        model
            .sections
            .iter()
            .find(|section| &section.id == id)
            .unwrap_or_else(|| panic!("no section {id:?} in {:?}", names(model)))
    }

    fn keys(section: &Section) -> Vec<SessionKey> {
        section.rows.iter().map(|row| row.key).collect()
    }

    fn row(section: &Section, key: SessionKey) -> &Row {
        section.rows.iter().find(|row| row.key == key).unwrap()
    }

    #[test]
    fn a_session_is_in_its_folders_group_by_default() {
        let mut sessions = Sessions::default();
        let a = stored(&mut sessions, "a1", MINDRUNE, 60);
        let b = stored(&mut sessions, "b1", NOTES, 120);
        let groups = Groups::default();
        assert_eq!(groups.group_of(entry(&sessions, a)), folder(MINDRUNE));
        assert_eq!(groups.group_of(entry(&sessions, b)), folder(NOTES));
        let model = model(&sessions, &groups);
        assert_eq!(names(&model), vec!["rune_claude", "notes"]);
        let first = &model.sections[0];
        assert_eq!(
            (first.id.clone(), first.custom, first.collapsed, keys(first)),
            (folder(MINDRUNE), false, false, vec![a])
        );
        assert_eq!(
            first.rows[0],
            Row {
                key: a,
                title: "About a1".into(),
                folder_label: None,
                mark: Mark::None,
                shown: false,
                focused: false,
            }
        );
        assert_eq!(keys(&model.sections[1]), vec![b]);
        assert!(!first.can_delete, "nothing to reset");
    }

    #[test]
    fn a_row_without_a_title_is_a_new_session() {
        let mut sessions = Sessions::default();
        let key = sessions.add(Session::opening(MINDRUNE), NOW);
        let model = model(&sessions, &Groups::default());
        assert_eq!(model.sections[0].rows[0].title, "New session");
        assert_eq!(model.sections[0].rows[0].key, key);
    }

    #[test]
    fn a_renamed_group_keeps_its_sessions() {
        let mut sessions = Sessions::default();
        stored(&mut sessions, "a1", MINDRUNE, 60);
        stored(&mut sessions, "a2", MINDRUNE, 70);
        stored(&mut sessions, "b1", NOTES, 120);
        let mut groups = Groups::default();
        let before = keys(section(&model(&sessions, &groups), &folder(MINDRUNE)));
        groups.rename(&folder(MINDRUNE), "MindRune");
        assert_eq!(groups.name(&folder(MINDRUNE)), "MindRune");
        let model_now = model(&sessions, &groups);
        let renamed = section(&model_now, &folder(MINDRUNE));
        assert_eq!(renamed.name, "MindRune");
        assert_eq!(keys(renamed), before);
        assert!(renamed.can_delete, "a name of its own");
        assert_eq!(names(&model_now), vec!["MindRune", "notes"]);

        groups.rename(&folder(MINDRUNE), "  ");
        assert_eq!(groups.name(&folder(MINDRUNE)), "rune_claude");
        let model_now = model(&sessions, &groups);
        assert_eq!(section(&model_now, &folder(MINDRUNE)).name, "rune_claude");
        assert!(!section(&model_now, &folder(MINDRUNE)).can_delete);

        groups.rename(&folder(MINDRUNE), "  Mind Rune \t");
        assert_eq!(groups.name(&folder(MINDRUNE)), "Mind Rune", "trimmed");
        assert_eq!(
            groups.name(&folder("/")),
            "/",
            "a path with no last component"
        );
    }

    #[test]
    fn a_session_moves_to_another_group() {
        let mut sessions = Sessions::default();
        let a = stored(&mut sessions, "a1", MINDRUNE, 60);
        let b = stored(&mut sessions, "b1", NOTES, 120);
        let mut groups = Groups::default();
        let side = groups.create("Side projects");
        groups.place(entry(&sessions, a), &side);
        assert_eq!(groups.group_of(entry(&sessions, a)), side);
        let model_now = model(&sessions, &groups);
        assert_eq!(names(&model_now), vec!["Side projects", "notes"]);
        let custom = section(&model_now, &side);
        assert!(custom.custom && custom.can_delete);
        assert_eq!(keys(custom), vec![a]);
        assert_eq!(custom.rows[0].folder_label.as_deref(), Some("rune_claude"));

        groups.place(entry(&sessions, a), &folder(MINDRUNE));
        assert_eq!(groups.group_of(entry(&sessions, a)), folder(MINDRUNE));
        let model_now = model(&sessions, &groups);
        let home = section(&model_now, &folder(MINDRUNE));
        assert_eq!(home.rows[0].folder_label, None);
        assert!(section(&model_now, &side).rows.is_empty());

        groups.place(entry(&sessions, a), &folder(NOTES));
        let model_now = model(&sessions, &groups);
        let notes = section(&model_now, &folder(NOTES));
        assert_eq!(keys(notes), vec![a, b]);
        assert_eq!(row(notes, a).folder_label.as_deref(), Some("rune_claude"));
        assert_eq!(row(notes, b).folder_label, None);
        assert!(notes.can_delete, "a session from another folder");
        assert_eq!(names(&model_now), vec!["Side projects", "notes"]);
    }

    #[test]
    fn deleting_a_custom_group_sends_its_sessions_home() {
        let mut sessions = Sessions::default();
        let a = stored(&mut sessions, "a1", MINDRUNE, 60);
        let b = stored(&mut sessions, "b1", NOTES, 120);
        let mut groups = Groups::default();
        let side = groups.create("Side projects");
        groups.place(entry(&sessions, a), &side);
        groups.place(entry(&sessions, b), &side);
        assert_eq!(names(&model(&sessions, &groups)), vec!["Side projects"]);
        groups.delete(&side);
        assert_eq!(groups.group_of(entry(&sessions, a)), folder(MINDRUNE));
        assert_eq!(groups.group_of(entry(&sessions, b)), folder(NOTES));
        assert_eq!(
            names(&model(&sessions, &groups)),
            vec!["rune_claude", "notes"]
        );
        groups.place(entry(&sessions, a), &side);
        assert_eq!(
            groups.group_of(entry(&sessions, a)),
            folder(MINDRUNE),
            "a group that is gone takes nothing"
        );
    }

    #[test]
    fn deleting_a_folder_group_resets_it() {
        let mut sessions = Sessions::default();
        let a = stored(&mut sessions, "a1", MINDRUNE, 60);
        let b = stored(&mut sessions, "b1", NOTES, 120);
        let c = stored(&mut sessions, "b2", NOTES, 130);
        let mut groups = Groups::default();
        let home = folder(MINDRUNE);
        groups.rename(&home, "MindRune");
        groups.set_collapsed(&home, true);
        groups.place(entry(&sessions, b), &home);
        let model_now = model(&sessions, &groups);
        let before = section(&model_now, &home);
        assert_eq!(
            (before.name.as_str(), before.collapsed, keys(before)),
            ("MindRune", true, vec![a, b])
        );
        groups.set_hidden(&home, true);
        assert_eq!(
            model(&sessions, &groups).hidden,
            vec![(home.clone(), "MindRune".to_string())]
        );

        groups.delete(&home);
        let model_now = model(&sessions, &groups);
        assert!(model_now.hidden.is_empty());
        let after = section(&model_now, &home);
        assert_eq!(
            (after.name.as_str(), after.collapsed, keys(after)),
            ("rune_claude", false, vec![a])
        );
        assert!(!after.can_delete, "nothing left to reset");
        assert_eq!(groups.group_of(entry(&sessions, b)), folder(NOTES));
        assert_eq!(keys(section(&model_now, &folder(NOTES))), vec![b, c]);
    }

    #[test]
    fn an_empty_custom_group_comes_first() {
        let mut sessions = Sessions::default();
        let a = stored(&mut sessions, "a1", MINDRUNE, 60);
        let mut groups = Groups::default();
        let side = groups.create("Side projects");
        let unnamed = groups.create("  ");
        assert_ne!(side, unnamed);
        let model_now = model(&sessions, &groups);
        assert_eq!(
            names(&model_now),
            vec!["New group", "Side projects", "rune_claude"]
        );
        assert!(model_now.sections[0].rows.is_empty());
        assert_eq!(model_now.sections[0].older, 0);
        assert!(model_now.sections[0].can_delete);
        assert_eq!(keys(&model_now.sections[2]), vec![a]);

        groups.rename(&unnamed, " ");
        assert_eq!(
            groups.name(&unnamed),
            "New group",
            "an empty name changes nothing"
        );
        groups.rename(&unnamed, "Later");
        assert_eq!(groups.name(&unnamed), "Later");
    }

    #[test]
    fn groups_and_rows_are_ordered_by_the_last_message() {
        let mut sessions = Sessions::default();
        let a1 = stored(&mut sessions, "a1", MINDRUNE, 300);
        let b1 = stored(&mut sessions, "b1", NOTES, 100);
        let a2 = stored(&mut sessions, "a2", MINDRUNE, 200);
        let a3 = stored(&mut sessions, "a3", MINDRUNE, 200);
        let groups = Groups::default();
        let model_now = model(&sessions, &groups);
        assert_eq!(names(&model_now), vec!["notes", "rune_claude"]);
        assert_eq!(keys(&model_now.sections[0]), vec![b1]);
        assert_eq!(
            keys(&model_now.sections[1]),
            vec![a2, a3, a1],
            "newest first, then by key"
        );

        sessions.get_mut(a1).unwrap().last_active_unix = NOW - 50;
        let model_now = model(&sessions, &groups);
        assert_eq!(names(&model_now), vec!["rune_claude", "notes"]);
        assert_eq!(keys(&model_now.sections[0]), vec![a1, a2, a3]);
    }

    #[test]
    fn old_sessions_are_counted_not_listed() {
        let mut sessions = Sessions::default();
        let old = stored(&mut sessions, "a1", MINDRUNE, 31 * DAY);
        let young = stored(&mut sessions, "a2", MINDRUNE, 29 * DAY);
        let mut groups = Groups::default();
        let model_now = model(&sessions, &groups);
        let home = section(&model_now, &folder(MINDRUNE));
        assert_eq!((keys(home), home.older), (vec![young], 1));

        groups.show_older(&folder(MINDRUNE));
        let model_now = model(&sessions, &groups);
        let home = section(&model_now, &folder(MINDRUNE));
        assert_eq!((keys(home), home.older), (vec![young, old], 0));

        let groups = Groups::default();
        let shown = sidebar_model(&sessions, &groups, &tiles(&[old]), NOW, false);
        let home = section(&shown, &folder(MINDRUNE));
        assert_eq!((keys(home), home.older), (vec![young, old], 0), "shown");
        assert!(row(home, old).shown && !row(home, young).shown);

        sessions.get_mut(old).unwrap().touched = true;
        let model_now = model(&sessions, &groups);
        assert_eq!(
            keys(section(&model_now, &folder(MINDRUNE))).len(),
            2,
            "touched"
        );

        sessions.get_mut(old).unwrap().touched = false;
        sessions.get_mut(old).unwrap().session.phase = Phase::Idle;
        let model_now = model(&sessions, &groups);
        assert_eq!(
            keys(section(&model_now, &folder(MINDRUNE))).len(),
            2,
            "live"
        );
    }

    #[test]
    fn rows_know_their_tile() {
        let mut sessions = Sessions::default();
        let a = stored(&mut sessions, "a1", MINDRUNE, 60);
        let old = stored(&mut sessions, "a2", MINDRUNE, 31 * DAY);
        let b = stored(&mut sessions, "b1", NOTES, 120);
        let mut groups = Groups::default();
        groups.set_hidden(&folder(NOTES), true);
        let model_now = sidebar_model(&sessions, &groups, &tiles(&[old, b]), NOW, false);
        assert_eq!(
            names(&model_now),
            vec!["rune_claude", "notes"],
            "a hidden group with a session in a tile is listed"
        );
        let home = section(&model_now, &folder(MINDRUNE));
        assert_eq!(
            (keys(home), home.older),
            (vec![a, old], 0),
            "an old session in a tile is listed"
        );
        let notes = section(&model_now, &folder(NOTES));
        let tiled = |section: &Section, key| {
            let row = row(section, key);
            (row.shown, row.focused)
        };
        assert_eq!(tiled(home, old), (true, false));
        assert_eq!(tiled(notes, b), (true, true));
        assert_eq!(tiled(home, a), (false, false));
    }

    #[test]
    fn a_folder_with_only_old_sessions_is_an_older_group() {
        let mut sessions = Sessions::default();
        let a = stored(&mut sessions, "a1", MINDRUNE, 60 * DAY);
        sessions.get_mut(a).unwrap().touched = true;
        stored(&mut sessions, "b1", NOTES, 40 * DAY);
        stored(&mut sessions, "b2", NOTES, 50 * DAY);
        let mut groups = Groups::default();
        let model_now = model(&sessions, &groups);
        assert_eq!(names(&model_now), vec!["rune_claude"]);
        assert_eq!(
            (model_now.older_groups, model_now.older_groups_open),
            (1, false)
        );
        assert_eq!(model_now.lines.last(), Some(&Line::OlderGroups));

        groups.toggle_older_groups();
        let model_now = model(&sessions, &groups);
        assert_eq!(
            names(&model_now),
            vec!["rune_claude", "notes"],
            "after the others, though newer"
        );
        let notes = section(&model_now, &folder(NOTES));
        assert_eq!((notes.rows.len(), notes.older), (0, 2));
        assert_eq!(
            (model_now.older_groups, model_now.older_groups_open),
            (1, true)
        );
        assert_eq!(model_now.lines.last(), Some(&Line::OlderGroups));

        groups.toggle_older_groups();
        groups.rename(&folder(NOTES), "Notes");
        let model_now = model(&sessions, &groups);
        assert_eq!(
            names(&model_now),
            vec!["Notes", "rune_claude"],
            "a record of its own"
        );
        assert_eq!(model_now.older_groups, 0);
        assert!(!model_now.lines.contains(&Line::OlderGroups));
    }

    #[test]
    fn a_hidden_group_shows_while_it_needs_you() {
        let mut sessions = Sessions::default();
        stored(&mut sessions, "a1", MINDRUNE, 60);
        let b = stored(&mut sessions, "b1", NOTES, 120);
        let mut groups = Groups::default();
        groups.set_hidden(&folder(NOTES), true);
        let model_now = model(&sessions, &groups);
        assert_eq!(names(&model_now), vec!["rune_claude"]);
        assert_eq!(model_now.hidden, vec![(folder(NOTES), "notes".to_string())]);
        assert_eq!(model_now.lines.last(), Some(&Line::HiddenGroups));

        sessions.get_mut(b).unwrap().session.phase = Phase::NeedsUser;
        let model_now = model(&sessions, &groups);
        assert_eq!(names(&model_now), vec!["rune_claude", "notes"]);
        assert_eq!(section(&model_now, &folder(NOTES)).mark, Mark::NeedsYou);
        assert!(model_now.hidden.is_empty());

        sessions.get_mut(b).unwrap().session.phase = Phase::Idle;
        let model_now = model(&sessions, &groups);
        assert_eq!(names(&model_now), vec!["rune_claude"]);
        assert_eq!(model_now.hidden.len(), 1);

        let shown = sidebar_model(&sessions, &groups, &tiles(&[b]), NOW, false);
        assert_eq!(names(&shown), vec!["rune_claude", "notes"], "shown");

        groups.set_hidden(&folder(NOTES), false);
        let model_now = model(&sessions, &groups);
        assert_eq!(names(&model_now), vec!["rune_claude", "notes"]);
        assert!(model_now.hidden.is_empty());
    }

    #[test]
    fn a_section_carries_its_strongest_mark() {
        let mut sessions = Sessions::default();
        let x = stored(&mut sessions, "a1", MINDRUNE, 60);
        let y = stored(&mut sessions, "a2", MINDRUNE, 70);
        let z = stored(&mut sessions, "a3", MINDRUNE, 80);
        let groups = Groups::default();
        let mark = |sessions: &Sessions| section(&model(sessions, &groups), &folder(MINDRUNE)).mark;
        assert_eq!(mark(&sessions), Mark::None);
        sessions.get_mut(x).unwrap().unseen = true;
        assert_eq!(mark(&sessions), Mark::Finished);
        sessions.get_mut(y).unwrap().session.phase = Phase::Working;
        assert_eq!(mark(&sessions), Mark::Working);
        sessions.get_mut(z).unwrap().session.phase = Phase::NeedsUser;
        assert_eq!(mark(&sessions), Mark::NeedsYou);
        let model_now = model(&sessions, &groups);
        let home = section(&model_now, &folder(MINDRUNE));
        let marks: Vec<Mark> = home.rows.iter().map(|row| row.mark).collect();
        assert_eq!(marks, vec![Mark::Finished, Mark::Working, Mark::NeedsYou]);
    }

    #[test]
    fn a_session_without_an_id_keeps_its_place_when_it_gets_one() {
        let mut sessions = Sessions::default();
        let key = sessions.add(Session::opening(MINDRUNE), NOW);
        let mut groups = Groups::default();
        let side = groups.create("Side projects");
        groups.place(entry(&sessions, key), &side);
        assert_eq!(groups.group_of(entry(&sessions, key)), side);
        let id = SessionId("s1".into());
        sessions.get_mut(key).unwrap().session.id = Some(id.clone());
        groups.adopt(key, &id);
        assert_eq!(groups.group_of(entry(&sessions, key)), side);
        assert_eq!(keys(section(&model(&sessions, &groups), &side)), vec![key]);
    }

    #[test]
    fn lines_follow_the_sections() {
        let mut sessions = Sessions::default();
        stored(&mut sessions, "a1", MINDRUNE, 60);
        stored(&mut sessions, "a2", MINDRUNE, 70);
        stored(&mut sessions, "a3", MINDRUNE, 40 * DAY);
        stored(&mut sessions, "a4", MINDRUNE, 50 * DAY);
        stored(&mut sessions, "b1", NOTES, 120);
        stored(&mut sessions, "b2", NOTES, 130);
        let mut groups = Groups::default();
        groups.set_collapsed(&folder(NOTES), true);
        let archive = groups.create("Archive");
        groups.set_hidden(&archive, true);
        let model_now = model(&sessions, &groups);
        assert_eq!(names(&model_now), vec!["rune_claude", "notes"]);
        assert_eq!(model_now.sections[0].older, 2);
        use Line::*;
        assert_eq!(
            model_now.lines,
            vec![
                Header(0),
                Row(0, 0),
                Row(0, 1),
                ShowOlder(0),
                Header(1),
                NewGroup,
                HiddenGroups
            ]
        );
        assert_eq!(model_now.hidden, vec![(archive, "Archive".to_string())]);

        let loading = sidebar_model(&sessions, &groups, &Tiles::default(), NOW, true);
        assert!(loading.loading);
        assert_eq!(
            loading.lines[4..],
            [Header(1), Loading, NewGroup, HiddenGroups]
        );
    }

    #[test]
    fn key_text_is_stable_and_distinct() {
        let home = folder(MINDRUNE).key_text();
        assert_ne!(home, folder(NOTES).key_text());
        assert_eq!(home, GroupId::Folder(PathBuf::from(MINDRUNE)).key_text());
        assert_eq!(
            home,
            folder("/home/alex/rune_claude/").key_text(),
            "as paths compare"
        );
        assert_eq!(
            home, "f1c9acc7e3bc1f90c",
            "the same in every run of every build"
        );
        assert_eq!(GroupId::Custom(3).key_text(), "c3");
    }
}
