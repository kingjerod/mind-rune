//! The end of a stored conversation, read from the end of its file along the chain of
//! replies, so that a very large session opens as fast as a small one.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::Path;

use mr_agent::{AgentError, HistoryItem, StoredHistory};
use serde::Deserialize;
use serde_json::Value;

use crate::history::{Walk, add_record, contains, parse_record, stored_images};
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
            effort: None,
            tasks: Vec::new(),
            choices: None,
        });
    };
    let (steps, end) = records.chain_to(leaf);

    let mut walk = Walk::default();
    let mut model = None;
    let mut effort = None;
    // The user's messages that have images, by their place in `items`, with their records:
    // the images are decoded once the end is known, for the messages kept only.
    let mut with_images: VecDeque<(usize, Value)> = VecDeque::new();
    for step in steps.iter().rev() {
        match step {
            Step::Compaction => walk.items.push(HistoryItem::Compacted),
            Step::Record(index) => {
                let Some(record) = parse_record(&region[records.nodes[*index].line.clone()]) else {
                    continue;
                };
                if let Some(name) = answering_model(&record) {
                    model = Some(name.to_string());
                }
                if let Some(level) = reply_effort(&record) {
                    effort = Some(level.to_string());
                }
                let before = walk.items.len();
                add_record(&record, &mut walk);
                if walk.items.len() > before
                    && let Some(HistoryItem::User { image_count, .. }) = walk.items.last()
                    && *image_count > 0
                {
                    with_images.push_back((before, record));
                }
                // Items are only ever added: one this far from the end is cut.
                while with_images
                    .front()
                    .is_some_and(|(at, _)| walk.items.len() - at > max_items)
                {
                    with_images.pop_front();
                }
            }
        }
    }

    // The list and the offer come of every record read, not only of the items kept.
    let (mut items, tasks, choices) = walk.end();
    let cut = items.len().saturating_sub(max_items);
    items.drain(..cut);
    for (at, record) in with_images {
        if let Some(HistoryItem::User { images, .. }) = items.get_mut(at - cut) {
            let read = stored_images(&record);
            // `UserContent::of` counted the slots by the same rule `stored_images` reads by.
            debug_assert_eq!(read.len(), images.len(), "one entry per image counted");
            *images = read;
        }
    }
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
        effort,
        tasks,
        choices,
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

/// The effort level an `assistant` record names.
fn reply_effort(record: &Value) -> Option<&str> {
    if record.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    record
        .get("effort")
        .and_then(Value::as_str)
        .filter(|level| !level.is_empty())
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

    /// The user's message `uuid` of `text`.
    fn said(uuid: &str, text: &str) -> HistoryItem {
        HistoryItem::User {
            text: text.into(),
            image_count: 0,
            files: Vec::new(),
            images: Vec::new(),
            stored: Some(uuid.into()),
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
                said("u1", "first question"),
                answered("first answer"),
                said("u3", "second question"),
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
                said("u1", "first question"),
                answered("first answer"),
                said("u2", "second question"),
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
                said("u1", "first question"),
                answered("first answer"),
                HistoryItem::Compacted,
                said("u2", "second question"),
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
                said("u9", "question 9"),
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
        assert_eq!(history.items.last(), Some(&said("m199", "turn 199")));
        assert!(history.earlier_left_out);
        assert!(history.items.len() < 200);
        let whole: Vec<HistoryItem> = (0..200)
            .map(|n| said(&format!("m{n:03}"), &format!("turn {n:03}")))
            .collect();
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
            vec![said("u1", "first question"), answered("first answer")]
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

    /// An answer by `claude-x-1` that names the effort level it was made with.
    fn assistant_with_effort(uuid: &str, parent: Option<&str>, text: &str, effort: &str) -> String {
        let mut record: Value = serde_json::from_str(&assistant(uuid, parent, text)).unwrap();
        record["effort"] = json!(effort);
        record["perTurnEffort"] = Value::Null;
        record.to_string()
    }

    #[test]
    fn a_stored_session_gives_the_effort_of_its_last_reply() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            &[
                user("u1", None, "one"),
                assistant_with_effort("a1", Some("u1"), "one back", "low"),
                user("u2", Some("a1"), "abandoned"),
                assistant_with_effort("a2", Some("u2"), "abandoned back", "max"),
                user("u3", Some("a1"), "two"),
                assistant_with_effort("a3", Some("u3"), "two back", "high"),
                user("u4", Some("a3"), "three"),
            ],
        );
        assert_eq!(read_all(&path).effort.as_deref(), Some("high"));
    }

    #[test]
    fn a_stored_session_without_effort_gives_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            &[
                user("u1", None, "one"),
                assistant("a1", Some("u1"), "one back"),
            ],
        );
        assert_eq!(read_all(&path).effort, None);
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
                said("u1", "first question"),
                answered("first answer"),
                said("u2", "second question"),
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
                said("u1", "Fix the reconnect bug please"),
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
            vec![said("u1", "Fix the reconnect bug please")]
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
                image_count: 1,
                files: Vec::new(),
                images: vec![None],
                stored: Some("u1".into()),
            }]
        );
    }

    /// A user record of `blocks`, the first on its chain. Written by hand, field names and
    /// shapes only: never seen live beyond one image block per image sent.
    fn user_of(blocks: serde_json::Value) -> String {
        json!({
            "type": "user", "uuid": "u1", "parentUuid": null, "sessionId": "s1",
            "message": {"role": "user", "content": blocks},
        })
        .to_string()
    }

    /// An image block holding `bytes` as base64, as the Messages format writes one.
    fn image_block(bytes: &[u8]) -> serde_json::Value {
        use base64::Engine;
        json!({"type": "image", "source": {
            "type": "base64", "media_type": "image/png",
            "data": base64::engine::general_purpose::STANDARD.encode(bytes),
        }})
    }

    /// Made-up bytes that begin as a PNG does: the reader does not decode images.
    fn png_like(fill: u8, length: usize) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(length, fill);
        bytes
    }

    /// Whether `image` was read, and holds `bytes`.
    fn holds(image: &Option<mr_agent::ImageData>, bytes: &[u8]) -> bool {
        image
            .as_ref()
            .is_some_and(|image| image.as_bytes() == bytes)
    }

    /// The images of the only item of the history at `path`, and its image count.
    fn images_of(path: &Path) -> (usize, Vec<Option<mr_agent::ImageData>>) {
        match read_all(path).items.as_slice() {
            [
                HistoryItem::User {
                    image_count,
                    images,
                    ..
                },
            ] => (*image_count, images.clone()),
            other => panic!("not one message of the user's: {other:?}"),
        }
    }

    #[test]
    fn a_stored_image_gives_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = png_like(7, 1000);
        let record = user_of(json!([{"type": "text", "text": "see"}, image_block(&bytes)]));
        let path = write(dir.path(), &[record]);
        let (count, images) = images_of(&path);
        assert_eq!(count, 1);
        assert_eq!(images.len(), 1);
        assert!(holds(&images[0], &bytes), "the bytes changed");
        // Every length of the last group of base64 decodes whole.
        for length in 9..13 {
            let bytes = png_like(3, length);
            let path = write(dir.path(), &[user_of(json!([image_block(&bytes)]))]);
            let (_, images) = images_of(&path);
            assert!(
                images.len() == 1 && holds(&images[0], &bytes),
                "{length} bytes did not come through"
            );
        }
    }

    #[test]
    fn an_image_with_another_source_is_counted_and_gives_no_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let by_address = json!({"type": "image", "source": {
            "type": "url", "url": "https://example.org/shot.png",
        }});
        let path = write(dir.path(), &[user_of(json!([by_address]))]);
        let (count, images) = images_of(&path);
        assert_eq!(count, 1);
        assert_eq!(images, vec![None]);
    }

    #[test]
    fn bad_base64_is_counted_and_gives_no_bytes() {
        let dir = tempfile::tempdir().unwrap();
        for data in ["not base64 at all!", "QUJD=", "", "QU"] {
            let block = json!({"type": "image", "source": {
                "type": "base64", "media_type": "image/png", "data": data,
            }});
            let path = write(dir.path(), &[user_of(json!([block]))]);
            let (count, images) = images_of(&path);
            assert_eq!(count, 1, "{data:?}");
            assert_eq!(images, vec![None], "{data:?}");
        }
    }

    #[test]
    fn images_files_and_text_keep_their_counts_and_order() {
        let dir = tempfile::tempdir().unwrap();
        let (first, second) = (png_like(1, 40), png_like(2, 50));
        let by_address = json!({"type": "image", "source": {
            "type": "url", "url": "https://example.org/shot.png",
        }});
        let record = user_of(json!([
            {"type": "text", "text": "look\n\nAttached files:\n- /home/alex/proj/notes.txt"},
            image_block(&first),
            by_address,
            image_block(&second),
        ]));
        let path = write(dir.path(), &[record]);
        let history = read_all(&path);
        let [
            HistoryItem::User {
                text,
                image_count,
                files,
                images,
                ..
            },
        ] = history.items.as_slice()
        else {
            panic!("not one message of the user's");
        };
        assert_eq!(text, "look");
        assert_eq!(files, &[PathBuf::from("/home/alex/proj/notes.txt")]);
        assert_eq!(*image_count, 3);
        assert_eq!(images.len(), 3);
        assert!(holds(&images[0], &first), "the first changed");
        // The one held another way keeps its place: the third does not move up.
        assert_eq!(images[1], None);
        assert!(holds(&images[2], &second), "the third changed");
    }

    #[test]
    fn the_images_of_a_message_kept_come_through_the_cut() {
        let dir = tempfile::tempdir().unwrap();
        let (early, late) = (png_like(1, 30), png_like(2, 30));
        let first = json!({
            "type": "user", "uuid": "u1", "parentUuid": null,
            "message": {"content": [{"type": "text", "text": "one"}, image_block(&early)]},
        });
        let second = json!({
            "type": "user", "uuid": "u2", "parentUuid": "a1",
            "message": {"content": [{"type": "text", "text": "two"}, image_block(&late)]},
        });
        let path = write(
            dir.path(),
            &[
                first.to_string(),
                assistant("a1", Some("u1"), "Done."),
                second.to_string(),
            ],
        );
        let history = read_tail(&path, 1, TAIL_BYTES).unwrap();
        let [HistoryItem::User { text, images, .. }] = history.items.as_slice() else {
            panic!("not the last message alone: {:?}", history.items);
        };
        assert_eq!(text, "two");
        assert_eq!(images.len(), 1);
        assert!(holds(&images[0], &late), "the image changed");
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
        assert_eq!(history.items, vec![said("o5", "caf\u{e9}")]);
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

    /// Records written one after another, each the reply to the one before. Written by
    /// hand, field names and shapes only.
    struct Chain {
        lines: Vec<String>,
        last: Option<String>,
    }

    impl Chain {
        /// A chain whose first record answers `parent`, which need not be in the file.
        fn after(parent: Option<&str>) -> Self {
            Chain {
                lines: Vec::new(),
                last: parent.map(str::to_string),
            }
        }

        fn new() -> Self {
            Chain::after(None)
        }

        fn push(&mut self, mut record: serde_json::Value) -> &mut Self {
            let uuid = format!("c{}", self.lines.len() + 1);
            record["uuid"] = json!(uuid);
            record["parentUuid"] = json!(self.last);
            record["sessionId"] = json!("s1");
            self.lines.push(record.to_string());
            self.last = Some(uuid);
            self
        }

        fn user(&mut self, text: &str) -> &mut Self {
            self.push(json!({"type": "user", "message": {"role": "user", "content": text}}))
        }

        fn reply(&mut self, text: &str) -> &mut Self {
            self.push(json!({"type": "assistant", "message": {"role": "assistant",
                "model": "claude-x-1", "content": [{"type": "text", "text": text}]}}))
        }

        /// A call of the tool `name` with `input`, as the call `id`.
        fn call(&mut self, id: &str, name: &str, input: serde_json::Value) -> &mut Self {
            self.push(json!({"type": "assistant", "message": {"role": "assistant",
                "model": "claude-x-1",
                "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}}))
        }

        /// The result `text` of the call `id`, with `top` merged into the record.
        fn result_with(&mut self, id: &str, text: &str, top: serde_json::Value) -> &mut Self {
            let mut record = json!({"type": "user", "message": {"role": "user",
                "content": [{"type": "tool_result", "tool_use_id": id, "content": text}]}});
            for (key, value) in top.as_object().into_iter().flatten() {
                record[key] = value.clone();
            }
            self.push(record)
        }

        fn result(&mut self, id: &str, text: &str) -> &mut Self {
            self.result_with(id, text, json!({}))
        }

        /// A `TaskCreate` of `subject` as the call `id`, and its result naming the step `step`.
        fn create(&mut self, id: &str, step: &str, subject: &str) -> &mut Self {
            self.call(
                id,
                "TaskCreate",
                json!({"subject": subject, "description": subject,
                "activeForm": format!("Doing {subject}")}),
            )
            .result_with(
                id,
                &format!("Task #{step} created successfully: {subject}"),
                json!({"toolUseResult": {"task": {"id": step, "subject": subject}}}),
            )
        }

        /// A `TaskUpdate` of the step `step` to `status`, as the call `id`, and its result.
        fn update(&mut self, id: &str, step: &str, status: &str) -> &mut Self {
            self.call(id, "TaskUpdate", json!({"taskId": step, "status": status}))
                .result_with(
                    id,
                    &format!("Updated task #{step} status"),
                    json!({"toolUseResult": {"success": true, "taskId": step}}),
                )
        }

        fn offer(&mut self, id: &str, question: &str, labels: [&str; 2]) -> &mut Self {
            let choices: Vec<serde_json::Value> = labels
                .iter()
                .map(|label| json!({"label": label, "message": format!("{label}, please")}))
                .collect();
            self.call(
                id,
                "mcp__mindrune__offer_choices",
                json!({"question": question, "choices": choices}),
            )
            .result(id, "The choices are shown to the user.")
        }

        fn read(&self) -> StoredHistory {
            let dir = tempfile::tempdir().unwrap();
            read_all(&write(dir.path(), &self.lines))
        }
    }

    fn steps(history: &StoredHistory) -> Vec<(&str, mr_agent::TaskStatus)> {
        history
            .tasks
            .iter()
            .map(|step| (step.id.as_str(), step.status))
            .collect()
    }

    fn tool_names(history: &StoredHistory) -> Vec<&str> {
        history
            .items
            .iter()
            .filter_map(|item| match item {
                HistoryItem::Tool { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_list_with_an_open_step_comes_back_and_its_calls_are_no_items() {
        use mr_agent::TaskStatus::{Completed, InProgress};
        let history = Chain::new()
            .user("Make three files")
            .create("toolu_a", "1", "Create a.txt")
            .call(
                "toolu_b",
                "TaskCreate",
                json!({"subject": "Create b.txt", "activeForm": "Creating b.txt"}),
            )
            .result("toolu_b", "Task #2 created successfully: Create b.txt")
            .update("toolu_c", "1", "in_progress")
            .update("toolu_d", "1", "completed")
            .update("toolu_e", "2", "in_progress")
            .call(
                "toolu_f",
                "Write",
                json!({"file_path": "/home/alex/proj/b.txt", "content": "b"}),
            )
            .result("toolu_f", "File created")
            .read();
        assert_eq!(steps(&history), [("1", Completed), ("2", InProgress)]);
        assert_eq!(history.tasks[1].subject, "Create b.txt");
        assert_eq!(history.tasks[1].active.as_deref(), Some("Creating b.txt"));
        assert_eq!(tool_names(&history), ["Write"]);
        assert_eq!(
            history.items.len(),
            2,
            "the message and the Write: {:?}",
            history.items
        );
    }

    #[test]
    fn a_finished_list_does_not_come_back() {
        let history = Chain::new()
            .user("Make two files")
            .create("toolu_a", "1", "Create a.txt")
            .create("toolu_b", "2", "Create b.txt")
            .update("toolu_c", "1", "completed")
            .update("toolu_d", "2", "completed")
            .reply("Both are made.")
            .read();
        assert!(history.tasks.is_empty(), "{:?}", history.tasks);
        assert!(tool_names(&history).is_empty());
    }

    #[test]
    fn a_new_message_ends_the_turn_for_the_list() {
        let history = Chain::new()
            .user("Make two files")
            .create("toolu_a", "1", "Create a.txt")
            .create("toolu_b", "2", "Create b.txt")
            .update("toolu_c", "1", "completed")
            .update("toolu_d", "2", "completed")
            .user("And a third")
            .create("toolu_e", "3", "Create c.txt")
            .read();
        assert_eq!(steps(&history), [("3", mr_agent::TaskStatus::Pending)]);
    }

    #[test]
    fn the_last_offer_comes_back_unless_something_was_said_after_it() {
        let mut chain = Chain::new();
        chain
            .user("Fix the bug")
            .reply("Fixed.")
            .offer("toolu_a", "Commit the fix?", ["Commit", "Not yet"])
            .reply("Over to you.");
        let history = chain.read();
        let offer = history.choices.as_ref().expect("the offer comes back");
        assert_eq!(offer.question, "Commit the fix?");
        assert_eq!(
            offer
                .choices
                .iter()
                .map(|c| (c.label.as_str(), c.message.as_str()))
                .collect::<Vec<_>>(),
            [("Commit", "Commit, please"), ("Not yet", "Not yet, please")]
        );
        assert!(tool_names(&history).is_empty(), "the offer is no tool line");

        let mut answered = Chain::new();
        answered
            .user("Fix the bug")
            .offer("toolu_a", "Commit the fix?", ["Commit", "Not yet"])
            .user("Commit, please");
        assert_eq!(answered.read().choices, None);

        let mut interrupted = Chain::new();
        interrupted
            .user("Fix the bug")
            .offer("toolu_a", "Commit the fix?", ["Commit", "Not yet"])
            .user("[Request interrupted by user]");
        assert_eq!(
            interrupted.read().choices,
            None,
            "a stored interruption is said after it"
        );

        let mut twice = Chain::new();
        twice
            .user("Fix the bug")
            .offer("toolu_a", "Commit the fix?", ["Commit", "Not yet"])
            .offer("toolu_b", "Push it too?", ["Push", "Wait"]);
        assert_eq!(
            twice.read().choices.map(|offer| offer.question),
            Some("Push it too?".into())
        );

        let mut refused = Chain::new();
        refused
            .user("Fix the bug")
            .offer("toolu_a", "Commit the fix?", ["Commit", "Not yet"])
            .call(
                "toolu_b",
                "mcp__mindrune__offer_choices",
                json!({"question": "", "choices": []}),
            )
            .result_with(
                "toolu_b",
                "A question is needed.",
                json!({"toolUseResult": "Error: A question is needed."}),
            );
        let history = refused.read();
        assert_eq!(
            history
                .choices
                .as_ref()
                .map(|offer| offer.question.as_str()),
            Some("Commit the fix?"),
            "an offer that cannot be shown changes nothing"
        );
        assert!(tool_names(&history).is_empty());
    }

    #[test]
    fn the_live_spelling_of_the_result_is_read_too() {
        let history = Chain::new()
            .user("Make a file")
            .call("toolu_a", "TaskCreate", json!({"subject": "Create a.txt"}))
            .result_with(
                "toolu_a",
                "Done.",
                json!({"tool_use_result": {"task": {"id": "7", "subject": "Create a.txt"}}}),
            )
            .read();
        assert_eq!(steps(&history), [("7", mr_agent::TaskStatus::Pending)]);
    }

    #[test]
    fn a_cut_off_conversation_costs_only_what_was_cut() {
        let history = Chain::after(Some("before-the-file"))
            .update("toolu_a", "1", "completed")
            .update("toolu_b", "2", "in_progress")
            .create("toolu_c", "3", "Create c.txt")
            .read();
        assert_eq!(steps(&history), [("3", mr_agent::TaskStatus::Pending)]);
        assert!(history.items.is_empty(), "{:?}", history.items);
        assert!(history.earlier_left_out);
    }

    #[test]
    fn a_failed_call_and_a_search_for_the_tools_leave_no_trace() {
        let mut chain = Chain::new();
        chain
            .user("Make a file")
            .call(
                "toolu_a",
                "ToolSearch",
                json!({"query": "select:TaskCreate,TaskUpdate", "max_results": 2}),
            )
            .result("toolu_a", "Loaded.")
            .call("toolu_b", "TaskCreate", json!({"subject": "Create a.txt"}));
        chain.push(
            json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "toolu_b", "is_error": true,
             "content": "Task #1 created successfully: Create a.txt"}]},
            "toolUseResult": {"task": {"id": "1"}}}),
        );
        chain
            .call(
                "toolu_c",
                "ToolSearch",
                json!({"query": "select:TaskCreate,Bash", "max_results": 2}),
            )
            .result("toolu_c", "Loaded.");
        let history = chain.read();
        assert!(history.tasks.is_empty(), "{:?}", history.tasks);
        assert_eq!(tool_names(&history), ["ToolSearch"]);
        assert!(matches!(
            history.items.last(),
            Some(HistoryItem::Tool { id, outcome: Some(ToolOutcome::Ok { .. }), .. }) if id.0 == "toolu_c"
        ));
    }

    #[test]
    fn a_record_of_several_results_names_each_step_by_its_text() {
        let mut chain = Chain::new();
        chain
            .user("Make two files")
            .push(json!({"type": "assistant", "message": {
            "role": "assistant", "model": "claude-x-1", "content": [
                {"type": "tool_use", "id": "toolu_a", "name": "TaskCreate",
                 "input": {"subject": "Create a.txt"}},
                {"type": "tool_use", "id": "toolu_b", "name": "TaskCreate",
                 "input": {"subject": "Create b.txt"}}]}}));
        chain.push(
            json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "toolu_a",
             "content": "Task #1 created successfully: Create a.txt"},
            {"type": "tool_result", "tool_use_id": "toolu_b",
             "content": "Task #2 created successfully: Create b.txt"}]},
            "toolUseResult": {"task": {"id": "9"}}}),
        );
        use mr_agent::TaskStatus::Pending;
        assert_eq!(steps(&chain.read()), [("1", Pending), ("2", Pending)]);
    }

    #[test]
    fn a_result_that_comes_after_the_next_message_is_applied() {
        let history = Chain::new()
            .user("Make a file")
            .call("toolu_a", "TaskCreate", json!({"subject": "Create a.txt"}))
            .user("Stop")
            .result("toolu_a", "Task #1 created successfully: Create a.txt")
            .read();
        assert_eq!(steps(&history), [("1", mr_agent::TaskStatus::Pending)]);
    }

    #[test]
    fn a_slash_command_answers_an_offer_and_ends_the_lists_turn() {
        let mut chain = Chain::new();
        chain
            .user("Fix the bug")
            .create("toolu_a", "1", "Fix it")
            .update("toolu_b", "1", "completed")
            .offer("toolu_c", "Apply the fix?", ["Apply", "Not now"])
            .user("<command-name>/apply</command-name>\n<command-message>apply</command-message>");
        chain.push(
            json!({"type": "user", "isMeta": true, "message": {"role": "user",
            "content": "Apply the change that was made."}}),
        );
        chain.reply("Applied.").create("toolu_d", "2", "Check it");
        let history = chain.read();
        assert_eq!(history.choices, None, "the offer was answered");
        assert_eq!(steps(&history), [("2", mr_agent::TaskStatus::Pending)]);
        let messages = history
            .items
            .iter()
            .filter(|item| matches!(item, HistoryItem::User { .. }))
            .count();
        assert_eq!(
            messages, 1,
            "the command is still no item: {:?}",
            history.items
        );
    }

    #[test]
    fn an_offer_comes_back_only_once_its_result_says_it_was_shown() {
        let offer = json!({"question": "Go on?", "choices": [{"label": "Yes", "message": "Yes."}]});
        let mut unanswered = Chain::new();
        unanswered.user("Fix the bug").call(
            "toolu_a",
            "mcp__mindrune__offer_choices",
            offer.clone(),
        );
        assert_eq!(unanswered.read().choices, None, "no result yet");

        let mut failed = Chain::new();
        failed
            .user("Fix the bug")
            .call("toolu_a", "mcp__mindrune__offer_choices", offer.clone());
        failed.push(
            json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "toolu_a", "is_error": true,
             "content": "Permission to use the tool was denied."}]}}),
        );
        assert_eq!(failed.read().choices, None, "the program refused the call");

        let mut late = Chain::new();
        late.user("Fix the bug")
            .call("toolu_a", "mcp__mindrune__offer_choices", offer)
            .user("Something else")
            .result("toolu_a", "Shown to the user.");
        assert_eq!(
            late.read().choices,
            None,
            "something was said before its result"
        );
    }
}
