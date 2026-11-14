//! One session's life: its phase, the message waiting to be sent, and what the
//! user can ask of it. Every intent returns the command to send instead of
//! sending it, so this module stays plain data.

use std::path::PathBuf;

use mr_agent::{
    Answer, CommandKind, ModelOption, PermissionMode, RequestId, SessionCommand, SessionEvent,
    SessionId, StartSpec, StoredHistory, StoredSession,
};

use crate::conversation::{Conversation, NoticeKind};

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// No program runs for it; its conversation is on disk. The next message starts one.
    Resting,
    /// The program is starting; nothing can be sent yet.
    Opening,
    Idle,
    /// A turn is in flight.
    Working,
    /// A turn is in flight and at least one request waits for the user.
    NeedsUser,
    Ended {
        clean: bool,
        detail: String,
    },
}

/// How much of a stored conversation the session holds.
#[derive(Clone, Debug, PartialEq)]
pub enum HistoryState {
    NotLoaded,
    Loading,
    /// The history is in the conversation, or there was none to read.
    Loaded,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub folder: PathBuf,
    pub id: Option<SessionId>,
    /// The agent's name for the session or, until it gives one, a stand-in made from the first
    /// message sent; for a stored session, the store's title. `None` before any message.
    pub title: Option<String>,
    pub phase: Phase,
    /// The full id of the model in use, once known.
    pub model: Option<String>,
    pub models: Vec<ModelOption>,
    pub mode: PermissionMode,
    pub conversation: Conversation,
    pub history: HistoryState,
    /// Goes up whenever the conversation's items are replaced, not appended to.
    pub generation: u64,
    /// A message typed while it could not be sent yet.
    pub queued: Option<String>,
    pub cost_usd: Option<f64>,
    pub context_used: Option<f32>,
    /// The option id the user last picked, for naming the model they chose.
    chosen_model: Option<String>,
    /// The mode is known from the program or from the user, so a history that arrives later
    /// does not change it.
    mode_known: bool,
    /// `models` came from the session's own program, not from a list it was offered.
    own_models: bool,
    /// It came from the store.
    from_store: bool,
    /// Messages handed over in this run, less those that could not be.
    handed_over: usize,
}

impl Session {
    pub fn opening(folder: impl Into<PathBuf>) -> Session {
        Session {
            folder: folder.into(),
            id: None,
            title: None,
            phase: Phase::Opening,
            model: None,
            models: Vec::new(),
            mode: PermissionMode::Ask,
            conversation: Conversation::new(),
            history: HistoryState::Loaded,
            generation: 0,
            queued: None,
            cost_usd: None,
            context_used: None,
            chosen_model: None,
            mode_known: false,
            own_models: false,
            from_store: false,
            handed_over: 0,
        }
    }

    /// A stored session, resting: its id, folder and title, and no history yet.
    pub fn resting(stored: &StoredSession) -> Session {
        let mut session = Session::opening(stored.folder.clone());
        session.id = Some(stored.id.clone());
        session.title = Some(stored.title.clone()).filter(|title| !title.is_empty());
        session.phase = Phase::Resting;
        session.history = HistoryState::NotLoaded;
        session.from_store = true;
        session
    }

    /// Puts the stored history before what this run added to the conversation. The history's
    /// model and mode are taken when neither the user nor the program has named them yet,
    /// except Bypass or an unknown mode, which leave the mode as it is. A history that is
    /// already loaded is not loaded again.
    pub fn load_history(&mut self, history: StoredHistory) {
        if self.history == HistoryState::Loaded {
            return;
        }
        let revision = self.conversation.revision();
        self.conversation.put_history_first(&history);
        if self.conversation.revision() != revision {
            self.generation += 1;
        }
        if self.model.is_none() {
            self.model = history.model;
        }
        // A file never turns permission checks off: Bypass, and any mode this app does not
        // know, would start the program with less asked of the user than the picker offers.
        if !self.mode_known
            && let Some(mode) = history.permission_mode
            && !matches!(mode, PermissionMode::Bypass | PermissionMode::Other(_))
        {
            self.mode = mode;
        }
        self.history = HistoryState::Loaded;
    }

    /// The history could not be read: `reason` says why. A history already loaded stays.
    pub fn history_failed(&mut self, reason: &str) {
        if self.history != HistoryState::Loaded {
            self.history = HistoryState::Failed(reason.into());
        }
    }

    /// Whether the agent has anything stored for it: it came from the store, or a message
    /// was handed over in this run.
    pub fn is_stored(&self) -> bool {
        self.from_store || self.handed_over > 0
    }

    /// Resting to Opening, with `text` queued for when the program is ready. Trimmed as
    /// [`Session::submit`] trims; a resting session is not woken by whitespace alone.
    pub fn wake(&mut self, text: &str) {
        if self.phase != Phase::Resting || text.trim().is_empty() {
            return;
        }
        self.phase = Phase::Opening;
        self.queue(text.trim_end());
    }

    /// Undoes `wake` before a program was opened: back to Resting, giving the text back.
    pub fn unwake(&mut self) -> Option<String> {
        if self.phase != Phase::Opening || !self.is_stored() {
            return None;
        }
        self.phase = Phase::Resting;
        self.queued.take()
    }

    /// How to start its program: resuming when `is_stored`, afresh otherwise; with the model
    /// and mode it shows. The model is the option the user chose while it names the model in
    /// use, else the model's id; the mode is passed unless it is Ask, the program's default.
    pub fn start_spec(&self) -> StartSpec {
        let mut spec = StartSpec::new(self.folder.clone());
        if self.is_stored() {
            spec.resume = self.id.clone();
        }
        let chosen = self
            .named_by()
            .filter(|option| self.chosen_model.as_ref() == Some(&option.id));
        spec.model = chosen
            .map(|option| option.id.clone())
            .or_else(|| self.model.clone());
        if self.mode != PermissionMode::Ask {
            spec.permission_mode = Some(self.mode.clone());
        }
        spec
    }

    /// A message could not be handed over: Working goes back to Idle.
    pub fn send_failed(&mut self) {
        self.handed_over = self.handed_over.saturating_sub(1);
        if self.phase == Phase::Working {
            self.phase = Phase::Idle;
        }
    }

    /// Gives the session models to choose from when it has none of its own yet.
    pub fn offer_models(&mut self, models: &[ModelOption]) {
        if !self.own_models {
            self.models = models.to_vec();
        }
    }

    /// Applies an event. Returns commands the session now wants sent: a queued message, and the
    /// request for a title when that message is the session's first.
    pub fn apply(&mut self, event: SessionEvent, now: f64) -> Vec<SessionCommand> {
        self.conversation.apply(&event, now);
        match event {
            SessionEvent::Started(info) => {
                if self.phase == Phase::Opening {
                    self.id = Some(info.id);
                    self.model = Some(info.model);
                    self.models = info.models;
                    self.own_models = true;
                    self.mode = info.permission_mode;
                    self.mode_known = true;
                    self.phase = Phase::Idle;
                    return self.send_queued();
                }
            }
            SessionEvent::TurnStarted => {
                if !self.at_rest() {
                    self.phase = if self.conversation.waiting() > 0 {
                        Phase::NeedsUser
                    } else {
                        Phase::Working
                    };
                }
            }
            SessionEvent::NeedsUser(_) => {
                if self.phase == Phase::Working {
                    self.phase = Phase::NeedsUser;
                }
            }
            SessionEvent::RequestWithdrawn { .. } => self.carry_on_when_nothing_waits(),
            SessionEvent::TurnFinished(summary) => {
                if self.in_turn() {
                    self.cost_usd = Some(summary.cost_usd);
                    if summary.context_used.is_some() {
                        self.context_used = summary.context_used;
                    }
                    self.phase = Phase::Idle;
                    return self.send_queued();
                }
            }
            SessionEvent::TurnFailed { .. } => {
                if self.in_turn() {
                    self.phase = Phase::Idle;
                    return self.send_queued();
                }
            }
            SessionEvent::TitleChanged(title) => self.title = Some(title),
            SessionEvent::ModelChanged(model) => self.model = Some(model),
            SessionEvent::PermissionModeChanged(mode) => {
                self.mode = mode;
                self.mode_known = true;
            }
            SessionEvent::CommandFailed { what, reason } => {
                let wording = match what {
                    CommandKind::SetModel => "Could not change the model",
                    CommandKind::SetPermissionMode => "Could not change the permission mode",
                    CommandKind::Rename => "Could not rename the session",
                    CommandKind::SetRemoteAccess => "Could not change Remote Control",
                    // Asked for in the background; the stand-in title simply stays.
                    CommandKind::RefreshUsage | CommandKind::SuggestTitle => return Vec::new(),
                };
                self.conversation
                    .push_notice(NoticeKind::CommandFailed, &format!("{wording}: {reason}"));
            }
            // A clean end leaves a stored conversation that the next message carries on.
            SessionEvent::Exited { clean, detail } => {
                self.phase = if clean && self.is_stored() {
                    Phase::Resting
                } else {
                    Phase::Ended { clean, detail }
                };
            }
            SessionEvent::TextDelta(_)
            | SessionEvent::ThinkingDelta(_)
            | SessionEvent::ThinkingStarted
            | SessionEvent::BlockFinished
            | SessionEvent::ToolStarted { .. }
            | SessionEvent::ToolFinished { .. }
            | SessionEvent::UsageLimits(_)
            | SessionEvent::RemoteAccess(_) => {}
        }
        Vec::new()
    }

    /// Sends `text`, or queues it while it cannot be sent yet. Only trailing whitespace is
    /// dropped: a first line's indentation is part of what was typed. A message with nothing
    /// but whitespace is neither sent nor queued. A session's first message sent also names the
    /// session for now and asks the agent for a title. A resting session takes nothing here:
    /// it is woken with [`Session::wake`], since its program must be started first.
    pub fn submit(&mut self, text: &str) -> Vec<SessionCommand> {
        if text.trim().is_empty() {
            return Vec::new();
        }
        let text = text.trim_end();
        match self.phase {
            Phase::Idle => self.hand_over(text.into()),
            Phase::Opening | Phase::Working | Phase::NeedsUser => {
                self.queue(text);
                Vec::new()
            }
            Phase::Resting | Phase::Ended { .. } => Vec::new(),
        }
    }

    /// Adds `text` to the queued message, after a blank line.
    fn queue(&mut self, text: &str) {
        match &mut self.queued {
            Some(queued) => {
                queued.push_str("\n\n");
                queued.push_str(text);
            }
            None => self.queued = Some(text.into()),
        }
    }

    pub fn stop(&self) -> Option<SessionCommand> {
        self.in_turn().then_some(SessionCommand::Stop)
    }

    pub fn answer(&mut self, id: &RequestId, answer: Answer) -> Option<SessionCommand> {
        if self.at_rest() || !self.conversation.answered(id, &answer_words(&answer)) {
            return None;
        }
        self.carry_on_when_nothing_waits();
        Some(SessionCommand::Answer {
            id: id.clone(),
            answer,
        })
    }

    /// Asks for the model option `option_id`. Nothing is sent when it is already the model in
    /// use and already the option the model is named by: the picker then only closes. A
    /// resting session takes the choice at once, sending nothing: its program starts with it.
    pub fn choose_model(&mut self, option_id: &str) -> Option<SessionCommand> {
        if matches!(self.phase, Phase::Opening | Phase::Ended { .. }) {
            return None;
        }
        let option = self.models.iter().find(|option| option.id == option_id)?;
        let in_use = self.model.as_ref() == Some(&option.resolved);
        if in_use && self.named_by().is_some_and(|named| named.id == option.id) {
            return None;
        }
        self.chosen_model = Some(option_id.into());
        if self.phase == Phase::Resting {
            self.model = Some(option.resolved.clone());
            return None;
        }
        Some(SessionCommand::SetModel(option_id.into()))
    }

    /// Asks for `mode`. A resting session takes it at once, sending nothing.
    pub fn choose_mode(&mut self, mode: PermissionMode) -> Option<SessionCommand> {
        if matches!(self.phase, Phase::Opening | Phase::Ended { .. }) || self.mode == mode {
            return None;
        }
        if self.phase == Phase::Resting {
            self.mode = mode;
            self.mode_known = true;
            return None;
        }
        Some(SessionCommand::SetPermissionMode(mode))
    }

    /// Gives the queued message back, for the input box.
    pub fn take_queued(&mut self) -> Option<String> {
        self.queued.take()
    }

    /// The model in use, as the picker names it: by the option it is named by (see
    /// [`model_option_label`]), or by its id when no option names it.
    pub fn model_label(&self) -> String {
        let Some(model) = &self.model else {
            return "Model".into();
        };
        self.named_by()
            .map_or_else(|| model.clone(), model_option_label)
    }

    /// Whether the `Auto` permission mode can be offered: the option that names the model in
    /// use supports it.
    pub fn auto_mode(&self) -> bool {
        self.named_by().is_some_and(|option| option.auto_mode)
    }

    /// The option the model in use is named by: the one the user chose while it is the model
    /// in use, otherwise the first that resolves to it.
    fn named_by(&self) -> Option<&ModelOption> {
        let model = self.model.as_ref()?;
        let chosen = self
            .chosen_model
            .as_ref()
            .and_then(|id| self.models.iter().find(|option| option.id == *id))
            .filter(|option| option.resolved == *model);
        chosen.or_else(|| {
            self.models
                .iter()
                .find(|option| option.resolved == *model || option.id == *model)
        })
    }

    /// Back to `Opening` for a restart, keeping the conversation.
    pub fn reopening(&mut self) {
        self.phase = Phase::Opening;
    }

    /// It can be sent a message: its program runs or starts, or it rests and a message wakes
    /// it. An ended one takes none until it is restarted.
    pub fn takes_messages(&self) -> bool {
        !matches!(self.phase, Phase::Ended { .. })
    }

    /// No program runs for it: it rests or has ended.
    pub fn at_rest(&self) -> bool {
        matches!(self.phase, Phase::Resting | Phase::Ended { .. })
    }

    fn in_turn(&self) -> bool {
        matches!(self.phase, Phase::Working | Phase::NeedsUser)
    }

    fn carry_on_when_nothing_waits(&mut self) {
        if self.phase == Phase::NeedsUser && self.conversation.waiting() == 0 {
            self.phase = Phase::Working;
        }
    }

    /// Starts a turn with the queued message, when there is one.
    fn send_queued(&mut self) -> Vec<SessionCommand> {
        match self.queued.take() {
            Some(text) => self.hand_over(text),
            None => Vec::new(),
        }
    }

    /// Starts a turn with `text`: the `Send`, and when the session has no title yet, the title
    /// is set at once to a stand-in made from the message and a `SuggestTitle` follows the
    /// `Send`, so the agent can name the session. A session with a title, its own or a stand-in,
    /// never asks, so each session asks at most once.
    fn hand_over(&mut self, text: String) -> Vec<SessionCommand> {
        self.conversation.push_user(&text);
        self.phase = Phase::Working;
        self.handed_over += 1;
        let suggest = self.title.is_none().then(|| {
            self.title = Some(title_stand_in(&text));
            SessionCommand::SuggestTitle {
                from: text.chars().take(TITLE_FROM_CHARS).collect(),
            }
        });
        let send = SessionCommand::Send {
            text,
            attachments: vec![],
        };
        std::iter::once(send).chain(suggest).collect()
    }
}

/// The longest stand-in title, in characters, before its "…".
const STAND_IN_CHARS: usize = 60;

/// The most of a first message the agent is given to name its session from, in characters.
const TITLE_FROM_CHARS: usize = 2_000;

/// What a session is called until the agent names it: the message's first line that is not
/// blank, each run of whitespace made one space, cut to 60 characters with "…" in place of
/// the rest.
fn title_stand_in(message: &str) -> String {
    let line = message
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    let words = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() <= STAND_IN_CHARS {
        return words;
    }
    let kept: String = words.chars().take(STAND_IN_CHARS).collect();
    format!("{}…", kept.trim_end())
}

/// How a model choice reads in the picker: its name, then the model it stands for when the
/// name does not say it, as in "Default (recommended) · Fable 5.2".
pub fn model_option_label(option: &ModelOption) -> String {
    match &option.stands_for {
        Some(model) => format!("{} · {model}", option.name),
        None => option.name.clone(),
    }
}

pub fn mode_label(mode: &PermissionMode) -> String {
    match mode {
        PermissionMode::Ask => "Ask before edits".into(),
        PermissionMode::AcceptEdits => "Accept edits".into(),
        PermissionMode::Plan => "Plan".into(),
        PermissionMode::Auto => "Auto".into(),
        PermissionMode::Bypass => "Bypass permissions".into(),
        PermissionMode::Other(text) => text.clone(),
    }
}

pub fn answer_words(answer: &Answer) -> String {
    match answer {
        Answer::Allow { always: false } => "Allowed once".into(),
        Answer::Allow { always: true } => "Always allowed".into(),
        Answer::Deny { .. } => "Denied".into(),
        Answer::Answers(answers) => {
            let words: Vec<&str> = answers
                .iter()
                .flat_map(|a| a.chosen.iter().chain(a.free_text.iter()))
                .map(String::as_str)
                .collect();
            if words.is_empty() {
                "Answered".into()
            } else {
                words.join(", ")
            }
        }
        Answer::ApprovePlan => "Plan approved".into(),
        Answer::KeepPlanning { .. } => "Kept planning".into(),
    }
}

/// Test data shared with other modules' tests in this crate.
#[cfg(test)]
pub(crate) mod testing {
    use mr_agent::{ModelOption, PermissionMode, SessionId, SessionInfo};

    /// What a started session reports: id `s1`, folder `/p`, the Haiku model,
    /// and two models to choose from.
    pub(crate) fn info() -> SessionInfo {
        SessionInfo {
            id: SessionId("s1".into()),
            folder: "/p".into(),
            model: "claude-haiku-5-5".into(),
            permission_mode: PermissionMode::Ask,
            agent_version: "2.1.319".into(),
            models: vec![
                ModelOption {
                    id: "default".into(),
                    name: "Default".into(),
                    resolved: "claude-fable-5-2".into(),
                    stands_for: None,
                    auto_mode: false,
                },
                ModelOption {
                    id: "haiku".into(),
                    name: "Haiku 5.5".into(),
                    resolved: "claude-haiku-5-5".into(),
                    stands_for: None,
                    auto_mode: false,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::info;
    use super::*;
    use crate::conversation::{DID_NOT_FINISH, ItemBody};
    use crate::tools::{ToolCall, ToolGroup};
    use mr_agent::{
        HistoryItem, QuestionAnswer, StoredHistory, StoredSession, ToolCallId, ToolDetail,
        ToolOutcome, TurnSummary, UserRequest, UserRequestKind,
    };

    fn started() -> Session {
        let mut s = Session::opening("/p");
        s.apply(SessionEvent::Started(info()), 0.0);
        s
    }

    fn done() -> SessionEvent {
        SessionEvent::TurnFinished(TurnSummary {
            cost_usd: 0.25,
            context_used: Some(0.5),
            duration_ms: 1,
            interrupted: false,
        })
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

    #[test]
    fn a_message_is_sent_when_idle_and_queued_otherwise() {
        let mut s = Session::opening("/p");
        assert_eq!(s.submit("early"), vec![]);
        assert_eq!(s.queued.as_deref(), Some("early"));
        let sent = s.apply(SessionEvent::Started(info()), 0.0);
        assert_eq!(sent, vec![send("early"), suggest("early")]);
        assert_eq!((s.phase.clone(), s.queued.clone()), (Phase::Working, None));
        assert_eq!(s.submit("  "), vec![]);
        assert_eq!(s.submit("two"), vec![]);
        assert_eq!(s.submit("three"), vec![]);
        assert_eq!(s.queued.as_deref(), Some("two\n\nthree"));
        let sent = s.apply(done(), 0.0);
        assert_eq!(
            sent,
            vec![SessionCommand::Send {
                text: "two\n\nthree".into(),
                attachments: vec![]
            }]
        );
        assert_eq!(s.phase, Phase::Working);
        assert_eq!(s.apply(done(), 0.0), vec![]);
        assert_eq!(s.phase, Phase::Idle);
        assert_eq!((s.cost_usd, s.context_used), (Some(0.25), Some(0.5)));
        let users = s
            .conversation
            .items()
            .iter()
            .filter(|i| matches!(i.body, ItemBody::User { .. }))
            .count();
        assert_eq!(users, 2);
    }

    #[test]
    fn a_message_keeps_its_indentation_and_loses_only_trailing_whitespace() {
        let mut s = started();
        let typed = "    let x = 1;\n  done  \n\n";
        let as_sent = "    let x = 1;\n  done";
        assert_eq!(s.submit(typed)[0], send(as_sent));
        let shown = s.conversation.items().iter().find_map(|i| match &i.body {
            ItemBody::User { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(shown.as_deref(), Some(as_sent));
        assert_eq!(s.submit("  - one\n"), vec![]);
        assert_eq!(s.submit("  - two \t"), vec![]);
        assert_eq!(s.queued.as_deref(), Some("  - one\n\n  - two"));
        assert_eq!(s.submit(" \n\t "), vec![]);
        assert_eq!(s.queued.as_deref(), Some("  - one\n\n  - two"));
    }

    #[test]
    fn typing_while_a_request_waits_never_answers_it() {
        let mut s = started();
        s.submit("go");
        s.apply(permission("r1"), 0.0);
        assert_eq!(s.phase, Phase::NeedsUser);
        assert_eq!(s.submit("yes"), vec![]);
        assert_eq!(s.conversation.waiting(), 1);
        assert_eq!(s.queued.as_deref(), Some("yes"));
        let sent = s.answer(&RequestId("r1".into()), Answer::Allow { always: false });
        assert_eq!(
            sent,
            Some(SessionCommand::Answer {
                id: RequestId("r1".into()),
                answer: Answer::Allow { always: false }
            })
        );
        assert_eq!(s.phase, Phase::Working);
        assert_eq!(
            s.answer(&RequestId("r1".into()), Answer::Deny { message: None }),
            None
        );
    }

    #[test]
    fn a_withdrawn_request_lets_the_turn_carry_on() {
        let mut s = started();
        s.submit("go");
        s.apply(permission("r1"), 0.0);
        s.apply(
            SessionEvent::RequestWithdrawn {
                id: RequestId("r1".into()),
            },
            0.0,
        );
        assert_eq!(s.phase, Phase::Working);
        assert_eq!(
            s.answer(&RequestId("r1".into()), Answer::Allow { always: false }),
            None
        );
    }

    #[test]
    fn nothing_is_sent_to_a_session_that_ended() {
        let mut s = started();
        s.submit("go");
        s.apply(permission("r1"), 0.0);
        s.submit("later");
        s.apply(
            SessionEvent::Exited {
                clean: false,
                detail: "killed".into(),
            },
            0.0,
        );
        assert_eq!(
            s.phase,
            Phase::Ended {
                clean: false,
                detail: "killed".into()
            }
        );
        assert_eq!(
            s.answer(&RequestId("r1".into()), Answer::Allow { always: false }),
            None
        );
        assert_eq!(
            (
                s.submit("x"),
                s.stop(),
                s.choose_model("haiku"),
                s.choose_mode(PermissionMode::Plan)
            ),
            (vec![], None, None, None)
        );
        assert_eq!(s.take_queued().as_deref(), Some("later"));
        let spec = s.start_spec();
        assert_eq!(
            (spec.folder, spec.resume, spec.fork),
            (PathBuf::from("/p"), Some(SessionId("s1".into())), false)
        );
        s.reopening();
        assert_eq!(s.phase, Phase::Opening);
        assert!(!s.conversation.items().is_empty());
    }

    #[test]
    fn stop_is_offered_only_during_a_turn() {
        let mut s = started();
        assert_eq!(s.stop(), None);
        s.submit("go");
        assert_eq!(s.stop(), Some(SessionCommand::Stop));
    }

    #[test]
    fn the_model_and_mode_follow_what_the_agent_confirms() {
        let mut s = started();
        assert_eq!(s.model_label(), "Haiku 5.5");
        assert_eq!(s.choose_model("nope"), None);
        assert_eq!(
            s.choose_model("default"),
            Some(SessionCommand::SetModel("default".into()))
        );
        assert_eq!(s.model_label(), "Haiku 5.5");
        s.apply(SessionEvent::ModelChanged("claude-fable-5-2".into()), 0.0);
        assert_eq!(s.model_label(), "Default");
        s.apply(SessionEvent::ModelChanged("claude-new-9".into()), 0.0);
        assert_eq!(s.model_label(), "claude-new-9");
        assert_eq!(s.choose_mode(PermissionMode::Ask), None);
        assert_eq!(
            s.choose_mode(PermissionMode::Plan),
            Some(SessionCommand::SetPermissionMode(PermissionMode::Plan))
        );
        assert_eq!(s.mode, PermissionMode::Ask);
        s.apply(
            SessionEvent::PermissionModeChanged(PermissionMode::Plan),
            0.0,
        );
        assert_eq!(s.mode, PermissionMode::Plan);
        assert_eq!(Session::opening("/p").model_label(), "Model");
    }

    #[test]
    fn choosing_the_model_already_in_use_sends_nothing() {
        let mut s = started();
        assert_eq!(s.model_label(), "Haiku 5.5");
        assert_eq!(s.choose_model("haiku"), None, "the model in use, as named");
        assert_eq!(
            s.choose_model("default"),
            Some(SessionCommand::SetModel("default".into()))
        );
        s.apply(SessionEvent::ModelChanged("claude-fable-5-2".into()), 0.0);
        assert_eq!(s.choose_model("default"), None);
        // Another option for the same model is another choice: it names the model differently
        // and follows a different setting.
        s.models.push(ModelOption {
            id: "fable".into(),
            name: "Fable 5.2".into(),
            resolved: "claude-fable-5-2".into(),
            stands_for: None,
            auto_mode: false,
        });
        assert_eq!(
            s.choose_model("fable"),
            Some(SessionCommand::SetModel("fable".into()))
        );
        assert_eq!(s.model_label(), "Fable 5.2");
        assert_eq!(s.choose_model("fable"), None);
        assert_eq!(
            s.choose_model("haiku"),
            Some(SessionCommand::SetModel("haiku".into()))
        );
    }

    #[test]
    fn a_choice_that_does_not_name_its_model_is_labelled_with_it() {
        let mut s = started();
        s.models[0].stands_for = Some("Fable 5.2".into());
        assert_eq!(model_option_label(&s.models[0]), "Default · Fable 5.2");
        assert_eq!(model_option_label(&s.models[1]), "Haiku 5.5");
        assert_eq!(s.model_label(), "Haiku 5.5");
        assert_eq!(
            s.choose_model("default"),
            Some(SessionCommand::SetModel("default".into())),
            "chosen by its id, whatever its label"
        );
        s.apply(SessionEvent::ModelChanged("claude-fable-5-2".into()), 0.0);
        assert_eq!(s.model_label(), "Default · Fable 5.2");
    }

    #[test]
    fn auto_mode_is_offered_when_the_model_in_use_supports_it() {
        let mut s = started();
        assert!(!s.auto_mode(), "Haiku 5.5 does not");
        s.models[0].auto_mode = true;
        assert!(
            !s.auto_mode(),
            "only the option that names the model in use counts"
        );
        s.apply(SessionEvent::ModelChanged("claude-fable-5-2".into()), 0.0);
        assert!(s.auto_mode());
        s.apply(SessionEvent::ModelChanged("claude-unknown".into()), 0.0);
        assert!(!s.auto_mode(), "a model no option names");
        assert!(!Session::opening("/p").auto_mode());
    }

    #[test]
    fn a_refused_command_is_explained_in_the_conversation() {
        let mut s = started();
        s.apply(
            SessionEvent::CommandFailed {
                what: CommandKind::SetModel,
                reason: "unknown model".into(),
            },
            0.0,
        );
        s.apply(
            SessionEvent::CommandFailed {
                what: CommandKind::RefreshUsage,
                reason: "x".into(),
            },
            0.0,
        );
        let notices: Vec<_> = s
            .conversation
            .items()
            .iter()
            .filter_map(|i| match &i.body {
                ItemBody::Notice {
                    kind: NoticeKind::CommandFailed,
                    text,
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(notices, vec!["Could not change the model: unknown model"]);
    }

    #[test]
    fn the_wording_is_as_written() {
        assert_eq!(mode_label(&PermissionMode::Ask), "Ask before edits");
        assert_eq!(mode_label(&PermissionMode::Auto), "Auto");
        assert_eq!(mode_label(&PermissionMode::Other("auto".into())), "auto");
        assert_eq!(
            answer_words(&Answer::Allow { always: true }),
            "Always allowed"
        );
        assert_eq!(
            answer_words(&Answer::KeepPlanning { feedback: None }),
            "Kept planning"
        );
        let answers = Answer::Answers(vec![
            QuestionAnswer {
                prompt: "a".into(),
                chosen: vec!["Red".into(), "Blue".into()],
                free_text: None,
            },
            QuestionAnswer {
                prompt: "b".into(),
                chosen: vec![],
                free_text: Some("tall".into()),
            },
        ]);
        assert_eq!(answer_words(&answers), "Red, Blue, tall");
        assert_eq!(answer_words(&Answer::Answers(vec![])), "Answered");
    }

    fn send(text: &str) -> SessionCommand {
        SessionCommand::Send {
            text: text.into(),
            attachments: vec![],
        }
    }

    fn suggest(from: &str) -> SessionCommand {
        SessionCommand::SuggestTitle { from: from.into() }
    }

    #[test]
    fn a_title_stand_in_is_the_first_line_that_is_not_blank() {
        assert_eq!(title_stand_in("Fix the parser"), "Fix the parser");
        assert_eq!(
            title_stand_in("\n   \n\t\n  Fix   the\tparser  \nand the tests"),
            "Fix the parser"
        );
        let sixty = "a".repeat(60);
        assert_eq!(title_stand_in(&sixty), sixty, "60 characters fit whole");
        let long = format!("{sixty}bcd");
        assert_eq!(title_stand_in(&long), format!("{sixty}…"));
        // Whitespace is made one space before the cut, and none is left before the "…".
        let spaced = format!("{}  {}", "a".repeat(59), "b".repeat(10));
        assert_eq!(title_stand_in(&spaced), format!("{}…", "a".repeat(59)));
        // The cut counts characters, not bytes.
        let wide = format!("{}日本語のテキスト", "x".repeat(59));
        assert_eq!(title_stand_in(&wide), format!("{}日…", "x".repeat(59)));
        let accents = "é".repeat(61);
        assert_eq!(title_stand_in(&accents), format!("{}…", "é".repeat(60)));
    }

    #[test]
    fn the_first_message_is_sent_then_a_title_is_asked_for_once() {
        let mut s = started();
        assert_eq!(s.title, None);
        let message = "  Fix the parser\nand the tests";
        assert_eq!(
            s.submit(message),
            vec![send(message), suggest(message)],
            "the Send first, then the request"
        );
        assert_eq!(s.title.as_deref(), Some("Fix the parser"));
        s.apply(done(), 0.0);
        assert_eq!(s.submit("again"), vec![send("again")], "once only");
        s.apply(SessionEvent::TitleChanged("Fixing the parser".into()), 0.0);
        assert_eq!(s.title.as_deref(), Some("Fixing the parser"));
        s.apply(done(), 0.0);
        assert_eq!(s.submit("and again"), vec![send("and again")]);
    }

    #[test]
    fn the_title_request_carries_at_most_two_thousand_characters() {
        let mut s = started();
        let message = "é".repeat(2_500);
        let sent = s.submit(&message);
        assert_eq!(sent[0], send(&message), "the message itself is whole");
        assert_eq!(sent[1], suggest(&"é".repeat(2_000)));
    }

    #[test]
    fn a_queued_first_message_asks_for_the_title_when_it_is_sent() {
        let mut s = Session::opening("/p");
        assert_eq!(s.submit("early"), vec![]);
        assert_eq!(s.title, None, "not while it is only queued");
        let sent = s.apply(SessionEvent::Started(info()), 0.0);
        assert_eq!(sent, vec![send("early"), suggest("early")]);
        assert_eq!(s.title.as_deref(), Some("early"));
        // A queue that waited behind a turn counts the same way: only the first asks.
        s.submit("two");
        assert_eq!(s.apply(done(), 0.0), vec![send("two")]);
    }

    #[test]
    fn a_refused_title_request_changes_nothing() {
        let mut s = started();
        s.submit("Fix the parser");
        let before = s.conversation.items().len();
        s.apply(
            SessionEvent::CommandFailed {
                what: CommandKind::SuggestTitle,
                reason: "no model call allowed".into(),
            },
            0.0,
        );
        assert_eq!(s.conversation.items().len(), before, "no notice");
        assert_eq!(s.title.as_deref(), Some("Fix the parser"));
    }

    #[test]
    fn a_session_that_has_a_title_never_asks_for_one() {
        let mut s = started();
        s.apply(SessionEvent::TitleChanged("Renamed".into()), 0.0);
        assert_eq!(s.submit("go"), vec![send("go")]);
        assert_eq!(s.title.as_deref(), Some("Renamed"));
        // Restarted after it ended: it keeps its title and does not ask either.
        let mut s = started();
        s.submit("go");
        s.apply(
            SessionEvent::Exited {
                clean: false,
                detail: "killed".into(),
            },
            0.0,
        );
        s.reopening();
        s.apply(SessionEvent::Started(info()), 0.0);
        s.apply(done(), 0.0);
        assert_eq!(s.submit("again"), vec![send("again")]);
        assert_eq!(s.title.as_deref(), Some("go"));
    }

    #[test]
    fn a_session_that_ended_before_any_message_asks_after_a_restart() {
        let mut s = started();
        s.apply(
            SessionEvent::Exited {
                clean: false,
                detail: "killed".into(),
            },
            0.0,
        );
        s.reopening();
        s.apply(SessionEvent::Started(info()), 0.0);
        assert_eq!(s.submit("first"), vec![send("first"), suggest("first")]);
    }

    fn stored() -> StoredSession {
        StoredSession {
            id: SessionId("s9".into()),
            folder: "/home/alex/project".into(),
            title: "Fix the parser".into(),
            last_active_unix: 1_700_000_000,
        }
    }

    /// Models known before the session's program runs: Opus, which offers Auto, and `m-2`.
    fn offered() -> Vec<ModelOption> {
        vec![
            ModelOption {
                id: "opus".into(),
                name: "Opus 5.5".into(),
                resolved: "claude-opus-5-5".into(),
                stands_for: None,
                auto_mode: true,
            },
            ModelOption {
                id: "m-2".into(),
                name: "Model 2".into(),
                resolved: "m-2".into(),
                stands_for: None,
                auto_mode: false,
            },
        ]
    }

    fn said(text: &str) -> HistoryItem {
        HistoryItem::User {
            text: text.into(),
            image_count: 0,
        }
    }

    fn clean_exit() -> SessionEvent {
        SessionEvent::Exited {
            clean: true,
            detail: String::new(),
        }
    }

    fn killed() -> SessionEvent {
        SessionEvent::Exited {
            clean: false,
            detail: "killed".into(),
        }
    }

    fn bodies(s: &Session) -> Vec<ItemBody> {
        s.conversation
            .items()
            .iter()
            .map(|i| i.body.clone())
            .collect()
    }

    #[test]
    fn a_stored_session_rests() {
        let s = Session::resting(&stored());
        assert_eq!(s.id, Some(SessionId("s9".into())));
        assert_eq!(s.title.as_deref(), Some("Fix the parser"));
        assert_eq!(s.folder, PathBuf::from("/home/alex/project"));
        assert_eq!(s.phase, Phase::Resting);
        assert_eq!(s.history, HistoryState::NotLoaded);
        assert!(s.conversation.items().is_empty());
        assert!(s.is_stored());
        let mut untitled = stored();
        untitled.title = String::new();
        assert_eq!(
            Session::resting(&untitled).title,
            None,
            "an empty title is none"
        );
        let new = Session::opening("/p");
        assert_eq!(
            new.history,
            HistoryState::Loaded,
            "a new session has no history to read"
        );
        assert!(!new.is_stored());
    }

    #[test]
    fn history_becomes_the_conversation() {
        let mut s = Session::resting(&stored());
        let generation = s.generation;
        let ls = ToolDetail::Command {
            command: "ls".into(),
        };
        s.load_history(StoredHistory {
            items: vec![
                said("one"),
                HistoryItem::AssistantText("Sure.".into()),
                HistoryItem::Tool {
                    id: ToolCallId("t1".into()),
                    name: "Bash".into(),
                    detail: ls.clone(),
                    outcome: None,
                },
                HistoryItem::Compacted,
                said("two"),
            ],
            earlier_left_out: true,
            model: None,
            permission_mode: None,
        });
        let closed = ToolCall {
            id: ToolCallId("t1".into()),
            name: "Bash".into(),
            detail: ls,
            outcome: Some(ToolOutcome::Failed {
                output: DID_NOT_FINISH.into(),
            }),
        };
        assert_eq!(
            bodies(&s),
            vec![
                ItemBody::Notice {
                    kind: NoticeKind::EarlierLeftOut,
                    text: "Earlier messages are not shown.".into()
                },
                ItemBody::User {
                    text: "one".into(),
                    image_count: 0
                },
                ItemBody::Reply {
                    markdown: "Sure.".into(),
                    streaming: false
                },
                ItemBody::Tools(ToolGroup::start(closed)),
                ItemBody::Notice {
                    kind: NoticeKind::Compacted,
                    text: "Earlier messages were summarised here to save room.".into()
                },
                ItemBody::User {
                    text: "two".into(),
                    image_count: 0
                },
            ]
        );
        assert_eq!(s.history, HistoryState::Loaded);
        assert_eq!(s.generation, generation + 1);
        assert_eq!(s.phase, Phase::Resting);
    }

    #[test]
    fn history_names_the_model_and_the_mode() {
        let named = || StoredHistory {
            items: vec![said("one")],
            model: Some("m-2".into()),
            permission_mode: Some(PermissionMode::Plan),
            ..StoredHistory::default()
        };
        let mut s = Session::resting(&stored());
        s.load_history(named());
        assert_eq!(
            (s.model.as_deref(), &s.mode),
            (Some("m-2"), &PermissionMode::Plan)
        );
        // What the user chose while the history was on its way is kept.
        let mut s = Session::resting(&stored());
        s.offer_models(&offered());
        s.history = HistoryState::Loading;
        assert_eq!(s.choose_mode(PermissionMode::AcceptEdits), None);
        assert_eq!(s.choose_model("opus"), None);
        s.load_history(named());
        assert_eq!(
            (s.model.as_deref(), &s.mode),
            (Some("claude-opus-5-5"), &PermissionMode::AcceptEdits)
        );
        assert_eq!(s.model_label(), "Opus 5.5");
    }

    #[test]
    fn a_history_never_turns_permission_checks_off() {
        for mode in [
            PermissionMode::Bypass,
            PermissionMode::Other("dontAsk".into()),
        ] {
            let mut s = Session::resting(&stored());
            s.load_history(StoredHistory {
                items: vec![said("one")],
                permission_mode: Some(mode.clone()),
                ..StoredHistory::default()
            });
            assert_eq!(
                s.mode,
                PermissionMode::Ask,
                "{mode:?} is not taken from a file"
            );
            assert_eq!(s.start_spec().permission_mode, None, "{mode:?}");
        }
        for mode in [
            PermissionMode::Plan,
            PermissionMode::AcceptEdits,
            PermissionMode::Auto,
        ] {
            let mut s = Session::resting(&stored());
            s.load_history(StoredHistory {
                items: vec![said("one")],
                permission_mode: Some(mode.clone()),
                ..StoredHistory::default()
            });
            assert_eq!(s.mode, mode, "{mode:?} is carried");
            assert_eq!(s.start_spec().permission_mode, Some(mode));
        }
    }

    #[test]
    fn history_that_arrives_late_goes_first() {
        let mut s = Session::resting(&stored());
        s.wake("hi");
        s.apply(SessionEvent::Started(info()), 0.0);
        s.apply(SessionEvent::TextDelta("Hello.".into()), 0.0);
        s.apply(done(), 0.0);
        let generation = s.generation;
        s.load_history(StoredHistory {
            items: vec![
                said("before"),
                HistoryItem::AssistantText("Earlier.".into()),
            ],
            model: Some("m-2".into()),
            permission_mode: Some(PermissionMode::Plan),
            earlier_left_out: false,
        });
        assert_eq!(
            bodies(&s),
            vec![
                ItemBody::User {
                    text: "before".into(),
                    image_count: 0
                },
                ItemBody::Reply {
                    markdown: "Earlier.".into(),
                    streaming: false
                },
                ItemBody::User {
                    text: "hi".into(),
                    image_count: 0
                },
                ItemBody::Reply {
                    markdown: "Hello.".into(),
                    streaming: false
                },
            ]
        );
        assert_eq!(s.generation, generation + 1);
        assert_eq!(s.history, HistoryState::Loaded);
        // The running program's model and mode are newer than the history's.
        assert_eq!(
            (s.model.as_deref(), &s.mode),
            (Some("claude-haiku-5-5"), &PermissionMode::Ask)
        );
        // A history read again changes nothing more.
        s.load_history(StoredHistory {
            items: vec![said("again")],
            ..StoredHistory::default()
        });
        assert_eq!(
            (s.conversation.items().len(), s.generation),
            (4, generation + 1)
        );
    }

    #[test]
    fn a_history_that_cannot_be_read_says_why() {
        let mut s = Session::resting(&stored());
        s.history = HistoryState::Loading;
        s.history_failed("i/o error: gone");
        assert_eq!(s.history, HistoryState::Failed("i/o error: gone".into()));
        assert_eq!(s.phase, Phase::Resting);
        assert!(s.conversation.items().is_empty());
    }

    #[test]
    fn the_pickers_work_before_the_program_starts() {
        let mut s = Session::resting(&stored());
        s.offer_models(&offered());
        assert_eq!(s.choose_mode(PermissionMode::Auto), None);
        assert_eq!(s.mode, PermissionMode::Auto);
        assert_eq!(s.choose_model("opus"), None);
        assert_eq!(s.model_label(), "Opus 5.5");
        assert!(s.auto_mode());
        assert_eq!(s.choose_model("nope"), None);
        assert_eq!(s.model_label(), "Opus 5.5");
        assert_eq!(
            s.start_spec(),
            StartSpec {
                folder: "/home/alex/project".into(),
                resume: Some(SessionId("s9".into())),
                fork: false,
                model: Some("opus".into()),
                permission_mode: Some(PermissionMode::Auto),
            }
        );
    }

    #[test]
    fn a_session_with_nothing_stored_starts_afresh() {
        let mut s = Session::opening("/p");
        s.apply(killed(), 0.0);
        let spec = s.start_spec();
        assert_eq!((spec.folder, spec.resume), (PathBuf::from("/p"), None));
        // Started, so it has an id, but it died before any message.
        let mut s = started();
        s.apply(killed(), 0.0);
        let spec = s.start_spec();
        assert_eq!((spec.folder, spec.resume), (PathBuf::from("/p"), None));
        // Its only message could not be handed over.
        let mut s = started();
        s.submit("go");
        s.send_failed();
        s.apply(killed(), 0.0);
        assert_eq!(s.start_spec().resume, None);
    }

    #[test]
    fn a_restart_keeps_the_model_and_the_mode() {
        let mut s = started();
        s.models[0].auto_mode = true;
        s.submit("go");
        s.apply(done(), 0.0);
        assert_eq!(
            s.choose_model("default"),
            Some(SessionCommand::SetModel("default".into()))
        );
        s.apply(SessionEvent::ModelChanged("claude-fable-5-2".into()), 0.0);
        assert_eq!(
            s.choose_mode(PermissionMode::Auto),
            Some(SessionCommand::SetPermissionMode(PermissionMode::Auto))
        );
        s.apply(
            SessionEvent::PermissionModeChanged(PermissionMode::Auto),
            0.0,
        );
        s.apply(killed(), 0.0);
        let spec = s.start_spec();
        assert_eq!(
            (spec.resume, spec.model, spec.permission_mode),
            (
                Some(SessionId("s1".into())),
                Some("default".into()),
                Some(PermissionMode::Auto)
            )
        );
        // Without a choice, the model in use is named by its id; Ask is the program's own
        // default and is not passed.
        let mut s = started();
        s.submit("go");
        s.apply(killed(), 0.0);
        let spec = s.start_spec();
        assert_eq!(
            (spec.model, spec.permission_mode),
            (Some("claude-haiku-5-5".into()), None)
        );
        // A choice the agent refused is not what the session shows, so it is not passed.
        let mut s = started();
        s.submit("go");
        s.apply(done(), 0.0);
        s.choose_model("default");
        s.apply(
            SessionEvent::CommandFailed {
                what: CommandKind::SetModel,
                reason: "unknown model".into(),
            },
            0.0,
        );
        s.apply(killed(), 0.0);
        assert_eq!(s.start_spec().model, Some("claude-haiku-5-5".into()));
        assert_eq!(Session::resting(&stored()).start_spec().model, None);
    }

    #[test]
    fn waking_queues_the_message() {
        let mut s = Session::resting(&stored());
        s.wake("hi");
        assert_eq!(s.phase, Phase::Opening);
        assert_eq!(s.queued.as_deref(), Some("hi"));
        assert_eq!(
            s.apply(SessionEvent::Started(info()), 0.0),
            vec![send("hi")],
            "a stored session has a title, so none is asked for"
        );
        let mut s = Session::resting(&stored());
        s.wake("hi");
        assert_eq!(s.unwake().as_deref(), Some("hi"));
        assert_eq!((s.phase.clone(), s.queued.clone()), (Phase::Resting, None));
        // Nothing is woken by whitespace, and nothing but a waking session is put back to rest.
        s.wake(" \n ");
        assert_eq!(s.phase, Phase::Resting);
        let mut live = started();
        assert_eq!(live.unwake(), None);
        assert_eq!(live.phase, Phase::Idle);
    }

    #[test]
    fn a_resting_session_takes_no_command() {
        let mut s = Session::resting(&stored());
        assert_eq!(s.submit("hi"), vec![], "the app wakes it instead");
        assert_eq!((s.phase.clone(), s.queued.clone()), (Phase::Resting, None));
        assert_eq!(s.stop(), None);
        assert_eq!(
            s.answer(&RequestId("r1".into()), Answer::Allow { always: false }),
            None
        );
        s.apply(SessionEvent::TurnStarted, 0.0);
        assert_eq!(s.phase, Phase::Resting);
    }

    #[test]
    fn a_clean_end_puts_a_stored_session_to_rest() {
        let mut s = started();
        s.submit("go");
        s.apply(done(), 0.0);
        let items = s.conversation.items().len();
        s.apply(clean_exit(), 0.0);
        assert_eq!(s.phase, Phase::Resting);
        assert_eq!(s.conversation.items().len(), items, "no notice");
        let mut s = started();
        s.apply(clean_exit(), 0.0);
        assert_eq!(
            s.phase,
            Phase::Ended {
                clean: true,
                detail: String::new()
            }
        );
        for mut s in [started(), Session::resting(&stored())] {
            if s.phase == Phase::Resting {
                s.wake("go");
                s.apply(SessionEvent::Started(info()), 0.0);
            } else {
                s.submit("go");
            }
            s.apply(killed(), 0.0);
            assert!(matches!(s.phase, Phase::Ended { clean: false, .. }));
        }
        // A stored session that was woken rests again after a clean end.
        let mut s = Session::resting(&stored());
        s.wake("go");
        s.apply(SessionEvent::Started(info()), 0.0);
        s.apply(done(), 0.0);
        s.apply(clean_exit(), 0.0);
        assert_eq!(s.phase, Phase::Resting);
    }

    #[test]
    fn a_failed_message_returns_to_idle() {
        let mut s = started();
        s.submit("go");
        assert_eq!(s.phase, Phase::Working);
        s.send_failed();
        assert_eq!(s.phase, Phase::Idle);
        assert_eq!(
            bodies(&s),
            vec![ItemBody::User {
                text: "go".into(),
                image_count: 0
            }]
        );
        let mut s = started();
        s.send_failed();
        assert_eq!(s.phase, Phase::Idle);
    }

    #[test]
    fn offered_models_do_not_replace_the_sessions_own() {
        let mut s = started();
        let own = s.models.clone();
        s.offer_models(&offered());
        assert_eq!(s.models, own);
        let mut r = Session::resting(&stored());
        r.offer_models(&offered());
        assert_eq!(r.models, offered());
        // A newer list replaces one that was only offered.
        let newer = offered()[..1].to_vec();
        r.offer_models(&newer);
        assert_eq!(r.models, newer);
        r.wake("hi");
        r.apply(SessionEvent::Started(info()), 0.0);
        r.offer_models(&offered());
        assert_eq!(r.models, info().models);
    }
}
