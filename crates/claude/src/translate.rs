//! Turns the stream of parsed `claude` output into the agent-neutral
//! [`SessionEvent`]s the UI consumes.
//!
//! [`Translator`] is stateful: it remembers whether a turn is open, which kind
//! of block is open, and the last known model, permission mode and title. Each message
//! kind has its own method, so a new rule is a new arm in [`Translator::push`]
//! plus one method.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use mr_agent::{
    AgentError, Answer, CommandKind, ContextBreakdown, ContextKind, ContextPart, LimitWindow,
    ModelOption, PermissionMode, Question, QuestionOption, RemoteAccess, RemoteFailure, RequestId,
    SessionEvent, SessionId, SessionInfo, ToolCallId, ToolDetail, ToolOutcome, TurnSummary,
    UsageLimits, UserRequest, UserRequestKind,
};
use serde_json::{Value, json};

use crate::encode::{PermissionReply, get_context_usage, get_settings};
use crate::mode::mode_from_wire;
use crate::tasks::{self, Hidden};
use crate::time::iso8601_to_unix;
use crate::wire::{Block, BlockKind, Incoming, StreamEvent, ToolResult};

/// What a control request this adapter sends asks for, so its reply can be
/// turned into an event. See [`Translator::track`].
#[derive(Clone, Debug, PartialEq)]
pub enum Tracked {
    /// The `initialize` request; its reply starts the session when the
    /// translator knows the session (see [`Translator::for_session`]).
    Initialize,
    RemoteOn,
    RemoteOff,
    SetModel(String),
    SetMode(PermissionMode),
    Rename(String),
    /// The `generate_session_title` request; its reply may name the session.
    SuggestTitle,
    /// The `get_usage` request; its reply gives the usage limits.
    Usage,
    /// The `apply_flag_settings` request that sets this effort level.
    SetEffort(String),
    /// The `get_settings` request; its reply gives the effort level in use.
    /// `asked` is the level a `SetEffort` asked for, when this read follows one.
    Settings {
        asked: Option<String>,
    },
    /// The `get_context_usage` request; its reply says what the context is made of.
    ContextUsage,
    Other,
}

/// What the adapter knows about a session before `claude` reports anything:
/// the id it chose or resumed, the folder, and what the user asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionStart {
    pub id: SessionId,
    pub folder: PathBuf,
    /// The model asked for, such as the alias `haiku`; `None` for the default.
    pub model: Option<String>,
    pub permission_mode: Option<PermissionMode>,
}

/// Translates one session's output, message by message. It never writes: a request it
/// wants sent waits in a list, which [`Translator::take_requests`] hands over.
#[derive(Debug, Default)]
pub struct Translator {
    /// The session as known before any output, for `Started` at the reply to
    /// `initialize`. Without it, the first `Init` gives `Started`.
    start: Option<SessionStart>,
    /// Whether `Started` has been given.
    started: bool,
    /// The last model `Init` named. Shared with the rules that report model changes.
    model: Option<String>,
    /// The last known permission mode. Shared by the `Init` and `ModeChanged` rules.
    mode: Option<PermissionMode>,
    /// The last title reported. `Init` does not name one.
    title: Option<String>,
    /// The effort level last given, once it has been read: `None` until the first reading.
    effort: Option<Option<String>>,
    /// The level the latest `SetEffort` asked for. Only the reading that follows it reports a
    /// level not taken.
    effort_asked: Option<String>,
    /// The `session_url` of the most recent successful `RemoteOn` reply,
    /// forgotten when Remote Control is turned off or fails to turn on.
    remote_url: Option<String>,
    /// Control requests sent and not yet answered, by `request_id`.
    tracked: HashMap<String, Tracked>,
    turn_open: bool,
    /// The kind of the block a `BlockStart` opened and a `BlockStop` has not closed.
    open_block: Option<BlockKind>,
    /// Whether a non-empty delta was passed on for the open block.
    block_has_text: bool,
    /// Whether the current top-level message is being streamed: set at its
    /// `MessageStart`, cleared at its `MessageStop` and at `Result`. While it
    /// is set, an `Assistant` record's text and thinking were already given
    /// as deltas.
    streaming: bool,
    /// `usage_tokens` of the latest top-level `Assistant` record that is a reply with usage.
    last_usage_tokens: Option<u64>,
    /// The model of that record.
    last_usage_model: Option<String>,
    /// Requests raised and not yet answered or withdrawn, by `request_id`.
    pending: HashMap<String, PendingRequest>,
    /// Calls to the tools that are requests to the user (see [`REQUEST_TOOLS`]) whose result
    /// has not come yet, by `tool_use` id. They are not reported as tool calls, nor are their
    /// results.
    request_calls: HashSet<String>,
    /// Calls of the main conversation that are not shown as tool calls (see [`tasks::hidden`])
    /// and whose result has not come yet, by `tool_use` id, with what each does. Dropped at a
    /// turn's `result`: a call never answered does nothing.
    hidden_calls: HashMap<String, Hidden>,
    /// The requests to send next, in order. See [`Translator::take_requests`].
    requests: Vec<(Tracked, Value)>,
    /// The background tasks last counted: by the program's own record, or as at least one
    /// since a tool call ran in the background (see [`Translator::background_started`]). A
    /// translator serves one program, so the next program starts with none.
    background: usize,
}

/// The model a record names when the program made it itself rather than a model's reply.
const SYNTHETIC_MODEL: &str = "<synthetic>";

/// The tools `claude` asks the user through: it raises a request for each call, and the
/// request is what the user sees and answers, so the call itself is not work to report.
const REQUEST_TOOLS: [&str; 2] = ["AskUserQuestion", "ExitPlanMode"];

/// What a raised request needs to remember to build its reply.
#[derive(Debug)]
struct PendingRequest {
    kind: PendingKind,
    /// The request's `input`, always a JSON object.
    input: Value,
    /// What always-allow sends back: the first suggestion alone, and only when
    /// it has a label. Decided when the request is raised, so the label shown
    /// and the permission granted cannot differ.
    always_allow: Option<Value>,
}

#[derive(Debug, PartialEq)]
enum PendingKind {
    Permission,
    Question,
    Plan,
}

impl Translator {
    /// A translator that knows nothing in advance: the first `Init` gives
    /// `Started`, with no models.
    pub fn new() -> Self {
        Self::default()
    }

    /// A translator for a session the adapter opened: the reply to the request
    /// tracked as [`Tracked::Initialize`] gives `Started`, before any turn, and
    /// the first `Init` then reports only what differs from it.
    pub fn for_session(start: SessionStart) -> Self {
        Self {
            start: Some(start),
            ..Self::default()
        }
    }

    /// Records what a control request asks for. Call it before sending the
    /// request, so its reply cannot arrive first.
    pub fn track(&mut self, request_id: &str, what: Tracked) {
        if let Tracked::SetEffort(level) = &what {
            self.effort_asked = Some(level.clone());
        }
        self.tracked.insert(request_id.to_string(), what);
    }

    /// The requests the translator wants sent next, in order. Emptied by the call.
    pub fn take_requests(&mut self) -> Vec<(Tracked, Value)> {
        std::mem::take(&mut self.requests)
    }

    /// Feeds one parsed line and returns the events it gives, in order.
    pub fn push(&mut self, message: Incoming) -> Vec<SessionEvent> {
        match message {
            Incoming::Init {
                session_id,
                model,
                permission_mode,
                version,
                cwd,
            } => self.init(session_id, model, permission_mode, version, cwd),
            Incoming::ModeChanged { permission_mode } => self.mode_changed(&permission_mode),
            Incoming::Stream {
                parent: None,
                event,
            } => self.stream(event),
            Incoming::Assistant {
                parent: None,
                blocks,
                model,
                usage_tokens,
                ..
            } => self.assistant(blocks, model, usage_tokens),
            Incoming::ToolResults {
                parent: None,
                results,
            } => results
                .into_iter()
                .filter(|result| !self.request_calls.remove(&result.tool_use_id))
                .filter_map(
                    |result| match self.hidden_calls.remove(&result.tool_use_id) {
                        Some(hidden) => tasks::finished(
                            hidden,
                            result.is_error,
                            &result.structured,
                            &result.text,
                        ),
                        None => Some(tool_finished(result)),
                    },
                )
                .collect(),
            Incoming::ControlRequest {
                request_id,
                subtype,
                body,
            } if subtype == "can_use_tool" => self.can_use_tool(request_id, &body),
            Incoming::ControlCancel { request_id } => self.cancelled(&request_id),
            Incoming::RateLimit {
                five_hour,
                seven_day,
            } => vec![SessionEvent::UsageLimits(UsageLimits {
                session: five_hour.map(limit_window),
                weekly: seven_day.map(limit_window),
            })],
            Incoming::BridgeState {
                state,
                detail,
                failure_kind,
            } => self.bridge_state(&state, detail, failure_kind.as_deref()),
            Incoming::TitleChanged { title } => self.title_changed(title).into_iter().collect(),
            Incoming::BackgroundTasks { tasks } => {
                self.background = background_tasks(&tasks);
                vec![SessionEvent::BackgroundTasks {
                    running: self.background,
                }]
            }
            // A sub-agent's work is not reported, but a task it leaves running is the program's.
            Incoming::Assistant {
                parent: Some(_),
                blocks,
                ..
            } => blocks
                .iter()
                .filter(
                    |block| matches!(block, Block::ToolUse { input, .. } if in_background(input)),
                )
                .map(|_| self.background_started())
                .collect(),
            Incoming::ControlResponse {
                request_id,
                ok,
                body,
                error,
                error_code,
            } => self.control_response(&request_id, ok, &body, error, error_code.as_deref()),
            Incoming::Result {
                is_error,
                interrupted,
                text,
                cost_usd,
                duration_ms,
                context_windows,
            } => self.result(
                is_error,
                interrupted,
                text,
                cost_usd,
                duration_ms,
                &context_windows,
            ),
            // Sub-agent records (`parent` set) and the kinds later rules cover.
            Incoming::Stream { .. }
            | Incoming::ToolResults { .. }
            | Incoming::ControlRequest { .. }
            | Incoming::Ignored { .. } => Vec::new(),
        }
    }

    fn init(
        &mut self,
        session_id: String,
        model: String,
        permission_mode: String,
        version: String,
        cwd: String,
    ) -> Vec<SessionEvent> {
        let mode = mode_from_wire(&permission_mode);
        if !self.started {
            self.started = true;
            self.model = Some(model.clone());
            self.mode = Some(mode.clone());
            self.read_after_start_or_turn();
            return vec![SessionEvent::Started(SessionInfo {
                id: SessionId(session_id),
                folder: cwd.into(),
                model,
                permission_mode: mode,
                agent_version: version,
                models: Vec::new(),
            })];
        }
        let mut events: Vec<SessionEvent> = self.model_changed_to(model).into_iter().collect();
        events.extend(self.mode_changed_to(mode));
        events
    }

    fn mode_changed(&mut self, permission_mode: &str) -> Vec<SessionEvent> {
        self.mode_changed_to(mode_from_wire(permission_mode))
            .into_iter()
            .collect()
    }

    fn mode_changed_to(&mut self, mode: PermissionMode) -> Option<SessionEvent> {
        if self.mode.as_ref() == Some(&mode) {
            return None;
        }
        self.mode = Some(mode.clone());
        Some(SessionEvent::PermissionModeChanged(mode))
    }

    fn model_changed_to(&mut self, model: String) -> Option<SessionEvent> {
        if self.model.as_deref() == Some(model.as_str()) {
            return None;
        }
        self.model = Some(model.clone());
        Some(SessionEvent::ModelChanged(model))
    }

    fn title_changed(&mut self, title: String) -> Option<SessionEvent> {
        if self.title.as_deref() == Some(title.as_str()) {
            return None;
        }
        self.title = Some(title.clone());
        Some(SessionEvent::TitleChanged(title))
    }

    /// There is no "stopped" state: turning Remote Control off gives only the reply.
    fn bridge_state(
        &mut self,
        state: &str,
        detail: Option<String>,
        failure_kind: Option<&str>,
    ) -> Vec<SessionEvent> {
        let access = match state {
            "failed" => RemoteAccess::Failed {
                reason: detail.unwrap_or_else(|| "Remote Control failed".into()),
                why: match failure_kind {
                    Some("auth") => RemoteFailure::Login,
                    Some("ended_elsewhere") => RemoteFailure::Elsewhere,
                    _ => RemoteFailure::Other,
                },
            },
            "policy_disabled" => RemoteAccess::Failed {
                reason: detail.unwrap_or_else(|| "Remote Control is turned off by policy".into()),
                why: RemoteFailure::Policy,
            },
            "ready" => RemoteAccess::Starting,
            "reconnecting" => RemoteAccess::Reconnecting,
            // After a drop it comes back with `recovered`, at the same address.
            "connected" => RemoteAccess::On {
                url: self.remote_url.clone(),
            },
            _ => return Vec::new(),
        };
        vec![SessionEvent::RemoteAccess(access)]
    }

    /// The event for the reply to a request that was tracked, which is then forgotten.
    fn control_response(
        &mut self,
        request_id: &str,
        ok: bool,
        body: &Value,
        error: Option<String>,
        error_code: Option<&str>,
    ) -> Vec<SessionEvent> {
        let Some(tracked) = self.tracked.remove(request_id) else {
            return Vec::new();
        };
        let reason = || {
            error
                .clone()
                .unwrap_or_else(|| "The request was refused".into())
        };
        let event = match (tracked, ok) {
            (Tracked::Other, _) | (Tracked::Initialize, false) => None,
            (Tracked::ContextUsage, true) => context_breakdown(body),
            // Asked for in the background: the breakdown shown stays.
            (Tracked::ContextUsage, false) => None,
            (Tracked::Initialize, true) => self.initialized(body),
            (Tracked::RemoteOn, true) => {
                self.remote_url = body
                    .get("session_url")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                Some(SessionEvent::RemoteAccess(RemoteAccess::Starting))
            }
            (Tracked::RemoteOn, false) => {
                self.remote_url = None;
                Some(SessionEvent::RemoteAccess(RemoteAccess::Failed {
                    reason: error
                        .clone()
                        .unwrap_or_else(|| "Remote Control failed".into()),
                    why: match error_code {
                        Some("remote_control_auth") => RemoteFailure::Login,
                        Some("remote_control_policy_disabled") => RemoteFailure::Policy,
                        Some("remote_control_ended_elsewhere") => RemoteFailure::Elsewhere,
                        _ => RemoteFailure::Other,
                    },
                }))
            }
            (Tracked::RemoteOff, true) => {
                self.remote_url = None;
                Some(SessionEvent::RemoteAccess(RemoteAccess::Off))
            }
            (Tracked::SetModel(asked), true) => {
                let model = body
                    .get("model")
                    .and_then(Value::as_str)
                    .map_or(asked, str::to_string);
                // A model can go on with another level than the last one.
                self.read_effort(None);
                self.model_changed_to(model)
            }
            (Tracked::SetMode(mode), true) => self.mode_changed_to(mode),
            (Tracked::Rename(title), true) => self.title_changed(title),
            // Read from the program, not yet seen on the wire (see `generate_session_title`).
            (Tracked::SuggestTitle, true) => body
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .and_then(|title| self.title_changed(title.to_string())),
            (Tracked::Usage, true) => Some(usage_limits(body)),
            (Tracked::RemoteOff, false) => {
                Some(command_failed(CommandKind::SetRemoteAccess, reason()))
            }
            (Tracked::SetModel(_), false) => Some(command_failed(CommandKind::SetModel, reason())),
            (Tracked::SetMode(_), false) => {
                Some(command_failed(CommandKind::SetPermissionMode, reason()))
            }
            (Tracked::Rename(_), false) => Some(command_failed(CommandKind::Rename, reason())),
            (Tracked::SuggestTitle, false) => {
                Some(command_failed(CommandKind::SuggestTitle, reason()))
            }
            (Tracked::Usage, false) => Some(command_failed(CommandKind::RefreshUsage, reason())),
            // The reply confirms nothing: a level not taken is a success too.
            (Tracked::SetEffort(level), true) => {
                self.read_effort(Some(level));
                None
            }
            (Tracked::SetEffort(_), false) => {
                Some(command_failed(CommandKind::SetEffort, reason()))
            }
            // A reading can give two events.
            (Tracked::Settings { asked }, ok) => {
                return self.effort_read(asked, ok, body, reason());
            }
        };
        event.into_iter().collect()
    }

    /// Queues what is read once the session has started and after every turn: the effort level
    /// in use, then what the context is made of.
    fn read_after_start_or_turn(&mut self) {
        self.read_effort(None);
        self.requests
            .push((Tracked::ContextUsage, get_context_usage()));
    }

    /// Queues a reading of the effort level in use, after a change to `asked` when there is one.
    fn read_effort(&mut self, asked: Option<String>) {
        self.requests
            .push((Tracked::Settings { asked }, get_settings()));
    }

    /// The events for a reply to `get_settings`. The reply holds the user's settings; only
    /// `applied.effort` is read from it. A reading after a change that a later one replaced
    /// reports no failure: the later reading does.
    fn effort_read(
        &mut self,
        asked: Option<String>,
        ok: bool,
        body: &Value,
        reason: String,
    ) -> Vec<SessionEvent> {
        let asked = asked.filter(|asked| self.effort_asked.as_ref() == Some(asked));
        if !ok {
            return asked
                .map(|_| command_failed(CommandKind::SetEffort, reason))
                .into_iter()
                .collect();
        }
        let level = body
            .get("applied")
            .and_then(|applied| applied.get("effort"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut events = Vec::new();
        if let Some(asked) = asked
            && level.as_deref() != Some(asked.as_str())
        {
            let reason = match &level {
                Some(level) => format!("Claude Code goes on with {level}"),
                None => "Claude Code goes on with no effort level".to_string(),
            };
            events.push(command_failed(CommandKind::SetEffort, reason));
        }
        if self.effort.as_ref() != Some(&level) {
            self.effort = Some(level.clone());
            events.push(SessionEvent::EffortChanged(level));
        }
        events
    }

    /// `Started` from a successful reply to `initialize`, once, and only when
    /// the session is known in advance.
    fn initialized(&mut self, body: &Value) -> Option<SessionEvent> {
        if self.started {
            return None;
        }
        let start = self.start.clone()?;
        let text = |key: &str| body.get(key).and_then(Value::as_str);
        let models = model_options(body.get("models").and_then(Value::as_array));
        let wanted = start.model.as_deref().unwrap_or("default");
        let model = models
            .iter()
            .find(|option| option.id == wanted && !option.resolved.is_empty())
            .map_or_else(
                || start.model.clone().unwrap_or_default(),
                |option| option.resolved.clone(),
            );
        let permission_mode = text("current_permission_mode")
            .map(mode_from_wire)
            .or(start.permission_mode)
            .unwrap_or(PermissionMode::Ask);
        self.started = true;
        self.model = Some(model.clone());
        self.mode = Some(permission_mode.clone());
        self.read_after_start_or_turn();
        Some(SessionEvent::Started(SessionInfo {
            id: start.id,
            folder: start.folder,
            model,
            permission_mode,
            agent_version: text("claude_code_version").unwrap_or_default().to_string(),
            models,
        }))
    }

    fn stream(&mut self, event: StreamEvent) -> Vec<SessionEvent> {
        match event {
            StreamEvent::MessageStart => {
                self.streaming = true;
                if self.turn_open {
                    Vec::new()
                } else {
                    self.turn_open = true;
                    vec![SessionEvent::TurnStarted]
                }
            }
            StreamEvent::BlockStart(kind) => {
                let thinking = kind == BlockKind::Thinking;
                self.open_block = Some(kind);
                self.block_has_text = false;
                if thinking {
                    vec![SessionEvent::ThinkingStarted]
                } else {
                    Vec::new()
                }
            }
            StreamEvent::TextDelta(text) => self.delta(text, SessionEvent::TextDelta),
            StreamEvent::ThinkingDelta(text) => self.delta(text, SessionEvent::ThinkingDelta),
            StreamEvent::BlockStop => {
                // A thinking phase is always announced, so it always ends;
                // a text block is shown only once it has text.
                let shown = match self.open_block {
                    Some(BlockKind::Thinking) => true,
                    Some(BlockKind::Text) => self.block_has_text,
                    _ => false,
                };
                self.open_block = None;
                self.block_has_text = false;
                if shown {
                    vec![SessionEvent::BlockFinished]
                } else {
                    Vec::new()
                }
            }
            StreamEvent::MessageStop => {
                self.streaming = false;
                Vec::new()
            }
            StreamEvent::Other => Vec::new(),
        }
    }

    fn delta(&mut self, text: String, make: fn(String) -> SessionEvent) -> Vec<SessionEvent> {
        if text.is_empty() {
            return Vec::new();
        }
        self.block_has_text = true;
        vec![make(text)]
    }

    fn assistant(
        &mut self,
        blocks: Vec<Block>,
        model: Option<String>,
        usage_tokens: Option<u64>,
    ) -> Vec<SessionEvent> {
        let streamed = self.streaming;
        // Only a reply counts: not a record without usage, nor one the program made itself.
        if usage_tokens.is_some() && model.as_deref() != Some(SYNTHETIC_MODEL) {
            self.last_usage_tokens = usage_tokens;
            self.last_usage_model = model;
        }
        let mut events = Vec::new();
        for block in blocks {
            match block {
                Block::ToolUse { id, name, .. } if REQUEST_TOOLS.contains(&name.as_str()) => {
                    self.request_calls.insert(id);
                }
                Block::ToolUse { id, name, input } => {
                    if let Some(hidden) = tasks::hidden(&name, &input) {
                        self.hidden_calls.insert(id, hidden);
                        continue;
                    }
                    let detail = tool_detail(&name, &input);
                    events.push(SessionEvent::ToolStarted {
                        id: ToolCallId(id),
                        name,
                        detail,
                    });
                    if in_background(&input) {
                        events.push(self.background_started());
                    }
                }
                Block::Text(text) if !streamed && !text.is_empty() => {
                    events.push(SessionEvent::TextDelta(text));
                    events.push(SessionEvent::BlockFinished);
                }
                Block::Thinking(text) if !streamed => {
                    events.push(SessionEvent::ThinkingStarted);
                    if !text.is_empty() {
                        events.push(SessionEvent::ThinkingDelta(text));
                    }
                    events.push(SessionEvent::BlockFinished);
                }
                Block::Text(_) | Block::Thinking(_) | Block::Other => {}
            }
        }
        events
    }

    /// A tool call was run in the background: from now on at least one task counts as running,
    /// until a record of the program's tasks arrives and decides. A second guard beside that
    /// record, which was never seen with a task in it: should none come when a task starts, a
    /// program that started one is still not taken for idle.
    fn background_started(&mut self) -> SessionEvent {
        self.background = self.background.max(1);
        SessionEvent::BackgroundTasks {
            running: self.background,
        }
    }

    fn can_use_tool(&mut self, request_id: String, body: &Value) -> Vec<SessionEvent> {
        // A request that cannot be understood still reaches the user: `claude` waits for a reply.
        let tool = body
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown tool");
        let input = match body.get("input") {
            Some(input) if input.is_object() => input.clone(),
            _ => json!({}),
        };
        let suggestions = body
            .get("permission_suggestions")
            .cloned()
            .unwrap_or(Value::Null);
        let label = always_allow_label(&suggestions);
        let always_allow = label
            .as_ref()
            .and_then(|_| suggestions.get(0))
            .map(|first| json!([first]));
        let (pending_kind, kind) = match tool {
            "AskUserQuestion" => (
                PendingKind::Question,
                UserRequestKind::Question {
                    questions: questions(&input),
                },
            ),
            "ExitPlanMode" => (
                PendingKind::Plan,
                UserRequestKind::PlanApproval {
                    plan_markdown: input
                        .get("plan")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                },
            ),
            _ => (
                PendingKind::Permission,
                UserRequestKind::Permission {
                    tool: tool.to_string(),
                    detail: tool_detail(tool, &input),
                    always_allow_label: label,
                },
            ),
        };
        self.pending.insert(
            request_id.clone(),
            PendingRequest {
                kind: pending_kind,
                input,
                always_allow,
            },
        );
        vec![SessionEvent::NeedsUser(UserRequest {
            id: RequestId(request_id),
            kind,
        })]
    }

    fn cancelled(&mut self, request_id: &str) -> Vec<SessionEvent> {
        match self.pending.remove(request_id) {
            Some(_) => vec![SessionEvent::RequestWithdrawn {
                id: RequestId(request_id.to_string()),
            }],
            None => Vec::new(),
        }
    }

    /// Builds the reply to a raised request from the user's answer. The request
    /// is forgotten only when the answer fits it; a mismatch leaves it pending.
    pub fn answer(
        &mut self,
        id: &RequestId,
        answer: Answer,
    ) -> Result<PermissionReply, AgentError> {
        let pending = self
            .pending
            .get(&id.0)
            .ok_or_else(|| AgentError::Protocol(format!("no pending request {}", id.0)))?;
        let reply = match (&pending.kind, answer) {
            (PendingKind::Permission, Answer::Allow { always }) => {
                let updated_permissions = pending.always_allow.clone().filter(|_| always);
                PermissionReply::Allow {
                    updated_input: pending.input.clone(),
                    updated_permissions,
                }
            }
            (PendingKind::Permission, Answer::Deny { message }) => PermissionReply::Deny {
                message: message.unwrap_or_else(|| "Denied by the user".into()),
            },
            (PendingKind::Question, Answer::Answers(list)) => {
                let answers: serde_json::Map<String, Value> = list
                    .into_iter()
                    .map(|a| {
                        // The options chosen, then the user's own words, joined as the
                        // window shows the answer once it is sent.
                        let words: Vec<String> = a.chosen.into_iter().chain(a.free_text).collect();
                        (a.prompt, Value::String(words.join(", ")))
                    })
                    .collect();
                let mut updated_input = pending.input.clone();
                updated_input["answers"] = Value::Object(answers);
                PermissionReply::Allow {
                    updated_input,
                    updated_permissions: None,
                }
            }
            (PendingKind::Plan, Answer::ApprovePlan) => PermissionReply::Allow {
                updated_input: pending.input.clone(),
                updated_permissions: None,
            },
            (PendingKind::Plan, Answer::KeepPlanning { feedback }) => PermissionReply::Deny {
                message: feedback.unwrap_or_else(|| "Keep planning".into()),
            },
            (kind, answer) => {
                return Err(AgentError::Protocol(format!(
                    "{answer:?} does not fit the {kind:?} request {}",
                    id.0
                )));
            }
        };
        self.pending.remove(&id.0);
        Ok(reply)
    }

    fn result(
        &mut self,
        is_error: bool,
        interrupted: bool,
        text: String,
        cost_usd: f64,
        duration_ms: u64,
        context_windows: &BTreeMap<String, u64>,
    ) -> Vec<SessionEvent> {
        self.turn_open = false;
        self.streaming = false;
        self.hidden_calls.clear();
        self.open_block = None;
        self.block_has_text = false;
        self.read_after_start_or_turn();
        // A stopped turn is reported as an error by `claude`, but is not a failure.
        if is_error && !interrupted {
            return vec![SessionEvent::TurnFailed { reason: text }];
        }
        // The window of the last message's model, else the largest one named.
        let window = self
            .last_usage_model
            .as_ref()
            .and_then(|model| context_windows.get(model))
            .or_else(|| context_windows.values().max())
            .copied();
        let context_used = match (self.last_usage_tokens, window) {
            (Some(used), Some(window)) if window > 0 => {
                Some(((used as f64 / window as f64) as f32).min(1.0))
            }
            _ => None,
        };
        vec![SessionEvent::TurnFinished(TurnSummary {
            cost_usd,
            context_used,
            duration_ms,
            interrupted,
        })]
    }
}

/// One entry of the `initialize` reply's `models`. An entry without a `value`
/// cannot be asked for, so it is skipped. A missing name is the `value`.
/// The entries of a `models` list that have a `value`, each with the name of the model it
/// stands for when its own name does not say it. That name is, in this order: the name of
/// another entry for the model this one resolves to, when the two names differ; for the
/// `default` entry alone, its description (the recorded list describes "Default
/// (recommended)" as "Fable 5.2"); otherwise none.
fn model_options(entries: Option<&Vec<Value>>) -> Vec<ModelOption> {
    let parsed: Vec<(ModelOption, Option<&str>)> = entries
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let text = |key: &str| entry.get(key).and_then(Value::as_str);
            let id = text("value")?;
            let option = ModelOption {
                id: id.to_string(),
                name: text("displayName").unwrap_or(id).to_string(),
                resolved: text("resolvedModel").unwrap_or_default().to_string(),
                stands_for: None,
                auto_mode: entry
                    .get("supportsAutoMode")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                effort_levels: effort_levels(entry),
            };
            Some((option, text("description")))
        })
        .collect();
    parsed
        .iter()
        .enumerate()
        .map(|(i, (option, description))| {
            let named_elsewhere = parsed
                .iter()
                .enumerate()
                .find(|(j, (other, _))| {
                    *j != i && other.id == option.resolved && other.name != option.name
                })
                .map(|(_, (other, _))| other.name.clone());
            let described = || {
                (option.id == "default")
                    .then_some(*description)
                    .flatten()
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_string)
            };
            ModelOption {
                stands_for: named_elsewhere.or_else(described),
                ..option.clone()
            }
        })
        .collect()
}

/// The levels a `models` entry lists in `supportedEffortLevels`, in its order, when its
/// `supportsEffort` is true; none otherwise.
fn effort_levels(entry: &Value) -> Vec<String> {
    if entry.get("supportsEffort").and_then(Value::as_bool) != Some(true) {
        return Vec::new();
    }
    entry
        .get("supportedEffortLevels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn limit_window((used, resets_at_unix): (f32, i64)) -> LimitWindow {
    LimitWindow {
        used,
        resets_at_unix,
    }
}

/// The limits in a `get_usage` reply. Unlike the per-turn event, `utilization`
/// is a percentage and `resets_at` is ISO 8601 text. A window missing either
/// value is not known, which is not the same as unused.
fn usage_limits(body: &Value) -> SessionEvent {
    let window = |key: &str| {
        let window = body.get("rate_limits")?.get(key)?;
        let percent = window.get("utilization")?.as_f64()?;
        let resets_at_unix = iso8601_to_unix(window.get("resets_at")?.as_str()?)?;
        Some(LimitWindow {
            used: (percent as f32 / 100.0).clamp(0.0, 1.0),
            resets_at_unix,
        })
    };
    SessionEvent::UsageLimits(UsageLimits {
        session: window("five_hour"),
        weekly: window("seven_day"),
    })
}

/// The breakdown in a reply to `get_context_usage`: its `categories` of the kinds `used`,
/// `buffer` and `free`, in its order, each with a `name` and whole `tokens`, and its
/// `maxTokens` as the window. Rows of the kind `deferred` are kept out of the window by the
/// program and are left out, as are rows of a kind not known. Nothing when the window is not
/// above nothing or no row is left. Nothing else of the reply is read: it also names memory
/// files, tools and skills.
fn context_breakdown(body: &Value) -> Option<SessionEvent> {
    let window = body
        .get("maxTokens")?
        .as_u64()
        .filter(|window| *window > 0)?;
    let parts: Vec<ContextPart> = body
        .get("categories")?
        .as_array()?
        .iter()
        .filter_map(|row| {
            let kind = match row.get("kind")?.as_str()? {
                "used" => ContextKind::Used,
                "buffer" => ContextKind::Buffer,
                "free" => ContextKind::Free,
                _ => return None,
            };
            Some(ContextPart {
                name: row.get("name")?.as_str()?.to_string(),
                tokens: row.get("tokens")?.as_u64()?,
                kind,
            })
        })
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(SessionEvent::ContextBreakdown(ContextBreakdown {
        parts,
        window,
    }))
}

/// Counts the tasks that need the program to stay up: every entry but those that run off this
/// computer. A list that cannot be read, and an entry that is not an object, count as a task:
/// a program is never stopped because its record could not be read.
fn background_tasks(tasks: &Value) -> usize {
    match tasks.as_array() {
        Some(list) => list
            .iter()
            .filter(|task| task.get("off_host").and_then(Value::as_bool) != Some(true))
            .count(),
        None => 1,
    }
}

/// Whether a tool call's input asks for the tool to run in the background: the field
/// `run_in_background`, set to `true`.
fn in_background(input: &Value) -> bool {
    input.get("run_in_background").and_then(Value::as_bool) == Some(true)
}

fn command_failed(what: CommandKind, reason: String) -> SessionEvent {
    SessionEvent::CommandFailed { what, reason }
}

fn tool_finished(result: ToolResult) -> SessionEvent {
    let output = result.text;
    SessionEvent::ToolFinished {
        id: ToolCallId(result.tool_use_id),
        outcome: if result.is_error {
            ToolOutcome::Failed { output }
        } else {
            ToolOutcome::Ok { output }
        },
    }
}

/// The questions an `AskUserQuestion` input holds. Entries without a prompt are skipped.
fn questions(input: &Value) -> Vec<Question> {
    let list = input.get("questions").and_then(Value::as_array);
    list.into_iter()
        .flatten()
        .filter_map(|q| {
            let prompt = q.get("question").and_then(Value::as_str)?.to_string();
            let options = q
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|o| {
                    let label = o.get("label").and_then(Value::as_str)?.to_string();
                    Some(QuestionOption {
                        label,
                        description: o
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                })
                .collect();
            let multi_select = q
                .get("multiSelect")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let header = q.get("header").and_then(Value::as_str).map(str::to_string);
            Some(Question {
                header,
                prompt,
                options,
                multi_select,
            })
        })
        .collect()
}

/// The words for the "always allow" choice, from the first of a request's
/// permission suggestions only; that suggestion is also the only one sent back.
/// A rule suggestion names every rule it grants, each as `Tool(content)`, or as
/// `Tool` when it has no content, and says where the grant lasts. `None` when
/// the suggestion is not one this adapter can describe exactly: a rule without
/// a tool name, or with content that is not text, never gets a label.
pub fn always_allow_label(suggestions: &Value) -> Option<String> {
    let first = suggestions.get(0)?;
    fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
        value.get(key).and_then(Value::as_str)
    }
    let destination = text(first, "destination")?;
    match text(first, "type")? {
        "addRules" if text(first, "behavior") == Some("allow") => {
            let rules = first
                .get("rules")?
                .as_array()?
                .iter()
                .map(rule_label)
                .collect::<Option<Vec<_>>>()?;
            if rules.is_empty() {
                return None;
            }
            let scope = match destination {
                "session" => "for this session",
                "localSettings" => "in this project",
                "projectSettings" => "for everyone in this project",
                "userSettings" => "in all projects",
                _ => return None,
            };
            Some(format!("Always allow {} {scope}", rules.join(", ")))
        }
        "setMode" if text(first, "mode") == Some("acceptEdits") && destination == "session" => {
            Some("Allow all edits for this session".to_string())
        }
        _ => None,
    }
}

/// One permission rule as `Tool(content)` or `Tool`.
fn rule_label(rule: &Value) -> Option<String> {
    let tool = rule
        .get("toolName")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())?;
    match rule.get("ruleContent") {
        None | Some(Value::Null) => Some(tool.to_string()),
        Some(Value::String(content)) if content.is_empty() => Some(tool.to_string()),
        Some(Value::String(content)) => Some(format!("{tool}({content})")),
        Some(_) => None,
    }
}

/// What a tool call does, in terms the UI can show. A tool that is not known,
/// or a known one missing its fields, gives `Other` with the input as compact JSON.
pub fn tool_detail(name: &str, input: &Value) -> ToolDetail {
    let field = |key: &str| input.get(key).and_then(Value::as_str).map(str::to_string);
    let known = match name {
        "Bash" => field("command").map(|command| ToolDetail::Command { command }),
        "Edit" => match (field("file_path"), field("old_string"), field("new_string")) {
            (Some(path), Some(old_text), Some(new_text)) => Some(ToolDetail::FileEdit {
                path,
                old_text,
                new_text,
            }),
            _ => None,
        },
        "Write" => match (field("file_path"), field("content")) {
            (Some(path), Some(content)) => Some(ToolDetail::FileWrite { path, content }),
            _ => None,
        },
        "Read" => field("file_path").map(|path| ToolDetail::FileRead { path }),
        "Grep" | "Glob" => field("pattern").map(|query| ToolDetail::Search { query }),
        "WebSearch" => field("query").map(|query| ToolDetail::Search { query }),
        _ => None,
    };
    known.unwrap_or_else(|| ToolDetail::Other {
        input_json: input.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::parse_line;
    use mr_agent::{
        Choice, Choices, ModelOption, QuestionAnswer, TaskChange, TaskStatus, TaskStep,
    };
    use serde_json::json;
    use std::path::PathBuf;

    fn feed(lines: &[&str]) -> Vec<SessionEvent> {
        let mut translator = Translator::new();
        lines
            .iter()
            .flat_map(|line| translator.push(parse_line(line)))
            .collect()
    }

    const INIT: &str = r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-haiku-5-5","permissionMode":"default","claude_code_version":"2.1.319","cwd":"/work"}"#;
    const MSG_START: &str =
        r#"{"type":"stream_event","event":{"type":"message_start","message":{}}}"#;
    const TEXT_START: &str = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}"#;
    const DELTA_OK: &str = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}}"#;
    const BLOCK_STOP: &str =
        r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#;
    const ASSISTANT_OK: &str = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":40000,"cache_read_input_tokens":9000,"cache_creation_input_tokens":0,"output_tokens":1000}}}"#;
    const MSG_STOP: &str = r#"{"type":"stream_event","event":{"type":"message_stop"}}"#;
    const RESULT_OK: &str = r#"{"type":"result","is_error":false,"duration_ms":915,"total_cost_usd":0.002,"result":"ok","modelUsage":{"claude-haiku-5-5":{"contextWindow":1000000}}}"#;

    #[test]
    fn a_streamed_text_turn() {
        // The order recorded live: the `assistant` record for a block comes
        // before that block's `content_block_stop`.
        let events = feed(&[
            INIT,
            MSG_START,
            TEXT_START,
            DELTA_OK,
            ASSISTANT_OK,
            BLOCK_STOP,
            MSG_STOP,
            RESULT_OK,
        ]);
        assert_eq!(
            events,
            vec![
                SessionEvent::Started(SessionInfo {
                    id: SessionId("s1".into()),
                    folder: "/work".into(),
                    model: "claude-haiku-5-5".into(),
                    permission_mode: PermissionMode::Ask,
                    agent_version: "2.1.319".into(),
                    models: vec![]
                }),
                SessionEvent::TurnStarted,
                SessionEvent::TextDelta("ok".into()),
                SessionEvent::BlockFinished,
                SessionEvent::TurnFinished(TurnSummary {
                    cost_usd: 0.002,
                    context_used: Some(0.049),
                    duration_ms: 915,
                    interrupted: false
                }),
            ]
        );
    }

    /// The first line of the V1 recording: the real reply to `initialize`.
    fn initialize_reply() -> &'static str {
        include_str!("../tests/fixtures/live/v1-remote.jsonl")
            .lines()
            .next()
            .unwrap()
    }

    fn opening(model: Option<&str>, mode: Option<PermissionMode>) -> Translator {
        let mut t = Translator::for_session(SessionStart {
            id: SessionId("chosen".into()),
            folder: "/work".into(),
            model: model.map(str::to_string),
            permission_mode: mode,
        });
        t.track("probe_1", Tracked::Initialize);
        t
    }

    fn started_info(events: &[SessionEvent]) -> SessionInfo {
        match events {
            [SessionEvent::Started(info)] => info.clone(),
            other => panic!("expected Started alone, got {other:?}"),
        }
    }

    #[test]
    fn the_initialize_reply_starts_the_session_before_any_turn() {
        let mut t = opening(None, None);
        let info = started_info(&t.push(parse_line(initialize_reply())));
        assert_eq!(info.id, SessionId("chosen".into()));
        assert_eq!(info.folder, PathBuf::from("/work"));
        assert_eq!(info.model, "claude-fable-5-2", "the default entry");
        assert_eq!(info.permission_mode, PermissionMode::Ask);
        assert_eq!(info.agent_version, "2.1.319");
        assert_eq!(
            info.models[1],
            ModelOption {
                id: "opus".into(),
                name: "Opus 5.5".into(),
                resolved: "claude-opus-5-5".into(),
                stands_for: None,
                auto_mode: true,
                effort_levels: ["low", "medium", "high", "xhigh", "max"]
                    .map(String::from)
                    .to_vec(),
            }
        );
        assert_eq!(info.models.len(), 3);
        // The turn's init repeats what is known: nothing new to report.
        let init = r#"{"type":"system","subtype":"init","session_id":"chosen","model":"claude-fable-5-2","permissionMode":"default","claude_code_version":"2.1.319","cwd":"/work"}"#;
        assert!(t.push(parse_line(init)).is_empty());
        assert!(t.push(parse_line(initialize_reply())).is_empty());
    }

    /// What each entry of `models` (a JSON list as `claude` writes it) stands for, as the
    /// `Started` of a session whose `initialize` reply lists them says.
    fn stands_for(models: &str) -> Vec<Option<String>> {
        let mut t = opening(None, None);
        let reply = format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"probe_1","response":{{"models":{models}}}}}}}"#
        );
        started_info(&t.push(parse_line(&reply)))
            .models
            .into_iter()
            .map(|option| option.stands_for)
            .collect()
    }

    #[test]
    fn a_model_choice_says_whether_it_supports_auto_mode() {
        let mut t = opening(None, None);
        let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"probe_1","response":{"models":[
            {"value":"default","resolvedModel":"m-1","supportsAutoMode":true},
            {"value":"slow","resolvedModel":"m-2","supportsAutoMode":false},
            {"value":"old","resolvedModel":"m-3"}]}}}"#;
        let info = started_info(&t.push(parse_line(&reply.replace('\n', ""))));
        let auto: Vec<bool> = info.models.iter().map(|m| m.auto_mode).collect();
        assert_eq!(auto, vec![true, false, false], "false when not said");
        let recorded = include_str!("../tests/fixtures/live/v9-control.jsonl")
            .lines()
            .next()
            .unwrap();
        let mut t = opening(None, None);
        let info = started_info(&t.push(parse_line(recorded)));
        assert!(info.models.iter().all(|m| m.auto_mode));
    }

    #[test]
    fn a_model_choice_names_the_model_it_stands_for_when_its_name_does_not() {
        // The recorded list: "Default (recommended)" is Fable 5.2, which has its own entry.
        let recorded = include_str!("../tests/fixtures/live/v9-control.jsonl")
            .lines()
            .next()
            .unwrap();
        let mut t = opening(None, None);
        let info = started_info(&t.push(parse_line(recorded)));
        let names: Vec<_> = info
            .models
            .iter()
            .map(|m| (m.name.as_str(), m.stands_for.as_deref()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Default (recommended)", Some("Fable 5.2")),
                ("Opus 5.5", None),
                ("Fable 5.2", None),
            ]
        );
        // No entry for the model it resolves to: the default entry's description names it.
        assert_eq!(
            stands_for(
                r#"[{"value":"default","resolvedModel":"m-9","displayName":"Default","description":"Sonnet 9"},
                    {"value":"fast","resolvedModel":"m-1","displayName":"Fast","description":"Quick answers"}]"#
            ),
            vec![Some("Sonnet 9".into()), None],
            "only the default entry's description is a model's name"
        );
        // An entry for it with the same name says nothing more; an empty description neither.
        assert_eq!(
            stands_for(
                r#"[{"value":"default","resolvedModel":"m-9","displayName":"Sonnet 9","description":""},
                    {"value":"m-9","resolvedModel":"m-9","displayName":"Sonnet 9"}]"#
            ),
            vec![None, None]
        );
        assert_eq!(
            stands_for(r#"[{"value":"default","resolvedModel":"m-9","displayName":"Default"}]"#),
            vec![None]
        );
    }

    #[test]
    fn the_model_asked_for_is_resolved_from_the_list_or_kept_as_asked() {
        let mut t = opening(Some("opus"), Some(PermissionMode::Plan));
        let info = started_info(&t.push(parse_line(initialize_reply())));
        assert_eq!(info.model, "claude-opus-5-5");
        assert_eq!(
            info.permission_mode,
            PermissionMode::Ask,
            "the reply's mode wins"
        );

        let mut t = opening(Some("haiku"), None);
        let info = started_info(&t.push(parse_line(initialize_reply())));
        assert_eq!(info.model, "haiku", "no entry for it");
        // The first init then reports only what differs.
        assert_eq!(
            t.push(parse_line(INIT)),
            vec![SessionEvent::ModelChanged("claude-haiku-5-5".into())]
        );
    }

    #[test]
    fn a_sparse_initialize_reply_falls_back_to_what_was_asked() {
        let mut t = opening(None, Some(PermissionMode::AcceptEdits));
        let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"probe_1","response":{"models":[{"displayName":"No value"},{"value":"x","displayName":"X","resolvedModel":"claude-x"}]}}}"#;
        let info = started_info(&t.push(parse_line(reply)));
        assert_eq!(info.model, "", "nothing asked and no default entry");
        assert_eq!(info.permission_mode, PermissionMode::AcceptEdits);
        assert_eq!(info.agent_version, "");
        assert_eq!(
            info.models,
            vec![ModelOption {
                id: "x".into(),
                name: "X".into(),
                resolved: "claude-x".into(),
                stands_for: None,
                auto_mode: false,
                effort_levels: Vec::new(),
            }]
        );
        assert_eq!(
            t.push(parse_line(INIT)),
            vec![
                SessionEvent::ModelChanged("claude-haiku-5-5".into()),
                SessionEvent::PermissionModeChanged(PermissionMode::Ask)
            ]
        );
    }

    #[test]
    fn a_failed_initialize_gives_nothing_and_the_init_starts_the_session() {
        let mut t = opening(None, None);
        let failed = r#"{"type":"control_response","response":{"subtype":"error","request_id":"probe_1","error":"no"}}"#;
        assert!(t.push(parse_line(failed)).is_empty());
        let info = started_info(&t.push(parse_line(INIT)));
        assert_eq!(info.model, "claude-haiku-5-5");
    }

    #[test]
    fn without_partial_messages_the_whole_text_still_arrives() {
        let events = feed(&[INIT, ASSISTANT_OK, RESULT_OK]);
        assert_eq!(
            &events[1..3],
            &[
                SessionEvent::TextDelta("ok".into()),
                SessionEvent::BlockFinished
            ]
        );
    }

    fn assistant_text(text: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    }

    fn texts(events: &[SessionEvent]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|e| match e {
                SessionEvent::TextDelta(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_message_that_was_not_streamed_arrives_after_one_that_was() {
        let later = assistant_text("later");
        let next = assistant_text("next turn");
        let streamed = [
            MSG_START,
            TEXT_START,
            DELTA_OK,
            ASSISTANT_OK,
            BLOCK_STOP,
            MSG_STOP,
        ];
        let mut lines = vec![INIT];
        lines.extend(streamed);
        lines.push(&later);
        lines.push(RESULT_OK);
        lines.push(INIT);
        lines.extend(streamed);
        lines.push(RESULT_OK);
        lines.push(INIT);
        lines.push(&next);
        lines.push(RESULT_OK);
        let events = feed(&lines);
        assert_eq!(texts(&events), ["ok", "later", "ok", "next turn"]);
        let at = events
            .iter()
            .position(|e| *e == SessionEvent::TextDelta("later".into()))
            .unwrap();
        assert_eq!(events[at + 1], SessionEvent::BlockFinished);
    }

    #[test]
    fn a_message_cut_off_by_its_result_does_not_hide_the_next_one() {
        let stopped = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"terminal_reason":"aborted_streaming","duration_ms":1,"total_cost_usd":0}"#;
        let next = assistant_text("next turn");
        let events = feed(&[INIT, MSG_START, TEXT_START, DELTA_OK, stopped, &next]);
        assert_eq!(texts(&events), ["ok", "next turn"]);
    }

    #[test]
    fn a_tool_call_starts_and_finishes() {
        let call = r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#;
        let done = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"a.txt"}]}}"#;
        let events = feed(&[INIT, MSG_START, call, done]);
        assert_eq!(
            &events[2..],
            &[
                SessionEvent::ToolStarted {
                    id: ToolCallId("t1".into()),
                    name: "Bash".into(),
                    detail: ToolDetail::Command {
                        command: "ls".into()
                    }
                },
                SessionEvent::ToolFinished {
                    id: ToolCallId("t1".into()),
                    outcome: ToolOutcome::Ok {
                        output: "a.txt".into()
                    }
                },
            ]
        );
    }

    #[test]
    fn a_question_or_a_plan_is_a_request_and_never_a_tool_call() {
        let calls = r#"{"type":"assistant","message":{"id":"m3","content":[
            {"type":"tool_use","id":"t_q","name":"AskUserQuestion","input":{"questions":[]}},
            {"type":"tool_use","id":"t_p","name":"ExitPlanMode","input":{"plan":"Plan"}},
            {"type":"tool_use","id":"t_b","name":"Bash","input":{"command":"ls"}}]}}"#
            .replace('\n', "");
        let results = r#"{"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"t_q","content":"User has answered your questions"},
            {"type":"tool_result","tool_use_id":"t_p","content":"User has approved your plan"},
            {"type":"tool_result","tool_use_id":"t_b","content":"a.txt"}]}}"#
            .replace('\n', "");
        let events = feed(&[INIT, MSG_START, &calls, &results]);
        assert_eq!(
            &events[2..],
            &[
                SessionEvent::ToolStarted {
                    id: ToolCallId("t_b".into()),
                    name: "Bash".into(),
                    detail: ToolDetail::Command {
                        command: "ls".into()
                    }
                },
                SessionEvent::ToolFinished {
                    id: ToolCallId("t_b".into()),
                    outcome: ToolOutcome::Ok {
                        output: "a.txt".into()
                    }
                },
            ]
        );
    }

    /// An assistant record of the main conversation with one tool call.
    fn tool_use(id: &str, name: &str, input: Value) -> String {
        json!({"type":"assistant","message":{"id":format!("m_{id}"),"content":[
            {"type":"tool_use","id":id,"name":name,"input":input}]},"parent_tool_use_id":null})
        .to_string()
    }

    /// A user record with one tool result, and its top-level `tool_use_result` when given.
    fn tool_result(id: &str, text: &str, is_error: bool, structured: Option<Value>) -> String {
        let mut block = json!({"tool_use_id":id,"type":"tool_result","content":text});
        if is_error {
            block["is_error"] = json!(true);
        }
        let mut record = json!({"type":"user","message":{"role":"user","content":[block]},"parent_tool_use_id":null});
        if let Some(structured) = structured {
            record["tool_use_result"] = structured;
        }
        record.to_string()
    }

    fn create_a() -> String {
        tool_use(
            "t1",
            "TaskCreate",
            json!({"subject":"Create a.txt","description":"Write a.txt with one line","activeForm":"Creating a.txt"}),
        )
    }

    fn created_a(id: &str, is_error: bool) -> String {
        tool_result(
            id,
            "Task #1 created successfully: Create a.txt",
            is_error,
            Some(
                json!({"task":{"id":"1","subject":"Create a.txt","description":"Write a.txt with one line"}}),
            ),
        )
    }

    #[test]
    fn a_task_call_becomes_a_step_when_its_result_names_the_id() {
        let events = feed(&[INIT, MSG_START, &create_a(), &created_a("t1", false)]);
        assert_eq!(
            &events[2..],
            &[SessionEvent::TaskAdded(TaskStep {
                id: "1".into(),
                subject: "Create a.txt".into(),
                active: Some("Creating a.txt".into()),
                status: TaskStatus::Pending,
            })]
        );
        let in_background = tool_use("t2", "TaskList", json!({"run_in_background":true}));
        let listed = tool_result("t2", "No tasks found", false, None);
        assert_eq!(
            feed(&[INIT, MSG_START, &in_background, &listed]).len(),
            2,
            "no tool line and no background task"
        );
        let sub = r#"{"type":"assistant","parent_tool_use_id":"t9","message":{"id":"m3","content":[{"type":"tool_use","id":"t3","name":"TaskCreate","input":{"subject":"S"}}]}}"#;
        let sub_done = r#"{"type":"user","parent_tool_use_id":"t9","message":{"content":[{"type":"tool_result","tool_use_id":"t3","content":"Task #7 created successfully: S"}]}}"#;
        assert_eq!(
            feed(&[INIT, MSG_START, sub, sub_done]).len(),
            2,
            "a sub-agent's list is not shown"
        );
    }

    #[test]
    fn a_task_call_that_failed_or_was_never_answered_adds_nothing() {
        let events = feed(&[INIT, MSG_START, &create_a(), &created_a("t1", true)]);
        assert_eq!(
            &events[2..],
            &[],
            "a failed call adds no step and no tool line"
        );
        let events = feed(&[
            INIT,
            MSG_START,
            &create_a(),
            RESULT_OK,
            MSG_START,
            &created_a("t1", false),
        ]);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SessionEvent::TaskAdded(_))),
            "{events:?}"
        );
    }

    #[test]
    fn an_update_changes_a_step_and_deleted_removes_it() {
        let mut translator = Translator::new();
        let mut push = |line: &str| translator.push(parse_line(line));
        push(INIT);
        push(MSG_START);
        let update = tool_use(
            "t1",
            "TaskUpdate",
            json!({"taskId":"1","status":"in_progress"}),
        );
        assert_eq!(push(&update), []);
        let updated = tool_result(
            "t1",
            "Updated task #1 status",
            false,
            Some(
                json!({"success":true,"taskId":"1","updatedFields":["status"],"statusChange":{"from":"pending","to":"in_progress"}}),
            ),
        );
        assert_eq!(
            push(&updated),
            [SessionEvent::TaskChanged(TaskChange {
                id: "1".into(),
                status: Some(TaskStatus::InProgress),
                subject: None,
                active: None,
            })]
        );
        let delete = tool_use("t2", "TaskUpdate", json!({"taskId":"4","status":"deleted"}));
        assert_eq!(push(&delete), []);
        let deleted = tool_result("t2", "Updated task #4 deleted", false, None);
        assert_eq!(
            push(&deleted),
            [SessionEvent::TaskRemoved { id: "4".into() }]
        );
    }

    #[test]
    fn an_offer_is_announced_at_its_result_and_a_refused_or_failed_one_is_not() {
        let mut translator = Translator::new();
        let mut push = |line: &str| translator.push(parse_line(line));
        push(INIT);
        push(MSG_START);
        let choice = |label: &str| json!({"label": label, "message": format!("{label}, please.")});
        let offer = |id: &str| {
            tool_use(
                id,
                "mcp__mindrune__offer_choices",
                json!({"question":"Keep the files?","choices":[choice("Keep"), choice("Delete")]}),
            )
        };
        let expected = Choices::new(
            "Keep the files?",
            vec![
                Choice {
                    label: "Keep".into(),
                    message: "Keep, please.".into(),
                },
                Choice {
                    label: "Delete".into(),
                    message: "Delete, please.".into(),
                },
            ],
        )
        .unwrap();
        assert_eq!(push(&offer("t1")), [], "nothing at the call");
        let shown = |id: &str| {
            tool_result(
                id,
                "Shown to the user.",
                false,
                Some(json!([{"type":"text","text":"Shown to the user."}])),
            )
        };
        assert_eq!(push(&shown("t1")), [SessionEvent::ChoicesOffered(expected)]);
        let four = tool_use(
            "t2",
            "mcp__mindrune__offer_choices",
            json!({"question":"Which?","choices":[choice("A"), choice("B"), choice("C"), choice("D")]}),
        );
        assert_eq!(push(&four), []);
        let refused = tool_result("t2", "One to three choices are needed.", true, None);
        assert_eq!(push(&refused), []);
        assert_eq!(push(&offer("t3")), []);
        let denied = tool_result("t3", "Permission to use the tool was denied.", true, None);
        assert_eq!(push(&denied), [], "the program refused the call: no box");
    }

    #[test]
    fn an_offer_whose_result_comes_after_its_turn_shows_nothing() {
        let offer = tool_use(
            "t1",
            "mcp__mindrune__offer_choices",
            json!({"question":"Go on?","choices":[{"label":"Yes","message":"Yes."}]}),
        );
        let shown = tool_result("t1", "Shown to the user.", false, None);
        let events = feed(&[INIT, MSG_START, &offer, RESULT_OK, MSG_START, &shown]);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SessionEvent::ChoicesOffered(_))),
            "{events:?}"
        );
    }

    #[test]
    fn a_task_create_whose_result_gives_no_id_adds_nothing() {
        let created = tool_result("t1", "Done.", false, Some(json!({"ok":true})));
        let events = feed(&[INIT, MSG_START, &create_a(), &created]);
        assert_eq!(&events[2..], &[], "no step and no tool line");
    }

    #[test]
    fn a_result_without_its_call_is_an_ordinary_result() {
        let events = feed(&[INIT, MSG_START, &created_a("t9", false)]);
        assert_eq!(
            &events[2..],
            &[SessionEvent::ToolFinished {
                id: ToolCallId("t9".into()),
                outcome: ToolOutcome::Ok {
                    output: "Task #1 created successfully: Create a.txt".into()
                }
            }]
        );
    }

    #[test]
    fn the_recorded_task_list_gives_six_steps_and_no_tool_line_of_its_own() {
        let events = replay("v12-tasks.jsonl");
        let added: Vec<(&str, &str, Option<&str>)> = events
            .iter()
            .filter_map(|e| match e {
                SessionEvent::TaskAdded(step) => Some((
                    step.id.as_str(),
                    step.subject.as_str(),
                    step.active.as_deref(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(added.len(), 6);
        assert_eq!(added[0], ("1", "Create a.txt", Some("Creating a.txt")));
        assert_eq!(added[5].0, "6");
        let changes: Vec<&TaskChange> = events
            .iter()
            .filter_map(|e| match e {
                SessionEvent::TaskChanged(change) => Some(change),
                _ => None,
            })
            .collect();
        assert_eq!(changes.len(), 12);
        assert_eq!(
            changes[0],
            &TaskChange {
                id: "1".into(),
                status: Some(TaskStatus::InProgress),
                subject: None,
                active: None
            }
        );
        let names: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                SessionEvent::ToolStarted { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            names,
            [
                "Write", "Write", "Write", "Bash", "Edit", "Edit", "Edit", "Bash"
            ]
        );
        let finished = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::TurnFinished(_)))
            .count();
        assert_eq!(finished, 2);
    }

    #[test]
    fn the_recorded_offer_is_announced_before_its_turn_ends() {
        let events = replay("v14-choices.jsonl");
        let offers: Vec<&Choices> = events
            .iter()
            .filter_map(|e| match e {
                SessionEvent::ChoicesOffered(choices) => Some(choices),
                _ => None,
            })
            .collect();
        assert_eq!(offers.len(), 1);
        assert_eq!(
            offers[0].question,
            "Keep or delete the three text files in this folder?"
        );
        assert_eq!(
            offers[0].choices,
            [
                Choice {
                    label: "Keep them".into(),
                    message: "Keep the three text files.".into()
                },
                Choice {
                    label: "Delete them".into(),
                    message: "Delete the three text files in this folder.".into()
                }
            ]
        );
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, SessionEvent::ToolStarted { .. })),
            "its search and its call are hidden"
        );
        let offer_at = events
            .iter()
            .position(|e| matches!(e, SessionEvent::ChoicesOffered(_)))
            .unwrap();
        let end_at = events
            .iter()
            .position(|e| matches!(e, SessionEvent::TurnFinished(_)))
            .unwrap();
        assert!(offer_at < end_at);
        assert!(
            events[offer_at..end_at]
                .iter()
                .any(|e| matches!(e, SessionEvent::TextDelta(_))),
            "Claude wrote on after it"
        );
    }

    #[test]
    fn a_second_init_reports_only_what_changed() {
        let same = INIT;
        let other = r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-sonnet-5-5","permissionMode":"plan","claude_code_version":"2.1.319","cwd":"/work"}"#;
        let events = feed(&[INIT, same, other]);
        assert_eq!(
            &events[1..],
            &[
                SessionEvent::ModelChanged("claude-sonnet-5-5".into()),
                SessionEvent::PermissionModeChanged(PermissionMode::Plan)
            ]
        );
    }

    #[test]
    fn sub_agent_records_are_not_shown() {
        let sub = r#"{"type":"assistant","parent_tool_use_id":"t9","message":{"id":"m3","content":[{"type":"text","text":"inner"}]}}"#;
        assert_eq!(feed(&[INIT, sub]).len(), 1);
    }

    #[test]
    fn a_failed_result_fails_the_turn() {
        let bad = r#"{"type":"result","is_error":true,"result":"Failed to authenticate","duration_ms":92,"total_cost_usd":0}"#;
        assert_eq!(
            feed(&[INIT, bad])[1],
            SessionEvent::TurnFailed {
                reason: "Failed to authenticate".into()
            }
        );
    }

    #[test]
    fn tool_details_by_tool() {
        assert_eq!(
            tool_detail(
                "Edit",
                &json!({"file_path":"/a","old_string":"x","new_string":"y"})
            ),
            ToolDetail::FileEdit {
                path: "/a".into(),
                old_text: "x".into(),
                new_text: "y".into()
            }
        );
        assert_eq!(
            tool_detail("Write", &json!({"file_path":"/a","content":"c"})),
            ToolDetail::FileWrite {
                path: "/a".into(),
                content: "c".into()
            }
        );
        assert_eq!(
            tool_detail("Read", &json!({"file_path":"/a"})),
            ToolDetail::FileRead { path: "/a".into() }
        );
        assert_eq!(
            tool_detail("Grep", &json!({"pattern":"TODO"})),
            ToolDetail::Search {
                query: "TODO".into()
            }
        );
        assert_eq!(
            tool_detail("WebSearch", &json!({"query":"rust"})),
            ToolDetail::Search {
                query: "rust".into()
            }
        );
        assert_eq!(
            tool_detail("Mystery", &json!({"a":1})),
            ToolDetail::Other {
                input_json: r#"{"a":1}"#.into()
            }
        );
        assert_eq!(
            tool_detail("Bash", &json!({})),
            ToolDetail::Other {
                input_json: "{}".into()
            }
        );
    }

    #[test]
    fn a_stopped_turn_finishes_without_failing() {
        let stopped = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"terminal_reason":"aborted_streaming","errors":["x"],"duration_ms":3555,"total_cost_usd":0.06}"#;
        assert_eq!(
            feed(&[INIT, MSG_START, stopped])[2],
            SessionEvent::TurnFinished(TurnSummary {
                cost_usd: 0.06,
                context_used: None,
                duration_ms: 3555,
                interrupted: true
            })
        );
    }

    #[test]
    fn a_thinking_block_is_announced_even_though_its_text_is_empty() {
        let events = feed(&[
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"model":"m"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":""}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"x"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#,
        ]);
        assert_eq!(
            events,
            vec![
                SessionEvent::TurnStarted,
                SessionEvent::ThinkingStarted,
                SessionEvent::BlockFinished
            ]
        );
    }

    #[test]
    fn a_streamed_thinking_block_with_text_keeps_its_text_between_start_and_finish() {
        let think_start = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}"#;
        let think_delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hm"}}}"#;
        let events = feed(&[MSG_START, think_start, think_delta, BLOCK_STOP]);
        assert_eq!(
            events,
            vec![
                SessionEvent::TurnStarted,
                SessionEvent::ThinkingStarted,
                SessionEvent::ThinkingDelta("hm".into()),
                SessionEvent::BlockFinished
            ]
        );
    }

    #[test]
    fn a_thinking_block_without_partial_messages_arrives_whole_before_the_text() {
        let both = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"thinking","thinking":"hm","signature":"sig"},{"type":"text","text":"ok"}]}}"#;
        let events = feed(&[INIT, both, RESULT_OK]);
        assert_eq!(
            &events[1..6],
            &[
                SessionEvent::ThinkingStarted,
                SessionEvent::ThinkingDelta("hm".into()),
                SessionEvent::BlockFinished,
                SessionEvent::TextDelta("ok".into()),
                SessionEvent::BlockFinished,
            ]
        );
    }

    #[test]
    fn an_empty_thinking_block_without_partial_messages_is_still_announced() {
        let whole = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"thinking","thinking":"","signature":"sig"}]}}"#;
        assert_eq!(
            &feed(&[INIT, whole])[1..],
            &[SessionEvent::ThinkingStarted, SessionEvent::BlockFinished]
        );
    }

    #[test]
    fn a_thinking_block_already_streamed_is_not_announced_again_by_its_record() {
        let think_start = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}"#;
        let whole = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"thinking","thinking":"hm","signature":"sig"}]}}"#;
        let events = feed(&[MSG_START, think_start, whole, BLOCK_STOP, MSG_STOP]);
        assert_eq!(
            events,
            vec![
                SessionEvent::TurnStarted,
                SessionEvent::ThinkingStarted,
                SessionEvent::BlockFinished
            ]
        );
    }

    #[test]
    fn a_question_keeps_its_header_and_option_descriptions() {
        let input = serde_json::json!({"questions":[{
            "question":"Which colour?","header":"Colour","multiSelect":true,
            "options":[{"label":"Red","description":"Warm"},{"label":"Blue"}]
        }]});
        assert_eq!(
            questions(&input),
            vec![Question {
                header: Some("Colour".into()),
                prompt: "Which colour?".into(),
                options: vec![
                    QuestionOption {
                        label: "Red".into(),
                        description: Some("Warm".into())
                    },
                    QuestionOption {
                        label: "Blue".into(),
                        description: None
                    },
                ],
                multi_select: true,
            }]
        );
    }

    #[test]
    fn a_status_record_reports_a_mode_change_once() {
        let plan = r#"{"type":"system","subtype":"status","status":null,"permissionMode":"plan"}"#;
        assert_eq!(
            &feed(&[INIT, plan, plan])[1..],
            &[SessionEvent::PermissionModeChanged(PermissionMode::Plan)]
        );
    }

    fn request(
        id: &str,
        tool: &str,
        input: serde_json::Value,
        suggestions: serde_json::Value,
    ) -> Incoming {
        Incoming::ControlRequest {
            request_id: id.into(),
            subtype: "can_use_tool".into(),
            body: json!({"subtype":"can_use_tool","tool_name":tool,"input":input,
                         "permission_suggestions":suggestions,"tool_use_id":"t1"}),
        }
    }

    fn project_rule() -> serde_json::Value {
        json!([{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"npm test:*"}],
                "behavior":"allow","destination":"localSettings"}])
    }

    #[test]
    fn a_permission_request_needs_the_user_and_can_be_allowed_once() {
        let mut t = Translator::new();
        let events = t.push(request(
            "req_1",
            "Bash",
            json!({"command":"npm test"}),
            project_rule(),
        ));
        assert_eq!(
            events,
            vec![SessionEvent::NeedsUser(UserRequest {
                id: RequestId("req_1".into()),
                kind: UserRequestKind::Permission {
                    tool: "Bash".into(),
                    detail: ToolDetail::Command {
                        command: "npm test".into()
                    },
                    always_allow_label: Some(
                        "Always allow Bash(npm test:*) in this project".into()
                    )
                }
            })]
        );
        let reply = t
            .answer(&RequestId("req_1".into()), Answer::Allow { always: false })
            .unwrap();
        assert_eq!(
            reply,
            PermissionReply::Allow {
                updated_input: json!({"command":"npm test"}),
                updated_permissions: None
            }
        );
        assert!(
            t.answer(&RequestId("req_1".into()), Answer::Allow { always: false })
                .is_err(),
            "answered once only"
        );
    }

    #[test]
    fn always_allow_sends_back_exactly_the_suggestion() {
        let mut t = Translator::new();
        t.push(request(
            "req_2",
            "Bash",
            json!({"command":"npm test"}),
            project_rule(),
        ));
        let reply = t
            .answer(&RequestId("req_2".into()), Answer::Allow { always: true })
            .unwrap();
        assert_eq!(
            reply,
            PermissionReply::Allow {
                updated_input: json!({"command":"npm test"}),
                updated_permissions: Some(project_rule())
            }
        );
    }

    #[test]
    fn deny_has_a_default_message() {
        let mut t = Translator::new();
        t.push(request(
            "req_3",
            "Bash",
            json!({"command":"rm -rf /"}),
            json!(null),
        ));
        assert_eq!(
            t.answer(&RequestId("req_3".into()), Answer::Deny { message: None })
                .unwrap(),
            PermissionReply::Deny {
                message: "Denied by the user".into()
            }
        );
    }

    #[test]
    fn labels_follow_the_suggestion() {
        let rule = |dest: &str| json!([{"type":"addRules","rules":[{"toolName":"WebFetch"}],"behavior":"allow","destination":dest}]);
        assert_eq!(
            always_allow_label(&rule("session")).as_deref(),
            Some("Always allow WebFetch for this session")
        );
        assert_eq!(
            always_allow_label(&rule("localSettings")).as_deref(),
            Some("Always allow WebFetch in this project")
        );
        assert_eq!(
            always_allow_label(&rule("projectSettings")).as_deref(),
            Some("Always allow WebFetch for everyone in this project")
        );
        assert_eq!(
            always_allow_label(&rule("userSettings")).as_deref(),
            Some("Always allow WebFetch in all projects")
        );
        assert_eq!(always_allow_label(&rule("somewhereNew")), None);
        assert_eq!(
            always_allow_label(
                &json!([{"type":"setMode","mode":"acceptEdits","destination":"session"}])
            )
            .as_deref(),
            Some("Allow all edits for this session")
        );
        assert_eq!(always_allow_label(&json!([])), None);
        assert_eq!(always_allow_label(&json!(null)), None);
        assert_eq!(
            always_allow_label(
                &json!([{"type":"addRules","rules":[{"toolName":"Bash"}],"behavior":"deny","destination":"session"}])
            ),
            None
        );
    }

    #[test]
    fn a_label_says_which_tool_each_rule_is_for() {
        let rule = |tool: &str, content: &str| {
            json!([{"type":"addRules","behavior":"allow","destination":"localSettings",
                "rules":[{"toolName":tool,"ruleContent":content}]}])
        };
        let bash = always_allow_label(&rule("Bash", "//a/**"));
        let read = always_allow_label(&rule("Read", "//a/**"));
        assert_eq!(
            bash.as_deref(),
            Some("Always allow Bash(//a/**) in this project")
        );
        assert_eq!(
            read.as_deref(),
            Some("Always allow Read(//a/**) in this project")
        );
        assert_eq!(
            always_allow_label(&rule("WebFetch", "")).as_deref(),
            Some("Always allow WebFetch in this project"),
            "empty content is the whole tool"
        );
        assert_eq!(always_allow_label(&rule("", "npm test:*")), None);
    }

    #[test]
    fn a_question_is_answered_by_adding_answers_to_the_input() {
        let input = json!({"questions":[{"question":"Which?","header":"Pick",
            "options":[{"label":"A","description":"a"},{"label":"B","description":"b"}],"multiSelect":false}]});
        let mut t = Translator::new();
        let events = t.push(request(
            "req_4",
            "AskUserQuestion",
            input.clone(),
            json!(null),
        ));
        assert_eq!(
            events,
            vec![SessionEvent::NeedsUser(UserRequest {
                id: RequestId("req_4".into()),
                kind: UserRequestKind::Question {
                    questions: vec![Question {
                        header: Some("Pick".into()),
                        prompt: "Which?".into(),
                        options: vec![
                            QuestionOption {
                                label: "A".into(),
                                description: Some("a".into())
                            },
                            QuestionOption {
                                label: "B".into(),
                                description: Some("b".into())
                            }
                        ],
                        multi_select: false
                    }]
                }
            })]
        );
        assert!(
            t.answer(&RequestId("req_4".into()), Answer::Allow { always: false })
                .is_err(),
            "wrong kind of answer"
        );
        let reply = t
            .answer(
                &RequestId("req_4".into()),
                Answer::Answers(vec![QuestionAnswer {
                    prompt: "Which?".into(),
                    chosen: vec!["A".into()],
                    free_text: None,
                }]),
            )
            .unwrap();
        let mut expected = input;
        expected["answers"] = json!({"Which?":"A"});
        assert_eq!(
            reply,
            PermissionReply::Allow {
                updated_input: expected,
                updated_permissions: None
            }
        );
    }

    #[test]
    fn a_typed_answer_is_sent_with_the_options_chosen() {
        let input = json!({"questions":[
            {"question":"Which colour?","options":[{"label":"Red"},{"label":"Blue"}],"multiSelect":false},
            {"question":"Which sizes?","options":[{"label":"S"},{"label":"M"}],"multiSelect":true},
            {"question":"Anything else?","options":[{"label":"No"}],"multiSelect":false}]});
        let mut t = Translator::new();
        t.push(request(
            "req_9",
            "AskUserQuestion",
            input.clone(),
            json!(null),
        ));
        let answer = |prompt: &str, chosen: &[&str], free_text: Option<&str>| QuestionAnswer {
            prompt: prompt.into(),
            chosen: chosen.iter().map(|c| c.to_string()).collect(),
            free_text: free_text.map(str::to_owned),
        };
        let reply = t
            .answer(
                &RequestId("req_9".into()),
                Answer::Answers(vec![
                    answer("Which colour?", &["Red"], Some("but darker")),
                    answer("Which sizes?", &["S", "M"], Some("XL")),
                    answer("Anything else?", &[], Some("Gift wrap")),
                ]),
            )
            .unwrap();
        let PermissionReply::Allow { updated_input, .. } = reply else {
            panic!("a question is answered by allowing it: {reply:?}");
        };
        assert_eq!(
            updated_input["answers"],
            json!({"Which colour?":"Red, but darker","Which sizes?":"S, M, XL",
                   "Anything else?":"Gift wrap"})
        );
    }

    #[test]
    fn a_plan_is_approved_or_sent_back() {
        let mut t = Translator::new();
        let events = t.push(request(
            "req_5",
            "ExitPlanMode",
            json!({"plan":"# Plan"}),
            json!(null),
        ));
        assert!(matches!(&events[0], SessionEvent::NeedsUser(UserRequest {
            kind: UserRequestKind::PlanApproval { plan_markdown }, .. }) if plan_markdown == "# Plan"));
        assert_eq!(
            t.answer(
                &RequestId("req_5".into()),
                Answer::KeepPlanning { feedback: None }
            )
            .unwrap(),
            PermissionReply::Deny {
                message: "Keep planning".into()
            }
        );
    }

    #[test]
    fn other_control_requests_raise_nothing() {
        let mut t = Translator::new();
        let other = Incoming::ControlRequest {
            request_id: "req_6".into(),
            subtype: "hook_callback".into(),
            body: json!({}),
        };
        assert!(t.push(other).is_empty());
    }

    #[test]
    fn always_allow_sends_only_the_first_suggestion() {
        let three = json!([
            {"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"touch probe-marker.txt"}],"behavior":"allow","destination":"localSettings"},
            {"type":"addDirectories","directories":["/work"],"destination":"session"},
            {"type":"setMode","mode":"acceptEdits","destination":"session"}]);
        let mut t = Translator::new();
        t.push(request(
            "req_1",
            "Bash",
            json!({"command":"touch probe-marker.txt"}),
            three.clone(),
        ));
        let reply = t
            .answer(&RequestId("req_1".into()), Answer::Allow { always: true })
            .unwrap();
        assert_eq!(
            reply,
            PermissionReply::Allow {
                updated_input: json!({"command":"touch probe-marker.txt"}),
                updated_permissions: Some(json!([three[0].clone()]))
            }
        );
    }

    #[test]
    fn always_allow_without_a_usable_suggestion_allows_once() {
        let mut t = Translator::new();
        t.push(request(
            "req_1",
            "Bash",
            json!({"command":"ls"}),
            json!(null),
        ));
        let reply = t
            .answer(&RequestId("req_1".into()), Answer::Allow { always: true })
            .unwrap();
        assert_eq!(
            reply,
            PermissionReply::Allow {
                updated_input: json!({"command":"ls"}),
                updated_permissions: None
            }
        );
    }

    #[test]
    fn a_cancelled_request_is_withdrawn() {
        let mut t = Translator::new();
        t.push(request(
            "req_1",
            "Bash",
            json!({"command":"ls"}),
            json!(null),
        ));
        assert_eq!(
            t.push(Incoming::ControlCancel {
                request_id: "req_1".into()
            }),
            vec![SessionEvent::RequestWithdrawn {
                id: RequestId("req_1".into())
            }]
        );
        assert!(
            t.answer(&RequestId("req_1".into()), Answer::Allow { always: false })
                .is_err()
        );
        assert!(
            t.push(Incoming::ControlCancel {
                request_id: "nope".into()
            })
            .is_empty()
        );
    }

    #[test]
    fn a_mode_suggestion_is_labelled_only_when_it_lasts_for_the_session() {
        let mode = |dest: &str| json!([{"type":"setMode","mode":"acceptEdits","destination":dest}]);
        assert_eq!(
            always_allow_label(&mode("session")).as_deref(),
            Some("Allow all edits for this session")
        );
        assert_eq!(always_allow_label(&mode("userSettings")), None);
        assert_eq!(always_allow_label(&mode("localSettings")), None);
    }

    #[test]
    fn a_label_names_every_rule_the_suggestion_grants() {
        let two = json!([{"type":"addRules","behavior":"allow","destination":"session",
            "rules":[{"toolName":"Read","ruleContent":"//a/**"},{"toolName":"Bash"}]}]);
        assert_eq!(
            always_allow_label(&two).as_deref(),
            Some("Always allow Read(//a/**), Bash for this session")
        );
        let nameless = json!([{"type":"addRules","behavior":"allow","destination":"session",
            "rules":[{"toolName":"Read"},{"ruleContent":"x"}]}]);
        assert_eq!(always_allow_label(&nameless), None);
        let odd_content = json!([{"type":"addRules","behavior":"allow","destination":"session",
            "rules":[{"toolName":"Read","ruleContent":7}]}]);
        assert_eq!(always_allow_label(&odd_content), None);
        let none =
            json!([{"type":"addRules","behavior":"allow","destination":"session","rules":[]}]);
        assert_eq!(always_allow_label(&none), None);
        // Whatever the label says, always-allow sends the suggestion as received.
        let mut t = Translator::new();
        t.push(request("req_1", "Read", json!({}), two.clone()));
        assert_eq!(
            t.answer(&RequestId("req_1".into()), Answer::Allow { always: true })
                .unwrap(),
            PermissionReply::Allow {
                updated_input: json!({}),
                updated_permissions: Some(two)
            }
        );
    }

    #[test]
    fn a_question_with_a_malformed_input_can_still_be_answered() {
        let mut t = Translator::new();
        let events = t.push(request("req_1", "AskUserQuestion", json!("x"), json!(null)));
        assert!(matches!(&events[0], SessionEvent::NeedsUser(UserRequest {
            kind: UserRequestKind::Question { questions }, .. }) if questions.is_empty()));
        let reply = t
            .answer(
                &RequestId("req_1".into()),
                Answer::Answers(vec![QuestionAnswer {
                    prompt: "Which?".into(),
                    chosen: vec![],
                    free_text: Some("none".into()),
                }]),
            )
            .unwrap();
        assert_eq!(
            reply,
            PermissionReply::Allow {
                updated_input: json!({"answers":{"Which?":"none"}}),
                updated_permissions: None
            }
        );
        t.push(request("req_2", "Bash", json!(null), json!(null)));
        assert_eq!(
            t.answer(&RequestId("req_2".into()), Answer::Allow { always: false })
                .unwrap(),
            PermissionReply::Allow {
                updated_input: json!({}),
                updated_permissions: None
            }
        );
    }

    #[test]
    fn a_request_without_a_tool_name_still_asks_the_user() {
        let mut t = Translator::new();
        let nameless = Incoming::ControlRequest {
            request_id: "req_1".into(),
            subtype: "can_use_tool".into(),
            body: json!({"subtype":"can_use_tool","input":{"a":1},"permission_suggestions":project_rule()}),
        };
        assert_eq!(
            t.push(nameless),
            vec![SessionEvent::NeedsUser(UserRequest {
                id: RequestId("req_1".into()),
                kind: UserRequestKind::Permission {
                    tool: "unknown tool".into(),
                    detail: ToolDetail::Other {
                        input_json: r#"{"a":1}"#.into()
                    },
                    always_allow_label: Some(
                        "Always allow Bash(npm test:*) in this project".into()
                    )
                }
            })]
        );
        assert_eq!(
            t.answer(&RequestId("req_1".into()), Answer::Deny { message: None })
                .unwrap(),
            PermissionReply::Deny {
                message: "Denied by the user".into()
            }
        );
    }

    #[test]
    fn remote_control_turns_on_as_recorded_live() {
        let mut t = Translator::new();
        t.track("probe_2", Tracked::RemoteOn);
        let ready =
            r#"{"type":"system","subtype":"bridge_state","state":"ready","session_id":"s1"}"#;
        let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"probe_2","response":{"session_url":"https://example.org/session","bridge_epoch":1}}}"#;
        let connected = r#"{"type":"system","subtype":"bridge_state","state":"connected","bridge_epoch":1,"session_id":"s1"}"#;
        let mut events = Vec::new();
        for l in [ready, reply, connected] {
            events.extend(t.push(parse_line(l)));
        }
        assert_eq!(
            events,
            vec![
                SessionEvent::RemoteAccess(RemoteAccess::Starting),
                SessionEvent::RemoteAccess(RemoteAccess::Starting),
                SessionEvent::RemoteAccess(RemoteAccess::On {
                    url: Some("https://example.org/session".into())
                })
            ]
        );
    }

    fn remote_reply(id: &str, url: Option<&str>) -> String {
        let body = match url {
            Some(url) => format!(r#","response":{{"session_url":"{url}"}}"#),
            None => String::new(),
        };
        format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{id}"{body}}}}}"#
        )
    }

    const READY: &str = r#"{"type":"system","subtype":"bridge_state","state":"ready"}"#;
    const CONNECTED: &str = r#"{"type":"system","subtype":"bridge_state","state":"connected"}"#;
    const URL: &str = "https://example.org/session";

    /// Remote Control on with a URL, connected, then off.
    fn on_then_off() -> Translator {
        let mut t = Translator::new();
        t.track("on1", Tracked::RemoteOn);
        t.track("off", Tracked::RemoteOff);
        for line in [READY, &remote_reply("on1", Some(URL)), CONNECTED] {
            t.push(parse_line(line));
        }
        assert_eq!(
            t.push(parse_line(&remote_reply("off", None))),
            vec![SessionEvent::RemoteAccess(RemoteAccess::Off)]
        );
        t
    }

    #[test]
    fn remote_control_on_off_on_forgets_the_first_url() {
        let mut t = on_then_off();
        t.track("on2", Tracked::RemoteOn);
        t.push(parse_line(READY));
        t.push(parse_line(&remote_reply("on2", None)));
        assert_eq!(
            t.push(parse_line(CONNECTED)),
            vec![SessionEvent::RemoteAccess(RemoteAccess::On { url: None })]
        );
    }

    #[test]
    fn the_url_is_forgotten_when_remote_control_is_turned_off() {
        // Here the second `connected` comes before the second reply.
        let mut t = on_then_off();
        t.track("on2", Tracked::RemoteOn);
        t.push(parse_line(READY));
        assert_eq!(
            t.push(parse_line(CONNECTED)),
            vec![SessionEvent::RemoteAccess(RemoteAccess::On { url: None })]
        );
    }

    #[test]
    fn the_url_is_forgotten_when_turning_remote_control_on_fails() {
        let mut t = Translator::new();
        t.track("on1", Tracked::RemoteOn);
        t.track("on2", Tracked::RemoteOn);
        for line in [READY, &remote_reply("on1", Some(URL)), CONNECTED] {
            t.push(parse_line(line));
        }
        let failed = r#"{"type":"control_response","response":{"subtype":"error","request_id":"on2","error":"no"}}"#;
        t.push(parse_line(failed));
        assert_eq!(
            t.push(parse_line(CONNECTED)),
            vec![SessionEvent::RemoteAccess(RemoteAccess::On { url: None })]
        );
    }

    #[test]
    fn a_rename_is_reported_once() {
        let mut t = Translator::new();
        t.track("probe_2", Tracked::Rename("Probe rename B".into()));
        let changed = r#"{"type":"system","subtype":"session_title_changed","title":"Probe rename B","session_id":"s1"}"#;
        let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"probe_2"}}"#;
        let mut events = t.push(parse_line(changed));
        events.extend(t.push(parse_line(reply)));
        assert_eq!(
            events,
            vec![SessionEvent::TitleChanged("Probe rename B".into())]
        );
    }

    /// Tracks `t1` as a title request and feeds its reply: a success with `body` as its
    /// `response` (none when `None`), or the refusal "no" when `body` is `"error"`.
    fn title_reply(body: Option<&str>) -> Vec<SessionEvent> {
        let mut t = Translator::new();
        t.track("t1", Tracked::SuggestTitle);
        let line = match body {
            Some("error") => r#"{"type":"control_response","response":{"subtype":"error","request_id":"t1","error":"no"}}"#.to_string(),
            Some(body) => format!(
                r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"t1","response":{body}}}}}"#
            ),
            None => r#"{"type":"control_response","response":{"subtype":"success","request_id":"t1"}}"#.to_string(),
        };
        t.push(parse_line(&line))
    }

    #[test]
    fn a_suggested_title_is_reported_when_there_is_one() {
        assert_eq!(
            title_reply(Some(r#"{"title":"Listing the folder"}"#)),
            vec![SessionEvent::TitleChanged("Listing the folder".into())]
        );
        assert_eq!(
            title_reply(Some(r#"{"title":"  Listing the folder\n"}"#)),
            vec![SessionEvent::TitleChanged("Listing the folder".into())],
            "reported trimmed"
        );
        assert_eq!(title_reply(Some(r#"{"title":null}"#)), vec![]);
        assert_eq!(title_reply(Some(r#"{"title":"  \n "}"#)), vec![]);
        assert_eq!(title_reply(Some(r#"{"title":""}"#)), vec![]);
        assert_eq!(title_reply(Some(r#"{}"#)), vec![]);
        assert_eq!(title_reply(None), vec![]);
        assert_eq!(
            title_reply(Some("error")),
            vec![SessionEvent::CommandFailed {
                what: CommandKind::SuggestTitle,
                reason: "no".into()
            }]
        );
    }

    #[test]
    fn a_suggested_title_is_reported_once_whichever_record_comes_first() {
        let changed = r#"{"type":"system","subtype":"session_title_changed","title":"Listing the folder","session_id":"s1"}"#;
        let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"t1","response":{"title":"Listing the folder"}}}"#;
        let once = vec![SessionEvent::TitleChanged("Listing the folder".into())];
        for order in [[changed, reply], [reply, changed]] {
            let mut t = Translator::new();
            t.track("t1", Tracked::SuggestTitle);
            let events: Vec<_> = order.iter().flat_map(|l| t.push(parse_line(l))).collect();
            assert_eq!(events, once, "{order:?}");
        }
    }

    #[test]
    fn a_model_change_reports_the_resolved_name() {
        let mut t = Translator::new();
        t.track("probe_2", Tracked::SetModel("sonnet".into()));
        let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"probe_2","response":{"model":"claude-sonnet-5-5"}}}"#;
        assert_eq!(
            t.push(parse_line(reply)),
            vec![SessionEvent::ModelChanged("claude-sonnet-5-5".into())]
        );
    }

    /// Tracks `u1` as a usage request and feeds the successful reply whose
    /// `response` is `body`.
    fn usage_reply(body: &str) -> Vec<SessionEvent> {
        let mut t = Translator::new();
        t.track("u1", Tracked::Usage);
        t.push(parse_line(&format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"u1","response":{body}}}}}"#
        )))
    }

    #[test]
    fn a_usage_reply_becomes_usage_limits() {
        let body = r#"{"subscription_type":"max","rate_limits_available":true,"rate_limits":{
            "five_hour":{"utilization":37,"resets_at":"2026-11-11T08:19:59.618411+00:00"},
            "seven_day":{"utilization":10,"resets_at":"2026-11-15T20:59:59.618435+00:00"}}}"#;
        assert_eq!(
            usage_reply(body),
            vec![SessionEvent::UsageLimits(UsageLimits {
                session: Some(LimitWindow {
                    used: 0.37,
                    resets_at_unix: 1794385199
                }),
                weekly: Some(LimitWindow {
                    used: 0.10,
                    resets_at_unix: 1794776399
                }),
            })]
        );
    }

    #[test]
    fn a_usage_reply_without_numbers_reports_nothing_as_known() {
        assert_eq!(
            usage_reply(r#"{"rate_limits_available":false}"#),
            vec![SessionEvent::UsageLimits(UsageLimits {
                session: None,
                weekly: None
            })]
        );
        let one = r#"{"rate_limits":{"five_hour":{"utilization":"lots","resets_at":"soon"},
            "seven_day":{"utilization":101.5,"resets_at":"2026-11-15T20:59:59Z"}}}"#;
        assert_eq!(
            usage_reply(one),
            vec![SessionEvent::UsageLimits(UsageLimits {
                session: None,
                weekly: Some(LimitWindow {
                    used: 1.0,
                    resets_at_unix: 1794776399
                }),
            })]
        );
    }

    #[test]
    fn a_refused_usage_request_is_a_command_failure() {
        let mut t = Translator::new();
        t.track("u1", Tracked::Usage);
        let refused = r#"{"type":"control_response","response":{"subtype":"error","request_id":"u1","error":"not signed in"}}"#;
        assert_eq!(
            t.push(parse_line(refused)),
            vec![SessionEvent::CommandFailed {
                what: CommandKind::RefreshUsage,
                reason: "not signed in".into()
            }]
        );
    }

    #[test]
    fn usage_limits_from_the_live_recording() {
        let l = r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":0.08,"resetsAt":1794385200},"seven_day":{"utilization":0.06,"resetsAt":1794776400}}}}"#;
        assert_eq!(
            feed(&[l]),
            vec![SessionEvent::UsageLimits(UsageLimits {
                session: Some(LimitWindow {
                    used: 0.08,
                    resets_at_unix: 1794385200
                }),
                weekly: Some(LimitWindow {
                    used: 0.06,
                    resets_at_unix: 1794776400
                })
            })]
        );
    }

    #[test]
    fn a_refused_remote_control_request_says_a_login_is_needed() {
        let mut t = Translator::new();
        t.track("rc_1", Tracked::RemoteOn);
        let refused = parse_line(
            r#"{"type":"control_response","response":{"subtype":"error","request_id":"rc_1","error":"Claude.ai login was rejected","error_code":"remote_control_auth"}}"#,
        );
        assert_eq!(
            t.push(refused.clone()),
            vec![SessionEvent::RemoteAccess(RemoteAccess::Failed {
                reason: "Claude.ai login was rejected".into(),
                why: RemoteFailure::Login
            })]
        );
        assert!(t.push(refused).is_empty(), "a tracked id is used once");
    }

    #[test]
    fn a_failed_bridge_state_is_a_failure() {
        let l = r#"{"type":"system","subtype":"bridge_state","state":"failed","detail":"Claude.ai login was rejected","failure_kind":"auth"}"#;
        assert_eq!(
            feed(&[l]),
            vec![SessionEvent::RemoteAccess(RemoteAccess::Failed {
                reason: "Claude.ai login was rejected".into(),
                why: RemoteFailure::Login
            })]
        );
        assert!(
            feed(&[
                r#"{"type":"system","subtype":"bridge_state","state":"a-state-nobody-has-seen"}"#
            ])
            .is_empty()
        );
    }

    #[test]
    fn accepted_requests_confirm_what_was_asked() {
        let ok = |id: &str| {
            parse_line(&format!(
                r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{id}"}}}}"#
            ))
        };
        let mut t = Translator::new();
        t.track("a", Tracked::RemoteOn);
        t.track("b", Tracked::RemoteOff);
        t.track("c", Tracked::SetModel("opus".into()));
        t.track("d", Tracked::SetMode(PermissionMode::Plan));
        t.track("e", Tracked::Rename("New".into()));
        assert_eq!(
            t.push(ok("a")),
            vec![SessionEvent::RemoteAccess(RemoteAccess::Starting)]
        );
        assert_eq!(
            t.push(ok("b")),
            vec![SessionEvent::RemoteAccess(RemoteAccess::Off)]
        );
        assert_eq!(
            t.push(ok("c")),
            vec![SessionEvent::ModelChanged("opus".into())]
        );
        assert_eq!(
            t.push(ok("d")),
            vec![SessionEvent::PermissionModeChanged(PermissionMode::Plan)]
        );
        assert_eq!(
            t.push(ok("e")),
            vec![SessionEvent::TitleChanged("New".into())]
        );
        assert!(t.push(ok("never-tracked")).is_empty());
    }

    #[test]
    fn a_refused_model_change_is_reported() {
        let mut t = Translator::new();
        t.track("m", Tracked::SetModel("nope".into()));
        let refused = parse_line(
            r#"{"type":"control_response","response":{"subtype":"error","request_id":"m","error":"unknown model"}}"#,
        );
        assert_eq!(
            t.push(refused),
            vec![SessionEvent::CommandFailed {
                what: CommandKind::SetModel,
                reason: "unknown model".into()
            }]
        );
    }

    #[test]
    fn a_mode_change_is_reported_once_whichever_record_comes_first() {
        let mut t = Translator::new();
        t.push(parse_line(INIT));
        t.track("p", Tracked::SetMode(PermissionMode::Plan));
        let status = r#"{"type":"system","subtype":"status","status":null,"permissionMode":"plan","session_id":"s1"}"#;
        let reply =
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"p"}}"#;
        let mut events = t.push(parse_line(status));
        events.extend(t.push(parse_line(reply)));
        assert_eq!(
            events,
            vec![SessionEvent::PermissionModeChanged(PermissionMode::Plan)]
        );
    }

    #[test]
    fn a_model_the_session_already_uses_is_not_reported_again() {
        let mut t = Translator::new();
        t.push(parse_line(INIT));
        t.track("m", Tracked::SetModel("haiku".into()));
        let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"m","response":{"model":"claude-haiku-5-5"}}}"#;
        assert!(t.push(parse_line(reply)).is_empty());
        // The same kind of reply naming another model is reported, so the
        // silence above came from a matched reply.
        t.track("m2", Tracked::SetModel("sonnet".into()));
        let other = r#"{"type":"control_response","response":{"subtype":"success","request_id":"m2","response":{"model":"claude-sonnet-5-5"}}}"#;
        assert_eq!(
            t.push(parse_line(other)),
            vec![SessionEvent::ModelChanged("claude-sonnet-5-5".into())]
        );
    }

    #[test]
    fn a_title_announced_again_is_not_reported_again() {
        let l = r#"{"type":"system","subtype":"session_title_changed","title":"Same"}"#;
        let other = r#"{"type":"system","subtype":"session_title_changed","title":"Other"}"#;
        assert_eq!(
            feed(&[l, l, other]),
            vec![
                SessionEvent::TitleChanged("Same".into()),
                SessionEvent::TitleChanged("Other".into())
            ]
        );
    }

    #[test]
    fn replies_to_other_requests_and_failures_of_each_kind() {
        let err = |id: &str| {
            parse_line(&format!(
                r#"{{"type":"control_response","response":{{"subtype":"error","request_id":"{id}","error":"no"}}}}"#
            ))
        };
        let mut t = Translator::new();
        t.track("o", Tracked::Other);
        t.track("r", Tracked::RemoteOff);
        t.track("d", Tracked::SetMode(PermissionMode::Plan));
        t.track("e", Tracked::Rename("x".into()));
        assert!(t.push(err("o")).is_empty());
        for (id, what) in [
            ("r", CommandKind::SetRemoteAccess),
            ("d", CommandKind::SetPermissionMode),
            ("e", CommandKind::Rename),
        ] {
            assert_eq!(
                t.push(err(id)),
                vec![SessionEvent::CommandFailed {
                    what,
                    reason: "no".into()
                }]
            );
        }
        // A failed request leaves what was known unchanged: the next init
        // names the same model as before, so there is nothing to report.
        t.push(parse_line(INIT));
        t.track("m", Tracked::SetModel("opus".into()));
        assert_eq!(
            t.push(err("m")),
            vec![SessionEvent::CommandFailed {
                what: CommandKind::SetModel,
                reason: "no".into()
            }]
        );
        assert!(t.push(parse_line(INIT)).is_empty());
        assert!(t.push(err("never")).is_empty());
    }

    #[test]
    fn bridge_states_without_a_url_and_without_a_detail() {
        let connected = r#"{"type":"system","subtype":"bridge_state","state":"connected"}"#;
        let failed = r#"{"type":"system","subtype":"bridge_state","state":"failed"}"#;
        assert_eq!(
            feed(&[connected, failed]),
            vec![
                SessionEvent::RemoteAccess(RemoteAccess::On { url: None }),
                SessionEvent::RemoteAccess(RemoteAccess::Failed {
                    reason: "Remote Control failed".into(),
                    why: RemoteFailure::Other
                }),
            ]
        );
    }

    /// A `bridge_state` line with these fields after `state`, as the program writes it.
    fn bridge(state: &str, rest: &str) -> String {
        format!(
            r#"{{"type":"system","subtype":"bridge_state","state":"{state}"{rest},"bridge_epoch":1,"session_id":"s1"}}"#
        )
    }

    /// The event a refused request to turn Remote Control on gives, with these fields after
    /// `request_id`.
    fn remote_refused(rest: &str) -> Vec<SessionEvent> {
        let mut t = Translator::new();
        t.track("rc", Tracked::RemoteOn);
        t.push(parse_line(&format!(
            r#"{{"type":"control_response","response":{{"subtype":"error","request_id":"rc"{rest}}}}}"#
        )))
    }

    fn remote(access: RemoteAccess) -> Vec<SessionEvent> {
        vec![SessionEvent::RemoteAccess(access)]
    }

    fn failed(reason: &str, why: RemoteFailure) -> Vec<SessionEvent> {
        remote(RemoteAccess::Failed {
            reason: reason.into(),
            why,
        })
    }

    #[test]
    fn a_dropped_connection_reconnects_and_comes_back() {
        // Never seen live: `reconnecting` and `recovered` are written from the program.
        let mut t = Translator::new();
        t.track("on", Tracked::RemoteOn);
        for line in [READY, &remote_reply("on", Some(URL)), CONNECTED] {
            t.push(parse_line(line));
        }
        assert_eq!(
            t.push(parse_line(&bridge(
                "reconnecting",
                r#","detail":"The connection dropped""#
            ))),
            remote(RemoteAccess::Reconnecting)
        );
        assert_eq!(
            t.push(parse_line(&bridge("connected", r#","recovered":true"#))),
            remote(RemoteAccess::On {
                url: Some(URL.into())
            })
        );
    }

    #[test]
    fn a_policy_that_forbids_remote_control_is_a_failure_of_its_own() {
        // Never seen live: `policy_disabled` is written from the program.
        assert_eq!(
            feed(&[&bridge(
                "policy_disabled",
                r#","detail":"Your organisation turned it off""#
            )]),
            failed("Your organisation turned it off", RemoteFailure::Policy)
        );
        assert_eq!(
            feed(&[&bridge("policy_disabled", "")]),
            failed(
                "Remote Control is turned off by policy",
                RemoteFailure::Policy
            )
        );
    }

    #[test]
    fn each_kind_of_failure_says_why() {
        // Never seen live: no failure was recorded. The lines are written from the program.
        let state = |kind: &str| {
            feed(&[&bridge(
                "failed",
                &format!(r#","detail":"It stopped"{kind}"#),
            )])
        };
        assert_eq!(
            state(r#","failure_kind":"auth""#),
            failed("It stopped", RemoteFailure::Login)
        );
        assert_eq!(
            state(r#","failure_kind":"ended_elsewhere""#),
            failed("It stopped", RemoteFailure::Elsewhere)
        );
        for other in [
            r#","failure_kind":"terminal""#,
            r#","failure_kind":"network""#,
            "",
        ] {
            assert_eq!(state(other), failed("It stopped", RemoteFailure::Other));
        }
        // The reason is passed on as given, even when it is not a sentence.
        assert_eq!(
            feed(&[&bridge(
                "failed",
                r#","detail":"/login","failure_kind":"auth""#
            )]),
            failed("/login", RemoteFailure::Login)
        );

        let code = |code: &str| remote_refused(&format!(r#","error":"Refused"{code}"#));
        assert_eq!(
            code(r#","error_code":"remote_control_auth""#),
            failed("Refused", RemoteFailure::Login)
        );
        assert_eq!(
            code(r#","error_code":"remote_control_policy_disabled""#),
            failed("Refused", RemoteFailure::Policy)
        );
        assert_eq!(
            code(r#","error_code":"remote_control_ended_elsewhere""#),
            failed("Refused", RemoteFailure::Elsewhere)
        );
        for other in [r#","error_code":"remote_control_terminal""#, ""] {
            assert_eq!(code(other), failed("Refused", RemoteFailure::Other));
        }
        assert_eq!(
            remote_refused(r#","error":"/login","error_code":"remote_control_auth""#),
            failed("/login", RemoteFailure::Login)
        );
    }

    #[test]
    fn an_unknown_state_still_says_nothing() {
        let mut t = Translator::new();
        t.track("on", Tracked::RemoteOn);
        for line in [READY, &remote_reply("on", Some(URL)), CONNECTED] {
            t.push(parse_line(line));
        }
        assert!(
            t.push(parse_line(&bridge(
                "a-state-nobody-has-seen",
                r#","detail":"Something""#
            )))
            .is_empty()
        );
        assert_eq!(
            t.push(parse_line(CONNECTED)),
            remote(RemoteAccess::On {
                url: Some(URL.into())
            })
        );
    }

    /// The `context_used` of every finished turn, in order.
    fn context_used(events: &[SessionEvent]) -> Vec<Option<f32>> {
        events
            .iter()
            .filter_map(|event| match event {
                SessionEvent::TurnFinished(summary) => Some(summary.context_used),
                _ => None,
            })
            .collect()
    }

    /// `used` tokens over a window of `window`, as a fraction.
    fn fraction(used: u64, window: u64) -> Option<f32> {
        Some((used as f64 / window as f64) as f32)
    }

    #[test]
    fn context_counts_the_input_side_only() {
        // V1's two turns: input, cache creation and cache read of the last message, over the
        // window of its model. Its 4 output tokens are not counted.
        assert_eq!(
            context_used(&replay("v1-remote.jsonl")),
            vec![
                fraction(2 + 14281 + 12932, 1_000_000),
                fraction(2 + 12142 + 16420, 1_000_000)
            ]
        );
    }

    /// An `assistant` record of `model` whose usage puts `input` tokens and 49,000 of cache in
    /// the context, or with no usage at all. Written by hand; a record without usage was never
    /// seen live.
    fn reply_of(model: &str, input: Option<u64>) -> String {
        let usage = input.map_or(String::new(), |input| {
            format!(
                r#","usage":{{"input_tokens":{input},"cache_creation_input_tokens":9000,"cache_read_input_tokens":40000,"output_tokens":5000}}"#
            )
        });
        format!(
            r#"{{"type":"assistant","message":{{"id":"m","model":"{model}","content":[]{usage}}}}}"#
        )
    }

    const HAIKU_RESULT: &str = r#"{"type":"result","is_error":false,"duration_ms":5,"total_cost_usd":0.5,"result":"ok","modelUsage":{"claude-haiku-5-5":{"contextWindow":200000}}}"#;

    #[test]
    fn a_record_without_usage_keeps_the_last_reading() {
        let lines = [
            reply_of("claude-haiku-5-5", Some(1000)),
            reply_of("claude-haiku-5-5", None),
            HAIKU_RESULT.to_string(),
        ];
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        assert_eq!(context_used(&feed(&lines)), vec![fraction(50_000, 200_000)]);
    }

    #[test]
    fn a_synthetic_record_does_not_count_as_context() {
        // A record the program makes itself, as when a turn is stopped, names the model
        // `<synthetic>` and has a usage of nothing. Written by hand: never seen live.
        let synthetic = r#"{"type":"assistant","message":{"id":"m","model":"<synthetic>","content":[],"usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0}}}"#;
        let lines = [
            reply_of("claude-haiku-5-5", Some(1000)),
            synthetic.to_string(),
            HAIKU_RESULT.to_string(),
        ];
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        assert_eq!(context_used(&feed(&lines)), vec![fraction(50_000, 200_000)]);
    }

    #[test]
    fn context_is_measured_against_the_window_of_the_model_in_use() {
        let message = |model: &str, input: u64| {
            format!(
                r#"{{"type":"assistant","message":{{"id":"m","model":"{model}","content":[],"usage":{{"input_tokens":{input},"cache_creation_input_tokens":9000,"cache_read_input_tokens":40000,"output_tokens":5000}}}}}}"#
            )
        };
        let result = r#"{"type":"result","is_error":false,"duration_ms":5,"total_cost_usd":0.5,"result":"ok","modelUsage":{"claude-sonnet-5-5":{"contextWindow":1000000},"claude-haiku-5-5":{"contextWindow":200000}}}"#;
        let turns = [
            message("claude-haiku-5-5", 1000),
            result.to_string(),
            message("claude-sonnet-5-5", 1000),
            result.to_string(),
            // A model the result does not name is measured against the largest window.
            message("claude-opus-5-5", 1000),
            result.to_string(),
            // More than the window is all of it.
            message("claude-haiku-5-5", 400_000),
            result.to_string(),
        ];
        let lines: Vec<&str> = turns.iter().map(String::as_str).collect();
        assert_eq!(
            context_used(&feed(&lines)),
            vec![
                fraction(50_000, 200_000),
                fraction(50_000, 1_000_000),
                fraction(50_000, 1_000_000),
                Some(1.0)
            ]
        );
    }

    /// Every event one recording gives, through a fresh `Translator::new()`.
    fn replay(name: &str) -> Vec<SessionEvent> {
        let path = format!("{}/tests/fixtures/live/{name}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap();
        let mut t = Translator::new();
        text.lines().flat_map(|l| t.push(parse_line(l))).collect()
    }

    /// Per turn of one recording, the text the translator streamed and the
    /// text its `assistant` records hold. A turn ends at a `result` record.
    fn text_per_turn(name: &str) -> (Vec<String>, Vec<String>) {
        let path = format!(
            "{}/tests/fixtures/live/{name}.jsonl",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).unwrap();
        let mut t = Translator::new();
        let (mut streamed, mut recorded) = (Vec::new(), Vec::new());
        let (mut s, mut r) = (String::new(), String::new());
        for line in text.lines() {
            for event in t.push(parse_line(line)) {
                if let SessionEvent::TextDelta(text) = event {
                    s.push_str(&text);
                }
            }
            let Ok(record) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            match record.get("type").and_then(Value::as_str) {
                Some("assistant")
                    if record.get("parent_tool_use_id").is_none_or(Value::is_null) =>
                {
                    let blocks = record.pointer("/message/content").and_then(Value::as_array);
                    for block in blocks.into_iter().flatten() {
                        if block.get("type").and_then(Value::as_str) == Some("text") {
                            r.push_str(block.get("text").and_then(Value::as_str).unwrap_or(""));
                        }
                    }
                }
                Some("result") => {
                    let stopped = record
                        .get("terminal_reason")
                        .and_then(Value::as_str)
                        .is_some_and(|reason| reason.starts_with("aborted"));
                    // A stopped turn's stream is cut first: its text is a prefix.
                    let (got, held) = (std::mem::take(&mut s), std::mem::take(&mut r));
                    if stopped {
                        assert!(
                            held.starts_with(&got),
                            "{name}: a stopped turn streamed {got:?}, held {held:?}"
                        );
                        // The prefix holds, so the caller can compare the two for equality.
                        streamed.push(held.clone());
                    } else {
                        streamed.push(got);
                    }
                    recorded.push(held);
                }
                _ => {}
            }
        }
        (streamed, recorded)
    }

    #[test]
    fn every_recording_streams_exactly_the_text_its_records_hold() {
        for name in [
            "v1-remote",
            "v2-permission",
            "v4-image-model-stop",
            "v7-question-plan",
            "v9-control",
            "v11-resume",
            "v11-fork",
        ] {
            let (streamed, recorded) = text_per_turn(name);
            assert_eq!(streamed, recorded, "{name}");
        }
    }

    /// The properties every real recording must have.
    fn check_recording(name: &str, events: &[SessionEvent]) {
        let started = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::Started(_)))
            .count();
        assert_eq!(started, 1, "{name}: one Started");
        assert!(
            matches!(events.first(), Some(SessionEvent::Started(_))),
            "{name}: Started first"
        );
        let mut turn_open = false;
        let mut turns = 0;
        let mut finished_tools = std::collections::HashSet::new();
        for (i, event) in events.iter().enumerate() {
            match event {
                SessionEvent::TurnStarted => {
                    assert!(!turn_open, "{name}: event {i}: a turn inside a turn");
                    turn_open = true;
                    turns += 1;
                }
                SessionEvent::TurnFinished(_) => {
                    assert!(turn_open, "{name}: event {i}: no turn to finish");
                    turn_open = false;
                }
                SessionEvent::TurnFailed { reason } => {
                    panic!("{name}: event {i}: a turn failed: {reason}")
                }
                SessionEvent::ToolFinished { id, .. } => assert!(
                    finished_tools.insert(id.clone()),
                    "{name}: event {i}: {id:?} finished twice"
                ),
                SessionEvent::ThinkingDelta(_) => {
                    panic!("{name}: event {i}: thinking is empty live")
                }
                SessionEvent::TextDelta(text) => {
                    assert!(!text.is_empty(), "{name}: event {i}: empty text")
                }
                SessionEvent::BlockFinished => assert_ne!(
                    events.get(i + 1),
                    Some(&SessionEvent::BlockFinished),
                    "{name}: event {i}: two BlockFinished in a row"
                ),
                _ => {}
            }
        }
        assert!(!turn_open, "{name}: the last turn never ended");
        assert!(turns > 0, "{name}: has turns");
    }

    #[test]
    fn the_permission_recording_translates_cleanly() {
        let events = replay("v2-permission.jsonl");
        check_recording("V2", &events);
    }

    #[test]
    fn the_stopped_turn_recording_finishes_one_turn_as_interrupted() {
        let events = replay("v4-image-model-stop.jsonl");
        check_recording("V4", &events);
        let interrupted = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::TurnFinished(s) if s.interrupted))
            .count();
        assert_eq!(interrupted, 1);
    }

    #[test]
    fn the_question_and_plan_recording_withdraws_and_changes_mode() {
        let events = replay("v7-question-plan.jsonl");
        check_recording("V7", &events);
        let withdrawn = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::RequestWithdrawn { .. }))
            .count();
        assert!(withdrawn >= 2, "{withdrawn} withdrawn");
        let modes: Vec<&PermissionMode> = events
            .iter()
            .filter_map(|e| match e {
                SessionEvent::PermissionModeChanged(mode) => Some(mode),
                _ => None,
            })
            .collect();
        let plan = modes
            .iter()
            .position(|m| **m == PermissionMode::Plan)
            .expect("a change to Plan");
        assert!(
            modes[plan..].contains(&&PermissionMode::Ask),
            "and back to Ask: {modes:?}"
        );
    }

    // The replies to `apply_flag_settings` and `get_settings` below are written by hand from
    // what the program says it sends: never seen live. Their values are made up.

    /// A reply to the request `id` that succeeded with no body.
    fn succeeded(id: &str) -> String {
        format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{id}"}}}}"#
        )
    }

    fn refused(id: &str, error: &str) -> String {
        format!(
            r#"{{"type":"control_response","response":{{"subtype":"error","request_id":"{id}","error":"{error}"}}}}"#
        )
    }

    /// A reply to `get_settings` whose `applied.effort` is `effort`, among settings of the
    /// user's that are not read.
    fn settings(id: &str, effort: Value) -> String {
        json!({"type": "control_response", "response": {
        "subtype": "success", "request_id": id, "response": {
            "effective": {"effortLevel": "low", "theme": "made-up-theme"},
            "sources": [{"source": "userSettings",
                         "settings": {"effortLevel": "low", "theme": "made-up-theme"}}],
            "applied": {"model": "claude-x-1", "effort": effort},
        }}})
        .to_string()
    }

    /// Sends what the translator wants sent next as the adapter does, tracked under `id`.
    /// It must be one reading of the level, after a change to `asked` when there is one.
    fn send_read(t: &mut Translator, id: &str, asked: Option<&str>) {
        let tracked = Tracked::Settings {
            asked: asked.map(str::to_string),
        };
        assert_eq!(
            t.take_requests(),
            vec![(tracked.clone(), crate::encode::get_settings())]
        );
        t.track(id, tracked);
    }

    /// Sends what the translator wants sent after the session starts or a turn ends, as the
    /// adapter does: a reading of the level, tracked under `id`, and a request for the
    /// breakdown of the context, which is left untracked.
    fn send_reads(t: &mut Translator, id: &str) {
        let tracked = Tracked::Settings { asked: None };
        assert_eq!(
            t.take_requests(),
            vec![
                (tracked.clone(), crate::encode::get_settings()),
                (Tracked::ContextUsage, crate::encode::get_context_usage()),
            ]
        );
        t.track(id, tracked);
    }

    /// A session started from the recorded `initialize` reply, whose first reading of the
    /// level said `level`.
    fn started_with(level: Option<&str>) -> Translator {
        let mut t = opening(None, None);
        t.push(parse_line(initialize_reply()));
        send_reads(&mut t, "read_0");
        assert_eq!(
            t.push(parse_line(&settings("read_0", json!(level)))),
            vec![SessionEvent::EffortChanged(level.map(str::to_string))]
        );
        t
    }

    fn effort(level: Option<&str>) -> SessionEvent {
        SessionEvent::EffortChanged(level.map(str::to_string))
    }

    fn not_taken(reason: &str) -> SessionEvent {
        SessionEvent::CommandFailed {
            what: CommandKind::SetEffort,
            reason: reason.into(),
        }
    }

    #[test]
    fn the_model_list_says_which_levels_a_model_takes() {
        let mut t = opening(None, None);
        let info = started_info(&t.push(parse_line(initialize_reply())));
        for model in &info.models {
            assert_eq!(
                model.effort_levels,
                vec!["low", "medium", "high", "xhigh", "max"],
                "{}",
                model.id
            );
        }
        let levels = |models: &str| -> Vec<Vec<String>> {
            let mut t = opening(None, None);
            let reply = format!(
                r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"probe_1","response":{{"models":{models}}}}}}}"#
            );
            started_info(&t.push(parse_line(&reply)))
                .models
                .into_iter()
                .map(|option| option.effort_levels)
                .collect()
        };
        assert_eq!(
            levels(
                r#"[{"value":"a","supportsEffort":false,"supportedEffortLevels":["low","high"]},
                    {"value":"b","supportedEffortLevels":["low","high"]},
                    {"value":"c","supportsEffort":true},
                    {"value":"d","supportsEffort":true,"supportedEffortLevels":["high",7,"low"]}]"#
            ),
            vec![
                vec![],
                vec![],
                vec![],
                vec!["high".to_string(), "low".to_string()]
            ]
        );
    }

    #[test]
    fn the_level_is_read_after_the_session_starts() {
        let mut t = opening(None, None);
        assert!(t.take_requests().is_empty());
        started_info(&t.push(parse_line(initialize_reply())));
        send_reads(&mut t, "rc_2");
        assert!(t.take_requests().is_empty(), "the list is emptied");
        assert_eq!(
            t.push(parse_line(&settings("rc_2", json!("high")))),
            vec![effort(Some("high"))]
        );
        assert!(
            !format!("{t:?}").contains("made-up-theme"),
            "nothing else of the user's settings is kept"
        );

        let mut t = opening(None, None);
        t.push(parse_line(initialize_reply()));
        send_reads(&mut t, "rc_2");
        assert_eq!(
            t.push(parse_line(&settings("rc_2", Value::Null))),
            vec![effort(None)]
        );
    }

    #[test]
    fn a_level_read_again_and_unchanged_says_nothing() {
        for level in [Some("high"), None] {
            let mut t = started_with(level);
            t.push(parse_line(RESULT_OK));
            send_reads(&mut t, "read_1");
            assert!(
                t.push(parse_line(&settings("read_1", json!(level))))
                    .is_empty()
            );
        }
        // A level that is not text is none.
        let mut t = started_with(None);
        t.push(parse_line(RESULT_OK));
        send_reads(&mut t, "read_1");
        assert!(t.push(parse_line(&settings("read_1", json!(3)))).is_empty());
        // A change is said.
        t.push(parse_line(RESULT_OK));
        send_reads(&mut t, "read_2");
        assert_eq!(
            t.push(parse_line(&settings("read_2", json!("low")))),
            vec![effort(Some("low"))]
        );
    }

    #[test]
    fn the_level_is_read_after_every_turn_and_after_a_change_of_model() {
        let mut t = started_with(Some("high"));
        for line in [
            INIT,
            MSG_START,
            TEXT_START,
            DELTA_OK,
            ASSISTANT_OK,
            BLOCK_STOP,
            MSG_STOP,
        ] {
            t.push(parse_line(line));
            assert!(
                t.take_requests().is_empty(),
                "nothing is read inside a turn"
            );
        }
        t.push(parse_line(RESULT_OK));
        send_reads(&mut t, "read_1");
        // A turn that failed ends too.
        let failed = r#"{"type":"result","is_error":true,"subtype":"error_during_execution","result":"it broke"}"#;
        t.push(parse_line(failed));
        send_reads(&mut t, "read_2");

        t.track("model_1", Tracked::SetModel("opus".into()));
        assert_eq!(
            t.push(parse_line(&succeeded("model_1"))),
            vec![SessionEvent::ModelChanged("opus".into())]
        );
        send_read(&mut t, "read_3", None);
        // The new model goes on with another level.
        assert_eq!(
            t.push(parse_line(&settings("read_3", json!("medium")))),
            vec![effort(Some("medium"))]
        );
        // A change of model that was refused changed nothing, so nothing is read.
        t.track("model_2", Tracked::SetModel("haiku".into()));
        t.push(parse_line(&refused("model_2", "no such model")));
        assert!(t.take_requests().is_empty());
    }

    #[test]
    fn a_change_of_effort_is_confirmed_by_reading_it() {
        let mut t = started_with(Some("medium"));
        t.track("effort_1", Tracked::SetEffort("high".into()));
        assert!(t.push(parse_line(&succeeded("effort_1"))).is_empty());
        send_read(&mut t, "read_1", Some("high"));
        assert_eq!(
            t.push(parse_line(&settings("read_1", json!("high")))),
            vec![effort(Some("high"))]
        );
    }

    #[test]
    fn a_level_that_was_not_taken_says_so() {
        // Asked for max, the session goes on with high, which it had before.
        let mut t = started_with(Some("high"));
        t.track("effort_1", Tracked::SetEffort("max".into()));
        t.push(parse_line(&succeeded("effort_1")));
        send_read(&mut t, "read_1", Some("max"));
        assert_eq!(
            t.push(parse_line(&settings("read_1", json!("high")))),
            vec![not_taken("Claude Code goes on with high")]
        );
        // Asked for max, the session goes on with high, which it did not have before.
        let mut t = started_with(Some("medium"));
        t.track("effort_1", Tracked::SetEffort("max".into()));
        t.push(parse_line(&succeeded("effort_1")));
        send_read(&mut t, "read_1", Some("max"));
        assert_eq!(
            t.push(parse_line(&settings("read_1", json!("high")))),
            vec![
                not_taken("Claude Code goes on with high"),
                effort(Some("high"))
            ]
        );
        // Asked for low, the session goes on with no level at all.
        let mut t = started_with(Some("medium"));
        t.track("effort_1", Tracked::SetEffort("low".into()));
        t.push(parse_line(&succeeded("effort_1")));
        send_read(&mut t, "read_1", Some("low"));
        assert_eq!(
            t.push(parse_line(&settings("read_1", Value::Null))),
            vec![
                not_taken("Claude Code goes on with no effort level"),
                effort(None)
            ]
        );
    }

    #[test]
    fn two_choices_in_a_row_report_only_the_last() {
        let mut t = started_with(Some("medium"));
        t.track("effort_1", Tracked::SetEffort("high".into()));
        t.track("effort_2", Tracked::SetEffort("low".into()));
        assert!(t.push(parse_line(&succeeded("effort_1"))).is_empty());
        send_read(&mut t, "read_1", Some("high"));
        assert!(t.push(parse_line(&succeeded("effort_2"))).is_empty());
        send_read(&mut t, "read_2", Some("low"));
        // The program took both before the first read: that read finds the last choice.
        assert_eq!(
            t.push(parse_line(&settings("read_1", json!("low")))),
            vec![effort(Some("low"))]
        );
        assert!(
            t.push(parse_line(&settings("read_2", json!("low"))))
                .is_empty()
        );
        // A failed read after an older choice says nothing either.
        let mut t = started_with(Some("medium"));
        t.track("effort_1", Tracked::SetEffort("high".into()));
        t.track("effort_2", Tracked::SetEffort("low".into()));
        t.push(parse_line(&succeeded("effort_1")));
        send_read(&mut t, "read_1", Some("high"));
        assert!(
            t.push(parse_line(&refused("read_1", "settings are not ready")))
                .is_empty()
        );
    }

    #[test]
    fn a_refused_change_of_effort_fails() {
        let mut t = started_with(Some("medium"));
        t.track("effort_1", Tracked::SetEffort("high".into()));
        assert_eq!(
            t.push(parse_line(&refused(
                "effort_1",
                "settings must be an object"
            ))),
            vec![not_taken("settings must be an object")]
        );
        assert!(t.take_requests().is_empty());
    }

    #[test]
    fn a_failed_read_after_a_change_fails_the_change() {
        let mut t = started_with(Some("medium"));
        t.track("effort_1", Tracked::SetEffort("high".into()));
        t.push(parse_line(&succeeded("effort_1")));
        send_read(&mut t, "read_1", Some("high"));
        assert_eq!(
            t.push(parse_line(&refused("read_1", "settings are not ready"))),
            vec![not_taken("settings are not ready")]
        );
    }

    #[test]
    fn a_failed_read_nobody_asked_for_says_nothing() {
        let mut t = opening(None, None);
        t.push(parse_line(initialize_reply()));
        send_reads(&mut t, "read_0");
        assert!(
            t.push(parse_line(&refused("read_0", "settings are not ready")))
                .is_empty()
        );
        // A failed read is no reading: the next one is still the first.
        t.push(parse_line(RESULT_OK));
        send_reads(&mut t, "read_1");
        assert_eq!(
            t.push(parse_line(&settings("read_1", json!("high")))),
            vec![effort(Some("high"))]
        );
    }

    // The replies to `get_context_usage` below are written by hand in the shape of the one
    // recorded live (V9), whose list was cut to three `used` rows; the other kinds were read
    // in the program and never seen live. Their values are made up.

    /// A reply to `get_context_usage` with `response` as its body.
    fn context_reply(id: &str, response: Value) -> String {
        json!({"type": "control_response", "response": {
            "subtype": "success", "request_id": id, "response": response}})
        .to_string()
    }

    /// A row of `categories`.
    fn category(name: &str, tokens: u64, kind: &str) -> Value {
        json!({"id": "made_up", "name": name, "tokens": tokens, "color": "inactive", "kind": kind})
    }

    fn part(name: &str, tokens: u64, kind: ContextKind) -> ContextPart {
        ContextPart {
            name: name.into(),
            tokens,
            kind,
        }
    }

    /// A translator whose breakdown request was sent and tracked under `id`.
    fn asked_for_breakdown(id: &str) -> Translator {
        let mut t = opening(None, None);
        t.push(parse_line(initialize_reply()));
        t.take_requests();
        t.track(id, Tracked::ContextUsage);
        t
    }

    #[test]
    fn a_context_breakdown_is_asked_for_after_the_start_and_after_every_turn() {
        let breakdowns = |t: &mut Translator| {
            t.take_requests()
                .into_iter()
                .filter(|(tracked, body)| {
                    *tracked == Tracked::ContextUsage && *body == crate::encode::get_context_usage()
                })
                .count()
        };
        // Started from the reply to `initialize`, or from the first `init`.
        let mut t = opening(None, None);
        assert_eq!(breakdowns(&mut t), 0);
        t.push(parse_line(initialize_reply()));
        assert_eq!(breakdowns(&mut t), 1);
        let mut t = Translator::new();
        t.push(parse_line(INIT));
        assert_eq!(breakdowns(&mut t), 1);
        // Nothing inside a turn; once at its end, whether it ended well or not.
        for line in [
            INIT,
            MSG_START,
            TEXT_START,
            DELTA_OK,
            ASSISTANT_OK,
            BLOCK_STOP,
            MSG_STOP,
        ] {
            t.push(parse_line(line));
            assert_eq!(breakdowns(&mut t), 0, "{line}");
        }
        t.push(parse_line(RESULT_OK));
        assert_eq!(breakdowns(&mut t), 1);
        let failed = r#"{"type":"result","is_error":true,"subtype":"error_during_execution","result":"it broke"}"#;
        t.push(parse_line(failed));
        assert_eq!(breakdowns(&mut t), 1);
        let stopped = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"terminal_reason":"aborted_streaming","duration_ms":5,"total_cost_usd":0.06}"#;
        t.push(parse_line(stopped));
        assert_eq!(breakdowns(&mut t), 1);
        // Not after a change of model, which reads only the level.
        t.track("model_1", Tracked::SetModel("opus".into()));
        t.push(parse_line(&succeeded("model_1")));
        assert_eq!(breakdowns(&mut t), 0);
    }

    #[test]
    fn the_breakdown_keeps_used_buffer_and_free_rows_in_order() {
        let mut t = asked_for_breakdown("ctx_1");
        let reply = context_reply(
            "ctx_1",
            json!({
                "categories": [
                    category("System prompt", 3_237, "used"),
                    category("System tools", 5_870, "used"),
                    category("MCP tools", 1_200, "deferred"),
                    category("Something new", 50, "made_up_kind"),
                    {"id": "skills", "name": "Skills", "color": "inactive", "kind": "used"},
                    {"id": "agents", "tokens": 40, "color": "inactive", "kind": "used"},
                    category("Messages", 18_105, "used"),
                    category("Autocompact buffer", 33_000, "buffer"),
                    category("Free space", 939_788, "free"),
                ],
                "totalTokens": 27_212,
                "maxTokens": 1_000_000,
                "percentage": 3,
                "memoryFiles": [{"path": "/home/alex/notes.md", "tokens": 68}],
            }),
        );
        assert_eq!(
            t.push(parse_line(&reply)),
            vec![SessionEvent::ContextBreakdown(ContextBreakdown {
                parts: vec![
                    part("System prompt", 3_237, ContextKind::Used),
                    part("System tools", 5_870, ContextKind::Used),
                    part("Messages", 18_105, ContextKind::Used),
                    part("Autocompact buffer", 33_000, ContextKind::Buffer),
                    part("Free space", 939_788, ContextKind::Free),
                ],
                window: 1_000_000,
            })]
        );
        assert!(t.take_requests().is_empty(), "nothing more is asked");
        assert!(
            !format!("{t:?}").contains("notes.md"),
            "nothing else of the reply is kept"
        );
    }

    #[test]
    fn the_recorded_breakdown_gives_its_three_rows() {
        // Line 14 of the V9 recording answers the request `p2`.
        let recorded = include_str!("../tests/fixtures/live/v9-control.jsonl")
            .lines()
            .nth(13)
            .unwrap();
        let mut t = asked_for_breakdown("p2");
        assert_eq!(
            t.push(parse_line(recorded)),
            vec![SessionEvent::ContextBreakdown(ContextBreakdown {
                parts: vec![
                    part("System prompt", 3_237, ContextKind::Used),
                    part("System tools", 5_870, ContextKind::Used),
                    part("MCP server instructions", 717, ContextKind::Used),
                ],
                window: 1_000_000,
            })]
        );
    }

    #[test]
    fn a_refused_breakdown_says_nothing() {
        let mut t = asked_for_breakdown("ctx_1");
        assert!(
            t.push(parse_line(&refused("ctx_1", "not ready")))
                .is_empty()
        );
        assert!(t.take_requests().is_empty());
    }

    #[test]
    fn a_reply_without_categories_says_nothing() {
        let rows = json!([category("Messages", 8_000, "used")]);
        for response in [
            json!({"maxTokens": 200_000}),
            json!({"categories": rows}),
            json!({"categories": rows, "maxTokens": 0}),
            json!({"categories": rows, "maxTokens": "lots"}),
            json!({"categories": "none", "maxTokens": 200_000}),
            json!({"categories": [], "maxTokens": 200_000}),
            json!({"categories": [category("MCP tools", 1_200, "deferred")], "maxTokens": 200_000}),
            Value::Null,
        ] {
            let mut t = asked_for_breakdown("ctx_1");
            assert!(
                t.push(parse_line(&context_reply("ctx_1", response.clone())))
                    .is_empty(),
                "{response}"
            );
        }
    }

    // The `background_tasks_changed` records below are written by hand from what the program
    // says it sends: never seen live with a task in them. Their values are made up.

    /// A record of the program's background tasks, with `tasks` as given.
    fn background(tasks: &str) -> String {
        format!(
            r#"{{"type":"system","subtype":"background_tasks_changed","tasks":{tasks},"uuid":"u","session_id":"s"}}"#
        )
    }

    fn running(n: usize) -> Vec<SessionEvent> {
        vec![SessionEvent::BackgroundTasks { running: n }]
    }

    #[test]
    fn background_tasks_are_counted() {
        // A task that runs off this computer does not need the program to stay up.
        let tasks = r#"[
            {"task_id":"t1","task_type":"local_bash","description":"npm run dev"},
            {"task_id":"t2","task_type":"local_agent","description":"review","off_host":true}
        ]"#
        .replace('\n', "");
        assert_eq!(feed(&[&background(&tasks)]), running(1));
        let on_host = r#"[{"task_id":"t1","off_host":false},{"task_id":"t2"}]"#;
        assert_eq!(feed(&[&background(on_host)]), running(2));
    }

    #[test]
    fn an_empty_list_is_no_task() {
        assert_eq!(feed(&[&background("[]")]), running(0));
        let off_host = r#"[{"task_id":"t1","off_host":true}]"#;
        assert_eq!(feed(&[&background(off_host)]), running(0));
    }

    #[test]
    fn a_list_that_cannot_be_read_counts_as_one() {
        assert_eq!(feed(&[&background(r#""x""#)]), running(1));
        assert_eq!(feed(&[&background("null")]), running(1));
        let missing =
            r#"{"type":"system","subtype":"background_tasks_changed","uuid":"u","session_id":"s"}"#;
        assert_eq!(feed(&[missing]), running(1));
        // An entry that is not an object counts as a task of its own.
        let odd = r#"["t1",{"task_id":"t2","off_host":true},7]"#;
        assert_eq!(feed(&[&background(odd)]), running(2));
    }

    #[test]
    fn each_record_replaces_the_count() {
        let two = r#"[{"task_id":"t1"},{"task_id":"t2"}]"#;
        let one = r#"[{"task_id":"t2"}]"#;
        assert_eq!(
            feed(&[&background(two), &background(one), &background("[]")]),
            [running(2), running(1), running(0)].concat()
        );
    }

    // The tool calls below are written by hand: `run_in_background` is the field the program's
    // shell and agent tools take for a task that outlives the call. Never seen live.

    /// A top-level record with one call of the shell tool, its input as given.
    fn shell_call(id: &str, input: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"id":"m2","content":[{{"type":"tool_use","id":"{id}","name":"Bash","input":{input}}}]}}}}"#
        )
    }

    /// The background task counts among `events`, in order.
    fn counts(events: &[SessionEvent]) -> Vec<usize> {
        events
            .iter()
            .filter_map(|event| match event {
                SessionEvent::BackgroundTasks { running } => Some(*running),
                _ => None,
            })
            .collect()
    }

    const IN_BACKGROUND: &str = r#"{"command":"npm run dev","run_in_background":true}"#;

    #[test]
    fn a_tool_call_run_in_the_background_counts_until_a_record_says_otherwise() {
        let events = feed(&[&shell_call("t1", IN_BACKGROUND)]);
        assert!(
            matches!(&events[0], SessionEvent::ToolStarted { name, .. } if name == "Bash"),
            "{events:?}"
        );
        assert_eq!(events[1..], running(1), "after the call it belongs to");
        // No record ever comes: nothing later takes the count back, not a call that is not in
        // the background and not the end of the turn.
        let later = feed(&[
            &shell_call("t1", IN_BACKGROUND),
            &shell_call("t2", r#"{"command":"ls"}"#),
            RESULT_OK,
            MSG_START,
            ASSISTANT_OK,
            RESULT_OK,
        ]);
        assert_eq!(counts(&later), vec![1]);
        // A second such call still counts as one: only the records know how many run.
        let twice = feed(&[
            &shell_call("t1", IN_BACKGROUND),
            &shell_call("t2", IN_BACKGROUND),
        ]);
        assert_eq!(counts(&twice), vec![1, 1]);
        // A call a sub-agent makes is not shown, and counts all the same.
        let sub = r#"{"type":"assistant","parent_tool_use_id":"t9","message":{"id":"m3","content":[{"type":"tool_use","id":"t3","name":"Bash","input":{"command":"make","run_in_background":true}}]}}"#;
        assert_eq!(feed(&[sub]), running(1));
        // Each program has a translator of its own: the next one starts with nothing counted.
        let fresh = feed(&[&shell_call("t1", r#"{"command":"ls"}"#)]);
        assert!(counts(&fresh).is_empty());
    }

    #[test]
    fn a_record_after_it_takes_over() {
        let one = r#"[{"task_id":"t1"}]"#;
        let events = feed(&[
            &shell_call("t1", IN_BACKGROUND),
            &background(one),
            &background("[]"),
            &shell_call("t2", r#"{"command":"ls"}"#),
        ]);
        assert_eq!(counts(&events), vec![1, 1, 0], "the records decide");
        // A call after a record counts again, and never takes away what the record counted.
        let two = r#"[{"task_id":"t1"},{"task_id":"t2"}]"#;
        let events = feed(&[
            &background("[]"),
            &shell_call("t1", IN_BACKGROUND),
            &background(two),
            &shell_call("t2", IN_BACKGROUND),
        ]);
        assert_eq!(counts(&events), vec![0, 1, 2, 2]);
    }

    #[test]
    fn a_tool_call_not_in_the_background_counts_nothing() {
        for input in [
            r#"{"command":"ls"}"#,
            r#"{"command":"ls","run_in_background":false}"#,
            r#"{"command":"ls","run_in_background":null}"#,
            r#"{"command":"ls","run_in_background":"true"}"#,
            r#""run_in_background""#,
        ] {
            let events = feed(&[&shell_call("t1", input)]);
            assert!(counts(&events).is_empty(), "{input}");
            assert_eq!(events.len(), 1, "{input}: the call itself");
        }
        // A tool that asks the user is no task, whatever its input says.
        let ask = r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"t_q","name":"AskUserQuestion","input":{"questions":[],"run_in_background":true}}]}}"#;
        assert_eq!(feed(&[ask]), vec![]);
    }
}
