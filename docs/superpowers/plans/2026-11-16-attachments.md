# Attachments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A message can carry images and files. An image pasted from the clipboard, a file picked with a button, a file copied in the file manager and pasted, and a file dropped on a tile each become a chip over the text in that tile's input box. A chip shows a thumbnail or a file sign, its name, a way to remove it, and its problem when it has one. Images go to Claude as images, other files by their path. The input box no longer grows out of a short tile.

**Architecture:** The seam already carries attachments and the adapter already encodes them; neither was ever fed by the window. `mr-core` gains the chips of a session (read, checked and shrunk on the library thread, never in a view), and a queued message carries its chips wherever its text goes. The adapter gets one writer thread for the program's input, so a megabyte line can never lock against the reader. The window builds the message field from Rune's own text field, because the kit's text area cannot hear a paste, and draws the chips, the attach button and a drop veil.

**Tech Stack:** Rust 1.99.1 (edition 2024), `rune` and `rune-kit` at the pinned revision, `serde`, `serde_json`, `base64` 0.22 (now also in `mr-agent`), std threads and channels, `tempfile` for tests. One new direct dependency: `image` 0.25 in `mr-core` (already in `Cargo.lock` at 0.25.10 through Rune), with `default-features = false` and the features `png`, `jpeg`, `gif`, `webp`.

**Spec:** `docs/superpowers/specs/2026-11-10-claude-code-ui-design.md`: the attachments of section 5.5 (lines 197 to 203), "Attachment too large or unreadable" under the error cases (line 481), and the test "send a message with attachments". Read these before starting, in this order: the spec's lines, `docs/notes-for-later-plans.md` (search for "plan 6" and "For plan 6"), then this plan.

**For the controller:** Jerod's rule since 2026-11-13: every subagent (implementer, reviewer, fixer) runs on Opus 5.5 at high effort or better. Nothing less, whatever the size of the task. Two research reports made for this plan are on disk and not in git: `.superpowers/sdd/plan-6-research/code-as-built.md` (the code on `main` at `5cfc31a`, with file and line for everything this plan touches) and `.superpowers/sdd/plan-6-research/rune-for-attachments.md` (what Rune at the pinned revision offers for pasting, drops, dialogs and images, with file and line in Rune's source). Give each implementer the report its task needs. Where a report and the code differ, the code wins.

## This is plan 6 of 7

| Plan | Delivers |
| --- | --- |
| 1 to 5 (done) | Foundations and the Claude adapter, one session in a window, many sessions, tiles, stacked tiles, the right panel, Remote Control, the effort level, the context breakdown |
| **6. Attachments (this plan)** | Pasting an image, attaching and dropping files, the chips, the input box's height in a short tile, one writer for the program's input |
| 7. Running in the background | Closing the window keeps sessions working, the tray, Quit rules, stopping idle programs |

**Not in this plan, on purpose.** Drawing a sent image in the conversation (a sent message says how many images it had, as a stored one does today). Keeping a draft or its chips when the app is closed (a draft's text is not kept today either). PDFs and other documents as documents: they go by path. Pop-up menus for `@` file mentions and slash commands. Running without a window (plan 7). Task 13 of plan 5, which still waits for Jerod's recording.

## What rests on what was never seen

No agent runs Claude Code. What this plan needs from it:

| Point | Read where | Seen working | Made sure by |
| --- | --- | --- | --- |
| A user message with a PNG image block is answered | recording V4 (2026-11-10) | yes | the live test, again |
| JPEG, GIF and WebP image blocks are accepted | the Messages format | no | nothing in this plan; Jerod's look, by attaching one |
| A path under "Attached files:" in the text makes Claude read the file | built in plan 1 | no | the live test |
| How the program stores a message with an image and with the "Attached files:" text | nowhere (the history reader's test is hand-written) | no | the live test, which reads the stored session back |
| The limits: 2,000 pixels a side and 5 MB once encoded | the program's own text (its image settings) | no | nothing: the app stays under them by itself |
| A pasted image arrives in a GNOME Wayland session | Rune's source | no | Jerod's look |
| The file dialog opens through the desktop portal in a Wayland session | Rune's source; the folder dialog already works | the folder dialog, yes | Jerod's look |

Should the live test show that the stored form differs from what the history reader expects, Task 3's reader is corrected from the test's printed shape before the merge.

## What this window will not do yet

Say this to Jerod before their look.

- **Dropping a file onto the window does nothing in a Wayland session**, which is what Jerod's desktop runs. The toolkit hears no file drops there (winit 0.30 reports none). The drop is built and tested, and works under X11. Until the toolkit hears drops on Wayland the ways in are: paste an image, copy files in the file manager and paste them, or the attach button.
- **The attach button picks one file at a time.** The toolkit's dialog has no way to pick several. Press it again for the next.
- **The button shows a plus sign, not a paperclip.** The theme has no paperclip.
- A sent image is not drawn in the conversation. The message says "1 image".
- Chips are lost when the app closes, as a draft's text is.
- An image larger than 2,000 pixels a side, or heavier than about 3.9 MB, is made smaller before it is sent. The file on disk is never changed.
- A file that is not a PNG, JPEG, GIF or WebP image goes by its path, also a PDF, an SVG or a HEIC photo. Claude reads it from disk with its own tools.

## Decisions in this plan that the design does not make, or makes differently

Each is the planner's call. Jerod has not seen them; they are listed in the handover for review.

1. **Drops are built although Jerod's desktop cannot deliver them.** The spec asks for them, the code is small, and it works the day the toolkit hears them. The other choice is to leave them out until then.
2. **Copied files can be pasted.** Files copied in the file manager and pasted into the box become chips. The spec does not ask for it; on Wayland it stands in for the drop.
3. **One file per press of the attach button**, and a plus sign for its face. Both are limits of the toolkit. Picking several would mean the app talking to the desktop portal itself.
4. **The chips sit in a row over the text, and the row scrolls sideways.** It never wraps, so the box's height does not depend on how many chips there are.
5. **What is an image is decided by the file's first bytes, not its name.** PNG, JPEG, GIF and WebP are images. Everything else goes by path.
6. **A large image is made smaller, not refused.** Above 2,000 pixels a side it is scaled to fit; above 3,932,160 bytes it is encoded again and, if need be, halved up to three times. Only what is still too heavy after that, or is over 32 MiB on disk, shows "Too large to send". These are Claude Code's own limits for an image. An animated GIF that is made smaller keeps its first frame only.
7. **An image within the limits is sent byte for byte.**
8. **Send waits for the chips.** While a chip is still being read or shows a problem, Enter and the send button do nothing; the spec says so for a problem. A message can be chips alone, with no text.
9. **A message's chips go wherever its text goes:** into the queue while a turn runs, back into the box with "Edit", to the copy with "Continue as a copy", back to the box on "Cancel".
10. **The same file is attached once.** A path already among the chips is not added again. A pasted image is always added.
11. **A sent message names its files.** Under the user's text the conversation shows the file names in one muted line, and the image count as today. A stored message's "Attached files:" block is shown the same way and no longer as text the user typed.
12. **A session whose first message has no text** is named after its first chip until a message with text names it.
13. **A tile with chips and no text is kept** when it is closed, as a tile with a draft is.
14. **A drop goes to the tile under the pointer**, which takes the focus. A paste goes to the box that has the keyboard, and with no box focused to the focused tile.
15. **A pasted image with text beside it is pasted as text.** Spreadsheets copy a cell as both; the text is what is meant there. An image alone, or with only formatted text beside it as a browser gives, becomes a chip. Claude Code itself has two keys and so never chooses: its image key takes the image first, the terminal's paste gives the text. Told this on 2026-11-16, Jerod chose: "Keep text first".
16. **The message field is built in the app** from the toolkit's own text field, with the kit's look copied. The kit's text area cannot hear a paste. The other choice is a change to the kit, which is not this project's code.
17. **The text grows as far as its tile allows:** at most ten lines as today, fewer in a short tile so that 48 points of conversation stay, never under one line.
18. **The program's input gets one writer thread.** A line is queued, not written, by whoever sends it. A write that fails is no longer reported for the message itself: it shows as the session's end, which is what a broken pipe means.
19. **No limit on how many chips.** Should Claude Code refuse a message, the turn fails with its words, as any failed turn.
20. **`Capabilities.images` is still not read.** Both agents take images.

## Global Constraints

- Rust toolchain is exactly `1.99.1`, edition `2024`. In an agent's shell cargo is not on `PATH`: start every command with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Rune is pinned to `rev = "2a44156b21d63b0342a366b26d9222a6edd4622c"`. Its source is at `~/.cargo/git/checkouts/rune-c9b805e9ed8a3730/2a44156`; read it instead of guessing an API. Where this plan and the source differ, the source wins: say so in your report.
- `mr-agent` and `mr-core` depend on neither `mr-claude` nor `rune`. No type from `mr-claude` appears in `mr-core` or in any view. Only `crates/app/src/main.rs`, `crates/app/tests/live.rs` and `crates/probe` may name `ClaudeAgent`.
- The UI thread never blocks. A file is read, and an image decoded, shrunk or encoded, on the library thread only: never in a view, never in `App`, never in a handler.
- An image's bytes are never copied where a shared pointer will do, and never printed: no `Debug`, log line, panic message or test failure shows them.
- A session's events are trusted only while they carry the number of that session's current open.
- Every session intent of `App` takes the `SessionKey` it acts on. No view under a tile reads "the focused session".
- Every `App` intent a view calls is followed by `glue::run`.
- A press on a chip's remove control or on the attach button never takes the keyboard out of the field the user types in.
- The software never writes inside `~/.claude`, never changes the user's Claude Code settings, and never changes or deletes a file the user attaches.
- `SAVED_VERSION` stays 1. This plan adds nothing to `Saved`.
- Never assign `App.sessions`, `App.groups` or `App.tiles` wholesale after `App::new`.
- Every key this app gives an element starts with `mr-`.
- No Anthropic logos, colours or fonts.
- User-visible wording is exact where this plan quotes it. Never use a real person's name or e-mail address as test data: `alex`, `/home/alex`. Never copy text out of a real stored session or out of `target/rec/` into a test or a fixture. Test images are made by the test (a few coloured pixels), never taken from disk.
- All window tests and all screenshots use `FakeAgent`, through the demo window. No agent starts the real `claude`, sets `MINDRUNE_LIVE` or runs a test with `--ignored`. Lines of the protocol that were never recorded are written by hand; the test's comment says "never seen live".
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` (no warnings) and `cargo test --workspace` pass before every commit. Say in the report which existing tests you changed and why.
- **Git.** Work on the branch `plan-6-attachments`, cut from `main`. Never push. Never commit `target/` or `.superpowers/`. Commit by explicit path, not `git add -A`. Another session of Jerod's may work in the same checkout: never touch, stash, reset or commit a file that is not yours. End every commit message with a blank line and `Co-Authored-By: Claude <noreply@anthropic.com>`, in exactly that form.
- Version of `claude` this plan was written against: `2.1.323`.

## Review Focus

The spec implies these conditions and no screen in it shows them. Each has a test in the task named.

1. **A message that is chips alone.** Every "is it empty" rule in the code looks at text only. Expected: it is sent, its tile is kept on closing, its session gets a name, and a message with neither text nor chips is still refused. Tests: Tasks 5, 6 and 8.
2. **A chip that is not ready when the user sends, or is removed while it is being read.** Expected: nothing is sent while one is being read; the answer for a removed chip is dropped; no chip of one session ever lands in another. Tests: Task 5.
3. **Files that are not what their name says, or cannot be had.** A text file named `.png`, a broken image, an empty file, a folder, a file that is gone, a file with no read permission, a name with a line break in it, an "image" over 32 MiB. Expected: an image, a path, or a problem on the chip; never a panic, never a blocked window. Tests: Task 4.
4. **A megabyte line while the program is talking.** The program writes more than a pipe's worth while a large message is being written to it, and asks something the reader must answer. Expected: neither side stalls. Tests: Task 2.
5. **The smallest tile with everything in the box.** A stacked tile about 172 points high with a queued message, chips and a long draft. Even one line of text does not fit there. Expected: the text and the send button stay inside the tile and can be used, what does not fit is cut off at the top of the box, and nothing is drawn over the tile below. Tests: Task 10.

## File Structure

```
crates/agent/src/command.rs        Task 1: image bytes behind a shared pointer
crates/agent/src/agent.rs          Task 1: the files of a stored user message
crates/claude/src/process.rs       Task 2: one writer thread
crates/claude/src/agent.rs         Task 2: nobody but the writer touches the input
crates/claude/tests/               Task 2: a stub that floods
crates/claude/src/encode.rs        Tasks 1, 3: the attached files block, written and read back
crates/claude/src/history.rs       Task 3: a stored message's files
crates/core/src/attach.rs          Task 4 (new): sources, chips, limits, preparing
crates/core/src/backend.rs         Task 5: preparing on the library thread
crates/core/src/app.rs             Tasks 5, 6: attach, detach, what may be sent
crates/core/src/session.rs         Tasks 5, 6: a session's chips; messages with chips
crates/core/src/conversation.rs    Task 6: a user message's files
crates/app/src/view/field.rs       Task 7 (new): the message field
crates/app/src/view/input.rs       Tasks 7, 8, 10: paste, chips, the attach button, the height
crates/app/src/view/chips.rs       Task 8 (new): the chips row
crates/app/src/view/items.rs       Task 8: the files line of a user message
crates/app/src/view/tiles.rs       Task 9: drops on a tile
crates/app/src/view/mod.rs         Task 9: a paste with no box focused
crates/app/src/glue.rs             Tasks 7, 8, 9: attaching from the window, thumbnails, the drop veil
crates/app/src/demo.rs             Task 11: scenes
crates/app/tests/                  Task 11: snapshots, live
docs/                              Task 11: notes
```

## Facts this plan builds on

From the two research reports. Check each against the source before relying on it.

- **The seam** has `SessionCommand::Send { text, attachments: Vec<Attachment> }` and `Attachment::{Image { media_type, data: Vec<u8> }, File { path }}`. `HistoryItem::User { text, image_count }`.
- **The adapter** writes a user message as one text block, when there is text, then one `image` block per image with a base64 source. Files are named in the text: a blank line, `Attached files:`, then one `- PATH` line each. The program does not echo a user message.
- **One place** makes a `Send`: `Session::hand_over`, with `attachments: vec![]`.
- **Text-only rules:** `input::submit`, the send button, `App::submit`, `Session::submit`, `Session::wake`, `glue::leaving`, and `empty` in `app.rs`. The queue is `Session.queued: Option<String>`.
- **The program's input** is written under one lock by the backend's open thread and by the adapter's reader thread (`Reader::send`, `Reader::refuse`).
- **Claude Code's own image limits** (its text, 2.1.323): 2,000 by 2,000 pixels, 5,242,880 bytes of base64, and a target of 3,932,160 raw bytes.
- **Pasting in Rune:** only an element made by `rune::text_input` can take `on_input_paste(|s, content: &ClipboardContent, tx| -> bool)`; `true` stops the text paste. `content.image()` is PNG bytes with a size, `content.files()` the paths of copied files, `content.text()` the text. The kit's `TextArea` forwards no such hook. With no field focused the root's `on_paste_content` hears a paste.
- **Drops in Rune:** `on_drop_files`, `accepts_files`, `on_drop_hover(bool)`; the innermost accepting element under the pointer gets the paths; a drop target takes no pointer events. No events at all on Wayland.
- **The file dialog:** `tx.open_file(FileDialogOptions, |s, Result<Option<PathBuf>, FileDialogError>, tx|)`, one path, answered on a later frame. `view/folder.rs` already shows a dialog's error.
- **Images in Rune:** `ImageCache::new(waker)`, `cache.rgba(width, height, bytes) -> Image`, drawn with `El::pixels_fit(image.pixels(), SurfaceFit::Fill)` and `.radius(..)`. View tests do not run `glue::setup`, so they have no cache unless they make one.
- **Tests can drive** a paste (`rt.paste_content(ClipboardContent::new("").with_image(ClipboardImage::from_png(bytes)?))`, `.with_files(..)`), a drop (`rt.drag_files`, `rt.drop_files`, `rt.drop_hovered`) and a dialog's answer (`rt.file_dialog()`, `rt.resolve_file_dialog(Some(path))`). `testkit::press_with` Ctrl+V does not paste.
- **The input box:** `LINES = (1, 10)`, a line is 17.5 points, the field's padding 8 on each side, `SEND_SIZE` 28, `BOX_PAD` 8, `AREA_PAD_BOTTOM` 16, a tile's header 36. `tiles::tile_height(ui, key)` gives a tile's height without a layout read. `input::model_room` shares the bottom row's width.
- **The theme's icons** have `PLUS`, `FILE_DOCUMENT`, `FOLDER`, `PHOTO`; none is a paperclip.

---

### Task 1: The seam carries image bytes safely and a stored message's files

**Files:**
- Modify: `crates/agent/src/command.rs`, `crates/agent/src/agent.rs`, `crates/agent/src/lib.rs`, `crates/agent/Cargo.toml`, and every user of the two changed types (`crates/claude/src/encode.rs`, `crates/claude/src/history.rs`, `crates/claude/src/history_tail.rs`, `crates/core/src/conversation.rs`, `crates/probe/src/repl.rs`, tests)

**Interfaces:**
- Produces:
  ```rust
  // agent/src/command.rs
  /// The bytes of an image, shared and never printed.
  #[derive(Clone, PartialEq, Eq)]
  pub struct ImageData(Arc<[u8]>);
  impl ImageData {
      pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self;
      pub fn as_bytes(&self) -> &[u8];
      pub fn len(&self) -> usize;
      pub fn is_empty(&self) -> bool;
  }
  pub enum Attachment {
      Image { media_type: String, data: ImageData },
      File { path: PathBuf },
  }
  // agent/src/agent.rs
  HistoryItem::User { text: String, image_count: usize, files: Vec<PathBuf> }
  ```

Rules:

- `Debug` of `ImageData` is `ImageData(N bytes)`. Serde writes it as one base64 string and reads it back.
- `files` is empty everywhere in this task; Task 3 fills it.
- Nothing else changes: the adapter encodes the same JSON as before.

- [ ] **Step 1: Write the failing tests** in `crates/agent/src/command.rs`:
  - `image_bytes_are_never_printed`: `format!("{:?}", Attachment::Image { .. })` of 1,000 bytes holds `1000 bytes` and is under 120 characters.
  - `image_bytes_are_written_as_base64`: bytes `[1, 2, 3]` serialise as `"AQID"` and read back equal.
  - `a_clone_shares_the_bytes`: `as_bytes().as_ptr()` is equal for a value and its clone.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement**, and follow the compiler through the workspace.
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(agent): image bytes are shared and never printed`.

### Task 2: One writer for the program's input

**Files:**
- Modify: `crates/claude/src/process.rs`, `crates/claude/src/agent.rs`, `crates/claude/tests/process.rs`
- Create: `crates/claude/tests/stub/flood.sh`

**Interfaces:**
- Consumes: `ClaudeProcess::spawn`, `write_line`, `close_input`, the shared `request` function, `Reader::send`, `Reader::refuse`.
- Produces: the same signatures. `write_line(&self, line: &str) -> Result<(), AgentError>` now queues.

Rules:

- `ClaudeProcess` owns a writer thread that holds the child's input and takes lines from a channel, in the order they were queued, writing and flushing each.
- `write_line` returns `AgentError::Closed` once the input was closed, the writer has stopped, or the child has exited; otherwise `Ok` as soon as the line is queued. It never blocks on the pipe.
- `close_input` is queued like a line: the lines before it are written, then the input is dropped. Dropping the process or killing the child ends the writer without waiting for a blocked write for more than the kill takes.
- When a write fails the writer stops and marks the input closed. Nothing is reported for the line; the child's exit is what the session hears.
- `request` still records the request's id before its line is queued.
- No other thread writes to the child's input, and the reader thread never waits on the writer.

- [ ] **Step 1: Write the failing tests** in `crates/claude/tests/process.rs`:
  - `a_large_line_and_the_reader_do_not_lock_each_other`: `flood.sh` waits 300 ms, prints a first line, then 300 lines of 1,024 characters, and only then reads its input and echoes each line's length. The wait lets the large write get under way first. The test's `on_line` answers the first line with `write_line("pong")`, as the reader does for a request it refuses, while a thread the test started has queued one line of 2,000,000 characters before (a thread of its own, so that the test can give up at its deadline on the code as it is). Within 10 seconds the stub has echoed `2000000` and `4`, in the order they were queued. The comment says which lock this used to hang on.
  - `lines_are_written_in_the_order_they_were_queued`: 200 short lines from two threads, each thread's own lines arrive in its order.
  - `closing_the_input_writes_what_was_queued_first`.
  - `a_line_after_the_child_has_gone_is_refused`.
- [ ] **Step 2: Run them.** The first hangs until its deadline on the code as it is; say so in the report with the output.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks; run the first test twenty times in a loop and say so.
- [ ] **Step 5: Commit** `fix(claude): one writer thread for the program's input`.

### Task 3: A stored message gives its files back

**Files:**
- Modify: `crates/claude/src/encode.rs`, `crates/claude/src/history.rs`, `crates/claude/src/history_tail.rs` (if it shares the reader)

**Interfaces:**
- Produces:
  ```rust
  // claude/src/encode.rs
  /// What `user_message` added to `text` for its files, taken off again: the text as typed and
  /// the paths. Text without such a block comes back whole.
  pub fn split_attached(text: &str) -> (&str, Vec<PathBuf>);
  ```

Rules:

- The block is recognised only at the end of the text: the line `Attached files:`, at the very start or after one blank line, followed by nothing but lines that start with `- `. Otherwise the text is left whole.
- `user_message` writes a path with `Path::to_str`; Task 4 keeps a path that has no such form, or holds a line break, from ever reaching it. A debug assertion says so.
- The history reader gives a user message its `files` and its text without the block. A message that had only files is kept, with empty text. The listing's fallback title is made from the text without the block.

- [ ] **Step 1: Write the failing tests.**
  In `encode.rs`: `the_attached_files_come_back_off_the_text` (text and two files; files alone; text alone; a block in the middle of the text is left; a line under the heading that does not start with `- ` leaves the text whole); `what_is_written_is_read_back` (round trip through `user_message`, read from its JSON's text block); `images_and_files_together_keep_their_order` (text block first, images after, in order).
  In `history.rs`, with hand-written records ("never seen live"): `a_stored_message_names_its_files`; `a_stored_message_of_files_alone_is_kept`; `the_listing_does_not_take_its_title_from_the_files_block`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(claude): a stored message gives its attached files back`.

### Task 4: Sources, chips and preparing an attachment

**Files:**
- Create: `crates/core/src/attach.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/Cargo.toml`, `Cargo.lock`

**Interfaces:**
- Produces:
  ```rust
  // core/src/attach.rs
  pub const MAX_SIDE: u32 = 2000;
  pub const MAX_BYTES: usize = 3_932_160;
  pub const READ_LIMIT: u64 = 32 * 1024 * 1024;
  pub const THUMB_SIDE: u32 = 96;

  /// Where an attachment comes from.
  #[derive(Clone, Debug, PartialEq)]
  pub enum Source { Pasted(ImageData), Path(PathBuf) }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Problem { TooLarge, Unreadable, Name }
  impl Problem { pub fn words(self) -> &'static str; }
  /// A small picture of an image, straight-alpha RGBA. Never printed.
  #[derive(Clone, PartialEq)]
  pub struct Thumb { pub width: u32, pub height: u32, pub rgba: Arc<[u8]> }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Kind { Image, File, Folder }
  #[derive(Clone, Debug, PartialEq)]
  pub struct Prepared { pub kind: Kind, pub attachment: Attachment, pub thumb: Option<Thumb> }
  #[derive(Clone, Debug, PartialEq)]
  pub enum ChipState { Reading, Ready(Prepared), Problem(Problem) }
  #[derive(Clone, Debug, PartialEq)]
  pub struct Chip { pub id: u64, pub name: String, pub source: Source, pub state: ChipState }

  /// The name a chip shows: "Pasted image", or the path's last part.
  pub fn name_of(source: &Source) -> String;
  /// Reads and checks `source`. Slow: the library thread only.
  pub fn prepare(source: &Source) -> Result<Prepared, Problem>;
  ```

Rules:

- **Words:** `TooLarge` "Too large to send", `Unreadable` "Cannot be read", `Name` "Its name cannot be sent".
- **A path** that is not UTF-8 or holds a line break is `Name`. One whose metadata cannot be read is `Unreadable`. A folder is `Attachment::File` of kind `Folder`. A file whose first bytes cannot be read is `Unreadable`.
- **Image or not** is decided by `image::guess_format` on the file's first bytes: PNG, JPEG, GIF and WebP are images, with the media types `image/png`, `image/jpeg`, `image/gif`, `image/webp`. Anything else, an empty file too, is `Attachment::File` of kind `File`, and is not read further.
- **An image file** over `READ_LIMIT` is `TooLarge`, judged by its size without reading it. Otherwise it is read whole and decoded; one that does not decode is `Unreadable`.
- **Within the limits** (both sides at most `MAX_SIDE`, at most `MAX_BYTES`) the bytes go out as they are, with the guessed media type.
- **Over them:** scale to fit `MAX_SIDE` on both sides, keeping the shape, never enlarging; encode as PNG when the image has an alpha channel, else as JPEG at quality 85. While the result is over `MAX_BYTES`, halve both sides and encode again, at most three times. Still over: `TooLarge`.
- **A pasted image** is PNG bytes and goes the same way as an image file's bytes.
- **The thumb** of every image fits `THUMB_SIDE` on both sides, keeping the shape.
- `Debug` of `Thumb` is `Thumb(WxH)`.

- [ ] **Step 1: Write the failing tests** in `attach.rs`, with images made in the test and files in a temporary folder:
  - `a_small_image_goes_out_byte_for_byte`, for a PNG and a JPEG, with the media type of each and a thumb no larger than 96.
  - `an_image_is_known_by_its_bytes_not_its_name`: a PNG named `notes.txt` is an image; a text file named `shot.png` goes by path.
  - `a_wide_image_is_scaled_to_fit`: 3,000 by 1,000 comes out 2,000 wide and 666 or 667 high, and decodes.
  - `a_heavy_image_is_halved_until_it_fits`: 2,000 by 2,000 of noise with an alpha channel comes out under `MAX_BYTES` and smaller.
  - `an_image_without_alpha_is_encoded_as_jpeg`, `one_with_alpha_stays_png`, both for images over the limits.
  - `an_image_over_the_read_limit_is_too_large`: a file with a PNG's first bytes, made 33 MiB long with `set_len`, without its bytes being read (the test finishes in well under a second).
  - `a_broken_image_cannot_be_read`: a PNG cut in half.
  - `a_folder_an_empty_file_and_a_pdf_go_by_path`, each with its kind.
  - `a_file_that_is_gone_cannot_be_read`; on Unix, `a_file_without_permission_cannot_be_read` for an image (skipped when the test runs as root).
  - `a_name_with_a_line_break_cannot_be_sent`.
  - `a_pasted_image_is_prepared_as_a_png`, and `the_name_of_a_source`.
  - `a_thumb_is_never_printed`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): attachments are read, checked and made to fit`.

### Task 5: A session has chips

**Files:**
- Modify: `crates/core/src/session.rs`, `crates/core/src/app.rs`, `crates/core/src/backend.rs`, `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: Task 4; the library thread (`LibraryJob`, `ToBackend`, `FromBackend`), `App::receive`, as `ReadBranch` uses them.
- Produces:
  ```rust
  // core/src/session.rs
  pub chips: Vec<Chip>,                     // on Session, empty at first
  // core/src/app.rs
  pub enum Effect { /* as it is */ Prepare { chip: u64, source: Source } }
  /// Adds a chip for each source to `key`'s session and asks for each to be read.
  pub fn attach(&mut self, key: SessionKey, sources: Vec<Source>) -> Vec<Effect>;
  /// Takes chip `chip` off `key`'s session.
  pub fn detach(&mut self, key: SessionKey, chip: u64);
  /// Whether Enter would send `text` with `key`'s chips now. The rule itself is
  /// `Session::can_send(&self, text: &str) -> bool`, which this asks.
  pub fn can_send(&self, key: SessionKey, text: &str) -> bool;
  // core/src/backend.rs
  pub enum ToBackend { /* as it is */ Prepare { chip: u64, source: Source } }
  pub enum FromBackend { /* as it is */ Prepared { chip: u64, result: Result<Prepared, Problem> } }
  ```

Rules:

- Chip ids are given by `App` and never used twice in a run.
- `attach` does nothing for a session that takes no message (`takes_message` is false). A `Source::Path` equal to the path of a chip the session already has is left out.
- A new chip is `Reading` with `name_of` its source. `Prepared` sets the chip with that id, in whatever session it is, to `Ready` or `Problem`. An answer for a chip that is gone is dropped.
- `can_send`: the session takes a message, no chip is `Reading` or `Problem`, and the text has a character that is not white space or there is at least one chip.
- `empty` in `app.rs` is false for a session with chips, so `forget_if_empty` keeps it.
- Preparing runs on the library thread, one job per chip.

- [ ] **Step 1: Write the failing tests.**
  In `app.rs`: `attaching_adds_reading_chips_and_asks_for_each`; `the_same_path_is_attached_once` (a pasted image twice gives two); `an_answer_fills_its_chip` (ready, and a problem); `an_answer_for_a_removed_chip_is_dropped`; `a_chip_belongs_to_its_session` (two sessions, answers in the other order); `an_ended_session_takes_no_chips`; `nothing_can_be_sent_while_a_chip_is_read_or_has_a_problem`; `chips_alone_can_be_sent`; `a_session_with_chips_is_not_empty`.
  In `backend.rs`: `the_library_prepares_an_attachment` (a small PNG in a temporary folder comes back `Ready` with a thumb).
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): a session has chips, read on the library thread`.

### Task 6: A message goes out with its chips

**Files:**
- Modify: `crates/core/src/session.rs`, `crates/core/src/app.rs`, `crates/core/src/sessions.rs` (if the prompt needs it), `crates/core/src/conversation.rs`

**Interfaces:**
- Consumes: Tasks 1 and 5.
- Produces:
  ```rust
  // core/src/session.rs
  /// A message waiting to be handed over: its text and its chips, all ready.
  #[derive(Clone, Debug, PartialEq)]
  pub struct Outgoing { pub text: String, pub chips: Vec<Chip> }
  pub queued: Option<Outgoing>,             // was Option<String>
  // core/src/conversation.rs
  ItemBody::User { text: String, image_count: usize, files: Vec<PathBuf> }
  pub fn push_user(&mut self, text: &str, image_count: usize, files: Vec<PathBuf>);
  ```
  `App::submit(key, text)`, `Session::submit(text)`, `Session::wake(text)`, `Session::unwake() -> Option<String>` and `Session::take_queued() -> Option<String>` keep their signatures.

Rules:

- **Submit and wake** take the session's chips with the text when `Session::can_send` holds, and do nothing otherwise. The session has no chips afterwards.
- **The queue** joins a second message to the first: the texts with a blank line between them when both have text, the chips one after the other.
- **Handing over** sends `SessionCommand::Send` with the text and each chip's attachment in the chips' order, and pushes the user's message with its image count and its files' paths.
- **Coming back.** `take_queued` and `unwake` give the text back as today and put the message's chips back on the session, before any chips it has by then. "Cancel" on the "open elsewhere" prompt therefore brings the chips back with the text. "Continue as a copy" gives the copy the text and the chips, and the original keeps neither.
- **The name.** With text, as today. A first message without text names the session after its first chip, and asks for no title; the next message with text is treated as the first was.
- **A stored message** shows its `files` through `push_stored`.

- [ ] **Step 1: Write the failing tests.**
  In `session.rs`: `a_message_goes_out_with_its_chips_in_order` (text, an image, a file: one `Send`, attachments in order, no chips left, the conversation's item has `image_count` 1 and the file); `chips_alone_go_out`; `a_message_with_an_unready_chip_is_not_sent_and_keeps_everything`; `a_queued_message_keeps_its_chips_and_a_second_joins_it`; `editing_the_queue_brings_the_chips_back_in_front`; `a_woken_session_sends_its_chips_and_unwaking_brings_them_back`; `a_first_message_of_chips_is_named_after_the_first_chip_and_asks_for_no_title`, then a message with text asks for one.
  In `app.rs`: `cancel_on_open_elsewhere_brings_the_chips_back`; `continue_as_a_copy_takes_the_chips_along` (the copy sends them, the original has none).
  In `conversation.rs`: `a_stored_message_shows_its_files`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): a message goes out with its chips`.

### Task 7: The message field hears a paste

**Files:**
- Create: `crates/app/src/view/field.rs`
- Modify: `crates/app/src/view/input.rs`, `crates/app/src/view/mod.rs`, `crates/app/src/glue.rs`, `crates/app/src/testkit.rs`

**Interfaces:**
- Consumes: Task 5 (`App::attach`), `rune::text_input`, `rune::auto_grow`, `El::on_input_paste`.
- Produces:
  ```rust
  // app/src/glue.rs
  /// Attaches `sources` to `key`'s session and runs what that asks for.
  pub fn attach(ui: &mut Ui, key: SessionKey, sources: Vec<Source>);
  // app/src/view/input.rs
  /// What a paste into `key`'s box does with `content`. True when the app took it.
  pub fn pasted(ui: &mut Ui, key: SessionKey, content: &ClipboardContent) -> bool;
  // app/src/testkit.rs
  pub fn paste_image(rt: &mut Runtime<Ui>, png: Vec<u8>);
  pub fn paste_files(rt: &mut Runtime<Ui>, paths: &[PathBuf]);
  pub fn tiny_png() -> Vec<u8>;             // a few pixels, made in code
  ```
  `field.rs` builds the element keyed `mr-input` that `input::text_area` used to get from the kit's `TextArea`.

Rules:

- **The field** is `rune::text_input` in `rune::auto_grow`, dressed as the kit's text area is (read `rune-kit/src/text_input.rs`, `TextArea::build`): the same key, label "Message", placeholders, padding, radius, colours, focus ring, line height, growth from one line to ten, Enter to send, Shift+Enter for a line, Escape to stop, the disabled look of an ended session. Every existing test of the input box passes unchanged.
- **A paste** (`pasted`), in this order: files in the content become `Source::Path` chips and the paste is taken; else, with text that is not empty, it is not taken and the text is pasted as before; else an image becomes a `Source::Pasted` chip and is taken; else it is not taken. A session that takes no message takes nothing.
- A taken paste leaves the draft's text and the caret as they were, and the keyboard in the box.

- [ ] **Step 1: Write the failing tests** in `input.rs`:
  - `the_field_is_where_the_kits_text_area_was`: the rectangles of `mr-input` and `mr-input-box` with one line and with three lines of draft are what they were before this task (write the numbers down from the code as it is, before changing it).
  - `a_pasted_image_becomes_a_chip_and_no_text`: one chip named "Pasted image", the draft unchanged, the box still focused.
  - `pasted_files_become_chips_and_no_text`.
  - `text_is_pasted_as_text_also_beside_an_image`.
  - `an_ended_session_takes_no_paste`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks. Compare the `working` scene's screenshot in `target/snap/final` before and after, in both themes, and say what you see.
- [ ] **Step 5: Commit** `feat(app): the message field hears a paste`.

### Task 8: The chips, the attach button, and a sent message's files

**Files:**
- Create: `crates/app/src/view/chips.rs`
- Modify: `crates/app/src/view/input.rs`, `crates/app/src/view/items.rs`, `crates/app/src/glue.rs`, `crates/app/src/view/mod.rs`

**Interfaces:**
- Consumes: Tasks 5 to 7; `rune::ImageCache`, `tx.open_file`.
- Produces: keys `mr-chips`, `mr-chip-{id}`, `mr-chip-thumb-{id}`, `mr-chip-name-{id}`, `mr-chip-problem-{id}`, `mr-chip-remove-{id}`, `mr-attach`, `mr-user-files-{id}`; `Ui.images: Option<ImageCache>` (set in `glue::setup`), `Ui.thumbs: HashMap<u64, rune::Image>`; `testkit::attach_file(rt, path)` (presses `mr-attach` and answers the dialog).

Rules:

- **The row** `mr-chips` sits in the box between the queued strip and the text: from the top, the queued strip, the chips, the text, the bottom row. It is there only while the session has chips. It is 32 points high, scrolls sideways, never wraps, with 6 points between chips.
- **A chip** is 32 points high with a 1-point border and the box's small radius: a 24 by 24 picture, the name cut in the middle at 160 points, the remove control.
  - The picture, by the chip's `Kind`: an image's thumb, filling the square with 4-point corners, or `PHOTO` while there is none; `FOLDER` for a folder; `FILE_DOCUMENT` for a file, and for any chip not ready yet whose source is a path.
  - While reading: the name in the muted colour and the tooltip "Reading…".
  - With a problem: the border and the problem's words in the danger colour, after the name, as `mr-chip-problem-{id}`; the tooltip is the path and the words.
  - Ready: the tooltip is the file's whole path, or "Pasted image".
  - The remove control is an icon button with the label "Remove NAME". It calls `App::detach`, and the keyboard stays in the box.
- **Thumbs.** A view cannot make a handle, so `glue::poll` makes it: when a chip's answer arrives with a thumb and a cache is present, `cache.rgba(..)` gives the handle kept in `Ui.thumbs` under the chip's id. Where the views are tidied, a handle is dropped once its id is on no session, neither among its chips nor in its queued message. Without a handle an image chip shows `PHOTO`.
- **Sending.** The send button is enabled when `App::can_send` says so; Enter sends under the same rule and otherwise does nothing. After a send the draft is cleared as today.
- **The attach button** `mr-attach` is the first control of the bottom row: the `PLUS` icon in a button as large as the send button, label and tooltip "Attach a file", disabled in a session that takes no message. `model_room` gives the model picker that much less. A press opens the file dialog with the title "Attach a file" in the session's folder; a path attaches it; a cancel does nothing; an error is shown as the folder dialog's error is.
- **Leaving.** `glue::leaving` keeps a session with chips as it keeps one with a draft.
- **The queued strip** shows the queued text's first line as today, and the first chip's name when the queued message has no text.
- **A sent message** shows under its text, after the images line, one muted line `mr-user-files-{id}`: the files' names (the last part of each path) with ", " between them. Its tooltip is the whole paths, one a line.

- [ ] **Step 1: Write the failing tests.**
  In `chips.rs`: `a_chip_shows_its_name_and_can_be_removed` (the keyboard stays in the box); `a_reading_chip_says_so`; `a_problem_shows_its_words` ("Too large to send"); `an_image_chip_shows_its_thumb` (with a cache made in the test and `Image::wait`); `a_long_name_is_cut_and_the_tooltip_has_the_path`; `many_chips_scroll_and_the_box_is_no_higher` (the box's height with 1 chip and with 12 is the same).
  In `input.rs`: `chips_alone_can_be_sent_with_enter_and_with_the_button`; `nothing_is_sent_while_a_chip_has_a_problem` (and it is sent once the chip is removed); `the_attach_button_adds_a_file`; `a_cancelled_dialog_adds_nothing`; `the_attach_button_is_off_in_an_ended_session`; `a_tile_with_chips_and_no_text_is_kept_when_closed`; `a_queued_message_of_chips_alone_shows_its_first_chips_name`; the two `in_the_smallest_window_*` tests still pass with the button in the row.
  In `items.rs`: `a_sent_message_names_its_files` (two files, names only, the paths in the tooltip); `a_message_of_an_image_alone_shows_its_count`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): chips in the input box and an attach button`.

### Task 9: Files dropped on a tile, and a paste with no box focused

**Files:**
- Modify: `crates/app/src/view/tiles.rs`, `crates/app/src/view/mod.rs`, `crates/app/src/glue.rs`

**Interfaces:**
- Consumes: Task 7 (`glue::attach`, `input::pasted`); `El::on_drop_files`, `accepts_files`, `on_drop_hover`, `on_paste_content`.
- Produces: `Ui.file_drop: Option<SessionKey>`; keys `mr-drop-files` and `mr-drop-files-text`.

Rules:

- Each tile takes dropped files when its session takes a message. A tile of an ended session takes none, and the drag passes it by.
- While files are held over a tile, that tile shows a veil over its whole pane under the header: the accent colour at 12 percent, the words "Drop to attach" in its middle, inert, keyed `mr-drop-files`. The tile's outline does not change. The veil goes when the files leave, are dropped, or the tile goes.
- A drop attaches every path to that tile's session, gives the tile the focus and the keyboard to its box.
- A drop anywhere else in the window does nothing.
- An in-app drag of a row or a header is untouched: neither shows the veil, and a file drag shows none of the in-app drag's marks.
- With no field focused, a paste of an image or of files goes to the focused tile's session by `input::pasted`. Text pasted with no field focused does nothing, as today.

- [ ] **Step 1: Write the failing tests** in `tiles.rs`:
  - `files_held_over_a_tile_show_the_veil_and_leaving_takes_it_away` (`rt.drag_files`, `rt.drag_files_leave`).
  - `dropped_files_become_chips_of_the_tile_under_the_pointer`: two tiles, the drop on the one that is not focused; it has the chips and the focus, the other has none.
  - `an_ended_sessions_tile_takes_no_files`.
  - `a_dragged_row_shows_no_veil`.
  - `a_paste_with_no_box_focused_goes_to_the_focused_tile`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): files dropped on a tile become chips`.

### Task 10: The box stays inside a short tile

**Files:**
- Modify: `crates/app/src/view/input.rs`, `crates/app/src/view/field.rs`, `crates/app/src/view/tiles.rs` (to hand the height down)

**Interfaces:**
- Consumes: `tiles::tile_height`, Task 7's field.
- Produces:
  ```rust
  // app/src/view/input.rs
  /// The least height of conversation a tile keeps over its input box.
  pub const CONVERSATION_MIN: f32 = 48.0;
  /// How many lines the text may grow to in a tile `tile_height` high, when the box holds
  /// `extra` points besides the text and the bottom row. Ten when the height is not known.
  pub fn lines_for(tile_height: Option<f32>, extra: f32) -> usize;
  ```

Rules:

- `lines_for` is `((tile_height - 36 - CONVERSATION_MIN - 84 - extra) / 17.5)` rounded down, at least 1 and at most 10. 36 is the tile's header; 84 is the box around one line of text and its bottom row (16 under the box, 8 and 8 of the box's padding, 8 and 8 of the field's, 8 between the text and the row, 28 for the row). Use the code's constants, not these numbers.
- `extra` is what is in the box besides: the chips row and the queued strip with the gap each brings, and the "folder is gone" line.
- The field grows to that many lines and scrolls inside beyond, the caret kept in view.
- Where even one line does not fit (the lowest stacked tile with a queued strip and chips), the box keeps its place at the bottom of its tile: the text and the bottom row stay inside the tile, what does not fit is cut off at the top of the box, and nothing of the tile is drawn outside it.
- Nothing is read from the layout to do this.

- [ ] **Step 1: Write the failing tests** in `input.rs`:
  - `the_lines_a_tile_allows`: no height 10; 172 gives 1; 280 gives 6; 280 with 40 extra gives 4; 600 gives 10; a height under the header gives 1.
  - `a_long_draft_stays_inside_a_short_tile`: two stacked tiles in a window 560 by 400, a draft of thirty lines in the upper one: the whole of `mr-input-box` is inside its tile's rectangle, under the header, and the text shows one line.
  - `the_lowest_tile_with_everything_keeps_its_text_and_its_button`: the same with a queued message and three chips besides: `mr-input` and `mr-send` are inside the upper tile's rectangle, `mr-send` can be pressed and sends, and nothing of the upper tile is drawn inside the lower tile's rectangle.
  - `a_tall_tile_still_grows_to_ten_lines`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `fix(app): the input box stays inside a short tile`.

### Task 11: Scenes, the real path, notes, and Jerod's look

**Files:**
- Modify: `crates/app/src/demo.rs`, `crates/app/tests/snapshots.rs`, `crates/app/tests/live.rs`, `crates/probe/src/repl.rs` (only if Task 1 left it text-only), `docs/protocol-notes.md`, `docs/notes-for-later-plans.md`

Rules:

- **Scenes**, in both lists, dark and light: `chips` (a focused tile with a draft of two lines and four chips: a ready image with a thumb, a file, a folder, an image being read) and `chips-problem` (one chip "Too large to send", the send button off; beside it a tile whose last sent message shows "1 image" and two file names). Thumbs in a scene are made in code. The demo reads nothing from the real disk.
- **The live test** (never run by an agent) sends one more message in its own session: the text "What colour is the image, and what is the first word of the file?", a 16 by 16 red PNG made in the test, and a small text file it writes into its own temporary folder. It prints, and never fails on:
  - `live: image and file sent`
  - `live: the answer names the colour: yes|no` and `live: the answer names the file's word: yes|no`
  - after reading that session's stored history back: `live: the stored message shows N image(s) and N file(s), text kept whole: yes|no`
  - `live: a 3 MB line went through: yes|no`, for a second image of noise just under the limit
- **`docs/protocol-notes.md`:** a section "Attachments, as sent" with the shapes written by hand and which of them the live test settles.
- **`docs/notes-for-later-plans.md`:** a first section "Jerod's look at plan 6": the two commands, "Before looking" (this plan's "What this window will not do yet"), what to try in the window, every decision of this plan with what was built, the rulings made while it ran, and what is left.
- **What to try in the window:**
  1. Copy an image (a screenshot, an image in a browser) and paste it into a tile's box: a chip with a thumbnail.
  2. Copy two files in the file manager and paste them: two chips.
  3. The plus button attaches a file; a second press, a second file.
  4. Send text with an image and a file: Claude answers about both, and the message names the files.
  5. Attach a very large photo: it is sent, made smaller. Attach something that cannot be read: the chip says so and nothing is sent until it is removed.
  6. Remove a chip; queue a message with chips while a turn runs, then "Edit" it back.
  7. In a short stacked tile, type many lines: the box stays inside the tile.
  8. Drag a file from the file manager onto a tile: on Wayland nothing happens (known). Does that matter enough to do something about it?

- [ ] **Step 1:** The scenes and their snapshots; look at each PNG in both themes and say what you see.
- [ ] **Step 2:** The live test's lines (compile only: `cargo test -p mindrune --test live --no-run`).
- [ ] **Step 3:** The notes.
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): scenes with chips`, `test(live): a message with an image and a file`, `docs: plan 6, in the notes`.
