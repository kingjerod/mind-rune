//! Stored sessions: the JSON-lines files Claude Code keeps on disk, one per
//! session, at `<projects_dir>/<folder with slashes as dashes>/<session id>.jsonl`.
//! Everything here only reads, and takes the projects directory as a parameter.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use mr_agent::{AgentError, HistoryItem, SessionId, StoredSession, ToolCallId, ToolOutcome};
use serde_json::Value;

use crate::translate::tool_detail;

/// How much of each end of a file `describe` looks at.
const REGION_BYTES: u64 = 262_144;
const TITLE_CHARS: usize = 80;
const FALLBACK_TITLE_CHARS: usize = 8;

/// A session file found on disk, known only by its name and metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredEntry {
    pub id: SessionId,
    pub path: PathBuf,
    pub modified_unix: i64,
    pub bytes: u64,
}

/// What a session file says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredMeta {
    pub folder: Option<PathBuf>,
    pub title: Option<String>,
}

/// The `*.jsonl` files directly inside each immediate subdirectory of
/// `projects_dir`. Reads no file contents. A missing directory gives an empty list.
pub fn list_entries(projects_dir: &Path) -> Vec<StoredEntry> {
    let Ok(projects) = std::fs::read_dir(projects_dir) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for project in projects.flatten() {
        let project_path = project.path();
        if !project_path.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&project_path) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            entries.push(StoredEntry {
                id: SessionId(id.to_string()),
                modified_unix: meta.modified().map(unix_seconds).unwrap_or(0),
                bytes: meta.len(),
                path,
            });
        }
    }
    entries
}

fn unix_seconds(time: std::time::SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    }
}

/// The folder and title of a session, read from at most the first and the last
/// 262,144 bytes of its file, complete lines only. Never fails: a file that
/// cannot be read describes as nothing.
pub fn describe(path: &Path) -> StoredMeta {
    let mut scan = Scan::default();
    if let Ok(mut file) = File::open(path)
        && let Ok(len) = file.metadata().map(|m| m.len())
    {
        if len <= REGION_BYTES {
            if let Some(bytes) = read_region(&mut file, 0, len) {
                scan.lines(&bytes);
            }
        } else {
            // The head ends mid-line, so its last partial line is dropped.
            if let Some(bytes) = read_region(&mut file, 0, REGION_BYTES) {
                let complete = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
                scan.lines(&bytes[..complete]);
            }
            // The tail starts mid-line unless the byte before it is a newline.
            // Read that byte too and drop everything up to the first newline.
            let start = len - REGION_BYTES;
            if let Some(bytes) = read_region(&mut file, start - 1, REGION_BYTES + 1) {
                let first_line = bytes
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(bytes.len(), |i| i + 1);
                scan.lines(&bytes[first_line..]);
            }
        }
    }
    StoredMeta {
        folder: scan.folder,
        title: scan.ai_title.or(scan.first_user_text),
    }
}

fn read_region(file: &mut File, start: u64, len: u64) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(len).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

#[derive(Default)]
struct Scan {
    folder: Option<PathBuf>,
    ai_title: Option<String>,
    first_user_text: Option<String>,
}

impl Scan {
    fn lines(&mut self, bytes: &[u8]) {
        for line in bytes.split(|&b| b == b'\n') {
            self.line(line);
        }
    }

    fn line(&mut self, line: &[u8]) {
        // Cheap checks first: most lines are large records that tell nothing here.
        let wants_folder = self.folder.is_none() && contains(line, b"cwd");
        let wants_title = contains(line, b"ai-title");
        let wants_text = self.first_user_text.is_none() && contains(line, b"user");
        if !(wants_folder || wants_title || wants_text) {
            return;
        }
        let Some(record) = parse_record(line) else {
            return;
        };
        if wants_folder
            && let Some(cwd) = record.get("cwd").and_then(Value::as_str)
            && !cwd.is_empty()
        {
            self.folder = Some(PathBuf::from(cwd));
        }
        match record.get("type").and_then(Value::as_str) {
            Some("ai-title") => {
                if let Some(title) = record.get("aiTitle").and_then(Value::as_str)
                    && !title.trim().is_empty()
                {
                    self.ai_title = Some(title.to_string());
                }
            }
            Some("user") if self.first_user_text.is_none() && !is_side_or_meta(&record) => {
                let content = UserContent::of(&record);
                let text = content.text.trim();
                if !text.is_empty() && !is_command_wrapper(text) {
                    self.first_user_text = Some(text.chars().take(TITLE_CHARS).collect());
                }
            }
            _ => {}
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// One line as a JSON object. Invalid UTF-8 is replaced, not fatal.
fn parse_record(line: &[u8]) -> Option<Value> {
    match serde_json::from_str(&String::from_utf8_lossy(line)) {
        Ok(record @ Value::Object(_)) => Some(record),
        _ => None,
    }
}

fn is_side_or_meta(record: &Value) -> bool {
    let flag = |key: &str| record.get(key).and_then(Value::as_bool).unwrap_or(false);
    flag("isSidechain") || flag("isMeta")
}

fn is_command_wrapper(text: &str) -> bool {
    text.starts_with("<local-command-") || text.starts_with("<command-name>")
}

/// Every stored session at least as new as `modified_since_unix` (all of them
/// for `None`) that names a folder, newest first.
pub fn stored_sessions(
    projects_dir: &Path,
    modified_since_unix: Option<i64>,
) -> Vec<StoredSession> {
    let mut entries = list_entries(projects_dir);
    if let Some(since) = modified_since_unix {
        entries.retain(|entry| entry.modified_unix >= since);
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.modified_unix));
    entries
        .into_iter()
        .filter_map(|entry| {
            let meta = describe(&entry.path);
            Some(StoredSession {
                title: meta
                    .title
                    .unwrap_or_else(|| entry.id.0.chars().take(FALLBACK_TITLE_CHARS).collect()),
                folder: meta.folder?,
                last_active_unix: entry.modified_unix,
                id: entry.id,
            })
        })
        .collect()
}

/// The file of the session with this id, when one exists.
pub fn find_session(projects_dir: &Path, id: &SessionId) -> Option<PathBuf> {
    // An id is a file name. One that is anything else names nothing here.
    if id.0.is_empty() || id.0.contains(['/', '\\', '\0']) || id.0 == "." || id.0 == ".." {
        return None;
    }
    let file_name = format!("{}.jsonl", id.0);
    std::fs::read_dir(projects_dir)
        .ok()?
        .flatten()
        .map(|project| project.path().join(&file_name))
        .find(|path| path.is_file())
}

/// A stored conversation as the UI shows it. A line that is not complete JSON,
/// such as the cut-off last line of a file still being written, is skipped.
pub fn read_history(path: &Path) -> Result<Vec<HistoryItem>, AgentError> {
    let file = File::open(path).map_err(|e| AgentError::Io(e.to_string()))?;
    let mut reader = BufReader::new(file);
    let mut items: Vec<HistoryItem> = Vec::new();
    let mut tool_positions: HashMap<String, usize> = HashMap::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|e| AgentError::Io(e.to_string()))?;
        if read == 0 {
            break;
        }
        // Records that matter name one of these; skip parsing the rest.
        if !(contains(&line, b"user") || contains(&line, b"assistant")) {
            continue;
        }
        if let Some(record) = parse_record(&line) {
            add_record(&record, &mut items, &mut tool_positions);
        }
    }
    Ok(items)
}

fn add_record(
    record: &Value,
    items: &mut Vec<HistoryItem>,
    tool_positions: &mut HashMap<String, usize>,
) {
    if is_side_or_meta(record) {
        return;
    }
    match record.get("type").and_then(Value::as_str) {
        Some("user") => add_user(record, items, tool_positions),
        Some("assistant") => add_assistant(record, items, tool_positions),
        _ => {}
    }
}

fn add_user(record: &Value, items: &mut Vec<HistoryItem>, tool_positions: &HashMap<String, usize>) {
    let content = UserContent::of(record);
    for (tool_use_id, outcome) in content.tool_results {
        if let Some(&position) = tool_positions.get(&tool_use_id)
            && let Some(HistoryItem::Tool { outcome: slot, .. }) = items.get_mut(position)
        {
            *slot = Some(outcome);
        }
    }
    if is_command_wrapper(&content.text) || (content.text.is_empty() && content.image_count == 0) {
        return;
    }
    items.push(HistoryItem::User {
        text: content.text,
        image_count: content.image_count,
    });
}

fn add_assistant(
    record: &Value,
    items: &mut Vec<HistoryItem>,
    tool_positions: &mut HashMap<String, usize>,
) {
    let Some(content) = record.get("message").and_then(|m| m.get("content")) else {
        return;
    };
    if let Some(text) = content.as_str() {
        if !text.is_empty() {
            items.push(HistoryItem::AssistantText(text.to_string()));
        }
        return;
    }
    for block in content.as_array().into_iter().flatten() {
        let text_of = |key: &str| {
            block
                .get(key)
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
        };
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = text_of("text") {
                    items.push(HistoryItem::AssistantText(text.to_string()));
                }
            }
            Some("thinking") => {
                if let Some(text) = text_of("thinking") {
                    items.push(HistoryItem::Thinking(text.to_string()));
                }
            }
            Some("tool_use") => {
                let (Some(id), Some(name)) = (
                    block.get("id").and_then(Value::as_str),
                    block.get("name").and_then(Value::as_str),
                ) else {
                    continue;
                };
                let input = block.get("input").unwrap_or(&Value::Null);
                tool_positions.insert(id.to_string(), items.len());
                items.push(HistoryItem::Tool {
                    id: ToolCallId(id.to_string()),
                    name: name.to_string(),
                    detail: tool_detail(name, input),
                    outcome: None,
                });
            }
            _ => {}
        }
    }
}

/// What a `user` record carries: the typed text (text blocks joined by a
/// newline), how many images, and the tool results it answers.
struct UserContent {
    text: String,
    image_count: usize,
    tool_results: Vec<(String, ToolOutcome)>,
}

impl UserContent {
    fn of(record: &Value) -> Self {
        let mut content = UserContent {
            text: String::new(),
            image_count: 0,
            tool_results: Vec::new(),
        };
        match record.get("message").and_then(|m| m.get("content")) {
            Some(Value::String(text)) => content.text = text.clone(),
            Some(Value::Array(blocks)) => {
                let mut texts: Vec<&str> = Vec::new();
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => texts.extend(block.get("text").and_then(Value::as_str)),
                        Some("image") => content.image_count += 1,
                        Some("tool_result") => {
                            if let Some(id) = block.get("tool_use_id").and_then(Value::as_str) {
                                content
                                    .tool_results
                                    .push((id.to_string(), tool_outcome(block)));
                            }
                        }
                        _ => {}
                    }
                }
                content.text = texts.join("\n");
            }
            _ => {}
        }
        content
    }
}

fn tool_outcome(block: &Value) -> ToolOutcome {
    let output = match block.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    if block
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        ToolOutcome::Failed { output }
    } else {
        ToolOutcome::Ok { output }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_agent::{ToolCallId, ToolDetail, ToolOutcome};
    use std::path::{Path, PathBuf};

    /// Writes `lines` joined with newlines as `<dir>/<folder>/<id>.jsonl`.
    fn write(dir: &Path, folder: &str, id: &str, lines: &[&str]) -> PathBuf {
        let project = dir.join(folder);
        std::fs::create_dir_all(&project).unwrap();
        let path = project.join(format!("{id}.jsonl"));
        std::fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    const USER: &str = r#"{"type":"user","cwd":"/home/u/proj","message":{"role":"user","content":"Fix the reconnect bug please"}}"#;
    const ASSISTANT: &str = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"On it."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#;
    const TOOL_DONE: &str = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"a.txt"}]}}"#;
    const TITLE: &str = r#"{"type":"ai-title","aiTitle":"Fix flaky reconnect","sessionId":"s1"}"#;

    #[test]
    fn listing_sees_only_session_files_one_level_down() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "-home-u-proj", "s1", &[USER]);
        std::fs::write(dir.path().join("-home-u-proj/notes.txt"), "x").unwrap();
        std::fs::create_dir_all(dir.path().join("-home-u-proj/s1/subagents")).unwrap();
        std::fs::write(dir.path().join("-home-u-proj/s1/subagents/a.jsonl"), USER).unwrap();
        let entries = list_entries(dir.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, SessionId("s1".into()));
        assert!(list_entries(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn a_session_is_described_by_its_folder_and_latest_title() {
        let dir = tempfile::tempdir().unwrap();
        let older = r#"{"type":"ai-title","aiTitle":"First guess"}"#;
        let path = write(
            dir.path(),
            "-home-u-proj",
            "s1",
            &[USER, older, ASSISTANT, TITLE],
        );
        assert_eq!(describe(&path).folder, Some("/home/u/proj".into()));
        assert_eq!(
            describe(&path).title.as_deref(),
            Some("Fix flaky reconnect")
        );
    }

    #[test]
    fn without_a_title_the_first_message_names_it_cut_to_eighty_characters() {
        let dir = tempfile::tempdir().unwrap();
        let long = format!(
            r#"{{"type":"user","cwd":"/p","message":{{"content":"{}"}}}}"#,
            "x".repeat(200)
        );
        let path = write(dir.path(), "-p", "s2", &[&long]);
        assert_eq!(describe(&path).title.unwrap().chars().count(), 80);
    }

    #[test]
    fn a_title_at_the_end_of_a_large_file_is_still_found() {
        let dir = tempfile::tempdir().unwrap();
        let filler = format!(
            r#"{{"type":"api-request-blob","message":"{}"}}"#,
            "y".repeat(4000)
        );
        let mut lines = vec![USER.to_string()];
        lines.extend(std::iter::repeat_n(filler, 300)); // about 1.2 MB
        lines.push(TITLE.to_string());
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let path = write(dir.path(), "-home-u-proj", "s3", &refs);
        assert_eq!(
            describe(&path).title.as_deref(),
            Some("Fix flaky reconnect")
        );
        assert_eq!(describe(&path).folder, Some("/home/u/proj".into()));
    }

    #[test]
    fn stored_sessions_are_newest_first_and_filtered_by_time() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "-home-u-proj", "s1", &[USER, TITLE]);
        write(dir.path(), "-home-u-proj", "nofolder", &[TITLE]);
        let all = stored_sessions(dir.path(), None);
        assert_eq!(all.len(), 1, "a session with no folder is left out");
        assert_eq!(all[0].title, "Fix flaky reconnect");
        assert_eq!(all[0].folder, PathBuf::from("/home/u/proj"));
        assert!(stored_sessions(dir.path(), Some(all[0].last_active_unix + 10)).is_empty());
        assert_eq!(
            find_session(dir.path(), &SessionId("s1".into()))
                .unwrap()
                .file_name()
                .unwrap(),
            "s1.jsonl"
        );
        assert!(find_session(dir.path(), &SessionId("zzz".into())).is_none());
    }

    #[test]
    fn a_conversation_is_read_in_order_with_tool_outcomes_attached() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "-home-u-proj",
            "s1",
            &[USER, ASSISTANT, TOOL_DONE, TITLE],
        );
        assert_eq!(
            read_history(&path).unwrap(),
            vec![
                HistoryItem::User {
                    text: "Fix the reconnect bug please".into(),
                    image_count: 0
                },
                HistoryItem::Thinking("hm".into()),
                HistoryItem::AssistantText("On it.".into()),
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
        let side = r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"inner"}]}}"#;
        let meta = r#"{"type":"user","isMeta":true,"message":{"content":"meta"}}"#;
        let wrapper =
            r#"{"type":"user","message":{"content":"<command-name>/model</command-name>"}}"#;
        let path = write(dir.path(), "-p", "s4", &[side, meta, wrapper, USER]);
        assert_eq!(read_history(&path).unwrap().len(), 1);
    }

    #[test]
    fn a_cut_off_last_line_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "-p",
            "s5",
            &[
                USER,
                r#"{"type":"assistant","message":{"content":[{"type":"te"#,
            ],
        );
        assert_eq!(read_history(&path).unwrap().len(), 1);
        assert!(matches!(
            read_history(Path::new("/nonexistent.jsonl")),
            Err(AgentError::Io(_))
        ));
    }

    #[test]
    fn images_in_a_message_are_counted() {
        let dir = tempfile::tempdir().unwrap();
        let with_image = r#"{"type":"user","message":{"content":[{"type":"text","text":"see"},{"type":"image","source":{}}]}}"#;
        let path = write(dir.path(), "-p", "s6", &[with_image]);
        assert_eq!(
            read_history(&path).unwrap(),
            vec![HistoryItem::User {
                text: "see".into(),
                image_count: 1
            }]
        );
    }
    #[test]
    fn odd_lines_and_shapes_never_stop_the_reading() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("-p");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.join("s7.jsonl");
        let mut bytes = Vec::new();
        for line in [
            "not json at all, with the word user in it".as_bytes(),
            b"[1,2,3] user",
            br#"{"type":"user","message":null}"#,
            br#"{"type":"user","message":{"content":42}}"#,
            br#"{"type":"assistant","message":{"content":[{"type":"tool_use"},7,{"type":"text"}]}}"#,
            br#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"nobody","content":"x"}]}}"#,
            b"\xff\xfe broken bytes user",
            br#"{"type":"user","message":{"content":"caf\u00e9"}}"#,
        ] {
            bytes.extend_from_slice(line);
            bytes.push(b'\n');
        }
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(
            read_history(&path).unwrap(),
            vec![HistoryItem::User {
                text: "caf\u{e9}".into(),
                image_count: 0
            }]
        );
        assert_eq!(
            describe(&path),
            StoredMeta {
                folder: None,
                title: Some("caf\u{e9}".into())
            }
        );
    }

    #[test]
    fn an_id_that_is_not_a_plain_file_name_finds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects");
        // Files a lookup without the guard would find, through `<project>/<id>.jsonl`.
        write(&projects, "-p", "real", &[USER]);
        write(&projects, "-p/a", "b", &[USER]);
        write(&projects, "-p", "", &[USER]);
        write(&projects, "-p", "..", &[USER]);
        write(&projects, "-p", "a\\b", &[USER]);
        write(dir.path(), "projects", "x", &[USER]);
        assert!(find_session(&projects, &SessionId("real".into())).is_some());
        for id in ["../x", "a/b", "", "..", "a\\b"] {
            let planted = projects.join("-p").join(format!("{id}.jsonl"));
            assert!(planted.is_file(), "{id:?} names a planted file");
            assert_eq!(
                find_session(&projects, &SessionId(id.into())),
                None,
                "{id:?}"
            );
        }
    }

    #[test]
    fn a_failed_tool_and_list_shaped_output_are_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let failed = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":[{"type":"text","text":"no"},{"type":"text","text":"way"}]}]}}"#;
        let path = write(dir.path(), "-p", "s8", &[ASSISTANT, failed]);
        let items = read_history(&path).unwrap();
        assert!(matches!(
            items.last(),
            Some(HistoryItem::Tool { outcome: Some(ToolOutcome::Failed { output }), .. }) if output == "no\nway"
        ));
    }

    #[test]
    fn a_line_longer_than_the_head_region_does_not_break_describe() {
        let dir = tempfile::tempdir().unwrap();
        let huge = format!(
            r#"{{"type":"api-request-blob","message":"{}"}}"#,
            "z".repeat(600_000)
        );
        let path = write(dir.path(), "-p", "s9", &[USER, &huge, TITLE]);
        let meta = describe(&path);
        assert_eq!(meta.folder, Some("/home/u/proj".into()));
        assert_eq!(meta.title.as_deref(), Some("Fix flaky reconnect"));
    }
}
