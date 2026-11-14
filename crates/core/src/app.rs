//! Which screen shows, the sessions the app knows and the one it shows, and the numbered opens
//! that keep a replaced program from changing anything. Every intent returns the effects to
//! carry out instead of doing the work, so this module stays plain data.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mr_agent::{
    Account, AgentError, Answer, Capabilities, ModelOption, PermissionMode, RequestId,
    SessionCommand, SessionEvent, SessionId, StartSpec, StoredSession, UsageLimits,
};
use serde::{Deserialize, Serialize};

use crate::backend::FromBackend;
use crate::conversation::NoticeKind;
use crate::groups::{GroupId, Groups, Recency, SidebarModel, sidebar_model};
use crate::saved::{RECENT_FOLDERS, SAVED_VERSION, Saved, remember_folder};
use crate::session::{HistoryState, Phase, Session};
use crate::sessions::{Entry, Prompt, SessionKey, Sessions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Checking,
    NotInstalled,
    SignedOut,
    Main,
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
/// How long after asking for the usage limits they are asked for again, once a window has
/// reset: seconds.
pub const USAGE_RETRY_SECONDS: i64 = 300;

/// How far the listing of stored sessions has come.
#[derive(Clone, Debug, PartialEq)]
pub enum Listing {
    NotAsked,
    Loading,
    Done,
    /// The last listing failed, for this reason. The entries there are stay.
    Failed(String),
}

/// What showing another session does with the one shown until now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leaving {
    /// It is closed and forgotten when it is empty: nothing was handed over in it and nothing
    /// waits to be, so the agent stores nothing for it.
    ForgetIfEmpty,
    /// It stays, empty or not: the window has text typed in its input box.
    Keep,
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
    CheckActivity {
        key: SessionKey,
        id: SessionId,
    },
    Look {
        folder: PathBuf,
    },
    /// Write what is kept between runs.
    SaveState(Box<Saved>),
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
    /// The session the window shows.
    pub shown: Option<SessionKey>,
    pub width: Width,
    /// The wall clock in Unix seconds, as the app was last told it: what the user's and the
    /// programs' activity is stamped with.
    pub now_unix: i64,
    pub listing: Listing,
    /// The listing of every stored session, not only the recent ones, has been merged.
    pub listed_all: bool,
    /// The newest list of models any program or look gave.
    pub known_models: Vec<ModelOption>,
    pub groups: Groups,
    /// What the sidebar shows, as of the last [`App::settle`].
    pub sidebar: SidebarModel,
    /// Goes up by one each time [`App::settle`] rebuilds `sidebar`.
    pub sidebar_version: u64,
    /// The folders new sessions started in, the latest first: at most [`RECENT_FOLDERS`], each
    /// once.
    pub recent_folders: Vec<PathBuf>,
    launch: Launch,
    /// The number of the latest open. Each open gets the next, so a number is never reused.
    open: u64,
    /// When the recent listing was last asked for; `None` before signing in.
    listed_unix: Option<i64>,
    /// The listing of every stored session was asked for and has not failed.
    all_asked: bool,
    /// The look after the first listing was asked for.
    looked: bool,
    /// When the usage limits were last asked for, by a program or by a look.
    usage_asked_unix: Option<i64>,
    /// What `sidebar` was built from; `None` before the first [`App::settle`].
    settled: Option<Settled>,
    /// The session shown when the last run ended, by the agent's id, while it may still be
    /// shown again: until it is, or the complete listing has come without it.
    restore: Option<SessionId>,
    /// The entry shown because it was shown when the last run ended.
    restored: Option<SessionKey>,
    /// From the last run: the sessions whose program it ran, with when it closed (see
    /// [`Saved::closed_by_app`]).
    closed_by_app: Vec<(SessionId, i64)>,
}

/// What the sidebar's model was last built from: while all of it holds, the model holds.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Settled {
    sessions: u64,
    groups: u64,
    shown: Option<SessionKey>,
    loading: bool,
    recency: Recency,
}

impl App {
    /// Starts on `Screen::Checking` and asks for the account, with what `launch` kept from the
    /// last run. The session shown then is shown again once a listing knows it, unless there
    /// is a launch folder.
    pub fn new(
        capabilities: Capabilities,
        width: Width,
        mut launch: Launch,
        now_unix: i64,
    ) -> (App, Vec<Effect>) {
        let saved = std::mem::take(&mut launch.saved);
        let mut recent_folders = saved.recent_folders.clone();
        recent_folders.truncate(RECENT_FOLDERS);
        let restore = saved.shown.clone().filter(|_| launch.folder.is_none());
        let app = App {
            screen: Screen::Checking,
            account: None,
            capabilities,
            usage: UsageLimits {
                session: None,
                weekly: None,
            },
            sessions: Sessions::default(),
            shown: None,
            width,
            now_unix,
            listing: Listing::NotAsked,
            listed_all: false,
            known_models: Vec::new(),
            // Before any `settle`, so no version it has seen is used again.
            groups: Groups::restored(&saved),
            sidebar: SidebarModel::default(),
            sidebar_version: 0,
            recent_folders,
            launch,
            open: 0,
            listed_unix: None,
            all_asked: false,
            looked: false,
            usage_asked_unix: None,
            settled: None,
            restore,
            restored: None,
            closed_by_app: saved.closed_by_app,
        };
        (app, vec![Effect::CheckAccount])
    }

    pub fn shown_entry(&self) -> Option<&Entry> {
        self.shown.and_then(|key| self.sessions.get(key))
    }

    /// Picks the screen for `account`. Signed in with a launch folder and nothing shown, a new
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
            && self.shown.is_none()
            && let Some(folder) = self.launch.folder.clone()
        {
            // Nothing is shown yet, so nothing is left.
            effects = self.new_session(folder, Leaving::ForgetIfEmpty);
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
    /// its session goes and the app says so; any other failure ends the session.
    pub fn open_failed(&mut self, open: u64, error: AgentError) -> Vec<Effect> {
        let Some(key) = self.sessions.by_open(open).map(|entry| entry.key) else {
            return Vec::new();
        };
        if error == AgentError::NotInstalled {
            self.screen = Screen::NotInstalled;
            self.sessions.remove(key);
            if self.shown == Some(key) {
                self.shown = None;
            }
            return Vec::new();
        }
        let exited = SessionEvent::Exited {
            clean: false,
            detail: error.to_string(),
        };
        self.event(open, exited, 0.0)
    }

    pub fn send_failed(
        &mut self,
        open: u64,
        command: &SessionCommand,
        error: AgentError,
    ) -> Vec<Effect> {
        let now_unix = self.now_unix;
        let Some(mut entry) = self.sessions.by_open(open) else {
            return Vec::new();
        };
        entry.last_own_unix = Some(now_unix);
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

    /// Applies `event` to the session whose current open is `open`, stamped `now`. A turn that
    /// ends out of sight leaves its session unseen; a program that is gone lets go of its open.
    pub fn event(&mut self, open: u64, event: SessionEvent, now: f64) -> Vec<Effect> {
        let (now_unix, shown) = (self.now_unix, self.shown);
        let Some(mut entry) = self.sessions.by_open(open) else {
            return Vec::new();
        };
        if let SessionEvent::UsageLimits(limits) = &event {
            take_usage(&mut self.usage, limits);
        }
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
        let ends_turn = matches!(
            event,
            SessionEvent::TurnFinished(_)
                | SessionEvent::TurnFailed { .. }
                | SessionEvent::Exited { clean: false, .. }
        );
        entry.last_own_unix = Some(now_unix);
        let mut effects: Vec<Effect> = entry
            .session
            .apply(event, now)
            .into_iter()
            .map(|command| Effect::Send { open, command })
            .collect();
        if ends_turn && shown != Some(entry.key) {
            entry.unseen = true;
        }
        if entry.session.at_rest() {
            entry.open = None;
        }
        let key = entry.key;
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
            FromBackend::History { key, result } => {
                if let Some(mut entry) = self.sessions.get_mut(key) {
                    match result {
                        Ok(history) => entry.session.load_history(history),
                        Err(error) => entry.session.history_failed(&error.to_string()),
                    }
                }
                Vec::new()
            }
            FromBackend::Activity { key, result } => self.activity_checked(key, result),
            FromBackend::Look { models, usage, .. } => {
                if let Some(limits) = usage {
                    take_usage(&mut self.usage, &limits);
                }
                self.learn_models(models);
                Vec::new()
            }
        }
    }

    /// Merges a listing since `since_unix`: a listing of every stored session is complete. The
    /// first recent listing merged asks for all of them, the session shown when the last run
    /// ended is shown again once it is known (see [`App::restore_shown`]), and the first answer
    /// of any kind asks for a look when the app looks.
    fn listed(
        &mut self,
        since_unix: Option<i64>,
        result: Result<Vec<StoredSession>, AgentError>,
    ) -> Vec<Effect> {
        let complete = since_unix.is_none();
        let mut effects = Vec::new();
        match result {
            Ok(listed) => {
                self.sessions.merge_listing(listed, complete);
                self.listing = Listing::Done;
                if complete {
                    self.listed_all = true;
                } else if !self.all_asked {
                    self.all_asked = true;
                    effects.push(Effect::ListSessions { since_unix: None });
                }
                effects.extend(self.restore_shown(complete));
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

    /// Shows the session that was shown when the last run ended, when nothing is shown and a
    /// listing knows it, without starting its program. Once the complete listing has been
    /// merged it is not tried again, nor once something else is shown.
    fn restore_shown(&mut self, complete: bool) -> Vec<Effect> {
        let Some(id) = self.restore.take() else {
            return Vec::new();
        };
        if self.shown.is_some() {
            return Vec::new();
        }
        let Some(key) = self.sessions.by_id(&id) else {
            if !complete {
                self.restore = Some(id);
            }
            return Vec::new();
        };
        self.restored = Some(key);
        self.show(key, Leaving::ForgetIfEmpty)
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

    /// Shows a new session in `folder` and starts its program, and puts `folder` first among the
    /// recent folders, asking to save when that changed them. The session shown until now is
    /// left as `leaving` says (see [`App::show`]). When the shown session is an empty one in
    /// that folder already, and its program has not ended, nothing happens.
    pub fn new_session(&mut self, folder: PathBuf, leaving: Leaving) -> Vec<Effect> {
        if self.screen != Screen::Main {
            return Vec::new();
        }
        if self.shown_entry().is_some_and(|entry| {
            empty(&entry.session) && entry.live() && entry.session.folder == folder
        }) {
            return Vec::new();
        }
        let remembered = remember_folder(&mut self.recent_folders, &folder);
        let key = self.sessions.add(Session::opening(folder), self.now_unix);
        let mut effects = self.show(key, leaving);
        effects.extend(self.start(key));
        if remembered {
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

    /// Shows `key`'s session and counts it as seen, offering it the models known. The session
    /// shown until now is closed and forgotten when it is empty, unless `leaving` says to keep
    /// it. A stored history not read yet is asked for.
    pub fn show(&mut self, key: SessionKey, leaving: Leaving) -> Vec<Effect> {
        if self.shown == Some(key) {
            return Vec::new();
        }
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        entry.unseen = false;
        entry.touched = true;
        if !self.known_models.is_empty() {
            entry.session.offer_models(&self.known_models);
        }
        let read = read_history(&mut entry);
        drop(entry);
        let mut effects = match (self.shown.replace(key), leaving) {
            (Some(left), Leaving::ForgetIfEmpty) => self.forget_if_empty(left),
            _ => Vec::new(),
        };
        effects.extend(read);
        effects
    }

    /// The app is going away, at `now_unix` by the wall clock: its state saved first, then
    /// `Close` for every session whose program has not ended. The state keeps, for every
    /// session whose program this run ran, that this app closed it then.
    pub fn close(&mut self, now_unix: i64) -> Vec<Effect> {
        self.now_unix = now_unix;
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
                .filter(|entry| entry.live())
                .filter_map(|entry| entry.open)
                .map(|open| Effect::Send {
                    open,
                    command: SessionCommand::Close,
                }),
        );
        effects
    }

    /// What is kept between runs, as of now. The sidebar's width and whether it is collapsed
    /// are the window's: they are left at their defaults.
    ///
    /// Once the complete listing has been merged, a placement whose session the app does not
    /// know is left out. The shown session is kept by its id; while nothing is shown, the one
    /// that may still be shown again from the last run is kept instead. Of the sessions the
    /// last run closed, only those it closed recently enough that a change it made could still
    /// look new to the activity check are kept ([`App::close`] adds this run's).
    pub fn saved(&self) -> Saved {
        let recent = ELSEWHERE_SECONDS + OWN_EXIT_SLACK_SECONDS;
        let mut saved = Saved {
            version: SAVED_VERSION,
            recent_folders: self.recent_folders.clone(),
            shown: match self.shown_entry() {
                Some(entry) => entry.session.id.clone(),
                None => self.restore.clone(),
            },
            closed_by_app: self
                .closed_by_app
                .iter()
                .filter(|(_, closed)| self.now_unix - closed <= recent)
                .cloned()
                .collect(),
            ..Saved::default()
        };
        self.groups.save_into(&mut saved);
        if self.listed_all {
            let known: HashSet<&SessionId> = self
                .sessions
                .iter()
                .filter_map(|entry| entry.session.id.as_ref())
                .collect();
            saved.placed.retain(|(id, _)| known.contains(id));
        }
        saved
    }

    /// No session has a program still running.
    pub fn all_ended(&self) -> bool {
        !self.sessions.iter().any(Entry::live)
    }

    /// Starts an ended session's program again. A resting one is woken by a message instead.
    pub fn restart(&mut self, key: SessionKey) -> Vec<Effect> {
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        if !matches!(entry.session.phase, Phase::Ended { .. }) {
            return Vec::new();
        }
        entry.session.reopening();
        drop(entry);
        self.start(key)
    }

    /// Sends `text` to `key`'s session, or queues it there, and makes the session active now.
    /// A resting session wakes with it queued: when this app has not run its program in this
    /// run, the store is first asked whether another app may have it open. A session that does
    /// not take a message now (see [`App::takes_message`]) takes nothing.
    pub fn submit(&mut self, key: SessionKey, text: &str) -> Vec<Effect> {
        if text.trim().is_empty() || !self.takes_message(key) {
            return Vec::new();
        }
        let now_unix = self.now_unix;
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        if entry.session.phase == Phase::Resting {
            entry.session.wake(text);
            entry.touched = true;
            if entry.last_own_unix.is_none()
                && let Some(id) = entry.session.id.clone()
            {
                entry.checking = true;
                return vec![Effect::CheckActivity { key, id }];
            }
            entry.last_active_unix = now_unix;
            drop(entry);
            return self.start(key);
        }
        entry.last_active_unix = now_unix;
        let commands = entry.session.submit(text);
        drop(entry);
        self.send(key, commands)
    }

    /// Whether `key`'s session takes a message now. An ended one does not, until it is
    /// restarted. Nor does one with a question waiting or whose check in the store runs: the
    /// message then stays where it was typed. Other sessions are not held up by either.
    pub fn takes_message(&self, key: SessionKey) -> bool {
        self.sessions.get(key).is_some_and(|entry| {
            entry.session.takes_messages() && entry.prompt.is_none() && !entry.checking
        })
    }

    /// The question waiting for the user about `key`'s session.
    pub fn prompt(&self, key: SessionKey) -> Option<&Prompt> {
        self.sessions.get(key)?.prompt.as_ref()
    }

    pub fn stop(&mut self, key: SessionKey) -> Vec<Effect> {
        let command = self
            .sessions
            .get(key)
            .and_then(|entry| entry.session.stop());
        self.send(key, command)
    }

    pub fn answer(&mut self, key: SessionKey, id: &RequestId, answer: Answer) -> Vec<Effect> {
        let command = self
            .sessions
            .get_mut(key)
            .and_then(|mut entry| entry.session.answer(id, answer));
        self.send(key, command)
    }

    pub fn choose_model(&mut self, key: SessionKey, option_id: &str) -> Vec<Effect> {
        let command = self
            .sessions
            .get_mut(key)
            .and_then(|mut entry| entry.session.choose_model(option_id));
        self.send(key, command)
    }

    pub fn choose_mode(&mut self, key: SessionKey, mode: PermissionMode) -> Vec<Effect> {
        let command = self
            .sessions
            .get_mut(key)
            .and_then(|mut entry| entry.session.choose_mode(mode));
        self.send(key, command)
    }

    /// Brings the clock up to date and returns what is due: a new listing, a usage refresh.
    pub fn tick(&mut self, now_unix: i64) -> Vec<Effect> {
        self.now_unix = now_unix;
        if self.screen != Screen::Main {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if self
            .listed_unix
            .is_some_and(|asked| now_unix - asked >= RELIST_SECONDS)
        {
            effects.push(self.list_recent());
        }
        effects.extend(self.usage_after_reset());
        effects
    }

    /// Answers the question waiting about `key`'s session and clears it. `ContinueHere` wakes
    /// the session with the message and opens its program; `ContinueAsCopy` puts a copy of it
    /// beside it, shows the copy and opens a program that forks the stored conversation, the
    /// message queued in it, leaving the original resting. The text that goes back into that
    /// session's input box, when the choice was Cancel.
    pub fn answer_prompt(
        &mut self,
        key: SessionKey,
        choice: PromptChoice,
    ) -> (Vec<Effect>, Option<String>) {
        let now_unix = self.now_unix;
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return (Vec::new(), None);
        };
        let Some(Prompt::OpenElsewhere { text }) = entry.prompt.take() else {
            return (Vec::new(), None);
        };
        if entry.session.phase != Phase::Resting {
            return (Vec::new(), None);
        }
        match choice {
            PromptChoice::Cancel => (Vec::new(), Some(text)),
            PromptChoice::ContinueHere => {
                entry.session.wake(&text);
                entry.last_active_unix = now_unix;
                drop(entry);
                (self.start(key), None)
            }
            PromptChoice::ContinueAsCopy => {
                let group = self.groups.group_of(&entry);
                let mut spec = entry.session.start_spec();
                spec.fork = true;
                let mut copy = entry.session.clone();
                drop(entry);
                // Its own id comes from its program; until then the stored one is only read.
                let read_from = copy.id.take();
                copy.wake(&text);
                let copy = self.sessions.add(copy, now_unix);
                if let Some(entry) = self.sessions.get(copy) {
                    self.groups.place(entry, &group);
                }
                // What it leaves is the original, which is stored: nothing to forget.
                let mut effects = self.show(copy, Leaving::Keep);
                if let Some(mut entry) = self.sessions.get_mut(copy)
                    && let Some(id) = read_from
                    && matches!(
                        entry.session.history,
                        HistoryState::NotLoaded | HistoryState::Loading
                    )
                {
                    entry.session.history = HistoryState::Loading;
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

    /// Rebuilds `sidebar` when sessions, groups or the shown session changed since the last call.
    /// So does the end of the first listing, and a clock that moved an entry across the
    /// [`RECENT_DAYS`] line. Each rebuild puts `sidebar_version` up by one.
    pub fn settle(&mut self) {
        let loading = self.listing == Listing::Loading;
        if self.settled.is_some_and(|settled| {
            settled.sessions == self.sessions.version()
                && settled.groups == self.groups.version()
                && settled.shown == self.shown
                && settled.loading == loading
                && settled.recency.holds_at(self.now_unix)
        }) {
            return;
        }
        self.sidebar = sidebar_model(
            &self.sessions,
            &self.groups,
            self.shown,
            self.now_unix,
            loading,
        );
        self.sidebar_version += 1;
        self.settled = Some(Settled {
            sessions: self.sessions.version(),
            groups: self.groups.version(),
            shown: self.shown,
            loading,
            recency: Recency::of(&self.sessions, self.now_unix),
        });
    }

    /// [`App::new_session`] in `folder`, and the session shown then placed in `group`. Asks
    /// once to save, after the placement.
    pub fn new_session_in(
        &mut self,
        folder: PathBuf,
        group: &GroupId,
        leaving: Leaving,
    ) -> Vec<Effect> {
        let mut effects = self.new_session(folder, leaving);
        effects.retain(|effect| !matches!(effect, Effect::SaveState(_)));
        if self.screen == Screen::Main
            && let Some(entry) = self.shown.and_then(|key| self.sessions.get(key))
        {
            self.groups.place(entry, group);
            effects.push(self.save_state());
        }
        effects
    }

    /// Puts `key`'s session in `group`. This and the group intents below that change what is
    /// kept between runs ask to save.
    pub fn move_session(&mut self, key: SessionKey, group: &GroupId) -> Vec<Effect> {
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

    pub fn delete_group(&mut self, id: &GroupId) -> Vec<Effect> {
        self.groups.delete(id);
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

    /// Asks to write what is kept between runs, as it is now.
    fn save_state(&self) -> Effect {
        Effect::SaveState(Box::new(self.saved()))
    }

    /// Starts a program for `key`'s session, under the next open's number.
    fn start(&mut self, key: SessionKey) -> Vec<Effect> {
        let Some(spec) = self
            .sessions
            .get(key)
            .map(|entry| entry.session.start_spec())
        else {
            return Vec::new();
        };
        self.start_with(key, spec)
    }

    /// Starts a program for `key`'s session as `spec` says, under the next open's number.
    fn start_with(&mut self, key: SessionKey, spec: StartSpec) -> Vec<Effect> {
        let Some(mut entry) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
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

    /// Once a usage window's reset time has passed, and the limits were last asked for more
    /// than [`USAGE_RETRY_SECONDS`] ago: `RefreshUsage` to a session whose program runs, or a
    /// look when none does and the app looks.
    fn usage_after_reset(&mut self) -> Vec<Effect> {
        let now_unix = self.now_unix;
        let reset = [&self.usage.session, &self.usage.weekly]
            .into_iter()
            .flatten()
            .any(|window| window.resets_at_unix <= now_unix);
        let asked_lately = self
            .usage_asked_unix
            .is_some_and(|asked| now_unix - asked <= USAGE_RETRY_SECONDS);
        if !reset || asked_lately {
            return Vec::new();
        }
        let live = self
            .sessions
            .iter()
            .filter(|entry| entry.live())
            .find_map(|entry| entry.open);
        match live {
            Some(open) => {
                self.usage_asked_unix = Some(now_unix);
                vec![Effect::Send {
                    open,
                    command: SessionCommand::RefreshUsage,
                }]
            }
            None if self.launch.look => vec![self.look()],
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

    /// Closes and forgets `key`'s session when it is empty.
    fn forget_if_empty(&mut self, key: SessionKey) -> Vec<Effect> {
        if !self
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

    /// The commands for `key`'s session, sent to its current open.
    fn send(
        &self,
        key: SessionKey,
        commands: impl IntoIterator<Item = SessionCommand>,
    ) -> Vec<Effect> {
        let Some(open) = self.sessions.get(key).and_then(|entry| entry.open) else {
            return Vec::new();
        };
        commands
            .into_iter()
            .map(|command| Effect::Send { open, command })
            .collect()
    }
}

/// Takes in the windows `limits` reports, keeping one it does not.
fn take_usage(usage: &mut UsageLimits, limits: &UsageLimits) {
    if limits.session.is_some() {
        usage.session = limits.session.clone();
    }
    if limits.weekly.is_some() {
        usage.weekly = limits.weekly.clone();
    }
}

/// `entry`'s stored history, asked for when it has not been read yet.
fn read_history(entry: &mut Entry) -> Option<Effect> {
    if entry.session.history != HistoryState::NotLoaded {
        return None;
    }
    let id = entry.session.id.clone()?;
    entry.session.history = HistoryState::Loading;
    Some(Effect::ReadHistory {
        key: entry.key,
        id,
        max_items: HISTORY_ITEMS,
    })
}

/// Nothing was handed over in it and nothing waits to be: the agent stores nothing for it, so
/// leaving it forgets it.
fn empty(session: &Session) -> bool {
    !session.is_stored() && session.queued.is_none()
}

#[cfg(test)]
mod tests {
    use mr_agent::{
        HistoryItem, LimitWindow, StoredHistory, ToolDetail, TurnSummary, UserRequest,
        UserRequestKind,
    };

    use super::*;
    use crate::conversation::ItemBody;
    use crate::groups::Group;
    use crate::saved::{RECENT_FOLDERS, SAVED_VERSION};
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
        app.show(key, Leaving::ForgetIfEmpty);
        let history = StoredHistory {
            items: vec![
                HistoryItem::User {
                    text: "read the parser".into(),
                    image_count: 0,
                },
                HistoryItem::AssistantText("It reads tokens.".into()),
            ],
            ..StoredHistory::default()
        };
        app.receive(
            FromBackend::History {
                key,
                result: Ok(history),
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
        let key = app.shown.expect("the launch folder's session is shown");
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
        app.new_session(folder.into(), Leaving::ForgetIfEmpty);
        let key = app.shown.unwrap();
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
                Effect::Open {
                    open: 1,
                    spec: StartSpec::new("/p")
                },
                save(&app),
                recent(NOW)
            ],
            "the launch folder is a recent folder now"
        );
        let shown = app.shown_entry().unwrap();
        assert_eq!((app.screen, shown.open), (Screen::Main, Some(1)));
        assert_eq!(shown.session.phase, Phase::Opening);
        assert_eq!(app.check_again(), vec![]);
    }

    #[test]
    fn without_a_launch_folder_no_session_starts() {
        let (mut app, _) = App::new(caps(), Width::Unlimited, Launch::default(), NOW);
        assert_eq!(app.account_checked(signed_in()), vec![recent(NOW)]);
        assert_eq!(app.screen, Screen::Main);
        assert!(app.sessions.is_empty() && app.shown.is_none());
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
        let (mut app, _) = ready();
        app.event(
            1,
            SessionEvent::UsageLimits(UsageLimits {
                session: window(0.2),
                weekly: window(0.4),
            }),
            0.0,
        );
        app.event(
            1,
            SessionEvent::UsageLimits(UsageLimits {
                session: window(0.3),
                weekly: None,
            }),
            0.0,
        );
        assert_eq!(
            app.usage,
            UsageLimits {
                session: window(0.3),
                weekly: window(0.4)
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
        let effects = unsaved(app.new_session("/q".into(), Leaving::ForgetIfEmpty));
        assert_eq!(
            effects,
            vec![
                Effect::Send {
                    open: 1,
                    command: SessionCommand::Close
                },
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
        let new = app.shown.unwrap();
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
        let effects = app.new_session("/p".into(), Leaving::ForgetIfEmpty);
        assert_eq!(effects.len(), 1, "{effects:?}");
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
        let key = app.shown.unwrap();
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
        assert!(app.shown.is_none() && app.sessions.is_empty());
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

    #[test]
    fn each_message_from_the_backend_reaches_its_handler() {
        let (mut app, _) = App::new(caps(), Width::Unlimited, launch("/p"), NOW);
        let effects = unsaved(app.receive(FromBackend::Account(signed_in()), 0.0));
        assert_eq!(
            effects,
            vec![
                Effect::Open {
                    open: 1,
                    spec: StartSpec::new("/p")
                },
                recent(NOW)
            ]
        );
        assert_eq!(app.screen, Screen::Main);
        let key = app.shown.unwrap();

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
        assert!(app.shown.is_none() && app.sessions.is_empty());
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
        app.show(a, Leaving::ForgetIfEmpty);
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
        app.show(b, Leaving::ForgetIfEmpty);
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
        app.show(a, Leaving::ForgetIfEmpty);
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
        app.show(a, Leaving::ForgetIfEmpty);
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
        app.new_session("/a".into(), Leaving::ForgetIfEmpty);
        let a = app.shown.unwrap();
        assert_eq!(
            app.show(a, Leaving::ForgetIfEmpty),
            vec![],
            "showing the shown entry does nothing"
        );
        assert_eq!(closes(&app.show(other, Leaving::ForgetIfEmpty)), vec![1]);
        assert!(app.sessions.get(a).is_none());
        assert_eq!(app.shown, Some(other));

        let mut app = main_screen();
        let other = app.sessions.add(stored("s9"), NOW - 60);
        let a = busy(&mut app, "/a");
        assert_eq!(
            closes(&app.show(other, Leaving::ForgetIfEmpty)),
            Vec::<u64>::new(),
            "a message was sent"
        );
        assert!(app.sessions.get(a).is_some());

        let mut app = main_screen();
        let other = app.sessions.add(stored("s9"), NOW - 60);
        app.new_session("/a".into(), Leaving::ForgetIfEmpty);
        let a = app.shown.unwrap();
        app.submit(a, "early");
        assert_eq!(mark(&app, a), Mark::Working);
        assert_eq!(
            closes(&app.show(other, Leaving::ForgetIfEmpty)),
            Vec::<u64>::new(),
            "a message is queued"
        );
        assert!(app.sessions.get(a).is_some());
    }

    #[test]
    fn an_empty_session_told_to_stay_is_kept() {
        let mut app = main_screen();
        let other = app.sessions.add(stored("s9"), NOW - 60);
        app.new_session("/a".into(), Leaving::ForgetIfEmpty);
        let a = app.shown.unwrap();
        assert_eq!(closes(&app.show(other, Leaving::Keep)), Vec::<u64>::new());
        assert!(app.sessions.get(a).is_some(), "kept");
        assert_eq!(app.show(a, Leaving::ForgetIfEmpty), vec![]);
        // A new session in another folder keeps it too.
        assert_eq!(
            closes(&app.new_session("/b".into(), Leaving::Keep)),
            Vec::<u64>::new()
        );
        let b = app.shown.unwrap();
        assert!(app.sessions.get(a).is_some(), "kept again");
        // Left later with nothing typed, each goes.
        assert_eq!(closes(&app.show(a, Leaving::ForgetIfEmpty)), vec![2]);
        assert!(app.sessions.get(b).is_none());
        assert_eq!(closes(&app.show(other, Leaving::ForgetIfEmpty)), vec![1]);
        assert!(app.sessions.get(a).is_none());
    }

    #[test]
    fn new_session_keeps_an_empty_one_in_the_same_folder() {
        let mut app = main_screen();
        assert_eq!(
            unsaved(app.new_session("/a".into(), Leaving::ForgetIfEmpty)),
            vec![Effect::Open {
                open: 1,
                spec: StartSpec::new("/a")
            }]
        );
        let a = app.shown;
        assert_eq!(app.new_session("/a".into(), Leaving::ForgetIfEmpty), vec![]);
        assert_eq!((app.shown, app.sessions.len()), (a, 1));
        assert_eq!(
            unsaved(app.new_session("/b".into(), Leaving::ForgetIfEmpty)),
            vec![
                Effect::Send {
                    open: 1,
                    command: SessionCommand::Close
                },
                Effect::Open {
                    open: 2,
                    spec: StartSpec::new("/b")
                },
            ]
        );
        assert_eq!(app.sessions.len(), 1);
        assert_ne!(app.shown, a);
        assert_eq!(
            app.shown_entry().unwrap().session.folder,
            std::path::Path::new("/b")
        );
    }

    #[test]
    fn new_session_replaces_an_empty_session_that_ended() {
        let mut app = main_screen();
        app.new_session("/a".into(), Leaving::ForgetIfEmpty);
        let failed = app.shown.unwrap();
        app.open_failed(1, AgentError::Io("no such folder".into()));
        assert!(matches!(session(&app, failed).phase, Phase::Ended { .. }));
        assert_eq!(
            app.new_session("/a".into(), Leaving::ForgetIfEmpty),
            vec![Effect::Open {
                open: 2,
                spec: StartSpec::new("/a")
            }]
        );
        assert!(app.sessions.get(failed).is_none(), "the ended one is gone");
        let fresh = app.shown_entry().unwrap();
        assert_ne!(fresh.key, failed);
        assert_eq!(
            (fresh.session.phase.clone(), fresh.open),
            (Phase::Opening, Some(2))
        );
        assert_eq!(app.sessions.len(), 1);
    }

    #[test]
    fn the_old_session_keeps_working() {
        let mut app = main_screen();
        let a = busy(&mut app, "/a");
        assert_eq!(session(&app, a).phase, Phase::Working);
        assert_eq!(
            unsaved(app.new_session("/b".into(), Leaving::ForgetIfEmpty)),
            vec![Effect::Open {
                open: 2,
                spec: StartSpec::new("/b")
            }]
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
        assert_eq!(app.show(unknown, Leaving::ForgetIfEmpty), vec![]);
        assert_eq!(app.shown, Some(key));
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
        app.show(touched, Leaving::ForgetIfEmpty);
        let live = busy(&mut app, "/a");
        listing(&mut app, None, vec![]);
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
        app.new_session("/p".into(), Leaving::ForgetIfEmpty);
        let key = app.shown.unwrap();
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
            app.show(key, Leaving::ForgetIfEmpty),
            vec![Effect::ReadHistory {
                key,
                id: id("a1"),
                max_items: HISTORY_ITEMS
            }]
        );
        assert_eq!(session(&app, key).history, HistoryState::Loading);
        let reads = |effects: &[Effect]| {
            effects
                .iter()
                .filter(|e| matches!(e, Effect::ReadHistory { .. }))
                .count()
        };
        app.new_session("/b".into(), Leaving::ForgetIfEmpty);
        assert_eq!(
            reads(&app.show(key, Leaving::ForgetIfEmpty)),
            0,
            "while it loads"
        );
        let history = StoredHistory {
            items: vec![HistoryItem::User {
                text: "read the parser".into(),
                image_count: 0,
            }],
            ..StoredHistory::default()
        };
        let answer = |key| FromBackend::History {
            key,
            result: Ok(history.clone()),
        };
        assert_eq!(app.receive(answer(key), 0.0), vec![]);
        assert_eq!(session(&app, key).history, HistoryState::Loaded);
        assert_eq!(session(&app, key).conversation.items().len(), 1);
        app.new_session("/b".into(), Leaving::ForgetIfEmpty);
        assert_eq!(
            reads(&app.show(key, Leaving::ForgetIfEmpty)),
            0,
            "once it is loaded"
        );
        let gone = answer(SessionKey(99));
        assert_eq!(app.receive(gone, 0.0), vec![], "for a key that is gone");
    }

    #[test]
    fn a_history_that_fails_is_shown_as_failed() {
        let mut app = main_screen();
        let a1 = store("a1", "Fix the parser", "/home/alex/project", NOW - 60);
        listing(&mut app, Some(NOW - 2_592_000), vec![a1]);
        let key = by_id(&app, "a1");
        app.show(key, Leaving::ForgetIfEmpty);
        app.receive(
            FromBackend::History {
                key,
                result: Err(AgentError::Io("gone".into())),
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
        assert_eq!(entry.session.queued.as_deref(), Some("carry on"));
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
        assert_eq!(session(&app, other).queued.as_deref(), Some("hello"));
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
        app.show(other, Leaving::ForgetIfEmpty);
        let project = GroupId::Folder("/home/alex/project".into());
        app.collapse_group(&project, true);
        app.settle();
        let version = app.sidebar_version;
        app.receive(
            FromBackend::Activity {
                key,
                result: Ok(Some(NOW - 5)),
            },
            0.0,
        );
        assert_eq!(mark(&app, key), Mark::NeedsYou);
        app.settle();
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
        app.settle();
        assert!(
            app.sidebar
                .sections
                .iter()
                .any(|section| section.id == project),
            "a hidden group is listed while the question waits"
        );
        app.answer_prompt(key, PromptChoice::Cancel);
        assert_eq!(mark(&app, key), Mark::None);
        app.settle();
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
        assert_eq!(session(&app, key).queued.as_deref(), Some("carry on"));
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
        let (effects, back) = app.answer_prompt(key, PromptChoice::ContinueAsCopy);
        assert_eq!((back, app.prompt(key)), (None, None));
        let mut spec = resume_a1();
        spec.fork = true;
        assert_eq!(effects, vec![Effect::Open { open: 1, spec }]);
        let copy = app.shown.unwrap();
        assert_ne!(copy, key);
        assert_eq!(app.sessions.len(), 2);
        let (original, forked) = (session(&app, key), session(&app, copy));
        assert_eq!(forked.title.as_deref(), Some("Fix the parser"));
        assert_eq!(forked.folder, original.folder);
        assert_eq!(forked.conversation.items(), original.conversation.items());
        assert_eq!(forked.conversation.items().len(), 2);
        assert_eq!(forked.phase, Phase::Opening);
        assert_eq!(forked.queued.as_deref(), Some("carry on"));
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
        assert_eq!(entry.session.queued.as_deref(), Some("more"));
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
        app.show(key, Leaving::ForgetIfEmpty);
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
        app.show(later, Leaving::ForgetIfEmpty);
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
        assert_eq!(usage_asks(app.tick(NOW + 1301)), refresh);

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
        assert_eq!(usage_asks(app.tick(NOW + 602)), look);
    }

    const DAY: i64 = 86_400;

    fn group_of(app: &App, key: SessionKey) -> GroupId {
        app.groups.group_of(app.sessions.get(key).unwrap())
    }

    #[test]
    fn settle_rebuilds_only_after_a_change() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.settle();
        let (version, model) = (app.sidebar_version, app.sidebar.clone());
        assert_eq!(app.sidebar.sections.len(), 1);
        assert_eq!(app.sidebar.sections[0].mark, Mark::Working);
        assert!(app.sidebar.loading, "the first listing is on its way");
        app.settle();
        assert_eq!((app.sidebar_version, &app.sidebar), (version, &model));
        app.now_unix += 60;
        app.settle();
        assert_eq!(app.sidebar_version, version, "no entry crossed the line");

        app.event(1, permission("r1"), 0.0);
        app.settle();
        assert_eq!(app.sidebar_version, version + 1);
        assert_ne!(app.sidebar, model);
        assert_eq!(app.sidebar.sections[0].mark, Mark::NeedsYou);
        app.settle();
        assert_eq!(app.sidebar_version, version + 1);

        let notes = store("b1", "Tidy the notes", "/home/alex/notes", NOW - 29 * DAY);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![notes]);
        app.settle();
        assert_eq!(app.sidebar_version, version + 2);
        assert!(!app.sidebar.loading);
        assert_eq!(app.sidebar.sections.len(), 2);

        app.now_unix = NOW + 2 * DAY;
        app.settle();
        assert_eq!(app.sidebar_version, version + 3, "b1 is old now");
        assert_eq!(app.sidebar.sections.len(), 1);
        assert_eq!(app.sidebar.older_groups, 1);
        app.settle();
        assert_eq!(app.sidebar_version, version + 3);

        app.toggle_older_groups();
        app.settle();
        assert_eq!(app.sidebar_version, version + 4, "a group changed");
        app.show(by_id(&app, "b1"), Leaving::ForgetIfEmpty);
        app.settle();
        assert_eq!(app.sidebar_version, version + 5, "another session is shown");
        let rows = (&app.sidebar.sections[0].rows, &app.sidebar.sections[1].rows);
        assert_eq!((rows.0[0].key, rows.0[0].shown), (key, false));
        assert!(rows.1[0].shown);
    }

    #[test]
    fn a_new_session_in_a_group_is_placed_there() {
        let mut app = main_screen();
        let (side, _) = app.new_group("Side projects");
        let effects =
            unsaved(app.new_session_in("/home/alex/notes".into(), &side, Leaving::ForgetIfEmpty));
        assert_eq!(
            effects,
            vec![Effect::Open {
                open: 1,
                spec: StartSpec::new("/home/alex/notes")
            }]
        );
        let key = app.shown.unwrap();
        app.settle();
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
            Leaving::ForgetIfEmpty,
        );
        let other = app.shown.unwrap();
        assert_ne!(other, key);
        assert_eq!(group_of(&app, other), home);
    }

    #[test]
    fn a_copy_is_placed_where_the_original_is() {
        let (mut app, key, _) = checked(5);
        let (side, _) = app.new_group("Side projects");
        app.move_session(key, &side);
        app.answer_prompt(key, PromptChoice::ContinueAsCopy);
        let copy = app.shown.unwrap();
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
        app.settle();
        assert_eq!(app.sidebar.sections[0].name, "project");
        assert_eq!(app.rename_group(&home, "Parser"), vec![save(&app)]);
        assert_eq!(app.collapse_group(&home, true), vec![save(&app)]);
        app.settle();
        let section = &app.sidebar.sections[0];
        assert_eq!((section.name.as_str(), section.collapsed), ("Parser", true));
        assert_eq!(app.delete_group(&home), vec![save(&app)]);
        app.settle();
        let section = &app.sidebar.sections[0];
        assert_eq!(
            (section.name.as_str(), section.collapsed),
            ("project", false)
        );

        let (side, _) = app.new_group("Side projects");
        app.move_session(key, &side);
        assert_eq!(app.hide_group(&side, true), vec![save(&app)]);
        app.settle();
        assert_eq!(
            app.sidebar.sections[0].id, side,
            "the shown session keeps it listed"
        );
        app.new_session("/home/alex/notes".into(), Leaving::ForgetIfEmpty);
        app.settle();
        assert_eq!(
            app.sidebar.hidden,
            vec![(side.clone(), "Side projects".to_string())]
        );
        assert_eq!(app.hide_group(&side, false), vec![save(&app)]);
        assert_eq!(app.show_older(&side), vec![]);
        assert_eq!(app.toggle_older_groups(), vec![]);
        app.settle();
        assert!(app.sidebar.older_groups_open);
        assert!(app.sidebar.hidden.is_empty());
    }

    #[test]
    fn streamed_text_rebuilds_nothing() {
        let (mut app, key) = ready();
        app.submit(key, "go");
        app.settle();
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
            app.settle();
        }
        assert_eq!(
            app.sidebar_version, version,
            "nothing the sidebar shows changed"
        );
        assert_eq!(app.sidebar.sections[0].mark, Mark::Working);
        app.event(1, done(), 0.0);
        app.settle();
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
        app.delete_group(&gone);
        app.move_session(by_id(&app, "a2"), &side);
        app.collapse_group(&notes(), true);
        app.settle();
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
        again.settle();
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
                },
                Group {
                    id: docs,
                    name: None,
                    collapsed: false,
                    hidden: true,
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
        assert_eq!(app.shown, Some(key));
        assert_eq!(
            effects,
            vec![
                Effect::ListSessions { since_unix: None },
                Effect::ReadHistory {
                    key,
                    id: id("a2"),
                    max_items: HISTORY_ITEMS
                },
            ]
        );
        assert_eq!(session(&app, key).phase, Phase::Resting);
        assert_eq!(app.saved().shown, Some(id("a2")));

        let mut app = restored(saved.clone());
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![]);
        assert_eq!(effects, vec![Effect::ListSessions { since_unix: None }]);
        assert_eq!(app.shown, None);
        assert_eq!(app.saved().shown, Some(id("a2")), "it may still come");
        let old = store("a2", "Write the docs", "/home/alex/docs", NOW - 90 * DAY);
        let effects = listing(&mut app, None, vec![old]);
        let key = by_id(&app, "a2");
        assert_eq!(
            app.shown,
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
    fn nothing_is_restored_with_a_launch_folder() {
        let saved = Saved {
            version: SAVED_VERSION,
            shown: Some(id("a2")),
            ..Saved::default()
        };
        let mut app = restored_with(launch("/home/alex/notes"), saved);
        let started = app.shown.expect("the launch folder's session");
        let effects = listing(&mut app, Some(NOW - RECENT_DAYS * DAY), three_stored());
        assert_eq!(effects, vec![Effect::ListSessions { since_unix: None }]);
        listing(&mut app, None, three_stored());
        assert_eq!(app.shown, Some(started));
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
        assert_eq!(app.shown, None);
        assert_eq!(app.saved().shown, None);
        let back = store("gone", "Back again", "/home/alex/notes", NOW - 10);
        listing(&mut app, Some(NOW - RECENT_DAYS * DAY), vec![back]);
        assert_eq!(app.shown, None, "not tried after the complete listing");
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

        app.new_session("/home/alex/f5".into(), Leaving::ForgetIfEmpty);
        let mut expected: Vec<String> = vec!["/home/alex/f5".into()];
        expected.extend(
            (2..=13)
                .rev()
                .filter(|n| *n != 5)
                .map(|n| format!("/home/alex/f{n}")),
        );
        assert_eq!(folders(&app), expected);
        assert_eq!(app.saved().recent_folders, app.recent_folders);

        let key = app.shown.unwrap();
        app.event(open_of(&app, key), SessionEvent::Started(info()), 0.0);
        app.submit(key, "go");
        let effects = app.new_session("/home/alex/f5".into(), Leaving::ForgetIfEmpty);
        assert_eq!(saves(&effects), 0, "nothing changed: {effects:?}");
        assert_eq!(folders(&app), expected);
        let effects = app.new_session("/home/alex/f9".into(), Leaving::ForgetIfEmpty);
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
        assert_eq!(app.delete_group(&side), vec![save(&app)]);
        let (other, _) = app.new_group("Other");
        let effects =
            app.new_session_in("/home/alex/project".into(), &other, Leaving::ForgetIfEmpty);
        assert_eq!(effects.last(), Some(&save(&app)), "after the placement");
        assert_eq!(saves(&effects), 1, "{effects:?}");
        let effects = app.new_session_in(
            "/home/alex/project".into(),
            &project(),
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
}
