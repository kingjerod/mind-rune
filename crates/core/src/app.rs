//! Which screen shows, the sessions the app knows and the tiles it shows them in, and the
//! numbered opens that keep a replaced program from changing anything. Every intent returns the
//! effects to carry out instead of doing the work, so this module stays plain data.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use mr_agent::{
    Account, AgentError, Answer, Capabilities, CommandKind, ImageData, LimitWindow, ModelOption,
    PermissionMode, Removal, RequestId, SessionCommand, SessionEvent, SessionId, StartSpec,
    StoredSession, UsageLimits,
};
use serde::{Deserialize, Serialize};

use crate::attach::{Chip, ChipState, Prepared, Problem, Source, name_of};
use crate::backend::FromBackend;
use crate::conversation::NoticeKind;
use crate::git::Branch;
use crate::groups::{
    GroupId, Groups, Recency, SidebarModel, SidebarView, row_title, sidebar_model,
};
use crate::saved::{
    RECENT_FOLDERS, SAVED_VERSION, Saved, SavedBelow, SavedDeleted, SavedTile, remember_folder,
};
use crate::session::{FullImage, HistoryState, Phase, Remote, Session, Undelivered};
use crate::sessions::{Alert, Deleted, Entry, Mark, Prompt, SessionKey, Sessions};
use crate::tiles::{Column, Place, Put, RowSize, Spot, Tiles, Under};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Checking,
    NotInstalled,
    SignedOut,
    Main,
}

/// What a quit would cut off, in sessions: those whose program works for the user, and those
/// with Remote Control on. One session can be in both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Busy {
    pub working: usize,
    pub remote: usize,
}

/// How the sessions stand, counted by their mark: what the tray's menu says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Standing {
    pub working: usize,
    pub needs_you: usize,
    pub finished: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Width {
    Points(f32),
    Unlimited,
}

pub const DEFAULT_WIDTH: f32 = 1000.0;
pub const MIN_WIDTH: f32 = 480.0;

/// How far back a recent listing reaches, in days. Every listing is a recent one but the single
/// listing of all stored sessions.
pub const RECENT_DAYS: i64 = 30;
/// The most history items read for a stored session.
pub const HISTORY_ITEMS: usize = 2000;
/// A stored session whose history changed this recently may be open in another app: seconds.
pub const ELSEWHERE_SECONDS: i64 = 120;
/// How long after this app closed a session's program may still write to its history as it
/// exits: seconds. A change that late is still this app's own.
pub const OWN_EXIT_SLACK_SECONDS: i64 = 10;
/// How often the recent listing is asked for again: seconds.
pub const RELIST_SECONDS: i64 = 60;
/// How long after the usage limits were last asked for they may be asked for once more, once a
/// window has reset: seconds. An answer on its way is waited for that long.
pub const USAGE_RETRY_SECONDS: i64 = 300;
/// How long a program does nothing before it is stopped, when the user would not notice (see
/// [`Session::stoppable`]): seconds.
pub const IDLE_SECONDS: i64 = 30 * 60;

/// How long a session stays in the Deleted view before it is removed for good: seconds.
pub const DELETED_SECONDS: i64 = 7 * 24 * 60 * 60;
/// How often the tick removes what has been deleted long enough: seconds.
pub const PURGE_SECONDS: i64 = 60;
/// How long after a removal failed it is tried again: seconds.
pub const PURGE_RETRY_SECONDS: i64 = 60 * 60;

/// How long a resting session is out of every tile before it gives its conversation back to
/// the store (see [`App::let_go`]): seconds.
pub const LET_GO_SECONDS: i64 = 10 * 60;

/// How far the listing of stored sessions has come.
#[derive(Clone, Debug, PartialEq)]
pub enum Listing {
    NotAsked,
    Loading,
    Done,
    /// The last listing failed, for this reason. The entries there are stay.
    Failed(String),
}

/// What becomes of the session that leaves its tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leaving {
    /// It is closed and forgotten when it is empty: nothing was handed over in it and nothing
    /// waits to be, so the agent stores nothing for it.
    ForgetIfEmpty,
    /// It stays, empty or not: the window has text typed in its input box.
    Keep,
}

/// What deleting a session would do to it ([`App::deleting`]): what the question asked first
/// has to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deleting {
    /// The store has it and nothing would be cut off: it goes to the Deleted view.
    Kept,
    /// Its program would be stopped at work: a turn runs, a request waits, a background task
    /// runs, or Remote Control is on or coming.
    Stops,
    /// The store has it and nothing would be stopped, but a message in it that was not sent
    /// yet would be dropped: one queued, never delivered, or held by a question.
    DropsMessage,
    /// The store has nothing of it, so it would be forgotten for good. `holds`: chips or a
    /// message not sent yet are in it. A draft typed in its box is the window's to know.
    Forgotten { holds: bool },
}

/// How the user answered [`Prompt::OpenElsewhere`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptChoice {
    ContinueHere,
    ContinueAsCopy,
    Cancel,
}

/// Work for the backend to carry out. Each `open` is the number of the open
/// the work belongs to.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    CheckAccount,
    Open {
        open: u64,
        spec: StartSpec,
    },
    Send {
        open: u64,
        command: SessionCommand,
    },
    SaveWidth(Width),
    ListSessions {
        since_unix: Option<i64>,
    },
    ReadHistory {
        key: SessionKey,
        id: SessionId,
        max_items: usize,
    },
    /// `key`'s history is no longer wanted: a read not done yet is skipped, and one under way
    /// answers nothing.
    DropHistory {
        key: SessionKey,
    },
    CheckActivity {
        key: SessionKey,
        id: SessionId,
    },
    Look {
        folder: PathBuf,
    },
    /// Read the git branch of `folder`.
    ReadBranch {
        folder: PathBuf,
    },
    /// Read and check what chip `chip` holds.
    Prepare {
        chip: u64,
        source: Source,
    },
    /// Read again the full image behind picture `picture` (see [`App::full_image`]).
    ReadImage {
        picture: u64,
        id: SessionId,
        message: String,
        index: usize,
    },
    /// Remove the stored session `id` for good, unless its stored file changed after
    /// `unless_after_unix`.
    RemoveStored {
        id: SessionId,
        unless_after_unix: i64,
    },
    /// Write what is kept between runs.
    SaveState(Box<Saved>),
    /// Tell the desktop that `key`'s session, titled `title`, is ready or needs input; a
    /// notification about it that is out already is replaced.
    Notify {
        key: SessionKey,
        title: String,
        alert: Alert,
    },
    /// Take back the notification about `key`'s session.
    Unnotify {
        key: SessionKey,
    },
    /// Take back every notification.
    UnnotifyAll,
}

/// What the app was started with.
#[derive(Clone, Debug, Default)]
pub struct Launch {
    /// The folder it was started in: once the account is signed in, a new session starts there.
    pub folder: Option<PathBuf>,
    /// Without a launch folder, a session is started in the background to learn the models
    /// and the usage limits (a look).
    pub look: bool,
    /// The user's home folder: where a look starts when no session is known.
    pub home: PathBuf,
    /// What was kept from the last run. [`App::new`] takes it out: the app keeps no copy here.
    pub saved: Saved,
}

pub struct App {
    pub screen: Screen,
    pub account: Option<Account>,
    pub capabilities: Capabilities,
    pub usage: UsageLimits,
    pub sessions: Sessions,
    /// The row of tiles the window shows sessions in.
    pub tiles: Tiles,
    pub width: Width,
    /// The wall clock in Unix seconds, as the app was last told it: what the user's and the
    /// programs' activity is stamped with.
    pub now_unix: i64,
    pub listing: Listing,
    /// The listing of every stored session, not only the recent ones, has been merged.
    pub listed_all: bool,
    /// A listing of every stored session that held at least one has been merged: only from
    /// then on is what the store no longer has forgotten (see [`App::listed`]).
    store_seen: bool,
    /// The newest list of models any program or look gave.
    pub known_models: Vec<ModelOption>,
    pub groups: Groups,
    /// What the sidebar shows, as of the last [`App::settle`].
    pub sidebar: SidebarModel,
    /// Goes up by one each time [`App::settle`] rebuilds `sidebar`.
    pub sidebar_version: u64,
    /// The list the sidebar shows. Not saved: a run starts with the sessions.
    sidebar_view: SidebarView,
    /// The folders new sessions started in, the latest first: at most [`RECENT_FOLDERS`], each
    /// once.
    pub recent_folders: Vec<PathBuf>,
    /// New sessions start with Remote Control wanted.
    pub remote_for_new: bool,
    launch: Launch,
    /// The number of the latest open. Each open gets the next, so a number is never reused.
    open: u64,
    /// When the recent listing was last asked for; `None` before signing in.
    listed_unix: Option<i64>,
    /// When the tick last removed what was deleted long enough; `None` before it first did.
    purged_unix: Option<i64>,
    /// The listing of every stored session was asked for and has not failed.
    all_asked: bool,
    /// The look after the first listing was asked for.
    looked: bool,
    /// When the usage limits were last asked for, by a program or by a look.
    usage_asked_unix: Option<i64>,
    /// The latest reset time, of the usage windows whose reset had passed, that the limits were
    /// asked about again for: each such reset is asked about once.
    usage_reset_asked: Option<i64>,
    /// What `sidebar` was built from; `None` before the first [`App::settle`].
    settled: Option<Settled>,
    /// The size of the tile row as the window last laid it out; each `None` until it has.
    row: RowSize,
    /// The tiles open when the last run ended, as read, while they may still be opened again:
    /// until a listing knows them all or is complete, or anything is put in a tile.
    restore: Option<Waiting>,
    /// The entry focused because it was focused when the last run ended.
    restored: Option<SessionKey>,
    /// From the last run: the sessions whose program it ran, with when it closed (see
    /// [`Saved::closed_by_app`]).
    closed_by_app: Vec<(SessionId, i64)>,
    /// From the last run: the names kept for sessions no listing has given yet (see
    /// [`Saved::names`]).
    names: Vec<(SessionId, String)>,
    /// From the last run: the marks of deleted sessions no listing has given yet (see
    /// [`Saved::deleted`]).
    deleted_marks: Vec<SavedDeleted>,
    /// The branch last read for each folder; a folder whose answer was none is not here.
    branches: HashMap<PathBuf, Branch>,
    /// The id of the latest chip. Each chip gets the next, so an id is never reused.
    chip: u64,
    /// [`App::close`] was called: no program is started again for a message it never got.
    closing: bool,
    /// How long a program does nothing before it is stopped: seconds.
    idle_after: i64,
    /// A window shows ([`App::set_window`]).
    window: bool,
    /// The window has the keyboard ([`App::set_active`]).
    active: bool,
    /// Each session's mark as the last [`App::settle`] saw it.
    marks: HashMap<SessionKey, Mark>,
    /// The sessions' version and whether the user looked, as the last [`App::settle`] saw
    /// them: while both hold, no mark changed.
    alerts_seen: Option<(u64, bool)>,
}

/// The tiles of the last run, as [`Saved`] had them.
#[derive(Debug)]
struct Waiting {
    tiles: Vec<SavedTile>,
    shown: Option<SessionId>,
}

impl Waiting {
    /// The tiles to open: a state with no tiles and a shown session (one plan 3 wrote) has
    /// that one alone.
    fn row(&self) -> Vec<SavedTile> {
        match (&self.tiles[..], &self.shown) {
            ([], Some(id)) => vec![SavedTile {
                id: id.clone(),
                share: 1.0,
                below: None,
            }],
            _ => self.tiles.clone(),
        }
    }
}

/// What the sidebar's model was last built from: while all of it holds, the model holds.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Settled {
    sessions: u64,
    groups: u64,
    placement: u64,
    loading: bool,
    complete: bool,
    view: SidebarView,
    recency: Recency,
}

impl App {
    /// Starts on `Screen::Checking` and asks for the account, with what `launch` kept from the
    /// last run. Its tiles open again once a listing knows them, unless there is a launch
    /// folder.
    pub fn new(
        capabilities: Capabilities,
        width: Width,
        mut launch: Launch,
        now_unix: i64,
    ) -> (App, Vec<Effect>) {
        let saved = std::mem::take(&mut launch.saved);
        let mut recent_folders = saved.recent_folders.clone();
        recent_folders.truncate(RECENT_FOLDERS);
        let restore = Waiting {
            tiles: saved.tiles.clone(),
            shown: saved.shown.clone(),
        };
        let nothing = restore.tiles.is_empty() && restore.shown.is_none();
        let restore = (launch.folder.is_none() && !nothing).then_some(restore);
        let app = App {
            screen: Screen::Checking,
            account: None,
            capabilities,
            usage: UsageLimits {
                session: None,
                weekly: None,
            },
            sessions: Sessions::default(),
            tiles: Tiles::default(),
            width,
            now_unix,
            listing: Listing::NotAsked,
            listed_all: false,
            store_seen: false,
            known_models: Vec::new(),
            // Before any `settle`, so no version it has seen is used again.
            groups: Groups::restored(&saved),
            sidebar: SidebarModel::default(),
            sidebar_version: 0,
            sidebar_view: SidebarView::Sessions,
            recent_folders,
            remote_for_new: saved.remote_for_new,
            launch,
            open: 0,
            listed_unix: None,
            purged_unix: None,
            all_asked: false,
            looked: false,
            usage_asked_unix: None,
            usage_reset_asked: None,
            settled: None,
            row: RowSize::default(),
            restore,
            restored: None,
            closed_by_app: saved.closed_by_app,
            names: saved.names,
            deleted_marks: saved.deleted,
            branches: HashMap::new(),
            chip: 0,
            closing: false,
            idle_after: IDLE_SECONDS,
            window: true,
            active: true,
            marks: HashMap::new(),
            alerts_seen: None,
        };
        (app, vec![Effect::CheckAccount])
    }

    /// The session in the focused tile.
    pub fn focused(&self) -> Option<SessionKey> {
        self.tiles.focused()
    }

    pub fn focused_entry(&self) -> Option<&Entry> {
        self.focused().and_then(|key| self.sessions.get(key))
    }

    /// The full image behind a picture of `key`'s session: its bytes when they are at hand, else
    /// the effect that reads them from the store, under the id the conversation was read from,
    /// else neither.
    pub fn full_image(&self, key: SessionKey, picture: u64) -> (Option<ImageData>, Vec<Effect>) {
        let Some(entry) = self.sessions.get(key) else {
            return (None, Vec::new());
        };
        match entry.session.full_image(picture) {
            FullImage::AtHand(image) => (Some(image), Vec::new()),
            FullImage::Stored { id, message, index } => (
                None,
                vec![Effect::ReadImage {
                    picture,
                    id,
                    message,
                    index,
                }],
            ),
            FullImage::Nowhere => (None, Vec::new()),
        }
    }

    /// The branch last read for `key`'s folder.
    pub fn branch_of(&self, key: SessionKey) -> Option<&Branch> {
        self.branches.get(&self.sessions.get(key)?.session.folder)
    }

    /// Whether a window shows. While none does, a turn that ends leaves its session ready also
    /// in the focused tile, and no look is made for the usage limits; when one shows again the
    /// limits are asked for when that is due (see [`App::usage_after_reset`]). A session ready
    /// stays so until the user acts on it ([`App::attend`]).
    pub fn set_window(&mut self, shown: bool) -> Vec<Effect> {
        self.window = shown;
        if !shown || self.screen != Screen::Main {
            return Vec::new();
        }
        self.usage_after_reset()
    }

    /// Whether the window has the keyboard. While it has not, a turn that ends leaves its
    /// session ready also in the focused tile.
    pub fn set_active(&mut self, active: bool) -> Vec<Effect> {
        self.active = active;
        Vec::new()
    }

    /// The user is looking at MindRune: a window shows and has the keyboard. Notifications are
    /// given only while they are not, and taken back when they are ([`App::alerts`]).
    pub fn looking(&self) -> bool {
        self.window && self.active
    }

    /// The user acted on this session: it is no longer ready. A press on its tile or row, the
    /// keyboard coming to its tile, a message sent or a question answered in it, and the like
    /// come here ([`App::focus_tile`], [`App::show`], [`App::move_tile`], the session intents),
    /// and so does typing in its input box. Nothing for a deleted session.
    pub fn attend(&mut self, key: SessionKey) -> Vec<Effect> {
        if let Some(mut entry) = self.sessions.get_mut(key)
            && entry.deleted.is_none()
        {
            entry.unseen = false;
        }
        Vec::new()
    }

    /// The user pressed the notification of this session. As a press on its row
    /// ([`App::show`]): its tile takes the focus, it shows in the focused tile when it is in
    /// none, the session leaving that tile leaves as `leaving` says, and the user acted on it
    /// ([`App::attend`]). Nothing for a session that is gone or deleted.
    pub fn alert_pressed(&mut self, key: SessionKey, leaving: Leaving) -> Vec<Effect> {
        self.show(key, Place::Focused, leaving)
    }

    /// Another idle length for this run; anything under 60 is taken as 60.
    pub fn set_idle_after(&mut self, seconds: i64) {
        self.idle_after = seconds.max(60);
    }

    /// The width of the tile row, as the window last laid it out.
    pub fn set_row_width(&mut self, width: f32) {
        self.row.width = Some(width);
    }

    pub fn row_width(&self) -> Option<f32> {
        self.row.width
    }

    /// The row's height, as the window last laid it out.
    pub fn set_row_height(&mut self, height: f32) {
        self.row.height = Some(height);
    }

    pub fn row_height(&self) -> Option<f32> {
        self.row.height
    }

    /// What is known of the row's size.
    pub fn row_size(&self) -> RowSize {
        self.row
    }

    /// A tile more would fit. True while the row's width is not known.
    pub fn room_for_tile(&self) -> bool {
        self.row
            .width
            .is_none_or(|width| self.tiles.fits_another(width))
    }

    /// Picks the screen for `account`. Signed in with a launch folder and no tile, a new
    /// session starts in that folder. Signed in for the first time, the recent stored sessions
    /// are asked for.
    pub fn account_checked(&mut self, account: Account) -> Vec<Effect> {
        self.screen = match account {
            Account::Missing => Screen::NotInstalled,
            Account::SignedOut => Screen::SignedOut,
            Account::SignedIn { .. } => Screen::Main,
        };
        let signed_in = self.screen == Screen::Main;
        self.account = Some(account);
        let mut effects = Vec::new();
        if signed_in
            && self.tiles.is_empty()
            && let Some(folder) = self.launch.folder.clone()
        {
            // There is no tile yet, so nothing leaves one.
            effects = self.new_session(folder, Place::Focused, Leaving::ForgetIfEmpty);
        }
        if signed_in && self.listing == Listing::NotAsked {
            effects.push(self.list_recent());
        }
        effects
    }

    pub fn check_again(&mut self) -> Vec<Effect> {
        if matches!(self.screen, Screen::NotInstalled | Screen::SignedOut) {
            self.screen = Screen::Checking;
            return vec![Effect::CheckAccount];
        }
        Vec::new()
    }

    /// The open numbered `open` could not start its program. When the agent is not installed,
    /// its session and its tile go and the app says so; any other failure ends the session.
    pub fn open_failed(&mut self, open: u64, error: AgentError) -> Vec<Effect> {
        let Some(key) = self.sessions.by_open(open).map(|entry| entry.key) else {
            return Vec::new();
        };
        if error == AgentError::NotInstalled {
            let focused = self.focused();
            self.screen = Screen::NotInstalled;
            self.sessions.remove(key);
            self.remove_tile(key);
            return self.branch_if_moved(focused).into_iter().collect();
        }
        let exited = SessionEvent::Exited {
            clean: false,
            detail: error.to_string(),
        };
        self.event(open, exited, 0.0)
    }

    /// A command to the open numbered `open` could not be sent. A message refused because its
    /// program had ended is kept to go again (see [`Session::keep_undelivered`]), also when the
    /// open has ended already: a session that rests is then started again at once. Any other
    /// failure counts only for a session's current open. A refused ask for the usage limits is
    /// the app's own business: it does not count for the idle stop. Nothing is kept for a
    /// deleted session, whatever was refused.
    pub fn send_failed(
        &mut self,
        open: u64,
        command: &SessionCommand,
        error: AgentError,
    ) -> Vec<Effect> {
        let now_unix = self.now_unix;
        let undelivered = match (command, &error) {
            (SessionCommand::Send { text, attachments }, AgentError::Closed) => Some(Undelivered {
                text: text.clone(),
                attachments: attachments.clone(),
            }),
            _ => None,
        };
        let Some(mut entry) = self.sessions.by_open(open) else {
            let Some(message) = undelivered else {
                return Vec::new();
            };
            let Some(mut entry) = self
                .sessions
                .by_past_open(open)
                .filter(|entry| entry.deleted.is_none())
            else {
                return Vec::new();
            };
            let key = entry.key;
            let commands = entry.session.keep_undelivered(message, false);
            drop(entry);
            let mut effects = self.send(key, commands);
            effects.extend(self.start_undelivered(key));
            return effects;
        };
        if entry.deleted.is_some() {
            return Vec::new();
        }
        entry.last_own_unix = Some(now_unix);
        if !matches!(command, SessionCommand::RefreshUsage) {
            entry.idle_from_unix = Some(now_unix);
        }
        if let Some(message) = undelivered {
            entry.session.keep_undelivered(message, true);
            return Vec::new();
        }
        if let SessionCommand::Send { .. } = command {
            entry.session.send_failed();
        }
        let text = match (&error, command) {
            // A title is asked for in the background: the stand-in simply stays.
            (AgentError::Closed, _) | (_, SessionCommand::SuggestTitle { .. }) => {
                return Vec::new();
            }
            (AgentError::Protocol(_), SessionCommand::Answer { .. }) => {
                "That request was no longer waiting.".to_string()
            }
            _ => error.to_string(),
        };
        entry
            .session
            .conversation
            .push_notice(NoticeKind::CommandFailed, &text);
        Vec::new()
    }

    /// Applies `event` to the session whose current open is `open`, stamped `now`. A turn that ends
    /// leaves its session ready (unseen) unless the user is looking ([`App::looking`]) and it is
    /// the focused one; one that ends in the focused session asks for the branch of its folder. A
    /// program that is gone lets go of its open; one stopped for idleness ([`App::stop_idle`]) is
    /// taken as gone cleanly whatever it says, so its session rests with no notice and no mark.
    /// When the session then rests with a message a program never got, it is started again, unless
    /// that program ended before it started; and one stopped for idleness whose session wants
    /// Remote Control by then is started again too. Its conversation stays: only the tick lets one
    /// go ([`App::let_go`]). A deleted session's program was sent `Close`
    /// ([`App::delete_session`]): its exit is a clean one too, a change of the store up to
    /// [`OWN_EXIT_SLACK_SECONDS`] later is this app's own, nothing is started, its conversation
    /// goes, and the purge rule runs at once ([`App::purge`]), so one whose week is over is
    /// removed at its exit. What answers the app's own ask for the usage limits does not count
    /// for the idle stop. A save is asked for when the name kept for the session changed: its id
    /// became known, or its program confirmed or refused it.
    pub fn event(&mut self, open: u64, event: SessionEvent, now: f64) -> Vec<Effect> {
        let (now_unix, focused, looking) = (self.now_unix, self.focused(), self.looking());
        let Some(mut entry) = self.sessions.by_open(open) else {
            return Vec::new();
        };
        // A program stopped for idleness ends as a clean one does, also when it had to be killed.
        let stopped = entry.stopping == Some(open) && matches!(event, SessionEvent::Exited { .. });
        let event = match event {
            SessionEvent::Exited { detail, .. } if stopped => SessionEvent::Exited {
                clean: true,
                detail,
            },
            event => event,
        };
        if let SessionEvent::UsageLimits(limits) = &event {
            take_usage(&mut self.usage, limits, now_unix);
        }
        let usage_answer = matches!(
            event,
            SessionEvent::UsageLimits(_)
                | SessionEvent::CommandFailed {
                    what: CommandKind::RefreshUsage,
                    ..
                }
        );
        let models = match &event {
            SessionEvent::Started(info) => Some(info.models.clone()),
            _ => None,
        };
        let started = models.is_some();
        // The id it had: a program that starts may give it another.
        let had_id = if started {
            entry.session.id.clone()
        } else {
            None
        };
        // A program that ends before it started is not started again for a message it never
        // got: one that always does so would be started over and over.
        let had_started = entry.session.phase != Phase::Opening;
        let ends_turn = matches!(
            event,
            SessionEvent::TurnFinished(_)
                | SessionEvent::TurnFailed { .. }
                | SessionEvent::Exited { clean: false, .. }
        );
        entry.last_own_unix = Some(now_unix);
        if !usage_answer {
            entry.idle_from_unix = Some(now_unix);
        }
        // Only these change the name kept for it.
        let names_event = matches!(
            event,
            SessionEvent::Started(_)
                | SessionEvent::TitleChanged(_)
                | SessionEvent::CommandFailed {
                    what: CommandKind::Rename,
                    ..
                }
        );
        let kept = names_event.then(|| entry.session.kept_name());
        let mut effects: Vec<Effect> = entry
            .session
            .apply(event, now)
            .into_iter()
            .map(|command| Effect::Send { open, command })
            .collect();
        let names_changed = kept.is_some_and(|kept| entry.session.kept_name() != kept);
        if ends_turn && entry.deleted.is_none() && (focused != Some(entry.key) || !looking) {
            entry.unseen = true;
        }
        if ends_turn && focused == Some(entry.key) {
            effects.push(Effect::ReadBranch {
                folder: entry.session.folder.clone(),
            });
        }
        let gone = entry.session.at_rest();
        if gone {
            entry.past_open = entry.open.take();
            entry.stopping = None;
        }
        let key = entry.key;
        let deleted_gone = gone && entry.deleted.is_some();
        // A deleted session's program wrote its last as it ended, and its conversation goes.
        if gone && let Some(deleted) = entry.deleted {
            entry.deleted = Some(Deleted {
                seen_unix: deleted.seen_unix.max(now_unix + OWN_EXIT_SLACK_SECONDS),
                ..deleted
            });
            if entry.session.let_go() {
                effects.push(Effect::DropHistory { key });
            }
        }
        let id = if started {
            entry.session.id.clone()
        } else {
            None
        };
        drop(entry);
        if let Some(models) = models {
            if let Some(id) = id {
                match had_id {
                    Some(had) if had != id => self.groups.follow(&had, &id),
                    _ => self.groups.adopt(key, &id),
                }
            }
            self.sessions.drop_copies_of(key);
            self.learn_models(models);
        }
        if started && self.capabilities.usage_limits {
            self.usage_asked_unix = Some(now_unix);
            effects.push(Effect::Send {
                open,
                command: SessionCommand::RefreshUsage,
            });
        }
        if names_changed {
            effects.push(self.save_state());
        }
        if had_started {
            effects.extend(self.start_undelivered(key));
        }
        if stopped {
            effects.extend(self.start_for_remote(key));
        }
        // Nothing holds its removal back now: after "Delete All" it goes at this exit.
        if deleted_gone {
            effects.extend(self.purge());
        }
        effects
    }

    /// Takes in one message from the backend and returns the effects it wants.
    /// `now` stamps a session event. The window and the live test both call this,
    /// so they treat the backend's messages the same way.
    pub fn receive(&mut self, message: FromBackend, now: f64) -> Vec<Effect> {
        match message {
            FromBackend::Account(account) => self.account_checked(account),
            FromBackend::OpenFailed { open, error } => self.open_failed(open, error),
            FromBackend::Event { open, event } => self.event(open, event, now),
            FromBackend::SendFailed {
                open,
                command,
                error,
            } => self.send_failed(open, &command, error),
            FromBackend::Sessions { since_unix, result } => self.listed(since_unix, result),
            FromBackend::History {
                key,
                result,
                pictures,
            } => {
                // Only a read still waited for is taken: a session that let its history go
                // meanwhile does not want this one.
                if let Some(mut entry) = self.sessions.get_mut(key)
                    && entry.session.history == HistoryState::Loading
                {
                    match result {
                        Ok(history) => entry.session.load_history(history, &pictures),
                        Err(error) => entry.session.history_failed(&error.to_string()),
                    }
                }
                Vec::new()
            }
            FromBackend::Activity { key, result } => self.activity_checked(key, result),
            FromBackend::Look { models, usage, .. } => {
                if let Some(limits) = usage {
                    take_usage(&mut self.usage, &limits, self.now_unix);
                }
                self.learn_models(models);
                Vec::new()
            }
            FromBackend::Branch { folder, branch } => {
                match branch {
                    Some(branch) => self.branches.insert(folder, branch),
                    None => self.branches.remove(&folder),
                };
                Vec::new()
            }
            FromBackend::Prepared { chip, result } => {
                self.prepared(chip, result);
                Vec::new()
            }
            // The window keeps the drawn picture; a picture nobody holds any more is dropped.
            FromBackend::Picture { .. } => Vec::new(),
            // The full image read again is the window's to show; the app keeps nothing of it.
            FromBackend::Image { .. } => Vec::new(),
            FromBackend::Removed { id, result } => self.removed(&id, result),
        }
    }

    /// Merges a listing since `since_unix`: a listing of every stored session is complete. The
    /// first recent listing merged asks for all of them, the tiles open when the last run ended
    /// may open again (see [`App::restore_tiles`]), and the first answer of any kind asks for a
    /// look when the app looks. A complete listing that holds no session at all forgets nothing
    /// of what was kept: no deleted mark, kept name or group place. A listed session takes the name kept for it from the last run,
    /// and its mark when it was deleted then (see [`App::take_marks`]); a save is asked for
    /// when that changed the names or the marks kept.
    fn listed(
        &mut self,
        since_unix: Option<i64>,
        result: Result<Vec<StoredSession>, AgentError>,
    ) -> Vec<Effect> {
        let complete = since_unix.is_none();
        let mut effects = Vec::new();
        match result {
            Ok(listed) => {
                let names = self.saved_names();
                let marks = self.saved_deleted();
                // A complete listing says what the store no longer has, and that is forgotten:
                // deleted marks, kept names, group places. One that holds no session at all
                // is not believed for that. A deleted session's file stays in the store for
                // its week, so with marks waiting an empty store was not really looked at: a
                // folder that exists with no `projects` in it, as an empty mount point does,
                // lists as empty. And where the store truly is empty, nothing is lost by
                // keeping what would have been forgotten until a listing holds a session.
                let whole = complete && !listed.is_empty();
                self.sessions.merge_listing(listed, whole, &mut self.names);
                if whole {
                    // The store has none of the sessions these were kept for.
                    self.names.clear();
                    self.store_seen = true;
                }
                self.take_marks(whole);
                self.listing = Listing::Done;
                if complete {
                    self.listed_all = true;
                } else if !self.all_asked {
                    self.all_asked = true;
                    effects.push(Effect::ListSessions { since_unix: None });
                }
                effects.extend(self.restore_tiles(complete));
                if self.saved_names() != names || self.saved_deleted() != marks {
                    effects.push(self.save_state());
                }
            }
            Err(error) => {
                self.listing = Listing::Failed(error.to_string());
                if complete {
                    self.all_asked = false;
                }
            }
        }
        if self.launch.look && self.launch.folder.is_none() && !self.looked {
            self.looked = true;
            effects.push(self.look());
        }
        effects
    }

    /// Puts each mark kept from the last run on its session once a listing gives it, unless a
    /// program runs for it here; after the complete listing, a mark whose session the store no
    /// longer has is dropped. A resting session no tile or start has touched, placed in a deleted
    /// group, was not known when the group was deleted: it is deleted with it, seen as listed,
    /// and its own week starts now.
    /// Then every deleted session whose stored file changed after its [`Deleted::seen_unix`] was
    /// used again elsewhere: it is recovered, unless its removal is under way, whose own answer
    /// then says that it changed ([`App::removed`]).
    fn take_marks(&mut self, complete: bool) {
        for mark in std::mem::take(&mut self.deleted_marks) {
            match self.sessions.by_id(&mark.id) {
                Some(key) => {
                    if let Some(mut entry) = self.sessions.get_mut(key)
                        && !entry.live()
                    {
                        entry.deleted = Some(Deleted {
                            at_unix: mark.at_unix,
                            seen_unix: mark.seen_unix,
                            failed_unix: None,
                            removing: false,
                        });
                    }
                }
                None if !complete => self.deleted_marks.push(mark),
                None => {}
            }
        }
        let in_deleted_groups: Vec<SessionKey> = self
            .sessions
            .iter()
            .filter(|entry| {
                entry.deleted.is_none()
                    && entry.session.phase == Phase::Resting
                    && !entry.touched
                    && self
                        .groups
                        .deleted_unix(&self.groups.group_of(entry))
                        .is_some()
            })
            .map(|entry| entry.key)
            .collect();
        let now_unix = self.now_unix;
        for key in in_deleted_groups {
            if let Some(mut entry) = self.sessions.get_mut(key) {
                entry.deleted = Some(Deleted {
                    at_unix: now_unix,
                    seen_unix: entry.last_active_unix,
                    failed_unix: None,
                    removing: false,
                });
            }
        }
        let used_again: Vec<SessionKey> = self
            .sessions
            .iter()
            .filter(|entry| {
                entry.deleted.is_some_and(|deleted| {
                    !deleted.removing && entry.last_active_unix > deleted.seen_unix
                })
            })
            .map(|entry| entry.key)
            .collect();
        for key in used_again {
            self.recover(key);
        }
    }

    /// Opens the tiles that were open when the last run ended, when there is no tile, without
    /// starting a program: once every one's session is known, the lower tiles' too, or once the
    /// complete listing has come, the known ones in their columns with their shares and splits;
    /// a column of two with one session not known or deleted shows the other alone. Each is
    /// marked as [`App::show`] marks a session, but none is attended to: one ready stays so.
    /// The focus goes to the one focused then when it is among them, else to the first column's
    /// upper tile, and the branch of its folder is asked for. Once it ran, or once anything is
    /// in a tile, it is not tried again.
    fn restore_tiles(&mut self, complete: bool) -> Vec<Effect> {
        let Some(waiting) = self.restore.take() else {
            return Vec::new();
        };
        if !self.tiles.is_empty() {
            return Vec::new();
        }
        let saved = waiting.row();
        let mut ids = saved.iter().flat_map(|tile| {
            std::iter::once(&tile.id).chain(tile.below.as_ref().map(|below| &below.id))
        });
        if !complete && ids.any(|id| self.sessions.by_id(id).is_none()) {
            self.restore = Some(waiting);
            return Vec::new();
        }
        // A deleted session is shown in no tile.
        let by_id = |id: &SessionId| {
            let key = self.sessions.by_id(id)?;
            self.sessions.get(key)?.deleted.is_none().then_some(key)
        };
        let known: Vec<Column> = saved
            .iter()
            .filter_map(|tile| {
                let under = tile.below.as_ref().and_then(|below| {
                    Some(Under {
                        key: by_id(&below.id)?,
                        split: below.split,
                    })
                });
                // Of two, one not known leaves the other alone.
                let (top, under) = match (by_id(&tile.id), under) {
                    (Some(top), under) => (top, under),
                    (None, Some(under)) => (under.key, None),
                    (None, None) => return None,
                };
                Some(Column {
                    share: tile.share,
                    top,
                    under,
                })
            })
            .collect();
        if known.is_empty() {
            return Vec::new();
        }
        let focused = waiting.shown.and_then(|id| by_id(&id));
        self.tiles.restore(known, focused);
        self.restored = self.focused();
        let mut effects: Vec<Effect> = self
            .tiles
            .keys()
            .into_iter()
            .filter_map(|key| self.mark_shown(key))
            .collect();
        effects.extend(self.read_branch());
        effects
    }

    /// The store's answer to whether `key`'s session may be open elsewhere. Changed within
    /// [`ELSEWHERE_SECONDS`] of now, and not by this app's last run (no later than it closed,
    /// with [`OWN_EXIT_SLACK_SECONDS`] for the program's own exit), the session rests again
    /// with a question for the user about the message, its [`Entry::prompt`]; otherwise, and
    /// when the store could not say, its program opens.
    fn activity_checked(
        &mut self,
        key: SessionKey,
        result: Result<Option<i64>, AgentError>,
    ) -> Vec<Effect> {
        let now_unix = self.now_unix;
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        entry.checking = false;
        if entry.session.phase != Phase::Opening || entry.open.is_some() {
            return Vec::new();
        }
        let own = |unix: i64| {
            self.closed_by_app.iter().any(|(id, closed)| {
                entry.session.id.as_ref() == Some(id) && unix <= closed + OWN_EXIT_SLACK_SECONDS
            })
        };
        if matches!(result, Ok(Some(unix)) if now_unix - unix <= ELSEWHERE_SECONDS && !own(unix)) {
            if let Some(text) = entry.session.unwake() {
                entry.prompt = Some(Prompt::OpenElsewhere { text });
            }
            return Vec::new();
        }
        entry.last_active_unix = now_unix;
        drop(entry);
        self.start(key)
    }

    /// Shows a new session in `folder` at `place` and starts its program, and puts `folder`
    /// first among the recent folders. The session wants Remote Control when `remote_for_new`
    /// says so. Asks once to save, after the start, when that changed them or [`App::show`]
    /// asked to. A session taken out of its tile leaves as `leaving` says (see [`App::show`]).
    /// When the focused session is an empty one in that folder already, and its program has
    /// not ended, nothing happens.
    pub fn new_session(&mut self, folder: PathBuf, place: Place, leaving: Leaving) -> Vec<Effect> {
        if self.screen != Screen::Main {
            return Vec::new();
        }
        if self.focused_entry().is_some_and(|entry| {
            empty(&entry.session) && entry.live() && entry.session.folder == folder
        }) {
            return Vec::new();
        }
        let remembered = remember_folder(&mut self.recent_folders, &folder);
        let mut session = Session::opening(folder);
        session.remote_wanted = self.remote_for_new;
        let key = self.sessions.add(session, self.now_unix);
        let mut effects = self.show(key, place, leaving);
        let asked = drop_saves(&mut effects);
        effects.extend(self.start(key));
        if remembered || asked {
            effects.push(self.save_state());
        }
        effects
    }

    /// Folders to offer for a new session: the recent ones, then the folders of the newest
    /// sessions, each once, at most `RECENT_FOLDERS`. Whether each is still there is for the
    /// caller to check.
    pub fn folder_choices(&self) -> Vec<PathBuf> {
        let mut newest: Vec<&Entry> = self.sessions.iter().collect();
        newest.sort_by_key(|entry| std::cmp::Reverse(entry.last_active_unix));
        let from_sessions = newest.into_iter().map(|entry| &entry.session.folder);
        let mut choices: Vec<PathBuf> = Vec::new();
        for folder in self.recent_folders.iter().chain(from_sessions) {
            if choices.len() == RECENT_FOLDERS {
                break;
            }
            if !choices.contains(folder) {
                choices.push(folder.clone());
            }
        }
        choices
    }

    /// The user's home folder, as the app was given it.
    pub fn home(&self) -> &Path {
        &self.launch.home
    }

    /// Shows `key`'s session at `place` (see [`Tiles`]): a session already in a tile only has its
    /// tile focused. The user acted on the session ([`App::attend`]), also when it is the focused
    /// one already; it is offered the models known, and a stored history not read yet is asked for,
    /// then the branch of its folder. The session taken out of its tile, if any, is closed and
    /// forgotten when it is empty, unless `leaving` says to keep it; one kept keeps its
    /// conversation for now (see [`App::let_go`]). Asks to save when a tile opened beside the
    /// focused one, at a place, or over or under a tile. A session shown in the focused tile, the
    /// first tile included, is saved with the next change that asks. A deleted session is never
    /// shown: nothing happens.
    pub fn show(&mut self, key: SessionKey, place: Place, leaving: Leaving) -> Vec<Effect> {
        if self.focused() == Some(key) {
            return self.attend(key);
        }
        if self
            .sessions
            .get(key)
            .is_none_or(|entry| entry.deleted.is_some())
        {
            return Vec::new();
        }
        let put = self.tiles.put(key, place, self.row_size());
        if put == Put::Nothing {
            return Vec::new();
        }
        // Something is in a tile: the last run's tiles no longer open.
        self.restore = None;
        let read = self.mark_shown(key);
        let mut effects = self.attend(key);
        effects.extend(match (put, leaving) {
            (Put::Replaced(left), Leaving::ForgetIfEmpty) => self.forget_if_empty(left),
            _ => Vec::new(),
        });
        if let Put::Replaced(left) = put {
            self.left_tiles(left);
        }
        effects.extend(read);
        effects.extend(self.read_branch());
        if put == Put::Added && place != Place::Focused {
            effects.push(self.save_state());
        }
        effects
    }

    /// Moves the focus to `key`'s tile and asks for the branch of its folder; the user acted on
    /// its session ([`App::attend`]), also when its tile has the focus already. The focus stays
    /// when it is in no tile. Asks for no save.
    pub fn focus_tile(&mut self, key: SessionKey) -> Vec<Effect> {
        self.focus(key)
    }

    /// Moves the focus back to `key`'s tile, as [`App::focus_tile`] does, without counting as
    /// the user's act: its session stays ready if it is. For the window, which keeps the focus
    /// on the tile whose text field holds the keyboard after an intent moved it elsewhere.
    pub fn return_focus(&mut self, key: SessionKey) -> Vec<Effect> {
        if self.tiles.focus(key) {
            self.read_branch().into_iter().collect()
        } else {
            Vec::new()
        }
    }

    /// Takes `key`'s tile out of the row; its session leaves as `leaving` says, and one kept keeps
    /// its conversation for now (see [`App::let_go`]). When that tile had the focus, the focus
    /// goes to the other tile of its column when it has two; else to the upper tile of the
    /// column on its right, else of the one on its left, and the branch of its session's folder
    /// is asked for. Asks to save.
    pub fn close_tile(&mut self, key: SessionKey, leaving: Leaving) -> Vec<Effect> {
        let focused = self.focused();
        if !self.remove_tile(key) {
            return Vec::new();
        }
        let mut effects = match leaving {
            Leaving::ForgetIfEmpty => self.forget_if_empty(key),
            Leaving::Keep => Vec::new(),
        };
        effects.extend(self.branch_if_moved(focused));
        effects.push(self.save_state());
        effects
    }

    /// Moves `key`'s tile to `spot` (see `Tiles::move_to`); it takes the focus and the user
    /// acted on its session ([`App::attend`]). Asks for the branch of its folder when the focus
    /// came to it, and to save. Nothing, and no effect, when nothing moved.
    pub fn move_tile(&mut self, key: SessionKey, spot: Spot) -> Vec<Effect> {
        let focused = self.focused();
        if !self.tiles.move_to(key, spot, self.row_size()) {
            return Vec::new();
        }
        let mut effects = self.attend(key);
        effects.extend(self.branch_if_moved(focused));
        effects.push(self.save_state());
        effects
    }

    /// Makes `left`'s column `width` points wide against the column on its right (see
    /// `Tiles::resize`). Nothing happens while the row's width is not known.
    /// Asks for no save: the view asks once, when the drag ends.
    pub fn resize_tile(&mut self, left: SessionKey, width: f32) {
        if let Some(row_width) = self.row.width {
            self.tiles.resize(left, width, row_width);
        }
    }

    /// Gives `left`'s column and the one on its right `shares`, as [`App::tile_pair`] read them
    /// before a drag of the divider between them: a cancelled drag puts them back exactly,
    /// whatever the least width. Asks for no save.
    pub fn put_tile_pair(&mut self, left: SessionKey, shares: [f32; 2]) {
        self.tiles.set_pair(left, shares);
    }

    /// The shares of `left`'s column and of the one on its right; none for the last column.
    pub fn tile_pair(&self, left: SessionKey) -> Option<[f32; 2]> {
        let columns = self.tiles.columns();
        let (index, _) = self.tiles.find(left)?;
        let right = columns.get(index + 1)?;
        Some([columns[index].share, right.share])
    }

    /// Whether a session that is in no tile could be shown at `spot` now (`Tiles::takes`).
    pub fn can_put(&self, spot: Spot) -> bool {
        self.tiles.takes(spot, self.row)
    }

    /// Whether `key`'s tile could be moved to `spot` now (`Tiles::would_move`).
    pub fn can_move_tile(&self, key: SessionKey, spot: Spot) -> bool {
        self.tiles.would_move(key, spot, self.row)
    }

    /// Makes `top`'s tile `height` points high against the tile under it (see
    /// `Tiles::resize_split`). Nothing happens while the row's height is not known. Asks for no
    /// save: the view asks once, when the drag ends.
    pub fn resize_split(&mut self, top: SessionKey, height: f32) {
        if let Some(row_height) = self.row.height {
            self.tiles.resize_split(top, height, row_height);
        }
    }

    /// Puts a split back as `tile_split` read it: a cancelled drag puts it back exactly,
    /// whatever the least height. Asks for no save.
    pub fn put_split(&mut self, top: SessionKey, split: f32) {
        self.tiles.set_split(top, split);
    }

    /// The upper tile's part of its column's height, when `top` is the upper of two.
    pub fn tile_split(&self, top: SessionKey) -> Option<f32> {
        self.tiles.split(top)
    }

    /// Gives the two tiles of `top`'s column the same height; asks for a save when that
    /// changed anything.
    pub fn even_split(&mut self, top: SessionKey) -> Vec<Effect> {
        if self.tiles.set_split(top, 0.5) {
            vec![self.save_state()]
        } else {
            Vec::new()
        }
    }

    /// Every tile gets the same width; asks for a save when that changed anything.
    pub fn even_tiles(&mut self) -> Vec<Effect> {
        if self.tiles.even() {
            vec![self.save_state()]
        } else {
            Vec::new()
        }
    }

    /// Asks for the state to be written.
    pub fn save_now(&self) -> Vec<Effect> {
        vec![self.save_state()]
    }

    /// The app is going away, at `now_unix` by the wall clock: its state saved first, then
    /// `Close` for every session whose program has not ended and is not being stopped already
    /// (it was sent one). The state keeps, for every
    /// session whose program this run ran, that this app closed it then, and for a deleted
    /// session whose program has not ended, that a change up to [`OWN_EXIT_SLACK_SECONDS`]
    /// from now is this app's own.
    pub fn close(&mut self, now_unix: i64) -> Vec<Effect> {
        self.now_unix = now_unix;
        self.closing = true;
        // A deleted session's program still running may write as it exits.
        for mut entry in self.sessions.iter_mut() {
            if entry.live()
                && let Some(deleted) = entry.deleted
            {
                entry.deleted = Some(Deleted {
                    seen_unix: deleted.seen_unix.max(now_unix + OWN_EXIT_SLACK_SECONDS),
                    ..deleted
                });
            }
        }
        let mut saved = self.saved();
        let ran = self
            .sessions
            .iter()
            .filter(|entry| entry.last_own_unix.is_some())
            .filter_map(|entry| entry.session.id.clone());
        for id in ran {
            saved.closed_by_app.retain(|(known, _)| *known != id);
            saved.closed_by_app.push((id, self.now_unix));
        }
        let mut effects = vec![Effect::SaveState(Box::new(saved))];
        effects.extend(
            self.sessions
                .iter()
                .filter(|entry| entry.live() && entry.stopping.is_none())
                .filter_map(|entry| entry.open)
                .map(|open| Effect::Send {
                    open,
                    command: SessionCommand::Close,
                }),
        );
        effects
    }

    /// What is kept between runs, as of now. The widths of the sidebar and the right panel and
    /// whether each shows are the window's: they are left at their defaults.
    ///
    /// Once a complete listing that held a session has been merged, a placement whose session
    /// the app does not know is left out. Every tile whose session the store has is kept, column by column, one
    /// of two whose mate is not kept as a column of one; and the focused session by its id, or
    /// the first kept column's upper tile when the focused one is not kept;
    /// while the last run's tiles may still open again, they are kept as they were read. Of
    /// the sessions the last run closed, only those it closed recently enough that a change it
    /// made could still look new to the activity check are kept ([`App::close`] adds this
    /// run's). The names no program has confirmed are always
    /// those the app holds now, and so are the marks of deleted sessions.
    pub fn saved(&self) -> Saved {
        let recent = ELSEWHERE_SECONDS + OWN_EXIT_SLACK_SECONDS;
        let (tiles, shown) = match &self.restore {
            Some(waiting) => (waiting.tiles.clone(), waiting.shown.clone()),
            _ => {
                // What the store does not have could not come back.
                let stored = |key: SessionKey| {
                    let session = &self.sessions.get(key)?.session;
                    session.id.clone().filter(|_| session.is_stored())
                };
                let tiles: Vec<SavedTile> = self
                    .tiles
                    .columns()
                    .iter()
                    .filter_map(|column| {
                        let below = column.under.and_then(|under| {
                            Some(SavedBelow {
                                id: stored(under.key)?,
                                split: under.split,
                            })
                        });
                        // Of two, one the store does not have leaves the other alone.
                        let (id, below) = match (stored(column.top), below) {
                            (Some(id), below) => (id, below),
                            (None, Some(below)) => (below.id, None),
                            (None, None) => return None,
                        };
                        Some(SavedTile {
                            id,
                            share: column.share,
                            below,
                        })
                    })
                    .collect();
                let shown = self
                    .focused()
                    .and_then(stored)
                    .or_else(|| tiles.first().map(|tile| tile.id.clone()));
                (tiles, shown)
            }
        };
        let mut saved = Saved {
            version: SAVED_VERSION,
            recent_folders: self.recent_folders.clone(),
            shown,
            tiles,
            closed_by_app: self
                .closed_by_app
                .iter()
                .filter(|(_, closed)| self.now_unix - closed <= recent)
                .cloned()
                .collect(),
            names: self.saved_names(),
            remote_for_new: self.remote_for_new,
            deleted: self.saved_deleted(),
            ..Saved::default()
        };
        self.groups.save_into(&mut saved);
        if self.store_seen {
            let known: HashSet<&SessionId> = self
                .sessions
                .iter()
                .filter_map(|entry| entry.session.id.as_ref())
                .collect();
            saved.placed.retain(|(id, _)| known.contains(id));
        }
        saved
    }

    /// The names to keep between runs, by id: every session's that no program has confirmed,
    /// and those kept from the last run for sessions not known yet.
    fn saved_names(&self) -> Vec<(SessionId, String)> {
        let mut names: Vec<(SessionId, String)> = self
            .sessions
            .iter()
            .filter_map(|entry| entry.session.kept_name())
            .collect();
        // A session the app knows holds its latest name itself.
        let known: HashSet<&SessionId> = self
            .sessions
            .iter()
            .filter_map(|entry| entry.session.id.as_ref())
            .collect();
        names.extend(
            self.names
                .iter()
                .filter(|(id, _)| !known.contains(id))
                .cloned(),
        );
        names.sort_by(|(a, _), (b, _)| a.0.cmp(&b.0));
        names
    }

    /// The marks to keep between runs, by id: every deleted session's that has an id, and those
    /// kept from the last run for sessions not listed yet.
    fn saved_deleted(&self) -> Vec<SavedDeleted> {
        let mut marks: Vec<SavedDeleted> = self
            .sessions
            .iter()
            .filter_map(|entry| {
                let deleted = entry.deleted?;
                Some(SavedDeleted {
                    id: entry.session.id.clone()?,
                    at_unix: deleted.at_unix,
                    seen_unix: deleted.seen_unix,
                })
            })
            .collect();
        marks.extend(self.deleted_marks.iter().cloned());
        marks.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        marks
    }

    /// What a quit would cut off now. A session is working while a turn runs, while a message
    /// waits for a program (queued, undelivered, or held by the question whether to continue a
    /// session open elsewhere), whatever its phase, and while its program runs a background
    /// task. A deleted session counts for nothing.
    pub fn busy(&self) -> Busy {
        let mut busy = Busy::default();
        for entry in self.sessions.iter().filter(|entry| entry.deleted.is_none()) {
            let session = &entry.session;
            let working = matches!(session.phase, Phase::Working | Phase::NeedsUser)
                || session.queued.is_some()
                || !session.undelivered.is_empty()
                || matches!(entry.prompt, Some(Prompt::OpenElsewhere { .. }))
                || session.background > 0;
            busy.working += usize::from(working);
            busy.remote += usize::from(session.remote != Remote::Off);
        }
        busy
    }

    /// The sessions by their mark (`Entry::mark`). A deleted session counts for nothing.
    pub fn standing(&self) -> Standing {
        let mut standing = Standing::default();
        for entry in self.sessions.iter().filter(|entry| entry.deleted.is_none()) {
            match entry.mark() {
                Mark::Working => standing.working += 1,
                Mark::NeedsYou => standing.needs_you += 1,
                Mark::Finished => standing.finished += 1,
                Mark::None => {}
            }
        }
        standing
    }

    /// No session has a program still running.
    pub fn all_ended(&self) -> bool {
        !self.sessions.iter().any(Entry::live)
    }

    /// Starts an ended session's program again: a message its program never got goes to the new
    /// one when it has started. A resting one is woken by a message instead. This and the
    /// session intents below are the user's acts on the session ([`App::attend`]): they first
    /// focus its tile, when it is in one without it, and then ask for the branch of its folder.
    pub fn restart(&mut self, key: SessionKey) -> Vec<Effect> {
        let mut effects = self.focus(key);
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return effects;
        };
        if !matches!(entry.session.phase, Phase::Ended { .. }) {
            return effects;
        }
        entry.session.reopening();
        drop(entry);
        effects.extend(self.start(key));
        effects
    }

    /// Sends `text` to `key`'s session, or queues it there, and makes the session active now.
    /// A resting session wakes with it queued: when this app has not run its program in this
    /// run, the store is first asked whether another app may have it open. The session's chips
    /// go with the text. Nothing is taken unless [`App::can_send`] holds.
    pub fn submit(&mut self, key: SessionKey, text: &str) -> Vec<Effect> {
        let mut effects = self.focus(key);
        if !self.can_send(key, text) {
            return effects;
        }
        let now_unix = self.now_unix;
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return effects;
        };
        if entry.session.phase == Phase::Resting {
            entry.session.wake(text);
            entry.touched = true;
            if entry.last_own_unix.is_none()
                && let Some(id) = entry.session.id.clone()
            {
                entry.checking = true;
                effects.push(Effect::CheckActivity { key, id });
                return effects;
            }
            entry.last_active_unix = now_unix;
            drop(entry);
            effects.extend(self.start(key));
            return effects;
        }
        entry.last_active_unix = now_unix;
        let commands = entry.session.submit(text);
        drop(entry);
        effects.extend(self.send(key, commands));
        effects
    }

    /// Whether `key`'s session takes a message now. An ended one does not, until it is
    /// restarted. Nor does one with a question waiting or whose check in the store runs: the
    /// message then stays where it was typed. Other sessions are not held up by either. A
    /// deleted session takes nothing.
    pub fn takes_message(&self, key: SessionKey) -> bool {
        self.sessions.get(key).is_some_and(|entry| {
            entry.deleted.is_none()
                && entry.session.takes_messages()
                && entry.prompt.is_none()
                && !entry.checking
        })
    }

    /// Adds a chip for each source to `key`'s session and asks for each to be read. A session
    /// that does not take a message now takes no chip, and a path it already has a chip for, in
    /// its input box or in its queued message, is left out. When a chip was added the user
    /// acted on the session: it is no longer ready ([`App::attend`]).
    pub fn attach(&mut self, key: SessionKey, sources: Vec<Source>) -> Vec<Effect> {
        if !self.takes_message(key) {
            return Vec::new();
        }
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        for source in sources {
            let session = &entry.session;
            let attached = matches!(source, Source::Path(_))
                && session
                    .chips
                    .iter()
                    .chain(session.queued.iter().flat_map(|queued| &queued.chips))
                    .any(|chip| chip.source == source);
            if attached {
                continue;
            }
            self.chip += 1;
            entry.session.chips.push(Chip {
                id: self.chip,
                name: name_of(&source),
                source: source.clone(),
                state: ChipState::Reading,
            });
            effects.push(Effect::Prepare {
                chip: self.chip,
                source,
            });
        }
        // The user acted on the session only when it took something.
        if !effects.is_empty() {
            entry.unseen = false;
        }
        effects
    }

    /// Takes the queued message of `key`'s session back for its input box: its text, and its
    /// chips back on the session ([`Session::take_queued`]). The user acted on the session
    /// ([`App::attend`]) when there was one. None for a session with nothing queued.
    pub fn edit_queued(&mut self, key: SessionKey) -> (Vec<Effect>, Option<String>) {
        let taken = self
            .sessions
            .get_mut(key)
            .and_then(|mut entry| entry.session.take_queued());
        let effects = match taken {
            Some(_) => self.attend(key),
            None => Vec::new(),
        };
        (effects, taken)
    }

    /// Takes chip `chip` off `key`'s session.
    pub fn detach(&mut self, key: SessionKey, chip: u64) {
        if let Some(mut entry) = self.sessions.get_mut(key) {
            entry.session.chips.retain(|held| held.id != chip);
        }
    }

    /// Whether Enter would send `text` with `key`'s chips now: the session takes a message and
    /// [`Session::can_send`] holds.
    pub fn can_send(&self, key: SessionKey, text: &str) -> bool {
        self.takes_message(key)
            && self
                .sessions
                .get(key)
                .is_some_and(|entry| entry.session.can_send(text))
    }

    /// Fills the chip with id `chip`, in whatever session it is, with what reading it gave. A
    /// chip taken off is gone: its answer is dropped. A pasted chip lets go of the clipboard's
    /// bytes: what is sent is the prepared image's.
    fn prepared(&mut self, chip: u64, result: Result<Prepared, Problem>) {
        let Some(key) = self
            .sessions
            .iter()
            .find(|entry| entry.session.chips.iter().any(|held| held.id == chip))
            .map(|entry| entry.key)
        else {
            return;
        };
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return;
        };
        if let Some(held) = entry.session.chips.iter_mut().find(|held| held.id == chip) {
            if let Source::Pasted(data) = &mut held.source {
                *data = ImageData::new(Vec::new());
            }
            held.state = match result {
                Ok(prepared) => ChipState::Ready(prepared),
                Err(problem) => ChipState::Problem(problem),
            };
        }
    }

    /// The question waiting for the user about `key`'s session.
    pub fn prompt(&self, key: SessionKey) -> Option<&Prompt> {
        self.sessions.get(key)?.prompt.as_ref()
    }

    pub fn stop(&mut self, key: SessionKey) -> Vec<Effect> {
        let mut effects = self.focus(key);
        let command = self
            .sessions
            .get(key)
            .and_then(|entry| entry.session.stop());
        effects.extend(self.send(key, command));
        effects
    }

    pub fn answer(&mut self, key: SessionKey, id: &RequestId, answer: Answer) -> Vec<Effect> {
        let mut effects = self.focus(key);
        let command = self
            .sessions
            .get_mut(key)
            .and_then(|mut entry| entry.session.answer(id, answer));
        effects.extend(self.send(key, command));
        effects
    }

    pub fn choose_model(&mut self, key: SessionKey, option_id: &str) -> Vec<Effect> {
        let mut effects = self.focus(key);
        let command = self
            .sessions
            .get_mut(key)
            .and_then(|mut entry| entry.session.choose_model(option_id));
        effects.extend(self.send(key, command));
        effects
    }

    /// Asks `key`'s session for the effort `level`. The focus does not move.
    pub fn choose_effort(&mut self, key: SessionKey, level: &str) -> Vec<Effect> {
        let command = self
            .sessions
            .get_mut(key)
            .and_then(|mut entry| entry.session.choose_effort(level));
        self.send(key, command)
    }

    /// Turns Remote Control on or off for `key`'s session (see [`Session::set_remote`]). Turned
    /// on for a resting session whose program this app ran in this run, as one stopped for
    /// idleness, the session is started for it (see [`App::start_for_remote`]); any other
    /// resting session waits for its next start. The focus does not move. Nothing for a deleted
    /// session.
    pub fn set_remote(&mut self, key: SessionKey, on: bool) -> Vec<Effect> {
        if self.is_deleted(key) {
            return Vec::new();
        }
        let command = self
            .sessions
            .get_mut(key)
            .and_then(|mut entry| entry.session.set_remote(on));
        let mut effects = self.send(key, command);
        effects.extend(self.start_for_remote(key));
        effects
    }

    /// Sets the preference and asks to save. Nothing when it is already so.
    pub fn set_remote_for_new(&mut self, on: bool) -> Vec<Effect> {
        if self.remote_for_new == on {
            return Vec::new();
        }
        self.remote_for_new = on;
        vec![self.save_state()]
    }

    pub fn choose_mode(&mut self, key: SessionKey, mode: PermissionMode) -> Vec<Effect> {
        let mut effects = self.focus(key);
        let command = self
            .sessions
            .get_mut(key)
            .and_then(|mut entry| entry.session.choose_mode(mode));
        effects.extend(self.send(key, command));
        effects
    }

    /// Names `key`'s session `name`, trimmed. Nothing for an empty name or the name it has.
    ///
    /// Its program is told when it runs and has started, else the next one is when it starts.
    /// Once the session has an id the name is kept between runs until a program confirms it,
    /// and a save is asked for. The focus does not move. Nothing for a deleted session.
    pub fn rename(&mut self, key: SessionKey, name: &str) -> Vec<Effect> {
        if self.is_deleted(key) {
            return Vec::new();
        }
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        let kept = entry.session.kept_name();
        let command = entry.session.rename(name);
        let names_changed = entry.session.kept_name() != kept;
        drop(entry);
        let mut effects = self.send(key, command);
        if names_changed {
            effects.push(self.save_state());
        }
        effects
    }

    /// Brings the clock up to date and returns what is due: a new listing, a usage refresh, the
    /// history of each session long enough in no tile that may let it go (see [`App::let_go`]:
    /// nothing else lets one go), the `Close` of each program idle too long (see
    /// [`App::stop_idle`]), and once a minute the removal of what was deleted a week ago (see
    /// [`App::purge`], which also runs when a removal answers and when a deleted session's
    /// program has gone).
    pub fn tick(&mut self, now_unix: i64) -> Vec<Effect> {
        self.now_unix = now_unix;
        if self.screen != Screen::Main {
            return Vec::new();
        }
        let held: Vec<SessionKey> = self
            .sessions
            .iter()
            .filter(|entry| {
                matches!(
                    entry.session.history,
                    HistoryState::Loaded | HistoryState::Loading
                ) && !self.tiles.contains(entry.key)
            })
            .map(|entry| entry.key)
            .collect();
        let mut effects: Vec<Effect> = held
            .into_iter()
            .filter_map(|key| self.let_go(key))
            .collect();
        if self
            .listed_unix
            .is_some_and(|asked| now_unix - asked >= RELIST_SECONDS)
        {
            effects.push(self.list_recent());
        }
        if self
            .purged_unix
            .is_none_or(|purged| now_unix - purged >= PURGE_SECONDS)
        {
            self.purged_unix = Some(now_unix);
            effects.extend(self.purge());
        }
        effects.extend(self.usage_after_reset());
        effects.extend(self.stop_idle());
        effects
    }

    /// Asks to remove for good each deleted session whose week is over, whose program has
    /// ended, whose removal is not under way and did not fail in the last
    /// [`PURGE_RETRY_SECONDS`]: unless its stored file changed after [`Deleted::seen_unix`]. A
    /// deleted group the user made whose week is over loses its record once no session is in
    /// it any more, after a complete listing that held a session (before it, one not listed
    /// yet may be), and a save is asked for.
    fn purge(&mut self) -> Vec<Effect> {
        let now_unix = self.now_unix;
        let over = |at_unix: i64| at_unix.saturating_add(DELETED_SECONDS) <= now_unix;
        let due: Vec<SessionKey> = self
            .sessions
            .iter()
            .filter(|entry| {
                entry.open.is_none()
                    && !entry.live()
                    && entry.deleted.is_some_and(|deleted| {
                        over(deleted.at_unix)
                            && !deleted.removing
                            && deleted
                                .failed_unix
                                .is_none_or(|failed| now_unix - failed >= PURGE_RETRY_SECONDS)
                    })
            })
            .map(|entry| entry.key)
            .collect();
        let mut effects = Vec::new();
        for key in due {
            let Some(mut entry) = self.sessions.get_mut(key) else {
                continue;
            };
            let (Some(deleted), Some(id)) = (entry.deleted, entry.session.id.clone()) else {
                continue;
            };
            entry.deleted = Some(Deleted {
                removing: true,
                ..deleted
            });
            effects.push(Effect::RemoveStored {
                id,
                unless_after_unix: deleted.seen_unix,
            });
        }
        let emptied: Vec<GroupId> = self
            .groups
            .deleted()
            .into_iter()
            .filter(|group| {
                self.store_seen
                    && self.groups.deleted_unix(group).is_some_and(over)
                    && !self
                        .sessions
                        .iter()
                        .any(|entry| self.groups.group_of(entry) == *group)
            })
            .collect();
        if !emptied.is_empty() {
            for group in &emptied {
                self.groups.ungroup(group);
            }
            effects.push(self.save_state());
        }
        effects
    }

    /// Takes the answer to the removal of the stored session `id`. Removed, or not in the store
    /// any more, it is forgotten with its mark, its placement and its kept name, and a save is
    /// asked for. Changed since it was deleted, it was used again elsewhere: it is recovered.
    /// A removal that failed leaves it deleted, to be tried again ([`PURGE_RETRY_SECONDS`]).
    /// An answer for a session whose removal is not under way, or that is no longer known, is
    /// dropped. Once a session is forgotten the purge rule runs at once ([`App::purge`]), so a
    /// deleted group it leaves empty goes with it and not a minute later.
    fn removed(&mut self, id: &SessionId, result: Result<Removal, AgentError>) -> Vec<Effect> {
        let now_unix = self.now_unix;
        let Some(key) = self.sessions.by_id(id) else {
            return Vec::new();
        };
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        // Only an answer that is waited for is taken. The user can neither recover nor delete
        // again a session being removed, so this drops nothing today. It stands for a session
        // recovered meanwhile, and for one recovered and deleted again, whose answer is that
        // of the removal before: either may show although its file is gone, since no recent
        // listing takes it away; the next run's complete listing does (see
        // `Sessions::merge_listing`).
        let Some(deleted) = entry.deleted.filter(|deleted| deleted.removing) else {
            return Vec::new();
        };
        match result {
            Ok(Removal::Removed | Removal::Missing) => {
                drop(entry);
                self.sessions.remove(key);
                self.groups.forget(key, id);
                self.names.retain(|(named, _)| named != id);
                self.deleted_marks.retain(|mark| mark.id != *id);
                let mut effects = self.purge();
                drop_saves(&mut effects);
                effects.push(self.save_state());
                effects
            }
            Ok(Removal::Changed) => {
                drop(entry);
                self.recover(key);
                vec![self.save_state()]
            }
            Err(_) => {
                entry.deleted = Some(Deleted {
                    failed_unix: Some(now_unix),
                    removing: false,
                    ..deleted
                });
                Vec::new()
            }
        }
    }

    /// Stops, once, each program that has done nothing for the idle length when the user would
    /// not notice: its session is [`Session::stoppable`], no question waits about it and the
    /// store is not being asked about it. Nothing its program said and no message the user sent
    /// came in that time; an answer to the app's own ask for the usage limits is not counted
    /// (`Entry::idle_from_unix`). Its exit then leaves it resting (see [`App::event`]).
    fn stop_idle(&mut self) -> Vec<Effect> {
        let (now_unix, idle_after) = (self.now_unix, self.idle_after);
        let idle: Vec<(SessionKey, u64)> = self
            .sessions
            .iter()
            .filter(|entry| {
                entry.stopping.is_none()
                    && entry.prompt.is_none()
                    && !entry.checking
                    && entry.session.stoppable()
                    && now_unix
                        - entry
                            .idle_from_unix
                            .unwrap_or(0)
                            .max(entry.last_active_unix)
                        >= idle_after
            })
            .filter_map(|entry| Some((entry.key, entry.open?)))
            .collect();
        idle.into_iter()
            .filter_map(|(key, open)| {
                self.sessions.get_mut(key)?.stopping = Some(open);
                Some(Effect::Send {
                    open,
                    command: SessionCommand::Close,
                })
            })
            .collect()
    }

    /// Answers the question waiting about `key`'s session and clears it. `ContinueHere` wakes
    /// the session with the message and opens its program; `ContinueAsCopy` puts a copy of it
    /// beside it, shows the copy in the original's tile (in the focused one when the original
    /// is in no tile) and opens a program that forks the stored conversation, the message
    /// queued in it, leaving the original resting. The message's chips are on the session while
    /// the question waits: they go with the message, to the copy when it is one, and stay on
    /// the session on Cancel. The text that goes back into that session's input box, when the
    /// choice was Cancel or the text and chips left make no message to send.
    pub fn answer_prompt(
        &mut self,
        key: SessionKey,
        choice: PromptChoice,
    ) -> (Vec<Effect>, Option<String>) {
        let mut effects = self.focus(key);
        let now_unix = self.now_unix;
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return (effects, None);
        };
        let Some(Prompt::OpenElsewhere { text }) = entry.prompt.take() else {
            return (effects, None);
        };
        if entry.session.phase != Phase::Resting {
            return (effects, None);
        }
        // Its chips may have been taken off meanwhile, leaving nothing to send.
        if !entry.session.can_send(&text) {
            return (effects, Some(text));
        }
        match choice {
            PromptChoice::Cancel => (effects, Some(text)),
            PromptChoice::ContinueHere => {
                entry.session.wake(&text);
                entry.last_active_unix = now_unix;
                drop(entry);
                effects.extend(self.start(key));
                (effects, None)
            }
            PromptChoice::ContinueAsCopy => {
                let group = self.groups.group_of(&entry);
                let mut spec = entry.session.start_spec();
                spec.fork = true;
                // The chips go with the message: none is on two sessions.
                let chips = std::mem::take(&mut entry.session.chips);
                let mut copy = entry.session.clone();
                copy.chips = chips;
                drop(entry);
                // Its own id comes from its program; until then the stored one is only read.
                let read_from = copy.id.take();
                copy.wake(&text);
                let copy = self.sessions.add(copy, now_unix);
                if let Some(entry) = self.sessions.get(copy) {
                    self.groups.place(entry, &group);
                }
                // In the original's tile, else in the focused one. What it leaves is the
                // original or a session the window may have text typed in: nothing to forget.
                let place = if self.tiles.contains(key) {
                    Place::Tile(key)
                } else {
                    Place::Focused
                };
                effects.extend(self.show(copy, place, Leaving::Keep));
                if let Some(mut entry) = self.sessions.get_mut(copy)
                    && let Some(id) = read_from
                    && matches!(
                        entry.session.history,
                        HistoryState::NotLoaded | HistoryState::Loading
                    )
                {
                    entry.session.history = HistoryState::Loading;
                    entry.session.read_from = Some(id.clone());
                    effects.push(Effect::ReadHistory {
                        key: copy,
                        id,
                        max_items: HISTORY_ITEMS,
                    });
                }
                effects.extend(self.start_with(copy, spec));
                (effects, None)
            }
        }
    }

    /// Takes out every tile whose session is gone, then rebuilds `sidebar` when sessions, groups
    /// or the tiles' placement (not their widths) changed since the last call. So do the end of
    /// the first listing, the complete listing coming in, a change of the view, a clock that
    /// moved an entry across the [`RECENT_DAYS`] line, and in the Deleted view one that took a
    /// day off a row's days left.
    /// Each rebuild puts `sidebar_version` up by one. Gives the notifications due and those to
    /// take back ([`App::alerts`]): a notification dropped here is never given again.
    #[must_use]
    pub fn settle(&mut self) -> Vec<Effect> {
        let gone: Vec<SessionKey> = self
            .tiles
            .keys()
            .into_iter()
            .filter(|key| self.sessions.get(*key).is_none())
            .collect();
        for key in gone {
            self.remove_tile(key);
        }
        let alerts = self.alerts();
        let loading = self.listing == Listing::Loading;
        if self.settled.is_some_and(|settled| {
            settled.sessions == self.sessions.version()
                && settled.groups == self.groups.version()
                && settled.placement == self.tiles.placement()
                && settled.loading == loading
                && settled.complete == self.listed_all
                && settled.view == self.sidebar_view
                && settled.recency.holds_at(self.now_unix)
        }) {
            return alerts;
        }
        self.sidebar = sidebar_model(
            &self.sessions,
            &self.groups,
            &self.tiles,
            self.now_unix,
            loading,
            self.sidebar_view,
        );
        self.sidebar.complete = self.listed_all;
        self.sidebar_version += 1;
        self.settled = Some(Settled {
            sessions: self.sessions.version(),
            groups: self.groups.version(),
            placement: self.tiles.placement(),
            loading,
            complete: self.listed_all,
            view: self.sidebar_view,
            recency: Recency::of(&self.sessions, self.now_unix, self.sidebar_view),
        });
        alerts
    }

    /// The notifications due and those to take back, from how each session's mark changed
    /// since the last call. Every change of a mark, whatever made it, is seen here: a mark is
    /// read from its entry alone, an entry whose mark changed puts the sessions' version up
    /// (see [`crate::EntryMut`]), and [`App::settle`] follows every change. While the user looks
    /// ([`App::looking`]) none is due, and those out are all taken back. Otherwise one is due
    /// for a session whose mark became [`Mark::Finished`] or [`Mark::NeedsYou`], unless one
    /// saying that is out already, and one is taken back when its session's mark is no longer
    /// the one it said, or the session is gone. A deleted session has no mark, so it is never
    /// the subject of one.
    fn alerts(&mut self) -> Vec<Effect> {
        let looking = self.looking();
        let seen = (self.sessions.version(), looking);
        if self.alerts_seen == Some(seen) {
            return Vec::new();
        }
        self.alerts_seen = Some(seen);
        let gone = self.sessions.take_unalerted();
        let sessions = &self.sessions;
        self.marks.retain(|key, _| sessions.get(*key).is_some());
        let mut changed = Vec::new();
        for entry in self.sessions.iter() {
            let mark = entry.mark();
            let before = self.marks.insert(entry.key, mark).unwrap_or(Mark::None);
            let alert = Alert::of(mark);
            let alerted = if looking {
                None
            } else if mark != before && alert.is_some() {
                alert
            } else {
                entry.alerted.filter(|&alerted| Some(alerted) == alert)
            };
            if alerted != entry.alerted {
                changed.push((entry.key, alerted));
            }
        }
        let mut effects = Vec::new();
        if looking {
            if !gone.is_empty() || !changed.is_empty() {
                effects.push(Effect::UnnotifyAll);
            }
        } else {
            effects.extend(gone.into_iter().map(|key| Effect::Unnotify { key }));
        }
        for (key, alerted) in changed {
            let Some(mut entry) = self.sessions.get_mut(key) else {
                continue;
            };
            entry.alerted = alerted;
            match alerted {
                Some(alert) => effects.push(Effect::Notify {
                    key,
                    title: row_title(&entry),
                    alert,
                }),
                None if !looking => effects.push(Effect::Unnotify { key }),
                None => {}
            }
        }
        effects
    }

    /// The list the sidebar shows.
    pub fn sidebar_view(&self) -> SidebarView {
        self.sidebar_view
    }

    /// Shows the sessions, or those deleted. Nothing is saved.
    pub fn set_sidebar_view(&mut self, view: SidebarView) -> Vec<Effect> {
        self.sidebar_view = view;
        Vec::new()
    }

    /// [`App::new_session`] in `folder`, and the session focused then placed in `group`. Asks
    /// once to save, after the placement. Nothing in a deleted group.
    pub fn new_session_in(
        &mut self,
        folder: PathBuf,
        group: &GroupId,
        place: Place,
        leaving: Leaving,
    ) -> Vec<Effect> {
        if self.groups.deleted_unix(group).is_some() {
            return Vec::new();
        }
        let mut effects = self.new_session(folder, place, leaving);
        drop_saves(&mut effects);
        if self.screen == Screen::Main
            && let Some(entry) = self.focused().and_then(|key| self.sessions.get(key))
        {
            self.groups.place(entry, group);
            effects.push(self.save_state());
        }
        effects
    }

    /// Puts `key`'s session in `group`. This and the group intents below that change what is
    /// kept between runs ask to save. Nothing for a deleted session, or into a deleted group.
    pub fn move_session(&mut self, key: SessionKey, group: &GroupId) -> Vec<Effect> {
        if self.is_deleted(key) || self.groups.deleted_unix(group).is_some() {
            return Vec::new();
        }
        if let Some(entry) = self.sessions.get(key) {
            self.groups.place(entry, group);
        }
        vec![self.save_state()]
    }

    pub fn rename_group(&mut self, id: &GroupId, name: &str) -> Vec<Effect> {
        self.groups.rename(id, name);
        vec![self.save_state()]
    }

    pub fn new_group(&mut self, name: &str) -> (GroupId, Vec<Effect>) {
        let id = self.groups.create(name);
        (id, vec![self.save_state()])
    }

    /// Deletes every session of the group `id` that is not deleted yet, of any age (see
    /// [`App::delete_session`]). A group the user made is marked deleted and keeps its record
    /// and the places of its sessions; a folder's group takes no mark. Asks once to save.
    ///
    /// Nothing before the complete listing is in ([`SidebarModel::complete`]): until then the
    /// app does not know every session of the group, so the count the user was asked about
    /// would not be the whole truth, and a session of a folder's group listed afterwards would
    /// stay behind.
    pub fn delete_group(&mut self, id: &GroupId) -> Vec<Effect> {
        if !self.listed_all {
            return Vec::new();
        }
        let mut effects = Vec::new();
        for key in self.sessions_of(id) {
            effects.extend(self.delete_session(key));
        }
        drop_saves(&mut effects);
        self.groups.set_deleted(id, Some(self.now_unix));
        effects.push(self.save_state());
        effects
    }

    /// A group the user made goes and its sessions go home; a folder's group is reset (see
    /// [`Groups::ungroup`]). Nothing for a folder's group that has nothing to reset.
    pub fn ungroup(&mut self, id: &GroupId) -> Vec<Effect> {
        if !self.groups.has_anything(id) {
            return Vec::new();
        }
        self.groups.ungroup(id);
        vec![self.save_state()]
    }

    /// What deleting would cut off: sessions with a turn running, a request waiting, a
    /// background task running, Remote Control on or coming, or a message queued, undelivered
    /// or held by a question.
    pub fn in_flight(&self, keys: &[SessionKey]) -> usize {
        keys.iter()
            .filter_map(|key| self.sessions.get(*key))
            .filter(|entry| stops(entry) || unsent(entry))
            .count()
    }

    /// What deleting `key`'s session would do to it now, as [`App::delete_session`] does it.
    /// None for a session that is deleted or not known.
    pub fn deleting(&self, key: SessionKey) -> Option<Deleting> {
        let entry = self.sessions.get(key).filter(|e| e.deleted.is_none())?;
        Some(if forgotten_by_delete(entry) {
            Deleting::Forgotten {
                holds: unsent(entry) || !entry.session.chips.is_empty(),
            }
        } else if stops(entry) {
            Deleting::Stops
        } else if unsent(entry) {
            Deleting::DropsMessage
        } else {
            Deleting::Kept
        })
    }

    /// The title the row of `key`'s session shows.
    pub fn title_of(&self, key: SessionKey) -> Option<String> {
        self.sessions.get(key).map(row_title)
    }

    /// Every session of the group `group` that is not deleted, also those listed only on
    /// request for their age.
    pub fn sessions_of(&self, group: &GroupId) -> Vec<SessionKey> {
        self.sessions
            .iter()
            .filter(|entry| entry.deleted.is_none() && self.groups.group_of(entry) == *group)
            .map(|entry| entry.key)
            .collect()
    }

    /// Moves `key`'s session to the Deleted view, and asks to save. Its tile closes. A session
    /// the store has nothing of is forgotten instead, as an empty one is, its program closed if
    /// it runs. Otherwise its queued and undelivered messages go with their chips, its program
    /// is sent `Close` once, one that ended rests, its question, check and mark are cleared, it
    /// no longer wants Remote Control, and its conversation is given back to the store as
    /// [`App::let_go`] gives it, or when its program has ended. Nothing for a deleted session.
    pub fn delete_session(&mut self, key: SessionKey) -> Vec<Effect> {
        if self
            .sessions
            .get(key)
            .is_none_or(|entry| entry.deleted.is_some())
        {
            return Vec::new();
        }
        let focused = self.focused();
        self.remove_tile(key);
        let mut effects: Vec<Effect> = self.branch_if_moved(focused).into_iter().collect();
        let now_unix = self.now_unix;
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return effects;
        };
        if forgotten_by_delete(&entry) {
            drop(entry);
            if let Some(entry) = self.sessions.remove(key)
                && entry.live()
                && let Some(open) = entry.open
            {
                effects.push(Effect::Send {
                    open,
                    command: SessionCommand::Close,
                });
            }
            effects.push(self.save_state());
            return effects;
        }
        entry.session.queued = None;
        entry.session.undelivered.clear();
        // Woken while the store was asked about it, so no program was opened yet, or ended: a
        // deleted session rests, and so lets its conversation go.
        if entry.open.is_none()
            && matches!(entry.session.phase, Phase::Opening | Phase::Ended { .. })
        {
            entry.session.phase = Phase::Resting;
        }
        let runs = entry.live();
        if let Some(open) = entry.open
            && runs
            && entry.stopping != Some(open)
        {
            entry.stopping = Some(open);
            effects.push(Effect::Send {
                open,
                command: SessionCommand::Close,
            });
        }
        entry.unseen = false;
        entry.prompt = None;
        entry.checking = false;
        entry.session.remote_wanted = false;
        // What its own program wrote, also as it ended, is this app's own doing.
        let own_exit = entry
            .last_own_unix
            .map_or(i64::MIN, |unix| unix + OWN_EXIT_SLACK_SECONDS);
        let closed = if runs { now_unix } else { i64::MIN };
        let seen_unix = entry.last_active_unix.max(own_exit).max(closed);
        entry.deleted = Some(Deleted {
            at_unix: now_unix,
            seen_unix,
            failed_unix: None,
            removing: false,
        });
        if entry.session.let_go() {
            effects.push(Effect::DropHistory { key });
        }
        drop(entry);
        effects.push(self.save_state());
        effects
    }

    /// Removes everything in the Deleted view for good, whatever its age.
    ///
    /// The week of every deleted session and of every deleted group the user made ends now, a
    /// removal that failed is tried again, and what is due goes at once, without waiting for
    /// the tick (see [`App::purge`]); a session whose program has not ended yet goes when it
    /// has ([`App::event`]). It does not reach the marks still waiting for sessions no listing
    /// has given yet: such a session keeps the week it had. Asks once to save; nothing when
    /// nothing is deleted.
    pub fn delete_all(&mut self) -> Vec<Effect> {
        let ended = self.now_unix.saturating_sub(DELETED_SECONDS);
        let keys: Vec<SessionKey> = self
            .sessions
            .iter()
            .filter(|entry| entry.deleted.is_some())
            .map(|entry| entry.key)
            .collect();
        let groups = self.groups.deleted();
        if keys.is_empty() && groups.is_empty() {
            return Vec::new();
        }
        for key in keys {
            if let Some(mut entry) = self.sessions.get_mut(key)
                && let Some(deleted) = entry.deleted
            {
                entry.deleted = Some(Deleted {
                    at_unix: ended,
                    failed_unix: None,
                    ..deleted
                });
            }
        }
        for group in &groups {
            self.groups.set_deleted(group, Some(ended));
        }
        let mut effects = self.purge();
        drop_saves(&mut effects);
        effects.push(self.save_state());
        effects
    }

    /// Takes `key`'s session out of the Deleted view, and its group too when that is deleted.
    /// Asks to save; nothing when it is not deleted, and nothing while its removal is under way
    /// ([`Deleted::removing`]): its file may be going, also under a program started for it.
    pub fn recover_session(&mut self, key: SessionKey) -> Vec<Effect> {
        if self.being_removed(key) {
            return Vec::new();
        }
        if self.recover(key) {
            vec![self.save_state()]
        } else {
            Vec::new()
        }
    }

    /// Takes the group `id` out of the Deleted view, with every deleted session in it, also
    /// those placed in it that no listing has given yet: their marks go. A session whose
    /// removal is under way stays deleted (see [`App::recover_session`]). Asks to save.
    pub fn recover_group(&mut self, id: &GroupId) -> Vec<Effect> {
        self.groups.set_deleted(id, None);
        let groups = &self.groups;
        self.deleted_marks
            .retain(|mark| groups.placement(&mark.id) != Some(id));
        let deleted: Vec<SessionKey> = self
            .sessions
            .iter()
            .filter(|entry| {
                entry.deleted.is_some_and(|deleted| !deleted.removing)
                    && self.groups.group_of(entry) == *id
            })
            .map(|entry| entry.key)
            .collect();
        for key in deleted {
            self.recover(key);
        }
        vec![self.save_state()]
    }

    pub fn collapse_group(&mut self, id: &GroupId, collapsed: bool) -> Vec<Effect> {
        self.groups.set_collapsed(id, collapsed);
        vec![self.save_state()]
    }

    pub fn hide_group(&mut self, id: &GroupId, hidden: bool) -> Vec<Effect> {
        self.groups.set_hidden(id, hidden);
        vec![self.save_state()]
    }

    /// Lists `id`'s old sessions too. Only for this run: nothing is saved.
    pub fn show_older(&mut self, id: &GroupId) -> Vec<Effect> {
        self.groups.show_older(id);
        Vec::new()
    }

    /// Lists the older groups, or stops. Only for this run: nothing is saved.
    pub fn toggle_older_groups(&mut self) -> Vec<Effect> {
        self.groups.toggle_older_groups();
        Vec::new()
    }

    pub fn set_width(&mut self, width: Width) -> Vec<Effect> {
        let width = match width {
            Width::Points(points) => Width::Points(points.max(MIN_WIDTH)),
            Width::Unlimited => Width::Unlimited,
        };
        if width == self.width {
            return Vec::new();
        }
        self.width = width;
        vec![Effect::SaveWidth(width)]
    }

    /// [`App::recover_session`] with no save. True when it was deleted. It comes back with no
    /// mark. Also for a session being removed: the removal's own answer that it changed comes
    /// here ([`App::removed`]), so the user's intents and the listing refuse such a session
    /// themselves.
    fn recover(&mut self, key: SessionKey) -> bool {
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return false;
        };
        if entry.deleted.take().is_none() {
            return false;
        }
        entry.unseen = false;
        drop(entry);
        if let Some(entry) = self.sessions.get(key) {
            let group = self.groups.group_of(entry);
            if self.groups.deleted_unix(&group).is_some() {
                self.groups.set_deleted(&group, None);
            }
        }
        true
    }

    fn is_deleted(&self, key: SessionKey) -> bool {
        self.sessions
            .get(key)
            .is_some_and(|entry| entry.deleted.is_some())
    }

    /// The removal of `key`'s session was asked for and has not answered.
    fn being_removed(&self, key: SessionKey) -> bool {
        self.sessions
            .get(key)
            .and_then(|entry| entry.deleted)
            .is_some_and(|deleted| deleted.removing)
    }

    /// Asks to write what is kept between runs, as it is now.
    fn save_state(&self) -> Effect {
        Effect::SaveState(Box::new(self.saved()))
    }

    /// Starts a program for `key`'s session, under the next open's number. Every start but a
    /// copy's comes here. A deleted session is never started: one its caller made ready to start
    /// rests again.
    fn start(&mut self, key: SessionKey) -> Vec<Effect> {
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        if entry.deleted.is_some() {
            if entry.open.is_none() && entry.session.phase == Phase::Opening {
                entry.session.phase = Phase::Resting;
            }
            return Vec::new();
        }
        let spec = entry.session.start_spec();
        drop(entry);
        self.start_with(key, spec)
    }

    /// Starts `key`'s session again, with no question to the store, when it rests with a message
    /// its program never got. Not once the app is going away. And not without end: after the
    /// starts [`Session::reopen_for_undelivered`] allows, the session is left ended with a
    /// notice, marked as a program that died out of sight is, and [`App::restart`] delivers the
    /// message.
    fn start_undelivered(&mut self, key: SessionKey) -> Vec<Effect> {
        let (focused, looking) = (self.focused(), self.looking());
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        if self.closing
            || entry.session.phase != Phase::Resting
            || entry.session.undelivered.is_empty()
        {
            return Vec::new();
        }
        if !entry.session.reopen_for_undelivered() {
            if entry.deleted.is_none() && (focused != Some(key) || !looking) {
                entry.unseen = true;
            }
            return Vec::new();
        }
        drop(entry);
        self.start(key)
    }

    /// Starts `key`'s session again, with no question to the store, when it rests wanting
    /// Remote Control and this app ran its program in this run: the program that starts is
    /// asked for it (see [`Session::apply`]). Not for a session that is not stored, has a
    /// question waiting or is being asked about in the store, and not once the app is going
    /// away.
    fn start_for_remote(&mut self, key: SessionKey) -> Vec<Effect> {
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        if self.closing
            || entry.session.phase != Phase::Resting
            || !entry.session.remote_wanted
            || entry.last_own_unix.is_none()
            || !entry.session.is_stored()
            || entry.prompt.is_some()
            || entry.checking
        {
            return Vec::new();
        }
        entry.session.reopening();
        drop(entry);
        self.start(key)
    }

    /// Starts a program for `key`'s session as `spec` says, under the next open's number.
    fn start_with(&mut self, key: SessionKey, spec: StartSpec) -> Vec<Effect> {
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        // Every caller starts a session whose last program has ended: a second open while one
        // runs would take its events for stale ones.
        debug_assert!(entry.open.is_none(), "a program already runs for it");
        self.open += 1;
        entry.open = Some(self.open);
        entry.touched = true;
        vec![Effect::Open {
            open: self.open,
            spec,
        }]
    }

    /// Asks for the stored sessions active in the last [`RECENT_DAYS`].
    fn list_recent(&mut self) -> Effect {
        if self.listing == Listing::NotAsked {
            self.listing = Listing::Loading;
        }
        self.listed_unix = Some(self.now_unix);
        Effect::ListSessions {
            since_unix: Some(self.now_unix - RECENT_DAYS * 86_400),
        }
    }

    /// Asks for a look, which also asks for the usage limits.
    fn look(&mut self) -> Effect {
        self.usage_asked_unix = Some(self.now_unix);
        Effect::Look {
            folder: self.look_folder(),
        }
    }

    /// Where a look starts its session: in the folder of the entry shown again from the last
    /// run, else in the home folder the app was given. Never in the newest entry's folder,
    /// which is wherever some program last wrote to the store. Whether the folder is still
    /// there is for the caller to check.
    fn look_folder(&self) -> PathBuf {
        self.restored
            .and_then(|key| self.sessions.get(key))
            .map_or_else(
                || self.launch.home.clone(),
                |entry| entry.session.folder.clone(),
            )
    }

    /// Once a usage window's reset time has passed, the limits are asked for again, once for
    /// that reset, and not within [`USAGE_RETRY_SECONDS`] of the last ask: `RefreshUsage` to a
    /// session whose program runs and is not being stopped, or a look when none does, the app
    /// looks and a window shows. A look starts a program: with no window to show the limits
    /// in, it waits for one ([`App::set_window`]). The answer clears a lapsed window it leaves
    /// out (see [`take_usage`]); whatever it says, nothing more is asked until a program
    /// reports a window that then lapses.
    fn usage_after_reset(&mut self) -> Vec<Effect> {
        let now_unix = self.now_unix;
        let lapsed = [&self.usage.session, &self.usage.weekly]
            .into_iter()
            .flatten()
            .map(|window| window.resets_at_unix)
            .filter(|reset| *reset <= now_unix)
            .max();
        let Some(lapsed) = lapsed.filter(|reset| self.usage_reset_asked.is_none_or(|a| a < *reset))
        else {
            return Vec::new();
        };
        if self
            .usage_asked_unix
            .is_some_and(|asked| now_unix - asked <= USAGE_RETRY_SECONDS)
        {
            return Vec::new();
        }
        let live = self
            .sessions
            .iter()
            .filter(|entry| entry.live() && entry.stopping.is_none())
            .find_map(|entry| entry.open);
        match live {
            Some(open) => {
                self.usage_asked_unix = Some(now_unix);
                self.usage_reset_asked = Some(lapsed);
                vec![Effect::Send {
                    open,
                    command: SessionCommand::RefreshUsage,
                }]
            }
            None if self.launch.look && self.window => {
                self.usage_reset_asked = Some(lapsed);
                vec![self.look()]
            }
            None => Vec::new(),
        }
    }

    /// Keeps `models` as the newest known, when there are any, and offers them to every session.
    fn learn_models(&mut self, models: Vec<ModelOption>) {
        if models.is_empty() {
            return;
        }
        for mut entry in self.sessions.iter_mut() {
            entry.session.offer_models(&models);
        }
        self.known_models = models;
    }

    /// Marks `key`'s session as one put in a tile: it is no longer out of every tile, and is
    /// offered the models known. Its stored history, when not read yet, is asked for. Whether
    /// it is ready is left as it is: the user's own act clears that ([`App::attend`]).
    fn mark_shown(&mut self, key: SessionKey) -> Option<Effect> {
        let mut entry = self.sessions.get_mut(key)?;
        entry.touched = true;
        entry.hidden_unix = None;
        if !self.known_models.is_empty() {
            entry.session.offer_models(&self.known_models);
        }
        read_history(&mut entry)
    }

    /// The user acted on `key`'s session ([`App::attend`]); the focus moves to its tile, when
    /// it is in one without it, and the branch of its folder is asked for when the focus moved.
    fn focus(&mut self, key: SessionKey) -> Vec<Effect> {
        let mut effects = self.attend(key);
        if self.tiles.focus(key) {
            effects.extend(self.read_branch());
        }
        effects
    }

    /// Asks for the branch of the focused session's folder.
    fn read_branch(&self) -> Option<Effect> {
        let folder = self.focused_entry()?.session.folder.clone();
        Some(Effect::ReadBranch { folder })
    }

    /// [`App::read_branch`], when the focus is no longer on `before`.
    fn branch_if_moved(&self, before: Option<SessionKey>) -> Option<Effect> {
        (self.focused() != before)
            .then(|| self.read_branch())
            .flatten()
    }

    /// Takes `key`'s tile out of the row. The session whose tile takes the focus stays ready if
    /// it is: the user did not act on it. True when there was such a tile.
    fn remove_tile(&mut self, key: SessionKey) -> bool {
        if !self.tiles.remove(key) {
            return false;
        }
        self.left_tiles(key);
        true
    }

    /// Closes and forgets `key`'s session when it is empty and in no tile.
    fn forget_if_empty(&mut self, key: SessionKey) -> Vec<Effect> {
        if self.tiles.contains(key)
            || !self
                .sessions
                .get(key)
                .is_some_and(|entry| empty(&entry.session))
        {
            return Vec::new();
        }
        match self.sessions.remove(key) {
            Some(entry) if entry.live() => entry
                .open
                .map(|open| Effect::Send {
                    open,
                    command: SessionCommand::Close,
                })
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Notes when `key`'s session left its last tile, if it is in none now.
    fn left_tiles(&mut self, key: SessionKey) {
        if self.tiles.contains(key) {
            return;
        }
        let now_unix = self.now_unix;
        if let Some(mut entry) = self.sessions.get_mut(key) {
            entry.hidden_unix = Some(now_unix);
        }
    }

    /// `key`'s session gives its conversation back to the store ([`Session::let_go`]) when it
    /// has been in no tile for [`LET_GO_SECONDS`], no question waits about it and the store is
    /// not being asked about it: the backend is then told the history is no longer wanted. Only
    /// [`App::tick`] calls this, so a session swapped out of a tile and back within that time
    /// is not read again. One that was never in a tile counts from when its program last said
    /// anything, which at rest is its exit: a session that did not come from the store has had
    /// a program.
    fn let_go(&mut self, key: SessionKey) -> Option<Effect> {
        if self.tiles.contains(key) {
            return None;
        }
        let now_unix = self.now_unix;
        let mut entry = self.sessions.get_mut(key)?;
        let since = entry.hidden_unix.or(entry.last_own_unix)?;
        if now_unix - since < LET_GO_SECONDS
            || entry.prompt.is_some()
            || entry.checking
            || !entry.session.let_go()
        {
            return None;
        }
        Some(Effect::DropHistory { key })
    }

    /// The commands for `key`'s session, sent to its current open. None to a deleted session.
    fn send(
        &self,
        key: SessionKey,
        commands: impl IntoIterator<Item = SessionCommand>,
    ) -> Vec<Effect> {
        let Some(open) = self
            .sessions
            .get(key)
            .filter(|entry| entry.deleted.is_none())
            .and_then(|entry| entry.open)
        else {
            return Vec::new();
        };
        commands
            .into_iter()
            .map(|command| Effect::Send { open, command })
            .collect()
    }
}

/// Takes in the windows `limits` reports. One it does not report is kept, unless its reset time
/// has passed by `now_unix`: what it said no longer holds, so it is cleared.
fn take_usage(usage: &mut UsageLimits, limits: &UsageLimits, now_unix: i64) {
    let take = |held: &mut Option<LimitWindow>, reported: &Option<LimitWindow>| {
        if reported.is_some() {
            *held = reported.clone();
        } else if held.as_ref().is_some_and(|w| w.resets_at_unix <= now_unix) {
            *held = None;
        }
    };
    take(&mut usage.session, &limits.session);
    take(&mut usage.weekly, &limits.weekly);
}

/// Takes the asks to save out of `effects`. True when there was one.
fn drop_saves(effects: &mut Vec<Effect>) -> bool {
    let before = effects.len();
    effects.retain(|effect| !matches!(effect, Effect::SaveState(_)));
    effects.len() != before
}

/// `entry`'s stored history, asked for when it has not been read yet. Never a deleted one's.
fn read_history(entry: &mut Entry) -> Option<Effect> {
    if entry.deleted.is_some() || entry.session.history != HistoryState::NotLoaded {
        return None;
    }
    let id = entry.session.id.clone()?;
    entry.session.history = HistoryState::Loading;
    entry.session.read_from = Some(id.clone());
    Some(Effect::ReadHistory {
        key: entry.key,
        id,
        max_items: HISTORY_ITEMS,
    })
}

/// Deleting stops `entry`'s program at work: a turn runs, a request waits, a background task
/// runs, or Remote Control is on or coming.
fn stops(entry: &Entry) -> bool {
    let session = &entry.session;
    matches!(session.phase, Phase::Working | Phase::NeedsUser)
        || session.background > 0
        || session.remote != Remote::Off
}

/// `entry` holds a message that was not sent yet: queued, never delivered, or held by a
/// question.
fn unsent(entry: &Entry) -> bool {
    entry.session.queued.is_some()
        || !entry.session.undelivered.is_empty()
        || entry.prompt.is_some()
}

/// The store has nothing of `entry`'s session, so deleting it forgets it.
fn forgotten_by_delete(entry: &Entry) -> bool {
    entry.session.id.is_none() || !entry.session.is_stored()
}

/// Nothing was handed over in it, nothing waits to be and nothing is attached: the agent stores
/// nothing for it, so leaving it forgets it.
fn empty(session: &Session) -> bool {
    !session.is_stored()
        && session.queued.is_none()
        && session.undelivered.is_empty()
        && session.chips.is_empty()
}

#[cfg(test)]
mod tests {
    use mr_agent::{
        Attachment, HistoryItem, ImageData, LimitWindow, RemoteAccess, SessionInfo, StoredHistory,
        ToolDetail, TurnSummary, UserRequest, UserRequestKind,
    };

    use super::*;
    use crate::attach::Kind as AttachKind;
    use crate::conversation::ItemBody;
    use crate::groups::Group;
    use crate::saved::{RECENT_FOLDERS, SAVED_VERSION, SavedBelow, SavedDeleted, SavedTile};
    use crate::session::testing::info;
    use crate::sessions::Mark;

    /// The wall clock the tests' apps start at.
    const NOW: i64 = 1_800_000_000;

    fn caps() -> Capabilities {
        Capabilities {
            remote_access: true,
            plan_mode: true,
            usage_limits: true,
            images: true,
        }
    }

    fn signed_in() -> Account {
        Account::SignedIn {
            name: "alex".into(),
            plan: Some("Max".into()),
            full_login: true,
        }
    }

    fn launch(folder: &str) -> Launch {
        Launch {
            folder: Some(folder.into()),
            ..Launch::default()
        }
    }

    /// A launch with no folder that looks, from `/home/alex`.
    fn looking() -> Launch {
        Launch {
            folder: None,
            look: true,
            home: "/home/alex".into(),
            ..Launch::default()
        }
    }

    /// What the recent listing asks for at `now`.
    fn recent(now: i64) -> Effect {
        Effect::ListSessions {
            since_unix: Some(now - RECENT_DAYS * 86_400),
        }
    }

    /// The answer to a listing since `since_unix` that gives `sessions`.
    fn listing(
        app: &mut App,
        since_unix: Option<i64>,
        sessions: Vec<StoredSession>,
    ) -> Vec<Effect> {
        app.receive(
            FromBackend::Sessions {
                since_unix,
                result: Ok(sessions),
            },
            0.0,
        )
    }

    fn store(id: &str, title: &str, folder: &str, last_active_unix: i64) -> StoredSession {
        StoredSession {
            id: SessionId(id.into()),
            folder: folder.into(),
            title: title.into(),
            last_active_unix,
        }
    }

    fn id(text: &str) -> SessionId {
        SessionId(text.into())
    }

    /// The entry whose session has the agent's id `text`.
    fn by_id(app: &App, text: &str) -> SessionKey {
        app.sessions.by_id(&id(text)).expect("listed")
    }

    /// An app with no launch folder, signed in, that knows the stored session `a1` (in
    /// `/home/alex/project`, an hour old, its history read) and shows it.
    fn shows_stored() -> (App, SessionKey) {
        let mut app = main_screen();
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 3600);
        listing(&mut app, Some(NOW - RECENT_DAYS * 86_400), vec![a1]);
        let key = by_id(&app, "a1");
        app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        let history = StoredHistory {
            items: vec![
                HistoryItem::User {
                    text: "read the parser".into(),
                    image_count: 0,
                    files: Vec::new(),
                    images: Vec::new(),
                    stored: None,
                },
                HistoryItem::AssistantText("It reads tokens.".into()),
            ],
            ..StoredHistory::default()
        };
        app.receive(
            FromBackend::History {
                key,
                result: Ok(history),
                pictures: Vec::new(),
            },
            0.0,
        );
        (app, key)
    }

    /// [`shows_stored`] with "carry on" sent to it and an answer that it changed `ago` seconds
    /// before now.
    fn checked(ago: i64) -> (App, SessionKey, Vec<Effect>) {
        let (mut app, key) = shows_stored();
        let effects = app.submit(key, "carry on");
        assert_eq!(effects, vec![Effect::CheckActivity { key, id: id("a1") }]);
        let effects = app.receive(
            FromBackend::Activity {
                key,
                result: Ok(Some(NOW - ago)),
            },
            0.0,
        );
        (app, key, effects)
    }

    /// How `a1` is resumed.
    fn resume_a1() -> StartSpec {
        let mut spec = StartSpec::new("/home/alex/project");
        spec.resume = Some(id("a1"));
        spec
    }

    fn sends_text(effects: &[Effect], open: u64, text: &str) -> bool {
        effects.iter().any(|effect| {
            matches!(
                effect,
                Effect::Send {
                    open: o,
                    command: SessionCommand::Send { text: t, .. }
                } if *o == open && t == text
            )
        })
    }

    /// The effects that ask for the usage limits: a `RefreshUsage` or a look.
    fn usage_asks(effects: Vec<Effect>) -> Vec<Effect> {
        effects
            .into_iter()
            .filter(|effect| {
                matches!(
                    effect,
                    Effect::Look { .. }
                        | Effect::Send {
                            command: SessionCommand::RefreshUsage,
                            ..
                        }
                )
            })
            .collect()
    }

    /// An app launched in `/p`, signed in, its session started; and that session's key.
    fn ready() -> (App, SessionKey) {
        let (mut app, _) = App::new(caps(), Width::Unlimited, launch("/p"), NOW);
        app.account_checked(signed_in());
        app.event(1, SessionEvent::Started(info()), 0.0);
        let key = app.focused().expect("the launch folder's session is shown");
        (app, key)
    }

    /// An app with no launch folder, signed in, with no session yet.
    fn main_screen() -> App {
        let (mut app, _) = App::new(caps(), Width::Unlimited, Launch::default(), NOW);
        app.account_checked(signed_in());
        app
    }

    /// A new session in `folder`, started, with a message handed over: a turn runs in it, and
    /// it is not forgotten when another is shown.
    fn busy(app: &mut App, folder: &str) -> SessionKey {
        app.new_session(folder.into(), Place::Focused, Leaving::ForgetIfEmpty);
        let key = app.focused().unwrap();
        app.event(open_of(app, key), SessionEvent::Started(info()), 0.0);
        app.submit(key, "go");
        key
    }

    fn open_of(app: &App, key: SessionKey) -> u64 {
        app.sessions.get(key).unwrap().open.unwrap()
    }

    fn session(app: &App, key: SessionKey) -> &Session {
        &app.sessions.get(key).unwrap().session
    }

    fn mark(app: &App, key: SessionKey) -> Mark {
        app.sessions.get(key).unwrap().mark()
    }

    /// `effects` without the asks to save, for tests about something else.
    fn unsaved(effects: Vec<Effect>) -> Vec<Effect> {
        effects
            .into_iter()
            .filter(|effect| !matches!(effect, Effect::SaveState(_)))
            .collect()
    }

    /// The opens `effects` send `Close` to, in order.
    fn closes(effects: &[Effect]) -> Vec<u64> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Send {
                    open,
                    command: SessionCommand::Close,
                } => Some(*open),
                _ => None,
            })
            .collect()
    }

    fn done() -> SessionEvent {
        SessionEvent::TurnFinished(TurnSummary {
            cost_usd: 0.01,
            context_used: None,
            duration_ms: 1,
            interrupted: false,
        })
    }

    fn exited(clean: bool) -> SessionEvent {
        SessionEvent::Exited {
            clean,
            detail: if clean {
                String::new()
            } else {
                "killed".into()
            },
        }
    }

    fn permission(id: &str) -> SessionEvent {
        SessionEvent::NeedsUser(UserRequest {
            id: RequestId(id.into()),
            kind: UserRequestKind::Permission {
                tool: "Bash".into(),
                detail: ToolDetail::Command {
                    command: "ls".into(),
                },
                always_allow_label: None,
            },
        })
    }

    /// A stored session, resting.
    fn stored(id: &str) -> Session {
        Session::resting(&StoredSession {
            id: SessionId(id.into()),
            folder: "/home/alex/project".into(),
            title: "Fix the parser".into(),
            last_active_unix: NOW - 60,
        })
    }

    #[test]
    fn the_first_screen_depends_on_the_account() {
        let (mut app, effects) = App::new(caps(), Width::Points(DEFAULT_WIDTH), launch("/p"), NOW);
        assert_eq!(
            (app.screen, effects),
            (Screen::Checking, vec![Effect::CheckAccount])
        );
        assert_eq!(app.account_checked(Account::Missing), vec![]);
        assert_eq!(app.screen, Screen::NotInstalled);
        assert_eq!(app.check_again(), vec![Effect::CheckAccount]);
        assert_eq!(app.screen, Screen::Checking);
        assert_eq!(app.account_checked(Account::SignedOut), vec![]);
        assert_eq!(app.screen, Screen::SignedOut);
        app.check_again();
        let effects = app.account_checked(signed_in());
        assert_eq!(
            effects,
            vec![
                reads_branch("/p"),
                Effect::Open {
                    open: 1,
                    spec: StartSpec::new("/p")
                },
                save(&app),
                recent(NOW)
            ],
            "the launch folder is a recent folder now"
        );
        let shown = app.focused_entry().unwrap();
        assert_eq!((app.screen, shown.open), (Screen::Main, Some(1)));
        assert_eq!(shown.session.phase, Phase::Opening);
        assert_eq!(app.check_again(), vec![]);
    }

    #[test]
    fn without_a_launch_folder_no_session_starts() {
        let (mut app, _) = App::new(caps(), Width::Unlimited, Launch::default(), NOW);
        assert_eq!(app.account_checked(signed_in()), vec![recent(NOW)]);
        assert_eq!(app.screen, Screen::Main);
        assert!(app.sessions.is_empty() && app.focused().is_none());
    }

    #[test]
    fn a_started_session_asks_for_the_usage_limits() {
        let (mut app, _) = App::new(caps(), Width::Unlimited, launch("/p"), NOW);
        app.account_checked(signed_in());
        let effects = app.event(1, SessionEvent::Started(info()), 0.0);
        assert_eq!(
            effects,
            vec![Effect::Send {
                open: 1,
                command: SessionCommand::RefreshUsage
            }]
        );
        let mut without = caps();
        without.usage_limits = false;
        let (mut app, _) = App::new(without, Width::Unlimited, launch("/p"), NOW);
        app.account_checked(signed_in());
        assert_eq!(app.event(1, SessionEvent::Started(info()), 0.0), vec![]);
    }

    fn window(used: f32) -> Option<LimitWindow> {
        Some(LimitWindow {
            used,
            resets_at_unix: 5,
        })
    }

    #[test]
    fn usage_keeps_a_window_that_was_not_reported_this_time() {
        // A window that has not reset yet.
        let ahead = |used| {
            Some(LimitWindow {
                used,
                resets_at_unix: NOW + 3600,
            })
        };
        let (mut app, _) = ready();
        app.event(
            1,
            SessionEvent::UsageLimits(UsageLimits {
                session: ahead(0.2),
                weekly: ahead(0.4),
            }),
            0.0,
        );
        app.event(
            1,
            SessionEvent::UsageLimits(UsageLimits {
                session: ahead(0.3),
                weekly: None,
            }),
            0.0,
        );
        assert_eq!(
            app.usage,
            UsageLimits {
                session: ahead(0.3),
                weekly: ahead(0.4)
            }
        );
        // Once its reset time has passed, a window a report leaves out is cleared: what it
        // said no longer holds.
        app.tick(NOW + 3600);
        app.event(
            1,
            SessionEvent::UsageLimits(UsageLimits {
                session: None,
                weekly: window(0.1),
            }),
            0.0,
        );
        assert_eq!(
            app.usage,
            UsageLimits {
                session: None,
                weekly: window(0.1)
            }
        );
    }

    #[test]
    fn the_newest_usage_from_any_session_wins() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        let b = busy(&mut app, "/b");
        let report = |used| {
            SessionEvent::UsageLimits(UsageLimits {
                session: window(used),
                weekly: None,
            })
        };
        app.event(open_of(&app, b), report(0.3), 0.0);
        assert_eq!(app.usage.session, window(0.3), "from a session shown");
        app.event(open_of(&app, a), report(0.5), 0.0);
        assert_eq!(app.usage.session, window(0.5), "from one out of sight");
    }

    #[test]
    fn closing_the_app_closes_a_session_that_has_not_ended() {
        let (mut app, _) = App::new(caps(), Width::Unlimited, launch("/p"), NOW);
        assert_eq!(unsaved(app.close(NOW)), vec![], "no session yet");
        assert!(app.all_ended());
        app.account_checked(signed_in());
        let close = vec![Effect::Send {
            open: 1,
            command: SessionCommand::Close,
        }];
        assert_eq!(unsaved(app.close(NOW)), close, "a session still opening");
        app.event(1, SessionEvent::Started(info()), 0.0);
        assert!(!app.all_ended());
        assert_eq!(unsaved(app.close(NOW)), close);
        app.event(1, exited(true), 0.0);
        assert!(app.all_ended());
        assert_eq!(unsaved(app.close(NOW)), vec![], "already ended");
    }

    #[test]
    fn a_replaced_session_can_no_longer_change_anything() {
        let (mut app, old) = ready();
        let effects = unsaved(app.new_session("/q".into(), Place::Focused, Leaving::ForgetIfEmpty));
        assert_eq!(
            effects,
            vec![
                Effect::Send {
                    open: 1,
                    command: SessionCommand::Close
                },
                reads_branch("/q"),
                Effect::Open {
                    open: 2,
                    spec: StartSpec::new("/q")
                },
            ]
        );
        assert!(
            app.sessions.get(old).is_none(),
            "an empty session is replaced"
        );
        let new = app.focused().unwrap();
        for stale in [
            SessionEvent::TextDelta("late".into()),
            SessionEvent::UsageLimits(UsageLimits {
                session: window(0.9),
                weekly: None,
            }),
            exited(true),
        ] {
            assert_eq!(app.event(1, stale, 0.0), vec![]);
        }
        assert_eq!(app.open_failed(1, AgentError::Io("x".into())), vec![]);
        let session = session(&app, new);
        assert_eq!(session.phase, Phase::Opening);
        assert!(session.conversation.items().is_empty());
        assert_eq!(
            app.usage,
            UsageLimits {
                session: None,
                weekly: None
            }
        );
        app.event(2, SessionEvent::Started(info()), 0.0);
        assert_eq!(
            app.submit(new, "go"),
            vec![
                Effect::Send {
                    open: 2,
                    command: SessionCommand::Send {
                        text: "go".into(),
                        attachments: vec![]
                    }
                },
                Effect::Send {
                    open: 2,
                    command: SessionCommand::SuggestTitle { from: "go".into() }
                }
            ]
        );
    }

    #[test]
    fn an_ended_session_restarts_where_it_was() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.event(1, exited(false), 0.0);
        let effects = app.new_session("/p".into(), Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(effects.len(), 2, "its branch and its open: {effects:?}");
        assert_eq!(
            closes(&effects),
            Vec::<u64>::new(),
            "an ended session is not sent Close"
        );
        let (mut app, key) = ready();
        app.submit(key, "go");
        assert_eq!(
            app.restart(key),
            vec![],
            "a live session cannot be restarted"
        );
        app.event(1, exited(false), 0.0);
        let effects = app.restart(key);
        let mut spec = StartSpec::new("/p");
        spec.resume = Some(SessionId("s1".into()));
        spec.model = Some("claude-haiku-5-5".into());
        assert_eq!(effects, vec![Effect::Open { open: 2, spec }]);
        assert!(session(&app, key).conversation.items().len() >= 2);
    }

    #[test]
    fn a_failed_open_shows_where_the_problem_is() {
        let (mut app, _) = App::new(caps(), Width::Unlimited, launch("/p"), NOW);
        app.account_checked(signed_in());
        let key = app.focused().unwrap();
        app.open_failed(1, AgentError::Io("no such folder".into()));
        assert_eq!(
            session(&app, key).phase,
            Phase::Ended {
                clean: false,
                detail: "i/o error: no such folder".into()
            }
        );
        assert_eq!(app.sessions.get(key).unwrap().open, None);
        let (mut app, _) = App::new(caps(), Width::Unlimited, launch("/p"), NOW);
        app.account_checked(signed_in());
        app.open_failed(1, AgentError::NotInstalled);
        assert_eq!(app.screen, Screen::NotInstalled);
        assert!(app.focused().is_none() && app.sessions.is_empty());
    }

    #[test]
    fn a_refused_answer_is_explained_and_a_closed_session_is_not() {
        let (mut app, key) = ready();
        let answer = SessionCommand::Answer {
            id: RequestId("r1".into()),
            answer: Answer::Allow { always: false },
        };
        app.send_failed(1, &answer, AgentError::Closed);
        assert!(session(&app, key).conversation.items().is_empty());
        app.send_failed(1, &answer, AgentError::Protocol("not waiting".into()));
        let last = session(&app, key)
            .conversation
            .items()
            .last()
            .unwrap()
            .body
            .clone();
        assert_eq!(
            last,
            ItemBody::Notice {
                kind: NoticeKind::CommandFailed,
                text: "That request was no longer waiting.".into()
            }
        );
    }

    #[test]
    fn a_message_that_could_not_be_sent_ends_the_turn() {
        for error in [AgentError::Io("broken pipe".into()), AgentError::Closed] {
            let (mut app, key) = ready();
            app.submit(key, "go");
            let send = SessionCommand::Send {
                text: "go".into(),
                attachments: vec![],
            };
            app.send_failed(1, &send, error.clone());
            let session = session(&app, key);
            assert_eq!(session.phase, Phase::Idle, "{error:?}");
            assert!(matches!(
                session.conversation.items()[0].body,
                ItemBody::User { .. }
            ));
        }
    }

    #[test]
    fn a_session_at_rest_is_not_closed() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.event(1, exited(true), 0.0);
        assert_eq!(session(&app, key).phase, Phase::Resting);
        assert!(app.all_ended(), "no program runs for it");
        assert_eq!(unsaved(app.close(NOW)), vec![]);
        assert_eq!(app.restart(key), vec![], "it wakes with a message instead");
    }

    #[test]
    fn a_title_request_that_could_not_be_sent_is_not_explained() {
        let (mut app, key) = ready();
        let suggest = SessionCommand::SuggestTitle { from: "go".into() };
        app.send_failed(1, &suggest, AgentError::Io("broken pipe".into()));
        app.send_failed(1, &suggest, AgentError::Protocol("odd".into()));
        assert!(session(&app, key).conversation.items().is_empty());
    }

    /// [`ready`] with "first" handed over, so the agent stores the session, and its turn
    /// finished; then "hello" handed over to open 1.
    fn sent_hello() -> (App, SessionKey) {
        let (mut app, key) = ready();
        app.submit(key, "first");
        app.event(1, done(), 0.0);
        app.submit(key, "hello");
        (app, key)
    }

    /// The backend's word that the `Send` of `text` with `attachments` to `open` was refused
    /// because its program had ended.
    fn closed_send(open: u64, text: &str, attachments: Vec<Attachment>) -> FromBackend {
        FromBackend::SendFailed {
            open,
            command: SessionCommand::Send {
                text: text.into(),
                attachments,
            },
            error: AgentError::Closed,
        }
    }

    fn exit_of(open: u64, clean: bool) -> FromBackend {
        FromBackend::Event {
            open,
            event: exited(clean),
        }
    }

    fn started_on(open: u64) -> FromBackend {
        FromBackend::Event {
            open,
            event: SessionEvent::Started(info()),
        }
    }

    /// The opens `effects` start.
    fn opened(effects: &[Effect]) -> Vec<&StartSpec> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Open { spec, .. } => Some(spec),
                _ => None,
            })
            .collect()
    }

    /// The text and attachments of each message `effects` send, in order.
    fn sent_messages(effects: &[Effect]) -> Vec<(&str, &[Attachment])> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Send {
                    command: SessionCommand::Send { text, attachments },
                    ..
                } => Some((text.as_str(), attachments.as_slice())),
                _ => None,
            })
            .collect()
    }

    /// How many of the conversation's items are the user's message `text`.
    fn user_messages(session: &Session, text: &str) -> usize {
        session
            .conversation
            .items()
            .iter()
            .filter(|item| matches!(&item.body, ItemBody::User { text: t, .. } if t == text))
            .count()
    }

    #[test]
    fn a_send_that_fails_after_a_clean_exit_starts_the_session_again_with_it() {
        let (mut app, key) = sent_hello();
        assert!(opened(&app.receive(exit_of(1, true), 0.0)).is_empty());
        assert_eq!(session(&app, key).phase, Phase::Resting);
        let effects = app.receive(closed_send(1, "hello", vec![]), 0.0);
        let specs = opened(&effects);
        assert_eq!(specs.len(), 1, "{effects:?}");
        assert_eq!(specs[0].resume, Some(id("s1")));
        assert_eq!(mark(&app, key), Mark::Working);
        assert_eq!(user_messages(session(&app, key), "hello"), 1);
        let effects = app.receive(started_on(2), 0.0);
        assert_eq!(sent_messages(&effects), vec![("hello", &[][..])]);
        assert!(sends_text(&effects, 2, "hello"));
        assert!(session(&app, key).undelivered.is_empty());
        assert_eq!(user_messages(session(&app, key), "hello"), 1);
    }

    #[test]
    fn after_an_unclean_exit_it_waits_for_the_restart() {
        let (mut app, key) = sent_hello();
        app.receive(exit_of(1, false), 0.0);
        let effects = app.receive(closed_send(1, "hello", vec![]), 0.0);
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert_eq!(session(&app, key).undelivered.len(), 1);
        assert_eq!(opened(&app.restart(key)).len(), 1);
        let effects = app.receive(started_on(2), 0.0);
        assert_eq!(sent_messages(&effects), vec![("hello", &[][..])]);
        assert!(session(&app, key).undelivered.is_empty());
    }

    #[test]
    fn an_undelivered_message_goes_before_the_queued_one() {
        let (mut app, key) = sent_hello();
        app.receive(exit_of(1, false), 0.0);
        app.receive(closed_send(1, "hello", vec![]), 0.0);
        app.restart(key);
        app.submit(key, "later");
        let effects = app.receive(started_on(2), 0.0);
        let texts: Vec<&str> = sent_messages(&effects).iter().map(|(t, _)| *t).collect();
        assert_eq!(texts, vec!["hello", "later"]);
        assert_eq!(user_messages(session(&app, key), "hello"), 1);
        assert_eq!(user_messages(session(&app, key), "later"), 1);
    }

    #[test]
    fn an_undelivered_message_keeps_its_images() {
        let (mut app, key) = sent_hello();
        app.receive(exit_of(1, false), 0.0);
        let image = pasted_image();
        app.receive(closed_send(1, "hello", vec![image.clone()]), 0.0);
        app.restart(key);
        let effects = app.receive(started_on(2), 0.0);
        let sent = sent_messages(&effects);
        assert_eq!(sent, vec![("hello", &[image.clone()][..])]);
        let (Attachment::Image { data: was, .. }, [Attachment::Image { data: is, .. }]) =
            (&image, sent[0].1)
        else {
            panic!("an image went");
        };
        assert!(
            std::ptr::eq(was.as_bytes(), is.as_bytes()),
            "the bytes are shared, not copied"
        );
    }

    #[test]
    fn a_failure_of_an_open_before_the_last_is_dropped() {
        let (mut app, key) = sent_hello();
        app.receive(exit_of(1, false), 0.0);
        app.restart(key);
        app.receive(started_on(2), 0.0);
        app.receive(exit_of(2, false), 0.0);
        let before = session(&app, key).clone();
        assert_eq!(app.receive(closed_send(1, "hello", vec![]), 0.0), vec![]);
        assert!(session(&app, key).undelivered.is_empty());
        assert_eq!(session(&app, key), &before);
    }

    #[test]
    fn a_failed_answer_after_the_exit_is_still_dropped() {
        let (mut app, key) = sent_hello();
        app.receive(exit_of(1, true), 0.0);
        let before = session(&app, key).clone();
        let answer = SessionCommand::Answer {
            id: RequestId("r1".into()),
            answer: Answer::Allow { always: false },
        };
        let send = SessionCommand::Send {
            text: "hello".into(),
            attachments: vec![],
        };
        for (command, error) in [
            (answer.clone(), AgentError::Closed),
            (answer, AgentError::Protocol("not waiting".into())),
            (send, AgentError::Io("broken pipe".into())),
        ] {
            let failed = FromBackend::SendFailed {
                open: 1,
                command,
                error,
            };
            assert_eq!(app.receive(failed, 0.0), vec![]);
        }
        assert_eq!(session(&app, key), &before, "no notice, nothing kept");
    }

    #[test]
    fn a_session_with_an_undelivered_message_is_not_forgotten() {
        let mut app = main_screen();
        let other = app.sessions.add(stored("s9"), NOW - 60);
        let a = busy(&mut app, "/a");
        let open = open_of(&app, a);
        app.receive(closed_send(open, "go", vec![]), 0.0);
        app.receive(exit_of(open, true), 0.0);
        assert!(!session(&app, a).is_stored(), "nothing reached the agent");
        app.show(other, Place::Focused, Leaving::ForgetIfEmpty);
        assert!(app.sessions.get(a).is_some());
        assert_eq!(session(&app, a).undelivered.len(), 1);
    }

    #[test]
    fn a_send_that_fails_before_the_exit_is_not_sent_again_on_that_open() {
        let (mut app, key) = sent_hello();
        app.receive(
            FromBackend::Event {
                open: 1,
                event: permission("r1"),
            },
            0.0,
        );
        let effects = app.receive(closed_send(1, "hello", vec![]), 0.0);
        assert!(sent_messages(&effects).is_empty(), "{effects:?}");
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert_eq!(session(&app, key).undelivered.len(), 1);
        app.submit(key, "later");
        let effects = app.receive(
            FromBackend::Event {
                open: 1,
                event: done(),
            },
            0.0,
        );
        assert!(sent_messages(&effects).is_empty(), "{effects:?}");
        assert_eq!(session(&app, key).undelivered.len(), 1);
    }

    #[test]
    fn then_a_clean_exit_starts_the_session_again_with_it() {
        let (mut app, key) = sent_hello();
        assert_eq!(session(&app, key).phase, Phase::Working);
        let effects = app.receive(closed_send(1, "hello", vec![]), 0.0);
        assert!(sent_messages(&effects).is_empty(), "{effects:?}");
        assert!(opened(&effects).is_empty(), "{effects:?}");
        let effects = app.receive(exit_of(1, true), 0.0);
        let specs = opened(&effects);
        assert_eq!(specs.len(), 1, "{effects:?}");
        assert_eq!(specs[0].resume, Some(id("s1")));
        assert_eq!(session(&app, key).phase, Phase::Opening);
        assert_eq!(mark(&app, key), Mark::Working);
        let effects = app.receive(started_on(2), 0.0);
        assert_eq!(sent_messages(&effects), vec![("hello", &[][..])]);
        assert!(sends_text(&effects, 2, "hello"));
        assert_eq!(user_messages(session(&app, key), "hello"), 1);
    }

    #[test]
    fn then_an_unclean_exit_waits_for_the_restart() {
        let (mut app, key) = sent_hello();
        app.receive(closed_send(1, "hello", vec![]), 0.0);
        let effects = app.receive(exit_of(1, false), 0.0);
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert_eq!(session(&app, key).undelivered.len(), 1);
        assert_eq!(opened(&app.restart(key)).len(), 1);
        let effects = app.receive(started_on(2), 0.0);
        assert_eq!(sent_messages(&effects), vec![("hello", &[][..])]);
        assert_eq!(user_messages(session(&app, key), "hello"), 1);
    }

    #[test]
    fn a_program_that_ends_before_it_starts_is_not_started_again() {
        let (mut app, key) = sent_hello();
        app.receive(closed_send(1, "hello", vec![]), 0.0);
        app.receive(exit_of(1, true), 0.0);
        assert_eq!(session(&app, key).phase, Phase::Opening);
        let effects = app.receive(exit_of(2, true), 0.0);
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert_eq!(session(&app, key).phase, Phase::Resting);
        assert_eq!(session(&app, key).undelivered.len(), 1);
        let effects = app.submit(key, "again");
        assert_eq!(opened(&effects).len(), 1, "a new message wakes it");
        let effects = app.receive(started_on(3), 0.0);
        let texts: Vec<&str> = sent_messages(&effects).iter().map(|(t, _)| *t).collect();
        assert_eq!(texts, vec!["hello", "again"]);
    }

    #[test]
    fn a_session_closing_with_the_app_is_not_started_again() {
        let (mut app, key) = sent_hello();
        app.receive(closed_send(1, "hello", vec![]), 0.0);
        app.close(NOW);
        assert!(opened(&app.receive(exit_of(1, true), 0.0)).is_empty());
        assert!(app.all_ended(), "nothing is left to wait for");
        assert_eq!(session(&app, key).undelivered.len(), 1);
    }

    #[test]
    fn each_message_from_the_backend_reaches_its_handler() {
        let (mut app, _) = App::new(caps(), Width::Unlimited, launch("/p"), NOW);
        let effects = unsaved(app.receive(FromBackend::Account(signed_in()), 0.0));
        assert_eq!(
            effects,
            vec![
                reads_branch("/p"),
                Effect::Open {
                    open: 1,
                    spec: StartSpec::new("/p")
                },
                recent(NOW)
            ]
        );
        assert_eq!(app.screen, Screen::Main);
        let key = app.focused().unwrap();

        let started = FromBackend::Event {
            open: 1,
            event: SessionEvent::Started(info()),
        };
        assert_eq!(
            app.receive(started, 0.0),
            vec![Effect::Send {
                open: 1,
                command: SessionCommand::RefreshUsage
            }]
        );
        app.submit(key, "go");
        app.receive(
            FromBackend::Event {
                open: 1,
                event: SessionEvent::ThinkingStarted,
            },
            2.0,
        );
        let items = session(&app, key).conversation.items();
        assert!(
            matches!(
                items.last().map(|i| &i.body),
                Some(ItemBody::Thinking { started, .. }) if *started == 2.0
            ),
            "an event is stamped with `now`: {items:?}"
        );

        let refused = FromBackend::SendFailed {
            open: 1,
            command: SessionCommand::Answer {
                id: RequestId("r1".into()),
                answer: Answer::Allow { always: false },
            },
            error: AgentError::Protocol("not waiting".into()),
        };
        assert_eq!(app.receive(refused, 0.0), vec![]);
        let last = &session(&app, key).conversation.items().last().unwrap().body;
        assert_eq!(
            *last,
            ItemBody::Notice {
                kind: NoticeKind::CommandFailed,
                text: "That request was no longer waiting.".into()
            }
        );

        let failed = FromBackend::OpenFailed {
            open: 1,
            error: AgentError::NotInstalled,
        };
        assert_eq!(app.receive(failed, 0.0), vec![]);
        assert_eq!(app.screen, Screen::NotInstalled);
        assert!(app.focused().is_none() && app.sessions.is_empty());
    }

    #[test]
    fn the_width_has_a_floor_and_saves_only_changes() {
        let (mut app, _) = ready();
        assert_eq!(
            app.set_width(Width::Points(100.0)),
            vec![Effect::SaveWidth(Width::Points(MIN_WIDTH))]
        );
        assert_eq!(app.set_width(Width::Points(MIN_WIDTH)), vec![]);
        assert_eq!(
            app.set_width(Width::Unlimited),
            vec![Effect::SaveWidth(Width::Unlimited)]
        );
    }

    #[test]
    fn each_session_gets_only_its_own_events() {
        let mut app = main_screen();
        let keys = [
            busy(&mut app, "/a"),
            busy(&mut app, "/b"),
            busy(&mut app, "/c"),
        ];
        let opens: Vec<u64> = keys.iter().map(|key| open_of(&app, *key)).collect();
        assert_eq!(opens, vec![1, 2, 3]);
        let before: Vec<Session> = keys.iter().map(|key| session(&app, *key).clone()).collect();
        app.event(opens[1], SessionEvent::TextDelta("only here".into()), 0.0);
        assert_eq!(session(&app, keys[0]), &before[0]);
        assert_eq!(session(&app, keys[2]), &before[2]);
        let second = session(&app, keys[1]).conversation.items();
        assert_eq!(second.len(), 2);
        assert!(
            matches!(
                &second[1].body,
                ItemBody::Reply { markdown, .. } if markdown == "only here"
            ),
            "{second:?}"
        );
    }

    #[test]
    fn an_event_stamps_its_session_and_an_ended_program_lets_go_of_its_open() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        let b = busy(&mut app, "/b");
        app.now_unix = NOW + 90;
        app.event(open_of(&app, a), SessionEvent::TextDelta("hi".into()), 0.0);
        assert_eq!(app.sessions.get(a).unwrap().last_own_unix, Some(NOW + 90));
        assert_eq!(app.sessions.get(b).unwrap().last_own_unix, Some(NOW));
        app.event(open_of(&app, b), exited(true), 0.0);
        let b = app.sessions.get(b).unwrap();
        assert_eq!((b.session.phase.clone(), b.open), (Phase::Resting, None));
        assert!(!b.live());
    }

    #[test]
    fn a_message_sent_makes_its_session_active_now() {
        let (mut app, key) = ready();
        assert_eq!(app.sessions.get(key).unwrap().last_active_unix, NOW);
        app.now_unix = NOW + 30;
        app.submit(key, "   ");
        assert_eq!(
            app.sessions.get(key).unwrap().last_active_unix,
            NOW,
            "blank"
        );
        app.submit(key, "go");
        assert_eq!(app.sessions.get(key).unwrap().last_active_unix, NOW + 30);
    }

    #[test]
    fn a_stale_open_changes_nothing() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.event(1, exited(false), 0.0);
        app.restart(key);
        assert_eq!(open_of(&app, key), 2);
        let before = session(&app, key).clone();
        for stale in [
            SessionEvent::Started(info()),
            SessionEvent::TextDelta("late".into()),
            done(),
            exited(false),
        ] {
            assert_eq!(app.event(1, stale, 0.0), vec![]);
        }
        assert_eq!(app.open_failed(1, AgentError::Io("x".into())), vec![]);
        let send = SessionCommand::Send {
            text: "go".into(),
            attachments: vec![],
        };
        assert_eq!(
            app.send_failed(1, &send, AgentError::Io("x".into())),
            vec![]
        );
        assert_eq!(session(&app, key), &before);
        assert_eq!(open_of(&app, key), 2);
    }

    #[test]
    fn a_turn_that_ends_out_of_sight_is_marked_finished() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        let b = busy(&mut app, "/b");
        let c = busy(&mut app, "/c");
        app.show(a, Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(mark(&app, b), Mark::Working);
        app.event(open_of(&app, b), done(), 0.0);
        assert_eq!(mark(&app, b), Mark::Finished);
        let failed = SessionEvent::TurnFailed {
            reason: "overloaded".into(),
        };
        app.event(open_of(&app, c), failed, 0.0);
        assert_eq!(mark(&app, c), Mark::Finished, "a failed turn ended too");
        app.event(open_of(&app, a), done(), 0.0);
        assert_eq!(mark(&app, a), Mark::None, "it ended in sight");
        app.show(b, Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(mark(&app, b), Mark::None);
        let b = app.sessions.get(b).unwrap();
        assert!(!b.unseen && b.touched);
    }

    #[test]
    fn a_program_that_dies_out_of_sight_is_marked_finished() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        let b = busy(&mut app, "/b");
        let c = busy(&mut app, "/c");
        app.show(a, Place::Focused, Leaving::ForgetIfEmpty);
        app.event(open_of(&app, b), exited(false), 0.0);
        assert_eq!(mark(&app, b), Mark::Finished);
        app.event(open_of(&app, c), exited(true), 0.0);
        assert_eq!(
            mark(&app, c),
            Mark::None,
            "a clean end is not a turn ending"
        );
    }

    #[test]
    fn a_request_out_of_sight_marks_needs_you() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        let b = busy(&mut app, "/b");
        app.show(a, Place::Focused, Leaving::ForgetIfEmpty);
        app.event(open_of(&app, b), permission("r1"), 0.0);
        assert_eq!(mark(&app, b), Mark::NeedsYou);
        assert_eq!(mark(&app, a), Mark::Working);
        let answer = Answer::Allow { always: false };
        assert_eq!(
            app.answer(b, &RequestId("r1".into()), answer.clone()),
            vec![Effect::Send {
                open: open_of(&app, b),
                command: SessionCommand::Answer {
                    id: RequestId("r1".into()),
                    answer
                }
            }]
        );
        assert_eq!(mark(&app, b), Mark::Working);
    }

    #[test]
    fn an_empty_session_is_forgotten_when_left() {
        let mut app = main_screen();
        let other = app.sessions.add(stored("s9"), NOW - 60);
        app.new_session("/a".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let a = app.focused().unwrap();
        assert_eq!(
            app.show(a, Place::Focused, Leaving::ForgetIfEmpty),
            vec![],
            "showing the shown entry does nothing"
        );
        assert_eq!(
            closes(&app.show(other, Place::Focused, Leaving::ForgetIfEmpty)),
            vec![1]
        );
        assert!(app.sessions.get(a).is_none());
        assert_eq!(app.focused(), Some(other));

        let mut app = main_screen();
        let other = app.sessions.add(stored("s9"), NOW - 60);
        let a = busy(&mut app, "/a");
        assert_eq!(
            closes(&app.show(other, Place::Focused, Leaving::ForgetIfEmpty)),
            Vec::<u64>::new(),
            "a message was sent"
        );
        assert!(app.sessions.get(a).is_some());

        let mut app = main_screen();
        let other = app.sessions.add(stored("s9"), NOW - 60);
        app.new_session("/a".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let a = app.focused().unwrap();
        app.submit(a, "early");
        assert_eq!(mark(&app, a), Mark::Working);
        assert_eq!(
            closes(&app.show(other, Place::Focused, Leaving::ForgetIfEmpty)),
            Vec::<u64>::new(),
            "a message is queued"
        );
        assert!(app.sessions.get(a).is_some());
    }

    #[test]
    fn an_empty_session_told_to_stay_is_kept() {
        let mut app = main_screen();
        let other = app.sessions.add(stored("s9"), NOW - 60);
        app.new_session("/a".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let a = app.focused().unwrap();
        assert_eq!(
            closes(&app.show(other, Place::Focused, Leaving::Keep)),
            Vec::<u64>::new()
        );
        assert!(app.sessions.get(a).is_some(), "kept");
        assert_eq!(
            app.show(a, Place::Focused, Leaving::ForgetIfEmpty),
            vec![reads_branch("/a")],
            "the stored one it takes the place of keeps its history for now"
        );
        // A new session in another folder keeps it too.
        assert_eq!(
            closes(&app.new_session("/b".into(), Place::Focused, Leaving::Keep)),
            Vec::<u64>::new()
        );
        let b = app.focused().unwrap();
        assert!(app.sessions.get(a).is_some(), "kept again");
        // Left later with nothing typed, each goes.
        assert_eq!(
            closes(&app.show(a, Place::Focused, Leaving::ForgetIfEmpty)),
            vec![2]
        );
        assert!(app.sessions.get(b).is_none());
        assert_eq!(
            closes(&app.show(other, Place::Focused, Leaving::ForgetIfEmpty)),
            vec![1]
        );
        assert!(app.sessions.get(a).is_none());
    }

    #[test]
    fn new_session_keeps_an_empty_one_in_the_same_folder() {
        let mut app = main_screen();
        assert_eq!(
            unsaved(app.new_session("/a".into(), Place::Focused, Leaving::ForgetIfEmpty)),
            vec![
                reads_branch("/a"),
                Effect::Open {
                    open: 1,
                    spec: StartSpec::new("/a")
                }
            ]
        );
        let a = app.focused();
        assert_eq!(
            app.new_session("/a".into(), Place::Focused, Leaving::ForgetIfEmpty),
            vec![]
        );
        assert_eq!((app.focused(), app.sessions.len()), (a, 1));
        assert_eq!(
            unsaved(app.new_session("/b".into(), Place::Focused, Leaving::ForgetIfEmpty)),
            vec![
                Effect::Send {
                    open: 1,
                    command: SessionCommand::Close
                },
                reads_branch("/b"),
                Effect::Open {
                    open: 2,
                    spec: StartSpec::new("/b")
                },
            ]
        );
        assert_eq!(app.sessions.len(), 1);
        assert_ne!(app.focused(), a);
        assert_eq!(
            app.focused_entry().unwrap().session.folder,
            std::path::Path::new("/b")
        );
    }

    #[test]
    fn new_session_replaces_an_empty_session_that_ended() {
        let mut app = main_screen();
        app.new_session("/a".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let failed = app.focused().unwrap();
        app.open_failed(1, AgentError::Io("no such folder".into()));
        assert!(matches!(session(&app, failed).phase, Phase::Ended { .. }));
        assert_eq!(
            app.new_session("/a".into(), Place::Focused, Leaving::ForgetIfEmpty),
            vec![
                reads_branch("/a"),
                Effect::Open {
                    open: 2,
                    spec: StartSpec::new("/a")
                }
            ]
        );
        assert!(app.sessions.get(failed).is_none(), "the ended one is gone");
        let fresh = app.focused_entry().unwrap();
        assert_ne!(fresh.key, failed);
        assert_eq!(
            (fresh.session.phase.clone(), fresh.open),
            (Phase::Opening, Some(2))
        );
        assert_eq!(app.sessions.len(), 1);
    }

    /// An app with two busy sessions side by side: tiles `[a, b]`, `a` focused.
    fn two_tiles() -> (App, SessionKey, SessionKey) {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        let b = busy(&mut app, "/b");
        app.show(a, Place::Focused, Leaving::ForgetIfEmpty);
        app.show(b, Place::Side, Leaving::ForgetIfEmpty);
        app.focus_tile(a);
        (app, a, b)
    }

    /// The sessions in the tiles, left to right.
    fn tile_keys(app: &App) -> Vec<SessionKey> {
        app.tiles.keys()
    }

    /// The sidebar's row of `key`'s session.
    fn row_of(app: &App, key: SessionKey) -> &crate::groups::Row {
        app.sidebar
            .sections
            .iter()
            .flat_map(|section| &section.rows)
            .find(|row| row.key == key)
            .expect("listed")
    }

    fn reads_s9(key: SessionKey) -> Effect {
        Effect::ReadHistory {
            key,
            id: id("s9"),
            max_items: HISTORY_ITEMS,
        }
    }

    #[test]
    fn a_click_swaps_the_focused_tiles_session() {
        let (mut app, a, b) = two_tiles();
        let c = app.sessions.add(stored("s9"), NOW - 60);
        assert_eq!(
            app.show(c, Place::Focused, Leaving::ForgetIfEmpty),
            vec![reads_s9(c), reads_branch("/home/alex/project")],
            "no save"
        );
        assert_eq!((tile_keys(&app), app.focused()), (vec![c, b], Some(c)));
        assert!(app.sessions.get(a).is_some(), "a message was sent in it");
    }

    #[test]
    fn to_the_side_adds_a_tile_and_asks_for_a_save() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        let c = app.sessions.add(stored("s9"), NOW - 60);
        let effects = app.show(c, Place::Side, Leaving::ForgetIfEmpty);
        let branch = reads_branch("/home/alex/project");
        assert_eq!(effects, vec![reads_s9(c), branch, save(&app)]);
        assert_eq!((tile_keys(&app), app.focused()), (vec![a, c], Some(c)));
    }

    #[test]
    fn a_session_in_a_tile_is_only_focused() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        let b = app.sessions.add(stored("s9"), NOW - 60);
        app.show(b, Place::Side, Leaving::ForgetIfEmpty);
        app.focus_tile(a);
        assert_eq!(app.focused(), Some(a));
        assert_eq!(
            app.show(b, Place::Focused, Leaving::ForgetIfEmpty),
            vec![reads_branch("/home/alex/project")]
        );
        assert_eq!((tile_keys(&app), app.focused()), (vec![a, b], Some(b)));
        assert_eq!(
            app.show(a, Place::Side, Leaving::ForgetIfEmpty),
            vec![reads_branch("/a")]
        );
        assert_eq!((tile_keys(&app), app.focused()), (vec![a, b], Some(a)));
    }

    #[test]
    fn a_turn_that_ends_in_an_unfocused_tile_is_marked_finished() {
        let (mut app, a, b) = two_tiles();
        app.event(open_of(&app, a), done(), 0.0);
        assert_eq!(mark(&app, a), Mark::None, "it ended in the focused tile");
        app.event(open_of(&app, b), done(), 0.0);
        assert_eq!(mark(&app, b), Mark::Finished, "though it is in a tile");
        assert_eq!(app.focus_tile(b), vec![reads_branch("/b")], "no save");
        assert_eq!(mark(&app, b), Mark::None);
    }

    #[test]
    fn sending_in_a_tile_focuses_it() {
        type Intent = fn(&mut App, SessionKey) -> Vec<Effect>;
        let intents: [(&str, Intent); 7] = [
            ("submit", |app, key| app.submit(key, "hi")),
            ("restart", |app, key| app.restart(key)),
            ("answer_prompt", |app, key| {
                app.answer_prompt(key, PromptChoice::ContinueHere).0
            }),
            ("stop", |app, key| app.stop(key)),
            ("answer", |app, key| {
                let allow = Answer::Allow { always: false };
                app.answer(key, &RequestId("r1".into()), allow)
            }),
            ("model", |app, key| app.choose_model(key, "default")),
            ("mode", |app, key| {
                app.choose_mode(key, PermissionMode::Plan)
            }),
        ];
        for (name, intent) in intents {
            let (mut app, a, b) = two_tiles();
            intent(&mut app, b);
            assert_eq!(
                (tile_keys(&app), app.focused()),
                (vec![a, b], Some(b)),
                "{name}"
            );
        }
    }

    #[test]
    fn choosing_an_effort_sends_it_to_that_session_only() {
        let (mut app, a, b) = two_tiles();
        for key in [a, b] {
            let mut entry = app.sessions.get_mut(key).unwrap();
            entry.session.models[1].effort_levels = vec!["low".into(), "high".into()];
        }
        let before = session(&app, a).clone();
        assert_eq!(
            app.choose_effort(b, "high"),
            vec![Effect::Send {
                open: open_of(&app, b),
                command: SessionCommand::SetEffort("high".into())
            }]
        );
        assert_eq!(
            (tile_keys(&app), app.focused()),
            (vec![a, b], Some(a)),
            "the focus does not move"
        );
        assert_eq!(session(&app, a), &before);
        assert_eq!(app.choose_effort(b, "max"), vec![], "not a level it takes");
        assert_eq!(app.choose_effort(SessionKey(99), "high"), vec![]);
    }

    #[test]
    fn switching_remote_control_sends_to_that_session_only() {
        let (mut app, a, b) = two_tiles();
        let _ = app.settle();
        let (before, sidebar) = (session(&app, a).clone(), app.sidebar_version);
        assert_eq!(
            app.set_remote(b, true),
            vec![Effect::Send {
                open: open_of(&app, b),
                command: SessionCommand::SetRemoteAccess { on: true }
            }]
        );
        assert!(session(&app, b).remote_wanted);
        assert_eq!(
            (tile_keys(&app), app.focused()),
            (vec![a, b], Some(a)),
            "the focus does not move"
        );
        assert_eq!(session(&app, a), &before);
        let open = open_of(&app, b);
        app.event(
            open,
            SessionEvent::RemoteAccess(RemoteAccess::Starting),
            0.0,
        );
        let _ = app.settle();
        assert_eq!(app.sidebar_version, sidebar, "no sidebar row shows it");
        assert_eq!(app.set_remote(SessionKey(99), true), vec![]);
    }

    #[test]
    fn a_new_session_wants_remote_control_when_the_preference_says_so() {
        let mut app = main_screen();
        assert!(!app.remote_for_new);
        app.set_remote_for_new(true);
        app.new_session("/p".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let key = app.focused().unwrap();
        assert!(session(&app, key).remote_wanted);
        let open = open_of(&app, key);
        let effects = app.event(open, SessionEvent::Started(info()), 0.0);
        let turn_on = Effect::Send {
            open,
            command: SessionCommand::SetRemoteAccess { on: true },
        };
        assert!(effects.contains(&turn_on), "{effects:?}");

        app.set_remote_for_new(false);
        app.new_session("/q".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let key = app.focused().unwrap();
        assert!(!session(&app, key).remote_wanted);
        let open = open_of(&app, key);
        let effects = app.event(open, SessionEvent::Started(info()), 0.0);
        assert!(
            !effects.iter().any(|effect| matches!(
                effect,
                Effect::Send {
                    command: SessionCommand::SetRemoteAccess { .. },
                    ..
                }
            )),
            "{effects:?}"
        );
    }

    #[test]
    fn a_stored_session_and_a_copy_do_not_get_the_preference() {
        let mut app = main_screen();
        app.set_remote_for_new(true);
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 3600);
        listing(&mut app, Some(NOW - RECENT_DAYS * 86_400), vec![a1]);
        let key = by_id(&app, "a1");
        app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        assert!(!session(&app, key).remote_wanted, "a stored session");

        let (mut app, key, _) = checked(5);
        app.set_remote_for_new(true);
        app.answer_prompt(key, PromptChoice::ContinueAsCopy);
        let copy = app.focused().unwrap();
        assert_ne!(copy, key);
        assert!(!session(&app, copy).remote_wanted, "a copy");
        let effects = app.event(1, SessionEvent::Started(info()), 0.0);
        assert!(
            !effects.iter().any(|effect| matches!(
                effect,
                Effect::Send {
                    command: SessionCommand::SetRemoteAccess { .. },
                    ..
                }
            )),
            "{effects:?}"
        );
    }

    #[test]
    fn the_preference_is_saved_and_read_back() {
        let mut app = main_screen();
        let effects = app.set_remote_for_new(true);
        assert!(app.remote_for_new);
        assert_eq!(effects, vec![save(&app)]);
        assert!(app.saved().remote_for_new);
        assert!(restored(app.saved()).remote_for_new);
        assert_eq!(app.set_remote_for_new(true), vec![], "nothing changed");
        let effects = app.set_remote_for_new(false);
        assert_eq!(effects, vec![save(&app)]);
        assert!(!app.saved().remote_for_new);
        assert!(!restored(app.saved()).remote_for_new);
        assert!(!restored(Saved::default()).remote_for_new);
    }

    #[test]
    fn closing_a_tile_keeps_its_session_running() {
        let (mut app, a, b) = two_tiles();
        let effects = app.close_tile(b, Leaving::ForgetIfEmpty);
        assert_eq!(effects, vec![save(&app)], "no Close");
        assert_eq!((tile_keys(&app), app.focused()), (vec![a], Some(a)));
        assert_eq!(session(&app, b).phase, Phase::Working);
        let _ = app.settle();
        assert!(!row_of(&app, b).shown && row_of(&app, a).shown);
        assert_eq!(app.close_tile(b, Leaving::ForgetIfEmpty), vec![], "no tile");
    }

    #[test]
    fn closing_a_tile_forgets_an_empty_new_session() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        let b = app.focused().unwrap();
        let effects = app.close_tile(b, Leaving::ForgetIfEmpty);
        assert_eq!(closes(&effects), vec![2]);
        assert_eq!(effects.last(), Some(&save(&app)));
        assert!(app.sessions.get(b).is_none());
        assert_eq!((tile_keys(&app), app.focused()), (vec![a], Some(a)));

        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        let b = app.focused().unwrap();
        let effects = app.close_tile(b, Leaving::Keep);
        assert_eq!(effects, vec![reads_branch("/a"), save(&app)]);
        assert!(app.sessions.get(b).is_some(), "text is typed in it");
        assert_eq!(tile_keys(&app), vec![a]);
    }

    #[test]
    fn the_session_swapped_out_is_forgotten_only_when_it_is_in_no_tile() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        let b = app.focused().unwrap();
        assert_eq!(
            app.show(a, Place::Focused, Leaving::ForgetIfEmpty),
            vec![reads_branch("/a")]
        );
        assert!(app.sessions.get(b).is_some(), "it is still in its tile");
        app.focus_tile(b);
        let c = app.sessions.add(stored("s9"), NOW - 60);
        let effects = app.show(c, Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(closes(&effects), vec![2]);
        assert!(app.sessions.get(b).is_none());
        assert_eq!(tile_keys(&app), vec![a, c]);
    }

    #[test]
    fn a_session_that_could_not_open_leaves_its_tile() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        assert_eq!(tile_keys(&app).len(), 2);
        app.open_failed(2, AgentError::NotInstalled);
        let _ = app.settle();
        assert_eq!((tile_keys(&app), app.focused()), (vec![a], Some(a)));
    }

    #[test]
    fn a_copy_takes_its_originals_tile() {
        let (mut app, key, _) = checked(5);
        let a = app.sessions.add(stored("s9"), NOW - 60);
        app.show(a, Place::At(0), Leaving::ForgetIfEmpty);
        assert_eq!((tile_keys(&app), app.focused()), (vec![a, key], Some(a)));
        app.answer_prompt(key, PromptChoice::ContinueAsCopy);
        let copy = app.focused().unwrap();
        assert_ne!(copy, key);
        assert_eq!(tile_keys(&app), vec![a, copy]);
        assert_eq!(session(&app, key).phase, Phase::Resting);
    }

    #[test]
    fn a_copy_of_a_session_in_no_tile_is_shown_in_the_focused_one() {
        let (mut app, key, _) = checked(5);
        let other = app.sessions.add(stored("s9"), NOW - 60);
        app.show(other, Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(tile_keys(&app), vec![other], "the original is in no tile");
        app.answer_prompt(key, PromptChoice::ContinueAsCopy);
        let copy = app.focused().unwrap();
        assert!(copy != key && copy != other);
        assert_eq!(tile_keys(&app), vec![copy]);
        assert!(app.sessions.get(other).is_some(), "it is stored");
    }

    #[test]
    fn a_session_gone_from_the_store_leaves_its_tile() {
        let (mut app, a, b) = two_tiles();
        app.sessions.remove(b);
        let _ = app.settle();
        assert_eq!((tile_keys(&app), app.focused()), (vec![a], Some(a)));

        let (mut app, a, b) = two_tiles();
        app.sessions.remove(a);
        let _ = app.settle();
        assert_eq!((tile_keys(&app), app.focused()), (vec![b], Some(b)));
    }

    #[test]
    fn without_room_side_swaps_the_focused_tile() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        assert!(app.room_for_tile(), "the row's width is not known");
        app.set_row_width(800.0);
        assert_eq!(app.row_width(), Some(800.0));
        assert!(!app.room_for_tile());
        let b = app.sessions.add(stored("s9"), NOW - 60);
        assert_eq!(
            app.show(b, Place::Side, Leaving::ForgetIfEmpty),
            vec![reads_s9(b), reads_branch("/home/alex/project")]
        );
        assert_eq!((tile_keys(&app), app.focused()), (vec![b], Some(b)));
        assert!(app.sessions.get(a).is_some());
        app.set_row_width(840.0);
        assert!(app.room_for_tile());
    }

    #[test]
    fn the_sidebar_is_rebuilt_when_the_focus_moves() {
        let (mut app, a, b) = two_tiles();
        let _ = app.settle();
        let version = app.sidebar_version;
        assert!(row_of(&app, a).focused && !row_of(&app, b).focused);
        app.focus_tile(b);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 1);
        assert!(!row_of(&app, a).focused && row_of(&app, b).focused);
        assert!(row_of(&app, a).shown && row_of(&app, b).shown);
        app.focus_tile(b);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 1, "it had the focus");
    }

    fn reads_branch(folder: &str) -> Effect {
        Effect::ReadBranch {
            folder: folder.into(),
        }
    }

    /// The folders whose branch `effects` ask for, in order.
    fn branch_asks(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::ReadBranch { folder } => folder.to_str(),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_branch_is_asked_for_when_the_focus_comes_and_when_a_turn_ends() {
        let (mut app, a, b) = two_tiles();
        assert_eq!(branch_asks(&app.focus_tile(b)), ["/b"]);
        assert!(branch_asks(&app.focus_tile(b)).is_empty(), "it had it");
        let effects = app.event(open_of(&app, b), done(), 0.0);
        assert_eq!(branch_asks(&effects), ["/b"], "a turn ended there");
        let effects = app.event(open_of(&app, a), done(), 0.0);
        assert!(branch_asks(&effects).is_empty(), "in another tile");
        app.submit(b, "again");
        let failed = SessionEvent::TurnFailed {
            reason: "it broke".into(),
        };
        let effects = app.event(open_of(&app, b), failed, 0.0);
        assert_eq!(branch_asks(&effects), ["/b"], "a turn failed there");
        let effects = app.event(open_of(&app, b), exited(false), 0.0);
        assert_eq!(branch_asks(&effects), ["/b"], "its program died");
        let effects = app.close_tile(b, Leaving::Keep);
        assert_eq!(branch_asks(&effects), ["/a"], "the focus went to a");
        let c = app.sessions.add(stored("s9"), NOW - 60);
        let effects = app.show(c, Place::Side, Leaving::ForgetIfEmpty);
        assert_eq!(branch_asks(&effects), ["/home/alex/project"]);
        let effects = app.move_tile(a, Spot::Edge(2));
        assert_eq!(branch_asks(&effects), ["/a"], "a moved tile takes it");
        let effects = app.new_session("/c".into(), Place::Focused, Leaving::Keep);
        assert_eq!(branch_asks(&effects), ["/c"], "a session made there");

        // An intent on a session in another tile focuses it.
        type Intent = fn(&mut App, SessionKey) -> Vec<Effect>;
        let intents: [(&str, Intent); 7] = [
            ("submit", |app, key| app.submit(key, "hi")),
            ("restart", |app, key| app.restart(key)),
            ("answer_prompt", |app, key| {
                app.answer_prompt(key, PromptChoice::ContinueHere).0
            }),
            ("stop", |app, key| app.stop(key)),
            ("answer", |app, key| {
                let allow = Answer::Allow { always: false };
                app.answer(key, &RequestId("r1".into()), allow)
            }),
            ("model", |app, key| app.choose_model(key, "default")),
            ("mode", |app, key| {
                app.choose_mode(key, PermissionMode::Plan)
            }),
        ];
        for (name, intent) in intents {
            let (mut app, _, b) = two_tiles();
            let effects = intent(&mut app, b);
            assert_eq!(branch_asks(&effects), ["/b"], "{name}");
        }
        // The focused session's tile goes when its program is not installed.
        let (mut app, a, _) = two_tiles();
        let effects = app.open_failed(open_of(&app, a), AgentError::NotInstalled);
        assert_eq!(branch_asks(&effects), ["/b"], "not installed");
    }

    #[test]
    fn the_branch_is_asked_for_the_session_focused_at_start() {
        let saved = Saved {
            version: SAVED_VERSION,
            shown: Some(id("a2")),
            ..Saved::default()
        };
        let mut app = restored(saved);
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        assert_eq!(branch_asks(&effects), ["/home/alex/project"]);

        let (mut app, _) = App::new(caps(), Width::Unlimited, launch("/p"), NOW);
        let effects = app.account_checked(signed_in());
        assert_eq!(branch_asks(&effects), ["/p"], "the launch folder");
    }

    #[test]
    fn two_sessions_of_one_folder_share_the_answer() {
        let mut app = main_screen();
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        let (a1, a2, b1) = (by_id(&app, "a1"), by_id(&app, "a2"), by_id(&app, "b1"));
        let answer = |folder: &str, branch: &Branch| FromBackend::Branch {
            folder: folder.into(),
            branch: Some(branch.clone()),
        };
        let main = Branch::Named("main".into());
        // None of them is focused: an answer is kept for its folder all the same.
        let effects = app.receive(answer("/home/alex/project", &main), 0.0);
        assert_eq!(effects, vec![]);
        let branches = |app: &App| [a1, a2, b1].map(|key| app.branch_of(key).cloned());
        let shared = Some(main.clone());
        assert_eq!(branches(&app), [shared.clone(), shared, None]);
        let detached = Branch::Detached("0123456".into());
        app.receive(answer("/home/alex/project", &detached), 0.0);
        app.receive(answer("/home/alex/notes", &main), 0.0);
        assert_eq!(
            branches(&app),
            [Some(detached.clone()), Some(detached), Some(main)]
        );
    }

    #[test]
    fn an_answer_of_none_takes_the_branch_away() {
        let (mut app, key) = ready();
        let main = Branch::Named("main".into());
        let answer = |folder: &str, branch: Option<Branch>| FromBackend::Branch {
            folder: folder.into(),
            branch,
        };
        app.receive(answer("/p", Some(main.clone())), 0.0);
        assert_eq!(app.branch_of(key), Some(&main));
        app.receive(answer("/q", None), 0.0);
        assert_eq!(app.branch_of(key), Some(&main), "another folder's answer");
        app.receive(answer("/p", None), 0.0);
        assert_eq!(app.branch_of(key), None);
    }

    #[test]
    fn resizing_asks_for_no_save_and_evening_out_does() {
        let (mut app, a, b) = two_tiles();
        app.resize_tile(a, 900.0);
        assert_eq!(shares(&app), vec![1.0, 1.0], "the row's width is not known");
        app.set_row_width(1500.0);
        app.resize_tile(a, 900.0);
        assert_near(&app.tiles.widths(1500.0), &[900.0, 600.0]);
        app.resize_tile(b, 900.0);
        assert_near(&app.tiles.widths(1500.0), &[900.0, 600.0]);
        assert_eq!(saves(&app.even_tiles()), 1);
        assert_near(&app.tiles.widths(1500.0), &[750.0, 750.0]);
        assert_eq!(app.even_tiles(), Vec::new(), "even already");
    }

    #[test]
    fn resizing_a_tile_does_not_rebuild_the_sidebar() {
        let (mut app, a, b) = two_tiles();
        app.set_row_width(1500.0);
        let _ = app.settle();
        let version = app.sidebar_version;
        app.resize_tile(a, 900.0);
        let _ = app.settle();
        app.even_tiles();
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version);
        app.focus_tile(b);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 1);
    }

    #[test]
    fn moving_a_tile_focuses_it_and_asks_for_a_save() {
        let (mut app, a, b) = two_tiles();
        let effects = app.move_tile(b, Spot::Edge(0));
        assert_eq!(effects, vec![reads_branch("/b"), save(&app)]);
        assert_eq!((tile_keys(&app), app.focused()), (vec![b, a], Some(b)));
    }

    #[test]
    fn a_move_of_nothing_asks_for_nothing() {
        let (mut app, a, b) = two_tiles();
        for edge in [0, 1] {
            assert_eq!(app.move_tile(a, Spot::Edge(edge)), vec![], "edge {edge}");
        }
        assert_eq!(
            app.move_tile(SessionKey(99), Spot::Edge(0)),
            vec![],
            "no tile"
        );
        assert_eq!((tile_keys(&app), app.focused()), (vec![a, b], Some(a)));
    }

    #[test]
    fn a_moved_tile_counts_as_seen() {
        let (mut app, _, b) = two_tiles();
        app.event(open_of(&app, b), done(), 0.0);
        assert_eq!(mark(&app, b), Mark::Finished);
        app.move_tile(b, Spot::Edge(0));
        assert_ne!(mark(&app, b), Mark::Finished);
    }

    #[test]
    fn the_new_order_is_saved() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let [a1, a2, b1] = ["a1", "a2", "b1"].map(|text| by_id(&app, text));
        app.show(a1, Place::Focused, Leaving::ForgetIfEmpty);
        app.show(a2, Place::Side, Leaving::ForgetIfEmpty);
        app.show(b1, Place::Side, Leaving::ForgetIfEmpty);
        // As in `the_arrangement_is_written_and_read_back`: shares of 2, 1 and 1, scaled.
        assert!(app.tiles.resize(a1, 1500.0, 3000.0));
        assert!(app.tiles.resize(a2, 750.0, 3000.0));
        assert_near(&shares(&app), &[1.5, 0.75, 0.75]);
        app.move_tile(a1, Spot::Edge(3));
        let saved = app.saved();
        assert_eq!(
            saved.tiles,
            vec![tile("a2", 0.75), tile("b1", 0.75), tile("a1", 1.5)]
        );
        assert_eq!(saved.shown, Some(id("a1")));
    }

    /// The tiles column by column, each column's upper tile first.
    fn tile_columns(app: &App) -> Vec<Vec<SessionKey>> {
        app.tiles
            .columns()
            .iter()
            .map(|column| {
                std::iter::once(column.top)
                    .chain(column.under.map(|under| under.key))
                    .collect()
            })
            .collect()
    }

    /// `[a/c][b]`, `c` focused, from [`two_tiles`]: `c` is the stored `s9`.
    fn stacked_tiles() -> (App, SessionKey, SessionKey, SessionKey) {
        let (mut app, a, b) = two_tiles();
        let c = app.sessions.add(stored("s9"), NOW - 60);
        app.show(c, Place::Below(a), Leaving::ForgetIfEmpty);
        (app, a, b, c)
    }

    #[test]
    fn a_session_opens_under_the_focused_tile() {
        let (mut app, a, b) = two_tiles();
        app.set_row_height(900.0);
        assert_eq!(app.row_height(), Some(900.0));
        let c = app.sessions.add(stored("s9"), NOW - 60);
        let effects = app.show(c, Place::Below(a), Leaving::ForgetIfEmpty);
        let branch = reads_branch("/home/alex/project");
        assert_eq!(effects, vec![reads_s9(c), branch, save(&app)]);
        assert_eq!(
            (tile_columns(&app), app.focused()),
            (vec![vec![a, c], vec![b]], Some(c))
        );
    }

    #[test]
    fn no_tile_opens_under_a_tile_of_a_low_row() {
        let (mut app, a, b) = two_tiles();
        let version = app.tiles.version();
        app.set_row_height(500.0);
        assert_eq!(app.tiles.version(), version, "the tiles stay as they are");
        assert_eq!(
            app.row_size(),
            RowSize {
                width: None,
                height: Some(500.0)
            }
        );
        let c = app.sessions.add(stored("s9"), NOW - 60);
        assert!(!app.can_put(Spot::Below(a)));
        for place in [Place::Below(a), Place::Above(a)] {
            assert_eq!(
                app.show(c, place, Leaving::ForgetIfEmpty),
                vec![],
                "{place:?}"
            );
            assert_eq!(app.tiles.version(), version, "{place:?}");
        }
        assert_eq!(
            (tile_columns(&app), app.focused()),
            (vec![vec![a], vec![b]], Some(a))
        );
        assert!(!app.sessions.get(c).unwrap().touched, "it was not shown");
    }

    #[test]
    fn can_put_and_can_move_say_what_show_and_move_do() {
        // A row with room for neither a column nor a split, then one with room for both.
        let sizes = [(1000.0, 500.0), (1300.0, 900.0)];
        let spots = |a, b, c| {
            [
                Spot::Edge(0),
                Spot::Edge(1),
                Spot::Edge(2),
                Spot::Edge(5),
                Spot::Above(a),
                Spot::Below(a),
                Spot::Above(b),
                Spot::Below(b),
                Spot::Above(c),
                Spot::Below(c),
                Spot::Below(SessionKey(99)),
            ]
        };
        let setup = |(width, height)| {
            let (mut app, a, b, c) = stacked_tiles();
            app.set_row_width(width);
            app.set_row_height(height);
            (app, a, b, c)
        };
        // How many could not and could, of the puts and of the moves.
        let (mut puts, mut moves) = ([0; 2], [0; 2]);
        for size in sizes {
            let (_, a, b, c) = setup(size);
            for spot in spots(a, b, c) {
                let (mut app, ..) = setup(size);
                let can = app.can_put(spot);
                let place = match spot {
                    Spot::Edge(edge) => Place::At(edge),
                    Spot::Above(key) => Place::Above(key),
                    Spot::Below(key) => Place::Below(key),
                };
                let d = app.sessions.add(stored("s8"), NOW - 60);
                app.show(d, place, Leaving::ForgetIfEmpty);
                assert_eq!(can, app.tiles.contains(d), "{size:?} {spot:?}");
                puts[usize::from(can)] += 1;
                for key in [a, b, c] {
                    let (mut app, ..) = setup(size);
                    let can = app.can_move_tile(key, spot);
                    let before = app.tiles.clone();
                    let effects = app.move_tile(key, spot);
                    assert_eq!(can, !effects.is_empty(), "{size:?} {key:?} {spot:?}");
                    assert_eq!(can, app.tiles != before, "{size:?} {key:?} {spot:?}");
                    moves[usize::from(can)] += 1;
                }
            }
        }
        assert!(
            puts.iter().chain(&moves).all(|&n| n > 0),
            "{puts:?} {moves:?}"
        );
    }

    #[test]
    fn moving_a_tile_into_a_column_asks_for_a_save() {
        let (mut app, a, b) = two_tiles();
        let effects = app.move_tile(b, Spot::Below(a));
        assert_eq!(effects, vec![reads_branch("/b"), save(&app)]);
        assert_eq!(
            (tile_columns(&app), app.focused()),
            (vec![vec![a, b]], Some(b))
        );
        let effects = app.move_tile(b, Spot::Edge(1));
        assert_eq!(effects, vec![save(&app)]);
        assert_eq!(
            (tile_columns(&app), app.focused()),
            (vec![vec![a], vec![b]], Some(b))
        );
    }

    #[test]
    fn a_tile_moved_into_a_column_counts_as_seen() {
        let (mut app, a, b) = two_tiles();
        app.event(open_of(&app, b), done(), 0.0);
        assert_eq!(mark(&app, b), Mark::Finished);
        app.move_tile(b, Spot::Above(a));
        assert_eq!(tile_columns(&app), vec![vec![b, a]]);
        assert_ne!(mark(&app, b), Mark::Finished);
    }

    #[test]
    fn closing_the_focused_lower_tile_focuses_the_upper() {
        let (mut app, a, b, c) = stacked_tiles();
        app.event(open_of(&app, a), done(), 0.0);
        assert_eq!(mark(&app, a), Mark::Finished, "c has the focus");
        let effects = app.close_tile(c, Leaving::Keep);
        assert_eq!(effects, vec![reads_branch("/a"), save(&app)]);
        assert_eq!(
            (tile_columns(&app), app.focused()),
            (vec![vec![a], vec![b]], Some(a))
        );
        assert_eq!(mark(&app, a), Mark::Finished, "the user did not act on it");
    }

    #[test]
    fn the_split_is_resized_without_a_save_and_evened_with_one() {
        let (mut app, a, _, c) = stacked_tiles();
        assert_eq!(app.tile_split(a), Some(0.5));
        app.resize_split(a, 300.0);
        assert_eq!(
            app.tile_split(a),
            Some(0.5),
            "the row's height is not known"
        );
        app.set_row_height(900.0);
        app.resize_split(a, 300.0);
        assert_near(&[app.tile_split(a).unwrap()], &[1.0 / 3.0]);
        app.resize_split(a, 100.0);
        assert_near(&[app.tile_split(a).unwrap()], &[280.0 / 900.0]);
        app.put_split(a, 0.25);
        assert_eq!(app.tile_split(a), Some(0.25));
        assert_eq!(app.tile_split(c), None, "the lower tile has no split");
        let effects = app.even_split(a);
        assert_eq!(effects, vec![save(&app)]);
        assert_eq!(app.tile_split(a), Some(0.5));
    }

    #[test]
    fn evening_an_even_split_asks_for_nothing() {
        let (mut app, a, b, c) = stacked_tiles();
        for key in [a, b, c, SessionKey(99)] {
            assert_eq!(app.even_split(key), vec![], "{key:?}");
        }
        assert_eq!(app.tile_split(a), Some(0.5));
    }

    #[test]
    fn the_old_session_keeps_working() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        assert_eq!(session(&app, a).phase, Phase::Working);
        assert_eq!(
            unsaved(app.new_session("/b".into(), Place::Focused, Leaving::ForgetIfEmpty)),
            vec![
                reads_branch("/b"),
                Effect::Open {
                    open: 2,
                    spec: StartSpec::new("/b")
                }
            ]
        );
        let a = app.sessions.get(a).unwrap();
        assert_eq!((a.session.phase.clone(), a.open), (Phase::Working, Some(1)));
    }

    #[test]
    fn close_ends_every_program() {
        let mut app = main_screen();
        let live = [
            busy(&mut app, "/a"),
            busy(&mut app, "/b"),
            busy(&mut app, "/c"),
        ];
        let resting = busy(&mut app, "/d");
        app.event(open_of(&app, resting), exited(true), 0.0);
        assert_eq!(session(&app, resting).phase, Phase::Resting);
        let opens: Vec<u64> = live.iter().map(|key| open_of(&app, *key)).collect();
        let mut closed = closes(&app.close(NOW));
        closed.sort_unstable();
        assert_eq!(closed, opens);
        for open in opens {
            assert!(!app.all_ended());
            app.event(open, exited(true), 0.0);
        }
        assert!(app.all_ended());
    }

    #[test]
    fn intents_for_an_unknown_key_do_nothing() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.event(1, permission("r1"), 0.0);
        let before = session(&app, key).clone();
        let unknown = SessionKey(99);
        assert_eq!(
            app.show(unknown, Place::Focused, Leaving::ForgetIfEmpty),
            vec![]
        );
        assert_eq!(app.focused(), Some(key));
        assert_eq!(app.submit(unknown, "more"), vec![]);
        assert_eq!(app.stop(unknown), vec![]);
        let allow = Answer::Allow { always: false };
        assert_eq!(app.answer(unknown, &RequestId("r1".into()), allow), vec![]);
        assert_eq!(app.choose_model(unknown, "default"), vec![]);
        assert_eq!(app.choose_mode(unknown, PermissionMode::Plan), vec![]);
        assert_eq!(app.restart(unknown), vec![]);
        assert_eq!(session(&app, key), &before);
    }

    #[test]
    fn signing_in_lists_the_recent_sessions_then_all() {
        let (mut app, _) = App::new(caps(), Width::Unlimited, Launch::default(), NOW);
        assert_eq!(app.listing, Listing::NotAsked);
        assert_eq!(
            app.account_checked(signed_in()),
            vec![Effect::ListSessions {
                since_unix: Some(NOW - 2_592_000)
            }]
        );
        assert_eq!(app.listing, Listing::Loading);
        assert_eq!(
            listing(&mut app, Some(NOW - 2_592_000), vec![]),
            vec![Effect::ListSessions { since_unix: None }]
        );
        assert_eq!(app.listing, Listing::Done);
        assert!(!app.listed_all);
        assert_eq!(listing(&mut app, None, vec![]), vec![]);
        assert!(app.listed_all);
        assert_eq!(
            listing(&mut app, Some(NOW - 2_592_000), vec![]),
            vec![],
            "every stored session is asked for once"
        );
    }

    #[test]
    fn a_listing_that_fails_keeps_the_entries() {
        let mut app = main_screen();
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 60);
        listing(&mut app, Some(NOW - 2_592_000), vec![a1]);
        let failed = app.receive(
            FromBackend::Sessions {
                since_unix: None,
                result: Err(AgentError::Io("denied".into())),
            },
            0.0,
        );
        assert_eq!(failed, vec![]);
        assert_eq!(app.listing, Listing::Failed("i/o error: denied".into()));
        assert!(!app.listed_all);
        assert_eq!(app.sessions.len(), 1);
        assert_eq!(
            listing(&mut app, Some(NOW - 2_592_000), vec![]),
            vec![Effect::ListSessions { since_unix: None }],
            "a listing of all that failed is asked for again"
        );
        assert_eq!(app.listing, Listing::Done);
    }

    #[test]
    fn a_listing_adds_resting_entries() {
        let mut app = main_screen();
        listing(
            &mut app,
            Some(NOW - 2_592_000),
            vec![
                store("a1", "Fix the parser", "/home/alex/project", NOW - 60),
                store("a2", "Write the docs", "/home/alex/docs", NOW - 7200),
            ],
        );
        assert_eq!(app.sessions.len(), 2);
        for (text, title, unix) in [
            ("a1", "Fix the parser", NOW - 60),
            ("a2", "Write the docs", NOW - 7200),
        ] {
            let entry = app.sessions.get(by_id(&app, text)).unwrap();
            assert_eq!(entry.session.phase, Phase::Resting, "{text}");
            assert_eq!(entry.session.title.as_deref(), Some(title));
            assert_eq!(entry.last_active_unix, unix);
            assert_eq!(entry.session.history, HistoryState::NotLoaded);
            assert!(entry.open.is_none() && !entry.touched);
        }
        listing(
            &mut app,
            Some(NOW - 2_592_000),
            vec![store("a1", "Fix the lexer", "/home/alex/project", NOW - 5)],
        );
        assert_eq!(app.sessions.len(), 2, "nothing is added twice");
        let entry = app.sessions.get(by_id(&app, "a1")).unwrap();
        assert_eq!(entry.session.title.as_deref(), Some("Fix the lexer"));
        assert_eq!(entry.last_active_unix, NOW - 5);
    }

    #[test]
    fn a_listing_does_not_disturb_a_live_session() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.event(1, SessionEvent::TextDelta("hi".into()), 0.0);
        let before = session(&app, key).clone();
        listing(
            &mut app,
            None,
            vec![store("s1", "Old title", "/p", NOW - 3600)],
        );
        assert_eq!(app.sessions.len(), 1);
        assert_eq!(session(&app, key), &before);
        assert_eq!(app.sessions.get(key).unwrap().last_active_unix, NOW);
    }

    #[test]
    fn a_complete_listing_drops_what_is_gone() {
        let mut app = main_screen();
        listing(
            &mut app,
            Some(NOW - 2_592_000),
            vec![
                store("a1", "Fix the parser", "/home/alex/project", NOW - 60),
                store("a2", "Write the docs", "/home/alex/docs", NOW - 120),
            ],
        );
        let (untouched, touched) = (by_id(&app, "a1"), by_id(&app, "a2"));
        app.show(touched, Place::Focused, Leaving::ForgetIfEmpty);
        let live = busy(&mut app, "/a");
        // The store still has another session: a complete listing with none at all is not
        // believed (see `an_empty_full_listing_forgets_no_deletion`).
        let other = store("z9", "Sort the notes", "/home/alex/notes", NOW - 30);
        listing(&mut app, None, vec![other]);
        assert!(app.sessions.get(untouched).is_none());
        assert!(app.sessions.get(touched).is_some());
        assert!(app.sessions.get(live).is_some());
        listing(&mut app, Some(NOW - 2_592_000), vec![]);
        assert!(
            app.sessions.get(touched).is_some(),
            "a recent listing removes nothing"
        );
    }

    #[test]
    fn a_listing_before_its_program_started_makes_no_second_entry() {
        let mut app = main_screen();
        app.new_session("/p".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let key = app.focused().unwrap();
        app.submit(key, "go");
        listing(&mut app, None, vec![store("s1", "Go", "/p", NOW)]);
        assert_eq!(app.sessions.len(), 2, "the listing came first");
        app.event(1, SessionEvent::Started(info()), 0.0);
        assert_eq!(app.sessions.len(), 1);
        assert_eq!(app.sessions.by_id(&id("s1")), Some(key));
    }

    #[test]
    fn showing_a_stored_session_reads_its_history_once() {
        let mut app = main_screen();
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 60);
        listing(&mut app, Some(NOW - 2_592_000), vec![a1]);
        let key = by_id(&app, "a1");
        assert_eq!(
            app.show(key, Place::Focused, Leaving::ForgetIfEmpty),
            vec![
                Effect::ReadHistory {
                    key,
                    id: id("a1"),
                    max_items: HISTORY_ITEMS
                },
                reads_branch("/home/alex/project")
            ]
        );
        assert_eq!(session(&app, key).history, HistoryState::Loading);
        let reads = |effects: &[Effect]| {
            effects
                .iter()
                .filter(|e| matches!(e, Effect::ReadHistory { .. }))
                .count()
        };
        // Beside it, so that the stored session stays in its tile.
        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        assert_eq!(
            reads(&app.show(key, Place::Focused, Leaving::ForgetIfEmpty)),
            0,
            "while it loads"
        );
        let history = StoredHistory {
            items: vec![HistoryItem::User {
                text: "read the parser".into(),
                image_count: 0,
                files: Vec::new(),
                images: Vec::new(),
                stored: None,
            }],
            ..StoredHistory::default()
        };
        let answer = |key| FromBackend::History {
            key,
            result: Ok(history.clone()),
            pictures: Vec::new(),
        };
        assert_eq!(app.receive(answer(key), 0.0), vec![]);
        assert_eq!(session(&app, key).history, HistoryState::Loaded);
        assert_eq!(session(&app, key).conversation.items().len(), 1);
        app.new_session("/c".into(), Place::Side, Leaving::ForgetIfEmpty);
        assert_eq!(
            reads(&app.show(key, Place::Focused, Leaving::ForgetIfEmpty)),
            0,
            "once it is loaded"
        );
        let gone = answer(SessionKey(99));
        assert_eq!(app.receive(gone, 0.0), vec![], "for a key that is gone");
    }

    #[test]
    fn a_picture_for_nobody_is_dropped() {
        let (mut app, key) = shows_stored();
        let before = session(&app, key).clone();
        let thumb = crate::attach::Thumb {
            width: 1,
            height: 1,
            rgba: vec![0u8; 4].into(),
        };
        for thumb in [None, Some(thumb)] {
            let picture = FromBackend::Picture {
                id: 1 << 63 | 41,
                thumb,
            };
            assert_eq!(app.receive(picture, 0.0), vec![]);
        }
        assert_eq!(session(&app, key), &before);
    }

    #[test]
    fn a_history_that_fails_is_shown_as_failed() {
        let mut app = main_screen();
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 60);
        listing(&mut app, Some(NOW - 2_592_000), vec![a1]);
        let key = by_id(&app, "a1");
        app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        app.receive(
            FromBackend::History {
                key,
                result: Err(AgentError::Io("gone".into())),
                pictures: Vec::new(),
            },
            0.0,
        );
        assert_eq!(
            session(&app, key).history,
            HistoryState::Failed("i/o error: gone".into())
        );
    }

    #[test]
    fn a_message_to_a_resting_session_checks_then_opens() {
        let (mut app, key, effects) = checked(3600);
        assert_eq!(
            effects,
            vec![Effect::Open {
                open: 1,
                spec: resume_a1()
            }]
        );
        let entry = app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, Phase::Opening);
        assert_eq!(queued_text(&entry.session), Some("carry on"));
        assert_eq!(entry.last_active_unix, NOW);
        let effects = app.event(1, SessionEvent::Started(info()), 0.0);
        assert!(sends_text(&effects, 1, "carry on"), "{effects:?}");
        assert_eq!(app.prompt(key), None);
        for answer in [Ok(None), Err(AgentError::Io("gone".into()))] {
            let (mut app, key) = shows_stored();
            app.submit(key, "carry on");
            let effects = app.receive(
                FromBackend::Activity {
                    key,
                    result: answer.clone(),
                },
                0.0,
            );
            assert_eq!(
                effects,
                vec![Effect::Open {
                    open: 1,
                    spec: resume_a1()
                }],
                "{answer:?}"
            );
        }
    }

    #[test]
    fn a_session_active_elsewhere_asks_first() {
        let (mut app, key, effects) = checked(5);
        assert_eq!(effects, vec![]);
        let entry = app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, Phase::Resting);
        assert_eq!((entry.session.queued.clone(), entry.open), (None, None));
        let prompt = Prompt::OpenElsewhere {
            text: "carry on".into(),
        };
        assert_eq!(app.prompt(key), Some(&prompt));
        assert!(!app.takes_message(key));
        assert_eq!(app.submit(key, "more"), vec![], "one question at a time");
        assert_eq!(app.prompt(key), Some(&prompt));
    }

    #[test]
    fn a_question_holds_up_only_its_own_session() {
        let (mut app, key, _) = checked(5);
        let a2 = store("a2", "Write the docs", "/home/alex/docs", NOW - 7200);
        listing(&mut app, None, vec![a2]);
        let other = by_id(&app, "a2");
        assert!(app.takes_message(other), "another session takes a message");
        assert_eq!(
            app.submit(other, "hello"),
            vec![Effect::CheckActivity {
                key: other,
                id: id("a2")
            }]
        );
        assert_eq!(queued_text(session(&app, other)), Some("hello"));
        assert!(app.prompt(key).is_some(), "the first question still waits");
    }

    #[test]
    fn checks_run_for_several_sessions_at_once() {
        let (mut app, key) = shows_stored();
        let a2 = store("a2", "Write the docs", "/home/alex/docs", NOW - 7200);
        listing(&mut app, None, vec![a2]);
        let other = by_id(&app, "a2");
        app.submit(key, "carry on");
        assert!(!app.takes_message(key), "its own check runs");
        assert!(app.takes_message(other));
        assert_eq!(
            app.submit(other, "hello"),
            vec![Effect::CheckActivity {
                key: other,
                id: id("a2")
            }]
        );
        let effects = app.receive(
            FromBackend::Activity {
                key,
                result: Ok(None),
            },
            0.0,
        );
        assert_eq!(
            effects,
            vec![Effect::Open {
                open: 1,
                spec: resume_a1()
            }]
        );
        assert!(app.takes_message(key), "it is waking: a message is queued");
        assert!(!app.takes_message(other), "its check still runs");
    }

    #[test]
    fn two_questions_are_answered_each_on_its_own() {
        let (mut app, key, _) = checked(5);
        let a2 = store("a2", "Write the docs", "/home/alex/docs", NOW - 7200);
        listing(&mut app, None, vec![a2]);
        let other = by_id(&app, "a2");
        app.submit(other, "hello");
        app.receive(
            FromBackend::Activity {
                key: other,
                result: Ok(Some(NOW - 10)),
            },
            0.0,
        );
        let asked = |text: &str| {
            Some(Prompt::OpenElsewhere {
                text: text.to_string(),
            })
        };
        assert_eq!(app.prompt(key).cloned(), asked("carry on"));
        assert_eq!(app.prompt(other).cloned(), asked("hello"));
        let (effects, back) = app.answer_prompt(other, PromptChoice::Cancel);
        assert_eq!((effects, back.as_deref()), (vec![], Some("hello")));
        assert_eq!(app.prompt(other), None);
        assert_eq!(app.prompt(key).cloned(), asked("carry on"), "untouched");
        let (effects, back) = app.answer_prompt(key, PromptChoice::ContinueHere);
        assert_eq!(back, None);
        assert_eq!(
            effects,
            vec![Effect::Open {
                open: 1,
                spec: resume_a1()
            }]
        );
        assert_eq!(app.prompt(key), None);
        assert_eq!(session(&app, other).phase, Phase::Resting);
    }

    #[test]
    fn a_question_out_of_sight_marks_its_row() {
        let (mut app, key) = shows_stored();
        let a2 = store("a2", "Write the docs", "/home/alex/docs", NOW - 7200);
        listing(&mut app, None, vec![a2]);
        let other = by_id(&app, "a2");
        app.submit(key, "carry on");
        // The user looks elsewhere before the store answers.
        app.show(other, Place::Focused, Leaving::ForgetIfEmpty);
        let project = GroupId::Folder("/home/alex/project".into());
        app.collapse_group(&project, true);
        let _ = app.settle();
        let version = app.sidebar_version;
        app.receive(
            FromBackend::Activity {
                key,
                result: Ok(Some(NOW - 5)),
            },
            0.0,
        );
        assert_eq!(mark(&app, key), Mark::NeedsYou);
        let _ = app.settle();
        assert!(app.sidebar_version > version, "the sidebar is rebuilt");
        let section = app
            .sidebar
            .sections
            .iter()
            .find(|section| section.id == project)
            .expect("its group is listed");
        assert_eq!(
            section.mark,
            Mark::NeedsYou,
            "the collapsed header shows it"
        );
        app.hide_group(&project, true);
        let _ = app.settle();
        assert!(
            app.sidebar
                .sections
                .iter()
                .any(|section| section.id == project),
            "a hidden group is listed while the question waits"
        );
        app.answer_prompt(key, PromptChoice::Cancel);
        assert_eq!(mark(&app, key), Mark::None);
        let _ = app.settle();
        assert!(
            !app.sidebar
                .sections
                .iter()
                .any(|section| section.id == project),
            "and hidden again once it is answered"
        );
    }

    #[test]
    fn continue_here_opens_it() {
        let (mut app, key, _) = checked(5);
        let (effects, back) = app.answer_prompt(key, PromptChoice::ContinueHere);
        assert_eq!(
            effects,
            vec![Effect::Open {
                open: 1,
                spec: resume_a1()
            }]
        );
        assert_eq!((back, app.prompt(key)), (None, None));
        assert_eq!(queued_text(session(&app, key)), Some("carry on"));
        let effects = app.event(1, SessionEvent::Started(info()), 0.0);
        assert!(sends_text(&effects, 1, "carry on"), "{effects:?}");
    }

    #[test]
    fn cancel_gives_the_text_back() {
        let (mut app, key, _) = checked(5);
        let (effects, back) = app.answer_prompt(key, PromptChoice::Cancel);
        assert_eq!(effects, vec![]);
        assert_eq!(back.as_deref(), Some("carry on"));
        assert_eq!(app.prompt(key), None);
        let entry = app.sessions.get(key).unwrap();
        assert_eq!(
            (entry.session.phase.clone(), entry.open),
            (Phase::Resting, None)
        );
        assert_eq!(
            app.answer_prompt(key, PromptChoice::Cancel),
            (vec![], None),
            "no question waits"
        );
        assert!(app.takes_message(key));
    }

    #[test]
    fn continue_as_a_copy_forks() {
        let (mut app, key, _) = checked(5);
        let before = session(&app, key).conversation.items().to_vec();
        assert_eq!(before.len(), 2);
        let (effects, back) = app.answer_prompt(key, PromptChoice::ContinueAsCopy);
        assert_eq!((back, app.prompt(key)), (None, None));
        let mut spec = resume_a1();
        spec.fork = true;
        let branch = reads_branch("/home/alex/project");
        assert_eq!(effects, vec![branch, Effect::Open { open: 1, spec }]);
        let copy = app.focused().unwrap();
        assert_ne!(copy, key);
        assert_eq!(app.sessions.len(), 2);
        let (original, forked) = (session(&app, key), session(&app, copy));
        assert_eq!(forked.title.as_deref(), Some("Fix the parser"));
        assert_eq!(forked.folder, original.folder);
        assert_eq!(forked.conversation.items(), &before[..]);
        // The original, in no tile now, keeps its conversation until the tick lets it go.
        assert_eq!(original.conversation.items(), &before[..]);
        assert_eq!(forked.phase, Phase::Opening);
        assert_eq!(queued_text(forked), Some("carry on"));
        assert_eq!(original.phase, Phase::Resting);
        assert_eq!(original.queued, None);
        assert_eq!(app.sessions.get(key).unwrap().open, None);
        let effects = app.event(1, SessionEvent::Started(info()), 0.0);
        assert!(sends_text(&effects, 1, "carry on"), "{effects:?}");
        assert_eq!(app.sessions.by_id(&id("a1")), Some(key));
    }

    #[test]
    fn a_session_this_app_ran_is_not_checked() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.event(1, done(), 0.0);
        app.event(1, exited(true), 0.0);
        assert_eq!(session(&app, key).phase, Phase::Resting);
        app.now_unix = NOW + 30;
        let mut spec = StartSpec::new("/p");
        spec.resume = Some(id("s1"));
        spec.model = Some("claude-haiku-5-5".into());
        assert_eq!(
            app.submit(key, "more"),
            vec![Effect::Open { open: 2, spec }]
        );
        let entry = app.sessions.get(key).unwrap();
        assert_eq!(queued_text(&entry.session), Some("more"));
        assert_eq!(entry.last_active_unix, NOW + 30);
    }

    /// The saved state an app left that ran `s1` until it closed at `NOW` and knew `a2`, which
    /// it did not run.
    fn ran_and_closed() -> Saved {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.event(1, done(), 0.0);
        let unran = store("a2", "Write the docs", "/home/alex/docs", NOW - 10);
        listing(&mut app, None, vec![unran]);
        assert_eq!(session(&app, key).id, Some(id("s1")), "its program's id");
        let Some(Effect::SaveState(saved)) = app.close(NOW).into_iter().next() else {
            panic!("closing saves first");
        };
        *saved
    }

    /// A second run started `later` seconds after the first closed, with what it saved: `s1`
    /// is listed, shown, and sent a message; the store then says it changed at `changed`.
    fn reopened(saved: Saved, later: i64, changed: i64) -> (App, SessionKey) {
        let launch = Launch {
            saved,
            ..Launch::default()
        };
        let (mut app, _) = App::new(caps(), Width::Unlimited, launch, NOW + later);
        app.account_checked(signed_in());
        let s1 = store("s1", "Fix the parser", "/p", changed);
        listing(&mut app, None, vec![s1]);
        let key = by_id(&app, "s1");
        app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(
            app.submit(key, "more"),
            vec![Effect::CheckActivity { key, id: id("s1") }]
        );
        app.receive(
            FromBackend::Activity {
                key,
                result: Ok(Some(changed)),
            },
            0.0,
        );
        (app, key)
    }

    #[test]
    fn closing_keeps_the_sessions_this_run_ran() {
        let saved = ran_and_closed();
        assert_eq!(saved.closed_by_app, vec![(id("s1"), NOW)], "not a2");
        // Closed later than the clock was last told: the closing time is the one given.
        let (mut app, key) = ready();
        app.submit(key, "go");
        let Some(Effect::SaveState(saved)) = app.close(NOW + 25).into_iter().next() else {
            panic!("closing saves first");
        };
        assert_eq!(saved.closed_by_app, vec![(id("s1"), NOW + 25)]);
    }

    #[test]
    fn this_apps_own_last_run_does_not_ask() {
        // Its program's last writes come a moment after the window closed.
        let (app, key) = reopened(ran_and_closed(), 30, NOW + OWN_EXIT_SLACK_SECONDS);
        assert_eq!(app.prompt(key), None);
        assert_eq!(session(&app, key).phase, Phase::Opening, "it opens");
        assert!(app.sessions.get(key).unwrap().open.is_some());
    }

    #[test]
    fn a_change_after_this_app_closed_asks() {
        let (app, key) = reopened(ran_and_closed(), 30, NOW + OWN_EXIT_SLACK_SECONDS + 1);
        assert!(app.prompt(key).is_some());
        assert_eq!(session(&app, key).phase, Phase::Resting);
    }

    #[test]
    fn only_recent_closes_are_kept() {
        let window = ELSEWHERE_SECONDS + OWN_EXIT_SLACK_SECONDS;
        for (later, kept) in [(window, true), (window + 1, false)] {
            let launch = Launch {
                saved: ran_and_closed(),
                ..Launch::default()
            };
            let (app, _) = App::new(caps(), Width::Unlimited, launch, NOW + later);
            let expected = if kept { vec![(id("s1"), NOW)] } else { vec![] };
            assert_eq!(app.saved().closed_by_app, expected, "{later} s later");
        }
        // A second close keeps the first run's that are still recent, and its own anew.
        let (mut app, key) = reopened(ran_and_closed(), 60, NOW + 5);
        app.event(open_of(&app, key), SessionEvent::Started(info()), 0.0);
        let Some(Effect::SaveState(saved)) = app.close(NOW + 60).into_iter().next() else {
            panic!("closing saves first");
        };
        assert_eq!(saved.closed_by_app, vec![(id("s1"), NOW + 60)]);
    }

    #[test]
    fn the_listing_is_asked_again_every_minute() {
        let mut app = main_screen();
        assert_eq!(app.tick(NOW + 59), vec![]);
        assert_eq!(app.now_unix, NOW + 59);
        assert_eq!(app.tick(NOW + 60), vec![recent(NOW + 60)]);
        assert_eq!(app.tick(NOW + 119), vec![]);
        assert_eq!(app.tick(NOW + 120), vec![recent(NOW + 120)]);
        let (mut app, _) = App::new(caps(), Width::Unlimited, Launch::default(), NOW);
        assert_eq!(app.tick(NOW + 600), vec![], "not before signing in");
    }

    #[test]
    fn a_look_fills_the_models_and_the_limits() {
        let (mut app, _) = App::new(caps(), Width::Unlimited, looking(), NOW);
        assert_eq!(app.account_checked(signed_in()), vec![recent(NOW)]);
        let stored = vec![
            store("a2", "Write the docs", "/home/alex/docs", NOW - 7200),
            store("a1", "Fix the parser", "/home/alex/project", NOW - 60),
        ];
        let effects = listing(&mut app, Some(NOW - 2_592_000), stored.clone());
        assert_eq!(
            effects,
            vec![
                Effect::ListSessions { since_unix: None },
                Effect::Look {
                    folder: "/home/alex".into()
                }
            ],
            "nothing restored: in the home folder, never the newest entry's"
        );
        assert_eq!(listing(&mut app, None, stored), vec![], "one look");
        let limits = UsageLimits {
            session: window(0.3),
            weekly: None,
        };
        let look = FromBackend::Look {
            models: info().models,
            model: Some("claude-fable-5-2".into()),
            usage: Some(limits.clone()),
        };
        assert_eq!(app.receive(look, 0.0), vec![]);
        assert_eq!(app.known_models, info().models);
        assert_eq!(app.usage, limits);
        let key = by_id(&app, "a2");
        assert_eq!(session(&app, key).models, info().models, "offered to all");

        let (mut app, _) = App::new(caps(), Width::Unlimited, looking(), NOW);
        app.account_checked(signed_in());
        assert_eq!(
            listing(&mut app, Some(NOW - 2_592_000), vec![]),
            vec![
                Effect::ListSessions { since_unix: None },
                Effect::Look {
                    folder: "/home/alex".into()
                }
            ],
            "with no entry, in the home folder"
        );
    }

    #[test]
    fn a_started_program_tells_every_session_its_models() {
        let mut app = main_screen();
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 60);
        listing(&mut app, Some(NOW - 2_592_000), vec![a1]);
        let resting = by_id(&app, "a1");
        busy(&mut app, "/a");
        assert_eq!(app.known_models, info().models);
        assert_eq!(session(&app, resting).models, info().models);
        let a3 = store("a3", "Tidy up", "/home/alex/project", NOW - 60);
        listing(&mut app, None, vec![a3]);
        let later = by_id(&app, "a3");
        assert!(session(&app, later).models.is_empty());
        app.show(later, Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(session(&app, later).models, info().models, "when shown");
    }

    #[test]
    fn no_look_with_a_launch_folder() {
        let launch = Launch {
            folder: Some("/p".into()),
            ..looking()
        };
        let (mut app, _) = App::new(caps(), Width::Unlimited, launch, NOW);
        app.account_checked(signed_in());
        assert_eq!(
            listing(&mut app, Some(NOW - 2_592_000), vec![]),
            vec![Effect::ListSessions { since_unix: None }]
        );
        let (mut app, _) = App::new(caps(), Width::Unlimited, Launch::default(), NOW);
        app.account_checked(signed_in());
        assert_eq!(
            listing(&mut app, Some(NOW - 2_592_000), vec![]),
            vec![Effect::ListSessions { since_unix: None }],
            "nor without `look`"
        );
    }

    #[test]
    fn usage_is_asked_again_after_a_reset() {
        let (mut app, _) = ready();
        let limits = |resets_at_unix| {
            SessionEvent::UsageLimits(UsageLimits {
                session: Some(LimitWindow {
                    used: 0.9,
                    resets_at_unix,
                }),
                weekly: None,
            })
        };
        app.event(1, limits(NOW + 10_000), 0.0);
        assert_eq!(usage_asks(app.tick(NOW + 900)), vec![], "not reset yet");
        app.event(1, limits(NOW + 10), 0.0);
        let refresh = vec![Effect::Send {
            open: 1,
            command: SessionCommand::RefreshUsage,
        }];
        assert_eq!(usage_asks(app.tick(NOW + 1000)), refresh);
        assert_eq!(usage_asks(app.tick(NOW + 1300)), vec![]);
        assert_eq!(usage_asks(app.tick(NOW + 1301)), vec![], "asked once");
        assert_eq!(usage_asks(app.tick(NOW + 9000)), vec![]);
        // Its program reports a new window: when that one has reset, it is asked about.
        app.event(1, limits(NOW + 9500), 0.0);
        assert_eq!(usage_asks(app.tick(NOW + 9499)), vec![]);
        assert_eq!(usage_asks(app.tick(NOW + 9500)), refresh);
        assert_eq!(usage_asks(app.tick(NOW + 9801)), vec![]);

        let (mut app, _) = App::new(caps(), Width::Unlimited, looking(), NOW);
        app.account_checked(signed_in());
        listing(&mut app, Some(NOW - 2_592_000), vec![]);
        let look = FromBackend::Look {
            models: Vec::new(),
            model: None,
            usage: Some(UsageLimits {
                session: window(0.9),
                weekly: None,
            }),
        };
        app.receive(look, 0.0);
        assert_eq!(usage_asks(app.tick(NOW + 300)), vec![], "the look asked");
        let look = vec![Effect::Look {
            folder: "/home/alex".into(),
        }];
        assert_eq!(usage_asks(app.tick(NOW + 301)), look);
        assert_eq!(usage_asks(app.tick(NOW + 302)), vec![]);
        assert_eq!(usage_asks(app.tick(NOW + 601)), vec![]);
        assert_eq!(usage_asks(app.tick(NOW + 602)), vec![], "asked once");
    }

    const DAY: i64 = 86_400;

    fn group_of(app: &App, key: SessionKey) -> GroupId {
        app.groups.group_of(app.sessions.get(key).unwrap())
    }

    #[test]
    fn settle_rebuilds_only_after_a_change() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        let _ = app.settle();
        let (version, model) = (app.sidebar_version, app.sidebar.clone());
        assert_eq!(app.sidebar.sections.len(), 1);
        assert_eq!(app.sidebar.sections[0].mark, Mark::Working);
        assert!(app.sidebar.loading, "the first listing is on its way");
        let _ = app.settle();
        assert_eq!((app.sidebar_version, &app.sidebar), (version, &model));
        app.now_unix += 60;
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version, "no entry crossed the line");

        app.event(1, permission("r1"), 0.0);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 1);
        assert_ne!(app.sidebar, model);
        assert_eq!(app.sidebar.sections[0].mark, Mark::NeedsYou);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 1);

        let notes = store("b1", "Tidy the notes", "/home/alex/notes", NOW - 29 * DAY);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![notes]);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 2);
        assert!(!app.sidebar.loading);
        assert_eq!(app.sidebar.sections.len(), 2);

        app.now_unix = NOW + 2 * DAY;
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 3, "b1 is old now");
        assert_eq!(app.sidebar.sections.len(), 1);
        assert_eq!(app.sidebar.older_groups, 1);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 3);

        app.toggle_older_groups();
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 4, "a group changed");
        app.show(by_id(&app, "b1"), Place::Focused, Leaving::ForgetIfEmpty);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 5, "another session is shown");
        let rows = (&app.sidebar.sections[0].rows, &app.sidebar.sections[1].rows);
        assert_eq!((rows.0[0].key, rows.0[0].shown), (key, false));
        assert!(rows.1[0].shown);
    }

    #[test]
    fn a_new_session_in_a_group_is_placed_there() {
        let mut app = main_screen();
        let (side, _) = app.new_group("Side projects");
        let effects = unsaved(app.new_session_in(
            "/home/alex/notes".into(),
            &side,
            Place::Focused,
            Leaving::ForgetIfEmpty,
        ));
        assert_eq!(
            effects,
            vec![
                reads_branch("/home/alex/notes"),
                Effect::Open {
                    open: 1,
                    spec: StartSpec::new("/home/alex/notes")
                }
            ]
        );
        let key = app.focused().unwrap();
        let _ = app.settle();
        let section = &app.sidebar.sections[0];
        assert_eq!(section.id, side);
        assert_eq!(section.rows.len(), 1);
        let row = &section.rows[0];
        assert_eq!(
            (row.key, row.folder_label.as_deref(), row.shown),
            (key, Some("notes"), true)
        );
        app.event(1, SessionEvent::Started(info()), 0.0);
        assert_eq!(group_of(&app, key), side, "its id came after it was placed");

        let home = GroupId::Folder("/home/alex/rune_claude".into());
        app.new_session_in(
            "/home/alex/rune_claude".into(),
            &home,
            Place::Focused,
            Leaving::ForgetIfEmpty,
        );
        let other = app.focused().unwrap();
        assert_ne!(other, key);
        assert_eq!(group_of(&app, other), home);
    }

    #[test]
    fn a_copy_is_placed_where_the_original_is() {
        let (mut app, key, _) = checked(5);
        let (side, _) = app.new_group("Side projects");
        app.move_session(key, &side);
        app.answer_prompt(key, PromptChoice::ContinueAsCopy);
        let copy = app.focused().unwrap();
        assert_ne!(copy, key);
        assert_eq!(group_of(&app, copy), side);
        app.event(1, SessionEvent::Started(info()), 0.0);
        assert_eq!(
            (group_of(&app, copy), group_of(&app, key)),
            (side.clone(), side)
        );
    }

    #[test]
    fn the_group_intents_change_the_sidebar() {
        let (mut app, key) = shows_stored();
        let home = GroupId::Folder("/home/alex/project".into());
        let _ = app.settle();
        assert_eq!(app.sidebar.sections[0].name, "project");
        assert_eq!(app.rename_group(&home, "Parser"), vec![save(&app)]);
        assert_eq!(app.collapse_group(&home, true), vec![save(&app)]);
        let _ = app.settle();
        let section = &app.sidebar.sections[0];
        assert_eq!((section.name.as_str(), section.collapsed), ("Parser", true));
        assert_eq!(app.ungroup(&home), vec![save(&app)]);
        let _ = app.settle();
        let section = &app.sidebar.sections[0];
        assert_eq!(
            (section.name.as_str(), section.collapsed),
            ("project", false)
        );

        let (side, _) = app.new_group("Side projects");
        app.move_session(key, &side);
        assert_eq!(app.hide_group(&side, true), vec![save(&app)]);
        let _ = app.settle();
        assert_eq!(
            app.sidebar.sections[0].id, side,
            "the shown session keeps it listed"
        );
        app.new_session(
            "/home/alex/notes".into(),
            Place::Focused,
            Leaving::ForgetIfEmpty,
        );
        let _ = app.settle();
        assert_eq!(
            app.sidebar.hidden,
            vec![(side.clone(), "Side projects".to_string())]
        );
        assert_eq!(app.hide_group(&side, false), vec![save(&app)]);
        assert_eq!(app.show_older(&side), vec![]);
        assert_eq!(app.toggle_older_groups(), vec![]);
        let _ = app.settle();
        assert!(app.sidebar.older_groups_open);
        assert!(app.sidebar.hidden.is_empty());
    }

    #[test]
    fn streamed_text_rebuilds_nothing() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        let _ = app.settle();
        let version = app.sidebar_version;
        for event in [
            SessionEvent::TurnStarted,
            SessionEvent::ThinkingStarted,
            SessionEvent::ThinkingDelta("Reading".into()),
            SessionEvent::BlockFinished,
            SessionEvent::TextDelta("Hello".into()),
            SessionEvent::TextDelta(", alex".into()),
            SessionEvent::BlockFinished,
        ] {
            app.event(1, event, 0.0);
            let _ = app.settle();
        }
        assert_eq!(
            app.sidebar_version, version,
            "nothing the sidebar shows changed"
        );
        assert_eq!(app.sidebar.sections[0].mark, Mark::Working);
        app.event(1, done(), 0.0);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 1);
        assert_eq!(app.sidebar.sections[0].mark, Mark::None);
    }

    #[test]
    fn a_placement_follows_a_new_id() {
        let (mut app, key) = shows_stored();
        let (side, _) = app.new_group("Side projects");
        app.move_session(key, &side);
        app.submit(key, "carry on");
        let effects = app.receive(
            FromBackend::Activity {
                key,
                result: Ok(Some(NOW - 3600)),
            },
            0.0,
        );
        assert_eq!(
            effects,
            vec![Effect::Open {
                open: 1,
                spec: resume_a1()
            }]
        );
        // The program says the id is `s1`, not `a1`.
        app.event(1, SessionEvent::Started(info()), 0.0);
        assert_eq!(session(&app, key).id, Some(id("s1")));
        assert_eq!(group_of(&app, key), side);
    }

    /// The effect that writes `app`'s state as it is now.
    fn save(app: &App) -> Effect {
        Effect::SaveState(Box::new(app.saved()))
    }

    fn saves(effects: &[Effect]) -> usize {
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::SaveState(_)))
            .count()
    }

    /// An app with no launch folder, started from `saved` and signed in.
    fn restored(saved: Saved) -> App {
        restored_with(Launch::default(), saved)
    }

    fn restored_with(launch: Launch, saved: Saved) -> App {
        let launch = Launch { saved, ..launch };
        let (mut app, _) = App::new(caps(), Width::Unlimited, launch, NOW);
        app.account_checked(signed_in());
        app
    }

    /// `a1` and `a2` in `/home/alex/project`, `b1` in `/home/alex/notes`, all recent.
    fn three_stored() -> Vec<StoredSession> {
        vec![
            store("a1", "Fix the parser", "/home/alex/project", NOW - 60),
            store("a2", "Write the docs", "/home/alex/project", NOW - 120),
            store("b1", "Tidy the notes", "/home/alex/notes", NOW - 180),
        ]
    }

    /// The recent listing, then the complete one, both giving `stored`.
    fn list_both(app: &mut App, stored: Vec<StoredSession>) {
        listing(app, Some(NOW - RECENT_DAYS * DAY), stored.clone());
        listing(app, None, stored);
    }

    /// The sidebar's sections as ids, names, collapsed states and the agent's ids of their rows.
    fn layout(app: &App) -> Vec<(GroupId, String, bool, Vec<SessionId>)> {
        app.sidebar
            .sections
            .iter()
            .map(|section| {
                let ids = section
                    .rows
                    .iter()
                    .map(|row| session(app, row.key).id.clone().unwrap())
                    .collect();
                (
                    section.id.clone(),
                    section.name.clone(),
                    section.collapsed,
                    ids,
                )
            })
            .collect()
    }

    fn project() -> GroupId {
        GroupId::Folder("/home/alex/project".into())
    }

    fn notes() -> GroupId {
        GroupId::Folder("/home/alex/notes".into())
    }

    #[test]
    fn groups_come_back_as_they_were() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        app.rename_group(&project(), "Parser");
        let (side, _) = app.new_group("Side projects");
        let (gone, _) = app.new_group("Gone soon");
        app.ungroup(&gone);
        app.move_session(by_id(&app, "a2"), &side);
        app.collapse_group(&notes(), true);
        let _ = app.settle();
        let before = layout(&app);
        assert_eq!(
            before,
            vec![
                (project(), "Parser".into(), false, vec![id("a1")]),
                (side.clone(), "Side projects".into(), false, vec![id("a2")]),
                (notes(), "notes".into(), true, vec![id("b1")]),
            ]
        );

        let mut again = restored(app.saved());
        list_both(&mut again, three_stored());
        let _ = again.settle();
        assert_eq!(layout(&again), before);
        assert_eq!(again.saved(), app.saved());
        let (newer, _) = again.new_group("Later");
        assert_eq!(
            (side, newer),
            (GroupId::Custom(1), GroupId::Custom(3)),
            "a deleted group's number is not given again"
        );
    }

    #[test]
    fn only_groups_with_something_to_remember_are_saved() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let docs = GroupId::Folder("/home/alex/docs".into());
        app.rename_group(&project(), "Parser");
        app.rename_group(&project(), " ");
        app.collapse_group(&notes(), true);
        app.collapse_group(&notes(), false);
        app.show_older(&notes());
        app.toggle_older_groups();
        app.hide_group(&docs, true);
        let (side, _) = app.new_group("");
        let saved = app.saved();
        assert_eq!(
            saved.groups,
            vec![
                Group {
                    id: side,
                    name: Some("New group".into()),
                    collapsed: false,
                    hidden: false,
                    deleted_unix: None,
                },
                Group {
                    id: docs,
                    name: None,
                    collapsed: false,
                    hidden: true,
                    deleted_unix: None,
                },
            ]
        );
        assert_eq!((saved.version, saved.next_custom), (SAVED_VERSION, 2));
        assert!(saved.placed.is_empty());
    }

    #[test]
    fn a_placement_waits_for_the_complete_listing() {
        let side = GroupId::Custom(1);
        let saved = Saved {
            version: SAVED_VERSION,
            groups: vec![Group {
                id: side.clone(),
                name: Some("Side projects".into()),
                collapsed: false,
                hidden: false,
                deleted_unix: None,
            }],
            placed: vec![
                (id("a1"), side.clone()),
                (id("b1"), project()),
                (id("c1"), GroupId::Custom(7)),
                (id("gone"), side.clone()),
            ],
            next_custom: 8,
            ..Saved::default()
        };
        let mut app = restored(saved);
        assert_eq!(
            app.saved().placed,
            vec![
                (id("a1"), side.clone()),
                (id("b1"), project()),
                (id("gone"), side.clone()),
            ],
            "a placement into a custom group that is gone is dropped at once"
        );
        let old = store("a1", "Fix the parser", "/home/alex/project", NOW - 90 * DAY);
        let b1 = store("b1", "Tidy the notes", "/home/alex/notes", NOW - 60);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![b1.clone()]);
        assert_eq!(group_of(&app, by_id(&app, "b1")), project());
        assert_eq!(
            app.saved().placed.len(),
            3,
            "kept while the listing is not complete"
        );
        // A store that could not be read is not an empty store.
        app.receive(
            FromBackend::Sessions {
                since_unix: None,
                result: Err(AgentError::Io(
                    "/home/alex/.store: permission denied".into(),
                )),
            },
            0.0,
        );
        assert!(!app.listed_all);
        assert_eq!(app.saved().placed.len(), 3, "kept after a failed listing");
        listing(&mut app, None, vec![old, b1]);
        assert_eq!(group_of(&app, by_id(&app, "a1")), side);
        assert_eq!(
            app.saved().placed,
            vec![(id("a1"), side), (id("b1"), project())],
            "missing from both listings"
        );
    }

    #[test]
    fn the_last_shown_session_is_restored_without_a_program() {
        let saved = Saved {
            version: SAVED_VERSION,
            shown: Some(id("a2")),
            ..Saved::default()
        };
        let mut app = restored(saved.clone());
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        let key = by_id(&app, "a2");
        assert_eq!(app.focused(), Some(key));
        assert_eq!(
            effects,
            vec![
                Effect::ListSessions { since_unix: None },
                Effect::ReadHistory {
                    key,
                    id: id("a2"),
                    max_items: HISTORY_ITEMS
                },
                reads_branch("/home/alex/project"),
            ]
        );
        assert_eq!(session(&app, key).phase, Phase::Resting);
        assert_eq!(app.saved().shown, Some(id("a2")));

        let mut app = restored(saved.clone());
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![]);
        assert_eq!(effects, vec![Effect::ListSessions { since_unix: None }]);
        assert_eq!(app.focused(), None);
        assert_eq!(app.saved().shown, Some(id("a2")), "it may still come");
        let old = store("a2", "Write the docs", "/home/alex/docs", NOW - 90 * DAY);
        let effects = listing(&mut app, None, vec![old]);
        let key = by_id(&app, "a2");
        assert_eq!(
            app.focused(),
            Some(key),
            "an old one comes with the complete listing"
        );
        assert!(!effects.iter().any(|e| matches!(e, Effect::Open { .. })));

        let mut elsewhere = three_stored();
        elsewhere[1].folder = "/home/alex/docs".into();
        let mut app = restored_with(looking(), saved);
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), elsewhere);
        assert_eq!(
            effects.last(),
            Some(&Effect::Look {
                folder: "/home/alex/docs".into()
            }),
            "the restored session's folder, not the newest entry's"
        );
    }

    #[test]
    fn the_usage_look_runs_where_the_first_look_did() {
        let reset = |app: &mut App| {
            let look = FromBackend::Look {
                models: Vec::new(),
                model: None,
                usage: Some(UsageLimits {
                    session: window(0.9),
                    weekly: None,
                }),
            };
            app.receive(look, 0.0);
            usage_asks(app.tick(NOW + 301))
        };
        // Nothing restored: at home, though the store knows newer sessions elsewhere.
        let mut app = restored_with(looking(), Saved::default());
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        let home = Effect::Look {
            folder: "/home/alex".into(),
        };
        assert_eq!(effects.last(), Some(&home));
        assert_eq!(reset(&mut app), vec![home]);
        // Restored: in its folder, both times.
        let saved = Saved {
            version: SAVED_VERSION,
            shown: Some(id("b1")),
            ..Saved::default()
        };
        let mut app = restored_with(looking(), saved);
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        let notes = Effect::Look {
            folder: "/home/alex/notes".into(),
        };
        assert_eq!(effects.last(), Some(&notes));
        assert_eq!(reset(&mut app), vec![notes]);
    }

    #[test]
    fn a_shown_session_that_is_gone_restores_nothing() {
        let saved = Saved {
            version: SAVED_VERSION,
            shown: Some(id("gone")),
            ..Saved::default()
        };
        let mut app = restored(saved);
        list_both(&mut app, three_stored());
        assert_eq!(app.focused(), None);
        assert_eq!(app.saved().shown, None);
        let back = store("gone", "Back again", "/home/alex/notes", NOW - 10);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![back]);
        assert_eq!(app.focused(), None, "not tried after the complete listing");
    }

    fn tile(text: &str, share: f32) -> SavedTile {
        SavedTile {
            id: id(text),
            share,
            below: None,
        }
    }

    /// A state with `tiles` and `shown` focused, as a run that closed wrote it.
    fn with_tiles(tiles: Vec<SavedTile>, shown: &str) -> Saved {
        Saved {
            version: SAVED_VERSION,
            shown: Some(id(shown)),
            tiles,
            next_custom: 1,
            ..Saved::default()
        }
    }

    fn shares(app: &App) -> Vec<f32> {
        app.tiles
            .columns()
            .iter()
            .map(|column| column.share)
            .collect()
    }

    fn assert_near(found: &[f32], expected: &[f32]) {
        assert_eq!(found.len(), expected.len(), "{found:?}");
        for (found_one, expected_one) in found.iter().zip(expected) {
            assert!(
                (found_one - expected_one).abs() < 0.001,
                "{found:?}, not {expected:?}"
            );
        }
    }

    fn reads(key: SessionKey, text: &str) -> Effect {
        Effect::ReadHistory {
            key,
            id: id(text),
            max_items: HISTORY_ITEMS,
        }
    }

    #[test]
    fn the_arrangement_is_written_and_read_back() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let [a1, a2, b1] = ["a1", "a2", "b1"].map(|text| by_id(&app, text));
        app.show(a1, Place::Focused, Leaving::ForgetIfEmpty);
        app.show(a2, Place::Side, Leaving::ForgetIfEmpty);
        app.show(b1, Place::Side, Leaving::ForgetIfEmpty);
        // Two divider drags in a row 3000 points wide make it 1500, 750 and 750: 2, 1 and 1.
        assert!(app.tiles.resize(a1, 1500.0, 3000.0));
        assert!(app.tiles.resize(a2, 750.0, 3000.0));
        app.focus_tile(a2);
        assert_near(&shares(&app), &[1.5, 0.75, 0.75]);

        let saved = app.saved();
        let json = serde_json::to_string(&saved).unwrap();
        let read: Saved = serde_json::from_str(&json).unwrap();
        let mut again = restored(read.checked());
        let effects = listing(&mut again, None, three_stored());
        let [a1, a2, b1] = ["a1", "a2", "b1"].map(|text| by_id(&again, text));
        assert_eq!(tile_keys(&again), vec![a1, a2, b1]);
        assert_near(&shares(&again), &shares(&app));
        assert_eq!(again.focused(), Some(a2));
        assert_eq!(
            effects,
            vec![
                reads(a1, "a1"),
                reads(a2, "a2"),
                reads(b1, "b1"),
                reads_branch("/home/alex/project")
            ],
            "no Open"
        );
        assert!(
            [a1, a2, b1]
                .iter()
                .all(|&key| session(&again, key).phase == Phase::Resting)
        );
        assert_eq!(again.saved(), saved);
    }

    #[test]
    fn a_state_from_plan_3_restores_one_tile() {
        let read: Saved = serde_json::from_str(r#"{"version":1,"shown":"s1"}"#).unwrap();
        let mut app = restored(read.checked());
        let s1 = store("s1", "Tidy the notes", "/home/alex/notes", NOW - 60);
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![s1]);
        let key = by_id(&app, "s1");
        assert_eq!((tile_keys(&app), app.focused()), (vec![key], Some(key)));
        assert_eq!(
            effects,
            vec![
                Effect::ListSessions { since_unix: None },
                reads(key, "s1"),
                reads_branch("/home/alex/notes")
            ]
        );
        let saved = app.saved();
        assert_eq!(
            (saved.tiles, saved.shown),
            (vec![tile("s1", 1.0)], Some(id("s1")))
        );
    }

    #[test]
    fn tiles_wait_for_every_session_or_the_whole_listing() {
        let saved = with_tiles(vec![tile("a1", 1.0), tile("old", 1.0)], "a1");
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 60);
        let old = store("old", "Old work", "/home/alex/notes", NOW - 90 * DAY);

        let mut app = restored(saved.clone());
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![a1.clone()]);
        assert_eq!(effects, vec![Effect::ListSessions { since_unix: None }]);
        assert_eq!(app.focused(), None, "the old one may still come");
        listing(&mut app, None, vec![a1.clone(), old.clone()]);
        let both = vec![by_id(&app, "a1"), by_id(&app, "old")];
        assert_eq!(
            (tile_keys(&app), app.focused()),
            (both.clone(), Some(both[0]))
        );

        let mut app = restored(saved);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![a1, old]);
        let both = vec![by_id(&app, "a1"), by_id(&app, "old")];
        assert_eq!(tile_keys(&app), both, "every one is known: no wait");
    }

    #[test]
    fn a_tile_whose_session_is_gone_is_left_out() {
        let saved = with_tiles(
            vec![tile("a1", 1.0), tile("gone", 1.0), tile("b1", 1.0)],
            "gone",
        );
        let mut app = restored(saved);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        assert_eq!(app.focused(), None, "it may still come");
        listing(&mut app, None, three_stored());
        let (a1, b1) = (by_id(&app, "a1"), by_id(&app, "b1"));
        assert_eq!((tile_keys(&app), app.focused()), (vec![a1, b1], Some(a1)));
        let saved = app.saved();
        assert_eq!(
            (saved.tiles, saved.shown),
            (vec![tile("a1", 1.0), tile("b1", 1.0)], Some(id("a1")))
        );
    }

    #[test]
    fn odd_shares_and_twins_are_put_right() {
        let saved = with_tiles(
            vec![tile("a1", 0.0), tile("a2", f32::NAN), tile("a1", 4.0)],
            "a2",
        );
        let mut app = restored(saved);
        list_both(&mut app, three_stored());
        let (a1, a2) = (by_id(&app, "a1"), by_id(&app, "a2"));
        assert_eq!((tile_keys(&app), app.focused()), (vec![a1, a2], Some(a2)));
        assert_eq!(shares(&app), vec![1.0, 1.0]);

        // Their sum is not finite: the row would have no width to give.
        let huge = vec![tile("a1", f32::MAX), tile("a2", f32::MAX * 0.75)];
        let mut app = restored(with_tiles(huge, "a1"));
        list_both(&mut app, three_stored());
        assert_eq!(tile_keys(&app).len(), 2);
        assert_near(&shares(&app), &[8.0 / 7.0, 6.0 / 7.0]);
        let widths = app.tiles.widths(1400.0);
        assert!((widths[0] - 800.0).abs() < 0.5, "{widths:?}");
        assert!((widths[1] - 600.0).abs() < 0.5, "{widths:?}");
    }

    #[test]
    fn a_new_session_without_an_id_is_not_saved() {
        let (mut app, _) = shows_stored();
        app.new_session(
            "/home/alex/notes".into(),
            Place::Side,
            Leaving::ForgetIfEmpty,
        );
        assert_eq!(tile_keys(&app).len(), 2);
        let saved = app.saved();
        assert_eq!(
            (saved.tiles, saved.shown),
            (vec![tile("a1", 1.0)], Some(id("a1"))),
            "the first kept tile stands for the focused one"
        );
    }

    #[test]
    fn a_new_session_nothing_was_sent_in_is_not_saved() {
        let (mut app, a1) = shows_stored();
        app.new_session(
            "/home/alex/notes".into(),
            Place::Side,
            Leaving::ForgetIfEmpty,
        );
        let new = app.focused().unwrap();
        assert_ne!(new, a1);
        let started = SessionInfo {
            id: id("new"),
            ..info()
        };
        app.event(open_of(&app, new), SessionEvent::Started(started), 0.0);
        assert_eq!(session(&app, new).id, Some(id("new")));
        let Some(Effect::SaveState(saved)) = app.close(NOW).into_iter().next() else {
            panic!("closing saves first");
        };
        assert_eq!(
            (saved.tiles.clone(), saved.shown.clone()),
            (vec![tile("a1", 1.0)], Some(id("a1")))
        );

        // The next run has every tile at the recent listing.
        let mut app = restored(*saved);
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 3600);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![a1]);
        let a1 = by_id(&app, "a1");
        assert_eq!((tile_keys(&app), app.focused()), (vec![a1], Some(a1)));
    }

    #[test]
    fn a_session_driven_from_another_device_is_kept() {
        let (mut app, a1) = shows_stored();
        app.new_session(
            "/home/alex/notes".into(),
            Place::Side,
            Leaving::ForgetIfEmpty,
        );
        let new = app.focused().unwrap();
        let started = SessionInfo {
            id: id("new"),
            ..info()
        };
        let open = open_of(&app, new);
        app.event(open, SessionEvent::Started(started), 0.0);
        // A prompt typed on a phone: a turn runs that nothing here handed over.
        app.event(open, SessionEvent::TurnStarted, 0.0);
        app.event(open, done(), 0.0);
        assert_eq!(
            app.saved().tiles,
            vec![tile("a1", 1.0), tile("new", 1.0)],
            "its tile is kept"
        );
        // Leaving it with nothing typed neither ends its program nor forgets it.
        assert_eq!(
            closes(&app.show(a1, Place::Focused, Leaving::ForgetIfEmpty)),
            Vec::<u64>::new()
        );
        assert!(app.sessions.get(new).is_some());
        assert_eq!(
            closes(&app.close_tile(new, Leaving::ForgetIfEmpty)),
            Vec::<u64>::new()
        );
        assert!(app.sessions.get(new).is_some());
    }

    #[test]
    fn a_launch_folder_restores_nothing() {
        for saved in [
            with_tiles(vec![tile("a1", 1.0), tile("a2", 1.0)], "a2"),
            with_tiles(Vec::new(), "a2"),
        ] {
            let mut app = restored_with(launch("/home/alex/notes"), saved);
            let started = app.focused().expect("the launch folder's session");
            let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
            assert_eq!(effects, vec![Effect::ListSessions { since_unix: None }]);
            listing(&mut app, None, three_stored());
            assert_eq!(
                (tile_keys(&app), app.focused()),
                (vec![started], Some(started))
            );
        }
    }

    /// The tiles and the focused session `effects` ask to save.
    fn saved_row(effects: &[Effect]) -> (Vec<SavedTile>, Option<SessionId>) {
        let saves: Vec<&Saved> = effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::SaveState(saved) => Some(&**saved),
                _ => None,
            })
            .collect();
        assert_eq!(saves.len(), 1, "{effects:?}");
        (saves[0].tiles.clone(), saves[0].shown.clone())
    }

    #[test]
    fn a_restore_that_waited_does_not_run_after_a_tile_was_opened() {
        let saved = with_tiles(vec![tile("a1", 1.0), tile("old", 1.0)], "a1");
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 60);
        let old = store("old", "Old work", "/home/alex/notes", NOW - 90 * DAY);

        // A row shown by a click, then closed.
        let mut app = restored(saved.clone());
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![a1.clone()]);
        assert_eq!(app.focused(), None, "it waits for the old one");
        let key = by_id(&app, "a1");
        app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        let effects = app.close_tile(key, Leaving::ForgetIfEmpty);
        assert_eq!(saved_row(&effects), (Vec::new(), None), "the empty row");
        listing(&mut app, None, vec![a1.clone(), old.clone()]);
        assert_eq!(tile_keys(&app), vec![], "the user left the row empty");

        // A new session, then closed.
        let mut app = restored(saved);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![a1.clone()]);
        app.new_session(
            "/home/alex/notes".into(),
            Place::Focused,
            Leaving::ForgetIfEmpty,
        );
        let key = app.focused().unwrap();
        let effects = app.close_tile(key, Leaving::Keep);
        assert_eq!(saved_row(&effects), (Vec::new(), None), "the empty row");
        listing(&mut app, None, vec![a1, old]);
        assert_eq!(tile_keys(&app), vec![]);
    }

    #[test]
    fn waiting_tiles_are_written_back_unchanged() {
        let read = with_tiles(vec![tile("a1", 2.0), tile("gone", 0.5)], "gone");
        let mut app = restored(read.clone());
        assert_eq!(app.saved(), read);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        assert_eq!(
            app.saved(),
            read,
            "the recent listing does not know them all"
        );
    }

    /// A saved column of `top` over `below`, the upper tile `split` of its height.
    fn column(top: &str, share: f32, below: &str, split: f32) -> SavedTile {
        SavedTile {
            below: Some(SavedBelow {
                id: id(below),
                split,
            }),
            ..tile(top, share)
        }
    }

    /// `[a1/a2][b1]` of [`three_stored`], listed in full, the upper tile a quarter of its
    /// column, `a2` focused.
    fn stacked_stored() -> App {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let [a1, a2, b1] = ["a1", "a2", "b1"].map(|text| by_id(&app, text));
        app.show(a1, Place::Focused, Leaving::ForgetIfEmpty);
        app.show(b1, Place::Side, Leaving::ForgetIfEmpty);
        app.show(a2, Place::Below(a1), Leaving::ForgetIfEmpty);
        app.put_split(a1, 0.25);
        app
    }

    #[test]
    fn a_column_of_two_is_saved_with_its_split() {
        let app = stacked_stored();
        let saved = app.saved();
        assert_eq!(
            (saved.tiles, saved.shown),
            (
                vec![column("a1", 1.0, "a2", 0.25), tile("b1", 1.0)],
                Some(id("a2"))
            )
        );
    }

    #[test]
    fn a_column_whose_upper_session_is_not_stored_saves_the_lower_alone() {
        let (mut app, a1) = shows_stored();
        app.new_session(
            "/home/alex/notes".into(),
            Place::Above(a1),
            Leaving::ForgetIfEmpty,
        );
        let new = app.focused().unwrap();
        assert_eq!(tile_columns(&app), vec![vec![new, a1]]);
        let saved = app.saved();
        assert_eq!(
            (saved.tiles, saved.shown),
            (vec![tile("a1", 1.0)], Some(id("a1")))
        );
    }

    #[test]
    fn a_column_whose_lower_session_is_not_stored_saves_the_upper_alone() {
        let (mut app, a1) = shows_stored();
        app.new_session(
            "/home/alex/notes".into(),
            Place::Below(a1),
            Leaving::ForgetIfEmpty,
        );
        let new = app.focused().unwrap();
        assert_eq!(tile_columns(&app), vec![vec![a1, new]]);
        let saved = app.saved();
        assert_eq!(
            (saved.tiles, saved.shown),
            (vec![tile("a1", 1.0)], Some(id("a1")))
        );
    }

    #[test]
    fn a_saved_column_of_two_comes_back() {
        let app = stacked_stored();
        let saved = app.saved();
        let json = serde_json::to_string(&saved).unwrap();
        let read: Saved = serde_json::from_str(&json).unwrap();
        let mut again = restored(read.checked());
        let effects = listing(&mut again, None, three_stored());
        let [a1, a2, b1] = ["a1", "a2", "b1"].map(|text| by_id(&again, text));
        assert_eq!(tile_columns(&again), vec![vec![a1, a2], vec![b1]]);
        assert_eq!(again.tile_split(a1), Some(0.25));
        assert_eq!(again.focused(), Some(a2));
        assert_eq!(
            effects,
            vec![
                reads(a1, "a1"),
                reads(a2, "a2"),
                reads(b1, "b1"),
                reads_branch("/home/alex/project")
            ],
            "no Open"
        );
        assert!(
            [a1, a2, b1]
                .iter()
                .all(|&key| session(&again, key).phase == Phase::Resting)
        );
        assert_eq!(again.saved(), saved);
    }

    #[test]
    fn a_saved_column_with_one_session_gone_comes_back_as_one_tile() {
        let saved = with_tiles(
            vec![
                column("gone", 2.0, "a2", 0.3),
                column("b1", 1.0, "lost", 0.3),
            ],
            "gone",
        );
        let mut app = restored(saved);
        list_both(&mut app, three_stored());
        let (a2, b1) = (by_id(&app, "a2"), by_id(&app, "b1"));
        assert_eq!(
            (tile_columns(&app), app.focused()),
            (vec![vec![a2], vec![b1]], Some(a2))
        );
        assert_near(&shares(&app), &[4.0 / 3.0, 2.0 / 3.0]);
        let saved = app.saved();
        assert_eq!(
            (saved.tiles, saved.shown),
            (
                vec![tile("a2", 4.0 / 3.0), tile("b1", 2.0 / 3.0)],
                Some(id("a2"))
            )
        );
    }

    #[test]
    fn restoring_waits_for_a_lower_tiles_session() {
        let saved = with_tiles(vec![column("a1", 1.0, "old", 0.4)], "old");
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 60);
        let old = store("old", "Old work", "/home/alex/notes", NOW - 90 * DAY);

        let mut app = restored(saved.clone());
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![a1.clone()]);
        assert_eq!(effects, vec![Effect::ListSessions { since_unix: None }]);
        assert_eq!(
            (tile_keys(&app), app.focused()),
            (vec![], None),
            "the old one may still come"
        );
        assert_eq!(app.saved().tiles, saved.tiles, "kept as it was read");
        listing(&mut app, None, vec![a1.clone(), old.clone()]);
        let (a1_key, old_key) = (by_id(&app, "a1"), by_id(&app, "old"));
        assert_eq!(
            (tile_columns(&app), app.focused()),
            (vec![vec![a1_key, old_key]], Some(old_key))
        );
        assert_eq!(app.tile_split(a1_key), Some(0.4));

        let mut app = restored(saved);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![a1, old]);
        let both = vec![vec![by_id(&app, "a1"), by_id(&app, "old")]];
        assert_eq!(tile_columns(&app), both, "every one is known: no wait");
    }

    #[test]
    fn recent_folders_are_newest_first_without_duplicates_up_to_twelve() {
        let saved = Saved {
            version: SAVED_VERSION,
            recent_folders: vec!["/home/alex/f0".into()],
            ..Saved::default()
        };
        let mut app = restored(saved);
        assert_eq!(app.recent_folders, vec![PathBuf::from("/home/alex/f0")]);
        for n in 1..=13 {
            busy(&mut app, &format!("/home/alex/f{n}"));
        }
        let folders = |app: &App| -> Vec<String> {
            app.recent_folders
                .iter()
                .map(|folder| folder.display().to_string())
                .collect()
        };
        let expected: Vec<String> = (2..=13).rev().map(|n| format!("/home/alex/f{n}")).collect();
        assert_eq!(folders(&app), expected);
        assert_eq!(RECENT_FOLDERS, 12);

        app.new_session(
            "/home/alex/f5".into(),
            Place::Focused,
            Leaving::ForgetIfEmpty,
        );
        let mut expected: Vec<String> = vec!["/home/alex/f5".into()];
        expected.extend(
            (2..=13)
                .rev()
                .filter(|n| *n != 5)
                .map(|n| format!("/home/alex/f{n}")),
        );
        assert_eq!(folders(&app), expected);
        assert_eq!(app.saved().recent_folders, app.recent_folders);

        let key = app.focused().unwrap();
        app.event(open_of(&app, key), SessionEvent::Started(info()), 0.0);
        app.submit(key, "go");
        let effects = app.new_session(
            "/home/alex/f5".into(),
            Place::Focused,
            Leaving::ForgetIfEmpty,
        );
        assert_eq!(saves(&effects), 0, "nothing changed: {effects:?}");
        assert_eq!(folders(&app), expected);
        let effects = app.new_session(
            "/home/alex/f9".into(),
            Place::Focused,
            Leaving::ForgetIfEmpty,
        );
        assert_eq!(effects.last(), Some(&save(&app)));
        assert_eq!(saves(&effects), 1);
    }

    #[test]
    fn folder_choices_are_recent_first_then_from_sessions() {
        // A hand-edited saved list, with a folder twice.
        let saved = Saved {
            version: SAVED_VERSION,
            recent_folders: vec![
                "/home/alex/project".into(),
                "/home/alex/notes".into(),
                "/home/alex/project".into(),
            ],
            ..Saved::default()
        };
        let mut app = restored(saved);
        let stored = vec![
            store("b1", "Tidy the notes", "/home/alex/notes", NOW - 60),
            store("c1", "Older work", "/home/alex/docs", NOW - 7200),
            store("d1", "Newest work", "/home/alex/garden", NOW - 30),
            store("c2", "More docs", "/home/alex/docs", NOW - 120),
        ];
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), stored.clone());
        let shown = |app: &App| -> Vec<String> {
            app.folder_choices()
                .iter()
                .map(|folder| folder.display().to_string())
                .collect()
        };
        assert_eq!(
            shown(&app),
            [
                "/home/alex/project",
                "/home/alex/notes",
                "/home/alex/garden",
                "/home/alex/docs"
            ],
            "the recent ones, then the newest sessions' folders, each once"
        );

        let mut many = stored;
        many.extend((0..20).map(|n| {
            let folder = format!("/home/alex/f{n:02}");
            store(&format!("f{n}"), "Work", &folder, NOW - 1000 - n)
        }));
        listing(&mut app, None, many);
        let choices = shown(&app);
        assert_eq!(choices.len(), RECENT_FOLDERS, "{choices:?}");
        assert_eq!(choices[3], "/home/alex/docs");
        assert_eq!(choices[4], "/home/alex/f00", "the newest of the rest");
        assert_eq!(choices[11], "/home/alex/f07");
    }

    #[test]
    fn every_group_change_asks_to_save() {
        let (mut app, key) = shows_stored();
        let (side, effects) = app.new_group("Side projects");
        assert_eq!(effects, vec![save(&app)]);
        assert_eq!(app.move_session(key, &side), vec![save(&app)]);
        assert_eq!(app.saved().placed, vec![(id("a1"), side.clone())]);
        assert_eq!(app.rename_group(&side, "Parser work"), vec![save(&app)]);
        assert_eq!(app.collapse_group(&side, true), vec![save(&app)]);
        assert_eq!(app.hide_group(&project(), true), vec![save(&app)]);
        assert_eq!(app.ungroup(&side), vec![save(&app)]);
        let (other, _) = app.new_group("Other");
        let effects = app.new_session_in(
            "/home/alex/project".into(),
            &other,
            Place::Focused,
            Leaving::ForgetIfEmpty,
        );
        assert_eq!(effects.last(), Some(&save(&app)), "after the placement");
        assert_eq!(saves(&effects), 1, "{effects:?}");
        let effects = app.new_session_in(
            "/home/alex/project".into(),
            &project(),
            Place::Focused,
            Leaving::ForgetIfEmpty,
        );
        assert_eq!(effects, vec![save(&app)], "the empty one moves");
        assert_eq!(app.show_older(&project()), vec![], "not kept between runs");
        assert_eq!(app.toggle_older_groups(), vec![], "not kept between runs");
    }

    #[test]
    fn closing_asks_to_save() {
        let (mut app, key) = shows_stored();
        let effects = app.close(NOW);
        assert_eq!(effects, vec![save(&app)], "nothing runs");
        assert_eq!(app.saved().shown, Some(id("a1")));
        app.submit(key, "carry on");
        app.receive(
            FromBackend::Activity {
                key,
                result: Ok(None),
            },
            0.0,
        );
        let effects = app.close(NOW);
        assert_eq!(
            effects,
            vec![
                save(&app),
                Effect::Send {
                    open: 1,
                    command: SessionCommand::Close
                }
            ],
            "the state is written first"
        );
    }

    /// The `Rename` commands in `effects`, by their names.
    fn renames(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Send {
                    command: SessionCommand::Rename(name),
                    ..
                } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    fn names(app: &App) -> Vec<(SessionId, String)> {
        app.saved().names
    }

    fn garden(id_text: &str) -> Vec<(SessionId, String)> {
        vec![(id(id_text), "Garden".into())]
    }

    /// An app from a state that keeps the name "Garden" for `s1`, and the recent listing giving
    /// `s1` titled `listed`.
    fn named_before(listed: &str) -> (App, SessionKey, Vec<Effect>) {
        let mut app = restored(Saved {
            version: SAVED_VERSION,
            names: garden("s1"),
            ..Saved::default()
        });
        let s1 = store("s1", listed, "/home/alex/project", NOW - 60);
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![s1]);
        let key = by_id(&app, "s1");
        (app, key, effects)
    }

    #[test]
    fn renaming_a_running_session_tells_its_program() {
        let (mut app, key) = ready();
        let effects = app.rename(key, "  Garden ");
        assert_eq!(session(&app, key).title.as_deref(), Some("Garden"));
        assert_eq!(session(&app, key).user_title.as_deref(), Some("Garden"));
        assert_eq!(
            unsaved(effects.clone()),
            vec![Effect::Send {
                open: 1,
                command: SessionCommand::Rename("Garden".into())
            }]
        );
        assert_eq!(saves(&effects), 1, "it has an id");
        assert_eq!(names(&app), garden("s1"));
        let effects = app.event(1, SessionEvent::TitleChanged("Garden".into()), 0.0);
        assert_eq!(effects, vec![save(&app)], "confirmed");
        assert!(names(&app).is_empty());
        assert_eq!(session(&app, key).user_title.as_deref(), Some("Garden"));
    }

    #[test]
    fn renaming_a_resting_session_waits_for_its_program() {
        let (mut app, key) = shows_stored();
        let effects = app.rename(key, "Garden");
        assert_eq!(effects, vec![save(&app)], "nothing is sent");
        assert_eq!(session(&app, key).title.as_deref(), Some("Garden"));
        assert_eq!(names(&app), garden("a1"));
        app.submit(key, "carry on");
        let effects = app.receive(
            FromBackend::Activity {
                key,
                result: Ok(None),
            },
            0.0,
        );
        assert!(renames(&effects).is_empty(), "not before its start");
        let started = SessionInfo {
            id: id("a1"),
            ..info()
        };
        let effects = app.event(1, SessionEvent::Started(started), 0.0);
        assert_eq!(renames(&effects), vec!["Garden"]);
        assert!(sends_text(&effects, 1, "carry on"));
        assert_eq!(saves(&effects), 0, "it was kept already");
        assert_eq!(names(&app), garden("a1"), "until it is confirmed");
        let effects = app.event(1, SessionEvent::TitleChanged("Garden".into()), 0.0);
        assert_eq!(saves(&effects), 1);
        assert!(names(&app).is_empty());
    }

    #[test]
    fn a_new_session_renamed_before_its_start_is_named_at_the_start() {
        let mut app = main_screen();
        app.new_session("/p".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let key = app.focused().unwrap();
        let effects = app.rename(key, "Garden");
        assert!(effects.is_empty(), "no id yet: nothing to keep or send");
        assert!(names(&app).is_empty());
        let effects = app.event(1, SessionEvent::Started(info()), 0.0);
        assert_eq!(renames(&effects), vec!["Garden"]);
        assert_eq!(saves(&effects), 1, "its id is known now");
        assert_eq!(names(&app), garden("s1"));
        app.event(1, SessionEvent::TitleChanged("Garden".into()), 0.0);
        assert!(names(&app).is_empty());
        // A name the program confirmed is not sent again to the next one.
        app.event(1, exited(false), 0.0);
        app.restart(key);
        let effects = app.event(2, SessionEvent::Started(info()), 0.0);
        assert!(renames(&effects).is_empty());
        assert_eq!(saves(&effects), 0);
    }

    #[test]
    fn a_name_kept_from_the_last_run_is_sent_when_its_program_starts() {
        let (mut app, key, _) = named_before("Planting plan");
        app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        app.submit(key, "carry on");
        let effects = app.receive(
            FromBackend::Activity {
                key,
                result: Ok(None),
            },
            0.0,
        );
        assert!(renames(&effects).is_empty(), "not before its start");
        let effects = app.event(open_of(&app, key), SessionEvent::Started(info()), 0.0);
        assert_eq!(renames(&effects), vec!["Garden"]);
        let at = |wanted: &dyn Fn(&Effect) -> bool| effects.iter().position(wanted).unwrap();
        let rename = at(&|effect| {
            matches!(
                effect,
                Effect::Send {
                    command: SessionCommand::Rename(_),
                    ..
                }
            )
        });
        let message = at(&|effect| {
            matches!(
                effect,
                Effect::Send {
                    command: SessionCommand::Send { text, .. },
                    ..
                } if text == "carry on"
            )
        });
        assert!(rename < message, "before the queued message: {effects:?}");
    }

    #[test]
    fn a_name_kept_from_the_last_run_names_the_row() {
        let (mut app, key, effects) = named_before("Planting plan");
        assert_eq!(saves(&effects), 0);
        assert_eq!(session(&app, key).title.as_deref(), Some("Garden"));
        assert_eq!(session(&app, key).user_title.as_deref(), Some("Garden"));
        assert_eq!(names(&app), garden("s1"));
        let _ = app.settle();
        assert_eq!(row_of(&app, key).title, "Garden");
    }

    #[test]
    fn the_store_agreeing_drops_the_kept_name() {
        let (app, key, effects) = named_before("Garden");
        assert_eq!(session(&app, key).user_title.as_deref(), Some("Garden"));
        assert!(names(&app).is_empty());
        assert_eq!(saves(&effects), 1);
    }

    #[test]
    fn a_complete_listing_drops_the_names_of_sessions_it_does_not_give() {
        let mut kept = garden("s1");
        kept.push((id("gone"), "Old".into()));
        let mut app = restored(Saved {
            version: SAVED_VERSION,
            names: kept,
            ..Saved::default()
        });
        let s1 = store("s1", "Planting plan", "/home/alex/project", NOW - 60);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![s1.clone()]);
        assert_eq!(names(&app).len(), 2, "the recent listing is not all");
        let effects = listing(&mut app, None, vec![s1]);
        assert_eq!(names(&app), garden("s1"));
        assert_eq!(saves(&effects), 1);
    }

    #[test]
    fn a_late_generated_title_does_not_replace_the_users_name() {
        let (mut app, key) = ready();
        app.submit(key, "plan the garden");
        app.rename(key, "Garden");
        let effects = app.event(1, SessionEvent::TitleChanged("Something else".into()), 0.0);
        assert!(effects.is_empty());
        assert_eq!(session(&app, key).title.as_deref(), Some("Garden"));
        assert_eq!(names(&app), garden("s1"), "still not confirmed");
    }

    #[test]
    fn a_listing_does_not_rename_what_the_user_named() {
        let (mut app, key) = shows_stored();
        app.rename(key, "Garden");
        let a1 = store("a1", "Something else", "/home/alex/project", NOW - 30);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![a1]);
        assert_eq!(session(&app, key).title.as_deref(), Some("Garden"));
        assert_eq!(app.sessions.get(key).unwrap().last_active_unix, NOW - 30);
        assert_eq!(names(&app), garden("a1"));
    }

    #[test]
    fn an_empty_name_changes_nothing() {
        let (mut app, key) = shows_stored();
        assert!(app.rename(key, " \t\n").is_empty());
        assert_eq!(session(&app, key).title.as_deref(), Some("Fix the parser"));
        assert_eq!(session(&app, key).user_title, None);
        assert!(names(&app).is_empty());
    }

    #[test]
    fn the_same_name_sends_nothing() {
        let (mut app, key) = ready();
        app.rename(key, "Garden");
        assert!(app.rename(key, " Garden ").is_empty());
        let (mut app, key) = shows_stored();
        assert!(app.rename(key, "Fix the parser").is_empty());
        assert_eq!(session(&app, key).user_title, None);
        assert!(names(&app).is_empty());
    }

    #[test]
    fn a_refused_rename_is_said_and_not_kept() {
        let (mut app, key) = ready();
        app.rename(key, "Garden");
        let refused = SessionEvent::CommandFailed {
            what: CommandKind::Rename,
            reason: "no".into(),
        };
        let effects = app.event(1, refused, 0.0);
        assert_eq!(saves(&effects), 1);
        assert!(names(&app).is_empty());
        let last = session(&app, key)
            .conversation
            .items()
            .last()
            .unwrap()
            .body
            .clone();
        assert_eq!(
            last,
            ItemBody::Notice {
                kind: NoticeKind::CommandFailed,
                text: "Could not rename the session: no".into()
            }
        );
        assert_eq!(session(&app, key).title.as_deref(), Some("Garden"));
        // Nor is it told to the next program.
        app.event(1, exited(false), 0.0);
        app.restart(key);
        let effects = app.event(2, SessionEvent::Started(info()), 0.0);
        assert!(renames(&effects).is_empty());
    }

    /// The file or folder at `text`, as attached.
    fn at(text: &str) -> Source {
        Source::Path(text.into())
    }

    /// An image pasted: a few bytes stand for it.
    fn pasted() -> Source {
        Source::Pasted(ImageData::new(vec![1u8, 2, 3]))
    }

    /// The file at `text`, read and ready to send.
    fn file_ready(text: &str) -> Prepared {
        Prepared {
            kind: AttachKind::File,
            attachment: Attachment::File { path: text.into() },
            thumb: None,
        }
    }

    /// The backend's answer for chip `chip`.
    fn answer(app: &mut App, chip: u64, result: Result<Prepared, Problem>) -> Vec<Effect> {
        app.receive(FromBackend::Prepared { chip, result }, 0.0)
    }

    /// Each chip of `key`'s session: its id and its state.
    fn chips_of(app: &App, key: SessionKey) -> Vec<(u64, ChipState)> {
        session(app, key)
            .chips
            .iter()
            .map(|chip| (chip.id, chip.state.clone()))
            .collect()
    }

    #[test]
    fn attaching_adds_reading_chips_and_asks_for_each() {
        let (mut app, key) = ready();
        let effects = app.attach(key, vec![at("/home/alex/notes.txt"), pasted()]);
        assert_eq!(
            effects,
            vec![
                Effect::Prepare {
                    chip: 1,
                    source: at("/home/alex/notes.txt")
                },
                Effect::Prepare {
                    chip: 2,
                    source: pasted()
                },
            ]
        );
        assert_eq!(
            session(&app, key).chips,
            vec![
                Chip {
                    id: 1,
                    name: "notes.txt".into(),
                    source: at("/home/alex/notes.txt"),
                    state: ChipState::Reading,
                },
                Chip {
                    id: 2,
                    name: "Pasted image".into(),
                    source: pasted(),
                    state: ChipState::Reading,
                },
            ]
        );
        // An id taken off is not given again.
        app.detach(key, 2);
        assert_eq!(
            app.attach(key, vec![pasted()]),
            vec![Effect::Prepare {
                chip: 3,
                source: pasted()
            }]
        );
        assert_eq!(
            chips_of(&app, key),
            vec![(1, ChipState::Reading), (3, ChipState::Reading)]
        );
    }

    #[test]
    fn the_same_path_is_attached_once() {
        let (mut app, key) = ready();
        app.attach(key, vec![at("/home/alex/a.txt")]);
        let effects = app.attach(
            key,
            vec![
                at("/home/alex/a.txt"),
                at("/home/alex/b.txt"),
                at("/home/alex/b.txt"),
            ],
        );
        assert_eq!(
            effects,
            vec![Effect::Prepare {
                chip: 2,
                source: at("/home/alex/b.txt")
            }]
        );
        assert_eq!(
            app.attach(key, vec![pasted(), pasted()]).len(),
            2,
            "a pasted image twice is two chips"
        );
        assert_eq!(session(&app, key).chips.len(), 4);
        // Taken off, a path can be attached again.
        app.detach(key, 1);
        assert_eq!(
            app.attach(key, vec![at("/home/alex/a.txt")]),
            vec![Effect::Prepare {
                chip: 5,
                source: at("/home/alex/a.txt")
            }]
        );
    }

    #[test]
    fn a_path_queued_with_a_message_is_not_attached_again() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.attach(key, vec![at("/home/alex/a.txt")]);
        answer(&mut app, 1, Ok(file_ready("/home/alex/a.txt")));
        app.submit(key, "and read this");
        assert_eq!(queued_text(session(&app, key)), Some("and read this"));
        assert_eq!(chips_of(&app, key), vec![], "the chip waits with the text");
        assert_eq!(
            app.attach(key, vec![at("/home/alex/a.txt"), at("/home/alex/b.txt")]),
            vec![Effect::Prepare {
                chip: 2,
                source: at("/home/alex/b.txt")
            }]
        );
    }

    #[test]
    fn a_pasted_chip_lets_go_of_the_clipboards_bytes_once_read() {
        let (mut app, key) = ready();
        app.attach(key, vec![pasted(), pasted()]);
        let held = |app: &App| -> Vec<usize> {
            session(app, key)
                .chips
                .iter()
                .map(|chip| match &chip.source {
                    Source::Pasted(data) => data.len(),
                    Source::Path(_) => usize::MAX,
                })
                .collect()
        };
        assert_eq!(held(&app), vec![3, 3], "held while it is read");
        answer(&mut app, 1, Ok(image_ready()));
        answer(&mut app, 2, Err(Problem::Unreadable));
        assert_eq!(held(&app), vec![0, 0]);
        // What is sent is the prepared image's.
        assert_eq!(
            chips_of(&app, key),
            vec![
                (1, ChipState::Ready(image_ready())),
                (2, ChipState::Problem(Problem::Unreadable)),
            ]
        );
    }

    #[test]
    fn an_answer_fills_its_chip() {
        let (mut app, key) = ready();
        app.attach(key, vec![at("/home/alex/a.txt"), at("/home/alex/huge.png")]);
        assert_eq!(
            answer(&mut app, 1, Ok(file_ready("/home/alex/a.txt"))),
            vec![]
        );
        assert_eq!(answer(&mut app, 2, Err(Problem::TooLarge)), vec![]);
        assert_eq!(
            chips_of(&app, key),
            vec![
                (1, ChipState::Ready(file_ready("/home/alex/a.txt"))),
                (2, ChipState::Problem(Problem::TooLarge)),
            ]
        );
    }

    #[test]
    fn an_answer_for_a_removed_chip_is_dropped() {
        let (mut app, key) = ready();
        app.attach(key, vec![at("/home/alex/a.txt"), at("/home/alex/b.txt")]);
        app.detach(key, 1);
        assert_eq!(
            answer(&mut app, 1, Ok(file_ready("/home/alex/a.txt"))),
            vec![]
        );
        assert_eq!(
            answer(&mut app, 9, Ok(file_ready("/home/alex/c.txt"))),
            vec![],
            "nor is one for a chip never given"
        );
        assert_eq!(chips_of(&app, key), vec![(2, ChipState::Reading)]);
    }

    #[test]
    fn a_chip_belongs_to_its_session() {
        let (mut app, a, b) = two_tiles();
        app.attach(a, vec![at("/home/alex/a.txt")]);
        // The same path in another session is a chip of its own.
        app.attach(b, vec![at("/home/alex/a.txt")]);
        answer(&mut app, 2, Err(Problem::Unreadable));
        answer(&mut app, 1, Ok(file_ready("/home/alex/a.txt")));
        assert_eq!(
            chips_of(&app, a),
            vec![(1, ChipState::Ready(file_ready("/home/alex/a.txt")))]
        );
        assert_eq!(
            chips_of(&app, b),
            vec![(2, ChipState::Problem(Problem::Unreadable))]
        );
    }

    #[test]
    fn an_ended_session_takes_no_chips() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.attach(key, vec![at("/home/alex/a.txt")]);
        app.event(1, done(), 0.0);
        assert_eq!(
            chips_of(&app, key),
            vec![(1, ChipState::Reading)],
            "a turn ending keeps them"
        );
        app.event(1, exited(false), 0.0);
        assert_eq!(app.attach(key, vec![at("/home/alex/b.txt")]), vec![]);
        // It keeps the chips it had, and an answer still fills them.
        answer(&mut app, 1, Ok(file_ready("/home/alex/a.txt")));
        assert_eq!(
            chips_of(&app, key),
            vec![(1, ChipState::Ready(file_ready("/home/alex/a.txt")))]
        );
        // Restarted, it keeps them and takes chips again.
        app.restart(key);
        assert_eq!(session(&app, key).chips.len(), 1);
        assert_eq!(
            app.attach(key, vec![at("/home/alex/b.txt")]),
            vec![Effect::Prepare {
                chip: 2,
                source: at("/home/alex/b.txt")
            }]
        );
    }

    #[test]
    fn nothing_can_be_sent_while_a_chip_is_read_or_has_a_problem() {
        let (mut app, key) = ready();
        assert!(app.can_send(key, "hi"));
        app.attach(key, vec![at("/home/alex/a.txt")]);
        assert!(!app.can_send(key, "hi"), "it is read");
        answer(&mut app, 1, Err(Problem::Unreadable));
        assert!(!app.can_send(key, "hi"), "it has a problem");
        app.detach(key, 1);
        assert!(app.can_send(key, "hi"));
        app.attach(key, vec![at("/home/alex/b.txt")]);
        answer(&mut app, 2, Ok(file_ready("/home/alex/b.txt")));
        assert!(app.can_send(key, "hi"));
        app.event(1, exited(false), 0.0);
        assert!(
            !app.can_send(key, "hi"),
            "an ended session takes no message"
        );
        assert!(
            session(&app, key).can_send("hi"),
            "though its text and its chips are ready"
        );
    }

    #[test]
    fn chips_alone_can_be_sent() {
        let (mut app, key) = ready();
        assert!(!app.can_send(key, ""));
        assert!(!app.can_send(key, " \n\t"));
        app.attach(key, vec![at("/home/alex/a.txt")]);
        answer(&mut app, 1, Ok(file_ready("/home/alex/a.txt")));
        assert!(app.can_send(key, ""));
        assert!(app.can_send(key, " \n"));
    }

    #[test]
    fn a_session_with_chips_is_not_empty() {
        let mut app = main_screen();
        let other = app.sessions.add(stored("s9"), NOW - 60);
        app.new_session("/a".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let a = app.focused().unwrap();
        app.attach(a, vec![at("/home/alex/a.txt")]);
        assert_eq!(
            closes(&app.show(other, Place::Focused, Leaving::ForgetIfEmpty)),
            Vec::<u64>::new()
        );
        assert!(app.sessions.get(a).is_some(), "kept for its chip");
        // Without its chip, it goes when left.
        app.show(a, Place::Focused, Leaving::ForgetIfEmpty);
        app.detach(a, 1);
        assert_eq!(
            closes(&app.show(other, Place::Focused, Leaving::ForgetIfEmpty)),
            vec![1]
        );
        assert!(app.sessions.get(a).is_none());
    }

    /// The text of `session`'s queued message.
    fn queued_text(session: &Session) -> Option<&str> {
        session.queued.as_ref().map(|queued| queued.text.as_str())
    }

    /// The image `pasted()` gives, read and ready to send.
    fn image_ready() -> Prepared {
        Prepared {
            kind: AttachKind::Image,
            attachment: pasted_image(),
            thumb: None,
        }
    }

    fn pasted_image() -> Attachment {
        Attachment::Image {
            media_type: "image/png".into(),
            data: ImageData::new(vec![1u8, 2, 3]),
        }
    }

    /// `shows_stored`, with a file and an image ready in its input box and `text` sent: the
    /// store says another app had the session open, so the question waits.
    fn asked_with_chips(text: &str) -> (App, SessionKey) {
        let (mut app, key) = shows_stored();
        app.attach(key, vec![at("/home/alex/a.txt"), pasted()]);
        answer(&mut app, 1, Ok(file_ready("/home/alex/a.txt")));
        answer(&mut app, 2, Ok(image_ready()));
        assert_eq!(
            app.submit(key, text),
            vec![Effect::CheckActivity { key, id: id("a1") }]
        );
        assert_eq!(chips_of(&app, key), vec![], "they went with the message");
        app.receive(
            FromBackend::Activity {
                key,
                result: Ok(Some(NOW - 5)),
            },
            0.0,
        );
        assert_eq!(
            app.prompt(key),
            Some(&Prompt::OpenElsewhere { text: text.into() })
        );
        (app, key)
    }

    #[test]
    fn cancel_on_open_elsewhere_brings_the_chips_back() {
        for text in ["carry on", ""] {
            let (mut app, key) = asked_with_chips(text);
            let (effects, back) = app.answer_prompt(key, PromptChoice::Cancel);
            assert_eq!((effects, back.as_deref()), (vec![], Some(text)));
            assert_eq!(session(&app, key).queued, None);
            assert_eq!(
                chips_of(&app, key),
                vec![
                    (1, ChipState::Ready(file_ready("/home/alex/a.txt"))),
                    (2, ChipState::Ready(image_ready())),
                ],
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_message_left_with_nothing_to_send_is_given_back() {
        for choice in [PromptChoice::ContinueHere, PromptChoice::ContinueAsCopy] {
            let (mut app, key) = asked_with_chips("");
            app.detach(key, 1);
            app.detach(key, 2);
            let (effects, back) = app.answer_prompt(key, choice);
            assert_eq!((effects, back.as_deref()), (vec![], Some("")), "{choice:?}");
            assert_eq!(app.prompt(key), None);
            assert_eq!(app.sessions.len(), 1, "no copy");
            let entry = app.sessions.get(key).unwrap();
            assert_eq!((&entry.session.phase, entry.open), (&Phase::Resting, None));
        }
    }

    #[test]
    fn continue_here_sends_the_chips() {
        let (mut app, key) = asked_with_chips("carry on");
        let (effects, back) = app.answer_prompt(key, PromptChoice::ContinueHere);
        assert_eq!(
            effects,
            vec![Effect::Open {
                open: 1,
                spec: resume_a1()
            }]
        );
        assert_eq!((back, app.prompt(key)), (None, None));
        let effects = app.event(1, SessionEvent::Started(info()), 0.0);
        assert!(
            effects.contains(&Effect::Send {
                open: 1,
                command: SessionCommand::Send {
                    text: "carry on".into(),
                    attachments: vec![
                        Attachment::File {
                            path: "/home/alex/a.txt".into()
                        },
                        pasted_image(),
                    ],
                },
            }),
            "{effects:?}"
        );
        let session = session(&app, key);
        assert_eq!((&session.chips, &session.queued), (&vec![], &None));
    }

    #[test]
    fn continue_as_a_copy_takes_the_chips_along() {
        let (mut app, key) = asked_with_chips("carry on");
        app.answer_prompt(key, PromptChoice::ContinueAsCopy);
        let copy = app.focused().unwrap();
        assert_ne!(copy, key);
        let (original, forked) = (session(&app, key), session(&app, copy));
        assert_eq!((&original.chips, &original.queued), (&vec![], &None));
        assert_eq!(queued_text(forked), Some("carry on"));
        let queued: Vec<u64> = forked
            .queued
            .iter()
            .flat_map(|q| &q.chips)
            .map(|chip| chip.id)
            .collect();
        assert_eq!(queued, vec![1, 2]);
        // No chip is on two sessions.
        let mut held: Vec<u64> = app
            .sessions
            .iter()
            .flat_map(|entry| {
                let session = &entry.session;
                session
                    .chips
                    .iter()
                    .chain(session.queued.iter().flat_map(|q| &q.chips))
                    .map(|chip| chip.id)
                    .collect::<Vec<_>>()
            })
            .collect();
        held.sort();
        assert_eq!(held, vec![1, 2]);
        let effects = app.event(1, SessionEvent::Started(info()), 0.0);
        assert!(
            effects.contains(&Effect::Send {
                open: 1,
                command: SessionCommand::Send {
                    text: "carry on".into(),
                    attachments: vec![
                        Attachment::File {
                            path: "/home/alex/a.txt".into()
                        },
                        pasted_image(),
                    ],
                },
            }),
            "{effects:?}"
        );
        assert_eq!(session(&app, copy).chips, vec![]);
    }

    /// The sessions whose history `effects` let go, in order.
    fn drops(effects: &[Effect]) -> Vec<SessionKey> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::DropHistory { key } => Some(*key),
                _ => None,
            })
            .collect()
    }

    /// The sessions whose history `effects` ask for, in order.
    fn history_asks(effects: &[Effect]) -> Vec<SessionKey> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::ReadHistory { key, .. } => Some(*key),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_resting_session_out_of_every_tile_lets_its_conversation_go_after_ten_minutes() {
        let (mut app, key) = shows_stored();
        assert_eq!(session(&app, key).conversation.items().len(), 2);
        let generation = session(&app, key).generation;
        let effects = app.close_tile(key, Leaving::Keep);
        assert_eq!(drops(&effects), vec![], "the tick alone lets go");
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![key]);
        let session = session(&app, key);
        assert_eq!(session.history, HistoryState::NotLoaded);
        assert!(session.conversation.items().is_empty());
        assert!(session.generation > generation, "the items were replaced");
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![], "once");
    }

    #[test]
    fn showing_it_again_reads_it_again() {
        let (mut app, key) = shows_stored();
        app.close_tile(key, Leaving::Keep);
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![key]);
        let effects = app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(history_asks(&effects), vec![key]);
        assert!(effects.contains(&reads(key, "a1")));
        assert_eq!(session(&app, key).history, HistoryState::Loading);
    }

    #[test]
    fn a_session_whose_program_runs_keeps_its_conversation() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        let effects = app.close_tile(a, Leaving::Keep);
        assert_eq!(drops(&effects), vec![]);
        app.event(open_of(&app, a), done(), 0.0);
        assert_eq!(session(&app, a).phase, Phase::Idle);
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![]);
        assert_eq!(session(&app, a).history, HistoryState::Loaded);
        assert_eq!(user_messages(session(&app, a), "go"), 1);
    }

    #[test]
    fn a_queued_or_undelivered_message_keeps_it() {
        // Undelivered: its program refused "hello" and the one started for it ended at once.
        let (mut app, key) = sent_hello();
        app.receive(closed_send(1, "hello", vec![]), 0.0);
        app.receive(exit_of(1, true), 0.0);
        app.receive(exit_of(2, true), 0.0);
        assert_eq!(session(&app, key).phase, Phase::Resting);
        assert_eq!(session(&app, key).undelivered.len(), 1);
        assert_eq!(drops(&app.close_tile(key, Leaving::Keep)), vec![]);
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![]);
        assert_eq!(user_messages(session(&app, key), "hello"), 1);

        // Queued, as a resting session cannot be through the intents: the rule still holds.
        let (mut app, key) = shows_stored();
        app.sessions.get_mut(key).unwrap().session.queued = Some(crate::session::Outgoing {
            text: "later".into(),
            chips: Vec::new(),
        });
        assert_eq!(drops(&app.close_tile(key, Leaving::Keep)), vec![]);
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![]);
        assert_eq!(session(&app, key).history, HistoryState::Loaded);
        // Without the queued message, the same tick lets it go: nothing else kept it.
        app.sessions.get_mut(key).unwrap().session.queued = None;
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![key]);
    }

    #[test]
    fn a_question_waiting_keeps_it_until_it_is_answered() {
        let (mut app, key, _) = checked(5);
        assert!(app.prompt(key).is_some());
        assert_eq!(drops(&app.close_tile(key, Leaving::Keep)), vec![]);
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![]);
        app.answer_prompt(key, PromptChoice::Cancel);
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![key]);
        assert_eq!(session(&app, key).history, HistoryState::NotLoaded);
    }

    #[test]
    fn a_clean_exit_of_a_session_in_no_tile_lets_go_at_the_tick() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        app.new_session("/b".into(), Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(tile_keys(&app), vec![app.focused().unwrap()]);
        let open = open_of(&app, a);
        app.event(open, done(), 0.0);
        assert_eq!(mark(&app, a), Mark::Finished);
        // Its program runs on for an hour out of every tile, and then ends.
        assert_eq!(drops(&app.tick(NOW + 3600)), vec![], "its program runs");
        let effects = app.event(open, exited(true), 0.0);
        assert_eq!(drops(&effects), vec![], "not at the exit");
        assert_eq!(session(&app, a).history, HistoryState::Loaded);
        assert_eq!(
            drops(&app.tick(NOW + 3601)),
            vec![a],
            "ten minutes are long over"
        );
        assert_eq!(session(&app, a).history, HistoryState::NotLoaded);
        assert!(session(&app, a).conversation.items().is_empty());
        assert_eq!(mark(&app, a), Mark::Finished, "its mark stays");
    }

    #[test]
    fn a_history_for_a_session_that_let_go_is_dropped() {
        let mut app = main_screen();
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 3600);
        listing(&mut app, Some(NOW - RECENT_DAYS * 86_400), vec![a1]);
        let key = by_id(&app, "a1");
        app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(session(&app, key).history, HistoryState::Loading);
        app.close_tile(key, Leaving::Keep);
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![key]);
        let history = StoredHistory {
            items: vec![HistoryItem::AssistantText("It reads tokens.".into())],
            ..StoredHistory::default()
        };
        app.receive(
            FromBackend::History {
                key,
                result: Ok(history),
                pictures: Vec::new(),
            },
            0.0,
        );
        assert_eq!(session(&app, key).history, HistoryState::NotLoaded);
        assert!(session(&app, key).conversation.items().is_empty());
        app.receive(
            FromBackend::History {
                key,
                result: Err(AgentError::Io("gone".into())),
                pictures: Vec::new(),
            },
            0.0,
        );
        assert_eq!(session(&app, key).history, HistoryState::NotLoaded);
    }

    #[test]
    fn the_chips_of_a_session_that_let_go_stay() {
        let (mut app, key) = shows_stored();
        app.attach(key, vec![pasted()]);
        let chips = chips_of(&app, key);
        assert_eq!(chips.len(), 1);
        app.close_tile(key, Leaving::Keep);
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![key]);
        assert_eq!(chips_of(&app, key), chips);
        assert_eq!(session(&app, key).history, HistoryState::NotLoaded);
    }

    #[test]
    fn the_session_a_copy_leaves_keeps_its_history_and_the_copy_gets_its_own() {
        let mut app = main_screen();
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 3600);
        listing(&mut app, Some(NOW - RECENT_DAYS * 86_400), vec![a1]);
        let key = by_id(&app, "a1");
        app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        // The question comes while the original's history is still being read.
        app.submit(key, "carry on");
        app.receive(
            FromBackend::Activity {
                key,
                result: Ok(Some(NOW - 5)),
            },
            0.0,
        );
        let (effects, _) = app.answer_prompt(key, PromptChoice::ContinueAsCopy);
        let copy = app.focused().unwrap();
        assert_eq!(
            drops(&effects),
            vec![],
            "the original is in no tile: for the tick"
        );
        assert_eq!(history_asks(&effects), vec![copy]);
        let history = StoredHistory {
            items: vec![HistoryItem::AssistantText("It reads tokens.".into())],
            ..StoredHistory::default()
        };
        for at in [key, copy] {
            app.receive(
                FromBackend::History {
                    key: at,
                    result: Ok(history.clone()),
                    pictures: Vec::new(),
                },
                0.0,
            );
        }
        assert_eq!(session(&app, key).history, HistoryState::Loaded);
        assert_eq!(session(&app, copy).history, HistoryState::Loaded);
        assert!(!session(&app, copy).conversation.items().is_empty());
        // Ten minutes on, the original lets go and the copy, in its tile, does not.
        assert_eq!(drops(&app.tick(NOW + LET_GO_SECONDS)), vec![key]);
        assert_eq!(session(&app, copy).history, HistoryState::Loaded);
    }

    /// [`ready`] with "first" handed over and its turn finished: its program idles at `NOW`, and
    /// the agent stores the session.
    fn idle() -> (App, SessionKey) {
        let (mut app, key) = ready();
        app.submit(key, "first");
        app.event(1, done(), 0.0);
        (app, key)
    }

    /// [`idle`], its program at the effort high, which its model takes, and in the mode that
    /// accepts edits.
    fn idle_as_chosen() -> (App, SessionKey) {
        let (mut app, key) = idle();
        let mut entry = app.sessions.get_mut(key).unwrap();
        for option in &mut entry.session.models {
            option.effort_levels = vec!["low".into(), "high".into()];
        }
        drop(entry);
        app.event(1, SessionEvent::EffortChanged(Some("high".into())), 0.0);
        let mode = SessionEvent::PermissionModeChanged(PermissionMode::AcceptEdits);
        app.event(1, mode, 0.0);
        (app, key)
    }

    /// How many notices that its program ended `session`'s conversation holds.
    fn ended_notices(session: &Session) -> usize {
        session
            .conversation
            .items()
            .iter()
            .filter(|item| {
                matches!(
                    item.body,
                    ItemBody::Notice {
                        kind: NoticeKind::Ended,
                        ..
                    }
                )
            })
            .count()
    }

    #[test]
    fn busy_counts_what_a_quit_would_cut_off() {
        let mut app = main_screen();
        assert_eq!(app.busy(), Busy::default(), "no session");
        fn add(app: &mut App, name: &str, change: &dyn Fn(&mut Entry)) {
            let key = app.sessions.add(stored(name), NOW - 60);
            change(&mut app.sessions.get_mut(key).unwrap());
        }
        let queued = || {
            Some(crate::session::Outgoing {
                text: "go on".into(),
                chips: Vec::new(),
            })
        };
        let undelivered = || {
            vec![Undelivered {
                text: "go on".into(),
                attachments: Vec::new(),
            }]
        };
        add(&mut app, "s1", &|e| e.session.phase = Phase::Working);
        add(&mut app, "s2", &|e| e.session.phase = Phase::NeedsUser);
        add(&mut app, "s3", &|e| {
            e.session.phase = Phase::Opening;
            e.session.queued = queued();
        });
        add(&mut app, "s4", &|e| {
            e.session.phase = Phase::Idle;
            e.session.background = 1;
        });
        add(&mut app, "s5", &|e| {
            e.session.phase = Phase::Idle;
            e.session.remote = Remote::On { url: None };
        });
        add(&mut app, "s6", &|e| e.session.phase = Phase::Idle);
        assert_eq!(
            app.busy(),
            Busy {
                working: 4,
                remote: 1
            }
        );
        // A message that waits for a program counts in every phase: a quit would lose it.
        add(&mut app, "s7", &|e| {
            e.session.phase = Phase::Idle;
            e.session.queued = queued();
        });
        add(&mut app, "s8", &|e| e.session.undelivered = undelivered());
        add(&mut app, "s9", &|e| {
            e.session.phase = Phase::Ended {
                clean: true,
                detail: String::new(),
            };
            e.session.undelivered = undelivered();
        });
        add(&mut app, "s10", &|e| {
            e.session.phase = Phase::Opening;
            e.session.undelivered = undelivered();
        });
        // So does the message the "open elsewhere" question holds.
        add(&mut app, "s11", &|e| {
            e.prompt = Some(Prompt::OpenElsewhere {
                text: "go on".into(),
            });
        });
        add(&mut app, "s12", &|_| {});
        assert_eq!(
            app.busy(),
            Busy {
                working: 9,
                remote: 1
            }
        );
    }

    #[test]
    fn the_standing_counts_sessions_by_their_mark() {
        let mut app = main_screen();
        assert_eq!(app.standing(), Standing::default(), "no session");
        let mut add = |name: &str, change: &dyn Fn(&mut Entry)| {
            let key = app.sessions.add(stored(name), NOW - 60);
            change(&mut app.sessions.get_mut(key).unwrap());
        };
        add("s1", &|e| e.session.phase = Phase::Working);
        add("s2", &|e| {
            e.session.phase = Phase::Opening;
            e.session.queued = Some(crate::session::Outgoing {
                text: "go on".into(),
                chips: Vec::new(),
            });
        });
        add("s3", &|e| e.session.phase = Phase::NeedsUser);
        add("s4", &|e| {
            e.prompt = Some(Prompt::OpenElsewhere {
                text: "go on".into(),
            });
        });
        add("s5", &|e| {
            e.session.phase = Phase::Idle;
            e.unseen = true;
        });
        add("s6", &|e| e.unseen = true);
        add("s7", &|e| e.session.phase = Phase::Idle);
        add("s8", &|_| {});
        assert_eq!(
            app.standing(),
            Standing {
                working: 2,
                needs_you: 2,
                finished: 2
            }
        );
    }

    #[test]
    fn a_quit_sends_no_second_close_to_a_program_being_stopped() {
        let (mut app, key) = idle();
        assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
        assert!(closes(&app.close(NOW + 1801)).is_empty());
        assert_eq!(app.sessions.get(key).unwrap().stopping, Some(1));
    }

    #[test]
    fn a_stored_session_idle_for_half_an_hour_is_closed() {
        let (mut app, key) = idle();
        assert!(closes(&app.tick(NOW + 1799)).is_empty());
        assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
        assert_eq!(app.sessions.get(key).unwrap().stopping, Some(1));
        assert!(closes(&app.tick(NOW + 1801)).is_empty(), "once");
    }

    #[test]
    fn what_keeps_a_program_running() {
        let (mut app, _) = idle();
        assert_eq!(closes(&app.tick(NOW + 7200)), vec![1], "nothing keeps it");

        /// Puts on the session one thing that keeps its program running.
        type Keep = fn(&mut App, SessionKey);
        let cases: Vec<(&str, Keep)> = vec![
            ("a turn running", |app, key| {
                app.submit(key, "more");
            }),
            // Outside a turn, so that the request alone keeps it.
            ("a request waiting", |app, _| {
                app.event(1, permission("r1"), 0.0);
            }),
            ("Remote Control on", |app, _| {
                let url = Some("https://example.org/session".into());
                app.event(1, SessionEvent::RemoteAccess(RemoteAccess::On { url }), 0.0);
            }),
            ("Remote Control asked for", |app, key| {
                app.set_remote(key, true);
            }),
            // A running session hands a message over at once: one is queued only by hand.
            ("a queued message", |app, key| {
                app.sessions.get_mut(key).unwrap().session.queued =
                    Some(crate::session::Outgoing {
                        text: "later".into(),
                        chips: Vec::new(),
                    });
            }),
            ("an undelivered message", |app, key| {
                app.submit(key, "more");
                app.receive(closed_send(1, "more", vec![]), 0.0);
            }),
            ("a background task", |app, _| {
                app.event(1, SessionEvent::BackgroundTasks { running: 1 }, 0.0);
            }),
            // Asked only about a resting session today: put on a running one by hand.
            ("the open elsewhere question", |app, key| {
                app.sessions.get_mut(key).unwrap().prompt = Some(Prompt::OpenElsewhere {
                    text: "later".into(),
                });
            }),
            ("the store being asked", |app, key| {
                app.sessions.get_mut(key).unwrap().checking = true;
            }),
        ];
        for (what, keep) in cases {
            let (mut app, key) = idle();
            keep(&mut app, key);
            assert!(closes(&app.tick(NOW + 7200)).is_empty(), "{what}");
            assert_eq!(app.sessions.get(key).unwrap().stopping, None, "{what}");
        }

        let (mut app, key) = ready();
        assert!(!session(&app, key).is_stored());
        assert!(
            closes(&app.tick(NOW + 7200)).is_empty(),
            "a session that never had a message"
        );
    }

    #[test]
    fn anything_from_the_program_moves_the_deadline() {
        let (mut app, _) = idle();
        assert!(closes(&app.tick(NOW + 1000)).is_empty());
        app.event(1, SessionEvent::TitleChanged("Parser".into()), 0.0);
        assert!(closes(&app.tick(NOW + 1800)).is_empty());
        // But not an answer to the app's own ask for the usage limits.
        app.tick(NOW + 2000);
        app.event(1, SessionEvent::UsageLimits(no_limits()), 0.0);
        assert_eq!(closes(&app.tick(NOW + 2800)), vec![1]);
    }

    #[test]
    fn after_a_quiet_stop_the_session_rests_as_it_was() {
        let (mut app, key) = idle_as_chosen();
        let before = session(&app, key).clone();
        assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
        let effects = app.receive(exit_of(1, true), 0.0);
        assert!(opened(&effects).is_empty(), "{effects:?}");
        let entry = app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, Phase::Resting);
        assert_eq!(entry.open, None);
        assert_eq!(entry.stopping, None);
        assert_eq!(
            entry.session.conversation.items(),
            before.conversation.items()
        );
        assert_eq!(entry.session.model, before.model);
        assert_eq!(entry.session.effort, Some("high".into()));
        assert_eq!(entry.session.mode, PermissionMode::AcceptEdits);
        assert_eq!(entry.mark(), Mark::None);
    }

    #[test]
    fn a_quiet_stop_that_had_to_kill_still_rests() {
        let (mut app, key) = idle();
        // Another session takes the focus: an ended turn would leave this one unseen.
        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        assert!(app.tiles.contains(key) && app.focused() != Some(key));
        assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
        app.receive(exit_of(1, false), 0.0);
        let entry = app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, Phase::Resting);
        assert_eq!(entry.open, None);
        assert_eq!(entry.stopping, None);
        assert_eq!(ended_notices(&entry.session), 0);
        assert_eq!(entry.mark(), Mark::None);
    }

    #[test]
    fn the_next_message_starts_it_as_it_was() {
        let (mut app, key) = idle_as_chosen();
        let spec = session(&app, key).start_spec();
        assert_eq!(spec.resume, Some(id("s1")));
        assert_eq!(spec.model.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(spec.effort.as_deref(), Some("high"));
        assert_eq!(spec.permission_mode, Some(PermissionMode::AcceptEdits));
        assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
        app.receive(exit_of(1, true), 0.0);
        let effects = app.submit(key, "again");
        assert_eq!(opened(&effects), vec![&spec], "{effects:?}");
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::CheckActivity { .. })),
            "the store is not asked: this app ran it"
        );
    }

    #[test]
    fn a_message_sent_while_it_stops_is_delivered_after_the_restart() {
        let (mut app, key) = idle();
        assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
        assert!(sends_text(&app.submit(key, "hello"), 1, "hello"));
        assert!(opened(&app.receive(exit_of(1, true), 0.0)).is_empty());
        let effects = app.receive(closed_send(1, "hello", vec![]), 0.0);
        assert_eq!(opened(&effects).len(), 1, "{effects:?}");
        let effects = app.receive(started_on(2), 0.0);
        assert_eq!(sent_messages(&effects), vec![("hello", &[][..])]);
        assert!(sends_text(&effects, 2, "hello"));
        assert_eq!(user_messages(session(&app, key), "hello"), 1);
        assert_eq!(ended_notices(session(&app, key)), 0);
    }

    #[test]
    fn a_message_sent_while_it_stops_is_delivered_whichever_comes_first() {
        // The usual order: the backend refuses the message as soon as the `Close` shut its lane,
        // and the program's exit follows. Even an exit that had to kill it is a quiet one.
        for clean in [true, false] {
            let (mut app, key) = idle();
            assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
            app.submit(key, "hello");
            // Another session takes the focus: an ended turn would leave this one unseen.
            app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
            assert!(app.tiles.contains(key) && app.focused() != Some(key));
            let effects = app.receive(closed_send(1, "hello", vec![]), 0.0);
            assert!(opened(&effects).is_empty(), "{effects:?}");
            let effects = app.receive(exit_of(1, clean), 0.0);
            assert_eq!(opened(&effects).len(), 1, "clean {clean}: {effects:?}");
            let entry = app.sessions.get(key).unwrap();
            assert!(!entry.unseen, "clean {clean}");
            assert_eq!(entry.stopping, None);
            assert_eq!(ended_notices(&entry.session), 0, "clean {clean}");
            let open = entry.open.unwrap();
            let effects = app.receive(started_on(open), 0.0);
            assert_eq!(sent_messages(&effects), vec![("hello", &[][..])]);
            assert!(sends_text(&effects, open, "hello"));
            assert_eq!(user_messages(session(&app, key), "hello"), 1);
        }
    }

    #[test]
    fn a_stopped_session_in_no_tile_lets_go_at_the_next_tick() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        let open = open_of(&app, a);
        app.event(open, done(), 0.0);
        app.new_session("/b".into(), Place::Focused, Leaving::ForgetIfEmpty);
        assert!(!app.tiles.contains(a));
        let effects = app.tick(NOW + 1800);
        assert_eq!(closes(&effects), vec![open]);
        assert_eq!(drops(&effects), vec![], "its program still runs");
        let effects = app.receive(exit_of(open, true), 0.0);
        assert_eq!(drops(&effects), vec![], "not at the exit");
        assert_eq!(session(&app, a).phase, Phase::Resting);
        assert_eq!(session(&app, a).history, HistoryState::Loaded);
        // It left its tile half an hour ago: the next tick lets it go.
        assert_eq!(drops(&app.tick(NOW + 1801)), vec![a]);
        assert_eq!(session(&app, a).history, HistoryState::NotLoaded);
    }

    #[test]
    fn the_idle_length_can_be_set_and_never_under_a_minute() {
        assert_eq!(IDLE_SECONDS, 30 * 60);
        let (mut app, _) = idle();
        app.set_idle_after(120);
        assert!(closes(&app.tick(NOW + 119)).is_empty());
        assert_eq!(closes(&app.tick(NOW + 120)), vec![1]);
        let (mut app, _) = idle();
        app.set_idle_after(5);
        assert!(closes(&app.tick(NOW + 59)).is_empty());
        assert_eq!(closes(&app.tick(NOW + 60)), vec![1]);
    }

    #[test]
    fn a_turn_that_ends_with_no_window_is_unseen_also_in_the_focused_tile() {
        let (mut app, key) = ready();
        app.submit(key, "hello");
        assert!(app.set_window(false).is_empty());
        app.event(1, done(), 0.0);
        assert_eq!(app.focused(), Some(key));
        assert!(app.sessions.get(key).unwrap().unseen);
        assert_eq!(mark(&app, key), Mark::Finished);
    }

    #[test]
    fn the_window_coming_back_does_not_clear_ready() {
        let (mut app, key) = ready();
        app.submit(key, "hello");
        app.set_window(false);
        assert!(!app.looking());
        app.event(1, done(), 0.0);
        assert!(app.sessions.get(key).unwrap().unseen);
        assert!(app.set_window(true).is_empty());
        assert!(app.looking());
        assert_eq!(mark(&app, key), Mark::Finished, "until the user acts on it");
        // With the window back, a turn that ends in the focused session is seen there.
        app.submit(key, "again");
        app.event(1, done(), 0.0);
        assert!(!app.sessions.get(key).unwrap().unseen);
    }

    /// [`ready`] with a turn that ended while another app had the keyboard: its session is
    /// ready in the focused tile, and the keyboard is back.
    fn ready_unseen() -> (App, SessionKey) {
        let (mut app, key) = ready();
        app.submit(key, "hello");
        assert!(app.set_active(false).is_empty());
        app.event(1, done(), 0.0);
        assert!(app.set_active(true).is_empty());
        assert_eq!(mark(&app, key), Mark::Finished);
        (app, key)
    }

    #[test]
    fn a_turn_that_ends_while_another_app_has_the_keyboard_is_ready_also_in_the_focused_tile() {
        let (mut app, key) = ready();
        app.submit(key, "hello");
        assert!(app.looking());
        assert!(app.set_active(false).is_empty());
        assert!(!app.looking());
        app.event(1, done(), 0.0);
        assert_eq!(app.focused(), Some(key));
        assert!(app.sessions.get(key).unwrap().unseen);
        assert_eq!(mark(&app, key), Mark::Finished);
    }

    #[test]
    fn a_turn_that_ends_in_the_focused_tile_while_looking_is_not_ready() {
        let (mut app, key) = ready();
        app.submit(key, "hello");
        assert!(app.looking());
        app.event(1, done(), 0.0);
        assert_eq!(app.focused(), Some(key));
        assert_eq!(mark(&app, key), Mark::None);
    }

    #[test]
    fn a_turn_that_ends_in_another_tile_is_ready() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        assert!(app.tiles.contains(a) && app.focused() != Some(a));
        assert!(app.looking());
        app.event(open_of(&app, a), done(), 0.0);
        assert_eq!(mark(&app, a), Mark::Finished);
    }

    #[test]
    fn getting_the_keyboard_back_does_not_clear_ready() {
        let (app, key) = ready_unseen();
        assert!(app.looking());
        assert!(app.sessions.get(key).unwrap().unseen);
    }

    #[test]
    fn closing_another_tile_does_not_clear_ready() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        let b = app.focused().unwrap();
        app.event(open_of(&app, a), done(), 0.0);
        app.close_tile(b, Leaving::Keep);
        assert_eq!(app.focused(), Some(a), "the focus falls on it");
        assert_eq!(mark(&app, a), Mark::Finished);
    }

    #[test]
    fn a_press_on_the_focused_tile_clears_ready() {
        let (mut app, key) = ready_unseen();
        assert!(app.focus_tile(key).is_empty(), "the focus did not move");
        assert_eq!(mark(&app, key), Mark::None);
    }

    #[test]
    fn handing_the_focus_back_does_not_clear_ready() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        let b = app.focused().unwrap();
        app.event(open_of(&app, a), done(), 0.0);
        assert_eq!(mark(&app, a), Mark::Finished);
        assert_eq!(app.return_focus(a), vec![reads_branch("/a")]);
        assert_eq!(app.focused(), Some(a));
        assert_eq!(mark(&app, a), Mark::Finished, "not the user's act");
        assert_eq!(app.return_focus(a), vec![], "it has the focus already");
        assert_eq!(app.return_focus(SessionKey(999)), vec![]);
        assert_eq!(app.focused(), Some(a));
        // The user's own press does clear it.
        app.focus_tile(b);
        app.focus_tile(a);
        assert_eq!(mark(&app, a), Mark::None);
    }

    #[test]
    fn a_press_on_the_row_of_the_focused_session_clears_ready() {
        let (mut app, key) = ready_unseen();
        assert!(
            app.show(key, Place::Focused, Leaving::ForgetIfEmpty)
                .is_empty()
        );
        assert_eq!(mark(&app, key), Mark::None);
    }

    #[test]
    fn sending_in_it_clears_ready() {
        let (mut app, key) = ready_unseen();
        app.submit(key, "again");
        assert!(!app.sessions.get(key).unwrap().unseen);
    }

    #[test]
    fn acting_on_it_in_another_way_clears_ready() {
        type Act = fn(&mut App, SessionKey) -> Vec<Effect>;
        let acts: [(&str, Act); 4] = [
            ("stop", App::stop),
            ("mode", |app, key| {
                app.choose_mode(key, PermissionMode::AcceptEdits)
            }),
            ("model", |app, key| app.choose_model(key, "default")),
            ("restart", App::restart),
        ];
        for (what, act) in acts {
            let (mut app, key) = ready_unseen();
            act(&mut app, key);
            assert!(!app.sessions.get(key).unwrap().unseen, "{what}");
        }
    }

    #[test]
    fn attending_to_a_deleted_session_does_nothing() {
        let (mut app, key) = shows_stored();
        app.delete_session(key);
        assert!(app.sessions.get(key).unwrap().deleted.is_some());
        // Set by hand: a deleted session is never marked.
        app.sessions.get_mut(key).unwrap().unseen = true;
        assert_eq!(app.attend(key), vec![]);
        assert!(app.sessions.get(key).unwrap().unseen, "left alone");
    }

    #[test]
    fn restored_tiles_keep_no_ready_and_clear_none() {
        let saved = with_tiles(vec![tile("a1", 0.5), tile("b1", 0.5)], "a1");
        let mut app = restored(saved);
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 60);
        let b1 = store("b1", "Tidy the notes", "/home/alex/notes", NOW - 90 * DAY);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![a1.clone()]);
        assert_eq!(app.focused(), None, "b1 may still come");
        let a1_key = by_id(&app, "a1");
        app.sessions.get_mut(a1_key).unwrap().unseen = true;
        listing(&mut app, None, vec![a1, b1]);
        let b1_key = by_id(&app, "b1");
        assert_eq!(tile_keys(&app), vec![a1_key, b1_key]);
        assert_eq!(app.focused(), Some(a1_key));
        assert_eq!(mark(&app, a1_key), Mark::Finished, "still ready");
        assert_eq!(mark(&app, b1_key), Mark::None, "not made ready");
    }

    #[test]
    fn a_picture_gives_its_bytes_or_the_read_of_them() {
        let mut app = main_screen();
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 3600);
        listing(&mut app, Some(NOW - RECENT_DAYS * 86_400), vec![a1]);
        let key = by_id(&app, "a1");
        app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        let history = StoredHistory {
            items: vec![HistoryItem::User {
                text: "see these".into(),
                image_count: 2,
                files: Vec::new(),
                images: vec![None, None],
                stored: Some("m1".into()),
            }],
            ..StoredHistory::default()
        };
        let pictures = vec![vec![1 << 63 | 1, 1 << 63 | 2]];
        app.receive(
            FromBackend::History {
                key,
                result: Ok(history),
                pictures,
            },
            0.0,
        );
        assert_eq!(
            app.full_image(key, 1 << 63 | 2),
            (
                None,
                vec![Effect::ReadImage {
                    picture: 1 << 63 | 2,
                    id: id("a1"),
                    message: "m1".into(),
                    index: 1,
                }]
            )
        );
        let red = ImageData::new([200u8, 30, 30, 255].repeat(4));
        app.sessions.get_mut(key).unwrap().session.chips = vec![Chip {
            id: 5,
            name: "Pasted image".into(),
            source: Source::Pasted(red.clone()),
            state: ChipState::Ready(Prepared {
                kind: AttachKind::Image,
                attachment: Attachment::Image {
                    media_type: "image/png".into(),
                    data: red.clone(),
                },
                thumb: None,
            }),
        }];
        assert_eq!(app.full_image(key, 5), (Some(red), Vec::new()));
        assert_eq!(app.full_image(key, 6), (None, Vec::new()));
        assert_eq!(app.full_image(SessionKey(99), 5), (None, Vec::new()));
    }

    /// `Remote Control` asked of open `open`.
    fn asks_remote(effects: &[Effect], open: u64) -> bool {
        effects.contains(&Effect::Send {
            open,
            command: SessionCommand::SetRemoteAccess { on: true },
        })
    }

    /// [`idle`], its program stopped for idleness and gone: the session rests.
    fn stopped_for_idling() -> (App, SessionKey) {
        let (mut app, key) = idle();
        assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
        app.receive(exit_of(1, true), 0.0);
        assert_eq!(session(&app, key).phase, Phase::Resting);
        (app, key)
    }

    #[test]
    fn remote_control_asked_of_a_session_stopped_for_idling_starts_it() {
        let (mut app, key) = stopped_for_idling();
        let effects = app.set_remote(key, true);
        let specs = opened(&effects);
        assert_eq!(specs.len(), 1, "{effects:?}");
        assert_eq!(specs[0].resume, Some(id("s1")));
        assert_eq!(session(&app, key).phase, Phase::Opening);
        assert!(app.set_remote(key, true).is_empty(), "started once");
        let effects = app.receive(started_on(2), 0.0);
        assert!(asks_remote(&effects, 2), "{effects:?}");
        assert!(closes(&app.tick(NOW + 9000)).is_empty(), "and it stays up");
    }

    #[test]
    fn remote_control_asked_while_it_stops_starts_it_again_after_the_exit() {
        // Even an exit that had to kill it is a quiet one.
        for clean in [true, false] {
            let (mut app, key) = idle();
            assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
            // Its program still stands as running: it is asked, and the ask ends with it.
            app.set_remote(key, true);
            let effects = app.receive(exit_of(1, clean), 0.0);
            let specs = opened(&effects);
            assert_eq!(specs.len(), 1, "clean {clean}: {effects:?}");
            assert_eq!(specs[0].resume, Some(id("s1")));
            assert_eq!(ended_notices(session(&app, key)), 0, "clean {clean}");
            let effects = app.receive(started_on(2), 0.0);
            assert!(asks_remote(&effects, 2), "clean {clean}: {effects:?}");
        }
        // With a message its program never got as well, it is started once.
        let (mut app, key) = idle();
        assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
        app.set_remote(key, true);
        app.submit(key, "hello");
        app.receive(closed_send(1, "hello", vec![]), 0.0);
        let effects = app.receive(exit_of(1, true), 0.0);
        assert_eq!(opened(&effects).len(), 1, "{effects:?}");
    }

    #[test]
    fn remote_control_asked_of_a_session_never_run_here_still_waits() {
        let (mut app, key) = shows_stored();
        assert_eq!(app.set_remote(key, true), vec![]);
        assert_eq!(session(&app, key).phase, Phase::Resting);
        assert!(session(&app, key).remote_wanted, "for its next start");

        // Nor is one started that this app ran, while a question waits about it, while the
        // store is asked about it, or once the app is going away.
        type Hold = fn(&mut App, SessionKey);
        let cases: Vec<(&str, Hold)> = vec![
            ("a question waiting", |app, key| {
                app.sessions.get_mut(key).unwrap().prompt = Some(Prompt::OpenElsewhere {
                    text: "later".into(),
                });
            }),
            ("the store being asked", |app, key| {
                app.sessions.get_mut(key).unwrap().checking = true;
            }),
            ("the app going away", |app, _| {
                app.close(NOW + 1900);
            }),
        ];
        for (what, hold) in cases {
            let (mut app, key) = stopped_for_idling();
            hold(&mut app, key);
            let effects = app.set_remote(key, true);
            assert!(opened(&effects).is_empty(), "{what}: {effects:?}");
            assert_eq!(session(&app, key).phase, Phase::Resting, "{what}");
        }

        // A program that ends by itself with Remote Control wanted is not started again.
        let (mut app, key) = idle();
        app.set_remote(key, true);
        let effects = app.receive(exit_of(1, true), 0.0);
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert_eq!(session(&app, key).phase, Phase::Resting);
        // Turning it off starts nothing.
        let (mut app, key) = stopped_for_idling();
        assert!(app.set_remote(key, false).is_empty());
    }

    /// A usage report with the session window alone, resetting at `resets_at_unix`.
    fn session_limit(resets_at_unix: i64) -> UsageLimits {
        UsageLimits {
            session: Some(LimitWindow {
                used: 0.9,
                resets_at_unix,
            }),
            weekly: None,
        }
    }

    /// A usage report that names no window.
    fn no_limits() -> UsageLimits {
        UsageLimits {
            session: None,
            weekly: None,
        }
    }

    #[test]
    fn an_answer_to_the_apps_own_usage_ask_does_not_keep_a_program_from_the_idle_stop() {
        let (mut app, _) = idle();
        app.event(1, SessionEvent::UsageLimits(session_limit(NOW + 10)), 0.0);
        let mut closed = Vec::new();
        for minute in 1..=4 * 60 {
            let now = NOW + minute * 60;
            let effects = app.tick(now);
            closed.extend(closes(&effects).into_iter().map(|open| (open, now)));
            // Each ask is answered, the lapsed window left out; and every ten minutes an answer
            // and a refusal come anyway, as they would to asks made at every start.
            let asked = !usage_asks(effects).is_empty();
            if asked || minute % 10 == 0 {
                app.event(1, SessionEvent::UsageLimits(no_limits()), 0.0);
                let refresh = SessionCommand::RefreshUsage;
                app.send_failed(1, &refresh, AgentError::Io("broken pipe".into()));
                let refused = SessionEvent::CommandFailed {
                    what: CommandKind::RefreshUsage,
                    reason: "not now".into(),
                };
                app.event(1, refused, 0.0);
            }
        }
        assert_eq!(closed, vec![(1, NOW + 1800)], "closed once, on time");
    }

    /// An app that looks, signed in, with no session, its first look answered with a session
    /// window that resets ten seconds after `NOW`.
    fn looked_with_a_window() -> App {
        let (mut app, _) = App::new(caps(), Width::Unlimited, looking(), NOW);
        app.account_checked(signed_in());
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * 86_400), vec![]);
        assert_eq!(usage_asks(effects).len(), 1, "the first look");
        app.receive(look_answer(Some(session_limit(NOW + 10))), 0.0);
        app
    }

    fn look_answer(usage: Option<UsageLimits>) -> FromBackend {
        FromBackend::Look {
            models: Vec::new(),
            model: None,
            usage,
        }
    }

    #[test]
    fn a_lapsed_usage_window_is_asked_about_once() {
        // What the look answers: the lapsed window left out, the same window again, nothing
        // about usage, and no answer at all.
        let answers = [
            Some(look_answer(Some(no_limits()))),
            Some(look_answer(Some(session_limit(NOW + 10)))),
            Some(look_answer(None)),
            None,
        ];
        for (case, answer) in answers.into_iter().enumerate() {
            let mut app = looked_with_a_window();
            let mut looks = 0;
            for minute in 1..=8 * 60 {
                for ask in usage_asks(app.tick(NOW + minute * 60)) {
                    assert!(matches!(ask, Effect::Look { .. }), "nothing runs");
                    looks += 1;
                    if let Some(answer) = answer.clone() {
                        app.receive(answer, 0.0);
                    }
                }
            }
            assert_eq!(looks, 1, "case {case}");
            if case == 0 {
                assert_eq!(app.usage.session, None, "left out: cleared, not kept");
            }
        }
    }

    #[test]
    fn no_look_is_made_for_usage_while_no_window_shows() {
        let mut app = looked_with_a_window();
        app.set_window(false);
        for minute in 1..=8 * 60 {
            let asks = usage_asks(app.tick(NOW + minute * 60));
            assert_eq!(asks, vec![], "minute {minute}");
        }
    }

    #[test]
    fn the_usage_look_is_made_when_the_window_comes_back() {
        let mut app = looked_with_a_window();
        app.set_window(false);
        assert_eq!(usage_asks(app.tick(NOW + 3600)), vec![]);
        let look = vec![Effect::Look {
            folder: "/home/alex".into(),
        }];
        assert_eq!(usage_asks(app.set_window(true)), look);
        assert_eq!(usage_asks(app.set_window(true)), vec![], "once");
        assert_eq!(usage_asks(app.tick(NOW + 7200)), vec![]);
        // A program that runs is asked also while no window shows: nothing is started for it.
        let (mut app, _) = idle();
        app.event(1, SessionEvent::UsageLimits(session_limit(NOW + 10)), 0.0);
        app.set_window(false);
        let refresh = vec![Effect::Send {
            open: 1,
            command: SessionCommand::RefreshUsage,
        }];
        assert_eq!(usage_asks(app.tick(NOW + 301)), refresh);
    }

    /// The notices that a message could not be delivered in `session`'s conversation.
    fn not_delivered_notices(session: &Session) -> usize {
        session
            .conversation
            .items()
            .iter()
            .filter(|item| {
                matches!(
                    &item.body,
                    ItemBody::Notice { kind: NoticeKind::Ended, text }
                        if text == "The message could not be delivered. Restart to send it."
                )
            })
            .count()
    }

    #[test]
    fn a_program_that_keeps_refusing_is_started_twice_and_no_more() {
        let (mut app, key) = sent_hello();
        // Each program refuses "hello" as closed and ends cleanly.
        for open in 1..=2 {
            app.receive(closed_send(open, "hello", vec![]), 0.0);
            let effects = app.receive(exit_of(open, true), 0.0);
            assert_eq!(opened(&effects).len(), 1, "open {open}: {effects:?}");
            assert_eq!(not_delivered_notices(session(&app, key)), 0);
            let effects = app.receive(started_on(open + 1), 0.0);
            assert!(sends_text(&effects, open + 1, "hello"), "open {open}");
        }
        app.receive(closed_send(3, "hello", vec![]), 0.0);
        let effects = app.receive(exit_of(3, true), 0.0);
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert!(matches!(session(&app, key).phase, Phase::Ended { .. }));
        assert_eq!(not_delivered_notices(session(&app, key)), 1);
        assert_eq!(session(&app, key).undelivered.len(), 1);
        assert_eq!(app.sessions.get(key).unwrap().open, None);
        assert!(!app.takes_message(key), "until it is restarted");
        // Restart delivers it.
        assert_eq!(opened(&app.restart(key)).len(), 1);
        let effects = app.receive(started_on(4), 0.0);
        assert_eq!(sent_messages(&effects), vec![("hello", &[][..])]);
        assert!(session(&app, key).undelivered.is_empty());
        assert_eq!(user_messages(session(&app, key), "hello"), 1);
    }

    #[test]
    fn a_turn_that_starts_lets_a_later_message_be_started_for_twice_again() {
        let (mut app, key) = sent_hello();
        let mut open = 1;
        // Two rounds of two starts each, a turn started between them.
        for round in 0..2 {
            for _ in 0..2 {
                app.receive(closed_send(open, "hello", vec![]), 0.0);
                let effects = app.receive(exit_of(open, true), 0.0);
                assert_eq!(opened(&effects).len(), 1, "round {round}: {effects:?}");
                open += 1;
                app.receive(started_on(open), 0.0);
            }
            app.event(open, SessionEvent::TurnStarted, 0.0);
            app.event(open, done(), 0.0);
            app.submit(key, "hello");
        }
        assert_eq!(not_delivered_notices(session(&app, key)), 0);
    }

    #[test]
    fn a_copy_asks_for_its_stored_picture_under_the_id_it_was_read_from() {
        let picture = 1 << 63 | 1;
        let history = StoredHistory {
            items: vec![HistoryItem::User {
                text: "see this".into(),
                image_count: 1,
                files: Vec::new(),
                images: vec![None],
                stored: Some("m1".into()),
            }],
            ..StoredHistory::default()
        };
        let loaded = |key| FromBackend::History {
            key,
            result: Ok(history.clone()),
            pictures: vec![vec![picture]],
        };
        let read_under = |text: &str| {
            vec![Effect::ReadImage {
                picture,
                id: id(text),
                message: "m1".into(),
                index: 0,
            }]
        };
        // The copy is made of a conversation that is loaded, or of one still being read.
        for loaded_first in [true, false] {
            let mut app = main_screen();
            let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 3600);
            listing(&mut app, Some(NOW - RECENT_DAYS * 86_400), vec![a1]);
            let key = by_id(&app, "a1");
            app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
            if loaded_first {
                app.receive(loaded(key), 0.0);
            }
            app.submit(key, "carry on");
            let recent = FromBackend::Activity {
                key,
                result: Ok(Some(NOW - 5)),
            };
            app.receive(recent, 0.0);
            app.answer_prompt(key, PromptChoice::ContinueAsCopy);
            let copy = app.focused().unwrap();
            assert_ne!(copy, key);
            if !loaded_first {
                app.receive(loaded(copy), 0.0);
            }
            // Its program gives the copy an id of its own.
            let open = open_of(&app, copy);
            app.event(open, SessionEvent::Started(info()), 0.0);
            assert_eq!(session(&app, copy).id, Some(id("s1")));
            assert_eq!(
                app.full_image(copy, picture),
                (None, read_under("a1")),
                "loaded first {loaded_first}: read from the original's file"
            );
            // Let go and read again, from its own file by then, it asks under its own id.
            app.event(open, done(), 0.0);
            app.event(open, exited(true), 0.0);
            app.close_tile(copy, Leaving::Keep);
            app.tick(NOW + 3600);
            assert_eq!(session(&app, copy).history, HistoryState::NotLoaded);
            let effects = app.show(copy, Place::Focused, Leaving::ForgetIfEmpty);
            assert!(effects.contains(&reads(copy, "s1")), "{effects:?}");
            app.receive(loaded(copy), 0.0);
            assert_eq!(app.full_image(copy, picture), (None, read_under("s1")));
        }
    }

    #[test]
    fn a_message_refused_by_the_last_program_goes_to_the_one_running_now() {
        let (mut app, key) = sent_hello();
        app.receive(exit_of(1, false), 0.0);
        app.restart(key);
        app.receive(started_on(2), 0.0);
        assert_eq!(session(&app, key).phase, Phase::Idle);
        // The refusal of open 1 comes only now: the message goes to open 2 at once.
        let effects = app.receive(closed_send(1, "hello", vec![]), 0.0);
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert_eq!(sent_messages(&effects), vec![("hello", &[][..])]);
        assert!(sends_text(&effects, 2, "hello"), "{effects:?}");
        assert!(session(&app, key).undelivered.is_empty());
        assert_eq!(user_messages(session(&app, key), "hello"), 1);

        // While the newer program works, it waits for the turn to end, as a queued one does.
        let (mut app, key) = sent_hello();
        app.receive(exit_of(1, false), 0.0);
        app.restart(key);
        app.receive(started_on(2), 0.0);
        app.submit(key, "more");
        assert_eq!(session(&app, key).phase, Phase::Working);
        let effects = app.receive(closed_send(1, "hello", vec![]), 0.0);
        assert_eq!(effects, vec![]);
        assert_eq!(session(&app, key).undelivered.len(), 1);
        let effects = app.event(2, done(), 0.0);
        assert_eq!(sent_messages(&effects), vec![("hello", &[][..])]);
        assert!(sends_text(&effects, 2, "hello"), "{effects:?}");
        assert_eq!(user_messages(session(&app, key), "hello"), 1);
    }

    #[test]
    fn a_session_the_store_is_asked_about_keeps_its_conversation() {
        // Closed between the message and the store's answer.
        let (mut app, key) = shows_stored();
        let effects = app.submit(key, "carry on");
        assert_eq!(effects, vec![Effect::CheckActivity { key, id: id("a1") }]);
        assert!(app.sessions.get(key).unwrap().checking);
        assert_eq!(drops(&app.close_tile(key, Leaving::Keep)), vec![]);
        assert_eq!(drops(&app.tick(NOW + 3600)), vec![]);
        assert_eq!(session(&app, key).history, HistoryState::Loaded);
        assert_eq!(session(&app, key).conversation.items().len(), 2);

        // The check alone keeps it, as a resting session cannot be asked about through the
        // intents: the rule still holds.
        let (mut app, key) = shows_stored();
        app.sessions.get_mut(key).unwrap().checking = true;
        assert_eq!(drops(&app.close_tile(key, Leaving::Keep)), vec![]);
        assert_eq!(drops(&app.tick(NOW + 3600)), vec![]);
        assert_eq!(session(&app, key).history, HistoryState::Loaded);
        app.sessions.get_mut(key).unwrap().checking = false;
        assert_eq!(drops(&app.tick(NOW + 3600)), vec![key]);
    }

    #[test]
    fn a_session_out_of_its_tile_for_less_than_ten_minutes_keeps_its_conversation() {
        assert_eq!(LET_GO_SECONDS, 10 * 60);
        let (mut app, key) = shows_stored();
        assert_eq!(app.sessions.get(key).unwrap().hidden_unix, None, "shown");
        let generation = session(&app, key).generation;
        app.tick(NOW + 100);
        let effects = app.close_tile(key, Leaving::Keep);
        assert_eq!(drops(&effects), vec![], "not when its tile closes");
        assert_eq!(app.sessions.get(key).unwrap().hidden_unix, Some(NOW + 100));
        assert_eq!(drops(&app.tick(NOW + 699)), vec![]);
        assert_eq!(session(&app, key).history, HistoryState::Loaded);
        assert_eq!(session(&app, key).conversation.items().len(), 2);
        assert_eq!(drops(&app.tick(NOW + 700)), vec![key]);
        assert_eq!(session(&app, key).history, HistoryState::NotLoaded);
        assert!(session(&app, key).conversation.items().is_empty());
        assert!(session(&app, key).generation > generation);
        assert_eq!(drops(&app.tick(NOW + 701)), vec![], "once");

        // Shown again in time, it counts anew from when it next leaves every tile.
        let (mut app, key) = shows_stored();
        app.close_tile(key, Leaving::Keep);
        app.tick(NOW + 500);
        let effects = app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        assert_eq!(history_asks(&effects), vec![], "still held");
        assert_eq!(app.sessions.get(key).unwrap().hidden_unix, None);
        assert_eq!(drops(&app.tick(NOW + 5000)), vec![], "in a tile");
        app.close_tile(key, Leaving::Keep);
        assert_eq!(drops(&app.tick(NOW + 5599)), vec![]);
        assert_eq!(drops(&app.tick(NOW + 5600)), vec![key]);
    }

    #[test]
    fn switching_between_two_resting_sessions_reads_neither_again() {
        let mut app = main_screen();
        let stored = vec![
            store("a1", "Fix the parser", "/home/alex/project", NOW - 3600),
            store("a2", "Write the docs", "/home/alex/docs", NOW - 7200),
        ];
        listing(&mut app, Some(NOW - RECENT_DAYS * 86_400), stored);
        let (a, b) = (by_id(&app, "a1"), by_id(&app, "a2"));
        let history = StoredHistory {
            items: vec![HistoryItem::AssistantText("It reads tokens.".into())],
            ..StoredHistory::default()
        };
        for key in [a, b] {
            let effects = app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
            assert_eq!(history_asks(&effects), vec![key], "the first time");
            assert_eq!(drops(&effects), vec![]);
            let loaded = FromBackend::History {
                key,
                result: Ok(history.clone()),
                pictures: Vec::new(),
            };
            app.receive(loaded, 0.0);
        }
        let generations = [a, b].map(|key| session(&app, key).generation);
        // Back and forth in the one tile, a minute apart, for longer than ten minutes in all.
        for minute in 1..=12 {
            let key = if minute % 2 == 1 { a } else { b };
            assert_eq!(drops(&app.tick(NOW + minute * 60)), vec![]);
            let effects = app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
            assert_eq!(app.focused(), Some(key));
            assert_eq!(history_asks(&effects), vec![], "minute {minute}");
            assert_eq!(drops(&effects), vec![], "minute {minute}");
            assert_eq!(session(&app, key).history, HistoryState::Loaded);
        }
        assert_eq!([a, b].map(|key| session(&app, key).generation), generations);
        // Left alone, the one in no tile lets go ten minutes after it left.
        assert_eq!(app.focused(), Some(b));
        assert_eq!(drops(&app.tick(NOW + 12 * 60 + 599)), vec![]);
        assert_eq!(drops(&app.tick(NOW + 12 * 60 + 600)), vec![a]);
    }

    #[test]
    fn a_session_never_in_a_tile_counts_from_its_programs_exit() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        let open = open_of(&app, a);
        app.event(open, done(), 0.0);
        // Never shown since this entry was made: as one the row had no room for.
        app.new_session("/b".into(), Place::Focused, Leaving::ForgetIfEmpty);
        app.sessions.get_mut(a).unwrap().hidden_unix = None;
        app.tick(NOW + 5000);
        let effects = app.event(open, exited(true), 0.0);
        assert_eq!(drops(&effects), vec![], "not at the exit");
        assert_eq!(drops(&app.tick(NOW + 5599)), vec![]);
        assert_eq!(drops(&app.tick(NOW + 5600)), vec![a]);
    }

    /// What `key`'s session was marked deleted with.
    fn deleted(app: &App, key: SessionKey) -> Option<Deleted> {
        app.sessions.get(key).unwrap().deleted
    }

    #[test]
    fn deleting_a_resting_session_marks_it_and_closes_its_tile() {
        let (mut app, key) = shows_stored();
        let effects = app.delete_session(key);
        let mark = deleted(&app, key).expect("deleted");
        assert_eq!(
            mark,
            Deleted {
                at_unix: NOW,
                seen_unix: NOW - 3600,
                failed_unix: None,
                removing: false,
            }
        );
        assert!(!app.tiles.contains(key));
        assert_eq!(app.focused(), None);
        assert_eq!(saves(&effects), 1, "{effects:?}");
        assert_eq!(effects.last(), Some(&save(&app)));
        assert!(closes(&effects).is_empty(), "no program runs: {effects:?}");
        assert!(app.delete_session(key).is_empty(), "deleted once");
    }

    #[test]
    fn deleting_a_working_session_stops_it_once() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        let effects = app.delete_session(key);
        assert_eq!(closes(&effects), vec![1]);
        let seen = NOW + OWN_EXIT_SLACK_SECONDS;
        assert_eq!(
            deleted(&app, key).unwrap().seen_unix,
            seen,
            "its own program"
        );
        assert!(app.delete_session(key).is_empty());
        assert!(closes(&app.close(NOW)).is_empty(), "not again at quit");

        let (mut app, key) = ready();
        app.submit(key, "go");
        app.delete_session(key);
        let effects = app.receive(exit_of(1, false), 0.0);
        let entry = app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, Phase::Resting);
        assert_eq!(ended_notices(&entry.session), 0);
        assert_eq!((entry.mark(), entry.unseen), (Mark::None, false));
        assert_eq!(
            entry.deleted.unwrap().seen_unix,
            NOW + OWN_EXIT_SLACK_SECONDS
        );
        assert!(opened(&effects).is_empty(), "{effects:?}");
    }

    #[test]
    fn a_deleted_session_is_never_started_again() {
        // A message its program never got, refused before the delete, between the delete and
        // the exit, or after the exit.
        for order in [
            "refused, deleted, exited",
            "deleted, refused, exited",
            "deleted, exited, refused",
        ] {
            let (mut app, key) = sent_hello();
            let mut effects = Vec::new();
            for step in order.split(", ") {
                effects.extend(match step {
                    "refused" => app.receive(closed_send(1, "hello", vec![]), 0.0),
                    "deleted" => app.delete_session(key),
                    _ => app.receive(exit_of(1, true), 0.0),
                });
            }
            assert!(opened(&effects).is_empty(), "{order}: {effects:?}");
            let entry = app.sessions.get(key).unwrap();
            assert!(entry.session.undelivered.is_empty(), "{order}");
            assert!(entry.session.queued.is_none(), "{order}");
            assert_eq!(entry.session.phase, Phase::Resting, "{order}");
            assert!(!entry.unseen, "{order}");
            assert_eq!(entry.session.history, HistoryState::NotLoaded, "{order}");
            assert!(opened(&app.restart(key)).is_empty(), "{order}");
            app.recover_session(key);
            assert_eq!(app.busy(), Busy::default(), "{order}: nothing waits");
            assert_eq!(mark(&app, key), Mark::None, "{order}");
        }

        // Remote Control asked of a session stopped for idling.
        let (mut app, key) = idle();
        assert_eq!(closes(&app.tick(NOW + 1800)), vec![1]);
        app.set_remote(key, true);
        let effects = app.delete_session(key);
        assert!(closes(&effects).is_empty(), "it was sent one: {effects:?}");
        assert!(!session(&app, key).remote_wanted);
        let effects = app.receive(exit_of(1, true), 0.0);
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert!(app.set_remote(key, true).is_empty());
        assert_eq!(session(&app, key).phase, Phase::Resting);

        // A queued message, while the store is asked whether another app has it open.
        let (mut app, key) = shows_stored();
        app.submit(key, "carry on");
        app.delete_session(key);
        let entry = app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, Phase::Resting);
        assert_eq!(
            (entry.session.queued.is_none(), entry.checking),
            (true, false)
        );
        let effects = app.receive(
            FromBackend::Activity {
                key,
                result: Ok(Some(NOW - 3600)),
            },
            0.0,
        );
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert!(session(&app, key).queued.is_none());
    }

    #[test]
    fn a_deleted_session_cannot_be_shown_or_sent_to() {
        let (mut app, key) = shows_stored();
        let (side, _) = app.new_group("Side projects");
        app.delete_session(key);
        assert!(app.show(key, Place::Focused, Leaving::Keep).is_empty());
        assert!(app.show(key, Place::Side, Leaving::Keep).is_empty());
        assert!(!app.tiles.contains(key));
        assert!(!app.takes_message(key));
        assert!(app.submit(key, "carry on").is_empty());
        let file = Source::Path("/home/alex/notes.txt".into());
        assert!(app.attach(key, vec![file]).is_empty());
        assert!(app.rename(key, "Parser").is_empty());
        assert_eq!(session(&app, key).title.as_deref(), Some("Fix the parser"));
        assert!(app.move_session(key, &side).is_empty());
        assert_eq!(group_of(&app, key), project());
        assert!(app.set_remote(key, true).is_empty());
        assert!(!session(&app, key).remote_wanted);
        assert!(session(&app, key).queued.is_none());
    }

    #[test]
    fn deleting_lets_its_conversation_go() {
        let (mut app, key) = shows_stored();
        assert_eq!(session(&app, key).history, HistoryState::Loaded);
        let effects = app.delete_session(key);
        assert_eq!(drops(&effects), vec![key]);
        assert_eq!(session(&app, key).history, HistoryState::NotLoaded);
        assert!(session(&app, key).conversation.items().is_empty());

        // One whose program runs lets it go when its program has ended.
        let (mut app, key) = ready();
        app.submit(key, "go");
        assert_eq!(drops(&app.delete_session(key)), vec![]);
        let effects = app.receive(exit_of(1, true), 0.0);
        assert_eq!(drops(&effects), vec![key]);
        assert_eq!(session(&app, key).history, HistoryState::NotLoaded);
    }

    #[test]
    fn a_deleted_session_counts_nowhere() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        assert_eq!(app.standing().working, 1);
        assert_eq!(app.busy().working, 1);
        app.delete_session(key);
        assert_eq!(session(&app, key).phase, Phase::Working, "until it exits");
        assert_eq!(app.standing(), Standing::default());
        assert_eq!(app.busy(), Busy::default());
        assert_eq!(mark(&app, key), Mark::None);
    }

    #[test]
    fn in_flight_counts_what_deleting_would_cut_off() {
        let mut app = main_screen();
        let working = busy(&mut app, "/a");
        let waiting = busy(&mut app, "/b");
        app.event(open_of(&app, waiting), permission("r1"), 0.0);
        app.new_session("/c".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let queued = app.focused().unwrap();
        app.submit(queued, "early");
        assert!(session(&app, queued).queued.is_some());
        let resting = app.sessions.add(stored("s9"), NOW - 60);
        assert_eq!(app.in_flight(&[working, waiting, queued, resting]), 3);
        assert_eq!(app.in_flight(&[resting]), 0);
        assert_eq!(app.in_flight(&[]), 0);

        // An idle program that runs a background task, or has Remote Control on or coming:
        // deleting closes the program, and with it the task or the remote connection.
        let idle = |app: &mut App, name: &str, change: &dyn Fn(&mut Session)| {
            let key = app.sessions.add(stored(name), NOW - 60);
            let mut entry = app.sessions.get_mut(key).unwrap();
            entry.session.phase = Phase::Idle;
            change(&mut entry.session);
            key
        };
        let quiet = idle(&mut app, "i0", &|_| {});
        let task = idle(&mut app, "i1", &|s| s.background = 1);
        let remote = idle(&mut app, "i2", &|s| s.remote = Remote::On { url: None });
        let starting = idle(&mut app, "i3", &|s| s.remote = Remote::Starting);
        let back = idle(&mut app, "i4", &|s| s.remote = Remote::Reconnecting);
        assert_eq!(app.in_flight(&[quiet]), 0);
        for (key, what) in [
            (task, "a background task"),
            (remote, "Remote Control on"),
            (starting, "Remote Control starting"),
            (back, "Remote Control reconnecting"),
        ] {
            assert_eq!(app.in_flight(&[key]), 1, "{what}");
        }
        assert_eq!(app.in_flight(&[quiet, task, remote, starting, back]), 4);
    }

    #[test]
    fn a_session_never_sent_is_forgotten_by_delete() {
        // Started, with nothing handed over.
        let (mut app, key) = ready();
        let effects = app.delete_session(key);
        assert!(app.sessions.get(key).is_none());
        assert!(app.tiles.is_empty());
        assert_eq!(closes(&effects), vec![1]);
        assert!(app.saved().deleted.is_empty());

        // Not started yet: it has no id.
        let mut app = main_screen();
        app.new_session("/q".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let key = app.focused().unwrap();
        let effects = app.delete_session(key);
        assert!(app.sessions.get(key).is_none());
        assert_eq!(closes(&effects), vec![1]);
    }

    /// [`three_stored`], and `a3`, forty days old, in `/home/alex/project`.
    fn with_an_older_one() -> Vec<StoredSession> {
        let mut stored = three_stored();
        stored.push(store(
            "a3",
            "Old work",
            "/home/alex/project",
            NOW - 40 * DAY,
        ));
        stored
    }

    #[test]
    fn deleting_a_group_takes_its_older_sessions_too() {
        let mut app = main_screen();
        list_both(&mut app, with_an_older_one());
        let (a1, a2, a3, b1) = (
            by_id(&app, "a1"),
            by_id(&app, "a2"),
            by_id(&app, "a3"),
            by_id(&app, "b1"),
        );
        assert_eq!(app.sessions_of(&project()), vec![a1, a2, a3]);
        let effects = app.delete_group(&project());
        assert_eq!(saves(&effects), 1, "{effects:?}");
        assert_eq!(effects.last(), Some(&save(&app)));
        for key in [a1, a2, a3] {
            assert_eq!(deleted(&app, key).map(|d| d.at_unix), Some(NOW));
        }
        assert_eq!(deleted(&app, b1), None);
        assert_eq!(app.sessions_of(&project()), vec![]);
        assert_eq!(app.sessions_of(&notes()), vec![b1]);
        assert_eq!(
            app.groups.deleted_unix(&project()),
            None,
            "a folder's group"
        );
        assert!(app.saved().groups.is_empty());
    }

    #[test]
    fn a_group_you_made_is_marked_and_keeps_its_places() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        let (a1, b1) = (by_id(&app, "a1"), by_id(&app, "b1"));
        app.move_session(a1, &side);
        app.move_session(b1, &side);
        app.delete_group(&side);
        assert_eq!(app.groups.deleted_unix(&side), Some(NOW));
        assert!(deleted(&app, a1).is_some() && deleted(&app, b1).is_some());
        assert_eq!(
            (group_of(&app, a1), group_of(&app, b1)),
            (side.clone(), side.clone())
        );
        let saved = app.saved();
        assert_eq!(
            saved.placed,
            vec![(id("a1"), side.clone()), (id("b1"), side.clone())]
        );
        assert_eq!(saved.groups[0].deleted_unix, Some(NOW));

        // A deleted group takes no session.
        let a2 = by_id(&app, "a2");
        assert_eq!(app.move_session(a2, &side), vec![]);
        assert_eq!(group_of(&app, a2), project());

        let mut again = restored(saved);
        list_both(&mut again, three_stored());
        let a1 = by_id(&again, "a1");
        assert_eq!(group_of(&again, a1), side);
        assert!(deleted(&again, a1).is_some());
        assert_eq!(again.groups.deleted_unix(&side), Some(NOW));
        assert_eq!(again.saved(), app.saved());
    }

    #[test]
    fn ungroup_sends_the_sessions_home() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        let a1 = by_id(&app, "a1");
        app.move_session(a1, &side);
        assert_eq!(app.ungroup(&side), vec![save(&app)]);
        assert_eq!(group_of(&app, a1), project());
        assert_eq!(deleted(&app, a1), None, "nothing is deleted");
        assert!(app.saved().groups.is_empty());
        assert_eq!(app.ungroup(&notes()), vec![], "nothing to reset");
        app.rename_group(&notes(), "Notes");
        assert_eq!(app.ungroup(&notes()), vec![save(&app)]);
        assert!(app.saved().groups.is_empty());
    }

    #[test]
    fn recovering_a_session_brings_its_deleted_group_back() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        let (a1, b1) = (by_id(&app, "a1"), by_id(&app, "b1"));
        app.move_session(a1, &side);
        app.move_session(b1, &side);
        app.delete_group(&side);
        assert_eq!(app.recover_session(a1), vec![save(&app)]);
        assert_eq!(deleted(&app, a1), None);
        assert!(deleted(&app, b1).is_some(), "the other stays deleted");
        assert_eq!(app.groups.deleted_unix(&side), None);
        assert_eq!(group_of(&app, a1), side);
        assert_eq!(app.recover_session(a1), vec![], "not deleted");
    }

    #[test]
    fn recovering_a_group_recovers_its_sessions() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        let (a1, b1) = (by_id(&app, "a1"), by_id(&app, "b1"));
        app.move_session(a1, &side);
        app.move_session(b1, &side);
        app.delete_group(&side);
        assert_eq!(app.recover_group(&side), vec![save(&app)]);
        assert_eq!((deleted(&app, a1), deleted(&app, b1)), (None, None));
        assert_eq!(app.groups.deleted_unix(&side), None);
        assert_eq!(app.sessions_of(&side), vec![a1, b1]);
        let a2 = by_id(&app, "a2");
        app.delete_session(a2);
        app.recover_group(&project());
        assert_eq!(deleted(&app, a2), None, "a folder's group too");
    }

    #[test]
    fn switching_the_view_rebuilds_the_list() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let a1 = by_id(&app, "a1");
        app.delete_session(a1);
        let _ = app.settle();
        assert_eq!(app.sidebar_view(), SidebarView::Sessions);
        assert_eq!(
            (app.sidebar.view, app.sidebar.deleted),
            (SidebarView::Sessions, 1)
        );
        let (version, saved) = (app.sidebar_version, app.saved());

        assert_eq!(app.set_sidebar_view(SidebarView::Deleted), vec![]);
        assert_eq!(app.sidebar_view(), SidebarView::Deleted);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 1);
        assert_eq!(app.sidebar.view, SidebarView::Deleted);
        assert_eq!(
            layout(&app),
            vec![(project(), "project".to_string(), false, vec![id("a1")])]
        );
        app.set_sidebar_view(SidebarView::Deleted);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 1, "the same view");
        assert_eq!(app.saved(), saved, "the view is not saved");

        // The last one recovered: the view stays, and is empty.
        app.recover_session(a1);
        let _ = app.settle();
        assert_eq!(app.sidebar_view(), SidebarView::Deleted);
        assert_eq!(app.sidebar.deleted, 0);
        assert!(app.sidebar.sections.is_empty() && app.sidebar.lines.is_empty());

        app.set_sidebar_view(SidebarView::Sessions);
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 3);
        assert_eq!(app.sidebar.view, SidebarView::Sessions);
        assert_eq!(app.sidebar.sections.len(), 2);
        assert_eq!(
            restored(app.saved()).sidebar_view(),
            SidebarView::Sessions,
            "a run starts with the sessions"
        );
    }

    /// An app whose last run deleted the group "Side projects" a day before `NOW`, with `c1`
    /// placed in it but not known then; it has listed the three recent sessions, and then all,
    /// which hold `c1`, last active forty days ago.
    fn c1_arrives_in_a_deleted_group() -> (App, GroupId) {
        let side = GroupId::Custom(1);
        let saved = Saved {
            version: SAVED_VERSION,
            groups: vec![Group {
                id: side.clone(),
                name: Some("Side projects".into()),
                collapsed: false,
                hidden: false,
                deleted_unix: Some(NOW - DAY),
            }],
            placed: vec![(id("c1"), side.clone())],
            next_custom: 2,
            ..Saved::default()
        };
        let mut app = restored(saved);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        let mut all = three_stored();
        all.push(store(
            "c1",
            "Old plans",
            "/home/alex/project",
            NOW - 40 * DAY,
        ));
        listing(&mut app, None, all);
        let _ = app.settle();
        (app, side)
    }

    #[test]
    fn a_session_that_arrives_in_a_deleted_group_is_deleted_with_it() {
        let (mut app, side) = c1_arrives_in_a_deleted_group();
        let c1 = by_id(&app, "c1");
        // Its own week starts when it arrives, not when its group was deleted.
        assert_eq!(
            deleted(&app, c1),
            Some(Deleted {
                at_unix: NOW,
                seen_unix: NOW - 40 * DAY,
                failed_unix: None,
                removing: false,
            })
        );
        assert_eq!(group_of(&app, c1), side);
        assert!(!app.sessions_of(&project()).contains(&c1));
        assert_eq!(app.sessions_of(&side), vec![]);
        let listed = |app: &App| {
            let rows = app
                .sidebar
                .sections
                .iter()
                .flat_map(|section| &section.rows);
            rows.map(|row| row.key).collect::<Vec<SessionKey>>()
        };
        assert!(!listed(&app).contains(&c1), "not in the normal list");
        assert_eq!(app.saved().deleted.len(), 1, "its mark is kept");

        app.set_sidebar_view(SidebarView::Deleted);
        let _ = app.settle();
        assert_eq!(
            layout(&app),
            vec![(side, "Side projects".to_string(), false, vec![id("c1")])]
        );
    }

    #[test]
    fn such_a_session_is_removed_a_week_after_it_arrived() {
        let (mut app, side) = c1_arrives_in_a_deleted_group();
        // The group's week ends first: c1 is not removed then, and holds the group's record.
        let effects = app.tick(NOW + DELETED_SECONDS - DAY);
        assert_eq!(removals(&effects), vec![]);
        assert_eq!(app.groups.deleted_unix(&side), Some(NOW - DAY));
        let effects = app.tick(NOW + DELETED_SECONDS);
        assert_eq!(removals(&effects), vec![(id("c1"), NOW - 40 * DAY)]);
    }

    #[test]
    fn such_a_session_used_again_comes_back_with_its_group() {
        let (mut app, side) = c1_arrives_in_a_deleted_group();
        let used = store("c1", "Old plans", "/home/alex/project", NOW - 10);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![used]);
        let _ = app.settle();
        let c1 = by_id(&app, "c1");
        assert_eq!(deleted(&app, c1), None);
        assert_eq!(app.groups.deleted_unix(&side), None);
        assert_eq!(app.sessions_of(&side), vec![c1]);
        assert_eq!(
            layout(&app)[0],
            (side, "Side projects".to_string(), false, vec![id("c1")])
        );
    }

    #[test]
    fn a_passing_day_rebuilds_the_deleted_view() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        app.delete_session(by_id(&app, "a1"));
        let _ = app.settle();
        let version = app.sidebar_version;
        app.now_unix = NOW + DAY;
        let _ = app.settle();
        assert_eq!(
            app.sidebar_version, version,
            "the normal list shows no days"
        );

        app.set_sidebar_view(SidebarView::Deleted);
        let _ = app.settle();
        let days_left = |app: &App| app.sidebar.sections[0].rows[0].days_left;
        assert_eq!(
            (app.sidebar_version, days_left(&app)),
            (version + 1, Some(5))
        );
        app.now_unix = NOW + 2 * DAY - 1;
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 1, "the same day");
        app.now_unix = NOW + 2 * DAY;
        let _ = app.settle();
        assert_eq!(
            (app.sidebar_version, days_left(&app)),
            (version + 2, Some(4))
        );
        app.now_unix = NOW + 30 * DAY;
        let _ = app.settle();
        assert_eq!(
            (app.sidebar_version, days_left(&app)),
            (version + 3, Some(0))
        );
        app.now_unix = NOW + 31 * DAY;
        let _ = app.settle();
        assert_eq!(app.sidebar_version, version + 3, "0 stays 0");
    }

    #[test]
    fn a_deleted_session_used_again_elsewhere_comes_back() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let a1 = by_id(&app, "a1");
        app.delete_session(a1);
        let since = Some(NOW - RECENT_DAYS * DAY);
        listing(&mut app, since, three_stored());
        assert!(deleted(&app, a1).is_some(), "the same time");
        let mut later = three_stored();
        later[0].last_active_unix = NOW - 30;
        let effects = listing(&mut app, since, later);
        assert_eq!(deleted(&app, a1), None);
        assert_eq!(saves(&effects), 1, "{effects:?}");
        assert!(app.saved().deleted.is_empty());
    }

    /// A recent listing that gives `s1` in `/p` at `last_active_unix`.
    fn lists_s1(app: &mut App, last_active_unix: i64) -> Vec<Effect> {
        let s1 = store("s1", "Hello", "/p", last_active_unix);
        listing(app, Some(app.now_unix - RECENT_DAYS * DAY), vec![s1])
    }

    #[test]
    fn the_apps_own_exit_does_not_count_as_used_again() {
        let (mut app, key) = idle();
        app.delete_session(key);
        app.receive(exit_of(1, true), 0.0);
        lists_s1(&mut app, NOW + OWN_EXIT_SLACK_SECONDS);
        assert!(deleted(&app, key).is_some(), "inside the slack");
        lists_s1(&mut app, NOW + OWN_EXIT_SLACK_SECONDS + 1);
        assert_eq!(deleted(&app, key), None);

        // At quit, before its program has ended.
        let (mut app, key) = idle();
        app.delete_session(key);
        let effects = app.close(NOW + 20);
        let Some(Effect::SaveState(saved)) = effects.first() else {
            panic!("saved first: {effects:?}");
        };
        let seen = NOW + 20 + OWN_EXIT_SLACK_SECONDS;
        assert_eq!(saved.deleted[0].seen_unix, seen);
    }

    #[test]
    fn deleting_just_after_its_own_program_ended_does_not_bring_it_back() {
        let (mut app, key) = idle();
        app.receive(exit_of(1, true), 0.0);
        assert_eq!(session(&app, key).phase, Phase::Resting);
        // The last listing gave an older time.
        app.sessions.get_mut(key).unwrap().last_active_unix = NOW - 3600;
        app.now_unix = NOW + 5;
        let effects = app.delete_session(key);
        assert!(closes(&effects).is_empty());
        assert_eq!(
            deleted(&app, key).unwrap().seen_unix,
            NOW + OWN_EXIT_SLACK_SECONDS
        );
        lists_s1(&mut app, NOW);
        assert!(deleted(&app, key).is_some(), "what its own program wrote");
        lists_s1(&mut app, NOW + OWN_EXIT_SLACK_SECONDS + 60);
        assert_eq!(deleted(&app, key), None);
    }

    /// [`three_stored`] listed and `a1` deleted.
    fn a1_deleted() -> App {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        app.delete_session(by_id(&app, "a1"));
        app
    }

    fn a1_mark() -> SavedDeleted {
        SavedDeleted {
            id: id("a1"),
            at_unix: NOW,
            seen_unix: NOW - 60,
        }
    }

    #[test]
    fn the_marks_survive_a_restart_before_their_sessions_are_listed() {
        let saved = a1_deleted().saved();
        assert_eq!(saved.deleted, vec![a1_mark()]);
        let mut again = restored(saved.clone());
        assert_eq!(again.saved().deleted, saved.deleted);
        let others = three_stored()[1..].to_vec();
        listing(&mut again, Some(NOW - RECENT_DAYS * DAY), others);
        assert_eq!(again.saved().deleted, saved.deleted, "not listed yet");
    }

    #[test]
    fn a_mark_is_applied_when_its_session_is_listed() {
        let mut again = restored(a1_deleted().saved());
        listing(&mut again, Some(NOW - RECENT_DAYS * DAY), three_stored());
        let a1 = by_id(&again, "a1");
        assert_eq!(
            deleted(&again, a1),
            Some(Deleted {
                at_unix: NOW,
                seen_unix: NOW - 60,
                failed_unix: None,
                removing: false,
            })
        );
        assert_eq!(deleted(&again, by_id(&again, "a2")), None);
        assert_eq!(again.saved().deleted, vec![a1_mark()]);
    }

    #[test]
    fn a_mark_for_a_session_the_store_lost_is_dropped_after_the_complete_listing() {
        let mut again = restored(a1_deleted().saved());
        let others = three_stored()[1..].to_vec();
        listing(&mut again, Some(NOW - RECENT_DAYS * DAY), others.clone());
        assert_eq!(again.saved().deleted, vec![a1_mark()]);
        let effects = listing(&mut again, None, others);
        assert!(again.saved().deleted.is_empty());
        assert_eq!(saves(&effects), 1, "{effects:?}");
    }

    #[test]
    fn a_turn_that_ends_after_the_delete_leaves_no_mark_also_after_recovery() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.delete_session(key);
        app.event(1, done(), 0.0);
        assert!(!app.sessions.get(key).unwrap().unseen, "after the turn");
        app.receive(exit_of(1, true), 0.0);
        assert!(!app.sessions.get(key).unwrap().unseen, "after the exit");
        app.recover_session(key);
        assert_eq!(mark(&app, key), Mark::None);
        assert_eq!(app.standing(), Standing::default());

        // Recovering clears what was unseen before the delete, too.
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.delete_session(key);
        app.sessions.get_mut(key).unwrap().unseen = true;
        app.recover_session(key);
        assert!(!app.sessions.get(key).unwrap().unseen);
    }

    #[test]
    fn deleting_an_ended_session_lets_its_conversation_go() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.receive(exit_of(1, false), 0.0);
        assert!(matches!(session(&app, key).phase, Phase::Ended { .. }));
        let effects = app.delete_session(key);
        assert!(closes(&effects).is_empty(), "{effects:?}");
        assert_eq!(drops(&effects), vec![key]);
        assert_eq!(session(&app, key).phase, Phase::Resting);
        assert_eq!(session(&app, key).history, HistoryState::NotLoaded);
        assert!(session(&app, key).conversation.items().is_empty());
    }

    /// The removals `effects` ask for: each id with the time it holds.
    fn removals(effects: &[Effect]) -> Vec<(SessionId, i64)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::RemoveStored {
                    id,
                    unless_after_unix,
                } => Some((id.clone(), *unless_after_unix)),
                _ => None,
            })
            .collect()
    }

    /// The answer to the removal of `text`'s session.
    fn removed(text: &str, result: Result<Removal, AgentError>) -> FromBackend {
        FromBackend::Removed {
            id: id(text),
            result,
        }
    }

    #[test]
    fn a_week_after_deleting_the_session_is_removed() {
        let mut app = a1_deleted();
        let a1 = by_id(&app, "a1");
        let seen = deleted(&app, a1).unwrap().seen_unix;
        assert_eq!(removals(&app.tick(NOW + DELETED_SECONDS - 1)), vec![]);
        let next = NOW + DELETED_SECONDS - 1 + PURGE_SECONDS;
        assert_eq!(removals(&app.tick(next - 1)), vec![], "once a minute");
        assert_eq!(removals(&app.tick(next)), vec![(id("a1"), seen)]);
        assert!(deleted(&app, a1).unwrap().removing);
        assert_eq!(
            removals(&app.tick(next + PURGE_SECONDS)),
            vec![],
            "being removed"
        );

        // The first tick a week after the delete.
        let mut app = a1_deleted();
        let effects = app.tick(NOW + DELETED_SECONDS);
        assert_eq!(removals(&effects), vec![(id("a1"), seen)]);
        assert_eq!(saves(&effects), 0, "nothing kept changed: {effects:?}");
    }

    #[test]
    fn a_removed_session_is_forgotten() {
        for removal in [Removal::Removed, Removal::Missing] {
            // Only the recent listing: every placement is kept.
            let mut app = main_screen();
            listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
            let (side, _) = app.new_group("Side projects");
            let a1 = by_id(&app, "a1");
            app.move_session(a1, &side);
            app.rename(a1, "Parser");
            assert_eq!(app.saved().names, vec![(id("a1"), "Parser".to_string())]);
            app.delete_session(a1);
            app.tick(NOW + DELETED_SECONDS);
            let effects = app.receive(removed("a1", Ok(removal)), 0.0);
            assert_eq!(app.sessions.by_id(&id("a1")), None, "{removal:?}");
            assert_eq!(effects, vec![save(&app)], "{removal:?}");
            let saved = app.saved();
            assert!(saved.deleted.is_empty(), "{removal:?}");
            assert!(saved.placed.is_empty(), "{removal:?}");
            assert!(saved.names.is_empty(), "{removal:?}");
            assert_eq!(saved.groups.len(), 1, "its group stays");
            assert!(app.receive(removed("a1", Ok(removal)), 0.0).is_empty());
        }
    }

    #[test]
    fn a_session_that_changed_is_recovered_not_removed() {
        let mut app = a1_deleted();
        let a1 = by_id(&app, "a1");
        app.tick(NOW + DELETED_SECONDS);
        let effects = app.receive(removed("a1", Ok(Removal::Changed)), 0.0);
        assert_eq!(deleted(&app, a1), None);
        assert_eq!(effects, vec![save(&app)]);
        assert!(app.saved().deleted.is_empty());
        let later = NOW + DELETED_SECONDS + PURGE_SECONDS;
        assert_eq!(removals(&app.tick(later)), vec![]);
    }

    #[test]
    fn a_failed_removal_is_tried_again_after_an_hour_and_not_before() {
        let mut app = a1_deleted();
        let a1 = by_id(&app, "a1");
        let week = NOW + DELETED_SECONDS;
        assert_eq!(removals(&app.tick(week)).len(), 1);
        let failed = Err(AgentError::Io("the disk is gone".into()));
        let effects = app.receive(removed("a1", failed), 0.0);
        assert_eq!(effects, vec![]);
        let mark = deleted(&app, a1).expect("still deleted");
        assert_eq!((mark.failed_unix, mark.removing), (Some(week), false));
        assert_eq!(removals(&app.tick(week + PURGE_SECONDS)), vec![]);
        let hour = week + PURGE_RETRY_SECONDS;
        assert_eq!(removals(&app.tick(hour - 1)), vec![]);
        assert_eq!(
            removals(&app.tick(hour - 1 + PURGE_SECONDS)),
            vec![(id("a1"), NOW - 60)]
        );
    }

    #[test]
    fn a_session_whose_program_still_runs_is_not_removed_yet() {
        let (mut app, key) = idle();
        app.delete_session(key);
        let week = NOW + DELETED_SECONDS;
        assert_eq!(removals(&app.tick(week)), vec![]);
        // Its week is over: it goes when its program has, without waiting for the tick.
        let effects = app.receive(exit_of(1, true), 0.0);
        let seen = week + OWN_EXIT_SLACK_SECONDS;
        assert_eq!(removals(&effects), vec![(id("s1"), seen)]);
        assert_eq!(
            removals(&app.tick(week + PURGE_SECONDS)),
            vec![],
            "asked once"
        );

        // Within its week the exit removes nothing.
        let (mut app, key) = idle();
        app.delete_session(key);
        assert_eq!(removals(&app.receive(exit_of(1, true), 0.0)), vec![]);
        assert_eq!(removals(&app.tick(NOW + PURGE_SECONDS)), vec![]);
    }

    #[test]
    fn after_delete_all_a_running_session_goes_at_its_exit() {
        let (mut app, key) = idle();
        app.delete_session(key);
        assert_eq!(removals(&app.delete_all()), vec![], "its program runs");
        // The tick has just run: the exit does not wait a minute for the next.
        assert_eq!(removals(&app.tick(NOW)), vec![]);
        let effects = app.receive(exit_of(1, true), 0.0);
        let seen = NOW + OWN_EXIT_SLACK_SECONDS;
        assert_eq!(removals(&effects), vec![(id("s1"), seen)]);
        assert!(deleted(&app, key).unwrap().removing);
        assert_eq!(removals(&app.tick(NOW + 1)), vec![], "asked once");
    }

    #[test]
    fn delete_all_leaves_no_header_behind() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        app.move_session(by_id(&app, "a1"), &side);
        app.move_session(by_id(&app, "a2"), &side);
        app.delete_group(&side);
        app.set_sidebar_view(SidebarView::Deleted);
        let _ = app.settle();
        assert_eq!(layout(&app).len(), 1);
        // The tick has just run, so nothing here rides on the next one.
        app.tick(NOW);

        let effects = app.delete_all();
        assert_eq!(removals(&effects).len(), 2);
        let effects = app.receive(removed("a1", Ok(Removal::Removed)), 0.0);
        assert_eq!(saves(&effects), 1, "{effects:?}");
        assert_eq!(
            app.groups.deleted_unix(&side),
            Some(NOW - DELETED_SECONDS),
            "a2 is still in it"
        );
        let effects = app.receive(removed("a2", Ok(Removal::Missing)), 0.0);
        assert_eq!(saves(&effects), 1, "{effects:?}");
        assert_eq!(effects.last(), Some(&save(&app)));
        assert!(
            app.saved().groups.is_empty(),
            "the group went with its last"
        );
        let _ = app.settle();
        assert_eq!(layout(&app), vec![], "no header with no rows");
        assert_eq!(app.sidebar.deleted, 0);
    }

    #[test]
    fn delete_all_removes_everything_in_the_deleted_view_at_once() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (a1, a2, b1) = (by_id(&app, "a1"), by_id(&app, "a2"), by_id(&app, "b1"));
        let (side, _) = app.new_group("Side projects");
        app.move_session(b1, &side);
        app.delete_session(a1);
        app.tick(NOW + 2 * DAY);
        app.delete_session(a2);
        let now = NOW + 5 * DAY;
        app.tick(now);
        app.delete_group(&side);
        // b1's removal failed a minute ago.
        let mut entry = app.sessions.get_mut(b1).unwrap();
        let mark = entry.deleted.unwrap();
        entry.deleted = Some(Deleted {
            failed_unix: Some(now - 60),
            ..mark
        });
        drop(entry);

        let effects = app.delete_all();
        assert_eq!(
            removals(&effects),
            vec![
                (id("a1"), NOW - 60),
                (id("a2"), NOW - 120),
                (id("b1"), NOW - 180)
            ]
        );
        assert_eq!(saves(&effects), 1, "{effects:?}");
        assert_eq!(effects.last(), Some(&save(&app)));
        let ended = now - DELETED_SECONDS;
        for key in [a1, a2, b1] {
            let mark = deleted(&app, key).unwrap();
            assert_eq!(
                (mark.at_unix, mark.failed_unix, mark.removing),
                (ended, None, true)
            );
        }
        assert_eq!(app.groups.deleted_unix(&side), Some(ended));
        assert!(app.saved().deleted.iter().all(|mark| mark.at_unix == ended));
        assert_eq!(removals(&app.delete_all()), vec![], "asked once");
    }

    #[test]
    fn an_emptied_deleted_group_loses_its_record() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        let (archive, _) = app.new_group("Archive");
        app.move_session(by_id(&app, "a1"), &side);
        app.delete_group(&side);
        app.delete_group(&archive);
        let week = NOW + DELETED_SECONDS;
        let effects = app.tick(week - 30);
        assert_eq!(saves(&effects), 0, "{effects:?}");
        assert_eq!(app.saved().groups.len(), 2);

        // The empty one goes at its week's end; the other holds a1 until a1 is removed.
        let effects = app.tick(week + 30);
        assert_eq!(removals(&effects), vec![(id("a1"), NOW - 60)]);
        assert_eq!(saves(&effects), 1, "{effects:?}");
        assert_eq!(effects.last(), Some(&save(&app)));
        let names: Vec<Option<String>> = app.saved().groups.into_iter().map(|g| g.name).collect();
        assert_eq!(names, vec![Some("Side projects".to_string())]);
        assert_eq!(app.groups.deleted_unix(&archive), None);

        // With the answer, not at the next tick.
        let effects = app.receive(removed("a1", Ok(Removal::Removed)), 0.0);
        assert_eq!(effects, vec![save(&app)]);
        assert!(app.saved().groups.is_empty());
        assert_eq!(app.groups.deleted_unix(&side), None);
        assert_eq!(saves(&app.tick(week + 30 + PURGE_SECONDS)), 0);
    }

    #[test]
    fn an_answer_nobody_waits_for_is_dropped() {
        // A session that is not deleted.
        let mut app = a1_deleted();
        let a1 = by_id(&app, "a1");
        app.recover_session(a1);
        let effects = app.receive(removed("a1", Ok(Removal::Removed)), 0.0);
        assert_eq!(effects, vec![]);
        assert_eq!(deleted(&app, a1), None);
        assert_eq!(by_id(&app, "a1"), a1);

        // A deleted one whose removal was not asked for.
        app.delete_session(a1);
        let effects = app.receive(removed("a1", Ok(Removal::Removed)), 0.0);
        assert_eq!(effects, vec![]);
        assert!(deleted(&app, a1).is_some());

        // A session no longer known.
        let effects = app.receive(removed("z9", Ok(Removal::Removed)), 0.0);
        assert_eq!(effects, vec![]);
    }

    #[test]
    fn recovering_a_group_also_recovers_what_was_not_listed_yet() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        app.move_session(by_id(&app, "a1"), &side);
        app.delete_group(&side);

        let mut again = restored(app.saved());
        let others = three_stored()[1..].to_vec();
        listing(&mut again, Some(NOW - RECENT_DAYS * DAY), others);
        let effects = again.recover_group(&side);
        assert_eq!(effects, vec![save(&again)]);
        assert!(again.saved().deleted.is_empty());
        listing(&mut again, None, three_stored());
        let a1 = by_id(&again, "a1");
        assert_eq!(deleted(&again, a1), None);
        assert_eq!(group_of(&again, a1), side);
        assert_eq!(again.sessions_of(&side), vec![a1]);
    }

    #[test]
    fn a_new_session_is_not_started_in_a_deleted_group() {
        let mut app = main_screen();
        list_both(&mut app, Vec::new());
        let (side, _) = app.new_group("Side projects");
        app.delete_group(&side);
        assert!(app.groups.deleted_unix(&side).is_some());
        let effects = app.new_session_in(
            "/home/alex/notes".into(),
            &side,
            Place::Focused,
            Leaving::ForgetIfEmpty,
        );
        assert_eq!(effects, vec![]);
        assert!(app.sessions.is_empty());
        assert_eq!(app.focused(), None);
    }

    /// [`ready`] with "Fix the parser" for a title and "hello" sent, the window closed and
    /// settled: a turn runs while the user is not looking.
    fn away() -> (App, SessionKey) {
        let (mut app, key) = ready();
        app.event(1, SessionEvent::TitleChanged("Fix the parser".into()), 0.0);
        app.submit(key, "hello");
        app.set_window(false);
        assert_eq!(app.settle(), vec![]);
        (app, key)
    }

    fn notify(key: SessionKey, title: &str, alert: Alert) -> Effect {
        Effect::Notify {
            key,
            title: title.into(),
            alert,
        }
    }

    fn alerted(app: &App, key: SessionKey) -> Option<Alert> {
        app.sessions.get(key).unwrap().alerted
    }

    #[test]
    fn a_turn_that_ends_while_not_looking_is_notified_once() {
        let (mut app, key) = away();
        app.event(1, done(), 0.0);
        assert_eq!(
            app.settle(),
            vec![notify(key, "Fix the parser", Alert::Ready)]
        );
        assert_eq!(
            row_of(&app, key).title,
            "Fix the parser",
            "as the sidebar shows it"
        );
        assert_eq!(alerted(&app, key), Some(Alert::Ready));
        app.event(1, done(), 0.0);
        assert_eq!(app.settle(), vec![], "a second event gives none");
        assert_eq!(app.settle(), vec![]);
        assert_eq!(alerted(&app, key), Some(Alert::Ready));
    }

    #[test]
    fn a_request_while_not_looking_is_notified() {
        let (mut app, key) = ready();
        app.submit(key, "hello");
        app.set_active(false);
        assert_eq!(app.settle(), vec![]);
        app.event(1, permission("r1"), 0.0);
        let effects = app.settle();
        assert_eq!(effects, vec![notify(key, "hello", Alert::NeedsInput)]);
        assert_eq!(row_of(&app, key).title, "hello", "as the sidebar shows it");
        assert_eq!(alerted(&app, key), Some(Alert::NeedsInput));

        // The store's answer that another app may have it open asks the user too.
        let (mut app, key) = shows_stored();
        app.submit(key, "carry on");
        app.set_window(false);
        assert_eq!(app.settle(), vec![]);
        app.receive(
            FromBackend::Activity {
                key,
                result: Ok(Some(NOW - 30)),
            },
            0.0,
        );
        assert_eq!(mark(&app, key), Mark::NeedsYou);
        assert_eq!(
            app.settle(),
            vec![notify(key, "Fix the parser", Alert::NeedsInput)]
        );
    }

    #[test]
    fn nothing_is_notified_while_looking() {
        let (mut app, a, b) = two_tiles();
        assert!(app.looking());
        let _ = app.settle();
        app.event(open_of(&app, a), done(), 0.0);
        app.event(open_of(&app, b), done(), 0.0);
        assert_eq!(mark(&app, b), Mark::Finished, "ready in another tile");
        assert_eq!(app.settle(), vec![], "also not for another tile");
        app.submit(b, "again");
        app.event(open_of(&app, b), permission("r1"), 0.0);
        assert_eq!(mark(&app, b), Mark::NeedsYou);
        assert_eq!(app.settle(), vec![]);
        // What was ready or needed input before the user left is not notified when they leave.
        app.set_window(false);
        assert_eq!(app.settle(), vec![]);
        app.set_window(true);
        app.set_active(false);
        assert_eq!(app.settle(), vec![]);
        assert_eq!(alerted(&app, b), None);
    }

    #[test]
    fn ready_then_needing_input_notifies_again() {
        let (mut app, key) = away();
        app.event(1, done(), 0.0);
        assert_eq!(
            app.settle(),
            vec![notify(key, "Fix the parser", Alert::Ready)]
        );
        // A turn sent from another device asks for something.
        app.event(1, SessionEvent::TurnStarted, 0.0);
        app.event(1, permission("r1"), 0.0);
        assert_eq!(mark(&app, key), Mark::NeedsYou);
        assert_eq!(
            app.settle(),
            vec![notify(key, "Fix the parser", Alert::NeedsInput)],
            "it replaces the old one"
        );
        assert_eq!(alerted(&app, key), Some(Alert::NeedsInput));
    }

    #[test]
    fn answering_takes_the_notification_back() {
        let (mut app, key) = away();
        app.event(1, permission("r1"), 0.0);
        assert_eq!(
            app.settle(),
            vec![notify(key, "Fix the parser", Alert::NeedsInput)]
        );
        app.answer(
            key,
            &RequestId("r1".into()),
            Answer::Allow { always: false },
        );
        assert_eq!(mark(&app, key), Mark::Working);
        assert_eq!(app.settle(), vec![Effect::Unnotify { key }]);
        assert_eq!(alerted(&app, key), None);
        // Attending to a ready one takes it back too.
        app.event(1, done(), 0.0);
        assert_eq!(
            app.settle(),
            vec![notify(key, "Fix the parser", Alert::Ready)]
        );
        app.attend(key);
        assert_eq!(app.settle(), vec![Effect::Unnotify { key }]);
        assert_eq!(app.settle(), vec![]);
    }

    #[test]
    fn coming_back_takes_them_all_back_and_keeps_the_marks() {
        let (mut app, a, b) = two_tiles();
        app.set_window(false);
        let _ = app.settle();
        app.event(open_of(&app, a), done(), 0.0);
        app.event(open_of(&app, b), permission("r1"), 0.0);
        assert_eq!(app.settle().len(), 2);
        app.set_active(false);
        app.set_window(true);
        assert_eq!(
            app.settle(),
            vec![],
            "a window without the keyboard is not looked at"
        );
        app.set_active(true);
        assert_eq!(app.settle(), vec![Effect::UnnotifyAll]);
        assert_eq!((alerted(&app, a), alerted(&app, b)), (None, None));
        assert_eq!(
            (mark(&app, a), mark(&app, b)),
            (Mark::Finished, Mark::NeedsYou)
        );
        assert_eq!(app.settle(), vec![], "nothing is out");
        // Leaving again notifies none of them: the user saw them.
        app.set_active(false);
        assert_eq!(app.settle(), vec![]);
        // The window coming back while it has the keyboard takes them back as well.
        app.answer(b, &RequestId("r1".into()), Answer::Allow { always: false });
        app.event(open_of(&app, b), done(), 0.0);
        assert_eq!(app.settle().len(), 1);
        app.set_window(false);
        app.set_active(true);
        assert_eq!(app.settle(), vec![]);
        app.set_window(true);
        assert_eq!(app.settle(), vec![Effect::UnnotifyAll]);
        assert_eq!(mark(&app, b), Mark::Finished);
    }

    #[test]
    fn deleting_an_alerted_session_takes_its_notification_back() {
        let (mut app, key) = away();
        app.event(1, done(), 0.0);
        let _ = app.settle();
        assert_eq!(alerted(&app, key), Some(Alert::Ready));
        app.delete_session(key);
        assert!(app.is_deleted(key));
        assert_eq!(app.settle(), vec![Effect::Unnotify { key }]);
        assert_eq!(alerted(&app, key), None);

        // One the store has nothing of is forgotten: its notification goes all the same.
        let (mut app, key) = ready();
        app.set_window(false);
        let _ = app.settle();
        app.event(1, exited(false), 0.0);
        assert_eq!(app.settle(), vec![notify(key, "New session", Alert::Ready)]);
        app.delete_session(key);
        assert!(app.sessions.get(key).is_none());
        assert_eq!(app.settle(), vec![Effect::Unnotify { key }]);
        assert_eq!(app.settle(), vec![]);
    }

    #[test]
    fn a_deleted_session_is_never_notified() {
        let (mut app, key) = away();
        app.delete_session(key);
        assert_eq!(app.settle(), vec![]);
        app.event(1, permission("r1"), 0.0);
        app.event(1, done(), 0.0);
        app.event(1, exited(false), 0.0);
        assert_eq!(app.settle(), vec![]);
        assert_eq!(alerted(&app, key), None);
        app.set_window(true);
        assert_eq!(app.settle(), vec![]);
    }

    #[test]
    fn ten_sessions_ending_at_once_give_ten_notifications() {
        let mut app = main_screen();
        let keys: Vec<SessionKey> = (0..10)
            .map(|n| busy(&mut app, &format!("/home/alex/s{n}")))
            .collect();
        app.set_window(false);
        assert_eq!(app.settle(), vec![]);
        let failed = || SessionEvent::TurnFailed {
            reason: "overloaded".into(),
        };
        let mut wanted = Vec::new();
        for (n, &key) in keys.iter().enumerate() {
            let (event, alert) = match n % 4 {
                0 => (done(), Alert::Ready),
                1 => (failed(), Alert::Ready),
                2 => (exited(false), Alert::Ready),
                _ => (permission("r1"), Alert::NeedsInput),
            };
            app.event(open_of(&app, key), event, 0.0);
            wanted.push(notify(key, "go", alert));
        }
        assert_eq!(app.settle(), wanted);
        assert_eq!(app.settle(), vec![]);
    }

    #[test]
    fn a_press_on_a_notification_focuses_and_attends_the_session() {
        // In a tile.
        let (mut app, a, b) = two_tiles();
        app.set_window(false);
        let _ = app.settle();
        app.event(open_of(&app, b), done(), 0.0);
        assert_eq!(app.settle(), vec![notify(b, "go", Alert::Ready)]);
        app.alert_pressed(b, Leaving::ForgetIfEmpty);
        assert_eq!(app.focused(), Some(b));
        assert_eq!(tile_keys(&app), vec![a, b]);
        assert_eq!(mark(&app, b), Mark::None, "attended");
        assert_eq!(app.settle(), vec![Effect::Unnotify { key: b }]);

        // In none: it shows in the focused tile, as a press on its row shows it.
        let mut app = main_screen();
        let c = busy(&mut app, "/c");
        let d = busy(&mut app, "/d");
        assert_eq!(tile_keys(&app), vec![d]);
        app.set_window(false);
        let _ = app.settle();
        app.event(open_of(&app, c), done(), 0.0);
        assert_eq!(app.settle(), vec![notify(c, "go", Alert::Ready)]);
        let effects = app.alert_pressed(c, Leaving::ForgetIfEmpty);
        let mut row_press = main_screen();
        let c2 = busy(&mut row_press, "/c");
        busy(&mut row_press, "/d");
        row_press.event(open_of(&row_press, c2), done(), 0.0);
        assert_eq!(
            effects,
            row_press.show(c2, Place::Focused, Leaving::ForgetIfEmpty),
            "the effects of a press on its row"
        );
        assert_eq!(tile_keys(&app), vec![c]);
        assert_eq!(app.focused(), Some(c));
        assert_eq!(mark(&app, c), Mark::None, "attended");
        let effects = app.settle();
        assert_eq!(
            effects,
            vec![Effect::Unnotify { key: c }],
            "and nothing notified"
        );
    }

    #[test]
    fn a_press_for_a_session_that_is_gone_does_nothing() {
        let (mut app, a, b) = two_tiles();
        app.set_window(false);
        let _ = app.settle();
        app.event(open_of(&app, b), done(), 0.0);
        let _ = app.settle();
        app.delete_session(b);
        assert_eq!(app.settle(), vec![Effect::Unnotify { key: b }]);
        let placement = app.tiles.placement();
        assert_eq!(app.alert_pressed(b, Leaving::ForgetIfEmpty), vec![]);
        assert_eq!(
            app.alert_pressed(SessionKey(999), Leaving::ForgetIfEmpty),
            vec![]
        );
        assert_eq!(app.tiles.placement(), placement);
        assert_eq!(app.focused(), Some(a));
        assert_eq!(app.settle(), vec![]);
    }

    /// A listing since `since_unix` that failed because the store could not be found.
    fn no_store(app: &mut App, since_unix: Option<i64>) -> Vec<Effect> {
        app.receive(
            FromBackend::Sessions {
                since_unix,
                result: Err(AgentError::Io("the store could not be found".into())),
            },
            0.0,
        )
    }

    #[test]
    fn a_failed_listing_forgets_no_deletion() {
        // The last run deleted a1, named a2 and placed b1 in a group of the user's.
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        app.move_session(by_id(&app, "b1"), &side);
        app.rename(by_id(&app, "a2"), "Docs");
        app.delete_session(by_id(&app, "a1"));
        let saved = app.saved();
        assert_eq!(saved.deleted, vec![a1_mark()]);
        assert_eq!(saved.names, vec![(id("a2"), "Docs".to_string())]);
        assert_eq!(saved.placed, vec![(id("b1"), side)]);

        // This run finds no store: every listing fails, the complete one too.
        let mut again = restored(saved.clone());
        let kept = |app: &App| {
            let now = app.saved();
            (now.deleted, now.names, now.placed, now.groups)
        };
        let before = kept(&again);
        assert_eq!(
            before,
            (
                saved.deleted.clone(),
                saved.names.clone(),
                saved.placed.clone(),
                saved.groups.clone()
            )
        );
        assert_eq!(
            saves(&no_store(&mut again, Some(NOW - RECENT_DAYS * DAY))),
            0
        );
        assert_eq!(kept(&again), before, "after the recent listing");
        assert_eq!(saves(&no_store(&mut again, None)), 0);
        assert_eq!(kept(&again), before, "after the complete listing");
        assert!(!again.listed_all);
        assert!(matches!(again.listing, Listing::Failed(_)));
        // Nothing is removed or dropped at the ticks either, also a week on.
        let effects = again.tick(NOW + DELETED_SECONDS + PURGE_SECONDS);
        assert_eq!(removals(&effects), vec![]);
        assert_eq!(kept(&again), before, "after a week of ticks");

        // The store is back in a later run: the deletion is still there.
        let mut later = restored(again.saved());
        list_both(&mut later, three_stored());
        assert!(deleted(&later, by_id(&later, "a1")).is_some());
        assert_eq!(
            session(&later, by_id(&later, "a2")).user_title.as_deref(),
            Some("Docs")
        );
    }

    #[test]
    fn an_empty_full_listing_forgets_no_deletion() {
        // The last run deleted a1, named a2, placed b1 in a group of the user's, and deleted
        // another group of theirs, with a session placed in it, more than a week ago.
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        let (archive, _) = app.new_group("Archive");
        app.move_session(by_id(&app, "b1"), &side);
        app.rename(by_id(&app, "a2"), "Docs");
        app.delete_session(by_id(&app, "a1"));
        let mut saved = app.saved();
        saved.placed.push((id("old"), archive.clone()));
        for group in &mut saved.groups {
            if group.id == archive {
                group.deleted_unix = Some(NOW - 2 * DELETED_SECONDS);
            }
        }
        let kept = |app: &App| {
            let now = app.saved();
            (now.deleted, now.names, now.placed, now.groups)
        };

        // This run finds a store that lists as empty: a folder with no `projects` in it, as
        // an empty mount point is. Every listing succeeds and holds nothing.
        let mut again = restored(saved.clone());
        let before = kept(&again);
        assert_eq!(before.0, vec![a1_mark()]);
        assert_eq!(before.1, vec![(id("a2"), "Docs".to_string())]);
        assert_eq!(before.2.len(), 2);
        assert_eq!(before.3.len(), 2);
        let effects = listing(&mut again, Some(NOW - RECENT_DAYS * DAY), Vec::new());
        assert_eq!(saves(&effects), 0, "{effects:?}");
        let effects = listing(&mut again, None, Vec::new());
        assert_eq!(saves(&effects), 0, "{effects:?}");
        assert_eq!(kept(&again), before, "after the complete listing");
        assert!(again.listed_all, "the listing itself is in");
        // A row listed before stays as well: with it would go its deleted mark.
        let mut third = restored(saved.clone());
        listing(&mut third, Some(NOW - RECENT_DAYS * DAY), three_stored());
        listing(&mut third, None, Vec::new());
        assert!(deleted(&third, by_id(&third, "a1")).is_some());
        assert_eq!(third.sessions.len(), 3);
        assert_eq!(kept(&third), before);
        // Nor at the ticks: the deleted group's week is long over, and it keeps its record
        // and the place in it.
        let effects = again.tick(NOW + PURGE_SECONDS);
        assert_eq!(removals(&effects), vec![]);
        assert_eq!(saves(&effects), 0, "{effects:?}");
        assert_eq!(kept(&again), before, "after a tick");
        // A session started meanwhile and listed changes nothing of that.
        let new = store("n1", "New work", "/home/alex/project", NOW);
        listing(&mut again, Some(NOW - RECENT_DAYS * DAY), vec![new]);
        assert_eq!(kept(&again), before, "after a later recent listing");

        // The store is back in a later run: the deletion, the name and the places are there,
        // and what the store really no longer has is forgotten then.
        let mut later = restored(again.saved());
        list_both(&mut later, three_stored());
        assert!(deleted(&later, by_id(&later, "a1")).is_some());
        assert_eq!(
            session(&later, by_id(&later, "a2")).user_title.as_deref(),
            Some("Docs")
        );
        assert_eq!(group_of(&later, by_id(&later, "b1")), side);
        assert_eq!(
            later.saved().placed,
            vec![(id("b1"), side)],
            "\"old\" is gone"
        );
    }

    #[test]
    fn a_session_being_removed_is_not_recovered_by_the_user() {
        let mut app = a1_deleted();
        let a1 = by_id(&app, "a1");
        assert_eq!(removals(&app.tick(NOW + DELETED_SECONDS)).len(), 1);
        assert!(deleted(&app, a1).unwrap().removing);
        assert_eq!(app.recover_session(a1), vec![], "its file may be going");
        assert!(deleted(&app, a1).unwrap().removing);
        // The answer is still waited for, and forgets it.
        let effects = app.receive(removed("a1", Ok(Removal::Removed)), 0.0);
        assert_eq!(effects, vec![save(&app)]);
        assert_eq!(app.sessions.by_id(&id("a1")), None);

        // "Recover group" brings back the group and its other sessions, not that one.
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        let (a1, a2) = (by_id(&app, "a1"), by_id(&app, "a2"));
        app.move_session(a1, &side);
        app.move_session(a2, &side);
        app.delete_group(&side);
        let mark = deleted(&app, a1).unwrap();
        app.sessions.get_mut(a1).unwrap().deleted = Some(Deleted {
            removing: true,
            ..mark
        });
        app.recover_group(&side);
        assert_eq!(app.groups.deleted_unix(&side), None);
        assert_eq!(deleted(&app, a2), None);
        assert!(deleted(&app, a1).is_some_and(|mark| mark.removing));

        // Nor does a listing that shows it used again: the removal's own answer says so.
        let used = store("a1", "Fix the parser", "/home/alex/project", NOW + 5);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![used]);
        assert!(deleted(&app, a1).is_some_and(|mark| mark.removing));
        // Once the removal has failed, the session can be recovered again.
        let failed = Err(AgentError::Io("the disk is gone".into()));
        app.receive(removed("a1", failed), 0.0);
        assert_eq!(app.recover_session(a1), vec![save(&app)]);
        assert_eq!(deleted(&app, a1), None);
    }

    #[test]
    fn a_removal_that_answers_changed_still_recovers() {
        let mut app = main_screen();
        list_both(&mut app, three_stored());
        let (side, _) = app.new_group("Side projects");
        let a1 = by_id(&app, "a1");
        app.move_session(a1, &side);
        app.delete_group(&side);
        app.tick(NOW + DELETED_SECONDS);
        assert!(deleted(&app, a1).unwrap().removing);
        let effects = app.receive(removed("a1", Ok(Removal::Changed)), 0.0);
        assert_eq!(deleted(&app, a1), None, "used again elsewhere");
        assert_eq!(
            app.groups.deleted_unix(&side),
            None,
            "and its group with it"
        );
        assert_eq!(effects, vec![save(&app)]);
        assert_eq!(app.sessions_of(&side), vec![a1]);
    }

    #[test]
    fn a_folders_group_never_counts_as_deleted() {
        // A state edited by hand, or damaged: a folder's group with a name and a deleted mark.
        let saved = Saved {
            version: SAVED_VERSION,
            groups: vec![Group {
                id: project(),
                name: Some("Work".into()),
                collapsed: false,
                hidden: false,
                deleted_unix: Some(NOW - 30 * DAY),
            }],
            ..Saved::default()
        };
        let mut app = restored(saved);
        assert_eq!(app.groups.deleted_unix(&project()), None);
        assert_eq!(app.groups.deleted(), vec![]);
        list_both(&mut app, three_stored());
        for text in ["a1", "a2", "b1"] {
            assert_eq!(deleted(&app, by_id(&app, text)), None, "{text}");
        }
        assert_eq!(removals(&app.tick(NOW + PURGE_SECONDS)), vec![]);
        let saved = app.saved();
        assert!(saved.deleted.is_empty());
        assert_eq!(saved.groups.len(), 1, "its name is kept");
        assert_eq!(saved.groups[0].deleted_unix, None, "and the mark is not");
    }

    #[test]
    fn a_group_is_not_deleted_before_the_full_list_is_in() {
        let mut app = main_screen();
        let _ = app.settle();
        assert!(!app.sidebar.complete);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        let (side, _) = app.new_group("Side projects");
        let a1 = by_id(&app, "a1");
        app.move_session(a1, &side);
        let _ = app.settle();
        assert!(!app.sidebar.complete, "only the recent sessions are listed");

        // An older session of the folder is still to come: neither kind of group goes yet.
        for group in [project(), side.clone()] {
            assert_eq!(app.delete_group(&group), vec![], "{group:?}");
        }
        assert_eq!(app.groups.deleted_unix(&side), None);
        for text in ["a1", "a2", "b1"] {
            assert_eq!(deleted(&app, by_id(&app, text)), None, "{text}");
        }

        // A complete listing that fails leaves it so.
        no_store(&mut app, None);
        let _ = app.settle();
        assert!(!app.sidebar.complete);
        assert_eq!(app.delete_group(&project()), vec![]);

        let version = app.sidebar_version;
        listing(&mut app, None, with_an_older_one());
        let _ = app.settle();
        assert!(app.sidebar.complete);
        assert!(app.sidebar_version > version);
        assert_eq!(app.sessions_of(&project()).len(), 2, "a2 and the older a3");
        assert_eq!(saves(&app.delete_group(&project())), 1);
        assert!(
            deleted(&app, by_id(&app, "a3")).is_some(),
            "the older one too"
        );
        assert_eq!(saves(&app.delete_group(&side)), 1);
        assert!(deleted(&app, a1).is_some());
    }

    #[test]
    fn a_kept_name_goes_with_a_session_the_complete_listing_does_not_give() {
        // The last run named a1, placed it and deleted it; its removal ran, and the app was
        // closed before the answer came. Nothing of it may stay in the saved state.
        let mut app = main_screen();
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        let (side, _) = app.new_group("Side projects");
        let a1 = by_id(&app, "a1");
        app.move_session(a1, &side);
        app.rename(a1, "Parser");
        app.delete_session(a1);
        assert_eq!(removals(&app.tick(NOW + DELETED_SECONDS)).len(), 1);
        let saved = app.saved();
        assert_eq!(saved.names, vec![(id("a1"), "Parser".to_string())]);
        assert_eq!((saved.deleted.len(), saved.placed.len()), (1, 1));

        let mut again = restored(saved);
        let others = three_stored()[1..].to_vec();
        listing(&mut again, Some(NOW - RECENT_DAYS * DAY), others.clone());
        assert_eq!(again.saved().names.len(), 1, "it may still be listed");
        let effects = listing(&mut again, None, others);
        assert_eq!(saves(&effects), 1, "{effects:?}");
        let saved = again.saved();
        assert_eq!(saved.names, vec![]);
        assert_eq!(saved.deleted, vec![]);
        assert_eq!(saved.placed, vec![]);
    }

    #[test]
    fn attaching_clears_ready_only_when_something_was_taken() {
        let (mut app, key) = ready_unseen();
        let effects = app.attach(key, vec![at("/home/alex/notes.txt")]);
        assert_eq!(effects.len(), 1);
        assert_eq!(mark(&app, key), Mark::None, "the user acted on it");

        // The same path again adds no chip: nothing was taken, so nothing is cleared.
        app.sessions.get_mut(key).unwrap().unseen = true;
        assert_eq!(app.attach(key, vec![at("/home/alex/notes.txt")]), vec![]);
        assert_eq!(mark(&app, key), Mark::Finished);
        assert_eq!(app.attach(key, Vec::new()), vec![]);
        assert_eq!(mark(&app, key), Mark::Finished);

        // A session that takes no message takes no chip.
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.set_active(false);
        app.receive(exit_of(1, false), 0.0);
        app.set_active(true);
        assert_eq!(mark(&app, key), Mark::Finished);
        assert_eq!(app.attach(key, vec![pasted()]), vec![]);
        assert_eq!(mark(&app, key), Mark::Finished);
    }

    #[test]
    fn taking_the_queued_message_back_clears_ready() {
        // A turn runs with "later" queued behind it; another tile has the focus.
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        app.submit(a, "later");
        assert_eq!(queued_text(session(&app, a)), Some("later"));
        app.new_session("/b".into(), Place::Side, Leaving::ForgetIfEmpty);
        app.sessions.get_mut(a).unwrap().unseen = true;

        let (effects, text) = app.edit_queued(a);
        assert_eq!(text.as_deref(), Some("later"));
        assert_eq!(effects, vec![]);
        assert!(session(&app, a).queued.is_none());
        assert!(!app.sessions.get(a).unwrap().unseen, "the user acted on it");

        // Nothing queued: nothing is taken and nothing cleared.
        app.sessions.get_mut(a).unwrap().unseen = true;
        assert_eq!(app.edit_queued(a), (vec![], None));
        assert!(app.sessions.get(a).unwrap().unseen);
        assert_eq!(app.edit_queued(SessionKey(999)), (vec![], None));
    }

    #[test]
    fn what_deleting_a_session_costs() {
        // Stored and at rest: nothing.
        let (mut app, stored) = shows_stored();
        assert_eq!(app.deleting(stored), Some(Deleting::Kept));
        // A message held by the "open elsewhere" question, and nothing else.
        let (asked, key, _) = checked(5);
        assert!(asked.prompt(key).is_some());
        assert_eq!(asked.deleting(key), Some(Deleting::DropsMessage));
        // Queued while the store is asked about it.
        let (mut queued, key) = shows_stored();
        queued.submit(key, "carry on");
        assert_eq!(queued.deleting(key), Some(Deleting::DropsMessage));
        // Never delivered: its program ended uncleanly before it took the message.
        let (mut refused, key) = sent_hello();
        refused.receive(closed_send(1, "hello", vec![]), 0.0);
        refused.receive(exit_of(1, false), 0.0);
        assert!(!session(&refused, key).undelivered.is_empty());
        assert_eq!(refused.deleting(key), Some(Deleting::DropsMessage));

        // A turn runs, a request waits, a background task runs, Remote Control is on.
        let (mut working, key) = idle();
        assert_eq!(working.deleting(key), Some(Deleting::Kept), "it idles");
        working.submit(key, "go");
        assert_eq!(working.deleting(key), Some(Deleting::Stops));
        working.event(1, permission("r1"), 0.0);
        assert_eq!(working.deleting(key), Some(Deleting::Stops));
        let (mut task, key) = idle();
        task.sessions.get_mut(key).unwrap().session.background = 1;
        assert_eq!(task.deleting(key), Some(Deleting::Stops));
        let (mut remote, key) = idle();
        remote.sessions.get_mut(key).unwrap().session.remote = Remote::Starting;
        assert_eq!(remote.deleting(key), Some(Deleting::Stops));
        // Stopping wins over a message that waits behind the turn.
        let (mut both, key) = sent_hello();
        both.submit(key, "later");
        assert_eq!(both.deleting(key), Some(Deleting::Stops));

        // The store has nothing of it: it would be forgotten, whatever it holds.
        let (mut fresh, key) = ready();
        assert_eq!(
            fresh.deleting(key),
            Some(Deleting::Forgotten { holds: false })
        );
        fresh.attach(key, vec![pasted()]);
        assert_eq!(
            fresh.deleting(key),
            Some(Deleting::Forgotten { holds: true })
        );
        let mut early = main_screen();
        early.new_session("/c".into(), Place::Focused, Leaving::ForgetIfEmpty);
        let key = early.focused().unwrap();
        assert_eq!(
            early.deleting(key),
            Some(Deleting::Forgotten { holds: false })
        );
        early.submit(key, "early");
        assert!(session(&early, key).queued.is_some());
        assert_eq!(
            early.deleting(key),
            Some(Deleting::Forgotten { holds: true })
        );
        // Remote Control on a session never sent is forgotten with it.
        let (mut fresh, key) = ready();
        fresh.sessions.get_mut(key).unwrap().session.remote = Remote::On { url: None };
        assert_eq!(
            fresh.deleting(key),
            Some(Deleting::Forgotten { holds: false })
        );

        // What it says is what `delete_session` does.
        assert!(fresh.delete_session(key).len() >= 2);
        assert!(fresh.sessions.get(key).is_none(), "forgotten");
        app.delete_session(stored);
        assert!(deleted(&app, stored).is_some(), "kept for a week");

        // Nothing to say of a deleted session or of one that is not known.
        assert_eq!(app.deleting(stored), None);
        assert_eq!(app.deleting(SessionKey(999)), None);
    }

    #[test]
    fn a_session_is_called_what_its_row_shows() {
        let (app, key) = shows_stored();
        assert_eq!(app.title_of(key).as_deref(), Some("Fix the parser"));
        let (app, key) = ready();
        assert_eq!(app.title_of(key).as_deref(), Some("New session"));
        assert_eq!(app.title_of(SessionKey(999)), None);
    }
}
