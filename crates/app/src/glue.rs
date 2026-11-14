//! The window's state, and the glue between it and the core: effects are
//! carried out here and the backend's messages are taken in here, so the
//! views only read `Ui` and call the `App`'s intents.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mr_agent::{Agent, FakeAgent, RequestId};
use mr_core::{
    App, Backend, DEFAULT_WIDTH, Effect, FromBackend, ItemBody, ItemId, Phase, RequestState,
    ToBackend, Width,
};
use rune::prefs::Prefs;
use rune::runtime::Waker;
use rune::{TextEdit, Tx};
use rune_kit::shell::PaneState;

use crate::demo;

/// The settings key the conversation's width is kept under.
const WIDTH_KEY: &str = "conversation_width";

/// How often a usage window's reset wording is brought up to date.
const USAGE_TICK: Duration = Duration::from_secs(30);

/// How often the demo window polls, so the fake's scripted replies play on their own.
const DEMO_TICK: Duration = Duration::from_millis(50);

/// Which of the input box's pickers has its list open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picker {
    Model,
    Mode,
}

/// One question's answer in the making: the options chosen, in the order they were chosen,
/// and the "Something else" field.
#[derive(Clone, Default, PartialEq, Hash)]
pub struct QuestionDraft {
    pub chosen: Vec<String>,
    pub other: TextEdit,
}

pub struct Ui {
    pub app: App,
    pub agent: Arc<dyn Agent>,
    /// Present in the demo window and in tests: the agent, as the fake it is.
    pub fake: Option<FakeAgent>,
    pub backend: Option<Backend>,
    pub prefs: Prefs,
    pub draft: TextEdit,
    pub sidebar: PaneState,
    pub open_picker: Option<Picker>,
    /// Items the user has opened or closed, against each kind's default.
    pub toggled: HashSet<ItemId>,
    /// Items whose long output the user asked to see whole.
    pub whole: HashSet<ItemId>,
    /// What the user has chosen and typed so far in each waiting question, one draft per
    /// question. A request's drafts are dropped as soon as it stops waiting.
    pub answers: HashMap<RequestId, Vec<QuestionDraft>>,
    /// The conversation is scrolled to its end.
    pub following: bool,
    pub now_unix: i64,
    pub unix_clock: fn() -> i64,
    /// Messages for the backend that arrived before `setup` started it.
    waiting: Vec<ToBackend>,
    /// The `now` of the last `poll`.
    last_poll: Option<f64>,
    /// In the demo, the seconds the fake has been advanced: what events are stamped with.
    pub(crate) fake_time: f64,
}

impl Ui {
    pub fn new(
        agent: Arc<dyn Agent>,
        fake: Option<FakeAgent>,
        folder: PathBuf,
        prefs: Prefs,
    ) -> Ui {
        let width = prefs
            .get::<Width>(WIDTH_KEY)
            .unwrap_or(Width::Points(DEFAULT_WIDTH));
        let (app, effects) = App::new(folder, agent.capabilities(), width);
        let unix_clock: fn() -> i64 = system_unix;
        let mut ui = Ui {
            app,
            agent,
            fake,
            backend: None,
            prefs,
            draft: focused_draft(),
            sidebar: PaneState::new(260.0).range(200.0, 420.0),
            open_picker: None,
            toggled: HashSet::new(),
            whole: HashSet::new(),
            answers: HashMap::new(),
            following: true,
            now_unix: unix_clock(),
            unix_clock,
            waiting: Vec::new(),
            last_poll: None,
            fake_time: 0.0,
        };
        run(&mut ui, effects);
        ui
    }

    /// Forgets the messages waiting for `setup`, so they are never carried out.
    /// Only for the demo's `checking` scene, whose account check never comes back.
    pub(crate) fn forget_waiting(&mut self) {
        self.waiting.clear();
    }
}

/// An empty draft, focused: the text area takes keyboard focus as soon as it is first drawn,
/// which is when the main screen first shows.
fn focused_draft() -> TextEdit {
    let mut draft = TextEdit::new("");
    draft.focused = true;
    draft
}

fn system_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `AppSpec::setup`: starts the backend with the window's waker and carries out what was waiting.
/// A second call does nothing, so a scene that started its own backend keeps it.
pub fn setup(ui: &mut Ui, waker: Waker) {
    if ui.backend.is_some() {
        return;
    }
    let backend = Backend::start(ui.agent.clone(), waker);
    for message in ui.waiting.drain(..) {
        backend.send(message);
    }
    ui.backend = Some(backend);
}

/// `AppSpec::poll`.
pub fn poll(ui: &mut Ui, now: f64, tx: &mut Tx) -> bool {
    let since = ui.last_poll.map_or(0.0, |last| (now - last).max(0.0));
    ui.last_poll = Some(now);
    let mut changed = false;
    let stamp = if ui.fake.is_some() {
        changed |= demo::pump(ui, since as f32);
        tx.wake_after(DEMO_TICK);
        ui.fake_time
    } else {
        now
    };
    changed |= take_in(ui, stamp);
    let minute = ui.now_unix / 60;
    ui.now_unix = (ui.unix_clock)();
    let usage = &ui.app.usage;
    if usage.session.is_some() || usage.weekly.is_some() {
        changed |= ui.now_unix / 60 != minute;
        tx.wake_after(USAGE_TICK);
    }
    changed
}

/// Carries effects out: backend messages are sent, the width is saved.
/// Messages for the backend wait until `setup` has started it.
///
/// Every change to the `App` (an intent, or a message taken in) is followed by a call to this,
/// so it is where a request's drafts are dropped once the request stops waiting.
pub fn run(ui: &mut Ui, effects: Vec<Effect>) {
    drop_settled_drafts(ui);
    shut_disabled_picker(ui);
    for effect in effects {
        let message = match effect {
            Effect::SaveWidth(width) => {
                if let Err(error) = ui.prefs.set(WIDTH_KEY, &width) {
                    eprintln!("mindrune: could not save the conversation width: {error}");
                }
                continue;
            }
            Effect::CheckAccount => ToBackend::CheckAccount,
            Effect::Open { open, spec } => ToBackend::Open { open, spec },
            Effect::Send { open, command } => ToBackend::Send { open, command },
        };
        match &ui.backend {
            Some(backend) => backend.send(message),
            None => ui.waiting.push(message),
        }
    }
}

/// Forgets how the session that was just replaced was shown: the rows the user opened or
/// closed, the outputs shown whole, and a picker list left open. A new conversation numbers
/// its items from 0 again, so what was kept by item would land on the new session's items.
/// Called when "New session" replaces the session; Restart keeps the conversation and its
/// numbers, and so keeps all of this.
pub fn forget_session_view(ui: &mut Ui) {
    ui.toggled.clear();
    ui.whole.clear();
    ui.open_picker = None;
}

/// Shuts the picker list that is open while its picker cannot be used (the session is opening,
/// has ended, or there is none; for the model picker, also when there are no models to choose
/// from), so it does not open again by itself once it can.
fn shut_disabled_picker(ui: &mut Ui) {
    let session = ui.app.session.as_ref();
    let usable = session.is_some_and(|s| !matches!(s.phase, Phase::Opening | Phase::Ended { .. }));
    let models = session.is_some_and(|s| !s.models.is_empty());
    if !usable || (ui.open_picker == Some(Picker::Model) && !models) {
        ui.open_picker = None;
    }
}

/// How long closing the window waits for its session to end: a little more than the adapter
/// gives `claude` after its input closes before it kills it, so its end is always seen.
const CLOSE_WAIT: Duration = Duration::from_millis(2500);

/// `App::on_close`: the window is closing, and the app with it. Ends the session, waiting up to
/// [`CLOSE_WAIT`] for it, so the program that runs it is not left going without a window.
pub fn closing(ui: &mut Ui, _tx: &mut Tx) {
    if let Some(warning) = close_warning(ui, CLOSE_WAIT) {
        eprintln!("{warning}");
    }
}

/// Ends the session as [`close_session`] does, and says so when it had not ended after `limit`.
/// The demo's fake is only told to close, never waited for: nothing is wrong there, so there is
/// nothing to say.
pub(crate) fn close_warning(ui: &mut Ui, limit: Duration) -> Option<String> {
    if close_session(ui, limit) || ui.fake.is_some() {
        return None;
    }
    Some(format!(
        "mindrune: the session had not ended {} ms after the window closed",
        limit.as_millis()
    ))
}

/// Sends `Close` for the current session and waits up to `limit` for it to end. True when no
/// session is left running. The demo's fake is only told to close: its time does not pass
/// unless it is pumped, so nothing would come.
///
/// This wait is the one exception to the rule that the window's thread never blocks. It is
/// bounded, and it happens only as the window closes: once the app has quit, nothing would be
/// left to see that the program stops, and the adapter's own kill after its grace period runs
/// on a thread that dies with the app.
pub(crate) fn close_session(ui: &mut Ui, limit: Duration) -> bool {
    let effects = ui.app.close();
    if effects.is_empty() {
        return true;
    }
    run(ui, effects);
    if ui.fake.is_some() {
        return false;
    }
    let Some(backend) = &ui.backend else {
        return false;
    };
    let deadline = Instant::now() + limit;
    while !ui.app.session_ended() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let Some(message) = backend.recv_timeout(left) else {
            break;
        };
        // What the session would do next is not carried out: it is closing.
        let _ = ui.app.receive(message, 0.0);
    }
    ui.app.session_ended()
}

/// Drops the drafts of every request that is not waiting in the current session: answered,
/// withdrawn, or gone with a session that ended or was replaced. A later session that reuses a
/// request's id then starts from an empty draft.
///
/// Only the end of the conversation is read: the walk stops once it has met every request
/// still waiting, which come after everything a finished turn left.
fn drop_settled_drafts(ui: &mut Ui) {
    if ui.answers.is_empty() {
        return;
    }
    let mut waiting = Vec::new();
    if let Some(session) = &ui.app.session {
        let count = session.conversation.waiting();
        for item in session.conversation.items().iter().rev() {
            if waiting.len() == count {
                break;
            }
            if let ItemBody::Request {
                request,
                state: RequestState::Waiting,
            } = &item.body
            {
                waiting.push(&request.id);
            }
        }
    }
    ui.answers.retain(|id, _| waiting.contains(&id));
}

/// Takes every message waiting from the backend in one go, stamping events with `now`, and
/// carries out the effects that come back. True when at least one message was taken in.
pub(crate) fn take_in(ui: &mut Ui, now: f64) -> bool {
    let Some(backend) = &ui.backend else {
        return false;
    };
    let messages: Vec<FromBackend> = std::iter::from_fn(|| backend.try_recv()).collect();
    let took = !messages.is_empty();
    for message in messages {
        let effects = ui.app.receive(message, now);
        run(ui, effects);
    }
    took
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo, testkit};
    use mr_agent::{
        Account, AgentError, Capabilities, EventSink, FakeAgent, HistoryItem, PermissionMode,
        SessionCommand, SessionEvent, SessionHandle, SessionId, SessionInfo, StartSpec,
        StoredSession,
    };
    use mr_core::{ItemBody, Phase, Screen, Width};
    use rune::prefs::Prefs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    fn the_demo_window_reaches_the_main_screen_and_asks_for_usage() {
        let rt = testkit::window("empty");
        assert_eq!(rt.state.app.screen, Screen::Main);
        assert_eq!(rt.state.app.session.as_ref().unwrap().phase, Phase::Idle);
        assert!(testkit::sent(&rt).contains(&SessionCommand::RefreshUsage));
        assert!(rt.find("mr-tile").is_some());
        assert_eq!(rt.state.app.usage.session.as_ref().unwrap().percent(), 37);
    }

    #[test]
    fn each_account_state_has_its_screen() {
        for (scene, key) in [
            ("checking", "mr-screen-checking"),
            ("signed-out", "mr-screen-signin"),
            ("not-installed", "mr-screen-missing"),
            ("empty", "mr-tile"),
        ] {
            let rt = testkit::window(scene);
            assert!(rt.find(key).is_some(), "{scene}");
            assert!(rt.find("mr-root").is_some(), "{scene}");
        }
    }

    #[test]
    fn nothing_arriving_means_nothing_to_redraw() {
        let mut rt = testkit::window("empty");
        assert!(!demo::pump(&mut rt.state, 0.0));
        testkit::emit(&mut rt, SessionEvent::TitleChanged("Fix the parser".into()));
        assert_eq!(
            rt.state.app.session.as_ref().unwrap().title.as_deref(),
            Some("Fix the parser")
        );
    }

    #[test]
    fn in_the_demo_events_are_stamped_with_the_fakes_time() {
        let mut ui = demo::ui();
        (demo::SCENES
            .iter()
            .find(|(name, _)| *name == "empty")
            .unwrap()
            .1)(&mut ui);
        let fake = ui.fake.clone().unwrap();
        let id = ui.app.session.as_ref().unwrap().id.clone().unwrap();
        fake.emit(&id, SessionEvent::ThinkingStarted);
        demo::pump(&mut ui, 0.0);
        fake.emit(&id, SessionEvent::BlockFinished);
        demo::pump(&mut ui, 4.0);
        let items = ui.app.session.as_ref().unwrap().conversation.items();
        let Some(ItemBody::Thinking { seconds, .. }) = items.last().map(|i| &i.body) else {
            panic!("no thinking item: {items:?}");
        };
        assert_eq!(*seconds, Some(4.0));
    }

    #[test]
    fn closing_the_demo_window_closes_its_session_without_waiting() {
        let mut rt = testkit::window("working");
        let started = Instant::now();
        assert!(!close_session(&mut rt.state, CLOSE_WAIT));
        assert!(started.elapsed() < Duration::from_millis(500), "no wait");
        rt.state.backend.as_ref().unwrap().flush();
        assert_eq!(
            testkit::sent(&rt).last(),
            Some(&SessionCommand::Close),
            "{:?}",
            testkit::sent(&rt)
        );
        let mut rt = testkit::window("ended");
        let before = testkit::sent(&rt).len();
        assert!(close_session(&mut rt.state, CLOSE_WAIT), "nothing to close");
        rt.state.backend.as_ref().unwrap().flush();
        assert_eq!(testkit::sent(&rt).len(), before);
    }

    #[test]
    fn closing_the_demo_window_has_nothing_to_report() {
        let mut rt = testkit::window("working");
        assert_eq!(close_warning(&mut rt.state, CLOSE_WAIT), None);
        rt.state.backend.as_ref().unwrap().flush();
        assert_eq!(testkit::sent(&rt).last(), Some(&SessionCommand::Close));
    }

    /// An agent whose sessions start at once and end `exit_after` a `Close`, or never.
    struct SlowExit {
        exit_after: Option<Duration>,
        closes: Arc<AtomicUsize>,
    }

    struct SlowExitHandle {
        sink: Arc<Mutex<EventSink>>,
        exit_after: Option<Duration>,
        closes: Arc<AtomicUsize>,
    }

    impl Agent for SlowExit {
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                remote_access: false,
                plan_mode: true,
                usage_limits: false,
                images: false,
            }
        }
        fn account(&self) -> Account {
            Account::SignedIn {
                name: "alex".into(),
                plan: None,
                full_login: true,
            }
        }
        fn list_sessions(&self, _: Option<i64>) -> Result<Vec<StoredSession>, AgentError> {
            Ok(Vec::new())
        }
        fn read_session(&self, _: &SessionId) -> Result<Vec<HistoryItem>, AgentError> {
            Ok(Vec::new())
        }
        fn open(
            &self,
            spec: StartSpec,
            sink: EventSink,
        ) -> Result<Box<dyn SessionHandle>, AgentError> {
            sink(SessionEvent::Started(SessionInfo {
                id: SessionId("s1".into()),
                folder: spec.folder,
                model: "claude-haiku-5-5".into(),
                permission_mode: PermissionMode::Ask,
                agent_version: "2.1.319".into(),
                models: Vec::new(),
            }));
            Ok(Box::new(SlowExitHandle {
                sink: Arc::new(Mutex::new(sink)),
                exit_after: self.exit_after,
                closes: self.closes.clone(),
            }))
        }
    }

    impl SessionHandle for SlowExitHandle {
        fn send(&self, command: SessionCommand) -> Result<(), AgentError> {
            if command == SessionCommand::Close {
                self.closes.fetch_add(1, Ordering::SeqCst);
                if let Some(after) = self.exit_after {
                    let sink = self.sink.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(after);
                        (sink.lock().unwrap())(SessionEvent::Exited {
                            clean: true,
                            detail: String::new(),
                        });
                    });
                }
            }
            Ok(())
        }
    }

    /// A window state over `agent` (not the demo's fake), its session started and idle.
    fn idle_over(agent: Arc<SlowExit>) -> Ui {
        let mut ui = Ui::new(agent, None, "/home/alex/project".into(), Prefs::memory());
        setup(&mut ui, Arc::new(|| {}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ui
            .app
            .session
            .as_ref()
            .is_some_and(|s| s.phase == Phase::Idle)
        {
            take_in(&mut ui, 0.0);
            assert!(Instant::now() < deadline, "the session never started");
            std::thread::sleep(Duration::from_millis(2));
        }
        ui
    }

    #[test]
    fn closing_the_window_waits_for_the_session_to_end() {
        let agent = Arc::new(SlowExit {
            exit_after: Some(Duration::from_millis(300)),
            closes: Arc::new(AtomicUsize::new(0)),
        });
        let mut ui = idle_over(agent.clone());
        let started = Instant::now();
        assert!(close_session(&mut ui, CLOSE_WAIT));
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(300), "{waited:?}");
        assert!(
            waited < CLOSE_WAIT,
            "it stops waiting once the session ended: {waited:?}"
        );
        assert!(ui.app.session_ended());
        assert_eq!(agent.closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn closing_the_window_waits_no_longer_than_its_limit() {
        let agent = Arc::new(SlowExit {
            exit_after: None,
            closes: Arc::new(AtomicUsize::new(0)),
        });
        let mut ui = idle_over(agent.clone());
        let limit = Duration::from_millis(200);
        let started = Instant::now();
        assert_eq!(
            close_warning(&mut ui, limit).as_deref(),
            Some("mindrune: the session had not ended 200 ms after the window closed")
        );
        let waited = started.elapsed();
        assert!(
            waited >= limit && waited < Duration::from_secs(2),
            "{waited:?}"
        );
        assert_eq!(agent.closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_width_is_read_from_settings_and_written_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let mut prefs = Prefs::at(&path);
        prefs
            .set("conversation_width", &Width::Points(720.0))
            .unwrap();
        let fake = FakeAgent::new();
        let mut ui = Ui::new(
            Arc::new(fake.clone()),
            Some(fake),
            "/p".into(),
            Prefs::at(&path),
        );
        assert_eq!(ui.app.width, Width::Points(720.0));
        let effects = ui.app.set_width(Width::Unlimited);
        run(&mut ui, effects);
        assert_eq!(
            Prefs::at(&path).get::<Width>("conversation_width"),
            Some(Width::Unlimited)
        );
    }
}
