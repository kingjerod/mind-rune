# Many Sessions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The window knows many sessions at once: the sidebar lists them under groups, any of them can be shown in the conversation area while the others keep working, stored sessions open from disk, and a new session starts in a folder the user chooses.

**Architecture:** `mr-core`'s `App` stops holding one session and holds a store of them, each with its own numbered open, plus the groups, the saved state and the one session that is shown. Stored sessions are plain entries that rest on disk until a message is sent; their history is read off the window's thread, from the end of the file. The backend gets one thread per open session and one for everything else. The window keeps one conversation area; every view takes the session it draws as an argument, so that plan 4 can draw several.

**Tech Stack:** Rust 1.99.1 (edition 2024), `rune` and `rune-kit` at the pinned revision, `serde`, `serde_json`, std threads and channels, `tempfile` for tests. One change to dependencies: Task 12 turns on Rune's `native-file-dialog` feature.

**Spec:** `docs/superpowers/specs/2026-11-10-claude-code-ui-design.md`. This plan implements section 5.1 in full, the parts of 5.8 about new, stored and possibly-live sessions, the saved state of section 9 except tiles, and the "stored session file that cannot be read" case of section 10. Read these before starting, in this order: the spec, `docs/notes-for-later-plans.md`, `docs/protocol-notes.md`.

**For the controller:** Jerod's rule since 2026-11-13: every subagent (implementer, reviewer, fixer) runs on Opus 5.5 at high effort or better. Nothing less, whatever the size of the task.

## This is plan 3 of 5

The plan table used to have four rows. Plan 3 was "many sessions and tiles"; it is two plans now, so that Jerod can use the sidebar before tiles are built on top of it. After plan 2 Jerod judged the window against Claude Desktop as a whole, and the first thing missing was a row per session.

| Plan | Delivers |
| --- | --- |
| 1. Foundations and Claude adapter (done) | Workspace, agent seam, fake agent, Claude adapter, live protocol verification |
| 2. One session in a window (done) | App state, the shell, one conversation, request cards, input box, conversation width, usage footer, sign-in screen, titles, Auto mode |
| **3. Many sessions (this plan)** | Many sessions in the core, the sidebar with groups and state marks, stored sessions and their history, a folder for each new session, saved groups |
| 4. Tiles | Several sessions side by side, tile headers, the focused tile, "open to the side", renaming a session, the saved tile arrangement |
| 5. Attachments, Remote Control, background running | Paste and drop, right panel, Remote Control switch, keep running after the window closes, quit rules, the effort level |

**Not in this plan, on purpose.** More than one conversation on screen, tile headers, renaming a session, dragging a row into the middle (plan 4). Attachments, the right panel, Remote Control, background running, stopping idle programs, showing or choosing the effort level (plan 5). From "Left by plan 2" in the notes, this plan fixes only what many sessions make worse: Restart for a session with nothing stored, a session that ends cleanly, a failed send that leaves the spinner on, the stale usage footer, and the dead handles in the backend. The rest stays in the notes.

## What this window will not do yet

Say this to Jerod before their look, so the window is judged for what it is.

- One conversation is on screen at a time. Clicking a row swaps it. Side by side is plan 4.
- A session cannot be renamed. Its title is the generated one.
- Closing the window ends every session's program. Running in the background is plan 5.
- Rows cannot be dragged into the middle, and groups cannot be reordered.

## Decisions in this plan that the design does not make, or makes differently

Each is the planner's call. Jerod has not seen them; they are listed in the handover for review.

1. **Which sessions are listed.** A stored session is listed when it was active in the last 30 days, or was shown or started in this run, or was the session on screen when the app last closed. The design also says "every session this app has started or opened", for ever; that would need the app to remember every such session and the list would only grow. Older ones are under "Show older".
2. **Order.** Groups are ordered by the time of the last message the user sent in any of their sessions, newest first; a group with no sessions comes first. Rows in a group are ordered the same way. Nothing moves when a turn ends, only when the user sends.
3. **Launch.** `mindrune` with no folder restores the session that was on screen last time, without starting a program for it. `mindrune FOLDER` starts a new session in that folder. With nothing to restore, the conversation area says how to start.
4. **An empty new session leaves nothing behind.** A new session with no message that stops being shown is closed and forgotten.
5. **A look at launch.** So that the usage footer and the model list are filled before any session runs, the app starts `claude` once at launch in a quiet way, asks for the models and the limits, and closes it. This runs the user's session-start hooks once, in the shown session's folder or the home folder.
6. **The model and mode of a resting session.** A stored session shows the model that last answered in it and the mode it was last in, both read from its history. Both pickers work before the program starts, and what they show is passed to the program when it starts. A restart now keeps the model and the mode as well.
7. **How much history is read.** The end of the file: at most 16 MiB of it, at most 2,000 items, along the chain of replies that leads to the last one. Above that the conversation says "Earlier messages are not shown."
8. **A session whose program ends cleanly rests.** It is not "ended": the next message starts its program again.
9. **Closing the window ends every session**, waiting up to 2.5 seconds in all.
10. **Groups with no recent session are folded into one row**, "Older groups", at the bottom, beside "Hidden groups".
11. **Dragging a row onto a group waits for plan 4.** Rune has no dragging between elements inside a window; it has to be built by hand. Plan 4 builds one for tiles, and the same one will move rows. Until then a session moves through "Move to group" in its row's menu.
12. **A group is renamed from its menu**, not by a double-click: a click on the group's name folds and unfolds it.
13. **Rows are one line.** The folder is the group's name; a row shows its folder, in faint text at its end, only when it sits in another group.
14. **Choosing a folder** is a short menu under "New session": the folders used most recently, then "Choose a folder…", which opens the desktop's own folder picker.

## Global Constraints

- Rust toolchain is exactly `1.99.1`, edition `2024`. In an agent's shell cargo is not on `PATH`: start every command with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Rune is pinned to `rev = "2a44156b21d63b0342a366b26d9222a6edd4622c"` of `https://github.com/anomalyco/rune`. `rune` and `rune-kit` always use the same revision. Its source is at `~/.cargo/git/checkouts/rune-c9b805e9ed8a3730/2a44156`; read it instead of guessing an API. Where this plan and the source differ, the source wins: say so in your report.
- `mr-agent` and `mr-core` depend on neither `mr-claude` nor `rune`. No type from `mr-claude` appears in `mr-core` or in any view. Only `crates/app/src/main.rs`, `crates/app/tests/live.rs` and `crates/probe` may name `ClaudeAgent`.
- The UI thread never blocks. Every `Agent` method and `SessionHandle::send` is called only on a backend thread. The one exception stays the bounded wait when the window closes.
- A session's events are trusted only while they carry the number of that session's current open. Everything from the backend that belongs to an open carries its number, and a stale number is dropped.
- Every session intent of `App` takes the `SessionKey` it acts on. No view reads "the current session" from anywhere but its own argument.
- Every `App` intent a view calls is followed by `glue::run`.
- Request buttons (Allow once, Always allow, Deny, question options, Approve, Keep planning) never take keyboard focus and never react to Enter or Space typed in the input box. The same holds for the buttons of the "may be open in another app" card.
- The software never writes inside `~/.claude`. Its own settings go through `rune::prefs::Prefs::open("mindrune")`. Tests and headless runs set `RUNE_DIRS_ROOT` to a temporary directory or use `Prefs::memory()`.
- Nothing reads a whole stored session file into memory. History is read from the end, within the limits of decision 7.
- No Anthropic logos, colours or fonts. The theme is the kit's neutral one and follows the system's light or dark setting.
- User-visible wording is exact where this plan quotes it. Never use a real person's name or e-mail address as test data; use `alex`, `alex@example.org`, `/home/alex`. Never copy text out of a real stored session into a test or a fixture: build fixtures by hand.
- All window tests and all screenshots use `FakeAgent`, through the demo window. Tests that start the real `claude` stay opt-in behind `MINDRUNE_LIVE=1` and `#[ignore]`. No agent starts the real `claude`.
- Every key this app gives an element starts with `mr-`. A key that names a session ends with the session's key number (`mr-row-7`), a key that names a group with the group's key text (`mr-group-c3`, see Task 7).
- A view never does work that grows with the length of a conversation or with the number of stored sessions beyond the rows on screen. The sidebar's model is rebuilt in the core when something changed (`App::settle`), never in a view.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` (no warnings) and `cargo test --workspace` pass before every commit.
- **Git.** Work on a new branch `plan-3-many-sessions` cut from `main`. Never push. Never commit `target/` or `.superpowers/`. Commit by explicit path, not `git add -A`. End every commit message with a blank line and `Co-Authored-By: Claude <noreply@anthropic.com>`, in exactly that form.
- Version of `claude` this plan was written against: `2.1.323`.

## Review Focus

The spec implies these conditions and no screen in it shows them. Each has a test in the task named.

1. **A very large history.** One stored session on this machine is 242 MB; the median is 27 KB. Expected: opening it takes about as long as opening a small one, shows its end, and says that earlier messages are not shown. Tests: Task 2, Task 3, Task 13.
2. **Events from a session that is not on screen.** Three sessions work at once and the user looks at a fourth. Expected: each session's events reach only that session, its row's mark changes, and nothing is drawn in the conversation on screen. Tests: Task 5, Task 9, Task 10.
3. **A request arrives in a session that is not on screen.** Expected: its row shows "needs you", nothing answers it by accident, and the card is there when the session is shown. Tests: Task 5, Task 10.
4. **A stored session that changed under the app.** Its file was written two seconds ago by another program, or it was deleted, or it cannot be read. Expected: the "may be open in another app" card before anything is sent; a plain error in the conversation for a file that is gone or broken; the row stays. Tests: Task 2, Task 6, Task 13.
5. **A saved state that no longer fits.** The saved file names a group whose sessions are gone, a session that no longer exists, a folder that was deleted, or it is not valid at all. Expected: the app starts, drops what does not fit and keeps the rest. Tests: Task 8, Task 12.

## File Structure

```
crates/agent/src/agent.rs          Task 1: StoredHistory, read_session with a limit, last_active
crates/agent/src/fake.rs           Task 1: the fake's stored histories and activity times
crates/claude/src/history.rs       Task 2: titles, the describe cache
crates/claude/src/history_tail.rs  Task 2 (new): reading the end of a file along the chain
crates/core/src/conversation.rs    Task 3: a conversation from history
crates/core/src/session.rs         Task 3: Phase::Resting, history state, start_spec, waking
crates/core/src/backend.rs         Task 4: a thread per open, the library thread, the look
crates/core/src/sessions.rs        Tasks 4, 5, 6 (new): SessionKey, Entry, Sessions, Mark
crates/core/src/app.rs             Tasks 5, 6, 7, 8: the app over many sessions
crates/core/src/groups.rs          Task 7 (new): GroupId, Groups, the sidebar's model
crates/core/src/saved.rs           Task 8 (new): Saved, what is kept between runs
crates/app/src/glue.rs             Tasks 5, 6, 8, 9: effects, SessionView per session, closing
crates/app/src/view/mod.rs         Task 9: the conversation area takes a session
crates/app/src/view/sidebar.rs     Tasks 10, 11 (new): groups, rows, marks, menus
crates/app/src/view/shell.rs       Task 10: keeps the shell and the usage footer only
crates/app/src/view/folder.rs      Task 12 (new): choosing a folder for a new session
crates/app/src/view/resting.rs     Task 13 (new): loading, failed, and the "open elsewhere" card
crates/app/src/demo.rs             Tasks 9 to 14: scenes with several sessions
crates/app/tests/                  Task 14: snapshots, replay, live
docs/                              Task 14: notes, protocol notes
```

## Facts about stored sessions

Checked on 2026-11-13 against the real store (790 files, key names and shapes only, no content). Each stored session is one file of JSON lines.

- Records that carry conversation have `type` `user` or `assistant`, a `uuid` and a `parentUuid`. Other record types (`system`, `attachment`) also have `uuid` and `parentUuid`, and a `user` or `assistant` record's parent is often one of those. So the chain must be followed through every record that has a `uuid`, not only through `user` and `assistant`.
- A file can hold abandoned branches: two records with the same `parentUuid`. The conversation is the chain that ends at the last `user` or `assistant` record of the file that is not a side chain (`isSidechain: true`).
- Compaction writes a `system` record with `subtype: "compact_boundary"`, `parentUuid: null` and `logicalParentUuid` naming the record before it. It is followed by a `user` record with `isCompactSummary: true` (also `isVisibleInTranscriptOnly: true`) whose text is the summary.
- The model that answered is `message.model` on an `assistant` record. A program-made message has the model `<synthetic>`.
- A record `{"type":"permission-mode","permissionMode":"auto","sessionId":…}` is written when the mode changes. The names are the wire names (`default`, `acceptEdits`, `plan`, `auto`, `bypassPermissions`).
- A generated title is `{"type":"ai-title","aiTitle":…}`. A title the user gave is `{"type":"custom-title","customTitle":…}`. Both can appear many times; the last one counts, and a custom title wins.
- A session with no message stores nothing.

## Tasks

### Task 1: The seam reads a bounded history

**Files:**
- Modify: `crates/agent/src/agent.rs`, `crates/agent/src/fake.rs`, `crates/agent/src/lib.rs`
- Modify (to keep the workspace building, nothing more): `crates/claude/src/agent.rs`, every caller of `read_session` and `set_history`

**Interfaces:**
- Produces:
  ```rust
  pub struct StoredHistory {
      pub items: Vec<HistoryItem>,
      /// The session holds more than `items`, which are its end.
      pub earlier_left_out: bool,
      /// The full id of the model that last answered, when the history names one.
      pub model: Option<String>,
      /// The mode the session was last in, when the history names one.
      pub permission_mode: Option<PermissionMode>,
  }
  // trait Agent
  fn read_session(&self, id: &SessionId, max_items: usize) -> Result<StoredHistory, AgentError>;
  /// When the stored session last changed, in Unix seconds; `None` when nothing is stored
  /// for it. Reads no content.
  fn last_active(&self, id: &SessionId) -> Result<Option<i64>, AgentError>;
  // enum HistoryItem gains
  Compacted, // the agent summarised everything above this point to save room
  // FakeAgent
  pub fn set_history(&self, id: &SessionId, history: StoredHistory);
  pub fn fail_history(&self, id: &SessionId, detail: &str); // read_session gives Io(detail)
  pub fn set_last_active(&self, id: &SessionId, unix: Option<i64>);
  ```
- `StoredHistory` derives what `HistoryItem` derives, and `Default`.

- [ ] **Step 1: Write the failing tests** in `crates/agent/src/fake.rs`:
  - `the_fake_returns_the_end_of_a_long_history`: a history of five `AssistantText` items "0" to "4" with `model: Some("fake-model-1")` and `permission_mode: Some(PermissionMode::Plan)`. `read_session(&id, 2)` gives items "3" and "4", `earlier_left_out == true`, and the same model and mode. `read_session(&id, 10)` gives all five and `earlier_left_out == false`. A history stored with `earlier_left_out: true` keeps it at any limit.
  - `a_history_that_cannot_be_read_is_an_error`: after `fail_history(&id, "gone")`, `read_session` is `Err(AgentError::Io("gone".into()))`.
  - `the_fake_knows_when_a_session_was_last_active`: a session given to `set_sessions` with `last_active_unix: 500` answers `Ok(Some(500))`; after `set_last_active(&id, Some(900))` it answers `Ok(Some(900))`; an unknown id answers `Ok(None)`.
- [ ] **Step 2: Run them; they fail to compile.** `cargo test -p mr-agent`
- [ ] **Step 3: Implement.** In `ClaudeAgent`, `read_session` keeps reading as today and returns the last `max_items` with `earlier_left_out` set when it cut, `model` and `permission_mode` `None`; Task 2 replaces that body. `last_active` is the modification time of the file `find_session` finds, and `Ok(None)` when it finds none.
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: all pass.
- [ ] **Step 5: Commit** `feat(agent): a stored history has a limit, a model and a mode`

### Task 2: The adapter reads history from the end of the file

**Files:**
- Create: `crates/claude/src/history_tail.rs`
- Modify: `crates/claude/src/history.rs`, `crates/claude/src/agent.rs`, `crates/claude/src/lib.rs`

**Interfaces:**
- Consumes: `StoredHistory`, `HistoryItem::Compacted` (Task 1); `mode_from_wire` (`mode.rs`); the record-to-item code in `history.rs`.
- Produces:
  ```rust
  // history_tail.rs
  pub const TAIL_BYTES: u64 = 16 * 1024 * 1024;
  /// The end of a stored conversation: the last `max_items` items of the chain that leads to
  /// the file's last message, read from at most the last `tail_bytes` of the file.
  pub fn read_tail(path: &Path, max_items: usize, tail_bytes: u64) -> Result<StoredHistory, AgentError>;
  ```
  `ClaudeAgent::read_session` calls it with `TAIL_BYTES`. `read_history` goes, with its tests moved over.

Behaviour, with the facts above:

1. Read at most `tail_bytes` from the end. When the read does not start at byte 0, drop everything up to and including the first line break. A line that is not complete JSON is skipped.
2. The leaf is the last `user` or `assistant` record in the region that is not a side chain. From it, follow `parentUuid` through records of any type. At a record whose `parentUuid` is null and that has a `logicalParentUuid`, go on from that id and remember a compaction between the two. Stop at a record with no parent, or at a parent that is not in the region.
3. Turn the records on the chain into items, oldest first, with the rules `history.rs` already has. A record with `isCompactSummary: true` gives no item. Each remembered compaction gives one `HistoryItem::Compacted` at its place.
4. Keep the last `max_items`. `earlier_left_out` is true when items were cut, or the walk stopped at a parent outside the region, or the region did not start at byte 0 and the walk reached a record with no parent that is not the file's first record.
5. `model` is the `message.model` of the last `assistant` record on the chain whose model is not `<synthetic>`. `permission_mode` is the last `permission-mode` record in the region, through `mode_from_wire`.
6. A file that does not exist is `AgentError::Io`. An empty file, or a region with no message, is an empty history, not an error.

Also in `history.rs`:

- `describe` takes the last `custom-title` (`customTitle`) over the last `ai-title` (`aiTitle`).
- `stored_sessions` does not describe a file again whose path, modification time and length are unchanged since the last call. `ClaudeAgent` keeps that table behind a mutex.

- [ ] **Step 1: Write the failing tests.** Build each file by hand in a `tempfile` directory, with made-up text. In `history_tail.rs`:
  - `a_history_follows_the_chain_not_the_order_of_the_file`: records u1, a1 (parent u1), u2 (parent a1), a2 (parent u2), then u3 (parent a1) and a3 (parent u3). Items are the texts of u1, a1, u3, a3.
  - `the_chain_runs_through_records_that_are_not_messages`: a `system` record sits between a1 and u2 as u2's parent. Nothing is lost, and the `system` record gives no item.
  - `a_compaction_is_crossed_and_marked`: u1, a1, a boundary (`parentUuid` null, `logicalParentUuid` a1), a summary `user` (parent the boundary, `isCompactSummary: true`), u2 (parent the summary), a2. Items: u1, a1, `Compacted`, u2, a2. The summary's text is nowhere.
  - `only_the_end_is_kept`: ten turns, `max_items` 3: the last three items, `earlier_left_out == true`.
  - `a_file_larger_than_the_region_is_read_from_its_end`: 200 turns of about 100 bytes, `tail_bytes` 4096: no error, the last item is the file's last, `earlier_left_out == true`, and no item is a cut-off line.
  - `a_side_chain_is_never_the_end`: the file's last record has `isSidechain: true`; the leaf is the message before it.
  - `the_last_model_and_mode_are_reported`: models `claude-x-1`, then `claude-x-2`, then `<synthetic>`; mode records `plan` then `auto`. Result `Some("claude-x-2")` and `Some(PermissionMode::Auto)`.
  - `an_empty_file_is_an_empty_history`, `a_missing_file_is_an_error`, `a_broken_last_line_is_skipped`.
  In `history.rs`:
  - `a_custom_title_wins_over_a_generated_one`, in both file orders.
  - `an_unchanged_file_is_not_described_again`: the table is given a counting describe function; two listings of the same directory call it once per file; touching one file's length calls it once more for that file only.
- [ ] **Step 2: Run them; they fail.** `cargo test -p mr-claude history`
- [ ] **Step 3: Implement.** Read the region with one `seek` and one read; do not parse lines that contain none of `"uuid"`, `permission-mode`.
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: all pass, the existing history fixtures included.
- [ ] **Step 5: Commit** `feat(claude): history is read from the end of the file, along the chain`

### Task 3: A session can rest on disk

**Files:**
- Modify: `crates/core/src/session.rs`, `crates/core/src/conversation.rs`, `crates/core/src/lib.rs`, and `crates/core/src/app.rs` only where `restart_spec` is called

**Interfaces:**
- Consumes: `StoredHistory`, `StoredSession`, `HistoryItem::Compacted`.
- Produces:
  ```rust
  // Phase gains
  /// No program runs for it; its conversation is on disk. The next message starts one.
  Resting,
  pub enum HistoryState { NotLoaded, Loading, Loaded, Failed(String) }
  // Session gains
  pub history: HistoryState,
  /// Goes up whenever the conversation's items are replaced, not appended to.
  pub generation: u64,
  pub fn resting(stored: &StoredSession) -> Session; // id and title set, Resting, NotLoaded
  pub fn load_history(&mut self, history: StoredHistory);
  pub fn history_failed(&mut self, reason: &str);
  /// Whether the agent has anything stored for it: it came from the store, or a message
  /// was handed over in this run.
  pub fn is_stored(&self) -> bool;
  /// Resting to Opening, with `text` queued for when the program is ready.
  pub fn wake(&mut self, text: &str);
  /// Undoes `wake` before a program was opened: back to Resting, giving the text back.
  pub fn unwake(&mut self) -> Option<String>;
  /// How to start its program: resuming when `is_stored`, afresh otherwise; with the model
  /// and mode it shows. Replaces `restart_spec`.
  pub fn start_spec(&self) -> StartSpec;
  /// A message could not be handed over: Working goes back to Idle.
  pub fn send_failed(&mut self);
  /// Gives the session models to choose from when it has none of its own yet.
  pub fn offer_models(&mut self, models: &[ModelOption]);
  // Conversation gains
  pub fn from_history(history: &StoredHistory) -> Conversation;
  pub fn put_history_first(&mut self, history: &StoredHistory);
  // NoticeKind gains EarlierLeftOut, Compacted
  ```
- `Session::opening` gives `history: Loaded`. `start_spec` sets `model` to the option id the user chose, else the session's model id, else `None`; `permission_mode` to the session's mode unless it is `Ask`.

Wording: `EarlierLeftOut` is "Earlier messages are not shown." and is the first item. `Compacted` is "Earlier messages were summarised here to save room."

- [ ] **Step 1: Write the failing tests.**
  - `a_stored_session_rests`: `Session::resting(&stored)` has the id, the title, the folder, `Phase::Resting`, `HistoryState::NotLoaded`, an empty conversation, and `is_stored()`.
  - `history_becomes_the_conversation`: a history of `User`, `AssistantText`, a `Tool` with no outcome, `Compacted`, `User`, with `earlier_left_out: true`. The items are: the `EarlierLeftOut` notice, the user's message, the reply, the tool call closed as `ToolOutcome::Failed` with `DID_NOT_FINISH`, the `Compacted` notice, the second message. `history` is `Loaded`, `generation` went up by one, and the phase is still `Resting`.
  - `history_names_the_model_and_the_mode`: after loading a history with `model: Some("m-2")` and `permission_mode: Some(Plan)` the session has both. A mode or model the user chose before the history arrived is kept.
  - `history_that_arrives_late_goes_first`: a session that already holds a user message and a reply from this run gets a history of two items: the two come first, the run's items keep their order after them, and `generation` went up.
  - `the_pickers_work_before_the_program_starts`: resting, with models offered. `choose_mode(Auto)` and `choose_model("opus")` return no command and change what the session shows. `start_spec()` then has `resume: Some(id)`, `model: Some("opus")`, `permission_mode: Some(Auto)`.
  - `a_session_with_nothing_stored_starts_afresh`: `Session::opening`, then `Exited { clean: false, .. }` before any message: `start_spec().resume` is `None` and the folder is the same.
  - `waking_queues_the_message`: `wake("hi")` gives `Phase::Opening` and `queued == Some("hi")`. `Started` then returns the `Send` for "hi" and no `SuggestTitle`, because a stored session has a title. `unwake()` straight after `wake("hi")` gives `Some("hi")` and `Phase::Resting`.
  - `a_clean_end_puts_a_stored_session_to_rest`: with a message handed over, `Exited { clean: true, .. }` gives `Phase::Resting` and adds no notice. Without one it gives `Phase::Ended` as today. An unclean exit is `Ended` in both cases.
  - `a_failed_message_returns_to_idle`: `submit` then `send_failed()`: the phase is `Idle`, and the user's message stays in the conversation.
  - `offered_models_do_not_replace_the_sessions_own`.
- [ ] **Step 2: Run them; they fail.** `cargo test -p mr-core session`
- [ ] **Step 3: Implement.** `App::send_failed` calls `Session::send_failed` when the failed command was a `Send`. `App::restart` uses `start_spec`. Every `match` on `Phase` in the workspace handles `Resting` on purpose: a resting session's input box works, and its row has no mark.
- [ ] **Step 4: Run** `cargo test --workspace`.
- [ ] **Step 5: Commit** `feat(core): a session can rest on disk and wake with its history`

### Task 4: The backend gives every open its own thread

**Files:**
- Modify: `crates/core/src/backend.rs`, `crates/core/src/lib.rs`
- Create: `crates/core/src/sessions.rs` with only `SessionKey` for now

**Interfaces:**
- Produces:
  ```rust
  // sessions.rs
  /// This app's own number for a session it knows. It lasts for one run.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
  pub struct SessionKey(pub u64);
  // ToBackend gains
  ListSessions { since_unix: Option<i64> },
  ReadHistory { key: SessionKey, id: SessionId, max_items: usize },
  CheckActivity { key: SessionKey, id: SessionId },
  /// Start a session in `folder`, learn the models and the usage limits, close it.
  Look { folder: PathBuf },
  // FromBackend gains
  Sessions { since_unix: Option<i64>, result: Result<Vec<StoredSession>, AgentError> },
  History { key: SessionKey, result: Result<StoredHistory, AgentError> },
  Activity { key: SessionKey, result: Result<Option<i64>, AgentError> },
  Look { models: Vec<ModelOption>, model: Option<String>, usage: Option<UsageLimits> },
  // Backend
  /// How many opens still have a thread.
  pub fn open_count(&self) -> usize;
  pub const LOOK_START_LIMIT: Duration = Duration::from_secs(20);
  pub const LOOK_USAGE_LIMIT: Duration = Duration::from_secs(10);
  ```
- `Backend::start`, `send`, `try_recv`, `recv_timeout` and `flush` keep their signatures (`send` may take `&mut self`). `App::receive` ignores the four new messages until Task 6.

Rules:

- Each `Open` gets its own thread, which makes that open's `Agent::open` call and every `send` for it, in order. A `Send` for an open with no thread is answered `SendFailed` with `AgentError::Closed`.
- An open's thread ends when its `open` failed, when it passed on `Close`, when a `send` returned `Closed`, or when the session reported `Exited`. Nothing of it is kept after that.
- `CheckAccount`, `ListSessions`, `ReadHistory`, `CheckActivity` and `Look` run one at a time on one more thread, the library thread, in the order they were sent.
- `Look` opens a session, waits up to `LOOK_START_LIMIT` for `Started`, sends `RefreshUsage` when the agent has usage limits, waits up to `LOOK_USAGE_LIMIT` for `UsageLimits`, sends `Close`, and answers with what it learned. It always answers, with empty values when it learned nothing, and it always closes what it opened.
- Dropping the `Backend` waits for nothing; every thread passes `Close` to the session it holds and ends.
- `flush` returns when everything sent before it has been handled on every thread.

- [ ] **Step 1: Write the failing tests** in `backend.rs`. Where a test needs an agent that blocks, wrap `FakeAgent` in a test-only `Agent` whose `open` or `list_sessions` waits on a channel.
  - `a_slow_open_does_not_hold_up_another_session`: open 1 blocks; open 2 and a `Send` to it go through; the fake has the message while open 1 is still blocked.
  - `a_slow_listing_does_not_hold_up_a_send`.
  - `library_jobs_keep_their_order`: `ListSessions` then `ReadHistory` answer in that order.
  - `the_listing_and_the_history_come_back`: with the fake's `set_sessions`, `set_history` and `set_last_active`, each of the three library messages gives its `FromBackend` with the same key or `since_unix`.
  - `a_session_that_ended_keeps_no_thread`: after `Exited`, `open_count()` reaches 0 within a deadline, and a later `Send` is answered `SendFailed` with `Closed`.
  - `a_look_reports_and_closes`: the fake starts with two models and answers `RefreshUsage`; `FromBackend::Look` holds both models and the limits, and `fake.sent()` ends with `RefreshUsage` then `Close` for that session.
  - `a_look_that_learns_nothing_still_answers`: `fail_next_open`; the answer has no models and no usage.
  - `dropping_the_backend_closes_every_session`: three opens; after the drop the fake has `Close` for all three, within a deadline.
  - Every test of the old worker still passes.
- [ ] **Step 2: Run them; they fail.** `cargo test -p mr-core backend`
- [ ] **Step 3: Implement.** Tests wait with a deadline, never with a fixed sleep.
- [ ] **Step 4: Run** `cargo test --workspace`.
- [ ] **Step 5: Commit** `feat(core): one backend thread per open, and one for the library`

### Task 5: The app holds many sessions

**Files:**
- Modify: `crates/core/src/sessions.rs`, `crates/core/src/app.rs`, `crates/core/src/lib.rs`
- Modify, mechanically: every place in `crates/app` that reads `app.session` or calls a session intent

**Interfaces:**
- Consumes: `Session` (Task 3), `SessionKey` (Task 4).
- Produces:
  ```rust
  // sessions.rs
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Mark { None, Working, NeedsYou, Finished }
  pub struct Entry {
      pub key: SessionKey,
      pub session: Session,
      /// The number of its current open; `None` while no program runs for it.
      pub open: Option<u64>,
      /// When the user last sent a message in it, or what the store said: Unix seconds.
      pub last_active_unix: i64,
      /// When its own program last reported anything, in this run: Unix seconds.
      pub last_own_unix: Option<i64>,
      /// A turn ended while it was not shown, and it has not been shown since.
      pub unseen: bool,
      /// It was shown or started in this run.
      pub touched: bool,
  }
  impl Entry { pub fn mark(&self) -> Mark; pub fn live(&self) -> bool; }
  pub struct Sessions { /* private */ }
  impl Sessions {
      pub fn add(&mut self, session: Session, last_active_unix: i64) -> SessionKey;
      pub fn remove(&mut self, key: SessionKey) -> Option<Entry>;
      pub fn get(&self, key: SessionKey) -> Option<&Entry>;
      pub fn get_mut(&mut self, key: SessionKey) -> Option<&mut Entry>;
      pub fn by_open(&mut self, open: u64) -> Option<&mut Entry>;
      pub fn by_id(&self, id: &SessionId) -> Option<SessionKey>;
      pub fn iter(&self) -> impl Iterator<Item = &Entry>;
      pub fn len(&self) -> usize;
  }
  // app.rs
  pub struct Launch { pub folder: Option<PathBuf> }
  impl App {
      pub fn new(capabilities: Capabilities, width: Width, launch: Launch, now_unix: i64) -> (App, Vec<Effect>);
      pub sessions: Sessions, pub shown: Option<SessionKey>, pub now_unix: i64,
      pub fn shown_entry(&self) -> Option<&Entry>;
      pub fn new_session(&mut self, folder: PathBuf) -> Vec<Effect>;
      pub fn show(&mut self, key: SessionKey) -> Vec<Effect>;
      pub fn submit(&mut self, key: SessionKey, text: &str) -> Vec<Effect>;
      pub fn stop(&mut self, key: SessionKey) -> Vec<Effect>;
      pub fn answer(&mut self, key: SessionKey, id: &RequestId, answer: Answer) -> Vec<Effect>;
      pub fn choose_model(&mut self, key: SessionKey, option_id: &str) -> Vec<Effect>;
      pub fn choose_mode(&mut self, key: SessionKey, mode: PermissionMode) -> Vec<Effect>;
      pub fn restart(&mut self, key: SessionKey) -> Vec<Effect>;
      /// `Close` for every session whose program has not ended.
      pub fn close(&mut self) -> Vec<Effect>;
      /// No session has a program still running.
      pub fn all_ended(&self) -> bool;
  }
  ```
  `App.session`, `App.folder`, `current_open` and `session_ended` go.

Rules:

- `mark()`: `NeedsYou` in `Phase::NeedsUser`; `Working` in `Phase::Working`, and in `Phase::Opening` with a message queued; `Finished` when `unseen`; otherwise `None`. `NeedsYou` and `Working` win over `Finished`.
- An event or a failure from the backend goes to the entry whose `open` is its number, and to nothing when no entry has it. It sets `last_own_unix`. When it ends a turn (`TurnFinished`, `TurnFailed`, or an unclean `Exited`) and the entry is not the shown one, `unseen` becomes true. When the session's phase becomes `Resting` or `Ended`, `open` becomes `None`.
- `show(key)` sets `shown`, clears `unseen`, sets `touched`. The entry that was shown, when it is a session that is not stored (`!is_stored()`) and has nothing queued, is closed (`Close` when live) and removed. Showing the shown entry does nothing.
- `new_session(folder)`: when the shown entry is such an empty session in the same folder, nothing happens. Otherwise a new entry is added in `Phase::Opening`, opened and shown.
- `submit` sets `last_active_unix` to `now_unix`. `submit` for a `Resting` session is Task 6; until then it does nothing.
- With a launch folder, `account_checked` for a signed-in account starts a new session there, as plan 2 did.
- `UsageLimits` from any session feeds `usage`; the newest wins.

- [ ] **Step 1: Write the failing tests** in `app.rs` and `sessions.rs`.
  - `each_session_gets_only_its_own_events`: three sessions; a `TextDelta` with the second one's open number changes only the second conversation.
  - `a_stale_open_changes_nothing`: after `restart`, an event with the old number is dropped.
  - `a_turn_that_ends_out_of_sight_is_marked_finished`: session A is shown, B finishes a turn: B's mark is `Finished`; `show(B)` clears it. A turn that ends in the shown session leaves `None`.
  - `a_request_out_of_sight_marks_needs_you`: `NeedsUser` in B while A is shown gives `Mark::NeedsYou`; `answer(B, ..)` sends the answer to B's open.
  - `an_empty_session_is_forgotten_when_left`: `new_session("/a")`, then `show` of another entry: the first is gone and a `Close` went to its open. With a message sent, or one queued, it stays.
  - `new_session_keeps_an_empty_one_in_the_same_folder`: twice `new_session("/a")` opens once. `new_session("/b")` after it replaces it.
  - `the_old_session_keeps_working`: A is in a turn, `new_session("/b")`: A's phase and open are unchanged and no `Close` is sent.
  - `close_ends_every_program`: three live sessions and one resting: three `Close` effects. `all_ended()` is false until all three report `Exited`.
  - `intents_for_an_unknown_key_do_nothing`.
  - Every plan 2 test of `App` still passes, rewritten to name its key.
- [ ] **Step 2: Run them; they fail.** `cargo test -p mr-core`
- [ ] **Step 3: Implement the core.**
- [ ] **Step 4: Adapt `crates/app` mechanically.** The window still shows one conversation: wherever a view or the glue read `ui.app.session`, it reads the shown entry's session, and every intent call names `ui.app.shown`. `main.rs` passes its folder as `Launch { folder: Some(..) }`. The sidebar still shows one row, for the shown session. Change no behaviour and no test's meaning. `glue::close_session` sends every `Close` and waits, up to `CLOSE_WAIT` in all, until `all_ended()`.
- [ ] **Step 5: Run** `cargo test --workspace`. Expected: all pass; the window tests are unchanged in what they assert.
- [ ] **Step 6: Commit** `feat(core): the app holds many sessions, one of them shown`

### Task 6: Stored sessions in the app

**Files:**
- Modify: `crates/core/src/app.rs`, `crates/core/src/sessions.rs`, `crates/core/src/lib.rs`
- Modify: `crates/app/src/glue.rs` (carry the new effects out; call `tick`)

**Interfaces:**
- Consumes: Tasks 3, 4, 5.
- Produces:
  ```rust
  pub const RECENT_DAYS: i64 = 30;
  pub const HISTORY_ITEMS: usize = 2000;
  pub const ELSEWHERE_SECONDS: i64 = 120;
  pub const RELIST_SECONDS: i64 = 60;
  pub const USAGE_RETRY_SECONDS: i64 = 300;
  #[derive(Clone, Debug, PartialEq)]
  pub enum Listing { NotAsked, Loading, Done, Failed(String) }
  #[derive(Clone, Debug, PartialEq)]
  pub enum Prompt { OpenElsewhere { key: SessionKey, text: String } }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum PromptChoice { ContinueHere, ContinueAsCopy, Cancel }
  // Launch gains
  pub look: bool,
  // Effect gains: ListSessions { since_unix }, ReadHistory { key, id, max_items },
  //               CheckActivity { key, id }, Look { folder }
  // App gains
  pub listing: Listing, pub listed_all: bool, pub prompt: Option<Prompt>,
  pub known_models: Vec<ModelOption>,
  /// Brings the clock up to date and returns what is due: a new listing, a usage refresh.
  pub fn tick(&mut self, now_unix: i64) -> Vec<Effect>;
  /// The text that goes back into that session's input box, when the choice was Cancel.
  pub fn answer_prompt(&mut self, choice: PromptChoice) -> (Vec<Effect>, Option<(SessionKey, String)>);
  // Sessions gains
  /// Adds what is new, refreshes the title and time of resting entries, and, when `complete`,
  /// removes resting entries that were never touched and are no longer in the store.
  pub fn merge_listing(&mut self, listed: Vec<StoredSession>, complete: bool);
  ```
  `App::receive` handles the four messages of Task 4.

Rules:

- **Listing.** A signed-in account asks for `ListSessions { since_unix: Some(now − RECENT_DAYS days) }`. When that answer is merged, the app asks once for `ListSessions { since_unix: None }`; that answer is complete and sets `listed_all`. `tick` asks for the recent listing again every `RELIST_SECONDS`. A failed listing sets `Listing::Failed` and keeps the entries there are.
- **Showing.** `show` of an entry whose history is `NotLoaded` sets it `Loading` and asks for `ReadHistory` with `HISTORY_ITEMS`. The answer loads the history or sets `Failed` with the error's text. An answer for a key that is gone is dropped.
- **Models.** `known_models` is the newest list from any `Started` or from a look. Every entry is offered them when they arrive and when it is shown.
- **Sending to a resting session.** `submit` for a `Resting` entry wakes it. When `last_own_unix` is `None`, it first asks `CheckActivity`; the session is `Opening` with the text queued meanwhile. On the answer: when the store's time is within `ELSEWHERE_SECONDS` of `now_unix`, the session goes back to `Resting` with nothing queued and `prompt` becomes `OpenElsewhere` with the text. Otherwise, and on an error, the program is opened with `start_spec()`. When `last_own_unix` is set (this app ran it in this run), there is no check.
- **The prompt.** `ContinueHere` wakes the session with the text and opens it. `ContinueAsCopy` adds a new entry with the same folder, title and conversation, opens it with `start_spec()` of the original plus `fork: true`, queues the text in it, shows it, and leaves the original resting. `Cancel` returns the key and the text. Each clears `prompt`. Only one prompt exists at a time; a second `submit` that would need one while one is up does nothing.
- **A look.** With `launch.look` and no launch folder, a signed-in account also asks for `Look` in the folder of the entry to restore (Task 8), or of the newest entry, once the first listing is in; with no entry at all, in the folder the app was given as `home` in `Launch`. Its answer fills `known_models` and `usage`.
- **Usage after a reset.** When a usage window's reset time is in the past and the last ask was more than `USAGE_RETRY_SECONDS` ago, `tick` sends `RefreshUsage` to any live session, or asks for a `Look` when none is live and `launch.look` is set.

`Launch` therefore also gains `pub home: PathBuf`.

- [ ] **Step 1: Write the failing tests.**
  - `signing_in_lists_the_recent_sessions_then_all`: the two effects, in order, with `since_unix` `Some(now − 2_592_000)` then `None`; `listed_all` only after the second.
  - `a_listing_adds_resting_entries`: two stored sessions give two entries in `Phase::Resting` with their titles and times.
  - `a_listing_does_not_disturb_a_live_session`: an entry with a program keeps its phase, conversation and `last_active_unix` when the listing names it with an older time.
  - `a_complete_listing_drops_what_is_gone`: an untouched resting entry missing from a complete listing is removed; a touched one and a live one stay.
  - `showing_a_stored_session_reads_its_history_once`.
  - `a_history_that_fails_is_shown_as_failed`: `HistoryState::Failed("i/o error: gone")`.
  - `a_message_to_a_resting_session_checks_then_opens`: `CheckActivity`, then on `Ok(Some(now − 3600))` an `Open` whose spec resumes the id; after `Started` the queued `Send` goes out.
  - `a_session_active_elsewhere_asks_first`: on `Ok(Some(now − 5))` there is no `Open`, the session rests again, and the prompt holds the text.
  - `continue_here_opens_it`, `cancel_gives_the_text_back`.
  - `continue_as_a_copy_forks`: a new entry, shown, with the original's conversation and title; the `Open` has `resume: Some(original id)` and `fork: true`; the original is `Resting`.
  - `a_session_this_app_ran_is_not_checked`: after a clean end in this run, `submit` gives `Open` at once.
  - `the_listing_is_asked_again_every_minute`: `tick(now + 59)` gives nothing, `tick(now + 60)` the recent listing.
  - `a_look_fills_the_models_and_the_limits`, and `no_look_with_a_launch_folder`.
  - `usage_is_asked_again_after_a_reset`: with a live session `RefreshUsage` goes to it; with none, a `Look`; neither twice within `USAGE_RETRY_SECONDS`.
- [ ] **Step 2: Run them; they fail.** `cargo test -p mr-core app`
- [ ] **Step 3: Implement.** In `glue.rs`, `run` maps the four effects to their backend messages and `poll` calls `tick` with the wall clock and runs what comes back. `main.rs` passes `look: true` and the home folder; the demo passes `look: false`.
- [ ] **Step 4: Run** `cargo test --workspace`.
- [ ] **Step 5: Commit** `feat(core): stored sessions are listed, shown from disk and woken by a message`

### Task 7: Groups, and what the sidebar shows

**Files:**
- Create: `crates/core/src/groups.rs`
- Modify: `crates/core/src/app.rs`, `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `Sessions`, `Entry`, `Mark`, `RECENT_DAYS`.
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
  pub enum GroupId { Folder(PathBuf), Custom(u64) }
  impl GroupId { /// For element keys: "c3" for a custom group, "f" and 16 hex digits for a folder.
                 pub fn key_text(&self) -> String; }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct Group { pub id: GroupId, pub name: Option<String>, pub collapsed: bool, pub hidden: bool }
  pub struct Groups { /* private */ }
  impl Groups {
      pub fn group_of(&self, entry: &Entry) -> GroupId;
      pub fn name(&self, id: &GroupId) -> String;
      pub fn rename(&mut self, id: &GroupId, name: &str);
      pub fn create(&mut self, name: &str) -> GroupId;
      pub fn delete(&mut self, id: &GroupId);
      pub fn place(&mut self, entry: &Entry, group: &GroupId);
      /// A session that had no id when it was placed has one now.
      pub fn adopt(&mut self, key: SessionKey, id: &SessionId);
      pub fn set_collapsed(&mut self, id: &GroupId, collapsed: bool);
      pub fn set_hidden(&mut self, id: &GroupId, hidden: bool);
      pub fn show_older(&mut self, id: &GroupId);
      pub fn toggle_older_groups(&mut self);
  }
  pub struct Row { pub key: SessionKey, pub title: String, pub folder_label: Option<String>,
                   pub mark: Mark, pub shown: bool }
  pub struct Section { pub id: GroupId, pub name: String, pub custom: bool, pub collapsed: bool,
                       pub rows: Vec<Row>, /// Rows left out for their age.
                       pub older: usize, /// The strongest mark among its rows.
                       pub mark: Mark, /// Deleting it would change something.
                       pub can_delete: bool }
  /// One line of the sidebar's list, top to bottom. The numbers index `sections` and its rows.
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Line { Header(usize), Row(usize, usize), ShowOlder(usize), Loading, NewGroup,
                  HiddenGroups, OlderGroups }
  pub struct SidebarModel { pub sections: Vec<Section>, pub lines: Vec<Line>,
                            pub hidden: Vec<(GroupId, String)>,
                            pub older_groups: usize, pub older_groups_open: bool, pub loading: bool }
  pub fn sidebar_model(sessions: &Sessions, groups: &Groups, shown: Option<SessionKey>, now_unix: i64, loading: bool) -> SidebarModel;
  // App gains
  pub groups: Groups, pub sidebar: SidebarModel,
  /// Rebuilds `sidebar` when sessions, groups or the shown session changed since the last call.
  pub fn settle(&mut self);
  pub fn new_session_in(&mut self, folder: PathBuf, group: &GroupId) -> Vec<Effect>;
  pub fn move_session(&mut self, key: SessionKey, group: &GroupId) -> Vec<Effect>;
  pub fn rename_group(&mut self, id: &GroupId, name: &str) -> Vec<Effect>;
  pub fn new_group(&mut self, name: &str) -> (GroupId, Vec<Effect>);
  pub fn delete_group(&mut self, id: &GroupId) -> Vec<Effect>;
  pub fn collapse_group(&mut self, id: &GroupId, collapsed: bool) -> Vec<Effect>;
  pub fn hide_group(&mut self, id: &GroupId, hidden: bool) -> Vec<Effect>;
  pub fn show_older(&mut self, id: &GroupId) -> Vec<Effect>;
  pub fn toggle_older_groups(&mut self) -> Vec<Effect>;
  ```
  The group intents return no effects until Task 8 makes them save.

Rules:

- **Where a session is.** In the group the user placed it in, when that group still exists; otherwise in `GroupId::Folder(its folder)`. A folder's group needs no record: it exists whenever a session's folder is that folder. Placing a session in its own folder's group removes its placement. A copy made by "Continue as a copy" is placed where the original is.
- **Names.** A folder group is named after the folder's last component, or the whole path when it has none, until it is renamed. A custom group created with an empty name is "New group". A name is trimmed; renaming to an empty name gives a folder group its folder's name back and leaves a custom group's name as it was. Renaming never touches a folder on disk.
- **Deleting.** A custom group goes, and its sessions return to their folders' groups. A folder group loses its name, its collapsed and hidden state and every session placed in it from another folder; its own sessions stay, so it stays. `can_delete` is true for a custom group, and for a folder group that has a name of its own or a session from another folder.
- **Which rows.** An entry has a row when it is live, or shown, or touched in this run, or active within `RECENT_DAYS`, or its group was asked to show older. The others count in `older`. A row's title is the session's title, or "New session".
- **The folder label.** A row has `folder_label`, its folder's last component, when its group is custom or is another folder's group.
- **Order.** Sections by the newest `last_active_unix` among their entries, newest first; a section with no entry first, newest created first. Rows by `last_active_unix`, newest first, then by key.
- **Hidden.** A hidden group is in `hidden`, not in `sections`, unless one of its rows is shown or has a mark other than `None`: then it is in `sections` until that is over.
- **Older groups.** A folder group with no row, no record of its own and `older > 0` is counted in `older_groups` and left out of `sections` until `toggle_older_groups`; then such groups come after the others.
- **Marks.** A section's `mark` is `NeedsYou` when any row has it, else `Working`, else `Finished`, else `None`.
- `loading` is true while the first listing has not arrived.
- **Lines.** `lines` is the list as it is drawn: for each section its `Header`, then, unless it is collapsed, its rows and a `ShowOlder` when `older > 0`; then `Loading` while loading; then `NewGroup`; then `HiddenGroups` when `hidden` is not empty and `OlderGroups` when `older_groups > 0`.

- [ ] **Step 1: Write the failing tests** in `groups.rs`, building `Sessions` by hand.
  - `a_session_is_in_its_folders_group_by_default`, with two folders giving two sections named after them.
  - `a_renamed_group_keeps_its_sessions`: `rename(Folder("/home/alex/rune_claude"), "MindRune")`; the section is named "MindRune" and holds the same rows. `rename(.., "  ")` gives "rune_claude" back.
  - `a_session_moves_to_another_group`: placed in a custom group it is there, with `folder_label == Some("rune_claude")`; in its own folder's group the label is `None`; in another folder's group the label is set.
  - `deleting_a_custom_group_sends_its_sessions_home`.
  - `deleting_a_folder_group_resets_it`: renamed, collapsed, with a guest session: after `delete` it has its folder's name, is open, the guest is back home, and its own sessions are still in it.
  - `an_empty_custom_group_comes_first`.
  - `groups_and_rows_are_ordered_by_the_last_message`: three sessions in two folders with given times; the exact order of sections and rows.
  - `old_sessions_are_counted_not_listed`: one entry 31 days old and untouched, one 29 days old: one row, `older == 1`. After `show_older` both have rows and `older == 0`. A touched, a shown or a live old entry has a row.
  - `a_folder_with_only_old_sessions_is_an_older_group`: not in `sections`, `older_groups == 1`; after `toggle_older_groups` it is the last section.
  - `a_hidden_group_shows_while_it_needs_you`: hidden with a `NeedsYou` row it is in `sections`; with no mark and nothing shown it is only in `hidden`.
  - `a_section_carries_its_strongest_mark`.
  - `a_session_without_an_id_keeps_its_place_when_it_gets_one`: placed by key, then `adopt`.
  - `lines_follow_the_sections`: two sections, the second collapsed, the first with `older == 2`, one hidden group: exactly `[Header(0), Row(0,0), Row(0,1), ShowOlder(0), Header(1), NewGroup, HiddenGroups]`.
  - `key_text_is_stable_and_distinct`: two folders differ, the same folder twice is equal, `Custom(3)` is "c3".
  In `app.rs`: `settle_rebuilds_only_after_a_change` (two calls with no change leave the same model; an event that changes a mark changes it), and `a_new_session_in_a_group_is_placed_there`.
- [ ] **Step 2: Run them; they fail.** `cargo test -p mr-core groups`
- [ ] **Step 3: Implement.** `sidebar_model` is one pass over the entries and a sort.
- [ ] **Step 4: Run** `cargo test --workspace`.
- [ ] **Step 5: Commit** `feat(core): groups, and the sidebar's sections and rows`

### Task 8: What is kept between runs

**Files:**
- Create: `crates/core/src/saved.rs`
- Modify: `crates/core/src/app.rs`, `crates/core/src/groups.rs`, `crates/core/src/lib.rs`, `crates/app/src/glue.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const SAVED_VERSION: u32 = 1;
  pub const RECENT_FOLDERS: usize = 12;
  #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
  #[serde(default)]
  pub struct Saved {
      pub version: u32,
      pub groups: Vec<Group>,
      pub placed: Vec<(SessionId, GroupId)>,
      pub next_custom: u64,
      pub recent_folders: Vec<PathBuf>,
      pub shown: Option<SessionId>,
      pub sidebar_width: Option<f32>,
      pub sidebar_collapsed: bool,
  }
  impl Saved { /// What was read, or the default when it is of another version.
               pub fn checked(self) -> Saved; }
  // Launch gains
  pub saved: Saved,
  // Effect gains
  SaveState(Box<Saved>),
  // App gains
  pub recent_folders: Vec<PathBuf>,
  pub fn saved(&self) -> Saved;
  ```
  In `glue.rs`: `const STATE_KEY: &str = "state";`.

Rules:

- `Saved` is written under the one settings key `state`. It is read once at start. A value that cannot be read as `Saved`, or whose version is not `SAVED_VERSION`, counts as none; the app starts with defaults and says one line on stderr.
- Saved groups are only those with something to remember: every custom group, and a folder group that was renamed, collapsed or hidden.
- Placements are saved by session id. A session with no id yet is not saved. A placement whose session is unknown is kept until the complete listing has arrived and dropped then. A placement into a custom group that no longer exists is dropped at once.
- `recent_folders` gets a folder at its front whenever a new session starts there: no duplicates, at most `RECENT_FOLDERS`.
- `shown` is the shown session's id, when it has one.
- **Restoring.** After each listing, when nothing is shown and there was no launch folder, the entry whose id is `saved.shown` is shown, if it is known. After the complete listing this is not tried again.
- **When it is written.** Every group intent, `move_session`, a new session in a folder that changes `recent_folders`, and `close()` return `SaveState`. The glue fills `sidebar_width` and `sidebar_collapsed` from the sidebar's pane state before it writes, and applies both when it starts.

- [ ] **Step 1: Write the failing tests.** In `saved.rs`:
  - `a_saved_state_survives_a_round_trip` through `serde_json`, with a folder group, a custom group, a placement, two recent folders and a shown id.
  - `an_older_or_newer_version_counts_as_none`, `missing_fields_take_their_defaults` (the JSON `{"version":1}` reads as the default with version 1).
  In `app.rs`:
  - `groups_come_back_as_they_were`: build an app, rename a folder group, create a custom group, move a session into it, collapse one; `saved()`; a new app from that `Saved`, given the same listing, has the same `sidebar` sections, names, placements and collapsed states.
  - `only_groups_with_something_to_remember_are_saved`.
  - `a_placement_waits_for_the_complete_listing`: its session is missing from the recent listing and present in the complete one: it is placed. Missing from both: `saved()` no longer holds it.
  - `the_last_shown_session_is_restored_without_a_program`: no `Open` effect, a `ReadHistory` effect, `shown` set.
  - `nothing_is_restored_with_a_launch_folder`, `a_shown_session_that_is_gone_restores_nothing`.
  - `recent_folders_are_newest_first_without_duplicates_up_to_twelve`.
  - `every_group_change_asks_to_save`, `closing_asks_to_save`.
  In `glue.rs`: `a_state_that_cannot_be_read_starts_from_nothing` (a string under `state` in `Prefs::memory()`), and `the_state_is_written_and_read_back` through `Prefs::memory()`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace`.
- [ ] **Step 5: Commit** `feat: groups, recent folders and the shown session are kept between runs`

## Rune facts for the window tasks

Read from Rune's source at the pinned revision on 2026-11-13. File names are under `crates/rune-kit/src` unless they start with `rune/`. Check each against the source before relying on it. The full survey, with a file and line for every statement, is `.superpowers/plan-3-research/rune-survey.md` (not in git; it also covers what plan 4 needs: dragging, dividers and focus between tiles).

- **The sidebar list.** `Sidebar::virtual_rows(count, key, build)` (`shell/sidebar.rs:113`) builds only the rows near the viewport and needs one `row_height` for every row. It replaces any sections and children, so a grouped list is flattened: headers are rows of the same list. The kit's own test runs 5,000 rows. `SidebarSection` builds every row on every commit and its header cannot be renamed.
- **Rows.** `SidebarRow` has `.selected`, `.detail` (faint trailing text), `.status(RowStatus::{Busy, Attention, Unread, Other(El)})`, `.on_select`, `.renaming(&TextEdit, access, commit, cancel)` (Return commits, Escape cancels; nothing ends it when the field loses focus). Key the built row yourself. The list is one Tab stop; Up and Down move the selection and call `on_select`.
- **Headers.** `SidebarHeader` has `.collapsed`, `.count`, `.action(El)`, `.on_toggle`, no rename. An `action` with its own press handler takes its own presses.
- **Menus.** `context_menu(key, trigger, point)` and `Menu` (`menu.rs`) draw in the window and work on Linux; native pop-up menus do not. `El::on_secondary_press` gives the `PointerPress` to open at; the Menu key and Shift+F10 call it for the focused element. `Menu::at(x, y)`, `.item(MenuItem)`, `.separator()`, `.on_close`. `MenuItem::submenu(entries, open)`: the app owns `open`; nothing opens a submenu on hover or Right arrow. Keys: `{key}-menu`, `{key}-item{i}` (separators count), a submenu's items `{key}-item{i}-sub-item{j}`. An open menu is modal: Escape and a press outside call `on_close`.
- **The folder picker.** `tx.open_folder(FileDialogOptions { title, directory, .. }, |state, result, tx| ..)` (`rune/file_dialog.rs:261`); `Ok(None)` is cancel. It needs the `native-file-dialog` feature of `rune`, which on Linux uses the desktop portal over `zbus`, already in `Cargo.lock`. One dialog at a time. Tests: `rt.file_dialog()` and `rt.resolve_file_dialog(Some(path) | None)`; the callback runs on the next frame. `rune::dirs::AppDirs::home()` is the real home folder even under `RUNE_DIRS_ROOT`.
- **Keys.** Identity is a path, so equal keys are fine when they are not siblings. But overlay anchors, `tx.focus(key)` and `rune::memo` ids are looked up by plain key across the window, first match. A memo id used twice builds uncached, and a memo can be served another session's cached row when ids and versions coincide. So every memo id and every row version in the conversation carries the session's key.
- **Focus.** There is no focus-change callback on elements. Rune keeps a field's `TextEdit::focused` up to date on every commit. Ctrl+N reaches `AppSpec::on_key` even while a text field has focus, as `Key::Command('n')`.
- **Settings.** `Prefs::set(key, &value)` stores any `serde` value, rewrites the whole file and syncs it on the calling thread. A value of another shape reads back as `None`.
- **Test input.** There is no `rt.tap`. A right-click is `rt.secondary_down(x, y)`, `rt.secondary_up(x, y)`, then a frame. `testkit.rs` composes clicks from `pointer_down` and `pointer_up`.

### Task 9: The window draws the session it is given

**Files:**
- Modify: `crates/app/src/glue.rs`, `crates/app/src/view/mod.rs`, `conversation.rs`, `items.rs`, `requests.rs`, `input.rs`, `width.rs`, `crates/app/src/testkit.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: `App` of Tasks 5 to 8.
- Produces:
  ```rust
  // glue.rs
  /// How one session is shown: everything the window remembers about it that the core does not.
  pub struct SessionView {
      pub draft: TextEdit,
      pub open_picker: Option<Picker>,
      pub toggled: HashSet<ItemId>,
      pub whole: HashSet<ItemId>,
      pub answers: HashMap<RequestId, Vec<QuestionDraft>>,
      pub following: bool,
      /// The session's `generation` these were made for.
      pub generation: u64,
  }
  // Ui: the loose fields above go; it gains
  pub views: HashMap<SessionKey, SessionView>,
  pub fn view_of(ui: &Ui, key: SessionKey) -> &SessionView;        // a shared default when absent
  pub fn view_mut(ui: &mut Ui, key: SessionKey) -> &mut SessionView; // made when absent
  // view/mod.rs
  /// The conversation area for one session: its conversation and its input box.
  fn pane(ui: &Ui, cx: &mut Cx<Ui>, kit: &'static Kit, key: SessionKey, layer: &mut Vec<El>) -> El;
  // testkit.rs
  pub fn add_session(rt: &mut Runtime<Ui>, folder: &str) -> SessionKey;
  pub fn show(rt: &mut Runtime<Ui>, key: SessionKey);
  pub fn emit_to(rt: &mut Runtime<Ui>, key: SessionKey, event: SessionEvent);
  ```
  Every view function under `pane` takes the `SessionKey` and reads the session and its `SessionView` through it. None reads `ui.app.shown`.

Rules:

- `glue::run` ends by dropping the `SessionView` of every session that is gone, clearing `toggled`, `whole` and `open_picker` of a view whose `generation` is behind its session's, and calling `App::settle`.
- The conversation's scrolling element is keyed `mr-conversation-{key}`, so each session keeps its own place. Every memo id and every row version under it includes the session's key and `generation`. The version of the virtual list is made from both.
- A session shown for the first time starts at its end, with its input box focused. Showing a session again leaves its place and its draft as they were.
- Escape stops the turn of the shown session. Enter sends to the session whose input box it was typed in.
- Closing the window: every `Close`, one bounded wait for all of them, then the saved state is written.
- In the demo, a scene may hold several sessions; `emit` goes to the shown one, `emit_to` to the one named.

- [ ] **Step 1: Write the failing tests** in `glue.rs` and `view/mod.rs`, with the demo's `empty` scene and `add_session`.
  - `each_session_keeps_its_own_draft`: type "abc" in A; show B: the box is empty; type "xyz"; show A: the box holds "abc".
  - `switching_never_shows_the_other_sessions_rows`: A and B each hold one reply as item 0, with different text. After `show(B)` the window has B's text and not A's; after `show(A)`, the other way round. Run it twice in a row to cross the memo.
  - `opened_rows_belong_to_their_session`: open a tool group in A; the group with the same item id in B is closed.
  - `a_question_draft_belongs_to_its_session`.
  - `events_for_a_hidden_session_do_not_draw`: while A is shown, a reply streams into B: no text of B's is in the window, and A's rows are not rebuilt (the memo count is unchanged).
  - `history_that_replaces_a_conversation_forgets_opened_rows`.
  - `a_session_that_is_gone_leaves_no_view`.
  - `closing_ends_every_session_and_saves`: three live sessions; after `close_session` the fake has three `Close`, and the settings hold a `state`.
  - Every plan 2 window test still passes.
- [ ] **Step 2: Run them; they fail.** `cargo test -p mindrune`
- [ ] **Step 3: Implement.** Change no wording and no layout.
- [ ] **Step 4: Run** `cargo test --workspace`.
- [ ] **Step 5: Commit** `refactor(app): every view draws the session it is given`

### Task 10: The sidebar lists groups and sessions

**Files:**
- Create: `crates/app/src/view/sidebar.rs`
- Modify: `crates/app/src/view/shell.rs` (the session row goes; the shell and the footer stay), `crates/app/src/view/mod.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: `App::sidebar` with its `lines` (Task 7); `App::show`, `collapse_group`, `show_older`, `toggle_older_groups`.
- Produces: `pub fn list(ui: &Ui, kit: &Kit) -> El` in `sidebar.rs`, used by `shell::sidebar`; the demo scene `sessions`.

What is drawn, one element per `Line`, all of one height, as rows of `Sidebar::virtual_rows`:

| Line | Key | Shows | Press |
| --- | --- | --- | --- |
| `Header(s)` | `mr-group-{key_text}` | A chevron, the group's name, and, when it is collapsed, its mark | Folds or unfolds the group |
| `Row(s, r)` | `mr-row-{n}` | The title; the folder label as faint text at the end; the mark | Shows the session |
| `ShowOlder(s)` | `mr-older-{key_text}` | "Show older (N)" in muted text | `show_older` |
| `Loading` | `mr-sessions-loading` | "Loading sessions…" in muted text | Nothing |
| `OlderGroups` | `mr-older-groups` | "Older groups (N)", or "Hide older groups" while they show | `toggle_older_groups` |

`NewGroup` and `HiddenGroups` are Task 11; until then they draw nothing and have no height in the list (leave them out of the count).

Marks: `Working` is `RowStatus::Busy`, `NeedsYou` is `RowStatus::Attention`, `Finished` is `RowStatus::Unread`. The shown session's row is `selected`. A header is built by hand from the kit's parts (copy `SidebarHeader::build`), because Task 11 puts a text field in it.

The demo scene `sessions` has: a group "rune_claude" with three sessions, one working, one that needs the user, one finished and unseen; a custom group "Side projects" with one session from another folder; a collapsed group with a working session; a group with "Show older (4)"; and two older groups.

- [ ] **Step 1: Write the failing tests** in `sidebar.rs`, on the `sessions` scene.
  - `groups_and_their_sessions_are_listed`: the names and titles appear in the model's order.
  - `a_row_shows_its_sessions_mark`: each of the three marks, and none for an idle seen session.
  - `clicking_a_row_shows_that_session`: `shown` changes, the conversation shows its text, and the row is selected.
  - `a_finished_mark_goes_when_the_session_is_shown`.
  - `a_turn_in_a_hidden_session_changes_only_its_row`: with `emit_to`, the row's mark becomes busy and the conversation on screen is unchanged.
  - `a_request_in_a_hidden_session_waits_for_the_user`: a permission request arrives in B while A is shown: B's row shows the attention mark, no card is on screen, typing and Enter in A's input box send nothing to B, and after a click on B's row its card is there with its buttons.
  - `folding_a_group_hides_its_rows_and_shows_its_mark`.
  - `a_row_in_another_group_names_its_folder`.
  - `show_older_lists_the_rest`, `older_groups_can_be_shown_and_hidden`.
  - `a_long_list_builds_only_what_is_on_screen`: 2,000 stored sessions in one group with show older on: fewer than 200 row elements are built.
  - `arrow_keys_move_through_the_rows`: with the list focused, Down shows the next session.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace`, then look at `cargo run -p mindrune -- --demo --scene sessions --snap target/snap/t10-sessions.png` in both themes.
- [ ] **Step 5: Commit** `feat(app): the sidebar lists sessions under their groups, with marks`

### Task 11: Menus for rows and groups

**Files:**
- Modify: `crates/app/src/view/sidebar.rs`, `crates/app/src/glue.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: `move_session`, `rename_group`, `new_group`, `delete_group`, `hide_group`, `new_session_in` (Tasks 7, 8).
- Produces:
  ```rust
  // glue.rs
  pub enum MenuTarget { Row(SessionKey), Group(GroupId), HiddenGroups }
  pub struct SideMenu { pub target: MenuTarget, pub at: PointerPress, pub sub_open: bool }
  // Ui gains
  pub side_menu: Option<SideMenu>,
  /// The group whose name is being edited, and the field.
  pub renaming: Option<GroupId>, pub rename: TextEdit,
  ```
  One menu, keyed `mr-side-menu`, is built only while `side_menu` is set.

The menus, with exact wording:

- **On a row** (right-click, the Menu key, Shift+F10): "Move to group", which opens a submenu when chosen. The submenu lists every group but the row's own, by name, in the sidebar's order, hidden groups included; then a separator and "New group…". Choosing a group moves the session. "New group…" creates a group, moves the session into it and starts renaming it.
- **On a group's header:** "New session", "Rename", "Hide group", and, in the danger colour, "Delete group". For a folder group "New session" starts one in its folder; for a custom group it opens the folder menu of Task 12 (until Task 12, it is disabled). "Delete group" is disabled for a folder group that has nothing to reset.
- **The `NewGroup` line** (`mr-new-group`, "New group" with a plus icon, muted): creates a group and starts renaming it.
- **The `HiddenGroups` line** (`mr-hidden-groups`, "Hidden groups (N)"): opens the menu with one item per hidden group; choosing one shows it again.
- Each header also has a "+" button at its end (`mr-group-add-{key_text}`), which does what "New session" does and does not fold the group.

Renaming: the header's name becomes a one-line field holding the name, all selected and focused. Return keeps the name, Escape drops the change, and so does starting another rename. When the field loses focus for any other reason the name typed so far is kept. A click on the field does not fold the group.

- [ ] **Step 1: Write the failing tests.**
  - `a_right_click_on_a_row_opens_its_menu`, and Escape closes it without a change.
  - `move_to_group_lists_the_other_groups`: the row's own group is not among them.
  - `choosing_a_group_moves_the_session`: the row is under the other header, with its folder label, and a `state` was saved.
  - `new_group_from_a_row_moves_it_and_starts_renaming`.
  - `a_group_is_renamed_from_its_menu`: type "MindRune", Return: the header reads "MindRune", the sessions are the same, no folder was touched. Escape leaves the old name. Clicking another row keeps what was typed.
  - `two_groups_can_have_the_same_name`: both are drawn, no duplicate-key warning.
  - `hiding_a_group_removes_it_and_hidden_groups_brings_it_back`.
  - `deleting_a_custom_group_sends_its_sessions_home`, `a_folder_group_with_nothing_to_reset_cannot_be_deleted`.
  - `the_plus_on_a_folder_group_starts_a_session_there`: the new session is shown, is in that group, and the group did not fold.
  - `the_menu_key_opens_the_menu_of_the_focused_row`.
  - `menu_items_never_answer_a_request`: with a permission card waiting in the shown session, opening and using the menu sends no `Answer`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.** Try one `Menu` placed with `.at(x, y)` and no trigger first; if the kit needs a trigger, wrap only the element under the pointer. Say in the report which it was.
- [ ] **Step 4: Run** `cargo test --workspace`; snapshot the scene `group-menu` (a row's menu open on "Move to group").
- [ ] **Step 5: Commit** `feat(app): move sessions between groups; rename, create, hide and delete groups`

### Task 12: A folder for each new session

**Files:**
- Create: `crates/app/src/view/folder.rs`
- Modify: `crates/app/Cargo.toml` (feature `native-file-dialog` on `rune`), `Cargo.lock`, `crates/app/src/main.rs`, `crates/app/src/view/shell.rs`, `sidebar.rs`, `mod.rs`, `crates/app/src/glue.rs`, `crates/core/src/app.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Produces:
  ```rust
  // app.rs
  /// Folders to offer for a new session: the recent ones, then the folders of the newest
  /// sessions, each once, at most `RECENT_FOLDERS`.
  pub fn folder_choices(&self) -> Vec<PathBuf>;
  // glue.rs: Ui gains
  /// The folder menu is open, for this group when one is named.
  pub folder_menu: Option<Option<GroupId>>,
  /// Why the folder picker could not be used, shown under the button until the next try.
  pub folder_error: Option<String>,
  ```
  `main.rs`: `mindrune [FOLDER] [--demo]`; the folder is optional and no longer defaults to the current one.

Rules:

- "New session" (`mr-new-session`) and Ctrl+N open the folder menu (`mr-folder-menu`) under the button. It lists `folder_choices()` that still exist as folders: the folder's name, with its whole path as the item's description. Then a separator and "Choose a folder…". With no choices, the button opens the picker directly.
- Choosing a folder starts a new session there and shows it, with its input box focused. For a custom group's "New session" and "+" the same menu opens and the session is placed in that group.
- "Choose a folder…" opens the desktop's folder picker, titled "Choose a folder for the new session", starting in the shown session's folder or the home folder. Cancel does nothing. An error shows "The folder picker could not be opened: {error}" (`mr-folder-error`) under the button.
- With no shown session the conversation area (`mr-empty`) reads "No session is open." and, under it, "Choose a folder to start one.", with a "New session" button that does the same as the sidebar's.
- `mindrune FOLDER` starts a new session in that folder. `mindrune` restores (Task 8) or shows the empty area.

- [ ] **Step 1: Write the failing tests.** In `app.rs`: `folder_choices_are_recent_first_then_from_sessions`. In `folder.rs` and `main.rs`:
  - `new_session_offers_the_recent_folders`: names and paths, in order; a folder that no longer exists is not offered.
  - `choosing_a_recent_folder_starts_a_session_there`: an `Open` for that folder; the session is shown and in the folder's group; the folder is now first among the recent ones.
  - `choose_a_folder_opens_the_picker`: `rt.file_dialog()` is a folder dialog with the title; `resolve_file_dialog(Some(dir))` starts a session in `dir`; `None` starts nothing.
  - `with_no_recent_folder_the_button_opens_the_picker`.
  - `a_picker_error_is_shown_under_the_button`.
  - `ctrl_n_opens_the_folder_menu`, also while the input box has focus.
  - `a_custom_groups_plus_asks_for_a_folder_and_places_the_session`.
  - `with_nothing_shown_the_window_says_how_to_start`.
  - `the_command_line_folder_is_optional`: `parse(&[])` has `folder: None`; a folder that does not exist is still an error.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.** Check that `cargo tree -p mindrune --target x86_64-unknown-linux-gnu` gains no crate that was not already built; say in the report what `Cargo.lock` gained.
- [ ] **Step 4: Run** `cargo test --workspace`; snapshot the scenes `folders` (the menu open) and `empty-start`.
- [ ] **Step 5: Commit** `feat(app): a new session starts in a folder the user chooses`

### Task 13: A stored session on screen

**Files:**
- Create: `crates/app/src/view/resting.rs`
- Modify: `crates/app/src/view/mod.rs`, `conversation.rs`, `items.rs`, `input.rs`, `crates/app/src/demo.rs`

**Interfaces:**
- Consumes: `HistoryState`, `Phase::Resting`, `Prompt`, `PromptChoice`, `App::answer_prompt`, the two new `NoticeKind`s.
- Produces: the demo scenes `resting`, `history-loading`, `history-failed`, `elsewhere`.

What is drawn, with exact wording:

| State | Key | Shows |
| --- | --- | --- |
| History `Loading` | `mr-history-loading` | "Loading the conversation…" in the middle of the conversation area |
| History `Failed(reason)` | `mr-history-failed` | "This conversation could not be read." and under it the reason, in muted text |
| `EarlierLeftOut` notice | the item's row | "Earlier messages are not shown." in muted text, centred |
| `Compacted` notice | the item's row | "Earlier messages were summarised here to save room." in muted text, centred |
| `Prompt::OpenElsewhere` for this session | `mr-elsewhere` | A card at the end of the conversation: "This session may be open in another app", then "Its history changed less than two minutes ago. Continuing here adds to the same session.", then the buttons "Continue here", "Continue as a copy", "Cancel" |

Rules:

- The card's buttons follow the request buttons' rule: they never take focus and no key presses them.
- Cancel puts the message back into that session's input box, at the end of what is typed there, and focuses it.
- A resting session's input box works as an idle one's. Its pickers work when there are models to choose from; they show the model and mode the session will start with. With no models known the model picker shows the model's id when the history named one, else "Model", and is disabled.
- While a resting session wakes, the message shows as queued, as in plan 2, and the pickers are disabled.
- A session whose history failed can still be sent to; what the program then says is shown as any other error.

- [ ] **Step 1: Write the failing tests.**
  - `a_stored_session_shows_its_history`: user message, reply and a tool line from the scripted history; no program was opened (`fake.sent()` is empty and no `Open` happened).
  - `loading_and_failure_are_said_plainly`, with the two wordings.
  - `the_two_notices_are_drawn`.
  - `the_pickers_show_what_the_session_will_start_with`: the history's model through its option's label, the history's mode; choosing another mode before any message sends nothing and changes the label.
  - `sending_wakes_the_session`: Enter queues the message and opens the program resuming the id, with the chosen mode in the spec; after `Started` the message is sent.
  - `a_session_open_elsewhere_asks_before_sending`: the card appears, nothing was opened, the input box is empty.
  - `cancel_puts_the_message_back`, `continue_here_sends_it`, `continue_as_a_copy_shows_a_new_row_in_the_same_group`.
  - `no_key_presses_the_cards_buttons`: Tab never reaches them; Enter and Space in the input box do not choose.
  - `a_very_long_history_scrolls_smoothly`: 2,000 items: the first frame after `show` builds fewer than 100 rows.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace`; snapshot the four scenes in both themes.
- [ ] **Step 5: Commit** `feat(app): stored sessions open from disk and ask before joining a live one`

### Task 14: Screens, the real path, notes, and Jerod's look

**Files:**
- Modify: `crates/app/tests/snapshots.rs`, `crates/app/tests/replay.rs`, `crates/app/tests/live.rs`, `crates/claude/tests/` (as needed), `docs/notes-for-later-plans.md`, `docs/protocol-notes.md`

- [ ] **Step 1: Snapshots.** The snapshot test also writes `sessions`, `group-menu`, `folders`, `empty-start`, `resting`, `history-loading`, `history-failed` and `elsewhere`, in both themes, and checks each for `mr-root` and for no duplicate-key warning.
- [ ] **Step 2: Replay.** `replay.rs` keeps passing through the new `App`. Add: each recording is also played into one of three sessions at once, interleaved line by line, and each session ends exactly as it does alone.
- [ ] **Step 3: The live test.** `live.rs` is opt-in and no agent runs it. It keeps what it checks and then also prints, without failing on any of these:
  - `live: listed N recent sessions in X ms, M in all in Y ms`
  - `live: read the largest recent history (K items) in Z ms`
  - `live: the look gave N models; usage known: yes|no`
  and then, as a check that does fail the test: after the first turn it closes the session, reads it back from the store (the first message and the reply are in the history), wakes it with a second short message, and gets a reply. It prints `live: resumed with <model label> in <mode label>`.
- [ ] **Step 4: Run** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`. Commit `test: screens with many sessions, the replay across sessions, the live test resumes`.
- [ ] **Step 5: Notes.** In `docs/notes-for-later-plans.md`: a section "Jerod's look at plan 3" with the commands and the list below; what this plan left; what plan 4 must know (keys per session, the pane function, `SessionView`, the drag that is still missing). In `docs/protocol-notes.md`: the facts about stored sessions from this plan, and whatever Task 2 learned. Commit `docs: plan 3's notes`.
- [ ] **Step 6: Stop for Jerod.** Jerod runs, in their own terminal:
  ```bash
  MINDRUNE_LIVE=1 cargo test -p mindrune --test live -- --ignored --nocapture
  ```
  ```bash
  cargo run --release -p mindrune
  ```
  What to try: the sidebar fills with groups, and how long that takes; an old session opens at once and shows its end; a message to it gets a reply and the model shown is the one that answers; two sessions work at the same time and their marks follow; "New session" offers folders and the picker opens; a session is moved to a new group, the group renamed, the app closed and opened again, and everything is where it was; a session that is open in Claude Desktop asks before it is sent to. Record what Jerod reports. A defect in something this plan built is fixed in this plan.
