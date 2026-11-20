//! The program's requests for the choice tool the probe can serve (`raw --choices`). The
//! server itself is the adapter's (`mr_claude::choices`); the probe only picks its requests out
//! of the lines it reads, and the recording shows both sides.

use serde_json::Value;

/// The id and the `request` object of `line`, when it is the program's request to pass a
/// message to an in-process server.
pub fn request(line: &str) -> Option<(String, Value)> {
    let value: Value = serde_json::from_str(line).ok()?;
    let request = &value["request"];
    if value["type"] != "control_request" || request["subtype"] != "mcp_message" {
        return None;
    }
    Some((value["request_id"].as_str()?.to_string(), request.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_a_message_for_a_server_is_a_request() {
        let line = r#"{"type":"control_request","request_id":"req_1","request":{"subtype":"mcp_message","server_name":"mindrune","message":{"jsonrpc":"2.0","id":3,"method":"tools/list"}}}"#;
        assert_eq!(
            request(line),
            Some((
                "req_1".to_string(),
                json!({"subtype":"mcp_message","server_name":"mindrune","message":{"jsonrpc":"2.0","id":3,"method":"tools/list"}})
            ))
        );
        let permission = r#"{"type":"control_request","request_id":"req_2","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{}}}"#;
        assert_eq!(request(permission), None);
        assert_eq!(request("not json"), None);
        let no_id = r#"{"type":"control_request","request":{"subtype":"mcp_message","server_name":"mindrune","message":{}}}"#;
        assert_eq!(request(no_id), None);
    }
}
