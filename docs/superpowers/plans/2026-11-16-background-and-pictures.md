# Running in the Background, and Opening a Picture: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Closing the window no longer ends the app: its sessions keep working and stay reachable through Remote Control, launching the app again shows the same window, and a tray icon offers Show and Quit. Quit is explicit and asks first when something would be cut off. A program that has done nothing for half an hour is stopped unnoticed and started again by the next message. And a press on a picture, in a sent message or on a chip, opens it large over the window.

**Architecture:** Rune already keeps an app and its one state alive without a window (`App::keep_running`), has a tray (`rune::Tray`) and a full-window picture viewer in the kit (`rune_kit::image_zoom`); this plan wires them in and writes none of them. What Rune lacks on Linux, a way to tell a running copy it was launched again, is a small socket in the user's runtime folder. `mr-core` learns two things it never needed while the window's close ended everything: when a program may be stopped (and that a message caught by a stop is delivered after the restart), and how to give a picture's full image back, from memory for one sent in this run and from the store for an older one.

**Tech Stack:** Rust 1.99.1 (edition 2024), `rune` and `rune-kit` at the pinned revision, `serde`, `serde_json`, std threads, channels, `std::os::unix::net` and `File::try_lock`, `tempfile` for tests. No new dependency: the tray needs Rune's `dbus` feature, which is among its default features and already on.

**Spec:** `docs/superpowers/specs/2026-11-10-claude-code-ui-design.md`: the row "Closing the window" in the table of settled things (line 72), and section 5.8 from "Sessions keep running when their tile is closed" to the idle rule (lines 239 to 255). Read these before starting, in this order: the spec's lines, `docs/notes-for-later-plans.md` (search for "plan 7" and for "What plan 5 must do first", which since 2026-11-15 is addressed to this plan), then this plan. The picture is not in the spec: it is Jerod's request of 2026-11-16, quoted in the notes ("being able to click an attached picture to open it", and "In the window").

**For the controller:** Jerod's rule since 2026-11-13: every subagent (implementer, reviewer, fixer) runs on Opus 5.5 at high effort or better. Nothing less, whatever the size of the task. Two research reports made for this plan are on disk and not in git: `.superpowers/sdd/plan-7-research/code-as-built.md` (the code on `main` at `27b344b`, with file and line for everything this plan touches) and `.superpowers/sdd/plan-7-research/rune-for-background.md` (what Rune at the pinned revision offers for an app without a window, the tray, overlays and large images, with file and line in Rune's source). Give each implementer the report its task needs. Where a report and the code differ, the code wins.

## This is plan 7 of 7

| Plan | Delivers |
| --- | --- |
| 1 to 6 (done) | Foundations and the Claude adapter, one session in a window, many sessions, tiles, stacked tiles, the right panel, Remote Control, the effort level, the context breakdown, attachments, sent pictures |
| **7. Running in the background, and opening a picture (this plan)** | Closing the window keeps sessions working, launching again, the tray, Quit and its question, stopping idle programs, a picture opened large |

**Not in this plan, on purpose.** Desktop notifications when a session needs the user (the spec leaves them for later and Jerod has not been asked; the tray's menu says how many sessions wait, and nothing pops up). Starting the app at login. Sessions that come back running after a Quit (the spec: offered and not chosen). Keeping a draft or its chips across a Quit. A separate background service (the spec only asks that nothing rules it out; process supervision stays in the adapter). More than one window. Moving between pictures with the arrow keys. A title that arrives after its program's exit, which is lost as today. Task 13 of plan 5, which still waits for Jerod's recording.

## What rests on what was never seen

No agent runs Claude Code, and no agent sees the real window or the desktop. What this plan needs from them:

| Point | Read where | Seen working | Made sure by |
| --- | --- | --- | --- |
| With `keep_running`, a closed window on GNOME Wayland is destroyed and comes back with the same tiles, drafts and scroll | Rune's source and README | no | Jerod's look |
| Timers and `poll` run while no window shows | Rune's source | no | Jerod's look, with the idle time set short |
| The tray icon shows on Ubuntu's GNOME | Rune's source (a StatusNotifierItem); Ubuntu ships a host for those | no | Jerod's look |
| A launch from the launcher brings the window in front with the keyboard | Rune's and winit's source (an activation token) | no | Jerod's look |
| A program closed while idle ends cleanly and its session resumes with model, effort and mode | closing and resuming were each seen (the live test, 2026-11-16) | each alone, yes | the live test, in one go |
| The record `system`/`background_tasks_changed`: a list `tasks`, replaced whole each time, an entry's `off_host` | the program's own text (2.1.323) | it arrives (the live run of 2026-11-16 printed two "ignored" lines); its shape, no | the live test prints what arrived; a list that cannot be read counts as busy |
| A stored message's own id (`uuid`) finds it again, with its images | the history reader already follows `uuid` links | the links, yes; an image by it, no | the live test |

Should the live test show that an image cannot be read again by its message's id, Task 9's reader is corrected from the test's printed line before the merge. Until then only pictures of reopened sessions are affected: they show small in the viewer, with a line that says so.

## What this window will not do yet

Say this to Jerod before their look.

- **Nothing pops up when a session needs you while the window is closed.** The tray icon's menu says "1 needs you"; you have to look.
- **Where the desktop shows no tray icons there is none.** Ubuntu's GNOME shows them; plain GNOME needs an extension. Without one the way back is to launch MindRune again, and Quit is Ctrl+Q in the window, "Quit MindRune" on the launcher icon's menu, or `mindrune --quit`.
- **Started again from a terminal, the window may come up behind others.** The desktop lets a window take the front only with a token that a launcher hands over. From the launcher it comes to the front, once the desktop entry is installed again (it gains two lines).
- **A logout, or a `kill`, ends the app without the question.** The toolkit hears neither on Linux. The programs end with the app, as they do today.
- **A new build started while an old copy still runs shows the old copy's window.** It says so on stderr. Quit the old one first (`mindrune --quit`).
- After a Quit nothing is running. Sessions are in the history and continue with the next message.
- A draft and its chips now survive closing the window. They are still lost by Quit.
- The first message to a session whose program was stopped for being idle takes a second or two longer. A permission given "for this session" is forgotten by then, because the program that held it is gone.
- The large picture is the image as it was sent. An image over the limits was made smaller before it went, so that is what opens, not the file on disk.
- A sent picture opens with the pointer only. A chip's picture also opens from the keyboard.
- Only Linux and macOS keep running after the window closes. On Windows the close still ends the app (Rune's tray would work there; a second launch finding the first is not written).

## Decisions in this plan that the design does not make, or makes differently

Each is the planner's call. Jerod has not seen them; they are listed in the handover for review.

**Running in the background**

1. **Closing the window never ends the app, also when nothing is running.** This is the spec's rule taken at its word. The other choice is to end when no program runs, which leaves no lingering process but makes the close do two different things.
2. **The ways back:** launching MindRune again (the launcher, or `mindrune` in a terminal), a click on the tray icon, and "Show MindRune" in its menu.
3. **Launching again with a folder** (`mindrune ~/project`) opens a new session in that folder in the running copy, as a first launch with a folder does.
4. **The ways to quit:** Ctrl+Q, a "Quit" button at the sidebar's foot beside the account's name, "Quit" in the tray's menu, "Quit MindRune" on the launcher icon's menu, and `mindrune --quit`.
5. **Quit asks when a session is working or has Remote Control on.** Working means a turn runs, a request waits for the user, a message waits for its program, or the program has a background task. With nothing of the kind it quits at once. The question shows in the window; a Quit asked for while the window is closed opens the window for it.
6. **One running copy per user, found through a socket in the user's runtime folder** (`$XDG_RUNTIME_DIR/mindrune`). Rune finds a running copy only on Windows.
7. **The tray is Rune's own.** Its menu: a line of standing when there is something to say ("2 working · 1 needs you · 1 finished"), "Show MindRune", "Quit".
8. **With the window closed, nothing counts as seen.** A turn that ends in the focused tile while no window shows leaves its session marked, and the mark goes when the window is back.
9. **Closing the window puts away what lay over it:** an open menu or picker, a rename, a drag, the quit question, the large picture. The window comes back with its tiles, drafts and scroll, and nothing open over them.
10. **The store is listed again every minute also while the window is closed**, as today. It keeps the sidebar true for the moment the window returns, and costs one look at the session files a minute.

**Stopping idle programs**

11. **Idle means half an hour** without a message sent to the session and without anything from its program. The spec leaves the length to the builder. `MINDRUNE_IDLE_SECONDS` sets another length for one run, for looking at it.
12. **A program is never stopped** while a turn runs, a request waits, Remote Control is on or asked for, a message is queued or undelivered, the "open elsewhere" question shows, or the program has a background task. A session that has never had a message is not stopped either: it has nothing to resume, so stopping it could not go unnoticed.
13. **The program's background tasks are read from its own record**, which the adapter has ignored so far. A task marked as running off this machine does not count. A record that cannot be read counts as one task: better a program too many than a build killed.
14. **A stop that had to kill the program still counts as a quiet one.** The session rests and shows no "ended" notice.
15. **A message that could not be delivered because its program had just ended is sent when the session starts again**, and a clean end starts it again at once. Today such a message stays in the conversation, looks sent and is never answered. A stop for idleness makes that moment ordinary: the user sends in the second the program goes.
16. **A resting session that is in no tile gives its conversation back to the store**, and reads it again when it is shown. Today every history read stays in memory for the run. A read asked for and no longer wanted is skipped.

**Opening a picture**

17. **The picture opens over the whole window**, not over its tile. A tile on a wide monitor can be narrow, and large is the point.
18. **The viewer is the kit's.** It fits the picture to the window and never enlarges it past a pixel a point; it has a zoom bar; Escape fits a zoomed picture first and closes it second; a click beside the picture closes it; it has a close button.
19. **A chip's picture opens too**, showing the image as it will be sent. It is a Tab stop pressed with Space, as the chip's remove control is.
20. **A sent picture opens with the pointer only**, as everything in a conversation is pointer only today.
21. **An image sent in this run stays in memory in full** (at most 3.9 MB each) for as long as its conversation is held, so it can be opened at once. The other choice is to read every picture back from the store, which was never seen working and cannot find a message sent a moment ago.
22. **A picture of a reopened session is read again from the store** by its message's id, from the whole file and not only its last 16 MiB.
23. **Until the full image is there the small picture shows**, at its own size. If the full image cannot be had, the small one stays with the line "The full picture cannot be read."
24. **A picture that has not loaded yet** (the outlined box) does not open.

## Global Constraints

- Rust toolchain is exactly `1.99.1`, edition `2024`. In an agent's shell cargo is not on `PATH`: start every command with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Rune is pinned to `rev = "2a44156b21d63b0342a366b26d9222a6edd4622c"`. Its source is at `~/.cargo/git/checkouts/rune-c9b805e9ed8a3730/2a44156`; read it instead of guessing an API. Where this plan and the source differ, the source wins: say so in your report.
- No new dependency in any `Cargo.toml`, and nothing is downloaded.
- `mr-agent` and `mr-core` depend on neither `mr-claude` nor `rune`. No type from `mr-claude` appears in `mr-core` or in any view. Only `crates/app/src/main.rs`, `crates/app/tests/live.rs` and `crates/probe` may name `ClaudeAgent`.
- The UI thread never blocks, with one exception that exists today and moves from the window's close to Quit: the wait of at most 2.5 seconds for the programs to end. `Tray::new` can block for five seconds and is never called on the UI thread. A file is read on the library thread only.
- The tray's thread and the listener's thread touch `Ui` only through `AppHandle::update`, and call nothing else on it but `open`, `activate` and `wake`.
- In `--demo`, and whenever `rune::is_headless()` is true, the app claims no socket, starts no tray and does not keep running: it behaves as it does today. No test and no screenshot may reach a running copy of the real app.
- What arrives on the socket is data from another process of the same user: one line of at most 4,096 bytes, read with a time limit, decoded strictly; anything else is dropped. It can show the window, open a session in an existing folder and ask for Quit, and nothing more. The socket's folder is made with mode `0700`.
- An activation token is passed on and never printed or logged.
- An image's bytes are never copied where a shared pointer will do, and never printed: no `Debug`, log line, panic message or test failure shows them.
- A session's events are trusted only while they carry the number of that session's current open. The one addition: a failed send is also taken for the open that ended last (Task 1).
- A program is stopped unnoticed only under decision 12's conditions, all of them, checked in one place (`Session::stoppable` and its caller).
- Every session intent of `App` takes the `SessionKey` it acts on. No view under a tile reads "the focused session".
- Every `App` intent a view calls is followed by `glue::run`.
- The software never writes inside `~/.claude` and never changes the user's Claude Code settings.
- `SAVED_VERSION` stays 1. This plan adds nothing to `Saved`.
- Never assign `App.sessions`, `App.groups` or `App.tiles` wholesale after `App::new`.
- Every key this app gives an element starts with `mr-`. (The kit's viewer and dialog bring their own keys.)
- No Anthropic logos, colours or fonts.
- User-visible wording is exact where this plan quotes it. Never use a real person's name or e-mail address as test data: `alex`, `/home/alex`. Never copy text out of a real stored session or out of `target/rec/` into a test or a fixture. Test images are made by the test (a few coloured pixels), never taken from disk.
- All window tests and all screenshots use `FakeAgent`, through the demo window. No agent starts the real `claude`, sets `MINDRUNE_LIVE`, runs a test with `--ignored`, runs `scripts/install-desktop-entry.sh` outside a test's temporary folder, or starts the app outside `--demo`. Lines of the protocol that were never recorded are written by hand; the test's comment says "never seen live".
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` (no warnings) and `cargo test --workspace` pass before every commit. Say in the report which existing tests you changed and why.
- **Git.** Work on the branch `plan-7-background-pictures`, cut from `main`. Never push. Never commit `target/` or `.superpowers/`. Commit by explicit path, not `git add -A`. Another session of Jerod's may work in the same checkout: never touch, stash, reset or commit a file that is not yours. End every commit message with a blank line and `Co-Authored-By: Claude <noreply@anthropic.com>`, in exactly that form.
- Version of `claude` this plan was written against: `2.1.323`.

## Review Focus

The spec implies these conditions and no screen in it shows them. Each has a test in the task named.

1. **A message sent in the second its program goes.** The program is stopped for idleness, or ends by itself, while the user presses Enter. Expected: the message is in the conversation once, is delivered once after the restart, with its images, before anything queued behind it; nothing is lost and nothing is sent twice. Tests: Tasks 1 and 4.
2. **A program that looks idle and is not.** A background build runs with no turn; Remote Control was asked for and is not on yet; a request waits; a message is queued; the program's record of its tasks cannot be read. Expected: the program is left alone. Tests: Tasks 3 and 4.
3. **A second launch that meets something odd.** The first copy crashed and left its socket file; two copies start at the same moment; the first copy holds the lock and does not answer; a line of rubbish, or one with no end, arrives on the socket; the folder named does not exist. Expected: exactly one copy runs, a second launch ends within about two seconds with a line that says what happened, and the listener goes on working. Tests: Task 7.
4. **Keys while something lies over the window.** The quit question or the large picture is open and the user presses Escape, Ctrl+W, Ctrl+Q or types. Expected: Escape closes only what lies on top and stops no turn; no tile closes; no field takes the text; closing the window puts it away. Tests: Tasks 6 and 10.
5. **A picture whose full image cannot be had.** The session's file was removed or rewritten, the message is no longer in it, the bytes are not an image after all, or the answer comes after the viewer was closed or opened on another picture. Expected: the small picture and the line "The full picture cannot be read.", never a panic, never the wrong picture, and nothing kept once the viewer is closed. Tests: Tasks 9 and 10.

## File Structure

```
crates/agent/src/event.rs          Task 3: SessionEvent::BackgroundTasks
crates/agent/src/agent.rs          Task 9: a stored message's id; Agent::read_image, with a default
crates/agent/src/command.rs        Task 9: ImageData::shared
crates/agent/src/fake.rs           Task 2: pausing and counting history reads; Task 9: read_image
crates/claude/src/translate.rs     Task 3: the background tasks record
crates/claude/src/agent.rs         Task 3: no "ignored" line for it; Task 9: read_image
crates/claude/src/history.rs       Task 9: a message's id; one image read again
crates/claude/src/history_tail.rs  Task 9: a message's id
crates/core/src/sessions.rs        Task 1: Entry.past_open; Task 4: Entry.stopping
crates/core/src/session.rs         Tasks 1 to 4 and 9: undelivered, let_go, background, stoppable, full_image
crates/core/src/conversation.rs    Task 9: a user item's stored id
crates/core/src/app.rs             Tasks 1, 2, 4, 5, 6, 8, 9: the intents and effects named below
crates/core/src/backend.rs         Task 2: wanted histories; Task 9: reading one image
crates/app/src/main.rs             Tasks 4, 5, 7, 8: the idle length, the app's life, --quit, the handle
crates/app/src/instance.rs         Task 7 (new): the lock, the socket, the asks
crates/app/src/tray.rs             Task 8 (new): the tray's thread, its menu and its words
crates/app/src/glue.rs             Tasks 5, 6, 8, 10: closing and quitting, put_away, ask_quit, the viewer's state
crates/app/src/view/mod.rs         Tasks 6 and 10: keys, the layers
crates/app/src/view/quit.rs        Task 6 (new): the question and its words
crates/app/src/view/shell.rs       Task 6: the Quit button at the sidebar's foot
crates/app/src/view/viewer.rs      Task 10 (new): the large picture
crates/app/src/view/items.rs       Task 10: a sent picture opens
crates/app/src/view/chips.rs       Task 10: a chip's picture opens
assets/linux/mindrune.desktop      Task 7: StartupNotify and the Quit action
scripts/install-desktop-entry.sh   Task 7: an Exec line keeps its arguments
crates/app/tests/desktop.rs        Task 7
crates/app/src/demo.rs, tests/snapshots.rs, tests/live.rs, docs/   Task 11
```

## Facts this plan builds on

From the two reports; each has file and line there.

- The window's one state is `Ui` (`glue.rs`), which owns `mr_core::App` and the `Backend`. With `keep_running` Rune keeps that state and its runtime when the window closes, runs `poll` and `wake_after` timers with no window, and `AppHandle::open` shows the window again (made anew on Wayland) with the same state. `setup` does not run a second time.
- A close cannot be refused. `on_close` runs and the window goes. `Tx::quit` and `AppHandle::quit` end the app; they run `on_quit` and no `on_close`. Today `glue::closing` is the `on_close` hook and ends every program, waiting up to `CLOSE_WAIT` (2.5 s) in `close_session`.
- `AppHandle<Ui>` comes from `App::handle()` before `run`, is `Clone + Send + Sync`, and works from any thread: `open`, `activate(token)`, `update(closure)`, `wake`.
- `rune::single_instance` does something on Windows only. On Linux nothing tells a running copy that it was launched again.
- `rune::Tray::new(spec, on_event)` runs the tray on its own thread, can block its caller up to five seconds, and succeeds also where no tray host shows it. `TrayEvent::Activate { token }` and `TrayEvent::Menu(id)`. Not on macOS.
- `rune_kit::Dialog` builds a scrim and a modal panel into the overlay list `view::view` already hands to `rune_kit::root`; `El::modal` traps the focus, makes the rest inert and takes Escape. `rune_kit::image_zoom::{ImageZoom, Preview}` is a full-window viewer that draws no scrim of its own and needs the window's size from the app.
- The window's `on_key` sees a key before Rune's modal does when nothing has the focus (step 4 of 9 in `route_key`). Its Escape stops the focused tile's turn.
- `ImageCache::load(bytes: impl Into<Arc<[u8]>>)` decodes on Rune's own threads and applies a photo's orientation. A 2000 by 2000 image holds about 21 MB while a handle lives, and a 16 MB page more while it is drawn large.
- A session's phases are `Resting, Opening, Idle, Working, NeedsUser, Ended`. A clean exit of a stored session makes it `Resting`; `App::event` then lets go of `entry.open`. A resting session that this app ran before starts again on the next message without asking the store first, with `Session::start_spec()`, which carries the model, the effort and the mode it shows.
- `App::tick(now_unix)` runs on every poll, at least every 30 seconds; `Entry.last_own_unix` is when the program last reported anything and `Entry.last_active_unix` when the user last sent.
- Still true on `main` (the report's section 4): a `SendFailed` that arrives after its program's `Exited` finds no session, and a history once read is held for the run.
- The window holds only small pictures (`Ui.thumbs`, at most 320 pixels a side). A chip's full image is in `ChipState::Ready(Prepared { attachment, .. })` until `Session::hand_over` moves it into the `Send`; after that nothing holds it. A stored picture's id maps back to nothing. `Agent::read_session` is the only reader of the store's images.
- The fake agent can end a session and delay replies. It cannot pause a history read or count them.

---

The three checks, wherever a step says "run the three checks": `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`.

### Task 1: A message its program never got is sent again

The first of the two things the notes ask this plan to do first.

**Files:**
- Modify: `crates/core/src/sessions.rs`, `crates/core/src/session.rs`, `crates/core/src/app.rs`

**Interfaces:**
- Produces:
  ```rust
  // core/src/sessions.rs, in Entry
  /// The number of its last open that ended. A failed send of that open still finds its session.
  pub past_open: Option<u64>,
  impl Sessions { pub fn by_past_open(&mut self, open: u64) -> Option<EntryMut<'_>>; }

  // core/src/session.rs
  /// A message handed to a program that had ended: it is in the conversation and was never
  /// delivered. `Debug` prints the text and the number of attachments, never their bytes.
  pub struct Undelivered { pub text: String, pub attachments: Vec<Attachment> }
  // in Session
  pub undelivered: Vec<Undelivered>,
  ```

Rules:

- When `App::event` lets go of an entry's open at `Exited`, it first notes it in `past_open`.
- `App::send_failed` finds its entry by the current open, else by `past_open`. A failure of any older open is dropped, as today.
- A failed `SessionCommand::Send` whose error is `AgentError::Closed` is kept in `undelivered`, in the order the failures arrive. The message's item stays in the conversation and no second one is ever pushed for it. `handed_over` goes down by one, as today. Then, by the session's phase:
  - `Resting`: the session goes to `Opening` and its program is started with `App::start`, with no question to the store.
  - `Ended`: nothing. `App::restart` starts it and the message goes then.
  - `Opening`: nothing; it goes when the program is ready.
  - `Idle` (the session already runs under a newer open): it is sent now.
  - `Working` or `NeedsUser`: it goes when the turn ends.
- Undelivered messages go out before the queued message, oldest first, at every point where the queued message goes out today (`Session::send_queued`'s callers), and each counts as handed over again.
- A session with an undelivered message is not empty and is not forgotten (`forget_if_empty`); in `Opening` it shows the working mark, as one with a queued message does.
- Every other failed command is handled as today. A `Send` that fails for another reason than `Closed` is handled as today.

- [ ] **Step 1: Write the failing tests** in `app.rs`, driving `App::receive` with `FromBackend::Event { open, event: SessionEvent::Exited { .. } }` and then `FromBackend::SendFailed { open, command, error: AgentError::Closed }`:
  - `a_send_that_fails_after_a_clean_exit_starts_the_session_again_with_it`: the effects of the failure hold one `Effect::Open` whose `spec.resume` is the session's id; the conversation holds the user's message exactly once; after `Started` on the new open the effects hold one `Effect::Send` with `SessionCommand::Send { text: "hello", .. }`, and `undelivered` is empty.
  - `after_an_unclean_exit_it_waits_for_the_restart`: no `Effect::Open` from the failure, `undelivered.len() == 1`; `App::restart` and `Started` send it.
  - `an_undelivered_message_goes_before_the_queued_one`: two `Effect::Send`s after `Started`, the undelivered text first.
  - `an_undelivered_message_keeps_its_images`: the `ImageData` sent again is equal to the one that failed.
  - `a_failure_of_an_open_before_the_last_is_dropped`: no effect, `undelivered` empty.
  - `a_failed_answer_after_the_exit_is_still_dropped`.
  - `a_session_with_an_undelivered_message_is_not_forgotten`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `fix(core): a message its program never got is sent when the session starts again`.

### Task 2: A resting session that is not shown lets its conversation go

The second of the two.

**Files:**
- Modify: `crates/core/src/session.rs`, `crates/core/src/app.rs`, `crates/core/src/backend.rs`, `crates/agent/src/fake.rs`

**Interfaces:**
- Consumes: `Session.undelivered` (Task 1).
- Produces:
  ```rust
  // core/src/session.rs
  /// Gives the conversation back to the store: `Loaded` or `Loading` becomes `NotLoaded` and the
  /// items go. Only for a session at rest that is stored, with nothing queued or undelivered.
  /// Says whether it did.
  pub fn let_go(&mut self) -> bool;
  // core/src/app.rs
  Effect::DropHistory { key: SessionKey }
  // core/src/backend.rs
  ToBackend::DropHistory { key: SessionKey }
  // agent/src/fake.rs
  pub fn pause_reads(&self);          // read_session (and Task 9's read_image) wait until resumed
  pub fn resume_reads(&self);
  pub fn history_reads(&self) -> usize;
  ```

Rules:

- A session lets go when all of this holds: `Phase::Resting`; `is_stored()` and it has an id; it is in no tile (`Tiles::contains` is false); nothing is queued or undelivered; its entry has no `prompt` and is not `checking`; its history is `Loaded` or `Loading`. Its chips, its marks and the window's draft for it are not touched.
- It is looked at where one of these can become true: a tile is closed or given another session, a program exits, and in `App::tick`. Each session let go gives one `Effect::DropHistory`.
- Only a session whose history is `Loading` takes a `FromBackend::History`. An answer for any other is dropped.
- The backend keeps the set of keys whose history is wanted, shared with the library and the prepare threads: `ReadHistory` adds its key, `DropHistory` removes it. The library thread skips a queued read whose key is no longer wanted without touching the store; after a read it sends nothing, and makes no picture jobs, when the key stopped being wanted meanwhile. A picture job carries its key and is skipped the same way.
- A session shown again reads its history again through the existing path (`read_history` sees `NotLoaded`).
- "Continue as a copy" asks for a history too (`App::answer_prompt`). Read how it uses the answer before choosing where `let_go` is called: that read must still get its answer.

- [ ] **Step 1: Write the failing tests.**
  In `app.rs`: `closing_the_tile_of_a_resting_session_lets_its_conversation_go` (history `NotLoaded`, no items, one `Effect::DropHistory`); `showing_it_again_reads_it_again` (one `Effect::ReadHistory`); `a_session_whose_program_runs_keeps_its_conversation`; `a_queued_or_undelivered_message_keeps_it`; `a_clean_exit_of_a_session_in_no_tile_lets_go`; `a_history_for_a_session_that_let_go_is_dropped`; `the_chips_of_a_session_that_let_go_stay`.
  In `backend.rs`, with the fake paused: `a_read_that_is_dropped_before_its_turn_is_never_done` (two reads asked, the second dropped, the fake resumed: `history_reads() == 1` and one `History` arrives); `a_read_dropped_while_it_ran_answers_nothing` (no `History` and no `Picture` for it).
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `fix(core): a history not shown is let go, and a read not wanted is skipped`.

### Task 3: The program's background tasks are counted

**Files:**
- Modify: `crates/agent/src/event.rs`, `crates/claude/src/translate.rs`, `crates/claude/src/agent.rs`, `crates/core/src/session.rs`, `docs/protocol-notes.md`, and wherever a `match` on `SessionEvent` must name the new event

**Interfaces:**
- Produces:
  ```rust
  // agent/src/event.rs
  /// How many background tasks of the program are running and need it to stay up.
  SessionEvent::BackgroundTasks { running: usize }
  // core/src/session.rs, in Session
  pub background: usize,
  ```

The record, as the program's own text describes it (2.1.323), written here by hand and never seen in this shape:

```json
{"type":"system","subtype":"background_tasks_changed","tasks":[
  {"task_id":"t1","task_type":"local_bash","description":"npm run dev"},
  {"task_id":"t2","task_type":"local_agent","description":"review","off_host":true}
],"uuid":"u","session_id":"s"}
```

Rules:

- `tasks` is every live background task after the change; each record replaces the count. An entry counts unless its `off_host` is `true`. For the two tasks above the count is 1.
- A record whose `tasks` is missing or is not a list gives `running: 1`, and an entry that is not an object counts as one. The comment says why: a program is never stopped because its record could not be read.
- The program sends nothing at start: `Session.background` is set to 0 at `Started` and at `Exited`.
- The adapter no longer prints its "ignored a line" note for this kind.
- Nothing in the window shows the count. Tasks 4 and 6 read it.

- [ ] **Step 1: Write the failing tests.**
  In `translate.rs` ("never seen live"): `background_tasks_are_counted` (the record above gives `BackgroundTasks { running: 1 }`); `an_empty_list_is_no_task`; `a_list_that_cannot_be_read_counts_as_one` (`"tasks": "x"`, and no `tasks` at all); `each_record_replaces_the_count`.
  In `session.rs`: `a_new_program_has_no_background_tasks`; `an_exit_clears_them`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.** Add to `docs/protocol-notes.md` a section "Background tasks" with the record, where it was read, and that only its arrival was seen.
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(claude): the program's background tasks are counted`.

### Task 4: A program idle for half an hour is stopped unnoticed

**Files:**
- Modify: `crates/core/src/sessions.rs`, `crates/core/src/session.rs`, `crates/core/src/app.rs`, `crates/app/src/main.rs`

**Interfaces:**
- Consumes: `Session.undelivered` (Task 1), `Session::let_go` (Task 2), `Session.background` (Task 3).
- Produces:
  ```rust
  // core/src/app.rs
  pub const IDLE_SECONDS: i64 = 30 * 60;
  /// Another idle length for this run; anything under 60 is taken as 60.
  pub fn set_idle_after(&mut self, seconds: i64);
  // core/src/session.rs
  /// Whether its program can be stopped without the user noticing.
  pub fn stoppable(&self) -> bool;
  // core/src/sessions.rs, in Entry
  /// The open that is being stopped for idleness, until it has ended.
  pub stopping: Option<u64>,
  ```

Rules:

- `Session::stoppable`: `phase == Phase::Idle`, `is_stored()`, `remote == Remote::Off` and `!remote_wanted`, nothing queued, nothing undelivered, `background == 0`, and `conversation.waiting() == 0`.
- `App::tick` stops an entry when it has an open, is not already `stopping`, its session is `stoppable`, it has no `prompt` and is not `checking`, and `now_unix - max(last_own_unix or 0, last_active_unix) >= ` the idle length. Stopping is `Effect::Send { open, command: SessionCommand::Close }`, once, and `stopping = Some(open)`.
- The `Exited` of an open that was `stopping` is taken as clean whatever it says: the session rests, with no notice and no mark. `stopping` is cleared.
- Nothing else about the session changes: its conversation, chips, model, effort, mode and marks stay. The next message starts it through the existing path, which asks the store nothing because this app ran it.
- A message sent between the `Close` and the `Exited` fails as `Closed` and takes Task 1's path.
- A stopped session that is in no tile lets go as Task 2 says.
- `main.rs` reads `MINDRUNE_IDLE_SECONDS` outside `--demo`: a whole number sets the length through `set_idle_after`; anything else prints `mindrune: MINDRUNE_IDLE_SECONDS is not a number of seconds; using 1800` on stderr.

- [ ] **Step 1: Write the failing tests** in `app.rs` (`NOW` is the tests' clock):
  - `a_stored_session_idle_for_half_an_hour_is_closed`: `tick(NOW + 1799)` gives no `Close`; `tick(NOW + 1800)` gives exactly one `Effect::Send { command: SessionCommand::Close, .. }` for its open; a second `tick` gives none.
  - `what_keeps_a_program_running`: one case each for a turn running, a request waiting, Remote Control on, Remote Control asked for and not on, a queued message, an undelivered message, `background == 1`, the "open elsewhere" question, and a session that never had a message. None is closed at `NOW + 7200`.
  - `anything_from_the_program_moves_the_deadline`: an event at `NOW + 1000`; not closed at `NOW + 1800`, closed at `NOW + 2800`.
  - `after_a_quiet_stop_the_session_rests_as_it_was`: after `Exited { clean: true, .. }` the phase is `Resting`, `open` is `None`, the conversation's items, the model, the effort and the mode are unchanged, and it has no mark.
  - `a_quiet_stop_that_had_to_kill_still_rests`: `Exited { clean: false, .. }` gives `Resting` and no notice.
  - `the_next_message_starts_it_as_it_was`: one `Effect::Open` whose `spec` has `resume`, `model`, `effort` and `permission_mode` as before, and no `Effect::CheckActivity`.
  - `a_message_sent_while_it_stops_is_delivered_after_the_restart`: submit after the `Close`; then `Exited`, then `SendFailed { error: AgentError::Closed, .. }`; one `Effect::Open`; after `Started`, one `Send` with the text; the conversation holds the message once.
  - `the_idle_length_can_be_set_and_never_under_a_minute`.
  In `main.rs`: the environment value is read (`"120"` gives 120; `"soon"` gives the default).
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): a program idle for half an hour is stopped unnoticed`.

### Task 5: Closing the window keeps the app running

**Files:**
- Modify: `crates/app/src/main.rs`, `crates/app/src/glue.rs`, `crates/core/src/app.rs`

**Interfaces:**
- Produces:
  ```rust
  // core/src/app.rs
  /// Whether a window shows. While none does, a turn that ends leaves its session unseen also
  /// in the focused tile; when one shows again the focused session counts as seen.
  pub fn set_window(&mut self, shown: bool) -> Vec<Effect>;
  // app/src/glue.rs
  pub fn closing(ui: &mut Ui, tx: &mut Tx);    // on_close: the window goes, the app stays
  pub fn quitting(ui: &mut Ui, tx: &mut Tx);   // on_quit: what `closing` does today
  pub fn shown(ui: &mut Ui, shown: bool, tx: &mut Tx);   // on_visibility
  /// Shuts what lies over the window: menus, pickers, a rename, a drag. Tasks 6 and 10 add theirs.
  pub fn put_away(ui: &mut Ui);
  // in Ui
  pub quitting: bool,   // the programs were told to end; set once
  ```

Rules:

- `main.rs`, outside `--demo` and outside a headless run, on Unix: `.keep_running().on_close(glue::closing).on_quit(glue::quitting).on_visibility(glue::shown)`. In `--demo`, headless, and on other systems: `.on_close(glue::quitting)`, which is today's behaviour under a new name.
- `glue::closing`: when `rune::quitting()` is true it does nothing. Otherwise it saves (`App::save_now`, run through `glue::run`), calls `put_away`, and tells the core that no window shows.
- `glue::quitting`: when `ui.quitting` is already set it does nothing. Otherwise it sets it and does what `closing` does today (`close_session(ui, CLOSE_WAIT)`): saves, tells every program to close, and waits at most 2.5 seconds for them. This is the one place the UI thread waits.
- `put_away`: `side_menu` and `folder_menu` shut, every `SessionView.open_picker` shut, a rename ended without keeping it, `drag` and `file_drop` cleared. A draft and its chips are not touched.
- `glue::poll` must work with no window: it already only drains the backend, ticks and asks for its next wake. Keep it so; nothing in it may need a frame.
- A turn that ends while no window shows marks its session unseen also when it is the focused one. `set_window(true)` counts the focused session as seen, as focusing it does.

- [ ] **Step 1: Write the failing tests.**
  In `app.rs`: `a_turn_that_ends_with_no_window_is_unseen_also_in_the_focused_tile`; `the_window_coming_back_counts_the_focused_session_as_seen`.
  In `glue.rs`: `closing_the_window_ends_no_program` (after `closing`, the fake's `sent()` holds no `SessionCommand::Close`, the session is still `Idle`, and the state was saved); `a_turn_goes_on_with_the_window_closed` (after `closing`, the fake's reply arrives through `poll` and the conversation has it); `closing_puts_away_what_was_open` (a picker open and a rename under way are gone, the draft's text is still there); `quitting_ends_every_program_once` (two calls of `quitting` send one `Close` per session); the existing tests of `closing` move to `quitting` unchanged in what they assert.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): closing the window keeps the app and its sessions running`.

### Task 6: Quit, and its question

**Files:**
- Create: `crates/app/src/view/quit.rs`
- Modify: `crates/core/src/app.rs`, `crates/app/src/glue.rs`, `crates/app/src/view/mod.rs`, `crates/app/src/view/shell.rs`

**Interfaces:**
- Consumes: `glue::quitting`, `glue::put_away`, `Ui.quitting` (Task 5); `Session.background` (Task 3); `Session.undelivered` (Task 1).
- Produces:
  ```rust
  // core/src/app.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub struct Busy { pub working: usize, pub remote: usize }
  pub fn busy(&self) -> Busy;
  // app/src/glue.rs
  // in Ui
  pub quit_ask: bool,
  /// Quits, or asks first when something would be cut off. True when the question now shows.
  pub fn ask_quit(ui: &mut Ui, tx: &mut Tx) -> bool;
  /// Ends the programs (`quitting`) and the app (`Tx::quit`).
  pub fn quit_now(ui: &mut Ui, tx: &mut Tx);
  // app/src/view/quit.rs
  pub fn is_quit_key(ev: &KeyEvent) -> bool;                        // ev.is_command('q')
  pub fn words(busy: Busy) -> String;
  pub fn question(ui: &Ui, kit: &Kit, layer: &mut Vec<El>);
  ```

Rules:

- `Busy.working` counts the sessions that are `Working` or `NeedsUser`, or `Opening` with a message queued or undelivered, or have `background > 0`. `Busy.remote` counts those whose `remote` is not `Remote::Off`. One session can be in both.
- `words`, exactly:
  - working only: `1 session is working.` or `3 sessions are working.`
  - Remote Control only: `1 session has Remote Control on.` or `2 sessions have Remote Control on.`
  - both: `2 sessions are working and 1 has Remote Control on.` (the second part says `has` for one and `have` otherwise)
  - then always, after one space: `Quitting stops everything that is running. Sessions stay in the history and can be continued.`
- `ask_quit`: with the question already showing it returns true. With `busy()` all zero it calls `quit_now` and returns false. Otherwise it calls `put_away`, sets `quit_ask` and returns true.
- `quit_now` calls `glue::quitting` itself and then `tx.quit()`, so the programs are told to end whether or not Rune runs `on_quit` for a window that is closed.
- The question is the kit's dialog, pushed into the overlays: `Dialog::new("mr-quit", "Quit MindRune?")` with `words(busy)` as its description and two buttons as its child, `mr-quit-cancel` "Cancel" and `mr-quit-yes` "Quit", in that order; "Quit" is the default button. Closing it by Escape, a press outside, its own close button or "Cancel" clears `quit_ask`. Its words follow `busy()` while it shows.
- `put_away` also clears `quit_ask`.
- Keys in `view::on_key`: Ctrl+Q (`is_quit_key`) calls `ask_quit` on every screen, first of all. While `quit_ask` is set, `on_key` handles no key at all and returns false, so that Escape reaches the dialog and stops no turn, and Ctrl+W closes no tile.
- The sidebar's foot: the account's line becomes a row with the account's name and, at its right end, a quiet button `mr-quit` with the text "Quit" and the help "Quit MindRune (Ctrl+Q)". It shows also when no account is known. A press calls `ask_quit`.

- [ ] **Step 1: Write the failing tests.**
  In `app.rs`: `busy_counts_what_a_quit_would_cut_off` (one session each: working, waiting for the user, opening with a queued message, idle with a background task, idle with Remote Control on, idle with nothing; `Busy { working: 4, remote: 1 }`).
  In `quit.rs`: `the_words_say_what_would_stop`, with the five strings above in full.
  View tests: `ctrl_q_with_nothing_running_quits_at_once` (`ui.quitting`, no dialog; once with the keyboard in a message box and once with nothing focused); `ctrl_q_while_a_turn_runs_asks` (`mr-quit-dialog` shows with the words for one working session; the fake got no `Close`); `cancel_closes_the_question_and_nothing_stopped`; `escape_closes_the_question_and_stops_no_turn` (the fake got no interrupt; the turn is still running); `ctrl_w_under_the_question_closes_no_tile`; `typing_under_the_question_reaches_no_box` (the draft is unchanged); `quit_in_the_question_ends_the_programs` (`ui.quitting`, one `Close` per live session); `return_presses_quit`; `the_quit_button_in_the_sidebar_asks_the_same`; `the_question_is_a_dialog_to_a_screen_reader` (on `rt.accessibility_json()`: role dialog, name "Quit MindRune?").
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): Quit, with a question when something is running`.

### Task 7: One running copy, and launching again

**Files:**
- Create: `crates/app/src/instance.rs`
- Modify: `crates/app/src/main.rs`, `crates/app/src/glue.rs`, `assets/linux/mindrune.desktop`, `scripts/install-desktop-entry.sh`, `crates/app/tests/desktop.rs`

**Interfaces:**
- Consumes: `glue::ask_quit` (Task 6); the `keep_running` branch of `main.rs` (Task 5).
- Produces:
  ```rust
  // app/src/instance.rs  (the real thing under #[cfg(unix)]; elsewhere `claim` gives `Claim::Alone`)
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum Ask {
      /// Show the window; with a folder, open a new session there.
      Show { token: Option<String>, folder: Option<PathBuf> },
      Quit,
  }
  pub enum Claim { First(Listener), Second, Alone }
  /// `$XDG_RUNTIME_DIR/mindrune`, else `run` beside the app's own saved file.
  pub fn folder() -> PathBuf;
  pub fn claim(dir: &Path) -> io::Result<Claim>;
  /// Tells the running copy, and waits at most `ANSWER_WAIT` for its "ok".
  pub fn tell(dir: &Path, ask: &Ask) -> io::Result<()>;
  pub const ANSWER_WAIT: Duration = Duration::from_secs(2);
  pub const LINE_LIMIT: usize = 4096;
  pub fn encode(ask: &Ask) -> String;          // one line of JSON, no line break inside
  pub fn decode(line: &str) -> Option<Ask>;
  impl Listener {
      /// Serves on a thread named `mindrune-instance` until the process ends.
      pub fn serve(self, on_ask: impl Fn(Ask) + Send + 'static);
  }
  // app/src/glue.rs
  /// The app was launched again: with a folder that exists, a new session opens there.
  pub fn launched_again(ui: &mut Ui, folder: Option<PathBuf>);
  ```

Rules:

- **The claim.** `claim` makes `dir` with mode `0700` if it is missing, opens the file `lock` in it and tries `File::try_lock`. With the lock: it removes a `sock` file left behind, binds a `UnixListener` at `sock`, writes its process id into `lock`, and returns `First`; the listener holds the lock file open for the life of the process. Without the lock, because another holds it: `Second`. Any other error is returned.
- **The wire.** The asker tries to connect until `ANSWER_WAIT` is over, because a first copy holds the lock a moment before its socket is there. It then writes `encode(ask)` and a line break, and reads `ok`. The lines are `{"ask":"show"}`, with `"token"` and `"folder"` when there are any, and `{"ask":"quit"}`. `decode` takes nothing else: unknown keys, another `ask`, or text that is not one JSON object give `None`.
- **The listener** takes one connection at a time. It reads up to the first line break, at most `LINE_LIMIT` bytes, for at most `ANSWER_WAIT`. A line it cannot decode, too long or too late: it closes that connection, answers nothing and goes on. A good one: it calls `on_ask` and answers `ok`.
- **`main.rs`.** `parse` learns `--quit` (with a folder it is an error: `--quit takes no folder`). Outside `--demo` and a headless run:
  - `--quit` with a running copy: `tell(Ask::Quit)`, exit 0. With none: print `mindrune: not running` on stderr, exit 0, and start nothing.
  - A plain launch that is `Second`: `tell(Ask::Show { token, folder })`, where the token is `XDG_ACTIVATION_TOKEN`, else `DESKTOP_STARTUP_ID`, from its own environment, and the folder is the launch folder made absolute. Print `mindrune: already running; its window is shown` on stderr and exit 0. If the tell fails: `mindrune: a running copy does not answer ({the lock file's path})`, exit 1.
  - `First`: take `App::handle()` before `run` and serve. For `Show`: `handle.update` with `glue::launched_again`, then `handle.open()`, then `handle.activate(token)` when there is one. For `Quit`: `handle.update(move |ui, tx| if glue::ask_quit(ui, tx) { again.open() })` with a clone of the handle.
  - `Alone`, or a claim that failed (print why on stderr): the app runs as in `--demo`, where the window's close ends it. Without a way back it must not stay behind.
- `launched_again` does nothing unless the screen is the main one and the folder exists (`ui.folder_exists`); then it opens a new session there as the folder dialog's choice does.
- The token is never printed.
- **The desktop entry** gains `StartupNotify=true`, `Actions=quit;` and, at the end:
  ```
  [Desktop Action quit]
  Name=Quit MindRune
  Exec=mindrune --quit
  ```
  The install script writes the program's path into every `Exec=mindrune` line and keeps what follows the name: `Exec="/path/to/mindrune" --quit`.
- On macOS nothing more is written: Rune's own reopening shows a window closed under `keep_running` when the Dock icon is pressed.

- [ ] **Step 1: Write the failing tests.**
  In `instance.rs`, each in its own `tempfile` folder: `the_first_claims_and_the_second_is_told_so`; `a_socket_left_by_a_copy_that_died_is_taken_over` (claim, drop the listener without removing `sock`, claim again: `First`); `an_ask_goes_through` (`Show` with a token and a folder whose name has a space and a quote, then `Quit`, each received equal to what was sent); `what_is_written_is_read_back`, and `decode` refuses `{"ask":"run","cmd":"x"}`, `[]`, `show` and an empty line; `rubbish_and_a_line_without_end_are_dropped_and_the_listener_lives` (10,000 bytes with no line break on one connection, `hello` on a second, then a good ask is answered); `a_first_copy_that_does_not_answer_is_an_error_not_a_wait` (claim, never serve; `tell` fails in under three seconds); `a_second_that_comes_before_the_socket_waits_for_it` (the lock held and the socket bound half a second later: `tell` succeeds); `the_folder_is_only_the_users` (mode `0700`).
  In `main.rs`: `--quit` parses; `--quit` with a folder is an error.
  In `glue.rs`: `a_second_launch_with_a_folder_opens_a_session_there`; `with_a_folder_that_is_gone_it_opens_nothing`.
  In `tests/desktop.rs`: the entry in the repository has `StartupNotify=true` and the action; the installed entry's two `Exec` lines both carry the quoted program, and the second ends in ` --quit`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): one running copy; launching again shows its window`.

### Task 8: The tray

**Files:**
- Create: `crates/app/src/tray.rs`
- Modify: `crates/app/src/main.rs`, `crates/app/src/glue.rs`, `crates/core/src/app.rs`

**Interfaces:**
- Consumes: `glue::ask_quit` (Task 6); the handle in `main.rs` (Task 7); `mr_core::picture_of`.
- Produces:
  ```rust
  // core/src/app.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub struct Standing { pub working: usize, pub needs_you: usize, pub finished: usize }
  /// The sessions by their mark (`Entry::mark`).
  pub fn standing(&self) -> Standing;
  // app/src/tray.rs
  pub fn status_line(standing: Standing) -> Option<String>;
  pub fn menu(standing: Standing) -> Vec<TrayItem>;
  /// Starts the tray on its own thread. The sender tells it a new standing.
  pub fn start(handle: AppHandle<Ui>) -> Sender<Standing>;
  // app/src/glue.rs, in Ui
  pub tray: Option<Sender<Standing>>,
  tray_said: Standing,
  ```

Rules:

- `status_line`: the parts that are not zero, in this order, joined by ` · `: `2 working`, `1 needs you` (or `3 need you`), `1 finished`. All zero: `None`.
- `menu`: when there is a status line, an item with the id `status` and that text, not enabled, and a separator; then `show` "Show MindRune", a separator, `quit` "Quit".
- `start` spawns the thread `mindrune-tray`. On it: the icon is `assets/icon/mindrune-64.png`, built into the program and turned into pixels with `mr_core::picture_of`; then `Tray::new(TraySpec::new("mindrune", "MindRune", icon).tooltip("MindRune").menu(menu(Standing::default())), on_event)`. If that fails it prints `mindrune: no tray icon: {error}` once on stderr and the thread ends. Otherwise it waits on the receiver and calls `set_menu(menu(standing))` for each standing; it ends when the sender is gone.
- `on_event` runs on the tray's own thread and only hands work over. `Activate { token }` and `Menu("show")`: `handle.open()`, and `handle.activate(token)` with a token. `Menu("quit")`: as `Ask::Quit` in Task 7. Any other id: nothing.
- `main.rs` starts it only where Task 5's `keep_running` branch runs and the claim was `First`, and gives the sender to `Ui` with `handle.update`.
- `glue::poll`, after it has taken the backend's messages: when `ui.app.standing()` differs from `tray_said`, it sends it and remembers it. A send that fails (no tray) is ignored.
- `Tray::new` itself is not tested here: it needs a desktop's session bus.

- [ ] **Step 1: Write the failing tests.**
  In `app.rs`: `the_standing_counts_sessions_by_their_mark`.
  In `tray.rs`: `the_status_line_names_what_is_not_zero` (`Standing { working: 2, needs_you: 1, finished: 1 }` gives `2 working · 1 needs you · 1 finished`; `needs_you: 3` alone gives `3 need you`; all zero gives `None`); `the_menu_has_show_and_quit_and_a_status_only_when_there_is_one` (ids in order: `show`, `quit` with nothing to say; `status`, `show`, `quit` otherwise, the status not enabled).
  In `glue.rs`: `the_tray_is_told_when_the_standing_changes_and_only_then` (two polls with nothing new send one standing; a turn that starts sends a second).
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): a tray icon with Show and Quit`.

### Task 9: A picture's full image can be had again

**Files:**
- Modify: `crates/agent/src/agent.rs`, `crates/agent/src/command.rs`, `crates/agent/src/fake.rs`, `crates/claude/src/history.rs`, `crates/claude/src/history_tail.rs`, `crates/claude/src/agent.rs`, `crates/core/src/conversation.rs`, `crates/core/src/session.rs`, `crates/core/src/app.rs`, `crates/core/src/backend.rs`

**Interfaces:**
- Consumes: `Session::let_go` (Task 2); `FakeAgent::pause_reads` (Task 2).
- Produces:
  ```rust
  // agent/src/command.rs
  impl ImageData { /// The bytes behind their shared pointer, without a copy.
      pub fn shared(&self) -> Arc<[u8]>; }
  // agent/src/agent.rs: HistoryItem::User gains
  /// The message's own id in the store, when it has one.
  stored: Option<String>,
  // agent/src/agent.rs, in trait Agent, with a default that returns an error
  /// One image of one stored message, read again: `index` counts the message's images from 0.
  fn read_image(&self, id: &SessionId, message: &str, index: usize) -> Result<ImageData, AgentError>;
  // agent/src/fake.rs
  pub fn fail_image(&self, detail: &str);
  // core/src/conversation.rs: ItemBody::User gains
  stored: Option<String>,
  // core/src/session.rs
  pub enum FullImage {
      AtHand(ImageData),
      Stored { id: SessionId, message: String, index: usize },
      Nowhere,
  }
  pub fn full_image(&self, picture: u64) -> FullImage;
  // core/src/app.rs
  /// The full image behind a picture of `key`'s session: its bytes when they are at hand, else
  /// the effect that reads them from the store, else neither.
  pub fn full_image(&self, key: SessionKey, picture: u64) -> (Option<ImageData>, Vec<Effect>);
  Effect::ReadImage { picture: u64, id: SessionId, message: String, index: usize }
  // core/src/backend.rs
  ToBackend::ReadImage { picture: u64, id: SessionId, message: String, index: usize }
  FromBackend::Image { picture: u64, result: Result<ImageData, AgentError> }
  ```

Rules:

- `Session::full_image` looks, in this order: a chip of the session with that id that is `ChipState::Ready` with an image attachment; the same among a queued message's chips; the images kept from this run's sends; the conversation's user item that holds the picture's id and has a `stored` id, which gives `Stored` with the picture's place among that item's pictures as `index`; else `Nowhere`.
- `Session::hand_over` keeps, for each image it sends, a clone of its `ImageData` under the chip's id, in a private map of the session. A clone shares the bytes. The map loses an id that is no longer among `picture_ids()`, and `let_go` empties it.
- Both history readers fill `stored` from the record's `uuid`. A history item becomes a conversation item with it.
- The adapter's `read_image`: finds the session's file as `read_session` does, reads it line by line from the start, looks only at lines that contain the message's id, and takes the record whose `uuid` equals it; then `stored_images(&record)` gives the image at `index`. The whole file is read and no line is held longer than it is looked at. Errors, as `AgentError::Io` with these words: `the session is not in the store`, `the message is not in the session`, `the message has no such image`.
- The backend runs `ReadImage` on the library thread and answers `FromBackend::Image` with the same picture id. `App::receive` passes that message by; Task 10's window takes it.
- The fake answers `read_image` from the history it was given, by `stored` and `index`; after `fail_image` every read fails with that detail; reads wait while paused.
- No image's bytes are printed.

- [ ] **Step 1: Write the failing tests.**
  In `claude/src/history.rs` (hand-written records, images made in the test, "never seen live"): `a_stored_message_has_its_id`; `an_image_is_read_again_by_its_message` (the second of two images, equal to what the record holds); `a_message_at_the_start_of_a_long_file_is_still_found`; `a_message_that_is_gone_is_an_error`; `an_index_past_the_messages_images_is_an_error`; `a_line_that_only_mentions_the_id_is_not_the_message` (another record's `parentUuid` names it).
  In `core/src/session.rs`: `a_ready_chip_gives_its_image_at_hand`; `a_sent_picture_is_at_hand_and_shares_its_bytes` (`Arc::ptr_eq` between `shared()` of what was sent and of what is kept); `a_stored_picture_is_asked_from_the_store` (`Stored` with the right `index` for the second picture of a message); `a_picture_nobody_holds_is_nowhere`; `letting_go_drops_the_kept_images`.
  In `core/src/backend.rs`: `an_image_read_comes_back_under_its_picture_id`; `a_failed_read_comes_back_as_its_error`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat: a picture's full image can be had again`.

### Task 10: A press opens the picture

**Files:**
- Create: `crates/app/src/view/viewer.rs`
- Modify: `crates/app/src/glue.rs`, `crates/app/src/view/mod.rs`, `crates/app/src/view/items.rs`, `crates/app/src/view/chips.rs`

**Interfaces:**
- Consumes: `App::full_image`, `Effect::ReadImage`, `FromBackend::Image`, `ImageData::shared` (Task 9); `glue::put_away` (Task 5); the rule for keys under an overlay (Task 6).
- Produces:
  ```rust
  // app/src/glue.rs
  pub struct Viewing {
      pub picture: u64,
      pub key: SessionKey,
      /// The full image, once its bytes are there. It may still be decoding.
      pub full: Option<Image>,
      pub failed: bool,
  }
  // in Ui
  pub viewing: Option<Viewing>,
  pub zoom: ImageZoom,
  pub fn open_picture(ui: &mut Ui, key: SessionKey, picture: u64);
  pub fn close_picture(ui: &mut Ui);
  // app/src/view/viewer.rs
  pub fn layer(ui: &Ui, cx: &Cx, kit: &Kit, layer: &mut Vec<El>);
  ```

Rules:

- `open_picture` focuses the picture's tile as a press there does today, calls `put_away`, resets `zoom`, and asks `App::full_image`: bytes at hand are given to `ImageCache::load` at once; an effect is run; with neither, `failed` is set.
- `glue::take` keeps `FromBackend::Image` for itself: when a viewer is open on that picture id, a good answer is loaded and a bad one sets `failed`. An answer for any other picture is dropped.
- What the viewer draws: the full image once it `is_ready()` without an error; otherwise the small picture from `Ui.thumbs`. An image that failed to decode sets `failed`. With `failed`, the line `mr-viewer-error` "The full picture cannot be read." shows near the bottom edge.
- The layer, last among the overlays: a scrim `mr-viewer-scrim` over the whole window in the kit's scrim colour, then the kit's `Preview` with the shown image's size as its natural size (a pixel a point), the window's size read from the root element's last layout, and `on_close` calling `close_picture`. `glue::poll` calls `ImageZoom::poll` while a viewer is open.
- `close_picture` clears `viewing`, which drops the full image's handle, and resets `zoom`. `put_away` calls it. `tidy_views` calls it when the picture's id is no longer among its session's `picture_ids()` or the session is gone.
- While a viewer is open, `view::on_key` handles no key and returns false, as under the quit question.
- A sent picture that has loaded, `mr-user-picture-{id}`, takes a press: `open_picture`. Its name becomes "Open image N of M". It stays out of the Tab order, and the press leaves the keyboard where it is. The outlined box of a picture not yet loaded is unchanged and takes no press.
- A chip's picture, `mr-chip-thumb-{id}`, becomes a button built as the chip's remove control is: a Tab stop, pressed by the pointer or with Space, named "Open {the chip's name}". A chip that shows a sign and no picture is unchanged.

- [ ] **Step 1: Write the failing tests** (view tests, through the demo window):
  - `a_press_on_a_sent_picture_opens_it_over_the_whole_window`: `mr-viewer-scrim` and `image-preview` both have the window's rectangle, sidebar included; `ui.viewing` names the picture and its session.
  - `escape_closes_the_picture_and_stops_no_turn`: with a turn running, Escape closes the viewer and the fake got no interrupt; a second Escape stops the turn.
  - `a_press_beside_the_picture_closes_it`.
  - `ctrl_w_under_the_picture_closes_no_tile`.
  - `a_chip_picture_opens_what_will_be_sent`, and `space_on_a_focused_chip_picture_opens_it`.
  - `a_stored_picture_shows_small_until_the_store_answers`: with the fake's reads paused the viewer is open and `full` is `None`; resumed, `full` is there.
  - `a_picture_the_store_cannot_give_says_so`: after `fail_image`, the text "The full picture cannot be read." shows and the viewer stays open.
  - `an_answer_for_another_picture_is_dropped`: open one, close it, open a second while the first's read is paused; resumed, the viewer still names the second and is not `failed`.
  - `a_picture_not_loaded_does_not_open`.
  - `closing_the_window_closes_the_picture`: after `glue::closing`, `ui.viewing` is `None`.
  - `a_picture_whose_session_let_go_closes`.
  - `a_screen_reader_hears_a_button_that_opens_the_picture`, on `rt.accessibility_json()`: a button named "Open image 1 of 2"; the not-loaded one still an image named "Image 2 of 2, not loaded".
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): a press on a picture opens it large`.

### Task 11: Scenes, the real path, notes, and Jerod's look

**Files:**
- Modify: `crates/app/src/demo.rs`, `crates/app/tests/snapshots.rs`, `crates/app/tests/live.rs`, `docs/protocol-notes.md`, `docs/notes-for-later-plans.md`

Rules:

- **Scenes**, in both lists, dark and light: `quit-ask` (two tiles, one with a turn running and one with Remote Control on, and the question over them: "1 session is working and 1 has Remote Control on."), `viewer` (a sent picture open large: an image of 900 by 600 made in code from coloured blocks), and `viewer-failed` (a stored picture whose read failed: the small picture and the line). The demo reads nothing from the real disk.
- **The live test** (never run by an agent) prints, and never fails on:
  - `live: a program closed while idle ended cleanly: yes|no`, then, after opening the same session again with the same model, effort and mode and sending one short message: `live: started again, the session answered: yes|no`
  - `live: the stored message has an id: yes|no`, and `live: its image read again by that id: N bytes, the same as were sent: yes|no`
  - `live: background task records that arrived: N` and, when there was one, `live: the last one counted M task(s)`
- **`docs/protocol-notes.md`:** "Background tasks" (from Task 3) and a section "A stored image, read again", each saying what the live test settles.
- **`docs/notes-for-later-plans.md`:** a first section "Jerod's look at plan 7": the commands, "Before looking" (this plan's "What this window will not do yet", and: quit any running copy first; the desktop entry must be installed again for the launcher's token and its Quit entry, which is Jerod's to run), what to try, every decision of this plan with what was built, the rulings made while it ran, and what is left.
- **What to try in the window:**
  1. Start a long turn and close the window. Launch MindRune again: the same tiles, the turn still going or done, a draft still in its box.
  2. The tray icon in the top bar: a click shows the window; its menu has the standing, "Show MindRune" and "Quit".
  3. With the window closed, launch from the launcher and from a terminal: does the window come to the front both times?
  4. `mindrune ~/some/folder` while it runs: a new session in that folder.
  5. Ctrl+Q with nothing running: it quits at once. With a turn running: the question; Cancel; then Quit.
  6. The "Quit" button at the sidebar's foot, and "Quit MindRune" on the launcher icon's menu.
  7. Remote Control on, close the window, write from the other device: it still answers. Quit then asks.
  8. `MINDRUNE_IDLE_SECONDS=120 cargo run --release -p mindrune`: leave a session alone for three minutes, once with the window open and once closed. Its `claude` process is gone (the system monitor); the next message is answered, with the same model and effort.
  9. A press on a sent picture: large over the window, the zoom bar, Escape, a press beside it. The same in a session opened again, and on a chip before sending.
  10. Close the window with a picture open and come back: no picture, the same tiles.

- [ ] **Step 1:** The scenes and their snapshots; look at each PNG in both themes and say what you see.
- [ ] **Step 2:** The live test's lines (compile only: `cargo test -p mindrune --test live --no-run`).
- [ ] **Step 3:** The notes.
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): scenes for the quit question and the large picture`, `test(live): a quiet stop, an image read again, background tasks`, `docs: plan 7, in the notes`.

## After the last task

A whole-branch review on the most capable model, one fix wave, one re-review. Then Jerod runs the live test and looks; the branch is merged only on their word.
