# A Deleted View, Status Colours and Notifications: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A session or a group can be deleted: it moves into a Deleted view, can be recovered from there for a week, and is then removed for good, from Claude Code's history too. A session that is ready shows green on its tile's title bar and on its row, and one that needs input shows orange, until the user has dealt with it. And when the user is in another app, or the window is closed, a desktop notification says so and the tray icon carries a green or orange marker.

**Architecture:** Deleting is a mark, kept in the app's saved state by the agent's session id, so nothing leaves the disk until a week has passed or the user says "Delete All"; then the adapter removes that session's own files, which is the first and only thing this app ever changes in Claude Code's folder. "Ready" stops being "a turn ended in a tile that was not focused" and becomes "a turn ended while the user was not looking at it", which needs the one thing the app never tracked: whether its window has the keyboard. `mr-core` decides when a notification is due and when it is taken back, as effects; a small D-Bus client on its own thread shows them, built on the `zbus` that Rune's tray already brings into the build.

**Tech Stack:** Rust 1.99.1 (edition 2024), `rune` and `rune-kit` at the pinned revision, `serde`, `serde_json`, std threads and channels, `tempfile` for tests. On Linux the app crate names `zbus` 6 and `tokio` 1 in its own manifest, both already in `Cargo.lock` through Rune: nothing is downloaded and no package is added to the lock.

**Spec:** `docs/superpowers/specs/2026-11-10-claude-code-ui-design.md`, section 5 on the sidebar (lines 86 to 118: groups, and "Deleting a group sends its sessions back", which this plan changes) and section 6, which left "Desktop notifications when a session needs the user" for later. The rest is Jerod's request of 2026-11-17, quoted in `docs/notes-for-later-plans.md` under "What plan 8 is for". Read the notes' section first, then this plan.

**For the controller:** Jerod's rules: every subagent (implementer, reviewer, fixer) runs on Opus 5.5 at high effort or better; every commit ends with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>` and has a short message. Two research reports made for this plan are on disk and not in git: `.superpowers/sdd/plan-8-research/code-as-built.md` (the code on `main` at `8fece6e`, with file and line for everything this plan touches) and `.superpowers/sdd/plan-8-research/store-and-notifications.md` (what Claude Code keeps on disk for one session; desktop notifications on GNOME; what Rune's tray can and cannot do). Give each implementer the report its task needs. Where a report and the code differ, the code wins.

## What Jerod asked for

In their words, 2026-11-17: "I can't delete conversations or groups, I think that should be included. Along with alerts? I would like to make them more colorful than Claude Code, it only shows a little dot when a session is done. I want the entire name on the left to turn green when it's ready."

Asked what deleting means: "I think instead of permanent, there should be a menu at the top for Deleted messages/groups, that switches the sidebar to show those deleted ones. That menu has a 'Delete All' option, and every minute or so it permanently deletes those messages if they're older than a week. Groups/sessions from there can be recovered and moved back into the main view. ... Sort of like deleted text messages on an iphone." They agreed that a deleted session stays visible in Claude Code's own lists until the week is up.

On colours: "If I'm in another tile, I want the tile that is ready to have a nice green colored title background. When I click on that tile, the green color goes away back to it's current grey. Same with the entry on the left side." And: "Let's go with orange if a session needs input. That matches the mind rune icon colors."

On alerts in another app: a desktop notification together with a coloured tray icon "would work".

## Not in this plan, on purpose

Removing a single item for good from the Deleted view before its week is up (there is "Delete All"). A sound. A setting to turn notifications off (the desktop's own notification settings do that for MindRune). Notifications on macOS and Windows. Deleting from the keyboard with a Delete key. Changing the words "Working", "Needs you" and "Finished". A change to Rune so that a tray icon can change in place.

## What rests on what was never seen

| Point | Read where | Seen working | Made sure by |
| --- | --- | --- | --- |
| What Claude Code keeps on disk for one session: the transcript, a folder beside it, `file-history/<id>`, `session-env/<id>` | names in Jerod's own store (the research report) | the names, yes; that nothing else refers to a session, no | the live test removes the sessions it made and prints what is still named by their ids |
| A notification with the hint `desktop-entry = "mindrune"` shows MindRune's name and icon | GNOME Shell's source | no | Jerod's look |
| A click on a notification sends an activation token and then the action, and the token brings the window to the front | GNOME Shell's source | no | Jerod's look |
| A notification can be taken back and replaced over the connection that sent it | GNOME Shell's source | no | Jerod's look |
| The window hears when it gains and loses the keyboard on Wayland (`App::on_focus`) | Rune's and winit's source | no | Jerod's look |
| A tray icon made anew with a marker shows without a long gap | Rune's source (it cannot change an icon in place) | no | Jerod's look |

## What this window will not do yet

Say this to Jerod before their look.

- **A deleted session is still in Claude Code's own lists for its week.** `claude --resume` and Claude Desktop show it until MindRune removes it.
- **A deleted session that is used again elsewhere comes back.** If it is continued from a terminal during the week, MindRune puts it back in the normal list and does not remove it.
- **Deleting a session that is working stops it.** The app asks first.
- **Claude Code clears out old sessions by itself** (about thirty days, by its own setting). A session may be gone before its week in the Deleted view is over; MindRune takes that as done.
- **The tray icon blinks when its marker changes.** The toolkit cannot change a tray icon in place, so the icon is taken away and put back. If that is too restless, say so.
- **Notifications need the desktop entry installed.** Without it they show without MindRune's name and icon.
- **A notification shows only when you are not looking at MindRune.** With the window in front, the colours do the job.
- **Notifications are Linux only**, and there is no sound.
- The row and the title bar stay green until you click the tile or the row, or type in it. Coming back to the window does not clear them.

## Decisions in this plan that Jerod has not seen

Each is the planner's call, listed in the handover for review.

**Deleting**

1. **"Delete" on a resting session acts at once, without a question**: it can be recovered for a week. It asks first only when it would stop something: a session whose turn runs, that waits for input, or that holds a message not yet delivered.
2. **"Delete group" always asks**, and says how many sessions go with it. It takes every session of the group, also the older ones the list does not show.
3. **A group you made yourself keeps "Ungroup"**, which is what "Delete group" did until now: the group goes and its sessions return to their folders' groups. "Delete group" now moves the group and its sessions to the Deleted view.
4. **A folder's group can be deleted too.** Its sessions go to the Deleted view and the group leaves the list because nothing is left in it. A new session in that folder brings the group back, empty of the deleted ones.
5. **The switch is two words at the top of the sidebar**, "Sessions" and "Deleted (3)", above "New session". The Deleted view has one line under it: "Removed for good after 7 days." and the button "Delete All".
6. **The Deleted view lists the same groups as the normal list**, holding only what was deleted. Each row says how long it has left ("6 days left", "1 day left", "today"). A press on a row or a group opens its menu: "Recover" or "Recover group".
7. **Recovering a session whose group was deleted brings the group back with it**; the group's other sessions stay deleted until they are recovered too.
8. **A deleted session is not shown in a tile.** Deleting closes its tile, and a deleted session cannot be opened until it is recovered.
9. **"Delete All" asks first**, saying how many sessions will be removed for good and that it cannot be undone.
10. **What is removed for good**: the session's transcript, the folder of the same name beside it, and its folders under `file-history` and `session-env`. Nothing shared is touched: not Claude Code's history of typed prompts, not its settings, not the project's folder.
11. **A session whose file changed after it was deleted is not removed**: it is recovered, as one used again elsewhere. One that is already gone counts as removed.
12. **A removal that fails is tried again an hour later**, not every minute, and its row says "Could not be removed".
13. **Messages waiting in a deleted session are dropped** when it is deleted: a queued message and one its program never got. That is what the question in decision 1 warns of.
14. **The app reads Claude Code's folder where Claude Code itself looks**: `CLAUDE_CONFIG_DIR` when it is set, else `~/.claude`. Until now the app ignored the variable for reading while passing it on to the program. A delete must never look in another place than the program writes to.
15. **A session that was never sent a message is simply forgotten** by "Delete": nothing of it is stored.

**Colours**

16. **Green and orange are the icon's own**: the green of its hexagon with light text, the orange of its rays with dark text, the same in the dark and the light theme. Both pairs keep a contrast of at least 4.5 to 1.
17. **The whole title bar and the whole row take the colour**: the title, the folder's chip, the state word and the close button are drawn in the pair's text colour.
18. **Orange wins over the selection and over green.** A row that is selected and ready is green.
19. **The outline of a tile that needs input turns from amber to the same orange.** The usage warnings keep their amber.
20. **A collapsed group shows a green or orange dot** for the strongest state among its sessions, in place of today's blue and amber dots. A working session keeps its small moving mark.
21. **"Ready" now also covers the tile you were in when you left for another app.** A turn that ends while MindRune does not have the keyboard marks its session ready, also in the focused tile.
22. **Ready is cleared only by your own act on that session**: a press on its tile or its row (also when it is already the focused one), moving the keyboard into it, typing or sending in it, answering in it, dragging its tile, or a press on its notification. It is no longer cleared when the window comes back, or when another tile closes and the focus falls on it. This replaces plan 7's rule that the focused session counts as seen when the window returns.
23. **A failed turn and a program that died count as ready**, as they count as "finished" today: the session wants a look.

**Notifications and the tray**

24. **A notification is sent when a session becomes ready or starts to need input while you are not looking at MindRune**: its window is closed, or another app has the keyboard. Never while MindRune's window has the keyboard, also not for a session in another tile.
25. **Its title is the session's name; its text is "Finished." or "Needs your input."** One notification for each session: a new one for the same session replaces the old.
26. **A press on it brings the window forward and focuses the session**, opening it in the focused tile if it is in none. That press clears "ready".
27. **Notifications are taken back** when the session is dealt with, and all of them when MindRune's window gets the keyboard again.
28. **The tray's marker**: a filled circle at the icon's lower right, orange while any session needs input, else green while any is ready, else none. Deleted sessions never count.
29. **Where there is no notification service, or the app runs as the demo or without a window system, nothing is sent** and nothing else changes.

## Global Constraints

- Rust toolchain is exactly `1.99.1`, edition `2024`. In an agent's shell cargo is not on `PATH`: start every command with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Rune is pinned to `rev = "2a44156b21d63b0342a366b26d9222a6edd4622c"`. Its source is at `~/.cargo/git/checkouts/rune-c9b805e9ed8a3730/2a44156`; read it instead of guessing an API. Where this plan and the source differ, the source wins: say so in your report.
- **One exception to "never writes inside Claude Code's folder", and only one.** The adapter's `remove_session` removes the files of one session, named by its id, and nothing else. No other code path creates, writes, renames or removes anything under that folder. No test and no agent ever calls it on the real store: tests build a store in a `tempfile` folder. The user's Claude Code settings are never changed.
- `remove_session` takes an id only when it is a UUID in its usual written form (36 characters: hexadecimal digits and four dashes at their places). It never follows a symbolic link: a link in the place of a session's file or folder is removed as a link, never what it points to. It removes only paths whose last component is that id, or that id with `.jsonl`.
- No package is added to `Cargo.lock` and nothing is downloaded. `zbus` and `tokio` are named in `crates/app/Cargo.toml` for Linux only, `default-features = false`, with features that are a subset of what Rune already turns on; only the `mindrune` entry of the lock may change. `mr-core`, `mr-agent` and `mr-claude` gain no dependency.
- `mr-agent` and `mr-core` depend on neither `mr-claude` nor `rune`. No type from `mr-claude` appears in `mr-core` or in any view. Only `crates/app/src/main.rs`, `crates/app/tests/live.rs` and `crates/probe` may name `ClaudeAgent`.
- The UI thread never blocks and reads or removes no file. Removing runs on the library thread. `Tray::new` and every D-Bus call run on their own threads.
- The notifier's thread and the tray's thread touch `Ui` only through `AppHandle::update`, and call nothing else on the handle but `open`, `activate` and `wake`.
- In `--demo`, and whenever `rune::is_headless()` is true, the app starts no notifier and no tray, and sends no notification. No test sends a desktop notification or starts a tray.
- An activation token is passed on and never printed or logged. A session's title goes into a notification's text with `&`, `<` and `>` escaped.
- `SAVED_VERSION` stays 1. Every field added to `Saved` or to `Group` has a serde default and reads anything it cannot read as its default, so that a bad value costs only itself (`share_or_one` is the model). `SAMPLE_JSON` is updated.
- A deleted session is never started, never restarted, never shown in a tile, never counted in the tray's standing or in the quit question, never marked, and never the subject of a notification. These are checked in one place each, named in Task 2.
- A session's events are trusted only while they carry the number of that session's current open, with the one exception plan 7 made for a failed send.
- Every session intent of `App` takes the `SessionKey` it acts on. Every `App` intent a view calls is followed by `glue::run`.
- Never assign `App.sessions`, `App.groups` or `App.tiles` wholesale after `App::new`.
- Every key this app gives an element starts with `mr-`.
- No Anthropic logos, colours or fonts.
- User-visible wording is exact where this plan quotes it. Never use a real person's name or e-mail address as test data: `alex`, `/home/alex`. Never copy text out of a real stored session or out of `target/rec/` into a test or a fixture.
- All window tests and all screenshots use `FakeAgent`, through the demo window. No agent starts the real `claude`, sets `MINDRUNE_LIVE`, runs a test with `--ignored`, runs `scripts/install-desktop-entry.sh` outside a test's temporary folder, or starts the app outside `--demo`.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` (no warnings) and `cargo test --workspace` pass before every commit. Say in the report which existing tests you changed and why.
- **Git.** Work on the branch `plan-8-deleted-colours-alerts`, cut from `main`. Never push. Never commit `target/` or `.superpowers/`. Commit by explicit path. Another session of Jerod's may work in the same checkout: never touch, stash, reset or commit a file that is not yours. Keep commit messages short. End every commit message with a blank line and `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`, in exactly that form.
- Version of `claude` this plan was written against: `2.1.323`.

## Review Focus

The request implies these conditions and no screen shows them. Each has a test in the task named.

1. **A session deleted while something is in flight.** Its turn runs, a permission request waits, a message is queued or was never delivered, Remote Control is on or asked for, its program is being stopped for idleness, its history or a picture of it is being read, its picture is open in the viewer. Expected: its program is told to close once, it is never started again, its tile and its picture close, no mark and no notification come from it, and its file is not removed while its program lives. Tests: Tasks 2 and 4.
2. **The moment of removal.** The file changed since the session was deleted; the file is already gone; the id is not a UUID; a symbolic link sits where the session's folder should be; a path cannot be removed; the app quits or the window closes while removals are queued. Expected: a changed session is recovered, a missing one counts as removed, nothing outside the session's own paths is ever touched, a failure is retried after an hour and not every minute, and nothing is half-remembered after a restart. Tests: Tasks 1 and 4.
3. **The saved marks across a restart.** A session deleted in one run and not yet listed in the next; a deleted group you made yourself, with its sessions' places; a `deleted` value that cannot be read; an older build that reads the newer state. Expected: the Deleted view shows the same after a restart, a recovered group has its sessions again, and a bad value costs only itself. Tests: Tasks 2 and 3.
4. **Who was looking.** A turn ends in the focused tile while another app has the keyboard; the window is closed; the window comes back; another tile closes and the focus falls on a ready session; the user presses the tile that is already focused. Expected: ready is set whenever the user was not looking at that session, and cleared only by an act on it. Tests: Task 6.
5. **Notifications under stress.** Ten sessions finish at once; a title holds `<b>` and `&`; a press comes for a session that was deleted or forgotten meanwhile; no notification service answers; the user returns before the notification was shown. Expected: one notification for each session, text shown as typed, a press on a stale one only shows the window, the app works without a service, and nothing lingers once the user is back. Tests: Tasks 8 and 9.

## File Structure

```
crates/agent/src/agent.rs           Task 1: Removal, Agent::remove_session with a default
crates/agent/src/fake.rs            Task 1: removing in the fake, and what it was asked
crates/claude/src/agent.rs          Task 1: the store's folder from the environment; remove_session
crates/claude/src/remove.rs         Task 1 (new): the paths of one session, and their removal
crates/core/src/sessions.rs         Task 2: Entry.deleted; Task 6: who was looking; Task 8: Entry.alerted
crates/core/src/groups.rs           Tasks 2 and 3: Group.deleted_unix, the two views' models
crates/core/src/saved.rs            Task 2: Saved.deleted
crates/core/src/app.rs              Tasks 2, 3, 4, 6, 8: the intents and effects named below
crates/core/src/backend.rs          Task 4: removing on the library thread
crates/app/src/theme.rs             Task 7: the two pairs of colours
crates/app/src/view/shell.rs        Task 5: the switch and the Deleted view's line
crates/app/src/view/sidebar.rs      Tasks 5 and 7: menus, the Deleted view's rows, coloured rows
crates/app/src/view/deleted.rs      Task 5 (new): the three questions and their words
crates/app/src/view/tiles.rs        Task 7: the coloured title bar
crates/app/src/glue.rs              Tasks 5, 6, 9, 10: the questions' state, the focus hook, the notifier's sender
crates/app/src/notify.rs            Task 9 (new): the notifier's thread, its book of ids, its words
crates/app/src/tray.rs              Task 10: the marker
crates/app/src/main.rs              Tasks 6, 9, 10: the focus hook, starting the notifier
crates/app/Cargo.toml, Cargo.lock   Task 9: zbus and tokio named for Linux
crates/app/src/demo.rs, tests/snapshots.rs, tests/live.rs, docs/   Task 11
```

## Facts this plan builds on

From the two reports; each has file and line there.

- A session's row comes from `Entry` in `Sessions`; `Entry::mark()` gives `Mark::{None, Working, NeedsYou, Finished}`; `Finished` is the flag `Entry.unseen`. `EntryMut` rebuilds the sidebar only when `SidebarFacts` changed, so a new fact the sidebar reads must be part of `SidebarFacts`.
- `unseen` is set in `App::event` when a turn ends and the session is not the focused one or no window shows, and in `App::start_undelivered`. It is cleared in `App::set_window(true)`, `move_tile`, `mark_shown`, `App::focus` (only when the focus moved) and `remove_tile`. A press on the focused tile or its row clears nothing today, because `App::focus` and `App::show` return early.
- The app does not know whether its window has the keyboard. Rune has `App::on_focus(fn(&mut S, bool, &mut Tx))`, unused so far; on Wayland it follows the keyboard entering and leaving the window. `testkit::runtime` cannot install it, so tests call the glue's function directly.
- A folder's group always exists and every session falls into its folder's group unless it is placed elsewhere; a group the user made exists while it has a record. `Groups::delete` drops a record and the placements into it. `Group` has no serde default on its fields: a new field needs its own.
- `Saved` is read leniently field by field; per-session facts are keyed by `SessionId`. Once the complete listing is in, `App::saved` prunes placements to sessions the app knows.
- The every-minute listing covers the last thirty days and never removes an entry. An entry leaves `Sessions` only through `forget_if_empty`, `drop_copies_of`, or a complete listing that no longer holds it (and then only if it rests and was never touched). `App::settle` closes the tile of any session that is no longer in `Sessions`.
- After a program exits, `start_undelivered` and `start_for_remote` may start it again; only `App.closing` stops them today.
- The adapter only reads. It finds a session's file by scanning every project folder for `<id>.jsonl` (`find_session`). It reads `~/.claude/projects` and ignores `CLAUDE_CONFIG_DIR`, which it passes on to the program.
- A new effect needs: `Effect`, `ToBackend`, a `LibraryJob`, `FromBackend`, and an arm in `glue::run`'s exhaustive match (and in `crates/app/tests/live.rs`).
- The tile's title bar (`view/tiles.rs`, `header`) has no fill today. The outline of a tile that needs input is the kit's `warning.icon`, an amber. The kit has no orange. The kit's row draws its dots in fixed colours; `RowStatus::Other(El)` takes any element.
- Rune's tray has `set_menu` and `set_tooltip` and cannot change its icon. Dropping a `Tray` on the thread that made it is safe. `TrayIcon::rgba` takes straight RGBA.
- GNOME's notification service sends a press back only to the connection that sent the notification and refuses to close or replace another connection's; when a connection closes, its notifications go. It reads the body as markup, ignores the timeout, and ties a notification to an app by the `desktop-entry` hint. A press on the body with an action `default` sends `ActivationToken(id, token)` and then `ActionInvoked(id, "default")`.

---

The three checks, wherever a step says "run the three checks": `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`.

### Task 1: The adapter can remove one session, and reads where Claude Code writes

**Files:**
- Create: `crates/claude/src/remove.rs`
- Modify: `crates/agent/src/agent.rs`, `crates/agent/src/fake.rs`, `crates/claude/src/agent.rs`, `crates/claude/src/lib.rs`, `docs/protocol-notes.md`

**Interfaces:**
- Produces:
  ```rust
  // agent/src/agent.rs
  /// What became of a stored session that was to be removed.
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Removal { Removed, Missing, Changed }
  // in trait Agent, with a default that returns an error
  /// Removes a stored session for good, unless it changed after `unless_after_unix`.
  /// May block while files are removed.
  fn remove_session(&self, id: &SessionId, unless_after_unix: i64) -> Result<Removal, AgentError>;
  // agent/src/fake.rs
  pub fn removed(&self) -> Vec<SessionId>;        // in the order asked
  pub fn fail_remove(&self, detail: &str);        // every removal from now on fails with it
  pub fn allow_remove(&self);                     // removals work again
  // claude/src/remove.rs
  /// The store's own paths of one session, those that exist. `root` is Claude Code's folder.
  pub fn paths_of(root: &Path, id: &SessionId) -> Vec<PathBuf>;
  pub fn remove(root: &Path, id: &SessionId, unless_after_unix: i64) -> Result<Removal, AgentError>;
  /// Claude Code's folder: `CLAUDE_CONFIG_DIR` when set and not empty, else `.claude` in the home folder.
  pub fn root_from(config_dir: Option<&OsStr>, home: Option<&Path>) -> Option<PathBuf>;
  ```

Rules:

- `paths_of`, for an id that is a UUID in its written form and for nothing else (any other id gives no paths): every `projects/<p>/<id>.jsonl` (in every project folder, as `find_session` looks), the folder `projects/<p>/<id>` beside each, `file-history/<id>` and `session-env/<id>`. Only those that exist, looked at with `symlink_metadata`.
- `remove`: no transcript anywhere gives `Missing` (and still removes the other three kinds of path if they exist). A transcript modified later than `unless_after_unix` gives `Changed` and removes nothing. Otherwise each path is removed: a file or a link with `remove_file`, a real folder with `remove_dir_all`. The project folder itself stays, also when it is empty. An error removing any path is `AgentError::Io` with the words `the session could not be removed`, and names no path.
- `ClaudeAgent::from_environment` takes the store from `root_from`, so listing, reading and removing look in one place. `ClaudeAgent` keeps the root; `with_paths` gains it or derives it from `projects_dir`'s parent: say which you chose.
- The fake: `remove_session` records the id, then answers `Changed` when the listed session's time is later than `unless_after_unix`, `Missing` when it lists no such session, else `Removed` and takes it out of its list. After `fail_remove` it answers with that error and removes nothing, until `allow_remove`.
- `docs/protocol-notes.md` gains "A session's files, and removing them": the four kinds of path, what is shared and never touched, that the names were read in one real store and nothing else was, and what the live test prints.

- [ ] **Step 1: Write the failing tests.**
  In `remove.rs`, each in its own `tempfile` store: `the_four_kinds_of_path_are_found` ; `a_session_is_removed_with_its_folders_and_nothing_else` (a second session, a `memory` folder, `history.jsonl` at the root and an empty project folder are all still there, byte for byte); `a_session_that_changed_since_is_left` (`Changed`, every path still there); `a_session_that_is_gone_counts_as_missing`, and its leftover `file-history` folder goes; `an_id_that_is_no_uuid_removes_nothing` (`..`, `a/b`, an empty id, 36 characters that are not a UUID, a UUID with a trailing space: `Missing` and nothing changes); `a_link_in_place_of_the_folder_is_removed_and_its_target_stays` (the target folder outside the store keeps its file); `the_same_id_in_two_projects_goes_from_both`; `a_path_that_cannot_be_removed_is_an_error_without_a_path` (a read-only parent; skip the assertion when the test runs as root).
  In `remove.rs`: `the_store_is_where_the_program_looks` (`root_from(Some("/x/cfg"), Some("/home/alex"))` is `/x/cfg`; an empty variable and no variable give `/home/alex/.claude`; neither gives `None`).
  In `fake.rs`: `the_fake_removes_what_it_lists`, `the_fake_leaves_a_session_that_changed`, `a_failed_removal_removes_nothing`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(claude): one stored session can be removed`.

### Task 2: A session and a group can be marked deleted, and recovered

**Files:**
- Modify: `crates/core/src/sessions.rs`, `crates/core/src/groups.rs`, `crates/core/src/saved.rs`, `crates/core/src/app.rs`

**Interfaces:**
- Produces:
  ```rust
  // core/src/sessions.rs
  /// A session in the Deleted view.
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct Deleted {
      /// When it was deleted.
      pub at_unix: i64,
      /// The latest change of its stored file that is the app's own doing. A later one means
      /// it was used again elsewhere.
      pub seen_unix: i64,
      /// When its removal last failed.
      pub failed_unix: Option<i64>,
      /// Its removal was asked for and has not answered yet. Not saved.
      pub removing: bool,
  }
  // in Entry
  pub deleted: Option<Deleted>,
  // core/src/groups.rs, in Group
  #[serde(default)] pub deleted_unix: Option<i64>,     // read leniently
  // core/src/saved.rs, in Saved
  pub deleted: Vec<SavedDeleted>,                      // read leniently, entry by entry
  pub struct SavedDeleted { pub id: SessionId, pub at_unix: i64, pub seen_unix: i64 }
  // core/src/app.rs
  pub const DELETED_SECONDS: i64 = 7 * 24 * 60 * 60;
  /// What deleting would cut off: sessions with a turn running, a request waiting, or a
  /// message queued or undelivered.
  pub fn in_flight(&self, keys: &[SessionKey]) -> usize;
  pub fn sessions_of(&self, group: &GroupId) -> Vec<SessionKey>;   // every one not deleted, also the older
  pub fn delete_session(&mut self, key: SessionKey) -> Vec<Effect>;
  pub fn delete_group(&mut self, id: &GroupId) -> Vec<Effect>;     // new meaning
  pub fn ungroup(&mut self, id: &GroupId) -> Vec<Effect>;          // what delete_group did
  pub fn recover_session(&mut self, key: SessionKey) -> Vec<Effect>;
  pub fn recover_group(&mut self, id: &GroupId) -> Vec<Effect>;
  ```

Rules:

- `delete_session`: a session with no id is forgotten as an empty one is (its program closed if it runs). Otherwise: its tile is closed; a queued message and its undelivered messages are dropped, with their chips; if its program runs it is sent `SessionCommand::Close` once (not again if it is already `stopping`); `unseen`, `prompt` and `checking` are cleared and `remote_wanted` is set false; its conversation is given back to the store as `let_go` gives it (`Effect::DropHistory`, its kept images gone); `deleted = Some(Deleted { at_unix: now, seen_unix: max(last_active_unix, now if a program was closed), failed_unix: None, removing: false })`. Then a save.
- The `Exited` of a deleted session's program sets `seen_unix` to `now + OWN_EXIT_SLACK_SECONDS`, takes the exit as clean whatever it says, and starts nothing.
- **The one place each** for "a deleted session is never ...": started or restarted (`App::start`, checked first; `start_undelivered` and `start_for_remote` reach it); shown (`App::show` returns without effect; `restore_tiles` skips it); counted (`standing`, `busy`); marked (`Entry::mark()` gives `Mark::None`); read (`read_history` is not asked). `SidebarFacts` includes whether it is deleted.
- `delete_group(id)`: every session of the group is deleted as above. A group the user made gets `deleted_unix = Some(now)` and keeps its record and the places of its sessions. A folder's group gets no mark. Then a save.
- `ungroup(id)`: what `delete_group` did before this plan, for a group the user made; nothing for a folder's group that has nothing to reset. The existing tests of `delete_group` move to `ungroup` unchanged in what they assert.
- `recover_session`: clears `deleted`; if its group has `deleted_unix`, that is cleared too. `recover_group`: clears the group's `deleted_unix` and recovers every deleted session in it.
- **Used again elsewhere.** Wherever a listing gives a deleted entry a `last_active_unix` later than its `seen_unix`, the entry is recovered (`recover_session`).
- **Saving.** `Saved.deleted` holds every deleted session with an id, sorted by id. On start the marks are kept by id until their sessions are listed, and are applied to an entry when it appears. Before the complete listing is in, a mark whose session is not known yet is saved again unchanged; after it, a mark whose session the store no longer holds is dropped. Placements into a deleted group are kept as those into a live one. A group the user made is not offered by "Move to group" while it is deleted.
- A session that is deleted cannot be renamed, moved, or sent anything: the intents return without effect.

- [ ] **Step 1: Write the failing tests** in `app.rs` (and `groups.rs`, `saved.rs` where they belong):
  - `deleting_a_resting_session_marks_it_and_closes_its_tile`: `deleted` is set with `at_unix == NOW`, it is in no tile, one `Effect::SaveState`, no `Close`.
  - `deleting_a_working_session_stops_it_once`: one `Effect::Send { command: SessionCommand::Close, .. }`; after `Exited { clean: false, .. }` the phase rests, there is no notice, no mark, `seen_unix == NOW + OWN_EXIT_SLACK_SECONDS`, and no `Effect::Open`.
  - `a_deleted_session_is_never_started_again`: one case each for an undelivered message at the exit, Remote Control wanted, a queued message; none gives `Effect::Open`, and the messages are gone.
  - `a_deleted_session_cannot_be_shown_or_sent_to`, and `deleting_lets_its_conversation_go` (history `NotLoaded`, one `Effect::DropHistory`).
  - `a_deleted_session_counts_nowhere`: `standing()` and `busy()` are all zero with one deleted session that was working.
  - `in_flight_counts_what_deleting_would_cut_off` (a working one, a waiting one, one with a queued message, a resting one: 3).
  - `a_session_never_sent_is_forgotten_by_delete`.
  - `deleting_a_group_takes_its_older_sessions_too`, and `a_group_you_made_is_marked_and_keeps_its_places`.
  - `ungroup_sends_the_sessions_home` (the old tests, renamed).
  - `recovering_a_session_brings_its_deleted_group_back`, `recovering_a_group_recovers_its_sessions`.
  - `a_deleted_session_used_again_elsewhere_comes_back` (a listing with a later time), and `the_apps_own_exit_does_not_count_as_used_again` (a listing with a time inside the slack).
  - `the_marks_survive_a_restart_before_their_sessions_are_listed` (save, `App::new` from it, save again before any listing: `Saved.deleted` is equal), `a_mark_is_applied_when_its_session_is_listed`, `a_mark_for_a_session_the_store_lost_is_dropped_after_the_complete_listing`.
  - In `saved.rs`: `a_deleted_entry_that_cannot_be_read_costs_only_itself`; `SAMPLE_JSON` holds the new field. In `groups.rs`: `a_group_saved_before_this_plan_still_reads`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): sessions and groups can be deleted and recovered`.

### Task 3: The sidebar has two views

**Files:**
- Modify: `crates/core/src/groups.rs`, `crates/core/src/app.rs`

**Interfaces:**
- Consumes: `Entry.deleted`, `Group.deleted_unix` (Task 2).
- Produces:
  ```rust
  // core/src/groups.rs
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
  pub enum SidebarView { #[default] Sessions, Deleted }
  // in Row
  /// In the Deleted view: whole days until it is removed, 0 on its last day.
  pub days_left: Option<u8>,
  /// Its removal failed and waits to be tried again.
  pub stuck: bool,
  // in SidebarModel
  pub view: SidebarView,
  /// How many sessions are in the Deleted view, whichever view shows.
  pub deleted: usize,
  // core/src/app.rs
  pub fn sidebar_view(&self) -> SidebarView;
  pub fn set_sidebar_view(&mut self, view: SidebarView) -> Vec<Effect>;
  ```

Rules:

- In `Sessions`: as today, without deleted sessions and without groups marked deleted. A folder's group with nothing but deleted sessions has no section.
- In `Deleted`: only deleted sessions, of any age (no thirty day rule, no "Show older"), under their groups' headers, newest deletion first; a deleted group the user made shows also when it is empty. No "New group", "Hidden groups" or "Older groups" lines. Groups are never collapsed there.
- `days_left` is `ceil((at_unix + DELETED_SECONDS - now) / 86400) - 1`, never under 0: a session deleted a moment ago has 6, one in its last 24 hours has 0.
- The view is not saved; a run starts in `Sessions`. When the last deleted item is gone while `Deleted` shows, the view stays (it says it is empty).
- `settle` rebuilds the model when the view changes, and when a day passes for a row's `days_left`.

- [ ] **Step 1: Write the failing tests** in `groups.rs` and `app.rs`: `the_normal_list_leaves_deleted_sessions_out`; `a_folder_group_with_only_deleted_sessions_is_not_listed`; `the_deleted_view_lists_only_deleted_sessions_under_their_groups`; `an_old_deleted_session_is_listed_in_the_deleted_view` (older than thirty days); `a_deleted_empty_group_shows_in_the_deleted_view`; `days_left_counts_down` (`NOW` gives 6; `NOW + 6 * 86400 + 1` gives 0; later stays 0); `the_count_of_deleted_is_known_in_both_views`; `switching_the_view_rebuilds_the_list`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): the sidebar's Deleted view`.

### Task 4: After a week, and on "Delete All", a session is removed for good

**Files:**
- Modify: `crates/core/src/app.rs`, `crates/core/src/backend.rs`, `crates/app/src/glue.rs`, `crates/app/tests/live.rs` (the two only to pass the new effect on)

**Interfaces:**
- Consumes: `Agent::remove_session`, `Removal`, the fake's `removed` and `fail_remove` (Task 1); `Deleted` (Task 2).
- Produces:
  ```rust
  // core/src/app.rs
  pub const PURGE_SECONDS: i64 = 60;
  pub const PURGE_RETRY_SECONDS: i64 = 60 * 60;
  Effect::RemoveStored { id: SessionId, unless_after_unix: i64 }
  /// Removes everything in the Deleted view for good, whatever its age.
  pub fn delete_all(&mut self) -> Vec<Effect>;
  // core/src/backend.rs
  ToBackend::RemoveStored { id: SessionId, unless_after_unix: i64 }
  FromBackend::Removed { id: SessionId, result: Result<Removal, AgentError> }
  ```

Rules:

- `App::tick`, at most once every `PURGE_SECONDS` (the stamp pattern of the relisting): every deleted session whose `at_unix + DELETED_SECONDS <= now`, whose program has ended, that is not already being removed, and whose `failed_unix` is `None` or at least `PURGE_RETRY_SECONDS` ago, gives one `Effect::RemoveStored { id, unless_after_unix: seen_unix }`. A deleted group the user made whose week is over and that holds no session any more loses its record.
- The answer, by `id`: `Removed` or `Missing` takes the entry out of `Sessions` (with its mark, its placement, its kept name) and saves. `Changed` recovers it. An error sets `failed_unix = now` and leaves it deleted.
- `delete_all` ends the week of everything in the Deleted view: every deleted session gets `at_unix = now - DELETED_SECONDS` and `failed_unix = None`, every deleted group the user made gets `deleted_unix = now - DELETED_SECONDS`, and the rule above runs at once, without waiting for `PURGE_SECONDS`. A session whose program has not ended yet is thereby removed as soon as it has.
- A mark is dropped only when its removal has answered. If the app quits between the removal and its answer, the next run's complete listing no longer holds the session and drops the mark (Task 2).
- An answer for a session that was recovered meanwhile, or that is no longer known, is dropped. (The file is then gone although the session shows: the next listing takes the entry away. Say so in a comment.)
- The backend runs it on the library thread and answers under the same id.

- [ ] **Step 1: Write the failing tests.**
  In `app.rs`: `a_week_after_deleting_the_session_is_removed` (`tick(NOW + DELETED_SECONDS - 1)` gives nothing; one second later gives exactly one `RemoveStored` with `unless_after_unix == seen_unix`; the next tick gives none while it is being removed); `a_removed_session_is_forgotten` (`Removed` and `Missing`: gone from `Sessions`, from `Saved.deleted` and from `Saved.placed`); `a_session_that_changed_is_recovered_not_removed`; `a_failed_removal_is_tried_again_after_an_hour_and_not_before`; `a_session_whose_program_still_runs_is_not_removed_yet`; `delete_all_removes_everything_in_the_deleted_view_at_once` (three sessions of different ages, one of them failed a minute ago: three `RemoveStored` from `delete_all` itself); `an_emptied_deleted_group_loses_its_record`; `an_answer_for_a_recovered_session_is_dropped`.
  In `backend.rs`: `a_removal_runs_and_answers_under_its_id` (the fake's `removed()` holds it; `Removed` comes back), `a_failed_removal_comes_back_as_its_error`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): deleted sessions are removed after a week`.

### Task 5: The window: Delete, the switch, the Deleted view, Delete All

**Files:**
- Create: `crates/app/src/view/deleted.rs`
- Modify: `crates/app/src/view/shell.rs`, `crates/app/src/view/sidebar.rs`, `crates/app/src/view/mod.rs`, `crates/app/src/view/drag.rs`, `crates/app/src/glue.rs`

**Interfaces:**
- Consumes: Tasks 2 to 4's intents, `SidebarView`, `Row.days_left`, `Row.stuck`, `SidebarModel.deleted`, `in_flight`, `sessions_of`.
- Produces:
  ```rust
  // app/src/glue.rs
  /// A question about deleting that waits for its answer.
  #[derive(Clone, Debug, PartialEq)]
  pub enum DeleteAsk { Session(SessionKey), Group(GroupId), All }
  // in Ui
  pub delete_ask: Option<DeleteAsk>,
  pub fn ask_delete_session(ui: &mut Ui, key: SessionKey);   // deletes at once, or asks
  pub fn ask_delete_group(ui: &mut Ui, id: GroupId);         // always asks
  pub fn ask_delete_all(ui: &mut Ui);                         // always asks
  // app/src/view/deleted.rs
  pub fn words(ui: &Ui, ask: &DeleteAsk) -> (String, String, &'static str);   // title, text, the button
  pub fn days(days_left: u8) -> String;
  pub fn question(ui: &Ui, kit: &Kit, layer: &mut Vec<El>);
  ```

Rules:

- **The switch**, first in the sidebar's header, above "New session": a row `mr-view-switch` of two quiet buttons, `mr-view-sessions` "Sessions" and `mr-view-deleted` "Deleted" (with the count in brackets when it is not zero: "Deleted (3)"); the one that shows is drawn as chosen and both are Tab stops. A press calls `App::set_sidebar_view`.
- **In the Deleted view** the header shows, in place of "New session": the line `mr-deleted-note` "Removed for good after 7 days." and the button `mr-delete-all` "Delete All", which is disabled when the view is empty and calls `ask_delete_all`. An empty view shows the line `mr-deleted-empty` "Nothing is deleted." in the list.
- **Rows in the Deleted view**: the title, and as the row's detail `days(days_left)`: "6 days left", "1 day left", "today"; a stuck row says "Could not be removed" instead. No mark, no drag, not selected. A press, a secondary press and the menu key all open the row's menu, which has one entry: "Recover". A group's header there has no "+" and its menu has one entry: "Recover group". Recovering leaves the Deleted view showing.
- **In the normal list**: a session row's menu gains, last and after a separator, "Delete" (danger). A group's menu loses its old "Delete group" and gains: "Ungroup" (only for a group the user made) and, last and after a separator, "Delete group" (danger, for every group that holds a session or was made by the user).
- **The questions** are the kit's dialog, built as plan 7's quit question is (words in the dialog's body so that they wrap; the first button "Cancel" is the default; the second is the danger one). Exact words:
  - a session that is in flight: title "Delete this session?", text "It is still working. Deleting stops it. It can be recovered for 7 days.", button "Delete".
  - a group: title "Delete this group?", text "{n} sessions go with it." (or "1 session goes with it.", or "It has no sessions."), then, when any is in flight, " {m} of them are still working and will be stopped." (or "1 of them is still working and will be stopped."), then " They can be recovered for 7 days." (for no sessions: " It can be recovered for 7 days."), button "Delete".
  - Delete All: title "Delete everything for good?", text "{n} sessions will be removed from this computer. This cannot be undone." (or "1 session will be ..."), button "Delete All".
- `ask_delete_session` deletes at once when `in_flight(&[key]) == 0`, else sets `delete_ask`. The dialog's keys: `mr-delete`, `mr-delete-cancel`, `mr-delete-yes`. `put_away` clears `delete_ask`; `overlay_open` includes it; `tidy_sidebar` clears it when its session or group is gone. A press on "Delete" (or "Delete All") carries the intent out and closes the question.
- A drag from the normal list never drops into the Deleted view, and nothing is dragged there.

- [ ] **Step 1: Write the failing tests** (view tests, through the demo window):
  - `delete_on_a_resting_row_moves_it_to_the_deleted_view_at_once` (no dialog; the row is gone from the list; "Deleted (1)" shows on the switch).
  - `delete_on_a_working_row_asks_and_cancel_keeps_it` (the dialog's text in full; Return cancels; the session still works), and `delete_in_the_question_stops_and_deletes_it`.
  - `delete_group_asks_with_the_count` (the words for 3 sessions of which 1 works, in full), and `ungroup_is_offered_only_for_a_group_you_made`.
  - `the_switch_shows_the_deleted_view` (the note, "Delete All", the rows with "6 days left"), and `the_deleted_view_says_when_it_is_empty` ("Nothing is deleted.", "Delete All" disabled).
  - `a_press_on_a_deleted_row_offers_recover`, `recover_puts_the_row_back`, `recover_group_puts_the_group_back`.
  - `delete_all_asks_and_removes_for_good` (the words in full; after "Delete All" the fake's `removed()` holds every id and the view is empty).
  - `a_row_whose_removal_failed_says_so`.
  - `deleting_the_session_whose_picture_is_open_closes_the_picture`.
  - `escape_closes_the_question_and_deletes_nothing`; `the_question_is_a_dialog_to_a_screen_reader`.
  - In `deleted.rs`: `the_words_say_what_goes` (every string above), `the_days_are_said_plainly` (6, 1, 0).
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): Delete, the Deleted view and Delete All`.

### Task 6: "Ready" means the user was not looking, and stays until they act

**Files:**
- Modify: `crates/core/src/app.rs`, `crates/core/src/sessions.rs`, `crates/app/src/glue.rs`, `crates/app/src/main.rs`, `crates/app/src/view/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  // core/src/app.rs
  /// Whether the window has the keyboard. While it has not, a turn that ends leaves its
  /// session ready also in the focused tile.
  pub fn set_active(&mut self, active: bool) -> Vec<Effect>;
  /// The user is looking at MindRune: a window shows and has the keyboard.
  pub fn looking(&self) -> bool;
  /// The user acted on this session: it is no longer ready.
  pub fn attend(&mut self, key: SessionKey) -> Vec<Effect>;
  // app/src/glue.rs
  pub fn focus_changed(ui: &mut Ui, focused: bool, tx: &mut Tx);   // App::on_focus
  ```

Rules:

- `App` holds `active: bool`, true at the start. `looking()` is `window && active`.
- A turn that ends (as today: finished, failed, or its program died) sets `unseen` unless `looking()` and the session is the focused one. `start_undelivered`'s case follows the same rule.
- `unseen` is cleared by `attend(key)`, and `attend` is called from: a press on the session's tile and on its row, **also when it is already the focused one** (`App::focus_tile` and `App::show` call it before they return early); the keyboard's focus moving to its tile (F6, a click into its box, the glue's rule that the tile typed in takes the focus); `submit`, `answer`, `answer_prompt`, `stop`, `restart`, `choose_model`, `choose_mode` for that session; `move_tile` of its tile.
- `unseen` is **no longer** cleared by `set_window(true)` or by `remove_tile` when the focus falls on the session after another tile closed. `mark_shown` clears it only when the user's own act showed it (a row pressed, a tile opened), not in `restore_tiles`.
- `main.rs` registers `.on_focus(glue::focus_changed)` in every mode. `focus_changed` calls `set_active` and `run`. The existing tests of `set_window` that assert the old clearing are changed to the new rule: list them in your report.
- Nothing is drawn differently by this task.

- [ ] **Step 1: Write the failing tests.**
  In `app.rs`: `a_turn_that_ends_while_another_app_has_the_keyboard_is_ready_also_in_the_focused_tile`; `a_turn_that_ends_in_the_focused_tile_while_looking_is_not_ready`; `a_turn_that_ends_in_another_tile_is_ready` (as today); `the_window_coming_back_does_not_clear_ready`; `getting_the_keyboard_back_does_not_clear_ready`; `closing_another_tile_does_not_clear_ready`; `a_press_on_the_focused_tile_clears_ready` (`focus_tile` on the focused session); `a_press_on_the_row_of_the_focused_session_clears_ready` (`show`); `sending_in_it_clears_ready`; `restored_tiles_keep_no_ready_and_clear_none`.
  In `glue.rs`: `losing_the_keyboard_is_told_to_the_core` (`focus_changed(false)` then a finished turn in the focused tile: its entry is `unseen`).
  View test: `a_click_on_the_focused_tile_of_a_ready_session_clears_it`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): ready until the user acts on the session`.

### Task 7: Green for ready, orange for needs input

**Files:**
- Modify: `crates/app/src/theme.rs`, `crates/app/src/view/tiles.rs`, `crates/app/src/view/sidebar.rs`

**Interfaces:**
- Consumes: `Entry::mark()` as it is after Tasks 2 and 6.
- Produces:
  ```rust
  // app/src/theme.rs
  /// A fill and the colour of what is written on it.
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Pair { pub fill: Color, pub ink: Color }
  /// A session that is ready: the green of the icon's hexagon, with light writing.
  pub fn ready() -> Pair;
  /// A session that needs input: the orange of the icon's rays, with dark writing.
  pub fn needs_input() -> Pair;
  /// The pair of a mark, when it has one. Working and no mark have none.
  pub fn pair_of(mark: Mark) -> Option<Pair>;
  /// The contrast between two colours, as WCAG counts it.
  pub fn contrast(a: Color, b: Color) -> f32;
  ```

Rules:

- The fills are the icon's own colours, the same in both themes: green `#246b43` and orange `#f3922e` (given as OKLCH or as components; `rune` re-exports no hex reader). The inks are a near white for green and a near black for orange, each with `contrast(fill, ink) >= 4.5`.
- **A tile's title bar** (`header`): with a pair, the bar is filled with it and the title, the folder chip's text and border, the state word, the Remote word and the close button's icon are drawn in its ink (the chip loses its own fill). Without a pair it is as today.
- **A tile's outline**: for `NeedsYou` it is `needs_input().fill` in place of the kit's amber. The rest of `outline` is unchanged.
- **A session's row**: with a pair, the whole row is filled with it, its title and detail are in its ink, and its status slot is empty (`Working` keeps its spinner; rows without a pair are as today). The fill wins over the selected and the shown fills; hover does not change it. The row being renamed takes the same fill.
- **A collapsed group's mark**: a dot in the pair's fill for the strongest state among its rows, the spinner for working.
- The row's and the tile's words for a screen reader stay ("Needs attention", "Unread", "Needs you", "Finished").
- `sidebar::mark_color` gives the pair's fill for `NeedsYou` and `Finished`. The tests' helper `mark()` in `view/sidebar.rs` recognises a state by the row's fill.

- [ ] **Step 1: Write the failing tests.**
  In `theme.rs`: `the_pairs_can_be_read` (both contrasts at least 4.5), `the_fills_are_the_icons_colours` (within a small tolerance of `#246b43` and `#f3922e`), `only_ready_and_needs_input_have_a_pair`.
  View tests: `a_ready_tile_has_a_green_title_bar_and_a_green_row` (the header's and the row's fill equal `ready().fill`; the title's colour is its ink); `a_tile_that_needs_input_is_orange_on_both`, and its outline is the same orange; `a_working_tile_has_no_colour`; `clicking_the_ready_tile_returns_both_to_grey`; `clicking_the_ready_row_returns_both_to_grey`; `orange_wins_on_a_selected_row`; `a_collapsed_group_shows_the_strongest_colour`; `the_row_being_renamed_keeps_its_colour`; `a_screen_reader_still_hears_the_state`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): green for ready, orange for needs input`.

### Task 8: The core says when a notification is due, and when it is taken back

**Files:**
- Modify: `crates/core/src/app.rs`, `crates/core/src/sessions.rs`, `crates/core/src/lib.rs`, `crates/app/src/glue.rs`, `crates/app/tests/live.rs` (the two only so the new effects compile: they are dropped until Task 9)

**Interfaces:**
- Consumes: `looking()`, `set_active`, `attend` (Task 6); `Entry.deleted` (Task 2).
- Produces:
  ```rust
  // core/src/sessions.rs (exported from lib.rs)
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Alert { Ready, NeedsInput }
  // core/src/app.rs
  Effect::Notify { key: SessionKey, title: String, alert: Alert }
  Effect::Unnotify { key: SessionKey }
  Effect::UnnotifyAll
  /// The user pressed the notification of this session.
  pub fn alert_pressed(&mut self, key: SessionKey) -> Vec<Effect>;
  // core/src/sessions.rs, in Entry
  /// A notification about it is out.
  pub alerted: Option<Alert>,
  ```

Rules:

- After anything that can change a session's mark (an event, a failed send, an answer of the store), the core compares the mark before and after. When it became `NeedsYou` or `Finished`, the user is not `looking()`, and the session is not deleted: one `Effect::Notify` with the session's title as the sidebar shows it, and `alerted` is set. A session already alerted for the same state gives nothing; one that goes from ready to needing input gives a new `Notify` (it replaces the old one).
- When an alerted session's mark is no longer the alerted one (answered, attended, deleted, forgotten): `Effect::Unnotify { key }` and `alerted = None`.
- `set_active(true)` while a window shows, and `set_window(true)` while active: `Effect::UnnotifyAll` when anything is alerted, and every `alerted` is cleared. The marks stay.
- `alert_pressed(key)`: for a session that is known and not deleted, `show` it in the focused tile when it is in none, focus its tile, and `attend` it. For any other key: nothing.
- No `Notify` is ever given while `looking()`.

- [ ] **Step 1: Write the failing tests** in `app.rs`: `a_turn_that_ends_while_not_looking_is_notified_once` (window closed: one `Notify { alert: Alert::Ready, .. }` with the title; a second event gives none); `a_request_while_not_looking_is_notified` (`Alert::NeedsInput`); `nothing_is_notified_while_looking`, also for another tile; `ready_then_needing_input_notifies_again`; `answering_takes_the_notification_back`; `coming_back_takes_them_all_back_and_keeps_the_marks`; `deleting_an_alerted_session_takes_its_notification_back`; `a_deleted_session_is_never_notified`; `ten_sessions_ending_at_once_give_ten_notifications`; `a_press_on_a_notification_focuses_and_attends_the_session` (in a tile, and in none); `a_press_for_a_session_that_is_gone_does_nothing`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): when a notification is due and when it is taken back`.

### Task 9: Desktop notifications

**Files:**
- Create: `crates/app/src/notify.rs`
- Modify: `crates/app/Cargo.toml`, `Cargo.lock`, `crates/app/src/main.rs`, `crates/app/src/glue.rs`

**Interfaces:**
- Consumes: `Effect::Notify`, `Effect::Unnotify`, `Effect::UnnotifyAll`, `App::alert_pressed`, `Alert` (Task 8); `crate::show(handle, token)` in `main.rs` (plan 7).
- Produces:
  ```rust
  // app/src/notify.rs
  pub enum Note { Show { key: SessionKey, title: String, alert: Alert }, Close { key: SessionKey }, CloseAll }
  /// The words of a notification: its title as typed, its text.
  pub fn words(title: &str, alert: Alert) -> (String, &'static str);
  /// `&`, `<` and `>` made safe for a text that is read as markup.
  pub fn escape(text: &str) -> String;
  /// Which notification belongs to which session, and the token of a press.
  #[derive(Default)]
  pub struct Book { /* private */ }
  impl Book {
      pub fn replaces(&self, key: SessionKey) -> u32;            // 0 when none is out
      pub fn shown(&mut self, key: SessionKey, id: u32);
      pub fn id_of(&self, key: SessionKey) -> Option<u32>;
      pub fn all(&self) -> Vec<u32>;
      pub fn token(&mut self, id: u32, token: String);
      /// A press on `id`: its session, and its token when one came.
      pub fn pressed(&mut self, id: u32) -> Option<(SessionKey, Option<String>)>;
      pub fn closed(&mut self, id: u32);
  }
  /// Starts the notifier on its own thread. `None` where there is none (not Linux).
  pub fn start(handle: AppHandle<Ui>) -> Option<Sender<Note>>;
  // app/src/glue.rs, in Ui
  pub notifier: Option<Sender<Note>>,
  /// A notification of this session was pressed.
  pub fn alert_pressed(ui: &mut Ui, tx: &mut Tx, key: SessionKey);
  ```

Rules:

- `words`: the title is the session's title unchanged; the text is "Finished." for `Alert::Ready` and "Needs your input." for `Alert::NeedsInput`. A title that is empty reads "MindRune".
- **The thread** `mindrune-notify`, on Linux only, built as Rune's tray worker is (the report has file and line): a current-thread tokio runtime, **one** `zbus` connection to the session bus for the life of the app, notes taken from a channel. If the connection cannot be made within five seconds it prints `mindrune: no desktop notifications: {error}` once and ends; notes sent afterwards are dropped.
- `Show`: `Notify` on `org.freedesktop.Notifications` with `app_name` "MindRune", `replaces_id` from the book, `app_icon` "mindrune", the title as summary (plain), `escape`d text as body, actions `["default", "Open"]`, hints `desktop-entry` = "mindrune" and `urgency` = 1, timeout -1. The answer's id goes into the book.
- `Close`: `CloseNotification` for the session's id, when one is out. `CloseAll`: for every id in the book.
- Signals heard on the same connection: `ActivationToken(id, token)` goes into the book; `ActionInvoked(id, "default")` takes the press from the book and hands it over: `handle.update(move |ui, tx| glue::alert_pressed(ui, tx, key))`, then `crate::show(&handle, token)`; `NotificationClosed(id, _)` forgets the id. A press for an id the book does not know only shows the window.
- `glue::run` passes `Effect::Notify`, `Unnotify` and `UnnotifyAll` to `ui.notifier` as `Note::Show`, `Close` and `CloseAll` when there is one, and drops them otherwise. `glue::alert_pressed(ui, tx, key)` calls `App::alert_pressed` and `run`, and takes the keyboard into that tile's box.
- `main.rs` starts the notifier outside `--demo` and outside a headless run, in every such mode (also when the app does not stay without its window), and gives its sender to `Ui` through `handle.update`.
- `crates/app/Cargo.toml`: under `[target.'cfg(target_os = "linux")'.dependencies]`, `zbus = { version = "6", default-features = false, features = ["tokio"] }` and `tokio = { version = "1", features = ["rt", "sync", "time", "macros"] }`, trimmed to what the code uses. After `cargo build`, `git diff Cargo.lock` shows only lines inside the `mindrune` package's entry: paste that diff into your report. If any other package appears, stop and report BLOCKED.
- The D-Bus calls themselves are not tested: they need a desktop's session bus. Say in the report what is left untested and why.

- [ ] **Step 1: Write the failing tests** in `notify.rs`: `the_words_of_each_alert`; `a_title_is_shown_as_typed` (`escape("<b>a & b</b>")` is `&lt;b&gt;a &amp; b&lt;/b&gt;`); `an_empty_title_reads_mindrune`; `a_second_notification_for_a_session_replaces_the_first` (`replaces` is 0, then the id that was shown); `a_press_gives_the_session_and_its_token`, `a_press_without_a_token_still_gives_the_session`, `a_press_on_an_unknown_id_gives_nothing`; `a_closed_notification_is_forgotten` (its token too); `close_all_names_every_id`.
  In `glue.rs`: `a_notify_effect_reaches_the_notifier_and_is_dropped_without_one` (a plain channel made in the test); `a_pressed_alert_focuses_its_session`.
  In `main.rs`: the function that decides whether a notifier starts (`--demo` and headless: no).
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): desktop notifications when a session is ready or needs input`.

### Task 10: The tray's marker

**Files:**
- Modify: `crates/app/src/tray.rs`

**Interfaces:**
- Consumes: `Standing` (plan 7; deleted sessions are left out since Task 2); `theme::ready`, `theme::needs_input` (Task 7).
- Produces:
  ```rust
  // app/src/tray.rs
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Marker { None, Ready, NeedsInput }
  pub fn marker(standing: Standing) -> Marker;
  /// The icon's pixels with the marker drawn over them: straight RGBA, `side` by `side`.
  pub fn marked(pixels: &[u8], side: u32, marker: Marker) -> Vec<u8>;
  ```

Rules:

- `marker`: `NeedsInput` when `needs_you > 0`, else `Ready` when `finished > 0`, else `None`.
- `marked`: for `None` the pixels unchanged. Otherwise a filled circle whose diameter is 44% of the side, in the marker's fill, opaque, with a ring one twentieth of the side wide in black at 60% around it, placed so that the ring's outer edge touches the icon's right and bottom edges, drawn over the icon by plain source-over on straight alpha.
- The tray's thread: for each standing it takes (the last one waiting, when several wait), it sets the menu as today; when the marker differs from the one drawn, it drops the `Tray` and makes a new one with the marked icon and the same spec and handler. If making it fails, it prints the existing `no tray icon` line once and tries again at the next change of marker.
- `Tray::new` and the thread are not tested, as in plan 7.

- [ ] **Step 1: Write the failing tests** in `tray.rs`: `the_marker_follows_the_standing` (needs input wins over ready; working alone gives none); `no_marker_leaves_the_icon_as_it_is`; `the_marker_is_a_dot_at_the_lower_right` (for a 64 pixel icon: the pixel at (52, 52) is the fill, opaque; the pixel at (8, 8) is unchanged; the pixel count that changed is within 20% of the circle's and ring's area); `the_marked_icon_keeps_its_size`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): the tray icon shows ready and needs input`.

### Task 11: Scenes, the real path, notes, and Jerod's look

**Files:**
- Modify: `crates/app/src/demo.rs`, `crates/app/tests/snapshots.rs`, `crates/app/tests/live.rs`, `docs/protocol-notes.md`, `docs/notes-for-later-plans.md`, `README.md`

Rules:

- **Scenes**, in both lists, dark and light: `colours` (three tiles: one ready, one that needs input, one working, with the sidebar's rows in their colours and a collapsed group with a dot), `deleted` (the Deleted view with two groups, rows with different days left and one that could not be removed), `deleted-empty`, `delete-group-ask` (the question over the normal list), `delete-all-ask`. The existing scenes `sessions`, `tiles-needs-you` and `group-menu` change with the colours and the menu: look at them too.
- **The live test** (never run by an agent; it prints and never fails on these): at the end of each run it removes, through the adapter's `remove_session`, the sessions that the test itself made in that run, and only those (it knows their ids), and prints `live: the test's own sessions removed from the store: N of M`, and then, by names alone and never by content, `live: paths still named by a removed session's id: N` with, when N is not zero, the kinds of path (the names of their parent folders, never an id). It removes nothing it did not make, and nothing when it does not know an id.
- **`docs/protocol-notes.md`**: "A session's files, and removing them" (from Task 1) says what the live test settles.
- **`README.md`**: "What it does today" gains the Deleted view, the colours and the notifications, in three short lines.
- **`docs/notes-for-later-plans.md`**: a first section "Jerod's look at plan 8": the commands, "Before looking" (this plan's "What this window will not do yet"; quit any running copy first; the desktop entry must be installed for a notification to carry MindRune's name and icon), what to try, every decision of this plan with what was built, the rulings made while it ran, and what is left.
- **What to try in the window:**
  1. Right-click a test session, "Delete": it leaves the list at once. The switch says "Deleted (1)".
  2. The Deleted view: the row says "6 days left". Press it, "Recover": it is back where it was.
  3. "Delete group" on a group of test sessions: the question names the count. The group is in the Deleted view; "Recover group" brings it back; delete it again.
  4. Delete a session while its turn runs: the question; its tile closes; its `claude` process ends.
  5. "Delete All": the question; afterwards the sessions are gone from `claude --resume` too.
  6. Quit and start again with something in the Deleted view: it is still there, with a day count.
  7. Let a turn end in a tile you are not in: its title bar and its row turn green. Click the tile: both turn grey. The same by clicking the row.
  8. Let a session ask for permission in a tile you are not in: orange on both, and the tile's outline, until you answer.
  9. Switch to another app while a turn runs in the tile you were in: a notification with the session's name and "Finished."; the tray's icon has a green dot. Click the notification: MindRune comes to the front with that session. Back in MindRune the tile is green until you click it.
  10. The same with the window closed, and with a permission request ("Needs your input.", an orange dot).
  11. Watch the tray icon as the dot comes and goes: is the blink acceptable?
  12. Both themes: are the green and the orange pleasant, and is the writing on them easy to read?

- [ ] **Step 1:** The scenes and their snapshots; look at each PNG in both themes and say what you see.
- [ ] **Step 2:** The live test's lines (compile only: `cargo test -p mindrune --test live --no-run`).
- [ ] **Step 3:** The notes and the README.
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): scenes for the Deleted view and the colours`, `test(live): the test removes the sessions it made`, `docs: plan 8, in the notes`.

## After the last task

A whole-branch review on the most capable model, one fix wave, one re-review. Then Jerod runs the live test and looks; the branch is merged only on their word.
