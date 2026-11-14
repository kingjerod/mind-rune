//! Real turns through the same `App` and `Backend` the window uses, against the real `claude`.
//! Opt-in: it is ignored by default and returns at once unless `MINDRUNE_LIVE=1`.
//!
//! ```bash
//! MINDRUNE_LIVE=1 cargo test -p mindrune --test live -- --ignored --nocapture
//! ```
//!
//! It needs Claude Code installed and signed in. It runs the app twice, as the window would be
//! opened, closed and opened again, and starts the real program three times in all: the
//! session of the first run, the look the first run takes, and the same session resumed in the
//! second run. The account is checked once; the second run is handed what the first found.
//!
//! The first run opens with no folder, so it lists the stored sessions and looks, as the window
//! does; the test then starts a new session in a fresh temporary folder, asks for a one-word
//! reply and checks it. After the reply it prints what came of the requests read from the
//! program but not yet seen on the wire: the session's title, the model's label, and whether
//! `claude` accepts the Auto permission mode. Then it prints how long the two listings took,
//! what the look gave, and how long the largest of a few recent stored histories took to read.
//! None of these fails the test, except not getting back to "Ask before edits" after trying
//! Auto. The first run then closes, as the window closes, keeping what it would save.
//!
//! The second run starts with what the first kept, so it shows the test's session again from
//! the store. The first message and its reply must be in the history read back, a second
//! message must wake the session, resuming the same id, and get its reply. What the first run
//! kept also says that this app closed the session, so the second run is not expected to ask
//! whether another app has it open; it prints whether it did.
//!
//! Every way out of the test, a failed assertion included, closes the session, so no `claude`
//! is left running, and every wait has a deadline that says what it waited for. Of the user's
//! other stored sessions it prints only counts and times: never a title, a folder, a message
//! or an id. It writes nothing; only `claude` itself stores the test's session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mr_agent::{
    Account, Agent, CommandKind, PermissionMode, SessionCommand, SessionEvent, SessionId,
};
use mr_claude::ClaudeAgent;
use mr_core::{
    App, Backend, DEFAULT_WIDTH, Effect, FromBackend, HistoryState, ItemBody, Launch, Leaving,
    Phase, Prompt, PromptChoice, Saved, Screen, Session, SessionKey, ToBackend, Width, mode_label,
};

/// The whole run, from the account check to the reply; also a resumed session's reply.
const TURN_LIMIT: Duration = Duration::from_secs(120);

/// How long the session may take to end once it was told to close.
const CLOSE_LIMIT: Duration = Duration::from_secs(20);

/// How long the loop sleeps between two looks at the backend.
const TICK: Duration = Duration::from_millis(20);

/// How long the title may take to replace the stand-in, after the reply.
const TITLE_LIMIT: Duration = Duration::from_secs(30);

/// How long a permission mode change may take to be confirmed or refused.
const MODE_LIMIT: Duration = Duration::from_secs(10);

/// How long both listings of the store may take, from the first turn's end.
const LIST_LIMIT: Duration = Duration::from_secs(60);

/// How long the look may take once it was asked for: its own two limits and a margin.
const LOOK_LIMIT: Duration = Duration::from_secs(
    Backend::LOOK_START_LIMIT.as_secs() + Backend::LOOK_USAGE_LIMIT.as_secs() + 10,
);

/// How long one stored history may take to be read.
const HISTORY_LIMIT: Duration = Duration::from_secs(30);

/// How many of the most recent stored sessions have their history read.
const HISTORIES_READ: usize = 5;

/// How long the second run may take to show the test's session again, its history read.
const READ_BACK_LIMIT: Duration = Duration::from_secs(120);

/// How long the store may take to say when the session last changed, once a message woke it.
const ACTIVITY_LIMIT: Duration = Duration::from_secs(30);

const MESSAGE: &str = "Reply with the single word: ok";

const SECOND_MESSAGE: &str = "Reply with the single word: again";

#[test]
#[ignore = "starts the real claude; run with MINDRUNE_LIVE=1 and --ignored"]
fn a_real_turn_the_store_the_look_and_the_session_resumed() {
    if std::env::var("MINDRUNE_LIVE").as_deref() != Ok("1") {
        eprintln!("live: skipped; set MINDRUNE_LIVE=1 to run it");
        return;
    }
    let dir = tempfile::tempdir().expect("a temporary folder");
    let folder = dir.path().to_path_buf();
    let agent: Arc<dyn Agent> = Arc::new(ClaudeAgent::from_environment());

    // The first run: the window opened with no folder, and a new session in the test's folder.
    let launch = Launch {
        folder: None,
        look: true,
        home: folder.clone(),
        ..Launch::default()
    };
    let mut live = Live::start(agent.clone(), launch, &folder, None);
    let stand_in = live.first_turn();
    live.report_title_model_and_auto_mode(stand_in);
    live.report_listings();
    live.report_look();
    live.report_histories();
    let id = live.id().expect("the session has the agent's id");
    let saved = live.close_app();
    let account = live.app.account.clone().expect("the account was checked");
    drop(live);

    // The second run: the window opened again with what the first kept.
    let launch = Launch {
        folder: None,
        look: false,
        home: folder.clone(),
        saved,
    };
    let mut again = Live::start(agent, launch, &folder, Some(account));
    again.read_back(&id);
    again.resume(&id);
    again.close_app();
}

/// When something was asked of the store and what came back.
struct Answered {
    took: Duration,
    /// How many it gave, or `None` when it failed.
    count: Option<usize>,
}

/// One run of the `App` and the `Backend` over the real agent, the way the window holds them.
/// Dropping it before the test's session closed cleanly (a failed assertion, a timeout) closes
/// the session.
struct Live {
    app: App,
    backend: Backend,
    /// The test's own folder: the only place its sessions and its look start in.
    folder: PathBuf,
    /// The test's session in this run, once there is one.
    key: Option<SessionKey>,
    /// A new session is still to be started in `folder` once the account is signed in.
    start_new: bool,
    started: Instant,
    /// The test's session's program is meant to be running: until it closes, it may not rest.
    running: bool,
    closed: bool,
    /// Every command of the test's session the agent refused, with its reason, in order. The
    /// core keeps a refused title request to itself, so the test reads them as they come.
    refusals: Vec<(CommandKind, String)>,
    /// Every title the test's session was given, in order, as the agent sent it.
    titles: Vec<String>,
    /// Every permission mode the agent confirmed for the test's session, in order. Unlike the
    /// session's `mode`, this shows a confirmation of the mode already in use.
    modes: Vec<PermissionMode>,
    /// The test's session's own program reported the session's usage window.
    session_usage: bool,
    /// When each listing was asked for: the recent one, and the one of every stored session.
    recent_asked: Option<Instant>,
    all_asked: Option<Instant>,
    recent: Option<Answered>,
    all: Option<Answered>,
    look_asked: Option<Instant>,
    /// What the look gave: how long it took, how many models, whether it knew the usage.
    looked: Option<(Duration, usize, bool)>,
    /// When each history was asked for, by the entry it is for.
    reading: HashMap<SessionKey, Instant>,
    /// Each history read, with how many items it had.
    read: HashMap<SessionKey, Answered>,
    /// What the store said about when the test's session last changed: Unix seconds.
    activity: Option<Option<i64>>,
    /// The spec of the latest program opened for the test's session.
    opened: Option<mr_agent::StartSpec>,
}

impl Live {
    /// The app as the window starts it with `launch`. With `account`, that account is taken in
    /// at once instead of checking it again; without, the account is checked and a new session
    /// is started in `folder` once it is signed in.
    fn start(
        agent: Arc<dyn Agent>,
        launch: Launch,
        folder: &Path,
        account: Option<Account>,
    ) -> Live {
        let (app, mut effects) = App::new(
            agent.capabilities(),
            Width::Points(DEFAULT_WIDTH),
            launch,
            unix_now(),
        );
        let backend = Backend::start(agent, Arc::new(|| {}));
        let mut live = Live {
            app,
            backend,
            folder: folder.to_path_buf(),
            key: None,
            start_new: account.is_none(),
            started: Instant::now(),
            running: false,
            closed: false,
            refusals: Vec::new(),
            titles: Vec::new(),
            modes: Vec::new(),
            session_usage: false,
            recent_asked: None,
            all_asked: None,
            recent: None,
            all: None,
            look_asked: None,
            looked: None,
            reading: HashMap::new(),
            read: HashMap::new(),
            activity: None,
            opened: None,
        };
        if let Some(account) = account {
            effects.retain(|effect| *effect != Effect::CheckAccount);
            effects.extend(live.app.receive(FromBackend::Account(account), 0.0));
        }
        live.carry_out(effects);
        live
    }

    /// Waits for the session to be ready, sends the message once, waits for the turn to end and
    /// for the session's own usage, and checks the reply. Returns the stand-in title the
    /// session had once the message was sent.
    fn first_turn(&mut self) -> Option<String> {
        let deadline = self.started + TURN_LIMIT;
        let mut sent = false;
        let mut stand_in = None;
        loop {
            self.take_in();
            self.fail_on_a_dead_end();
            let phase = self.phase();
            if phase == Some(Phase::Idle) {
                if !sent {
                    let key = self.key.expect("the session was started");
                    let effects = self.app.submit(key, MESSAGE);
                    assert!(!effects.is_empty(), "the message was not sent: {effects:?}");
                    self.running = true;
                    self.carry_out(effects);
                    sent = true;
                    stand_in = self.title();
                } else if self.session_usage {
                    break;
                }
            }
            if Instant::now() >= deadline {
                if !sent {
                    panic!(
                        "the session never reached Idle within {} s: screen {:?}, phase {phase:?}",
                        TURN_LIMIT.as_secs(),
                        self.app.screen
                    );
                }
                if phase == Some(Phase::Idle) {
                    panic!(
                        "the turn ended but the session's usage limits never arrived within {} s",
                        TURN_LIMIT.as_secs()
                    );
                }
                panic!(
                    "no reply within {} s (phase {phase:?}); conversation:\n{:#?}",
                    TURN_LIMIT.as_secs(),
                    self.items()
                );
            }
            std::thread::sleep(TICK);
        }

        let items = self.items();
        let replies = finished_replies(&items);
        assert_eq!(
            replies.len(),
            1,
            "one finished reply was expected; conversation:\n{items:#?}"
        );
        assert!(
            replies[0].to_lowercase().contains("ok"),
            "the reply does not say ok: {:?}",
            replies[0]
        );
        assert!(self.app.usage.session.is_some(), "no session usage");
        let label = self.session().unwrap().model_label();
        assert_ne!(label, "Model", "the model was never named");
        eprintln!("live: replied {:?} with {label}", replies[0]);
        stand_in
    }

    /// The title, the model's label and the Auto mode, as plan 2's live test printed them.
    fn report_title_model_and_auto_mode(&mut self, stand_in: Option<String>) {
        self.report_title(stand_in);
        let label = self.session().unwrap().model_label();
        eprintln!("live: model label {label:?}");
        self.try_auto_mode();
    }

    /// Prints how many sessions each listing gave and how long it took, from asking to the
    /// answer. Fails on nothing.
    fn report_listings(&mut self) {
        let answered = self.wait_for(LIST_LIMIT, |live| {
            live.recent.is_some() && live.all.is_some()
        });
        match (&self.recent, &self.all) {
            (
                Some(Answered {
                    took: recent_took,
                    count: Some(recent),
                }),
                Some(Answered {
                    took: all_took,
                    count: Some(all),
                }),
            ) => eprintln!(
                "live: listed {recent} recent sessions in {} ms, {all} in all in {} ms",
                recent_took.as_millis(),
                all_took.as_millis()
            ),
            (recent, all) => eprintln!(
                "live: the listings did not both answer{}: recent {}, all {}",
                if answered {
                    String::new()
                } else {
                    format!(" within {} s", LIST_LIMIT.as_secs())
                },
                listing_word(recent, self.recent_asked),
                listing_word(all, self.all_asked)
            ),
        }
    }

    /// Prints what the look gave. Fails on nothing.
    fn report_look(&mut self) {
        if self.look_asked.is_none() {
            eprintln!("live: the app asked for no look");
            return;
        }
        self.wait_for(LOOK_LIMIT, |live| live.looked.is_some());
        match self.looked {
            Some((took, models, usage)) => eprintln!(
                "live: the look gave {models} models; usage known: {} ({} ms)",
                if usage { "yes" } else { "no" },
                took.as_millis()
            ),
            None => eprintln!(
                "live: the look gave no answer within {} s of the first turn's end",
                LOOK_LIMIT.as_secs()
            ),
        }
    }

    /// Shows the [`HISTORIES_READ`] most recent stored sessions one after the other, as a click
    /// on their rows would, and prints the largest history read, by its items, and how long
    /// that read took. The listing gives no sizes, so the largest is the largest of these. Then
    /// the test's session is shown again. Fails on nothing.
    fn report_histories(&mut self) {
        let mut stored: Vec<(i64, SessionKey)> = self
            .app
            .sessions
            .iter()
            .filter(|entry| {
                Some(entry.key) != self.key
                    && entry.session.history == HistoryState::NotLoaded
                    && entry.session.id.is_some()
            })
            .map(|entry| (entry.last_active_unix, entry.key))
            .collect();
        stored.sort_by_key(|&(last_active, _)| std::cmp::Reverse(last_active));
        stored.truncate(HISTORIES_READ);
        let mut unanswered = false;
        for &(_, key) in &stored {
            let effects = self.app.show(key, Leaving::ForgetIfEmpty);
            self.carry_out(effects);
            if !self.wait_for(HISTORY_LIMIT, |live| live.read.contains_key(&key)) {
                // The library thread is held up: what is asked next would only wait behind it.
                unanswered = true;
                break;
            }
        }
        let key = self.key.expect("the test's session");
        let effects = self.app.show(key, Leaving::ForgetIfEmpty);
        self.carry_out(effects);

        let largest = stored
            .iter()
            .filter_map(|(_, key)| self.read.get(key))
            .filter_map(|answer| Some((answer.count?, answer.took)))
            .max_by_key(|&(items, _)| items);
        let failed = stored
            .iter()
            .filter(|(_, key)| self.read.get(key).is_some_and(|a| a.count.is_none()))
            .count();
        match largest {
            Some((items, took)) => eprintln!(
                "live: read the largest of the {} most recent histories ({items} items) in {} ms",
                stored.len(),
                took.as_millis()
            ),
            None if stored.is_empty() => eprintln!("live: no stored session to read"),
            None => eprintln!("live: no recent history was read"),
        }
        if failed > 0 {
            eprintln!("live: {failed} of them could not be read");
        }
        if unanswered {
            eprintln!(
                "live: a history read gave no answer within {} s",
                HISTORY_LIMIT.as_secs()
            );
        }
    }

    /// Waits for the second run to show the test's session `id` again, as kept by the first,
    /// with its history read from the store, and checks that the first message and its reply
    /// are in it.
    fn read_back(&mut self, id: &SessionId) {
        let shown_id = |live: &Live| live.app.shown_entry().and_then(|e| e.session.id.clone());
        let read = self.wait_for(READ_BACK_LIMIT, |live| {
            live.app.shown_entry().is_some_and(|entry| {
                entry.session.id.as_ref() == Some(id)
                    && matches!(
                        entry.session.history,
                        HistoryState::Loaded | HistoryState::Failed(_)
                    )
            })
        });
        if !read {
            let shown = shown_id(self);
            panic!(
                "the second run did not show the session again with its history within {} s: \
                 listing {:?}; the session shown {}; its history {:?}",
                READ_BACK_LIMIT.as_secs(),
                self.app.listing,
                match shown {
                    Some(shown) if shown == *id => "is the test's",
                    Some(_) => "is another",
                    None => "is none",
                },
                self.app.shown_entry().map(|entry| &entry.session.history)
            );
        }
        self.key = self.app.shown;
        let session = self.session().expect("the test's session");
        if let HistoryState::Failed(reason) = &session.history {
            panic!("the session's history could not be read back from the store: {reason}");
        }
        assert_eq!(
            session.phase,
            Phase::Resting,
            "it rests, its program not started"
        );
        let items = self.items();
        let first = items
            .iter()
            .any(|body| matches!(body, ItemBody::User { text, .. } if text.contains(MESSAGE)));
        let replies = finished_replies(&items);
        assert!(
            first
                && replies
                    .iter()
                    .any(|reply| reply.to_lowercase().contains("ok")),
            "the history read back lacks the first message or its reply; conversation:\n\
             {items:#?}"
        );
        eprintln!(
            "live: read the session back from the store ({} items)",
            items.len()
        );
    }

    /// Sends the second message to the resting session and waits for the reply. The program
    /// must resume the same id. The store is asked first when the session last changed; the
    /// first run's saved state says this app closed the session then, so the app is not
    /// expected to ask whether another app has it open. It prints which happened, and when it
    /// did ask (the program wrote more than `OWN_EXIT_SLACK_SECONDS` after the first run
    /// closed) it continues here.
    fn resume(&mut self, id: &SessionId) {
        let key = self.key.expect("the test's session");
        let effects = self.app.tick(unix_now());
        self.carry_out(effects);
        let effects = self.app.submit(key, SECOND_MESSAGE);
        assert!(!effects.is_empty(), "the second message woke nothing");
        self.carry_out(effects);

        // A session this run has not run yet is first checked in the store.
        let decided = self.wait_for(ACTIVITY_LIMIT, |live| {
            live.app.prompt(key).is_some() || live.opened.is_some()
        });
        assert!(
            decided,
            "neither the program opened nor did the app ask within {} s of the second \
             message; the store answered when the session changed: {}",
            ACTIVITY_LIMIT.as_secs(),
            if self.activity.is_some() { "yes" } else { "no" }
        );
        let changed = match self.activity {
            Some(Some(unix)) => format!("{} s before", self.app.now_unix - unix),
            Some(None) => "never, by the store".into(),
            None => "unknown".into(),
        };
        if let Some(Prompt::OpenElsewhere { .. }) = self.app.prompt(key) {
            eprintln!("live: the session changed {changed}; the app asked first, continued here");
            let (effects, back) = self.app.answer_prompt(key, PromptChoice::ContinueHere);
            assert_eq!(back, None, "continuing gives no text back");
            self.carry_out(effects);
        } else {
            eprintln!("live: the session changed {changed}; the app did not ask");
        }
        let spec = self.opened.clone().expect("a program was opened");
        assert_eq!(
            spec.resume.as_ref(),
            Some(id),
            "it resumes the same session"
        );
        assert!(!spec.fork, "it resumes, not forks");
        self.running = true;

        let replied = |live: &Live| {
            live.phase() == Some(Phase::Idle) && {
                let items = live.items();
                let asked = items.iter().rposition(|body| {
                    matches!(body, ItemBody::User { text, .. } if text.contains(SECOND_MESSAGE))
                });
                asked.is_some_and(|at| !finished_replies(&items[at..]).is_empty())
            }
        };
        if !self.wait_for(TURN_LIMIT, replied) {
            panic!(
                "no reply to the second message within {} s (phase {:?}); conversation:\n{:#?}",
                TURN_LIMIT.as_secs(),
                self.phase(),
                self.items()
            );
        }
        let session = self.session().expect("the test's session");
        assert_eq!(session.id.as_ref(), Some(id), "the same session carried on");
        let items = self.items();
        let replies = finished_replies(&items);
        let last = replies.last().expect("a reply");
        assert!(
            last.to_lowercase().contains("again"),
            "the second reply does not say again: {last:?}"
        );
        let label = session.model_label();
        assert_ne!(label, "Model", "the model was never named");
        eprintln!(
            "live: resumed with {label} in {}",
            mode_label(&session.mode)
        );
    }

    /// The test's session in this run.
    fn session(&self) -> Option<&Session> {
        self.key
            .and_then(|key| self.app.sessions.get(key))
            .map(|entry| &entry.session)
    }

    /// The number of the test's session's current open, while its program runs.
    fn open(&self) -> Option<u64> {
        self.key
            .and_then(|key| self.app.sessions.get(key))
            .and_then(|entry| entry.open)
    }

    fn id(&self) -> Option<SessionId> {
        self.session().and_then(|s| s.id.clone())
    }

    /// Seconds since the start: what events are stamped with.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Takes every message waiting from the backend into the `App`, as the window's `poll`
    /// does, and carries out what comes back. Notes what the test reports on as it passes.
    fn take_in(&mut self) {
        while let Some(message) = self.backend.try_recv() {
            self.note(&message);
            let now = self.now();
            let effects = self.app.receive(message, now);
            self.carry_out(effects);
            if self.start_new && self.app.screen == Screen::Main {
                // As "New session" does, in the test's folder, before any listing is merged.
                self.start_new = false;
                let effects = self
                    .app
                    .new_session(self.folder.clone(), Leaving::ForgetIfEmpty);
                self.key = self.app.shown;
                self.carry_out(effects);
            }
        }
    }

    /// Notes what `message` says that the test reports on, before the `App` takes it.
    fn note(&mut self, message: &FromBackend) {
        match message {
            FromBackend::Event { open, event } if Some(*open) == self.open() => match event {
                SessionEvent::CommandFailed { what, reason } => {
                    self.refusals.push((*what, reason.clone()));
                }
                SessionEvent::TitleChanged(title) => self.titles.push(title.clone()),
                SessionEvent::PermissionModeChanged(mode) => self.modes.push(mode.clone()),
                SessionEvent::UsageLimits(limits) if limits.session.is_some() => {
                    self.session_usage = true;
                }
                _ => {}
            },
            FromBackend::Sessions { since_unix, result } => {
                let (asked, answered) = match since_unix {
                    Some(_) => (self.recent_asked, &mut self.recent),
                    None => (self.all_asked, &mut self.all),
                };
                if answered.is_none()
                    && let Some(asked) = asked
                {
                    *answered = Some(Answered {
                        took: asked.elapsed(),
                        count: result.as_ref().ok().map(Vec::len),
                    });
                }
            }
            FromBackend::Look { models, usage, .. } => {
                if let Some(asked) = self.look_asked {
                    self.looked = Some((asked.elapsed(), models.len(), usage.is_some()));
                }
            }
            FromBackend::History { key, result } => {
                if let Some(asked) = self.reading.get(key) {
                    self.read.insert(
                        *key,
                        Answered {
                            took: asked.elapsed(),
                            count: result.as_ref().ok().map(|history| history.items.len()),
                        },
                    );
                }
            }
            FromBackend::Activity { key, result } if Some(*key) == self.key => {
                self.activity = Some(result.as_ref().ok().copied().flatten());
            }
            _ => {}
        }
    }

    /// Takes messages in until `done` holds or `limit` has passed. True when `done` held.
    fn wait_for(&mut self, limit: Duration, done: impl Fn(&Live) -> bool) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            self.take_in();
            self.fail_on_a_dead_end();
            if done(self) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(TICK);
        }
    }

    /// The latest refusal of a command of this kind.
    fn refusal(&self, kind: CommandKind) -> Option<&str> {
        self.refusals
            .iter()
            .rev()
            .find(|(what, _)| *what == kind)
            .map(|(_, reason)| reason.as_str())
    }

    fn title(&self) -> Option<String> {
        self.session().and_then(|s| s.title.clone())
    }

    fn mode(&self) -> Option<PermissionMode> {
        self.session().map(|s| s.mode.clone())
    }

    /// Waits for the title `claude` gives the session in place of the stand-in made from the
    /// message, and prints it, or that none came (with the reason when it was refused). A title
    /// that `claude` sent counts even when it reads the same as the stand-in.
    fn report_title(&mut self, stand_in: Option<String>) {
        let named = self.wait_for(TITLE_LIMIT, |live| {
            !live.titles.is_empty() || live.title() != stand_in
        });
        if named {
            let title = self.titles.last().cloned().or_else(|| self.title());
            eprintln!("live: title {:?}", title.unwrap_or_default());
            return;
        }
        let seconds = TITLE_LIMIT.as_secs();
        match self.refusal(CommandKind::SuggestTitle) {
            Some(reason) => eprintln!("live: no title after {seconds} s (refused: {reason})"),
            None => eprintln!("live: no title after {seconds} s"),
        }
    }

    /// Chooses Auto when it is offered and prints whether `claude` accepted it, then goes back to
    /// "Ask before edits". Only failing to get back fails the test.
    ///
    /// Accepted: Ask is chosen through the app, which sends it because the mode in use is Auto.
    /// Refused: the mode never left Ask, so nothing is sent. No answer: `claude` may still accept
    /// Auto later, and the app would send nothing for Ask (it is the mode it knows), so the test
    /// sends `SetPermissionMode(Ask)` itself and passes only when `claude` confirms Ask.
    fn try_auto_mode(&mut self) {
        let offered = self.session().is_some_and(Session::auto_mode);
        if !offered {
            eprintln!("live: auto mode not offered");
            return;
        }
        let key = self.key.expect("the test's session");
        let refused_before = self.refusals.len();
        let effects = self.app.choose_mode(key, PermissionMode::Auto);
        self.carry_out(effects);
        let refused = |live: &Live| {
            live.refusals[refused_before..]
                .iter()
                .any(|(what, _)| *what == CommandKind::SetPermissionMode)
        };
        let answered = self.wait_for(MODE_LIMIT, |live| {
            live.mode() == Some(PermissionMode::Auto) || refused(live)
        });
        if self.mode() == Some(PermissionMode::Auto) {
            eprintln!("live: auto mode accepted");
        } else if answered {
            let reason = self
                .refusal(CommandKind::SetPermissionMode)
                .unwrap_or_default();
            eprintln!("live: auto mode refused: {reason}");
        } else {
            eprintln!(
                "live: auto mode: no answer after {} s",
                MODE_LIMIT.as_secs()
            );
        }
        if answered {
            let effects = self.app.choose_mode(key, PermissionMode::Ask);
            self.carry_out(effects);
            let back = self.wait_for(MODE_LIMIT, |live| live.mode() == Some(PermissionMode::Ask));
            assert!(
                back,
                "the session did not get back to \"Ask before edits\" within {} s: {:?}",
                MODE_LIMIT.as_secs(),
                self.mode()
            );
            return;
        }
        let (confirmed_before, refused_before) = (self.modes.len(), self.refusals.len());
        let open = self.open().expect("the session's program still runs");
        self.carry_out(vec![Effect::Send {
            open,
            command: SessionCommand::SetPermissionMode(PermissionMode::Ask),
        }]);
        let confirmed = |live: &Live| live.modes[confirmed_before..].contains(&PermissionMode::Ask);
        let refused = |live: &Live| {
            live.refusals[refused_before..]
                .iter()
                .any(|(what, _)| *what == CommandKind::SetPermissionMode)
        };
        self.wait_for(MODE_LIMIT, |live| confirmed(live) || refused(live));
        assert!(
            confirmed(self),
            "\"Ask before edits\" was not confirmed within {} s of asking for it again; \
             modes confirmed since: {:?}; refusals since: {:?}",
            MODE_LIMIT.as_secs(),
            &self.modes[confirmed_before..],
            &self.refusals[refused_before..]
        );
    }

    /// Sends what the `App` asks of the agent to the backend, noting when the test's reports
    /// start. Neither the width nor the state kept between runs is saved. A look is run in the
    /// test's folder only: the app asks for it in the folder of its newest session, which is
    /// the test's unless a session of the user's changed since the app started.
    fn carry_out(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            let message = match effect {
                Effect::SaveWidth(_) | Effect::SaveState(_) => continue,
                Effect::CheckAccount => ToBackend::CheckAccount,
                Effect::Open { open, spec } => {
                    if Some(open) == self.open() {
                        self.opened = Some(spec.clone());
                    }
                    ToBackend::Open { open, spec }
                }
                Effect::Send { open, command } => ToBackend::Send { open, command },
                Effect::ListSessions { since_unix } => {
                    let asked = match since_unix {
                        Some(_) => &mut self.recent_asked,
                        None => &mut self.all_asked,
                    };
                    asked.get_or_insert_with(Instant::now);
                    ToBackend::ListSessions { since_unix }
                }
                Effect::ReadHistory { key, id, max_items } => {
                    self.reading.insert(key, Instant::now());
                    ToBackend::ReadHistory { key, id, max_items }
                }
                Effect::CheckActivity { key, id } => ToBackend::CheckActivity { key, id },
                Effect::Look { folder } => {
                    if folder != self.folder {
                        eprintln!(
                            "live: the app asked for its look in the folder of a session newer \
                             than the test's; it runs in the test's folder instead"
                        );
                    }
                    self.look_asked.get_or_insert_with(Instant::now);
                    ToBackend::Look {
                        folder: self.folder.clone(),
                    }
                }
            };
            self.backend.send(message);
        }
    }

    fn phase(&self) -> Option<Phase> {
        self.session().map(|s| s.phase.clone())
    }

    fn items(&self) -> Vec<ItemBody> {
        self.session().map_or_else(Vec::new, |s| {
            s.conversation
                .items()
                .iter()
                .map(|i| i.body.clone())
                .collect()
        })
    }

    /// Fails at once on a state the run cannot get out of.
    fn fail_on_a_dead_end(&self) {
        match self.app.screen {
            Screen::NotInstalled => panic!("Claude Code is not installed (no `claude` found)"),
            Screen::SignedOut => panic!("Claude Code is not signed in: run `claude auth login`"),
            Screen::Checking | Screen::Main => {}
        }
        match self.phase() {
            Some(Phase::Ended { clean, detail }) => panic!(
                "the session ended before the reply (clean: {clean}): {detail}; conversation:\n{:#?}",
                self.items()
            ),
            Some(Phase::Resting) if self.running && !self.closed => panic!(
                "the session's program ended cleanly before the reply; conversation:\n{:#?}",
                self.items()
            ),
            Some(Phase::NeedsUser) => panic!(
                "the turn asked the user for something; conversation:\n{:#?}",
                self.items()
            ),
            _ => {}
        }
    }

    /// Sends `Close` to the test's session, once, unless its program has ended already.
    fn send_close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        if let Some(open) = self.open() {
            self.backend.send(ToBackend::Send {
                open,
                command: SessionCommand::Close,
            });
        }
    }

    /// Closes the app as the window does, and waits until the test's session has ended
    /// cleanly: with a message in it, it then rests. Returns what the app would save.
    fn close_app(&mut self) -> Saved {
        self.closed = true;
        let effects = self.app.close(unix_now());
        let saved = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::SaveState(saved) => Some((**saved).clone()),
                _ => None,
            })
            .expect("closing saves the state first");
        self.carry_out(effects);
        let deadline = Instant::now() + CLOSE_LIMIT;
        loop {
            self.take_in();
            match self.phase() {
                Some(Phase::Resting) => return saved,
                Some(Phase::Ended { clean, detail }) => {
                    panic!("the session did not rest after closing (clean: {clean}): {detail}")
                }
                phase => assert!(
                    Instant::now() < deadline,
                    "the session never ended within {} s of Close: phase {phase:?}",
                    CLOSE_LIMIT.as_secs()
                ),
            }
            std::thread::sleep(TICK);
        }
    }
}

impl Drop for Live {
    /// Closes the test's session if the test left without closing it, and gives it a moment to
    /// end, so a failure never leaves a `claude` running. Dropping the `Backend` afterwards
    /// also passes `Close` to every session its threads still hold, the look's included.
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        self.send_close();
        let deadline = Instant::now() + CLOSE_LIMIT;
        while Instant::now() < deadline {
            // Taken in only to see the session end; nothing more is sent.
            while let Some(message) = self.backend.try_recv() {
                let now = self.now();
                let _ = self.app.receive(message, now);
            }
            if matches!(
                self.phase(),
                Some(Phase::Resting | Phase::Ended { .. }) | None
            ) {
                return;
            }
            std::thread::sleep(TICK);
        }
        eprintln!(
            "live: the session had not ended {} s after Close",
            CLOSE_LIMIT.as_secs()
        );
    }
}

/// The finished replies among `items`, in order.
fn finished_replies(items: &[ItemBody]) -> Vec<&String> {
    items
        .iter()
        .filter_map(|body| match body {
            ItemBody::Reply {
                markdown,
                streaming: false,
            } => Some(markdown),
            _ => None,
        })
        .collect()
}

/// How a listing stands, in a word: how many it gave, failed, not answered or not asked.
fn listing_word(answer: &Option<Answered>, asked: Option<Instant>) -> String {
    match (answer, asked) {
        (Some(Answered { count: Some(n), .. }), _) => format!("{n} sessions"),
        (Some(Answered { count: None, .. }), _) => "failed".into(),
        (None, Some(_)) => "no answer".into(),
        (None, None) => "not asked".into(),
    }
}

/// The wall clock in Unix seconds.
fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
