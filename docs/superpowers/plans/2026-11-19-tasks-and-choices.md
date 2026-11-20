# A Task Checklist and a Choice Box: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A session's tile shows what Claude is working through as a checklist that ticks off in place, just above the message box. And when Claude's turn ends with a decision for the user, the tile shows one short question with up to three buttons there; a click sends that button's message at once, as if it had been typed.

**Architecture:** Both come from tool calls. The checklist is Claude Code's own task list, which the newer models keep only when the session asks for it: the app starts the program with one environment variable set, and the adapter turns the `TaskCreate` and `TaskUpdate` calls into events. The choice box is the first tool the app gives Claude itself: one tool on an in-process MCP server that the program reaches through control requests on the session's own channel, which the adapter answers on the spot. The adapter keeps all of Claude Code's tool names; the agent's vocabulary gains four events and two small types; the core keeps a list and an offer per session; the stored history rebuilds both after a restart.

**Tech Stack:** Rust 1.99.1 (edition 2024), `rune` and `rune-kit` at the pinned revision, `serde`, `serde_json`, `tempfile` for tests. No new package.

**Spec:** `docs/superpowers/specs/2026-11-10-claude-code-ui-design.md` for the tile and the conversation. The rest is Jerod's request of 2026-11-19, quoted below. The wire facts are in `docs/protocol-notes.md` under "The task list and a tool of the app's own", written from two recordings Jerod made that day; read that section first, then this plan.

**For the controller:** Jerod's rules: every subagent (implementer, reviewer, fixer) runs on Opus 5.5 at high effort or better; every commit ends with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>` and has a short message. A research report made for this plan is on disk and not in git: `.superpowers/sdd/plan-9-research/code-as-built.md` (the code at `5f1413d`, with file and line for everything this plan touches). Give it to each implementer. Where the report and the code differ, the code wins. The branch is `plan-9-tasks-and-choices`; it already holds the probe's `--choices` (`5f1413d`) and the fixtures and notes (`c42952f`).

## What Jerod asked for

In their words, 2026-11-19, with three screenshots of another app's task list, its buttons under a question, and a copy button beside a command: "I want the UI in the chatbox that is formatted. Lists for tasks with checkboxes, the buttons for me to click when deciding things etc."

Asked for a moment when it was missed: "I was running a task to apply for jobs. I like when it shows a list of what it's doing rather than spitting out a bunch of commands that I don't know what they're for. I also like to be able to click a button rather than have to type \"/apply\". I also like being able to just hit that \"copy\" button next to commands you give me."

A click on a button: "Send right away". The list above the message box, staying while the conversation scrolls: "No thats right". Offered one predicted next message as a single button, or a question with up to three buttons that Claude chooses: "Full choice box".

## Not in this plan, on purpose

A Run button on a command (the app has no terminal; a copy button is on every code block already). Claude Code's predicted next message (`--prompt-suggestions`): the choice box replaces it. A setting that turns the checklist or the choice tool off. The older `TodoWrite` tool. The steps of a sub-agent's own list. Folding the commands of a step under that step. Hiding the stored `AskUserQuestion` and `ExitPlanMode` calls that show as tool lines after a restart (older than this plan). Buttons that take the keyboard's focus.

## What rests on what was never seen

1. **`alwaysLoad`.** The recordings show both the task tools and the choice tool as deferred: Claude loads each with a `ToolSearch` call first. The documentation says a server's entry with `"alwaysLoad": true`, or a tool listed with `"_meta":{"anthropic/alwaysLoad":true}`, is loaded with the first prompt. This plan sets both for the choice tool and has not seen either work. It also hides a `ToolSearch` call that only loads these tools, so nothing shows if they do not. The live test reports which happened.
2. **The stored result's name.** Live, a `TaskCreate`'s id comes in the result record's `tool_use_result`. The plan guesses that a stored transcript keeps it as `toolUseResult`, reads both spellings, and falls back to the result's text (`Task #1 created successfully: …`), which was seen. The live test reports how many ids it found each way.
3. **`TaskUpdate` beyond a status.** Only `{"taskId","status"}` with `in_progress` and `completed` was seen. `deleted`, a changed `subject` or `activeForm`, and the misspelt keys the documentation warns of are handled from the documentation alone.
4. **How Claude uses the choice tool in real work.** One recording shows it called once, with a sensible question, and the question not repeated in the reply. How often it is called in a long working session, and whether it offers a project's own slash commands, is for Jerod's look.
5. **The list after the program compacts a long conversation**, and in a session resumed after days: not seen.

## Decisions in this plan that Jerod has not seen

1. **The app turns the task list on for every session it starts**, by setting `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` for the program, over whatever the user's shell says. Claude then writes a list for most work of three steps or more, and none for short requests.
2. **The list shows in the tile, above the message box and as wide as it**, under a line "Tasks" with a count such as "2 of 5". A finished step has a green tick and fainter words; the current one a spinner and its "doing" wording (`Creating a.txt`); the rest an empty circle.
3. **While no turn runs, the current step shows an empty circle**, not a spinner: nothing is happening.
4. **In a tile shorter than 480 points the list starts folded to one line**, `3 of 5: Creating c.txt`, and a click on that line opens it; a click on "Tasks" folds an open list. The choice lasts while that list lasts.
5. **An open list shows at most six steps**, the ones around the current step, with a last line such as `4 more` for the rest.
6. **The list goes away when a turn ends with every step ticked.** A list with a step still open stays, ticked steps included, also after the turn was stopped. The next turn's steps then join it.
7. **After a restart the list comes back if it had an open step**, rebuilt from Claude Code's stored conversation. In a very long session the oldest steps may be missing, because only the conversation's last part is read.
8. **The list's own tool calls are not shown as tool lines**, nor is the `ToolSearch` call that only loads them. Every other tool line stays as it is.
9. **The choice box**: one question and one to three buttons in a bordered box above the message box, under the list. The first button is the recommended one and is drawn stronger. Pointing at a button shows the message it sends.
10. **The box shows only once the turn has ended**, though Claude calls the tool a moment before: a message sent mid-turn would change that turn.
11. **A click sends through the same path as a typed message**: files attached in the box go with it, the text typed in the box stays, and while an attachment is still being read the buttons are greyed.
12. **The box goes away** on a click, when any other message is sent or queued, when a new turn starts, and when the turn was stopped or failed. It is not there while a permission request or the "open elsewhere" question waits.
13. **After a restart the box comes back** if nothing was said after it in the stored conversation.
14. **A box that waits does not turn the tile orange** and sends no "needs your input" notification. The turn has finished, so the session is green as after any turn.
15. **The tool is allowed without a permission question**, in every permission mode: it only shows buttons.
16. **The words that tell Claude when to offer choices** are added to every session's instructions (text in Task 2). They are the one thing here that changes how Claude behaves in MindRune.
17. **Limits on what Claude may offer**: a question of at most 300 characters, one to three choices, a label of at most 40, a message of at most 2,000. Anything else is refused with a sentence Claude can read and act on, and nothing is shown.
18. **The same session opened in a terminal or over Remote Control has no choice box**: the tool exists only where MindRune serves it.

## Global Constraints

- Rust toolchain is exactly `1.99.1`, edition `2024`. In an agent's shell cargo is not on `PATH`: start every command with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Rune is pinned to `rev = "2a44156b21d63b0342a366b26d9222a6edd4622c"`. Its source is at `~/.cargo/git/checkouts/rune-c9b805e9ed8a3730/2a44156`; read it instead of guessing an API. Where this plan and the source differ, the source wins: say so in your report.
- **No package is added to `Cargo.lock`, and `mr-core` and `mr-agent` gain no dependency.** No JSON is parsed in `mr-core` or in a view.
- **Claude Code's tool names live in `mr-claude` only**: `TaskCreate`, `TaskUpdate`, `TaskGet`, `TaskList`, `ToolSearch`, `mcp__mindrune__offer_choices`, `offer_choices`. They appear in no other crate's code outside its tests, the probe, and the live test.
- `mr-agent` and `mr-core` depend on neither `mr-claude` nor `rune`. No type from `mr-claude` appears in `mr-core` or in any view. Only `crates/app/src/main.rs`, `crates/app/tests/live.rs` and `crates/probe` may name `ClaudeAgent`.
- **The reader thread answers a server request at once and never waits**: an `mcp_message` request gets its reply from the thread that read it, with no lock held but the writer's own. Every other control request that is not `can_use_tool` is still refused as before.
- **Nothing from Claude is trusted for its shape.** A tool's input may lack any key, hold any type, or be very long. No input panics, none is shown half-made, and every text kept from one is cut to this plan's limits.
- The app still never writes inside Claude Code's folder, with the one exception plan 8 made (`remove_session`). No agent and no test reads, lists or touches `~/.claude`.
- The environment the app builds for the program still never carries a parent Claude session's variables. The one variable this plan sets is the app's own, set after that filter.
- The UI thread never blocks and reads no file.
- `SAVED_VERSION` stays 1 and `Saved` gains no field: a list and an offer are rebuilt from the stored conversation.
- A deleted session shows no list and no box, and a click reaches nothing in it.
- Every session intent of `App` takes the `SessionKey` it acts on. Every `App` intent a view calls is followed by `glue::run`.
- Never assign `App.sessions`, `App.groups` or `App.tiles` wholesale after `App::new`.
- Every key this app gives an element starts with `mr-`. No button of the list or the box takes the keyboard's focus (`requests::unfocusable` is the model).
- No Anthropic logos, colours or fonts.
- User-visible wording is exact where this plan quotes it. Never use a real person's name or e-mail address as test data: `alex`, `/home/alex`. Never copy text out of a real stored session or out of `target/rec/` into a test or a fixture; no agent opens `target/rec/`. The two new fixtures are already scrubbed and in git.
- All window tests and all screenshots use `FakeAgent`, through the demo window. No agent starts the real `claude`, sets `MINDRUNE_LIVE`, runs a test with `--ignored`, runs `scripts/install-desktop-entry.sh`, or starts the app outside `--demo`.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` (no warnings) and `cargo test --workspace` pass before every commit. Say in the report which existing tests you changed and why.
- **Git.** Work on the branch `plan-9-tasks-and-choices`. Never push. Never commit `target/` or `.superpowers/`. Commit by explicit path. Another session of Jerod's may work in the same checkout: never touch, stash, reset or commit a file that is not yours. Keep commit messages short. End every commit message with a blank line and `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`, in exactly that form.
- Version of `claude` this plan was written against: `2.1.323`.

## Review Focus

The request implies these conditions and no screen shows them. Each has a test in the task named.

1. **Input that is not what the documentation says.** A `TaskCreate` without a subject; a `taskId` that is a number, or spelt `id` or `task_id`; a status the app does not know; choices that are not a list, four of them, an empty label, a message of ten thousand characters; a `ToolSearch` that loads a hidden tool together with an ordinary one. Expected: no panic, no half-made step or box, a refusal Claude can read for the choice tool, and the mixed `ToolSearch` still shown. Tests: Tasks 1, 2 and 3.
2. **Calls and results out of step.** A result whose call was never seen; a call whose result never comes before the turn ends; an update for a step the app does not have; a `TaskCreate` that failed; two offers in one turn; an offer, then the turn is stopped. Expected: nothing is added for a failed or unanswered call, an unknown step is ignored, the last offer wins, and a stopped turn leaves no box. Tests: Tasks 3, 5 and 6.
3. **A restart.** A stored conversation with a finished list, one with an open step, one cut off before its `TaskCreate` calls, one whose last offer was followed by a message, one read again after it was let go while a turn of this run already changed the list. Expected: only an open list comes back, updates for unknown steps cost nothing, an answered offer does not come back, and what this run saw is never overwritten by what was stored. Tests: Tasks 4, 5 and 6.
4. **A click at a bad moment.** Twice in a row; while an attachment is being read; on a session at rest whose program must first be started; on a session that is deleted or has a question waiting; while a message is queued. Expected: one message at most, greyed buttons where a typed message would not go either, a resting session woken as by a typed message, and nothing for a deleted one. Tests: Tasks 6 and 8.
5. **A short tile.** At 280 points with a list, a box, a queued message and chips at once; a step whose words run to 300 characters; a list of 100 steps. Expected: the text area keeps at least one line, nothing overlaps, the list starts folded, long words are cut with an ellipsis on one line, and six steps at most are drawn. Tests: Tasks 7 and 8.

## File Structure

```
crates/agent/src/tasks.rs            Task 1: TaskStatus, TaskStep, TaskChange, TaskList
crates/agent/src/choices.rs          Task 1: Choice, Choices and their limits
crates/agent/src/event.rs            Task 1: four events
crates/agent/src/agent.rs            Task 1: StoredHistory gains tasks and choices
crates/claude/src/choices.rs         Task 2: the server, its tool, the program's arguments (moved from the probe)
crates/claude/src/env.rs             Task 2: the task tools' variable
crates/claude/src/agent.rs           Task 2: the reader answers the server's requests; open adds both
crates/claude/tests/stub/fake_claude.sh, tests/agent.rs   Task 2: a turn that calls the server
crates/probe/src/choices.rs, repl.rs Task 2: reuse the adapter's server
crates/claude/src/wire.rs            Task 3: a result's structured part
crates/claude/src/tasks.rs           Task 3: which calls are hidden, and what they say
crates/claude/src/translate.rs       Task 3: events for them, no tool lines
crates/claude/src/history.rs, history_tail.rs   Task 4: the same for a stored conversation
crates/core/src/session.rs           Tasks 5 and 6: the list, the offer
crates/core/src/app.rs               Task 6: choices, can_choose, choose
crates/app/tests/replay.rs           Task 6: the two fixtures into a real App
crates/app/src/view/tasks.rs         Task 7: the checklist
crates/app/src/view/choices.rs       Task 8: the choice box
crates/app/src/view/input.rs, mod.rs Tasks 7 and 8: their place and their height
crates/app/src/glue.rs               Task 7: SessionView.tasks_open
crates/app/src/demo.rs               Tasks 7 and 8: scenes tasks, tasks-folded, choices
crates/app/tests/snapshots.rs        Task 9: the three scenes
crates/app/tests/live.rs             Task 9: what the real program does
docs/notes-for-later-plans.md        Task 9: what was built, and Jerod's look
```

## Facts this plan builds on

From `docs/protocol-notes.md` (seen live) and the research report (the code as built):

- The program's control request `{"subtype":"mcp_message","server_name":"mindrune","message":{…JSON-RPC…}}` wants `{"type":"control_response","response":{"subtype":"success","request_id":…,"response":{"mcp_response":{…}}}}`. The handshake (`initialize`, `notifications/initialized`, `tools/list`) comes right after the host's own `initialize` request, before any user message.
- `Reader::line` (`crates/claude/src/agent.rs:258`) already writes one kind of reply itself (`refuse`, `:315`), through a weak link to the process that is set before the host's `initialize` is sent.
- A tool call reaches the core today only as `ToolStarted { name, detail: ToolDetail::Other { input_json } }`, and a result only as its text: `tool_use_result` is read nowhere.
- Live, `REQUEST_TOOLS` hides two tools in the translator (`translate.rs:122`); the stored-history reader hides nothing (`history.rs:516`).
- `Session::take_message` (`session.rs:717`) is the one place a user's message leaves the session; `App::submit` (`app.rs:1534`) the one path a typed message takes, a resting session's wake included.
- Between the conversation and the input box there is only the optional "open elsewhere" card; `input::extra_height` (`input.rs:82`) is what the text area's lines give way to; a stacked tile is 280 points high or less.
- `ClaudeAgent::open` (`agent.rs:189`) fills the program's arguments and environment for every start, the backend's look included; `build_args` and `child_env` have tests that pin their whole output.

---

### Task 1: The agent's vocabulary gains a task list and choices

**Files:**
- Create: `crates/agent/src/tasks.rs`, `crates/agent/src/choices.rs`
- Modify: `crates/agent/src/lib.rs` (the two modules, their types exported at the crate's root), `crates/agent/src/event.rs`, `crates/agent/src/agent.rs`
- Modify, to keep the workspace building: every exhaustive match on `SessionEvent` (`crates/core/src/session.rs`, `crates/core/src/conversation.rs`) gets the four new events as arms that do nothing yet; every place that builds a `StoredHistory` gets the two new fields empty.

**Interfaces:**
- Consumes: nothing.
- Produces, all `Clone, Debug, PartialEq, Serialize, Deserialize` (`TaskStatus` also `Copy, Eq`):

```rust
// tasks.rs
pub enum TaskStatus { Pending, InProgress, Completed }
pub struct TaskStep { pub id: String, pub subject: String, pub active: Option<String>, pub status: TaskStatus }
pub struct TaskChange { pub id: String, pub status: Option<TaskStatus>, pub subject: Option<String>, pub active: Option<String> }
pub const MAX_STEPS: usize = 100;
pub const MAX_STEP_TEXT: usize = 300;       // characters, of a subject or an active wording
#[derive(Default)] pub struct TaskList { /* private: the steps, in the order added */ }
impl TaskList {
    pub fn from_steps(steps: Vec<TaskStep>) -> TaskList;   // through `add`, so the limits hold
    pub fn steps(&self) -> &[TaskStep];
    pub fn is_empty(&self) -> bool;
    pub fn add(&mut self, step: TaskStep) -> bool;
    pub fn change(&mut self, change: &TaskChange) -> bool;
    pub fn remove(&mut self, id: &str) -> bool;
    pub fn done(&self) -> usize;                            // steps Completed
    pub fn current(&self) -> Option<usize>;                 // the first InProgress, else the first Pending
    pub fn turn_ended(&mut self) -> bool;                   // clears a list with no open step; true when it cleared
}

// choices.rs
pub const MAX_CHOICES: usize = 3;
pub const MAX_QUESTION: usize = 300;
pub const MAX_LABEL: usize = 40;
pub const MAX_MESSAGE: usize = 2000;
pub struct Choice { pub label: String, pub message: String }
pub struct Choices { pub question: String, pub choices: Vec<Choice> }
impl Choices { pub fn new(question: &str, choices: Vec<Choice>) -> Result<Choices, String>; }

// event.rs, new variants of SessionEvent
TaskAdded(TaskStep),
TaskChanged(TaskChange),
TaskRemoved { id: String },
ChoicesOffered(Choices),

// agent.rs, new fields of StoredHistory
pub tasks: Vec<TaskStep>,          // the list as the stored conversation leaves it; empty when it has no open step
pub choices: Option<Choices>,      // offered last, with nothing said after it
```

Rules the bodies must keep:
- `add` trims the subject and the active wording, cuts each to `MAX_STEP_TEXT` characters, and takes an empty active wording as none. It refuses, changing nothing: an empty id, an empty subject, an id already in the list, a list of `MAX_STEPS`.
- `change` finds the step by id and sets what the change names, with the same trimming and cutting; an empty subject is not taken; an empty active wording clears it. It returns false for an unknown id or when nothing differs.
- `Choices::new` trims every text. Its errors are these sentences, in this order of checking: `A question is needed.`; `The question is at most 300 characters.`; `One to three choices are needed.`; `Every choice needs a label and a message.`; `A label is at most 40 characters.`; `A message is at most 2000 characters.` Lengths are counted in characters, after trimming.
- The enum's doc gains: the four events come only inside a turn; a `ChoicesOffered` may be followed by more of that turn.

- [ ] **Step 1: Write the failing tests** in `tasks.rs` and `choices.rs`:

```rust
fn step(id: &str, subject: &str) -> TaskStep {
    TaskStep { id: id.into(), subject: subject.into(), active: None, status: TaskStatus::Pending }
}
fn status(id: &str, status: TaskStatus) -> TaskChange {
    TaskChange { id: id.into(), status: Some(status), subject: None, active: None }
}

#[test]
fn steps_are_kept_in_the_order_added_and_an_id_only_once() {
    let mut list = TaskList::default();
    assert!(list.add(step("1", "Create a.txt")));
    assert!(list.add(step("2", "Create b.txt")));
    assert!(!list.add(step("1", "Again")), "the id is there");
    assert!(!list.add(step("3", "   ")), "no subject");
    assert!(!list.add(step("", "No id")));
    let subjects: Vec<&str> = list.steps().iter().map(|s| s.subject.as_str()).collect();
    assert_eq!(subjects, ["Create a.txt", "Create b.txt"]);
}

#[test]
fn a_change_sets_only_what_it_names_and_an_unknown_step_is_ignored() {
    let mut list = TaskList::from_steps(vec![step("1", "Create a.txt")]);
    assert!(list.change(&status("1", TaskStatus::InProgress)));
    assert!(!list.change(&status("1", TaskStatus::InProgress)), "nothing differs");
    assert!(!list.change(&status("9", TaskStatus::Completed)));
    let wording = TaskChange { id: "1".into(), status: None, subject: Some("  ".into()), active: Some(" Creating a.txt ".into()) };
    assert!(list.change(&wording));
    assert_eq!(list.steps()[0].subject, "Create a.txt", "an empty subject is not taken");
    assert_eq!(list.steps()[0].active.as_deref(), Some("Creating a.txt"));
    assert_eq!(list.steps()[0].status, TaskStatus::InProgress);
}

#[test]
fn the_current_step_is_the_one_in_progress_or_else_the_next_pending() {
    let mut list = TaskList::from_steps(vec![step("1", "a"), step("2", "b"), step("3", "c")]);
    assert_eq!(list.current(), Some(0));
    list.change(&status("1", TaskStatus::Completed));
    assert_eq!((list.current(), list.done()), (Some(1), 1));
    list.change(&status("3", TaskStatus::InProgress));
    assert_eq!(list.current(), Some(2), "in progress wins over an earlier pending one");
}

#[test]
fn a_turns_end_clears_only_a_list_with_nothing_open() {
    let mut list = TaskList::from_steps(vec![step("1", "a"), step("2", "b")]);
    list.change(&status("1", TaskStatus::Completed));
    assert!(!list.turn_ended());
    assert_eq!(list.steps().len(), 2, "the ticked step stays with the open one");
    list.change(&status("2", TaskStatus::Completed));
    assert!(list.turn_ended());
    assert!(list.is_empty() && list.current().is_none());
    assert!(!TaskList::default().turn_ended(), "nothing to clear");
}

#[test]
fn long_texts_are_cut_and_the_list_is_bounded() {
    let long = "é".repeat(400);
    let mut list = TaskList::default();
    list.add(TaskStep { active: Some(long.clone()), ..step("1", &long) });
    assert_eq!(list.steps()[0].subject.chars().count(), MAX_STEP_TEXT);
    assert_eq!(list.steps()[0].active.as_ref().unwrap().chars().count(), MAX_STEP_TEXT);
    for n in 2..=200 { list.add(step(&n.to_string(), "x")); }
    assert_eq!(list.steps().len(), MAX_STEPS);
    assert!(list.remove("1") && !list.remove("1"));
}
```

```rust
fn choice(label: &str, message: &str) -> Choice { Choice { label: label.into(), message: message.into() } }

#[test]
fn choices_are_trimmed_and_kept_in_order() {
    let offer = Choices::new(" Apply now? ", vec![choice(" Apply ", " /apply "), choice("Not now", "Leave it for now.")]).unwrap();
    assert_eq!(offer.question, "Apply now?");
    assert_eq!(offer.choices, [choice("Apply", "/apply"), choice("Not now", "Leave it for now.")]);
}

#[test]
fn what_cannot_be_shown_is_refused_with_a_sentence() {
    let one = || vec![choice("Yes", "Yes.")];
    assert_eq!(Choices::new("  ", one()).unwrap_err(), "A question is needed.");
    assert_eq!(Choices::new(&"q".repeat(301), one()).unwrap_err(), "The question is at most 300 characters.");
    assert_eq!(Choices::new("Q?", vec![]).unwrap_err(), "One to three choices are needed.");
    assert_eq!(Choices::new("Q?", vec![choice("a", "a"); 4]).unwrap_err(), "One to three choices are needed.");
    assert_eq!(Choices::new("Q?", vec![choice(" ", "x")]).unwrap_err(), "Every choice needs a label and a message.");
    assert_eq!(Choices::new("Q?", vec![choice("x", "")]).unwrap_err(), "Every choice needs a label and a message.");
    assert_eq!(Choices::new("Q?", vec![choice(&"l".repeat(41), "x")]).unwrap_err(), "A label is at most 40 characters.");
    assert_eq!(Choices::new("Q?", vec![choice("x", &"m".repeat(2001))]).unwrap_err(), "A message is at most 2000 characters.");
    assert!(Choices::new(&"é".repeat(300), vec![choice(&"é".repeat(40), &"é".repeat(2000))]).is_ok(), "counted in characters");
}
```

In `event.rs`, extend the round-trip test with the four new events.

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p mr-agent`. Expected: does not compile (the types are missing).
- [ ] **Step 3: Implement** the two modules, the events and the fields, with the rules above. Add the do-nothing arms and the empty fields that keep the workspace building.
- [ ] **Step 4: Run the three checks.** Expected: all pass; no test outside `mr-agent` changes its assertions.
- [ ] **Step 5: Commit** by path: `feat(agent): a task list and choices in the agent's vocabulary`.

---

### Task 2: The adapter serves the choice tool and starts the program with both

**Files:**
- Create: `crates/claude/src/choices.rs` (the probe's `crates/probe/src/choices.rs` moves here and grows)
- Modify: `crates/claude/src/lib.rs`, `crates/claude/src/env.rs`, `crates/claude/src/agent.rs`, `crates/claude/tests/stub/fake_claude.sh`, `crates/claude/tests/agent.rs`
- Modify: `crates/probe/src/choices.rs` (keeps only `request`), `crates/probe/src/repl.rs`

**Interfaces:**
- Consumes: `Choices`, `Choice` (Task 1).
- Produces:

```rust
// crates/claude/src/choices.rs, exported as mr_claude::choices
pub const SERVER: &str = "mindrune";
pub const TOOL: &str = "offer_choices";
pub fn tool_name() -> String;                              // "mcp__mindrune__offer_choices"
pub fn args() -> Vec<String>;                              // six arguments, below
pub fn reply(request_id: &str, body: &Value) -> String;    // `body` is the control request's `request` object
pub fn offered(arguments: &Value) -> Result<Choices, String>;   // a call's input, read and checked

// crates/claude/src/env.rs
pub fn with_task_tools(env: Vec<(String, String)>) -> Vec<(String, String)>;
```

Exact values:
- `args()` is `["--mcp-config", C, "--allowedTools", "mcp__mindrune__offer_choices", "--append-system-prompt", INSTRUCTION]`, where `C` is the JSON `{"mcpServers":{"mindrune":{"type":"sdk","name":"mindrune","alwaysLoad":true}}}`.
- `INSTRUCTION` is the probe's, word for word (it is what the recording was made with): `This session runs in a desktop app that can show the user buttons. When your work in a turn is done and the next move is the user's decision (a choice between ways forward, a go-ahead, or an obvious next command), call the tool mcp__mindrune__offer_choices once, with one short question and one to three choices, your recommendation first. A choice's message is sent as the user's next message when they click it, and it may be a slash command of this project. Do not call it when nothing is pending, and do not ask the same question again in your reply.`
- The listed tool is the probe's, with two keys added beside `inputSchema`: `"annotations":{"readOnlyHint":true}` and `"_meta":{"anthropic/alwaysLoad":true}`.
- `offered` reads `question` (a string) and `choices` (a list of objects with string `label` and `message`) and passes them to `Choices::new`. A `question` that is missing or no string counts as empty; `choices` that is missing or no list as none; an entry that is no object, or whose `label` or `message` is missing or no string, as a choice with that text empty. So every malformed input ends in one of `Choices::new`'s sentences.
- `reply` answers as the probe does (`initialize` with the version asked for, a notification with an empty result, `tools/list`, an unknown method or server with code `-32601`), except `tools/call`: for `offer_choices` it answers `{"content":[{"type":"text","text":"Shown to the user."}]}` when `offered(params.arguments)` is `Ok`, and `{"content":[{"type":"text","text":"<the sentence> Nothing was shown."}],"isError":true}` when it is not. A body without `server_name` or `message` is answered as an unknown server.
- `with_task_tools` drops every entry named `CLAUDE_CODE_ENABLE_TODO_TOOLS` or `CLAUDE_CODE_ENABLE_TASKS` and appends `("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1")`; the order of the rest is kept.
- `Reader::line`: an `Incoming::ControlRequest` whose subtype is `mcp_message` is answered with `choices::reply(request_id, body)` written to the program, and nothing else is done with it. Every other subtype but `can_use_tool` is refused as today.
- `ClaudeAgent::open` builds its `SpawnConfig` through a new private `fn spawn_config(&self, spec: StartSpec, id: &SessionId, full_login: bool) -> SpawnConfig`: `args` are `build_args(..)` followed by `choices::args()`; `env` is `with_task_tools(child_env(..))`. `build_args` and `child_env` do not change.
- `HARMLESS_KINDS` gains `"system/thinking_tokens"`.
- The stub gains one more word: a user message containing `offer-choices` plays a turn that, after its `MSG_START`, sends the server request `req_m` (below) and holds the rest of the turn back until it reads a line containing both `"mcp_response"` and `req_m`; then it sends the `tool_use` record, its `tool_result`, a text reply and the result. Describe it in the stub's header.

```
{"type":"control_request","request_id":"req_m","request":{"subtype":"mcp_message","server_name":"mindrune","message":{"method":"tools/call","params":{"name":"offer_choices","arguments":{"question":"Publish the new banner?","choices":[{"label":"Publish","message":"/publish"},{"label":"Not now","message":"Leave it for now."}]},"_meta":{"claudecode/toolUseId":"t_c","progressToken":2}},"jsonrpc":"2.0","id":2}}}
```

- The probe: `crates/probe/src/choices.rs` keeps `request(line)` and its test; everything else it had is the adapter's now. `repl.rs` calls `mr_claude::choices::args()` and `mr_claude::choices::reply(&id, &body)`; `request` therefore gives `(request_id, body)`, the `request` object whole.

- [ ] **Step 1: Write the failing tests.** In `crates/claude/src/choices.rs`, the probe's six tests move over, changed where the interface did (`reply` takes the body; the config has `alwaysLoad`), and these are new:

```rust
fn call(arguments: Value) -> Value {
    let body = json!({"subtype":"mcp_message","server_name":"mindrune","message":
        {"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"offer_choices","arguments":arguments}}});
    let line: Value = serde_json::from_str(&reply("req_1", &body)).unwrap();
    line["response"]["response"]["mcp_response"]["result"].clone()
}

#[test]
fn an_offer_that_can_be_shown_is_answered_as_shown() {
    let result = call(json!({"question":"Apply now?","choices":[{"label":"Apply","message":"/apply"}]}));
    assert_eq!(result, json!({"content":[{"type":"text","text":"Shown to the user."}]}));
}

#[test]
fn an_offer_that_cannot_be_shown_is_refused_in_words() {
    let said = |arguments: Value| {
        let result = call(arguments);
        assert_eq!(result["isError"], true);
        result["content"][0]["text"].as_str().unwrap().to_string()
    };
    assert_eq!(said(json!({})), "A question is needed. Nothing was shown.");
    assert_eq!(said(json!({"question":"Q?","choices":"yes"})), "One to three choices are needed. Nothing was shown.");
    assert_eq!(said(json!({"question":"Q?","choices":[{"label":"a","message":"a"},{"label":"b","message":"b"},{"label":"c","message":"c"},{"label":"d","message":"d"}]})),
        "One to three choices are needed. Nothing was shown.");
    assert_eq!(said(json!({"question":"Q?","choices":[{"label":7,"message":"x"}]})), "Every choice needs a label and a message. Nothing was shown.");
    assert_eq!(said(json!({"question":"Q?","choices":["yes"]})), "Every choice needs a label and a message. Nothing was shown.");
    assert_eq!(said(json!({"question":["Q?"],"choices":[{"label":"a","message":"a"}]})), "A question is needed. Nothing was shown.");
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
    assert_eq!(config, json!({"mcpServers":{"mindrune":{"type":"sdk","name":"mindrune","alwaysLoad":true}}}));
}

#[test]
fn a_body_without_a_server_or_a_message_is_still_answered() {
    let line: Value = serde_json::from_str(&reply("req_9", &json!({"subtype":"mcp_message"}))).unwrap();
    assert_eq!(line["response"]["request_id"], "req_9");
    assert_eq!(line["response"]["response"]["mcp_response"]["error"]["code"], -32601);
}
```

In `env.rs`:

```rust
#[test]
fn the_task_tools_are_asked_for_once_whatever_the_shell_says() {
    let given = env(&[("PATH", "/bin"), ("CLAUDE_CODE_ENABLE_TODO_TOOLS", "0"), ("HOME", "/home/alex"), ("CLAUDE_CODE_ENABLE_TASKS", "0")]);
    assert_eq!(with_task_tools(given), env(&[("PATH", "/bin"), ("HOME", "/home/alex"), ("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1")]));
    assert_eq!(with_task_tools(Vec::new()), env(&[("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1")]));
}
```

In `agent.rs`'s unit tests (make an agent with `with_paths` as the existing ones do):

```rust
#[test]
fn every_start_serves_the_choice_tool_and_asks_for_the_task_tools() {
    // An agent whose parent environment is a Claude session's, with its own task setting.
    let config = /* agent.spawn_config(StartSpec::new("/work"), &id, false) */;
    let own = build_args(&StartSpec::new("/work"), Some(&id));
    assert_eq!(config.args[..own.len()], own[..]);
    assert_eq!(config.args[own.len()..], choices::args()[..]);
    let named: Vec<&(String, String)> = config.env.iter().filter(|(k, _)| k == "CLAUDE_CODE_ENABLE_TODO_TOOLS").collect();
    assert_eq!(named, [&("CLAUDE_CODE_ENABLE_TODO_TOOLS".to_string(), "1".to_string())]);
    assert!(config.env.iter().all(|(k, _)| k != "CLAUDECODE" && k != "CLAUDE_CODE_ENABLE_TASKS"));
}
```

In `crates/claude/tests/agent.rs`:

```rust
#[test]
fn the_programs_call_of_the_choice_tool_is_answered() {
    // Send "offer-choices please". The stub finishes the turn only once the server's answer reached it.
    // Assert: the events hold a TurnFinished within the test's usual limit, and no CommandFailed.
}
```

and keep `a_turn_arrives_as_agent_events` passing unchanged: a `hook_callback` request is still refused.

In the probe, `request`'s test now expects the body: `Some(("req_1".to_string(), json!({"subtype":"mcp_message","server_name":"mindrune","message":{"jsonrpc":"2.0","id":3,"method":"tools/list"}})))`.

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p mr-claude -p mr-probe`. Expected: does not compile.
- [ ] **Step 3: Implement** as specified. The reply is written from the reader thread through the same weak link `refuse` uses; hold no translator lock while writing.
- [ ] **Step 4: Run the three checks.** Also run the stand-in check by hand: `(sleep 2; echo quit) | target/debug/mindrune-probe raw --cwd /tmp --program crates/claude/tests/stub/fake_claude.sh --choices` must exit 0. Expected: all pass; `build_args`' and `child_env`'s own tests are unchanged.
- [ ] **Step 5: Commit** by path: `feat(claude): serve the choice tool and ask for the task tools at every start`.

---

### Task 3: The adapter turns the calls into events, and they are no tool lines

**Files:**
- Create: `crates/claude/src/tasks.rs`
- Modify: `crates/claude/src/lib.rs`, `crates/claude/src/wire.rs`, `crates/claude/src/translate.rs`

**Interfaces:**
- Consumes: `TaskStep`, `TaskChange`, `TaskStatus`, `Choices`, the four events (Task 1); `choices::offered`, `choices::tool_name` (Task 2).
- Produces:

```rust
// wire.rs: ToolResult gains
pub structured: Value,     // the record's top-level `tool_use_result`; Null when absent or when the record holds several results

// tasks.rs
pub(crate) enum Hidden {
    Plain,                                              // hidden, and says nothing
    Create { subject: String, active: Option<String> },
    Change(TaskChange),
    Remove { id: String },
    Offer,
}
pub(crate) fn hidden(name: &str, input: &Value) -> Option<Hidden>;      // None: an ordinary tool call
pub(crate) fn created_id(structured: &Value, text: &str) -> Option<String>;
```

Rules:
- `hidden`: `TaskCreate` gives `Create` with `subject` and `activeForm` (or `active_form`), or `Plain` when `subject` is missing or no string. `TaskUpdate` reads its id from `taskId`, else `id`, else `task_id`, a string or a number (a number is written without a fraction); with status `deleted` it gives `Remove`; otherwise `Change` with `status` (`pending`, `in_progress`, `completed`; any other word is no status), `subject`, and `activeForm` or `active_form`; without an id, `Plain`. `TaskGet` and `TaskList` give `Plain`. The choice tool's name gives `Offer`. `ToolSearch` gives `Plain` when `query` is `select:` followed by names separated by commas, at least one, each of them `TaskCreate`, `TaskUpdate`, `TaskGet`, `TaskList` or the choice tool's name (spaces around a name are ignored); any other `ToolSearch` is `None`. Every other name is `None`.
- `created_id`: `structured.task.id` as a string or a number; else, from text that starts with `Task #` and continues, after the id, with ` created`, the characters between, when there are any; else none.
- The translator, for a `tool_use` block of the main conversation that is not a request tool: when `hidden` gives `Some`, no `ToolStarted` and no `BackgroundTasks` come from it, and what it is is kept by the call's id. An `Offer` whose input `choices::offered` accepts gives `ChoicesOffered` at once; one it refuses gives nothing (the server already told Claude).
- For a result whose call is kept that way: no `ToolFinished`. When the result is no error: `Create` gives `TaskAdded(TaskStep { id, subject, active, status: Pending })` if `created_id(&result.structured, &result.text)` finds an id, else nothing; `Change` gives `TaskChanged`; `Remove` gives `TaskRemoved`. An error result gives nothing.
- At a turn's `result` record, what is still kept is dropped: a call that was never answered adds nothing.
- Records of sub-agents stay unseen, as today.

- [ ] **Step 1: Write the failing tests.** In `tasks.rs`:

```rust
#[test]
fn the_task_tools_are_hidden_and_say_what_they_do() {
    assert!(matches!(hidden("TaskCreate", &json!({"subject":"Create a.txt","description":"x","activeForm":"Creating a.txt"})),
        Some(Hidden::Create { subject, active }) if subject == "Create a.txt" && active.as_deref() == Some("Creating a.txt")));
    assert!(matches!(hidden("TaskCreate", &json!({"subject":"S","active_form":"Doing"})), Some(Hidden::Create { active, .. }) if active.as_deref() == Some("Doing")));
    assert!(matches!(hidden("TaskCreate", &json!({"subject":7})), Some(Hidden::Plain)));
    let change = |input: Value| match hidden("TaskUpdate", &input) { Some(Hidden::Change(c)) => c, other => panic!("{other:?}") };
    assert_eq!(change(json!({"taskId":"1","status":"in_progress"})), TaskChange { id: "1".into(), status: Some(TaskStatus::InProgress), subject: None, active: None });
    assert_eq!(change(json!({"id":2,"status":"completed"})).id, "2");
    assert_eq!(change(json!({"task_id":"3","status":"paused","subject":"New","activeForm":"Doing"})),
        TaskChange { id: "3".into(), status: None, subject: Some("New".into()), active: Some("Doing".into()) });
    assert!(matches!(hidden("TaskUpdate", &json!({"taskId":"4","status":"deleted"})), Some(Hidden::Remove { id }) if id == "4"));
    assert!(matches!(hidden("TaskUpdate", &json!({"status":"completed"})), Some(Hidden::Plain)));
    assert!(matches!(hidden("TaskList", &json!({})), Some(Hidden::Plain)));
    assert!(matches!(hidden("mcp__mindrune__offer_choices", &json!({})), Some(Hidden::Offer)));
    assert!(hidden("Bash", &json!({"command":"ls"})).is_none());
    assert!(hidden("TodoWrite", &json!({"todos":[]})).is_none());
}

#[test]
fn a_search_that_only_loads_hidden_tools_is_hidden_too() {
    let search = |query: &str| hidden("ToolSearch", &json!({"query": query, "max_results": 2})).is_some();
    assert!(search("select:TaskCreate,TaskUpdate"));
    assert!(search("select:mcp__mindrune__offer_choices"));
    assert!(search("select: TaskCreate , TaskList"));
    assert!(!search("select:TaskCreate,Bash"), "it loads an ordinary tool too");
    assert!(!search("select:"));
    assert!(!search("task list tools"));
    assert!(hidden("ToolSearch", &json!({})).is_none());
}

#[test]
fn a_new_steps_id_comes_from_the_result() {
    assert_eq!(created_id(&json!({"task":{"id":"1","subject":"Create a.txt"}}), "ignored").as_deref(), Some("1"));
    assert_eq!(created_id(&json!({"task":{"id":12}}), "").as_deref(), Some("12"));
    assert_eq!(created_id(&Value::Null, "Task #4 created successfully: Add second line to a.txt").as_deref(), Some("4"));
    assert_eq!(created_id(&json!([{"type":"text","text":"x"}]), "Task #abc created successfully: x").as_deref(), Some("abc"));
    assert_eq!(created_id(&Value::Null, "Updated task #1 status"), None);
    assert_eq!(created_id(&Value::Null, "Task # created"), None);
}
```

In `wire.rs`: a user record with one `tool_result` and a top-level `tool_use_result` gives that value as `structured`; without the key, `Value::Null`; with two `tool_result` blocks, `Null` for both.

In `translate.rs`, with the module's `feed` helper and lines written by hand in the recorded shapes:

```rust
#[test]
fn a_task_call_becomes_a_step_when_its_result_names_the_id() { /* TaskCreate use, then its result with tool_use_result {"task":{"id":"1",…}}:
    exactly one TaskAdded(TaskStep { id "1", subject "Create a.txt", active Some("Creating a.txt"), Pending }), no ToolStarted, no ToolFinished */ }
#[test]
fn a_task_call_that_failed_or_was_never_answered_adds_nothing() { /* result with "is_error":true: no TaskAdded;
    a TaskCreate use, then the turn's result record, then a late tool_result for it: still nothing, and no ToolFinished */ }
#[test]
fn an_update_changes_a_step_and_deleted_removes_it() { /* TaskChanged with InProgress at its result; TaskRemoved { id: "4" } */ }
#[test]
fn an_offer_is_announced_at_its_call_and_a_refused_one_is_not() { /* tool_use mcp__mindrune__offer_choices with two choices:
    ChoicesOffered at once, before its result; with four choices: nothing; neither gives a tool event */ }
#[test]
fn a_result_without_its_call_is_an_ordinary_result() { /* ToolFinished as today */ }
```

And two that play the fixtures whole, each line through `parse_line` into one `Translator`:

```rust
#[test]
fn the_recorded_task_list_gives_six_steps_and_no_tool_line_of_its_own() {
    let events = play("v12-tasks.jsonl");
    let added: Vec<(&str, &str, Option<&str>)> = /* every TaskAdded: (id, subject, active) */;
    assert_eq!(added.len(), 6);
    assert_eq!(added[0], ("1", "Create a.txt", Some("Creating a.txt")));
    assert_eq!(added[5].0, "6");
    assert_eq!(/* count of TaskChanged */, 12);
    assert_eq!(/* the first TaskChanged */, TaskChange { id: "1".into(), status: Some(TaskStatus::InProgress), subject: None, active: None });
    let names: Vec<&str> = /* the name of every ToolStarted, in order */;
    assert_eq!(names, ["Write", "Write", "Write", "Bash", "Edit", "Edit", "Edit", "Bash"]);
    assert_eq!(/* count of TurnFinished */, 2);
}

#[test]
fn the_recorded_offer_is_announced_before_its_turn_ends() {
    let events = play("v14-choices.jsonl");
    let offers: Vec<&Choices> = /* every ChoicesOffered */;
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].question, "Keep or delete the three text files in this folder?");
    assert_eq!(offers[0].choices, [
        Choice { label: "Keep them".into(), message: "Keep the three text files.".into() },
        Choice { label: "Delete them".into(), message: "Delete the three text files in this folder.".into() }]);
    assert!(events.iter().all(|e| !matches!(e, SessionEvent::ToolStarted { .. })), "its search and its call are hidden");
    let offer_at = /* position of the ChoicesOffered */; let end_at = /* position of the first TurnFinished */;
    assert!(offer_at < end_at);
    assert!(events[offer_at..end_at].iter().any(|e| matches!(e, SessionEvent::TextDelta(_))), "Claude wrote on after it");
}
```

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p mr-claude`. Expected: does not compile, then fails.
- [ ] **Step 3: Implement** as specified. Keep `REQUEST_TOOLS` and its own set as they are.
- [ ] **Step 4: Run the three checks.** `crates/app/tests/replay.rs` plays every fixture and must still pass unchanged.
- [ ] **Step 5: Commit** by path: `feat(claude): task and choice calls become events and are no tool lines`.

---

### Task 4: A stored conversation gives back its open list and its last offer

**Files:**
- Modify: `crates/claude/src/history.rs`, `crates/claude/src/history_tail.rs`

**Interfaces:**
- Consumes: `tasks::hidden`, `tasks::created_id`, `Hidden` (Task 3); `choices::offered` (Task 2); `TaskList`, `StoredHistory.tasks`, `StoredHistory.choices` (Task 1).
- Produces: `read_tail` (and any other reader of a whole transcript) fills `StoredHistory.tasks` and `StoredHistory.choices`, and leaves the hidden calls out of `items`.

Rules, applied to the records in the order the reader already walks them:
- A `tool_use` block for which `hidden` gives `Some` is no `HistoryItem`. What it is is kept by the call's id, as live. An `Offer` that `choices::offered` accepts becomes the offer; one it refuses changes nothing.
- A `tool_result` for a kept call fills no item. When it is no error it is applied to a `TaskList` as live. Its id for a `Create` is `created_id(structured, text)`, where `structured` is the record's top-level `toolUseResult`, or else its `tool_use_result`, or else null.
- A record that gives a `HistoryItem::User` starts a new turn: first the list's `turn_ended()` is called, and the offer is forgotten.
- At the end, `turn_ended()` is called once more. `tasks` is what the list then holds; `choices` is the offer, if any.
- Only the records that are read count: with a cut-off conversation, an update for a step that was never read changes nothing.

- [ ] **Step 1: Write the failing tests** in `history_tail.rs` (or where `read_tail`'s tests live), writing transcripts by hand in the stored shape with the module's helpers. Use ids such as `toolu_a`; chain the records by `uuid` and `parentUuid` as the existing tests do.

```rust
#[test]
fn a_list_with_an_open_step_comes_back_and_its_calls_are_no_items() {
    // user "Make three files"; assistant TaskCreate ×2 with results (one with toolUseResult {"task":{"id":"1"}},
    // one with only the text "Task #2 created successfully: Create b.txt"); TaskUpdate 1 in_progress, 1 completed,
    // 2 in_progress, each with a result; a Write call with its result.
    let history = /* read */;
    assert_eq!(history.tasks.iter().map(|s| (s.id.as_str(), s.status)).collect::<Vec<_>>(),
        [("1", TaskStatus::Completed), ("2", TaskStatus::InProgress)]);
    let names: Vec<&str> = /* names of the HistoryItem::Tool items */;
    assert_eq!(names, ["Write"]);
}
#[test]
fn a_finished_list_does_not_come_back() { /* both steps completed, then a reply: tasks is empty */ }
#[test]
fn a_new_message_ends_the_turn_for_the_list() { /* steps 1 and 2 completed; a user message; TaskCreate 3 (open):
    tasks holds only step 3 */ }
#[test]
fn the_last_offer_comes_back_unless_something_was_said_after_it() {
    // …assistant offer_choices call with its result, then a closing reply: choices is Some, with the question and both choices.
    // The same followed by a user message: choices is None.
    // Two offers in one turn: the second is the one.
}
#[test]
fn the_live_spelling_of_the_result_is_read_too() { /* a record with tool_use_result in place of toolUseResult */ }
#[test]
fn a_cut_off_conversation_costs_only_what_was_cut() { /* updates for steps 1 and 2 whose creates are not in the file,
    then TaskCreate 3 open: tasks holds only step 3 */ }
#[test]
fn a_failed_call_and_a_search_for_the_tools_leave_no_trace() { /* a TaskCreate whose result has is_error true;
    a ToolSearch "select:TaskCreate,TaskUpdate": no item for either, no step; a ToolSearch "select:TaskCreate,Bash" is an item */ }
```

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p mr-claude history`. Expected: fail (the fields are empty; the calls are items).
- [ ] **Step 3: Implement.** The state the walk needs (the kept calls, the list, the offer) travels with `add_record`'s existing arguments in one struct.
- [ ] **Step 4: Run the three checks.** Say in the report which existing history tests changed.
- [ ] **Step 5: Commit** by path: `feat(claude): a stored conversation gives its open task list and last offer`.

---

### Task 5: A session keeps its task list

**Files:**
- Modify: `crates/core/src/session.rs`

**Interfaces:**
- Consumes: `TaskList` and the three task events (Task 1); `StoredHistory.tasks` (Tasks 1 and 4).
- Produces: `pub tasks: TaskList` on `Session`, empty in `Session::opening` and `Session::resting`.

Rules:
- `apply`: `TaskAdded` adds, `TaskChanged` changes, `TaskRemoved` removes. They are taken in whatever the phase.
- When a turn ends (`TurnFinished` or `TurnFailed`, where the session leaves its turn) and when the program exits, `tasks.turn_ended()` is called.
- `load_history`: when this run has not run a turn of the session (`ran_turn` is false) and the list is empty, it becomes `TaskList::from_steps(history.tasks.clone())`. Otherwise the stored list is ignored.
- Letting a session's conversation go (`let_go`) does not touch the list.

- [ ] **Step 1: Write the failing tests** in `session.rs`, with the module's `started()` and `done()` helpers:

```rust
#[test]
fn the_list_follows_the_events_and_stays_while_a_step_is_open() {
    let mut s = started();
    /* hand over a message, TurnStarted, then TaskAdded 1 and 2, TaskChanged 1 to InProgress, then 1 to Completed */
    s.apply(done(), 0.0);
    assert_eq!(s.tasks.steps().len(), 2, "step 2 is open");
    assert_eq!((s.tasks.done(), s.tasks.current()), (1, Some(1)));
}
#[test]
fn a_turn_that_ends_with_every_step_ticked_clears_the_list() { /* …then 2 to Completed, done(): tasks.is_empty() */ }
#[test]
fn a_failed_turn_and_an_exit_end_the_turn_for_the_list_too() { /* all ticked, TurnFailed: empty; all ticked, Exited: empty */ }
#[test]
fn a_removed_step_and_an_unknown_one_cost_nothing() { /* TaskRemoved for 1, TaskChanged for 9: one step left, no panic */ }
#[test]
fn the_stored_list_is_taken_only_before_this_run_ran_a_turn() {
    // A resting session: load_history with tasks [step 3 open]: tasks holds it.
    // A session that ran a turn and holds step 5: load_history with tasks [step 3]: still only step 5.
    // A resting session whose list is not empty (taken once): a second load_history does not double it.
}
```

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p mr-core tasks` (or the tests' names). Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the three checks.**
- [ ] **Step 5: Commit** by path: `feat(core): a session keeps its task list`.

---

### Task 6: A session keeps the last offer, and a click sends a choice

**Files:**
- Modify: `crates/core/src/session.rs`, `crates/core/src/app.rs`, `crates/app/tests/replay.rs`

**Interfaces:**
- Consumes: `Choices`, `ChoicesOffered` (Task 1); `StoredHistory.choices` (Tasks 1 and 4).
- Produces:

```rust
// Session
pub fn choices(&self) -> Option<&Choices>;
// App
pub fn choices(&self, key: SessionKey) -> Option<&Choices>;
pub fn can_choose(&self, key: SessionKey) -> bool;
pub fn choose(&mut self, key: SessionKey, index: usize) -> Vec<Effect>;
```

Rules:
- The session keeps the offer privately. `ChoicesOffered` sets it while the session is in a turn, and is ignored otherwise; a second one in the same turn replaces the first.
- It is forgotten: at `TurnStarted`; when the turn ends interrupted (`TurnSummary.interrupted`) or fails; and in `take_message`, the one place a user's message leaves the session, so a typed message, a queued one and a resting session's wake all forget it.
- `Session::choices` gives the offer only when the phase is `Idle` or `Resting`, nothing is queued, and no request waits.
- `load_history`: when this run has not run a turn of the session and it has no offer, the offer becomes `history.choices.clone()`.
- `App::choices` gives `Session::choices` only when `takes_message(key)` holds (so not for a deleted session, one with the "open elsewhere" question, or one being checked).
- `App::can_choose`: there are choices, and `can_send(key, m)` holds for the first choice's message.
- `App::choose` takes the message of the choice at `index` and returns `self.submit(key, &message)`. Without choices, or with an index out of range, it returns no effects and changes nothing. It sends nothing itself: whatever `submit` does for a typed message it does for this one (chips go along, a resting session is woken and checked, focus moves to the tile).

- [ ] **Step 1: Write the failing tests.** In `session.rs`:

```rust
fn offer() -> Choices { Choices::new("Publish the new banner?", vec![
    Choice { label: "Publish".into(), message: "/publish".into() },
    Choice { label: "Not now".into(), message: "Leave it for now.".into() }]).unwrap() }

#[test]
fn an_offer_shows_only_once_its_turn_has_ended() {
    let mut s = started();
    /* submit "go", TurnStarted, ChoicesOffered(offer()) */
    assert_eq!(s.choices(), None, "the turn still runs");
    s.apply(done(), 0.0);
    assert_eq!(s.choices(), Some(&offer()));
}
#[test]
fn the_last_offer_of_a_turn_wins_and_one_outside_a_turn_is_ignored() { /* … */ }
#[test]
fn sending_anything_forgets_the_offer() {
    /* after the offer shows: s.submit("something else"): choices() is None, also after that turn ends */
}
#[test]
fn a_stopped_or_failed_turn_leaves_no_offer() { /* TurnFinished with interrupted: true; TurnFailed */ }
#[test]
fn a_new_turn_that_starts_by_itself_forgets_the_offer() { /* offer shows; TurnStarted with no message sent: None */ }
#[test]
fn the_offer_hides_while_a_request_waits_or_a_message_is_queued() { /* … */ }
#[test]
fn the_stored_offer_is_taken_only_before_this_run_ran_a_turn() { /* as Task 5's last test, for choices; a resting session shows it */ }
```

In `app.rs`, with the module's existing helpers for an app with a started session:

```rust
#[test]
fn a_click_sends_the_choices_message_as_a_typed_one() {
    /* offer shown on session k */
    let effects = app.choose(k, 0);
    /* effects hold Effect::Send whose command is SessionCommand::Send { text: "/publish", attachments: [] } */
    assert_eq!(app.choices(k), None);
    assert!(app.choose(k, 0).iter().all(|e| !matches!(e, Effect::Send { .. })), "a second click sends nothing");
}
#[test]
fn an_index_that_is_not_there_does_nothing() { /* app.choose(k, 5): no effects, the offer stays */ }
#[test]
fn a_click_takes_the_attached_files_with_it_and_waits_for_one_still_being_read() {
    /* a chip that is being read: can_choose is false, choose sends nothing and the offer stays;
       once it is ready: can_choose is true, and the Send carries that attachment */
}
#[test]
fn a_deleted_session_and_one_with_a_question_waiting_offer_nothing() { /* choices(k) is None, choose does nothing */ }
#[test]
fn a_click_on_a_resting_session_wakes_it_as_a_typed_message_does() { /* restored offer; choose gives the effects a submit gives there */ }
```

In `crates/app/tests/replay.rs`, with `Recording`, `Player` and `signed_in_app`:

```rust
#[test]
fn the_recorded_offer_ends_as_two_buttons_and_a_click_sends_its_message() {
    /* play v14-choices.jsonl up to and including its first result record (line 85) */
    let offer = app.choices(key).expect("offered");
    assert_eq!(offer.choices.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["Keep them", "Delete them"]);
    let effects = app.choose(key, 0);
    /* the sent command is SessionCommand::Send { text: "Keep the three text files.", attachments: [] } */
    assert_eq!(app.choices(key), None);
}
#[test]
fn the_recorded_task_list_fills_ticks_off_and_is_gone_at_the_turns_end() {
    /* play v12-tasks.jsonl through line 73: three steps, the first InProgress, done() == 0;
       through line 211 (the first result): the list is empty */
}
```

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p mr-core` and `cargo test -p mindrune --test replay`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the three checks.**
- [ ] **Step 5: Commit** by path: `feat(core): a session keeps its last offer and a click sends a choice`.

---

### Task 7: The checklist above the message box

**Files:**
- Create: `crates/app/src/view/tasks.rs`
- Modify: `crates/app/src/view/mod.rs`, `crates/app/src/view/input.rs`, `crates/app/src/glue.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: `Session.tasks` (Task 5).
- Produces:

```rust
// view/tasks.rs
pub const ROW: f32 = 22.0;              // the height of one line of the list
pub const ROWS_MAX: usize = 6;          // steps drawn at once in an open list
pub const FOLD_BELOW: f32 = 480.0;      // a tile shorter than this starts with its list folded
pub const GAP_BELOW: f32 = 8.0;         // between the list and what is under it
pub fn strip(ui: &Ui, kit: &Kit, key: SessionKey) -> Option<El>;
pub fn height(list: &TaskList, open: bool) -> f32;
pub fn is_open(view: &SessionView, tile_height: Option<f32>) -> bool;
pub fn window(len: usize, current: Option<usize>, max: usize) -> std::ops::Range<usize>;
pub fn folded_words(list: &TaskList) -> String;
// glue.rs: SessionView gains
pub tasks_open: Option<bool>,           // the user's own choice for this list; none until they make one
```

Exact behaviour:
- **Place.** Inside `mr-input-area`, above `mr-input-box` and as wide as it, so it does not scroll with the conversation. Nothing is drawn for an empty list or a deleted session.
- **Open**, keyed `mr-tasks-{n}` with the session's number as other keys of the tile have it: a head line `mr-tasks-head` (a down triangle, `Tasks` in the kit's small medium type and muted, then the count `2 of 5` small and faint), then one line per step drawn, keyed `mr-task-{i}` by its place in the list with its mark keyed `mr-task-mark-{i}`, then, when steps are left out, a line keyed `mr-tasks-more` reading `4 more` (small, faint; the number of steps not drawn).
- **A step's line**: a mark of 14 points, a gap of 8, the words on one line, cut with an ellipsis. Done: the kit's `check` glyph in `kit.c.success.fg`, words in `text_faint`. Current, while the session is in a turn (`Working` or `NeedsUser`): the tile's spinner (as `items::spinner`), the active wording, or the subject when there is none, in `text_base`. Current while no turn runs, and every other step: a ring (14 points, radius 7, a 1-point border in `icon_muted`), the subject in `text_muted`.
- **Which steps**: `window(len, current, ROWS_MAX)`: everything when `len <= max`; else `max` steps in a row that hold the current one with one step before it where there is one, kept inside the list; with no current step, the last `max`.
- **Folded**: one line `mr-tasks-head` with a right triangle, the current step's mark, and `folded_words`: `3 of 5: Creating c.txt` (the current step's place counted from one, the number of steps, its active wording or subject); with no open step, `5 of 5 done`.
- **Open or folded**: `is_open` is the user's choice when they made one, else `tile_height` is unknown or at least `FOLD_BELOW`. A press on the head line sets `tasks_open` to the opposite of what shows. `tidy_views` sets it back to none when the session's list is empty.
- **Height**: `height` is 0 for an empty list; folded, `ROW + GAP_BELOW`; open, `ROW` times (1 + the steps drawn + 1 if some are left out), plus `GAP_BELOW`. `input::extra_height` adds it, so the text area gives way to it. In a tile so short that the area is cut, the list is cut first, from its top (`pane`'s order as built).
- **Not focusable.** The head line is pressed with the pointer only (`requests::unfocusable`). For a screen reader the list is labelled `Tasks, 2 of 5 done`, and each step says `Done`, `In progress` or `Not started` after its words.
- **Two scenes** in `demo.rs`, both added to `demo::SCENES`: `tasks`, one tall tile mid-turn with five steps of the session "Resizing the banner" (`Measure the current banner` done, `Resize the image` done, `Update the stylesheet` current with the active wording `Updating the stylesheet`, `Check the page on a phone`, `Write the change note`), scripted with the new events and stopped mid-turn as `working` is; and `tasks-folded`, the same turn in the upper tile of a column of two.

- [ ] **Step 1: Write the failing tests.** In `view/tasks.rs`:

```rust
#[test]
fn the_window_keeps_the_current_step_in_sight() {
    assert_eq!(window(5, Some(2), 6), 0..5);
    assert_eq!(window(10, Some(0), 6), 0..6);
    assert_eq!(window(10, Some(4), 6), 3..9, "one step before the current one");
    assert_eq!(window(10, Some(9), 6), 4..10, "kept inside the list");
    assert_eq!(window(10, None, 6), 4..10);
    assert_eq!(window(0, None, 6), 0..0);
}
#[test]
fn the_folded_line_says_where_the_work_is() {
    /* five steps, two done, the third in progress with active "Updating the stylesheet" */
    assert_eq!(folded_words(&list), "3 of 5: Updating the stylesheet");
    /* no active wording: the subject */  assert_eq!(folded_words(&plain), "3 of 5: Update the stylesheet");
    /* all five done */                   assert_eq!(folded_words(&done), "5 of 5 done");
}
#[test]
fn the_height_counts_what_is_drawn() {
    assert_eq!(height(&TaskList::default(), true), 0.0);
    assert_eq!(height(&five, false), 30.0);
    assert_eq!(height(&five, true), 6.0 * 22.0 + 8.0);
    assert_eq!(height(&ten, true), 8.0 * 22.0 + 8.0, "the head, six steps and the line for the rest");
}
#[test]
fn a_short_tile_starts_folded_and_the_users_choice_wins() {
    let mut view = /* a fresh SessionView */;
    assert!(is_open(&view, None) && is_open(&view, Some(480.0)) && !is_open(&view, Some(479.0)));
    view.tasks_open = Some(true);  assert!(is_open(&view, Some(280.0)));
    view.tasks_open = Some(false); assert!(!is_open(&view, Some(900.0)));
}
```

Window tests, opening the scenes with `testkit::window`:

```rust
#[test]
fn the_tasks_scene_shows_the_list_above_the_box() {
    let rt = testkit::window("tasks");
    /* texts hold "Tasks", "2 of 5", "Measure the current banner", "Updating the stylesheet", "Write the change note";
       not "Update the stylesheet" (the current step shows its active wording);
       the list's rectangle ends above the input box's top and has its left edge and width;
       the element keyed for the current step's mark is the spinner; no tool line names TaskCreate, TaskUpdate or ToolSearch */
}
#[test]
fn a_press_on_the_head_folds_and_opens_the_list() {
    /* click mr-tasks-head: "3 of 5: Updating the stylesheet" shows and "Write the change note" does not; click again: back */
}
#[test]
fn the_folded_scene_starts_folded_in_its_short_tile() { /* tasks-folded: the folded words show; the other steps do not */ }
#[test]
fn outside_a_turn_the_current_step_has_no_spinner() { /* emit an interrupted TurnFinished: the list stays, and its current step's mark is a ring */ }
#[test]
fn the_list_goes_when_its_turn_ends_ticked() { /* tick the three open steps, finish the turn: no element keyed mr-tasks-…; tasks_open is none again */ }
#[test]
fn a_hundred_steps_draw_six_and_long_words_stay_on_one_line() {
    /* emit TaskAdded for 95 more steps, one with a 300-character subject made current:
       six step lines, "94 more", and that line's height is ROW */
}
```

In `input.rs`, extend `the_lines_a_tile_allows`: with a folded list (30 points of extra) a 280-point tile allows `lines_for(Some(280.0), 30.0)` lines, and that is at least 1; and assert that `extra_height` for a session with five steps in a 280-point tile is 30 more than for the same session without a list.

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p mindrune tasks`. Expected: does not compile.
- [ ] **Step 3: Implement.** Read `requests.rs` for `unfocusable` and the hand-drawn ring, `items.rs` for the spinner, `input.rs` for the area's parts and their heights.
- [ ] **Step 4: Run the three checks.** Look at `target/snap/final` only after Task 9 adds the scenes there.
- [ ] **Step 5: Commit** by path: `feat(app): the task checklist above the message box`.

---

### Task 8: The choice box

**Files:**
- Create: `crates/app/src/view/choices.rs`
- Modify: `crates/app/src/view/mod.rs`, `crates/app/src/view/input.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: `App::choices`, `App::can_choose`, `App::choose` (Task 6); the place and the height counting of Task 7.
- Produces:

```rust
// view/choices.rs
pub const HEIGHT: f32 = 96.0;           // counted for the box: its padding, one line of question, one row of buttons, the gap below
pub fn boxed(ui: &Ui, kit: &Kit, key: SessionKey) -> Option<El>;
pub fn height(ui: &Ui, key: SessionKey) -> f32;      // HEIGHT while a box shows, else 0
```

Exact behaviour:
- **Place.** Inside `mr-input-area`, under the list and above `mr-input-box`, as wide as the box. Drawn only while `ui.app.choices(key)` is `Some`.
- **Look.** A box keyed `mr-choices-{n}`: padding 12, gap 10, the kit's large radius, `bg_base` fill, a 1-point border in `border_weak`, `GAP_BELOW` under it. The question, keyed `mr-choices-question`, in the kit's `ui` type and `text_base`, wrapping. Under it a row that wraps, gap 8, of buttons keyed `mr-choice-{i}`: the first `ButtonVariant::Contrast`, the others `ButtonVariant::Neutral`, at the regular size, their labels as given.
- **Each button** is not focusable (`requests::unfocusable`), is disabled while `ui.app.can_choose(key)` is false, and has as its help (the kit's `.help`, as the copy buttons have "Copy") the message it sends, cut to 200 characters with an ellipsis.
- **A press** runs `let effects = ui.app.choose(key, i); glue::run(ui, effects);` and nothing else: the draft in the box is not touched.
- **Height.** `input::extra_height` adds `height(ui, key)`. A question that wraps, or buttons that do, make the box taller than counted; in a tile too short for that, the area is cut from its top as built.
- **A scene** `choices`, added to `demo::SCENES`: the session "Resizing the banner" after a finished turn whose reply ends "The banner is resized and the page checked on a phone.", with the offer `Publish the new banner?` and three choices: `Publish` (`/publish`), `Show me first` (`Show me the page before anything is published.`), `Not now` (`Leave it unpublished for now.`).

- [ ] **Step 1: Write the failing tests.** Window tests with `testkit::window("choices")`:

```rust
#[test]
fn the_choices_scene_shows_the_question_and_three_buttons_above_the_box() {
    /* texts hold "Publish the new banner?", "Publish", "Show me first", "Not now";
       the box's rectangle is above the input box, with its left edge and width, and below the conversation's frame;
       no tool line names the choice tool; no element of the box is focusable */
}
#[test]
fn a_click_sends_the_message_and_leaves_what_was_typed() {
    let mut rt = testkit::window("choices");
    /* type "half a thought" into the box */
    testkit::click(&mut rt, &/* the key of mr-choice-0 in the tile */);
    testkit::step(&mut rt);
    assert!(matches!(testkit::sent(&rt).last(), Some(SessionCommand::Send { text, attachments }) if text == "/publish" && attachments.is_empty()));
    /* no element keyed mr-choices-…; the draft still reads "half a thought"; the user's message "/publish" is in the conversation */
}
#[test]
fn typing_a_message_of_ones_own_takes_the_box_away() { /* type "wait", press Return: the box is gone and the Send's text is "wait" */ }
#[test]
fn the_box_is_not_there_while_the_turn_still_runs() {
    /* a session mid-turn that was offered choices: no box; after TurnFinished: the box */
}
#[test]
fn the_buttons_are_greyed_while_an_attachment_is_read() { /* a chip being read: a click sends nothing and the box stays */ }
#[test]
fn a_short_tile_with_everything_keeps_a_line_to_type_in() {
    /* at 280 points with a folded list, the box, a queued message and a chip: lines_for(...) >= 1,
       and the rectangles of the list's line, the box and mr-input-box do not overlap */
}
```

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p mindrune choices`. Expected: does not compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the three checks.**
- [ ] **Step 5: Commit** by path: `feat(app): the choice box above the message box`.

---

### Task 9: Scenes, the real path, notes, and Jerod's look

**Files:**
- Modify: `crates/app/tests/snapshots.rs`, `crates/app/src/demo.rs` (scene tests), `crates/app/tests/live.rs`, `docs/notes-for-later-plans.md`, `docs/protocol-notes.md` (one line, below)

**Interfaces:**
- Consumes: everything above.
- Produces: screenshots of `tasks`, `tasks-folded` and `choices` in both themes under `target/snap/final`; new `live:` lines; the notes.

What to do:
- **Snapshots.** Add the three scenes to `snapshots.rs`'s list (`tasks-folded` at the size its column of two needs). Add a scene test for each in `demo.rs`, as the existing scenes have: the marks and phases the scene must end in.
- **The live test** (run only by Jerod) gains one step in its first run, in the test's own session and folder, after the steps it has. It sends: `Keep a task list with exactly two steps, "one" and "two". Tick both off without doing anything else. Then offer me two choices with the tool for that: "Yes" and "No". Use no other tool and run nothing.` A permission request in this step is denied and stops the step. It prints, and none of these fails the test:
  - `live: task steps added N, changed M, open at the turn's end K`
  - `live: tool lines of the turn: N (named ToolSearch: M)`
  - `live: choices offered: true|false, buttons N, shown after the turn: true|false`
  - then, when choices show, it calls `App::choose(key, 0)` and prints `live: the choice was sent and answered: true|false`
  - in the second run, after the session's stored history was read back: `live: stored history gives task steps N, an offer: true|false` and `live: hidden calls among the stored tool items: N`.
  It never prints a step's words, a question or a label. Add each line to the module's doc with what it settles: a `ToolSearch` count of 0 with choices offered settles that `alwaysLoad` works; `hidden calls … 0` that the stored transcript is read as guessed.
- **`docs/protocol-notes.md`**: under "The task list and a tool of the app's own", add one paragraph "What the live test settles", naming those lines.
- **`docs/notes-for-later-plans.md`**: the top paragraphs name plan 9; add "What plan 9 is for" (Jerod's words, from this plan) and "Jerod's look at plan 9" in the form of plan 8's section: the live test and its new lines; before looking; what to try in the window (a list of numbered things, each one thing to do and what should be seen, covering every decision above that can be seen); every decision with what was built; the rulings made while the plan ran; things to look at in the screenshots; what no agent could see; what is left.

- [ ] **Step 1: Write the scene tests and add the snapshot entries.** Run `cargo test -p mindrune --test snapshots`. Expected: pass, with six new images.
- [ ] **Step 2: Look at the six images** and say in the report what each shows and anything that looks wrong (overlap, cut text, a colour that cannot be read in one theme).
- [ ] **Step 3: Write the live test's step and its lines.** It must compile and must not run: `cargo test -p mindrune --test live` reports it ignored.
- [ ] **Step 4: Write the notes.**
- [ ] **Step 5: Run the three checks.**
- [ ] **Step 6: Commit** by path: `test(app): scenes for the checklist and the choice box, the live test's lines, notes`.

---

## After the last task

A whole-branch review on the most capable model, one fix wave, one re-review. Then the rulings of the run go into `docs/superpowers/plans/2026-11-19-plan-9-rulings.md`, and Jerod is asked whether to merge. Nothing is merged or pushed without their word. Before a push, plan 9 is squashed into one commit as plans 1 to 8 were.
