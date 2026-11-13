# One Session in a Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `mindrune [FOLDER]` opens a window in which one real Claude Code session can be used from start to finish: type, watch the reply stream as Markdown, see tool calls, answer permission requests, questions and plans, switch model and mode, stop, and always see the usage limits.

**Architecture:** A new crate `mr-core` holds everything the window knows, as plain data with transitions that return effects: the conversation, one session's life, the app's screens and usage text. It also holds `Backend`, the one worker thread that owns the `Agent` and turns its events into messages for the window. The app crate `mindrune` becomes Rune views over that state plus the glue that carries effects out. All window tests and screenshots run against `FakeAgent`; only `main.rs` names `ClaudeAgent`.

**Tech Stack:** Rust 1.99.1 (edition 2024), `rune`, `rune-kit` (feature `markdown`) and `rune-theme-opencode` (icons and the monospace font only) at the pinned revision, `serde`, `similar` for diffs, std threads and channels, `tempfile` for tests.

**Spec:** `docs/superpowers/specs/2026-11-10-claude-code-ui-design.md`. This plan implements sections 5.3, 5.4, 5.5 (without attachments), 5.9, 5.10, the usage footer of 5.1 and the parts of section 10 that concern one session. Read these before starting, in this order: the spec, `docs/notes-for-later-plans.md`, `docs/protocol-notes.md`.

## This is plan 2 of 4

| Plan | Delivers |
| --- | --- |
| 1. Foundations and Claude adapter (done) | Workspace, agent seam, fake agent, Claude adapter, live protocol verification |
| **2. One session in a window (this plan)** | App state, the shell, one tile: conversation, request cards, input box with model and mode pickers, conversation width, usage footer, not-signed-in screen |
| 3. Many sessions | Sidebar with groups, stored sessions and their history, tiles side by side, saved layout, state marks |
| 4. Attachments, Remote Control, background running | Paste and drop, right panel, Remote Control switch, keep running after the window closes, quit rules |

**Not in this plan, on purpose.** Stored sessions and history from disk, groups, tiles and tile headers (plan 3). Attachments, the right panel, Remote Control, tray and background running (plan 4). From the seam table in `docs/notes-for-later-plans.md`: the answering model per message, declining a question (Stop is the way out), and tool input while it streams are left out; splitting `translate.rs` into a module waits for plan 4, which changes it most. Text selection inside the conversation is whatever the kit's Markdown gives; copy controls on code blocks and messages are in.

## Global Constraints

- Rust toolchain is exactly `1.99.1`, edition `2024`. In an agent's shell cargo is not on `PATH`: start every command with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Rune is pinned to `rev = "2a44156b21d63b0342a366b26d9222a6edd4622c"` of `https://github.com/anomalyco/rune`. `rune` and `rune-kit` always use the same revision. Its source is at `~/.cargo/git/checkouts/rune-c9b805e9ed8a3730/2a44156`; read it instead of guessing an API.
- `mr-agent` and `mr-core` depend on neither `mr-claude` nor `rune`. No type from `mr-claude` appears in `mr-core` or in any view. Only `crates/app/src/main.rs` and `crates/probe` may name `ClaudeAgent`.
- The UI thread never blocks. `Agent::open`, `Agent::account` and `SessionHandle::send` are called only on the `Backend` worker thread.
- A session's events are trusted only while that session is the current one. Everything from the backend carries the number of the `open` it belongs to, and a stale number is dropped.
- Request buttons (Allow once, Always allow, Deny, question options, Approve, Keep planning) never take keyboard focus and never react to Enter or Space typed in the input box.
- The software never writes inside `~/.claude`. Its own settings go through `rune::prefs::Prefs::open("mindrune")`. Tests and headless runs set `RUNE_DIRS_ROOT` to a temporary directory or use `Prefs::memory()`.
- No Anthropic logos, colours or fonts. The theme is the kit's neutral one and follows the system's light or dark setting.
- User-visible wording is exact where this plan quotes it. Never use a real person's name or e-mail address as test data; use `alex`, `alex@example.org`, `/home/alex`.
- All window tests and all screenshots use `FakeAgent`, through the demo window of Task 10. Tests that start the real `claude` stay opt-in behind `MINDRUNE_LIVE=1` and `#[ignore]`.
- Every key this app gives an element starts with `mr-`. Rune's keys are one namespace and the kit uses plain names such as `label`, `input` and `scroll`.
- A view never does work that grows with the length of the conversation: rows are virtual, and a finished row is wrapped in `rune::memo`.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` (no warnings) and `cargo test --workspace` pass before every commit.
- **Git.** Jerod removed the git data on 2026-11-11 and sets the repository up themself. Before Task 1, run `git rev-parse --is-inside-work-tree`. If it fails, stop and ask Jerod; never run `git init`. Never commit `target/` or `.superpowers/`. Commit by explicit path, not `git add -A`.
- Version of `claude` this plan was written against: `2.1.319`.

## Review Focus

The spec implies these conditions and no screen in it shows them. Each has a test in the task named.

1. **A very long conversation or one enormous reply.** Twenty thousand text pieces in one reply, or two thousand items. Expected: one reply item, no slowdown that grows with length, and the window builds only the rows on screen. Tests: Task 6, Task 12.
2. **Events from a session that was just replaced.** The user presses "New session" in the middle of a turn; the old session's last events arrive afterwards. Expected: they change nothing. Tests: Task 8, Task 9.
3. **A request arrives while the user is typing.** The user presses Enter a moment after a permission card appears. Expected: the text is queued as a message; the request is still waiting. Tests: Task 7, Task 14, Task 15.
4. **The program dies while a request is waiting.** Expected: the card stops offering buttons, the conversation says what happened and offers Restart, and nothing is sent to the dead session. Tests: Task 6, Task 7, Task 14.
5. **Hostile or broken text.** A reply that ends inside an open code fence, tool output of 50,000 lines or one line of a megabyte, a user message full of Markdown. Expected: no panic, long output is cut with a "Show all" control, and the user's own message is shown as plain text. Tests: Task 5, Task 12, Task 13.

## File Structure

```
Cargo.toml                         adds similar to workspace dependencies
crates/agent/src/                  Tasks 1, 2: event.rs, command.rs, agent.rs, fake.rs
crates/claude/src/                 Tasks 3, 4: translate.rs, encode.rs, agent.rs, time.rs (new)
crates/core/                       package mr-core (new)
  src/lib.rs                       re-exports
  src/conversation.rs              Conversation, Item, ItemBody: what a session has said (Task 6)
  src/tools.rs                     ToolGroup, folding, one-line summaries, clip, diff counts (Task 5)
  src/session.rs                   Session, Phase, the user's intents (Task 7)
  src/app.rs                       App, Screen, Width, Effect, stale-open rule (Task 8)
  src/usage.rs                     usage lines and reset wording (Task 8)
  src/backend.rs                   Backend worker thread, ToBackend, FromBackend (Task 9)
crates/app/                        package mindrune
  src/main.rs                      arguments, the one place that builds ClaudeAgent (Tasks 10, 17)
  src/glue.rs                      Ui state, carrying effects out, poll (Task 10)
  src/demo.rs                      scripted FakeAgent scenes for tests and screenshots (Tasks 10 to 17)
  src/testkit.rs                   test helpers: open a scene, click, type, read texts (Task 10)
  src/theme.rs                     colours and sizes (Task 11)
  src/view/mod.rs                  root view, which screen (Task 11)
  src/view/shell.rs                sidebar with footer, main area (Task 11)
  src/view/conversation.rs         the item list, following the latest (Task 12)
  src/view/items.rs                reply, user message, thinking, tool lines, notices (Task 13)
  src/view/requests.rs             permission, question and plan cards (Task 14)
  src/view/input.rs                input box, send and stop, pickers, queued message (Task 15)
  src/view/width.rs                the column and its drag handles (Task 16)
  src/view/signin.rs               not signed in, not installed (Task 11)
  tests/snapshots.rs               one screenshot per main screen (Task 17)
  tests/live.rs                    opt-in: one real turn through App and Backend (Task 17)
```

---

### Task 1: The fake agent behaves like the real one at the edges

Plan 1's last review found two wrong sentences and three places where `FakeAgent` is kinder than `ClaudeAgent`. The window is tested only against the fake, so the fake must be as strict.

**Files:**
- Modify: `crates/agent/src/agent.rs` (doc comment on `Agent::open`), `crates/agent/src/event.rs` (the "Order" doc comment), `crates/agent/src/fake.rs`

**Interfaces:**
- Produces: `FakeAgent::fail_next_open(&self, detail: &str)`. Unchanged signatures otherwise.

- [ ] **Step 1: Write the failing tests** in `crates/agent/src/fake.rs`, using the existing `collect()` helper

```rust
#[test]
fn a_session_that_dies_while_opening_gives_only_exited() {
    let fake = FakeAgent::new();
    fake.fail_next_open("no conversation found with that id");
    let (seen, sink) = collect();
    let handle = fake.open(StartSpec::new("/p"), sink).unwrap();
    fake.advance(0.0);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![SessionEvent::Exited { clean: false, detail: "no conversation found with that id".into() }]
    );
    assert_eq!(handle.send(SessionCommand::Stop), Err(AgentError::Closed));
    // Only the next open fails.
    let (seen, sink) = collect();
    fake.open(StartSpec::new("/p"), sink).unwrap();
    fake.advance(0.0);
    assert!(matches!(seen.lock().unwrap()[0], SessionEvent::Started(_)));
}

#[test]
fn a_second_close_is_accepted_until_the_exit_is_delivered() {
    let fake = FakeAgent::new();
    let (_seen, sink) = collect();
    let handle = fake.open(StartSpec::new("/p"), sink).unwrap();
    fake.advance(0.0);
    assert_eq!(handle.send(SessionCommand::Close), Ok(()));
    assert_eq!(handle.send(SessionCommand::Close), Ok(()));
    assert_eq!(handle.send(SessionCommand::Stop), Err(AgentError::Closed));
    fake.advance(0.0);
    assert_eq!(handle.send(SessionCommand::Close), Err(AgentError::Closed));
    let closes = fake.sent().iter().filter(|(_, c)| *c == SessionCommand::Close).count();
    assert_eq!(closes, 1);
}

#[test]
fn an_answer_is_refused_unless_its_request_is_waiting() {
    let fake = FakeAgent::new();
    let (seen, sink) = collect();
    let handle = fake.open(StartSpec::new("/p"), sink).unwrap();
    fake.advance(0.0);
    let id = started(&seen).id;
    let answer = |n: &str| SessionCommand::Answer { id: RequestId(n.into()), answer: Answer::Allow { always: false } };
    assert!(matches!(handle.send(answer("r1")), Err(AgentError::Protocol(_))));
    let request = UserRequest {
        id: RequestId("r1".into()),
        kind: UserRequestKind::PlanApproval { plan_markdown: "p".into() },
    };
    fake.emit(&id, SessionEvent::NeedsUser(request));
    fake.advance(0.0);
    assert_eq!(handle.send(answer("r1")), Ok(()));
    assert!(matches!(handle.send(answer("r1")), Err(AgentError::Protocol(_))));
    // A withdrawn request is no longer waiting either.
    let again = UserRequest { id: RequestId("r2".into()), kind: UserRequestKind::PlanApproval { plan_markdown: "p".into() } };
    fake.emit(&id, SessionEvent::NeedsUser(again));
    fake.emit(&id, SessionEvent::RequestWithdrawn { id: RequestId("r2".into()) });
    fake.advance(0.0);
    assert!(matches!(handle.send(answer("r2")), Err(AgentError::Protocol(_))));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p mr-agent`
Expected: FAIL to compile, `fail_next_open` not found.

- [ ] **Step 3: Implement**

`fail_next_open` sets a one-shot detail. An `open` that consumes it returns `Ok`, schedules only `Exited { clean: false, detail }` for the next `advance`, and its handle refuses every command with `Closed`. The fake records which request ids are waiting per session as it **delivers** `NeedsUser` and `RequestWithdrawn`; an accepted `Answer` removes the id; `Exited` clears them. A refused command is not added to `sent()`.

- [ ] **Step 4: Correct the two doc comments**

In both files replace "exactly once" for `Started` with: `Started` comes at most once. If the program exits before it is ready (a session id that cannot be resumed, a bad flag), `Exited` is the only event, and `open` may also have returned an error. `Exited` still comes exactly once.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p mr-agent`
Expected: PASS, including the older `close_ends_the_session_and_refuses_every_later_command`. If that test asserted that a second `Close` is refused before the exit is delivered, change that one assertion to match the real adapter; say so in the report.

- [ ] **Step 6: Commit**

```bash
git add crates/agent/src && git commit -m "fix(agent): the fake is as strict as the real adapter; Started is at most once"
```

---

### Task 2: Words the window needs that the seam lacks

**Files:**
- Modify: `crates/agent/src/event.rs`, `crates/agent/src/command.rs`, `crates/agent/src/lib.rs`, `crates/agent/src/fake.rs`
- Modify, only so the workspace compiles: `crates/claude/src/translate.rs`, `crates/claude/src/agent.rs`, `crates/probe/src/*.rs`

**Interfaces:**
- Produces, all re-exported from `mr_agent`:

```rust
// event.rs
pub enum SessionEvent {
    // new, placed after ThinkingDelta
    /// A thinking phase began. `BlockFinished` ends it, also when no thinking
    /// text was sent, except in a turn the user stopped.
    ThinkingStarted,
    // changed
    CommandFailed { what: CommandKind, reason: String },
    // every other variant unchanged
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandKind { SetModel, SetPermissionMode, Rename, SetRemoteAccess, RefreshUsage }
pub struct Question {
    pub header: Option<String>,
    pub prompt: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}
pub struct QuestionOption { pub label: String, pub description: Option<String> }

// command.rs
pub enum SessionCommand {
    // new, placed before Close
    /// Ask for the usage limits now. The answer is a `UsageLimits` event.
    RefreshUsage,
}
```

`Question` and `QuestionOption` derive the same traits as the other event types.

- [ ] **Step 1: Write the failing test** in `crates/agent/src/event.rs`

```rust
#[test]
fn the_new_words_survive_a_round_trip_through_json() {
    let events = vec![
        SessionEvent::ThinkingStarted,
        SessionEvent::CommandFailed { what: CommandKind::SetModel, reason: "no such model".into() },
        SessionEvent::NeedsUser(UserRequest {
            id: RequestId("r1".into()),
            kind: UserRequestKind::Question {
                questions: vec![Question {
                    header: Some("Colour".into()),
                    prompt: "Which colour?".into(),
                    options: vec![QuestionOption { label: "Red".into(), description: Some("Warm".into()) }],
                    multi_select: false,
                }],
            },
        }),
    ];
    let text = serde_json::to_string(&events).unwrap();
    assert_eq!(serde_json::from_str::<Vec<SessionEvent>>(&text).unwrap(), events);
    let command = SessionCommand::RefreshUsage;
    let text = serde_json::to_string(&command).unwrap();
    assert_eq!(serde_json::from_str::<SessionCommand>(&text).unwrap(), command);
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p mr-agent the_new_words`
Expected: FAIL to compile.

- [ ] **Step 3: Add the types, then make the workspace compile with the smallest change**

In `mr-claude`: `questions()` fills `header: None` and `description: None` for now; `command_failed` takes a `CommandKind` (`"remote access"` becomes `SetRemoteAccess`, `"model"` `SetModel`, `"permission mode"` `SetPermissionMode`, `"rename"` `Rename`); `RefreshUsage` is accepted and does nothing yet. The fake records `RefreshUsage` in `sent()` and emits nothing. Task 3 and Task 4 give these their real behaviour.

- [ ] **Step 4: Run everything**

Run: `cargo test --workspace`
Expected: PASS. Tests in `mr-claude` that compared `what` to a string now compare it to a `CommandKind`.

- [ ] **Step 5: Commit**

```bash
git add crates/agent crates/claude crates/probe && git commit -m "feat(agent): thinking start, typed command failures, question details, usage refresh"
```

---

### Task 3: The adapter reports thinking and whole questions

**Files:**
- Modify: `crates/claude/src/translate.rs`

**Interfaces:**
- Consumes: Task 2's types.
- Produces: no new signature. New behaviour of `Translator::push`:
  - a streamed thinking block gives `ThinkingStarted` at its start and `BlockFinished` at its end, whether or not any thinking text arrived;
  - without partial messages, a thinking block in an `assistant` record gives `ThinkingStarted`, a `ThinkingDelta` if it has text, and `BlockFinished`;
  - a question carries `header` (the wire's `header`) and each option's `description`.

`docs/protocol-notes.md` section V4 shows the thinking records; V7 shows an `AskUserQuestion` input.

- [ ] **Step 1: Write the failing tests** in `translate.rs`, beside the existing streaming tests and using their `feed` helper

```rust
#[test]
fn a_thinking_block_is_announced_even_though_its_text_is_empty() {
    let events = feed(&[
        r#"{"type":"stream_event","event":{"type":"message_start","message":{"model":"m"}}}"#,
        r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}"#,
        r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":""}}}"#,
        r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"x"}}}"#,
        r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#,
    ]);
    assert_eq!(
        events,
        vec![SessionEvent::TurnStarted, SessionEvent::ThinkingStarted, SessionEvent::BlockFinished]
    );
}

#[test]
fn a_question_keeps_its_header_and_option_descriptions() {
    let input = serde_json::json!({"questions":[{
        "question":"Which colour?","header":"Colour","multiSelect":true,
        "options":[{"label":"Red","description":"Warm"},{"label":"Blue"}]
    }]});
    assert_eq!(
        questions(&input),
        vec![Question {
            header: Some("Colour".into()),
            prompt: "Which colour?".into(),
            options: vec![
                QuestionOption { label: "Red".into(), description: Some("Warm".into()) },
                QuestionOption { label: "Blue".into(), description: None },
            ],
            multi_select: true,
        }]
    );
}
```

Add a third test for the path without partial messages: one `assistant` record with a thinking block holding the text `"hm"` followed by a text block gives `ThinkingStarted`, `ThinkingDelta("hm")`, `BlockFinished` before the text events. Build its line the way `without_partial_messages_the_whole_text_still_arrives` does.

- [ ] **Step 2: Close the gap in the replay test.** The existing test that replays every recording (`replay` helper near the end of the file) would not notice lost text. Add:

```rust
#[test]
fn every_recording_streams_exactly_the_text_its_records_hold() {
    for name in ["v1-remote", "v2-permission", "v4-image-model-stop", "v7-question-plan", "v9-control", "v11-resume", "v11-fork"] {
        let (streamed, recorded) = text_per_turn(name);
        assert_eq!(streamed, recorded, "{name}");
    }
}
```

`text_per_turn(name) -> (Vec<String>, Vec<String>)` splits the recording at each `result` record. For each turn, `streamed` is the concatenation of the `TextDelta` events the translator gave, and `recorded` is the concatenation of the `text` blocks of that turn's `assistant` records, read straight from the JSON. A turn the user stopped (its `result` has `terminal_reason` starting with `aborted`) is compared with `recorded.starts_with(streamed)` instead, because the stream is cut first.

- [ ] **Step 3: Run to see the new tests fail**

Run: `cargo test -p mr-claude translate`
Expected: the thinking and question tests FAIL. If the recording test fails, that is a real defect in plan 1's translator: fix it in this task and name it in the report.

- [ ] **Step 4: Implement** in `stream`, `assistant` and `questions`. A text block keeps its present rule (no `BlockFinished` for a text block that had no text).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p mr-claude`
Expected: PASS. Earlier tests that listed a turn's events exactly gain `ThinkingStarted` and `BlockFinished` where the turn had a thinking block.

- [ ] **Step 6: Commit**

```bash
git add crates/claude/src/translate.rs && git commit -m "feat(claude): report thinking phases and whole questions; replay test checks text"
```

---

### Task 4: Usage limits on request

The footer must show the limits when the window opens, not after the first turn.

**Files:**
- Create: `crates/claude/src/time.rs`
- Modify: `crates/claude/src/lib.rs`, `crates/claude/src/encode.rs`, `crates/claude/src/translate.rs`, `crates/claude/src/agent.rs`

**Interfaces:**
- Produces:
  - `pub fn iso8601_to_unix(text: &str) -> Option<i64>` in `time.rs`;
  - `pub fn get_usage() -> Value` in `encode.rs`, the body `{"subtype":"get_usage"}`;
  - `Tracked::Usage`;
  - `SessionCommand::RefreshUsage` sends that request tracked as `Tracked::Usage`. A successful reply gives one `SessionEvent::UsageLimits`; a refused one gives `CommandFailed { what: CommandKind::RefreshUsage, .. }`.

The reply's numbers differ from the per-turn event: `utilization` is a percentage and `resets_at` is ISO 8601 text. See "V9 Other control requests" in `docs/protocol-notes.md`.

- [ ] **Step 1: Write the failing tests**

In `time.rs`:

```rust
#[test]
fn iso_times_become_unix_seconds() {
    assert_eq!(iso8601_to_unix("2026-11-11T08:19:59.618411+00:00"), Some(1794385199));
    assert_eq!(iso8601_to_unix("2026-11-15T20:59:59Z"), Some(1794776399));
    assert_eq!(iso8601_to_unix("2026-03-01T00:30:00+02:00"), Some(1772317800));
    assert_eq!(iso8601_to_unix("1970-01-01T00:00:00Z"), Some(0));
    for bad in ["", "soon", "2026-13-01T00:00:00Z", "2026-11-11", "2026-11-11T08:19:59"] {
        assert_eq!(iso8601_to_unix(bad), None, "{bad}");
    }
}
```

In `translate.rs`, using the helpers the `set_model` reply tests use to track a request and push its `control_response`:

```rust
#[test]
fn a_usage_reply_becomes_usage_limits() {
    let body = r#"{"subscription_type":"max","rate_limits_available":true,"rate_limits":{
        "five_hour":{"utilization":37,"resets_at":"2026-11-11T08:19:59.618411+00:00"},
        "seven_day":{"utilization":10,"resets_at":"2026-11-15T20:59:59.618435+00:00"}}}"#;
    assert_eq!(
        usage_reply(body),
        vec![SessionEvent::UsageLimits(UsageLimits {
            session: Some(LimitWindow { used: 0.37, resets_at_unix: 1794385199 }),
            weekly: Some(LimitWindow { used: 0.10, resets_at_unix: 1794776399 }),
        })]
    );
}

#[test]
fn a_usage_reply_without_numbers_reports_nothing_as_known() {
    assert_eq!(
        usage_reply(r#"{"rate_limits_available":false}"#),
        vec![SessionEvent::UsageLimits(UsageLimits { session: None, weekly: None })]
    );
    let one = r#"{"rate_limits":{"five_hour":{"utilization":"lots","resets_at":"soon"},
        "seven_day":{"utilization":101.5,"resets_at":"2026-11-15T20:59:59Z"}}}"#;
    assert_eq!(
        usage_reply(one),
        vec![SessionEvent::UsageLimits(UsageLimits {
            session: None,
            weekly: Some(LimitWindow { used: 1.0, resets_at_unix: 1794776399 }),
        })]
    );
}
```

`usage_reply(body) -> Vec<SessionEvent>` is a test helper: track `"u1"` as `Tracked::Usage`, push a successful `control_response` for `"u1"` whose `response` is `body`. `used` is `utilization / 100`, clamped to `0.0..=1.0`. Compare `used` with the float values written above; they are exact results of the division in `f32`.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mr-claude`
Expected: FAIL to compile.

- [ ] **Step 3: Implement.** `iso8601_to_unix` accepts `YYYY-MM-DDTHH:MM:SS`, an optional fraction, and a required zone that is `Z` or `±HH:MM`; it checks month, day, hour, minute and second ranges and uses the days-from-civil calculation. No new dependency.

- [ ] **Step 4: Add one test at the process level** in `crates/claude/tests/agent.rs`, following the existing test that sends `SetModel` to the stub: send `RefreshUsage`, have the stub answer the `get_usage` request with the body from Step 1, and assert the sink receives the same `UsageLimits`. Extend the stub under `crates/claude/tests/stub/` in the way the existing cases are written.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p mr-claude`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/claude && git commit -m "feat(claude): usage limits on request"
```

---

### Task 5: Tool calls as lines a person can read

**Files:**
- Create: `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/tools.rs`
- Modify: `Cargo.toml` (add `similar = "2"` to `[workspace.dependencies]`)

**Interfaces:**
- Consumes: `mr_agent::{ToolCallId, ToolDetail, ToolOutcome}`.
- Produces: package `mr-core` (library `mr_core`), depending on `mr-agent`, `serde` and `similar` only. In `tools.rs`, re-exported from the crate root:

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall { pub id: ToolCallId, pub name: String, pub detail: ToolDetail, pub outcome: Option<ToolOutcome> }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolFamily { Read, Search, Command, Edit, Other }
#[derive(Clone, Debug, PartialEq)]
pub struct ToolGroup { pub family: ToolFamily, pub calls: Vec<ToolCall> }

impl ToolGroup {
    pub fn start(call: ToolCall) -> ToolGroup;
    /// Adds `call` when it folds into this group; gives it back when it does not.
    pub fn try_add(&mut self, call: ToolCall) -> Result<(), ToolCall>;
    pub fn summary(&self) -> String;
    /// A call has no outcome yet.
    pub fn running(&self) -> bool;
    pub fn failed(&self) -> bool;
}
pub fn family(detail: &ToolDetail) -> ToolFamily;
/// The last component of a path, or the whole text when it has none.
pub fn file_name(path: &str) -> &str;

pub const OUTPUT_LINES: usize = 20;
pub const OUTPUT_CHARS: usize = 4000;
#[derive(Debug, PartialEq)]
pub struct Clip<'a> { pub shown: &'a str, pub cut: bool, pub hidden_lines: usize }
pub fn clip(text: &str, max_lines: usize, max_chars: usize) -> Clip<'_>;
/// Lines added and lines removed going from `old` to `new`.
pub fn line_counts(old: &str, new: &str) -> (usize, usize);
```

Rules. `family`: `FileRead` is `Read`; `Search` is `Search`; `Command` is `Command`; `FileEdit` and `FileWrite` are `Edit`; `Other` is `Other`. Folding: a `Read` call folds into a `Read` group and a `Search` call into a `Search` group; an `Other` call folds into an `Other` group whose calls have the same `name`; `Command` and `Edit` never fold, because each carries details the user reads.

Summaries, exactly:

| Group | One call | Several calls |
| --- | --- | --- |
| Read | `Read main.rs` | `Read 3 files` |
| Search | `Searched for TODO` (query cut to 60 characters, then `…`) | `Searched 3 times` |
| Command | `Ran cargo test` (first line of the command, cut to 80 characters, then `…`) | never several |
| Edit, `FileEdit` | `Edited main.rs` | never several |
| Edit, `FileWrite` | `Wrote main.rs` | never several |
| Other | `WebFetch` (the tool's name) | `WebFetch, 3 times` |

`clip` shows at most `max_lines` lines and at most `max_chars` characters, whichever ends first, always cutting on a character boundary. `hidden_lines` is the number of lines not shown at all; a single over-long line gives `cut: true, hidden_lines: 0`.

- [ ] **Step 1: Write the failing tests** in `tools.rs`

```rust
fn call(id: &str, name: &str, detail: ToolDetail) -> ToolCall {
    ToolCall { id: ToolCallId(id.into()), name: name.into(), detail, outcome: None }
}
fn read(id: &str, path: &str) -> ToolCall { call(id, "Read", ToolDetail::FileRead { path: path.into() }) }

#[test]
fn reads_fold_and_a_command_does_not() {
    let mut group = ToolGroup::start(read("1", "/p/src/main.rs"));
    assert_eq!(group.summary(), "Read main.rs");
    assert!(group.try_add(read("2", "/p/a.rs")).is_ok());
    assert!(group.try_add(read("3", "b.rs")).is_ok());
    assert_eq!(group.summary(), "Read 3 files");
    let command = call("4", "Bash", ToolDetail::Command { command: "cargo test\n--workspace".into() });
    let back = group.try_add(command.clone()).unwrap_err();
    assert_eq!(back, command);
    let mut commands = ToolGroup::start(command.clone());
    assert_eq!(commands.summary(), "Ran cargo test");
    assert!(commands.try_add(command).is_err());
}

#[test]
fn other_tools_fold_only_with_the_same_name() {
    let other = |id: &str, name: &str| call(id, name, ToolDetail::Other { input_json: "{}".into() });
    let mut group = ToolGroup::start(other("1", "WebFetch"));
    assert_eq!(group.summary(), "WebFetch");
    assert!(group.try_add(other("2", "WebFetch")).is_ok());
    assert!(group.try_add(other("3", "TodoWrite")).is_err());
    assert_eq!(group.summary(), "WebFetch, 2 times");
}

#[test]
fn the_other_summaries_read_as_written() {
    let s = |name: &str, d: ToolDetail| ToolGroup::start(call("1", name, d)).summary();
    assert_eq!(s("Grep", ToolDetail::Search { query: "TODO".into() }), "Searched for TODO");
    assert_eq!(s("Grep", ToolDetail::Search { query: "x".repeat(61) }), format!("Searched for {}…", "x".repeat(60)));
    assert_eq!(s("Edit", ToolDetail::FileEdit { path: "/p/main.rs".into(), old_text: "a".into(), new_text: "b".into() }), "Edited main.rs");
    assert_eq!(s("Write", ToolDetail::FileWrite { path: "notes.md".into(), content: "x".into() }), "Wrote notes.md");
    assert_eq!(s("Bash", ToolDetail::Command { command: "y".repeat(81) }), format!("Ran {}…", "y".repeat(80)));
}

#[test]
fn running_and_failed_follow_the_outcomes() {
    let mut group = ToolGroup::start(read("1", "a"));
    assert!(group.running() && !group.failed());
    group.calls[0].outcome = Some(ToolOutcome::Failed { output: "no such file".into() });
    assert!(!group.running() && group.failed());
}

#[test]
fn long_output_is_cut_on_a_line_or_a_character() {
    let many: String = (0..50_000).map(|n| format!("line {n}\n")).collect();
    let c = clip(&many, OUTPUT_LINES, OUTPUT_CHARS);
    assert_eq!(c.shown.lines().count(), 20);
    assert_eq!((c.cut, c.hidden_lines), (true, 49_980));
    let wide = "é".repeat(1_000_000);
    let c = clip(&wide, OUTPUT_LINES, OUTPUT_CHARS);
    assert_eq!(c.shown.chars().count(), 4000);
    assert_eq!((c.cut, c.hidden_lines), (true, 0));
    let short = clip("one\ntwo", OUTPUT_LINES, OUTPUT_CHARS);
    assert_eq!(short, Clip { shown: "one\ntwo", cut: false, hidden_lines: 0 });
    assert_eq!(clip("", OUTPUT_LINES, OUTPUT_CHARS).shown, "");
}

#[test]
fn line_counts_count_added_and_removed_lines() {
    assert_eq!(line_counts("a\nb\nc\n", "a\nx\nc\nd\n"), (2, 1));
    assert_eq!(line_counts("", "one\ntwo\n"), (2, 0));
    assert_eq!(line_counts("same\n", "same\n"), (0, 0));
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mr-core`
Expected: FAIL to compile.

- [ ] **Step 3: Implement.** `line_counts` uses `similar::TextDiff::from_lines`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/core && git commit -m "feat(core): tool calls as folded, summarised lines; clipping and line counts"
```

---

### Task 6: The conversation

**Files:**
- Create: `crates/core/src/conversation.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: Task 5's `ToolGroup`, `ToolCall`; `mr_agent::{SessionEvent, UserRequest, RequestId}`.
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId(pub u64);
#[derive(Clone, Debug, PartialEq)]
pub struct Item { pub id: ItemId, pub body: ItemBody }
#[derive(Clone, Debug, PartialEq)]
pub enum ItemBody {
    User { text: String },
    Reply { markdown: String, streaming: bool },
    /// `seconds` is `None` while the thinking goes on.
    Thinking { text: String, started: f64, seconds: Option<f64> },
    Tools(ToolGroup),
    Request { request: UserRequest, state: RequestState },
    Notice { kind: NoticeKind, text: String },
}
#[derive(Clone, Debug, PartialEq)]
pub enum RequestState { Waiting, Answered(String), Withdrawn }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind { Stopped, TurnFailed, Ended, CommandFailed }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conversation { /* private */ }
impl Conversation {
    pub fn new() -> Self;
    pub fn items(&self) -> &[Item];
    /// Grows by at least one whenever anything a view shows has changed.
    pub fn revision(&self) -> u64;
    pub fn push_user(&mut self, text: &str);
    pub fn push_notice(&mut self, kind: NoticeKind, text: &str);
    /// `now` is seconds on any steady clock; it only times thinking.
    pub fn apply(&mut self, event: &SessionEvent, now: f64);
    /// Marks a waiting request answered, with `words` to show. False when it was not waiting.
    pub fn answered(&mut self, id: &RequestId, words: &str) -> bool;
    pub fn waiting(&self) -> usize;
}
```

What `apply` does:

| Event | Effect |
| --- | --- |
| `TextDelta(t)` | Appends `t` in place to the last item when it is a streaming `Reply`; otherwise closes an open thinking and pushes `Reply { streaming: true }`. |
| `ThinkingStarted` | Closes a streaming reply. Pushes `Thinking { started: now, seconds: None }`. |
| `ThinkingDelta(t)` | Appends to the open `Thinking`; pushes one first when none is open. |
| `BlockFinished` | The last item, when it is an open `Thinking`, gets `seconds = now - started`; when it is a streaming `Reply`, `streaming = false`. |
| `ToolStarted` | Closes a streaming reply and an open thinking. Adds the call to the last item when that is a group it folds into; otherwise pushes a new group. |
| `ToolFinished` | Sets the outcome of the call with that id, searching from the end. An unknown id changes nothing. |
| `NeedsUser(r)` | Pushes `Request { state: Waiting }`. |
| `RequestWithdrawn { id }` | A waiting request with that id becomes `Withdrawn`. |
| `TurnFinished(s)` | Closes what is open. When `s.interrupted`, pushes `Notice { Stopped, "Stopped" }`. |
| `TurnFailed { reason }` | Closes what is open. Pushes `Notice { TurnFailed, reason }`. |
| `Exited { clean, detail }` | Closes what is open. Every waiting request becomes `Withdrawn`. When not clean, pushes `Notice { Ended, detail }`, or `"The session ended unexpectedly."` when `detail` is empty. |
| anything else | Nothing, and `revision` does not change. |

"Closes what is open" means: a streaming reply stops streaming, and an open thinking gets its `seconds`. Item ids are never reused and never change.

- [ ] **Step 1: Write the failing tests** in `conversation.rs`

```rust
fn bodies(c: &Conversation) -> Vec<ItemBody> { c.items().iter().map(|i| i.body.clone()).collect() }
fn text(s: &str) -> SessionEvent { SessionEvent::TextDelta(s.into()) }
fn play(c: &mut Conversation, events: &[SessionEvent]) { for e in events { c.apply(e, 0.0); } }
fn read(id: &str) -> SessionEvent {
    SessionEvent::ToolStarted { id: ToolCallId(id.into()), name: "Read".into(), detail: ToolDetail::FileRead { path: format!("{id}.rs") } }
}
fn plan(id: &str) -> SessionEvent {
    SessionEvent::NeedsUser(UserRequest { id: RequestId(id.into()), kind: UserRequestKind::PlanApproval { plan_markdown: "p".into() } })
}
fn summary(interrupted: bool) -> TurnSummary {
    TurnSummary { cost_usd: 0.0, context_used: None, duration_ms: 1, interrupted }
}

#[test]
fn a_reply_streams_into_one_item() {
    let mut c = Conversation::new();
    c.push_user("**hi**");
    play(&mut c, &[SessionEvent::TurnStarted, text("Hel"), text("lo"), SessionEvent::BlockFinished]);
    assert_eq!(bodies(&c), vec![
        ItemBody::User { text: "**hi**".into() },
        ItemBody::Reply { markdown: "Hello".into(), streaming: false },
    ]);
}

#[test]
fn thinking_is_timed_by_the_clock_it_is_given() {
    let mut c = Conversation::new();
    c.apply(&SessionEvent::ThinkingStarted, 10.0);
    assert_eq!(bodies(&c), vec![ItemBody::Thinking { text: String::new(), started: 10.0, seconds: None }]);
    c.apply(&SessionEvent::BlockFinished, 13.5);
    assert_eq!(bodies(&c), vec![ItemBody::Thinking { text: String::new(), started: 10.0, seconds: Some(3.5) }]);
}

#[test]
fn tool_calls_fold_and_text_after_them_starts_a_new_reply() {
    let mut c = Conversation::new();
    play(&mut c, &[text("Looking."), read("a"), read("b"), SessionEvent::ToolFinished {
        id: ToolCallId("a".into()), outcome: ToolOutcome::Ok { output: "x".into() } }, text("Done.")]);
    let b = bodies(&c);
    assert_eq!(b.len(), 3);
    assert_eq!(b[0], ItemBody::Reply { markdown: "Looking.".into(), streaming: false });
    let ItemBody::Tools(group) = &b[1] else { panic!("{:?}", b[1]) };
    assert_eq!(group.summary(), "Read 2 files");
    assert!(group.calls[0].outcome.is_some() && group.calls[1].outcome.is_none());
    assert_eq!(b[2], ItemBody::Reply { markdown: "Done.".into(), streaming: true });
}

#[test]
fn a_request_is_answered_once_or_withdrawn() {
    let mut c = Conversation::new();
    play(&mut c, &[plan("r1"), plan("r2")]);
    assert_eq!(c.waiting(), 2);
    assert!(c.answered(&RequestId("r1".into()), "Plan approved"));
    assert!(!c.answered(&RequestId("r1".into()), "again"));
    assert!(!c.answered(&RequestId("nope".into()), "x"));
    c.apply(&SessionEvent::RequestWithdrawn { id: RequestId("r2".into()) }, 0.0);
    c.apply(&SessionEvent::RequestWithdrawn { id: RequestId("r1".into()) }, 0.0);
    let states: Vec<_> = bodies(&c).into_iter().map(|b| match b { ItemBody::Request { state, .. } => state, o => panic!("{o:?}") }).collect();
    assert_eq!(states, vec![RequestState::Answered("Plan approved".into()), RequestState::Withdrawn]);
    assert_eq!(c.waiting(), 0);
}

#[test]
fn a_dying_session_closes_everything_and_says_why() {
    let mut c = Conversation::new();
    c.apply(&SessionEvent::ThinkingStarted, 1.0);
    c.apply(&text("partial"), 2.0);
    c.apply(&plan("r1"), 2.0);
    c.apply(&SessionEvent::Exited { clean: false, detail: "killed".into() }, 4.0);
    assert_eq!(bodies(&c), vec![
        ItemBody::Thinking { text: String::new(), started: 1.0, seconds: Some(1.0) },
        ItemBody::Reply { markdown: "partial".into(), streaming: false },
        ItemBody::Request {
            request: UserRequest { id: RequestId("r1".into()), kind: UserRequestKind::PlanApproval { plan_markdown: "p".into() } },
            state: RequestState::Withdrawn,
        },
        ItemBody::Notice { kind: NoticeKind::Ended, text: "killed".into() },
    ]);
    let mut clean = Conversation::new();
    clean.apply(&SessionEvent::Exited { clean: true, detail: String::new() }, 0.0);
    assert!(clean.items().is_empty());
    let mut silent = Conversation::new();
    silent.apply(&SessionEvent::Exited { clean: false, detail: String::new() }, 0.0);
    assert_eq!(bodies(&silent), vec![ItemBody::Notice { kind: NoticeKind::Ended, text: "The session ended unexpectedly.".into() }]);
}

#[test]
fn a_stopped_turn_and_a_failed_turn_leave_a_notice() {
    let mut c = Conversation::new();
    play(&mut c, &[text("half"), SessionEvent::TurnFinished(summary(true))]);
    play(&mut c, &[SessionEvent::TurnFailed { reason: "rate limit reached".into() }]);
    play(&mut c, &[SessionEvent::TurnFinished(summary(false))]);
    assert_eq!(bodies(&c), vec![
        ItemBody::Reply { markdown: "half".into(), streaming: false },
        ItemBody::Notice { kind: NoticeKind::Stopped, text: "Stopped".into() },
        ItemBody::Notice { kind: NoticeKind::TurnFailed, text: "rate limit reached".into() },
    ]);
}

#[test]
fn twenty_thousand_pieces_make_one_item() {
    let mut c = Conversation::new();
    for _ in 0..20_000 { c.apply(&text("word "), 0.0); }
    assert_eq!(c.items().len(), 1);
    let ItemBody::Reply { markdown, .. } = &c.items()[0].body else { panic!() };
    assert_eq!(markdown.len(), 100_000);
}

#[test]
fn ids_stay_put_and_the_revision_tracks_real_changes() {
    let mut c = Conversation::new();
    c.push_user("one");
    let first = c.items()[0].id;
    let before = c.revision();
    c.apply(&SessionEvent::TitleChanged("t".into()), 0.0);
    assert_eq!(c.revision(), before);
    c.apply(&text("x"), 0.0);
    c.push_notice(NoticeKind::CommandFailed, "no");
    assert!(c.revision() > before);
    assert_eq!(c.items()[0].id, first);
    let ids: std::collections::HashSet<_> = c.items().iter().map(|i| i.id).collect();
    assert_eq!(ids.len(), c.items().len());
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mr-core conversation`
Expected: FAIL to compile.

- [ ] **Step 3: Implement** the table above. A text piece is appended to the existing `String`; the conversation never rebuilds or copies a reply as it grows.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src && git commit -m "feat(core): the conversation as items built from session events"
```

---

### Task 7: One session's life

**Files:**
- Create: `crates/core/src/session.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: Task 6's `Conversation`; `mr_agent` types.
- Produces:

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// The program is starting; nothing can be sent yet.
    Opening,
    Idle,
    /// A turn is in flight.
    Working,
    /// A turn is in flight and at least one request waits for the user.
    NeedsUser,
    Ended { clean: bool, detail: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub folder: PathBuf,
    pub id: Option<SessionId>,
    pub title: Option<String>,
    pub phase: Phase,
    /// The full id of the model in use, once known.
    pub model: Option<String>,
    pub models: Vec<ModelOption>,
    pub mode: PermissionMode,
    pub conversation: Conversation,
    /// A message typed while it could not be sent yet.
    pub queued: Option<String>,
    pub cost_usd: Option<f64>,
    pub context_used: Option<f32>,
}

impl Session {
    pub fn opening(folder: impl Into<PathBuf>) -> Session;
    /// Applies an event. Returns commands the session now wants sent: a queued message.
    pub fn apply(&mut self, event: SessionEvent, now: f64) -> Vec<SessionCommand>;
    pub fn submit(&mut self, text: &str) -> Option<SessionCommand>;
    pub fn stop(&self) -> Option<SessionCommand>;
    pub fn answer(&mut self, id: &RequestId, answer: Answer) -> Option<SessionCommand>;
    pub fn choose_model(&mut self, option_id: &str) -> Option<SessionCommand>;
    pub fn choose_mode(&self, mode: PermissionMode) -> Option<SessionCommand>;
    /// Gives the queued message back, for the input box.
    pub fn take_queued(&mut self) -> Option<String>;
    pub fn model_label(&self) -> String;
    /// How to open this session again after it ended.
    pub fn restart_spec(&self) -> Option<StartSpec>;
    /// Back to `Opening` for a restart, keeping the conversation.
    pub fn reopening(&mut self);
}
pub fn mode_label(mode: &PermissionMode) -> String;
pub fn answer_words(answer: &Answer) -> String;
```

Transitions:

| In phase | On | Result |
| --- | --- | --- |
| `Opening` | `Started(info)` | `Idle`; `id`, `model`, `models`, `mode` taken from `info`. A queued message is sent at once (see `TurnFinished`). |
| any but `Ended` | `TurnStarted` | `Working`, unless requests are waiting (`NeedsUser`). |
| `Working` | `NeedsUser` | `NeedsUser`. |
| `NeedsUser` | the last waiting request is answered or withdrawn | `Working`. |
| `Working`, `NeedsUser` | `TurnFinished(s)` or `TurnFailed` | `Idle`. `cost_usd` and `context_used` are updated from `s` (`context_used` only when it is `Some`). Then, if a message is queued: it is added to the conversation as a user item, the phase is `Working`, and `apply` returns the `Send`. |
| any | `Exited { clean, detail }` | `Ended { clean, detail }`. The queued message stays in `queued`, so the input box can give it back. |
| any | `TitleChanged`, `ModelChanged`, `PermissionModeChanged` | The field is updated. |
| any | `CommandFailed { what, reason }` | A `CommandFailed` notice, worded below. `RefreshUsage` gives no notice. |

Every event is also given to `conversation.apply`.

Intents:

- `submit`: text that is empty after trimming gives `None`. In `Idle`: user item, `Working`, `Some(Send { text, attachments: vec![] })`. In `Opening`, `Working` and `NeedsUser`: the text is queued and `None` is returned; a second queued message is appended to the first after a blank line. In `Ended`: `None` and nothing changes.
- `stop`: `Some(Stop)` in `Working` and `NeedsUser`, otherwise `None`.
- `answer`: `Some(Answer { .. })` only when `conversation.answered(id, &answer_words(&answer))` is true and the phase is not `Ended`.
- `choose_model`: `None` in `Opening` and `Ended`, and when `option_id` is not in `models`. Otherwise remembers `option_id` as the user's choice and returns `Some(SetModel(option_id))`. `model` changes only when `ModelChanged` arrives.
- `choose_mode`: `None` in `Opening` and `Ended` and when `mode` is already the mode. Otherwise `Some(SetPermissionMode(mode))`.
- `model_label`: the `name` of the option the user chose, when that option's `resolved` equals `model`; else the `name` of the first option whose `resolved` or `id` equals `model`; else `model` itself; `"Model"` when nothing is known.
- `restart_spec`: `None` unless `Ended`. A spec for `folder` with `resume` set to `id` when it is known, and with `model` and `permission_mode` left `None`.

Wording, exactly:

| | Text |
| --- | --- |
| `mode_label` | `Ask` "Ask before edits"; `AcceptEdits` "Accept edits"; `Plan` "Plan"; `Bypass` "Bypass permissions"; `Other(s)` the text `s` |
| `answer_words` | `Allow { always: false }` "Allowed once"; `Allow { always: true }` "Always allowed"; `Deny` "Denied"; `Answers(a)` the chosen labels and free texts joined with ", ", or "Answered" when there are none; `ApprovePlan` "Plan approved"; `KeepPlanning` "Kept planning" |
| failed command | `SetModel` "Could not change the model: {reason}"; `SetPermissionMode` "Could not change the permission mode: {reason}"; `Rename` "Could not rename the session: {reason}"; `SetRemoteAccess` "Could not change Remote Control: {reason}" |

- [ ] **Step 1: Write the failing tests** in `session.rs`. Helpers: `info()` returns a `SessionInfo` with id `"s1"`, folder `/p`, model `"claude-haiku-5-5"`, mode `Ask`, and the two models `{ id: "default", name: "Default", resolved: "claude-fable-5-2" }` and `{ id: "haiku", name: "Haiku 5.5", resolved: "claude-haiku-5-5" }`; `started()` returns a session that has received `Started(info())`; `done()` is `SessionEvent::TurnFinished` with `cost_usd: 0.25`, `context_used: Some(0.5)`, `interrupted: false`; `permission(id)` is a `NeedsUser` permission request for `Bash` with a `Command` detail and no always-allow label.

```rust
#[test]
fn a_message_is_sent_when_idle_and_queued_otherwise() {
    let mut s = Session::opening("/p");
    assert_eq!(s.submit("early"), None);
    assert_eq!(s.queued.as_deref(), Some("early"));
    let sent = s.apply(SessionEvent::Started(info()), 0.0);
    assert_eq!(sent, vec![SessionCommand::Send { text: "early".into(), attachments: vec![] }]);
    assert_eq!((s.phase.clone(), s.queued.clone()), (Phase::Working, None));
    assert_eq!(s.submit("  "), None);
    assert_eq!(s.submit("two"), None);
    assert_eq!(s.submit("three"), None);
    assert_eq!(s.queued.as_deref(), Some("two\n\nthree"));
    let sent = s.apply(done(), 0.0);
    assert_eq!(sent, vec![SessionCommand::Send { text: "two\n\nthree".into(), attachments: vec![] }]);
    assert_eq!(s.phase, Phase::Working);
    assert_eq!(s.apply(done(), 0.0), vec![]);
    assert_eq!(s.phase, Phase::Idle);
    assert_eq!((s.cost_usd, s.context_used), (Some(0.25), Some(0.5)));
    let users = s.conversation.items().iter().filter(|i| matches!(i.body, ItemBody::User { .. })).count();
    assert_eq!(users, 2);
}

#[test]
fn typing_while_a_request_waits_never_answers_it() {
    let mut s = started();
    s.submit("go");
    s.apply(permission("r1"), 0.0);
    assert_eq!(s.phase, Phase::NeedsUser);
    assert_eq!(s.submit("yes"), None);
    assert_eq!(s.conversation.waiting(), 1);
    assert_eq!(s.queued.as_deref(), Some("yes"));
    let sent = s.answer(&RequestId("r1".into()), Answer::Allow { always: false });
    assert_eq!(sent, Some(SessionCommand::Answer { id: RequestId("r1".into()), answer: Answer::Allow { always: false } }));
    assert_eq!(s.phase, Phase::Working);
    assert_eq!(s.answer(&RequestId("r1".into()), Answer::Deny { message: None }), None);
}

#[test]
fn a_withdrawn_request_lets_the_turn_carry_on() {
    let mut s = started();
    s.submit("go");
    s.apply(permission("r1"), 0.0);
    s.apply(SessionEvent::RequestWithdrawn { id: RequestId("r1".into()) }, 0.0);
    assert_eq!(s.phase, Phase::Working);
    assert_eq!(s.answer(&RequestId("r1".into()), Answer::Allow { always: false }), None);
}

#[test]
fn nothing_is_sent_to_a_session_that_ended() {
    let mut s = started();
    s.submit("go");
    s.apply(permission("r1"), 0.0);
    s.submit("later");
    s.apply(SessionEvent::Exited { clean: false, detail: "killed".into() }, 0.0);
    assert_eq!(s.phase, Phase::Ended { clean: false, detail: "killed".into() });
    assert_eq!(s.answer(&RequestId("r1".into()), Answer::Allow { always: false }), None);
    assert_eq!((s.submit("x"), s.stop(), s.choose_model("haiku"), s.choose_mode(PermissionMode::Plan)), (None, None, None, None));
    assert_eq!(s.take_queued().as_deref(), Some("later"));
    let spec = s.restart_spec().unwrap();
    assert_eq!((spec.folder, spec.resume, spec.fork), (PathBuf::from("/p"), Some(SessionId("s1".into())), false));
    s.reopening();
    assert_eq!(s.phase, Phase::Opening);
    assert!(!s.conversation.items().is_empty());
}

#[test]
fn stop_is_offered_only_during_a_turn() {
    let mut s = started();
    assert_eq!(s.stop(), None);
    s.submit("go");
    assert_eq!(s.stop(), Some(SessionCommand::Stop));
}

#[test]
fn the_model_and_mode_follow_what_the_agent_confirms() {
    let mut s = started();
    assert_eq!(s.model_label(), "Haiku 5.5");
    assert_eq!(s.choose_model("nope"), None);
    assert_eq!(s.choose_model("default"), Some(SessionCommand::SetModel("default".into())));
    assert_eq!(s.model_label(), "Haiku 5.5");
    s.apply(SessionEvent::ModelChanged("claude-fable-5-2".into()), 0.0);
    assert_eq!(s.model_label(), "Default");
    s.apply(SessionEvent::ModelChanged("claude-new-9".into()), 0.0);
    assert_eq!(s.model_label(), "claude-new-9");
    assert_eq!(s.choose_mode(PermissionMode::Ask), None);
    assert_eq!(s.choose_mode(PermissionMode::Plan), Some(SessionCommand::SetPermissionMode(PermissionMode::Plan)));
    assert_eq!(s.mode, PermissionMode::Ask);
    s.apply(SessionEvent::PermissionModeChanged(PermissionMode::Plan), 0.0);
    assert_eq!(s.mode, PermissionMode::Plan);
    assert_eq!(Session::opening("/p").model_label(), "Model");
}

#[test]
fn a_refused_command_is_explained_in_the_conversation() {
    let mut s = started();
    s.apply(SessionEvent::CommandFailed { what: CommandKind::SetModel, reason: "unknown model".into() }, 0.0);
    s.apply(SessionEvent::CommandFailed { what: CommandKind::RefreshUsage, reason: "x".into() }, 0.0);
    let notices: Vec<_> = s.conversation.items().iter().filter_map(|i| match &i.body {
        ItemBody::Notice { kind: NoticeKind::CommandFailed, text } => Some(text.as_str()), _ => None }).collect();
    assert_eq!(notices, vec!["Could not change the model: unknown model"]);
}

#[test]
fn the_wording_is_as_written() {
    assert_eq!(mode_label(&PermissionMode::Ask), "Ask before edits");
    assert_eq!(mode_label(&PermissionMode::Other("auto".into())), "auto");
    assert_eq!(answer_words(&Answer::Allow { always: true }), "Always allowed");
    assert_eq!(answer_words(&Answer::KeepPlanning { feedback: None }), "Kept planning");
    let answers = Answer::Answers(vec![
        QuestionAnswer { prompt: "a".into(), chosen: vec!["Red".into(), "Blue".into()], free_text: None },
        QuestionAnswer { prompt: "b".into(), chosen: vec![], free_text: Some("tall".into()) },
    ]);
    assert_eq!(answer_words(&answers), "Red, Blue, tall");
    assert_eq!(answer_words(&Answer::Answers(vec![])), "Answered");
}

#[test]
fn a_session_that_dies_while_opening_has_nothing_to_resume() {
    let mut s = Session::opening("/p");
    s.apply(SessionEvent::Exited { clean: false, detail: "bad flag".into() }, 0.0);
    let spec = s.restart_spec().unwrap();
    assert_eq!((spec.folder, spec.resume), (PathBuf::from("/p"), None));
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mr-core session`
Expected: FAIL to compile.

- [ ] **Step 3: Implement** the tables above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src && git commit -m "feat(core): a session's phases, queued messages and the user's intents"
```

---

### Task 8: The app, its screens and the usage text

**Files:**
- Create: `crates/core/src/app.rs`, `crates/core/src/usage.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: Task 7's `Session`.
- Produces, in `app.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen { Checking, NotInstalled, SignedOut, Main }

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Width { Points(f32), Unlimited }
pub const DEFAULT_WIDTH: f32 = 1000.0;
pub const MIN_WIDTH: f32 = 480.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    CheckAccount,
    Open { open: u64, spec: StartSpec },
    Send { open: u64, command: SessionCommand },
    SaveWidth(Width),
}

pub struct App {
    pub screen: Screen,
    pub account: Option<Account>,
    pub capabilities: Capabilities,
    pub usage: UsageLimits,
    pub folder: PathBuf,
    pub session: Option<Session>,
    pub width: Width,
    /* private: the number of the current open */
}

impl App {
    /// Starts on `Screen::Checking` and asks for the account.
    pub fn new(folder: PathBuf, capabilities: Capabilities, width: Width) -> (App, Vec<Effect>);
    pub fn current_open(&self) -> u64;
    pub fn account_checked(&mut self, account: Account) -> Vec<Effect>;
    pub fn check_again(&mut self) -> Vec<Effect>;
    pub fn open_failed(&mut self, open: u64, error: AgentError) -> Vec<Effect>;
    pub fn send_failed(&mut self, open: u64, command: &SessionCommand, error: AgentError) -> Vec<Effect>;
    pub fn event(&mut self, open: u64, event: SessionEvent, now: f64) -> Vec<Effect>;
    pub fn new_session(&mut self) -> Vec<Effect>;
    pub fn restart(&mut self) -> Vec<Effect>;
    pub fn submit(&mut self, text: &str) -> Vec<Effect>;
    pub fn stop(&mut self) -> Vec<Effect>;
    pub fn answer(&mut self, id: &RequestId, answer: Answer) -> Vec<Effect>;
    pub fn choose_model(&mut self, option_id: &str) -> Vec<Effect>;
    pub fn choose_mode(&mut self, mode: PermissionMode) -> Vec<Effect>;
    pub fn set_width(&mut self, width: Width) -> Vec<Effect>;
}
```

and in `usage.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level { Normal, Warning, Alarm }
#[derive(Clone, Debug, PartialEq)]
pub struct UsageLine { pub text: String, pub fraction: Option<f32>, pub level: Level, pub resets: Option<String> }
/// The five-hour line, then the weekly line.
pub fn usage_lines(limits: &UsageLimits, now_unix: i64) -> [UsageLine; 2];
pub fn resets_in(resets_at_unix: i64, now_unix: i64) -> String;
/// Name and plan for the footer, such as "alex · Max".
pub fn account_line(account: &Account) -> String;
```

Rules for `App`:

- **Opens are numbered.** Each new `Effect::Open` uses the next number, starting at 1. `event`, `open_failed` and `send_failed` do nothing and return nothing when `open` is not `current_open()`.
- `account_checked`: `Missing` gives `Screen::NotInstalled`; `SignedOut` gives `Screen::SignedOut`; `SignedIn` gives `Screen::Main` and, when there is no session, a new `Session::opening(folder)` with `Effect::Open { spec: StartSpec::new(folder) }`. The account is kept.
- `check_again`: only on `NotInstalled` and `SignedOut`; goes to `Checking` and returns `CheckAccount`.
- `open_failed`: `AgentError::NotInstalled` gives `Screen::NotInstalled` and no session. Any other error is treated as `Exited { clean: false, detail: error.to_string() }` for the session.
- `event`: a `UsageLimits` event updates `usage`, replacing only the windows that are `Some`. `Started` is followed by `Effect::Send { command: RefreshUsage }` when `capabilities.usage_limits`. Every event goes to the session, and the commands it returns become `Effect::Send`.
- `new_session`: only on `Screen::Main`. When the old session has not ended, `Effect::Send { open: old, command: Close }` comes first. Then a new open number, a new `Session::opening(folder)` and its `Effect::Open`.
- `restart`: only when the session has ended. A new open number, `session.reopening()`, and `Effect::Open` with `restart_spec()`.
- `send_failed`: `Closed` changes nothing, because `Exited` follows. `Protocol` for an `Answer` adds the notice "That request was no longer waiting." Anything else adds a `CommandFailed` notice with the error's text.
- `submit`, `stop`, `answer`, `choose_model`, `choose_mode` pass to the session and wrap its command in `Effect::Send` with the current open.
- `set_width`: `Points(p)` is raised to `MIN_WIDTH` when smaller. Returns `SaveWidth` only when the width changed.

Rules for the usage text:

- A known window reads `"{N}% of current session used"` and `"{N}% of weekly limit used"`, with `N` from `LimitWindow::percent()`. An unknown window reads `"Usage not known yet"` with `fraction: None` and `resets: None`.
- `Level`: `Normal` below 75, `Warning` from 75, `Alarm` from 90.
- `resets_in`: under a minute "Resets in under a minute"; under an hour "Resets in 14 min"; under a day "Resets in 2 h 14 min"; otherwise "Resets in 3 d 4 h"; a time that has passed "Resets soon".
- `account_line`: `SignedIn { name, plan: Some(p), .. }` gives `"{name} · {p}"`; without a plan, the name; `SignedOut` "Not signed in"; `Missing` "Claude Code not found".

- [ ] **Step 1: Write the failing tests.** In `usage.rs`:

```rust
#[test]
fn the_lines_read_as_the_design_says() {
    let limits = UsageLimits {
        session: Some(LimitWindow { used: 0.37, resets_at_unix: 10_000 + 2 * 3600 + 14 * 60 }),
        weekly: None,
    };
    let [session, weekly] = usage_lines(&limits, 10_000);
    assert_eq!(session, UsageLine {
        text: "37% of current session used".into(), fraction: Some(0.37), level: Level::Normal,
        resets: Some("Resets in 2 h 14 min".into()),
    });
    assert_eq!(weekly, UsageLine { text: "Usage not known yet".into(), fraction: None, level: Level::Normal, resets: None });
}

#[test]
fn the_level_rises_at_75_and_90() {
    let level = |used: f32| usage_lines(&UsageLimits { session: None, weekly: Some(LimitWindow { used, resets_at_unix: 0 }) }, 0)[1].level;
    assert_eq!((level(0.74), level(0.75), level(0.89), level(0.90), level(1.0)),
        (Level::Normal, Level::Warning, Level::Warning, Level::Alarm, Level::Alarm));
    let [_, weekly] = usage_lines(&UsageLimits { session: None, weekly: Some(LimitWindow { used: 1.0, resets_at_unix: 0 }) }, 0);
    assert_eq!(weekly.text, "100% of weekly limit used");
}

#[test]
fn reset_times_are_said_in_words() {
    assert_eq!(resets_in(1_030, 1_000), "Resets in under a minute");
    assert_eq!(resets_in(1_000 + 14 * 60, 1_000), "Resets in 14 min");
    assert_eq!(resets_in(1_000 + 3 * 86_400 + 4 * 3600 + 59, 1_000), "Resets in 3 d 4 h");
    assert_eq!(resets_in(999, 1_000), "Resets soon");
}

#[test]
fn the_account_line_names_the_person_and_plan() {
    let signed = |plan: Option<&str>| Account::SignedIn { name: "alex".into(), plan: plan.map(str::to_string), full_login: true };
    assert_eq!(account_line(&signed(Some("Max"))), "alex · Max");
    assert_eq!(account_line(&signed(None)), "alex");
    assert_eq!(account_line(&Account::SignedOut), "Not signed in");
    assert_eq!(account_line(&Account::Missing), "Claude Code not found");
}
```

In `app.rs`. Helpers: `caps()` is `Capabilities` with every field true; `signed_in()` is an `Account::SignedIn` for `alex`; `ready()` builds an app for `/p`, checks `signed_in()` into it and gives it `Started` for open 1 (reuse `info()` from the session tests by making that helper `pub(crate)` under `#[cfg(test)]`); it returns the app.

```rust
#[test]
fn the_first_screen_depends_on_the_account() {
    let (mut app, effects) = App::new("/p".into(), caps(), Width::Points(DEFAULT_WIDTH));
    assert_eq!((app.screen, effects), (Screen::Checking, vec![Effect::CheckAccount]));
    assert_eq!(app.account_checked(Account::Missing), vec![]);
    assert_eq!(app.screen, Screen::NotInstalled);
    assert_eq!(app.check_again(), vec![Effect::CheckAccount]);
    assert_eq!(app.screen, Screen::Checking);
    assert_eq!(app.account_checked(Account::SignedOut), vec![]);
    assert_eq!(app.screen, Screen::SignedOut);
    app.check_again();
    let effects = app.account_checked(signed_in());
    assert_eq!(effects, vec![Effect::Open { open: 1, spec: StartSpec::new("/p") }]);
    assert_eq!((app.screen, app.current_open()), (Screen::Main, 1));
    assert_eq!(app.session.as_ref().unwrap().phase, Phase::Opening);
    assert_eq!(app.check_again(), vec![]);
}

#[test]
fn a_started_session_asks_for_the_usage_limits() {
    let (mut app, _) = App::new("/p".into(), caps(), Width::Unlimited);
    app.account_checked(signed_in());
    let effects = app.event(1, SessionEvent::Started(info()), 0.0);
    assert_eq!(effects, vec![Effect::Send { open: 1, command: SessionCommand::RefreshUsage }]);
    let mut without = caps();
    without.usage_limits = false;
    let (mut app, _) = App::new("/p".into(), without, Width::Unlimited);
    app.account_checked(signed_in());
    assert_eq!(app.event(1, SessionEvent::Started(info()), 0.0), vec![]);
}

#[test]
fn usage_keeps_a_window_that_was_not_reported_this_time() {
    let mut app = ready();
    let window = |used| Some(LimitWindow { used, resets_at_unix: 5 });
    app.event(1, SessionEvent::UsageLimits(UsageLimits { session: window(0.2), weekly: window(0.4) }), 0.0);
    app.event(1, SessionEvent::UsageLimits(UsageLimits { session: window(0.3), weekly: None }), 0.0);
    assert_eq!(app.usage, UsageLimits { session: window(0.3), weekly: window(0.4) });
}

#[test]
fn a_replaced_session_can_no_longer_change_anything() {
    let mut app = ready();
    let sent = app.submit("go");
    assert_eq!(sent, vec![Effect::Send { open: 1, command: SessionCommand::Send { text: "go".into(), attachments: vec![] } }]);
    let effects = app.new_session();
    assert_eq!(effects, vec![
        Effect::Send { open: 1, command: SessionCommand::Close },
        Effect::Open { open: 2, spec: StartSpec::new("/p") },
    ]);
    assert!(app.session.as_ref().unwrap().conversation.items().is_empty());
    for stale in [
        SessionEvent::TextDelta("late".into()),
        SessionEvent::UsageLimits(UsageLimits { session: Some(LimitWindow { used: 0.9, resets_at_unix: 1 }), weekly: None }),
        SessionEvent::Exited { clean: true, detail: String::new() },
    ] {
        assert_eq!(app.event(1, stale, 0.0), vec![]);
    }
    assert_eq!(app.open_failed(1, AgentError::Io("x".into())), vec![]);
    let session = app.session.as_ref().unwrap();
    assert_eq!(session.phase, Phase::Opening);
    assert!(session.conversation.items().is_empty());
    assert_eq!(app.usage, UsageLimits { session: None, weekly: None });
}

#[test]
fn an_ended_session_restarts_where_it_was() {
    let mut app = ready();
    app.submit("go");
    app.event(1, SessionEvent::Exited { clean: false, detail: "killed".into() }, 0.0);
    assert_eq!(app.new_session().len(), 1, "an ended session is not sent Close");
    let mut app = ready();
    app.submit("go");
    assert_eq!(app.restart(), vec![], "a live session cannot be restarted");
    app.event(1, SessionEvent::Exited { clean: false, detail: "killed".into() }, 0.0);
    let effects = app.restart();
    let mut spec = StartSpec::new("/p");
    spec.resume = Some(SessionId("s1".into()));
    assert_eq!(effects, vec![Effect::Open { open: 2, spec }]);
    assert!(app.session.as_ref().unwrap().conversation.items().len() >= 2);
}

#[test]
fn a_failed_open_shows_where_the_problem_is() {
    let (mut app, _) = App::new("/p".into(), caps(), Width::Unlimited);
    app.account_checked(signed_in());
    app.open_failed(1, AgentError::Io("no such folder".into()));
    assert_eq!(app.session.as_ref().unwrap().phase, Phase::Ended { clean: false, detail: "i/o error: no such folder".into() });
    let (mut app, _) = App::new("/p".into(), caps(), Width::Unlimited);
    app.account_checked(signed_in());
    app.open_failed(1, AgentError::NotInstalled);
    assert_eq!(app.screen, Screen::NotInstalled);
    assert!(app.session.is_none());
}

#[test]
fn a_refused_answer_is_explained_and_a_closed_session_is_not() {
    let mut app = ready();
    let answer = SessionCommand::Answer { id: RequestId("r1".into()), answer: Answer::Allow { always: false } };
    app.send_failed(1, &answer, AgentError::Closed);
    assert!(app.session.as_ref().unwrap().conversation.items().is_empty());
    app.send_failed(1, &answer, AgentError::Protocol("not waiting".into()));
    let last = app.session.as_ref().unwrap().conversation.items().last().unwrap().body.clone();
    assert_eq!(last, ItemBody::Notice { kind: NoticeKind::CommandFailed, text: "That request was no longer waiting.".into() });
}

#[test]
fn the_width_has_a_floor_and_saves_only_changes() {
    let mut app = ready();
    assert_eq!(app.set_width(Width::Points(100.0)), vec![Effect::SaveWidth(Width::Points(MIN_WIDTH))]);
    assert_eq!(app.set_width(Width::Points(MIN_WIDTH)), vec![]);
    assert_eq!(app.set_width(Width::Unlimited), vec![Effect::SaveWidth(Width::Unlimited)]);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mr-core`
Expected: FAIL to compile.

- [ ] **Step 3: Implement** the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mr-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src && git commit -m "feat(core): app screens, numbered opens, usage wording"
```

---

### Task 9: The worker thread between the window and the agent

**Files:**
- Create: `crates/core/src/backend.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `mr_agent::{Agent, SessionHandle, EventSink, FakeAgent}`.
- Produces:

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum ToBackend {
    CheckAccount,
    Open { open: u64, spec: StartSpec },
    Send { open: u64, command: SessionCommand },
}
#[derive(Clone, Debug, PartialEq)]
pub enum FromBackend {
    Account(Account),
    OpenFailed { open: u64, error: AgentError },
    Event { open: u64, event: SessionEvent },
    SendFailed { open: u64, command: SessionCommand, error: AgentError },
}
pub type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct Backend { /* private */ }
impl Backend {
    /// Starts the worker thread. `wake` is called after every message is queued for the window.
    pub fn start(agent: Arc<dyn Agent>, wake: Wake) -> Backend;
    /// Never blocks.
    pub fn send(&self, message: ToBackend);
    /// Never blocks.
    pub fn try_recv(&self) -> Option<FromBackend>;
    /// Waits until the worker has handled every message sent so far. For tests and
    /// the scripted demo only: with a real agent this can block for seconds.
    pub fn flush(&self);
}
```

One worker thread takes `ToBackend` messages in order and makes every blocking call: `agent.account()`, `agent.open(..)` and `handle.send(..)`. Each open's sink tags its events with the open's number, queues them and calls `wake`; it runs on the agent's own thread. A `Send` for an open the worker does not hold gives `SendFailed` with `AgentError::Closed`. A handle is dropped when its `send` returns `Closed` or after `Close` was passed on. Dropping the `Backend` passes `Close` to every session it still holds and lets the worker end without waiting for it.

Note for plan 3: with one worker an `open` that blocks on the sign-in check delays commands to other sessions. Plan 3 gives each open its own thread.

- [ ] **Step 1: Write the failing tests** in `backend.rs`. The worker is asynchronous and `FakeAgent` delivers only on `advance`, so the helper pumps both:

```rust
fn next(fake: &FakeAgent, backend: &Backend) -> FromBackend {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        fake.advance(0.0);
        if let Some(message) = backend.try_recv() { return message; }
        assert!(std::time::Instant::now() < deadline, "nothing arrived");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
fn start(fake: &FakeAgent) -> (Backend, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let backend = Backend::start(Arc::new(fake.clone()), Arc::new(move || { counter.fetch_add(1, Ordering::SeqCst); }));
    (backend, wakes)
}

#[test]
fn the_account_comes_back_and_wakes_the_window() {
    let fake = FakeAgent::new();
    let account = Account::SignedIn { name: "alex".into(), plan: Some("Max".into()), full_login: true };
    fake.set_account(account.clone());
    let (backend, wakes) = start(&fake);
    backend.send(ToBackend::CheckAccount);
    assert_eq!(next(&fake, &backend), FromBackend::Account(account));
    assert!(wakes.load(Ordering::SeqCst) >= 1);
}

#[test]
fn events_carry_the_number_of_their_open() {
    let fake = FakeAgent::new();
    let (backend, _) = start(&fake);
    backend.send(ToBackend::Open { open: 7, spec: StartSpec::new("/p") });
    let FromBackend::Event { open: 7, event: SessionEvent::Started(first) } = next(&fake, &backend) else { panic!() };
    backend.send(ToBackend::Open { open: 8, spec: StartSpec::new("/q") });
    let FromBackend::Event { open: 8, event: SessionEvent::Started(second) } = next(&fake, &backend) else { panic!() };
    assert_ne!(first.id, second.id);
    fake.emit(&first.id, SessionEvent::TextDelta("late".into()));
    assert_eq!(next(&fake, &backend), FromBackend::Event { open: 7, event: SessionEvent::TextDelta("late".into()) });
}

#[test]
fn commands_reach_the_right_session_and_failures_come_back() {
    let fake = FakeAgent::new();
    let (backend, _) = start(&fake);
    backend.send(ToBackend::Open { open: 1, spec: StartSpec::new("/p") });
    let FromBackend::Event { event: SessionEvent::Started(info), .. } = next(&fake, &backend) else { panic!() };
    backend.send(ToBackend::Send { open: 1, command: SessionCommand::Stop });
    let answer = SessionCommand::Answer { id: RequestId("r9".into()), answer: Answer::Allow { always: false } };
    backend.send(ToBackend::Send { open: 1, command: answer.clone() });
    let FromBackend::SendFailed { open: 1, command, error: AgentError::Protocol(_) } = next(&fake, &backend) else { panic!() };
    assert_eq!(command, answer);
    assert_eq!(fake.sent(), vec![(info.id, SessionCommand::Stop)]);
    backend.send(ToBackend::Send { open: 99, command: SessionCommand::Stop });
    assert_eq!(next(&fake, &backend), FromBackend::SendFailed { open: 99, command: SessionCommand::Stop, error: AgentError::Closed });
}

#[test]
fn a_failed_open_is_reported_with_its_number() {
    struct Absent;
    impl Agent for Absent { /* capabilities: all false; account: Missing; lists: empty; open: Err(AgentError::NotInstalled) */ }
    let (backend, _) = start_with(Arc::new(Absent));
    backend.send(ToBackend::Open { open: 3, spec: StartSpec::new("/p") });
    assert_eq!(wait(&backend), FromBackend::OpenFailed { open: 3, error: AgentError::NotInstalled });
}

#[test]
fn dropping_the_backend_closes_its_sessions() {
    let fake = FakeAgent::new();
    let (backend, _) = start(&fake);
    backend.send(ToBackend::Open { open: 1, spec: StartSpec::new("/p") });
    let FromBackend::Event { event: SessionEvent::Started(info), .. } = next(&fake, &backend) else { panic!() };
    drop(backend);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !fake.sent().contains(&(info.id.clone(), SessionCommand::Close)) {
        assert!(std::time::Instant::now() < deadline, "the session was not closed");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
```

`start_with` and `wait` are the same helpers without a fake to pump. Write `Absent` out in full. Add one more test for `flush`:

```rust
#[test]
fn after_a_flush_the_worker_has_done_what_was_asked() {
    let fake = FakeAgent::new();
    let (backend, _) = start(&fake);
    backend.send(ToBackend::Open { open: 1, spec: StartSpec::new("/p") });
    backend.flush();
    fake.advance(0.0);
    assert!(matches!(backend.try_recv(), Some(FromBackend::Event { open: 1, event: SessionEvent::Started(_) })));
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mr-core backend`
Expected: FAIL to compile.

- [ ] **Step 3: Implement.** Channels are `std::sync::mpsc`. The worker owns a `HashMap<u64, Box<dyn SessionHandle>>`.

- [ ] **Step 4: Run the tests twenty times,** because this is the one place with real threads

Run: `for i in $(seq 20); do cargo test -p mr-core backend -q 2>&1 | tail -1; done`
Expected: twenty lines that each say `ok`.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src && git commit -m "feat(core): one worker thread between the window and the agent"
```

---

## Rune facts for Tasks 10 to 17

Checked against the pinned source on 2026-11-11. Paths are relative to the Rune checkout named in Global Constraints. Read the file before using an API; where this section and the source differ, the source wins and the report says so.

- **Read first:** `crates/rune/README.md` (the sections on layout, "Smooth scrolling", "Launching fast" and "Agent tooling"), `examples/web-hello/src/lib.rs` (a small chat: messages in a bottom-following scroll, an input that submits), `crates/rune-kit/examples/shell.rs` (a desktop agent-chat shell with `SplitView`, `Sidebar` and `rune::extra_args()`), `examples/live-values/src/main.rs` (a channel drained in `poll`).
- **App.** `AppSpec` is a struct of plain `fn` pointers, built as a literal ending in `..AppSpec::new(title, size, init, view)`. Its fields include `min_size`, `scenes: &'static [(&'static str, fn(&mut S))]`, `fonts`, `families`, `setup: Option<fn(&mut S, Waker)>`, `poll: Option<fn(&mut S, f64, &mut Tx) -> bool>` and `on_key: Option<fn(&mut S, &KeyEvent, &mut Tx) -> bool>` (`crates/rune/src/tooling.rs`). `init` cannot capture; `rune::extra_args()` returns the command-line arguments Rune did not recognise and can be read inside `init`.
- **State changes** happen only in handlers (`impl Fn(&mut S)` or `impl Fn(&mut S, &mut Tx)`, which may capture) and in `poll`. The whole view re-runs after a handler, and after a `poll` that returns true. `poll` must return false when nothing changed. `Tx::wake_after(Duration)` asks for a later `poll`; `Tx::clock()` is the time in seconds.
- **Tests.** `Runtime::new(state, view, size, Env { system_dark, scale })` does not install `setup`, `poll` or `on_key`; set `rt.poll` and `rt.on_key` (public fields) and call the setup function yourself. `rt.frame(dt)` runs `poll` and commits. `rt.find(key)` gives an element's centre. `rt.elements()` lists elements with `key`, `text`, `focused`, `visible`. There is no click or typing helper: a click is `pointer_move`, `pointer_down`, `pointer_up`; typing is one `KeyEvent { key: Key::Text(..), .. }` per character, or `rt.paste`. `rt.scroll_at_bottom(key)` and `rt.wheel(x, y, dy)` exist. `rt.use_clipboard(&rune::clipboard::memory())` keeps tests off the real clipboard.
- **Headless.** `mindrune --demo --scene NAME --snap out.png --tree out.json --size 1200x800 [--theme light]`. The tree's `warnings` lists duplicate keys.
- **Layout.** A scroll needs a bounded height: `.grow(1.0)` and `.min_h(0.0)` on every column between the window and the scroll. `text()` does not wrap and is white unless told otherwise: use `.wrap()` and `.color(..)`.
- **Scrolling.** `.scroll(ScrollSpec { stick_bottom: true })` follows new content until the user scrolls up. `El::on_at_bottom(|s, bool|)` reports it; `tx.scroll_to_bottom(key)` returns to the end. Long content is a growth stack of virtual rows: `col().growth(GrowthSpec::default()).virtual_rows(count, estimated_height, key_fn, row_fn)` with `.virtual_rows_version(v)`; the growing last row is marked `.streaming(true)` (`crates/rune/src/el/virtual_rows.rs`, `el/growth.rs`). Rows use padding inside, never margins or gaps, and their handlers find the item by id, not by index.
- **Text input.** `TextEdit` lives in state (`.text`, `.clear()`, `.insert(..)`, `focused`). `rune_kit::TextArea::new(&edit).placeholder(..).width(None).auto_grow(min, max).disabled(bool).on_edit(|s| &mut s.draft).on_submit(|s, tx| ..).on_key(|s, &KeyEvent, tx| -> bool).build(&kit)`. In a multi-line field plain Enter calls `on_submit`, Shift+Enter adds a line. **A focused text field swallows Escape**, so Escape must be caught in `TextArea::on_key` as well as in `AppSpec::on_key`. `KeyEvent` implements `Drop`: match on `&ev.key`.
- **Kit.** Every component is built with `.build(&kit)`; overlays (`Select`, `Menu`, `Dialog`) also take `&mut Vec<El>`, and the root view returns `rune_kit::root(base, layer)`. `Kit::neutral(dark)` builds a kit by value (`crates/rune-kit/src/neutral.rs`). Useful parts: `Button`, `IconButton`, `Select::new(key, &labels).current(..).open(bool).on_toggle(..).on_close(..).on_choose(|s, &str, tx| ..)` whose trigger is keyed `"{key}-trigger"`, `Spinner`, `Collapsible`, `shell::{SplitView, Sidebar, SidebarRow, PaneState, RowStatus}`.
- **Markdown.** `rune_kit::Markdown::new(whole_text).streaming(bool).selectable(true).build(&kit)`. There is no incremental feed: pass the whole text each time. An unclosed code fence draws as a growing code block. A ```` ```diff ```` fence colours added and removed lines. `.code_frame(..)` customises the frame around a code block. Several on screen need distinct `.key(..)`s.
- **Other.** `tx.copy(text)` copies. `El::on_drag(|s, &mut Drag, tx|)` gives `x`, `start_x`, `dx` and a phase. `cx.layout_size(key)` reads an element's last size. `rune::prefs::Prefs` has `open(app_id)`, `memory()`, `get(key)`, `set(key, &value)`.
- **Icons and fonts.** `rune_theme_opencode::icons::*` (for example `ARROW_UP`, `STOP`, `COPY`, `CHECK`, `CHEVRON_DOWN`, `TERMINAL`, `WARNING`, `PLUS`) and `rune_theme_opencode::{FONTS, FAMILIES}` (Inter and IBM Plex Mono). Colours do not come from that crate.

---

### Task 10: The window's state and the demo window

This task replaces the smoke window with a window that reaches the right screen and shows only its name. Every later task adds views to it and scenes to the demo.

**Files:**
- Modify: `crates/app/Cargo.toml`, `crates/app/src/main.rs`
- Create: `crates/app/src/glue.rs`, `crates/app/src/demo.rs`, `crates/app/src/testkit.rs` (compiled only for tests), `crates/app/src/view/mod.rs`

**Interfaces:**
- Consumes: `mr_core::{App, Backend, Effect, FromBackend, ToBackend, Width, ItemId}`; `mr_agent::{Agent, FakeAgent}`.
- Produces, in `glue.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picker { Model, Mode }

pub struct Ui {
    pub app: App,
    pub agent: Arc<dyn Agent>,
    /// Present in the demo window and in tests: the agent, as the fake it is.
    pub fake: Option<FakeAgent>,
    pub backend: Option<Backend>,
    pub prefs: Prefs,
    pub draft: TextEdit,
    pub sidebar: PaneState,
    pub open_picker: Option<Picker>,
    /// Items the user has opened or closed, against each kind's default.
    pub toggled: HashSet<ItemId>,
    /// Items whose long output the user asked to see whole.
    pub whole: HashSet<ItemId>,
    /// The conversation is scrolled to its end.
    pub following: bool,
    pub now_unix: i64,
    pub unix_clock: fn() -> i64,
    /* private: effects waiting for setup, the time of the last poll */
}

impl Ui {
    pub fn new(agent: Arc<dyn Agent>, fake: Option<FakeAgent>, folder: PathBuf, prefs: Prefs) -> Ui;
}
/// `AppSpec::setup`: starts the backend with the window's waker and carries out what was waiting.
pub fn setup(ui: &mut Ui, waker: Waker);
/// `AppSpec::poll`.
pub fn poll(ui: &mut Ui, now: f64, tx: &mut Tx) -> bool;
/// Carries effects out: backend messages are sent, the width is saved.
pub fn run(ui: &mut Ui, effects: Vec<Effect>);
```

in `demo.rs`:

```rust
/// A window state over a `FakeAgent` signed in as alex on the Max plan, with settings kept in memory.
pub fn ui() -> Ui;
/// Advances the fake by `seconds` and takes in everything it produced. Demo and tests only.
pub fn pump(ui: &mut Ui, seconds: f32) -> bool;
pub const SCENES: &[(&str, fn(&mut Ui))];
```

and in `view/mod.rs`: `pub fn view(ui: &Ui, cx: &mut Cx<Ui>) -> El` and `pub fn on_key(ui: &mut Ui, ev: &KeyEvent, tx: &mut Tx) -> bool` (returns false for now).

Rules:

- `Ui::new` reads the width from prefs key `"conversation_width"` (a `Width`; default `Width::Points(DEFAULT_WIDTH)`), builds the `App` and keeps its first effects until `setup`.
- `run`: `CheckAccount`, `Open` and `Send` become `ToBackend` messages; `SaveWidth(w)` is written to prefs key `"conversation_width"`. Effects that arrive before `setup` wait.
- `poll` takes every waiting `FromBackend` in one go, gives each to the `App` (`Account` to `account_checked`, `OpenFailed` to `open_failed`, `Event` to `event` with `now`, `SendFailed` to `send_failed`) and runs the effects that come back. It returns true when at least one message was taken in, or when `now_unix / 60` changed since the last poll while a usage window is known. When a usage window is known it calls `tx.wake_after` for 30 seconds, so the reset wording stays current.
- In the demo (`fake` is `Some`), `poll` first calls `pump` with the time since the last poll. `pump` is: `backend.flush()`, `fake.advance(seconds)`, take everything in, `backend.flush()`. This makes the demo deterministic although the backend has a thread. In the demo, time is the fake's: `pump` adds `seconds` to the clock that events are stamped with, so a scene that emits `ThinkingStarted`, pumps 0 seconds, emits `BlockFinished` and pumps 4 seconds shows "Thought for 4 s".
- `main.rs`: `mindrune [FOLDER] [--demo]`, read from `rune::extra_args()`. `FOLDER` defaults to the current directory and must be an existing directory, otherwise the program prints `mindrune: not a folder: {path}` to stderr and exits with status 2 before any window opens. Without `--demo`: `ClaudeAgent::from_environment()` and `Prefs::open("mindrune")`. With `--demo`: `demo::ui()`. The `AppSpec` has title `"MindRune"`, size `(1200.0, 800.0)`, `min_size: Some((560.0, 400.0))`, `scenes: demo::SCENES`, the fonts and families from `rune_theme_opencode`, and `setup`, `poll`, `on_key` set.
- Every scene function first replaces the state with `demo::ui()` and starts its backend with a waker that does nothing, so a scene never touches the real agent. Scenes in this task: `"checking"` (nothing pumped), `"signed-out"` and `"not-installed"` (the fake's account set before pumping), `"empty"` (pumped until the session is `Idle`; the fake has answered the usage request with 37% and 10%, resetting 2 h 14 min and 3 d 4 h after `now_unix`). In the demo `unix_clock` is fixed at `1_800_000_000`.
- The view for now: a full-size column keyed `mr-root`, filled with the kit's base colour, holding one text keyed `mr-screen` that reads `checking`, `not-installed`, `signed-out` or `main`.
- `testkit.rs`: `window(scene: &str) -> Runtime<Ui>` (1200 by 800, dark, scale 1.0, `poll` and `on_key` installed, in-memory clipboard, the scene run, three frames stepped); `click(rt, key)` (panics with the key when it is not found); `type_text(rt, text)`; `press(rt, Key)`; `press_with(rt, Key, Mods)`; `step(rt)` (three frames of 16 ms); `texts(rt) -> Vec<String>` (the text of every visible element); `has_text(rt, part) -> bool`; `sent(rt) -> Vec<SessionCommand>` (the fake's record, without session ids); `emit(rt, SessionEvent)` (to the current session, then `step`).

How a scene is built. `FakeAgent` does not answer `RefreshUsage` by itself, so the scene emits the `UsageLimits` event after the session started. A scene that shows a turn first scripts the turn with `FakeAgent::reply` (a `Send` with nothing scripted fails the turn), then sends the user's message through `ui.app.submit` and `run`, then pumps. Steps scripted for later than the scene pumps stay undelivered, which is how a scene stops in the middle of a turn. Events outside a turn are given with `FakeAgent::emit`.

- [ ] **Step 1: Write the failing tests** in `glue.rs`

```rust
#[test]
fn the_demo_window_reaches_the_main_screen_and_asks_for_usage() {
    let rt = testkit::window("empty");
    assert_eq!(rt.state.app.screen, Screen::Main);
    assert_eq!(rt.state.app.session.as_ref().unwrap().phase, Phase::Idle);
    assert!(testkit::sent(&rt).contains(&SessionCommand::RefreshUsage));
    assert!(testkit::has_text(&rt, "main"));
    assert_eq!(rt.state.app.usage.session.as_ref().unwrap().percent(), 37);
}

#[test]
fn each_account_state_has_its_screen() {
    assert!(testkit::has_text(&testkit::window("checking"), "checking"));
    assert!(testkit::has_text(&testkit::window("signed-out"), "signed-out"));
    assert!(testkit::has_text(&testkit::window("not-installed"), "not-installed"));
}

#[test]
fn nothing_arriving_means_nothing_to_redraw() {
    let mut rt = testkit::window("empty");
    assert!(!demo::pump(&mut rt.state, 0.0));
    testkit::emit(&mut rt, SessionEvent::TitleChanged("Fix the parser".into()));
    assert_eq!(rt.state.app.session.as_ref().unwrap().title.as_deref(), Some("Fix the parser"));
}

#[test]
fn the_width_is_read_from_settings_and_written_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prefs.json");
    let mut prefs = Prefs::at(&path);
    prefs.set("conversation_width", &Width::Points(720.0)).unwrap();
    let fake = FakeAgent::new();
    let mut ui = Ui::new(Arc::new(fake.clone()), Some(fake), "/p".into(), Prefs::at(&path));
    assert_eq!(ui.app.width, Width::Points(720.0));
    let effects = ui.app.set_width(Width::Unlimited);
    run(&mut ui, effects);
    assert_eq!(Prefs::at(&path).get::<Width>("conversation_width"), Some(Width::Unlimited));
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mindrune`
Expected: FAIL to compile.

- [ ] **Step 3: Add the dependencies** to `crates/app/Cargo.toml`: `mr-agent`, `mr-core` and `mr-claude` by path; `rune-kit` with `features = ["markdown"]` and `rune-theme-opencode`, both from the same git URL and `rev` as `rune`; `serde`; `tempfile` as a dev-dependency. The first build of the kit takes several minutes.

- [ ] **Step 4: Implement** `glue.rs`, `demo.rs`, `testkit.rs`, the placeholder view and `main.rs`. Remove the smoke test and its `app-title` element.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p mindrune`
Expected: PASS.

- [ ] **Step 6: Look at it headless**

Run: `cargo run -p mindrune -- --demo --scene empty --snap target/snap/t10.png --tree target/snap/t10.json && grep -c mr-screen target/snap/t10.json`
Expected: prints at least `1`. Open `target/snap/t10.png` and confirm it shows the word `main`. Then run `cargo run -p mindrune -- /no/such/folder; echo $?` and confirm the message and `2`.

- [ ] **Step 7: Commit**

```bash
git add crates/app Cargo.lock && git commit -m "feat(app): window state over the core, with a scripted demo window"
```

---

### Task 11: The shell, the usage footer and the sign-in screens

**Files:**
- Create: `crates/app/src/theme.rs`, `crates/app/src/view/shell.rs`, `crates/app/src/view/signin.rs`
- Modify: `crates/app/src/view/mod.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: `mr_core::{usage_lines, account_line, Level, Screen, Phase}`; Task 10's `Ui`.
- Produces:
  - `theme::kit(dark: bool) -> &'static Kit`, the kit's neutral theme built once per setting; `theme::level_color(kit: &Kit, level: Level) -> Color` (the muted text colour, the warning colour, the danger colour).
  - `shell::shell(ui: &Ui, kit: &Kit, main: El) -> El`: a `SplitView` with the sidebar on the left and `main` in the middle.
  - `signin::screen(ui: &Ui, kit: &Kit) -> El` for `Checking`, `SignedOut` and `NotInstalled`.
  - `view` picks the kit from `cx.system_dark()`, shows `signin::screen` or the shell, and returns `rune_kit::root(base, layer)`. The middle of the shell is, for now, a column keyed `mr-tile` holding an empty area.

The sidebar, top to bottom:

- A button "New session", keyed `mr-new-session`. Pressing it runs `ui.app.new_session()`.
- One row for the current session, keyed `mr-session-row`: its title, or "New session" when it has none, with the folder's name as the subtitle. Its status mark is busy in `Opening` and `Working`, attention in `NeedsUser`, none otherwise. This row is where plan 3's list will grow.
- The footer, keyed `mr-footer`, always visible at the bottom:
  - the account line (`account_line`), keyed `mr-account`;
  - for each of the two usage lines: the text (keyed `mr-usage-session` and `mr-usage-weekly`), a thin bar filled to the line's fraction in `level_color`, and the reset wording beneath in the faint text colour. An unknown line shows only its text and an empty bar.

The sidebar can be dragged wider and collapsed with the kit's own controls. `on_key` handles the kit's sidebar toggle (`shell::is_sidebar_toggle`).

The sign-in screens, each centred in the window, with exactly this wording:

| Screen | Key | Title | Body | Button |
| --- | --- | --- | --- | --- |
| `Checking` | `mr-screen-checking` | "Checking your Claude Code sign-in…" with a spinner | none | none |
| `SignedOut` | `mr-screen-signin` | "Claude Code is not signed in" | "Run this in a terminal, then check again:" followed by `claude auth login` in the monospace font with a copy button keyed `mr-copy-command` | "Check again", keyed `mr-check-again` |
| `NotInstalled` | `mr-screen-missing` | "Claude Code is not installed" | "MindRune runs the `claude` program and could not find it. Install Claude Code, then check again." | "Check again", keyed `mr-check-again` |

"Check again" runs `ui.app.check_again()`. Add the scene `"limit"`: like `"empty"`, with the session window at 100% and the weekly window at 82%.

- [ ] **Step 1: Write the failing tests** in `view/shell.rs` and `view/signin.rs`

```rust
#[test]
fn the_footer_always_shows_who_and_how_much() {
    let rt = testkit::window("empty");
    for part in ["alex · Max", "37% of current session used", "Resets in 2 h 14 min",
                 "10% of weekly limit used", "Resets in 3 d 4 h"] {
        assert!(testkit::has_text(&rt, part), "{part}");
    }
    let rt = testkit::window("limit");
    assert!(testkit::has_text(&rt, "100% of current session used"));
}

#[test]
fn before_any_numbers_the_footer_says_so() {
    let mut rt = testkit::window("empty");
    rt.state.app.usage = UsageLimits { session: None, weekly: None };
    rt.invalidate();
    testkit::step(&mut rt);
    let unknown = testkit::texts(&rt).iter().filter(|t| t.as_str() == "Usage not known yet").count();
    assert_eq!(unknown, 2);
}

#[test]
fn new_session_replaces_the_session() {
    let mut rt = testkit::window("empty");
    testkit::click(&mut rt, "mr-new-session");
    testkit::step(&mut rt);
    assert_eq!(rt.state.app.current_open(), 2);
    assert!(testkit::sent(&rt).contains(&SessionCommand::Close));
}

#[test]
fn the_session_row_shows_the_sessions_title() {
    let mut rt = testkit::window("empty");
    assert!(testkit::has_text(&rt, "New session"));
    testkit::emit(&mut rt, SessionEvent::TitleChanged("Fix the parser".into()));
    assert!(testkit::has_text(&rt, "Fix the parser"));
}
```

```rust
#[test]
fn signed_out_says_what_to_run_and_checks_again() {
    let mut rt = testkit::window("signed-out");
    for part in ["Claude Code is not signed in", "Run this in a terminal, then check again:", "claude auth login"] {
        assert!(testkit::has_text(&rt, part), "{part}");
    }
    testkit::click(&mut rt, "mr-check-again");
    testkit::step(&mut rt);
    assert_eq!(rt.state.app.screen, Screen::SignedOut, "still signed out");
    let account = Account::SignedIn { name: "alex".into(), plan: Some("Max".into()), full_login: true };
    rt.state.fake.as_ref().unwrap().set_account(account);
    testkit::click(&mut rt, "mr-check-again");
    testkit::step(&mut rt);
    assert_eq!(rt.state.app.screen, Screen::Main);
    assert!(rt.find("mr-footer").is_some());
}

#[test]
fn not_installed_has_its_own_words() {
    let rt = testkit::window("not-installed");
    assert!(testkit::has_text(&rt, "Claude Code is not installed"));
    assert!(rt.find("mr-check-again").is_some());
    assert!(rt.find("mr-footer").is_none());
}

#[test]
fn the_command_can_be_copied() {
    let mut rt = testkit::window("signed-out");
    testkit::click(&mut rt, "mr-copy-command");
    assert_eq!(testkit::clipboard(&rt).as_deref(), Some("claude auth login"));
}
```

Add `clipboard(rt) -> Option<String>` to the test kit, reading the in-memory clipboard the window was given.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mindrune`
Expected: FAIL.

- [ ] **Step 3: Implement.** Remove the `mr-screen` placeholder text and update Task 10's tests that read it to check `rt.find("mr-screen-checking")`, `mr-screen-signin`, `mr-screen-missing` and `mr-tile` instead.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mindrune`
Expected: PASS.

- [ ] **Step 5: Look at it, dark and light**

Run: `for s in empty limit signed-out not-installed; do cargo run -q -p mindrune -- --demo --scene $s --snap target/snap/t11-$s.png; done; cargo run -q -p mindrune -- --demo --scene empty --theme light --snap target/snap/t11-empty-light.png --tree target/snap/t11.json`
Expected: five images. Open each. Check: the footer is at the bottom left and readable in both themes; the 100% bar is in the danger colour and the 82% bar in the warning colour; nothing overlaps or is cut off; `"warnings"` in `t11.json` is empty.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src && git commit -m "feat(app): shell with usage footer; sign-in and not-installed screens"
```

---

### Task 12: The conversation follows the reply

**Files:**
- Create: `crates/app/src/view/conversation.rs`, `crates/app/src/view/items.rs`
- Modify: `crates/app/src/view/mod.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: `mr_core::{Conversation, Item, ItemBody}`.
- Produces:
  - `conversation::conversation(ui: &Ui, kit: &Kit) -> El`: the scrolling list for the current session, keyed `mr-conversation`, placed in `mr-tile` and taking all the height the input box will not need.
  - `items::row(ui: &Ui, item: &Item, kit: &Kit) -> El`: one item, keyed `mr-item-{id}`. In this task it draws `User` and `Reply`; every other kind draws as an empty row until Tasks 13 and 14.

Rules:

- The list is a growth stack of virtual rows, one per item, versioned by `conversation.revision()`. A row whose item is a streaming reply is marked as the streaming row. A row that is not streaming is wrapped in `rune::memo` keyed by the item id and versioned by what it draws from: the conversation's revision is too coarse, so version it by a hash of the item's body and of whether the item is in `ui.toggled` and `ui.whole`.
- The scroll sticks to the bottom. `on_at_bottom` keeps `ui.following` current. When `following` is false a button "Jump to latest", keyed `mr-jump-latest`, floats over the bottom of the list; pressing it calls `tx.scroll_to_bottom("mr-conversation")`.
- A user message is plain, selectable text in a filled, rounded box at the right of the column. It is never parsed as Markdown.
- A reply is `Markdown` over the whole accumulated text, `streaming` while the item is, selectable, keyed by the item.
- With no items, the list shows one centred line: "What should Claude do in {folder name}?" keyed `mr-empty-hint`.
- Rows have their spacing as padding inside the row.

Scenes: `"conversation"` (the user's message `**hi**, what is in this folder?`, then a finished reply with a heading, a list, a table and a Rust code block; write the Markdown out in `demo.rs`), `"streaming"` (the same reply cut off inside the code block, still streaming), `"long"` (2,000 finished replies, numbered `Reply 1` to `Reply 2000`, built by emitting events before the first frame).

- [ ] **Step 1: Write the failing tests** in `view/conversation.rs`

```rust
fn item_keys(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements().into_iter().filter_map(|e| e.key).filter(|k| k.starts_with("mr-item-")).collect()
}

#[test]
fn an_empty_session_asks_what_to_do() {
    let rt = testkit::window("empty");
    assert!(rt.find("mr-empty-hint").is_some());
    assert!(testkit::texts(&rt).iter().any(|t| t.starts_with("What should Claude do in ")));
}

#[test]
fn the_users_message_is_shown_as_typed_and_the_reply_as_markdown() {
    let rt = testkit::window("conversation");
    assert!(testkit::has_text(&rt, "**hi**, what is in this folder?"));
    assert!(rt.find("mr-empty-hint").is_none());
    assert!(!testkit::texts(&rt).iter().any(|t| t.contains("```")), "a fence must not show as text");
}

#[test]
fn a_reply_cut_off_inside_a_code_block_still_draws() {
    let rt = testkit::window("streaming");
    assert_eq!(item_keys(&rt).len(), 2);
}

#[test]
fn two_thousand_items_build_only_what_is_on_screen() {
    let rt = testkit::window("long");
    let keys = item_keys(&rt);
    assert!(keys.len() < 100, "{} rows were built", keys.len());
    assert!(testkit::has_text(&rt, "Reply 2000"));
    assert!(!testkit::has_text(&rt, "Reply 1000"));
}

#[test]
fn the_list_follows_new_text_until_the_user_scrolls_up() {
    let mut rt = testkit::window("long");
    assert_eq!(rt.scroll_at_bottom("mr-conversation"), Some(true));
    assert!(rt.find("mr-jump-latest").is_none());
    testkit::emit(&mut rt, SessionEvent::TextDelta("A new line at the end".into()));
    assert_eq!(rt.scroll_at_bottom("mr-conversation"), Some(true));
    assert!(testkit::has_text(&rt, "A new line at the end"));
    let (x, y) = rt.find("mr-conversation").unwrap();
    rt.wheel(x, y, 600.0);
    testkit::step(&mut rt);
    assert_eq!(rt.scroll_at_bottom("mr-conversation"), Some(false));
    testkit::emit(&mut rt, SessionEvent::TextDelta(" and more".into()));
    assert_eq!(rt.scroll_at_bottom("mr-conversation"), Some(false), "new text must not pull the view down");
    testkit::click(&mut rt, "mr-jump-latest");
    rt.settle_all();
    testkit::step(&mut rt);
    assert_eq!(rt.scroll_at_bottom("mr-conversation"), Some(true));
}
```

If `rt.wheel` scrolls the other way with a positive number, flip the sign; `crates/rune/tests/pinned_scroll.rs` shows the convention.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mindrune conversation`
Expected: FAIL.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mindrune`
Expected: PASS, with no "slow commit" or "duplicate key" warning on stderr from the `long` scene. If Rune warns, fix the cause; do not silence the warning.

- [ ] **Step 5: Look at it**

Run: `for s in conversation streaming long; do cargo run -q -p mindrune -- --demo --scene $s --snap target/snap/t12-$s.png; done`
Expected: the heading, list, table and coloured code block are drawn as such; the user's message shows its asterisks; the cut-off reply ends in a code block, not in raw backticks; `long` shows the last replies.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src && git commit -m "feat(app): the conversation as virtual rows that follow the reply"
```

---

### Task 13: Thinking, tool calls, edits and notices

**Files:**
- Modify: `crates/core/src/tools.rs`, `crates/core/src/conversation.rs`, `crates/core/src/lib.rs`, `crates/app/src/view/items.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Produces, in `mr-core`:

```rust
// tools.rs
/// A unified diff of the two texts with three lines of context, without file headers.
pub fn unified_diff(old: &str, new: &str) -> String;
/// `body` inside a Markdown code fence that `body` cannot close early.
pub fn fenced(language: &str, body: &str) -> String;
// conversation.rs
/// "Thinking…" while it goes on, then "Thought for 4 s" or "Thought for 1 min 5 s".
pub fn thinking_label(seconds: Option<f64>) -> String;
```

- Produces, in the app: `items::row` draws every kind of item except `Request`.

What each item looks like:

- **Thinking.** One muted line with `thinking_label`, and a spinner while it goes on. When the thinking has text, the line toggles (keyed `mr-toggle-{id}`) and shows the text beneath; when it has none, the line does not react.
- **Tool group.** One line: an icon for the family, the group's `summary()`, then a spinner while `running()`, or a warning mark when `failed()`. The line is keyed `mr-toggle-{id}` and toggles the details. `Edit` groups start open; every other family starts closed. `ui.toggled` holds the items the user has flipped from that default.
- **Details, per call:**
  - Read: the path.
  - Search: the query, then the output.
  - Command: the whole command in the monospace font, then the output.
  - Edit, `FileEdit`: the file name, then `+{added} −{removed}` from `line_counts`, then `fenced("diff", &unified_diff(old, new))` drawn as Markdown.
  - Edit, `FileWrite`: the file name, then `+{lines}`, then the content.
  - Other: the input JSON, then the output.
  - Output and content are in the monospace font and cut with `clip(text, OUTPUT_LINES, OUTPUT_CHARS)`. When cut, a control keyed `mr-show-all-{id}` reads "Show all ({N} more lines)", or "Show all" when `hidden_lines` is 0; pressing it adds the item to `ui.whole`, after which the whole text shows. A failed call's output is in the danger colour.
- **Notices.** `Stopped`: a muted line. `TurnFailed` and `Ended`: the text in the danger colour. `CommandFailed`: the text in the warning colour. The last `Ended` notice, while the session's phase is `Ended`, also has a button "Restart" keyed `mr-restart` that runs `ui.app.restart()`.
- **Copying.** A finished reply shows a copy button keyed `mr-copy-{id}` beneath it; it copies the reply's Markdown source. Each code block in a reply has a copy button that copies the block's text: use `Markdown::code_frame` unless the kit's own frame already has one.

- [ ] **Step 1: Write the failing tests in `mr-core`**

```rust
// tools.rs
#[test]
fn a_diff_shows_what_changed_with_context() {
    let diff = unified_diff("a\nb\nc\n", "a\nx\nc\n");
    assert!(diff.contains("-b\n") && diff.contains("+x\n") && diff.contains(" a\n"));
    assert!(!diff.contains("---") && !diff.contains("+++"));
    assert_eq!(unified_diff("same\n", "same\n"), "");
}

#[test]
fn a_fence_is_longer_than_any_run_of_backticks_inside() {
    assert_eq!(fenced("diff", "+x\n"), "```diff\n+x\n```\n");
    assert_eq!(fenced("", "no newline"), "```\nno newline\n```\n");
    let inner = "text\n```rust\nfn f() {}\n```\n````\n";
    let out = fenced("diff", inner);
    assert!(out.starts_with("`````diff\n") && out.ends_with("`````\n"));
}

// conversation.rs
#[test]
fn thinking_is_labelled_in_whole_seconds() {
    assert_eq!(thinking_label(None), "Thinking…");
    assert_eq!(thinking_label(Some(0.2)), "Thought for 1 s");
    assert_eq!(thinking_label(Some(3.5)), "Thought for 4 s");
    assert_eq!(thinking_label(Some(65.0)), "Thought for 1 min 5 s");
}
```

- [ ] **Step 2: Write the failing window tests** in `view/items.rs`. Scenes to add: `"tools"` (in order: a thinking phase of 4 seconds with no text; three reads; a search for `TODO`; a `Bash` command `cargo test` whose output is 500 numbered lines; an edit of `src/main.rs` replacing two lines with three; a failed read of `missing.rs` with the output `No such file`; a finished reply), `"working"` (a thinking phase still going on, then a `Bash` command with no outcome yet), `"ended"` (a reply, then `Exited { clean: false, detail: "claude exited with status 1" }`).

```rust
#[test]
fn tool_calls_are_single_lines_until_opened() {
    let mut rt = testkit::window("tools");
    for part in ["Thought for 4 s", "Read 3 files", "Searched for TODO", "Ran cargo test", "Edited main.rs", "Read missing.rs"] {
        assert!(testkit::has_text(&rt, part), "{part}");
    }
    assert!(!testkit::has_text(&rt, "line 3"), "command output starts closed");
    let command = toggle_key(&rt, "Ran cargo test");
    testkit::click(&mut rt, &command);
    testkit::step(&mut rt);
    assert!(testkit::has_text(&rt, "line 3"));
    assert!(!testkit::has_text(&rt, "line 400"));
    assert!(testkit::has_text(&rt, "Show all (480 more lines)"));
}

#[test]
fn an_edit_shows_its_counts_and_its_diff_without_being_opened() {
    let rt = testkit::window("tools");
    assert!(testkit::has_text(&rt, "+3 −2"));
}

#[test]
fn a_running_tool_and_a_running_thought_say_so() {
    let rt = testkit::window("working");
    assert!(testkit::has_text(&rt, "Thinking…"));
    assert!(testkit::has_text(&rt, "Ran sleep 30"));
}

#[test]
fn a_dead_session_offers_a_restart() {
    let mut rt = testkit::window("ended");
    assert!(testkit::has_text(&rt, "claude exited with status 1"));
    testkit::click(&mut rt, "mr-restart");
    testkit::step(&mut rt);
    assert_eq!(rt.state.app.current_open(), 2);
    assert_eq!(rt.state.app.session.as_ref().unwrap().phase, Phase::Idle);
    assert!(rt.find("mr-restart").is_none());
}

#[test]
fn a_reply_can_be_copied_as_its_source() {
    let mut rt = testkit::window("conversation");
    let key = rt.elements().into_iter().filter_map(|e| e.key).find(|k| k.starts_with("mr-copy-")).unwrap();
    testkit::click(&mut rt, &key);
    assert!(testkit::clipboard(&rt).unwrap().contains("```rust"));
}
```

`toggle_key(rt, summary)` finds the `mr-toggle-` key of the row whose text is `summary`. In `"working"` the command is `sleep 30`.

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p mr-core && cargo test -p mindrune items`
Expected: FAIL.

- [ ] **Step 4: Implement.**

- [ ] **Step 5: Run the tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Look at it**

Run: `for s in tools working ended; do cargo run -q -p mindrune -- --demo --scene $s --snap target/snap/t13-$s.png; done`
Expected: the tool lines read as one tidy list; the edit's diff has green and red lines; the failed read is marked; the ended session shows its message and the Restart button.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src crates/app/src && git commit -m "feat(app): thinking, tool lines with details, diffs, notices and copying"
```

---

### Task 14: Permission requests, questions and plans

A request must be impossible to miss and impossible to answer by accident.

**Files:**
- Create: `crates/app/src/view/requests.rs`
- Modify: `crates/app/src/view/items.rs`, `crates/app/src/glue.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: `mr_agent::{UserRequest, UserRequestKind, Question, Answer, QuestionAnswer, RequestId}`; `mr_core::RequestState`.
- Produces:

```rust
// glue.rs: a new field on Ui
pub answers: HashMap<RequestId, Vec<QuestionDraft>>,
pub struct QuestionDraft { pub chosen: Vec<String>, pub other: TextEdit }

// requests.rs
pub fn card(ui: &Ui, item: &Item, kit: &Kit) -> El;
/// The headline of a permission card.
pub fn permission_title(tool: &str, detail: &ToolDetail) -> String;
/// The answers to send, or `None` while a question has neither a choice nor text.
pub fn question_answers(questions: &[Question], drafts: &[QuestionDraft]) -> Option<Vec<QuestionAnswer>>;
/// Chooses or un-chooses `label`: one choice at most unless the question allows several.
pub fn choose(draft: &mut QuestionDraft, question: &Question, label: &str);
```

A **waiting** card has a border in the warning colour and the small label "Needs you" above its title. Its buttons are ordinary pressable elements that are **not focusable**, are never the window's default button, and have no keyboard shortcut.

| Kind | Title | Body | Buttons, with keys |
| --- | --- | --- | --- |
| Permission | `permission_title` | The detail, drawn as in Task 13's details: the command, the diff, the path, or the input JSON | "Allow once" `mr-allow-{id}`; the request's `always_allow_label` word for word `mr-always-{id}`, present only when the label is; "Deny" `mr-deny-{id}` |
| Question | "Claude has a question" | Per question: its header in muted text when present, the prompt, one button per option (`mr-option-{id}-{q}-{o}`, with the description beneath the label, shown as chosen when it is), and a single-line field with the placeholder "Something else" (`mr-other-{id}-{q}`) | "Send answer" `mr-answer-{id}`, disabled until `question_answers` is `Some` |
| Plan | "Claude's plan" | The plan as Markdown | "Approve" `mr-approve-{id}`; "Keep planning" `mr-keep-{id}` |

`permission_title`: `Command` "Run a command"; `FileEdit` "Edit {file name}"; `FileWrite` "Write {file name}"; `FileRead` "Read {file name}"; `Search` "Search for {query}"; `Other` "Use {tool}".

Answers sent: `Allow { always: false }`, `Allow { always: true }`, `Deny { message: None }`, `Answers(..)`, `ApprovePlan`, `KeepPlanning { feedback: None }`, each through `ui.app.answer`. In `question_answers`, each `QuestionAnswer` has the question's prompt, the chosen labels in the order the options are listed, and `free_text` when the field is not empty after trimming. Enter in a "Something else" field sends the answer when it is complete and does nothing otherwise.

An **answered** card shrinks to one line: the title, then the words from `RequestState::Answered`. A **withdrawn** card shrinks to the title and "No longer needed". Neither has buttons. The drafts for a request are dropped when it stops waiting.

Scenes: `"permission"` (a user message, then a `Bash` request `r1` for `touch a.txt` with the label "Always allow Bash(touch a.txt) in this project"), `"permission-plain"` (the same without a label, id `r2`), `"question"` (request `q1` with two questions: header "Colour", "Which colour?", options Red with description "Warm" and Blue, single choice; and "Which sizes?", options S, M, L, several allowed), `"plan"` (request `p1` with a plan of a heading and three steps).

- [ ] **Step 1: Write the failing tests** in `view/requests.rs`

```rust
const BUTTONS: [&str; 7] = ["mr-allow-", "mr-always-", "mr-deny-", "mr-option-", "mr-answer-", "mr-approve-", "mr-keep-"];
fn a_request_button_has_focus(rt: &Runtime<Ui>) -> bool {
    rt.elements().iter().any(|e| e.focused && e.key.as_deref().is_some_and(|k| BUTTONS.iter().any(|b| k.starts_with(b))))
}
fn answers(rt: &Runtime<Ui>) -> Vec<Answer> {
    testkit::sent(rt).into_iter().filter_map(|c| match c { SessionCommand::Answer { answer, .. } => Some(answer), _ => None }).collect()
}

#[test]
fn a_permission_card_names_the_tool_and_offers_three_answers() {
    let mut rt = testkit::window("permission");
    for part in ["Needs you", "Run a command", "touch a.txt", "Allow once", "Always allow Bash(touch a.txt) in this project", "Deny"] {
        assert!(testkit::has_text(&rt, part), "{part}");
    }
    assert_eq!(rt.state.app.session.as_ref().unwrap().phase, Phase::NeedsUser);
    testkit::click(&mut rt, "mr-always-r1");
    testkit::step(&mut rt);
    assert_eq!(answers(&rt), vec![Answer::Allow { always: true }]);
    assert!(testkit::has_text(&rt, "Always allowed"));
    assert!(rt.find("mr-allow-r1").is_none() && rt.find("mr-deny-r1").is_none());
}

#[test]
fn without_a_suggested_rule_there_is_no_always_button() {
    let mut rt = testkit::window("permission-plain");
    assert!(rt.find("mr-always-r2").is_none());
    testkit::click(&mut rt, "mr-deny-r2");
    testkit::step(&mut rt);
    assert_eq!(answers(&rt), vec![Answer::Deny { message: None }]);
    assert!(testkit::has_text(&rt, "Denied"));
}

#[test]
fn the_keyboard_cannot_answer_a_request() {
    let mut rt = testkit::window("permission");
    assert!(!a_request_button_has_focus(&rt));
    for _ in 0..30 {
        testkit::press(&mut rt, Key::Tab);
        assert!(!a_request_button_has_focus(&rt));
        testkit::press(&mut rt, Key::Enter);
        testkit::press(&mut rt, Key::Text(" ".into()));
    }
    testkit::step(&mut rt);
    assert_eq!(answers(&rt), vec![]);
    assert_eq!(rt.state.app.session.as_ref().unwrap().conversation.waiting(), 1);
}

#[test]
fn a_request_that_dies_with_its_session_stops_offering_buttons() {
    let mut rt = testkit::window("permission");
    testkit::emit(&mut rt, SessionEvent::Exited { clean: false, detail: "killed".into() });
    assert!(rt.find("mr-allow-r1").is_none());
    assert!(testkit::has_text(&rt, "No longer needed"));
    assert!(rt.find("mr-restart").is_some());
    assert_eq!(answers(&rt), vec![]);
}

#[test]
fn a_withdrawn_request_stops_offering_buttons() {
    let mut rt = testkit::window("permission");
    testkit::emit(&mut rt, SessionEvent::RequestWithdrawn { id: RequestId("r1".into()) });
    assert!(rt.find("mr-allow-r1").is_none());
    assert!(testkit::has_text(&rt, "No longer needed"));
}

#[test]
fn a_question_is_sent_only_when_every_part_is_answered() {
    let mut rt = testkit::window("question");
    for part in ["Claude has a question", "Colour", "Which colour?", "Red", "Warm", "Which sizes?"] {
        assert!(testkit::has_text(&rt, part), "{part}");
    }
    testkit::click(&mut rt, "mr-answer-q1");
    testkit::click(&mut rt, "mr-option-q1-0-0");
    testkit::click(&mut rt, "mr-option-q1-0-1");
    testkit::click(&mut rt, "mr-answer-q1");
    testkit::step(&mut rt);
    assert_eq!(answers(&rt), vec![], "the second question has no answer yet");
    testkit::click(&mut rt, "mr-option-q1-1-2");
    testkit::click(&mut rt, "mr-option-q1-1-0");
    testkit::click(&mut rt, "mr-answer-q1");
    testkit::step(&mut rt);
    assert_eq!(answers(&rt), vec![Answer::Answers(vec![
        QuestionAnswer { prompt: "Which colour?".into(), chosen: vec!["Blue".into()], free_text: None },
        QuestionAnswer { prompt: "Which sizes?".into(), chosen: vec!["S".into(), "L".into()], free_text: None },
    ])]);
    assert!(testkit::has_text(&rt, "Blue, S, L"));
}

#[test]
fn free_text_counts_as_an_answer() {
    let q = |multi| Question { header: None, prompt: "p".into(), options: vec![QuestionOption { label: "A".into(), description: None }], multi_select: multi };
    let mut draft = QuestionDraft { chosen: vec![], other: TextEdit::new("") };
    assert_eq!(question_answers(&[q(false)], std::slice::from_ref(&draft)), None);
    draft.other = TextEdit::new("  my own  ");
    assert_eq!(question_answers(&[q(false)], std::slice::from_ref(&draft)),
        Some(vec![QuestionAnswer { prompt: "p".into(), chosen: vec![], free_text: Some("my own".into()) }]));
    choose(&mut draft, &q(false), "A");
    choose(&mut draft, &q(false), "A");
    assert!(draft.chosen.is_empty(), "choosing again un-chooses");
}

#[test]
fn a_plan_is_approved_or_sent_back() {
    let mut rt = testkit::window("plan");
    assert!(testkit::has_text(&rt, "Claude's plan"));
    testkit::click(&mut rt, "mr-keep-p1");
    testkit::step(&mut rt);
    assert_eq!(answers(&rt), vec![Answer::KeepPlanning { feedback: None }]);
    assert!(testkit::has_text(&rt, "Kept planning"));
    let mut rt = testkit::window("plan");
    testkit::click(&mut rt, "mr-approve-p1");
    testkit::step(&mut rt);
    assert_eq!(answers(&rt), vec![Answer::ApprovePlan]);
}

#[test]
fn the_titles_read_as_written() {
    let t = |d| permission_title("Tool", &d);
    assert_eq!(t(ToolDetail::FileEdit { path: "/p/a.rs".into(), old_text: String::new(), new_text: String::new() }), "Edit a.rs");
    assert_eq!(t(ToolDetail::FileWrite { path: "b.md".into(), content: String::new() }), "Write b.md");
    assert_eq!(t(ToolDetail::FileRead { path: "c".into() }), "Read c");
    assert_eq!(t(ToolDetail::Search { query: "x".into() }), "Search for x");
    assert_eq!(t(ToolDetail::Other { input_json: "{}".into() }), "Use Tool");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mindrune requests`
Expected: FAIL.

- [ ] **Step 3: Implement.** `items::row` hands `Request` items to `requests::card`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mindrune`
Expected: PASS.

- [ ] **Step 5: Look at it**

Run: `for s in permission question plan; do cargo run -q -p mindrune -- --demo --scene $s --snap target/snap/t14-$s.png; done`
Expected: each card stands out with its amber border; the always-allow label fits or wraps without being cut; the sidebar row carries the attention mark.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src && git commit -m "feat(app): permission, question and plan cards that only a click can answer"
```

---

### Task 15: The input box

**Files:**
- Create: `crates/app/src/view/input.rs`
- Modify: `crates/app/src/view/mod.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: `mr_core::{Phase, mode_label}`; `Ui::draft`, `Ui::open_picker`.
- Produces:
  - `input::input(ui: &Ui, kit: &Kit, layer: &mut Vec<El>) -> El`, placed at the bottom of `mr-tile` under the conversation;
  - `input::mode_choices(current: &PermissionMode, plan_mode: bool) -> Vec<PermissionMode>`;
  - `view::on_key` handles Escape when no text field has focus.

Layout: a rounded box. Inside, top to bottom: the queued message when there is one; the text area; a row with the model picker and the mode picker on the left and the send or stop button on the right.

Rules:

- The text area is keyed `mr-input`, has the placeholder "Message Claude", grows from one to ten lines, and has focus when the main screen first shows. Enter submits; Shift+Enter adds a line.
- **Submit** takes the draft's text, calls `ui.app.submit`, and clears the draft unless the text was empty after trimming.
- In `Idle` and `Opening` the button is send, keyed `mr-send`, disabled while the draft is empty. In `Working` and `NeedsUser` it is stop, keyed `mr-stop`, and runs `ui.app.stop()`. In `Ended` the text area and the button are disabled and the placeholder reads "This session has ended".
- **Escape** stops the turn when one is running, whether the text area has focus (`TextArea::on_key`, returning true) or not (`view::on_key`). When no turn is running Escape is not handled, so the field's own behaviour stays.
- **Queued message.** A strip keyed `mr-queued` reads "Queued: " followed by the message's first line, with a button "Edit" keyed `mr-queued-edit`. Edit takes the message back (`take_queued`) and puts it at the start of the draft, followed by a line break when the draft was not empty.
- **Model picker**, keyed `mr-model` (its trigger is `mr-model-trigger`): shows `model_label()`; lists the `name` of every entry in `session.models`; choosing a name runs `ui.app.choose_model` with that entry's `id`. Disabled in `Opening` and `Ended` and when the list is empty.
- **Mode picker**, keyed `mr-mode`: shows `mode_label(&session.mode)`; lists the labels of `mode_choices`; choosing runs `ui.app.choose_mode`. `mode_choices` is `Ask`, `AcceptEdits`, then `Plan` when `plan_mode` is true, then `current` when it is none of those. Bypass is therefore shown only when the session is already in it; this app has no way to learn whether the user's settings allow it, so it never offers it.
- Opening one picker closes the other. Both use `ui.open_picker`.

- [ ] **Step 1: Write the failing tests** in `view/input.rs`. `sends(rt)` returns the texts of the `Send` commands in `testkit::sent(rt)`. `reply(rt, events)` scripts the fake's next reply (`FakeAgent::reply`, with every step at 0 seconds).

```rust
#[test]
fn enter_sends_and_shift_enter_adds_a_line() {
    let mut rt = testkit::window("empty");
    testkit::type_text(&mut rt, "one");
    testkit::press_with(&mut rt, Key::Enter, Mods { shift: true, ..Mods::default() });
    testkit::type_text(&mut rt, "two");
    assert_eq!(sends(&rt), Vec::<String>::new());
    testkit::press(&mut rt, Key::Enter);
    testkit::step(&mut rt);
    assert_eq!(sends(&rt), vec!["one\ntwo".to_string()]);
    assert_eq!(rt.state.draft.text, "");
    assert!(testkit::has_text(&rt, "one\ntwo") || testkit::has_text(&rt, "two"));
    testkit::press(&mut rt, Key::Enter);
    testkit::type_text(&mut rt, "   ");
    testkit::press(&mut rt, Key::Enter);
    assert_eq!(sends(&rt).len(), 1, "an empty message is not sent");
}

#[test]
fn while_a_turn_runs_the_button_stops_it_and_typing_queues() {
    let mut rt = testkit::window("working");
    assert!(rt.find("mr-stop").is_some() && rt.find("mr-send").is_none());
    testkit::type_text(&mut rt, "then run the tests\nand report");
    testkit::press(&mut rt, Key::Enter);
    testkit::step(&mut rt);
    assert_eq!(sends(&rt).len(), 1, "only the scene's own message was sent");
    assert!(testkit::has_text(&rt, "Queued: then run the tests"));
    testkit::click(&mut rt, "mr-queued-edit");
    testkit::step(&mut rt);
    assert_eq!(rt.state.draft.text, "then run the tests\nand report");
    assert!(rt.find("mr-queued").is_none());
    testkit::click(&mut rt, "mr-stop");
    testkit::step(&mut rt);
    assert!(testkit::sent(&rt).contains(&SessionCommand::Stop));
}

#[test]
fn a_queued_message_goes_out_when_the_turn_ends() {
    let mut rt = testkit::window("working");
    testkit::type_text(&mut rt, "next");
    testkit::press(&mut rt, Key::Enter);
    testkit::emit(&mut rt, SessionEvent::TurnFinished(TurnSummary { cost_usd: 0.0, context_used: None, duration_ms: 1, interrupted: false }));
    assert_eq!(sends(&rt).last().map(String::as_str), Some("next"));
    assert!(rt.find("mr-queued").is_none());
}

#[test]
fn typing_enter_beside_a_waiting_request_queues_and_answers_nothing() {
    let mut rt = testkit::window("permission");
    testkit::type_text(&mut rt, "yes");
    testkit::press(&mut rt, Key::Enter);
    testkit::step(&mut rt);
    assert!(!testkit::sent(&rt).iter().any(|c| matches!(c, SessionCommand::Answer { .. })));
    assert!(testkit::has_text(&rt, "Queued: yes"));
    assert!(rt.find("mr-allow-r1").is_some());
}

#[test]
fn escape_stops_a_turn_from_anywhere_and_does_nothing_otherwise() {
    let mut rt = testkit::window("working");
    testkit::press(&mut rt, Key::Escape);
    testkit::step(&mut rt);
    assert_eq!(testkit::sent(&rt).iter().filter(|c| **c == SessionCommand::Stop).count(), 1);
    let mut rt = testkit::window("working");
    rt.state.draft.focused = false;
    rt.invalidate();
    testkit::step(&mut rt);
    testkit::press(&mut rt, Key::Escape);
    testkit::step(&mut rt);
    assert_eq!(testkit::sent(&rt).iter().filter(|c| **c == SessionCommand::Stop).count(), 1);
    let mut rt = testkit::window("empty");
    testkit::press(&mut rt, Key::Escape);
    testkit::step(&mut rt);
    assert!(!testkit::sent(&rt).contains(&SessionCommand::Stop));
}

#[test]
fn the_model_picker_shows_the_model_and_changes_it() {
    let mut rt = testkit::window("empty");
    assert_eq!(rt.state.app.session.as_ref().unwrap().model_label(), "Default");
    assert!(testkit::has_text(&rt, "Default"));
    testkit::click(&mut rt, "mr-model-trigger");
    testkit::step(&mut rt);
    testkit::click_text(&mut rt, "Fast");
    testkit::step(&mut rt);
    assert!(testkit::sent(&rt).contains(&SessionCommand::SetModel("fast".into())));
    assert_eq!(rt.state.app.session.as_ref().unwrap().model_label(), "Default", "not until the agent confirms");
    testkit::emit(&mut rt, SessionEvent::ModelChanged("fake-model-fast".into()));
    assert_eq!(rt.state.app.session.as_ref().unwrap().model_label(), "Fast");
    assert!(testkit::has_text(&rt, "Fast"));
    assert_eq!(rt.state.open_picker, None);
}

#[test]
fn the_mode_picker_offers_the_three_modes() {
    let mut rt = testkit::window("empty");
    assert!(testkit::has_text(&rt, "Ask before edits"));
    testkit::click(&mut rt, "mr-mode-trigger");
    testkit::step(&mut rt);
    assert!(testkit::has_text(&rt, "Accept edits") && testkit::has_text(&rt, "Plan"));
    assert!(!testkit::has_text(&rt, "Bypass permissions"));
    testkit::click_text(&mut rt, "Plan");
    testkit::step(&mut rt);
    assert!(testkit::sent(&rt).contains(&SessionCommand::SetPermissionMode(PermissionMode::Plan)));
}

#[test]
fn the_mode_choices_keep_an_unusual_current_mode_visible() {
    use PermissionMode::*;
    assert_eq!(mode_choices(&Ask, true), vec![Ask, AcceptEdits, Plan]);
    assert_eq!(mode_choices(&Ask, false), vec![Ask, AcceptEdits]);
    assert_eq!(mode_choices(&Bypass, true), vec![Ask, AcceptEdits, Plan, Bypass]);
    assert_eq!(mode_choices(&Other("auto".into()), true), vec![Ask, AcceptEdits, Plan, Other("auto".into())]);
}

#[test]
fn an_ended_session_cannot_be_typed_into() {
    let mut rt = testkit::window("ended");
    testkit::type_text(&mut rt, "hello");
    testkit::press(&mut rt, Key::Enter);
    testkit::step(&mut rt);
    assert_eq!(rt.state.draft.text, "");
    assert!(testkit::has_text(&rt, "This session has ended"));
}
```

Add `click_text(rt, text)` to the test kit: clicks the centre of the visible element whose text is exactly `text` and is not the picker's trigger. The demo's models are the two every `FakeAgent` session offers: `Default` (id `default`, resolving to `fake-model`) and `Fast` (id `fast`, resolving to `fake-model-fast`); a demo session starts on `Default`.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mindrune input`
Expected: FAIL.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mindrune`
Expected: PASS.

- [ ] **Step 5: Look at it**

Run: `for s in empty working ended; do cargo run -q -p mindrune -- --demo --scene $s --snap target/snap/t15-$s.png; done`
Expected: the model's name is readable beside the input in every image; the stop button replaces send in `working`; the box is visibly disabled in `ended`.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src && git commit -m "feat(app): input box with send, stop, queueing and the model and mode pickers"
```

---

### Task 16: The conversation is as wide as the user wants

This is the reason the project exists. The conversation and the input box share one column whose width the user sets by dragging.

**Files:**
- Create: `crates/app/src/view/width.rs`
- Modify: `crates/app/src/view/mod.rs`, `crates/app/src/view/conversation.rs`, `crates/app/src/view/input.rs`

**Interfaces:**
- Consumes: `mr_core::{Width, MIN_WIDTH}`; `App::set_width`; `El::on_drag`; `cx.layout_size("mr-tile")`.
- Produces:

```rust
/// The column's width inside a tile of `tile` points.
pub fn column_width(width: Width, tile: f32) -> f32;
/// The width a drag asks for, from the pointer's distance to the tile's centre line.
pub fn width_from_drag(pointer_x: f32, tile_left: f32, tile: f32) -> Width;
/// Wraps the conversation and the input box in the centred column with its two handles.
pub fn column(ui: &Ui, cx: &mut Cx<Ui>, kit: &Kit, content: El) -> El;
pub const EDGE_SNAP: f32 = 24.0;
```

Rules:

- `column_width`: `Unlimited` is `tile`; `Points(p)` is `p` or `tile`, whichever is smaller.
- `width_from_drag`: the asked width is twice the distance from `pointer_x` to the tile's centre. When the pointer is within `EDGE_SNAP` of either edge of the tile, or outside it, the result is `Unlimited`. Otherwise it is `Points`, never less than `MIN_WIDTH`.
- The column is keyed `mr-column` and centred in `mr-tile`. Rows inside the conversation are as wide as the column; the scroll itself spans the tile, so the wheel works over the empty margins too.
- A handle sits on each edge of the column, keyed `mr-width-left` and `mr-width-right`: 8 points wide, the full height of the tile, invisible until hovered, with a horizontal-resize cursor. When the column fills the tile the handles sit at the tile's edges, so a width of `Unlimited` can always be dragged back in.
- Dragging either handle calls `ui.app.set_width(width_from_drag(..))` on every move and carries the effects out, so the width is saved as it changes. Both handles move together.

- [ ] **Step 1: Write the failing tests** in `view/width.rs`

```rust
#[test]
fn the_column_is_the_chosen_width_or_the_tile() {
    assert_eq!(column_width(Width::Points(1000.0), 1600.0), 1000.0);
    assert_eq!(column_width(Width::Points(1000.0), 700.0), 700.0);
    assert_eq!(column_width(Width::Unlimited, 1600.0), 1600.0);
}

#[test]
fn a_drag_sets_the_width_symmetrically() {
    // A tile from x = 300 to x = 1900: centre 1100.
    assert_eq!(width_from_drag(1500.0, 300.0, 1600.0), Width::Points(800.0));
    assert_eq!(width_from_drag(700.0, 300.0, 1600.0), Width::Points(800.0));
    assert_eq!(width_from_drag(1110.0, 300.0, 1600.0), Width::Points(MIN_WIDTH));
    assert_eq!(width_from_drag(1880.0, 300.0, 1600.0), Width::Unlimited);
    assert_eq!(width_from_drag(310.0, 300.0, 1600.0), Width::Unlimited);
    assert_eq!(width_from_drag(2500.0, 300.0, 1600.0), Width::Unlimited);
    assert_eq!(width_from_drag(1875.0, 300.0, 1600.0), Width::Points(1550.0));
}

fn drag(rt: &mut Runtime<Ui>, key: &str, to_x: f32) {
    let (x, y) = rt.find(key).unwrap();
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_move((x + to_x) / 2.0, y);
    rt.pointer_move(to_x, y);
    rt.pointer_up(to_x, y);
    testkit::step(rt);
}
fn column(rt: &Runtime<Ui>) -> [f32; 4] { rt.presented("mr-column").unwrap().0 }

#[test]
fn dragging_a_handle_changes_the_width_and_remembers_it() {
    let mut rt = testkit::window_sized("conversation", (2400.0, 900.0));
    let [_, _, w, _] = column(&rt);
    assert_eq!(w, 1000.0);
    let [tile_x, _, tile_w, _] = rt.presented("mr-tile").unwrap().0;
    let centre = tile_x + tile_w / 2.0;
    drag(&mut rt, "mr-width-right", centre + 700.0);
    assert_eq!(rt.state.app.width, Width::Points(1400.0));
    assert_eq!(column(&rt)[2], 1400.0);
    assert_eq!(rt.state.prefs.get::<Width>("conversation_width"), Some(Width::Points(1400.0)));
    drag(&mut rt, "mr-width-left", tile_x + 2.0);
    assert_eq!(rt.state.app.width, Width::Unlimited);
    assert_eq!(column(&rt)[2], tile_w);
    drag(&mut rt, "mr-width-left", centre - 300.0);
    assert_eq!(rt.state.app.width, Width::Points(600.0));
}

#[test]
fn the_input_box_is_as_wide_as_the_conversation() {
    let rt = testkit::window_sized("conversation", (2400.0, 900.0));
    let input = rt.presented("mr-input").unwrap().0;
    let [x, _, w, _] = column(&rt);
    assert!(input[0] >= x && input[0] + input[2] <= x + w + 0.5);
}

#[test]
fn a_narrow_window_uses_all_of_its_tile() {
    let rt = testkit::window_sized("conversation", (900.0, 700.0));
    let [_, _, tile_w, _] = rt.presented("mr-tile").unwrap().0;
    assert_eq!(column(&rt)[2], tile_w);
}
```

Add `window_sized(scene, size)` to the test kit; `window` calls it with 1200 by 800.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p mindrune width`
Expected: FAIL.

- [ ] **Step 3: Implement.** `crates/rune-kit/src/shell/split.rs` shows how the kit's own divider uses `on_drag`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mindrune`
Expected: PASS.

- [ ] **Step 5: Look at it on an ultrawide**

Run: `cargo run -q -p mindrune -- --demo --scene tools --snap target/snap/t16-wide.png --size 3440x1440 --scale 1`
Expected: a 1000-point column centred in a very wide tile, with the sidebar on the left. Then write `{"events":[{"t":0.1,"type":"drag","key":"mr-width-right","x":3300,"y":700}]}` to `target/snap/drag.json` and run the same command with `--script target/snap/drag.json --at 1 --snap target/snap/t16-dragged.png`; if the script's `drag` event takes other field names, `crates/rune/src/tooling/script.rs` has them. Expected: the conversation now fills almost the whole tile.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src && git commit -m "feat(app): a conversation width the user drags, remembered between runs"
```

---

### Task 17: Screenshots, a live turn, and Jerod's first look

**Files:**
- Create: `crates/app/tests/snapshots.rs`, `crates/app/tests/live.rs`
- Modify: `docs/notes-for-later-plans.md`, `docs/protocol-notes.md` (only if the live run shows something new)

**Interfaces:**
- Consumes: the finished binary and its scenes; `mr_core::{App, Backend}`; `mr_claude::ClaudeAgent`.

- [ ] **Step 1: Write the screenshot test** in `tests/snapshots.rs`

For each scene in `checking`, `signed-out`, `not-installed`, `empty`, `limit`, `conversation`, `streaming`, `tools`, `working`, `ended`, `permission`, `question`, `plan`, and for both `--theme dark` and `--theme light`: run `env!("CARGO_BIN_EXE_mindrune")` with `--demo --scene NAME --theme T --snap DIR/NAME-T.png --tree DIR/NAME-T.json` where `DIR` is `target/snap/final`. Assert for each run: the exit status is 0; the image exists and is larger than 2,000 bytes; the tree parses as JSON; its `warnings` array is empty; it contains an element keyed `mr-root`. One test function per theme, so a failure names the theme; the assertion message names the scene.

- [ ] **Step 2: Run it**

Run: `cargo test -p mindrune --test snapshots`
Expected: PASS, 26 images in `target/snap/final`. Open every dark image and at least `conversation`, `permission` and `limit` in light. Fix anything cut off, overlapping, unreadable or misaligned, then run again. List what was fixed in the report.

- [ ] **Step 3: Write the live test** in `tests/live.rs`: `#[ignore]`, and it returns at once unless `MINDRUNE_LIVE=1`

It builds `ClaudeAgent::from_environment()`, a `Backend` with a waker that does nothing, and an `App` for a fresh temporary folder with `StartSpec.model` left to the agent's default. It then loops: take messages from the backend into the `App` exactly as `glue::poll` does (move that mapping into a function both use), send the effects, sleep 20 ms, for at most 120 seconds. It sends "Reply with the single word: ok" once the session is `Idle`, and ends when the session is `Idle` again with one finished `Reply` item. It asserts: the reply's text contains "ok" in any letter case; `app.usage.session` is `Some`; the session's `model_label()` is not "Model". At the end it sends `Close` and waits for the phase `Ended { clean: true, .. }`.

- [ ] **Step 4: Human checkpoint. Stop and ask Jerod to run these in their own terminal, not in an agent's shell.**

```bash
MINDRUNE_LIVE=1 cargo test -p mindrune --test live -- --ignored --nocapture
```

```bash
cargo run --release -p mindrune -- .
```

Ask Jerod to try, in the window, and say what they saw for each:

1. The window opens and the footer shows their name, plan and both usage lines.
2. A message gets a reply that streams in as formatted text.
3. "Use the Bash tool to run: touch probe-marker.txt" shows a permission card; "Allow once" lets it run.
4. A long request ("write a 600 word essay") can be stopped with Escape and with the stop button.
5. The model picker changes the model; the next reply still works.
6. Dragging the edge of the conversation makes it wider, and the width is kept after closing and reopening.
7. On the ultrawide: does the text feel right at full width, and is anything too small or too faint?

Record the answers in `docs/notes-for-later-plans.md`. A defect in something this plan built is fixed in this task. Anything else is recorded for plan 3 or 4.

- [ ] **Step 5: Bring the notes up to date** in `docs/notes-for-later-plans.md`

Remove what this plan did from "Fix first in plan 2" and from the seam table: the `Started` wording, the fake's strictness, the replay test, thinking, the usage query, question details, the typed `CommandFailed`. Keep what it did not do, under a heading "Left by plan 2": the answering model per message, declining a question, tool input while it streams, the `translate.rs` split, one thread per open in `Backend`, feedback text with "Keep planning", offering Bypass, a sign-in that expires while the window is open (the turn fails with Claude Code's message, and the sign-in screen appears only at the next launch), and anything the implementers reported. Add what plan 3 must know about the window: the keys in use, that `Ui` holds one session where plan 3 needs many, and that `mr-session-row` is the place the list grows from.

- [ ] **Step 6: Final checks**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets 2>&1 | grep -c warning; cargo test --workspace 2>&1 | grep -E '^test result' | grep -v ' 0 failed' ; grep -rn -E 'ClaudeAgent|mr_claude' crates/core crates/app/src --include=*.rs | grep -v 'crates/app/src/main.rs'`
Expected: `0` warnings; no test-result line with failures; the last command prints nothing.

- [ ] **Step 7: Commit**

```bash
git add crates/app/tests docs && git commit -m "test(app): screenshots of every screen and an opt-in live turn; notes for plan 3"
```

---

## Done when

- `mindrune .` gives Jerod a window in which a real Claude Code session works from the first message to a stopped turn, with permission requests answered by clicks only.
- Both usage lines, the account and the model are visible without opening anything.
- The conversation width follows a drag and is remembered.
- Every screen has a screenshot produced from scripted events, in both themes, with no duplicate-key warning.
- `cargo test --workspace` passes, and the live test passes on Jerod's machine.
- `mr-core` and every view are free of `mr-claude`.
