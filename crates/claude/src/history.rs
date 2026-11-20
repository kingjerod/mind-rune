//! Stored sessions: the JSON-lines files Claude Code keeps on disk, one per
//! session, at `<projects_dir>/<folder with slashes as dashes>/<session id>.jsonl`.
//! Everything here only reads, and takes the projects directory as a parameter.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mr_agent::{
    AgentError, Choices, HistoryItem, ImageData, SessionEvent, SessionId, StoredSession, TaskList,
    TaskStep, ToolCallId, ToolOutcome,
};
use serde_json::Value;

use crate::encode::split_attached;
use crate::remove::no_store;
use crate::tasks::{self, Hidden};
use crate::translate::tool_detail;

/// How much of each end of a file `describe` looks at.
const REGION_BYTES: u64 = 262_144;
const TITLE_CHARS: usize = 80;
const FALLBACK_TITLE_CHARS: usize = 8;
/// What a record's `entrypoint` says of a session started through the TypeScript or the Python
/// SDK, which a program does. A person starts one in the terminal (`cli`), in Claude Desktop
/// (`claude-desktop`) or here (`sdk-cli`, which `claude -p` writes as well).
const PROGRAM_ENTRYPOINTS: [&str; 2] = ["sdk-ts", "sdk-py"];

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
    /// How the session was started: the `entrypoint` of the first record that names one, among
    /// the lines [`describe`] reads.
    pub entrypoint: Option<String>,
}

impl StoredMeta {
    /// A program started the session through the SDK; no person did.
    pub(crate) fn made_by_program(&self) -> bool {
        self.entrypoint
            .as_deref()
            .is_some_and(|entrypoint| PROGRAM_ENTRYPOINTS.contains(&entrypoint))
    }
}

/// The `*.jsonl` files directly inside each immediate subdirectory of
/// `projects_dir`. Reads no file contents. A directory not made yet, in a folder that is
/// there, gives an empty list: a fresh install. One whose folder is not there, or that is a
/// link to nowhere, is the error "the store could not be found"; one that cannot be read, or
/// a project's directory that cannot, is an error too. So a store that could not be found or
/// read is never taken for an empty one. A project's directory or a file gone while it is
/// listed is skipped.
pub fn list_entries(projects_dir: &Path) -> Result<Vec<StoredEntry>, AgentError> {
    let projects = match std::fs::read_dir(projects_dir) {
        Ok(projects) => projects,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return if not_made_yet(projects_dir) {
                Ok(Vec::new())
            } else {
                Err(no_store())
            };
        }
        Err(e) => return Err(unreadable(projects_dir, &e)),
    };
    let mut entries = Vec::new();
    for project in projects {
        let project_path = project.map_err(|e| unreadable(projects_dir, &e))?.path();
        if !project_path.is_dir() {
            continue;
        }
        let files = match std::fs::read_dir(&project_path) {
            Ok(files) => files,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(unreadable(&project_path, &e)),
        };
        for file in files {
            let path = file.map_err(|e| unreadable(&project_path, &e))?.path();
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
    Ok(entries)
}

/// Whether a `projects_dir` that was not found is only not made yet: the folder it belongs
/// in is there, and nothing, not even a link, is in its place.
fn not_made_yet(projects_dir: &Path) -> bool {
    let in_a_folder = projects_dir.parent().is_some_and(Path::is_dir);
    let nothing_there = matches!(
        std::fs::symlink_metadata(projects_dir),
        Err(e) if e.kind() == ErrorKind::NotFound
    );
    in_a_folder && nothing_there
}

/// The error for a directory of the store that could not be read.
fn unreadable(dir: &Path, error: &std::io::Error) -> AgentError {
    AgentError::Io(format!("{}: {error}", dir.display()))
}

/// When the file at `path` last changed, in Unix seconds; `None` when the file is gone.
pub fn modified_unix(path: &Path) -> Result<Option<i64>, AgentError> {
    match std::fs::metadata(path).and_then(|meta| meta.modified()) {
        Ok(time) => Ok(Some(unix_seconds(time))),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AgentError::Io(e.to_string())),
    }
}

fn unix_seconds(time: std::time::SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    }
}

/// The folder and title of a session and how it was started, read from at most the first
/// and the last 262,144 bytes of its file, complete lines only. The title is the last one
/// the user gave, else the last generated one, else the first message. Never fails: a
/// file that cannot be read describes as nothing.
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
        title: scan.custom_title.or(scan.ai_title).or(scan.first_user_text),
        entrypoint: scan.entrypoint,
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
    custom_title: Option<String>,
    ai_title: Option<String>,
    first_user_text: Option<String>,
    entrypoint: Option<String>,
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
        let wants_title = contains(line, b"ai-title") || contains(line, b"custom-title");
        let wants_text = self.first_user_text.is_none() && contains(line, b"user");
        let wants_entrypoint = self.entrypoint.is_none() && contains(line, b"entrypoint");
        if !(wants_folder || wants_title || wants_text || wants_entrypoint) {
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
        if wants_entrypoint
            && let Some(entrypoint) = record.get("entrypoint").and_then(Value::as_str)
            && !entrypoint.is_empty()
        {
            self.entrypoint = Some(entrypoint.to_string());
        }
        let title = |key: &str| {
            record
                .get(key)
                .and_then(Value::as_str)
                .filter(|title| !title.trim().is_empty())
                .map(str::to_string)
        };
        match record.get("type").and_then(Value::as_str) {
            Some("ai-title") => {
                if let Some(title) = title("aiTitle") {
                    self.ai_title = Some(title);
                }
            }
            Some("custom-title") => {
                if let Some(title) = title("customTitle") {
                    self.custom_title = Some(title);
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

pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// One line as a JSON object. Invalid UTF-8 is replaced, not fatal.
pub(crate) fn parse_record(line: &[u8]) -> Option<Value> {
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

/// What [`describe`] said about each session file, with the modification time
/// and length the file had then.
#[derive(Debug, Default)]
pub struct DescriptionTable {
    known: HashMap<PathBuf, Described>,
}

#[derive(Debug)]
struct Described {
    modified_unix: i64,
    bytes: u64,
    meta: StoredMeta,
}

impl DescriptionTable {
    /// What `describe` says about the entry's file, asked again only when the file's
    /// modification time or length changed since the table last asked.
    fn meta(
        &mut self,
        entry: &StoredEntry,
        describe: &mut impl FnMut(&Path) -> StoredMeta,
    ) -> StoredMeta {
        if let Some(known) = self.known.get(&entry.path)
            && known.modified_unix == entry.modified_unix
            && known.bytes == entry.bytes
        {
            return known.meta.clone();
        }
        let meta = describe(&entry.path);
        self.known.insert(
            entry.path.clone(),
            Described {
                modified_unix: entry.modified_unix,
                bytes: entry.bytes,
                meta: meta.clone(),
            },
        );
        meta
    }

    /// Forgets the files not among `entries`.
    fn keep_only(&mut self, entries: &[StoredEntry]) {
        let listed: HashSet<&Path> = entries.iter().map(|entry| entry.path.as_path()).collect();
        self.known.retain(|path, _| listed.contains(path.as_path()));
    }
}

/// Every stored session at least as new as `modified_since_unix` (all of them
/// for `None`) that names a folder and that no program made (see
/// [`StoredMeta::made_by_program`]), newest first. A file the table already
/// describes, unchanged since, is not read again. A store that cannot be read is
/// an error (see [`list_entries`]).
pub fn stored_sessions(
    projects_dir: &Path,
    modified_since_unix: Option<i64>,
    table: &mut DescriptionTable,
) -> Result<Vec<StoredSession>, AgentError> {
    stored_sessions_with(projects_dir, modified_since_unix, table, describe)
}

fn stored_sessions_with(
    projects_dir: &Path,
    modified_since_unix: Option<i64>,
    table: &mut DescriptionTable,
    mut describe: impl FnMut(&Path) -> StoredMeta,
) -> Result<Vec<StoredSession>, AgentError> {
    let mut entries = list_entries(projects_dir)?;
    table.keep_only(&entries);
    if let Some(since) = modified_since_unix {
        entries.retain(|entry| entry.modified_unix >= since);
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.modified_unix));
    Ok(entries
        .into_iter()
        .filter_map(|entry| {
            let meta = table.meta(&entry, &mut describe);
            if meta.made_by_program() {
                return None;
            }
            Some(StoredSession {
                title: meta
                    .title
                    .unwrap_or_else(|| entry.id.0.chars().take(FALLBACK_TITLE_CHARS).collect()),
                folder: meta.folder?,
                last_active_unix: entry.modified_unix,
                id: entry.id,
            })
        })
        .collect())
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

/// One image of the stored message `message` of session `id`, read again: `index` counts the
/// message's images from 0. The whole file is read from its start, a line at a time, and only
/// a line that contains the id is parsed: the message is the first record whose `uuid` it is.
/// Of its images only the one asked for is decoded.
pub fn read_image(
    projects_dir: &Path,
    id: &SessionId,
    message: &str,
    index: usize,
) -> Result<ImageData, AgentError> {
    let said = |words: &str| AgentError::Io(words.into());
    let path =
        find_session(projects_dir, id).ok_or_else(|| said("the session is not in the store"))?;
    if message.is_empty() {
        return Err(said("the message is not in the session"));
    }
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(said("the session is not in the store"));
        }
        Err(e) => return Err(AgentError::Io(e.to_string())),
    };
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let record = loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|e| AgentError::Io(e.to_string()))?;
        if read == 0 {
            return Err(said("the message is not in the session"));
        }
        if contains(&line, message.as_bytes())
            && let Some(record) = parse_record(&line)
            && record.get("uuid").and_then(Value::as_str) == Some(message)
        {
            break record;
        }
    };
    drop(line);
    image_blocks(&record)
        .nth(index)
        .and_then(image_bytes)
        .ok_or_else(|| said("the message has no such image"))
}

/// What the records of a conversation give, read one after another by [`add_record`].
#[derive(Default)]
pub(crate) struct Walk {
    pub(crate) items: Vec<HistoryItem>,
    /// Where each tool call shown is in `items`, by the call's id.
    tool_positions: HashMap<String, usize>,
    /// The calls not shown (see [`tasks::hidden`]) of the turn, by id, until their result.
    hidden_calls: HashMap<String, Hidden>,
    tasks: TaskList,
    /// The last offer, while nothing was said after it.
    choices: Option<Choices>,
}

impl Walk {
    /// The items, and the task list and the offer as the conversation leaves them once its
    /// last turn has ended.
    pub(crate) fn end(mut self) -> (Vec<HistoryItem>, Vec<TaskStep>, Option<Choices>) {
        self.tasks.turn_ended();
        (self.items, self.tasks.steps().to_vec(), self.choices)
    }

    /// Does to the list or the offer what the result of a hidden call does.
    fn apply(&mut self, done: Option<SessionEvent>) {
        match done {
            Some(SessionEvent::TaskAdded(step)) => {
                self.tasks.add(step);
            }
            Some(SessionEvent::TaskChanged(change)) => {
                self.tasks.change(&change);
            }
            Some(SessionEvent::TaskRemoved { id }) => {
                self.tasks.remove(&id);
            }
            Some(SessionEvent::ChoicesOffered(offer)) => self.choices = Some(offer),
            // `tasks::finished` gives no other event.
            _ => {}
        }
    }

    /// A message of the user's starts a new turn: the list's turn ends, and the offer is
    /// forgotten, as at a turn's end live, with the offers not yet answered. The task calls
    /// not yet answered are kept: the program may have taken the message in mid-turn, and
    /// their results follow it.
    fn new_turn(&mut self) {
        self.tasks.turn_ended();
        self.choices = None;
        self.hidden_calls
            .retain(|_, hidden| !matches!(hidden, Hidden::Offer(_)));
    }
}

/// Adds what one record of a conversation shows to `walk`'s items. Side chains, meta records
/// and compaction summaries show nothing. A tool result fills in the outcome of its tool's
/// item. A user's message comes without its images' bytes, `None` in each image's place:
/// [`stored_images`] gives them. A call of a task tool or of the choice tool is no item: its
/// result changes the walk's task list or offer.
pub(crate) fn add_record(record: &Value, walk: &mut Walk) {
    let compact_summary = record
        .get("isCompactSummary")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if is_side_or_meta(record) || compact_summary {
        return;
    }
    match record.get("type").and_then(Value::as_str) {
        Some("user") => add_user(record, walk),
        Some("assistant") => add_assistant(record, walk),
        _ => {}
    }
}

/// The structured value of a record's tool result: its `toolUseResult`, or as the live stream
/// spells it `tool_use_result`. Null when it has none, or when the record holds several results
/// and so cannot say whose it is.
fn structured_result(record: &Value, results: usize) -> &Value {
    static NONE: Value = Value::Null;
    record
        .get("toolUseResult")
        .or_else(|| record.get("tool_use_result"))
        .filter(|_| results == 1)
        .unwrap_or(&NONE)
}

fn add_user(record: &Value, walk: &mut Walk) {
    let content = UserContent::of(record);
    let structured = structured_result(record, content.tool_results.len());
    for (tool_use_id, outcome) in content.tool_results {
        if let Some(hidden) = walk.hidden_calls.remove(&tool_use_id) {
            let (is_error, text) = match &outcome {
                ToolOutcome::Ok { output } => (false, output),
                ToolOutcome::Failed { output } => (true, output),
            };
            walk.apply(tasks::finished(hidden, is_error, structured, text));
        } else if let Some(&position) = walk.tool_positions.get(&tool_use_id)
            && let Some(HistoryItem::Tool { outcome: slot, .. }) = walk.items.get_mut(position)
        {
            *slot = Some(outcome);
        }
    }
    let empty = content.text.is_empty() && content.image_count == 0 && content.files.is_empty();
    if empty {
        return;
    }
    walk.new_turn();
    // A slash command answers an offer as a message does, but is no item.
    if is_command_wrapper(&content.text) {
        return;
    }
    walk.items.push(HistoryItem::User {
        text: content.text,
        image_count: content.image_count,
        files: content.files,
        images: vec![None; content.image_count],
        stored: record
            .get("uuid")
            .and_then(Value::as_str)
            .map(str::to_string),
    });
}

fn add_assistant(record: &Value, walk: &mut Walk) {
    let items = &mut walk.items;
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
                if let Some(hidden) = tasks::hidden(name, input) {
                    walk.hidden_calls.insert(id.to_string(), hidden);
                    continue;
                }
                walk.tool_positions.insert(id.to_string(), items.len());
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

/// The bytes of each image a `user` record holds, one entry per image in their order (the
/// blocks `UserContent::of` counts): `None`
/// for an image held any other way than as base64, or whose base64 does not decode. Slow for
/// large images: for the messages a history keeps only.
pub(crate) fn stored_images(record: &Value) -> Vec<Option<ImageData>> {
    image_blocks(record).map(image_bytes).collect()
}

/// The image blocks of a `user` record, in their order.
fn image_blocks(record: &Value) -> impl Iterator<Item = &Value> {
    record
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("image"))
}

/// The bytes of an image block of the form
/// `{"type":"image","source":{"type":"base64","media_type":"…","data":"…"}}`, decoded
/// straight into the buffer they are kept in.
fn image_bytes(block: &Value) -> Option<ImageData> {
    let source = block.get("source")?;
    if source.get("type").and_then(Value::as_str) != Some("base64") {
        return None;
    }
    let data = source.get("data").and_then(Value::as_str)?;
    // Every four characters are three bytes, and a last group of two or three is one or two.
    let length = data.trim_end_matches('=').len() * 3 / 4;
    if length == 0 {
        return None;
    }
    let mut bytes: Arc<[u8]> = std::iter::repeat_n(0, length).collect();
    let written = STANDARD
        .decode_slice(data, Arc::get_mut(&mut bytes)?)
        .ok()?;
    (written == length).then(|| ImageData::new(bytes))
}

/// What a `user` record carries: the typed text (text blocks joined by a
/// newline, without the block that names its files), the files, how many images,
/// and the tool results it answers.
struct UserContent {
    text: String,
    files: Vec<PathBuf>,
    image_count: usize,
    tool_results: Vec<(String, ToolOutcome)>,
}

impl UserContent {
    fn of(record: &Value) -> Self {
        let mut content = UserContent {
            text: String::new(),
            files: Vec::new(),
            image_count: 0,
            tool_results: Vec::new(),
        };
        let mut text = String::new();
        match record.get("message").and_then(|m| m.get("content")) {
            Some(Value::String(whole)) => text = whole.clone(),
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
                text = texts.join("\n");
            }
            _ => {}
        }
        let (typed, files) = split_attached(&text);
        content.text = typed.to_string();
        content.files = files;
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
    use std::collections::HashMap;
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
    const TITLE: &str = r#"{"type":"ai-title","aiTitle":"Fix flaky reconnect","sessionId":"s1"}"#;

    #[test]
    fn listing_sees_only_session_files_one_level_down() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "-home-u-proj", "s1", &[USER]);
        std::fs::write(dir.path().join("-home-u-proj/notes.txt"), "x").unwrap();
        std::fs::create_dir_all(dir.path().join("-home-u-proj/s1/subagents")).unwrap();
        std::fs::write(dir.path().join("-home-u-proj/s1/subagents/a.jsonl"), USER).unwrap();
        let entries = list_entries(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, SessionId("s1".into()));
        assert!(
            list_entries(&dir.path().join("missing"))
                .unwrap()
                .is_empty()
        );
    }

    /// Takes every permission away from `dir` while `check` runs, then gives them back. Skips
    /// `check` when `dir` can still be read without them, as it can for root.
    #[cfg(unix)]
    fn unreadable(dir: &Path, check: impl FnOnce()) {
        use std::os::unix::fs::PermissionsExt;
        let before = std::fs::metadata(dir).unwrap().permissions();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = std::fs::read_dir(dir).is_ok();
        if readable {
            eprintln!("skipped: {} can be read without permissions", dir.display());
        } else {
            check();
        }
        std::fs::set_permissions(dir, before).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn a_store_that_cannot_be_read_is_an_error_not_an_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects");
        write(&projects, "-home-u-proj", "s1", &[USER, TITLE]);
        write(&projects, "-home-u-other", "s2", &[USER]);
        let mut table = DescriptionTable::default();
        assert_eq!(
            stored_sessions(&projects, None, &mut table).unwrap().len(),
            2
        );
        unreadable(&projects.join("-home-u-other"), || {
            assert!(matches!(list_entries(&projects), Err(AgentError::Io(_))));
            let listed = stored_sessions(&projects, None, &mut table);
            assert!(matches!(listed, Err(AgentError::Io(_))), "{listed:?}");
        });
        unreadable(&projects, || {
            assert!(matches!(list_entries(&projects), Err(AgentError::Io(_))));
        });
        assert_eq!(
            stored_sessions(&dir.path().join("missing"), None, &mut table).unwrap(),
            vec![],
            "a store that does not exist yet has no sessions"
        );
        assert_eq!(
            stored_sessions(&projects, None, &mut table).unwrap().len(),
            2
        );
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
        let mut table = DescriptionTable::default();
        let all = stored_sessions(dir.path(), None, &mut table).unwrap();
        assert_eq!(all.len(), 1, "a session with no folder is left out");
        assert_eq!(all[0].title, "Fix flaky reconnect");
        assert_eq!(all[0].folder, PathBuf::from("/home/u/proj"));
        assert!(
            stored_sessions(dir.path(), Some(all[0].last_active_unix + 10), &mut table)
                .unwrap()
                .is_empty()
        );
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
            describe(&path),
            StoredMeta {
                folder: None,
                title: Some("caf\u{e9}".into()),
                entrypoint: None,
            }
        );
    }

    #[test]
    fn a_custom_title_wins_over_a_generated_one() {
        let dir = tempfile::tempdir().unwrap();
        let custom =
            r#"{"type":"custom-title","customTitle":"Alex's reconnect work","sessionId":"s1"}"#;
        let older_custom =
            r#"{"type":"custom-title","customTitle":"An older name","sessionId":"s1"}"#;
        let custom_first = write(
            dir.path(),
            "-home-u-proj",
            "s1",
            &[USER, older_custom, custom, ASSISTANT, TITLE],
        );
        let generated_first = write(dir.path(), "-home-u-proj", "s2", &[USER, TITLE, custom]);
        for path in [custom_first, generated_first] {
            assert_eq!(
                describe(&path).title.as_deref(),
                Some("Alex's reconnect work"),
                "{path:?}"
            );
        }
    }

    #[test]
    fn an_unchanged_file_is_not_described_again() {
        let dir = tempfile::tempdir().unwrap();
        let first = write(dir.path(), "-home-u-proj", "s1", &[USER, TITLE]);
        let second = write(dir.path(), "-home-u-proj", "s2", &[USER]);
        let mut table = DescriptionTable::default();
        let mut calls: HashMap<PathBuf, usize> = HashMap::new();
        let list = |table: &mut DescriptionTable, calls: &mut HashMap<PathBuf, usize>| {
            stored_sessions_with(dir.path(), None, table, |path| {
                *calls.entry(path.to_path_buf()).or_default() += 1;
                describe(path)
            })
            .unwrap()
        };

        let once = list(&mut table, &mut calls);
        let twice = list(&mut table, &mut calls);
        assert_eq!(once.len(), 2);
        assert_eq!(once, twice);
        assert_eq!(calls[&first], 1);
        assert_eq!(calls[&second], 1);

        let mut grown = std::fs::read_to_string(&first).unwrap();
        grown.push_str("\n{\"type\":\"ai-title\",\"aiTitle\":\"A new name\"}");
        std::fs::write(&first, grown).unwrap();
        let after = list(&mut table, &mut calls);
        assert_eq!(calls[&first], 2);
        assert_eq!(calls[&second], 1);
        assert!(after.iter().any(|session| session.title == "A new name"));
    }

    /// A user's first message as the store holds it, started the way `entrypoint` names.
    fn started_by(entrypoint: &str) -> String {
        format!(
            r#"{{"type":"user","cwd":"/home/u/proj","entrypoint":"{entrypoint}","message":{{"role":"user","content":"Condense this"}}}}"#
        )
    }

    #[test]
    fn a_session_a_program_made_through_the_sdk_is_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        for (id, entrypoint) in [
            ("ts", "sdk-ts"),
            ("py", "sdk-py"),
            ("own", "sdk-cli"),
            ("terminal", "cli"),
            ("desktop", "claude-desktop"),
            ("unknown", "something-new"),
        ] {
            write(dir.path(), "-home-u-proj", id, &[&started_by(entrypoint)]);
        }
        // Records from before the store named an entrypoint.
        write(dir.path(), "-home-u-proj", "unnamed", &[USER]);
        let mut table = DescriptionTable::default();
        let mut listed: Vec<String> = stored_sessions(dir.path(), None, &mut table)
            .unwrap()
            .into_iter()
            .map(|session| session.id.0)
            .collect();
        listed.sort();
        assert_eq!(listed, ["desktop", "own", "terminal", "unknown", "unnamed"]);
        // Left out of the list only: it is still found by its id.
        assert!(find_session(dir.path(), &SessionId("ts".into())).is_some());
    }

    #[test]
    fn how_a_session_was_started_is_what_its_first_record_says() {
        let dir = tempfile::tempdir().unwrap();
        // The first records of a real file name no entrypoint at all.
        let queued = r#"{"type":"queue-operation","operation":"enqueue"}"#;
        let by_program = write(
            dir.path(),
            "-home-u-proj",
            "program",
            &[queued, &started_by("sdk-ts"), &started_by("cli")],
        );
        let by_person = write(
            dir.path(),
            "-home-u-proj",
            "person",
            &[queued, &started_by("cli"), &started_by("sdk-ts")],
        );
        assert_eq!(describe(&by_program).entrypoint.as_deref(), Some("sdk-ts"));
        assert_eq!(describe(&by_person).entrypoint.as_deref(), Some("cli"));
        assert_eq!(
            describe(&write(dir.path(), "-p", "s", &[USER])).entrypoint,
            None
        );
        let mut table = DescriptionTable::default();
        let listed = stored_sessions(dir.path(), None, &mut table).unwrap();
        assert!(listed.iter().any(|session| session.id.0 == "person"));
        assert!(!listed.iter().any(|session| session.id.0 == "program"));
    }

    #[test]
    fn how_a_large_session_was_started_is_read_from_its_start() {
        let dir = tempfile::tempdir().unwrap();
        // A record that says nothing else the listing looks for.
        let attached = r#"{"type":"attachment","entrypoint":"sdk-ts"}"#;
        let filler = format!(
            r#"{{"type":"api-request-blob","message":"{}"}}"#,
            "y".repeat(4000)
        );
        let mut lines = vec![attached.to_string(), USER.to_string()];
        lines.extend(std::iter::repeat_n(filler, 300)); // about 1.2 MB
        lines.push(started_by("cli"));
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let path = write(dir.path(), "-home-u-proj", "large", &refs);
        assert_eq!(describe(&path).entrypoint.as_deref(), Some("sdk-ts"));
        let mut table = DescriptionTable::default();
        assert_eq!(
            stored_sessions(dir.path(), None, &mut table).unwrap(),
            vec![]
        );
    }

    #[test]
    fn a_session_left_out_is_not_described_again() {
        let dir = tempfile::tempdir().unwrap();
        let left_out = write(dir.path(), "-home-u-proj", "ts", &[&started_by("sdk-ts")]);
        let mut table = DescriptionTable::default();
        let mut calls = 0;
        for _ in 0..2 {
            let listed = stored_sessions_with(dir.path(), None, &mut table, |path| {
                assert_eq!(path, left_out);
                calls += 1;
                describe(path)
            })
            .unwrap();
            assert_eq!(listed, vec![]);
        }
        assert_eq!(calls, 1);
    }

    #[test]
    fn a_file_that_is_gone_has_no_modification_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "-p", "s1", &[USER]);
        assert!(matches!(modified_unix(&path), Ok(Some(unix)) if unix > 0));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(modified_unix(&path), Ok(None));
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

    /// The items `add_record` makes of one record.
    fn read_one(record: &str) -> Vec<HistoryItem> {
        let mut walk = Walk::default();
        add_record(&serde_json::from_str(record).unwrap(), &mut walk);
        walk.items
    }

    #[test]
    fn a_stored_message_names_its_files() {
        // Written by hand, field names and shapes only: never seen live.
        let stored = r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":[{"type":"text","text":"read these\n\nAttached files:\n- /home/alex/proj/notes.txt\n- /home/alex/proj/plan.md"},{"type":"image","source":{}}]}}"#;
        assert_eq!(
            read_one(stored),
            vec![HistoryItem::User {
                text: "read these".into(),
                image_count: 1,
                files: vec![
                    PathBuf::from("/home/alex/proj/notes.txt"),
                    PathBuf::from("/home/alex/proj/plan.md"),
                ],
                images: vec![None],
                stored: Some("u1".into()),
            }]
        );
    }

    #[test]
    fn a_stored_message_of_files_alone_is_kept() {
        // Written by hand, field names and shapes only: never seen live.
        let stored = r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":[{"type":"text","text":"Attached files:\n- /home/alex/proj/notes.txt"}]}}"#;
        assert_eq!(
            read_one(stored),
            vec![HistoryItem::User {
                text: String::new(),
                image_count: 0,
                files: vec![PathBuf::from("/home/alex/proj/notes.txt")],
                images: Vec::new(),
                stored: Some("u1".into()),
            }]
        );
    }

    #[test]
    fn the_listing_does_not_take_its_title_from_the_files_block() {
        let dir = tempfile::tempdir().unwrap();
        // Written by hand, field names and shapes only: never seen live.
        let files_alone = r#"{"type":"user","cwd":"/home/alex/proj","message":{"role":"user","content":[{"type":"text","text":"Attached files:\n- /home/alex/proj/notes.txt"}]}}"#;
        let with_text = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Fix the parser\n\nAttached files:\n- /home/alex/proj/log.txt"}]}}"#;
        let path = write(
            dir.path(),
            "-home-alex-proj",
            "s1",
            &[files_alone, with_text],
        );
        assert_eq!(describe(&path).title.as_deref(), Some("Fix the parser"));
    }

    /// The bytes of a picture of `count` pixels of one colour, made here: RGBA, as a picture
    /// is drawn. The reader does not decode images.
    fn pixels(colour: [u8; 4], count: usize) -> Vec<u8> {
        colour.repeat(count)
    }

    /// A message of the user's with id `uuid`, its text and an image block for each of
    /// `images`. Written by hand, field names and shapes only: never seen live.
    fn message_with(uuid: &str, images: &[&[u8]]) -> String {
        let mut content = vec![serde_json::json!({"type": "text", "text": "look"})];
        content.extend(images.iter().map(|bytes| {
            serde_json::json!({"type": "image", "source": {
                "type": "base64", "media_type": "image/png", "data": STANDARD.encode(bytes),
            }})
        }));
        serde_json::json!({
            "type": "user", "uuid": uuid, "parentUuid": null,
            "message": {"role": "user", "content": content},
        })
        .to_string()
    }

    /// A reply to `parent`, with id `uuid`. Written by hand, field names and shapes only:
    /// never seen live.
    fn reply_to(uuid: &str, parent: &str) -> String {
        serde_json::json!({
            "type": "assistant", "uuid": uuid, "parentUuid": parent,
            "message": {"role": "assistant", "content": [{"type": "text", "text": "Seen."}]},
        })
        .to_string()
    }

    fn image_of(bytes: &[u8]) -> ImageData {
        ImageData::new(bytes.to_vec())
    }

    #[test]
    fn a_stored_message_has_its_id() {
        let record = message_with("m1", &[]);
        let items = read_one(&record);
        let [HistoryItem::User { stored, .. }] = items.as_slice() else {
            panic!("not one message of the user's");
        };
        assert_eq!(stored.as_deref(), Some("m1"));
        // The reader of a history fills it in too.
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "-home-alex-proj", "s1", &[&record]);
        let history = crate::history_tail::read_tail(&path, 10, 1 << 20).unwrap();
        let [HistoryItem::User { stored, .. }] = history.items.as_slice() else {
            panic!("not one message of the user's");
        };
        assert_eq!(stored.as_deref(), Some("m1"));
    }

    #[test]
    fn an_image_is_read_again_by_its_message() {
        let dir = tempfile::tempdir().unwrap();
        let red = pixels([200, 30, 30, 255], 4);
        let blue = pixels([30, 30, 200, 255], 6);
        let other = pixels([30, 200, 30, 255], 4);
        write(
            dir.path(),
            "-home-alex-proj",
            "s1",
            &[
                &message_with("m0", &[&other]),
                &reply_to("a0", "m0"),
                &message_with("m1", &[&red, &blue]),
                &reply_to("a1", "m1"),
            ],
        );
        let id = SessionId("s1".into());
        assert_eq!(read_image(dir.path(), &id, "m1", 1), Ok(image_of(&blue)));
        assert_eq!(read_image(dir.path(), &id, "m1", 0), Ok(image_of(&red)));
    }

    #[test]
    fn a_message_at_the_start_of_a_long_file_is_still_found() {
        let dir = tempfile::tempdir().unwrap();
        let red = pixels([200, 30, 30, 255], 4);
        let first = message_with("m1", &[&red]);
        // More than the end a history is read from, in lines of 64 KiB.
        let filler = reply_to("a1", &"x".repeat(65_536));
        let count = (crate::history_tail::TAIL_BYTES as usize) / filler.len() + 2;
        let mut lines = vec![first.as_str()];
        lines.extend(std::iter::repeat_n(filler.as_str(), count));
        write(dir.path(), "-home-alex-proj", "s1", &lines);
        let id = SessionId("s1".into());
        assert_eq!(read_image(dir.path(), &id, "m1", 0), Ok(image_of(&red)));
    }

    #[test]
    fn a_message_that_is_gone_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let red = pixels([200, 30, 30, 255], 4);
        write(
            dir.path(),
            "-home-alex-proj",
            "s1",
            &[&message_with("m1", &[&red])],
        );
        assert_eq!(
            read_image(dir.path(), &SessionId("s1".into()), "m2", 0),
            Err(AgentError::Io("the message is not in the session".into()))
        );
        assert_eq!(
            read_image(dir.path(), &SessionId("s2".into()), "m1", 0),
            Err(AgentError::Io("the session is not in the store".into()))
        );
    }

    #[test]
    fn an_image_that_cannot_be_decoded_keeps_its_place() {
        // Written by hand: an image held by its address, as the program can store one, before
        // one held as base64. Never seen live.
        let dir = tempfile::tempdir().unwrap();
        let red = pixels([200, 30, 30, 255], 4);
        let message = serde_json::json!({
            "type": "user", "uuid": "m1", "parentUuid": null,
            "message": {"role": "user", "content": [
                {"type": "image", "source": {"type": "url", "url": "https://example.org/a.png"}},
                {"type": "image", "source": {
                    "type": "base64", "media_type": "image/png", "data": STANDARD.encode(&red),
                }},
            ]},
        })
        .to_string();
        write(dir.path(), "-home-alex-proj", "s1", &[&message]);
        let id = SessionId("s1".into());
        assert_eq!(read_image(dir.path(), &id, "m1", 1), Ok(image_of(&red)));
        assert_eq!(
            read_image(dir.path(), &id, "m1", 0),
            Err(AgentError::Io("the message has no such image".into()))
        );
    }

    #[test]
    fn an_index_past_the_messages_images_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let red = pixels([200, 30, 30, 255], 4);
        let blue = pixels([30, 30, 200, 255], 4);
        write(
            dir.path(),
            "-home-alex-proj",
            "s1",
            &[&message_with("m1", &[&red, &blue])],
        );
        assert_eq!(
            read_image(dir.path(), &SessionId("s1".into()), "m1", 2),
            Err(AgentError::Io("the message has no such image".into()))
        );
    }

    #[test]
    fn a_line_that_only_mentions_the_id_is_not_the_message() {
        let dir = tempfile::tempdir().unwrap();
        let id = SessionId("s1".into());
        write(
            dir.path(),
            "-home-alex-proj",
            "s1",
            &[&reply_to("a1", "m1")],
        );
        assert_eq!(
            read_image(dir.path(), &id, "m1", 0),
            Err(AgentError::Io("the message is not in the session".into()))
        );
        // The line that names it comes first; the message is still the one taken.
        let red = pixels([200, 30, 30, 255], 4);
        write(
            dir.path(),
            "-home-alex-proj",
            "s1",
            &[&reply_to("a1", "m1"), &message_with("m1", &[&red])],
        );
        assert_eq!(read_image(dir.path(), &id, "m1", 0), Ok(image_of(&red)));
    }
}
