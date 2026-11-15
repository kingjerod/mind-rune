//! The sessions this app knows: each one's session, the number of its current open, and what
//! the sidebar marks it with.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::{Deref, DerefMut};

use mr_agent::{SessionId, StoredSession};

use crate::session::{Phase, Session};

/// This app's own number for a session it knows. It lasts for one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionKey(pub u64);

/// What a session's row is marked with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    None,
    Working,
    NeedsYou,
    Finished,
}

/// A question the app waits on the user for, about one session. Each session has at most one.
#[derive(Clone, Debug, PartialEq)]
pub enum Prompt {
    /// The session's stored history changed very recently, so another app may have it open.
    /// `text` is the message the user sent it, held until they choose.
    OpenElsewhere { text: String },
}

/// One session the app knows.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: SessionKey,
    pub session: Session,
    /// The number of its current open; `None` while no program runs for it.
    pub open: Option<u64>,
    /// When the user last sent a message in it, or what the store said: Unix seconds.
    pub last_active_unix: i64,
    /// When its own program last reported anything, in this run: Unix seconds.
    pub last_own_unix: Option<i64>,
    /// A turn ended in it while it was not the focused session, also when it was in a tile, and
    /// it has not been focused since.
    pub unseen: bool,
    /// It was in a tile or started in this run.
    pub touched: bool,
    /// The store is being asked whether another app may have it open, before it wakes.
    pub checking: bool,
    /// The question waiting for the user about it.
    pub prompt: Option<Prompt>,
}

impl Entry {
    /// `NeedsYou` while a request or a question waits, `Working` while a turn runs or a message
    /// waits for the program to start, `Finished` while it is `unseen`.
    pub fn mark(&self) -> Mark {
        if self.prompt.is_some() {
            return Mark::NeedsYou;
        }
        match self.session.phase {
            Phase::NeedsUser => Mark::NeedsYou,
            Phase::Working => Mark::Working,
            Phase::Opening if self.session.queued.is_some() => Mark::Working,
            _ if self.unseen => Mark::Finished,
            _ => Mark::None,
        }
    }

    /// A program runs for it, or is starting.
    pub fn live(&self) -> bool {
        !self.session.at_rest()
    }
}

/// Every session the app knows, by key. Keys are never reused in a run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sessions {
    entries: BTreeMap<SessionKey, Entry>,
    /// The number the next key gets.
    next_key: u64,
    /// See [`Sessions::version`]. A cell, so that every entry [`Sessions::iter_mut`] lends out
    /// can put it up.
    version: Cell<u64>,
}

impl Sessions {
    /// Adds `session` under a new key, with no open, not shown and not touched yet.
    pub fn add(&mut self, session: Session, last_active_unix: i64) -> SessionKey {
        self.bump();
        self.next_key += 1;
        let key = SessionKey(self.next_key);
        let entry = Entry {
            key,
            session,
            open: None,
            last_active_unix,
            last_own_unix: None,
            unseen: false,
            touched: false,
            checking: false,
            prompt: None,
        };
        self.entries.insert(key, entry);
        key
    }

    /// Adds what is new, refreshes the title and time of resting entries, and, when `complete`,
    /// removes resting entries that were never touched and are no longer in the store. A title
    /// the user gave is not replaced (see [`Session::take_listed_title`]). A new entry whose id
    /// has a name among `names` takes that name, and the name leaves `names`.
    pub fn merge_listing(
        &mut self,
        listed: Vec<StoredSession>,
        complete: bool,
        names: &mut Vec<(SessionId, String)>,
    ) {
        let mut known: HashMap<SessionId, SessionKey> = self
            .entries
            .values()
            .filter_map(|entry| Some((entry.session.id.clone()?, entry.key)))
            .collect();
        let mut in_store = HashSet::new();
        for stored in listed {
            match known.get(&stored.id).copied() {
                Some(key) => {
                    if let Some(mut entry) = self.get_mut(key)
                        && entry.session.phase == Phase::Resting
                    {
                        entry.session.take_listed_title(&stored.title);
                        entry.last_active_unix = stored.last_active_unix;
                    }
                }
                None => {
                    let mut session = Session::resting(&stored);
                    if let Some(at) = names.iter().position(|(id, _)| *id == stored.id) {
                        session.keep_name(names.remove(at).1);
                        session.take_listed_title(&stored.title);
                    }
                    let key = self.add(session, stored.last_active_unix);
                    known.insert(stored.id.clone(), key);
                }
            }
            in_store.insert(stored.id);
        }
        if complete {
            self.retain(|entry| {
                entry.session.phase != Phase::Resting
                    || entry.touched
                    || entry
                        .session
                        .id
                        .as_ref()
                        .is_none_or(|id| in_store.contains(id))
            });
        }
    }

    /// Removes the resting entries, never touched, that have the same agent's id as `key`'s
    /// session: a listing that arrived before `key`'s program said its id added them.
    pub fn drop_copies_of(&mut self, key: SessionKey) {
        let Some(id) = self.get(key).and_then(|entry| entry.session.id.clone()) else {
            return;
        };
        self.retain(|entry| {
            entry.key == key
                || entry.touched
                || entry.session.phase != Phase::Resting
                || entry.session.id.as_ref() != Some(&id)
        });
    }

    pub fn remove(&mut self, key: SessionKey) -> Option<Entry> {
        let removed = self.entries.remove(&key);
        if removed.is_some() {
            self.bump();
        }
        removed
    }

    pub fn get(&self, key: SessionKey) -> Option<&Entry> {
        self.entries.get(&key)
    }

    pub fn get_mut(&mut self, key: SessionKey) -> Option<EntryMut<'_>> {
        let version = &self.version;
        self.entries
            .get_mut(&key)
            .map(|entry| EntryMut::new(entry, version))
    }

    /// The entry whose current open is number `open`.
    pub fn by_open(&mut self, open: u64) -> Option<EntryMut<'_>> {
        let version = &self.version;
        self.entries
            .values_mut()
            .find(|entry| entry.open == Some(open))
            .map(|entry| EntryMut::new(entry, version))
    }

    /// The key of the entry whose session has the agent's id `id`.
    pub fn by_id(&self, id: &SessionId) -> Option<SessionKey> {
        self.entries
            .values()
            .find(|entry| entry.session.id.as_ref() == Some(id))
            .map(|entry| entry.key)
    }

    /// Every entry, in the order they were added.
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = EntryMut<'_>> {
        let version = &self.version;
        self.entries
            .values_mut()
            .map(move |entry| EntryMut::new(entry, version))
    }

    /// Goes up whenever an entry is added or removed, or an entry lent out to change comes
    /// back with something the sidebar shows of it changed (see [`EntryMut`]). Every way to
    /// change an entry goes through one of these, so a caller can tell that nothing the sidebar
    /// shows changed without comparing.
    pub fn version(&self) -> u64 {
        self.version.get()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn bump(&self) {
        self.version.set(self.version.get() + 1);
    }

    /// Keeps the entries `keep` says to keep.
    fn retain(&mut self, mut keep: impl FnMut(&Entry) -> bool) {
        let before = self.entries.len();
        self.entries.retain(|_, entry| keep(entry));
        if self.entries.len() != before {
            self.bump();
        }
    }
}

/// An entry lent out to change. When it is given back, its [`Sessions::version`] goes up if
/// anything the sidebar shows of it changed: its mark, whether it is live or touched, when it
/// was last active, its title, its id or its folder.
pub struct EntryMut<'a> {
    entry: &'a mut Entry,
    version: &'a Cell<u64>,
    before: SidebarFacts,
}

impl<'a> EntryMut<'a> {
    fn new(entry: &'a mut Entry, version: &'a Cell<u64>) -> EntryMut<'a> {
        let before = SidebarFacts::of(entry);
        EntryMut {
            entry,
            version,
            before,
        }
    }
}

impl Deref for EntryMut<'_> {
    type Target = Entry;

    fn deref(&self) -> &Entry {
        self.entry
    }
}

impl DerefMut for EntryMut<'_> {
    fn deref_mut(&mut self) -> &mut Entry {
        self.entry
    }
}

impl Drop for EntryMut<'_> {
    fn drop(&mut self) {
        if SidebarFacts::of(self.entry) != self.before {
            self.version.set(self.version.get() + 1);
        }
    }
}

/// Everything the sidebar's model reads of an entry, but its key, which never changes. The
/// strings are kept as one hash so that lending an entry out allocates nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SidebarFacts {
    mark: Mark,
    live: bool,
    touched: bool,
    last_active_unix: i64,
    /// The title, the id and the folder.
    names: u64,
}

impl SidebarFacts {
    fn of(entry: &Entry) -> SidebarFacts {
        let mut names = DefaultHasher::new();
        (
            &entry.session.title,
            &entry.session.id,
            &entry.session.folder,
        )
            .hash(&mut names);
        SidebarFacts {
            mark: entry.mark(),
            live: entry.live(),
            touched: entry.touched,
            last_active_unix: entry.last_active_unix,
            names: names.finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use mr_agent::{SessionEvent, StoredSession};

    use super::*;
    use crate::session::testing::info;

    fn entry(session: Session) -> Entry {
        let mut sessions = Sessions::default();
        let key = sessions.add(session, 0);
        sessions.remove(key).unwrap()
    }

    #[test]
    fn the_mark_follows_the_phase_and_what_was_not_seen() {
        let mut opening = entry(Session::opening("/p"));
        assert_eq!(opening.mark(), Mark::None, "opening with nothing queued");
        opening.session.submit("early");
        assert_eq!(
            opening.mark(),
            Mark::Working,
            "opening with a message queued"
        );

        let mut started = Session::opening("/p");
        started.apply(SessionEvent::Started(info()), 0.0);
        let mut idle = entry(started.clone());
        assert_eq!(idle.mark(), Mark::None);
        idle.unseen = true;
        assert_eq!(idle.mark(), Mark::Finished);

        let mut working = entry(started.clone());
        working.session.submit("go");
        working.unseen = true;
        assert_eq!(working.mark(), Mark::Working, "working wins over finished");
        working.session.phase = Phase::NeedsUser;
        assert_eq!(
            working.mark(),
            Mark::NeedsYou,
            "needs you wins over finished"
        );

        let mut ended = entry(started);
        ended.session.phase = Phase::Ended {
            clean: false,
            detail: "killed".into(),
        };
        assert_eq!(ended.mark(), Mark::None);
        ended.unseen = true;
        assert_eq!(ended.mark(), Mark::Finished);

        let stored = StoredSession {
            id: SessionId("s9".into()),
            folder: "/home/alex/project".into(),
            title: "Fix the parser".into(),
            last_active_unix: 5,
        };
        let mut asked = entry(Session::resting(&stored));
        asked.unseen = true;
        asked.prompt = Some(Prompt::OpenElsewhere {
            text: "carry on".into(),
        });
        assert_eq!(asked.mark(), Mark::NeedsYou, "a question waits");
    }

    #[test]
    fn a_session_is_live_until_no_program_runs_for_it() {
        let mut live = entry(Session::opening("/p"));
        assert!(live.live(), "opening");
        live.session.apply(SessionEvent::Started(info()), 0.0);
        assert!(live.live(), "idle");
        live.session.phase = Phase::Ended {
            clean: true,
            detail: String::new(),
        };
        assert!(!live.live(), "ended");
        let stored = StoredSession {
            id: SessionId("s9".into()),
            folder: "/home/alex/project".into(),
            title: "Fix the parser".into(),
            last_active_unix: 5,
        };
        assert!(!entry(Session::resting(&stored)).live(), "resting");
    }

    #[test]
    fn entries_are_found_by_key_by_open_and_by_id() {
        let mut sessions = Sessions::default();
        assert!(sessions.is_empty());
        let a = sessions.add(Session::opening("/a"), 10);
        let b = sessions.add(Session::opening("/b"), 20);
        assert_ne!(a, b);
        let entry = sessions.get(b).unwrap();
        assert_eq!(
            (entry.key, entry.last_active_unix, entry.open, entry.touched),
            (b, 20, None, false)
        );
        sessions.get_mut(b).unwrap().open = Some(7);
        sessions.get_mut(a).unwrap().session.id = Some(SessionId("s1".into()));
        assert_eq!(sessions.by_open(7).map(|e| e.key), Some(b));
        assert!(sessions.by_open(8).is_none());
        assert_eq!(sessions.by_id(&SessionId("s1".into())), Some(a));
        assert_eq!(sessions.by_id(&SessionId("s2".into())), None);
        let keys: Vec<_> = sessions.iter().map(|e| e.key).collect();
        assert_eq!(keys, vec![a, b]);
        assert_eq!(sessions.remove(a).map(|e| e.key), Some(a));
        assert_eq!(sessions.len(), 1);
        let c = sessions.add(Session::opening("/c"), 30);
        assert!(c != a && c != b, "a key is never given twice");
    }

    #[test]
    fn the_version_goes_up_only_for_what_the_sidebar_shows() {
        let mut sessions = Sessions::default();
        let mut last = sessions.version();
        let mut moved = |sessions: &Sessions| {
            let moved = sessions.version() != last;
            last = sessions.version();
            moved
        };
        let key = sessions.add(Session::opening("/a"), 10);
        assert!(moved(&sessions), "add");
        sessions.get_mut(key).unwrap().open = Some(1);
        sessions.get_mut(key).unwrap().last_own_unix = Some(5);
        sessions.by_open(1).unwrap().session.cost_usd = Some(0.5);
        sessions
            .iter_mut()
            .for_each(|mut entry| entry.session.queued = None);
        assert!(!moved(&sessions), "nothing the sidebar shows");

        type Change = (&'static str, fn(&mut Entry));
        let changes: [Change; 9] = [
            ("touched", |entry| entry.touched = true),
            ("last active", |entry| entry.last_active_unix = 20),
            ("mark", |entry| entry.unseen = true),
            ("live and mark", |entry| {
                entry.session.phase = Phase::Resting
            }),
            ("title", |entry| entry.session.title = Some("Fix it".into())),
            ("id", |entry| {
                entry.session.id = Some(SessionId("s1".into()))
            }),
            ("folder", |entry| entry.session.folder = "/b".into()),
            ("question", |entry| {
                entry.prompt = Some(Prompt::OpenElsewhere {
                    text: "carry on".into(),
                })
            }),
            ("question answered", |entry| entry.prompt = None),
        ];
        for (what, change) in changes {
            change(&mut sessions.get_mut(key).unwrap());
            assert!(moved(&sessions), "{what} through get_mut");
        }
        sessions.by_open(1).unwrap().touched = false;
        assert!(moved(&sessions), "through by_open");
        sessions
            .iter_mut()
            .for_each(|mut entry| entry.unseen = false);
        assert!(moved(&sessions), "through iter_mut");

        let stored = StoredSession {
            id: SessionId("s1".into()),
            folder: "/b".into(),
            title: "Fix it".into(),
            last_active_unix: 20,
        };
        sessions.merge_listing(vec![stored.clone()], true, &mut Vec::new());
        assert!(!moved(&sessions), "a listing that tells nothing new");
        let retitled = StoredSession {
            title: "Fix it all".into(),
            ..stored.clone()
        };
        sessions.merge_listing(vec![retitled], false, &mut Vec::new());
        assert!(moved(&sessions), "a new title");
        let other = StoredSession {
            id: SessionId("s2".into()),
            ..stored
        };
        sessions.merge_listing(vec![other], false, &mut Vec::new());
        assert!(moved(&sessions), "a new entry");
        sessions.drop_copies_of(key);
        assert!(!moved(&sessions), "no copies");
        assert!(sessions.remove(SessionKey(99)).is_none());
        assert!(!moved(&sessions), "nothing to remove");
        sessions.remove(key);
        assert!(moved(&sessions), "remove");
        let _ = (sessions.get(key), sessions.by_id(&SessionId("s2".into())));
        assert_eq!(sessions.iter().count(), 1);
        assert!(!moved(&sessions), "reading changes nothing");
    }
}
