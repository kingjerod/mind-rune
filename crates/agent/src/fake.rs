//! A scripted `Agent` for tests. Time is virtual: nothing is delivered until
//! the test calls `advance`, and then in order of due time.
//!
//! It keeps the contract the real adapter keeps: `Started` (with a fixed list
//! of two made-up models) first, a new id for a fork and the old one for a
//! plain resume, and `Exited` after `Close`, with every later command refused.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::agent::{
    Account, Agent, AgentError, Capabilities, EventSink, HistoryItem, SessionHandle, StartSpec,
    StoredSession,
};
use crate::command::SessionCommand;
use crate::event::{ModelOption, PermissionMode, SessionEvent, SessionInfo};
use crate::ids::SessionId;

/// One event of a scripted reply, due `after` seconds after the `Send`.
#[derive(Clone, Debug, PartialEq)]
pub struct FakeStep {
    pub after: f32,
    pub event: SessionEvent,
}

impl FakeStep {
    pub fn at(after: f32, event: SessionEvent) -> Self {
        Self { after, event }
    }
}

struct Pending {
    due: f32,
    session: SessionId,
    event: SessionEvent,
}

struct Inner {
    account: Account,
    sessions: Vec<StoredSession>,
    history: HashMap<SessionId, Vec<HistoryItem>>,
    now: f32,
    opened: usize,
    replies: VecDeque<Vec<FakeStep>>,
    /// Kept in insertion order; delivery picks the due ones by `due`, stably.
    pending: Vec<Pending>,
    sinks: HashMap<SessionId, Arc<Mutex<EventSink>>>,
    sent: Vec<(SessionId, SessionCommand)>,
    /// Sessions that were sent `Close`.
    closed: HashSet<SessionId>,
}

impl Inner {
    fn schedule(&mut self, delay: f32, session: &SessionId, event: SessionEvent) {
        self.pending.push(Pending {
            due: self.now + delay,
            session: session.clone(),
            event,
        });
    }
}

/// Clones share state, so a test keeps one clone and hands another to the app.
#[derive(Clone)]
pub struct FakeAgent {
    inner: Arc<Mutex<Inner>>,
}

impl Default for FakeAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeAgent {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                account: Account::SignedIn {
                    name: "Test User".into(),
                    plan: Some("Max plan".into()),
                    full_login: true,
                },
                sessions: Vec::new(),
                history: HashMap::new(),
                now: 0.0,
                opened: 0,
                replies: VecDeque::new(),
                pending: Vec::new(),
                sinks: HashMap::new(),
                sent: Vec::new(),
                closed: HashSet::new(),
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_account(&self, account: Account) {
        self.lock().account = account;
    }

    pub fn set_sessions(&self, sessions: Vec<StoredSession>) {
        self.lock().sessions = sessions;
    }

    pub fn set_history(&self, id: &SessionId, items: Vec<HistoryItem>) {
        self.lock().history.insert(id.clone(), items);
    }

    /// Queue one scripted reply; each `Send` consumes the oldest.
    pub fn reply(&self, steps: Vec<FakeStep>) {
        self.lock().replies.push_back(steps);
    }

    /// Deliver `event` to session `id` on the next `advance`.
    pub fn emit(&self, id: &SessionId, event: SessionEvent) {
        self.lock().schedule(0.0, id, event);
    }

    /// Move the virtual clock forward and deliver every due event, in order of
    /// due time (insertion order among equals). The fake's lock is not held
    /// while a sink runs, so a sink may call `emit`, `reply`, `sent`, a handle's
    /// `send` and `open`, but not `advance`.
    pub fn advance(&self, seconds: f32) {
        let due = {
            let mut inner = self.lock();
            inner.now += seconds;
            let now = inner.now;
            let (mut due, later): (Vec<_>, Vec<_>) =
                inner.pending.drain(..).partition(|p| p.due <= now);
            inner.pending = later;
            due.sort_by(|a, b| a.due.total_cmp(&b.due));
            due.into_iter()
                .filter_map(|p| inner.sinks.get(&p.session).cloned().map(|s| (s, p.event)))
                .collect::<Vec<_>>()
        };
        for (sink, event) in due {
            let sink = sink.lock().unwrap_or_else(|e| e.into_inner());
            (*sink)(event);
        }
    }

    /// Every command any handle received, with the session it was sent to.
    pub fn sent(&self) -> Vec<(SessionId, SessionCommand)> {
        self.lock().sent.clone()
    }
}

impl Agent for FakeAgent {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            remote_access: true,
            plan_mode: true,
            usage_limits: true,
            images: true,
        }
    }

    fn account(&self) -> Account {
        self.lock().account.clone()
    }

    fn list_sessions(
        &self,
        _modified_since_unix: Option<i64>,
    ) -> Result<Vec<StoredSession>, AgentError> {
        Ok(self.lock().sessions.clone())
    }

    fn read_session(&self, id: &SessionId) -> Result<Vec<HistoryItem>, AgentError> {
        self.lock()
            .history
            .get(id)
            .cloned()
            .ok_or_else(|| AgentError::Protocol(format!("fake: no history for {}", id.0)))
    }

    fn open(&self, spec: StartSpec, sink: EventSink) -> Result<Box<dyn SessionHandle>, AgentError> {
        let mut inner = self.lock();
        let id = match spec.resume {
            Some(id) if !spec.fork => id,
            _ => {
                inner.opened += 1;
                SessionId(format!("fake-{}", inner.opened))
            }
        };
        inner.sinks.insert(id.clone(), Arc::new(Mutex::new(sink)));
        let models = fake_models();
        let asked = spec.model.as_deref().unwrap_or("default");
        let model = models
            .iter()
            .find(|option| option.id == asked)
            .map_or_else(|| asked.to_string(), |option| option.resolved.clone());
        let info = SessionInfo {
            id: id.clone(),
            folder: spec.folder,
            model,
            permission_mode: spec.permission_mode.unwrap_or(PermissionMode::Ask),
            agent_version: "fake".into(),
            models,
        };
        inner.schedule(0.0, &id, SessionEvent::Started(info));
        Ok(Box::new(FakeHandle {
            id,
            agent: self.clone(),
        }))
    }
}

/// The models every fake session offers.
fn fake_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            id: "default".into(),
            name: "Default".into(),
            resolved: "fake-model".into(),
        },
        ModelOption {
            id: "fast".into(),
            name: "Fast".into(),
            resolved: "fake-model-fast".into(),
        },
    ]
}

struct FakeHandle {
    id: SessionId,
    agent: FakeAgent,
}

impl SessionHandle for FakeHandle {
    /// After `Close` every command, another `Close` too, is refused and not
    /// recorded. `Close` drops this session's events that are not yet due, as
    /// the real adapter's exit cuts a turn short, and schedules `Exited`.
    fn send(&self, command: SessionCommand) -> Result<(), AgentError> {
        let mut inner = self.agent.lock();
        if inner.closed.contains(&self.id) {
            return Err(AgentError::Closed);
        }
        if matches!(command, SessionCommand::Close) {
            let now = inner.now;
            let id = &self.id;
            inner.pending.retain(|p| p.session != *id || p.due <= now);
            inner.closed.insert(self.id.clone());
            inner.schedule(
                0.0,
                &self.id,
                SessionEvent::Exited {
                    clean: true,
                    detail: String::new(),
                },
            );
        }
        if matches!(command, SessionCommand::Send { .. }) {
            match inner.replies.pop_front() {
                Some(steps) => {
                    for step in steps {
                        inner.schedule(step.after, &self.id, step.event);
                    }
                }
                None => inner.schedule(
                    0.0,
                    &self.id,
                    SessionEvent::TurnFailed {
                        reason: "fake: no scripted reply".into(),
                    },
                ),
            }
        }
        inner.sent.push((self.id.clone(), command));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::TurnSummary;

    fn collect() -> (Arc<Mutex<Vec<SessionEvent>>>, EventSink) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        (seen, Box::new(move |e| s.lock().unwrap().push(e)))
    }

    #[test]
    fn a_scripted_reply_plays_on_the_virtual_clock() {
        let fake = FakeAgent::new();
        fake.reply(vec![
            FakeStep::at(0.0, SessionEvent::TurnStarted),
            FakeStep::at(0.5, SessionEvent::TextDelta("Hi".into())),
            FakeStep::at(
                1.0,
                SessionEvent::TurnFinished(TurnSummary {
                    cost_usd: 0.01,
                    context_used: None,
                    duration_ms: 1000,
                    interrupted: false,
                }),
            ),
        ]);
        let (seen, sink) = collect();
        let handle = fake.open(StartSpec::new("/tmp/p"), sink).unwrap();
        assert!(
            seen.lock().unwrap().is_empty(),
            "nothing is delivered inside open"
        );
        fake.advance(0.0);
        match &seen.lock().unwrap()[0] {
            SessionEvent::Started(info) => assert_eq!(info.id, SessionId("fake-1".into())),
            other => panic!("expected Started, got {other:?}"),
        }
        handle
            .send(SessionCommand::Send {
                text: "hello".into(),
                attachments: vec![],
            })
            .unwrap();
        fake.advance(0.6);
        assert_eq!(seen.lock().unwrap().len(), 3); // Started, TurnStarted, TextDelta
        fake.advance(0.5);
        assert_eq!(seen.lock().unwrap().len(), 4);
        assert_eq!(fake.sent().len(), 1);
    }

    #[test]
    fn a_send_with_nothing_scripted_fails_the_turn() {
        let fake = FakeAgent::new();
        let (seen, sink) = collect();
        let handle = fake.open(StartSpec::new("/tmp/p"), sink).unwrap();
        handle
            .send(SessionCommand::Send {
                text: "x".into(),
                attachments: vec![],
            })
            .unwrap();
        fake.advance(0.0);
        assert_eq!(
            seen.lock().unwrap().last().unwrap(),
            &SessionEvent::TurnFailed {
                reason: "fake: no scripted reply".into()
            }
        );
    }

    #[test]
    fn stored_sessions_history_and_account_are_what_the_test_set() {
        let fake = FakeAgent::new();
        let id = SessionId("s1".into());
        fake.set_sessions(vec![StoredSession {
            id: id.clone(),
            folder: "/p".into(),
            title: "T".into(),
            last_active_unix: 5,
        }]);
        fake.set_history(&id, vec![HistoryItem::AssistantText("old".into())]);
        fake.set_account(Account::SignedOut);
        assert_eq!(fake.list_sessions(None).unwrap().len(), 1);
        assert_eq!(
            fake.read_session(&id).unwrap(),
            vec![HistoryItem::AssistantText("old".into())]
        );
        assert_eq!(fake.account(), Account::SignedOut);
        assert!(matches!(
            fake.read_session(&SessionId("nope".into())),
            Err(AgentError::Protocol(_))
        ));
    }

    fn started(seen: &Mutex<Vec<SessionEvent>>) -> SessionInfo {
        match seen.lock().unwrap().first() {
            Some(SessionEvent::Started(info)) => info.clone(),
            other => panic!("expected Started first, got {other:?}"),
        }
    }

    #[test]
    fn started_lists_the_models_and_resolves_the_one_asked_for() {
        let fake = FakeAgent::new();
        let (plain, sink) = collect();
        fake.open(StartSpec::new("/p"), sink).unwrap();
        let (asked, sink) = collect();
        let mut spec = StartSpec::new("/p");
        spec.model = Some("fast".into());
        fake.open(spec, sink).unwrap();
        fake.advance(0.0);
        let info = started(&plain);
        assert_eq!(info.models.len(), 2);
        assert_eq!(info.models[0].id, "default");
        assert_eq!(info.model, info.models[0].resolved);
        assert_eq!(started(&asked).model, info.models[1].resolved);
    }

    #[test]
    fn a_fork_gets_a_new_id_and_a_plain_resume_keeps_its_own() {
        let fake = FakeAgent::new();
        let old = SessionId("old".into());
        let (resumed, sink) = collect();
        let mut spec = StartSpec::new("/p");
        spec.resume = Some(old.clone());
        fake.open(spec.clone(), sink).unwrap();
        let (forked, sink) = collect();
        spec.fork = true;
        fake.open(spec, sink).unwrap();
        fake.advance(0.0);
        assert_eq!(started(&resumed).id, old);
        assert_eq!(started(&forked).id, SessionId("fake-1".into()));
    }

    #[test]
    fn close_ends_the_session_and_refuses_every_later_command() {
        let fake = FakeAgent::new();
        fake.reply(vec![
            FakeStep::at(0.0, SessionEvent::TurnStarted),
            FakeStep::at(5.0, SessionEvent::TextDelta("late".into())),
        ]);
        let (seen, sink) = collect();
        let handle = fake.open(StartSpec::new("/p"), sink).unwrap();
        handle
            .send(SessionCommand::Send {
                text: "x".into(),
                attachments: vec![],
            })
            .unwrap();
        handle.send(SessionCommand::Close).unwrap();
        assert_eq!(seen.lock().unwrap().len(), 0, "delivered by advance only");
        fake.advance(0.0);
        let exited = SessionEvent::Exited {
            clean: true,
            detail: String::new(),
        };
        assert_eq!(seen.lock().unwrap().last(), Some(&exited));
        assert_eq!(seen.lock().unwrap().len(), 3); // Started, TurnStarted, Exited
        fake.advance(10.0);
        assert_eq!(seen.lock().unwrap().len(), 3, "nothing after Exited");
        for command in [SessionCommand::Stop, SessionCommand::Close] {
            assert_eq!(handle.send(command), Err(AgentError::Closed));
        }
        assert_eq!(fake.sent().len(), 2, "refused commands are not recorded");
    }

    #[test]
    fn emitted_events_reach_the_right_session_on_the_next_advance() {
        let fake = FakeAgent::new();
        let (a, sink_a) = collect();
        let (b, sink_b) = collect();
        fake.open(StartSpec::new("/a"), sink_a).unwrap();
        fake.open(StartSpec::new("/b"), sink_b).unwrap();
        fake.advance(0.0);
        fake.emit(
            &SessionId("fake-2".into()),
            SessionEvent::TitleChanged("B".into()),
        );
        fake.advance(0.0);
        assert_eq!(a.lock().unwrap().len(), 1);
        assert_eq!(b.lock().unwrap().len(), 2);
    }
}
