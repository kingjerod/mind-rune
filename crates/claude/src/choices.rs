//! The choice tool the adapter serves to the program: one tool on an in-process MCP server,
//! with which Claude offers the user up to three buttons for what to do next. The program
//! reaches such a server through control requests on the session's own channel, and the
//! reader answers them at once.
//!
//! The answer only says whether the offer can be shown: the offer itself is read from the
//! call's `tool_use` block, which carries the same input, and shown once the call's result
//! says it worked.

use mr_agent::{Choice, Choices, MAX_CHOICES, MAX_LABEL, MAX_MESSAGE, MAX_QUESTION};
use serde_json::{Value, json};

/// The server's name, as the program is told it and as it names it in its requests.
pub const SERVER: &str = "mindrune";

/// The tool's name on the server.
pub const TOOL: &str = "offer_choices";

/// What the program is told about when to use the tool.
const INSTRUCTION: &str = "This session runs in a desktop app that can show the user buttons. \
When your work in a turn is done and the next move is the user's decision (a choice between \
ways forward, a go-ahead, or an obvious next command), call the tool \
mcp__mindrune__offer_choices once, with one short question and one to three choices, your \
recommendation first. A choice's message is sent as the user's next message when they click \
it, and it may be a slash command of this project. Do not call it when nothing is pending. Your \
reply must still make sense to someone who cannot see the buttons: end it by naming the decision \
in a few words.";

/// The tool's name as the program calls it and as permission rules name it.
pub fn tool_name() -> String {
    format!("mcp__{SERVER}__{TOOL}")
}

/// The arguments that give the program the server, its tool loaded at the start and allowed
/// without a question, and add the instruction.
pub fn args() -> Vec<String> {
    let config =
        json!({"mcpServers": {SERVER: {"type": "sdk", "name": SERVER, "alwaysLoad": true}}});
    vec![
        "--mcp-config".into(),
        config.to_string(),
        "--allowedTools".into(),
        tool_name(),
        "--append-system-prompt".into(),
        INSTRUCTION.into(),
    ]
}

/// The line that answers the program's `mcp_message` request `request_id`, whose `request`
/// object is `body`: the JSON-RPC answer of the server named, inside a successful control
/// response. A body that names no server or holds no message is answered as for an unknown
/// server.
pub fn reply(request_id: &str, body: &Value) -> String {
    let message = &body["message"];
    let answer = match body["server_name"].as_str() {
        Some(SERVER) if !message.is_null() => answer(message),
        Some(server) if server != SERVER => {
            failed(message, &format!("Server '{server}' not found"))
        }
        _ => failed(message, "Server not found"),
    };
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": {"mcp_response": answer},
        },
    })
    .to_string()
}

/// The offer in a call's `arguments`, or the sentence that says why it cannot be shown. A
/// text that is missing or no string counts as empty, and so do the choices that are no list.
pub fn offered(arguments: &Value) -> Result<Choices, String> {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    let choices = arguments["choices"]
        .as_array()
        .map(|choices| {
            choices
                .iter()
                .map(|choice| Choice {
                    label: text(&choice["label"]),
                    message: text(&choice["message"]),
                })
                .collect()
        })
        .unwrap_or_default();
    Choices::new(&text(&arguments["question"]), choices)
}

/// The server's JSON-RPC answer to `message`. A `ping` and a notification get an empty result,
/// as the program expects some answer to every control request.
fn answer(message: &Value) -> Value {
    let result = match message["method"].as_str() {
        Some("initialize") => json!({
            // The version the program asked for: the server has nothing a version changes.
            "protocolVersion": message["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": SERVER, "version": env!("CARGO_PKG_VERSION")},
        }),
        Some("tools/list") => json!({"tools": [tool()]}),
        Some("tools/call") => called(&message["params"]),
        Some("ping") => json!({}),
        Some(method) if method.starts_with("notifications/") => {
            return json!({"jsonrpc": "2.0", "result": {}});
        }
        _ => return failed(message, "Method not found"),
    };
    json!({"jsonrpc": "2.0", "id": message["id"], "result": result})
}

/// The MCP version answered when the program names none.
const PROTOCOL: &str = "2025-06-18";

/// A JSON-RPC "method not found" for `message`, with `why`.
fn failed(message: &Value, why: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32601, "message": why}})
}

/// The result of calling the tool named in `params`: shown when its offer can be, else an
/// error that says why, which Claude reads.
fn called(params: &Value) -> Value {
    if params["name"] != TOOL {
        return json!({"content": [{"type": "text", "text": "Unknown tool."}], "isError": true});
    }
    match offered(&params["arguments"]) {
        Ok(_) => json!({"content": [{"type": "text", "text": "Shown to the user."}]}),
        Err(why) => json!({
            "content": [{"type": "text", "text": format!("{why} Nothing was shown.")}],
            "isError": true,
        }),
    }
}

/// The tool as the server lists it: it changes nothing, and is loaded with the session's
/// first prompt rather than looked up.
fn tool() -> Value {
    json!({
        "name": TOOL,
        "description": format!(
            "Shows the user one short question with up to three buttons for what to do next. \
             Call it once, when your work in a turn is done and the next move is the user's \
             decision. A click sends that choice's message as the user's next message. The \
             question is at most {MAX_QUESTION} characters, a label at most {MAX_LABEL} and a \
             message at most {MAX_MESSAGE}."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": format!(
                        "The one decision, in one short sentence of at most {MAX_QUESTION} \
                         characters."
                    ),
                },
                "choices": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_CHOICES,
                    "description": "The buttons, your recommendation first.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": {
                                "type": "string",
                                "description": format!(
                                    "The button's text, one to three words, at most \
                                     {MAX_LABEL} characters."
                                ),
                            },
                            "message": {
                                "type": "string",
                                "description": format!(
                                    "What a click sends as the user's message, at most \
                                     {MAX_MESSAGE} characters on one line. It may be a slash \
                                     command of this project."
                                ),
                            },
                        },
                        "required": ["label", "message"],
                    },
                },
            },
            "required": ["question", "choices"],
        },
        "annotations": {"readOnlyHint": true},
        "_meta": {"anthropic/alwaysLoad": true},
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answered(server: &str, message: Value) -> Value {
        let body = json!({"subtype": "mcp_message", "server_name": server, "message": message});
        let line = reply("req_7", &body);
        assert!(!line.contains('\n'), "one line");
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["type"], "control_response");
        assert_eq!(value["response"]["subtype"], "success");
        assert_eq!(value["response"]["request_id"], "req_7");
        value["response"]["response"]["mcp_response"].clone()
    }

    fn call(arguments: Value) -> Value {
        let body = json!({"subtype":"mcp_message","server_name":"mindrune","message":
            {"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"offer_choices","arguments":arguments}}});
        let line: Value = serde_json::from_str(&reply("req_1", &body)).unwrap();
        line["response"]["response"]["mcp_response"]["result"].clone()
    }

    #[test]
    fn the_arguments_name_the_server_allow_the_tool_and_add_the_instruction() {
        assert_eq!(tool_name(), "mcp__mindrune__offer_choices");
        let args = args();
        assert_eq!(args.len(), 6);
        assert_eq!(args[0], "--mcp-config");
        let config: Value = serde_json::from_str(&args[1]).unwrap();
        assert_eq!(
            config,
            json!({"mcpServers": {"mindrune": {"type": "sdk", "name": "mindrune", "alwaysLoad": true}}})
        );
        assert_eq!(args[2], "--allowedTools");
        assert_eq!(args[3], "mcp__mindrune__offer_choices");
        assert_eq!(args[4], "--append-system-prompt");
        assert!(args[5].contains("mcp__mindrune__offer_choices"));
    }

    #[test]
    fn the_handshake_is_answered_with_the_version_asked_for() {
        let answer = answered(
            SERVER,
            json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}),
        );
        assert_eq!(answer["jsonrpc"], "2.0");
        assert_eq!(answer["id"], 0);
        assert_eq!(answer["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(answer["result"]["capabilities"], json!({"tools": {}}));
        assert_eq!(answer["result"]["serverInfo"]["name"], "mindrune");
        // A notification has no id and wants no result of its own.
        let noted = answered(
            SERVER,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        );
        assert_eq!(noted, json!({"jsonrpc":"2.0","result":{}}));
    }

    #[test]
    fn the_list_holds_the_one_tool_with_its_schema() {
        let answer = answered(
            SERVER,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        );
        let tools = answer["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "offer_choices");
        assert!(tools[0]["description"].as_str().unwrap().len() > 40);
        let schema = &tools[0]["inputSchema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["question", "choices"]));
        let choices = &schema["properties"]["choices"];
        assert_eq!(choices["minItems"], 1);
        assert_eq!(choices["maxItems"], 3);
        assert_eq!(choices["items"]["required"], json!(["label", "message"]));
    }

    #[test]
    fn a_call_of_the_tool_is_answered_as_shown() {
        let answer = answered(
            SERVER,
            json!({"jsonrpc":"2.0","id":"c1","method":"tools/call","params":{"name":"offer_choices","arguments":{"question":"Apply now?","choices":[{"label":"Apply","message":"/apply"}]}}}),
        );
        assert_eq!(answer["id"], "c1");
        assert_eq!(answer["result"]["content"][0]["type"], "text");
        assert!(answer["result"]["isError"].is_null() || answer["result"]["isError"] == false);
        // Another tool's name is an error the program can show Claude.
        let other = answered(
            SERVER,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"other","arguments":{}}}),
        );
        assert_eq!(other["result"]["isError"], true);
    }

    #[test]
    fn what_it_does_not_know_is_an_error_with_the_requests_id() {
        let method = answered(
            SERVER,
            json!({"jsonrpc":"2.0","id":9,"method":"resources/list"}),
        );
        assert_eq!(method["id"], 9);
        assert_eq!(method["error"]["code"], -32601);
        let server = answered(
            "another",
            json!({"jsonrpc":"2.0","id":4,"method":"tools/list"}),
        );
        assert_eq!(server["id"], 4);
        assert_eq!(server["error"]["code"], -32601);
        assert!(server["result"].is_null());
    }

    #[test]
    fn an_offer_that_can_be_shown_is_answered_as_shown() {
        let result =
            call(json!({"question":"Apply now?","choices":[{"label":"Apply","message":"/apply"}]}));
        assert_eq!(
            result,
            json!({"content":[{"type":"text","text":"Shown to the user."}]})
        );
    }

    #[test]
    fn an_offer_that_cannot_be_shown_is_refused_in_words() {
        let said = |arguments: Value| {
            let result = call(arguments);
            assert_eq!(result["isError"], true);
            result["content"][0]["text"].as_str().unwrap().to_string()
        };
        assert_eq!(said(json!({})), "A question is needed. Nothing was shown.");
        assert_eq!(
            said(json!({"question":"Q?","choices":"yes"})),
            "One to three choices are needed. Nothing was shown."
        );
        assert_eq!(
            said(
                json!({"question":"Q?","choices":[{"label":"a","message":"a"},{"label":"b","message":"b"},{"label":"c","message":"c"},{"label":"d","message":"d"}]})
            ),
            "One to three choices are needed. Nothing was shown."
        );
        assert_eq!(
            said(json!({"question":"Q?","choices":[{"label":7,"message":"x"}]})),
            "Every choice needs a label and a message. Nothing was shown."
        );
        assert_eq!(
            said(json!({"question":"Q?","choices":["yes"]})),
            "Every choice needs a label and a message. Nothing was shown."
        );
        assert_eq!(
            said(json!({"question":["Q?"],"choices":[{"label":"a","message":"a"}]})),
            "A question is needed. Nothing was shown."
        );
    }

    #[test]
    fn the_tool_is_listed_to_be_loaded_at_once() {
        let body = json!({"subtype":"mcp_message","server_name":"mindrune","message":{"jsonrpc":"2.0","id":1,"method":"tools/list"}});
        let line: Value = serde_json::from_str(&reply("req_1", &body)).unwrap();
        let tool = &line["response"]["response"]["mcp_response"]["result"]["tools"][0];
        assert_eq!(tool["name"], "offer_choices");
        assert_eq!(tool["_meta"], json!({"anthropic/alwaysLoad": true}));
        assert_eq!(tool["annotations"], json!({"readOnlyHint": true}));
        let config: Value = serde_json::from_str(&args()[1]).unwrap();
        assert_eq!(
            config,
            json!({"mcpServers":{"mindrune":{"type":"sdk","name":"mindrune","alwaysLoad":true}}})
        );
    }

    #[test]
    fn the_reply_is_told_to_stand_without_the_buttons() {
        assert!(INSTRUCTION.ends_with(
            "Do not call it when nothing is pending. Your reply must still make sense to someone \
             who cannot see the buttons: end it by naming the decision in a few words."
        ));
        assert!(!INSTRUCTION.contains("same question"));
    }

    #[test]
    fn the_tool_names_its_limits_and_no_projects_command() {
        let tool = tool();
        let text = tool.to_string();
        assert!(!text.contains("/apply"), "{text}");
        let description = tool["description"].as_str().unwrap();
        assert!(
            description.ends_with(
                "The question is at most 300 characters, a label at most 40 and a message at most \
                 200."
            ),
            "{description}"
        );
        let item = &tool["inputSchema"]["properties"]["choices"]["items"]["properties"];
        assert!(
            item["message"]["description"]
                .as_str()
                .unwrap()
                .contains("a slash command of this project")
        );
    }

    #[test]
    fn a_ping_is_answered_with_an_empty_result() {
        let answer = answered(SERVER, json!({"jsonrpc":"2.0","id":5,"method":"ping"}));
        assert_eq!(answer, json!({"jsonrpc":"2.0","id":5,"result":{}}));
    }

    #[test]
    fn a_body_without_a_server_or_a_message_is_still_answered() {
        let line: Value =
            serde_json::from_str(&reply("req_9", &json!({"subtype":"mcp_message"}))).unwrap();
        assert_eq!(line["response"]["request_id"], "req_9");
        assert_eq!(
            line["response"]["response"]["mcp_response"]["error"]["code"],
            -32601
        );
    }
}
