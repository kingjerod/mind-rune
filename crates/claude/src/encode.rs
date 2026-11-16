//! Everything the adapter writes to the child's stdin. Each encoder returns
//! exactly one line of JSON with no trailing newline; the process layer adds it.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mr_agent::{Attachment, PermissionMode};
use serde_json::{Value, json};

use crate::mode::mode_to_wire;

/// A user turn. The content is one text block (omitted when there is no text),
/// then one base64 image block per image. Files are not content blocks: they
/// are listed by absolute path at the end of the text and Claude reads them.
pub fn user_message(text: &str, attachments: &[Attachment]) -> String {
    let files: Vec<String> = attachments
        .iter()
        .filter_map(|a| match a {
            Attachment::File { path } => Some(format!("- {}", path.display())),
            Attachment::Image { .. } => None,
        })
        .collect();

    let mut full_text = text.to_string();
    if !files.is_empty() {
        if !full_text.is_empty() {
            full_text.push_str("\n\n");
        }
        full_text.push_str("Attached files:\n");
        full_text.push_str(&files.join("\n"));
    }

    let mut content = Vec::new();
    if !full_text.is_empty() {
        content.push(json!({"type": "text", "text": full_text}));
    }
    for attachment in attachments {
        if let Attachment::Image { media_type, data } = attachment {
            content.push(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": STANDARD.encode(data),
                },
            }));
        }
    }

    line(json!({"type": "user", "message": {"role": "user", "content": content}}))
}

/// Wraps a request body (see `initialize`, `interrupt`, ...) in its envelope.
pub fn control_request(request_id: &str, body: Value) -> String {
    line(json!({"type": "control_request", "request_id": request_id, "request": body}))
}

pub fn initialize() -> Value {
    json!({"subtype": "initialize", "hooks": null})
}

pub fn interrupt() -> Value {
    json!({"subtype": "interrupt"})
}

pub fn set_model(model: &str) -> Value {
    json!({"subtype": "set_model", "model": model})
}

pub fn set_permission_mode(mode: &PermissionMode) -> Value {
    json!({"subtype": "set_permission_mode", "mode": mode_to_wire(mode)})
}

pub fn remote_control(on: bool, name: Option<&str>) -> Value {
    let mut body = json!({"subtype": "remote_control", "enabled": on});
    if let Some(name) = name {
        body["name"] = json!(name);
    }
    body
}

pub fn get_usage() -> Value {
    json!({"subtype": "get_usage"})
}

/// Sets the effort level of the running session's next messages. The reply is a bare
/// success also for a level that was not taken, so `get_settings` reads what is in use.
/// Read from the program (2.1.323), never seen live.
pub fn set_effort(level: &str) -> Value {
    json!({"subtype": "apply_flag_settings", "settings": {"effortLevel": level}})
}

/// Reads the settings in use; its reply's `applied.effort` is the level the next message
/// will use. The reply also holds the user's own settings. Read from the program
/// (2.1.323), never seen live.
pub fn get_settings() -> Value {
    json!({"subtype": "get_settings"})
}

/// Asks what the context is made of; its reply's `categories` and `maxTokens` are read. The
/// reply also names memory files, tools and skills, which are not. Seen working live (V9).
pub fn get_context_usage() -> Value {
    json!({"subtype": "get_context_usage"})
}

pub fn rename_session(title: &str) -> Value {
    json!({"subtype": "rename_session", "title": title})
}

/// Asks `claude` to name the session from `description`, and to save the name (`persist`).
/// Read from the program (2.1.323), not yet seen on the wire: the live test prints what comes
/// back. Its reply's body is `{"title": STRING or null}`; a `session_title_changed` line may
/// come as well, before or after it.
pub fn generate_session_title(description: &str) -> Value {
    json!({"subtype": "generate_session_title", "description": description, "persist": true})
}

/// The answer to a `can_use_tool` permission request.
#[derive(Clone, Debug, PartialEq)]
pub enum PermissionReply {
    Allow {
        updated_input: Value,
        updated_permissions: Option<Value>,
    },
    Deny {
        message: String,
    },
}

pub fn permission_reply(request_id: &str, reply: &PermissionReply) -> String {
    let response = match reply {
        PermissionReply::Allow {
            updated_input,
            updated_permissions,
        } => {
            let mut response = json!({"behavior": "allow", "updatedInput": updated_input});
            if let Some(permissions) = updated_permissions {
                response["updatedPermissions"] = permissions.clone();
            }
            response
        }
        PermissionReply::Deny { message } => json!({"behavior": "deny", "message": message}),
    };
    line(json!({
        "type": "control_response",
        "response": {"subtype": "success", "request_id": request_id, "response": response},
    }))
}

/// The answer to a control request this adapter does not handle.
pub fn unsupported_reply(request_id: &str) -> String {
    line(json!({
        "type": "control_response",
        "response": {
            "subtype": "error",
            "request_id": request_id,
            "error": "unsupported by mindrune",
        },
    }))
}

/// `to_string` escapes newlines inside strings, so the result is a single line.
fn line(value: Value) -> String {
    serde_json::to_string(&value).expect("a JSON value always serialises")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_agent::{Attachment, PermissionMode};
    use serde_json::{Value, json};

    fn parsed(line: &str) -> Value {
        assert!(!line.contains('\n'), "a line has no embedded newline");
        serde_json::from_str::<Value>(line).unwrap()
    }

    #[test]
    fn a_text_message() {
        assert_eq!(
            parsed(&user_message("hi", &[])),
            json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":"hi"}]}})
        );
    }

    #[test]
    fn an_image_becomes_a_base64_block_after_the_text() {
        let png = Attachment::Image {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
        };
        assert_eq!(
            parsed(&user_message("look", &[png])),
            json!({"type":"user","message":{"role":"user","content":[
                {"type":"text","text":"look"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"AQID"}}]}})
        );
    }

    #[test]
    fn an_image_alone_sends_no_empty_text_block() {
        let png = Attachment::Image {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
        };
        let v = parsed(&user_message("", &[png]));
        assert_eq!(v["message"]["content"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn files_are_listed_by_path_in_the_text() {
        let f = Attachment::File {
            path: "/work/notes.txt".into(),
        };
        let v = parsed(&user_message("read this", &[f]));
        assert_eq!(
            v["message"]["content"][0]["text"],
            "read this\n\nAttached files:\n- /work/notes.txt"
        );
    }

    #[test]
    fn control_requests_are_framed() {
        assert_eq!(
            parsed(&control_request("rc_1", interrupt())),
            json!({"type":"control_request","request_id":"rc_1","request":{"subtype":"interrupt"}})
        );
        assert_eq!(initialize(), json!({"subtype":"initialize","hooks":null}));
        assert_eq!(
            set_model("opus"),
            json!({"subtype":"set_model","model":"opus"})
        );
        assert_eq!(
            set_permission_mode(&PermissionMode::Plan),
            json!({"subtype":"set_permission_mode","mode":"plan"})
        );
        assert_eq!(
            remote_control(true, Some("My session")),
            json!({"subtype":"remote_control","enabled":true,"name":"My session"})
        );
        assert_eq!(
            remote_control(false, None),
            json!({"subtype":"remote_control","enabled":false})
        );
        assert_eq!(
            rename_session("New title"),
            json!({"subtype":"rename_session","title":"New title"})
        );
        assert_eq!(get_usage(), json!({"subtype":"get_usage"}));
        assert_eq!(
            set_effort("high"),
            json!({"subtype":"apply_flag_settings","settings":{"effortLevel":"high"}})
        );
        assert_eq!(get_settings(), json!({"subtype":"get_settings"}));
        assert_eq!(get_context_usage(), json!({"subtype":"get_context_usage"}));
        assert_eq!(
            generate_session_title("Fix the parser"),
            json!({"subtype":"generate_session_title","description":"Fix the parser","persist":true})
        );
    }

    #[test]
    fn permission_replies_are_framed_as_control_responses() {
        let allow = PermissionReply::Allow {
            updated_input: json!({"command":"ls"}),
            updated_permissions: None,
        };
        assert_eq!(
            parsed(&permission_reply("req_9", &allow)),
            json!({"type":"control_response","response":{"subtype":"success","request_id":"req_9",
               "response":{"behavior":"allow","updatedInput":{"command":"ls"}}}})
        );
        let always = PermissionReply::Allow {
            updated_input: json!({}),
            updated_permissions: Some(json!([{"type":"addRules"}])),
        };
        assert_eq!(
            parsed(&permission_reply("req_9", &always))["response"]["response"]["updatedPermissions"],
            json!([{"type":"addRules"}])
        );
        let deny = PermissionReply::Deny {
            message: "no".into(),
        };
        assert_eq!(
            parsed(&permission_reply("req_9", &deny))["response"]["response"],
            json!({"behavior":"deny","message":"no"})
        );
        assert_eq!(
            parsed(&unsupported_reply("req_3")),
            json!({"type":"control_response","response":{"subtype":"error","request_id":"req_3",
               "error":"unsupported by mindrune"}})
        );
    }
}
