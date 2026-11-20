//! The `raw` subcommand: one real session, typed commands in, every protocol
//! line out. The command grammar lives here too so it can be tested alone.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use mr_agent::{Attachment, ImageData, SessionId, StartSpec};
use mr_claude::{
    ClaudeProcess, ExitInfo, PermissionReply, SpawnConfig, build_args, child_env, control_request,
    initialize, interrupt, mode_from_wire, permission_reply, remote_control, rename_session,
    set_model, set_permission_mode, user_message,
};
use serde_json::{Value, json};

use crate::choices;

/// How long the probe waits for a child to end after its input was closed.
const EXIT_WAIT: Duration = Duration::from_secs(5);

pub const HELP: &str = "\
say TEXT          send a user message
image PATH TEXT   send TEXT with the image at PATH attached
{...}             send a line starting with { unchanged
stop              interrupt the current turn
model NAME        set the model
mode NAME         set the permission mode (default, acceptEdits, plan, bypassPermissions)
rc on | rc off    turn Remote Control on or off
rename TITLE      rename the session
allow ID          allow permission request ID
always ID         allow it and grant the first suggested permission
deny ID           deny it
quit              close input and exit
help              this list";

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Say(String),
    Image { path: PathBuf, text: String },
    Raw(String),
    Stop,
    Model(String),
    Mode(String),
    Remote(bool),
    Rename(String),
    Allow(String),
    AllowAlways(String),
    Deny(String),
    Quit,
    Help,
}

/// Reads one line typed at the probe. The error is a message for the person typing.
pub fn parse_command(line: &str) -> Result<Cmd, String> {
    let line = line.trim();
    if line.starts_with('{') {
        return Ok(Cmd::Raw(line.to_string()));
    }
    let (word, rest) = match line.split_once(char::is_whitespace) {
        Some((word, rest)) => (word, rest.trim()),
        None => (line, ""),
    };
    let needs = |what: &str| -> Result<String, String> {
        if rest.is_empty() {
            Err(format!("`{word}` needs {what}"))
        } else {
            Ok(rest.to_string())
        }
    };
    match word {
        "say" => needs("some text").map(Cmd::Say),
        "image" => {
            let (path, text) = match rest.split_once(char::is_whitespace) {
                Some((path, text)) => (path, text.trim()),
                None => (rest, ""),
            };
            if path.is_empty() {
                return Err("`image` needs a file path".into());
            }
            Ok(Cmd::Image {
                path: path.into(),
                text: text.into(),
            })
        }
        "stop" => Ok(Cmd::Stop),
        "model" => needs("a model name").map(Cmd::Model),
        "mode" => needs("a mode name").map(Cmd::Mode),
        "rc" => match rest {
            "on" => Ok(Cmd::Remote(true)),
            "off" => Ok(Cmd::Remote(false)),
            _ => Err("`rc` takes `on` or `off`".into()),
        },
        "rename" => needs("a title").map(Cmd::Rename),
        "allow" => needs("a request id").map(Cmd::Allow),
        "always" => needs("a request id").map(Cmd::AllowAlways),
        "deny" => needs("a request id").map(Cmd::Deny),
        "quit" => Ok(Cmd::Quit),
        "help" => Ok(Cmd::Help),
        "" => Err("empty line".into()),
        other => Err(format!("unknown command `{other}`; type `help`")),
    }
}

/// What `raw` was asked to do. `program` is already an absolute path or a bare
/// command name.
pub struct RawOptions {
    pub cwd: PathBuf,
    pub program: PathBuf,
    pub resume: Option<String>,
    pub fork: bool,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub record: Option<PathBuf>,
    pub full_login: bool,
    /// Passed to the program after the probe's own arguments, in this order.
    pub extra_args: Vec<String>,
    /// Serve the choice tool to the program and answer its requests for it (`choices`).
    pub choices: bool,
}

/// What the probe needs to answer a `can_use_tool` request later.
struct PendingPermission {
    input: Value,
    /// The first permission suggestion alone, in a one-element list: what
    /// `always` sends back, as the adapter does.
    suggestions: Option<Value>,
}

type Pending = Arc<Mutex<HashMap<String, PendingPermission>>>;

enum Event {
    Typed(String),
    InputEnded,
    ChildExited(ExitInfo),
    /// The program's message for an in-process server: the request's id and its `request`
    /// object.
    ForServer(String, Value),
}

/// The record file, flushed after every line because another process may be
/// following it while the probe runs.
struct Recorder {
    file: Mutex<Option<File>>,
}

impl Recorder {
    fn open(path: Option<&PathBuf>) -> Result<Recorder, String> {
        let file = match path {
            None => None,
            Some(path) => {
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
                }
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
                Some(file)
            }
        };
        Ok(Recorder {
            file: Mutex::new(file),
        })
    }

    fn line(&self, prefix: &str, line: &str) {
        if let Some(file) = lock(&self.file).as_mut() {
            let written = writeln!(file, "{prefix}{line}").and_then(|()| file.flush());
            if let Err(e) = written {
                eprintln!("probe: cannot write the record: {e}");
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs one session until the child exits, `quit` is typed or stdin ends.
pub fn run_raw(options: RawOptions) -> Result<(), String> {
    let recorder = Arc::new(Recorder::open(options.record.as_ref())?);
    let pending: Pending = Arc::default();

    let mut spec = StartSpec::new(&options.cwd);
    spec.resume = options.resume.map(SessionId);
    spec.fork = options.fork;
    spec.model = options.model;
    spec.permission_mode = options.mode.as_deref().map(mode_from_wire);
    let fresh_id = SessionId(uuid::Uuid::new_v4().to_string());
    let config = SpawnConfig {
        program: options.program,
        args: program_args(&spec, &fresh_id, &options.extra_args, options.choices),
        cwd: options.cwd,
        env: child_env(std::env::vars().collect(), options.full_login),
    };

    let (events, inbox) = mpsc::channel();
    let process = {
        let recorder = Arc::clone(&recorder);
        let pending = Arc::clone(&pending);
        let events = events.clone();
        let for_server = events.clone();
        ClaudeProcess::spawn(
            config,
            move |line| {
                recorder.line("< ", &line);
                remember_permission_request(&line, &pending);
                // A closed stdout must not stop the session.
                let _ = writeln!(io::stdout().lock(), "< {}", echoed(&line));
                if let Some((id, body)) = choices::request(&line) {
                    let _ = for_server.send(Event::ForServer(id, body));
                }
            },
            move |info| {
                let _ = events.send(Event::ChildExited(info));
            },
        )
        .map_err(|e| format!("cannot start the program: {e}"))?
    };

    thread::spawn(move || read_typed_lines(events));

    let mut session = Session {
        process,
        recorder,
        pending,
        next_request: 1,
    };
    session.send_control(initialize())?;

    loop {
        match inbox.recv() {
            Ok(Event::Typed(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                match parse_command(&line) {
                    Ok(Cmd::Quit) => break,
                    Ok(command) => {
                        if let Err(message) = session.run(command) {
                            eprintln!("probe: {message}");
                        }
                    }
                    Err(message) => eprintln!("probe: {message}"),
                }
            }
            Ok(Event::InputEnded) => break,
            Ok(Event::ForServer(id, body)) => {
                // Answered also without `--choices`: the program waits for some answer.
                if let Err(message) = session.send_line(&mr_claude::choices::reply(&id, &body)) {
                    eprintln!("probe: {message}");
                }
            }
            Ok(Event::ChildExited(info)) => {
                report_exit(&info);
                return Ok(());
            }
            Err(_) => return Ok(()),
        }
    }

    // Quit or end of input: close the child's input and wait for it to finish.
    session.process.close_input();
    let mut killed = false;
    loop {
        match inbox.recv_timeout(EXIT_WAIT) {
            Ok(Event::ChildExited(info)) => {
                report_exit(&info);
                return Ok(());
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) if !killed => {
                eprintln!("probe: the child did not stop; killing it");
                session.process.kill();
                killed = true;
            }
            Err(_) => return Ok(()),
        }
    }
}

/// The keys whose values say where a session can be reached by Remote Control.
const ADDRESS_KEYS: [&str; 3] = ["session_url", "connect_url", "bridge_session_id"];

/// `line` as the terminal shows it: a JSON line with the value of every key in
/// [`ADDRESS_KEYS`], at any depth, replaced by `<hidden>`. A line with none of them, or one
/// that is not JSON, is shown as it is. The recording keeps the raw line.
fn echoed(line: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(line) else {
        return line.to_string();
    };
    if hide_addresses(&mut value) {
        value.to_string()
    } else {
        line.to_string()
    }
}

/// Replaces the value of every key in [`ADDRESS_KEYS`] in `value` by `<hidden>`. True when
/// one was found.
fn hide_addresses(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            let mut hid = false;
            for (key, inner) in map.iter_mut() {
                if ADDRESS_KEYS.contains(&key.as_str()) {
                    *inner = Value::String("<hidden>".into());
                    hid = true;
                } else {
                    hid |= hide_addresses(inner);
                }
            }
            hid
        }
        Value::Array(items) => {
            // Every item, not only up to the first with an address.
            let mut hid = false;
            for item in items {
                hid |= hide_addresses(item);
            }
            hid
        }
        _ => false,
    }
}

/// The program's arguments: the probe's own for `spec`, with `choices` those of the choice
/// tool, then `extra` as given.
fn program_args(
    spec: &StartSpec,
    fresh_id: &SessionId,
    extra: &[String],
    choices: bool,
) -> Vec<String> {
    let mut args = build_args(spec, Some(fresh_id));
    if choices {
        args.extend(mr_claude::choices::args());
    }
    args.extend_from_slice(extra);
    args
}

struct Session {
    process: ClaudeProcess,
    recorder: Arc<Recorder>,
    pending: Pending,
    next_request: u64,
}

impl Session {
    fn send_line(&self, line: &str) -> Result<(), String> {
        self.process
            .write_line(line)
            .map_err(|e| format!("cannot write to the child: {e}"))?;
        self.recorder.line("> ", line);
        Ok(())
    }

    /// Sends a control request numbered `probe_1`, `probe_2`, ….
    fn send_control(&mut self, body: Value) -> Result<(), String> {
        let id = format!("probe_{}", self.next_request);
        self.next_request += 1;
        self.send_line(&control_request(&id, body))
    }

    fn answer(&mut self, id: &str, always: Option<bool>) -> Result<(), String> {
        let pending = lock(&self.pending)
            .remove(id)
            .ok_or_else(|| format!("no pending permission request `{id}`"))?;
        let reply = match always {
            None => PermissionReply::Deny {
                message: "Denied from the probe".into(),
            },
            Some(always) => PermissionReply::Allow {
                updated_input: pending.input,
                updated_permissions: if always { pending.suggestions } else { None },
            },
        };
        self.send_line(&permission_reply(id, &reply))
    }

    fn run(&mut self, command: Cmd) -> Result<(), String> {
        match command {
            Cmd::Say(text) => self.send_line(&user_message(&text, &[])),
            Cmd::Image { path, text } => {
                let media_type = image_media_type(&path)?;
                let data = std::fs::read(&path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                let image = Attachment::Image {
                    media_type,
                    data: ImageData::new(data),
                };
                self.send_line(&user_message(&text, &[image]))
            }
            Cmd::Raw(line) => self.send_line(&line),
            Cmd::Stop => self.send_control(interrupt()),
            Cmd::Model(name) => self.send_control(set_model(&name)),
            Cmd::Mode(name) => self.send_control(set_permission_mode(&mode_from_wire(&name))),
            Cmd::Remote(on) => self.send_control(remote_control(on, None)),
            Cmd::Rename(title) => self.send_control(rename_session(&title)),
            Cmd::Allow(id) => self.answer(&id, Some(false)),
            Cmd::AllowAlways(id) => self.answer(&id, Some(true)),
            Cmd::Deny(id) => self.answer(&id, None),
            Cmd::Help => {
                let _ = writeln!(io::stdout().lock(), "{HELP}");
                Ok(())
            }
            Cmd::Quit => Ok(()),
        }
    }
}

fn image_media_type(path: &std::path::Path) -> Result<String, String> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png") => Ok("image/png".into()),
        Some("jpg" | "jpeg") => Ok("image/jpeg".into()),
        Some("gif") => Ok("image/gif".into()),
        Some("webp") => Ok("image/webp".into()),
        _ => Err(format!(
            "cannot tell the image type of {} (use png, jpg, gif or webp)",
            path.display()
        )),
    }
}

/// Keeps the request's own `input` and its first permission suggestion for a later reply.
fn remember_permission_request(line: &str, pending: &Pending) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let request = &value["request"];
    if value["type"] != "control_request" || request["subtype"] != "can_use_tool" {
        return;
    }
    let Some(id) = value["request_id"].as_str() else {
        return;
    };
    let input = match &request["input"] {
        Value::Null => json!({}),
        input => input.clone(),
    };
    lock(pending).insert(
        id.to_string(),
        PendingPermission {
            input,
            suggestions: request
                .get("permission_suggestions")
                .and_then(|list| list.get(0))
                .map(|first| json!([first])),
        },
    );
}

fn read_typed_lines(events: mpsc::Sender<Event>) {
    let mut stdin = io::stdin().lock();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match stdin.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf).into_owned();
                if events.send(Event::Typed(line)).is_err() {
                    return;
                }
            }
        }
    }
    let _ = events.send(Event::InputEnded);
}

/// One line, on stderr so stdout carries only protocol lines.
fn report_exit(info: &ExitInfo) {
    let how = match info.code {
        Some(code) => format!("exit code {code}"),
        None => "stopped by a signal".to_string(),
    };
    let tail = info.stderr_tail.trim();
    if tail.is_empty() {
        eprintln!("probe: the child exited ({how})");
    } else {
        let tail = tail.lines().collect::<Vec<_>>().join(" | ");
        eprintln!("probe: the child exited ({how}); stderr: {tail}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_echo_hides_a_sessions_address_wherever_it_sits() {
        // Made up, in the shape of the reply to turning Remote Control on.
        let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"rc_1","response":{"session_url":"https://example.org/session","connect_url":"https://example.org/session?env=e1","environment_id":"e1","bridge_session_id":"b1"}}}"#;
        let shown = echoed(reply);
        assert!(!shown.contains("example.org"), "{shown}");
        assert!(!shown.contains("\"b1\""), "{shown}");
        let value: Value = serde_json::from_str(&shown).unwrap();
        let inner = &value["response"]["response"];
        for key in ["session_url", "connect_url", "bridge_session_id"] {
            assert_eq!(inner[key], "<hidden>", "{key}");
        }
        assert_eq!(inner["environment_id"], "e1", "the rest is kept");
        assert_eq!(value["response"]["request_id"], "rc_1");
        // Nested in an array, and at the top.
        let line = r#"{"session_url":"https://example.org/session","list":[{"connect_url":"https://example.org/session"},{"session_url":"https://example.org/session"}]}"#;
        assert!(!echoed(line).contains("example.org"));
        // A line with nothing to hide, or that is not JSON, is echoed as it is.
        let plain = r#"{"type":"system", "subtype":"init"}"#;
        assert_eq!(echoed(plain), plain);
        let text = "not json: https://example.org/session";
        assert_eq!(echoed(text), text);
    }

    #[test]
    fn commands_parse() {
        assert_eq!(
            parse_command("say hello there"),
            Ok(Cmd::Say("hello there".into()))
        );
        assert_eq!(
            parse_command("image /tmp/a.png what is this"),
            Ok(Cmd::Image {
                path: "/tmp/a.png".into(),
                text: "what is this".into()
            })
        );
        assert_eq!(
            parse_command("{\"type\":\"x\"}"),
            Ok(Cmd::Raw("{\"type\":\"x\"}".into()))
        );
        assert_eq!(parse_command("rc on"), Ok(Cmd::Remote(true)));
        assert_eq!(parse_command("rc off"), Ok(Cmd::Remote(false)));
        assert_eq!(
            parse_command("always req_1"),
            Ok(Cmd::AllowAlways("req_1".into()))
        );
        assert_eq!(parse_command("deny req_1"), Ok(Cmd::Deny("req_1".into())));
        assert!(parse_command("dance").is_err());
        assert!(parse_command("rc maybe").is_err());
    }

    #[test]
    fn the_extra_arguments_come_after_the_probes_own_in_their_order() {
        let mut spec = StartSpec::new("/p");
        spec.model = Some("opus".into());
        let id = SessionId("11111111-1111-4111-8111-111111111111".into());
        let extra = vec!["--replay-user-messages".to_string(), "--debug".to_string()];
        let args = program_args(&spec, &id, &extra, false);
        let own = build_args(&spec, Some(&id));
        assert_eq!(args[..own.len()], own[..]);
        assert_eq!(args[own.len()..], extra[..]);
        assert_eq!(program_args(&spec, &id, &[], false), own);
        // The choice tool's come between, so an extra one can still override them.
        let with = program_args(&spec, &id, &extra, true);
        let tool = mr_claude::choices::args();
        assert_eq!(with[..own.len()], own[..]);
        assert_eq!(with[own.len()..own.len() + tool.len()], tool[..]);
        assert_eq!(with[own.len() + tool.len()..], extra[..]);
    }

    #[test]
    fn arguments_are_required_where_a_command_needs_them() {
        assert!(parse_command("say").is_err());
        assert!(parse_command("image").is_err());
        assert!(parse_command("allow").is_err());
        assert!(parse_command("").is_err());
        assert_eq!(
            parse_command("image /tmp/a.png"),
            Ok(Cmd::Image {
                path: "/tmp/a.png".into(),
                text: String::new()
            })
        );
        assert_eq!(parse_command("quit"), Ok(Cmd::Quit));
        assert_eq!(parse_command("stop"), Ok(Cmd::Stop));
        assert_eq!(parse_command("mode plan"), Ok(Cmd::Mode("plan".into())));
    }

    #[test]
    fn a_permission_request_is_remembered_with_its_input_and_first_suggestion() {
        let pending: Pending = Arc::default();
        remember_permission_request(
            r#"{"type":"control_request","request_id":"req_1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"},"permission_suggestions":[{"type":"addRules"},{"type":"setMode"}]}}"#,
            &pending,
        );
        remember_permission_request("not json", &pending);
        remember_permission_request(
            r#"{"type":"control_request","request_id":"req_2","request":{"subtype":"other"}}"#,
            &pending,
        );
        let pending = lock(&pending);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending["req_1"].input, json!({"command":"ls"}));
        assert_eq!(
            pending["req_1"].suggestions,
            Some(json!([{"type":"addRules"}]))
        );
    }
}
