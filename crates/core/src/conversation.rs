//! The list of things a session has said, built from its events.
//!
//! Three promises the view relies on: an item's id is never reused, grows with
//! the item's place, and changes only when [`Conversation::put_history_first`]
//! numbers every item again; [`Conversation::revision`] grows whenever anything a
//! view shows has changed and stays the same otherwise; and an item's
//! [`Item::changed`] is the revision of its own last change.

use std::path::PathBuf;

use mr_agent::{
    HistoryItem, RequestId, SessionEvent, StoredHistory, ToolCallId, ToolOutcome, UserRequest,
};

use crate::changes::{self, Changes};
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
        /// The images the message had.
        image_count: usize,
        /// The files the message was sent with, by path.
        files: Vec<PathBuf>,
        /// The id of each of its images' pictures, in order: one per image.
        pictures: Vec<u64>,
        /// The message's own id in the store, when it came from there.
        stored: Option<String>,
    },
    Reply {
        markdown: String,
        streaming: bool,
    },
    Thinking {
        text: String,
        started: f64,
        time: ThinkingTime,
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

/// How long a thought took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ThinkingTime {
    /// It goes on.
    Going,
    /// It took this many seconds.
    Took(f64),
    /// It is over, and how long it took is not known: it came from the store.
    Unknown,
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
    /// The history shown is only the end of what is stored.
    EarlierLeftOut,
    /// The agent summarised everything above this point.
    Compacted,
}

const UNEXPECTED_END: &str = "The session ended unexpectedly.";
const EARLIER_LEFT_OUT: &str = "Earlier messages are not shown.";
const COMPACTED: &str = "Earlier messages were summarised here to save room.";

/// The outcome given to a call still running when its turn ends.
pub const DID_NOT_FINISH: &str = "Did not finish";

/// "Thinking…" while it goes on, then "Thought for 4 s" or "Thought for 1 min 5 s"; only
/// "Thought" when how long it took is not known.
///
/// The time is rounded to whole seconds, and a thought of less than a second still took one.
pub fn thinking_label(time: ThinkingTime) -> String {
    let seconds = match time {
        ThinkingTime::Going => return "Thinking…".into(),
        ThinkingTime::Unknown => return "Thought".into(),
        ThinkingTime::Took(seconds) => seconds,
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
    /// The files the edits changed, counted as each call finishes.
    changes: Changes,
    /// The picture ids of the user's messages, in their order.
    pictures: Vec<u64>,
}

impl Conversation {
    pub fn new() -> Self {
        Self::default()
    }

    /// A stored history as a finished conversation: no reply streams, no thinking goes on,
    /// and a call that has no outcome did not finish. `pictures` holds the picture ids of each
    /// of the history's user messages in turn, as many as the message has images (see
    /// `push_stored`).
    pub fn from_history(history: &StoredHistory, pictures: &[Vec<u64>]) -> Conversation {
        let mut conversation = Conversation::new();
        if history.earlier_left_out {
            conversation.push_notice(NoticeKind::EarlierLeftOut, EARLIER_LEFT_OUT);
        }
        let mut pictures = pictures.iter();
        for item in &history.items {
            let ids = match item {
                HistoryItem::User { .. } => pictures.next().cloned().unwrap_or_default(),
                _ => Vec::new(),
            };
            conversation.push_stored(item, ids);
        }
        conversation.close_calls();
        conversation
    }

    /// Puts `history` before the items there are, which keep their order, its user messages
    /// with `pictures` as [`Conversation::from_history`] gives them. Every item is numbered
    /// again, so ids still grow with the place; a session marks this by raising its
    /// `generation`. An empty history changes nothing.
    pub fn put_history_first(&mut self, history: &StoredHistory, pictures: &[Vec<u64>]) {
        let mut earlier = Conversation::from_history(history, pictures);
        let shift = earlier.items.len();
        if shift == 0 {
            return;
        }
        let later = std::mem::replace(&mut self.items, earlier.items);
        self.items.extend(later);
        self.changes = earlier.changes.then(std::mem::take(&mut self.changes));
        earlier.pictures.append(&mut self.pictures);
        self.pictures = earlier.pictures;
        self.revision += 1;
        for item in &mut self.items {
            item.id = ItemId(self.next_id);
            self.next_id += 1;
            item.changed = self.revision;
        }
        for at in self
            .open
            .iter_mut()
            .chain(&mut self.waiting)
            .chain(&mut self.calls)
        {
            *at += shift;
        }
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The files this conversation's edits changed.
    pub fn changes(&self) -> &Changes {
        &self.changes
    }

    /// Grows by at least one whenever anything a view shows has changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The picture id of every image of the user's messages, in their order. Costs nothing
    /// that grows with the conversation's length.
    pub fn pictures(&self) -> impl Iterator<Item = u64> + '_ {
        self.pictures.iter().copied()
    }

    /// A message of the user's, with one picture id per image.
    pub fn push_user(&mut self, text: &str, pictures: Vec<u64>, files: Vec<PathBuf>) {
        self.push(ItemBody::User {
            text: text.into(),
            image_count: pictures.len(),
            files,
            pictures,
            stored: None,
        });
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
            | SessionEvent::EffortChanged(_)
            | SessionEvent::PermissionModeChanged(_)
            | SessionEvent::ContextBreakdown(_)
            | SessionEvent::BackgroundTasks { .. }
            | SessionEvent::CommandFailed { .. }
            | SessionEvent::TaskAdded(_)
            | SessionEvent::TaskChanged(_)
            | SessionEvent::TaskRemoved { .. }
            | SessionEvent::ChoicesOffered(_) => {}
        }
    }

    /// Marks a waiting request answered, with `words` to show. False when it was not waiting.
    pub fn answered(&mut self, id: &RequestId, words: &str) -> bool {
        self.settle(id, RequestState::Answered(words.into()))
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// One stored item, pushed as the live events would have left it, but closed. Thinking
    /// keeps no time: the store does not say how long it took. A user's message takes
    /// `pictures` as its pictures' ids, one per image; its image count is the number of ids,
    /// so it never counts an image it has no slot for.
    fn push_stored(&mut self, item: &HistoryItem, pictures: Vec<u64>) {
        match item {
            HistoryItem::User {
                text,
                image_count,
                files,
                stored,
                ..
            } => {
                debug_assert_eq!(
                    pictures.len(),
                    *image_count,
                    "a stored message is given one picture id per image"
                );
                self.push(ItemBody::User {
                    text: text.clone(),
                    image_count: pictures.len(),
                    files: files.clone(),
                    pictures,
                    stored: stored.clone(),
                })
            }
            HistoryItem::AssistantText(markdown) => self.push(ItemBody::Reply {
                markdown: markdown.clone(),
                streaming: false,
            }),
            HistoryItem::Thinking(text) => self.push(ItemBody::Thinking {
                text: text.clone(),
                started: 0.0,
                time: ThinkingTime::Unknown,
            }),
            HistoryItem::Tool {
                id,
                name,
                detail,
                outcome,
            } => self.tool_started(ToolCall {
                id: id.clone(),
                name: name.clone(),
                detail: detail.clone(),
                outcome: outcome.clone(),
            }),
            HistoryItem::Compacted => self.push_notice(NoticeKind::Compacted, COMPACTED),
        }
    }

    fn push(&mut self, body: ItemBody) {
        if let ItemBody::User { pictures, .. } = &body {
            self.pictures.extend(pictures);
        }
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
            time: ThinkingTime::Going,
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
                    time: ThinkingTime::Going,
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
        // A stored call comes with its outcome, which may already count.
        self.changes.add(&call);
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
            let counted = changes::counts(call);
            call.outcome = Some(outcome.clone());
            match (counted, changes::counts(call)) {
                (false, true) => self.changes.add(call),
                (true, false) => self.changes.take_back(call),
                _ => {}
            }
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
                time: time @ ThinkingTime::Going,
                ..
            }) => {
                *time = ThinkingTime::Took((now - *started).max(0.0));
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes::FileChange;
    use crate::session::{HistoryState, Session};
    use crate::tools::line_counts;
    use mr_agent::{
        HistoryItem, RequestId, SessionEvent, StoredHistory, ToolCallId, ToolDetail, ToolOutcome,
        TurnSummary, UserRequest, UserRequestKind,
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
        assert_eq!(thinking_label(ThinkingTime::Going), "Thinking…");
        assert_eq!(thinking_label(ThinkingTime::Took(0.2)), "Thought for 1 s");
        assert_eq!(thinking_label(ThinkingTime::Took(3.5)), "Thought for 4 s");
        assert_eq!(
            thinking_label(ThinkingTime::Took(65.0)),
            "Thought for 1 min 5 s"
        );
        assert_eq!(thinking_label(ThinkingTime::Unknown), "Thought");
    }

    #[test]
    fn an_items_change_stamp_moves_only_when_it_changes() {
        let stamps = |c: &Conversation| c.items().iter().map(|i| i.changed).collect::<Vec<_>>();
        let mut c = Conversation::new();
        c.push_user("one", Vec::new(), Vec::new());
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
            c.push_user("go", Vec::new(), Vec::new());
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
        c.push_user("**hi**", Vec::new(), Vec::new());
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
                    text: "**hi**".into(),
                    image_count: 0,
                    files: Vec::new(),
                    pictures: Vec::new(),
                    stored: None,
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
                time: ThinkingTime::Going
            }]
        );
        c.apply(&SessionEvent::BlockFinished, 13.5);
        assert_eq!(
            bodies(&c),
            vec![ItemBody::Thinking {
                text: String::new(),
                started: 10.0,
                time: ThinkingTime::Took(3.5)
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
                    time: ThinkingTime::Took(1.0)
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
        c.push_user("one", Vec::new(), Vec::new());
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

    fn history(items: Vec<HistoryItem>) -> StoredHistory {
        StoredHistory {
            items,
            ..StoredHistory::default()
        }
    }

    fn stored_read(id: &str, outcome: Option<ToolOutcome>) -> HistoryItem {
        HistoryItem::Tool {
            id: ToolCallId(id.into()),
            name: "Read".into(),
            detail: ToolDetail::FileRead {
                path: format!("{id}.rs"),
            },
            outcome,
        }
    }

    #[test]
    fn a_history_is_drawn_as_a_finished_conversation() {
        let c = Conversation::from_history(
            &StoredHistory {
                items: vec![
                    HistoryItem::User {
                        text: "Look at the parser".into(),
                        image_count: 0,
                        files: Vec::new(),
                        images: Vec::new(),
                        stored: None,
                    },
                    HistoryItem::Thinking("hm".into()),
                    stored_read("a", Some(ToolOutcome::Ok { output: "x".into() })),
                    stored_read("b", None),
                    HistoryItem::AssistantText("It reads fine.".into()),
                ],
                earlier_left_out: true,
                ..StoredHistory::default()
            },
            &[],
        );
        let b = bodies(&c);
        assert_eq!(b.len(), 5, "{b:?}");
        assert_eq!(
            b[0],
            ItemBody::Notice {
                kind: NoticeKind::EarlierLeftOut,
                text: "Earlier messages are not shown.".into()
            }
        );
        assert_eq!(
            b[2],
            ItemBody::Thinking {
                text: "hm".into(),
                started: 0.0,
                time: ThinkingTime::Unknown
            },
            "stored thinking is over, and claims no time"
        );
        let ItemBody::Tools(group) = &b[3] else {
            panic!("{:?}", b[3])
        };
        assert_eq!(
            group.summary(),
            "Read 2 files",
            "stored calls fold as live ones do"
        );
        assert_eq!(
            group.calls[1].outcome,
            Some(ToolOutcome::Failed {
                output: DID_NOT_FINISH.into()
            })
        );
        assert_eq!(
            b[4],
            ItemBody::Reply {
                markdown: "It reads fine.".into(),
                streaming: false
            }
        );
        let ids: Vec<_> = c.items().iter().map(|i| i.id.0).collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4]);
        assert!(c.items().iter().all(|i| i.changed <= c.revision()));
    }

    #[test]
    fn a_stored_thought_reads_thought_with_no_time() {
        let c = Conversation::from_history(&history(vec![HistoryItem::Thinking("hm".into())]), &[]);
        let ItemBody::Thinking { time, .. } = c.items()[0].body else {
            panic!("{:?}", c.items()[0].body)
        };
        assert_eq!(thinking_label(time), "Thought");
    }

    #[test]
    fn a_stored_message_keeps_its_image_count() {
        let c = Conversation::from_history(
            &history(vec![HistoryItem::User {
                text: String::new(),
                image_count: 2,
                files: Vec::new(),
                images: vec![None; 2],
                stored: None,
            }]),
            &[vec![STORED | 1, STORED | 2]],
        );
        assert_eq!(
            bodies(&c),
            vec![ItemBody::User {
                text: String::new(),
                image_count: 2,
                files: Vec::new(),
                pictures: vec![STORED | 1, STORED | 2],
                stored: None,
            }]
        );
        let mut typed = Conversation::new();
        typed.push_user("hi", vec![9], Vec::new());
        assert!(matches!(
            &typed.items()[0].body,
            ItemBody::User { image_count: 1, pictures, .. } if pictures == &[9]
        ));
    }

    /// The top bit of a stored picture's id.
    const STORED: u64 = 1 << 63;

    /// A user's stored message of `text` with `images` images.
    fn stored_said(text: &str, images: usize) -> HistoryItem {
        HistoryItem::User {
            text: text.into(),
            image_count: images,
            files: Vec::new(),
            images: vec![None; images],
            stored: None,
        }
    }

    #[test]
    fn the_conversation_knows_its_pictures() {
        let mut c = Conversation::new();
        assert_eq!(c.pictures().count(), 0);
        c.push_user("two", vec![4, 5], Vec::new());
        c.push_user("none", Vec::new(), vec!["/home/alex/a.txt".into()]);
        play(&mut c, &[text("Seen.")]);
        c.push_user("one", vec![6], Vec::new());
        assert_eq!(c.pictures().collect::<Vec<_>>(), vec![4, 5, 6]);
        // A history put first brings its own, each stored message with the ids given for it.
        c.put_history_first(
            &history(vec![
                stored_said("first", 1),
                HistoryItem::AssistantText("Earlier".into()),
                stored_said("plain", 0),
                stored_said("second", 2),
            ]),
            &[vec![STORED | 1], Vec::new(), vec![STORED | 2, STORED | 3]],
        );
        assert_eq!(
            c.pictures().collect::<Vec<_>>(),
            vec![STORED | 1, STORED | 2, STORED | 3, 4, 5, 6]
        );
        let held: Vec<u64> = c
            .items()
            .iter()
            .flat_map(|item| match &item.body {
                ItemBody::User { pictures, .. } => pictures.clone(),
                _ => Vec::new(),
            })
            .collect();
        assert_eq!(held, c.pictures().collect::<Vec<_>>());
        // An empty history brings none; a conversation made of a history knows its own.
        c.put_history_first(&StoredHistory::default(), &[]);
        assert_eq!(c.pictures().count(), 6);
        let stored = Conversation::from_history(
            &history(vec![stored_said("first", 1)]),
            &[vec![STORED | 9]],
        );
        assert_eq!(stored.pictures().collect::<Vec<_>>(), vec![STORED | 9]);
    }

    #[test]
    fn a_stored_message_shows_its_files() {
        let files: Vec<PathBuf> = vec!["/home/alex/a.txt".into(), "/home/alex/docs".into()];
        let c = Conversation::from_history(
            &history(vec![HistoryItem::User {
                text: "read these".into(),
                image_count: 1,
                files: files.clone(),
                images: vec![None; 1],
                stored: None,
            }]),
            &[vec![STORED | 1]],
        );
        assert_eq!(
            bodies(&c),
            vec![ItemBody::User {
                text: "read these".into(),
                image_count: 1,
                files,
                pictures: vec![STORED | 1],
                stored: None,
            }]
        );
    }

    /// Every user message of `c`, as its image count and its pictures.
    fn counted(c: &Conversation) -> Vec<(usize, Vec<u64>)> {
        c.items()
            .iter()
            .filter_map(|item| match &item.body {
                ItemBody::User {
                    image_count,
                    pictures,
                    ..
                } => Some((*image_count, pictures.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_stored_item_has_one_id_for_each_image() {
        let stored = history(vec![
            stored_said("two", 2),
            HistoryItem::AssistantText("Seen.".into()),
            stored_said("none", 0),
            stored_said("one", 1),
        ]);
        let ids = [vec![STORED | 1, STORED | 2], Vec::new(), vec![STORED | 3]];
        let c = Conversation::from_history(&stored, &ids);
        assert_eq!(
            counted(&c),
            vec![
                (2, vec![STORED | 1, STORED | 2]),
                (0, Vec::new()),
                (1, vec![STORED | 3])
            ]
        );
        let mut later = Conversation::new();
        later.push_user("live", vec![7], Vec::new());
        later.put_history_first(&stored, &ids);
        assert!(
            counted(&later)
                .iter()
                .all(|(count, pictures)| *count == pictures.len())
        );
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "a stored message is given one picture id per image")
    )]
    fn a_stored_item_given_too_few_ids_never_counts_more_images_than_it_holds() {
        let c = Conversation::from_history(&history(vec![stored_said("two", 2)]), &[vec![9]]);
        // Without the debug check: the count follows the ids.
        assert_eq!(counted(&c), vec![(1, vec![9])]);
    }

    #[test]
    fn a_history_put_first_leaves_what_is_open_working() {
        let mut c = Conversation::new();
        c.push_user("go", Vec::new(), Vec::new());
        play(&mut c, &[read("r1"), plan("p1"), text("Work")]);
        let revision = c.revision();
        c.put_history_first(
            &history(vec![
                HistoryItem::User {
                    text: "before".into(),
                    image_count: 0,
                    files: Vec::new(),
                    images: Vec::new(),
                    stored: None,
                },
                HistoryItem::AssistantText("Earlier".into()),
            ]),
            &[],
        );
        assert!(c.revision() > revision);
        let b = bodies(&c);
        assert_eq!(
            b[..3],
            [
                ItemBody::User {
                    text: "before".into(),
                    image_count: 0,
                    files: Vec::new(),
                    pictures: Vec::new(),
                    stored: None,
                },
                ItemBody::Reply {
                    markdown: "Earlier".into(),
                    streaming: false
                },
                ItemBody::User {
                    text: "go".into(),
                    image_count: 0,
                    files: Vec::new(),
                    pictures: Vec::new(),
                    stored: None,
                },
            ]
        );
        // Ids keep growing with the position, so a view can still look an item up by its id.
        let ids: Vec<_> = c.items().iter().map(|i| i.id.0).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
        assert!(ids[0] >= 4, "no id is used twice: {ids:?}");
        // The reply still streams into its own item, the request can still be answered, and
        // ending the turn still closes the call left running.
        play(&mut c, &[text(" goes on")]);
        assert_eq!(c.items().len(), 6);
        assert_eq!(
            c.items()[5].body,
            ItemBody::Reply {
                markdown: "Work goes on".into(),
                streaming: true
            }
        );
        assert!(c.answered(&RequestId("p1".into()), "Plan approved"));
        play(&mut c, &[SessionEvent::TurnFinished(summary(false))]);
        assert_eq!(
            group(&c, 3).calls[0].outcome,
            Some(ToolOutcome::Failed {
                output: DID_NOT_FINISH.into()
            })
        );
        assert!(matches!(
            c.items()[5].body,
            ItemBody::Reply {
                streaming: false,
                ..
            }
        ));
    }

    #[test]
    fn an_empty_history_changes_nothing() {
        let mut c = Conversation::new();
        c.push_user("go", Vec::new(), Vec::new());
        let (before, revision) = (c.items().to_vec(), c.revision());
        c.put_history_first(&StoredHistory::default(), &[]);
        assert_eq!((c.items(), c.revision()), (&before[..], revision));
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
                    time: ThinkingTime::Took(2.0)
                },
                ItemBody::Reply {
                    markdown: "so".into(),
                    streaming: true
                },
            ]
        );
    }

    const MAIN: &str = "/home/alex/app/src/main.rs";
    const LIB: &str = "/home/alex/app/src/lib.rs";
    const NOTES: &str = "/home/alex/app/NOTES.md";

    fn edit(id: &str, path: &str, old_text: &str, new_text: &str) -> SessionEvent {
        SessionEvent::ToolStarted {
            id: ToolCallId(id.into()),
            name: "Edit".into(),
            detail: ToolDetail::FileEdit {
                path: path.into(),
                old_text: old_text.into(),
                new_text: new_text.into(),
            },
        }
    }
    fn write(id: &str, path: &str, content: &str) -> SessionEvent {
        SessionEvent::ToolStarted {
            id: ToolCallId(id.into()),
            name: "Write".into(),
            detail: ToolDetail::FileWrite {
                path: path.into(),
                content: content.into(),
            },
        }
    }
    fn finished(id: &str, outcome: ToolOutcome) -> SessionEvent {
        SessionEvent::ToolFinished {
            id: ToolCallId(id.into()),
            outcome,
        }
    }
    fn ok() -> ToolOutcome {
        ToolOutcome::Ok {
            output: "done".into(),
        }
    }
    fn failed(output: &str) -> ToolOutcome {
        ToolOutcome::Failed {
            output: output.into(),
        }
    }
    fn change(path: &str, added: usize, removed: usize) -> FileChange {
        FileChange {
            path: path.into(),
            added,
            removed,
        }
    }

    #[test]
    fn an_edit_that_succeeded_counts_its_lines() {
        let (old, new) = ("fn main() {}\n", "fn main() {\n    run();\n}\n");
        let mut c = Conversation::new();
        play(&mut c, &[edit("e1", MAIN, old, new)]);
        assert!(c.changes().files().is_empty(), "not finished yet");
        play(&mut c, &[finished("e1", ok())]);
        // The numbers the call's own row shows.
        assert_eq!(line_counts(old, new), (3, 1));
        assert_eq!(c.changes().files(), [change(MAIN, 3, 1)]);
    }

    #[test]
    fn a_written_file_counts_its_lines_as_added() {
        let mut c = Conversation::new();
        play(
            &mut c,
            &[
                write("w1", LIB, "pub mod a;\npub mod b;\npub mod c;"),
                finished("w1", ok()),
            ],
        );
        assert_eq!(c.changes().files(), [change(LIB, 3, 0)]);
    }

    #[test]
    fn a_failed_a_denied_and_an_unfinished_edit_do_not_count() {
        let mut c = Conversation::new();
        play(
            &mut c,
            &[
                edit("e1", MAIN, "a\n", "b\n"),
                finished("e1", failed("The text to replace was not found.")),
                write("w1", LIB, "one\n"),
                finished("w1", failed("Denied by the user")),
                edit("e2", NOTES, "a\n", "b\n"),
                SessionEvent::TurnFinished(summary(false)),
            ],
        );
        assert_eq!(c.changes(), &Changes::default());
    }

    #[test]
    fn a_late_result_counts_and_a_result_that_turns_to_failed_is_taken_back() {
        let mut c = Conversation::new();
        play(
            &mut c,
            &[
                edit("e1", MAIN, "a\n", "b\n"),
                edit("e2", LIB, "a\n", "b\nc\n"),
                finished("e2", ok()),
                SessionEvent::TurnFinished(summary(false)),
            ],
        );
        assert_eq!(c.changes().files(), [change(LIB, 2, 1)]);
        // The call the turn closed as "Did not finish" still takes its real outcome.
        play(&mut c, &[finished("e1", ok())]);
        assert_eq!(c.changes().files(), [change(LIB, 2, 1), change(MAIN, 1, 1)]);
        // Another output for a call that succeeded counts nothing again.
        play(
            &mut c,
            &[finished(
                "e1",
                ToolOutcome::Ok {
                    output: "again".into(),
                },
            )],
        );
        assert_eq!(c.changes().files(), [change(LIB, 2, 1), change(MAIN, 1, 1)]);
        play(&mut c, &[finished("e2", failed("gone"))]);
        assert_eq!(c.changes().files(), [change(MAIN, 1, 1)]);
    }

    #[test]
    fn other_tools_change_nothing() {
        let mut c = Conversation::new();
        let started = |id: &str, name: &str, detail: ToolDetail| SessionEvent::ToolStarted {
            id: ToolCallId(id.into()),
            name: name.into(),
            detail,
        };
        play(
            &mut c,
            &[
                read("r1"),
                finished("r1", ok()),
                started(
                    "c1",
                    "Bash",
                    ToolDetail::Command {
                        command: "rm /home/alex/app/old.txt".into(),
                    },
                ),
                finished("c1", ok()),
                started(
                    "s1",
                    "Grep",
                    ToolDetail::Search {
                        query: "fn main".into(),
                    },
                ),
                finished("s1", ok()),
                started(
                    "o1",
                    "NotebookEdit",
                    ToolDetail::Other {
                        input_json: r#"{"notebook_path":"/home/alex/app/a.ipynb"}"#.into(),
                    },
                ),
                finished("o1", ok()),
            ],
        );
        assert_eq!(c.changes(), &Changes::default());
    }

    #[test]
    fn a_stored_sessions_edits_are_counted_once_and_come_first() {
        let stored_edit = |id: &str, path: &str, outcome: Option<ToolOutcome>| HistoryItem::Tool {
            id: ToolCallId(id.into()),
            name: "Edit".into(),
            detail: ToolDetail::FileEdit {
                path: path.into(),
                old_text: "a\n".into(),
                new_text: "b\nc\n".into(),
            },
            outcome,
        };
        let stored = StoredHistory {
            earlier_left_out: true,
            ..history(vec![
                HistoryItem::User {
                    text: "tidy up".into(),
                    image_count: 0,
                    files: Vec::new(),
                    images: Vec::new(),
                    stored: None,
                },
                stored_edit("h1", MAIN, Some(ok())),
                HistoryItem::Tool {
                    id: ToolCallId("h2".into()),
                    name: "Write".into(),
                    detail: ToolDetail::FileWrite {
                        path: LIB.into(),
                        content: "one\ntwo\n".into(),
                    },
                    outcome: Some(ok()),
                },
                stored_edit("h3", NOTES, Some(failed("Denied by the user"))),
                // Its result is not in the store; the program may still send it.
                stored_edit("h4", NOTES, None),
            ])
        };
        let mut s = Session::opening("/home/alex/app");
        s.conversation
            .push_user("and the docs", Vec::new(), Vec::new());
        play(
            &mut s.conversation,
            &[edit("e1", LIB, "two\n", "2\n"), finished("e1", ok())],
        );
        s.history = HistoryState::Loading;
        s.load_history(stored.clone(), &[]);
        s.load_history(stored.clone(), &[]);
        let expected = [change(MAIN, 2, 1), change(LIB, 3, 1)];
        assert_eq!(s.conversation.changes().files(), expected);
        // The history alone, read as often as asked, counts the same.
        let alone = [change(MAIN, 2, 1), change(LIB, 2, 0)];
        assert_eq!(
            Conversation::from_history(&stored, &[]).changes().files(),
            alone
        );
        assert_eq!(
            Conversation::from_history(&stored, &[]).changes().files(),
            alone
        );
        // A stored call's result sent again counts nothing again.
        play(&mut s.conversation, &[finished("h1", ok())]);
        assert_eq!(s.conversation.changes().files(), expected);
        // The result of a stored call that had none counts once it comes.
        play(&mut s.conversation, &[finished("h4", ok())]);
        assert_eq!(
            s.conversation.changes().files(),
            [change(MAIN, 2, 1), change(LIB, 3, 1), change(NOTES, 2, 1)]
        );
    }
}
