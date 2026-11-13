# Foundations and Claude Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust workspace in which a Rune window compiles and renders headless on this machine, and a tested library drives the real `claude` program through an agent-neutral interface, with every uncertain part of the protocol verified live.

**Architecture:** Three library layers and two binaries. `mr-agent` holds the agent-neutral vocabulary, the `Agent` trait and a scripted `FakeAgent`. `mr-claude` implements `Agent` by running one `claude` child process per session and translating its JSON lines. `mr-probe` is a command-line tool used to verify the protocol live and record fixtures. `mindrune` is the app binary, which in this plan is only a smoke test that Rune builds and renders here.

**Tech Stack:** Rust 1.99.1 (edition 2024), Rune at a pinned `dev` revision, `serde` / `serde_json`, `base64`, `uuid`, std threads and channels (no async runtime), `tempfile` for tests, bash stub scripts standing in for `claude` in tests.

**Spec:** `docs/superpowers/specs/2026-11-10-claude-code-ui-design.md`. Read it before starting. This plan implements its sections 7, 8, 9 (agent, claude adapter, fake adapter), 11 and the "verify first" list in section 12.

## This is plan 1 of 4

The spec covers several subsystems. Each plan below ends in working, tested software. Plans 2 to 4 are written after this one lands, because their details depend on what Task 9 verifies.

| Plan | Delivers | Spec sections |
| --- | --- | --- |
| **1. Foundations and Claude adapter (this plan)** | Workspace, Rune smoke test, agent seam, fake agent, Claude adapter, live protocol verification | 7, 8, 9, 11, 12 |
| 2. One session in a window | App state, the shell, one tile: conversation, permission cards, input box with model and mode pickers, usage footer, not-signed-in screen | 5.3 to 5.5, 5.9, 5.10 |
| 3. Many sessions | Sidebar with user-managed groups, stored sessions, tiles side by side, saved layout, session state marks | 5.1, 5.2, 5.8 |
| 4. Attachments, Remote Control, background running | Paste and drop, right panel, Remote Control switch, keep running after the window closes, quit rules, idle process stop | 5.5 to 5.8, 10 |

## Global Constraints

- Rust toolchain is exactly `1.99.1`, edition `2024`.
- Rune is pinned to `rev = "2a44156b21d63b0342a366b26d9222a6edd4622c"` of `https://github.com/anomalyco/rune`. `rune` and `rune-kit` always use the same revision.
- `mr-agent` depends on neither `mr-claude` nor `rune`. No type from `mr-claude` may appear in `mr-agent`, and later in app state or UI code. Only a binary's `main.rs` may name `ClaudeAgent`.
- The software never writes inside `~/.claude`. Reading there is allowed.
- A child `claude` never receives the variables a parent Claude session injects. Task 5 fixes the exact rule.
- An unrecognised or malformed line from `claude` is ignored, never a panic or an error surfaced to the user.
- No blocking work on a UI thread. Everything in `mr-claude` that waits (process I/O, `claude auth status`, reading history) is callable from a worker thread and has no hidden dependency on a window.
- Tests that start the real `claude` are opt-in behind `MINDRUNE_LIVE=1` and `#[ignore]`, use `--model haiku`, and build the child environment from a clean list, not from the test runner's environment.
- No Anthropic logos, colours or fonts anywhere.
- Linux is the first platform. Do not add code that rules out macOS or Windows; bash stub scripts in tests are acceptable.
- `cargo fmt --all` and `cargo test --workspace` pass before every commit.
- Version of `claude` this plan was written against: `2.1.319`.

## Review Focus

The spec implies these conditions but does not spell out tests for them. Each has a test in the task named.

1. **A newer `claude` emits a line this code has never seen** (unknown `type`, unknown `subtype`, missing fields, not JSON). Expected: the line is ignored and the session carries on. Tests: Task 10.
2. **The `claude` process dies in the middle of a turn.** Expected: one `Exited` event with the tail of stderr, and any later command returns `AgentError::Closed` instead of panicking or hanging. Tests: Task 6, Task 16.
3. **A single output line is several megabytes** (a large tool result, an echoed image). Expected: delivered whole. Test: Task 6.
4. **Output contains bytes that are not valid UTF-8.** Expected: decoded lossily, no crash, later lines still arrive. Test: Task 6.
5. **A stored session file is being appended to while it is read**, so its last line is cut off. Expected: the partial line is skipped and everything before it is returned. Test: Task 15.

## File Structure

```
Cargo.toml                         workspace manifest, shared profiles
rust-toolchain.toml                pins 1.99.1
.gitignore
docs/protocol-notes.md             what Task 9 observed, the reference for later plans
crates/agent/                      package mr-agent
  src/lib.rs                       re-exports
  src/ids.rs                       SessionId, RequestId, ToolCallId
  src/event.rs                     SessionEvent and the data it carries
  src/command.rs                   SessionCommand, Attachment, Answer
  src/agent.rs                     Agent, SessionHandle, StartSpec, Capabilities, Account,
                                   StoredSession, HistoryItem, AgentError
  src/fake.rs                      FakeAgent, FakeStep
crates/claude/                     package mr-claude
  src/lib.rs
  src/mode.rs                      permission mode names on the wire
  src/env.rs                       environment given to a child claude
  src/args.rs                      command-line arguments for a session
  src/process.rs                   spawn, read lines, write lines, exit
  src/encode.rs                    everything written to claude's stdin
  src/wire.rs                      parse one stdout line into Incoming
  src/translate.rs                 Incoming stream -> SessionEvent, pending requests
  src/account.rs                   claude auth status
  src/history.rs                   stored sessions on disk
  src/agent.rs                     ClaudeAgent: impl Agent
  tests/stub/stub.sh               stand-in executable for process tests
  tests/stub/fake_claude.sh        stand-in that plays a canned conversation
  tests/fixtures/live/             scrubbed recordings from Task 9
  tests/process.rs, tests/agent.rs, tests/live.rs
crates/probe/                      package mr-probe, binary mindrune-probe
  src/main.rs, src/repl.rs, src/scrub.rs
crates/app/                        package mindrune, binary mindrune
  src/main.rs                      smoke window only, in this plan
```

---

### Task 1: Machine setup and workspace scaffold

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`
- Create: `crates/agent/Cargo.toml`, `crates/agent/src/lib.rs`

**Interfaces:**
- Produces: a workspace where `cargo test --workspace` runs; package `mr-agent` with `serde` (feature `derive`) and `serde_json` as dependencies.

- [ ] **Step 1: Human checkpoint. Stop and ask Jerod to run these three commands themself.**

The machine has no C compiler, no Rust toolchain and no saved claude.ai login. The first needs `sudo`, the second downloads and runs an installer, the third is an interactive sign-in. None of them may be run by an agent.

```bash
sudo apt-get install -y build-essential pkg-config libxkbcommon-dev libwayland-dev libxkbcommon-x11-0
```

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.99.1 --profile minimal -c clippy -c rustfmt
```

```bash
env -u CLAUDE_CODE_OAUTH_TOKEN claude auth login
```

The third command switches off the token from `~/.bashrc` for that one command so the sign-in is saved as a full login. Tasks 2 to 8 need only the first two. Task 9 needs the third.

- [ ] **Step 2: Verify the setup**

Run: `cc --version && ~/.cargo/bin/cargo --version && pkg-config --exists xkbcommon wayland-client && echo deps-ok`
Expected: a compiler version, `cargo 1.99.1 (...)`, and `deps-ok`.

- [ ] **Step 3: Create the repository and the ignore file**

Run `git init -b main`. `.gitignore` contains exactly two lines: `target/` and `.superpowers/`.

- [ ] **Step 4: Write the workspace manifest and toolchain file**

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.99.1"
components = ["clippy", "rustfmt"]
```

`Cargo.toml`. The profile settings matter: Rune's own notes say full debug info made one test build use 18 to 23 GB of memory.

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
edition = "2024"
license = "MIT"
version = "0.1.0"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false
```

- [ ] **Step 5: Create the `mr-agent` package with an empty `lib.rs`**

`crates/agent/Cargo.toml` names the package `mr-agent`, takes `edition`, `license` and `version` from the workspace, and depends on `serde` and `serde_json` from the workspace.

- [ ] **Step 6: Verify the workspace builds**

Run: `cargo test --workspace`
Expected: compiles, `test result: ok. 0 passed`.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "chore: workspace scaffold, toolchain pin, design and plan"
```

---

### Task 2: Rune builds and renders on this machine

**Files:**
- Create: `crates/app/Cargo.toml`, `crates/app/src/main.rs`

**Interfaces:**
- Produces: package `mindrune` with one binary `mindrune`; `fn view(s: &State, cx: &mut Cx<State>) -> El`; a root element keyed `"root"` and a text element keyed `"app-title"`. Plan 2 replaces the view.

Read `examples/hello/src/main.rs` and "Agent tooling" in `crates/rune/README.md` in the Rune repository at the pinned revision before writing this. The hello example is the template for both the app and its test.

- [ ] **Step 1: Write the failing test** in `crates/app/src/main.rs` under `#[cfg(test)] mod tests`

```rust
use super::*;
use rune::{Env, Runtime};

#[test]
fn the_window_shows_its_name() {
    let env = Env { system_dark: true, scale: 2.0 };
    let mut rt = Runtime::new(State::default(), view, (1200.0, 520.0), env);
    rt.frame(0.0);
    assert!(rt.find("root").is_some());
    assert!(rt.find("app-title").is_some());
}
```

- [ ] **Step 2: Add the dependency and run the test to see it fail**

```toml
[dependencies]
rune = { git = "https://github.com/anomalyco/rune", rev = "2a44156b21d63b0342a366b26d9222a6edd4622c" }
```

Run: `cargo test -p mindrune`
Expected: FAIL to compile, `State` and `view` not found. The first build of Rune takes several minutes. If the build stops on a missing system library, stop and ask Jerod to install it; record the package name in Task 1 Step 1.

- [ ] **Step 3: Implement `State`, `view` and `main`**

`State` is an empty struct deriving `Default`. `view` returns a full-size centred column keyed `"root"`, filled with `rune::tok::theme(true).bg`, holding `text("MindRune")` keyed `"app-title"`. `main` calls `rune::run(AppSpec::new("MindRune", (1200.0, 520.0), State::default, view))`.

- [ ] **Step 4: Run the test**

Run: `cargo test -p mindrune`
Expected: PASS.

- [ ] **Step 5: Verify headless rendering**

Run: `cargo run -p mindrune -- --snap target/smoke.png --size 2100x900 && cargo run -p mindrune -- --tree target/smoke.json`
Expected: both files exist; `grep -c app-title target/smoke.json` prints at least `1`; opening `target/smoke.png` shows the words "MindRune" on a dark background.

If it fails with no GPU adapter, retry with `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json` set and record that in `docs/protocol-notes.md` under "Machine notes".

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(app): smoke window proves Rune builds and renders headless"
```

---

### Task 3: The agent vocabulary

**Files:**
- Create: `crates/agent/src/ids.rs`, `event.rs`, `command.rs`, `agent.rs`
- Modify: `crates/agent/src/lib.rs` (re-export everything below with `pub use`)

**Interfaces:**
- Produces: every type below, exactly as named. All data types derive `Clone, Debug, PartialEq, Serialize, Deserialize`. Later tasks and later plans use these names.

```rust
// ids.rs  (also derive Eq, Hash)
pub struct SessionId(pub String);
pub struct RequestId(pub String);
pub struct ToolCallId(pub String);

// event.rs
pub enum SessionEvent {
    Started(SessionInfo),
    TurnStarted,
    TextDelta(String),
    ThinkingDelta(String),
    BlockFinished,                                   // the current text or thinking block ended
    ToolStarted { id: ToolCallId, name: String, detail: ToolDetail },
    ToolFinished { id: ToolCallId, outcome: ToolOutcome },
    NeedsUser(UserRequest),
    TurnFinished(TurnSummary),
    TurnFailed { reason: String },
    UsageLimits(UsageLimits),
    RemoteAccess(RemoteAccess),
    TitleChanged(String),
    ModelChanged(String),
    PermissionModeChanged(PermissionMode),
    CommandFailed { what: String, reason: String },  // a SetModel, Rename etc. was refused
    Exited { clean: bool, detail: String },
}
pub struct SessionInfo { pub id: SessionId, pub folder: PathBuf, pub model: String,
                         pub permission_mode: PermissionMode, pub agent_version: String }
pub enum PermissionMode { Ask, AcceptEdits, Plan, Bypass, Other(String) }
pub enum ToolDetail {
    Command { command: String },
    FileEdit { path: String, old_text: String, new_text: String },
    FileWrite { path: String, content: String },
    FileRead { path: String },
    Search { query: String },
    Other { input_json: String },
}
pub enum ToolOutcome { Ok { output: String }, Failed { output: String } }
pub struct UserRequest { pub id: RequestId, pub kind: UserRequestKind }
pub enum UserRequestKind {
    Permission { tool: String, detail: ToolDetail, always_allow_label: Option<String> },
    Question { questions: Vec<Question> },
    PlanApproval { plan_markdown: String },
}
pub struct Question { pub prompt: String, pub options: Vec<String>, pub multi_select: bool }
pub struct TurnSummary { pub cost_usd: f64, pub context_used: Option<f32>, pub duration_ms: u64 }
pub struct UsageLimits { pub session: Option<LimitWindow>, pub weekly: Option<LimitWindow> }
pub struct LimitWindow { pub used: f32, pub resets_at_unix: i64 }   // used is 0.0 ..= 1.0
pub enum RemoteAccess { Off, Starting, On { url: Option<String> },
                        Failed { reason: String, needs_login: bool } }

// command.rs
pub enum SessionCommand {
    Send { text: String, attachments: Vec<Attachment> },
    Answer { id: RequestId, answer: Answer },
    Stop,
    SetModel(String),
    SetPermissionMode(PermissionMode),
    SetRemoteAccess { on: bool },
    Rename(String),
    Close,
}
pub enum Attachment { Image { media_type: String, data: Vec<u8> }, File { path: PathBuf } }
pub enum Answer {
    Allow { always: bool },
    Deny { message: Option<String> },
    Answers(Vec<QuestionAnswer>),
    ApprovePlan,
    KeepPlanning { feedback: Option<String> },
}
pub struct QuestionAnswer { pub prompt: String, pub chosen: Vec<String>, pub free_text: Option<String> }

// agent.rs
pub type EventSink = Box<dyn Fn(SessionEvent) + Send + 'static>;
pub trait Agent: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    fn account(&self) -> Account;
    fn list_sessions(&self, modified_since_unix: Option<i64>) -> Result<Vec<StoredSession>, AgentError>;
    fn read_session(&self, id: &SessionId) -> Result<Vec<HistoryItem>, AgentError>;
    fn open(&self, spec: StartSpec, sink: EventSink) -> Result<Box<dyn SessionHandle>, AgentError>;
}
pub trait SessionHandle: Send {
    fn send(&self, command: SessionCommand) -> Result<(), AgentError>;
}
pub struct StartSpec { pub folder: PathBuf, pub resume: Option<SessionId>, pub fork: bool,
                       pub model: Option<String>, pub permission_mode: Option<PermissionMode> }
impl StartSpec { pub fn new(folder: impl Into<PathBuf>) -> Self }   // everything else None / false
pub struct Capabilities { pub remote_access: bool, pub plan_mode: bool,
                          pub usage_limits: bool, pub images: bool }
pub enum Account { Missing, SignedOut,
                   SignedIn { name: String, plan: Option<String>, full_login: bool } }
pub struct StoredSession { pub id: SessionId, pub folder: PathBuf, pub title: String,
                           pub last_active_unix: i64 }
pub enum HistoryItem {
    User { text: String, image_count: usize },
    AssistantText(String),
    Thinking(String),
    Tool { id: ToolCallId, name: String, detail: ToolDetail, outcome: Option<ToolOutcome> },
}
pub enum AgentError { NotInstalled, Closed, Io(String), Protocol(String) }  // impl Display, Error
impl LimitWindow { pub fn percent(&self) -> u8 }
```

`EventSink` is called from the adapter's own thread. The caller makes it cheap and thread-safe, typically by pushing onto a channel.

- [ ] **Step 1: Write the failing tests** in `crates/agent/src/event.rs`

```rust
#[test]
fn events_survive_a_json_round_trip() {
    let e = SessionEvent::UsageLimits(UsageLimits {
        session: Some(LimitWindow { used: 0.25, resets_at_unix: 1794385200 }),
        weekly: Some(LimitWindow { used: 0.45, resets_at_unix: 1794776400 }),
    });
    let json = serde_json::to_string(&e).unwrap();
    assert_eq!(serde_json::from_str::<SessionEvent>(&json).unwrap(), e);
}

#[test]
fn a_limit_window_reports_a_whole_percentage() {
    let pct = |used| LimitWindow { used, resets_at_unix: 0 }.percent();
    assert_eq!(pct(0.08), 8);
    assert_eq!(pct(0.456), 46);
    assert_eq!(pct(1.2), 100);
    assert_eq!(pct(-0.1), 0);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-agent`
Expected: FAIL to compile, types not found.

- [ ] **Step 3: Define the types and `LimitWindow::percent`**

`percent` rounds `used * 100` to the nearest whole number and clamps to `0..=100`.

- [ ] **Step 4: Run the tests, and check the dependency rule**

Run: `cargo test -p mr-agent && cargo tree -p mr-agent | grep -cE 'mr-claude|rune' ; true`
Expected: tests PASS; the count printed is `0`.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(agent): agent-neutral events, commands and traits"
```

---

### Task 4: FakeAgent

**Files:**
- Create: `crates/agent/src/fake.rs`
- Modify: `crates/agent/src/lib.rs`

**Interfaces:**
- Consumes: Task 3 types.
- Produces:

```rust
#[derive(Clone)]
pub struct FakeAgent { /* Arc<Mutex<..>> inside, so clones share state */ }
pub struct FakeStep { pub after: f32, pub event: SessionEvent }
impl FakeStep { pub fn at(after: f32, event: SessionEvent) -> Self }
impl FakeAgent {
    pub fn new() -> Self;                                   // Account::SignedIn { name: "Test User", plan: Some("Max plan"), full_login: true }
    pub fn set_account(&self, account: Account);
    pub fn set_sessions(&self, sessions: Vec<StoredSession>);
    pub fn set_history(&self, id: &SessionId, items: Vec<HistoryItem>);
    pub fn reply(&self, steps: Vec<FakeStep>);              // queue one scripted reply
    pub fn emit(&self, id: &SessionId, event: SessionEvent);// deliver on the next advance
    pub fn advance(&self, seconds: f32);                    // move the virtual clock, deliver due events in order
    pub fn sent(&self) -> Vec<(SessionId, SessionCommand)>; // every command any handle received
}
impl Agent for FakeAgent { /* capabilities: all true */ }
```

Behaviour the tests pin: `open` queues a `Started` event, delivered on the next `advance` (including `advance(0.0)`). A new session's id is `fake-1`, `fake-2`, … in order of opening; a resumed one keeps its id. Each `Send` command takes the oldest queued reply and schedules its steps relative to the virtual time of the `Send`. A `Send` with no reply queued schedules `TurnFailed { reason: "fake: no scripted reply" }` at once. Events are never delivered inside `open` or `send`, only inside `advance`. `list_sessions` ignores its argument and returns what `set_sessions` stored.

- [ ] **Step 1: Write the failing tests** in `crates/agent/src/fake.rs`

```rust
fn collect() -> (Arc<Mutex<Vec<SessionEvent>>>, EventSink) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    (seen, Box::new(move |e| s.lock().unwrap().push(e)))
}

#[test]
fn a_scripted_reply_plays_on_the_virtual_clock() {
    let fake = FakeAgent::new();
    fake.reply(vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(0.5, SessionEvent::TextDelta("Hi".into())),
        FakeStep::at(1.0, SessionEvent::TurnFinished(TurnSummary { cost_usd: 0.01, context_used: None, duration_ms: 1000 })),
    ]);
    let (seen, sink) = collect();
    let handle = fake.open(StartSpec::new("/tmp/p"), sink).unwrap();
    assert!(seen.lock().unwrap().is_empty(), "nothing is delivered inside open");
    fake.advance(0.0);
    match &seen.lock().unwrap()[0] {
        SessionEvent::Started(info) => assert_eq!(info.id, SessionId("fake-1".into())),
        other => panic!("expected Started, got {other:?}"),
    }
    handle.send(SessionCommand::Send { text: "hello".into(), attachments: vec![] }).unwrap();
    fake.advance(0.6);
    assert_eq!(seen.lock().unwrap().len(), 3);            // Started, TurnStarted, TextDelta
    fake.advance(0.5);
    assert_eq!(seen.lock().unwrap().len(), 4);
    assert_eq!(fake.sent().len(), 1);
}

#[test]
fn a_send_with_nothing_scripted_fails_the_turn() {
    let fake = FakeAgent::new();
    let (seen, sink) = collect();
    let handle = fake.open(StartSpec::new("/tmp/p"), sink).unwrap();
    handle.send(SessionCommand::Send { text: "x".into(), attachments: vec![] }).unwrap();
    fake.advance(0.0);
    assert_eq!(seen.lock().unwrap().last().unwrap(),
               &SessionEvent::TurnFailed { reason: "fake: no scripted reply".into() });
}

#[test]
fn stored_sessions_history_and_account_are_what_the_test_set() {
    let fake = FakeAgent::new();
    let id = SessionId("s1".into());
    fake.set_sessions(vec![StoredSession { id: id.clone(), folder: "/p".into(), title: "T".into(), last_active_unix: 5 }]);
    fake.set_history(&id, vec![HistoryItem::AssistantText("old".into())]);
    fake.set_account(Account::SignedOut);
    assert_eq!(fake.list_sessions(None).unwrap().len(), 1);
    assert_eq!(fake.read_session(&id).unwrap(), vec![HistoryItem::AssistantText("old".into())]);
    assert_eq!(fake.account(), Account::SignedOut);
    assert!(matches!(fake.read_session(&SessionId("nope".into())), Err(AgentError::Protocol(_))));
}

#[test]
fn emitted_events_reach_the_right_session_on_the_next_advance() {
    let fake = FakeAgent::new();
    let (a, sink_a) = collect();
    let (b, sink_b) = collect();
    fake.open(StartSpec::new("/a"), sink_a).unwrap();
    fake.open(StartSpec::new("/b"), sink_b).unwrap();
    fake.advance(0.0);
    fake.emit(&SessionId("fake-2".into()), SessionEvent::TitleChanged("B".into()));
    fake.advance(0.0);
    assert_eq!(a.lock().unwrap().len(), 1);
    assert_eq!(b.lock().unwrap().len(), 2);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-agent fake`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `FakeAgent`**

One mutex-guarded inner struct holding the virtual clock, the reply queue, a time-ordered list of pending `(due, session, event)` entries, the sinks, and the log of sent commands. `advance` must release the lock before calling a sink, so a sink may call back into the fake.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-agent`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(agent): scripted FakeAgent on a virtual clock"
```

---

### Task 5: Child environment, arguments and mode names

**Files:**
- Create: `crates/claude/Cargo.toml`, `crates/claude/src/lib.rs`, `src/mode.rs`, `src/env.rs`, `src/args.rs`

**Interfaces:**
- Consumes: `mr_agent::{StartSpec, SessionId, PermissionMode}`.
- Produces:

```rust
// mode.rs
pub fn mode_from_wire(name: &str) -> PermissionMode;
pub fn mode_to_wire(mode: &PermissionMode) -> String;
// env.rs
pub fn child_env(parent: Vec<(String, String)>, has_full_login: bool) -> Vec<(String, String)>;
// args.rs
pub fn build_args(spec: &StartSpec, new_id: Option<&SessionId>) -> Vec<String>;
```

`mr-claude` depends on `mr-agent`, `serde`, `serde_json`, `base64 = "0.22"` and `uuid = { version = "1", features = ["v4"] }`; dev-dependency `tempfile = "3"`.

The environment rule, from spec section 8 and the hour it cost during design:

- If the parent environment contains `CLAUDECODE` or `CLAUDE_CODE_SESSION_ID`, this program was started from inside a Claude session. Remove `CLAUDECODE`, every variable starting `ANTHROPIC_`, and every variable starting `CLAUDE_CODE_` except `CLAUDE_CODE_OAUTH_TOKEN`.
- Otherwise pass everything through, including the user's own `ANTHROPIC_*` settings.
- In both cases remove `CLAUDE_CODE_OAUTH_TOKEN` when `has_full_login` is true, so Remote Control works.
- Order of the remaining variables is preserved.

Mode names: `"default"` and `"manual"` read as `Ask`; `"acceptEdits"`, `"plan"`, `"bypassPermissions"` read as their variants; anything else is `Other(name)`. Writing: `Ask` is `"default"`, `Other(s)` is `s`.

Arguments always start with `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio`. Then `--resume <id>` (plus `--fork-session` when `spec.fork`) if `spec.resume` is set, else `--session-id <new_id>` when one is given. Then `--model <m>` if set. Then `--permission-mode <wire name>` if set and not `Ask`: with `Ask` the flag is left off so the user's own Claude Code settings decide.

- [ ] **Step 1: Write the failing tests**

`src/env.rs`:

```rust
fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn variables_from_a_parent_claude_session_are_removed() {
    let parent = env(&[("PATH", "/bin"), ("CLAUDECODE", "1"), ("CLAUDE_CODE_SESSION_ID", "x"),
        ("CLAUDE_CODE_SIMPLE", "1"), ("ANTHROPIC_BASE_URL", "http://x"),
        ("CLAUDE_CODE_OAUTH_TOKEN", "t"), ("HOME", "/h")]);
    assert_eq!(child_env(parent, false),
        env(&[("PATH", "/bin"), ("CLAUDE_CODE_OAUTH_TOKEN", "t"), ("HOME", "/h")]));
}

#[test]
fn a_plain_shell_keeps_the_users_own_settings() {
    let parent = env(&[("PATH", "/bin"), ("ANTHROPIC_API_KEY", "k"), ("CLAUDE_CODE_OAUTH_TOKEN", "t")]);
    assert_eq!(child_env(parent.clone(), false), parent);
}

#[test]
fn the_token_is_dropped_when_a_full_login_exists() {
    let parent = env(&[("PATH", "/bin"), ("CLAUDE_CODE_OAUTH_TOKEN", "t")]);
    assert_eq!(child_env(parent, true), env(&[("PATH", "/bin")]));
}
```

`src/args.rs`:

```rust
const BASE: [&str; 9] = ["-p", "--input-format", "stream-json", "--output-format", "stream-json",
    "--verbose", "--include-partial-messages", "--permission-prompt-tool", "stdio"];
fn with(extra: &[&str]) -> Vec<String> {
    BASE.iter().chain(extra).map(|s| s.to_string()).collect()
}

#[test]
fn a_new_session_is_given_its_id() {
    let id = SessionId("11111111-1111-4111-8111-111111111111".into());
    assert_eq!(build_args(&StartSpec::new("/p"), Some(&id)),
        with(&["--session-id", "11111111-1111-4111-8111-111111111111"]));
}

#[test]
fn a_resumed_session_ignores_the_new_id_and_can_fork() {
    let mut spec = StartSpec::new("/p");
    spec.resume = Some(SessionId("abc".into()));
    let id = SessionId("unused".into());
    assert_eq!(build_args(&spec, Some(&id)), with(&["--resume", "abc"]));
    spec.fork = true;
    assert_eq!(build_args(&spec, None), with(&["--resume", "abc", "--fork-session"]));
}

#[test]
fn model_and_mode_follow_and_ask_leaves_the_mode_flag_off() {
    let mut spec = StartSpec::new("/p");
    spec.model = Some("opus".into());
    spec.permission_mode = Some(PermissionMode::Plan);
    assert_eq!(build_args(&spec, None), with(&["--model", "opus", "--permission-mode", "plan"]));
    spec.permission_mode = Some(PermissionMode::Ask);
    assert_eq!(build_args(&spec, None), with(&["--model", "opus"]));
}
```

`src/mode.rs`:

```rust
#[test]
fn mode_names_map_both_ways_and_keep_unknown_ones() {
    assert_eq!(mode_from_wire("default"), PermissionMode::Ask);
    assert_eq!(mode_from_wire("manual"), PermissionMode::Ask);
    assert_eq!(mode_from_wire("acceptEdits"), PermissionMode::AcceptEdits);
    assert_eq!(mode_from_wire("plan"), PermissionMode::Plan);
    assert_eq!(mode_from_wire("bypassPermissions"), PermissionMode::Bypass);
    assert_eq!(mode_from_wire("dontAsk"), PermissionMode::Other("dontAsk".into()));
    assert_eq!(mode_to_wire(&PermissionMode::Ask), "default");
    assert_eq!(mode_to_wire(&PermissionMode::Other("dontAsk".into())), "dontAsk");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-claude`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the three functions**

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-claude`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(claude): child environment, arguments and mode names"
```

---

### Task 6: Running a child process

**Files:**
- Create: `crates/claude/src/process.rs`, `crates/claude/tests/process.rs`, `crates/claude/tests/stub/stub.sh` (executable)
- Modify: `crates/claude/src/lib.rs`

**Interfaces:**
- Consumes: `mr_agent::AgentError`.
- Produces:

```rust
pub struct SpawnConfig { pub program: PathBuf, pub args: Vec<String>, pub cwd: PathBuf,
                         pub env: Vec<(String, String)> }
pub struct ExitInfo { pub code: Option<i32>, pub stderr_tail: String }
pub struct ClaudeProcess { /* .. */ }
impl ClaudeProcess {
    pub fn spawn(config: SpawnConfig,
                 on_line: impl Fn(String) + Send + 'static,
                 on_exit: impl FnOnce(ExitInfo) + Send + 'static) -> Result<ClaudeProcess, AgentError>;
    pub fn write_line(&self, line: &str) -> Result<(), AgentError>;
    pub fn close_input(&self);
    pub fn kill(&self);
}
```

Rules the tests pin: the child sees only `config.env` (the parent environment is cleared first). `on_line` gets each stdout line without its newline, decoded lossily, whatever its length. `on_exit` is called exactly once, after the last line, with the exit code and the last 4096 bytes of stderr. `write_line` appends a newline and flushes; after the child has exited it returns `AgentError::Closed`. A program that cannot be found returns `AgentError::NotInstalled`. Dropping a `ClaudeProcess` kills the child.

`tests/stub/stub.sh` behaves by its first argument:

| Argument | Behaviour |
| --- | --- |
| `echo` | for each stdin line, print `got:` followed by the line; exit 0 at end of input |
| `env` | print the environment, one `KEY=VALUE` per line; exit 0 |
| `big` | print one line of exactly 5,242,880 `x` characters; exit 0 |
| `badutf8` | print the bytes `ff fe` then ` ok` and a newline, then the line `second`; exit 0 |
| `crash` | print `before`, write `boom` to stderr, exit 3 |
| anything else | the same as `echo` |

- [ ] **Step 1: Write the stub script and the failing tests** in `crates/claude/tests/process.rs`

A helper `run(arg) -> (ClaudeProcess, Receiver<String>, Receiver<ExitInfo>)` spawns the stub with `env = [("PATH", <the test's PATH>), ("ONLY_THIS", "1")]` and forwards both callbacks into channels. Every receive uses `recv_timeout(Duration::from_secs(5))`.

```rust
#[test]
fn lines_go_in_and_come_back() {
    let (p, lines, _exit) = run("echo");
    p.write_line("hello").unwrap();
    assert_eq!(recv(&lines), "got:hello");
}

#[test]
fn the_child_sees_only_the_environment_it_was_given() {
    let (_p, lines, exit) = run("env");
    recv_exit(&exit);
    let all: Vec<String> = lines.try_iter().collect();
    assert!(all.contains(&"ONLY_THIS=1".to_string()));
    // cargo sets CARGO_* in the test process, so their absence shows nothing was inherited
    assert!(!all.iter().any(|l| l.starts_with("CARGO_")));
}

#[test]
fn a_five_megabyte_line_arrives_whole() {
    let (_p, lines, _exit) = run("big");
    assert_eq!(recv(&lines).len(), 5 * 1024 * 1024);
}

#[test]
fn bytes_that_are_not_utf8_do_not_stop_the_stream() {
    let (_p, lines, _exit) = run("badutf8");
    assert!(recv(&lines).ends_with(" ok"));
    assert_eq!(recv(&lines), "second");
}

#[test]
fn a_crash_reports_its_code_and_stderr_and_closes_the_handle() {
    let (p, lines, exit) = run("crash");
    assert_eq!(recv(&lines), "before");
    let info = recv_exit(&exit);
    assert_eq!(info.code, Some(3));
    assert!(info.stderr_tail.contains("boom"));
    assert!(matches!(p.write_line("late"), Err(AgentError::Closed)));
}

#[test]
fn a_missing_program_is_not_installed() {
    let config = SpawnConfig { program: "/nonexistent/claude".into(), args: vec![],
                               cwd: std::env::temp_dir(), env: vec![] };
    assert!(matches!(ClaudeProcess::spawn(config, |_| {}, |_| {}), Err(AgentError::NotInstalled)));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-claude --test process`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `ClaudeProcess`**

Use `std::process::Command` with `env_clear()`, piped stdin, stdout and stderr. One thread reads stdout with `BufRead::read_until(b'\n', ..)` into a byte buffer and converts with `String::from_utf8_lossy`. One thread drains stderr into a ring of the last 4096 bytes. The stdout thread, at end of stream, waits for the child, joins the stderr thread, marks the handle closed, then calls `on_exit`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-claude --test process`
Expected: PASS, six tests.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(claude): child process with line I/O and exit reporting"
```

---

### Task 7: Everything written to claude

**Files:**
- Create: `crates/claude/src/encode.rs`
- Modify: `crates/claude/src/lib.rs`

**Interfaces:**
- Consumes: `mr_agent::{Attachment, PermissionMode}`, `mode_to_wire`.
- Produces (every `String` is one line of JSON with no trailing newline):

```rust
pub fn user_message(text: &str, attachments: &[Attachment]) -> String;
pub fn control_request(request_id: &str, body: serde_json::Value) -> String;
pub fn initialize() -> serde_json::Value;
pub fn interrupt() -> serde_json::Value;
pub fn set_model(model: &str) -> serde_json::Value;
pub fn set_permission_mode(mode: &PermissionMode) -> serde_json::Value;
pub fn remote_control(on: bool, name: Option<&str>) -> serde_json::Value;
pub fn rename_session(title: &str) -> serde_json::Value;
pub enum PermissionReply {
    Allow { updated_input: serde_json::Value, updated_permissions: Option<serde_json::Value> },
    Deny { message: String },
}
pub fn permission_reply(request_id: &str, reply: &PermissionReply) -> String;
pub fn unsupported_reply(request_id: &str) -> String;
```

Shapes come from the Python Agent SDK (`_internal/query.py`, `types.py`) and from the live test in the spec. A user message's content is a list: one text block if the text is not empty, then one image block per image attachment. File attachments are not content blocks; they are appended to the text as a blank line, the line `Attached files:`, then one `- <absolute path>` line per file, and Claude reads them itself.

- [ ] **Step 1: Write the failing tests** in `crates/claude/src/encode.rs`

All comparisons parse the produced line with `serde_json::from_str::<Value>` and compare with `json!`.

```rust
#[test]
fn a_text_message() {
    assert_eq!(parsed(&user_message("hi", &[])),
        json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}));
}

#[test]
fn an_image_becomes_a_base64_block_after_the_text() {
    let png = Attachment::Image { media_type: "image/png".into(), data: vec![1, 2, 3] };
    assert_eq!(parsed(&user_message("look", &[png])),
        json!({"type":"user","message":{"role":"user","content":[
            {"type":"text","text":"look"},
            {"type":"image","source":{"type":"base64","media_type":"image/png","data":"AQID"}}]}}));
}

#[test]
fn an_image_alone_sends_no_empty_text_block() {
    let png = Attachment::Image { media_type: "image/png".into(), data: vec![1, 2, 3] };
    let v = parsed(&user_message("", &[png]));
    assert_eq!(v["message"]["content"].as_array().unwrap().len(), 1);
}

#[test]
fn files_are_listed_by_path_in_the_text() {
    let f = Attachment::File { path: "/work/notes.txt".into() };
    let v = parsed(&user_message("read this", &[f]));
    assert_eq!(v["message"]["content"][0]["text"], "read this\n\nAttached files:\n- /work/notes.txt");
}

#[test]
fn control_requests_are_framed() {
    assert_eq!(parsed(&control_request("rc_1", interrupt())),
        json!({"type":"control_request","request_id":"rc_1","request":{"subtype":"interrupt"}}));
    assert_eq!(initialize(), json!({"subtype":"initialize","hooks":null}));
    assert_eq!(set_model("opus"), json!({"subtype":"set_model","model":"opus"}));
    assert_eq!(set_permission_mode(&PermissionMode::Plan), json!({"subtype":"set_permission_mode","mode":"plan"}));
    assert_eq!(remote_control(true, Some("My session")), json!({"subtype":"remote_control","enabled":true,"name":"My session"}));
    assert_eq!(remote_control(false, None), json!({"subtype":"remote_control","enabled":false}));
    assert_eq!(rename_session("New title"), json!({"subtype":"rename_session","title":"New title"}));
}

#[test]
fn permission_replies_are_framed_as_control_responses() {
    let allow = PermissionReply::Allow { updated_input: json!({"command":"ls"}), updated_permissions: None };
    assert_eq!(parsed(&permission_reply("req_9", &allow)),
        json!({"type":"control_response","response":{"subtype":"success","request_id":"req_9",
               "response":{"behavior":"allow","updatedInput":{"command":"ls"}}}}));
    let always = PermissionReply::Allow { updated_input: json!({}), updated_permissions: Some(json!([{"type":"addRules"}])) };
    assert_eq!(parsed(&permission_reply("req_9", &always))["response"]["response"]["updatedPermissions"],
               json!([{"type":"addRules"}]));
    let deny = PermissionReply::Deny { message: "no".into() };
    assert_eq!(parsed(&permission_reply("req_9", &deny))["response"]["response"],
               json!({"behavior":"deny","message":"no"}));
    assert_eq!(parsed(&unsupported_reply("req_3")),
        json!({"type":"control_response","response":{"subtype":"error","request_id":"req_3",
               "error":"unsupported by mindrune"}}));
}
```

The `rename_session` body was a guess when this task was written. Task 9 item V9 confirmed it.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-claude encode`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the functions**

Build values with `serde_json::json!` and serialise with `serde_json::to_string`, which never emits a raw newline. Use `base64::engine::general_purpose::STANDARD`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-claude encode`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(claude): encode user messages, control requests and replies"
```

---

### Task 8: The probe

**Files:**
- Create: `crates/probe/Cargo.toml`, `crates/probe/src/main.rs`, `src/repl.rs`, `src/scrub.rs`

**Interfaces:**
- Consumes: `mr_claude::{ClaudeProcess, SpawnConfig, child_env, build_args, encode::*}`.
- Produces: binary `mindrune-probe` with two subcommands, and

```rust
// repl.rs
pub enum Cmd { Say(String), Image { path: PathBuf, text: String }, Raw(String), Stop,
               Model(String), Mode(String), Remote(bool), Rename(String),
               Allow(String), AllowAlways(String), Deny(String), Quit, Help }
pub fn parse_command(line: &str) -> Result<Cmd, String>;
// scrub.rs
pub fn scrub_lines(input: &str, home: &str) -> String;
```

**`mindrune-probe raw --cwd DIR [--program PATH] [--resume ID] [--fork] [--model M] [--mode MODE] [--record FILE]`** starts one session through the same code the app will use (`build_args`, `child_env` with `has_full_login` taken from the flag `--full-login`), sends `initialize`, prints every stdout line prefixed `< `, and reads commands from its own stdin:

| Typed | Effect |
| --- | --- |
| `say TEXT` | send a user message |
| `image PATH TEXT` | send TEXT with the file at PATH as an image attachment |
| `{...}` (a line starting with `{`) | send the line unchanged |
| `stop` | send an `interrupt` control request |
| `model NAME`, `mode NAME` | `set_model`, `set_permission_mode` |
| `rc on`, `rc off` | `remote_control` |
| `rename TITLE` | `rename_session` |
| `allow ID`, `always ID`, `deny ID` | answer permission request ID. `allow` echoes the request's own `input` as `updatedInput`; `always` also echoes its `permission_suggestions` as `updatedPermissions` |
| `quit` | close input and exit |

Every line in either direction is appended to `--record FILE` as written, prefixed with `> ` or `< `. Control requests it sends are numbered `probe_1`, `probe_2`, ….

**`mindrune-probe scrub IN OUT`** turns a recording into a fixture: keeps only `< ` lines without the prefix; drops records whose `type` is `system` and whose `subtype` starts with `hook_`; replaces each distinct UUID with `00000000-0000-4000-8000-00000000000N` (N counting from 1 in order of first appearance, zero-padded to 12 digits); replaces any e-mail address with `user@example.com`; replaces the home directory path with `/home/user`.

- [ ] **Step 1: Write the failing tests**

`src/repl.rs`:

```rust
#[test]
fn commands_parse() {
    assert_eq!(parse_command("say hello there"), Ok(Cmd::Say("hello there".into())));
    assert_eq!(parse_command("image /tmp/a.png what is this"),
               Ok(Cmd::Image { path: "/tmp/a.png".into(), text: "what is this".into() }));
    assert_eq!(parse_command("{\"type\":\"x\"}"), Ok(Cmd::Raw("{\"type\":\"x\"}".into())));
    assert_eq!(parse_command("rc on"), Ok(Cmd::Remote(true)));
    assert_eq!(parse_command("rc off"), Ok(Cmd::Remote(false)));
    assert_eq!(parse_command("always req_1"), Ok(Cmd::AllowAlways("req_1".into())));
    assert_eq!(parse_command("deny req_1"), Ok(Cmd::Deny("req_1".into())));
    assert!(parse_command("dance").is_err());
    assert!(parse_command("rc maybe").is_err());
}
```

`src/scrub.rs`:

```rust
#[test]
fn a_recording_becomes_a_fixture() {
    let rec = concat!(
        "> {\"type\":\"user\"}\n",
        "< {\"type\":\"system\",\"subtype\":\"hook_started\",\"uuid\":\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\"}\n",
        "< {\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb\",\"cwd\":\"/home/alex/x\"}\n",
        "< {\"type\":\"result\",\"session_id\":\"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb\",\"who\":\"alex@example.org\"}\n");
    assert_eq!(scrub_lines(rec, "/home/alex"), concat!(
        "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"00000000-0000-4000-8000-000000000001\",\"cwd\":\"/home/user/x\"}\n",
        "{\"type\":\"result\",\"session_id\":\"00000000-0000-4000-8000-000000000001\",\"who\":\"user@example.com\"}\n"));
}
```

Scrubbing works on the text of each line, not on parsed JSON, so key order is untouched.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-probe`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `parse_command`, `scrub_lines` and the two subcommands**

The probe remembers each incoming `can_use_tool` request by `request_id` so `allow`, `always` and `deny` can build their replies. Parse arguments by hand; no argument-parsing crate.

- [ ] **Step 4: Run the tests, then try the probe against the stub**

Run: `cargo test -p mr-probe && printf 'say hi\nquit\n' | cargo run -q -p mr-probe -- raw --cwd /tmp --program crates/claude/tests/stub/stub.sh`
Expected: tests PASS. The stub echoes whatever it is sent, so the run prints two lines: `< got:` followed by the `initialize` control request, and `< got:` followed by the user message carrying `hi`. Then it exits with status 0.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(probe): raw session probe and fixture scrubber"
```

---

### Task 9: Live verification (gate; needs Jerod)

**Files:**
- Create: `docs/protocol-notes.md`
- Create: `crates/claude/tests/fixtures/live/*.jsonl`

**Interfaces:**
- Consumes: the probe from Task 8.
- Produces: `docs/protocol-notes.md`, which Tasks 10 to 16 and plans 2 to 4 treat as the source of truth, and scrubbed fixtures. **Where a recording differs from a shape assumed later in this plan, the recording wins:** correct the later task's test data to match and note the difference in `docs/protocol-notes.md`.

This task writes no code and has no unit tests. It exists because spec section 12 says to verify these things before building on them.

- [ ] **Step 1: Confirm the sign-in**

Run: `env -u CLAUDE_CODE_OAUTH_TOKEN claude auth status`
Expected: `"loggedIn": true`. If not, stop: Task 1 Step 1's third command has not been completed. Save the full output, with the e-mail replaced, in the notes as item V10. It decides Task 14's field names.

The probe must run in Jerod's own terminal, not in an agent's sandboxed shell, which cannot see the login. Use `--cwd target/probe-cwd` (create it; it is ignored by git) and `--model haiku --full-login --record target/rec/<item>.txt` for every run.

- [ ] **Step 2: V1 Remote Control. Stop the whole plan if this fails.**

Type `say Reply with the single word: ok`, wait for the `result` line, then `rc on`. Record the `control_response` and every `system`/`bridge_state` line that follows, with their `state` values in order. Ask Jerod to confirm the session appears at claude.ai/code. Then `rc off` and record again.

If Remote Control cannot be turned on, stop and tell Jerod. Do not continue to Task 10, and do not drop the feature.

- [ ] **Step 3: V2 and V3 permission round trip**

- V2: `say Use the Bash tool to run: echo mindrune-probe`. Expect an incoming `control_request` with `subtype` `can_use_tool`. If none arrives because Jerod's own Claude Code settings already allow that command, use one they do not cover, such as `touch probe-marker.txt`. Record the whole request, especially `permission_suggestions`. Answer `allow ID`; confirm the tool ran and its output appears in a `user` record with a `tool_result` block. Repeat the message and answer `deny ID`; record what Claude receives.
- V3: repeat once more and answer `always ID`. Send the same message a fourth time and record whether a request arrives. Record which file, if any, appeared under `target/probe-cwd/.claude/`.

- [ ] **Step 4: V4 to V8**

- V4 image: create a 16×16 solid red PNG at `target/probe-cwd/red.png`; `image target/probe-cwd/red.png What colour is this image? One word.` Expect "red".
- V5 model: `model sonnet`, then `say Reply with: ok`. Record where the new model name shows (a new `system`/`init`, the `assistant` record, the control response).
- V6 stop: `say Count slowly from 1 to 300, one number per line.` then `stop` after a few numbers. Record the `result` line's `subtype`, `is_error` and `stop_reason`, then `say Reply with: ok` to confirm the session still works.
- V7 questions and plans: `say Ask me one multiple-choice question with your question tool before doing anything else.` Record the request (`tool_name` and the shape of `input`). Answer with a raw line of this form, where `updatedInput` is the request's own `input` with one added key, and record whether Claude received the answer: `{"type":"control_response","response":{"subtype":"success","request_id":"<ID>","response":{"behavior":"allow","updatedInput":{"questions":[...],"answers":{"<the question text>":"<the chosen option's label>"}}}}}`. Then `mode plan`, `say Plan how to add a README here, then ask to leave plan mode.` Record the request, answer `allow ID`, and record what follows.
- V8 second turn: note whether a `system`/`init` record is sent again at the start of every turn or only once.

- [ ] **Step 5: V9 to V11**

- V9 other control requests: send each of these raw and record the response or the error: `{"type":"control_request","request_id":"p1","request":{"subtype":"get_usage"}}`, the same with `get_context_usage`, `list_models`, and `rename` followed by a title. Note any `session_title_changed` record.
- V10: already recorded in Step 1.
- V11 resume and fork: quit, then start the probe again with `--resume <the session id>`; send a message and confirm the earlier conversation is remembered. Start again with `--resume <id> --fork` and record the new `session_id` in `system`/`init`.

- [ ] **Step 6: Scrub the recordings into fixtures**

Run `mindrune-probe scrub target/rec/<item>.txt crates/claude/tests/fixtures/live/<item>.jsonl` for each. Read each fixture before committing and remove anything personal the scrubber missed.

- [ ] **Step 7: Write `docs/protocol-notes.md`**

One section per item V1 to V11 with: what was sent, what came back (the exact JSON of the key lines), the fixture file, and "Differs from the plan:" followed by either "no" or the exact difference and which task's test data must change. Start the file with a "Machine notes" section (anything from Task 2 Step 5) and the `claude` version.

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "docs: live protocol verification notes and fixtures"
```

---

### Task 10: Parsing a line from claude

**Files:**
- Create: `crates/claude/src/wire.rs`
- Modify: `crates/claude/src/lib.rs`
- Modify: `crates/claude/src/encode.rs` (remove one comment, see below)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces:

```rust
pub fn parse_line(line: &str) -> Incoming;      // never panics, never returns an error
pub enum Incoming {
    Init { session_id: String, model: String, permission_mode: String, version: String, cwd: String },
    BridgeState { state: String, detail: Option<String>, failure_kind: Option<String> },
    TitleChanged { title: String },
    Stream { parent: Option<String>, event: StreamEvent },
    Assistant { parent: Option<String>, message_id: String, blocks: Vec<Block>, usage_tokens: Option<u64> },
    ToolResults { parent: Option<String>, results: Vec<ToolResult> },
    Result { is_error: bool, interrupted: bool, text: String, cost_usd: f64, duration_ms: u64, context_window: Option<u64> },
    RateLimit { five_hour: Option<(f32, i64)>, seven_day: Option<(f32, i64)> },   // (utilization, resetsAt)
    ControlRequest { request_id: String, subtype: String, body: serde_json::Value },
    ControlResponse { request_id: String, ok: bool, body: serde_json::Value,
                      error: Option<String>, error_code: Option<String> },
    ControlCancel { request_id: String },
    ModeChanged { permission_mode: String },
    Ignored { kind: String },
}
pub enum StreamEvent { MessageStart, BlockStart(BlockKind), TextDelta(String), ThinkingDelta(String), BlockStop, Other }
pub enum BlockKind { Text, Thinking, ToolUse, Other }
pub enum Block { Text(String), Thinking(String), ToolUse { id: String, name: String, input: serde_json::Value }, Other }
pub struct ToolResult { pub tool_use_id: String, pub is_error: bool, pub text: String }
```

All derive `Clone, Debug, PartialEq`. Parse by walking a `serde_json::Value`, not by strict `Deserialize`, so extra and missing fields never fail. Field sources: `usage_tokens` is the sum of `input_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens` and `output_tokens` in `message.usage`; `context_window` is the largest `contextWindow` among the entries of `modelUsage`; `parent` is `parent_tool_use_id`; a `tool_result` block's `text` is its `content` if that is a string, else the `text` fields of its content blocks joined with newlines; `ControlRequest.body` is the `request` object; `ControlResponse.body` is `response.response` (or `null`); `Ignored.kind` is `type` plus `/subtype` when there is one, or `"not-json"` for a line that is not a JSON object. A record of a known type that lacks the fields its variant needs is `Ignored` too.

**Added after Task 9's live run (source: `docs/protocol-notes.md`). These rules and tests are part of this task. The text above was edited to agree with them.**

- `Incoming::Result.interrupted` is true when the record's `terminal_reason` is a string starting with `aborted`. `Result.text` is the record's `result` when that is a string; otherwise the strings in `errors` joined with `"; "`; otherwise the `subtype`; otherwise empty.
- `ControlCancel` is parsed from a record of type `control_cancel_request`.
- `ModeChanged` is parsed from `system`/`status` when it has a string `permissionMode`. A `system`/`status` without one is `Ignored { kind: "system/status" }`.
- A `user` record with no `tool_result` blocks is `Ignored { kind: "user" }`. That covers `message.content` being a string, and content that holds only text blocks.
- In `crates/claude/src/encode.rs`, remove the "unverified" marker comment on `rename_session`. V9 confirmed the shape.
- A `thinking_delta` is `ThinkingDelta` even when its text is empty (it always is, live). A `signature_delta` is `StreamEvent::Other`.
- The fixtures from Task 9 are swept by `the_live_recordings_parse_into_every_known_kind` below.

```rust
#[test]
fn a_stopped_turn_is_marked_interrupted() {
    let l = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"stop_reason":null,"terminal_reason":"aborted_streaming","errors":["[ede_diagnostic] result_type=user"],"duration_ms":3555,"total_cost_usd":0.06}"#;
    assert_eq!(parse_line(l), Incoming::Result { is_error: true, interrupted: true,
        text: "[ede_diagnostic] result_type=user".into(), cost_usd: 0.06, duration_ms: 3555, context_window: None });
    let no_text = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"duration_ms":1,"total_cost_usd":0}"#;
    assert_eq!(parse_line(no_text), Incoming::Result { is_error: true, interrupted: false,
        text: "error_max_turns".into(), cost_usd: 0.0, duration_ms: 1, context_window: None });
}

#[test]
fn cancel_status_and_title_records() {
    assert_eq!(parse_line(r#"{"type":"control_cancel_request","request_id":"r9"}"#),
        Incoming::ControlCancel { request_id: "r9".into() });
    assert_eq!(parse_line(r#"{"type":"system","subtype":"status","status":null,"permissionMode":"plan","session_id":"s1"}"#),
        Incoming::ModeChanged { permission_mode: "plan".into() });
    assert_eq!(parse_line(r#"{"type":"system","subtype":"status","status":"requesting","session_id":"s1"}"#),
        Incoming::Ignored { kind: "system/status".into() });
    assert_eq!(parse_line(r#"{"type":"system","subtype":"session_title_changed","title":"Probe rename B","session_id":"s1"}"#),
        Incoming::TitleChanged { title: "Probe rename B".into() });
}

#[test]
fn user_records_without_tool_results_are_ignored() {
    let replay = r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>Set model</local-command-stdout>"},"isReplay":true}"#;
    assert_eq!(parse_line(replay), Incoming::Ignored { kind: "user".into() });
    let note = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#;
    assert_eq!(parse_line(note), Incoming::Ignored { kind: "user".into() });
}

#[test]
fn the_live_recordings_parse_into_every_known_kind() {
    let mut seen = std::collections::BTreeSet::new();
    let mut lines = 0;
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/live")).unwrap() {
        for line in std::fs::read_to_string(entry.unwrap().path()).unwrap().lines() {
            lines += 1;
            let name = match parse_line(line) {
                Incoming::Init { .. } => "Init", Incoming::BridgeState { .. } => "BridgeState",
                Incoming::TitleChanged { .. } => "TitleChanged", Incoming::Stream { .. } => "Stream",
                Incoming::Assistant { .. } => "Assistant", Incoming::ToolResults { .. } => "ToolResults",
                Incoming::Result { .. } => "Result", Incoming::RateLimit { .. } => "RateLimit",
                Incoming::ControlRequest { .. } => "ControlRequest", Incoming::ControlResponse { .. } => "ControlResponse",
                Incoming::ControlCancel { .. } => "ControlCancel", Incoming::ModeChanged { .. } => "ModeChanged",
                Incoming::Ignored { kind } => { assert_ne!(kind, "not-json", "{line}"); "Ignored" }
            };
            seen.insert(name);
        }
    }
    assert!(lines > 500, "the fixtures are present");
    let all = ["Assistant", "BridgeState", "ControlCancel", "ControlRequest", "ControlResponse", "Ignored", "Init",
               "ModeChanged", "RateLimit", "Result", "Stream", "TitleChanged", "ToolResults"];
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), all);
}
```

- [ ] **Step 1: Write the failing tests** in `crates/claude/src/wire.rs`

Use these exact lines. They are the shapes observed live on 2026-11-10 with ids shortened.

```rust
#[test]
fn init() {
    let l = r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-haiku-5-5","permissionMode":"default","claude_code_version":"2.1.319","cwd":"/work","tools":["Bash"]}"#;
    assert_eq!(parse_line(l), Incoming::Init { session_id: "s1".into(), model: "claude-haiku-5-5".into(),
        permission_mode: "default".into(), version: "2.1.319".into(), cwd: "/work".into() });
}

#[test]
fn streamed_text() {
    let l = r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}}"#;
    assert_eq!(parse_line(l), Incoming::Stream { parent: None, event: StreamEvent::TextDelta("ok".into()) });
    let start = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}"#;
    assert_eq!(parse_line(start), Incoming::Stream { parent: None, event: StreamEvent::BlockStart(BlockKind::Thinking) });
}

#[test]
fn an_assistant_record_with_a_tool_call() {
    let l = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"id":"m1","content":[{"type":"text","text":"Running it."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":30,"output_tokens":40}}}"#;
    assert_eq!(parse_line(l), Incoming::Assistant { parent: None, message_id: "m1".into(),
        blocks: vec![Block::Text("Running it.".into()),
                     Block::ToolUse { id: "t1".into(), name: "Bash".into(), input: json!({"command":"ls"}) }],
        usage_tokens: Some(100) });
}

#[test]
fn tool_results_with_string_and_block_content() {
    let l = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"file.txt"},{"type":"tool_result","tool_use_id":"t2","is_error":true,"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}]}}"#;
    assert_eq!(parse_line(l), Incoming::ToolResults { parent: None, results: vec![
        ToolResult { tool_use_id: "t1".into(), is_error: false, text: "file.txt".into() },
        ToolResult { tool_use_id: "t2".into(), is_error: true, text: "a\nb".into() }] });
}

#[test]
fn result_and_rate_limit() {
    let r = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":915,"total_cost_usd":0.0020488399999999997,"result":"ok","modelUsage":{"claude-haiku-5-5":{"contextWindow":1000000}}}"#;
    assert_eq!(parse_line(r), Incoming::Result { is_error: false, interrupted: false, text: "ok".into(),
        cost_usd: 0.0020488399999999997, duration_ms: 915, context_window: Some(1000000) });
    let l = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1794385200,"rateLimitType":"five_hour","unifiedWindows":{"five_hour":{"utilization":0.08,"resetsAt":1794385200,"status":"allowed"},"seven_day":{"utilization":0.06,"resetsAt":1794776400,"status":"allowed"}}}}"#;
    assert_eq!(parse_line(l), Incoming::RateLimit { five_hour: Some((0.08, 1794385200)), seven_day: Some((0.06, 1794776400)) });
}

#[test]
fn bridge_state_and_control_messages() {
    let b = r#"{"type":"system","subtype":"bridge_state","state":"failed","detail":"Claude.ai login was rejected","failure_kind":"auth"}"#;
    assert_eq!(parse_line(b), Incoming::BridgeState { state: "failed".into(),
        detail: Some("Claude.ai login was rejected".into()), failure_kind: Some("auth".into()) });
    let q = r#"{"type":"control_request","request_id":"req_1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
    assert_eq!(parse_line(q), Incoming::ControlRequest { request_id: "req_1".into(), subtype: "can_use_tool".into(),
        body: json!({"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}) });
    let e = r#"{"type":"control_response","response":{"subtype":"error","request_id":"rc_on","error":"nope","error_code":"remote_control_auth"}}"#;
    assert_eq!(parse_line(e), Incoming::ControlResponse { request_id: "rc_on".into(), ok: false,
        body: json!(null), error: Some("nope".into()), error_code: Some("remote_control_auth".into()) });
    let s = r#"{"type":"control_response","response":{"subtype":"success","request_id":"rc_off"}}"#;
    assert_eq!(parse_line(s), Incoming::ControlResponse { request_id: "rc_off".into(), ok: true,
        body: json!(null), error: None, error_code: None });
}

#[test]
fn anything_unrecognised_is_ignored_not_fatal() {
    assert_eq!(parse_line(""), Incoming::Ignored { kind: "not-json".into() });
    assert_eq!(parse_line("not json at all"), Incoming::Ignored { kind: "not-json".into() });
    assert_eq!(parse_line(r#"{"type":"assistant""#), Incoming::Ignored { kind: "not-json".into() });
    assert_eq!(parse_line(r#"{"type":"brand_new_thing"}"#), Incoming::Ignored { kind: "brand_new_thing".into() });
    assert_eq!(parse_line(r#"{"type":"system","subtype":"hook_started"}"#), Incoming::Ignored { kind: "system/hook_started".into() });
    assert_eq!(parse_line(r#"{"type":"assistant"}"#), Incoming::Ignored { kind: "assistant".into() });
    assert_eq!(parse_line("[1,2,3]"), Incoming::Ignored { kind: "not-json".into() });
}
```

`TitleChanged` is parsed from `system`/`session_title_changed` with a `title` field.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-claude wire`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `parse_line`**

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-claude wire`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(claude): tolerant parser for claude's output lines"
```

---

### Task 11: Translating a turn into events

**Files:**
- Create: `crates/claude/src/translate.rs`
- Modify: `crates/claude/src/lib.rs`

**Interfaces:**
- Consumes: `Incoming` and friends (Task 10), `mode_from_wire` (Task 5), `mr_agent` events.
- Produces:

```rust
pub struct Translator { /* .. */ }
impl Translator {
    pub fn new() -> Self;
    pub fn push(&mut self, message: Incoming) -> Vec<SessionEvent>;
}
pub fn tool_detail(name: &str, input: &serde_json::Value) -> ToolDetail;
```

Rules:

- The first `Init` gives `Started`. A later `Init` gives `ModelChanged` and/or `PermissionModeChanged` only for values that differ from the last known ones, and otherwise nothing.
- Records whose `parent` is set belong to a sub-agent and give no events.
- `MessageStart` gives `TurnStarted` if no turn is open. A turn closes at `Result`.
- `BlockStart` records the kind of the open block. `TextDelta` and `ThinkingDelta` pass through, except that a delta whose text is empty gives nothing. `BlockStop` gives `BlockFinished` only when the open block was text or thinking and at least one non-empty delta was passed on for it.
- A complete `Assistant` record gives `ToolStarted` for each `ToolUse` block. Its text and thinking blocks give nothing if any stream event was seen since the last `Assistant` record; if none was (partial messages absent), each gives its delta event followed by `BlockFinished`, and a block whose text is empty gives nothing.
- `ToolResults` gives one `ToolFinished` per result: `Failed` when `is_error`, else `Ok`.
- `Result` gives `TurnFailed { reason: text }` when `is_error` and not `interrupted`. Otherwise it gives `TurnFinished` with `context_used = usage_tokens / context_window` using the latest top-level `Assistant` record's `usage_tokens`, or `None` when either number is missing.
- `tool_detail`: `Bash` → `Command { command }`; `Edit` → `FileEdit { path: file_path, old_text: old_string, new_text: new_string }`; `Write` → `FileWrite { path: file_path, content }`; `Read` → `FileRead { path: file_path }`; `Grep` and `Glob` → `Search { query: pattern }`; `WebSearch` → `Search { query }`; anything else, or a known tool missing its fields → `Other { input_json }` with the input serialised compactly.

**Added after Task 9's live run (source: `docs/protocol-notes.md`). These rules and tests are part of this task. The text above was edited to agree with them.**

- A stopped turn is not a failure: a `Result` with `interrupted` gives `TurnFinished` with the usual summary.
- Live, every `thinking_delta` has empty text, so a thinking block currently produces no events at all. That is intended.
- `Init` names the selected model. The model that answers can differ (see the notes, V5). `ModelChanged` follows `Init` only; do not read the model from `Assistant` records.
- `ModeChanged` gives `PermissionModeChanged` when the mode differs from the last known one, and updates the last known one. It shares the last-known value with the `Init` rule.

```rust
#[test]
fn a_stopped_turn_finishes_without_failing() {
    let stopped = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"terminal_reason":"aborted_streaming","errors":["x"],"duration_ms":3555,"total_cost_usd":0.06}"#;
    assert_eq!(feed(&[INIT, MSG_START, stopped])[2],
        SessionEvent::TurnFinished(TurnSummary { cost_usd: 0.06, context_used: None, duration_ms: 3555 }));
}

#[test]
fn an_empty_thinking_block_gives_no_events() {
    let think_start = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}}"#;
    let think_delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"","estimated_tokens":null}}}"#;
    let sig_delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}}"#;
    assert_eq!(feed(&[INIT, MSG_START, think_start, think_delta, sig_delta, BLOCK_STOP]).len(), 2, "Started and TurnStarted only");
    let whole = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"thinking","thinking":"","signature":"sig"}]}}"#;
    assert_eq!(feed(&[INIT, whole]).len(), 1);
}

#[test]
fn a_status_record_reports_a_mode_change_once() {
    let plan = r#"{"type":"system","subtype":"status","status":null,"permissionMode":"plan"}"#;
    assert_eq!(&feed(&[INIT, plan, plan])[1..], &[SessionEvent::PermissionModeChanged(PermissionMode::Plan)]);
}
```

- [ ] **Step 1: Write the failing tests** in `crates/claude/src/translate.rs`

A helper `feed(lines: &[&str]) -> Vec<SessionEvent>` parses each line with `parse_line` and pushes it through one `Translator`, collecting all events.

```rust
const INIT: &str = r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-haiku-5-5","permissionMode":"default","claude_code_version":"2.1.319","cwd":"/work"}"#;
const MSG_START: &str = r#"{"type":"stream_event","event":{"type":"message_start","message":{}}}"#;
const TEXT_START: &str = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}"#;
const DELTA_OK: &str = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}}"#;
const BLOCK_STOP: &str = r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#;
const ASSISTANT_OK: &str = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":40000,"cache_read_input_tokens":9000,"cache_creation_input_tokens":0,"output_tokens":1000}}}"#;
const RESULT_OK: &str = r#"{"type":"result","is_error":false,"duration_ms":915,"total_cost_usd":0.002,"result":"ok","modelUsage":{"claude-haiku-5-5":{"contextWindow":1000000}}}"#;

#[test]
fn a_streamed_text_turn() {
    let events = feed(&[INIT, MSG_START, TEXT_START, DELTA_OK, BLOCK_STOP, ASSISTANT_OK, RESULT_OK]);
    assert_eq!(events, vec![
        SessionEvent::Started(SessionInfo { id: SessionId("s1".into()), folder: "/work".into(),
            model: "claude-haiku-5-5".into(), permission_mode: PermissionMode::Ask, agent_version: "2.1.319".into() }),
        SessionEvent::TurnStarted,
        SessionEvent::TextDelta("ok".into()),
        SessionEvent::BlockFinished,
        SessionEvent::TurnFinished(TurnSummary { cost_usd: 0.002, context_used: Some(0.05), duration_ms: 915 }),
    ]);
}

#[test]
fn without_partial_messages_the_whole_text_still_arrives() {
    let events = feed(&[INIT, ASSISTANT_OK, RESULT_OK]);
    assert_eq!(&events[1..3], &[SessionEvent::TextDelta("ok".into()), SessionEvent::BlockFinished]);
}

#[test]
fn a_tool_call_starts_and_finishes() {
    let call = r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#;
    let done = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"a.txt"}]}}"#;
    let events = feed(&[INIT, MSG_START, call, done]);
    assert_eq!(&events[2..], &[
        SessionEvent::ToolStarted { id: ToolCallId("t1".into()), name: "Bash".into(),
            detail: ToolDetail::Command { command: "ls".into() } },
        SessionEvent::ToolFinished { id: ToolCallId("t1".into()), outcome: ToolOutcome::Ok { output: "a.txt".into() } },
    ]);
}

#[test]
fn a_second_init_reports_only_what_changed() {
    let same = INIT;
    let other = r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-sonnet-5-5","permissionMode":"plan","claude_code_version":"2.1.319","cwd":"/work"}"#;
    let events = feed(&[INIT, same, other]);
    assert_eq!(&events[1..], &[SessionEvent::ModelChanged("claude-sonnet-5-5".into()),
                               SessionEvent::PermissionModeChanged(PermissionMode::Plan)]);
}

#[test]
fn sub_agent_records_are_not_shown() {
    let sub = r#"{"type":"assistant","parent_tool_use_id":"t9","message":{"id":"m3","content":[{"type":"text","text":"inner"}]}}"#;
    assert_eq!(feed(&[INIT, sub]).len(), 1);
}

#[test]
fn a_failed_result_fails_the_turn() {
    let bad = r#"{"type":"result","is_error":true,"result":"Failed to authenticate","duration_ms":92,"total_cost_usd":0}"#;
    assert_eq!(feed(&[INIT, bad])[1], SessionEvent::TurnFailed { reason: "Failed to authenticate".into() });
}

#[test]
fn tool_details_by_tool() {
    assert_eq!(tool_detail("Edit", &json!({"file_path":"/a","old_string":"x","new_string":"y"})),
        ToolDetail::FileEdit { path: "/a".into(), old_text: "x".into(), new_text: "y".into() });
    assert_eq!(tool_detail("Write", &json!({"file_path":"/a","content":"c"})),
        ToolDetail::FileWrite { path: "/a".into(), content: "c".into() });
    assert_eq!(tool_detail("Read", &json!({"file_path":"/a"})), ToolDetail::FileRead { path: "/a".into() });
    assert_eq!(tool_detail("Grep", &json!({"pattern":"TODO"})), ToolDetail::Search { query: "TODO".into() });
    assert_eq!(tool_detail("WebSearch", &json!({"query":"rust"})), ToolDetail::Search { query: "rust".into() });
    assert_eq!(tool_detail("Mystery", &json!({"a":1})), ToolDetail::Other { input_json: r#"{"a":1}"#.into() });
    assert_eq!(tool_detail("Bash", &json!({})), ToolDetail::Other { input_json: "{}".into() });
}
```

Task 9 item V8 found that `init` is sent at the start of every turn, so the `a_second_init_reports_only_what_changed` rule runs on every turn.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-claude translate`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `Translator::push` for these message kinds and `tool_detail`**

Messages this task does not cover (`RateLimit`, `BridgeState`, `ControlRequest`, `ControlResponse`, `ControlCancel`, `TitleChanged`, `Ignored`) return an empty list for now.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-claude translate`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(claude): translate text, thinking, tools and turn results"
```

---

### Task 12: Requests that need the user

**Files:**
- Modify: `crates/claude/src/translate.rs`
- Modify: `crates/agent/src/event.rs` (one new event, see below)

**Interfaces:**
- Consumes: `PermissionReply` (Task 7), `tool_detail` (Task 11).
- Produces:

```rust
impl Translator {
    pub fn answer(&mut self, id: &RequestId, answer: Answer) -> Result<PermissionReply, AgentError>;
}
pub fn always_allow_label(suggestions: &serde_json::Value) -> Option<String>;
```

Rules:

- An incoming `ControlRequest` with subtype `can_use_tool` gives `NeedsUser`, and the translator remembers the request's `input` and `permission_suggestions` under its `request_id`. `tool_name` `AskUserQuestion` gives `Question`; `ExitPlanMode` gives `PlanApproval { plan_markdown: input.plan }`; any other tool gives `Permission` with `tool_detail(tool_name, input)`.
- A request's `input` that is not a JSON object is treated as `{}`. A request with no `tool_name` is a `Permission` for the tool `"unknown tool"`.
- Any other incoming `ControlRequest` subtype gives no event. Task 16 replies to those.
- `always_allow_label` looks at the first suggestion only:
  - `type` `addRules` with `behavior` `allow`: `"Always allow {rules} {scope}"`, where `{rules}` names every rule in the suggestion joined with `", "` (each rule's `ruleContent` if present, else its `toolName`; no rules, or a rule with neither, gives `None`), and `{scope}` is `for this session` for destination `session`, `in this project` for `localSettings` or `projectSettings`, `in all projects` for `userSettings`.
  - `type` `setMode` with `mode` `acceptEdits` and destination `session`: `"Allow all edits for this session"`.
  - anything else, an unknown destination, an empty list or `null`: `None`.
- `answer` removes the pending request and returns:
  - `Allow { always: false }` → `PermissionReply::Allow` with the remembered input and no permissions.
  - `Allow { always: true }` → the same with `updated_permissions: Some(<a one-element list holding only the first remembered suggestion>)`, but only when `always_allow_label` gives a label for the remembered suggestions. Otherwise exactly as `always: false`.
  - `Deny { message }` → `PermissionReply::Deny`, the message defaulting to `"Denied by the user"`.
  - `Answers(list)` for a question → `Allow` whose input is the remembered input with an added `"answers"` object mapping each `prompt` to the chosen options joined with `", "`, or to `free_text` when nothing was chosen.
  - `ApprovePlan` for a plan → `Allow` with the remembered input. `KeepPlanning { feedback }` → `Deny`, the message defaulting to `"Keep planning"`.
  - An unknown id, or an answer that does not fit the request's kind (for example `Answers` for a permission), → `Err(AgentError::Protocol(..))` and the request stays pending.

Task 9 item V7 confirmed the question and plan shapes.

**Added after Task 9's live run (source: `docs/protocol-notes.md`). These rules and tests are part of this task. The text above was edited to agree with them.**

- Why always-allow sends one suggestion: live requests carry several suggestions of different kinds, and sending them all back would also switch the session to `acceptEdits`.
- `ControlCancel { request_id }` for a pending request forgets it and gives `SessionEvent::RequestWithdrawn { id }`. For an unknown id it gives nothing.
- This task therefore also modifies `crates/agent/src/event.rs`: add the variant `RequestWithdrawn { id: RequestId }` to `SessionEvent`, next to `NeedsUser`.

```rust
#[test]
fn always_allow_sends_only_the_first_suggestion() {
    let three = json!([
        {"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"touch probe-marker.txt"}],"behavior":"allow","destination":"localSettings"},
        {"type":"addDirectories","directories":["/work"],"destination":"session"},
        {"type":"setMode","mode":"acceptEdits","destination":"session"}]);
    let mut t = Translator::new();
    t.push(request("req_1", "Bash", json!({"command":"touch probe-marker.txt"}), three.clone()));
    let reply = t.answer(&RequestId("req_1".into()), Answer::Allow { always: true }).unwrap();
    assert_eq!(reply, PermissionReply::Allow { updated_input: json!({"command":"touch probe-marker.txt"}),
        updated_permissions: Some(json!([three[0].clone()])) });
}

#[test]
fn always_allow_without_a_usable_suggestion_allows_once() {
    let mut t = Translator::new();
    t.push(request("req_1", "Bash", json!({"command":"ls"}), json!(null)));
    let reply = t.answer(&RequestId("req_1".into()), Answer::Allow { always: true }).unwrap();
    assert_eq!(reply, PermissionReply::Allow { updated_input: json!({"command":"ls"}), updated_permissions: None });
}

#[test]
fn a_cancelled_request_is_withdrawn() {
    let mut t = Translator::new();
    t.push(request("req_1", "Bash", json!({"command":"ls"}), json!(null)));
    assert_eq!(t.push(Incoming::ControlCancel { request_id: "req_1".into() }),
        vec![SessionEvent::RequestWithdrawn { id: RequestId("req_1".into()) }]);
    assert!(t.answer(&RequestId("req_1".into()), Answer::Allow { always: false }).is_err());
    assert!(t.push(Incoming::ControlCancel { request_id: "nope".into() }).is_empty());
}
```

- [ ] **Step 1: Write the failing tests**

```rust
fn request(id: &str, tool: &str, input: serde_json::Value, suggestions: serde_json::Value) -> Incoming {
    Incoming::ControlRequest { request_id: id.into(), subtype: "can_use_tool".into(),
        body: json!({"subtype":"can_use_tool","tool_name":tool,"input":input,
                     "permission_suggestions":suggestions,"tool_use_id":"t1"}) }
}
fn project_rule() -> serde_json::Value {
    json!([{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"npm test:*"}],
            "behavior":"allow","destination":"localSettings"}])
}

#[test]
fn a_permission_request_needs_the_user_and_can_be_allowed_once() {
    let mut t = Translator::new();
    let events = t.push(request("req_1", "Bash", json!({"command":"npm test"}), project_rule()));
    assert_eq!(events, vec![SessionEvent::NeedsUser(UserRequest { id: RequestId("req_1".into()),
        kind: UserRequestKind::Permission { tool: "Bash".into(),
            detail: ToolDetail::Command { command: "npm test".into() },
            always_allow_label: Some("Always allow npm test:* in this project".into()) } })]);
    let reply = t.answer(&RequestId("req_1".into()), Answer::Allow { always: false }).unwrap();
    assert_eq!(reply, PermissionReply::Allow { updated_input: json!({"command":"npm test"}), updated_permissions: None });
    assert!(t.answer(&RequestId("req_1".into()), Answer::Allow { always: false }).is_err(), "answered once only");
}

#[test]
fn always_allow_sends_back_exactly_the_suggestion() {
    let mut t = Translator::new();
    t.push(request("req_2", "Bash", json!({"command":"npm test"}), project_rule()));
    let reply = t.answer(&RequestId("req_2".into()), Answer::Allow { always: true }).unwrap();
    assert_eq!(reply, PermissionReply::Allow { updated_input: json!({"command":"npm test"}),
                                              updated_permissions: Some(project_rule()) });
}

#[test]
fn deny_has_a_default_message() {
    let mut t = Translator::new();
    t.push(request("req_3", "Bash", json!({"command":"rm -rf /"}), json!(null)));
    assert_eq!(t.answer(&RequestId("req_3".into()), Answer::Deny { message: None }).unwrap(),
               PermissionReply::Deny { message: "Denied by the user".into() });
}

#[test]
fn labels_follow_the_suggestion() {
    let rule = |dest: &str| json!([{"type":"addRules","rules":[{"toolName":"WebFetch"}],"behavior":"allow","destination":dest}]);
    assert_eq!(always_allow_label(&rule("session")).as_deref(), Some("Always allow WebFetch for this session"));
    assert_eq!(always_allow_label(&rule("projectSettings")).as_deref(), Some("Always allow WebFetch in this project"));
    assert_eq!(always_allow_label(&rule("userSettings")).as_deref(), Some("Always allow WebFetch in all projects"));
    assert_eq!(always_allow_label(&rule("somewhereNew")), None);
    assert_eq!(always_allow_label(&json!([{"type":"setMode","mode":"acceptEdits","destination":"session"}])).as_deref(),
               Some("Allow all edits for this session"));
    assert_eq!(always_allow_label(&json!([])), None);
    assert_eq!(always_allow_label(&json!(null)), None);
    assert_eq!(always_allow_label(&json!([{"type":"addRules","rules":[{"toolName":"Bash"}],"behavior":"deny","destination":"session"}])), None);
}

#[test]
fn a_question_is_answered_by_adding_answers_to_the_input() {
    let input = json!({"questions":[{"question":"Which?","header":"Pick",
        "options":[{"label":"A","description":"a"},{"label":"B","description":"b"}],"multiSelect":false}]});
    let mut t = Translator::new();
    let events = t.push(request("req_4", "AskUserQuestion", input.clone(), json!(null)));
    assert_eq!(events, vec![SessionEvent::NeedsUser(UserRequest { id: RequestId("req_4".into()),
        kind: UserRequestKind::Question { questions: vec![Question { prompt: "Which?".into(),
            options: vec!["A".into(), "B".into()], multi_select: false }] } })]);
    assert!(t.answer(&RequestId("req_4".into()), Answer::Allow { always: false }).is_err(), "wrong kind of answer");
    let reply = t.answer(&RequestId("req_4".into()), Answer::Answers(vec![
        QuestionAnswer { prompt: "Which?".into(), chosen: vec!["A".into()], free_text: None }])).unwrap();
    let mut expected = input; expected["answers"] = json!({"Which?":"A"});
    assert_eq!(reply, PermissionReply::Allow { updated_input: expected, updated_permissions: None });
}

#[test]
fn a_plan_is_approved_or_sent_back() {
    let mut t = Translator::new();
    let events = t.push(request("req_5", "ExitPlanMode", json!({"plan":"# Plan"}), json!(null)));
    assert!(matches!(&events[0], SessionEvent::NeedsUser(UserRequest {
        kind: UserRequestKind::PlanApproval { plan_markdown }, .. }) if plan_markdown == "# Plan"));
    assert_eq!(t.answer(&RequestId("req_5".into()), Answer::KeepPlanning { feedback: None }).unwrap(),
               PermissionReply::Deny { message: "Keep planning".into() });
}

#[test]
fn other_control_requests_raise_nothing() {
    let mut t = Translator::new();
    let other = Incoming::ControlRequest { request_id: "req_6".into(), subtype: "hook_callback".into(), body: json!({}) };
    assert!(t.push(other).is_empty());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-claude translate`
Expected: the new tests FAIL to compile or fail.

- [ ] **Step 3: Implement the request handling, `answer` and `always_allow_label`**

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-claude translate`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(claude): permission, question and plan requests and their answers"
```

---

### Task 13: Usage limits, Remote Control and replies to our requests

**Files:**
- Modify: `crates/claude/src/translate.rs`

**Interfaces:**
- Produces:

```rust
pub enum Tracked { RemoteOn, RemoteOff, SetModel(String), SetMode(PermissionMode), Rename(String), Other }
impl Translator {
    pub fn track(&mut self, request_id: &str, what: Tracked);   // call before sending a control request
}
```

Rules:

- `RateLimit` gives `UsageLimits { session: five_hour, weekly: seven_day }` with `used` taken from utilization.
- `BridgeState`: `failed` gives `RemoteAccess::Failed { reason: detail or "Remote Control failed", needs_login: failure_kind == "auth" }`. `ready` gives `Starting`. `connected` gives `On { url }`, where `url` is the `session_url` remembered from the most recent successful `RemoteOn` reply, or `None`. Any other state name gives no event.
- `TitleChanged` gives `SessionEvent::TitleChanged`.
- A `ControlResponse` for a tracked id (which is then forgotten):

| Tracked | `ok` | Event |
| --- | --- | --- |
| `RemoteOn` | yes | `RemoteAccess(Starting)` (the bridge state lines report the rest) |
| `RemoteOn` | no | `RemoteAccess(Failed { reason: error, needs_login: error_code == "remote_control_auth" })` |
| `RemoteOff` | yes | `RemoteAccess(Off)` |
| `SetModel(m)` | yes | `ModelChanged(x)`, `x` being `body.model` when that is a string, else `m` |
| `SetMode(m)` | yes | `PermissionModeChanged(m)` |
| `Rename(t)` | yes | `TitleChanged(t)` |
| `Other` | either | nothing |
| any other | no | `CommandFailed { what, reason: error }`, `what` being `"remote access"`, `"model"`, `"permission mode"` or `"rename"` |

- A `ControlResponse` for an id that was never tracked gives nothing.

**Added after Task 9's live run (source: `docs/protocol-notes.md`). These rules and tests are part of this task. The text above was edited to agree with them.**

- No "stopped" bridge state exists: turning Remote Control off produces only the reply.
- A successful `RemoteOn` reply gives `Starting` as the table says, and the translator remembers `body.session_url` when it is a string.
- `ModelChanged`, `PermissionModeChanged` and `TitleChanged` are each given only when the value differs from the last one the translator reported or learned from `Init`. `session_title_changed` arrives before the reply to a rename, and `system`/`status` reports a mode change as well as the reply.

```rust
#[test]
fn remote_control_turns_on_as_recorded_live() {
    let mut t = Translator::new();
    t.track("probe_2", Tracked::RemoteOn);
    let ready = r#"{"type":"system","subtype":"bridge_state","state":"ready","session_id":"s1"}"#;
    let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"probe_2","response":{"session_url":"https://claude.ai/code/session_X","bridge_epoch":1}}}"#;
    let connected = r#"{"type":"system","subtype":"bridge_state","state":"connected","bridge_epoch":1,"session_id":"s1"}"#;
    let mut events = Vec::new();
    for l in [ready, reply, connected] { events.extend(t.push(parse_line(l))); }
    assert_eq!(events, vec![
        SessionEvent::RemoteAccess(RemoteAccess::Starting),
        SessionEvent::RemoteAccess(RemoteAccess::Starting),
        SessionEvent::RemoteAccess(RemoteAccess::On { url: Some("https://claude.ai/code/session_X".into()) })]);
}

#[test]
fn a_rename_is_reported_once() {
    let mut t = Translator::new();
    t.track("probe_2", Tracked::Rename("Probe rename B".into()));
    let changed = r#"{"type":"system","subtype":"session_title_changed","title":"Probe rename B","session_id":"s1"}"#;
    let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"probe_2"}}"#;
    let mut events = t.push(parse_line(changed));
    events.extend(t.push(parse_line(reply)));
    assert_eq!(events, vec![SessionEvent::TitleChanged("Probe rename B".into())]);
}

#[test]
fn a_model_change_reports_the_resolved_name() {
    let mut t = Translator::new();
    t.track("probe_2", Tracked::SetModel("sonnet".into()));
    let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"probe_2","response":{"model":"claude-sonnet-5-5"}}}"#;
    assert_eq!(t.push(parse_line(reply)), vec![SessionEvent::ModelChanged("claude-sonnet-5-5".into())]);
}
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn usage_limits_from_the_live_recording() {
    let l = r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":0.08,"resetsAt":1794385200},"seven_day":{"utilization":0.06,"resetsAt":1794776400}}}}"#;
    assert_eq!(feed(&[l]), vec![SessionEvent::UsageLimits(UsageLimits {
        session: Some(LimitWindow { used: 0.08, resets_at_unix: 1794385200 }),
        weekly: Some(LimitWindow { used: 0.06, resets_at_unix: 1794776400 }) })]);
}

#[test]
fn a_refused_remote_control_request_says_a_login_is_needed() {
    let mut t = Translator::new();
    t.track("rc_1", Tracked::RemoteOn);
    let refused = parse_line(r#"{"type":"control_response","response":{"subtype":"error","request_id":"rc_1","error":"Claude.ai login was rejected","error_code":"remote_control_auth"}}"#);
    assert_eq!(t.push(refused.clone()), vec![SessionEvent::RemoteAccess(RemoteAccess::Failed {
        reason: "Claude.ai login was rejected".into(), needs_login: true })]);
    assert!(t.push(refused).is_empty(), "a tracked id is used once");
}

#[test]
fn a_failed_bridge_state_is_a_failure() {
    let l = r#"{"type":"system","subtype":"bridge_state","state":"failed","detail":"Claude.ai login was rejected","failure_kind":"auth"}"#;
    assert_eq!(feed(&[l]), vec![SessionEvent::RemoteAccess(RemoteAccess::Failed {
        reason: "Claude.ai login was rejected".into(), needs_login: true })]);
    assert!(feed(&[r#"{"type":"system","subtype":"bridge_state","state":"a-state-nobody-has-seen"}"#]).is_empty());
}

#[test]
fn accepted_requests_confirm_what_was_asked() {
    let ok = |id: &str| parse_line(&format!(r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{id}"}}}}"#));
    let mut t = Translator::new();
    t.track("a", Tracked::RemoteOn);
    t.track("b", Tracked::RemoteOff);
    t.track("c", Tracked::SetModel("opus".into()));
    t.track("d", Tracked::SetMode(PermissionMode::Plan));
    t.track("e", Tracked::Rename("New".into()));
    assert_eq!(t.push(ok("a")), vec![SessionEvent::RemoteAccess(RemoteAccess::Starting)]);
    assert_eq!(t.push(ok("b")), vec![SessionEvent::RemoteAccess(RemoteAccess::Off)]);
    assert_eq!(t.push(ok("c")), vec![SessionEvent::ModelChanged("opus".into())]);
    assert_eq!(t.push(ok("d")), vec![SessionEvent::PermissionModeChanged(PermissionMode::Plan)]);
    assert_eq!(t.push(ok("e")), vec![SessionEvent::TitleChanged("New".into())]);
    assert!(t.push(ok("never-tracked")).is_empty());
}

#[test]
fn a_refused_model_change_is_reported() {
    let mut t = Translator::new();
    t.track("m", Tracked::SetModel("nope".into()));
    let refused = parse_line(r#"{"type":"control_response","response":{"subtype":"error","request_id":"m","error":"unknown model"}}"#);
    assert_eq!(t.push(refused), vec![SessionEvent::CommandFailed { what: "model".into(), reason: "unknown model".into() }]);
}
```


- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-claude translate`
Expected: the new tests fail.

- [ ] **Step 3: Implement the rules**

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-claude`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(claude): usage limits, remote access states and control replies"
```

---

### Task 14: Account state

**Files:**
- Create: `crates/claude/src/account.rs`, `crates/claude/tests/stub/auth_stub.sh` (executable)
- Modify: `crates/claude/src/lib.rs`

**Interfaces:**
- Consumes: `child_env` (Task 5), `mr_agent::Account`.
- Produces:

```rust
pub struct StatusInfo { pub logged_in: bool, pub method: String,
                        pub email: Option<String>, pub subscription: Option<String> }
pub fn parse_status(json: &str) -> StatusInfo;          // unparseable input reads as logged out
pub fn plan_label(subscription: Option<&str>) -> Option<String>;
pub fn detect(program: &Path, parent_env: Vec<(String, String)>) -> Account;
```

`detect` runs `<program> auth status` up to twice and blocks while it does, so callers use a worker thread:

1. With `child_env(parent_env, true)`, which removes the token. Logged in means a saved full login exists: `SignedIn { full_login: true }`.
2. Otherwise with `child_env(parent_env, false)`. Logged in means the token is the only sign-in: `SignedIn { full_login: false }`.
3. Otherwise `SignedOut`. If the program cannot be started at all, `Missing`.

`name` is the status's e-mail when present, else `"Signed in"`. `plan` is `plan_label(subscription)`: `max` → `Max plan`, `pro` → `Pro plan`, `team` → `Team plan`, `enterprise` → `Enterprise plan`, anything else → `None`. The field names for e-mail and subscription are the ones recorded in `docs/protocol-notes.md` item V10; the tests below assume `email` and `subscriptionType`.

`tests/stub/auth_stub.sh` prints a status JSON chosen by the variable `STUB_AUTH`: `full` always prints logged in with `"authMethod":"claude.ai","email":"user@example.com","subscriptionType":"max"`; `token` prints logged in with `"authMethod":"oauth_token"` only when `CLAUDE_CODE_OAUTH_TOKEN` is set, else logged out; `none` always prints logged out.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_two_status_outputs_seen_live() {
    let out = parse_status(r#"{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}"#);
    assert!(!out.logged_in);
    let token = parse_status(r#"{"loggedIn": true, "authMethod": "oauth_token", "apiProvider": "firstParty"}"#);
    assert!(token.logged_in);
    assert_eq!(token.method, "oauth_token");
    assert_eq!(token.email, None);
    assert!(!parse_status("garbage").logged_in);
}

#[test]
fn plan_labels() {
    assert_eq!(plan_label(Some("max")).as_deref(), Some("Max plan"));
    assert_eq!(plan_label(Some("pro")).as_deref(), Some("Pro plan"));
    assert_eq!(plan_label(Some("mystery")), None);
    assert_eq!(plan_label(None), None);
}

fn detect_with(mode: &str, token: bool) -> Account {
    let mut env = vec![("PATH".to_string(), std::env::var("PATH").unwrap()), ("STUB_AUTH".to_string(), mode.to_string())];
    if token { env.push(("CLAUDE_CODE_OAUTH_TOKEN".to_string(), "t".to_string())); }
    detect(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/stub/auth_stub.sh")), env)
}

#[test]
fn a_saved_login_is_a_full_login() {
    assert_eq!(detect_with("full", true), Account::SignedIn {
        name: "user@example.com".into(), plan: Some("Max plan".into()), full_login: true });
}

#[test]
fn a_token_alone_signs_in_without_a_full_login() {
    assert_eq!(detect_with("token", true), Account::SignedIn { name: "Signed in".into(), plan: None, full_login: false });
    assert_eq!(detect_with("token", false), Account::SignedOut);
}

#[test]
fn nothing_is_signed_out_and_no_program_is_missing() {
    assert_eq!(detect_with("none", true), Account::SignedOut);
    assert_eq!(detect(Path::new("/nonexistent/claude"), vec![]), Account::Missing);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-claude account`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `parse_status`, `plan_label` and `detect`**

Use `std::process::Command` with `env_clear()`, stdin closed, and a 10 second limit; a run that exceeds it, or prints nothing, counts as logged out.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-claude account`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(claude): account state from claude auth status"
```

---

### Task 15: Stored sessions

**Files:**
- Create: `crates/claude/src/history.rs`
- Modify: `crates/claude/src/lib.rs`

**Interfaces:**
- Consumes: `tool_detail` (Task 11), `mr_agent::{StoredSession, HistoryItem, SessionId, AgentError}`.
- Produces:

```rust
pub struct StoredEntry { pub id: SessionId, pub path: PathBuf, pub modified_unix: i64, pub bytes: u64 }
pub struct StoredMeta { pub folder: Option<PathBuf>, pub title: Option<String> }
pub fn list_entries(projects_dir: &Path) -> Vec<StoredEntry>;
pub fn describe(path: &Path) -> StoredMeta;
pub fn stored_sessions(projects_dir: &Path, modified_since_unix: Option<i64>) -> Vec<StoredSession>;
pub fn find_session(projects_dir: &Path, id: &SessionId) -> Option<PathBuf>;
pub fn read_history(path: &Path) -> Result<Vec<HistoryItem>, AgentError>;
```

Layout on disk (spec section 8): `<projects_dir>/<folder name with slashes as dashes>/<session id>.jsonl`.

Rules:

- `list_entries` looks only at `*.jsonl` files directly inside each immediate subdirectory of `projects_dir`. It reads no file contents. A missing `projects_dir` gives an empty list.
- `describe` reads at most the first 262,144 bytes and the last 262,144 bytes of the file (the whole file when it is smaller), and within each region only complete lines. `folder` is the `cwd` of the first record that has one. `title` is the `aiTitle` of the last `ai-title` record found; failing that, the text of the first `user` record that is neither `isMeta` nor `isSidechain`, trimmed, cut to 80 characters; failing that, `None`.
- `stored_sessions` describes each entry at least as new as `modified_since_unix` (all of them for `None`), newest first. A session with no `folder` is left out. A missing title becomes the first 8 characters of the id.
- `read_history` reads line by line and never fails on a bad line: a line that is not complete JSON is skipped. Skipped too: records with `isSidechain` or `isMeta` true, and `user` text that starts with `<local-command-` or `<command-name>`.
  - `user` with string content, or text blocks → `User { text, image_count }`, counting `image` blocks.
  - `user` `tool_result` blocks → set the `outcome` of the earlier `Tool` item with that id.
  - `assistant` blocks → `AssistantText`, `Thinking` (skipping empty ones) and `Tool` with `outcome: None`.
  - every other record type is ignored.

- [ ] **Step 1: Write the failing tests** in `crates/claude/src/history.rs`

A helper `write(dir, "-home-u-proj", "s1", &[lines])` creates the project subdirectory in a `tempfile::TempDir` and writes the lines joined with newlines, returning the file path.

```rust
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
    let path = write(dir.path(), "-home-u-proj", "s1", &[USER, older, ASSISTANT, TITLE]);
    assert_eq!(describe(&path).folder, Some("/home/u/proj".into()));
    assert_eq!(describe(&path).title.as_deref(), Some("Fix flaky reconnect"));
}

#[test]
fn without_a_title_the_first_message_names_it_cut_to_eighty_characters() {
    let dir = tempfile::tempdir().unwrap();
    let long = format!(r#"{{"type":"user","cwd":"/p","message":{{"content":"{}"}}}}"#, "x".repeat(200));
    let path = write(dir.path(), "-p", "s2", &[&long]);
    assert_eq!(describe(&path).title.unwrap().chars().count(), 80);
}

#[test]
fn a_title_at_the_end_of_a_large_file_is_still_found() {
    let dir = tempfile::tempdir().unwrap();
    let filler = format!(r#"{{"type":"api-request-blob","message":"{}"}}"#, "y".repeat(4000));
    let mut lines = vec![USER.to_string()];
    lines.extend(std::iter::repeat(filler).take(300));      // about 1.2 MB
    lines.push(TITLE.to_string());
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let path = write(dir.path(), "-home-u-proj", "s3", &refs);
    assert_eq!(describe(&path).title.as_deref(), Some("Fix flaky reconnect"));
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
    assert_eq!(find_session(dir.path(), &SessionId("s1".into())).unwrap().file_name().unwrap(), "s1.jsonl");
    assert!(find_session(dir.path(), &SessionId("zzz".into())).is_none());
}

#[test]
fn a_conversation_is_read_in_order_with_tool_outcomes_attached() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "-home-u-proj", "s1", &[USER, ASSISTANT, TOOL_DONE, TITLE]);
    assert_eq!(read_history(&path).unwrap(), vec![
        HistoryItem::User { text: "Fix the reconnect bug please".into(), image_count: 0 },
        HistoryItem::Thinking("hm".into()),
        HistoryItem::AssistantText("On it.".into()),
        HistoryItem::Tool { id: ToolCallId("t1".into()), name: "Bash".into(),
            detail: ToolDetail::Command { command: "ls".into() },
            outcome: Some(ToolOutcome::Ok { output: "a.txt".into() }) },
    ]);
}

#[test]
fn side_chains_meta_records_and_command_wrappers_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let side = r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"inner"}]}}"#;
    let meta = r#"{"type":"user","isMeta":true,"message":{"content":"meta"}}"#;
    let wrapper = r#"{"type":"user","message":{"content":"<command-name>/model</command-name>"}}"#;
    let path = write(dir.path(), "-p", "s4", &[side, meta, wrapper, USER]);
    assert_eq!(read_history(&path).unwrap().len(), 1);
}

#[test]
fn a_cut_off_last_line_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "-p", "s5", &[USER, r#"{"type":"assistant","message":{"content":[{"type":"te"#]);
    assert_eq!(read_history(&path).unwrap().len(), 1);
    assert!(matches!(read_history(Path::new("/nonexistent.jsonl")), Err(AgentError::Io(_))));
}

#[test]
fn images_in_a_message_are_counted() {
    let dir = tempfile::tempdir().unwrap();
    let with_image = r#"{"type":"user","message":{"content":[{"type":"text","text":"see"},{"type":"image","source":{}}]}}"#;
    let path = write(dir.path(), "-p", "s6", &[with_image]);
    assert_eq!(read_history(&path).unwrap(), vec![HistoryItem::User { text: "see".into(), image_count: 1 }]);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-claude history`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the five functions**

- [ ] **Step 4: Run the tests, then try it on the real history (read-only)**

Run: `cargo test -p mr-claude history`
Expected: PASS.

Then add a temporary `#[ignore]` test that calls `stored_sessions(~/.claude/projects, None)` and prints the count and the elapsed time; run it once with `-- --ignored --nocapture`. Expected: over 1,000 sessions listed in well under 10 seconds. Record the numbers in `docs/protocol-notes.md` under "Machine notes" and delete the temporary test. If it takes longer, stop and report; plan 3's sidebar depends on this being quick.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(claude): list, describe and read stored sessions"
```

---

### Task 16: ClaudeAgent

**Files:**
- Create: `crates/claude/src/agent.rs`, `crates/claude/tests/agent.rs`, `crates/claude/tests/live.rs`, `crates/claude/tests/stub/fake_claude.sh` (executable)
- Modify: `crates/claude/src/lib.rs`, `crates/probe/src/main.rs`

**Interfaces:**
- Consumes: everything in `mr-claude` so far.
- Produces:

```rust
pub struct ClaudeAgent { /* .. */ }
impl ClaudeAgent {
    pub fn from_environment() -> Self;   // program "claude" found on PATH, ~/.claude/projects, std::env::vars()
    pub fn with_paths(program: PathBuf, projects_dir: PathBuf, parent_env: Vec<(String, String)>) -> Self;
}
impl Agent for ClaudeAgent { /* .. */ }
```

and a third probe subcommand, `mindrune-probe events --cwd DIR [--model M]`, which opens a session through `ClaudeAgent` and prints each `SessionEvent` with `{:?}`, reading `say TEXT`, `stop`, `rc on|off` and `quit` from stdin.

Behaviour:

- `capabilities` is all true. `account` calls `account::detect`. `list_sessions` and `read_session` call Task 15 (`read_session` uses `find_session`; not found is `AgentError::Protocol`).
- `open` generates a UUID v4 for a new session, builds arguments and environment, spawns in `spec.folder`, and at once sends `initialize` as request `rc_1`. `has_full_login` comes from the last `account()` result, which the agent caches; `open` computes it first if `account()` has never been called. Every call to `account()` checks again and refreshes the cache.
- The reader thread writes one line to stderr, prefixed `mindrune:`, the first time each distinct `Ignored` kind is seen in a session, and nothing after that.
- The reader thread parses each line and pushes it through a `Translator` shared with the handle behind a mutex, then calls the sink once per event, in order, with the mutex released.
- An incoming `ControlRequest` whose subtype is not `can_use_tool` is answered with `unsupported_reply`.
- When the process exits, the sink gets `Exited { clean: code == Some(0), detail: stderr_tail }`.
- `SessionHandle::send`:

| Command | Written to claude |
| --- | --- |
| `Send` | `user_message` |
| `Answer` | `translator.answer(..)` then `permission_reply`; an error from `answer` is returned and nothing is written |
| `Stop` | `interrupt` |
| `SetModel`, `SetPermissionMode`, `SetRemoteAccess`, `Rename` | the matching control request, tracked first with `Translator::track` |
| `Close` | close input; kill the process if it has not exited after 2 seconds |

- Control request ids are `rc_1`, `rc_2`, … per session. Any command after the process has exited returns `AgentError::Closed`.
- Task 9 found that `rename_session` is accepted, so `Rename` sends it like any other tracked request.

`tests/stub/fake_claude.sh` ignores its arguments and reads stdin line by line:

- a line containing `"subtype":"initialize"`: print a success `control_response` for its `request_id`;
- a line containing `please-die`, checked before every other rule: write `stub died` to stderr and exit 7;
- a line containing `"type":"user"`: print, in order, the lines held in the constants `INIT`, `MSG_START`, `TEXT_START`, `DELTA_OK`, `BLOCK_STOP` and `ASSISTANT_OK` in the tests of `crates/claude/src/translate.rs`, the rate-limit line from the test `usage_limits_from_the_live_recording` in the same file, a `control_request` `req_h` with subtype `hook_callback`. It does **not** print the turn result yet;
- a line containing `"subtype":"remote_control"`: print an error `control_response` for its `request_id` with `"error":"Claude.ai login was rejected"` and `"error_code":"remote_control_auth"`;
- a line containing `"subtype":"error"` and `req_h`: print the line in the constant `RESULT_OK`. The turn therefore finishes only if the adapter refused the hook request, which is what the first test's `TurnFinished` proves (changed in review: the earlier marker-line check could not fail);
- end of input: exit 0.

It extracts `request_id` with `sed`.

- [ ] **Step 1: Write the stub and the failing tests** in `crates/claude/tests/agent.rs`

A helper `open_stub() -> (Box<dyn SessionHandle>, Receiver<SessionEvent>)` builds `ClaudeAgent::with_paths(<fake_claude.sh>, <a temp dir>, vec![("PATH", ..)])`, opens a session in a temp folder and forwards events into a channel. `next(&rx)` is `recv_timeout(5 s)`.

```rust
#[test]
fn a_turn_arrives_as_agent_events() {
    let (handle, rx) = open_stub();
    handle.send(SessionCommand::Send { text: "hi".into(), attachments: vec![] }).unwrap();
    assert!(matches!(next(&rx), SessionEvent::Started(info) if info.model == "claude-haiku-5-5"));
    assert_eq!(next(&rx), SessionEvent::TurnStarted);
    assert_eq!(next(&rx), SessionEvent::TextDelta("ok".into()));
    assert_eq!(next(&rx), SessionEvent::BlockFinished);
    assert!(matches!(next(&rx), SessionEvent::UsageLimits(u) if u.session.unwrap().percent() == 8));
    assert!(matches!(next(&rx), SessionEvent::TurnFinished(s) if s.context_used == Some(0.05)));
}

#[test]
fn a_refused_remote_control_request_comes_back_as_a_failure() {
    let (handle, rx) = open_stub();
    handle.send(SessionCommand::SetRemoteAccess { on: true }).unwrap();
    assert_eq!(next(&rx), SessionEvent::RemoteAccess(RemoteAccess::Failed {
        reason: "Claude.ai login was rejected".into(), needs_login: true }));
}

#[test]
fn answering_a_request_that_does_not_exist_is_an_error_and_writes_nothing() {
    let (handle, _rx) = open_stub();
    let bad = SessionCommand::Answer { id: RequestId("nope".into()), answer: Answer::Allow { always: false } };
    assert!(matches!(handle.send(bad), Err(AgentError::Protocol(_))));
}

#[test]
fn closing_ends_the_session_cleanly() {
    let (handle, rx) = open_stub();
    handle.send(SessionCommand::Close).unwrap();
    assert_eq!(next(&rx), SessionEvent::Exited { clean: true, detail: String::new() });
    assert!(matches!(handle.send(SessionCommand::Stop), Err(AgentError::Closed)));
}

#[test]
fn a_dying_process_reports_why_and_closes_the_handle() {
    let (handle, rx) = open_stub();
    handle.send(SessionCommand::Send { text: "please-die".into(), attachments: vec![] }).unwrap();
    assert!(matches!(next(&rx), SessionEvent::Exited { clean: false, detail } if detail.contains("stub died")));
    assert!(matches!(handle.send(SessionCommand::Stop), Err(AgentError::Closed)));
}

#[test]
fn a_missing_claude_cannot_open_a_session() {
    let agent = ClaudeAgent::with_paths("/nonexistent/claude".into(), std::env::temp_dir(), vec![]);
    assert!(matches!(agent.open(StartSpec::new(std::env::temp_dir()), Box::new(|_| {})), Err(AgentError::NotInstalled)));
    assert_eq!(agent.account(), Account::Missing);
}
```

`crates/claude/tests/live.rs`, opt-in:

```rust
#[test]
#[ignore = "starts the real claude; run with MINDRUNE_LIVE=1"]
fn one_real_turn_on_the_cheapest_model() {
    if std::env::var("MINDRUNE_LIVE").as_deref() != Ok("1") { return; }
    let keep = ["HOME", "PATH", "USER", "LANG", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS", "CLAUDE_CODE_OAUTH_TOKEN"];
    let env: Vec<(String, String)> = std::env::vars().filter(|(k, _)| keep.contains(&k.as_str())).collect();
    let agent = ClaudeAgent::with_paths("claude".into(), dirs_projects(), env);
    let dir = tempfile::tempdir().unwrap();
    let mut spec = StartSpec::new(dir.path());
    spec.model = Some("haiku".into());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = agent.open(spec, Box::new(move |e| { let _ = tx.send(e); })).unwrap();
    handle.send(SessionCommand::Send { text: "Reply with the single word: ok".into(), attachments: vec![] }).unwrap();
    let mut saw_limits = false;
    loop {
        match rx.recv_timeout(std::time::Duration::from_secs(120)).expect("no event in two minutes") {
            SessionEvent::UsageLimits(_) => saw_limits = true,
            SessionEvent::TurnFinished(_) => break,
            SessionEvent::TurnFailed { reason } => panic!("turn failed: {reason}"),
            SessionEvent::Exited { detail, .. } => panic!("claude exited: {detail}"),
            _ => {}
        }
    }
    assert!(saw_limits, "usage limits should arrive during a turn");
    handle.send(SessionCommand::Close).unwrap();
}
```

`dirs_projects()` returns `$HOME/.claude/projects`. `ClaudeAgent::with_paths` given the bare name `claude` resolves it on the `PATH` in the environment it was given.

Add to `a_turn_arrives_as_agent_events`, after the last assertion, a check that the stub received the unsupported reply. The stub's marker line is an ignored `system` record, so assert it indirectly: send a second `Send` and confirm a second `TurnFinished` arrives, which shows the reader did not stall on the unanswered request.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mr-claude --test agent`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `ClaudeAgent`, its session handle and the probe's `events` subcommand**

The handle and the reader thread share `Arc<Mutex<Translator>>` and the `ClaudeProcess`. Never hold the translator's mutex while calling the sink or while writing to the process.

- [ ] **Step 4: Run all tests**

Run: `cargo fmt --all && cargo test --workspace`
Expected: PASS, with `live.rs` reported as ignored.

- [ ] **Step 5: Run the live test and the probe once (in Jerod's terminal, not a sandboxed shell)**

Run: `MINDRUNE_LIVE=1 cargo test -p mr-claude --test live -- --ignored --nocapture`
Expected: PASS in under a minute.

Run: `cargo run -q -p mr-probe -- events --cwd target/probe-cwd --model haiku`, type `say Reply with the single word: ok`, then `quit`.
Expected: `Started(..)`, `TurnStarted`, one or more `TextDelta(..)`, `BlockFinished`, `UsageLimits(..)`, `TurnFinished(..)`, then `Exited { clean: true, .. }`.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(claude): ClaudeAgent drives real sessions through the agent trait"
```

---

## When this plan is done

- `cargo test --workspace` passes; the opt-in live test passes on Jerod's machine.
- `mindrune-probe events` holds a real conversation and prints agent-neutral events.
- `docs/protocol-notes.md` records what was verified live, including Remote Control.
- Plan 2 can start. It builds app state and the first window against `FakeAgent`, and swaps in `ClaudeAgent` only in `main.rs`.
