//! A scripted `Agent` for tests. Time is virtual: nothing is delivered until
//! the test calls `advance`, and then in order of due time.
//!
//! It keeps the contract the real adapter keeps: `Started` (with a fixed list
//! of two made-up models) first, a new id for a fork and the old one for a
//! plain resume, and `Exited` after `Close`, with every later command refused.
//! Like the real adapter it also refuses an `Answer` whose request is not
//! waiting, and accepts a second `Close` until the first one's `Exited` has
//! been delivered. A `Send` plays the next reply queued with `reply`, a
//! `SuggestTitle` the next answer queued with `title_reply`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::agent::{
    Account, Agent, AgentError, Capabilities, EventSink, SessionHandle, StartSpec, StoredHistory,
    StoredSession,
};
use crate::command::SessionCommand;
use crate::event::{ModelOption, PermissionMode, SessionEvent, SessionInfo};
use crate::ids::{RequestId, SessionId};

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
    /// What `read_session` gives for each id: a history, or the detail of an
    /// `Io` error.
    history: HashMap<SessionId, Result<StoredHistory, String>>,
    /// Times set with `set_last_active`; they win over the stored sessions'.
    last_active: HashMap<SessionId, Option<i64>>,
    now: f32,
    opened: usize,
    replies: VecDeque<Vec<FakeStep>>,
    /// Scripted answers to `SuggestTitle`, oldest first.
    titles: VecDeque<Vec<FakeStep>>,
    /// Kept in insertion order; delivery picks the due ones by `due`, stably.
    pending: Vec<Pending>,
    sinks: HashMap<SessionId, Arc<Mutex<EventSink>>>,
    sent: Vec<(SessionId, SessionCommand)>,
    /// Sessions that were sent `Close` and whose `Exited` is not delivered yet.
    /// They accept another `Close` and refuse everything else.
    closing: HashSet<SessionId>,
    /// Sessions whose `Exited` was delivered, or that died while opening. They
    /// refuse every command until an `open` resumes the id.
    ended: HashSet<SessionId>,
    /// Requests delivered to the user and not yet answered or withdrawn.
    waiting: HashMap<SessionId, HashSet<RequestId>>,
    /// The detail the next `open` dies with, if a test asked for that.
    fail_next_open: Option<String>,
}

impl Inner {
    fn schedule(&mut self, delay: f32, session: &SessionId, event: SessionEvent) {
        self.pending.push(Pending {
            due: self.now + delay,
            session: session.clone(),
            event,
        });
    }

    /// Track what the user can still be asked, and whether the session lives,
    /// as its events are delivered.
    fn record_delivery(&mut self, session: &SessionId, event: &SessionEvent) {
        match event {
            SessionEvent::NeedsUser(request) => {
                self.waiting
                    .entry(session.clone())
                    .or_default()
                    .insert(request.id.clone());
            }
            SessionEvent::RequestWithdrawn { id } => {
                if let Some(ids) = self.waiting.get_mut(session) {
                    ids.remove(id);
                }
            }
            SessionEvent::Exited { .. } => {
                self.waiting.remove(session);
                self.closing.remove(session);
                self.ended.insert(session.clone());
            }
            _ => {}
        }
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
                last_active: HashMap::new(),
                now: 0.0,
                opened: 0,
                replies: VecDeque::new(),
                titles: VecDeque::new(),
                pending: Vec::new(),
                sinks: HashMap::new(),
                sent: Vec::new(),
                closing: HashSet::new(),
                ended: HashSet::new(),
                waiting: HashMap::new(),
                fail_next_open: None,
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

    /// What `read_session` gives for `id`: the end of `history`, cut to the
    /// limit asked for.
    pub fn set_history(&self, id: &SessionId, history: StoredHistory) {
        self.lock().history.insert(id.clone(), Ok(history));
    }

    /// Make `read_session` for `id` fail with `AgentError::Io(detail)`, until
    /// `set_history` gives it a history again.
    pub fn fail_history(&self, id: &SessionId, detail: &str) {
        self.lock()
            .history
            .insert(id.clone(), Err(detail.to_string()));
    }

    /// What `last_active` gives for `id`, in place of the `last_active_unix`
    /// of the session given to `set_sessions`.
    pub fn set_last_active(&self, id: &SessionId, unix: Option<i64>) {
        self.lock().last_active.insert(id.clone(), unix);
    }

    /// Queue one scripted reply; each `Send` consumes the oldest.
    pub fn reply(&self, steps: Vec<FakeStep>) {
        self.lock().replies.push_back(steps);
    }

    /// Queue the answer to one `SuggestTitle`, as `reply` does for a `Send`: each
    /// `SuggestTitle` consumes the oldest, its steps due `after` seconds after it. With nothing
    /// queued a `SuggestTitle` gets no answer, as from an agent that has no title to offer.
    pub fn title_reply(&self, steps: Vec<FakeStep>) {
        self.lock().titles.push_back(steps);
    }

    /// Make the next `open`, and only that one, behave like a program that
    /// exits before it is ready (a session id that cannot be resumed): `open`
    /// returns `Ok`, the only event is `Exited { clean: false, detail }`, and
    /// the handle refuses every command with `Closed`.
    pub fn fail_next_open(&self, detail: &str) {
        self.lock().fail_next_open = Some(detail.to_string());
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
            let mut delivered = Vec::new();
            for p in due {
                let Some(sink) = inner.sinks.get(&p.session).cloned() else {
                    continue;
                };
                inner.record_delivery(&p.session, &p.event);
                delivered.push((sink, p.event));
            }
            delivered
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

    fn read_session(&self, id: &SessionId, max_items: usize) -> Result<StoredHistory, AgentError> {
        let mut history = match self.lock().history.get(id) {
            Some(Ok(history)) => history.clone(),
            Some(Err(detail)) => return Err(AgentError::Io(detail.clone())),
            None => {
                return Err(AgentError::Protocol(format!(
                    "fake: no history for {}",
                    id.0
                )));
            }
        };
        let cut = history.items.len().saturating_sub(max_items);
        if cut > 0 {
            history.items.drain(..cut);
            history.earlier_left_out = true;
        }
        Ok(history)
    }

    fn last_active(&self, id: &SessionId) -> Result<Option<i64>, AgentError> {
        let inner = self.lock();
        if let Some(unix) = inner.last_active.get(id) {
            return Ok(*unix);
        }
        Ok(inner
            .sessions
            .iter()
            .find(|session| &session.id == id)
            .map(|session| session.last_active_unix))
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
        // A resumed id gets a new program: nothing of the old one is left.
        if inner.closing.remove(&id) {
            inner.pending.retain(|p| p.session != id);
        }
        inner.ended.remove(&id);
        inner.waiting.remove(&id);
        inner.sinks.insert(id.clone(), Arc::new(Mutex::new(sink)));
        if let Some(detail) = inner.fail_next_open.take() {
            inner.ended.insert(id.clone());
            inner.schedule(
                0.0,
                &id,
                SessionEvent::Exited {
                    clean: false,
                    detail,
                },
            );
            return Ok(Box::new(FakeHandle {
                id,
                agent: self.clone(),
            }));
        }
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

/// The models every fake session offers. The first, like the real default, has a name that
/// does not say which model it is, and supports the `Auto` mode; the second does not.
fn fake_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            id: "default".into(),
            name: "Default".into(),
            resolved: "fake-model".into(),
            stands_for: Some("Fake 1".into()),
            auto_mode: true,
        },
        ModelOption {
            id: "fast".into(),
            name: "Fast".into(),
            resolved: "fake-model-fast".into(),
            stands_for: None,
            auto_mode: false,
        },
    ]
}

struct FakeHandle {
    id: SessionId,
    agent: FakeAgent,
}

impl SessionHandle for FakeHandle {
    /// Once `Exited` is delivered every command is refused with `Closed` and
    /// not recorded. Between a `Close` and its `Exited` another `Close` is
    /// accepted and ignored, and everything else is refused. `Close` drops this
    /// session's events that are not yet due, as the real adapter's exit cuts a
    /// turn short, and schedules `Exited`. An `Answer` is accepted only while
    /// its request is waiting, and then it is no longer waiting.
    fn send(&self, command: SessionCommand) -> Result<(), AgentError> {
        let mut inner = self.agent.lock();
        if inner.ended.contains(&self.id) {
            return Err(AgentError::Closed);
        }
        if inner.closing.contains(&self.id) {
            return match command {
                SessionCommand::Close => Ok(()),
                _ => Err(AgentError::Closed),
            };
        }
        if let SessionCommand::Answer { id, .. } = &command {
            let waiting = inner
                .waiting
                .get_mut(&self.id)
                .is_some_and(|ids| ids.remove(id));
            if !waiting {
                return Err(AgentError::Protocol(format!(
                    "fake: no request {} is waiting",
                    id.0
                )));
            }
        }
        if matches!(command, SessionCommand::Close) {
            let now = inner.now;
            let id = &self.id;
            inner.pending.retain(|p| p.session != *id || p.due <= now);
            inner.closing.insert(self.id.clone());
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
        if matches!(command, SessionCommand::SuggestTitle { .. })
            && let Some(steps) = inner.titles.pop_front()
        {
            for step in steps {
                inner.schedule(step.after, &self.id, step.event);
            }
        }
        inner.sent.push((self.id.clone(), command));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::HistoryItem;
    use crate::command::Answer;
    use crate::event::{CommandKind, TurnSummary, UserRequest, UserRequestKind};
    use crate::ids::RequestId;

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
        fake.set_history(
            &id,
            StoredHistory {
                items: vec![HistoryItem::AssistantText("old".into())],
                ..StoredHistory::default()
            },
        );
        fake.set_account(Account::SignedOut);
        assert_eq!(fake.list_sessions(None).unwrap().len(), 1);
        assert_eq!(
            fake.read_session(&id, usize::MAX).unwrap().items,
            vec![HistoryItem::AssistantText("old".into())]
        );
        assert_eq!(fake.account(), Account::SignedOut);
        assert!(matches!(
            fake.read_session(&SessionId("nope".into()), usize::MAX),
            Err(AgentError::Protocol(_))
        ));
    }

    #[test]
    fn the_fake_returns_the_end_of_a_long_history() {
        let fake = FakeAgent::new();
        let id = SessionId("s1".into());
        let text = |n: usize| HistoryItem::AssistantText(n.to_string());
        fake.set_history(
            &id,
            StoredHistory {
                items: (0..5).map(text).collect(),
                earlier_left_out: false,
                model: Some("fake-model-1".into()),
                permission_mode: Some(PermissionMode::Plan),
            },
        );

        let end = fake.read_session(&id, 2).unwrap();
        assert_eq!(end.items, vec![text(3), text(4)]);
        assert!(end.earlier_left_out);
        assert_eq!(end.model.as_deref(), Some("fake-model-1"));
        assert_eq!(end.permission_mode, Some(PermissionMode::Plan));

        let all = fake.read_session(&id, 10).unwrap();
        assert_eq!(all.items, (0..5).map(text).collect::<Vec<_>>());
        assert!(!all.earlier_left_out);
        assert_eq!(all.model.as_deref(), Some("fake-model-1"));
        assert_eq!(all.permission_mode, Some(PermissionMode::Plan));

        let cut = SessionId("s2".into());
        fake.set_history(
            &cut,
            StoredHistory {
                items: vec![text(9)],
                earlier_left_out: true,
                ..StoredHistory::default()
            },
        );
        for limit in [0, 1, 10, usize::MAX] {
            assert!(fake.read_session(&cut, limit).unwrap().earlier_left_out);
        }
    }

    #[test]
    fn a_history_that_cannot_be_read_is_an_error() {
        let fake = FakeAgent::new();
        let id = SessionId("s1".into());
        fake.fail_history(&id, "gone");
        assert_eq!(
            fake.read_session(&id, 10),
            Err(AgentError::Io("gone".into()))
        );
    }

    #[test]
    fn the_fake_knows_when_a_session_was_last_active() {
        let fake = FakeAgent::new();
        let id = SessionId("s1".into());
        fake.set_sessions(vec![StoredSession {
            id: id.clone(),
            folder: "/home/alex/p".into(),
            title: "T".into(),
            last_active_unix: 500,
        }]);
        assert_eq!(fake.last_active(&id), Ok(Some(500)));
        fake.set_last_active(&id, Some(900));
        assert_eq!(fake.last_active(&id), Ok(Some(900)));
        assert_eq!(fake.last_active(&SessionId("nope".into())), Ok(None));
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

    #[test]
    fn a_session_that_dies_while_opening_gives_only_exited() {
        let fake = FakeAgent::new();
        fake.fail_next_open("no conversation found with that id");
        let (seen, sink) = collect();
        let handle = fake.open(StartSpec::new("/p"), sink).unwrap();
        fake.advance(0.0);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![SessionEvent::Exited {
                clean: false,
                detail: "no conversation found with that id".into()
            }]
        );
        assert_eq!(handle.send(SessionCommand::Stop), Err(AgentError::Closed));
        // Only the next open fails.
        let (seen, sink) = collect();
        fake.open(StartSpec::new("/p"), sink).unwrap();
        fake.advance(0.0);
        assert!(matches!(seen.lock().unwrap()[0], SessionEvent::Started(_)));
    }

    #[test]
    fn a_usage_refresh_is_recorded_and_emits_nothing() {
        let fake = FakeAgent::new();
        let (seen, sink) = collect();
        let handle = fake.open(StartSpec::new("/p"), sink).unwrap();
        fake.advance(0.0);
        let before = seen.lock().unwrap().len();
        assert_eq!(handle.send(SessionCommand::RefreshUsage), Ok(()));
        fake.advance(10.0);
        assert_eq!(seen.lock().unwrap().len(), before);
        assert!(
            fake.sent()
                .iter()
                .any(|(_, c)| *c == SessionCommand::RefreshUsage)
        );
    }

    #[test]
    fn a_title_suggestion_gets_the_answer_scripted_for_it_or_none() {
        let fake = FakeAgent::new();
        let (seen, sink) = collect();
        let handle = fake.open(StartSpec::new("/p"), sink).unwrap();
        fake.advance(0.0);
        let suggest = || SessionCommand::SuggestTitle {
            from: "Fix the parser".into(),
        };
        // Nothing scripted: no answer at all, as when the agent offers no title.
        assert_eq!(handle.send(suggest()), Ok(()));
        fake.advance(10.0);
        assert_eq!(seen.lock().unwrap().len(), 1, "only Started");
        fake.title_reply(vec![FakeStep::at(
            0.5,
            SessionEvent::TitleChanged("Fixing the parser".into()),
        )]);
        fake.title_reply(vec![FakeStep::at(
            0.0,
            SessionEvent::CommandFailed {
                what: CommandKind::SuggestTitle,
                reason: "no".into(),
            },
        )]);
        assert_eq!(handle.send(suggest()), Ok(()));
        fake.advance(0.4);
        assert_eq!(seen.lock().unwrap().len(), 1, "due half a second after");
        fake.advance(0.1);
        assert_eq!(
            seen.lock().unwrap().last(),
            Some(&SessionEvent::TitleChanged("Fixing the parser".into()))
        );
        assert_eq!(handle.send(suggest()), Ok(()));
        fake.advance(0.0);
        assert!(matches!(
            seen.lock().unwrap().last(),
            Some(SessionEvent::CommandFailed {
                what: CommandKind::SuggestTitle,
                ..
            })
        ));
        let asked = fake
            .sent()
            .iter()
            .filter(|(_, c)| matches!(c, SessionCommand::SuggestTitle { .. }))
            .count();
        assert_eq!(asked, 3);
    }

    #[test]
    fn a_second_close_is_accepted_until_the_exit_is_delivered() {
        let fake = FakeAgent::new();
        let (_seen, sink) = collect();
        let handle = fake.open(StartSpec::new("/p"), sink).unwrap();
        fake.advance(0.0);
        assert_eq!(handle.send(SessionCommand::Close), Ok(()));
        assert_eq!(handle.send(SessionCommand::Close), Ok(()));
        assert_eq!(handle.send(SessionCommand::Stop), Err(AgentError::Closed));
        fake.advance(0.0);
        assert_eq!(handle.send(SessionCommand::Close), Err(AgentError::Closed));
        let closes = fake
            .sent()
            .iter()
            .filter(|(_, c)| *c == SessionCommand::Close)
            .count();
        assert_eq!(closes, 1);
    }

    #[test]
    fn an_answer_is_refused_unless_its_request_is_waiting() {
        let fake = FakeAgent::new();
        let (seen, sink) = collect();
        let handle = fake.open(StartSpec::new("/p"), sink).unwrap();
        fake.advance(0.0);
        let id = started(&seen).id;
        let answer = |n: &str| SessionCommand::Answer {
            id: RequestId(n.into()),
            answer: Answer::Allow { always: false },
        };
        assert!(matches!(
            handle.send(answer("r1")),
            Err(AgentError::Protocol(_))
        ));
        let request = UserRequest {
            id: RequestId("r1".into()),
            kind: UserRequestKind::PlanApproval {
                plan_markdown: "p".into(),
            },
        };
        fake.emit(&id, SessionEvent::NeedsUser(request));
        fake.advance(0.0);
        assert_eq!(handle.send(answer("r1")), Ok(()));
        assert!(matches!(
            handle.send(answer("r1")),
            Err(AgentError::Protocol(_))
        ));
        // A withdrawn request is no longer waiting either.
        let again = UserRequest {
            id: RequestId("r2".into()),
            kind: UserRequestKind::PlanApproval {
                plan_markdown: "p".into(),
            },
        };
        fake.emit(&id, SessionEvent::NeedsUser(again));
        fake.emit(
            &id,
            SessionEvent::RequestWithdrawn {
                id: RequestId("r2".into()),
            },
        );
        fake.advance(0.0);
        assert!(matches!(
            handle.send(answer("r2")),
            Err(AgentError::Protocol(_))
        ));
    }

    #[test]
    fn a_resumed_session_works_again_after_it_ended() {
        let fake = FakeAgent::new();
        let old = SessionId("old".into());
        let waiting = |n: &str| {
            SessionEvent::NeedsUser(UserRequest {
                id: RequestId(n.into()),
                kind: UserRequestKind::PlanApproval {
                    plan_markdown: "p".into(),
                },
            })
        };
        let answer = |n: &str| SessionCommand::Answer {
            id: RequestId(n.into()),
            answer: Answer::Allow { always: false },
        };
        let resume = || {
            let mut spec = StartSpec::new("/p");
            spec.resume = Some(old.clone());
            spec
        };
        // Ended by Close, with a request still waiting.
        let (_seen, sink) = collect();
        let handle = fake.open(resume(), sink).unwrap();
        fake.emit(&old, waiting("r1"));
        fake.advance(0.0);
        handle.send(SessionCommand::Close).unwrap();
        fake.advance(0.0);
        assert_eq!(handle.send(SessionCommand::Stop), Err(AgentError::Closed));
        let (seen, sink) = collect();
        let handle = fake.open(resume(), sink).unwrap();
        fake.advance(0.0);
        assert_eq!(started(&seen).id, old);
        assert_eq!(handle.send(SessionCommand::Stop), Ok(()));
        assert!(matches!(
            handle.send(answer("r1")),
            Err(AgentError::Protocol(_))
        ));
        // Ended by an Exited delivered through emit, with a request waiting.
        fake.emit(&old, waiting("r2"));
        fake.emit(
            &old,
            SessionEvent::Exited {
                clean: false,
                detail: "gone".into(),
            },
        );
        fake.advance(0.0);
        assert_eq!(handle.send(SessionCommand::Stop), Err(AgentError::Closed));
        let (seen, sink) = collect();
        let handle = fake.open(resume(), sink).unwrap();
        fake.advance(0.0);
        assert_eq!(started(&seen).id, old);
        assert_eq!(handle.send(SessionCommand::Stop), Ok(()));
        assert!(matches!(
            handle.send(answer("r2")),
            Err(AgentError::Protocol(_))
        ));
    }
}
