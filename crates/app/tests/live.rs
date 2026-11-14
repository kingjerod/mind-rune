//! One real turn through the same `App` and `Backend` the window uses, against the real
//! `claude`. Opt-in: it is ignored by default and returns at once unless `MINDRUNE_LIVE=1`.
//!
//! ```bash
//! MINDRUNE_LIVE=1 cargo test -p mindrune --test live -- --ignored --nocapture
//! ```
//!
//! It needs Claude Code installed and signed in, opens a session in a fresh temporary folder,
//! asks for a one-word reply and closes the session. Every way out of the test, a failed
//! assertion included, closes the session, so no `claude` is left running.
//!
//! After the reply it also prints what came of the requests read from the program but not yet
//! seen on the wire: the session's title, the model's label, and whether `claude` accepts the
//! Auto permission mode. None of these fails the test, except not getting back to "Ask before
//! edits" after trying Auto.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mr_agent::{Agent, CommandKind, PermissionMode, SessionCommand, SessionEvent};
use mr_claude::ClaudeAgent;
use mr_core::{
    App, Backend, DEFAULT_WIDTH, Effect, FromBackend, ItemBody, Phase, Screen, ToBackend, Width,
};

/// The whole run, from the account check to the reply.
const TURN_LIMIT: Duration = Duration::from_secs(120);

/// How long the session may take to end once it was told to close.
const CLOSE_LIMIT: Duration = Duration::from_secs(20);

/// How long the loop sleeps between two looks at the backend.
const TICK: Duration = Duration::from_millis(20);

/// How long the title may take to replace the stand-in, after the reply.
const TITLE_LIMIT: Duration = Duration::from_secs(30);

/// How long a permission mode change may take to be confirmed or refused.
const MODE_LIMIT: Duration = Duration::from_secs(10);

const MESSAGE: &str = "Reply with the single word: ok";

#[test]
#[ignore = "starts the real claude; run with MINDRUNE_LIVE=1 and --ignored"]
fn one_real_turn_from_the_first_message_to_a_closed_session() {
    if std::env::var("MINDRUNE_LIVE").as_deref() != Ok("1") {
        eprintln!("live: skipped; set MINDRUNE_LIVE=1 to run it");
        return;
    }
    let folder = tempfile::tempdir().expect("a temporary folder");
    let mut live = Live::start(folder.path().to_path_buf());
    let deadline = live.started + TURN_LIMIT;

    // Wait for the session to be ready, send the message once, and wait for the turn to end.
    let mut sent = false;
    let mut stand_in = None;
    loop {
        live.take_in();
        live.fail_on_a_dead_end();
        let phase = live.phase();
        if phase == Some(Phase::Idle) {
            if !sent {
                let effects = live.app.submit(MESSAGE);
                assert!(!effects.is_empty(), "the message was not sent: {effects:?}");
                live.carry_out(effects);
                sent = true;
                stand_in = live.title();
            } else if live.app.usage.session.is_some() {
                break;
            }
        }
        if Instant::now() >= deadline {
            if !sent {
                panic!(
                    "the session never reached Idle within {} s: screen {:?}, phase {phase:?}",
                    TURN_LIMIT.as_secs(),
                    live.app.screen
                );
            }
            if phase == Some(Phase::Idle) {
                panic!(
                    "the turn ended but the usage limits never arrived within {} s",
                    TURN_LIMIT.as_secs()
                );
            }
            panic!(
                "no reply within {} s (phase {phase:?}); conversation:\n{:#?}",
                TURN_LIMIT.as_secs(),
                live.items()
            );
        }
        std::thread::sleep(TICK);
    }

    let items = live.items();
    let replies: Vec<&String> = items
        .iter()
        .filter_map(|body| match body {
            ItemBody::Reply {
                markdown,
                streaming: false,
            } => Some(markdown),
            _ => None,
        })
        .collect();
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
    assert!(live.app.usage.session.is_some(), "no session usage");
    let label = live.app.session.as_ref().unwrap().model_label();
    assert_ne!(label, "Model", "the model was never named");
    eprintln!("live: replied {:?} with {label}", replies[0]);

    live.report_title(stand_in);
    eprintln!("live: model label {label:?}");
    live.try_auto_mode();

    live.close_and_wait();
}

/// The `App` and the `Backend` over the real agent, the way the window holds them. Dropping it
/// before the session closed cleanly (a failed assertion, a timeout) closes the session.
struct Live {
    app: App,
    backend: Backend,
    started: Instant,
    closed: bool,
    /// Every command of the current session the agent refused, with its reason, in order. The
    /// core keeps a refused title request to itself, so the test reads them as they come.
    refusals: Vec<(CommandKind, String)>,
    /// Every title the current session was given, in order, as the agent sent it.
    titles: Vec<String>,
    /// Every permission mode the agent confirmed for the current session, in order. Unlike the
    /// session's `mode`, this shows a confirmation of the mode already in use.
    modes: Vec<PermissionMode>,
}

impl Live {
    fn start(folder: std::path::PathBuf) -> Live {
        let agent: Arc<dyn Agent> = Arc::new(ClaudeAgent::from_environment());
        let (app, effects) = App::new(folder, agent.capabilities(), Width::Points(DEFAULT_WIDTH));
        let backend = Backend::start(agent, Arc::new(|| {}));
        let mut live = Live {
            app,
            backend,
            started: Instant::now(),
            closed: false,
            refusals: Vec::new(),
            titles: Vec::new(),
            modes: Vec::new(),
        };
        live.carry_out(effects);
        live
    }

    /// Seconds since the start: what events are stamped with.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Takes every message waiting from the backend into the `App`, as the window's `poll`
    /// does, and carries out what comes back.
    fn take_in(&mut self) {
        while let Some(message) = self.backend.try_recv() {
            if let FromBackend::Event { open, event } = &message
                && *open == self.app.current_open()
            {
                match event {
                    SessionEvent::CommandFailed { what, reason } => {
                        self.refusals.push((*what, reason.clone()));
                    }
                    SessionEvent::TitleChanged(title) => self.titles.push(title.clone()),
                    SessionEvent::PermissionModeChanged(mode) => self.modes.push(mode.clone()),
                    _ => {}
                }
            }
            let now = self.now();
            let effects = self.app.receive(message, now);
            self.carry_out(effects);
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
        self.app.session.as_ref().and_then(|s| s.title.clone())
    }

    fn mode(&self) -> Option<PermissionMode> {
        self.app.session.as_ref().map(|s| s.mode.clone())
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
        let offered = self
            .app
            .session
            .as_ref()
            .is_some_and(mr_core::Session::auto_mode);
        if !offered {
            eprintln!("live: auto mode not offered");
            return;
        }
        let refused_before = self.refusals.len();
        let effects = self.app.choose_mode(PermissionMode::Auto);
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
            let effects = self.app.choose_mode(PermissionMode::Ask);
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
        let open = self.app.current_open();
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

    /// Sends what the `App` asks of the agent to the backend. The width is not saved.
    fn carry_out(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            let message = match effect {
                Effect::SaveWidth(_) => continue,
                Effect::CheckAccount => ToBackend::CheckAccount,
                Effect::Open { open, spec } => ToBackend::Open { open, spec },
                Effect::Send { open, command } => ToBackend::Send { open, command },
            };
            self.backend.send(message);
        }
    }

    fn phase(&self) -> Option<Phase> {
        self.app.session.as_ref().map(|s| s.phase.clone())
    }

    fn items(&self) -> Vec<ItemBody> {
        self.app.session.as_ref().map_or_else(Vec::new, |s| {
            s.conversation
                .items()
                .iter()
                .map(|i| i.body.clone())
                .collect()
        })
    }

    /// Fails at once on a state the turn cannot get out of.
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
            Some(Phase::NeedsUser) => panic!(
                "the turn asked the user for something; conversation:\n{:#?}",
                self.items()
            ),
            _ => {}
        }
    }

    /// Sends `Close` to the current session, once.
    fn send_close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.backend.send(ToBackend::Send {
            open: self.app.current_open(),
            command: SessionCommand::Close,
        });
    }

    /// Closes the session and waits until it has ended cleanly.
    fn close_and_wait(&mut self) {
        self.send_close();
        let deadline = Instant::now() + CLOSE_LIMIT;
        loop {
            self.take_in();
            match self.phase() {
                Some(Phase::Ended { clean: true, .. }) => return,
                Some(Phase::Ended {
                    clean: false,
                    detail,
                }) => panic!("the session did not end cleanly: {detail}"),
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
    /// Closes the session if the test left without closing it, and gives it a moment to end,
    /// so a failure never leaves a `claude` running. Dropping the `Backend` afterwards also
    /// passes `Close` to every session its worker still holds.
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
            if matches!(self.phase(), Some(Phase::Ended { .. }) | None) {
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
