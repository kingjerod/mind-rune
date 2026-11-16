//! Turns one line of `claude`'s stream-json output into a typed value.
//!
//! The parser walks a `serde_json::Value` instead of using strict
//! deserialisation, so unknown or missing fields never fail a line. A line that
//! is not understood becomes [`Incoming::Ignored`]; nothing here panics or
//! returns an error.

use std::collections::BTreeMap;

use serde_json::Value;

/// One parsed output line.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Init {
        session_id: String,
        model: String,
        permission_mode: String,
        version: String,
        cwd: String,
    },
    BridgeState {
        state: String,
        detail: Option<String>,
        failure_kind: Option<String>,
    },
    TitleChanged {
        title: String,
    },
    Stream {
        parent: Option<String>,
        event: StreamEvent,
    },
    Assistant {
        parent: Option<String>,
        message_id: String,
        blocks: Vec<Block>,
        /// The model that wrote the message.
        model: Option<String>,
        /// The input side of the message's usage: what it put in the context.
        usage_tokens: Option<u64>,
    },
    ToolResults {
        parent: Option<String>,
        results: Vec<ToolResult>,
    },
    Result {
        is_error: bool,
        interrupted: bool,
        text: String,
        cost_usd: f64,
        duration_ms: u64,
        /// The `contextWindow` of each model in `modelUsage` that names one.
        context_windows: BTreeMap<String, u64>,
    },
    /// Each window is `(utilization, resetsAt)`.
    RateLimit {
        five_hour: Option<(f32, i64)>,
        seven_day: Option<(f32, i64)>,
    },
    ControlRequest {
        request_id: String,
        subtype: String,
        body: Value,
    },
    ControlResponse {
        request_id: String,
        ok: bool,
        body: Value,
        error: Option<String>,
        error_code: Option<String>,
    },
    ControlCancel {
        request_id: String,
    },
    ModeChanged {
        permission_mode: String,
    },
    Ignored {
        kind: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    MessageStart,
    MessageStop,
    BlockStart(BlockKind),
    TextDelta(String),
    ThinkingDelta(String),
    BlockStop,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    Text,
    Thinking,
    ToolUse,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Text(String),
    Thinking(String),
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub is_error: bool,
    pub text: String,
}

/// Parses one output line. Never panics and never fails: a line that is not a
/// JSON object is `Ignored { kind: "not-json" }`, and a record that is not
/// recognised, or lacks what its variant needs, is `Ignored` with its type.
pub fn parse_line(line: &str) -> Incoming {
    let value: Value = match serde_json::from_str(line) {
        Ok(v @ Value::Object(_)) => v,
        _ => return ignored("not-json".to_string()),
    };
    parse_record(&value).unwrap_or_else(|| ignored(kind_of(&value)))
}

fn ignored(kind: String) -> Incoming {
    Incoming::Ignored { kind }
}

/// `type`, plus `/subtype` when there is one.
fn kind_of(v: &Value) -> String {
    let ty = str_at(v, "type").unwrap_or("unknown");
    match str_at(v, "subtype") {
        Some(sub) => format!("{ty}/{sub}"),
        None => ty.to_string(),
    }
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

fn string_at(v: &Value, key: &str) -> Option<String> {
    str_at(v, key).map(str::to_string)
}

fn parent_of(v: &Value) -> Option<String> {
    string_at(v, "parent_tool_use_id")
}

/// `None` means the type is unknown or the record lacks what its variant
/// needs; the caller turns that into `Ignored`.
fn parse_record(v: &Value) -> Option<Incoming> {
    match str_at(v, "type")? {
        "system" => parse_system(v),
        "stream_event" => {
            let event = v.get("event").filter(|e| e.is_object())?;
            Some(Incoming::Stream {
                parent: parent_of(v),
                event: parse_stream_event(event),
            })
        }
        "assistant" => parse_assistant(v),
        "user" => parse_user(v),
        "result" => parse_result(v),
        "rate_limit_event" => parse_rate_limit(v),
        "control_request" => {
            let body = v.get("request")?;
            Some(Incoming::ControlRequest {
                request_id: string_at(v, "request_id")?,
                subtype: string_at(body, "subtype")?,
                body: body.clone(),
            })
        }
        "control_response" => {
            let resp = v.get("response")?;
            Some(Incoming::ControlResponse {
                request_id: string_at(resp, "request_id")?,
                ok: str_at(resp, "subtype") == Some("success"),
                body: resp.get("response").cloned().unwrap_or(Value::Null),
                error: string_at(resp, "error"),
                error_code: string_at(resp, "error_code"),
            })
        }
        "control_cancel_request" => Some(Incoming::ControlCancel {
            request_id: string_at(v, "request_id")?,
        }),
        _ => None,
    }
}

fn parse_system(v: &Value) -> Option<Incoming> {
    match str_at(v, "subtype")? {
        "init" => Some(Incoming::Init {
            session_id: string_at(v, "session_id")?,
            model: string_at(v, "model")?,
            permission_mode: string_at(v, "permissionMode").unwrap_or_default(),
            version: string_at(v, "claude_code_version").unwrap_or_default(),
            cwd: string_at(v, "cwd").unwrap_or_default(),
        }),
        "bridge_state" => Some(Incoming::BridgeState {
            state: string_at(v, "state")?,
            detail: string_at(v, "detail"),
            failure_kind: string_at(v, "failure_kind"),
        }),
        "session_title_changed" => Some(Incoming::TitleChanged {
            title: string_at(v, "title")?,
        }),
        "status" => Some(Incoming::ModeChanged {
            permission_mode: string_at(v, "permissionMode")?,
        }),
        _ => None,
    }
}

fn parse_stream_event(event: &Value) -> StreamEvent {
    match str_at(event, "type") {
        Some("message_start") => StreamEvent::MessageStart,
        Some("message_stop") => StreamEvent::MessageStop,
        Some("content_block_stop") => StreamEvent::BlockStop,
        Some("content_block_start") => {
            let kind = match event.get("content_block").and_then(|b| str_at(b, "type")) {
                Some("text") => BlockKind::Text,
                Some("thinking") => BlockKind::Thinking,
                Some("tool_use") => BlockKind::ToolUse,
                _ => BlockKind::Other,
            };
            StreamEvent::BlockStart(kind)
        }
        Some("content_block_delta") => {
            let Some(delta) = event.get("delta") else {
                return StreamEvent::Other;
            };
            match str_at(delta, "type") {
                Some("text_delta") => match string_at(delta, "text") {
                    Some(text) => StreamEvent::TextDelta(text),
                    None => StreamEvent::Other,
                },
                // The text is always empty live, but the delta is still a thinking delta.
                Some("thinking_delta") => {
                    StreamEvent::ThinkingDelta(string_at(delta, "thinking").unwrap_or_default())
                }
                _ => StreamEvent::Other,
            }
        }
        _ => StreamEvent::Other,
    }
}

fn parse_assistant(v: &Value) -> Option<Incoming> {
    let message = v.get("message")?;
    let content = message.get("content")?.as_array()?;
    Some(Incoming::Assistant {
        parent: parent_of(v),
        message_id: string_at(message, "id").unwrap_or_default(),
        blocks: content.iter().map(parse_block).collect(),
        model: string_at(message, "model"),
        usage_tokens: message.get("usage").and_then(input_side),
    })
}

fn parse_block(b: &Value) -> Block {
    let parsed = match str_at(b, "type") {
        Some("text") => string_at(b, "text").map(Block::Text),
        Some("thinking") => string_at(b, "thinking").map(Block::Thinking),
        Some("tool_use") => (|| {
            Some(Block::ToolUse {
                id: string_at(b, "id")?,
                name: string_at(b, "name")?,
                input: b.get("input").cloned().unwrap_or(Value::Null),
            })
        })(),
        _ => None,
    };
    parsed.unwrap_or(Block::Other)
}

/// Input, cache read and cache creation tokens, as Claude Code's status line counts the
/// context. The output tokens are not counted.
fn input_side(usage: &Value) -> Option<u64> {
    usage.as_object()?;
    let n = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    // Saturate: a huge counter must not overflow (panic in debug, wrap in release).
    Some(
        [
            "input_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ]
        .into_iter()
        .map(n)
        .fold(0u64, u64::saturating_add),
    )
}

fn parse_user(v: &Value) -> Option<Incoming> {
    // String content, and content without tool results, is not a tool result record.
    let content = v.get("message")?.get("content")?.as_array()?;
    let results: Vec<ToolResult> = content.iter().filter_map(parse_tool_result).collect();
    if results.is_empty() {
        return None;
    }
    Some(Incoming::ToolResults {
        parent: parent_of(v),
        results,
    })
}

fn parse_tool_result(b: &Value) -> Option<ToolResult> {
    if str_at(b, "type")? != "tool_result" {
        return None;
    }
    let text = match b.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| str_at(p, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    Some(ToolResult {
        tool_use_id: string_at(b, "tool_use_id")?,
        is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
        text,
    })
}

/// Every `result` record ends the turn. Without a boolean `is_error`, a
/// `subtype` other than `success` marks an error.
fn parse_result(v: &Value) -> Option<Incoming> {
    let is_error = match v.get("is_error").and_then(Value::as_bool) {
        Some(is_error) => is_error,
        None => str_at(v, "subtype").is_some_and(|subtype| subtype != "success"),
    };
    let interrupted = str_at(v, "terminal_reason").is_some_and(|r| r.starts_with("aborted"));
    let text = match v.get("result") {
        Some(Value::String(s)) => s.clone(),
        _ => {
            let errors: Vec<&str> = v
                .get("errors")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if !errors.is_empty() {
                errors.join("; ")
            } else {
                string_at(v, "subtype").unwrap_or_default()
            }
        }
    };
    let context_windows = v
        .get("modelUsage")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(model, u)| Some((model.clone(), u.get("contextWindow")?.as_u64()?)))
        .collect();
    Some(Incoming::Result {
        is_error,
        interrupted,
        text,
        cost_usd: v
            .get("total_cost_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        duration_ms: v.get("duration_ms").and_then(Value::as_u64).unwrap_or(0),
        context_windows,
    })
}

fn parse_rate_limit(v: &Value) -> Option<Incoming> {
    let windows = v.get("rate_limit_info")?.get("unifiedWindows")?;
    let window = |name: &str| -> Option<(f32, i64)> {
        let w = windows.get(name)?;
        Some((
            w.get("utilization")?.as_f64()? as f32,
            w.get("resetsAt")?.as_i64()?,
        ))
    };
    Some(Incoming::RateLimit {
        five_hour: window("five_hour"),
        seven_day: window("seven_day"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_stopped_turn_is_marked_interrupted() {
        let l = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"stop_reason":null,"terminal_reason":"aborted_streaming","errors":["[ede_diagnostic] result_type=user"],"duration_ms":3555,"total_cost_usd":0.06}"#;
        assert_eq!(
            parse_line(l),
            Incoming::Result {
                is_error: true,
                interrupted: true,
                text: "[ede_diagnostic] result_type=user".into(),
                cost_usd: 0.06,
                duration_ms: 3555,
                context_windows: BTreeMap::new()
            }
        );
        let no_text = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"duration_ms":1,"total_cost_usd":0}"#;
        assert_eq!(
            parse_line(no_text),
            Incoming::Result {
                is_error: true,
                interrupted: false,
                text: "error_max_turns".into(),
                cost_usd: 0.0,
                duration_ms: 1,
                context_windows: BTreeMap::new()
            }
        );
    }

    #[test]
    fn a_result_without_a_boolean_is_error_still_ends_the_turn() {
        let result = |extra: &str| {
            parse_line(&format!(
                r#"{{"type":"result",{extra}"duration_ms":5,"total_cost_usd":0.5,"result":"r"}}"#
            ))
        };
        let expect = |is_error| Incoming::Result {
            is_error,
            interrupted: false,
            text: "r".into(),
            cost_usd: 0.5,
            duration_ms: 5,
            context_windows: BTreeMap::new(),
        };
        assert_eq!(result(r#""subtype":"success","#), expect(false));
        assert_eq!(result(r#""subtype":"error_max_turns","#), expect(true));
        assert_eq!(
            result(r#""subtype":"error_x","is_error":"yes","#),
            expect(true)
        );
        assert_eq!(result(""), expect(false));
        assert_eq!(
            result(r#""subtype":"error_x","is_error":false,"#),
            expect(false)
        );
    }

    #[test]
    fn an_assistant_record_without_a_message_id_is_kept() {
        let l = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#;
        assert_eq!(
            parse_line(l),
            Incoming::Assistant {
                parent: None,
                message_id: String::new(),
                blocks: vec![Block::Text("hi".into())],
                model: None,
                usage_tokens: None
            }
        );
    }

    #[test]
    fn message_stop_is_its_own_stream_event() {
        let l = r#"{"type":"stream_event","event":{"type":"message_stop"}}"#;
        assert_eq!(
            parse_line(l),
            Incoming::Stream {
                parent: None,
                event: StreamEvent::MessageStop
            }
        );
    }

    #[test]
    fn cancel_status_and_title_records() {
        assert_eq!(
            parse_line(r#"{"type":"control_cancel_request","request_id":"r9"}"#),
            Incoming::ControlCancel {
                request_id: "r9".into()
            }
        );
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"status","status":null,"permissionMode":"plan","session_id":"s1"}"#
            ),
            Incoming::ModeChanged {
                permission_mode: "plan".into()
            }
        );
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"status","status":"requesting","session_id":"s1"}"#
            ),
            Incoming::Ignored {
                kind: "system/status".into()
            }
        );
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"session_title_changed","title":"Probe rename B","session_id":"s1"}"#
            ),
            Incoming::TitleChanged {
                title: "Probe rename B".into()
            }
        );
    }

    #[test]
    fn user_records_without_tool_results_are_ignored() {
        let replay = r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>Set model</local-command-stdout>"},"isReplay":true}"#;
        assert_eq!(
            parse_line(replay),
            Incoming::Ignored {
                kind: "user".into()
            }
        );
        let note = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#;
        assert_eq!(
            parse_line(note),
            Incoming::Ignored {
                kind: "user".into()
            }
        );
    }

    #[test]
    fn the_live_recordings_parse_into_every_known_kind() {
        let mut seen = std::collections::BTreeSet::new();
        let mut lines = 0;
        for entry in
            std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/live")).unwrap()
        {
            for line in std::fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .lines()
            {
                lines += 1;
                let name = match parse_line(line) {
                    Incoming::Init { .. } => "Init",
                    Incoming::BridgeState { .. } => "BridgeState",
                    Incoming::TitleChanged { .. } => "TitleChanged",
                    Incoming::Stream { .. } => "Stream",
                    Incoming::Assistant { .. } => "Assistant",
                    Incoming::ToolResults { .. } => "ToolResults",
                    Incoming::Result { .. } => "Result",
                    Incoming::RateLimit { .. } => "RateLimit",
                    Incoming::ControlRequest { .. } => "ControlRequest",
                    Incoming::ControlResponse { .. } => "ControlResponse",
                    Incoming::ControlCancel { .. } => "ControlCancel",
                    Incoming::ModeChanged { .. } => "ModeChanged",
                    Incoming::Ignored { kind } => {
                        assert_ne!(kind, "not-json", "{line}");
                        "Ignored"
                    }
                };
                seen.insert(name);
            }
        }
        assert!(lines > 500, "the fixtures are present");
        let all = [
            "Assistant",
            "BridgeState",
            "ControlCancel",
            "ControlRequest",
            "ControlResponse",
            "Ignored",
            "Init",
            "ModeChanged",
            "RateLimit",
            "Result",
            "Stream",
            "TitleChanged",
            "ToolResults",
        ];
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), all);
    }

    #[test]
    fn init() {
        let l = r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-haiku-5-5","permissionMode":"default","claude_code_version":"2.1.319","cwd":"/work","tools":["Bash"]}"#;
        assert_eq!(
            parse_line(l),
            Incoming::Init {
                session_id: "s1".into(),
                model: "claude-haiku-5-5".into(),
                permission_mode: "default".into(),
                version: "2.1.319".into(),
                cwd: "/work".into()
            }
        );
    }

    #[test]
    fn streamed_text() {
        let l = r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}}"#;
        assert_eq!(
            parse_line(l),
            Incoming::Stream {
                parent: None,
                event: StreamEvent::TextDelta("ok".into())
            }
        );
        let start = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}"#;
        assert_eq!(
            parse_line(start),
            Incoming::Stream {
                parent: None,
                event: StreamEvent::BlockStart(BlockKind::Thinking)
            }
        );
    }

    #[test]
    fn an_assistant_record_with_a_tool_call() {
        let l = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"id":"m1","model":"claude-haiku-5-5","content":[{"type":"text","text":"Running it."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":30,"output_tokens":40}}}"#;
        assert_eq!(
            parse_line(l),
            Incoming::Assistant {
                parent: None,
                message_id: "m1".into(),
                blocks: vec![
                    Block::Text("Running it.".into()),
                    Block::ToolUse {
                        id: "t1".into(),
                        name: "Bash".into(),
                        input: json!({"command":"ls"})
                    }
                ],
                model: Some("claude-haiku-5-5".into()),
                // The output tokens are not context.
                usage_tokens: Some(60)
            }
        );
    }

    #[test]
    fn huge_token_counts_saturate_instead_of_overflowing() {
        let l = r#"{"type":"assistant","message":{"id":"m","content":[],"usage":{"input_tokens":18446744073709551615,"cache_read_input_tokens":18446744073709551615,"output_tokens":1}}}"#;
        assert_eq!(
            parse_line(l),
            Incoming::Assistant {
                parent: None,
                message_id: "m".into(),
                blocks: vec![],
                model: None,
                usage_tokens: Some(u64::MAX)
            }
        );
    }

    #[test]
    fn tool_results_with_string_and_block_content() {
        let l = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"file.txt"},{"type":"tool_result","tool_use_id":"t2","is_error":true,"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}]}}"#;
        assert_eq!(
            parse_line(l),
            Incoming::ToolResults {
                parent: None,
                results: vec![
                    ToolResult {
                        tool_use_id: "t1".into(),
                        is_error: false,
                        text: "file.txt".into()
                    },
                    ToolResult {
                        tool_use_id: "t2".into(),
                        is_error: true,
                        text: "a\nb".into()
                    }
                ]
            }
        );
    }

    #[test]
    fn result_and_rate_limit() {
        let r = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":915,"total_cost_usd":0.0020488399999999997,"result":"ok","modelUsage":{"claude-haiku-5-5":{"contextWindow":200000},"claude-sonnet-5-5":{"contextWindow":1000000},"claude-x":{}}}"#;
        assert_eq!(
            parse_line(r),
            Incoming::Result {
                is_error: false,
                interrupted: false,
                text: "ok".into(),
                cost_usd: 0.0020488399999999997,
                duration_ms: 915,
                context_windows: BTreeMap::from([
                    ("claude-haiku-5-5".into(), 200000),
                    ("claude-sonnet-5-5".into(), 1000000)
                ])
            }
        );
        let l = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1794385200,"rateLimitType":"five_hour","unifiedWindows":{"five_hour":{"utilization":0.08,"resetsAt":1794385200,"status":"allowed"},"seven_day":{"utilization":0.06,"resetsAt":1794776400,"status":"allowed"}}}}"#;
        assert_eq!(
            parse_line(l),
            Incoming::RateLimit {
                five_hour: Some((0.08, 1794385200)),
                seven_day: Some((0.06, 1794776400))
            }
        );
    }

    #[test]
    fn bridge_state_and_control_messages() {
        let b = r#"{"type":"system","subtype":"bridge_state","state":"failed","detail":"Claude.ai login was rejected","failure_kind":"auth"}"#;
        assert_eq!(
            parse_line(b),
            Incoming::BridgeState {
                state: "failed".into(),
                detail: Some("Claude.ai login was rejected".into()),
                failure_kind: Some("auth".into())
            }
        );
        let q = r#"{"type":"control_request","request_id":"req_1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
        assert_eq!(
            parse_line(q),
            Incoming::ControlRequest {
                request_id: "req_1".into(),
                subtype: "can_use_tool".into(),
                body: json!({"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}})
            }
        );
        let e = r#"{"type":"control_response","response":{"subtype":"error","request_id":"rc_on","error":"nope","error_code":"remote_control_auth"}}"#;
        assert_eq!(
            parse_line(e),
            Incoming::ControlResponse {
                request_id: "rc_on".into(),
                ok: false,
                body: json!(null),
                error: Some("nope".into()),
                error_code: Some("remote_control_auth".into())
            }
        );
        let s =
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"rc_off"}}"#;
        assert_eq!(
            parse_line(s),
            Incoming::ControlResponse {
                request_id: "rc_off".into(),
                ok: true,
                body: json!(null),
                error: None,
                error_code: None
            }
        );
    }

    #[test]
    fn anything_unrecognised_is_ignored_not_fatal() {
        assert_eq!(
            parse_line(""),
            Incoming::Ignored {
                kind: "not-json".into()
            }
        );
        assert_eq!(
            parse_line("not json at all"),
            Incoming::Ignored {
                kind: "not-json".into()
            }
        );
        assert_eq!(
            parse_line(r#"{"type":"assistant""#),
            Incoming::Ignored {
                kind: "not-json".into()
            }
        );
        assert_eq!(
            parse_line(r#"{"type":"brand_new_thing"}"#),
            Incoming::Ignored {
                kind: "brand_new_thing".into()
            }
        );
        assert_eq!(
            parse_line(r#"{"type":"system","subtype":"hook_started"}"#),
            Incoming::Ignored {
                kind: "system/hook_started".into()
            }
        );
        assert_eq!(
            parse_line(r#"{"type":"assistant"}"#),
            Incoming::Ignored {
                kind: "assistant".into()
            }
        );
        assert_eq!(
            parse_line("[1,2,3]"),
            Incoming::Ignored {
                kind: "not-json".into()
            }
        );
    }
}
