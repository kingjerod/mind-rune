# Right Panel, Remote Control and Effort Level Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The effort level is shown and changed beside the model in every tile. A panel on the right describes the focused tile's session: its folder, branch, model, effort and mode, how much of the context window is used, what it has cost, and which files it changed. Remote Control is switched on and off there, a tile says "Remote" while it is on, and a refusal says why.

**Architecture:** The seam gains the effort level (a list per model, a command, an event) and finer Remote Control states; the Claude adapter sets the level with `apply_flag_settings` and reads what is in use back with `get_settings`, because the first reply confirms nothing. `mr-core` keeps the effort and the Remote Control state on the session, counts changed files as edits finish, and reads the git branch from the repository's own files on the library thread. The window adds a third picker to the input box and puts the panel in the kit's own trailing pane (`SplitView::inspector`), which already resizes and collapses.

**Tech Stack:** Rust 1.99.1 (edition 2024), `rune` and `rune-kit` at the pinned revision, `serde`, `serde_json`, std threads and channels, `tempfile` for tests. No change to dependencies.

**Spec:** `docs/superpowers/specs/2026-11-10-claude-code-ui-design.md`, sections 5.5 (the effort level, added 2026-11-15), 5.6 and 5.7 but for the preference's place, and the Remote Control mark of 5.2. Read these before starting, in this order: the spec, `docs/notes-for-later-plans.md` ("What plan 5 must know", "Open checks that need Jerod", "The seam: additions the window will need"), `docs/protocol-notes.md`.

**For the controller:** Jerod's rule since 2026-11-13: every subagent (implementer, reviewer, fixer) runs on Opus 5.5 at high effort or better. Nothing less, whatever the size of the task. Two research reports made for this plan are on disk and not in git: `.superpowers/sdd/plan-5-research/protocol-facts.md` (what Claude Code 2.1.323 takes and gives, each fact marked as read in the program, seen in a recording, already in the adapter, or a guess, with the search string that finds it) and `.superpowers/sdd/plan-5-research/window-survey.md` (the code this plan meets, with signatures, and eighteen traps). Hand an implementer the path of the one its task needs. The rule files of plan 4's workspace hold for this plan as they are.

## This is plan 5 of 7

| Plan | Delivers |
| --- | --- |
| 1 to 4 (done) | Foundations and the Claude adapter, one session in a window, many sessions, tiles |
| Stacked tiles (before this plan) | Two tiles in a column |
| **5. Panel, Remote Control, effort (this plan)** | The effort level, the right panel, the Remote Control switch and its mark |
| 6. Attachments | Pasting an image, dropping files, the paperclip, the chips |
| 7. Running in the background | Closing the window keeps sessions working, the tray, Quit rules, stopping idle programs |

On 2026-11-15 Jerod chose this part of the old plan 5 to come first. This plan is written against the tiles as the stacked tiles plan leaves them (`Tiles::keys`, a tile placed by its rectangle). It touches the tiles in one place, a tile's header; where a name here differs from the code, the code wins.

**Not in this plan, on purpose.** Attachments. Running without a window, Quit asking first, stopping idle programs, and the two fixes the notes list under "What plan 5 must do first", which belong with the plan that stops idle programs (plan 7). A settings screen. Showing what a file's changes are from the panel.

## What rests on what was never seen

Claude Code was not run for this plan; no agent runs it. Part of what the plan builds on was read in the program's own text and never seen working. Each such point has a line the live test prints, or a step in Jerod's check, so that it stops being a guess:

| Point | Read where | Seen working | Made sure by |
| --- | --- | --- | --- |
| `apply_flag_settings` with `effortLevel` changes the running session's level | the program | no | the live test |
| `get_settings` gives `applied.effort` | the program | no | the live test |
| Remote Control can be turned on before a session's first turn | the program | no | the live test |
| The states `reconnecting` and `policy_disabled`, and the refusal codes | the program | no (`failed` and the auth refusal were seen on 2026-11-10, not recorded) | nothing: they need a dropped connection or a policy |
| A session with Remote Control on is listed at claude.ai/code and can be driven from there | nowhere | no | Jerod's check, Task 12 |
| A prompt typed on another device reaches this app's side as a record | the program, with `--replay-user-messages` | no | Jerod's check; Task 13 is built only from its recording |

Should the live test show that Remote Control cannot be turned on before a first turn, the fix is one condition in `Session::apply`: a new session that wants it asks when its first turn has ended, not at `Started`. Should it show that `get_settings` or `apply_flag_settings` does not do what the program's text says, the effort picker cannot be trusted and Tasks 2, 4 and 5 are reopened before the plan is merged.

## What this window will not do yet

Say this to Jerod before their look, so the window is judged for what it is.

- A prompt typed on a phone or at claude.ai/code runs, and its reply streams into the tile, but the prompt itself is not shown above the reply until Task 13 is built from a recording.
- The Remote Control switch does not start a session's program. On a session that is resting it is remembered and turned on with the next message.
- Remote Control is not turned on again by itself after the app is closed and opened, but for new sessions when "On for new sessions" is set.
- Closing the window still ends every session's program, so Remote Control ends with it. Running in the background is plan 7.
- The list of changed files is what the session's own edits say, not what git says, and it knows only the part of a long session that is loaded.
- The branch is read when the focus moves and when a turn ends. A branch switched in a terminal shows at the next of these.

## Decisions in this plan that the design does not make, or makes differently

Each is the planner's call. Jerod has not seen them; they are listed in the handover for review.

1. **The panel starts hidden.** A button at the top right of the window shows and hides it, and so does Ctrl+Alt+B (the sidebar's key is Ctrl+Shift+B). Whether it is open and how wide it is are remembered. Reason: on first start the middle is the point of this app, and one click brings the panel for good.
2. **Opening the panel makes every tile narrower.** None is closed, as when the window is made narrower (plan 4's decision 5).
3. **The effort level is a picker of its own** between the model and the mode. It shows the level the next message will use, as Claude Code itself reports it, under the names Low, Medium, High, Extra high and Max. It lists the levels the model in use takes. For a model that takes none it is not shown.
4. **A change of effort holds from the next message**, also when it is made while a turn runs. The picker shows the new level once Claude Code confirms it; if Claude Code goes on with another level (an environment variable or a cap in the user's settings decides before the app does), the conversation says so in a line.
5. **A stored session goes on with the effort it last used**, read from its history, as it goes on with its model (Jerod's decision of 2026-11-14 for the model). The list has no "default" entry: the level in use is always named.
6. **In a tile too narrow for three pickers** the mode is cut first, then the effort, and the model's name last.
7. **The Remote Control switch shows what was asked at once**, and goes back to off when Claude Code refuses, with the reason as a line in the conversation and under the switch. The spec asks for exactly this; the pickers, which wait for a confirmation, work the other way.
8. **The switch never starts a program.** On a resting session it is remembered, the panel says "Turns on when the session starts.", and it is turned on when the next message starts the program. The spec's rule that an old session's program starts only with a message stays whole.
9. **What was asked is kept for the run of the app.** A program that ends and starts again for the same session turns Remote Control on again. It is not kept between runs.
10. **"On for new sessions"** is a second switch in the panel, under the first, off by default. The spec calls it a preference; there is no settings screen to put it in. It holds for sessions made with "New session", not for stored ones.
11. **The mark.** A tile's header says "Remote" while Remote Control is on, and the same word in a muted colour while it connects or reconnects.
12. **"Copy link"** in the panel while Remote Control is on copies the session's claude.ai address. The address is shown nowhere and written to no log.
13. **Cost** is Claude Code's own running total for the session, which it carries over when a stored session is continued. **Context** is worked out as Claude Code's own status line works it out: the input side of the last message against the window of the model in use. Until a session's program has finished a turn, both read "not known yet".
14. **Files changed** are the Edit and Write calls that succeeded, summed per file with the counts their rows in the conversation already show. A notebook edit is not counted. It is not git's view of the folder.
15. **The branch** is read from the repository's files, never by running git. A folder outside a repository shows no branch line; a detached head shows "detached at" and seven characters.
16. **The live test** gains printed lines and no new way to fail. It turns Remote Control on in its own two sessions for a moment, so two test sessions appear at claude.ai/code for a few seconds each.
17. **A prompt from another device** is built only from a recording (Task 13). The window must never show a line as the user's that is not one, and what the program echoes was never seen.

## Global Constraints

- Rust toolchain is exactly `1.99.1`, edition `2024`. In an agent's shell cargo is not on `PATH`: start every command with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Rune is pinned to `rev = "2a44156b21d63b0342a366b26d9222a6edd4622c"`. Its source is at `~/.cargo/git/checkouts/rune-c9b805e9ed8a3730/2a44156`; read it instead of guessing an API. Where this plan and the source differ, the source wins: say so in your report.
- `mr-agent` and `mr-core` depend on neither `mr-claude` nor `rune`. No type from `mr-claude` appears in `mr-core` or in any view. Only `crates/app/src/main.rs`, `crates/app/tests/live.rs` and `crates/probe` may name `ClaudeAgent`.
- The UI thread never blocks. Every `Agent` method and `SessionHandle::send` is called only on a backend thread. A file is read on the library thread, never in a view.
- A session's events are trusted only while they carry the number of that session's current open.
- Every session intent of `App` takes the `SessionKey` it acts on. No view under a tile reads "the focused session". The panel is the one view that does: it draws `App::focused()`.
- Every `App` intent a view calls is followed by `glue::run`.
- A press on a control of the panel, on the panel's button or on the effort picker never takes the keyboard out of the field the user types in, and never moves the tile focus by itself.
- The software never writes inside `~/.claude`, and never changes the user's Claude Code settings: `update_settings` is never sent. Its own settings go through `rune::prefs::Prefs::open("mindrune")`.
- `SAVED_VERSION` stays 1. Fields added to `Saved` have serde defaults and read anything they cannot read as their default, so that a bad value costs only itself (`share_or_one` is the model). `saved.rs` compares its sample with an exact string, which changes with every new field.
- Never assign `App.sessions`, `App.groups` or `App.tiles` wholesale after `App::new`.
- A Remote Control address is never printed, logged, put in a test's name or message, or shown as text. Tests use `https://example.org/session`.
- Every key this app gives an element starts with `mr-`. A key Rune looks up window-wide by its exact text (a picker's anchor) carries the session's number: `mr-effort-7`. The panel exists once, so its keys carry none.
- A view never does work that grows with the length of a conversation: what the panel shows is kept up to date as events arrive.
- No Anthropic logos, colours or fonts.
- User-visible wording is exact where this plan quotes it. Never use a real person's name or e-mail address as test data. Never copy text out of a real stored session or out of `target/rec/` into a test or a fixture: field names and shapes only, values made up.
- All window tests and all screenshots use `FakeAgent`, through the demo window. No agent starts the real `claude`, sets `MINDRUNE_LIVE` or runs a test with `--ignored`. Lines of the protocol that were never recorded are written by hand from `protocol-facts.md`; the test's comment says "never seen live".
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` (no warnings) and `cargo test --workspace` pass before every commit. Say in the report which existing tests you changed and why.
- **Git.** Work on the branch `plan-5-panel-remote-effort`, cut from `main` after stacked tiles is merged. Never push. Never commit `target/` or `.superpowers/`. Commit by explicit path, not `git add -A`. End every commit message with a blank line and `Co-Authored-By: Claude <noreply@anthropic.com>`, in exactly that form.
- Version of `claude` this plan was written against: `2.1.323`.

## Review Focus

The spec implies these conditions and no screen in it shows them. Each has a test in the task named.

1. **Both side panes open in the smallest window.** At 560 points wide the tile row is left with almost nothing. Expected: nothing panics, no tile is closed, the panel's button is still inside the window and still closes the panel. Tests: Task 7.
2. **One refusal that arrives twice.** Claude Code reports a refused Remote Control both as a state and as the failed reply. Expected: one line in the conversation, the switch off once. Tests: Task 6.
3. **The level in use is not the one chosen.** Expected: the picker names the level in use, one line says the choice was not taken, and nothing asks again by itself. Tests: Task 2, Task 4, Task 5.
4. **The session the panel shows goes away or changes under it.** Its tile closes, the focus moves while Remote Control is connecting, the program exits. Expected: the panel shows the session now focused, or "No session open.", and no switch acts on a session that is not the one shown. Tests: Task 7, Task 10.
5. **Folders that are not plain repositories.** No repository, a worktree whose `.git` is a file, a detached head, a `HEAD` that cannot be read, a folder that is gone. Expected: a branch, "detached at", or no line; never an error on screen and never a panic. Tests: Task 8.

## File Structure

```
crates/agent/src/event.rs          Task 1: effort in ModelOption and the events, finer Remote Control states
crates/agent/src/command.rs        Task 1: SetEffort
crates/agent/src/agent.rs          Task 1: effort in StartSpec and StoredHistory
crates/agent/src/fake.rs           Task 1: the fake knows effort
crates/claude/src/args.rs          Task 2: --effort
crates/claude/src/encode.rs        Task 2: apply_flag_settings, get_settings
crates/claude/src/translate.rs     Tasks 2, 3: effort, settings, bridge states, context
crates/claude/src/wire.rs          Tasks 2, 3: what is read from a line
crates/claude/src/agent.rs         Task 2: requests the adapter sends by itself
crates/claude/src/history_tail.rs  Task 2: the effort of a stored session
crates/core/src/session.rs         Tasks 4, 6: effort and Remote Control on a session
crates/core/src/app.rs             Tasks 4, 6, 8: intents, the preference, the branch
crates/core/src/saved.rs           Tasks 6, 7: the preference; the panel's width and state
crates/core/src/git.rs             Task 8 (new): the branch of a folder
crates/core/src/backend.rs         Task 8: the read on the library thread
crates/core/src/changes.rs         Task 9 (new): files changed
crates/core/src/conversation.rs    Task 9: counting as edits finish
crates/app/src/glue.rs             Tasks 5, 7, 8: the picker, the panel's state, the branch read
crates/app/src/view/input.rs       Task 5: the effort picker
crates/app/src/view/shell.rs       Task 7: the panel in the frame, its button
crates/app/src/view/mod.rs         Task 7: the panel's key
crates/app/src/view/panel.rs       Tasks 7, 10, 11 (new): the panel
crates/app/src/view/tiles.rs       Task 10: the mark in a header
crates/app/src/demo.rs             Tasks 5, 7, 10, 11: scenes
crates/probe/src/                  Task 12: extra arguments for the program
crates/app/tests/                  Task 12: snapshots, live
docs/                              Task 12: notes
```

## Facts this plan builds on

From `protocol-facts.md` and `window-survey.md`. Check each against the program's text or the source before relying on it.

- **The model list** in the `initialize` reply gives each model `supportsEffort` and `supportedEffortLevels` (any of `low`, `medium`, `high`, `xhigh`, `max`). It gives no default level.
- **Starting with a level:** `--effort <level>`. An unknown value is ignored with a warning on stderr.
- **Changing it in a running session:** the control request `{"subtype":"apply_flag_settings","settings":{"effortLevel":"high"}}`. The reply is a bare success also for a level it did not take, and a level the model cannot do is lowered when used. It is among the requests handled while a turn runs.
- **Reading it:** the control request `{"subtype":"get_settings"}`; the reply's `applied.effort` is a level or `null` ("what the next message will use").
- **Never** `update_settings`: it writes the user's settings file.
- **A stored session's records** carry `effort` on each `assistant` record. Nothing was found that restores a level on `--resume`, so the app passes it.
- **Context:** Claude Code's own percentage is `input_tokens + cache_creation_input_tokens + cache_read_input_tokens` of the last message with usage, over the window of the model in use. The adapter today adds `output_tokens` and divides by the largest window of any model the session used.
- **Cost:** `result.total_cost_usd` is the session's running total, and a continued session starts from its stored total.
- **Remote Control states** (`system/bridge_state`): `ready`, `connected` (with `recovered: true` after a drop), `reconnecting`, `failed` with `failure_kind` `auth`, `terminal`, `ended_elsewhere` or `network`, and `policy_disabled`. A refused request answers with an error whose `error_code` is `remote_control_auth`, `remote_control_terminal`, `remote_control_ended_elsewhere`, `remote_control_policy_disabled`, or none; the state record usually comes before it. For a sign-in problem the `detail` is the bare text `/login`, which is not a sentence. Turning it off gives only the reply.
- **A request answered on another device** arrives as `control_cancel_request`, which the adapter already gives as `RequestWithdrawn`.
- **Git:** nothing on stdout names the branch. A repository's `.git` is a folder, or in a worktree a file holding `gitdir: PATH`; its `HEAD` holds `ref: refs/heads/NAME` or a commit id.
- **The kit's trailing pane:** `SplitView::inspector(&PaneState, access, content)` gives a pane on the right with its own grip, resized from its left edge, collapsed below half its least width, with keys `inspector-pane` and `inspector-grip`. It has no button of its own; Rune's example puts an `IconButton::new(&shell::INSPECTOR)` in a `Toolbar`'s trailing slot. The sidebar's `PaneState` shows how such a pane is kept and saved.
- **The kit's `Switch`** is a Tab stop and takes the keyboard on a press unless told otherwise.
- **The input box's bottom row** does not wrap. `input::model_room` gives the model picker what the others leave, and two tests named `in_the_smallest_window_*` pin its arithmetic.

## Tasks

### Task 1: The seam knows effort and finer Remote Control states

**Files:**
- Modify: `crates/agent/src/event.rs`, `command.rs`, `agent.rs`, `fake.rs`
- Modify, only to keep them compiling with no change of behaviour: every place that builds a `ModelOption`, a `StartSpec`, a `StoredHistory` or a `RemoteAccess::Failed`, or matches every `SessionEvent`, `SessionCommand` or `CommandKind` (`crates/claude/src/`, `crates/core/src/`, `crates/app/src/`, `crates/probe/src/`)

**Interfaces:**
- Produces:
  ```rust
  // event.rs
  pub struct ModelOption {
      /* as it is */
      /// The effort levels this model takes, lowest first; empty when it takes none.
      pub effort_levels: Vec<String>,
  }
  pub enum SessionEvent {
      /* as it is */
      /// The effort level the next message will use; none when the model takes none.
      EffortChanged(Option<String>),
  }
  pub enum CommandKind { /* as it is */ SetEffort }
  pub enum RemoteAccess {
      Off,
      Starting,
      On { url: Option<String> },
      /// The connection dropped and is being made again.
      Reconnecting,
      Failed { reason: String, why: RemoteFailure },
  }
  /// Why Remote Control was refused or ended.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub enum RemoteFailure {
      /// The sign-in is not a full claude.ai login.
      Login,
      /// The organisation's policy turns it off.
      Policy,
      /// It was ended for this session from another place.
      Elsewhere,
      Other,
  }
  // command.rs
  pub enum SessionCommand { /* as it is */ SetEffort(String) }
  // agent.rs
  pub struct StartSpec { /* as it is */ pub effort: Option<String> }
  pub struct StoredHistory {
      /* as it is */
      /// The effort level of its last reply, when the store says.
      pub effort: Option<String>,
  }
  ```

Rules:

- **What an agent promises.** After `SetEffort`, it says what is in use with `EffortChanged`; when that is not the level asked for, a `CommandFailed { what: SetEffort, .. }` comes with it. `EffortChanged` also follows `Started`, and comes again whenever the agent learns that the level changed (a change of model can change it). Write this into the doc comments of the command and the event.
- `RemoteAccess::Failed`'s `needs_login: bool` becomes `why`: true is `RemoteFailure::Login`, false is `Other`. Nothing else about Remote Control changes in this task.
- **The fake.** Its model "default" takes `low`, `medium` and `high`; its model "fast" takes none. After `Started` it gives `EffortChanged`: the level of `spec.effort` when there is one, else `medium`, and none for a model that takes no level. `SetEffort` is recorded and not answered, as `SetModel` is: a test gives the answer with `emit`.
- Everywhere else the new variants and fields are carried and ignored: `Session::apply` and `Conversation::apply` do nothing with `EffortChanged` yet, `start_spec` passes no effort, the adapter builds its lists with no levels.

- [ ] **Step 1: Write the failing tests** in `crates/agent/src/fake.rs`:
  - `the_fake_says_its_effort_after_it_starts`: `Started`, then `EffortChanged(Some("medium"))`; with `spec.effort` of `high`, that level; with `spec.model` of `fast`, `EffortChanged(None)`.
  - `the_fake_records_a_change_of_effort_and_answers_nothing`.
  - `the_fakes_default_model_takes_three_levels_and_fast_none`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement**, and make everything compile.
- [ ] **Step 4: Run** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`. Tests that counted a session's events or compared a whole `ModelOption` change by the new event and field only: list them in the report.
- [ ] **Step 5: Commit** `feat(agent): the seam knows the effort level and why Remote Control failed`.

### Task 2: The adapter sets and reads the effort level

**Files:**
- Modify: `crates/claude/src/args.rs`, `encode.rs`, `translate.rs`, `wire.rs`, `agent.rs`, `history_tail.rs`, `lib.rs`; `crates/claude/tests/agent.rs` and `tests/stub/fake_claude.sh`

**Interfaces:**
- Consumes: Task 1; `ClaudeSession::request`, `Translator::track`, `Translator::control_response`, `model_options`.
- Produces:
  ```rust
  // encode.rs
  /// `{"subtype":"apply_flag_settings","settings":{"effortLevel":level}}`
  pub fn set_effort(level: &str) -> Value;
  /// `{"subtype":"get_settings"}`
  pub fn get_settings() -> Value;
  // translate.rs
  pub enum Tracked {
      /* as it is */
      SetEffort(String),
      /// `asked` is the level a `SetEffort` asked for, when this read follows one.
      Settings { asked: Option<String> },
  }
  /// The requests the translator wants sent next, in order. Emptied by the call.
  pub fn take_requests(&mut self) -> Vec<(Tracked, Value)>;
  ```

Rules:

- **Start.** `build_args` adds `--effort LEVEL` when the spec has one.
- **The list.** `model_options` fills `effort_levels` from `supportedEffortLevels` when `supportsEffort` is true, keeping the strings it finds in their order, and leaves it empty otherwise.
- **The translator stays pure.** It never writes. When it wants a request sent it puts it in a list; the code that feeds it lines (`crates/claude/src/agent.rs`) takes the list after every line and sends each through the same path as `ClaudeSession::request`. A request that cannot be written is dropped silently: the session is ending.
- **When the level is read** (`get_settings`, `asked: None`): after the `initialize` reply, right after `Started` is given; after a `set_model` request succeeds; after every `result`.
- **`SetEffort(level)`** sends `set_effort(level)`. Its success queues `get_settings` with `asked: Some(level)`. Its failure gives `CommandFailed { what: SetEffort, reason }` with the error's text.
- **The reply to `get_settings`.** The level is `applied.effort` when it is a string, else none. `EffortChanged` is given when it is the first reading of this session or differs from the last one given. When `asked` is some level and the level read is another, `CommandFailed { what: SetEffort, reason: "Claude Code goes on with LEVEL" }` is given first, with the level read in place of LEVEL, or "Claude Code goes on with no effort level". A failed `get_settings` gives nothing when nothing was asked, and `CommandFailed { what: SetEffort, reason }` when a level was.
- **What is in that reply.** It holds the user's settings. Only `applied.effort` is read; the line is never printed, logged or kept.
- **History.** `StoredHistory.effort` is the `effort` string of the last `assistant` record on the chain the history is read along, as `model` is its model.
- **The stub** (`tests/stub/fake_claude.sh`) answers `apply_flag_settings` with a success and `get_settings` with an `applied.effort` that is the level last set, `medium` before any.

- [ ] **Step 1: Write the failing tests.**
  In `crates/claude/src/args.rs`: `an_effort_is_passed_at_start`, `no_effort_no_flag`.
  In `crates/claude/src/translate.rs`:
  - `the_model_list_says_which_levels_a_model_takes`: from the recorded `initialize` reply; and a made-up entry with `supportsEffort: false` and a list has none.
  - `the_level_is_read_after_the_session_starts`: after the `initialize` reply, `take_requests` holds one `get_settings`; its reply with `applied.effort` `"high"` gives `EffortChanged(Some("high"))`; a reply with `null` gives `EffortChanged(None)`.
  - `a_level_read_again_and_unchanged_says_nothing`.
  - `the_level_is_read_after_every_turn_and_after_a_change_of_model`.
  - `a_change_of_effort_is_confirmed_by_reading_it`: the success of `apply_flag_settings` gives no event and queues the read; the read gives `EffortChanged`.
  - `a_level_that_was_not_taken_says_so`: asked `max`, read `high`: `CommandFailed { SetEffort, "Claude Code goes on with high" }`, then nothing more when the level was `high` before, or `EffortChanged(Some("high"))` when it was not.
  - `a_refused_change_of_effort_fails`, `a_failed_read_after_a_change_fails_the_change`, `a_failed_read_nobody_asked_for_says_nothing`.
  In `crates/claude/src/history_tail.rs`: `a_stored_session_gives_the_effort_of_its_last_reply`, `a_stored_session_without_effort_gives_none`.
  In `crates/claude/tests/agent.rs`: `a_change_of_effort_goes_out_and_comes_back`, against the stub: `SetEffort("low")` ends in `EffortChanged(Some("low"))`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(claude): the effort level is set, read back and found in a stored session`.

### Task 3: The adapter tells Remote Control's states apart and counts context as Claude Code does

**Files:**
- Modify: `crates/claude/src/translate.rs`, `wire.rs`

**Interfaces:**
- Consumes: Task 1's `RemoteAccess::Reconnecting` and `RemoteFailure`.
- Produces: no new function.

Rules:

- **States.** `reconnecting` gives `RemoteAccess::Reconnecting`. `policy_disabled` gives `Failed { reason: its detail or "Remote Control is turned off by policy", why: Policy }`. `failed` gives `why` by its `failure_kind`: `auth` is `Login`, `ended_elsewhere` is `Elsewhere`, anything else `Other`. `connected` gives `On` whether or not it says `recovered`. A state the adapter does not know still gives nothing.
- **A refused request** gives `why` by its `error_code`: `remote_control_auth` is `Login`, `remote_control_policy_disabled` is `Policy`, `remote_control_ended_elsewhere` is `Elsewhere`, anything else or none `Other`.
- The reason is passed on as given, `/login` included: the core decides what is shown (Task 6).
- **Context.** `TurnSummary.context_used` is the last message's `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`, without `output_tokens`, over the `contextWindow` of that message's model in `modelUsage`, and over the largest window there when the model is not in it. Above 1 it is 1.

- [ ] **Step 1: Write the failing tests** in `crates/claude/src/translate.rs` and `wire.rs`. The lines for `reconnecting`, `policy_disabled` and the kinds of failure are written by hand from `protocol-facts.md`, section D2: never seen live.
  - `a_dropped_connection_reconnects_and_comes_back`: `reconnecting` gives `Reconnecting`; `connected` with `recovered: true` gives `On` with the address remembered from before.
  - `a_policy_that_forbids_remote_control_is_a_failure_of_its_own`.
  - `each_kind_of_failure_says_why`, for the state and for the refused request.
  - `an_unknown_state_still_says_nothing`.
  - `context_counts_the_input_side_only`: from a recorded turn, without the output tokens.
  - `context_is_measured_against_the_window_of_the_model_in_use`: two models with different windows in `modelUsage`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(claude): Remote Control's states and failures, and context as Claude Code counts it`.

### Task 4: A session has an effort level

**Files:**
- Modify: `crates/core/src/session.rs`, `crates/core/src/app.rs`

**Interfaces:**
- Consumes: Tasks 1 and 2; `Session::choose_model`, `named_by`, `start_spec`, the taking of `StoredHistory.model`.
- Produces:
  ```rust
  // session.rs, in Session
  /// The effort level in use: what the program last said, or while no program runs what the
  /// user chose or the store gave. None while it is not known or the model takes none.
  pub effort: Option<String>,
  /// The levels the model in use takes; empty when it takes none or is not known.
  pub fn effort_levels(&self) -> &[String];
  /// Asks for `level`. Nothing while the session opens or has ended, or when it is the level
  /// in use.
  pub fn choose_effort(&mut self, level: &str) -> Option<SessionCommand>;
  /// "Low", "Medium", "High", "Extra high", "Max"; any other level as it is.
  pub fn effort_label(level: &str) -> String;
  // app.rs
  pub fn choose_effort(&mut self, key: SessionKey, level: &str) -> Vec<Effect>;
  ```

Rules:

- `EffortChanged(level)` sets `effort`. `CommandFailed { what: SetEffort, reason }` adds the notice "Could not change the effort level: {reason}", as the other failed commands add theirs.
- **Choosing.** While a program runs: `SetEffort(level)`, and `effort` stays as it is until the program says. While the session rests: the level is kept and shown at once, and nothing is sent. Choosing a level the model in use does not list does nothing.
- **Starting.** `start_spec().effort` is the level the user chose while it rested; else, for a session from the store, the level its history gave; else none. A level is not passed when the model it starts with is known to take none, or known not to take that one. When that model's levels are not known the level is passed: the program ignores a level it does not know and lowers one the model cannot do, and says what is in use afterwards.
- **History.** A resting session takes `StoredHistory.effort` as its effort when its history is read, unless the user chose one meanwhile, as it takes the model.
- **A change of model while resting** to a model whose list does not hold the level kept forgets the level.
- `Exited` leaves `effort` as it is: a session that rests again shows the level it had.
- `App::choose_effort` focuses nothing and sends as `choose_model` does.

- [ ] **Step 1: Write the failing tests.**
  In `crates/core/src/session.rs`:
  - `the_effort_in_use_is_what_the_program_says`.
  - `choosing_an_effort_asks_for_it_and_shows_it_once_confirmed`.
  - `an_effort_that_was_not_taken_is_said_in_a_line_and_the_old_one_stays`.
  - `a_resting_session_keeps_the_effort_chosen_and_starts_with_it`.
  - `a_stored_session_goes_on_with_its_effort`, `the_users_choice_beats_the_stored_one`.
  - `a_model_without_levels_has_no_effort_to_choose`.
  - `a_level_the_new_model_does_not_take_is_forgotten`.
  - `nothing_is_asked_while_opening_or_after_the_end`, `choosing_the_level_in_use_asks_nothing`.
  - `the_levels_have_names`: `xhigh` is "Extra high"; `turbo` stays `turbo`.
  In `crates/core/src/app.rs`: `choosing_an_effort_sends_it_to_that_session_only`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): a session has an effort level`.

### Task 5: The effort picker

**Files:**
- Modify: `crates/app/src/view/input.rs`, `crates/app/src/glue.rs`, `crates/app/src/demo.rs`, `crates/app/tests/snapshots.rs`

**Interfaces:**
- Consumes: Task 4; `model_picker`, `mode_picker`, `picker_key`, `picker_enabled`, `toggle`, `close`, `chosen`, `model_room`, `glue::shut_disabled_picker`.
- Produces:
  ```rust
  // glue.rs
  pub enum Picker { Model, Effort, Mode }
  // view/input.rs
  /// The least the effort picker shrinks to, in points.
  const EFFORT_MIN: f32 = 48.0;
  /// The effort picker, or none for a model that takes no level.
  fn effort_picker(key: SessionKey, session: Option<&Session>, view: &SessionView, kit: &Kit,
                   layer: &mut Vec<El>) -> Option<El>;
  /// The room the model picker has in a column `width` wide, with or without an effort picker.
  fn model_room(width: f32, effort: bool) -> f32;
  ```
  Keys: `mr-effort-{n}` and what the kit derives from it. Demo scene: `effort`.

Rules:

- A kit `Select`, built as the model picker is, between the model picker and the mode picker: label "Effort", the current entry `Session::effort_label` of the level in use, or the word "Effort" while none is known; its entries the labels of `effort_levels()` in their order; placed above its trigger; enabled as the other pickers are (`picker_enabled`); a choice calls `App::choose_effort` and gives the keyboard back to the box, as `chosen` does.
- Not shown when `effort_levels()` is empty. A list open when its picker goes (the model changed to one without levels) is shut, as `shut_disabled_picker` shuts the others.
- **Room.** With an effort picker the row has a fourth gap, and `model_room` leaves `EFFORT_MIN` as well as `MODE_MIN`. The mode shrinks first, the effort picker next, each cut with an ellipsis inside its trigger, and the model's name is cut last. Nothing overlaps and nothing leaves the box at any width.
- Opening it shuts every other picker in every tile, as the others do; while it is open the window's keys wait, as for the others.
- **The scene `effort`**: one tile with the effort picker's list open on Low, Medium and High, Medium in use.

- [ ] **Step 1: Write the failing tests** in `crates/app/src/view/input.rs`:
  - `the_effort_picker_shows_the_level_and_changes_it`: "Medium" in the trigger; choosing "High" sends `SetEffort("high")` to that session, and the trigger says "High" only after `EffortChanged`.
  - `the_effort_picker_lists_the_models_levels_by_name`.
  - `a_model_without_levels_shows_no_effort_picker`, and a list open at that moment is shut.
  - `an_effort_not_known_yet_reads_effort`.
  - `a_resting_sessions_effort_is_chosen_without_a_program`: nothing is sent, the trigger changes at once.
  - `an_effort_that_was_not_taken_leaves_the_trigger_and_adds_a_line`.
  - `choosing_an_effort_keeps_the_keyboard_in_the_box_and_the_tile_focused`.
  - `two_tiles_each_choose_their_own_effort`.
  - `in_a_tile_of_420_points_the_model_and_the_effort_are_whole`: "Default · Fake 1" and "Medium" are not cut, and the mode is at least its least width.
  - The two tests named `in_the_smallest_window_*`, for three pickers: at 560 by 400 the three triggers and the button lie inside the box and do not overlap, the mode and the effort are at their least widths, and the model's label is the one cut. Say in the report what each asserted before.
  - `escape_in_the_effort_list_closes_it_and_the_turn_runs_on`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: The scene.** Add `effort` to `demo.rs` and to `SCENES`. Make its screenshot in both themes under `target/snap/p5/` and look at both, and at the `tiles` scene again: three pickers in each of three tiles at 1,800 by 900.
- [ ] **Step 5: Run** the three checks.
- [ ] **Step 6: Commit** `feat(app): the effort level is shown and chosen beside the model`.

### Task 6: A session knows its Remote Control

**Files:**
- Modify: `crates/core/src/session.rs`, `crates/core/src/app.rs`, `crates/core/src/saved.rs`

**Interfaces:**
- Consumes: Tasks 1 and 3; `Session::apply`, the commands `Started` gives back, `App::new_session`, `App::send`.
- Produces:
  ```rust
  // session.rs
  /// What the program last said of Remote Control.
  #[derive(Clone, Debug, PartialEq)]
  pub enum Remote { Off, Starting, On { url: Option<String> }, Reconnecting }
  // in Session
  pub remote: Remote,
  /// The user asked for it; true from the press on, until a refusal or a turning off.
  pub remote_wanted: bool,
  /// Why it last failed, in words for the user, until the next change.
  pub remote_said: Option<String>,
  /// Turns Remote Control on or off as far as the session's state allows.
  pub fn set_remote(&mut self, on: bool) -> Option<SessionCommand>;
  // app.rs
  /// New sessions start with Remote Control wanted.
  pub remote_for_new: bool,
  pub fn set_remote(&mut self, key: SessionKey, on: bool) -> Vec<Effect>;
  /// Sets the preference and asks to save.
  pub fn set_remote_for_new(&mut self, on: bool) -> Vec<Effect>;
  // saved.rs, in Saved
  pub remote_for_new: bool,
  ```

Rules:

- **Asking.** `set_remote(on)` sets `remote_wanted` to `on` and clears `remote_said`. While a program runs (idle, working, needing the user) it gives `SetRemoteAccess { on }`, unless that is what the program last said already (on for `On`, `Starting` and `Reconnecting`; off for `Off`). While the session opens or rests it gives nothing. After the end it does nothing at all.
- **Starting.** `Started` gives `SetRemoteAccess { on: true }` with the other commands it gives back when `remote_wanted` is set.
- **What the program says.** `Starting`, `On` and `Reconnecting` set `remote`. `Off` sets it and leaves `remote_wanted` (the press already set it). `Failed { reason, why }`: `remote` is `Off`, `remote_wanted` false, and the words below go into `remote_said` and into the conversation as a notice. A `Failed` that arrives while `remote` is `Off` and nothing is wanted is the same refusal told twice: it changes nothing and adds no line.
- **The words**, by `why`:
  - `Login`: "Remote Control needs a full claude.ai sign-in. Run `claude auth login` in a terminal, then turn it on again."
  - `Policy`: "Remote Control is turned off by your organisation's policy."
  - `Elsewhere`: "Remote Control was ended for this session from another place. Turn it on again to reconnect."
  - `Other`: "Remote Control stopped: {reason}", and "Remote Control stopped." when the reason is empty or is `/login`.
- `CommandFailed { what: SetRemoteAccess }` (a refused turning off) keeps its notice, and sets `remote_wanted` back to what `remote` says.
- **`Exited`.** `remote` is `Off`. `remote_wanted` stays, so the session's next program turns it on again.
- **The preference.** `App::new_session` sets `remote_wanted` on the session it makes when `remote_for_new` is set. It is saved in `Saved.remote_for_new` and read back by `App::new`; anything in the file that is not `true` reads as false. A stored session and a copy made by "Continue as a copy" do not get it.
- Nothing here bumps the sidebar's version: no sidebar row shows Remote Control.

- [ ] **Step 1: Write the failing tests.**
  In `crates/core/src/session.rs`:
  - `turning_remote_control_on_asks_the_program_and_follows_what_it_says`: `Starting`, then `On` with an address.
  - `a_refusal_turns_it_off_and_says_why`: one case for each `why`, with the exact words; `/login` is never shown.
  - `a_refusal_told_twice_adds_one_line`.
  - `a_resting_session_remembers_the_wish_and_asks_when_it_starts`.
  - `a_session_that_is_opening_asks_when_it_has_started`.
  - `a_program_that_ends_leaves_the_wish_and_the_next_one_turns_it_on_again`.
  - `turning_it_off_asks_once_and_forgets_the_wish`; `turning_it_off_while_it_connects_asks_too`.
  - `a_dropped_connection_shows_reconnecting_then_on`.
  - `a_refused_turning_off_keeps_it_on`.
  - `an_ended_session_takes_no_switch`.
  In `crates/core/src/app.rs`:
  - `a_new_session_wants_remote_control_when_the_preference_says_so`, and asks at its `Started`.
  - `a_stored_session_and_a_copy_do_not_get_the_preference`.
  - `the_preference_is_saved_and_read_back`.
  In `crates/core/src/saved.rs`: `a_preference_that_cannot_be_read_is_off_and_the_rest_is_read`; the sample and its string with the new field.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): a session knows its Remote Control, and new sessions can start with it`.

### Task 7: The panel's frame

**Files:**
- Create: `crates/app/src/view/panel.rs`
- Modify: `crates/app/src/view/shell.rs`, `crates/app/src/view/mod.rs`, `crates/app/src/glue.rs`, `crates/core/src/saved.rs`, `crates/app/src/demo.rs`, `crates/app/tests/snapshots.rs`

**Interfaces:**
- Consumes: `SplitView::inspector`, `PaneState`, `Toolbar`, `IconButton`, `shell::INSPECTOR`; how `Ui.sidebar` is made, toggled and saved; `App::focused_entry`, `App::save_now`.
- Produces:
  ```rust
  // glue.rs, in Ui
  /// The right panel: 300 points wide at first, 240 to 480, hidden at first.
  pub panel: PaneState,
  // core/src/saved.rs, in Saved
  /// The right panel's width in points; the window's own when none or not a number.
  pub panel_width: Option<f32>,
  /// Anything that is not `true` reads as false.
  pub panel_open: bool,
  // view/shell.rs
  /// Ctrl+Alt+B (Cmd+Alt+B on macOS): shows or hides the right panel.
  pub fn is_panel_toggle(ev: &KeyEvent) -> bool;
  // view/panel.rs
  /// What the right panel shows: the focused tile's session, or that there is none.
  pub fn panel(ui: &Ui, kit: &'static Kit) -> El;
  ```
  Keys: `mr-panel` (the content's root), `mr-panel-toggle` (the button), `mr-panel-title`, `mr-panel-empty`. Demo scene: `panel`.

Rules:

- The panel is the kit's trailing pane in the same `SplitView` as the sidebar. Its grip, its collapse and its double press are the kit's and are not rebuilt.
- **The button** is an icon button in the trailing slot of a `Toolbar` in the band above the tiles, labelled "Show panel" while the panel is hidden and "Hide panel" while it shows, pressed-looking while it shows. The band keeps its height, so the row of tiles starts where it started. The key does the same as the button, on `Screen::Main`, also while a field has the keyboard.
- **Saving.** Open or hidden and the width go into `Saved` where the sidebar's go (`glue::run`'s arm for `SaveState`). The button and the key ask for a save at once (`App::save_now`); a drag of the grip is saved with the next save, as the sidebar's is.
- **What it shows in this task.** With a focused tile: its session's title ("New session" while it has none) as the panel's heading. With none: "No session open." Tasks 10 and 11 add the sections under the heading.
- The panel follows `App::focused()` at every build: when the focus moves or the focused tile closes, it shows the session focused now.
- A press anywhere in the panel leaves the keyboard where it is and the tile focus where it is.
- The content scrolls as one column when it is higher than the window.
- **The scene `panel`**: the `tiles` scene at 1,800 by 900 with the panel open.

- [ ] **Step 1: Write the failing tests.**
  In `crates/app/src/view/panel.rs`:
  - `the_panel_is_hidden_at_first_and_the_button_shows_it`: after the press the pane is presented right of the tiles, and the row of tiles is narrower by its width.
  - `ctrl_alt_b_shows_and_hides_it`, also while the keyboard is in an input box, which keeps it.
  - `the_button_says_what_it_will_do`.
  - `the_panel_names_the_focused_session_and_follows_the_focus`: F6 changes the heading.
  - `with_no_tile_it_says_no_session_open`; `closing_the_focused_tile_shows_the_next_one`.
  - `opening_the_panel_closes_no_tile`: three tiles at 1,800 by 900 stay three, each narrower.
  - `a_press_in_the_panel_keeps_the_keyboard_and_the_focused_tile`.
  - `both_panes_open_in_the_smallest_window`: at 560 by 400 nothing panics, no tile closed, the button lies inside the window and hides the panel.
  - `the_tiles_start_where_they_started`: with the panel hidden, the row's rectangle is what it was before this task at 1,800 by 900 (`[260, 48, 1532, 844]`).
  In `crates/app/src/glue.rs`: `the_panel_is_saved_and_comes_back`: open at 360 points, read back so; `showing_the_panel_saves_at_once`.
  In `crates/core/src/saved.rs`: `a_panel_width_that_is_not_a_number_reads_as_none_and_the_rest_is_read`; the sample with the new fields.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: The scene.** Add `panel`. Make its screenshot in both themes under `target/snap/p5/` and look at both.
- [ ] **Step 5: Run** the three checks.
- [ ] **Step 6: Commit** `feat(app): a panel on the right for the focused session`.

### Task 8: The branch of a session's folder

**Files:**
- Create: `crates/core/src/git.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/src/backend.rs`, `crates/core/src/app.rs`, `crates/app/src/glue.rs`

**Interfaces:**
- Consumes: the library thread (`LibraryJob`, `ToBackend`, `FromBackend`), `App::receive`, `glue::run`.
- Produces:
  ```rust
  // core/src/git.rs
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub enum Branch {
      Named(String),
      /// The first seven characters of the commit a detached head is at.
      Detached(String),
  }
  /// The branch of the repository `folder` is in, read from its files. None outside a
  /// repository and for anything that cannot be read.
  pub fn branch(folder: &Path) -> Option<Branch>;
  // core/src/app.rs
  pub enum Effect { /* as it is */ ReadBranch { folder: PathBuf } }
  /// The branch last read for `key`'s folder.
  pub fn branch_of(&self, key: SessionKey) -> Option<&Branch>;
  // core/src/backend.rs
  pub enum ToBackend { /* as it is */ ReadBranch { folder: PathBuf } }
  pub enum FromBackend { /* as it is */ Branch { folder: PathBuf, branch: Option<Branch> } }
  ```

Rules:

- **Reading.** From `folder` upwards, the first `.git` found: a folder is the repository's; a file whose first line is `gitdir: PATH` points at it, a relative path being relative to the file's folder. In it, `HEAD`: `ref: refs/heads/NAME` gives `Named(NAME)`, the name kept whole with any `/` in it; a line of 40 or 64 hex characters gives `Detached` with its first seven; anything else none. No process is started. At most 64 folders are climbed and at most 4,096 bytes of a file are read.
- **Where.** On the library thread, as a job of its own, answered into the window's inbox.
- **When the app asks.** When the focus comes to a session; when a turn ends in the focused session; when a session is made in the focused tile. One answer is kept for each folder, so two sessions of one folder share it, and an answer of none takes away what was kept.
- A folder that is gone gives none.

- [ ] **Step 1: Write the failing tests.**
  In `crates/core/src/git.rs`, each in a temporary folder made by the test:
  - `a_repository_gives_its_branch`, also from a folder three levels down, also a name with a `/`.
  - `a_worktree_gives_its_own_branch`: `.git` is a file, with an absolute and with a relative `gitdir`.
  - `a_detached_head_gives_seven_characters`.
  - `outside_a_repository_there_is_none`; `a_head_that_cannot_be_read_gives_none`: empty, garbage, a folder named `HEAD`, a `gitdir` that points nowhere.
  - `a_folder_that_is_gone_gives_none`.
  In `crates/core/src/app.rs`:
  - `the_branch_is_asked_for_when_the_focus_comes_and_when_a_turn_ends`.
  - `two_sessions_of_one_folder_share_the_answer`.
  - `an_answer_of_none_takes_the_branch_away`.
  In `crates/core/src/backend.rs`: `the_library_reads_a_branch`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): the branch of a session's folder`.

### Task 9: The files a session changed

**Files:**
- Create: `crates/core/src/changes.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/src/conversation.rs`

**Interfaces:**
- Consumes: `ToolDetail::FileEdit`, `ToolDetail::FileWrite`, `ToolOutcome`, `tools::line_counts`, how the conversation takes a finished tool call and a history put before it.
- Produces:
  ```rust
  // core/src/changes.rs
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct FileChange { pub path: String, pub added: usize, pub removed: usize }
  /// The files a conversation's edits changed, in the order each was first changed.
  #[derive(Clone, Debug, Default, PartialEq, Eq)]
  pub struct Changes { /* private */ }
  impl Changes {
      pub fn files(&self) -> &[FileChange];
      /// Lines added and removed over all files.
      pub fn totals(&self) -> (usize, usize);
  }
  // core/src/conversation.rs
  pub fn changes(&self) -> &Changes;
  ```

Rules:

- A call counts when its outcome is `ToolOutcome::Ok` and its detail is `FileEdit` (added and removed as `line_counts` of its two texts gives them) or `FileWrite` (its content's lines as added, none removed): the numbers its own row in the conversation shows. A call that failed, was denied or has no outcome yet does not count.
- Counts of the same path add up. The path is kept as the program gave it.
- It is kept up to date where the conversation changes: when a call finishes, and once when a stored history is put before the live items, whose edits then come first in the order. No view and no accessor walks the conversation for it.
- A history whose earlier part was left out counts only what was read.

- [ ] **Step 1: Write the failing tests** in `crates/core/src/changes.rs` and `conversation.rs`:
  - `an_edit_that_succeeded_counts_its_lines`, `a_written_file_counts_its_lines_as_added`.
  - `two_edits_of_one_file_add_up`, `files_come_in_the_order_they_were_first_changed`.
  - `a_failed_a_denied_and_an_unfinished_edit_do_not_count`.
  - `other_tools_change_nothing`.
  - `a_stored_sessions_edits_are_counted_once_and_come_first`: history put before live items that hold an edit of their own; reading it twice does not double anything.
  - `the_totals_add_every_file`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): the files a session changed, counted as its edits finish`.

### Task 10: The panel shows the environment and switches Remote Control

**Files:**
- Modify: `crates/app/src/view/panel.rs`, `crates/app/src/view/tiles.rs`, `crates/app/src/demo.rs`, `crates/app/tests/snapshots.rs`

**Interfaces:**
- Consumes: Tasks 4, 6, 7 and 8; `Session::model_label`, `effort_label`, `mode_label`, `App::branch_of`, `App::set_remote`, `App::set_remote_for_new`, `App.capabilities`, the kit's `Switch`, how `items.rs` copies text, `tiles::header`.
- Produces: keys `mr-panel-folder`, `mr-panel-branch`, `mr-panel-model`, `mr-panel-effort`, `mr-panel-mode`, `mr-panel-remote` (the switch), `mr-panel-remote-status`, `mr-panel-remote-copy`, `mr-panel-remote-new`, and `mr-tile-remote` in a tile's header. Demo scenes: `panel-remote`, `panel-refused`.

Rules:

- **"Environment"**, a heading and rows of a muted label with its value:
  - "Folder": the session's folder, with the user's home folder written `~`, cut in the middle when it is too long, the whole path as its tooltip.
  - "Branch": the name, or "detached at" and the seven characters. No row when there is none.
  - "Model": `model_label()`. "Effort": the level's name; no row for a model that takes none or while none is known. "Mode": `mode_label`.
- **"Remote Control"**, a switch with that label. It shows `remote_wanted`. A press calls `App::set_remote` for the session shown, with the opposite. It is disabled, and says "This session has ended." or "Remote Control is not available.", when the session has ended or `capabilities.remote_access` is false.
- **The line under it** (`mr-panel-remote-status`), the first of these that holds:
  - `remote_said`, when there is one;
  - wanted, and the session rests or opens: "Turns on when the session starts.";
  - `Starting`: "Connecting…"; `Reconnecting`: "Reconnecting…";
  - `On`: "On. Reachable from claude.ai/code and the Claude app.";
  - else: "Off."
- **"Copy link"**, a small button beside the line while `remote` is `On` with an address: it copies the address and nothing else happens. The address is never drawn.
- **"On for new sessions"**, a second switch under the line, showing `App.remote_for_new`; a press calls `App::set_remote_for_new`. It is there also when no session is open.
- Both switches can be reached with Tab. A press with the pointer leaves the keyboard where it was.
- **The mark.** A tile's header shows "Remote" between the word for its state and the close button while its session's `remote` is not `Off`: in the accent's text colour while it is `On`, muted otherwise. It takes its room from the title. It has no press of its own.
- **Scenes** at 1,800 by 900 with the panel open: `panel-remote` (the focused tile's Remote Control on, the mark in its header, "Copy link" shown) and `panel-refused` (the switch off again, the sign-in line under it and in the conversation).

- [ ] **Step 1: Write the failing tests.**
  In `crates/app/src/view/panel.rs`:
  - `the_environment_names_folder_branch_model_effort_and_mode`, for the focused session; `a_folder_under_home_is_written_with_a_tilde`.
  - `no_branch_no_row`, `a_detached_head_reads_detached_at`.
  - `a_model_without_levels_has_no_effort_row`.
  - `the_rows_follow_a_change`: a model, an effort and a mode changed in the tile show in the panel.
  - `the_switch_turns_remote_control_on_for_the_session_shown`: `SetRemoteAccess { on: true }` goes to that session and to no other; the line reads "Connecting…", then "On. Reachable from claude.ai/code and the Claude app."
  - `a_refusal_puts_the_switch_back_and_says_why_under_it`.
  - `a_resting_session_says_it_turns_on_when_it_starts`, and nothing is sent.
  - `the_switch_follows_the_focus`: on in one tile; after F6 it shows the other session's state, and a press acts on that one.
  - `copy_link_copies_the_address_and_shows_it_nowhere`: no element of the window holds the address as text.
  - `an_ended_session_has_a_disabled_switch`.
  - `on_for_new_sessions_is_kept_and_a_new_session_starts_with_it`.
  - `a_press_on_a_switch_keeps_the_keyboard_in_the_box`.
  In `crates/app/src/view/tiles.rs`:
  - `a_tile_says_remote_while_it_is_on`, muted while it connects, gone when it is off or was refused.
  - `the_mark_takes_its_room_from_the_title`: in a tile of 420 points the close button and the state word are whole.
  - `a_press_on_the_mark_is_a_press_on_the_header`: it focuses the tile and can start a drag.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: The scenes.** Add both. Make their screenshots in both themes under `target/snap/p5/` and look at all four.
- [ ] **Step 5: Run** the three checks.
- [ ] **Step 6: Commit** `feat(app): the panel shows the environment and switches Remote Control`.

### Task 11: The panel shows context, cost and files changed

**Files:**
- Modify: `crates/app/src/view/panel.rs`, `crates/app/src/view/shell.rs` (only to share the bar), `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: Tasks 7 and 9; `Session.context_used`, `Session.cost_usd`, `Conversation::changes`, the sidebar footer's usage bar and its levels.
- Produces: keys `mr-panel-context`, `mr-panel-context-bar`, `mr-panel-cost`, `mr-panel-files`, `mr-panel-file-{i}`.

Rules:

- **"Context"**, a heading, then:
  - "{N}% of context used" over a thin bar, built and coloured as the sidebar's usage bars are (normal below 75, warning from 75, alarm from 90), N rounded to a whole number. While it is not known: "Context use is not known yet.", and no bar.
  - "Cost so far: $0.42", two decimals; "Cost so far: less than $0.01" for an amount above nothing and below a cent. While it is not known: "Cost is not known yet."
- **"Files changed"**, a heading with the number of files beside it when there are any, then a row for each: its path, relative to the session's folder when it lies in it, cut at its start when too long with the whole path as its tooltip; then `+{added}` and `−{removed}` in the colours a diff's counts have in the conversation, a count of 0 left out. None: "No files changed yet." When the session's history has an earlier part that is not loaded, a last muted line: "Changes in earlier messages that are not loaded are not listed."
- The rows are read from `Conversation::changes`: the view walks no conversation.
- A list longer than the panel scrolls with the panel.
- The `panel` scene's focused session has a context of 37%, a cost and four changed files, one of them outside the folder.

- [ ] **Step 1: Write the failing tests** in `crates/app/src/view/panel.rs`:
  - `context_and_cost_are_not_known_until_a_turn_ends`, then read "37% of context used" and "Cost so far: $0.42".
  - `the_context_bar_changes_colour_as_the_sidebars_do`: 74, 75 and 90.
  - `a_cost_below_a_cent_says_so`, `a_cost_of_nothing_reads_zero`.
  - `the_files_a_session_changed_are_listed_with_their_counts`: relative inside the folder, whole outside it, a count of 0 left out.
  - `the_list_grows_as_edits_finish`, and an edit that is denied adds nothing.
  - `no_files_changed_yet`.
  - `a_history_with_an_earlier_part_says_what_is_not_listed`.
  - `the_panel_builds_no_conversation_row`: opening it, and an event in another session, build no row of any conversation again.
  - `fifty_files_scroll_with_the_panel`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: The scene.** Make the `panel` screenshot again in both themes and look at both.
- [ ] **Step 5: Run** the three checks.
- [ ] **Step 6: Commit** `feat(app): the panel shows context, cost and the files changed`.

### Task 12: The real path, notes, and Jerod's look

**Files:**
- Modify: `crates/probe/src/main.rs`, `crates/probe/src/repl.rs`, `crates/app/tests/live.rs`, `docs/notes-for-later-plans.md`, `docs/protocol-notes.md`

Rules:

- **The probe** takes `--arg VALUE`, any number of times: each is passed to the program after the probe's own arguments. This is how a session is recorded with `--replay-user-messages`.
- **The live test** prints, in its first session, each on a line of its own and none of them a way to fail:
  - `live: effort at start: <level or none>`, from the first `EffortChanged`;
  - `live: effort changed to <level> in N ms`, after choosing another of the model's levels, or `live: effort did not change: <reason>`; then the same for changing it back;
  - `live: context <N>% and cost known: yes|no` after its first turn;
  - `live: remote control: on in N ms`, or `refused (<why>)`, or `no answer in 30 s`, after turning it on following the first turn; then `live: remote control: off`. The address is never printed.
  In its second session, before that session's first message: `live: remote control before a first turn: on in N ms`, or what else happened; then off.
  Every wait has a deadline and says what it waited for.
- **The notes** get a new first section, "Jerod's look at plan 5", in the shape of the looks before it: the two commands, "Before looking" from "What this window will not do yet", what the live test now prints, the things to try below, each ending "Seen: not yet recorded.", the check with a second device, every decision of this plan with what was built, and what the tasks left.
  1. The effort level shows beside the model in every tile, and a change holds for the next message.
  2. An old session opens with the effort it last used.
  3. The button at the top right and Ctrl+Alt+B show the panel; it comes back as it was after a restart.
  4. The panel follows the focused tile: folder, branch, model, effort, mode.
  5. Context and cost appear after a turn; the files a session changed are listed with their counts.
  6. The Remote Control switch turns on, the tile says "Remote", and "Copy link" gives a working address.
  7. "On for new sessions" makes the next new session start with it.
  8. On the ultrawide: is the panel worth its width, and should it start open?
- **The check with a second device**, written into the notes as steps for Jerod, the answer to the oldest open check in the file ("Remote Control, seen by a person"):
  1. In the window, turn Remote Control on in a session. At claude.ai/code or in the Claude app: is the session listed, and does a prompt typed there run here?
  2. For Task 13, a recording: `cargo run -p mr-probe -- raw --cwd SOME_FOLDER --full-login --arg --replay-user-messages --record target/rec/p5-remote.txt`, then `say Reply with the word one.`, `rc on`, a prompt typed on the other device, a permission request answered there if one comes, `rc off`, `quit`. The recording stays in `target/rec/`, which is never committed.
- `docs/protocol-notes.md` gets what this plan read in the program (the "Facts this plan builds on" above, with what was seen live marked apart from what was not).

- [ ] **Step 1: The probe**, with a test that the arguments are passed in their order.
- [ ] **Step 2: The live test.** It compiles and stays `#[ignore]`; no agent runs it.
- [ ] **Step 3: The notes.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(probe): extra arguments for the program`, `test(live): effort, context and Remote Control are printed`, `docs: plan 5, in the notes`.

### Task 13: A prompt typed on another device is shown (only from a recording)

This task is built only when Jerod has made the recording of Task 12 and it is on disk. If it is not there when Tasks 1 to 12 are done, the plan ends without this task, and the notes say so under "Left by plan 5" with what the window shows until then.

**Files:**
- Modify: `crates/claude/src/args.rs`, `wire.rs`, `translate.rs`, `crates/agent/src/event.rs`, `crates/core/src/conversation.rs`, `crates/core/src/session.rs`

**Interfaces:**
- Consumes: the recording `target/rec/p5-remote.txt`, for the names and shapes of its records only.
- Produces:
  ```rust
  // agent/src/event.rs
  pub enum SessionEvent {
      /* as it is */
      /// A prompt that came from somewhere other than this app's own input.
      PromptFromElsewhere(String),
  }
  ```

Rules:

- The program is started with `--replay-user-messages` only if the recording shows that a prompt from another device then arrives as a record, and that the app's own prompts can be told from it with certainty. How they are told apart is decided from the recording: say in the report which field or which order decides, and quote the record's shape with made-up values.
- A record that is the program's own output dressed as a user's (the recording of 2026-11-11 shows `<local-command-stdout>` ones) is never a prompt.
- When in doubt, nothing is shown: a missing prompt is the state before this task; a line shown as the user's that is not one is worse.
- `PromptFromElsewhere(text)` adds the text to the conversation as a message of the user's, before the reply it started.
- A fixture is written by hand in the recording's shapes, with made-up text and ids. The recording itself is not committed and nothing is copied out of it.

- [ ] **Step 1: Read the recording** and write down, in the report, what it shows for each rule above. If it does not show what the first rule needs, stop here: report it, change nothing, and the controller records the ruling.
- [ ] **Step 2: Write the failing tests**: `a_prompt_from_another_device_is_shown_before_its_reply`, `the_apps_own_prompt_is_not_shown_twice`, `the_programs_own_output_is_never_a_prompt`, `a_record_that_cannot_be_told_apart_shows_nothing`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat: a prompt typed on another device is shown`.
