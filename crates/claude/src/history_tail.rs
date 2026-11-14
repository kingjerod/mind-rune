//! The end of a stored conversation, read from the end of its file along the chain of
//! replies, so that a very large session opens as fast as a small one.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::Path;

use mr_agent::{AgentError, HistoryItem, StoredHistory};
use serde::Deserialize;
use serde_json::Value;

use crate::history::{add_record, contains, parse_record};
use crate::mode::mode_from_wire;

/// How much of the end of a session file is read for its history.
pub const TAIL_BYTES: u64 = 16 * 1024 * 1024;

/// The model `claude` names on a message it made itself.
const SYNTHETIC_MODEL: &str = "<synthetic>";

/// The end of a stored conversation: the last `max_items` items of the chain that leads to
/// the file's last message, read from at most the last `tail_bytes` of the file.
///
/// The file's last `user` or `assistant` record that is not a side chain ends the chain,
/// which is followed back through `parentUuid` across records of every type, and across a
/// compaction through `logicalParentUuid`. A line that is not complete JSON is skipped.
pub fn read_tail(
    path: &Path,
    max_items: usize,
    tail_bytes: u64,
) -> Result<StoredHistory, AgentError> {
    let (bytes, from_start) = read_end(path, tail_bytes)?;
    let region = if from_start {
        &bytes[..]
    } else {
        // The region starts inside a line, or at one it cannot tell is whole.
        bytes
            .iter()
            .position(|&b| b == b'\n')
            .map_or(&[][..], |i| &bytes[i + 1..])
    };
    let records = Records::of(region);
    let permission_mode = records.permission_mode.as_deref().map(mode_from_wire);

    let Some(leaf) = records.leaf else {
        return Ok(StoredHistory {
            items: Vec::new(),
            // Messages may lie before the region; nothing in it can say.
            earlier_left_out: !from_start,
            model: None,
            permission_mode,
        });
    };
    let (steps, end) = records.chain_to(leaf);

    let mut items = Vec::new();
    let mut tool_positions = HashMap::new();
    let mut model = None;
    for step in steps.iter().rev() {
        match step {
            Step::Compaction => items.push(HistoryItem::Compacted),
            Step::Record(index) => {
                let Some(record) = parse_record(&region[records.nodes[*index].line.clone()]) else {
                    continue;
                };
                if let Some(name) = answering_model(&record) {
                    model = Some(name.to_string());
                }
                add_record(&record, &mut items, &mut tool_positions);
            }
        }
    }

    let cut = items.len().saturating_sub(max_items);
    items.drain(..cut);
    let earlier_left_out = cut > 0
        || match end {
            ChainEnd::Outside => true,
            // A first record is known only when the region is the whole file.
            ChainEnd::Root => !from_start,
        };
    Ok(StoredHistory {
        items,
        earlier_left_out,
        model,
        permission_mode,
    })
}

/// At most the last `tail_bytes` of the file, with one seek and one read, and whether
/// they start at its first byte.
fn read_end(path: &Path, tail_bytes: u64) -> Result<(Vec<u8>, bool), AgentError> {
    let io = |e: std::io::Error| AgentError::Io(e.to_string());
    let mut file = File::open(path).map_err(io)?;
    let len = file.metadata().map_err(io)?.len();
    let start = len.saturating_sub(tail_bytes);
    file.seek(SeekFrom::Start(start)).map_err(io)?;
    let mut bytes = Vec::with_capacity(usize::try_from(len - start).unwrap_or(0));
    file.take(len - start).read_to_end(&mut bytes).map_err(io)?;
    Ok((bytes, start == 0))
}

/// The model that wrote an `assistant` record, unless the program made it.
fn answering_model(record: &Value) -> Option<&str> {
    if record.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    record
        .get("message")
        .and_then(|message| message.get("model"))
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty() && *model != SYNTHETIC_MODEL)
}

/// The top-level fields of a record that place it on a chain. Each is read as any JSON
/// value, so that a field of an odd shape counts as absent rather than spoiling the line.
#[derive(Deserialize)]
struct Head {
    #[serde(rename = "type")]
    kind: Option<Value>,
    uuid: Option<Value>,
    #[serde(rename = "parentUuid")]
    parent_uuid: Option<Value>,
    #[serde(rename = "logicalParentUuid")]
    logical_parent_uuid: Option<Value>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<Value>,
    #[serde(rename = "permissionMode")]
    permission_mode: Option<Value>,
}

fn text(value: Option<Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// A record that has a `uuid`, known by its links and where its line lies in the region.
struct Node {
    parent: Option<String>,
    logical_parent: Option<String>,
    line: Range<usize>,
}

/// What the walk needs from the records of a region, read without parsing their content.
#[derive(Default)]
struct Records {
    nodes: Vec<Node>,
    /// Each uuid's node, at its first appearance.
    by_uuid: HashMap<String, usize>,
    /// The last `user` or `assistant` node that is not a side chain.
    leaf: Option<usize>,
    /// The wire name in the last `permission-mode` record.
    permission_mode: Option<String>,
}

impl Records {
    fn of(region: &[u8]) -> Self {
        let mut records = Records::default();
        let mut offset = 0;
        for line in region.split(|&b| b == b'\n') {
            let range = offset..offset + line.len();
            offset = range.end + 1;
            // Most lines name a uuid; the rest matter only when they set the mode.
            if !(contains(line, b"\"uuid\"") || contains(line, b"permission-mode")) {
                continue;
            }
            if let Some(head) = parse_head(line) {
                records.add(head, range);
            }
        }
        records
    }

    fn add(&mut self, head: Head, line: Range<usize>) {
        let kind = text(head.kind);
        if kind.as_deref() == Some("permission-mode")
            && let Some(mode) = text(head.permission_mode)
        {
            self.permission_mode = Some(mode);
        }
        let Some(uuid) = text(head.uuid) else {
            return;
        };
        let index = self.nodes.len();
        self.nodes.push(Node {
            parent: text(head.parent_uuid),
            logical_parent: text(head.logical_parent_uuid),
            line,
        });
        self.by_uuid.entry(uuid).or_insert(index);
        let is_message = matches!(kind.as_deref(), Some("user" | "assistant"));
        let side_chain = head.is_sidechain == Some(Value::Bool(true));
        if is_message && !side_chain {
            self.leaf = Some(index);
        }
    }

    /// The chain that ends at `leaf`, newest first, and why it stopped.
    fn chain_to(&self, leaf: usize) -> (Vec<Step>, ChainEnd) {
        let mut steps = Vec::new();
        let mut seen = vec![false; self.nodes.len()];
        let mut at = leaf;
        loop {
            // A record seen twice means the links run in a circle: stop as at a root.
            if std::mem::replace(&mut seen[at], true) {
                return (steps, ChainEnd::Root);
            }
            steps.push(Step::Record(at));
            let node = &self.nodes[at];
            let next = match (&node.parent, &node.logical_parent) {
                (Some(parent), _) => parent,
                (None, Some(before_compaction)) => {
                    steps.push(Step::Compaction);
                    before_compaction
                }
                (None, None) => return (steps, ChainEnd::Root),
            };
            match self.by_uuid.get(next) {
                Some(&index) => at = index,
                None => return (steps, ChainEnd::Outside),
            }
        }
    }
}

/// One record as a [`Head`], when it is a JSON object.
fn parse_head(line: &[u8]) -> Option<Head> {
    let line = String::from_utf8_lossy(line);
    if !line.trim_start().starts_with('{') {
        return None;
    }
    serde_json::from_str(&line).ok()
}

/// A place on the chain: a record, or a compaction between two records.
enum Step {
    Record(usize),
    Compaction,
}

/// Why the walk back along a chain stopped.
enum ChainEnd {
    /// At a record with no parent.
    Root,
    /// At a parent that is not in the region.
    Outside,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_agent::{AgentError, HistoryItem, PermissionMode, ToolCallId, ToolDetail, ToolOutcome};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    /// Writes `lines`, each ended by a newline, as `<dir>/s1.jsonl`.
    fn write(dir: &Path, lines: &[String]) -> PathBuf {
        let path = dir.join("s1.jsonl");
        let mut text = String::new();
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        std::fs::write(&path, text).unwrap();
        path
    }

    fn user(uuid: &str, parent: Option<&str>, text: &str) -> String {
        json!({
            "type": "user", "uuid": uuid, "parentUuid": parent, "sessionId": "s1",
            "cwd": "/home/alex/proj",
            "message": {"role": "user", "content": text},
        })
        .to_string()
    }

    fn assistant_by(uuid: &str, parent: Option<&str>, text: &str, model: &str) -> String {
        json!({
            "type": "assistant", "uuid": uuid, "parentUuid": parent, "sessionId": "s1",
            "message": {"role": "assistant", "model": model,
                        "content": [{"type": "text", "text": text}]},
        })
        .to_string()
    }

    fn assistant(uuid: &str, parent: Option<&str>, text: &str) -> String {
        assistant_by(uuid, parent, text, "claude-x-1")
    }

    fn said(text: &str) -> HistoryItem {
        HistoryItem::User {
            text: text.into(),
            image_count: 0,
        }
    }

    fn answered(text: &str) -> HistoryItem {
        HistoryItem::AssistantText(text.into())
    }

    fn read_all(path: &Path) -> StoredHistory {
        read_tail(path, usize::MAX, TAIL_BYTES).unwrap()
    }

    #[test]
    fn a_history_follows_the_chain_not_the_order_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            &[
                user("u1", None, "first question"),
                assistant("a1", Some("u1"), "first answer"),
                user("u2", Some("a1"), "abandoned question"),
                assistant("a2", Some("u2"), "abandoned answer"),
                user("u3", Some("a1"), "second question"),
                assistant("a3", Some("u3"), "second answer"),
            ],
        );
        let history = read_all(&path);
        assert_eq!(
            history.items,
            vec![
                said("first question"),
                answered("first answer"),
                said("second question"),
                answered("second answer"),
            ]
        );
        assert!(!history.earlier_left_out);
    }

    #[test]
    fn the_chain_runs_through_records_that_are_not_messages() {
        let dir = tempfile::tempdir().unwrap();
        let system = json!({
            "type": "system", "subtype": "informational", "uuid": "sys1", "parentUuid": "a1",
            "content": "a note from the program",
        })
        .to_string();
        let path = write(
            dir.path(),
            &[
                user("u1", None, "first question"),
                assistant("a1", Some("u1"), "first answer"),
                system,
                user("u2", Some("sys1"), "second question"),
                assistant("a2", Some("u2"), "second answer"),
            ],
        );
        assert_eq!(
            read_all(&path).items,
            vec![
                said("first question"),
                answered("first answer"),
                said("second question"),
                answered("second answer"),
            ]
        );
    }

    #[test]
    fn a_compaction_is_crossed_and_marked() {
        let dir = tempfile::tempdir().unwrap();
        let boundary = json!({
            "type": "system", "subtype": "compact_boundary", "uuid": "b1",
            "parentUuid": null, "logicalParentUuid": "a1", "content": "Conversation compacted",
        })
        .to_string();
        let summary = json!({
            "type": "user", "uuid": "sum1", "parentUuid": "b1",
            "isCompactSummary": true, "isVisibleInTranscriptOnly": true,
            "message": {"role": "user", "content": "A made-up summary of the talk so far"},
        })
        .to_string();
        let path = write(
            dir.path(),
            &[
                user("u1", None, "first question"),
                assistant("a1", Some("u1"), "first answer"),
                boundary,
                summary,
                user("u2", Some("sum1"), "second question"),
                assistant("a2", Some("u2"), "second answer"),
            ],
        );
        let history = read_all(&path);
        assert_eq!(
            history.items,
            vec![
                said("first question"),
                answered("first answer"),
                HistoryItem::Compacted,
                said("second question"),
                answered("second answer"),
            ]
        );
        assert!(!format!("{history:?}").contains("made-up summary"));
        assert!(!history.earlier_left_out);
    }

    /// `turns` question-and-answer pairs, each answer the parent of the next question.
    fn turns(count: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for n in 0..count {
            let parent = (n > 0).then(|| format!("a{}", n - 1));
            lines.push(user(
                &format!("u{n}"),
                parent.as_deref(),
                &format!("question {n}"),
            ));
            lines.push(assistant(
                &format!("a{n}"),
                Some(&format!("u{n}")),
                &format!("answer {n}"),
            ));
        }
        lines
    }

    #[test]
    fn only_the_end_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &turns(10));
        let history = read_tail(&path, 3, TAIL_BYTES).unwrap();
        assert_eq!(
            history.items,
            vec![
                answered("answer 8"),
                said("question 9"),
                answered("answer 9")
            ]
        );
        assert!(history.earlier_left_out);
        let whole = read_tail(&path, 20, TAIL_BYTES).unwrap();
        assert_eq!(whole.items.len(), 20);
        assert!(!whole.earlier_left_out);
    }

    #[test]
    fn a_file_larger_than_the_region_is_read_from_its_end() {
        let dir = tempfile::tempdir().unwrap();
        let lines: Vec<String> = (0..200)
            .map(|n| {
                let parent = (n > 0).then(|| format!("m{:03}", n - 1));
                user(
                    &format!("m{n:03}"),
                    parent.as_deref(),
                    &format!("turn {n:03}"),
                )
            })
            .collect();
        let line_bytes = lines[100].len();
        assert!((80..=200).contains(&line_bytes), "{line_bytes}");
        let path = write(dir.path(), &lines);
        assert!(std::fs::metadata(&path).unwrap().len() > 4096);

        let history = read_tail(&path, usize::MAX, 4096).unwrap();
        assert_eq!(history.items.last(), Some(&said("turn 199")));
        assert!(history.earlier_left_out);
        assert!(history.items.len() < 200);
        let whole: Vec<HistoryItem> = (0..200).map(|n| said(&format!("turn {n:03}"))).collect();
        assert!(whole.ends_with(&history.items), "{:?}", history.items);
    }

    #[test]
    fn a_side_chain_is_never_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let side = json!({
            "type": "assistant", "uuid": "x1", "parentUuid": "a1", "isSidechain": true,
            "message": {"content": [{"type": "text", "text": "inner work"}]},
        })
        .to_string();
        let path = write(
            dir.path(),
            &[
                user("u1", None, "first question"),
                assistant("a1", Some("u1"), "first answer"),
                side,
            ],
        );
        assert_eq!(
            read_all(&path).items,
            vec![said("first question"), answered("first answer")]
        );
    }

    #[test]
    fn the_last_model_and_mode_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let mode = |name: &str| {
            json!({"type": "permission-mode", "permissionMode": name, "sessionId": "s1"})
                .to_string()
        };
        let path = write(
            dir.path(),
            &[
                user("u1", None, "one"),
                assistant_by("a1", Some("u1"), "one back", "claude-x-1"),
                mode("plan"),
                user("u2", Some("a1"), "two"),
                assistant_by("a2", Some("u2"), "two back", "claude-x-2"),
                mode("auto"),
                user("u3", Some("a2"), "three"),
                assistant_by("a3", Some("u3"), "No response requested.", "<synthetic>"),
            ],
        );
        let history = read_all(&path);
        assert_eq!(history.model.as_deref(), Some("claude-x-2"));
        assert_eq!(history.permission_mode, Some(PermissionMode::Auto));
    }

    #[test]
    fn an_empty_file_is_an_empty_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &[]);
        assert_eq!(read_all(&path), StoredHistory::default());
        let no_message = write(
            dir.path(),
            &[json!({"type": "ai-title", "aiTitle": "Nothing yet"}).to_string()],
        );
        assert_eq!(read_all(&no_message), StoredHistory::default());
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            read_tail(&dir.path().join("missing.jsonl"), 10, TAIL_BYTES),
            Err(AgentError::Io(_))
        ));
    }

    #[test]
    fn a_broken_last_line_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            &[
                user("u1", None, "first question"),
                assistant("a1", Some("u1"), "first answer"),
                user("u2", Some("a1"), "second question"),
                r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","message":{"content":[{"type":"te"#
                    .to_string(),
            ],
        );
        assert_eq!(
            read_all(&path).items,
            vec![
                said("first question"),
                answered("first answer"),
                said("second question"),
            ]
        );
    }

    // Moved here from `history.rs`, where they read whole files.

    const USER: &str = r#"{"type":"user","uuid":"u1","parentUuid":null,"cwd":"/home/alex/proj","message":{"role":"user","content":"Fix the reconnect bug please"}}"#;
    const ASSISTANT: &str = r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"On it."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#;
    const TOOL_DONE: &str = r#"{"type":"user","uuid":"r1","parentUuid":"a1","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"a.txt"}]}}"#;
    const TITLE: &str = r#"{"type":"ai-title","aiTitle":"Fix flaky reconnect","sessionId":"s1"}"#;

    fn lines(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn a_conversation_is_read_in_order_with_tool_outcomes_attached() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &lines(&[USER, ASSISTANT, TOOL_DONE, TITLE]));
        assert_eq!(
            read_all(&path).items,
            vec![
                said("Fix the reconnect bug please"),
                HistoryItem::Thinking("hm".into()),
                answered("On it."),
                HistoryItem::Tool {
                    id: ToolCallId("t1".into()),
                    name: "Bash".into(),
                    detail: ToolDetail::Command {
                        command: "ls".into()
                    },
                    outcome: Some(ToolOutcome::Ok {
                        output: "a.txt".into()
                    })
                },
            ]
        );
    }

    #[test]
    fn side_chains_meta_records_and_command_wrappers_are_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let side = r#"{"type":"assistant","uuid":"x1","parentUuid":"u1","isSidechain":true,"message":{"content":[{"type":"text","text":"inner"}]}}"#;
        let meta = r#"{"type":"user","uuid":"m1","parentUuid":"u1","isMeta":true,"message":{"content":"meta"}}"#;
        let wrapper = r#"{"type":"user","uuid":"w1","parentUuid":"m1","message":{"content":"<command-name>/model</command-name>"}}"#;
        let path = write(dir.path(), &lines(&[USER, side, meta, wrapper]));
        assert_eq!(
            read_all(&path).items,
            vec![said("Fix the reconnect bug please")]
        );
    }

    #[test]
    fn images_in_a_message_are_counted() {
        let dir = tempfile::tempdir().unwrap();
        let with_image = r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"content":[{"type":"text","text":"see"},{"type":"image","source":{}}]}}"#;
        let path = write(dir.path(), &lines(&[with_image]));
        assert_eq!(
            read_all(&path).items,
            vec![HistoryItem::User {
                text: "see".into(),
                image_count: 1
            }]
        );
    }

    #[test]
    fn odd_lines_and_shapes_never_stop_the_reading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s7.jsonl");
        let mut bytes = Vec::new();
        for line in [
            r#"not json at all, with the word "uuid" in it"#.as_bytes(),
            br#"["uuid", "uuid", "uuid", "uuid", "uuid", "uuid", "uuid"]"#,
            br#"{"type":"user","uuid":"o1","parentUuid":null,"message":null}"#,
            br#"{"type":7,"uuid":{"a":1},"parentUuid":[],"isSidechain":"yes"}"#,
            br#"{"type":"user","uuid":"o2","parentUuid":"o1","message":{"content":42}}"#,
            br#"{"type":"assistant","uuid":"o3","parentUuid":"o2","message":{"content":[{"type":"tool_use"},7,{"type":"text"}]}}"#,
            br#"{"type":"user","uuid":"o4","parentUuid":"o3","message":{"content":[{"type":"tool_result","tool_use_id":"nobody","content":"x"}]}}"#,
            b"\xff\xfe broken bytes \"uuid\"",
            br#"{"type":"user","uuid":"o5","parentUuid":"o4","message":{"content":"caf\u00e9"}}"#,
        ] {
            bytes.extend_from_slice(line);
            bytes.push(b'\n');
        }
        std::fs::write(&path, bytes).unwrap();
        let history = read_all(&path);
        assert_eq!(history.items, vec![said("caf\u{e9}")]);
        assert!(!history.earlier_left_out);
    }

    #[test]
    fn a_failed_tool_and_list_shaped_output_are_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let failed = r#"{"type":"user","uuid":"r1","parentUuid":"a1","message":{"content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":[{"type":"text","text":"no"},{"type":"text","text":"way"}]}]}}"#;
        let path = write(dir.path(), &lines(&[USER, ASSISTANT, failed]));
        let items = read_all(&path).items;
        assert!(matches!(
            items.last(),
            Some(HistoryItem::Tool { outcome: Some(ToolOutcome::Failed { output }), .. }) if output == "no\nway"
        ));
    }
}
