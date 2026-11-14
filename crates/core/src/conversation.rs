//! The list of things a session has said, built from its events.
//!
//! Three promises the view relies on: an item's id is never reused and never
//! changes, [`Conversation::revision`] grows whenever anything a view shows
//! has changed and stays the same otherwise, and an item's [`Item::changed`]
//! is the revision of its own last change.

use mr_agent::{RequestId, SessionEvent, ToolCallId, ToolOutcome, UserRequest};

use crate::tools::{ToolCall, ToolGroup};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId(pub u64);

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: ItemId,
    /// The conversation's revision when this item was pushed or last changed.
    pub changed: u64,
    pub body: ItemBody,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemBody {
    User {
        text: String,
    },
    Reply {
        markdown: String,
        streaming: bool,
    },
    /// `seconds` is `None` while the thinking goes on.
    Thinking {
        text: String,
        started: f64,
        seconds: Option<f64>,
    },
    Tools(ToolGroup),
    Request {
        request: UserRequest,
        state: RequestState,
    },
    Notice {
        kind: NoticeKind,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum RequestState {
    Waiting,
    Answered(String),
    Withdrawn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Stopped,
    TurnFailed,
    Ended,
    CommandFailed,
}

const UNEXPECTED_END: &str = "The session ended unexpectedly.";

/// The outcome given to a call still running when its turn ends.
pub const DID_NOT_FINISH: &str = "Did not finish";

/// "Thinking…" while it goes on, then "Thought for 4 s" or "Thought for 1 min 5 s".
///
/// `seconds` is rounded to whole seconds, and a thought of less than a second still took one.
pub fn thinking_label(seconds: Option<f64>) -> String {
    let Some(seconds) = seconds else {
        return "Thinking…".into();
    };
    let whole = (seconds.round() as u64).max(1);
    let (minutes, rest) = (whole / 60, whole % 60);
    match (minutes, rest) {
        (0, s) => format!("Thought for {s} s"),
        (m, 0) => format!("Thought for {m} min"),
        (m, s) => format!("Thought for {m} min {s} s"),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conversation {
    items: Vec<Item>,
    revision: u64,
    next_id: u64,
    /// Positions of the streaming replies and open thinking, so that closing
    /// them never scans the whole conversation.
    open: Vec<usize>,
    /// Positions of the requests still waiting for the user.
    waiting: Vec<usize>,
    /// Positions of the tool groups a call was started in this turn, so that ending the turn
    /// closes the calls left running without scanning the whole conversation.
    calls: Vec<usize>,
}

impl Conversation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Grows by at least one whenever anything a view shows has changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn push_user(&mut self, text: &str) {
        self.push(ItemBody::User { text: text.into() });
    }

    pub fn push_notice(&mut self, kind: NoticeKind, text: &str) {
        self.push(ItemBody::Notice {
            kind,
            text: text.into(),
        });
    }

    /// `now` is seconds on any steady clock; it only times thinking.
    pub fn apply(&mut self, event: &SessionEvent, now: f64) {
        match event {
            SessionEvent::TextDelta(piece) => self.text_delta(piece, now),
            SessionEvent::ThinkingStarted => {
                self.close_open(now);
                self.push_thinking(String::new(), now);
            }
            SessionEvent::ThinkingDelta(piece) => self.thinking_delta(piece, now),
            SessionEvent::BlockFinished => self.finish_block(now),
            SessionEvent::ToolStarted { id, name, detail } => {
                self.close_open(now);
                self.tool_started(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    detail: detail.clone(),
                    outcome: None,
                });
            }
            SessionEvent::ToolFinished { id, outcome } => self.tool_finished(id, outcome),
            SessionEvent::NeedsUser(request) => {
                self.waiting.push(self.items.len());
                self.push(ItemBody::Request {
                    request: request.clone(),
                    state: RequestState::Waiting,
                });
            }
            SessionEvent::RequestWithdrawn { id } => {
                self.settle(id, RequestState::Withdrawn);
            }
            SessionEvent::TurnFinished(summary) => {
                self.close_open(now);
                self.close_calls();
                if summary.interrupted {
                    self.push_notice(NoticeKind::Stopped, "Stopped");
                }
            }
            SessionEvent::TurnFailed { reason } => {
                self.close_open(now);
                self.close_calls();
                self.push_notice(NoticeKind::TurnFailed, reason);
            }
            SessionEvent::Exited { clean, detail } => {
                self.close_open(now);
                self.close_calls();
                self.withdraw_all();
                if !clean {
                    let text = if detail.is_empty() {
                        UNEXPECTED_END
                    } else {
                        detail
                    };
                    self.push_notice(NoticeKind::Ended, text);
                }
            }
            SessionEvent::Started(_)
            | SessionEvent::TurnStarted
            | SessionEvent::UsageLimits(_)
            | SessionEvent::RemoteAccess(_)
            | SessionEvent::TitleChanged(_)
            | SessionEvent::ModelChanged(_)
            | SessionEvent::PermissionModeChanged(_)
            | SessionEvent::CommandFailed { .. } => {}
        }
    }

    /// Marks a waiting request answered, with `words` to show. False when it was not waiting.
    pub fn answered(&mut self, id: &RequestId, words: &str) -> bool {
        self.settle(id, RequestState::Answered(words.into()))
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    fn push(&mut self, body: ItemBody) {
        let id = ItemId(self.next_id);
        self.next_id += 1;
        self.revision += 1;
        self.items.push(Item {
            id,
            changed: self.revision,
            body,
        });
    }

    /// Records that the item at `at` changed: the revision grows and the item takes it.
    fn touch(&mut self, at: usize) {
        self.revision += 1;
        if let Some(item) = self.items.get_mut(at) {
            item.changed = self.revision;
        }
    }

    fn text_delta(&mut self, piece: &str, now: f64) {
        // An empty piece shows nothing, so it changes nothing.
        if piece.is_empty() {
            return;
        }
        if let Some(Item {
            body:
                ItemBody::Reply {
                    markdown,
                    streaming: true,
                },
            changed,
            ..
        }) = self.items.last_mut()
        {
            markdown.push_str(piece);
            self.revision += 1;
            *changed = self.revision;
            return;
        }
        self.close_open(now);
        self.open.push(self.items.len());
        self.push(ItemBody::Reply {
            markdown: piece.into(),
            streaming: true,
        });
    }

    fn push_thinking(&mut self, text: String, now: f64) {
        self.open.push(self.items.len());
        self.push(ItemBody::Thinking {
            text,
            started: now,
            seconds: None,
        });
    }

    fn thinking_delta(&mut self, piece: &str, now: f64) {
        if piece.is_empty() {
            return;
        }
        if let Some(Item {
            body:
                ItemBody::Thinking {
                    text,
                    seconds: None,
                    ..
                },
            changed,
            ..
        }) = self.items.last_mut()
        {
            text.push_str(piece);
            self.revision += 1;
            *changed = self.revision;
            return;
        }
        self.close_open(now);
        self.push_thinking(piece.into(), now);
    }

    fn finish_block(&mut self, now: f64) {
        let last = self.items.len().wrapping_sub(1);
        if self.close_at(last, now) {
            self.open.retain(|&at| at != last);
            self.touch(last);
        }
    }

    fn tool_started(&mut self, call: ToolCall) {
        let call = match self.items.last_mut() {
            Some(Item {
                body: ItemBody::Tools(group),
                changed,
                ..
            }) => match group.try_add(call) {
                Ok(()) => {
                    self.revision += 1;
                    *changed = self.revision;
                    let at = self.items.len() - 1;
                    if self.calls.last() != Some(&at) {
                        self.calls.push(at);
                    }
                    return;
                }
                Err(call) => call,
            },
            _ => call,
        };
        self.calls.push(self.items.len());
        self.push(ItemBody::Tools(ToolGroup::start(call)));
    }

    /// Ends the calls the turn left running: each takes the outcome "Did not finish". A
    /// `ToolFinished` that comes later still sets its real outcome.
    fn close_calls(&mut self) {
        for at in std::mem::take(&mut self.calls) {
            let Some(ItemBody::Tools(group)) = self.items.get_mut(at).map(|item| &mut item.body)
            else {
                continue;
            };
            let mut closed = false;
            for call in group.calls.iter_mut().filter(|call| call.outcome.is_none()) {
                call.outcome = Some(ToolOutcome::Failed {
                    output: DID_NOT_FINISH.into(),
                });
                closed = true;
            }
            if closed {
                self.touch(at);
            }
        }
    }

    fn tool_finished(&mut self, id: &ToolCallId, outcome: &ToolOutcome) {
        let found = self
            .items
            .iter_mut()
            .rev()
            .find_map(|item| match &mut item.body {
                ItemBody::Tools(group) => group
                    .calls
                    .iter_mut()
                    .rev()
                    .find(|call| call.id == *id)
                    .map(|call| (call, &mut item.changed)),
                _ => None,
            });
        if let Some((call, changed)) = found
            && call.outcome.as_ref() != Some(outcome)
        {
            call.outcome = Some(outcome.clone());
            self.revision += 1;
            *changed = self.revision;
        }
    }

    /// Moves the waiting request `id` to `state`. False when it was not waiting.
    fn settle(&mut self, id: &RequestId, state: RequestState) -> bool {
        let Some(slot) = self.waiting.iter().position(|&at| {
            matches!(&self.items[at].body, ItemBody::Request { request, .. } if request.id == *id)
        }) else {
            return false;
        };
        let at = self.waiting.remove(slot);
        if let ItemBody::Request { state: current, .. } = &mut self.items[at].body {
            *current = state;
        }
        self.touch(at);
        true
    }

    fn withdraw_all(&mut self) {
        for at in std::mem::take(&mut self.waiting) {
            if let ItemBody::Request { state, .. } = &mut self.items[at].body {
                *state = RequestState::Withdrawn;
            }
            self.touch(at);
        }
    }

    /// Stops every streaming reply and ends every open thinking.
    fn close_open(&mut self, now: f64) {
        for at in std::mem::take(&mut self.open) {
            if self.close_at(at, now) {
                self.touch(at);
            }
        }
    }

    /// Closes the item at `at` when it is a streaming reply or open thinking.
    /// Does not touch the revision, the item's stamp or `open`.
    fn close_at(&mut self, at: usize, now: f64) -> bool {
        match self.items.get_mut(at).map(|item| &mut item.body) {
            Some(ItemBody::Reply { streaming, .. }) if *streaming => {
                *streaming = false;
                true
            }
            Some(ItemBody::Thinking {
                started,
                seconds: seconds @ None,
                ..
            }) => {
                *seconds = Some((now - *started).max(0.0));
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_agent::{
        RequestId, SessionEvent, ToolCallId, ToolDetail, ToolOutcome, TurnSummary, UserRequest,
        UserRequestKind,
    };

    fn bodies(c: &Conversation) -> Vec<ItemBody> {
        c.items().iter().map(|i| i.body.clone()).collect()
    }
    fn text(s: &str) -> SessionEvent {
        SessionEvent::TextDelta(s.into())
    }
    fn play(c: &mut Conversation, events: &[SessionEvent]) {
        for e in events {
            c.apply(e, 0.0);
        }
    }
    fn read(id: &str) -> SessionEvent {
        SessionEvent::ToolStarted {
            id: ToolCallId(id.into()),
            name: "Read".into(),
            detail: ToolDetail::FileRead {
                path: format!("{id}.rs"),
            },
        }
    }
    fn plan(id: &str) -> SessionEvent {
        SessionEvent::NeedsUser(UserRequest {
            id: RequestId(id.into()),
            kind: UserRequestKind::PlanApproval {
                plan_markdown: "p".into(),
            },
        })
    }
    fn summary(interrupted: bool) -> TurnSummary {
        TurnSummary {
            cost_usd: 0.0,
            context_used: None,
            duration_ms: 1,
            interrupted,
        }
    }

    #[test]
    fn thinking_is_labelled_in_whole_seconds() {
        assert_eq!(thinking_label(None), "Thinking…");
        assert_eq!(thinking_label(Some(0.2)), "Thought for 1 s");
        assert_eq!(thinking_label(Some(3.5)), "Thought for 4 s");
        assert_eq!(thinking_label(Some(65.0)), "Thought for 1 min 5 s");
    }

    #[test]
    fn an_items_change_stamp_moves_only_when_it_changes() {
        let stamps = |c: &Conversation| c.items().iter().map(|i| i.changed).collect::<Vec<_>>();
        let mut c = Conversation::new();
        c.push_user("one");
        play(&mut c, &[text("a"), read("r1")]);
        let before = stamps(&c);
        // The read's outcome changes the tool group alone.
        c.apply(
            &SessionEvent::ToolFinished {
                id: ToolCallId("r1".into()),
                outcome: ToolOutcome::Ok { output: "x".into() },
            },
            0.0,
        );
        let after = stamps(&c);
        assert_eq!(after[..2], before[..2]);
        assert!(after[2] > before[2]);
        assert_eq!(after[2], c.revision());
        // A second read joins the group and changes it again; nothing else moves.
        play(&mut c, &[read("r2")]);
        let joined = stamps(&c);
        assert_eq!(joined[..2], before[..2]);
        assert!(joined[2] > after[2]);
        // What changes nothing leaves every stamp alone.
        play(&mut c, &[text(""), SessionEvent::TitleChanged("t".into())]);
        assert_eq!(stamps(&c), joined);
        // Text after the tools closes nothing open and starts a new item.
        play(&mut c, &[text("b"), text("c")]);
        let streamed = stamps(&c);
        assert_eq!(streamed[..3], joined[..]);
        assert_eq!(streamed[3], c.revision());
        // Finishing the turn closes the streaming reply and the read left running.
        play(&mut c, &[SessionEvent::TurnFinished(summary(false))]);
        let closed = stamps(&c);
        assert_eq!(closed[..2], joined[..2]);
        assert!(closed[2] > joined[2]);
        assert!(closed[3] > streamed[3]);
        // An outcome for a call in an earlier group stamps that group alone.
        c.apply(
            &SessionEvent::ToolFinished {
                id: ToolCallId("r2".into()),
                outcome: ToolOutcome::Ok { output: "y".into() },
            },
            0.0,
        );
        let late = stamps(&c);
        assert_eq!(late[..2], closed[..2]);
        assert!(late[2] > closed[2]);
        assert_eq!(late[2], c.revision());
        assert_eq!(late[3], closed[3]);
    }

    fn group(c: &Conversation, at: usize) -> &ToolGroup {
        match &c.items()[at].body {
            ItemBody::Tools(group) => group,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_call_left_running_when_the_turn_ends_did_not_finish() {
        let did_not_finish = Some(ToolOutcome::Failed {
            output: DID_NOT_FINISH.into(),
        });
        let endings = [
            SessionEvent::TurnFinished(summary(true)),
            SessionEvent::TurnFailed {
                reason: "rate limit reached".into(),
            },
            SessionEvent::Exited {
                clean: false,
                detail: "claude exited with status 1".into(),
            },
        ];
        for end in endings {
            let mut c = Conversation::new();
            c.push_user("go");
            play(&mut c, &[read("a"), read("b")]);
            c.apply(
                &SessionEvent::ToolFinished {
                    id: ToolCallId("a".into()),
                    outcome: ToolOutcome::Ok { output: "x".into() },
                },
                0.0,
            );
            let (user, tools) = (c.items()[0].changed, c.items()[1].changed);
            c.apply(&end, 0.0);
            let g = group(&c, 1);
            assert_eq!(
                g.calls[0].outcome,
                Some(ToolOutcome::Ok { output: "x".into() }),
                "{end:?}"
            );
            assert_eq!(g.calls[1].outcome, did_not_finish, "{end:?}");
            assert!(!g.running() && g.failed(), "{end:?}");
            assert!(c.items()[1].changed > tools, "{end:?}");
            assert_eq!(c.items()[0].changed, user, "{end:?}");
            // A real outcome arriving later still counts.
            c.apply(
                &SessionEvent::ToolFinished {
                    id: ToolCallId("b".into()),
                    outcome: ToolOutcome::Ok {
                        output: "late".into(),
                    },
                },
                0.0,
            );
            assert_eq!(
                group(&c, 1).calls[1].outcome,
                Some(ToolOutcome::Ok {
                    output: "late".into()
                })
            );
        }
    }

    #[test]
    fn a_turn_whose_calls_all_finished_changes_no_outcome() {
        let mut c = Conversation::new();
        play(
            &mut c,
            &[
                read("a"),
                SessionEvent::ToolFinished {
                    id: ToolCallId("a".into()),
                    outcome: ToolOutcome::Ok { output: "x".into() },
                },
            ],
        );
        let (before, revision) = (c.items().to_vec(), c.revision());
        play(&mut c, &[SessionEvent::TurnFinished(summary(false))]);
        assert_eq!(c.items(), &before[..]);
        assert_eq!(c.revision(), revision);
    }

    #[test]
    fn a_reply_streams_into_one_item() {
        let mut c = Conversation::new();
        c.push_user("**hi**");
        play(
            &mut c,
            &[
                SessionEvent::TurnStarted,
                text("Hel"),
                text("lo"),
                SessionEvent::BlockFinished,
            ],
        );
        assert_eq!(
            bodies(&c),
            vec![
                ItemBody::User {
                    text: "**hi**".into()
                },
                ItemBody::Reply {
                    markdown: "Hello".into(),
                    streaming: false
                },
            ]
        );
    }

    #[test]
    fn thinking_is_timed_by_the_clock_it_is_given() {
        let mut c = Conversation::new();
        c.apply(&SessionEvent::ThinkingStarted, 10.0);
        assert_eq!(
            bodies(&c),
            vec![ItemBody::Thinking {
                text: String::new(),
                started: 10.0,
                seconds: None
            }]
        );
        c.apply(&SessionEvent::BlockFinished, 13.5);
        assert_eq!(
            bodies(&c),
            vec![ItemBody::Thinking {
                text: String::new(),
                started: 10.0,
                seconds: Some(3.5)
            }]
        );
    }

    #[test]
    fn tool_calls_fold_and_text_after_them_starts_a_new_reply() {
        let mut c = Conversation::new();
        play(
            &mut c,
            &[
                text("Looking."),
                read("a"),
                read("b"),
                SessionEvent::ToolFinished {
                    id: ToolCallId("a".into()),
                    outcome: ToolOutcome::Ok { output: "x".into() },
                },
                text("Done."),
            ],
        );
        let b = bodies(&c);
        assert_eq!(b.len(), 3);
        assert_eq!(
            b[0],
            ItemBody::Reply {
                markdown: "Looking.".into(),
                streaming: false
            }
        );
        let ItemBody::Tools(group) = &b[1] else {
            panic!("{:?}", b[1])
        };
        assert_eq!(group.summary(), "Read 2 files");
        assert!(group.calls[0].outcome.is_some() && group.calls[1].outcome.is_none());
        assert_eq!(
            b[2],
            ItemBody::Reply {
                markdown: "Done.".into(),
                streaming: true
            }
        );
    }

    #[test]
    fn a_request_is_answered_once_or_withdrawn() {
        let mut c = Conversation::new();
        play(&mut c, &[plan("r1"), plan("r2")]);
        assert_eq!(c.waiting(), 2);
        assert!(c.answered(&RequestId("r1".into()), "Plan approved"));
        assert!(!c.answered(&RequestId("r1".into()), "again"));
        assert!(!c.answered(&RequestId("nope".into()), "x"));
        c.apply(
            &SessionEvent::RequestWithdrawn {
                id: RequestId("r2".into()),
            },
            0.0,
        );
        c.apply(
            &SessionEvent::RequestWithdrawn {
                id: RequestId("r1".into()),
            },
            0.0,
        );
        let states: Vec<_> = bodies(&c)
            .into_iter()
            .map(|b| match b {
                ItemBody::Request { state, .. } => state,
                o => panic!("{o:?}"),
            })
            .collect();
        assert_eq!(
            states,
            vec![
                RequestState::Answered("Plan approved".into()),
                RequestState::Withdrawn
            ]
        );
        assert_eq!(c.waiting(), 0);
    }

    #[test]
    fn a_dying_session_closes_everything_and_says_why() {
        let mut c = Conversation::new();
        c.apply(&SessionEvent::ThinkingStarted, 1.0);
        c.apply(&text("partial"), 2.0);
        c.apply(&plan("r1"), 2.0);
        c.apply(
            &SessionEvent::Exited {
                clean: false,
                detail: "killed".into(),
            },
            4.0,
        );
        assert_eq!(
            bodies(&c),
            vec![
                ItemBody::Thinking {
                    text: String::new(),
                    started: 1.0,
                    seconds: Some(1.0)
                },
                ItemBody::Reply {
                    markdown: "partial".into(),
                    streaming: false
                },
                ItemBody::Request {
                    request: UserRequest {
                        id: RequestId("r1".into()),
                        kind: UserRequestKind::PlanApproval {
                            plan_markdown: "p".into()
                        }
                    },
                    state: RequestState::Withdrawn,
                },
                ItemBody::Notice {
                    kind: NoticeKind::Ended,
                    text: "killed".into()
                },
            ]
        );
        let mut clean = Conversation::new();
        clean.apply(
            &SessionEvent::Exited {
                clean: true,
                detail: String::new(),
            },
            0.0,
        );
        assert!(clean.items().is_empty());
        let mut silent = Conversation::new();
        silent.apply(
            &SessionEvent::Exited {
                clean: false,
                detail: String::new(),
            },
            0.0,
        );
        assert_eq!(
            bodies(&silent),
            vec![ItemBody::Notice {
                kind: NoticeKind::Ended,
                text: "The session ended unexpectedly.".into()
            }]
        );
    }

    #[test]
    fn a_stopped_turn_and_a_failed_turn_leave_a_notice() {
        let mut c = Conversation::new();
        play(
            &mut c,
            &[text("half"), SessionEvent::TurnFinished(summary(true))],
        );
        play(
            &mut c,
            &[SessionEvent::TurnFailed {
                reason: "rate limit reached".into(),
            }],
        );
        play(&mut c, &[SessionEvent::TurnFinished(summary(false))]);
        assert_eq!(
            bodies(&c),
            vec![
                ItemBody::Reply {
                    markdown: "half".into(),
                    streaming: false
                },
                ItemBody::Notice {
                    kind: NoticeKind::Stopped,
                    text: "Stopped".into()
                },
                ItemBody::Notice {
                    kind: NoticeKind::TurnFailed,
                    text: "rate limit reached".into()
                },
            ]
        );
    }

    #[test]
    fn twenty_thousand_pieces_make_one_item() {
        let mut c = Conversation::new();
        for _ in 0..20_000 {
            c.apply(&text("word "), 0.0);
        }
        assert_eq!(c.items().len(), 1);
        let ItemBody::Reply { markdown, .. } = &c.items()[0].body else {
            panic!()
        };
        assert_eq!(markdown.len(), 100_000);
    }

    #[test]
    fn ids_stay_put_and_the_revision_tracks_real_changes() {
        let mut c = Conversation::new();
        c.push_user("one");
        let first = c.items()[0].id;
        let before = c.revision();
        c.apply(&SessionEvent::TitleChanged("t".into()), 0.0);
        assert_eq!(c.revision(), before);
        c.apply(&text("x"), 0.0);
        c.push_notice(NoticeKind::CommandFailed, "no");
        assert!(c.revision() > before);
        assert_eq!(c.items()[0].id, first);
        let ids: std::collections::HashSet<_> = c.items().iter().map(|i| i.id).collect();
        assert_eq!(ids.len(), c.items().len());
    }

    #[test]
    fn what_changes_nothing_leaves_the_revision_alone() {
        let mut c = Conversation::new();
        play(&mut c, &[text("a"), read("a"), plan("r1")]);
        c.answered(&RequestId("r1".into()), "ok");
        // The read finishes, so ending the turn has no call left to close.
        c.apply(
            &SessionEvent::ToolFinished {
                id: ToolCallId("a".into()),
                outcome: ToolOutcome::Ok { output: "x".into() },
            },
            0.0,
        );
        let before = c.revision();
        play(
            &mut c,
            &[
                text(""),
                SessionEvent::BlockFinished,
                SessionEvent::ToolFinished {
                    id: ToolCallId("unknown".into()),
                    outcome: ToolOutcome::Ok { output: "x".into() },
                },
                SessionEvent::RequestWithdrawn {
                    id: RequestId("r1".into()),
                },
                SessionEvent::TurnFinished(summary(false)),
                SessionEvent::Exited {
                    clean: true,
                    detail: String::new(),
                },
            ],
        );
        assert_eq!(c.revision(), before);
        let ids: Vec<_> = c.items().iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![ItemId(0), ItemId(1), ItemId(2)]);
    }

    #[test]
    fn thinking_text_after_a_start_joins_it_and_text_after_it_closes_it() {
        let mut c = Conversation::new();
        c.apply(&SessionEvent::ThinkingStarted, 1.0);
        c.apply(&SessionEvent::ThinkingDelta("hm".into()), 1.5);
        c.apply(&SessionEvent::ThinkingDelta("m".into()), 1.6);
        c.apply(&text("so"), 3.0);
        assert_eq!(
            bodies(&c),
            vec![
                ItemBody::Thinking {
                    text: "hmm".into(),
                    started: 1.0,
                    seconds: Some(2.0)
                },
                ItemBody::Reply {
                    markdown: "so".into(),
                    streaming: true
                },
            ]
        );
    }
}
