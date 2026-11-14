//! Which screen shows, the one session, and the numbered opens that keep a
//! replaced session from changing anything. Every intent returns the effects
//! to carry out instead of doing the work, so this module stays plain data.

use std::path::PathBuf;

use mr_agent::{
    Account, AgentError, Answer, Capabilities, PermissionMode, RequestId, SessionCommand,
    SessionEvent, StartSpec, UsageLimits,
};
use serde::{Deserialize, Serialize};

use crate::backend::FromBackend;
use crate::conversation::NoticeKind;
use crate::session::{Phase, Session};

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

/// Work for the backend to carry out. Each `open` is the number of the open
/// the work belongs to.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    CheckAccount,
    Open { open: u64, spec: StartSpec },
    Send { open: u64, command: SessionCommand },
    SaveWidth(Width),
}

pub struct App {
    pub screen: Screen,
    pub account: Option<Account>,
    pub capabilities: Capabilities,
    pub usage: UsageLimits,
    pub folder: PathBuf,
    pub session: Option<Session>,
    pub width: Width,
    /// The number of the latest open; nothing from an earlier one counts.
    open: u64,
}

impl App {
    /// Starts on `Screen::Checking` and asks for the account.
    pub fn new(folder: PathBuf, capabilities: Capabilities, width: Width) -> (App, Vec<Effect>) {
        let app = App {
            screen: Screen::Checking,
            account: None,
            capabilities,
            usage: UsageLimits {
                session: None,
                weekly: None,
            },
            folder,
            session: None,
            width,
            open: 0,
        };
        (app, vec![Effect::CheckAccount])
    }

    pub fn current_open(&self) -> u64 {
        self.open
    }

    pub fn account_checked(&mut self, account: Account) -> Vec<Effect> {
        self.screen = match account {
            Account::Missing => Screen::NotInstalled,
            Account::SignedOut => Screen::SignedOut,
            Account::SignedIn { .. } => Screen::Main,
        };
        let signed_in = self.screen == Screen::Main;
        self.account = Some(account);
        if signed_in && self.session.is_none() {
            self.session = Some(Session::opening(self.folder.clone()));
            return vec![self.next_open(StartSpec::new(self.folder.clone()))];
        }
        Vec::new()
    }

    pub fn check_again(&mut self) -> Vec<Effect> {
        if matches!(self.screen, Screen::NotInstalled | Screen::SignedOut) {
            self.screen = Screen::Checking;
            return vec![Effect::CheckAccount];
        }
        Vec::new()
    }

    pub fn open_failed(&mut self, open: u64, error: AgentError) -> Vec<Effect> {
        if !self.is_current(open) {
            return Vec::new();
        }
        if error == AgentError::NotInstalled {
            self.screen = Screen::NotInstalled;
            self.session = None;
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
        if !self.is_current(open) {
            return Vec::new();
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
        if let Some(session) = &mut self.session {
            session
                .conversation
                .push_notice(NoticeKind::CommandFailed, &text);
        }
        Vec::new()
    }

    pub fn event(&mut self, open: u64, event: SessionEvent, now: f64) -> Vec<Effect> {
        if !self.is_current(open) {
            return Vec::new();
        }
        let mut effects = Vec::new();
        let started = matches!(event, SessionEvent::Started(_));
        if let SessionEvent::UsageLimits(limits) = &event {
            if limits.session.is_some() {
                self.usage.session = limits.session.clone();
            }
            if limits.weekly.is_some() {
                self.usage.weekly = limits.weekly.clone();
            }
        }
        if let Some(session) = &mut self.session {
            for command in session.apply(event, now) {
                effects.push(Effect::Send { open, command });
            }
        }
        if started && self.capabilities.usage_limits {
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
        }
    }

    pub fn new_session(&mut self) -> Vec<Effect> {
        if self.screen != Screen::Main {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if let Some(old) = &self.session
            && !ended(old)
        {
            effects.push(Effect::Send {
                open: self.open,
                command: SessionCommand::Close,
            });
        }
        self.session = Some(Session::opening(self.folder.clone()));
        effects.push(self.next_open(StartSpec::new(self.folder.clone())));
        effects
    }

    /// The app is going away: `Close` for the current session, unless it has ended already.
    pub fn close(&mut self) -> Vec<Effect> {
        if self.session_ended() {
            return Vec::new();
        }
        vec![Effect::Send {
            open: self.open,
            command: SessionCommand::Close,
        }]
    }

    /// Whether there is no session, or it has ended: nothing is left running for it.
    pub fn session_ended(&self) -> bool {
        self.session.as_ref().is_none_or(ended)
    }

    pub fn restart(&mut self) -> Vec<Effect> {
        let Some(session) = &mut self.session else {
            return Vec::new();
        };
        let Some(spec) = session.restart_spec() else {
            return Vec::new();
        };
        session.reopening();
        vec![self.next_open(spec)]
    }

    pub fn submit(&mut self, text: &str) -> Vec<Effect> {
        let commands = self
            .session
            .as_mut()
            .map(|s| s.submit(text))
            .unwrap_or_default();
        self.send(commands)
    }

    pub fn stop(&mut self) -> Vec<Effect> {
        let command = self.session.as_ref().and_then(Session::stop);
        self.send(command)
    }

    pub fn answer(&mut self, id: &RequestId, answer: Answer) -> Vec<Effect> {
        let command = self.session.as_mut().and_then(|s| s.answer(id, answer));
        self.send(command)
    }

    pub fn choose_model(&mut self, option_id: &str) -> Vec<Effect> {
        let command = self
            .session
            .as_mut()
            .and_then(|s| s.choose_model(option_id));
        self.send(command)
    }

    pub fn choose_mode(&mut self, mode: PermissionMode) -> Vec<Effect> {
        let command = self.session.as_ref().and_then(|s| s.choose_mode(mode));
        self.send(command)
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

    /// Whether `open` is the current open of a session that exists.
    fn is_current(&self, open: u64) -> bool {
        open == self.open && self.session.is_some()
    }

    fn next_open(&mut self, spec: StartSpec) -> Effect {
        self.open += 1;
        Effect::Open {
            open: self.open,
            spec,
        }
    }

    fn send(&self, commands: impl IntoIterator<Item = SessionCommand>) -> Vec<Effect> {
        commands
            .into_iter()
            .map(|command| Effect::Send {
                open: self.open,
                command,
            })
            .collect()
    }
}

fn ended(session: &Session) -> bool {
    matches!(session.phase, Phase::Ended { .. })
}

#[cfg(test)]
mod tests {
    use mr_agent::{LimitWindow, SessionId};

    use super::*;
    use crate::conversation::ItemBody;
    use crate::session::testing::info;

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

    fn ready() -> App {
        let (mut app, _) = App::new("/p".into(), caps(), Width::Unlimited);
        app.account_checked(signed_in());
        app.event(1, SessionEvent::Started(info()), 0.0);
        app
    }

    #[test]
    fn the_first_screen_depends_on_the_account() {
        let (mut app, effects) = App::new("/p".into(), caps(), Width::Points(DEFAULT_WIDTH));
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
            vec![Effect::Open {
                open: 1,
                spec: StartSpec::new("/p")
            }]
        );
        assert_eq!((app.screen, app.current_open()), (Screen::Main, 1));
        assert_eq!(app.session.as_ref().unwrap().phase, Phase::Opening);
        assert_eq!(app.check_again(), vec![]);
    }

    #[test]
    fn a_started_session_asks_for_the_usage_limits() {
        let (mut app, _) = App::new("/p".into(), caps(), Width::Unlimited);
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
        let (mut app, _) = App::new("/p".into(), without, Width::Unlimited);
        app.account_checked(signed_in());
        assert_eq!(app.event(1, SessionEvent::Started(info()), 0.0), vec![]);
    }

    #[test]
    fn usage_keeps_a_window_that_was_not_reported_this_time() {
        let mut app = ready();
        let window = |used| {
            Some(LimitWindow {
                used,
                resets_at_unix: 5,
            })
        };
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
    fn closing_the_app_closes_a_session_that_has_not_ended() {
        let (mut app, _) = App::new("/p".into(), caps(), Width::Unlimited);
        assert_eq!(app.close(), vec![], "no session yet");
        app.account_checked(signed_in());
        let close = vec![Effect::Send {
            open: 1,
            command: SessionCommand::Close,
        }];
        assert_eq!(app.close(), close, "a session still opening");
        app.event(1, SessionEvent::Started(info()), 0.0);
        assert!(!app.session_ended());
        assert_eq!(app.close(), close);
        let exited = SessionEvent::Exited {
            clean: true,
            detail: String::new(),
        };
        app.event(1, exited, 0.0);
        assert!(app.session_ended());
        assert_eq!(app.close(), vec![], "already ended");
    }

    #[test]
    fn a_replaced_session_can_no_longer_change_anything() {
        let mut app = ready();
        let sent = app.submit("go");
        assert_eq!(
            sent,
            vec![
                Effect::Send {
                    open: 1,
                    command: SessionCommand::Send {
                        text: "go".into(),
                        attachments: vec![]
                    }
                },
                Effect::Send {
                    open: 1,
                    command: SessionCommand::SuggestTitle { from: "go".into() }
                }
            ]
        );
        let effects = app.new_session();
        assert_eq!(
            effects,
            vec![
                Effect::Send {
                    open: 1,
                    command: SessionCommand::Close
                },
                Effect::Open {
                    open: 2,
                    spec: StartSpec::new("/p")
                },
            ]
        );
        assert!(
            app.session
                .as_ref()
                .unwrap()
                .conversation
                .items()
                .is_empty()
        );
        for stale in [
            SessionEvent::TextDelta("late".into()),
            SessionEvent::UsageLimits(UsageLimits {
                session: Some(LimitWindow {
                    used: 0.9,
                    resets_at_unix: 1,
                }),
                weekly: None,
            }),
            SessionEvent::Exited {
                clean: true,
                detail: String::new(),
            },
        ] {
            assert_eq!(app.event(1, stale, 0.0), vec![]);
        }
        assert_eq!(app.open_failed(1, AgentError::Io("x".into())), vec![]);
        let session = app.session.as_ref().unwrap();
        assert_eq!(session.phase, Phase::Opening);
        assert!(session.conversation.items().is_empty());
        assert_eq!(
            app.usage,
            UsageLimits {
                session: None,
                weekly: None
            }
        );
    }

    #[test]
    fn an_ended_session_restarts_where_it_was() {
        let mut app = ready();
        app.submit("go");
        app.event(
            1,
            SessionEvent::Exited {
                clean: false,
                detail: "killed".into(),
            },
            0.0,
        );
        assert_eq!(
            app.new_session().len(),
            1,
            "an ended session is not sent Close"
        );
        let mut app = ready();
        app.submit("go");
        assert_eq!(app.restart(), vec![], "a live session cannot be restarted");
        app.event(
            1,
            SessionEvent::Exited {
                clean: false,
                detail: "killed".into(),
            },
            0.0,
        );
        let effects = app.restart();
        let mut spec = StartSpec::new("/p");
        spec.resume = Some(SessionId("s1".into()));
        assert_eq!(effects, vec![Effect::Open { open: 2, spec }]);
        assert!(app.session.as_ref().unwrap().conversation.items().len() >= 2);
    }

    #[test]
    fn a_failed_open_shows_where_the_problem_is() {
        let (mut app, _) = App::new("/p".into(), caps(), Width::Unlimited);
        app.account_checked(signed_in());
        app.open_failed(1, AgentError::Io("no such folder".into()));
        assert_eq!(
            app.session.as_ref().unwrap().phase,
            Phase::Ended {
                clean: false,
                detail: "i/o error: no such folder".into()
            }
        );
        let (mut app, _) = App::new("/p".into(), caps(), Width::Unlimited);
        app.account_checked(signed_in());
        app.open_failed(1, AgentError::NotInstalled);
        assert_eq!(app.screen, Screen::NotInstalled);
        assert!(app.session.is_none());
    }

    #[test]
    fn a_refused_answer_is_explained_and_a_closed_session_is_not() {
        let mut app = ready();
        let answer = SessionCommand::Answer {
            id: RequestId("r1".into()),
            answer: Answer::Allow { always: false },
        };
        app.send_failed(1, &answer, AgentError::Closed);
        assert!(
            app.session
                .as_ref()
                .unwrap()
                .conversation
                .items()
                .is_empty()
        );
        app.send_failed(1, &answer, AgentError::Protocol("not waiting".into()));
        let last = app
            .session
            .as_ref()
            .unwrap()
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
    fn a_title_request_that_could_not_be_sent_is_not_explained() {
        let mut app = ready();
        let suggest = SessionCommand::SuggestTitle { from: "go".into() };
        app.send_failed(1, &suggest, AgentError::Io("broken pipe".into()));
        app.send_failed(1, &suggest, AgentError::Protocol("odd".into()));
        assert!(
            app.session
                .as_ref()
                .unwrap()
                .conversation
                .items()
                .is_empty()
        );
    }

    #[test]
    fn each_message_from_the_backend_reaches_its_handler() {
        let (mut app, _) = App::new("/p".into(), caps(), Width::Unlimited);
        let effects = app.receive(FromBackend::Account(signed_in()), 0.0);
        assert_eq!(
            effects,
            vec![Effect::Open {
                open: 1,
                spec: StartSpec::new("/p")
            }]
        );
        assert_eq!(app.screen, Screen::Main);

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
        app.submit("go");
        app.receive(
            FromBackend::Event {
                open: 1,
                event: SessionEvent::ThinkingStarted,
            },
            2.0,
        );
        let items = app.session.as_ref().unwrap().conversation.items();
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
        let last = &app
            .session
            .as_ref()
            .unwrap()
            .conversation
            .items()
            .last()
            .unwrap()
            .body;
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
        assert!(app.session.is_none());
    }

    #[test]
    fn the_width_has_a_floor_and_saves_only_changes() {
        let mut app = ready();
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
}
