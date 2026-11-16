# Notes for later plans

Carried over from plan 1 (foundations and the Claude adapter), plan 2 (one session in a window), plan 3 (many sessions), plan 4 (tiles), the stacked tiles plan (two tiles in a column) and plan 5 (the effort level, the right panel and Remote Control). Everything here was found while building or reviewing those plans and deliberately left for a later plan. Read it with `docs/protocol-notes.md` before writing the next plan.

Plan 5's section comes first, then the stacked tiles plan's, then plan 4's, then plan 3's. "What plan 5 must know", among plan 4's sections, also says what the stacked tiles plan changed. From "Jerod's first look" on, the file is what plan 2 left, kept as it was; where plan 3 settled something there, an added sentence in italics says so.

## Jerod's look at plan 5

Plan 5 (`docs/superpowers/plans/2026-11-15-panel-remote-effort.md`) adds the effort level beside the model, a panel on the right that describes the focused session, and a Remote Control switch with a mark in the tile's header. It is built on the branch `plan-5-panel-remote-effort`, cut from `main` at `0dc6d25`, the commit that records the merge of stacked tiles. The branch holds Tasks 1 to 12 in seventeen commits, from `cbda79e` to `ae29e10`, three of them fixes after a review (`7821362`, `b07dbfe`, `4938524`), these notes (`48acd5d`), and the fix wave after the whole-branch review (`0623aec`, `311a540`, `4041c30`, `20571e6`, `4dff2c9`, and the commit that brought these notes up to date), and a last commit by the controller with the rulings of the run (`docs/superpowers/plans/2026-11-15-plan-5-rulings.md`) and what the re-review of the fix wave left. One commit on it is not this plan's: `8b4c632`, the MindRune icon under `assets/`, put there by another session of Jerod's that works in the same checkout. Task 13 was not built (see "Left by plan 5"). Each task's review passed. The whole-branch review found nothing critical and asked for fixes, which one wave made (see "The whole-branch review of plan 5", at the end of this section). On 2026-11-16 Jerod had it merged into `main` (a fast-forward to `4afab10`, with the context breakdown they asked for during their look); the branch is gone and nothing is pushed. After that wave `cargo test --workspace` gives 1040 passed, 0 failed, 2 ignored (the live tests, not run).

Unlike stacked tiles, this look needs the live test, and it comes first. Much of what the effort picker and the switch rest on was read in the program's text and never seen working; the live test's lines say whether it holds. Jerod runs, in their own terminal, on `main`:

```bash
MINDRUNE_LIVE=1 cargo test -p mindrune --test live -- --ignored --nocapture
```

```bash
cargo run --release -p mindrune
```

The window starts as before: with no folder it shows again the tiles it had when it last closed. The panel is hidden on the first start. Once shown or hidden, it comes back that way, and as wide as it was left.

`cargo test --workspace` writes the screenshots of the four new scenes (`effort`, `panel`, `panel-remote` and `panel-refused`, dark and light) to `target/snap/final`. Those made while building, with a few more (the input box at 560 by 400 among them), are in `target/snap/p5/`; the three panel scenes as the fix wave left them are there as `fix-panel-*.png`. Neither is in git.

### The live test

It needs Claude Code installed and signed in, as before, and starts the real `claude` four times, as plan 4's does. It turns Remote Control on in its own two sessions for a moment (decision 16), so two test sessions show at claude.ai/code for a few seconds each. Nothing else about what stays behind changed: see "Jerod's look at plan 4".

What it now prints, besides the lines of plans 3 and 4, on stderr, one per line. None of them fails the test, and every wait has a deadline of 30 seconds and says what it waited for. N is in milliseconds. The address of a Remote Control session is never printed.

In the first session, after the histories line, the effort level:

- `live: effort at start: <level>`, from the first level the program gave. `none` means the model takes no level; `not said within 30 s` means no level came.
- `live: effort changed to <level> in N ms`, after choosing another of the model's levels, then the same line for going back. In a normal run: `effort at start: X`, `effort changed to Y in N ms`, `effort changed to X in N ms`.
- `live: effort did not change: <reason>` in place of a change. The reasons: `Claude Code goes on with <level>` (the level read back is not the one asked for), `no answer in 30 s`, `the model lists no other level`, `the app sent nothing for <level>`, for going back `<level> is still in use` or `there is no level to go back to`, or the program's own error text, when it refuses the request or the reading after it fails.
- `live: effort: the app knows no levels for this model`, in place of both changes, when the model list the app holds gives the model in use no level at all. `the model lists no other level` is then kept for a model with a single level.

Then context and cost, as the first turn left them:

- `live: context <N>% and cost known: yes|no`, or `live: context unknown and cost known: yes|no`.

Then Remote Control in the first session, after its first turn (the lead `remote control`), and in the second session once it is ready and before its first message (the lead `remote control before a first turn`). Each place prints one line for turning it on and, but after `the app sent nothing to turn it on`, one for turning it off:

- `live: <lead>: on in N ms`.
- `live: <lead>: refused (<why>)`, where `<why>` is `Login`, `Policy`, `Elsewhere` or `Other`. A refusal comes twice, as a state and as a reply, and the two give the kind by different fields: after the first the test listens 2 seconds more and prints every kind it heard, in order, as in `refused (Other, then Login)`. The program's own reason is not printed.
- `live: <lead>: no answer in 30 s`, when nothing came; `no answer in 30 s (last: <state>)` when something came but neither on nor a refusal, for example `Starting`.
- `live: <lead>: the app sent nothing to turn it on`.
- Then `live: <lead>: off`, or `turning it off was refused`, `turning it off had no answer in 30 s`, or `off, as it already was` (after a refusal nothing needs turning off). A program that has not answered being turned on is asked to turn it off all the same, by the app itself since the fix wave.

The time spent on Remote Control in the second session is taken out of plan 4's "the session to the side replied in X ms".

In the second run, after plan 4's `live: resumed with <model> in <mode>`:

- `live: resumed with effort <level or none>, the program says <level or none>`: the level the app passed with `--effort` when it resumed the session, read from its stored history, and the first level the resumed program said; `the program says nothing within 30 s` when none came.

What each line settles:

- `effort at start` with a level: the program answers `get_settings` with `applied.effort`, and the adapter reads it. `none` for a model that lists levels, or `not said within 30 s`, means the read does not work as the program's text says.
- `effort changed to` twice: `apply_flag_settings` with `effortLevel` changes the running session's level. `did not change: Claude Code goes on with` means it did not. If either of these two lines shows that the program does not do what its text says, the effort picker cannot be trusted, and Tasks 2, 4 and 5 are reopened before the plan is merged. So does a `did not change` line that gives the program's own error text: the request or the reading is refused.
- `effort: the app knows no levels for this model`: the `initialize` reply gives the model no `supportedEffortLevels`, so the picker is not drawn for it. Not a failure of the program, but nothing about the effort was tried.
- `context <N>%` and `cost known: yes`: a share of the context window and the cost arrive with the end of a turn, counted the new way.
- `remote control: on`: the new states and the switch's path work after a turn. `refused (Login)` most likely means the sign-in is not a full claude.ai one; it is not a failure of the test.
- `remote control before a first turn: on`: Remote Control can be turned on before a session's first turn. If it is refused or has no answer there and the first session's is on, the fix is one condition in `Session::apply`: a new session that wants it asks when its first turn has ended, not at `Started`.
- `remote control: off`: turning it off answers with a bare success, as seen on 2026-11-10.
- `refused (A, then B)` with two kinds: the state and the reply disagree, and which arrives first decides the words the app shows (Minor 6 of the review, below). One kind: they agree.
- `resumed with effort X, the program says X`: `--effort` is taken on resume. `the program says Y` with another level means the flag is ignored on resume (or the program's cap lowered it); `says nothing` means the reading after start did not come. A flag the program rejected would end it at start, and the resume would fail the test loudly.

Four things stay unseen after the live test: a change of effort sent while a turn runs (the test changes it between turns), that a message is made at the level read back (the test reads the level, it sends no message at the changed level), the states `reconnecting` and `policy_disabled` (they need a dropped connection or a policy), and anything typed on another device (the check below).

What fails it is what failed plan 4's; this plan adds no way to fail.

### Before looking

What this window will not do yet, so it is judged for what it is:

- A prompt typed on a phone or at claude.ai/code runs, and its reply streams into the tile, but the prompt itself is not shown above the reply. That is Task 13, which waits for a recording (see "Left by plan 5").
- The Remote Control switch does not start a session's program. On a session that is resting it is remembered, the panel says "Turns on when the session starts.", and it is turned on with the next message.
- Remote Control is not turned on again by itself after the app is closed and opened, but for new sessions when "On for new sessions" is set.
- Closing the window still ends every session's program, so Remote Control ends with it. Running in the background is plan 7.
- The list of changed files is what the session's own edits say, not what git says, and it knows only the part of a long session that is loaded. A notebook edit is not counted.
- The branch is read when the focus moves, when the app starts and when a turn ends in the focused session. A branch switched in a terminal shows at the next of these.
- An effort level chosen on a session that rests, and never sent before the app closes, is not kept: the next start shows the level the session's history gives.
- The panel cannot show what a file's changes are. It lists the files and their counts only.
- With "On for new sessions" set, a new session that is left empty still shows at claude.ai/code for as long as its program runs.
- The demo window (`cargo run -p mindrune -- --demo`, its scenes and the screenshots) uses a fake agent that answers neither the Remote Control switch nor an effort choice by itself: a press there shows "Connecting…", or a choice changes nothing, until a scene says what the program said. Its folders are made up, so it reads no branch from the disk; the `panel` scene gives "main" itself.

### What to try in the window

1. The effort level shows beside the model in every tile, and a change holds for the next message. Seen: looks good (Jerod, 2026-11-16).
2. An old session opens with the effort it last used. Seen: looks good (Jerod, 2026-11-16).
3. The button at the top right and Ctrl+Alt+B show the panel; it comes back as it was after a restart. Seen: looks good (Jerod, 2026-11-16).
4. The panel follows the focused tile: folder, branch, model, effort, mode. Seen: looks good (Jerod, 2026-11-16).
5. Context and cost appear after a turn; the files a session changed are listed with their counts. Seen: works (Jerod, 2026-11-16); a breakdown of the context is asked for, see below.
6. The Remote Control switch turns on, the tile says "Remote", and "Copy link" gives a working address. Seen: looks good (Jerod, 2026-11-16).
7. "On for new sessions" makes the next new session start with it. Seen: looks good (Jerod, 2026-11-16).
8. On the ultrawide: is the panel worth its width, and should it start open? Seen: looks good (Jerod, 2026-11-16).

### What Jerod reported on 2026-11-16

Jerod ran the window on the branch. They first asked when the context figure shows up, and were told: after a session's first turn ends. Their answer: "Ok that part works. I do want the breakdown by messages, system tools etc. Everything else looks good." So the eight things above are recorded as seen, on that one sentence. It says nothing of what the live test printed, of the check with a second device, or of the "Things to look at" below; those stay open until Jerod says so. The eighth thing asked two questions (is the panel worth its width, should it start open); no change was asked for, so it stays hidden at first.

The one thing asked for is new: under "Context" the panel is to show what the context is made of (system prompt, system tools, messages and so on), as Claude Code's own `/context` does. That is built after plan 5 as a piece of its own; see "The context breakdown" in `docs/protocol-notes.md` once it is there.

That breakdown is now built on the branch, after `91555e4`. Under the bar of "Context" the panel lists each part the program counts, in its order, with its tokens and its share of the window (`12.0k (6%)`, `717 (<1%)`); the autocompact buffer and the free space are in the muted colour. The app asks for it after a session starts and after every turn, with the request `get_context_usage` seen in V9; parts with no tokens are not listed, and the tool descriptions the program keeps out of the window (its "deferred" rows) are left out, as its own sums leave them out. What to look at: the rows themselves, against `/context` in a terminal for the same session; and a stored session woken again by a message, which now shows a share over the bar from the moment its program has started, while its first turn here still runs, taken from the breakdown, where it said "not known yet" until that turn ended (if the program answers at start: the live test's line `live: context breakdown at start` says whether it does). A resting session starts no program, so it still says "not known yet". After a turn the share over the bar is still the turn's own figure, which can differ a little from the sum of the used rows.

Shown the breakdown's demo screenshot and asked whether to merge, Jerod said: "Merge to main". Plan 5 with the breakdown went into `main` by a fast-forward to `4afab10`; on `main` the tests give 1054 passed, 0 failed, 2 ignored, and formatting and lints are clean. Nothing is pushed. The rows were merged before Jerod saw them in the real window, and the live test's output is still not reported: a defect found in either is fixed on `main`.

### The check with a second device

This answers the oldest open check in these notes, "Remote Control, seen by a person" (under "Open checks that need Jerod"). Two steps.

1. In the window, turn Remote Control on in a session: the switch in the panel. Wait until the tile's header says "Remote" in the accent colour. Then, at claude.ai/code or in the Claude app on another device: is the session listed, and does a prompt typed there run here? Its reply should stream into the tile; the prompt itself will not show above it (Task 13). "Copy link" puts the session's address on the clipboard: opened in a browser, does it go to that session? Turn the switch off at the end.
2. For Task 13, a recording, made in a terminal in the repository, with a folder of your choice for `SOME_FOLDER`:

   ```bash
   cargo run -p mr-probe -- raw --cwd SOME_FOLDER --full-login --arg --replay-user-messages --record target/rec/p5-remote.txt
   ```

   Then type, one at a time, waiting for each to be answered: `say Reply with the word one.`, then `rc on`, then a prompt typed on the other device (one that asks to run a command, so that a permission request may come), a permission request answered on the other device if one comes, then `rc off`, then `quit`.

   The file `target/rec/p5-remote.txt` will hold the Remote Control address of that test session: the probe's `raw` mode records every line the program sends. The terminal's echo of those lines shows the address's values (`session_url`, `connect_url`, `bridge_session_id`) as `<hidden>`; the file keeps them. Both also hold the account's e-mail address and your home path. Do not paste either anywhere. The file stays in `target/rec/`, which is never committed. Task 13 reads only the names and shapes of its records, and writes its test lines by hand with made-up values.

### Every decision of plan 5, with what was built

The seventeen decisions are the planner's; Jerod has not seen them.

1. **The panel starts hidden.** Built: the kit's trailing pane (`SplitView::inspector`), hidden until shown. The button `mr-panel-toggle` sits at the top right of the band above the tiles, labelled "Show panel" or "Hide panel", with the shortcut in its tooltip; it is no Tab stop. Ctrl+Alt+B (Cmd+Alt+B on macOS) does the same on the main screen, once per press, not on a held key. The panel is 300 points wide at first, and its grip takes it from 240 to 480; it collapses below half its least width, and a double press on the grip gives it 300 again, all the kit's own. Whether it is open and its width are kept in `Saved.panel_open` and `Saved.panel_width`, saved at once on a toggle; a drag of the grip is saved with the next save, as the sidebar's is.
2. **Opening the panel makes every tile narrower.** Built: none is closed. The row of tiles narrows by 292 points for a panel of 300, since the kit drops the main card's 8-point frame on its right while a pane borders it. Every tile is drawn where `Tiles::rects` places it in every frame of the panel's motion, and with both panes open in a window 560 wide nothing panics, no tile closes and the button still closes the panel.
3. **The effort level is a picker of its own.** Built in `view/input.rs`: a kit `Select` between the model and the mode, keyed `mr-effort-{n}`. It shows the level Claude Code reports, as Low, Medium, High, Extra high or Max (`Session::effort_label`), and "Effort" while none is known; its list holds the levels the model in use takes, in the model's order. For a model that takes none it is not drawn, and an open list of it shuts.
4. **A change of effort holds from the next message.** Built: a choice sends `apply_flag_settings` with `effortLevel`, and the adapter then reads the level back with `get_settings`, since the first reply confirms nothing. It also reads it after the session starts, after a change of model and after every turn. The picker changes once the level read back says so. When Claude Code goes on with another level, the conversation says "Could not change the effort level: Claude Code goes on with high" (or "with no effort level"), and the picker keeps the level in use. Of two choices in quick succession only the reading after the last can say so, and a choice is compared with the level last asked for while that is on its way, so going back to the level in use during a change asks for it (both since the fix wave). On a resting session the choice is taken at once and sent with `--effort` when the program starts. Nothing asks again by itself.
5. **A stored session goes on with the effort it last used.** Built: the history gives the `effort` of the last `assistant` record that names one, and the session passes it with `--effort` when it wakes. A level the model is known not to take is forgotten while the session rests. The list has no "default" entry.
6. **In a tile too narrow for three pickers the mode is cut first, then the effort, and the model's name last.** Built: the mode sits in a slot that takes the free room, so it gives up width first; the effort then shrinks to 48 points; the model gets what the others leave (`input::model_room`). At 48 points a trigger shows only an ellipsis and its chevron (see "Things to look at").
7. **The switch shows what was asked at once.** Built in `view/panel/mod.rs`: the switch `mr-panel-remote` shows on while Remote Control is wanted or while the program says it is not off, and a press asks for the opposite of what it shows. The session remembers what its running program was last asked until the program's answer settles it, so a second press before the first was answered is sent too (since the fix wave). The line under it says "Connecting…" from the press on. A refusal puts it back to off, with the reason under the switch and as a line in the conversation, once, also when the program tells it twice. The reasons are the plan's own words; a bare `/login` from the program is never shown.
8. **The switch never starts a program.** Built: on a resting or opening session the wish is kept and the line says "Turns on when the session starts."; the session asks when its program has started, after a rename and before the queued message.
9. **What was asked is kept for the run of the app.** Built: `Session.remote_wanted` survives the program's end, so a program started again for the session turns Remote Control on again. It is not saved.
10. **"On for new sessions".** Built: a second switch under the first, `mr-panel-remote-new`, off by default, saved in `Saved.remote_for_new`. It holds for sessions made with "New session" and for the session of a folder given at launch, not for stored sessions or a copy made by "Continue as a copy". It also shows when no session is open, and is disabled, as the first switch is, when the agent has no Remote Control ("Remote Control is not available.").
11. **The mark.** Built in `view/tiles.rs`: "Remote" (`mr-tile-remote`) between the state word and the close button, in the accent while on and muted while connecting or reconnecting. It takes its room from the title. A press on it is a press on the header.
12. **"Copy link".** Built: a small button beside the line, only while Remote Control is on and the address is known. It copies the address at the moment of the press. The address is not drawn, not a tooltip, not a label and not in the accessibility tree; the probe's `events` command prints it as `<hidden>`, and its `raw` mode's terminal echo hides the values of `session_url`, `connect_url` and `bridge_session_id`.
13. **Cost and context.** Built: "Context" in the panel reads "37% of context used" over a thin bar built and coloured by the sidebar's own usage line (the same levels at 75 and 90), and "Cost so far: $0.42", "less than $0.01" or "$0.00". Until a turn has ended they read "Context use is not known yet." and "Cost is not known yet.". The adapter now counts context as Claude Code's status line does: the input side of the last message that has usage and is not one the program made itself (`<synthetic>`), without output tokens, over the window of the model that answered, else the largest window named, at most 100%.
14. **Files changed.** Built: `Conversation::changes` in the core counts the Edit and Write calls that succeeded, per file, with the counts their rows show, as each finishes; a stored session's edits are counted once, when its history is read, and come first. The panel lists them under "Files changed" with their number: each path relative to the session's folder when it lies inside it, cut at its start, with the whole path as its tooltip, and its counts in a diff's colours, a count of 0 left out. With no file it says "No files changed yet." for a new session or a history that was read, and "Files changed are not known yet." while a stored session's history is not read, is being read or could not be read; edits made meanwhile show as rows. When the start of the history is not loaded, a last line says "Changes in earlier messages that are not loaded are not listed.".
15. **The branch.** Built in `crates/core/src/git.rs`: read from the repository's own files on the library thread, never by running git. It climbs at most 64 folders to a `.git` folder, or a `.git` file of a worktree that names its folder, and reads `HEAD`. The panel shows the name, or "detached at" and seven characters, and no line outside a repository or when `HEAD` cannot be read. One answer is kept per folder. The row appears when the answer arrives. The demo window reads none (`Ui.reads_branches`).
16. **The live test gains printed lines and no new way to fail.** Built: the lines above, in `crates/app/tests/live.rs`. It was compiled, not run.
17. **A prompt from another device is built only from a recording.** Not built: there is no recording yet (see "Left by plan 5").

### Rulings made while the plan ran

The controller made these, each recorded in the run's ledger with what it costs if it was wrong. All of them, with those made after the reviews, are in `docs/superpowers/plans/2026-11-15-plan-5-rulings.md`:

- **The level is said only when it is new.** The seam's doc comments promised an `EffortChanged` in answer to every `SetEffort`; Task 2's rule gives one only for the session's first reading or a level that differs from the last. Task 2's rule stands, the two doc comments say so, and the core never waits for an answer. If wrong: two doc comments.
- **The level a session starts with is the level it shows.** A session that rests again after its program said a level starts with that level, not with its history's older one. If wrong: one condition in `start_spec`.
- **A test for the panel's room.** The panel takes room from the row of tiles, which is a frame that uses both of Rune's re-run passes. Task 7 added `opening_the_panel_leaves_nothing_a_frame_behind`, and the panel makes no layout read. If wrong: one test.
- **The session focused at start gets its branch read.** If wrong: one file read at start.
- **The panel slides as the sidebar does.** The kit moves the pane's width on its spring, in about 0.6 seconds. It stands as built, for this look. If wrong: `tx.instant()` on the key's path, and `on_press_tx` on the button.
- **The switch shows on whenever Remote Control is on**, also when something other than the switch turned it on, so the switch never disagrees with the mark. A reason under the switch is cleared when the program says it is connecting, on or reconnecting. If wrong: one condition in the view and three lines in the core.
- **A copy keeps the wish.** A copy made by "Continue as a copy" keeps a wish switched on for the resting original; the preference never reaches a copy. If wrong: one line in `answer_prompt`.
- **No address is printed.** The probe's `events` command hides the address; since the fix wave its `raw` mode's terminal echo hides it too, while the file it records keeps the raw lines. If wrong: one match arm in the probe, and one replace in the echo.
- **The switches got an edge.** In the light theme an off switch was a white track on a near-white panel, about 1.1:1. The app now draws a 1-point edge round each of the panel's two switches in `icon_muted` (5.33:1 sampled in light), since no border colour of the theme reaches 3:1. The kit's colours and the theme are unchanged. If wrong: a look Jerod changes in one place.
- **"Files changed are not known yet."** while a stored session's history is not read or its read failed, as the context and cost lines say of theirs; built in the fix wave. If wrong: one string.
- **The panel's row keys carry `mr-`**: `mr-panel-label` and `mr-panel-value`. A review's fix, not a choice between answers; if wrong: two keys.
- **The panel's tests are in a file of their own**, `crates/app/src/view/panel/tests.rs`; the panel is `view/panel/mod.rs`. If wrong: a file moved back.
- **`tempfile` is a dev-dependency of `mr-core`**, for the branch tests' folders. If wrong: one line of `Cargo.toml`.
- **Tasks ran out of the plan's order** where they shared no file: Task 9 during Task 4's review, Task 8 during Task 4's re-review, and Task 12's probe and live test parts ("Task 12a") during Task 7's review, its notes ("Task 12b", these) after Task 11. If wrong: nothing; the commits are the ones the plan names.
- **The plan ends without Task 13**, by the plan's own rule: no recording was on disk after Task 11. If wrong: nothing built is lost; Task 13 waits for the recording either way.

### Things to look at

Each was decided by the build, not by the plan or the design:

- The panel slides open and shut in about 0.6 seconds, as the sidebar does: the kit's own motion. Plan 4 made tiles come and go at once. Is the slide welcome here?
- The row of tiles narrows by 292 points for a panel of 300, not by 300: the kit drops the main card's right frame while the pane is open. Can you see the difference?
- At 560 by 400 the mode and effort triggers are at their least width, 48 points, and show only an ellipsis and a chevron, no letter. Is that enough, or should they get more room at the cost of the model's name?
- Escape in the effort list sends the keyboard back to the input box. Escape in the model and mode lists leaves it on their trigger, as before this plan. Should they agree?
- The panel's button, while the panel shows, looks only faintly pressed, in the dark theme most of all. Can you tell whether the panel is open from the button?
- In the light theme an off switch is an outlined pill whose knob is white on white. Does it read as a switch that is off?
- The switch shows on whenever Remote Control is on, also when it came on by a way other than the switch. After a press to turn it off it stays on until the program answers, which it does at once; a second press in that moment asks for off again, which is on its way already, so nothing more is sent. A second press before the program answered a press to turn it on sends off, and the switch ends off once the program has answered both. Does the switch ever seem not to obey?
- Between a press to turn it on and the program's first word, the switch shows on but the header has no mark yet. Does the mark come soon enough?
- A press on a switch or on "Copy link" gives the keyboard to the focused tile's input box. A press while a name is being edited in a tile's header ends that edit; the name typed so far is kept. Should the edit stay open?
- Opening another tile's effort list does not focus that tile, unlike the model and mode lists; the focus stays where you type. Does it feel odd that the three lists differ?
- While a stored session's history is not read yet, is being read or could not be read, "Files changed" says "Files changed are not known yet.", in the same muted type as "No files changed yet.". Is that the right wording?
- The files are listed in the order their edits finished, not sorted, and not in the order of the rows: a stored call whose result comes late lands after live edits. Is that order useful?
- The branch is read on the library thread, after any history read queued there, so behind a long history it can show a moment late. Is the delay noticeable?
- The conversation writes "−0" for an edit that removed nothing, where the panel leaves the 0 out. Should the two agree?
- On Windows Ctrl+Alt is AltGr, so on some layouts Ctrl+Alt+B types a character and only the button opens the panel. It does not apply on Linux, but it is the same chord on Windows as on Linux.
- Screen readers hear the panel as "Inspector", the kit's name for its pane, which the app cannot set. A change to the kit would be needed.
- "Cost so far" is Claude Code's own running total for the session. On a subscription it is not money spent: it is what the session would have cost at the API's prices. Should it show for such an account, or say what it is?

### Left by plan 5

**Task 13 was not built.** No recording is at `target/rec/p5-remote.txt`. Until it is built, a prompt typed on another device runs and its reply streams into the tile, but the prompt itself is not shown above the reply. The recording made in step 2 of "The check with a second device" is what unblocks it: Task 13 is built only from that recording, and only if it shows that a prompt from another device arrives as a record the app can tell from its own prompts with certainty.

**Judged by the whole-branch review and left.** What the review said of each, and why it waits, is in "The whole-branch review of plan 5" below:

- The adapter's reader thread writes to the program's stdin (the `get_settings` reads), under the stdin lock. For plan 6.
- A stored session's edits are diffed once on the window's thread when its history loads (measured: Minor 9 below).
- The branch's climb is by the folder's name: a folder that is a link to a folder inside a repository finds no branch (one that links to a repository's root is found).
- `outside_a_repository_there_is_none` assumes the system's temporary folder is not inside a repository.
- Ctrl+Alt+B, like the sidebar's key, is handled before the check for a running drag, so the row can change size under a drag.
- A second copy of a refusal that arrives after a new press is taken as a new refusal. The two copies come one after the other, so the gap is small. Remembering what was asked (the fix of I-1) does not tell the copies apart: no reply says which request it answers.

**Found by the tasks and not fixed.** None blocks anything.

The seam and the adapter (`crates/agent`, `crates/claude`):

- The fake reports the effort it was started with even when its model does not list it.
- The history's effort does not skip program-made (`<synthetic>`) records, as its model does, and as the context reading now does. The test stub would answer `""` for an `effortLevel` of null.
- A `contextWindow` of 0 for the model in use gives no context, where the largest window would do. The fixture replay test names one model only.

The core (`crates/core`):

- When `App::settle` removes the focused tile of a session gone from the store, the focus moves and no branch is asked for.
- `a_stored_session_and_a_copy_do_not_get_the_preference` should also check the copy's queued message.
- A check in a `from_history` test runs twice and tests nothing; the conversation's test reaches into the session's internals.
- `session.rs` is about 2,500 lines and `translate.rs` about 3,250, most of them tests.

The window (`crates/app/src/view`):

- After a choice in the effort list the keyboard goes to the focused tile's input box, also when it was in that tile's "Something else" field or a rename. With an ended session in the focused tile, closing another tile's effort list leaves the keyboard on that trigger.
- With no session open, a press on "On for new sessions" leaves the keyboard on the switch.
- The panel builds one row per file each time it is built: it grows with the number of files, not with the conversation.
- A share or cost that is not a number is not guarded. The program does not send one.
- `TRACK_RADIUS` in the panel copies the kit's radius for its smallest switch, and would stop matching if the kit changed it.
- `view/panel/mod.rs` is about 570 lines and `view/panel/tests.rs` about 1,500.

Tests and the probe:

- No smallest-window test without an effort picker (the fake's model `fast`); the two-tile effort test opens a list by its toggle, not by a click; no test for Ctrl+Alt+B held down or on the sign-in screens.
- The long-folder and long-path tests check that the row stays inside the pane, not which end was cut; `Level::of` has no test of its own; a tiles test has the sidebar's width 260 written in.
- Two rewrapped comments run long or break oddly (`tests/snapshots.rs`, `view/mod.rs`).
- The live test does not print the program's reason for an `Other` refusal; it has two words no run can print (`(last: Refused)`, `(last: Off)`) and a near-copy of its Remote Control send for the effort. An older panic in it prints the whole conversation, which would hold a refusal's reason as a line. `live.rs` is about 1,530 lines.

### The whole-branch review of plan 5

After the twelve tasks a final reviewer read the whole branch, `0dc6d25..48acd5d`, for problems that live between tasks. It read in full the seam and the adapter's changes, every production hunk of `app.rs`, `session.rs` up to its tests, `saved.rs`, `usage.rs`, `changes.rs`, `conversation.rs`, `git.rs`, `glue.rs`, the panel, the probe, `process.rs`, these notes' first section and the new parts of `protocol-notes.md`; it sampled the tests, the live test, the demo's scenes and six screenshots. It answered two doubts by scratch tests in a copy of the sources outside the checkout, made with `git archive`, and its numbers come from those. Its verdict: ready to merge with fixes. Nothing critical; two Important findings, ten Minor. One fix wave followed, with the controller's rulings. The real `claude` was never started.

#### What was fixed

- **An "off" pressed before the program answered an "on" was not sent** (Important, I-1), so Remote Control came on against the user's last press, a double click on the switch being enough. A session now remembers what its running program was last asked (`Session.remote_asked`) until the program's answer settles it (on for an ask of on, off for an ask of off, a failure, a refused command, or the program's end), and a press is compared with that while an ask is on its way. Nothing of it sets the wish. The switch still shows on for as long as the program says so: after a quick on then off it goes off, comes back on with "Connecting…" and then "On. Reachable from claude.ai/code and the Claude app." and the header's mark when the late answer arrives, and goes off for good when the off is answered. If wrong: one field and its clearing rules. Tests: `turning_it_off_before_the_program_answered_asks_too`, `turning_it_on_again_while_the_off_is_in_flight_asks_too`, `what_was_asked_is_forgotten_when_the_program_ends`, and in the panel `a_second_press_before_the_answer_turns_it_off`. The live test no longer sends the off itself.
- **A session whose turns all came from another device counted as empty** (Important, I-2): closing its tile ended its program and forgot it, and a restart started it without its conversation. A session is now stored once its program ran a turn, whoever sent the prompt (`Session.ran_turn`, set at `TurnStarted`): it rests after a clean end, resumes its id, keeps its tile in the saved state and is not forgotten when left. Tests: `a_turn_nobody_here_sent_makes_the_session_stored`, `a_session_driven_from_another_device_is_kept`. The check with a second device stays as written.
- **Two effort choices in quick succession reported a failure that was none, and going back to the level in use during a change sent nothing** (Minor 1). The adapter keeps the level the latest choice asked for, and only the reading after it can say "goes on with"; the session compares a choice with the level last asked for while that is on its way. Tests: `two_choices_in_a_row_report_only_the_last`, `taking_a_choice_back_while_it_is_in_flight_asks_for_the_old_level`.
- **The context share could drop to 0% or go stale** after a record that is no reply (Minor 4, with the deferred finding of Task 3): it is now taken only from a record that has usage and is not program-made. Tests: `a_record_without_usage_keeps_the_last_reading`, `a_synthetic_record_does_not_count_as_context`.
- **"No files changed yet." stated as fact what was not known** (from Task 11): it now reads "Files changed are not known yet." until the history is read. Test: `files_are_not_known_until_the_history_is_read`.
- **The percentage was worked out twice** (from Task 11): the panel and `LimitWindow::percent` share `mr_agent::percent`.
- **No scene showed the Branch row, and the demo read the real disk for its made-up folders** (Minor 7). The demo reads no branch (`Ui.reads_branches`), and the `panel` scene gives "main" itself, so its screenshots show the row. Building the scene in a unit test showed that a demo session's first message could reach the fake after a later scene had scripted its reply, which it then took; `live_at` now waits until the fake has the message.
- **The live test** prints every kind of a refused Remote Control (from Task 12a), says when the app knows no levels for the model (from Task 12a), and in the second run prints the level passed with `--effort` on resume and the level the program then said (Minor 2). The first run's turns are in the order they were.
- **The probe's terminal echo hides a session's address** (from Task 12a, widened by Minor 3): the values of `session_url`, `connect_url` and `bridge_session_id`, wherever in a line they sit. The recording keeps raw lines.
- **Missing tests** (Minor 10): the branch is asked for after a failed turn and after an unclean end in the focused session; a new session passes no effort (`a_new_session_starts_with_no_effort`).
- **These notes**: the eight findings of their own review (every ruling has its cost, the live test's lines are listed as they can print, the chord on Windows, a file's size, the recording's e-mail address and home path, the `origin` key, the context reading in `protocol-notes.md`), and this section.

#### What was left, and why

- **"Connecting…" has no end when the program never answers** (Minor 5). The line stands as long as the wish does. A second press hides it and, since I-1's fix, tells the program. If the live test prints `no answer in 30 s`, this is to be settled then.
- **Which copy of a refusal arrives first decides the words** (Minor 6). The reply's `error_code` and the state's `failure_kind` are read by different fields; a reply without a code before a state with `auth` would show "Remote Control stopped: …" and never the sign-in help. Decided after the live run, which now prints every kind it hears.
- **`App.branches` is never pruned** (Minor 8): one entry per folder that had a branch while focused, bounded by the folders of the store.
- **A stored edit of thousands of wholly different lines costs a visible hitch once** (Minor 9). Measured by the reviewer in a release build: 600 stored edits of 200 lines take 41 ms in all through `Conversation::from_history`; one edit of 2,000 lines rewritten whole 23 ms, of 5,000 lines 141 ms. An unoptimised build is about ten times slower, which is why the look uses `--release`. No time limit is to be added; the counts move to the library thread only if a real session shows a stall.
- **The adapter's reader thread writes to the program's input** (the `get_settings` reads, under the stdin lock). A deadlock needs a line larger than the pipe's buffer being written by another thread, the reader wanting to write, and the program writing more than a pipe's worth before it reads that line; the reviewer could build no realistic case today. For plan 6: with attachments a line can be megabytes, so the reader is to hand its requests to the one thread that writes (one writer, a queue) instead of taking the stdin lock itself.
- **The deferred items the review's triage marks "No"**: the branch's climb by the folder's name (narrower than first written: only a link to a folder inside a repository misses), the temporary folder assumed outside a repository, Ctrl+Alt+B before the drag check, `settle`'s focus move asking no branch, the fake's effort for a model without levels, the history's effort and `<synthetic>` records, a `contextWindow` of 0, `TRACK_RADIUS`, the size of files, and the keyboard after an effort choice with a rename open. They are in "Left by plan 5" above.
- **A late second copy of a refusal after a new press** (from Task 6). The review thought remembering what was asked would cover it; it does not, since no reply says which request it answers. Left, as "Left by plan 5" says.

#### The re-review of the fix wave

One more reviewer, on the most capable model, read the six commits of the fix wave and ran four scratch cases in a copy outside the checkout. Every finding is addressed and nothing critical or important is new. There is no second wave, so what it found open is left as it is. All of it is small, and the rulings are 23 to 28 in `docs/superpowers/plans/2026-11-15-plan-5-rulings.md`.

- **Four fast presses on the switch (on, off, on, off) can leave Remote Control on.** The first "on"'s answer settles the second. The switch and the header show that it is on; one more press turns it off.
- **Three quick effort choices can end on the choice before the last, or show one false "Could not change the effort level" line.** The picker shows the level really in use; choose again.
- **A request the program never answers leaves its control inert.** After an unanswered "off" the switch stays on and more presses send nothing until the session is restarted; the same for an effort level never answered. It needs a program that drops a request, which was never seen. The live test prints `no answer in 30 s` if it happens.
- **A session driven only from another device reads "New session"** in its header and sidebar row until Claude Code names it. It is kept and resumed correctly.
- **The probe's terminal echo hides the session's address under three field names only.** Treat the terminal, like the recording file, as possibly holding it.
- **After a compaction the context bar keeps its old share until the next reply**, as Claude Code's own status line does.

#### What the final review declined to judge

Each line is the review's, followed by the controller's ruling.

- Whether "Cost so far: $0.42" is the right thing to show an account on a subscription, for whom it is not money spent: the spec asks for the session's cost and the plan takes Claude Code's own total. The line stays as Claude Code's own total; "Things to look at" asks Jerod.
- A stored session woken before its history has been read, or after its read failed, starts with neither its model nor its effort: the same race the model has had since plan 3. Older than this plan; left.
- "On for new sessions" switched on while an empty new session is focused does not reach that session: the plan says "sessions made with New session", and the session exists already. Stands as built.
- With the preference on, a new session left empty still shows at claude.ai/code for as long as its program runs: inherent in the preference. Stands as built; "Before looking" says it.
- An Edit with `replace_all` and a Write over an existing file count as their rows count, not as git would. Stands as built (decision 14).
- The pressed look of the panel's button, the 48-point triggers at 560 by 400, Escape in the effort list against the other two lists, AltGr on Windows, the pane's name for screen readers: in "Things to look at", for Jerod.
- A level chosen while a change is confirmed and the program then exits is lost: a narrow race with no data at stake. Stands as built.
- The test doubles' differences from the real program (the fake's effort for a model without levels, the stub's answer for a null level). Stand as built.
- The size of `session.rs`, `translate.rs`, `live.rs` and the panel's tests: for a later tidy.
- Commit `8b4c632`, the icon: not this plan's.

#### What rests on what was never seen

The review's table, short. Every one fails soft: no hang, no repeat, no loop.

- **`apply_flag_settings` with `effortLevel` changes the level.** If refused: one line, the picker stays. If ignored: "Could not change the effort level: Claude Code goes on with X", the picker stays on X. If unanswered: nothing changes and nothing is said. The live test's `effort changed to` shows the program reports the level, not that a message uses it.
- **`get_settings` gives `applied.effort`.** If it fails: silent; a new session's picker reads "Effort" for good, a stored one shows its history's level, and every choice ends in "Could not change the effort level: …". Another shape reads as no level. The picker is then useless and Tasks 2, 4 and 5 are reopened. Settled by `effort at start`.
- **Remote Control before a first turn.** If refused: a "Remote Control stopped: …" line in a session with no message yet, the wish dropped, and with the preference on in every new session. If unanswered: "Connecting…" without end (Minor 5). Settled by the second session's line.
- **`reconnecting`, `policy_disabled` and the codes.** An unknown state changes nothing, so a dropped connection keeps "Remote" in the accent and "On. Reachable …" until the next state, a claim that is then false. An unknown code reads "Remote Control stopped: {reason}". The live test does not settle these.
- **Listed at claude.ai/code and driven from there.** If not, "On. Reachable from claude.ai/code and the Claude app." is false. If it is, a session driven only from there is now kept (I-2). Jerod's check settles it.
- **A prompt from another device reaches this side as a record.** Nothing is built on it (Task 13); the reply shows without its prompt. The recording settles it.
- **`--effort` at start and on resume** (found by the review, not in the plan's table). A flag the program rejected would end it at start and fail the live test's resume loudly; a value silently ignored showed nowhere. Now settled by `live: resumed with effort X, the program says Y`.

## Jerod's look at stacked tiles

The stacked tiles plan (`docs/superpowers/plans/2026-11-15-stacked-tiles.md`) is built on the branch `stacked-tiles`, cut from `main`: the plan's fix `1e98a28`, the six tasks from `d7bff95` to `64c2921` (with one fix commit, `b9e1137`), these notes (`73add90`), and the fix wave after the whole-branch review (`8a4d1f5` to `9c09bb4`, and the commit that brought these notes up to date). On 2026-11-15 Jerod said to merge it into `main` before looking: `main` was moved forward to it (`c2746d0`), the branch `stacked-tiles` is gone, and nothing is pushed. None of the eight things to try was reported on when it was merged; they stay open, and a defect found in them is fixed on `main` or in the next plan. Each task's review passed. The whole-branch review found nothing critical and asked for fixes, which one wave made (see "The whole-branch review of stacked tiles", at the end of this section). After that wave `cargo test --workspace` gives 892 passed, 0 failed, 2 ignored (the live tests, not run). Jerod runs, in their own terminal, on `main`:

```bash
cargo run --release -p mindrune
```

With no folder the window shows again the tiles it had when it last closed: the same columns, the same tiles one above the other, the same heights and widths, the same one focused, without starting any program. Tiles saved by plan 4 come back as before, one to a column. Nothing makes a column of two by itself: a drop, "Open below", or Ctrl+Alt+PageUp and PageDown do (the things to try below). A build from before this merge that reads a file saved after it shows each column's upper tile only, and its next save drops the lower ones.

The screenshots of the two new scenes are in `target/snap/stack/` (`tiles-stacked` and `tiles-stack-drag`, dark and light), not in git.

### The live test

None is needed for this look, and the live test is not changed (decision 14). Stacking is a matter of the window and the core alone: nothing about it reaches Claude Code. Plan 4's live test stands as "Jerod's look at plan 4" describes it.

### Before looking

What this window will not do yet, so it is judged for what it is:

- A column holds two tiles at most. A third is never added to it, by any way in.
- A column is split only while the window is about 616 points high or more: the row of tiles, which is the window less the band above it and the frame, must be 560. In a lower window the ways to stack are disabled or give what they gave before.
- In a window too low for two tiles of 280 points the two tiles of a column are drawn even, whatever their line was set to, and the line comes back as it was set when the window is made taller.
- Moving a tile sideways by its header now needs the outer quarter of a neighbour. Let go over a neighbour's middle half, the tile joins that column. In plan 4 any point past the middle reordered.
- By the keyboard a tile joins only the column on its left (the right one when it is the first), cannot go from one column of two straight into another, and cannot leave a column of two when no further column fits. The header can do the second.
- "Open to the side", Ctrl+click and Ctrl+Shift+N never stack, also when no column fits and a tile could be split. A new session cannot be started below a tile.
- A long draft can still run out of a short tile. Ten lines typed in a tile 280 points high end about 9 points past it, and more with a message waiting above the box; in the lowest window, where the two tiles of a column are about 172 points high, about five lines do. Nothing typed is lost, and a taller window gives the room back.
- A tile cannot be dragged from one window to another; there is one window.
- While a row or a header is dragged the sidebar does not scroll by itself, as before.

The other points of plan 4's "Before looking" still hold. Its first, that tiles cannot be stacked, is what this plan changes.

### What to try in the window

1. A session dragged onto the lower half of a tile opens under it; onto the upper half, over it; near where two tiles meet, it still opens a column. Seen: not yet recorded.
2. "Open below" in a row's menu opens a session under the focused tile. Seen: not yet recorded.
3. The line between two tiles drags, stops at a sensible height, and evens out on a double click. Seen: not yet recorded.
4. A tile dragged by its header onto another tile joins its column; dragged to an edge it gets a column of its own; dropped on its mate the two swap. Seen: not yet recorded.
5. Ctrl+Shift+PageUp and PageDown, and Ctrl+Alt+PageUp and PageDown, move the focused tile as said; F6 goes through the tiles in reading order. Seen: not yet recorded.
6. Closing one of two gives the other the column. Seen: not yet recorded.
7. Close and open the app: the same columns, the same heights, the same focus. Seen: not yet recorded.
8. On the ultrawide: how many tiles are comfortable now, and is a half-height tile enough to work in? Seen: not yet recorded.

### Every decision of the stacked tiles plan, with what was built

The first decision is Jerod's; the others are the planner's, and Jerod has not seen them.

1. **Two at most (Jerod, 2026-11-15).** Built: a `Column` has room for one lower tile (`under: Option<Under>`), so a third cannot be held, and the saved form (`SavedTile.below`) has room for one too. Every way in refuses a third: a drop over a full column opens a column at an edge, or replaces a tile when no column fits; "Open below" is disabled; the keys do nothing.
2. **Height.** Built: `MIN_TILE_HEIGHT` is 280 and `Tiles::fits_two` asks for a row of 560. The line's drag stops each tile at 280 (`Tiles::resize_split`) and does nothing in a lower row. A column keeps its split, not its heights; none is closed. Changed after the whole-branch review: the plan said both tiles shrink in proportion in a lower window, and an upper tile could then be too short for its own input box. Now what is drawn keeps both tiles at least 280 high while the row fits two, and in a lower row draws the two the same height (`upper_height` in `crates/core/src/tiles.rs`, which `Tiles::rects` uses). The split as set is kept, in the model and in the saved state, so a window made taller shows it again. While the row's height is not known yet, stacking is allowed.
3. **A new lower or upper tile takes half its column.** Built: a split of 0.5. The line is a handle 8 points high over the upper tile's lower edge, as wide as the column, with the role "separator", the label "Tile height" and `Cursor::ResizeRow`; it takes neither the keyboard nor the tile focus. A double press calls `App::even_split`. A double click on a divider still evens the widths only (`Tiles::even` leaves the splits).
4. **Where a dragged row lands.** Built in `view::drag::target_in_tiles`, in this order: over the middle of a tile that can be split, its half; else, with room for a column, the nearest edge; else, over the whole width of a tile that can be split, its half; else the tile is replaced. Three tests of plan 4 that pinned a replacement in a window 1,300 by 900 now run 1,300 by 500, where it still happens; at 1,300 by 900 the tile now stacks.
5. **What shows during the drag.** Built: `mr-drop-half`, an outline 2 points wide in the accent, in the overlay layer, around the half the session will take, at once and with no line at any edge. While any target is in the tile row the focused tile's accent outline is not drawn; amber stays, also on a tile whose half is the target. The outline is drawn only while its target still holds (a ruling, below).
6. **"Open below".** Built in `view::sidebar::row_menu`, after "Open to the side": it opens the session under the focused tile with the keyboard in its box. It is disabled when the session is in a tile, when no tile is focused, and when `App::can_put(Spot::Below(focused))` is false (a full column, a low row). "Open to the side", Ctrl+click and Ctrl with a folder are unchanged.
7. **Moving a tile by its header.** Built in `view::drag::move_target`. Over the middle of another tile: its half, when that move is allowed. Over the middle of its column mate: the spot that swaps the two, outlined around all of the mate's tile, the place the dragged tile will take. Else the nearest edge of the row, when that move is allowed, as for any tile: for a tile of two that is an edge of its own column only while the pointer is over its own column (the dragged tile itself, or the outer quarters of its mate). When no further column fits, a tile of two shows no line anywhere. A tile alone in its column, over itself, shows nothing, as before.
8. **Moving a tile by the keyboard.** Built in `view::tiles::step_move` and `step_stack`, as the decision says; `move_step` and `stack_step` read the keys. On `Screen::Main` the keys are taken also when nothing moved. They wait while a list or a menu is open or a drag runs, and leave the keyboard where it is.
9. **F6 and Shift+F6** go column by column, the upper tile first. Built: `view::tiles::step_focus` goes through `Tiles::keys`.
10. **Closing.** Built in `Tiles::remove`. Out of a column of two the other tile has the column. The focus moves only when the closed tile had it: to the other tile of its column of two; else, its column gone, to the upper tile of the column now in its place, else of the one on its left.
11. **Widths belong to columns.** Built in `Tiles::move_to`: a tile that joins a column drops its column's share, and the other columns share the row in proportion. A tile out of a column of two gets the mean of the shares, and the columns are then kept to 420 points as when a tile is added. A tile that moves with its column keeps its share.
12. **Heights stay with the tiles when two swap.** Built: the swap sets the split to `1 - split`.
13. **Saving.** Built: one `SavedTile` per column, with `below` for its lower tile and the split. It is saved when a tile opens above or below, moves or closes, when the line is let go at another height than at its press, when a double press changed the split, and when the window closes. A session the store does not have is left out as before; of two, the other is kept as a column of one. The restore waits for the lower tiles' sessions too, and a column whose upper session is gone shows the lower one alone.
14. **The live test is not changed.** Built: `crates/app/tests/live.rs` only follows the renamed calls of Task 1; it was compiled, not run.

### Rulings made while the plan ran

The controller made these, each recorded in the run's ledger:

- **A test got a wider row.** Before Task 1, the plan's test `a_tile_leaves_its_column_for_one_of_its_own` asked for shares 2 and 3 in a row 1,600 wide, where the rule that keeps every column at 420 points raises A and changes them. The plan was fixed (`1e98a28`): the test's first case is in a row 2,600 wide, and a second case pins the raise at 1,600. The rule stays, the same as for a tile added to the row.
- **The two drags share one function.** The plan said the line's drag follows the divider's "phase for phase". Rather than a second copy, `view::tiles::drag_handle` runs both, given what differs as a `HandleDrag` (`DIVIDER_DRAG`, `SPLIT_DRAG`): the axis, the press slot (`Ui.divider_press`, `Ui.split_press`) and the app's intents. The function `view::tiles::drag_divider` is gone (a test helper of that name in `view/tiles.rs` stays); the divider's behaviour and its tests did not change.
- **The low-window test stacks first.** The core refuses to make a column of two once the row is known to be lower than 560 points, so `a_column_of_two_in_a_low_window_is_still_drawn` stacks in a window 1,800 by 900 and then makes it low. That is also how a user gets there.
- **The tiles' level is exported as `mr_core::TileLevel`.** Inside the core it is `tiles::Level` (`Whole`, `Top`, `Bottom`), but `mr_core::Level` is already the usage level (`Normal`, `Warning`, `Alarm`).
- **Points carried from one task's review into the next**, each into the task that worked in the same code:
  - From Task 1 into Task 2: `App::close_tile`'s doc reworded for columns, and the `allow(dead_code)` taken off `Tiles::resize_split` and `set_split`.
  - From Task 3 into Task 4: two tests. `the_upper_tile_of_two_draws_a_line_on_its_lower_edge` pins that line by a pixel of a rendered frame, since Rune's test interface does not show an element's borders. `a_column_of_two_in_a_low_window_is_still_drawn` also checks that each input box lies inside its tile.
  - From Task 5 into Task 6: the test helper `columns` moved to `testkit.rs` (it was in `drag.rs` and `sidebar.rs` word for word); the half's outline is drawn only while its target still holds; a test that a tile that needs you keeps its amber while a half of it is the target.

### Things to look at

- The line under the upper tile of two cannot be seen where both tiles of the column have an outline (the focused one and one that needs you, as in the `tiles-stacked` scene): the two outlines, 2 points each, cover the line of 1 point. The handle is there all the same. Is the line easy to find when you want to drag it?
- A tile dragged by its header over its column mate shows the outline around all of the mate's tile. Its place is tested; nobody has looked at it on screen. Does it say "these two swap"?

### Left by the tasks

Found by the per-task reviews and reports, and not fixed. None blocks anything.

The core (`crates/core`):

- The rule "of two, one missing leaves the other alone" is written twice, as a `match` in `App::restore_tiles` and in `App::saved` (`app.rs`).
- `app.rs` is 5,159 lines, mostly tests.

The window (`crates/app/src/view`):

- Every frame in which the row's size changes uses both of Rune's re-run passes (`PASSES = 2` in Rune's `runtime/layout_read.rs`), with or without a column of two, so no layout read may be chained behind a tile's rectangle. See "What plan 5 must know".
- The line at an edge (`mr-drop-edge`) is still drawn from a target that may have gone stale since the last move, as before this plan. The release checks again, so nothing wrong is done; the half's outline does check.
- `stack_step` and `move_step`, and their two blocks in `view::on_key`, differ by one modifier.
- In `view::tiles::row` the closures' parameter `laid` shadows the list of the same name.
- `view/tiles.rs` is 3,719 lines and `view/drag.rs` 2,688, mostly tests.

Tests:

- `the_upper_tile_of_two_draws_a_line_on_its_lower_edge` renders a frame on the GPU (`Gpu::headless()`) inside a unit test. On a machine with no GPU or software adapter it fails, where before only the snapshot test needed one.
- No test cancels the line's drag by a lost pointer; Escape is tested.
- The case where, with no room for a column, a whole tile stacks (the third rule of `target_in_tiles`) has no test that no conversation row is built again.
- The `tiles-stack-drag` scene's pointer, `STACK_AT` in `demo.rs`, is placed by hand and not tied to `target_in_tiles`, as `DRAG_AT` and `MOVE_AT` are not.
- In `with_no_room_a_tile_of_two_has_no_edge` (`drag.rs`) an inner `fy` shadows the outer one, and each case's tuple holds two equal bools. The test helper `key` in `view/tiles.rs` is shadowed by locals of the same name.

### The whole-branch review of stacked tiles

After the seven tasks a final reviewer read the whole branch, `e90a9fe..73add90`, for problems that live between tasks. It read all the code of `crates/core/src/tiles.rs`, `view/tiles.rs` and `view/drag.rs` outside the tests, every changed part of `app.rs`, `saved.rs`, `glue.rs`, `view/mod.rs`, `view/sidebar.rs`, `testkit.rs` and `demo.rs`, the plan, these notes and the parts of Rune the layout reads and the page keys go through; it sampled the tests and looked at two of the screenshots. In a copy of the sources it counted the runs of `view::tiles::row` and added six throwaway tests, and its numbers come from those. Its verdict: ready to merge with fixes. Nothing critical; one Important finding, in these notes; five Minor. It found no way to put a third tile in a column, to leave a column empty or to rebuild a tile. One fix wave followed, with the controller's rulings.

#### What was fixed

- **The notes on Rune's re-run passes** (the review's Important finding). They said only a window that starts with a column of two uses both passes, and that one more chained read would be a frame late at start. Measured, every frame in which the row's size changes uses both, and a read past them stays stale. Both places now say so ("Left by the tasks" and "What plan 5 must know").
- **A short tile's input box ran out of its tile** (Minor 1). With the line dragged to a stop and the window then made low, an upper tile was drawn 114 points high in a window 400 high, its box over the lower tile's header. A ruling: what is drawn keeps both tiles of a column at least 280 points high while the row fits two, and draws them the same height in a lower row; the split kept in the model and in the saved state is not changed, so a taller window shows it as it was set (`upper_height`, used by `Tiles::rects`). Decision 2's "both tiles shrink in proportion" is replaced by it. If wrong, it costs one function: Jerod may have wanted the proportion kept in a low window. Tested in the core (`the_drawn_split_keeps_both_tiles_usable`) and in the window (`a_line_dragged_to_its_stop_keeps_both_boxes_in_their_tiles`).
- **A cancel after the row changed put sizes back on another pair** (Minor 2). With a divider or the line held, a key that moved a tile and then Escape put the shares or the split kept at the press onto whatever pair was there now. A ruling: when the tiles' placement changes, by any way, `glue::run` forgets what a held divider or line keeps (`glue::tidy_handles`), so the cancel puts nothing back; the window's keys are not held back while a handle is held. If wrong, it costs this: a key pressed mid-hold still acts, as in plan 4. A change of the focus alone also counts as a change of placement, so Escape after F6 mid-hold puts nothing back either. Tested by `a_held_divider_puts_nothing_back_after_a_key_moved_the_tiles` and `a_held_line_puts_nothing_back_after_a_key_moved_the_tiles`.
- **A press on a handle before the row's size was known saved at its release** (from Task 4). A start that is not a number now counts as no change (`view::tiles::unchanged`).
- **`App::close_tile`'s doc** now says the focus moves only when the closed tile had it (from Task 2).
- **No test that dragging the line builds no conversation row** (Minor 5): `dragging_the_line_rebuilds_no_row` makes ten moves of the line with two long conversations.
- **Lines of these notes that were untrue or missing** (Minor 3 and the six points of Task 7's review): a drag test does have the target's column fill before the drop (`a_half_whose_column_filled_meanwhile_is_not_outlined`, `view/drag.rs`); the 560 points are said as the window Jerod sees; "Before looking" says what changed for moving by the header and the keyboard, that "Open to the side" never stacks, and how a low window draws a column of two; the keys make a column of two too; decisions 7 and 10 say what was built; "What plan 5 must know" says which statements of plan 4's bullets no longer hold; the function `drag_divider` is named as such.
- **"Neighbours meet exactly"** (Minor 4) holds for the numbers `Tiles::rects` gives, not for what Rune draws; "What plan 5 must know" says so.

#### What was left, and why

The review asked for none of these to be fixed before the merge. They are in "Left by the tasks" above:

- From Tasks 1 and 2: the rule "of two, one missing leaves the other alone" written twice, and the size of `app.rs`.
- From Task 3: the shadowed `laid` in `view::tiles::row`, and the size of `view/tiles.rs`.
- From Task 4: the pixel test that needs a GPU adapter (the snapshot test needs one already, so `cargo test --workspace` is no worse off), and no lost-pointer test for the line (it goes through `drag.cancelled()` as Escape does, which is tested).
- From Task 5: no test that rule 3 of `target_in_tiles` rebuilds no row, and `STACK_AT` placed by hand.
- From Task 6: the line at an edge drawn from a target that may be stale (while a half's target is stale no accent shows anywhere until the next move, and the release does nothing), the shadowed names in the tests, and `stack_step` and `move_step` with their two blocks in `view::on_key`.

The one re-review of the fix wave found every finding addressed and three small things more. There is no second wave, so they are left, by a ruling each:

- **A line pushed against its stop where it is already drawn there saves nothing at its release.** Since the drawn split is kept inside the least heights, a split of 0.2 in a row 600 points high is drawn at 280. Pushing the line up against that stop sets the split to 280 of 600 in the model, the drawn height stays 280, and `view::tiles::unchanged` compares drawn heights, so the release asks for no save. The saved state has the new split with the next save, at the latest when the window closes. If the app dies in between, the old split comes back, which draws the same in that window. Left: it costs nothing a user sees. The fix is to compare the split, not the drawn height, at the release.
- **A long draft in a short tile** still runs out of its tile, as "Before looking" says. The fix of this wave covers the split, not the height of the text area, which grows to ten lines whatever its tile's height (`view/input.rs`). Left for the plan that next works on the input box: plan 6 (attachments) adds chips to it and must settle how high the box may grow in a short tile.
- **The sentence over "What the final review set aside"** said each item was set aside because a decision already covers it. Several were set aside for other reasons. It now says so.

#### What the final review set aside

The review declined to judge these, each for the reason its line gives: a decision of the plan or of plan 4 already says how it is, or it is outside the plan, or only the real window can say. Each stands as built, and each is a small change if Jerod wants it otherwise after the look.

- Whether the middle half of a tile's width is the right share for stacking, against the outer quarters for a column (decision 4): stands as built.
- Whether Ctrl+Alt+PageUp and PageDown are free on Jerod's desktop: only the real window can say; Rune's own page keys refuse Ctrl and Alt, so nothing in the app takes them first. Stands as built.
- A tile alone in its column joins the column on its left by the keyboard, and the one on its right only when it is the first (decision 8): stands as built.
- "Open to the side" and Ctrl+click replace the focused session when no column fits, though a split would fit (decision 6): stands as built.
- No way to start a new session below a tile: outside the plan; stands as built.
- A double press on the line evens the two also in a row too low to drag in, and the handle shows the resize cursor there: the plan speaks only of the drag. Stands as built.
- The line cannot be seen where both tiles of a column are outlined: a question for Jerod under "Things to look at"; stands as built.
- A session swapped into a tile of a column by a plain click asks for no save: plan 4's ruling 6; stands as built.
- A stale edge target after a column went shifts the drop by one column when the pointer has not moved since: as before this branch; stands as built.
- A frame in which the window is resized costs one view run more than in plan 4: the cost of placing every tile by its rectangle; nothing in it grows with a conversation's length. Stands as built.
- A tile announced as a button, and the drop line's glide: plan 4's open items, untouched here; stand as built.

## Jerod's look at plan 4

Plan 4 ("tiles") is on `main`: on 2026-11-15 Jerod said to merge it there before looking, and to report on the look afterwards. The branch `plan-4-tiles` is gone, nothing is pushed, and none of the ten things to try was reported on when it was merged; they stay open, and a defect found in them is fixed on `main` or in the next plan. Jerod runs, in their own terminal:

```bash
MINDRUNE_LIVE=1 cargo test -p mindrune --test live -- --ignored --nocapture
```

```bash
cargo run --release -p mindrune
```

With no folder the window shows again the tiles it had when it last closed, as wide as they were and with the same one focused, without starting any program, or says how to start a session. A new session you sent nothing in is not among them: it is left out, the others take its room, and when it was the focused one the first tile is. `cargo run --release -p mindrune -- FOLDER` starts a new session in FOLDER instead.

### Before looking

What this window will not do yet, so it is judged for what it is:

- Tiles sit in one row. They cannot be stacked one above another.
- There is no right panel, and a tile's header has no Remote Control mark.
- Closing the window ends every session's program. Running in the background is plan 5.
- While a row is dragged the sidebar does not scroll by itself. A group that is out of sight is reached through "Move to group".
- Tiles that come back into a window narrower than the one they were saved in come back narrower than 420 points if they must. None of them is closed.

### The live test

It needs Claude Code installed and signed in. It runs the app twice, as the window would be opened, closed and opened again, and starts the real `claude` four times in all: the test's session, the look the first run takes at launch, a second session opened to the side of the first, and the first session resumed in the second run. The account is checked once.

What it prints, in order, besides plan 3's lines (see "Jerod's look at plan 3"). None of these lines fails the test:

- `live: the session to the side replied in X ms; each reply is in its own session`.
- `live: the name was confirmed in X ms; a listing gave it Y ms later`.
- `live: restored 2 tiles, the focus on the first`, in the second run, before it resumes the first session.

What fails it, besides what failed plan 3's: no reply in the second session, a reply that does not say the word asked for, either session holding the other's message or reply, the program not confirming the new name, no listing giving the new name within 30 seconds of its confirmation, and the second run not bringing back both tiles in their order, the first focused and under its new name.

Two things the rename step counts on were read from the program's protocol and never seen live: that `claude` confirms a new name by sending the title back, and that a listing of the store gives the new name within 30 seconds of that. If the test fails at the rename, the first thing to check is which of the two did not happen; its failure line says which ("did not confirm the name" or "no listing gave the new name").

What stays behind: the two sessions `claude` stores for the test, both in a temporary folder that is deleted when the test ends. They appear in the window's sidebar under one group named after that folder (`.tmp` and some letters); the first is named "MindRune live test". "Hide group" removes the group by hand. Of the other stored sessions the test prints only counts and times.

### What to try in the window

1. Two sessions side by side, both working at once, each answering in its own tile. Seen: works (Jerod, 2026-11-15).
2. Ctrl+click on a row and "Open to the side" in a row's menu open a tile; a plain click swaps the focused tile's session. Seen: works (Jerod, 2026-11-15).
3. The outline follows a click and typing; F6 and Ctrl+W do what they say. Seen: works (Jerod, 2026-11-15).
4. A turn that ends in the tile you are not in says "Finished" until you go there. Seen: works (Jerod, 2026-11-15).
5. A divider drags, stops at a sensible width, and evens out on a double click. Seen: works (Jerod, 2026-11-15).
6. A row dragged between two tiles opens there; a row dragged onto a group moves. Seen: works (Jerod, 2026-11-15).
7. A session renamed by a double click keeps its name, also in Claude Desktop once it has run. Seen: works (Jerod, 2026-11-15).
8. Close and open the app: the same tiles, the same widths, the same focus (but for a new session you sent nothing in, which is left out). Seen: works (Jerod, 2026-11-15).
9. On the ultrawide: how many tiles are comfortable, and is anything too small? Seen: works (Jerod, 2026-11-15).
10. A tile dragged by its header goes where the line shows, with its width; Ctrl+Shift+PageUp and Ctrl+Shift+PageDown move the focused tile. Seen: works (Jerod, 2026-11-15).

### What Jerod reported on 2026-11-15

Jerod ran the window after the merge. Shown plan 3's seven things again, they said: "Those look good." Asked what that covered, they chose: the window was run, and plan 4's ten things and plan 3's seven all work. So each of the seventeen is recorded as seen. Nothing was said about what the live test printed, about the decisions listed below, or about the "Things to look at"; those stand as built until Jerod says otherwise.

In the same message Jerod asked for the next two pieces of work and settled one thing about the first: tiles stacked one above another support two rows and no more. Asked which shape that means, they chose: a column of the row can hold two tiles, one above the other; a column with one tile keeps the full height. Asked what of plan 5 comes first, they chose the right panel, Remote Control and the effort level; attachments and running in the background follow in plans of their own.

A defect found later in something plan 4 built is fixed on `main` or in the next plan.

### Decisions made while building that you have not seen

The plan made 21 decisions of its own (in the plan under "Decisions in this plan that the design does not make"). These were made while it was built, each because the plan's wording did not hold up in the window; three after the review of the whole branch, and the last four with Task 15 (moving a tile), which you asked for on 2026-11-15:

- **Typing keeps its tile.** While you are typing in a tile, that tile stays the focused one. Pressing Allow, Deny, a question's option, the arrow that opens a tool's line, or "Show all" in another tile acts there and leaves you typing where you were. When you are typing nowhere, the same press focuses the tile it was in. A click on a tile's text or header always moves the focus there.
- **A tile whose session has ended can still be focused.** F6 and a click reach it; its input box cannot take text, so nothing has the keyboard then.
- **Selecting text in a reply** (a double click, a triple click, Shift+click) focuses its tile and keeps the selection, so Ctrl+C copies it; the input box does not take the keyboard.
- **A name you are typing is not lost easily.** Starting to rename something else keeps what you typed in the first, for groups too (plan 3 dropped it between two groups). Clicking elsewhere keeps it. Only Escape, or closing the tile whose title you are editing, drops it.
- **A drag can always be called off.** While a row is dragged, Escape cancels the drag and does nothing else, and the window's keys (Ctrl+N, Ctrl+Shift+N, F6, Ctrl+W) and the sidebar's arrow keys do nothing. A menu opening ends the drag.
- **Every drop target is marked in the accent colour**: the line between two tiles, the outline of the tile that would be replaced (also on a tile that needs you, whose amber gives way for the length of the drag), and a ring around a group's header.
- **While a dragged row points into the tiles, the focused tile loses its outline**, so the accent marks only where the drop lands; otherwise the line beside the focused tile lay on its outline and could not be seen. A tile that needs you keeps its amber while the line is at one of its edges. The outline comes back when the drag ends or leaves the tiles.
- **Letting go of a dragged row is not also a click** on that row, wherever it is let go.
- **The first tile opened by a plain click is not saved at once.** The arrangement is saved when a tile opens to the side, closes, has its divider let go, and when the window closes, as the plan said; one tile shown by a click is kept when the window closes.
- **The saved arrangement survives a damaged settings file.** A tile whose width cannot be read is kept, and comes back with about an even share of the row; a name that cannot be read is left out; the rest comes back.
- **The arrangement from last time is dropped** as soon as you open anything in a tile before it came back.
- **A session renamed before its program has started** is given its name as the program starts.
- **Ctrl held on a custom group's "+"** opens its folder choice for a session to the side, as Ctrl does elsewhere.
- **The arrow keys in the sidebar stay in the tile you browse in.** Moving through the list swaps the focused tile's session row by row; when the arrow reaches the row of a session that is in another tile, that row is only selected, and no tile changes or takes the focus. The next arrow goes on in the tile you were browsing in. Return, Space or a click on that row moves to its tile.
- **A new session you sent nothing in is not kept with the tiles.** Claude Code stores nothing for it, so it could not come back: the arrangement is saved without it, and when it was the focused one, the first tile kept is focused next time.
- **"Show all (1 more line)"** is said when exactly one line is left out, for a tool's output and for a message alike.
- **Moving a tile has keys.** You asked for moving a tile; the two keys (Ctrl+Shift+PageUp, Ctrl+Shift+PageDown) were added with it so that it can be done without the pointer, as every other tile action can. They are the keys a browser and a terminal move a tab with.
- **A row of a session that is already in a tile still only focuses its tile** when it is dropped on the tile row. A tile is moved by its header. Letting the row move its tile too would be a small change.
- **A tile dropped on the sidebar does nothing.** It does not move its session to the group under the pointer.
- **A press on a tile's header leaves the keyboard where it is until the release.** Before, the press took the keyboard from the field you were typing in at once. A click ends as it did, with the tile focused and the keyboard in its box; a drag cancelled with Escape now leaves you typing where you were.

### Things to look at

Each is a question the reviews left for you, with what they saw:

- In the light theme the focused tile's row in the sidebar and the other tiles' rows are almost the same grey, and rows of tiles give no feedback when the pointer is over them. Can you tell which row is the focused tile's?
- A divider's grip covers the outer 4 points of the right edge of the tile on its left: that tile's scroll bar there, and its own handle for the conversation width when the width is "no limit". Does either feel hard to reach?
- A name being edited, in a tile's header or in a row, has no box or border, as plan 3's group rename. Only the caret and the selection say it can be typed in. Is that enough?
- The "Show all" button under a long message of yours is as wide as the message, with its label centred under left-aligned text. Does it look right?
- The line that marks where a dragged row would open sits right against the amber outline of a tile that needs you. The title that follows the pointer is not kept inside the window at its right and bottom edges. Can you see where a drop will land?
- Closing the tile you are typing in (Ctrl+W) keeps its text and caret but loses that box's undo history. Does it matter?
- The word "Finished" in a tile's header is drawn in the accent colour; in the screenshots it reads clearly in both themes. Is it the right word and the right weight beside "Working" and "Needs you"?
- In a very narrow tile (three tiles in a window 1,200 wide, where a tile's text is about 270 points wide) a long line of code runs about 10 points past its block. Is a tile that narrow ever useful?
- A command waiting for your permission is listed above the card as "Ran touch a.txt" with a spinner, in the past tense while it has not run. This is from plan 2, seen again in the tile screenshots. Should it read otherwise while it waits?
- The arrangement is saved when a tile opens to the side, when one closes, when a divider is let go, and when the window closes. A session swapped into a tile by a plain click asks for no save of its own: it is written with the next thing that saves (one of those four, a rename, a change to a group, a new listing). So after an unclean end (the desktop logging out, the program killed) the tiles that come back are those of the last save, and may hold sessions that were swapped out since. Saving at every change would be cheap. Should it?
- While a row is dragged over the tiles, the blue line that shows where the tile would open glides from one edge to the next in under half a second, where everything else about tiles happens at once. Is the glide welcome, or should the line jump?

### Plan 3's seven things

The seven things to try of "Jerod's look at plan 3" were carried into this look as they are: see that section, below. Jerod reported on them on 2026-11-15 with the ten above: all seven work.

## What plan 5 must know

Plan 5 was to add attachments, the right panel, Remote Control and running in the background. On 2026-11-15 it was cut in three: plan 5 is the right panel, Remote Control and the effort level (`docs/superpowers/plans/2026-11-15-panel-remote-effort.md`), plan 6 attachments, plan 7 running in the background. Stacked tiles (`docs/superpowers/plans/2026-11-15-stacked-tiles.md`) are built before plan 5. What plan 4 built that these meet:

- **The tiles are in the core.** `crates/core/src/tiles.rs` holds `Tiles`: the tiles left to right, each a session's key and a share of the row, and the focused tile. `App.tiles` is one `Tiles`, never assigned after `App::new` (`Tiles::restore` replaces it in place when the saved arrangement comes back). Two counters say what changed: `version` with every change, `placement` with every change but of widths; `App::settle` rebuilds the sidebar on `placement` only. `Place` says where a session is shown (`Focused`, `Side`, `At(i)`, `Tile(key)`); `App::show(key, place, leaving)` is the one way in, and a session already in a tile only has its tile focused. `App::focused()` is the focused tile's session. Room is `MIN_TILE_WIDTH` (420 points) per tile, against the row's width the window reports (`App::set_row_width`); `App::room_for_tile` says yes while that width is not known. `App::resize_tile`, `even_tiles`, `put_tile_pair`, `move_tile` and `close_tile` change the row. A tile is moved to an edge of the row as it is before the move, counted as `Tiles::edge_at` counts them: `Tiles::moved_index` says where it would sit, and none at its own two edges; `Tiles::move_to` moves it there with its share and focuses it, counting once on `placement`. `App::move_tile` also counts its session as seen and asks to save. A turn that ends in a tile that is not focused sets `Entry.unseen`, which is `Mark::Finished` until that tile is focused.
- **What is kept.** `Saved.tiles` holds the tiles by the agent's id with their shares, `Saved.shown` the focused one, and `Saved.names` the names the user gave that no program has confirmed yet. `App::saved` keeps a tile only when the store has its session (`Session::is_stored`: it came from the store, or a message was handed over), since a new session nothing was sent in has an id the store does not know and would hold the restore back until the complete listing; `Saved.shown` is the focused session's id when its tile is kept, else the first kept tile's id, else none. `SAVED_VERSION` stays 1; a share that cannot be read reads as 1, the share of every tile in an even row (the tile is kept), a name entry that cannot be read is left out. Shares are scaled to a mean of 1 when restored.
- **A tile's header** (`view::tiles::header`) is one row: the folder chip, the title (or its name field while it is renamed), the state word, the close button. The place for the Remote Control mark is between the state word and the close button: the title is the part that shrinks (`grow(1)`, `min_w(0)`), so a mark there takes room from the title, never from the close button. The folder chip has a fixed largest width.
- **The right panel describes `App::focused()`.** Nothing under a tile reads the focused session: every view takes the key of the session it draws, and every handler captures it. The right panel is the one place that should follow the focus; it changes when `Tiles::placement` changes. The tile focus moves on a press on a tile's plain content or header, on F6, on a field taking the keyboard, and on a control used in a tile while no field holds the keyboard; a control that takes no keyboard focus, used in another tile while the user types, leaves the focus where the user types (`glue::focus_tile`, `glue::typing_in`, `view::tiles::focus_watch`, the give-back at the end of `glue::run`).
- **Dropping files onto a tile** can reuse the drop target of the row drag: `view::drag::target_in_tiles` finds the edge or the tile under a point from `Ui.row_rect`, and `view::tiles::outline` draws a tile as the target. How Rune reports files dropped from outside the window was not looked at in this plan.
- **The limits of the drag** (`crates/app/src/view/drag.rs`). It is built by hand on the pressed row's `on_drag`: past 4 points of movement it puts `Ui.drag` in state, a chip follows the pointer in the overlay layer, and the target is worked out on every move. Rune gives no move or release when the pressed row is gone (scrolled out, its group collapsed) or a modal opens mid-press, so a watcher under the whole window counts the moves the drag did not hear and ends it at the second. The count differs from the plan's letter: Rune runs the watcher's move before the drag's own, so every move counts as missed until the drag hears it, as the kit's own grip counts. While a row is dragged, Escape only cancels the drag, the window's keys and the sidebar rows' own keys do nothing, and a menu opening ends it (`tidy_sidebar` also clears a drag whenever a menu is open). Every drop target is in the accent: the line at an edge, the outline of the tile replaced (over amber), a ring on a group's header that takes no room. While the target is in the tile row the focused tile's accent outline is not drawn (`view::tiles::outline`), so the line beside it can be seen; amber stays at an edge target. Rune runs a row's press after the release of its own drag; `Ui.swallow_row_press`, set at every release past the slop and cleared by `glue::poll`, swallows it. A tile's header drags its tile through the same code (`view::drag::header`), with `RowDrag.from` set to `DragFrom::Header`, so the slop, the chip, the line, the watcher, Escape, the keys that wait and the menus that end it are the row's. Its target is only ever an edge of the tile row (`view::drag::move_target`); the sidebar is never asked, and a tile let go there does nothing. It ends when its tile closes, sets no press to swallow, and does not start with one tile in the row or while the header's name is edited. Rune gives a press and its drag to the topmost element with a handler for either, so the header and its title both carry the handler; the close button starts none. A press on the header leaves the keyboard where it is, so a drag let go over nothing or cancelled leaves it there. The sidebar does not scroll by itself near its edges, groups cannot be reordered, and the chip is not kept inside the window.
- **Keys inside a tile.** Rune's key paths match only an unbroken run of keys (`a/b` names `b` only when `a` is its nearest keyed ancestor), so `mr-tile-7/mr-send` names nothing in a Rune lookup. A lookup Rune makes inside a tile uses a key that carries the session's number (`mr-conversation-7`, `mr-model-7`, `mr-mode-7`) or climbs the real chain of keys (`items::toggle_path`). `testkit::tile(key, inner)` paths are for the test helpers only, which resolve them themselves. The plan's Global Constraint and its Rune fact on keys were wrong on this point.
- **Focus in the window.** `glue::run` ends by handing the tile focus back to the tile whose field holds the keyboard, in the same handler, so no other tile's outline flashes. A field that leaves the screen while it has the keyboard is rebuilt with the same text and caret (`glue::let_go`), losing its undo history: Rune would otherwise take the returning field for the old one.
- **Renaming.** `App::rename` names a session at once and sends `Rename` to its program when one runs; else the name is kept in `Saved.names` and told to the next program that starts. A title the program sends later is ignored until a restart; a listing that gives the user's name confirms it, and a complete listing drops the kept names of sessions it does not give. The window has one name field, `Ui.rename`, for whatever `Ui.renaming` says (a group, a row, a header), with `glue::start_rename`, `end_rename` and `finish_rename`. Starting another rename keeps what was typed in the first, groups included. A typed name is lost by Escape, or by closing its tile while its field still has the keyboard. When the place of a session's rename goes (its row or its tile), the name is kept if its field had already lost the keyboard and no sidebar menu is open, and dropped otherwise; a group's rename is always dropped when its group is no longer listed (`glue::tidy_sidebar`).

The stacked tiles plan changed the tiles after plan 4. What it built, which plan 5 meets as well; where a bullet above says otherwise, these hold:

- **The tiles are columns.** `Tiles::columns` gives the row's `Column`s left to right: each has a share of the row's width, its tile or the upper of two (`top`), and the lower one with the upper's part of the height (`under: Option<Under>`, `Under { key, split }`). A third tile in a column cannot be held. `Tiles::keys` gives every tile's session column by column, the upper before the lower. `len`, `find` (the column and the level, `mr_core::TileLevel`: `Whole`, `Top`, `Bottom`) and `mate` take the place of plan 4's `Tiles::list`, `index_of` and `moved_index`, which are gone with `Tile`. Widths, edges, room and `MIN_TILE_WIDTH` count columns. `Tiles::rects(size)` gives every tile a `Laid` (`key`, `column`, `level`, and `rect` as `[x, y, w, h]` from the row's top left), in the order of `keys`; every edge is put on the f32 grid of the row's length, so neighbours meet exactly and the last tile ends exactly at the row's edge. That holds for the numbers `Tiles::rects` gives. Rune draws to 1/64 of a point, so two drawn tiles can overlap by about 0.016 points: plan 5 must not rely on a drawn rectangle being equal to one from `rects`. The two of a column are drawn from their split kept so that each is at least `MIN_TILE_HEIGHT` high while the row fits two, and the same height in a lower row; `Tiles::split` gives the split as it was set, which can differ from what is drawn. A place a tile can go is a `Spot`: `Edge(e)`, `Above(key)`, `Below(key)`; `Place` gains `Above` and `Below`. `Tiles::takes` and `would_move` say whether a put or a move would do anything, and `App::can_put` and `App::can_move_tile` ask them with the row's size as the app knows it. `App::move_tile(key, spot)` and `glue::move_tile(ui, tx, key, spot, typing)` take a `Spot` where they took an edge. `MIN_TILE_HEIGHT` (280) and `Tiles::fits_two` say when a column may be split. Two statements of plan 4's first bullet no longer hold: `Tiles::placement` leaves out changes of the split as it leaves out widths, and the intents that change the row now include `App::resize_split`, `put_split` and `even_split` (`App::tile_split` reads a split).
- **Every tile is a direct child of `mr-tiles`, placed by its rectangle.** `view::tiles::row` reads the row's size with `cx.layout_size(ROW)` and gives each tile `.absolute(x, y).size(w, h)` from `Tiles::rects`. Only the pass before the row was ever laid out, which is never drawn, lays them out by the columns' shares. The reason is how Rune identifies an element: by the path of its ancestors' keys. A tile that moved under another parent (a column of its own, say) would be a new element, and Rune would drop what it kept for it: its scroll offset, the keyboard focus, a selection, a virtual list's measurements. Placed this way a tile never changes its parent, so stacking it, taking it out of a column or swapping it rebuilds nothing in it. The dividers (`mr-divider-{n}`, the row's full height) and the line between two tiles (`mr-split-{n}`, with the upper tile's number) are placed from the same rectangles. Whatever plan 5 adds to the row must keep every tile a direct child of `mr-tiles`. The limit, as the whole-branch review measured it: every frame in which the row's size changes uses both of Rune's re-run passes (`PASSES = 2` in Rune's `runtime/layout_read.rs`), with or without a column of two: the row's size, then the tiles' rectangles, then `view::width::current`'s read of its tile. When the passes run out Rune marks the reads as seen (`settle_layout_reads`), so a read chained behind a tile's rectangle is not a frame late: it stays stale until something else commits. So no layout read may be chained behind a tile's rectangle. The right panel of plan 5 takes room from the row, which is exactly this case. A frame in which the window is resized costs one view run more than in plan 4.
- **The row's height is known to the app.** `App` holds a `RowSize { width, height }`. `view::tiles::laid_out` reports both, through `App::set_row_width` and `App::set_row_height`, a frame after the layout (it is an `on_layout`); `App::row_height` and `App::row_size` read them. What is not known holds nothing back: while the height is not known, stacking is allowed. The view places the tiles from its own read, so a resized window, or a panel that takes room from the row, places them right in the frame it draws.
- **A file dropped on a tile can find its tile** with `Tiles::tile_at(size, at)`, the point measured from the row's top left: it gives the `Laid` of the tile under the point, or none outside the row. `view::drag::under` does this for both drags, from `Ui.row_rect`. `part_at(rect, at)` says which `Part` of a tile a point is in (`Left` and `Right` for the outer quarters, `Upper` and `Lower` for the halves of the middle). `glue::DropTarget` has `Above(key)` and `Below(key)`, and `mr-drop-half` (`view::drag::overlay`) outlines a half or a whole tile in the accent. A header's target is no longer only an edge of the tile row, as plan 4's bullet on the drag says: it is also the upper or lower half of another tile's middle, and over the middle of its column mate the spot that swaps the two (`view::drag::move_target`).
- **What is kept.** `Saved.tiles` holds one `SavedTile` per column; `below` (`SavedBelow { id, split }`) is its lower tile, written as `null` for a column of one. A `below` that cannot be read is none, a split missing or not a number is 0.5, and the rest of the state is still read. `SAVED_VERSION` stays 1.
- **The keys that are now taken.** Ctrl+Alt+PageUp and Ctrl+Alt+PageDown (Cmd+Alt on macOS) move the focused tile up and down (`view::tiles::stack_step`, `step_stack`). Ctrl+Shift+PageUp and PageDown now also take a tile out of a column of two (`step_move`). On `Screen::Main` all four are taken also when nothing moves, and they wait while a list or a menu is open or a drag runs (`view::drag_key`). Plan 5's Ctrl+Alt+B does not meet them. F6 and Shift+F6 go through `Tiles::keys`. A double press on the line between two tiles evens their heights. The window's keys still act while a divider or the line is held; when anything changes the tiles' placement, `glue::run` forgets what the held handle keeps for a cancel (`Ui.divider_press`, `Ui.split_press`, in `glue::tidy_handles`), so a cancel after it puts nothing back. The element keys new with this plan are `mr-split-{n}` and `mr-drop-half`. The sidebar row's menu reads "Move to group", "Open to the side", "Open below", "Rename".

### Every decision of plan 4, with what was built

1. **"Shown" becomes two things.** Built: `App.tiles`; in a tile means highlighted in the sidebar, history read, kept in the list; focused means seen, Escape stops its turn, a click on a row lands in it.
2. **"Finished" for a tile that is not focused.** Built: `Entry.unseen` is set when a turn ends out of the focused tile; the header and the row say "Finished" until the tile is focused.
3. **A click shows a session in the focused tile**, and a session already in a tile only focuses it; "New session" replaces the focused tile's session. Built as said (`Place::Focused`).
4. **Ctrl means "to the side".** Built: Ctrl+click on a row, "Open to the side" in its menu (disabled without room), Ctrl held while choosing a folder, Ctrl+Shift+N, and Ctrl on a custom group's "+". The arrow keys in the sidebar never open to the side.
5. **Room**: 420 points per tile when opening and dragging; a narrower window shrinks them all. Built in `Tiles` (`fits_another`, `resize`).
6. **Closing a tile** focuses its right neighbour, else its left; the session keeps running; an empty new session is forgotten. Built; when the closed tile was the focused one its neighbour's input box takes the keyboard, otherwise the keyboard stays.
7. **The accent outline shows only with two tiles or more**; amber always. Built (`view::tiles::outline`); a drop target's accent outline wins over amber for the length of a drag.
8. **What focuses a tile.** Narrowed while building: a control that takes no keyboard focus, used in another tile while you type, acts there and leaves the focus where you type. A selection in a reply focuses its tile and keeps the selection. A tile asked for on purpose (F6, a press on it) whose session has ended is focused with no field holding the keyboard, since its box cannot take text.
9. **Keys**: F6 and Shift+F6 go round the tiles' input boxes, Ctrl+W closes the focused tile, Escape stops its turn. Built in `view::on_key`; F6 and Ctrl+W do nothing while a menu or a picker's list is open, and during a drag Escape only cancels it and the other keys do nothing.
10. **Renaming** by a double click on a tile's title or "Rename" in the row's menu. Built; a resting session's name is kept and told at its program's next start.
11. **A name the user gave is not replaced in the same run.** Built in `Session` (`user_title`, `name_pending`).
12. **When the arrangement is saved**: opening to the side, closing, letting go of a divider, closing the window. Built; a first tile opened by a plain click asks for no save.
13. **Restoring** waits until every saved tile's session is known or the complete listing came; nothing with a launch folder. Built; a waiting restore is dropped once anything is put in a tile.
14. **Dragging a row** into the tiles (a line at an edge with room, a tile outlined without) or onto a group. Built in `view::drag`; a session already in a tile, dropped on the row, only focuses its tile. Escape cancels a drag, a menu opening ends it, every target is marked in the accent, and the focused tile's outline gives way while the target is in the tile row (all four decided while building).
15. **A double click on a divider evens the tiles.** Built (`App::even_tiles`).
16. **One conversation width for all tiles**; dragging its handle to the tile's edge means "no limit". Built; the width handle does not focus its tile.
17. **Tiles appear, go and resize at once.** Built (`Transition::none()`, `tx.instant()`).
18. **Arrow keys in the sidebar** swap the focused tile's session without taking the keyboard. Built as before; a ruling of the final review added that an arrow reaching the row of a session in another tile only selects that row: `view::sidebar::select` keeps it in `Ui.passed_row` with the tiles' placement, and the list draws it as its selected row (Rune's list moves the focus back to its selected row otherwise) until a row is pressed or the placement changes. No tile is focused and nothing is shown; Return, Space or a click on the row focus its tile.
19. **The row's menu** reads "Move to group", "Open to the side", "Rename". Built.
20. **A header on every tile.** Built (`view::tiles::header`).
21. **A long message of the user's is cut** after 20 lines or 4,000 characters, with "Show all". Built in `view::items` with the tool output's own cut and button; one line left out reads "Show all (1 more line)", for both (a ruling of the final review).
22. **A tile is moved by its header** (Task 15, asked for by Jerod after the review). Built: the header drags its tile, the line shows at the edge it would go to and not at its own two edges, and the drop puts it there at once with its width, focused, the keyboard in its box; a full row still takes a move. Ctrl+Shift+PageUp and Ctrl+Shift+PageDown (Cmd+Shift on macOS) move the focused tile one place, also held, without wrapping and without moving the keyboard; they wait as F6 waits. The tiles are keyed, so a move rebuilds nothing in them: a draft, a name being edited and a waiting request stay with their session. A sidebar row whose session is in a tile still only focuses it (decision 14 stands).

## Left by plan 4

Found by the per-task reviews of plan 4 and by its final review, and not fixed. None blocks anything. What a person would see is under "Things to look at" above.

The core (`crates/core`):

- The doc of `Tiles::widths` says a row "0 or not finite" gives no widths; a negative width gives none either (`tiles.rs`).
- `Session::rename` compares the new name with the title, so a name the program refused cannot be tried again in the same run without picking another first (`session.rs`).
- Two entries for one id in a hand-edited `Saved.names` apply the first, not the latest (`app.rs`).
- The doc on `App::restart` says the intents below it first focus the session's tile; `rename`, below it, does not (`app.rs`).
- `saved.rs` line 2 is a doc line of 119 characters.

Tiles and focus (`crates/app/src/view/tiles.rs`, `glue.rs`, `input.rs`):

- `glue::let_go` loses the undo history, and an input method's composition in progress, of a box that leaves the screen while it has the keyboard (Ctrl+W of the tile typed in) (`glue.rs`).
- `input::give_back` and `edit_queued` set a box's keyboard flag without focusing its tile; in a tile that is not the focused one the tile focus follows a frame later through `focus_watch` (`input.rs`).
- A tile is announced as a button to assistive tools, although the element is given the role of a group (`tiles.rs`): it needs a look at how Rune derives the accessible role from a press handler. `target/snap/t14/tiles-dark.json` shows it (seen by the review of Task 8, and Minor 6 of the final review).
- The outline's corners are square under the card's rounded corner (`tiles.rs`).
- A doc line of 125 columns at `tiles.rs` line 246, and one with an odd line break at `view/mod.rs` line 74.

Renaming (`crates/app/src/view/sidebar.rs`, `glue.rs`):

- `sidebar::renaming_row` copies the kit row's look, because the kit's own rename field forces its key (`sidebar.rs`).
- `glue.rs` is 1,439 lines; the rename functions could be a module of their own.

The drag (`crates/app/src/view/drag.rs`):

- A drag whose row went away and whose button was let go stays in state until two pointer moves, and holds the window's keys meanwhile.
- Escape typed into an input box reached by Tab during a drag stops that tile's turn and leaves the drag, since the box takes Escape first (`input.rs`).
- An assistive tool's activation of a row queued in the same frame as the release of a drag past 4 points is swallowed (stated in the doc of `Ui.swallow_row_press`).
- The comment on `Phase::Down` in `drag::row` is wrong for a press queued in the same frame as the release.
- The drop line is placed without a spring of its own (`Transition::none()` only), so when the target moves from one edge to another it glides there over about 25 frames (measured in the final fix wave) rather than jumping; decision 17 says tiles appear at once and does not speak of the line (asked of Jerod under "Things to look at").
- After an arrow key passed the row of a session in another tile, that row stays marked as the list's selected row when the keyboard then leaves the list (a click into a tile's box, Tab, the sidebar hidden and shown), until some tile changes; the focused tile's own row then shows only the fill of an open tile. `Ui.passed_row` would be cleared when the list loses the keyboard focus (`sidebar.rs`, `glue.rs`). No test covers it. Found by the re-review of the final fix wave.
- `drag.rs` is about 1,410 lines, mostly tests.

The conversation (`crates/app/src/view/items.rs`):

- A message cut inside a line ends with no ellipsis, as tool output.

Tests:

- The fourth clause of `closing_moves_the_focus_right_then_left` starts from one tile, not a fresh three (`crates/core/src/tiles.rs`).
- "Opening one tile's picker shuts the other's" is tested by a direct call only, and an `input.rs` test has a duplicated `let picker`.
- The outline test cannot see the drawn colour through Rune's test interface.
- No test for Ctrl on a custom group's "+", for Ctrl+Shift+N with no folder on offer, or for a click outside a side menu (`folder.rs`, `sidebar.rs`).
- `a_row_rename_ends_when_the_row_goes` hides the group through the core, not its menu (which keeps the name); and no test presses Escape in a row's name field during a running turn (`sidebar.rs`).
- `dragging_rebuilds_no_conversation_row` moves over edges only, never over a tile as target (`drag.rs`).
- `an_answer_that_comes_after_the_release_changes_nothing` cannot fail, since the guard is structural; two assertions that the press was not swallowed (`drag.rs` lines 495 and 948) pass either way now that `glue::poll` clears the flag.
- The drag tests copy `titled`, `group_named`, `group_of` and the scene's title constants.
- `two_saved_tiles_come_back_in_the_window` passes with the `tx.instant()` of `glue::poll` taken out: from an empty row nothing moves from an old place, and tiles have no transition of their own, so it pins what is seen rather than that call (`tiles.rs`).
- The `tiles-drag` scene sets the tile row's rectangle for a window of 1,800 by 900 itself, since a snapshot draws one frame and the line is placed from the row's last layout (`demo.rs`).

Left by Task 15 (moving a tile), which came after the final review and had its own review:

- The tile being dragged shows nothing itself; only the title by the pointer says which one it is.
- `RowDrag` holds a row's drag or a header's (`from: DragFrom`) and kept its name, which the tests of Tasks 11 and 12 use.
- For whoever writes a window test that scrolls: `rt.settle_all()` after wheeling a conversation up lands it at its bottom again and sets `following`, because Rune's settle lands follow-the-end. Step frames instead (`a_move_keeps_the_scroll_position` in `view/drag.rs`). The window's own host calls `settle_all` only for a window that fits its content, which this one is not.
- One comment line in `view/tiles.rs` (`focus_watch`'s doc) is over 100 columns, from before Task 15.

### What the final review set aside

The whole-branch review of plan 4 declined to judge these, each because the plan or an earlier decision already says how it is. Each stands as built, and each is a small change if Jerod wants it otherwise; the fifth was changed in the same wave.

- Tiles restored into a narrower window fall below 420 points: decision 5 covers a window made narrower, and this is the same rule (also under "Before looking").
- On macOS, Cmd+W with no tile closes the window: that is Rune's native default; Linux has none.
- Ctrl+Shift+N, or a folder chosen with Ctrl held, does nothing when the focused session is an empty one in the same folder: the plan's rule for `new_session`.
- A drop on a row of a hidden group that is listed because it holds a tile moves the session into a hidden group: the plan says hidden groups take no drop, and is silent on one that is listed.
- "Show all (1 more lines)" for a message of 21 lines was the plan's quoted wording, from plan 2's tool output. A ruling of the controller changed it to "Show all (1 more line)" in the final fix wave.
- A name given to a resting session masks a later rename made in another app until that session next runs here, when this app's name replaces it: decisions 10 and 11.
- Focus moves and swaps are saved only with the next saving event: decision 12 (asked of Jerod under "Things to look at").
- A press and release in the same frame (a touchpad tap) on a control that takes the keyboard in another tile may leave the outline on the tile typed in: it depends on when Rune writes the focus flags, and could not be reproduced without the window.
- A tile's title can be renamed by a double click only; the keyboard reaches renaming through the row's menu: the plan's design.
- The sidebar does not scroll during a drag: said in the plan.
- "Finished" shows for a session whose program ended badly in a tile that is not focused: plan 3's mark.
- Whether the real `claude` confirms `Rename`, and how long the complete listing takes on the real store: only the live test can say. The second bore on the review's Important finding, which is fixed (a new session nothing was sent in is no longer saved in a tile).
- An older complete listing arriving after a newer recent one (from "Left by plan 3"): unchanged by this branch.

## Jerod's look at plan 3

Its seven things to try were not reported on until 2026-11-15, when Jerod said all seven work; they are recorded here.

Plan 3 ("many sessions") is on the branch `plan-3-many-sessions`. Jerod runs, in their own terminal:

```bash
MINDRUNE_LIVE=1 cargo test -p mindrune --test live -- --ignored --nocapture
```

```bash
cargo run --release -p mindrune
```

With no folder the window shows again the session that was on screen when it last closed, without starting its program, or says how to start one. `cargo run --release -p mindrune -- FOLDER` starts a new session in FOLDER instead.

### Before looking

What this window will not do yet, so it is judged for what it is:

- One conversation is on screen at a time. Clicking a row swaps it. Side by side is plan 4.
- A session cannot be renamed. Its title is the generated one.
- Closing the window ends every session's program. Running in the background is plan 5.
- Rows cannot be dragged into the middle, and groups cannot be reordered. A session moves through "Move to group" in its row's menu.

Also worth knowing beforehand:

- "Hide group" on the group of the session on screen seems to do nothing at first: a group stays listed while its session is shown, so it hides when another session is shown. The same holds for a group whose session shows a mark.
- A folder that is gone starts nothing, whether it was chosen from the "New session" menu or the picker or is a folder group's own (its "+" and its menu's "New session"); a line near the button says "That folder no longer exists: {path}". A message to a stored session whose folder is gone stays in the input box, and a line under the box says "This session's folder no longer exists: {path}".
- A stored message that had images shows "1 image" or "N images" under its text. The images themselves are not shown.
- The plan lists fourteen decisions of its own that the design does not make, or makes differently (which sessions are listed, their order, what launch does, the look at launch, how much history is read, and so on). Jerod has not reviewed them yet; they are in the plan under "Decisions in this plan that the design does not make".
- **Decided by Jerod on 2026-11-14: a stored session goes on with the model it was using.** The code stays as built. Two known costs remain with it: the model read is the one that last answered, which a plan-mode turn can make differ from the one selected, and a session whose model no longer exists fails on its first message until another is picked. What was put to Jerod: Today it starts with the model that last answered in it, read from its history, as the plan's decision 6 says. The whole-branch review found two costs the plan did not weigh: that model is not always the one that was selected (a plan-mode turn can be answered by another), and an old session may name a model that no longer exists, so its first message fails. The other way is to start it with no model named unless Jerod picks one in this run, as `claude --resume` in a terminal does; since the model decides what a message costs, the choice is Jerod's.

### The live test

It needs Claude Code installed and signed in. It runs the app twice, as the window would be opened, closed and opened again, and starts the real `claude` three times in all: the test's session, the look the first run takes at launch, and the same session resumed in the second run. The account is checked once.

What it prints, in order. None of these lines fails the test:

- `live: replied "ok" with <model label>`, then the title, the model label and the Auto mode lines plan 2's test printed. These are about the test's own session only.
- `live: listed N recent sessions in X ms, M in all in Y ms`: how long the two listings of the store took, from asking to the answer.
- `live: the look gave N models; usage known: yes|no (Z ms)`.
- `live: read the largest of the 5 most recent histories (K items) in Z ms`. The listing gives no file sizes, so "largest" is the one with the most items among the five most recent sessions, each shown as a click on its row would show it. Lines follow if some could not be read.
- `live: read the session back from the store (K items)`.
- `live: the session changed N s before; the app did not ask`. The second run starts from what the first saved, and that says this app closed the session; a change no later than ten seconds after that close is the app's own, so the app does not ask whether another app has it open. Should the program have written later than that, the line ends `the app asked first, continued here` instead, and the test answers "Continue here". Neither fails the test.
- `live: resumed with <model label> in <mode label>`.

What fails it: no reply, a reply that does not say the word asked for, no usage from the session, not getting back to "Ask before edits" after trying Auto, the first message or its reply missing from the history read back, a second message that does not resume the same session, and no reply to it. Every wait has a deadline and says what it waited for.

Of the other stored sessions it prints only counts and times, never a title, a folder, a message or an id. It writes nothing itself. What stays behind is what `claude` stores for the test's own session: one session in a temporary folder that is deleted when the test ends. It will appear in the window's sidebar under a group named after that folder (`.tmp` and some letters). The look runs in the test's temporary folder, which the test gives the app as its home folder: the first run restores no session, so the app asks for the look there. Should the app ask for it in another folder, the test says so and runs it in its own folder anyway.

### What to try in the window

1. The sidebar fills with groups, and how long that takes. Seen: works (Jerod, 2026-11-15).
2. An old session opens at once and shows its end. Seen: works (Jerod, 2026-11-15).
3. A message to it gets a reply, and the model shown is the one that answers. Seen: works (Jerod, 2026-11-15).
4. Two sessions work at the same time, and their marks follow. Seen: works (Jerod, 2026-11-15).
5. "New session" offers folders, and "Choose a folder…" opens the desktop's folder picker. Seen: works (Jerod, 2026-11-15).
6. A session is moved to a new group, the group renamed, the app closed and opened again, and everything is where it was. Seen: works (Jerod, 2026-11-15).
7. A session that is open in Claude Desktop asks before it is sent to. Seen: works (Jerod, 2026-11-15).

A defect in something plan 3 built is fixed in plan 3.

### What Jerod reported on 2026-11-14

Jerod ran the live test and then the window, and sent one picture: the sidebar full of rows titled "Condense the tool payload below t…", and a conversation whose first message is a long block of text with escaped line breaks and whose only reply is "Failed to authenticate: OAuth session expired and could not be refreshed". Jerod had typed none of it. The terminal had been cleared, so what the live test printed was not seen.

What was found, each from the store or the code and not from a guess:

- **The live test worked as far as the store shows.** It left one session, in a temporary folder, on 2026-11-14 at 08:43. Both messages are in it, each with its reply ("ok", "again") from the model the account defaults to. Whether the test's own checks all passed was not seen.
- **The app sent nothing.** The session in the picture is a stored one from 2026-10-20, and its file has not changed since that day. The long first message and the sign-in error are its own content, drawn as stored. The saved state names it as the session on screen; a session comes on screen only by a click or Return on its row, or by being the one shown when the app last closed (`App::show`, `App::restore_shown`).
- **Where the rows come from.** The store holds 722 sessions in the folder `~/.claude-mem/observer-sessions`, dated 2026-10-18 to 2026-10-27. A plugin made them, not a person: each is one request to shorten a tool's output. Every one of the 722 ends in an error from the service and none has a real reply. Their records carry `"entrypoint": "sdk-ts"`; nothing else in the store does. The other values seen: `cli` (the terminal program, 30 sessions), `claude-desktop` (21), `sdk-cli` (this app, its tests and its probes, 19).
- **Why they fill the sidebar.** All 722 are inside the 30 days of decision 1, so all are listed: 722 of the 792 rows. Their group is the last of 20 by the order of decision 2, so the top of the list is right; the list below it is not usable.

Defects in what plan 3 built:

1. Sessions made by a program through the SDK are listed like a person's. The plan did not foresee them. **Fixed on 2026-11-14 (`d032b8e`), as Jerod chose: they are left out.** The listing leaves out a session whose first `entrypoint` is `sdk-ts` or `sdk-py`. There is no setting to show them. Against the real store the listing went from 792 recent sessions to 70. If the saved state names such a session as the one on screen, the app starts with nothing shown. `sdk-py` is a guess: only `sdk-ts` was seen. A program that drives `claude -p` writes `sdk-cli`, as this app does, and stays listed. Three limits the review of the fix named, none changed: the rule is "started through the TypeScript or Python SDK", so a person's sessions from a tool built on that SDK would be hidden as well; a session a program started and a person went on with stays hidden; and once the complete listing has come in, the saved state drops any group placement of a hidden session, which matters only if the rule is loosened later.
2. Each run of the live test leaves a group named after a temporary folder that no longer exists (five so far), and the probes left `target/probe-cwd` (9 sessions). The app must never write inside `~/.claude`, so the test cannot remove them. "Hide group" removes each by hand.
3. A very long message from the user is drawn in full (the one in the picture is about 26,000 characters). Not from plan 3; noted for whoever next works on the conversation.

Also found: a probe from plan 1 was still running in a terminal tab and holding a `claude` open with Remote Control failing over and over. It was stopped on 2026-11-14.

None of the seven things above was reported on, so each still says "not yet recorded".

Later on 2026-11-14 Jerod decided the model question (see "Before looking") and said to merge plan 3 into `main`. The seven things were still not reported on when it was merged; they stay open, and a defect found in them later is fixed on `main` or in the next plan.

## What plan 4 must know

Plan 4 puts several sessions on screen side by side. What plan 3 built for it, and what it did not:

- **The core holds many sessions.** `App.sessions` is a store of entries keyed by `SessionKey`, the app's own number for a session, which lasts one run. Each entry has its `Session`, the number of its current open (`None` while no program runs), its activity times, and whether it was touched in this run. `App.shown` is the one session on screen. Every session intent of `App` takes the key it acts on.
- **"Shown" has rules attached.** Showing a session marks it seen, offers it the known models, and asks for its history when it was never read. A session that stops being shown and has nothing stored (no message handed over, none queued) is closed and forgotten, unless something is typed in its input box: `App::show`, `App::new_session` and `App::new_session_in` take a `Leaving`, and the glue passes `Leaving::Keep` while the draft of the session left has anything but white space (`glue::leaving`). A kept session is forgotten later, when it is left with nothing typed. The sidebar's model is rebuilt in `App::settle` when the shown session changed. With tiles, `shown` becomes the set of sessions in tiles plus the focused one; each of these rules has to say which it means.
- **The question is per session.** Each entry holds its own question, `Entry::prompt` (today only "may be open in another app", with the message it holds), and whether its check in the store runs, `Entry::checking`, so several sessions can be checked or asked at once. `App::prompt(key)` reads the question and `App::answer_prompt(key, choice)` answers it. Only the session asked about stops taking messages, and a session with a question waiting is marked `NeedsYou`, so its row, a collapsed header and a hidden group show it. The card is drawn in its own session's pane, so two tiles can each show their own.
- **Opens are numbered per session.** Everything the backend sends about an open carries its number, and a number that is not the session's current open is dropped. The backend runs one thread per open, one library thread for the account, the listings, the histories and the activity checks, one at a time in order, and one thread per look.
- **One session's window state is one `SessionView`.** `Ui.views` maps each key to a `SessionView` (in `crates/app/src/glue.rs`): the draft, the open picker, the rows the user opened or closed, the outputs shown whole, the question drafts, whether the list follows the end, the first row on screen, and the generation they were made for. `glue::view` and `glue::view_mut` find it; `glue::run` ends by dropping the views of sessions that are gone and by calling `leave` on the view of a session that stopped being shown, which shuts its picker and gives up its focus. With two tiles on screen, "left" means "no longer in any tile".
- **The pane function.** `view::pane(ui, cx, kit, key, layer)` in `crates/app/src/view/mod.rs` draws one session's conversation area: the conversation, the "may be open in another app" card while a question waits about this session, and the input box, in the column whose width the user drags. Everything under it takes the key as an argument, and every handler captures the key when it is built. `view::view` is the one place the conversation area reads `ui.app.shown`. Other code reads it too: in `crates/app/src/view/mod.rs`, `on_key` (Escape stops the shown session's turn); in `crates/app/src/view/folder.rs`, `pick` (the picker starts in the shown session's folder), `start` (whether a new session was shown, so its box takes the focus) and `floating_error` (whether a session is on screen); in the glue, `show`, `leaving`, `tidy_views`, `Ui::shown_session` and the test-only `Ui::shown_entry_mut`; and the demo and the test kit. Each has to say what it means with tiles.
- **Keys already per session:** the conversation's scroll `mr-conversation-{key}`, the memo ids of rows `mr-item-{id}-g{generation}-{key}`, the sidebar's rows `mr-row-{key}`, and the group headers `mr-group-{group key}`. Rune resolves a plain key window-wide and takes the first match, so these still collide with two panes:
  - `mr-tile`, which `width::current` and `width::column` read the layout of;
  - the pickers, `Select::new("mr-model" | "mr-mode", ..)`, which anchor to `mr-model-trigger` and `mr-mode-trigger`, and the layer keys the kit derives from them (`-scrim`, `-content`, `-menu`): two tiles would both anchor to the first tile's trigger;
  - `tx.keep_on_screen("mr-toggle-{id}")` in `items::line`: item ids restart per session;
  - the card's buttons `mr-elsewhere-here`, `mr-elsewhere-copy`, `mr-elsewhere-cancel`, which carry no session number;
  - plain keys tests look up and focus: `mr-input`, `mr-send`, `mr-stop`, `mr-jump-latest`, `mr-empty-hint`, the request buttons (`mr-allow-r1` and the like), `mr-item-{id}`. `tx.focus(key)` takes an exact key, not a path, so each tile's input needs a key of its own (`mr-input-{key}`), or its `TextEdit::focused` set in state.
- **Focus between tiles.** Rune has no per-element focus callback. Rune writes the real focus into each draft's `TextEdit::focused` on every commit, so the focused tile can be read from `view.draft.focused`; `El::focus_within_ring` draws a ring while the focus is anywhere inside an element; `El::on_key` on a tile's root hears every key while the focus is inside it. Keep the focused tile in state and draw the mark from there.
- **The drag that is still missing.** Rune has no dragging between elements inside a window: no drop targets, no drag preview. It has to be built by hand from `El::on_drag` (the pointer is captured by the pressed element until release; check `cancelled()` on release), the targets' rectangles reported by `El::on_layout` or `drawn_rect_into`, and `Tx::element_at` to pick what is under the pointer. A held press keeps hover on the pressed element, so hover cannot light a drop target. After a release inside the pressed element its `on_press` runs too, so a drag let go over its own row also selects it. The kit's `Reorder` moves a row only within one flat list, and its `TabStrip` reorders tabs. Plan 3's decision 11 waits for plan 4's drag to move rows onto groups.
- **Dividers.** Rune has no splitter for more than two panes: the kit's `SplitView` has a sidebar, the main element and an inspector. Tiles side by side are a row of columns with divider elements between them, each with `on_drag`, as `crates/app/src/view/width.rs` drags the conversation's edge.
- **What is kept between runs** is one `Saved` record under the settings key `state`, version 1: the groups, where sessions were placed, the recent folders, the shown session by its id, the sidebar's width and whether it is collapsed, and `closed_by_app`, the sessions whose program the last run ran with the time it closed (only those closed within the last 130 seconds are kept). Fields have serde defaults, so a field added for tiles reads as its default from an older file. **Plan 4 must not raise `SAVED_VERSION` for added fields:** the defaults cover them, and a build reads only its own version. One malformed field still loses the whole record for that run, but a value under `state` that a build cannot read, or of another version, is first copied as it was to `state.unread`, so a newer build's state survives a run of an older one.
- **Do not assign `App.sessions` or `App.groups` wholesale.** Each counts its own changes, and `App::settle` compares those counts with the ones it last built from. A new store put in its place can start at a count `settle` already holds and hide a change. Plan 3 builds both once, in `App::new`.
- **Renaming a session** (plan 4) writes a `custom-title` record, which wins over every generated title. The listing reads titles only from the first and the last 256 KiB of a file, so in a large file a name given in the middle is not seen (see `docs/protocol-notes.md`).

## The whole-branch review of plan 3

After the fourteen tasks a final reviewer read the whole branch for problems that live between tasks. It found nothing critical: nothing loses stored data, crashes the window or leaves a program running. It found six problems a person would meet in ordinary use and ten small ones. One fix wave followed.

### What was fixed

- **A stored session no longer wakes in "Bypass permissions" because its file says so.** A history whose last mode is `bypassPermissions`, or a mode this app does not know, leaves the session in "Ask before edits". Plan, Accept edits and Auto are still taken from the history. Before, the program could be started with every permission check off while the only sign was the picker's label, although the input box never offers Bypass.
- **A new session with text typed in it is kept when the user looks at another session.** Before, looking up something in another session while writing a first message threw the new session and its text away. Now it stays, row and text, while its input box holds anything but white space; one left with nothing typed is still forgotten. This holds for a click or an arrow key on a row, and for a new session from the folder menu, the picker or Ctrl+N.
- **The "may be open in another app" question is per session.** Before, while one session's question waited, no other stored session could be sent to, and nothing on screen said why; the question could even wait in a session that was not on screen. Now each session holds its own question, several can be checked or asked at once, only the session asked about waits, and its row is marked like a session that needs the user.
- **The look at launch runs in the restored session's folder or at home.** Before, with nothing to restore it ran in the folder of whichever session was written last, which may belong to an automated run, and in a folder that no longer existed it failed silently and left the usage unknown. Now it never runs in the newest session's folder, and a folder that is gone falls back to the home folder, for the look after a usage reset too.
- **No question after this app's own last run.** Before, closing the window and opening it again within two minutes made the first message to the session just used ask "This session may be open in another app", although only this app had written to it. Now closing keeps, for each session whose program the run ran, its id and the closing time, and a change in the store no later than ten seconds after that time counts as the app's own.
- **A folder group's "+" and its menu's "New session" check the folder.** A folder that is gone, such as a removed worktree, starts nothing and says so, as a folder chosen from the menu does, and no longer goes to the front of the recent folders.
- **A message to a stored session whose folder is gone stays in the box.** Before, the program failed to start, the session ended, and the message sat in "Queued" behind a box that could not be used. Now nothing starts, the draft stays, and a line under the box says the folder no longer exists. The folder is checked when the user sends, not while the box is drawn.
- **A store that cannot be read is not an empty store.** Before, a projects folder or a project's folder that could not be read for a moment (permissions, a mount) gave "no sessions", and the next save dropped every saved placement. Now the listing fails and the app keeps what it knows. A projects folder that does not exist yet still means no sessions.
- **A saved state of another version is kept aside.** Before, running this build after a newer one overwrote the newer build's groups and placements at the first save. Now a value under `state` this build cannot read is copied as it was to `state.unread` first.

### What was left, and why

- **The model a stored session starts with** (the review's Important 2) is the plan's decision 6, which Jerod approved by name. Changing it changes what a message costs, so it is put to Jerod at the look (see "Before looking") and the code is unchanged.
- **Work that grows with the store** (Minor 9): `App::event` finds its session by walking every entry for each streamed piece; every program start and every look copies the model list into all of about 790 entries; and the window redraws whenever a hidden session streams, though its rows are memoised. Each is small at today's sizes and has a plain fix (a map from open to key; offer models only to live and shown sessions; redraw only when the shown session or the sidebar changed).
- **A session left "Queued:" for ever** (Minor 7): a program that exits cleanly before it started, or while a message waited behind a turn, leaves the session resting with the message still queued, and nothing sends it until the user sends something else. The fix is to hand it back to the draft, as Cancel does, or to wake the session with it.
- **Claude Code going missing mid-run** (Minor 10) removes the session it was asked to start, until the next listing, and drops its queued message. Ending it as any other failed start does would be kinder.
- **An unreadable session file** (Minor 8). The plan's header says it covers the design's case of a stored session file that cannot be read, which section 10 describes as listed with its id as the title and an error when opened. What is true: a file is listed only when it names its folder in its first or last 256 KiB; a file that does not is not listed at all, since it has no group to sit in. A file that names its folder there but cannot be read further on is listed, and opening it says why it could not be read.
- **Two things the check of the fixes itself found, both small and left as they are.** First: when the app closes it notes the closing time for every session it ran in that run, also for one whose program had ended long before. If another app wrote to such a session in between, and the app is closed and opened again within two minutes, the first message to it is not asked about. Noting the program's own last report for a session that was not running at the close would settle it. Second: one project folder in the store that stays unreadable now makes every listing fail, so no stored session is listed, where before only that folder was missing. Skipping it while counting the listing as not complete would list the rest and still keep the saved placements.

### What plan 5 must do first

Since the cut of 2026-11-15 this is for plan 7 (running in the background), the plan that stops idle programs; the plan 5 of that day does not.

Plan 5 stops idle programs quietly. Two things the review found are rare today and become ordinary then:

- **A send that fails after its program's exit leaves a message that looks sent** (Minor 6). `Exited` lets go of the open, so a `SendFailed` that comes after it reaches no session; after a clean exit the session rests with the user's message in the conversation and no reply, no spinner and no notice, and the message still counts as handed over. Keep the last open's number on the entry so the failure is found, take the message back out of the conversation (or mark it not delivered), and wake the session with it again. A title that arrives after the exit is lost the same way.
- **Histories are never let go** (Minor 5). A history read for a session (up to 2,000 items with their whole tool outputs) stays for the run, and holding an arrow key through forty stored sessions queues forty reads on the library thread and keeps forty conversations in memory. Drop the history of a resting session that is neither shown nor run (back to "not loaded"), and skip a queued read whose session is no longer shown.

## Left by plan 3

Found by the per-task reviews of plan 3 and not fixed. None blocks anything.

The adapter (`crates/claude`):

- `list_entries` computes a file's modification time in its own line, with another fallback than `modified_unix`.
- The history reader has no test for a walk that ends at a root inside a region cut from a larger file, for a cut region with no message, for a compaction whose earlier record lies outside the region, or for links that run in a circle.
- The test that moved with the side-chain rule no longer proves that a side-chain record on the chain is skipped.
- When the region read from the end of a file starts exactly at the start of a line, that whole line is dropped. The listing's `describe` reads one byte before its region to avoid this; the history reader does not.
- A record with the same top-level link key twice is skipped as broken.
- The listing sees titles only in the first and last 256 KiB of a file, so a name the user gave in the middle of a large file loses to a later generated one.

The core (`crates/core`):

- When a program's clean exit is taken in before a late failed send of the same open, the session rests although it counts as having nothing stored; it then starts afresh, which is right, but the two facts disagree.
- A thought read from the store still carries a start time of 0 that nothing uses.
- Two "closed" send failures of one open can arrive out of order. Nothing depends on their order.
- Ended opens and finished looks stay as empty entries in the backend's maps until the next open, send, look or flush.
- A test comment in `a_look_reports_and_closes` says the fake answers the usage request; the test itself does.
- The look's two time limits have no test.
- The backend's old `next` test helper spells out the full `std::time` names.
- `answer_prompt` drops the held message, even for Cancel, when its session is no longer resting. No path reaches this today.
- A new recent listing is sent even while one is still out, so an older complete listing that arrives last can remove entries a newer recent listing added, until the next listing.
- The 30 second tick asks for the usage after a reset without checking that the agent reports usage at all, as the start of a session does.
- No test covers "Continue as a copy" before the original's history was read, an activity answer for a session that is gone or not opening, or a listing that names one id twice.
- A copy has no id until its program reports one, so restarting a copy whose fork failed starts afresh.
- Every event clones the session's id, though only the start needs it.
- `Sessions` compares its change count too when two stores are compared.

What is kept between runs:

- Folder groups whose folder was deleted stay in the saved record. The "New session" menu lists only folders that still exist.
- Duplicates in a hand-edited list of recent folders survive the start.
- A new session placed in a custom group is saved by its id only at the next save, so a crash before then loses the placement.
- "Continue as a copy" does not put its folder among the recent ones.
- Reading the saved state checks its version twice.
- One malformed field loses the whole saved state for that run; the value is kept under `state.unread` until the next value that cannot be read replaces it.

The conversation area:

- A session's place comes back only to within a row, so a row taller than the window may come back elsewhere.
- Handlers that run late can recreate the view of a session that is gone, until the next run of the glue drops it.
- "This conversation could not be read." goes away once a session whose history failed has new items, and nothing then says that the earlier history is missing.
- Cancel on the "may be open in another app" card always adds a line break before the message it puts back, which leaves a blank line when the draft already ends with one.

The sidebar and its menus:

- Group headers, "Show older" and "Older groups" cannot be reached with the keyboard; the kit's own header is the same.
- When a group is created and a session moved into it, the view filters the save out of the core's effects; one core intent for both would be cleaner.
- The "Hidden groups" menu opens at the window's top-left corner when its line has not been laid out yet.
- After a menu opened with the pointer closes, the focus goes back to where it was (the input box), not to the row. This is Rune's behaviour for menus.
- Escape with the focus in the "Move to group" submenu is untested; it should close the whole menu.

Folders for new sessions:

- The menu opened from the empty area's button appears under the sidebar's button; with the sidebar collapsed and a session shown it opens at the window's corner.
- Ctrl+N while the folder picker is open can open the menu, and both answers then start a session.
- Tidying the sidebar shuts the folder menu without clearing the folders it offered; they are not read while it is shut.
- With the sidebar shown and no session shown, the same folder error is drawn twice, under both buttons.
- The floating error line (sidebar hidden, a session shown) lies over the top left of the conversation until the next try, and no screenshot shows it.

Tests:

- Two fixes made during plan 3 (a stored thought claims no time and a stored message keeps its image count; a new session replaces an empty one whose program ended) had their tests written after the code, so those tests were never seen failing.
- `a_new_session_forgets_the_old_ones_open_picker` asserts nothing that can fail now; it should check the picker of the view that was left.
- `history_that_replaces_a_conversation_forgets_opened_rows` sets the history to loading by hand.
- `cancel_puts_the_message_back` cannot catch a lost focus, because the box already has it.
- `a_very_long_history_scrolls_smoothly` does not set "earlier left out", so the line "Earlier messages are not shown." is not checked there.
- The demo's model list copies the fake agent's private one.
- One test empties the known models and a session's models by hand, a state no backend message produces.
- The close warning on standard error still says "the session", though every session's program is waited for.

## Rulings made while plan 3 ran

Calls made by the controller during the plan, where the plan or the brief did not settle a question. Each says what it costs if it was wrong.

- Work went on the branch in the main checkout, not in a separate worktree, as in plan 2: a worktree would rebuild Rune from nothing. If wrong: the main checkout sat on the plan's branch while work ran; `git switch main` undoes it.
- Every subagent (implementer, reviewer, fixer) ran on Opus 5.5 at high effort, because Jerod set that floor. The whole-branch review ran on the most capable model because the review process asks for it there. If wrong: the final review cost more than needed; it is one review.
- `Launch` derives `Default`, and callers build it with `..Default::default()`, because two tasks add fields to it. If wrong: one derive.
- The test that closing ends every program counts the `Close` effects, not the length of the list, because closing also asks to save. If wrong: nothing.
- The look at launch runs in the folder of the session shown again from the last run, else of the newest session, else the home folder. If wrong: one launch's look runs the user's session-start hooks in another folder than the plan's decision 5 says. *Replaced after the whole-branch review: the look runs in the restored session's folder, else in the home folder, never in the newest session's, and a folder that is not there falls back to the home folder; the look after a usage reset follows the same rule. The newest session is whichever program last wrote to the store, an automated run's for example, so its folder decided where the user's session-start hooks ran; that contradicts decision 5, which is the sentence Jerod approves.*
- A look runs on a thread of its own, not on the library thread, so a look that waits up to 30 seconds for the real program never holds up a stored session's history. If wrong: one short-lived thread more than planned.
- `App` has a public `sidebar_version`, which goes up each time `settle` rebuilds the sidebar's model; the sidebar's list uses it as its version, and the model's types compare and print. If wrong: one public field.
- The window tests' helper that adds a session hands one message over, so the app keeps that session when another is shown. If wrong: the helper is shaped differently; the tests that use it still cross the same item ids.
- On closing, the saved state is written before the bounded wait for the programs, not after, so the groups are safe even when the desktop cuts the wait short. If wrong: nothing visible; nothing learned during the wait is saved.
- When the part of a file read from its end does not start at the file's start, and the chain of replies ends at a record with no parent, the history says that earlier messages are not shown: the reader cannot know what the cut bytes held. If wrong: a file over 16 MiB whose whole conversation sits in its end shows that line once too often.
- A cut region that holds no message is an empty history that says earlier messages are not shown, for the same reason. If wrong: such a file shows only that line; one line to change.
- Choosing a row with the arrow keys leaves the focus in the sidebar's list; a click on a row, Return or Space on a focused row, or a new session focuses the input box. Return is a choice, not a move. If wrong: a keyboard user presses Tab once more or once less.
- A thought read from the store reads "Thought", with no time: the store keeps no duration. If wrong: the wording of old thinking lines.
- The core keeps a stored message's image count on the user's item, and the window draws it, so a message that was only an image is not an empty bubble. If wrong: one field nobody draws.
- A session's program is started with the model option the user chose only while that option names the model in use, else with the model's id, so a refused choice is never passed to a new program. If wrong: a choice made while the program was dying is not kept across a restart.
- "New session" keeps the shown empty session only while its program has not ended; an ended empty one is replaced, so the button never goes dead after a failed start. If wrong: one condition and one test.
- A session placed in a folder's group stays there even when no session of that folder is left: the user's placement stands. If wrong: a group named after a folder with only sessions from elsewhere in it lingers; "Delete group" clears it.
- "Show older" and "Older groups" ask for no save: what they change is not kept between runs. If wrong: one line each.
- "Move to group" leaves out the older groups while their line is closed; they appear once "Older groups" is open. Listing them needs a change to the core's model. If wrong: a session cannot be moved into such a group without opening that line first.
- Turning on Rune's native folder picker let cargo add the lock entry of `rfd`, a crate Rune names for macOS and Windows that is not built on Linux; nothing else in the lock moved. If wrong: reverting `Cargo.toml` and `Cargo.lock` undoes it.
- Three tests in `main.rs` that build the app from the command line's launch stand in for window tests of the launch rules, because with the fake agent a look stalls the demo's pump. If wrong: the glue carrying a look out at launch is untested until the live run.
- A folder chosen from the menu or in the picker is checked once more at the click; a folder that is gone starts nothing and says "That folder no longer exists: {path}". If wrong: one folder check per click and one line of wording Jerod has not seen. *After the whole-branch review, a folder group's "+" and its menu's "New session" are checked the same way.*


## Jerod's first look

At the end of plan 2 Jerod runs the opt-in live test and the real window in their own terminal:

```bash
MINDRUNE_LIVE=1 cargo test -p mindrune --test live -- --ignored --nocapture
cargo run --release -p mindrune -- .
```

What to try in the window, and what was seen. Jerod's first look was on 2026-11-13 and covered the live test and a few short messages; the rest is still open. A defect in something plan 2 built is fixed in plan 2; anything else goes to plan 3 or 4.

1. The window opens and the footer shows the name, the plan and both usage lines. Seen 2026-11-13: yes, in Jerod's screenshot (the name shown is the account's e-mail address; Jerod did not object).
2. A message gets a reply that streams in as formatted text. Seen 2026-11-13: short questions were answered and nothing was reported wrong with the replies. Streaming and formatting were not commented on.
3. "Use the Bash tool to run: touch probe-marker.txt" shows a permission card, and "Allow once" lets it run. Seen: not yet recorded.
4. A long request ("write a 600 word essay") can be stopped with Escape and with the stop button. Seen: not yet recorded.
5. The model picker changes the model, and the next reply still works. Seen: not yet recorded.
6. Dragging the edge of the conversation makes it wider, and the width is kept after closing and reopening. Seen: not yet recorded.
7. On the ultrawide: does the text feel right at full width, and is anything too small or too faint? Seen: not yet recorded.
8. Close the window while a long reply is still coming, then check in a terminal that no `claude` the window started is left running: `pgrep -af "claude"`. Closing waits up to 2.5 seconds for the session's program to end. Seen: not yet recorded.

Also for Jerod to judge while looking (each is how plan 2 left it; each would be a small change):

- The conversation has no scroll bar: Rune's scroll draws none and nothing adds the kit's. If one is added, the right width handle must be moved in by the bar's width when the width is "no limit", or a press on the bar starts a width drag.
- Collapsing the sidebar (the kit's own button, or Ctrl+Shift+B) also hides the usage footer, while the design says the limits are always visible.
- A width drag has no grab offset: the width is twice the pointer's distance from the centre line, so the edge jumps by up to 16 points at the first move, and in a tile narrower than the saved width (the conversation's part of the window, beside the sidebar) a slight drag snaps the setting to "no limit". Plan 2's own test pins this behaviour.
- A tool call cut short by a stopped turn, a failed turn or a dead session reads "Did not finish". The two words are not from the design.
- A running command reads in the past tense with a spinner beside it ("Ran sleep 30"), as the plan's wording asks. The same holds for an edit that waits for permission: its line reads "Edited ..." with a spinner, above the card that asks.
- "Show all" sits at the right end of a tool's line, not under the output. A long diff is cut at 20 lines like any other output. A read or an edit shows its output only when it failed.
- In a window at its smallest (560 by 400) the sidebar takes almost half the width and the mode picker's label is cut to "Ask befor…".

### What Jerod reported on 2026-11-13

**The live test passed** in 4.4 seconds against `claude` 2.1.323 (the plan was written against 2.1.319): one turn answered, and the session ended cleanly after `Close`. So the adapter's 2 second grace period is long enough for an idle session. The run printed three "ignored a `system/...` line" messages (`hook_started`, `hook_response`, `status`); they are noise.

Jerod then started a few sessions in the window and sent short questions. What they reported, and where each goes:

| Reported | Where it goes |
| --- | --- |
| Each "New session" should add a row on the left. Only one row is ever shown. | **Plan 3.** Plan 2 has one session and "New session" replaces it, as planned. Jerod expected rows from the start, so plan 3's sidebar is the next thing to build. The replaced sessions are not lost: Claude Code stores them, and plan 3 lists stored sessions. |
| The row reads "New session" for ever. It should get a title such as "Comparing X vs Y" or "Today's date". | **Fixed in plan 2's round after the first look.** Claude Code does not name a session by itself in the mode this app runs it in (the three stored sessions of the look have no `ai-title` record). The program has a request for it, `generate_session_title` with `description` and `persist`, read from the program and never run. |
| "There should be a Default category at the top." | **Plan 3, as the design has it.** Jerod, asked the same day: "One per folder, but user can move them around to other groups as they want. They can create new empty groups too, not just new sessions." That is section 5.1 unchanged: one group per project folder to start with, sessions can be moved to any group, and empty groups can be created. |
| A new session needs a folder Jerod can choose. | **Plan 3.** The design's section 5.8 already says so (recent folders, then a folder picker). Plan 2 uses the folder given on the command line. |
| The model picker says "Default (recommended)" and not which model that is. Jerod cannot tell an expensive default from a cheap one. | **Fixed in plan 2's round after the first look**: the model's own name is shown beside such a label. |
| Jerod also named the effort level ("Opus 5.5 Medium or Fable 5.2 High"). | **New wish, not in the design. For plan 3 or 4.** Nothing shows or sets the effort today. Read from the program, never run: the reply to `get_settings` has `applied.effort` and `applied.effortAuto`; each entry of the model list has `supportsEffort` and `supportedEffortLevels`; `claude` takes `--effort` at start; a `per_turn_effort_changed` event exists. How to change it in a running session is not known. |
| There is no "Auto" permission mode. | **Fixed in plan 2's round after the first look.** The design listed three modes as my call; Jerod wants Auto as well. `--permission-mode auto` is documented, and each model in the list says whether it supports it (`supportsAutoMode`). |

The round is commits `ac68735` to `d9ac286` on `plan-2-one-session`. It also fixed four small findings parked by the last review (rulings 29 to 31 and 33) and stopped the three "ignored a line" messages.

Not reported on yet: checks 3 to 8 above and the list of things to judge below.

### Jerod's second look

The title request and Auto mode were read from the program and have never been run. The live test now prints what comes back, so run it first:

```bash
MINDRUNE_LIVE=1 cargo test -p mindrune --test live -- --ignored --nocapture
```

It prints three new lines. None of them fails the test, except not getting back to "Ask before edits".

- `live: title "..."`, or `live: no title after 30 s` with the reason when the request was refused.
- `live: model label "..."`.
- `live: auto mode accepted`, `live: auto mode refused: ...`, `live: auto mode: no answer after 10 s`, or `live: auto mode not offered`.

Then in the window:

9. After the first message the row on the left shows the start of the message, and a moment later a short title. Seen 2026-11-13: worked, by Jerod's word.
10. The model picker names the model, as in "Default (recommended) · Fable 5.2". Seen 2026-11-13: worked, by Jerod's word.
11. The mode picker offers "Auto", and choosing it works or says in the conversation why not. Seen 2026-11-13: worked, by Jerod's word.

Jerod ran the live test and the window again on 2026-11-13 and answered "It all worked", for titles, the model's name and Auto mode. The printed lines were not pasted, so which of the Auto outcomes was printed is not on record. The comments in the code that mark `generate_session_title` and `set_permission_mode` with `auto` as never seen on the wire can be reworded when someone next has the lines in front of them.

Left by this round, for Jerod to judge or for plan 3:

- In a very narrow space the model's label is cut from the right, so "Default (recom…" loses the model's name, the part the label was changed to show. It is whole in the open list. Plan 3's tiles are narrow: consider the model's name first, or the name alone, in the trigger.
- A restarted session does not come back in Auto. `restart_spec` carries no mode, so the session returns in the mode `claude` reports. True for every mode since plan 2.
- A title is asked for once per session, from the first message only. It is never refreshed as the conversation moves on.
- A `system/status` line with a mode that is not text is now ignored without a note.
- The live test has one wrong failure left: Auto unanswered for 10 seconds, then refused late, then "Ask before edits" accepted. The printed lines would show it.
- `FakeAgent` keeps a scripted title answer that no session asked for, and gives it to the next session that asks.

## Left by plan 2

Seam additions plan 2 did not make:

- **The answering model per message.** `init` names the selected model; plan mode answers with a different one. `assistant.message.model` has it. The window shows only the session's model.
- **A way to decline a question.** A question can only be answered. Stop works as a way out.
- **Tool input while it streams.** `ToolStarted` waits for the complete input, so a long `Write` shows nothing until it is whole.
- **Feedback with "Keep planning".** `Answer::KeepPlanning` has a `feedback` field, but the plan card sends none; the user can only type their feedback as the next message.
- **Offering Bypass.** The mode picker offers Bypass only when the session is already in it, because the app cannot tell whether the user's settings allow it.

Behaviour plan 2 left as it is:

- **A sign-in that expires while the window is open.** The next turn fails with Claude Code's own message, and the sign-in screen appears only at the next launch.
- **A wrong first value is corrected late or never** (found by plan 1). When no model was asked for, `Started` names the `default` entry's model; if the user's own settings choose another, `ModelChanged` corrects it only at the first turn. A session id in a later `init` that differs from the one the adapter chose is ignored.
- **The width is saved once per drag**, when the drag ends, not on every move as plan 2's text says: writing the settings file means an fsync on the window's thread for every pointer event.
- **Escape and the pickers.** Holding Escape sends one `Stop` per key repeat. After the stop button is pressed from the keyboard, focus is lost when it turns back into the send button.
- **Question choices are keyed by their label**, so two options with the same label act together.
- **Closing the window waits for one session only.** It sends `Close` to the current session and waits up to 2.5 seconds for its program to end. A session replaced earlier by "New session" whose program has not ended yet is not waited for, nor is a session still starting (the first start can spend up to 20 seconds checking the sign-in). Its wait also keeps the closing window on screen, unresponsive, for that long at most. *Done in plan 3: closing sends `Close` to every session whose program runs and waits up to 2.5 seconds in all.*

Found by the final review of plan 2 and not fixed:

- **Restart can resume a session that has nothing stored.** A session that started and died before its first message has no conversation on disk, so `claude --resume` fails and Restart fails for ever, with a different error each time. Restart should resume only when the conversation holds a user message, and otherwise start afresh in the same folder. The fake resumes anything, so no test sees it. *Done in plan 3: a session resumes only when something is stored for it.*
- **A session that ends by itself with status 0 offers no Restart.** The Restart button lives on the "ended" notice, which is only added for an unclean exit; the input box then says "This session has ended" and only "New session" goes on. *Settled in plan 3 for a session with a conversation: it rests, and the next message starts its program again. One with nothing stored still ends, and "New session" replaces it.*
- **A failed send that is not `Closed` leaves the session working for ever.** `Session::submit` sets the phase to working before the send; when the send fails the app adds a notice but leaves the phase. It should return to idle when the failed command was a message. *Done in plan 3.*
- **Two "ended" notices are possible** when `open` both returns an error and reports `Exited` (the adapter can); the second should be skipped when the session has already ended.
- **The usage footer goes stale after a reset.** Usage is asked for when a session starts and otherwise arrives with turns, so after a window's reset time passes with no turn, the line keeps its old percentage with "Resets soon". The 30 second tick should ask again when it sees a reset time in the past. *Done in plan 3: the tick asks the running session, or takes a look when none runs.*
- **A queued message is lost by "New session".** Only the input box's draft survives. *Done in plan 3: the session it was queued in stays, with its message.*
- **A question whose input could not be read** shows a card with no questions and an enabled "Send answer" that does nothing; Stop is the only way out.
- **Claude can offer its own "Something else" option** (the question recording has one), which then sits directly above the field with the same words.
- **Diffs are computed on the window's thread without a time limit** (`unified_diff` and `line_counts`). Edits are small in practice; `similar` has a deadline setting.
- **Each Restart leaves the dead session's handle in the backend worker's map**, with its closed pipes, until the app quits. *Done in plan 3: each open has a thread of its own, which ends with its session.*
- **The live test uses the user's default model**, where the design asks for the cheapest; one model choice before the message would satisfy both.
- **The window redraws for every message taken in**, including stale ones from a replaced session that change nothing (`glue::poll`).
- **Tool lines never show a file's folder** ("Edited main.rs"); only a request card shows the whole path, where the user decides.
- **The saved conversation width is not checked against the minimum** when it is read, so a hand-edited settings file can give a 100 point column.

Left as they are, for Jerod to decide:

- **(Jerod's judgment) The footer shows the account's e-mail address as the name**, because that is all the adapter has; the design says "name".
- **(Jerod's judgment) "New session" during a turn closes the working session without asking.** Plan 3 replaces this behaviour. *Done in plan 3: the working session stays in the sidebar and goes on working.*
- **(Jerod's judgment) Stop with a message queued sends that message at once.** The design says only that a queued message is "sent when the turn ends".
- **(Jerod's judgment) The word "Claude" appears in the window's text** ("Message Claude", "Claude has a question"). It names the program being driven; no logo, colour or font is copied.
- **(Jerod's judgment) A command waiting for permission already reads "Ran ..."** on its tool line, with a spinner, above the card that asks whether it may run.
- **(Jerod's judgment) A call that did not finish is drawn as failed**, in the danger colour, even when the user stopped the turn themself.
- **(Jerod's judgment) Enter in a "Something else" field sends a complete answer from the keyboard.** The plan asks for it, and the field must be focused on purpose; no request button reacts to a key.
- **How long the window takes to be usable at launch** (one or two runs of `claude auth status`, then the session's start) was not measured without the real program.
- **A local slash command whose result comes only as the result record's text** (a cost summary, for example) may show nothing; what `claude` emits for them was not established.
- **The kit's Markdown parses the whole reply again on each frame while it streams.** This is the toolkit's design, cached inside the kit, and was not measured here.
- **API retries and compaction are ignored**, so the window shows only a spinner while they happen; they are outside this plan's events.
- **Sub-agent activity shows as one tool line with nothing inside**, as plan 1 decided.

Structure:

- **`translate.rs` is now about 2,350 lines**, most of it tests. Split it into a `translate/` module (turns, requests, control replies) the next time it is touched.
- **The `Backend` has one worker thread, shared by all sessions.** A slow `open` or `send` for one session holds up all the others. Plan 3, with many sessions, may want one thread per open. *Done in plan 3.*
- **`Backend` edge cases.** A panic in an agent call ends the worker silently and drops its sessions without `Close`. A session that exits by itself stays in the worker's map until a send to it fails. Two opens with the same number would drop the first handle without `Close`.
- **`FakeAgent` has no per-open identity.** An old handle of a resumed session id works again, where the real adapter's stays closed.
- **The conversation list's version for Rune is the number of the current open.** That relies on items only ever being appended. A change that inserts or removes items in the middle of a conversation must also change that version. *Changed in plan 3: the version is the session's key and its `generation`, which goes up whenever a conversation's items are replaced (a stored history put before them, for example).*
- **Every item carries a change stamp (`Item::changed`).** Every new change to `Conversation` must set it, or finished rows, which are memoised, keep showing their old content.
- **Every `App` intent a view calls must be followed by `glue::run`**, because the drafts of answered questions are dropped there.
- **Rows that are off screen keep the heights measured at the old width** until they are shown again (Rune's virtual list).
- **The "Always allow" control is built by hand**, because the kit's button cannot wrap its label, so a kit change to buttons does not reach it.
- **"This session has ended"** is a text placed where the input box's placeholder goes; its position copies two sizes hard-coded in the kit.
- **The demo window polls every 50 ms for as long as it is open**, and its `poll` waits on the worker thread on purpose, so a loaded test run can print Rune's debug warning "`poll` blocked N ms". Demo and tests only. The real window waits on the worker in one place only: when it is closed, for up to 2.5 seconds, until the session has ended.
- **`testkit::clipboard` reads the clipboard of the last window opened on the thread**, whatever window it is given.
- **Element keys inside a row** (`mr-call-{n}`, `mr-diff`, `mr-output`, `mr-counts`) repeat across rows. Rune allows it (keys only need to be unique among siblings), but a test that looks one up gets the first.
- **Test gap from plan 1:** the test that kills a hung sign-in check uses a 300 ms limit and could fail on a very busy machine.

## Recommendations for plan 3 from the final review of plan 2

- **Keep the replay.** `crates/app/tests/replay.rs` plays every recording of a real `claude` session through the adapter's translator into the core's `Session` and checks the end of every turn. It is the only automatic check of the real path that runs without `claude`. Add a recording whenever a live run shows something new, and make every demo scene something a recording could contain. *Done in plan 3: the replay now goes through `App`, and also plays the recordings three at a time into three sessions of one `App`, each ending as it does alone. Still open: no recording was added.*
- **Put one session's view state in one struct before plan 3.** Which rows are open, which outputs are shown whole, question drafts, the open picker, whether the list follows the end and the draft are loose fields on `Ui` today; with many sessions each must be per session anyway, and replacing a session then becomes one assignment. *Done in plan 3: `SessionView`, one per session, in `Ui.views`.*
- **Carry the tool call's id on a request.** `claude`'s permission request names the tool call it is about (`tool_use_id`). With it on `UserRequest`, the window could draw a request and its tool line as one thing instead of relying on their order. *Still open.*
- **Decide quitting once, in the adapter.** A process group and a kill on shutdown (plan 4 needs both anyway) would make closing the window a few lines and cover every session, not only the current one. *Still open: plan 3 closes every session from the window and waits for all of them, without a process group.*
- **For the reviews:** give the reviewer of each view task the adapter's recordings, and check the plan's wording tables for what the user needs to know to decide, not only for wording. *Followed in plan 3, where a task had recordings to give.*

## What plan 3 must know about the window

- **Keys in use.** Enter sends the message (or queues it while a turn runs); Shift+Enter adds a line. Escape stops a running turn, from the input box or from anywhere else in the window; a picker list that is open takes Escape first and closes. Ctrl+Shift+B (Cmd+Shift+B on macOS, also Ctrl+Cmd+S there) shows and hides the sidebar; this is the kit's shortcut. Tab moves between the input box, the two pickers, the send or stop button, the sidebar toggle and the sidebar's buttons. No key answers a request: request buttons never take focus and ignore Enter and Space. *Still true after plan 3, which added: Ctrl+N opens the "New session" folder menu; the arrow keys move through the sidebar's rows, and Return or Space on a row shows it; the Menu key or Shift+F10 opens a row's menu; the buttons of the "may be open in another app" card follow the request buttons' rule; Escape stops only the shown session's turn.*
- **One session.** `App` holds one `Option<Session>`, and `Ui` holds the state of that one session's view (the draft, which rows are open, which outputs are shown whole, question drafts, whether the list follows the end). Plan 3 needs many sessions: each of these becomes per session, and the stale-open rule (everything from the backend carries the number of its open) must become per session too. *Done in plan 3: see "What plan 4 must know".*
- **The session list grows from `mr-session-row`.** The sidebar shows one `SidebarRow` keyed `mr-session-row`, with the session's title (or "New session"), the folder name as its subtitle, a spinner while a turn runs and a dot when a request waits. The list of sessions goes there, in the kit's `Sidebar`, above the usage footer. *Done in plan 3: `mr-session-row` is gone; the sidebar lists groups (`mr-group-{group key}`) and rows (`mr-row-{session key}`) in its own virtual list.*
- **Every key this app gives an element starts with `mr-`.** The root of the window is `mr-root`; the conversation's middle is `mr-tile`; rows are `mr-item-{id}`. The screenshot test checks every screen for `mr-root` and for Rune's duplicate-key warnings. *Still true; plan 3 kept the rule and added the sessions' and groups' keys.*
- **Screens are tested from scripted scenes.** `crates/app/src/demo.rs` holds the scenes (`--demo --scene NAME`); `cargo test -p mindrune --test snapshots` writes a picture of each main screen in both themes to `target/snap/final`. *Still true; plan 3 added the scenes `sessions`, `group-menu`, `folders`, `empty-start`, `resting`, `history-loading`, `history-failed` and `elsewhere`.*

## Open checks that need Jerod

- **Remote Control, seen by a person.** Turning it on works at the protocol level and returns a claude.ai link. Nobody has yet confirmed the session is listed at claude.ai/code and can be driven from there. Do this before plan 4 builds the switch. *Plan 5 built the switch without this check. The check is now two steps for Jerod: "The check with a second device", in "Jerod's look at plan 5". Its second step makes the recording Task 13 needs.*
- **Listing speed on the real history.** `stored_sessions` was never timed against the real `~/.claude/projects` (over 1,000 files). A permission check refused the run for an agent. Plan 3's sidebar assumes it finishes in well under 10 seconds. Listing reads only file metadata, and `describe` reads at most about 512 KB per file. *Plan 3's live test now prints how long both listings take; open until Jerod runs it.*
- **The repository.** On 2026-11-11 Jerod had the git data removed from this folder and set the repository up again; plan 1's work is the first commit on `main`, and plan 2 is on the branch `plan-2-one-session`, not pushed. Commit ids and the branch name `plan-1-foundations` in older notes point at history that no longer exists. Keep `.gitignore`: it keeps `target/` and `.superpowers/` out of the repository, and `target/rec/` holds raw session recordings that must not be committed.

## The seam: additions the window will need

Plan 1 kept the seam to what its own tasks needed. The whole-branch review named these as likely next additions. Plan 2 made the thinking events, the usage query, the question details and the typed command failure; what it did not make is under "Left by plan 2". Decide each of these when the plan that needs it is written.

| Needed for | Addition | Why |
| --- | --- | --- |
| Plan 3 | A cheap "last modified" for one session | The "live elsewhere" check in the design (5.8). *Done in plan 3: `Agent::last_active`.* |
| Plan 3 | Paging or a limit on `list_sessions` and `read_session` | Over 1,000 sessions; histories of tens of megabytes. *Done in plan 3: a listing since a time, and a history read from the end with a limit on its items.* |
| Plan 3 | Reading history along the `parentUuid` chain | File order would show abandoned branches and compaction summaries. *Done in plan 3.* |
| Plan 3 | How a rename is stored on disk | `describe` reads only `ai-title` records. A renamed session may list under its old title. Not checked. *Settled in plan 3: a name the user gives is a `custom-title` record, and the listing reads it.* |
| Plan 4 | User messages that arrive through Remote Control | `user` text records are ignored today, so the window would show a reply with no prompt. First verify what `claude` emits. |
| Plan 4 | Bridge states other than `ready`, `connected`, `failed` | Unknown states are ignored. If the bridge drops by itself the window would keep showing "On". Not observed yet. |
| Plan 4 | Whether Remote Control can be turned on before a session's first turn | Not exercised. The "on by default" preference depends on it. |

## Behaviour to design around

- **`open` can block.** The first `open` checks the sign-in, which runs `claude auth status` up to twice with a 10 second limit each. `send` can block on a large message. Call both off the UI thread.
- **Events arrive on a background thread.** The sink is called from the thread that reads the child's output. A sink may call `send`.
- **`TitleChanged` can come before `Started`** on a resumed session.
- **A stopped turn** ends with `TurnFinished { interrupted: true }` and may end without a `BlockFinished` for the block that was open.
- **`Exited` ends everything.** No separate `TurnFailed` or `RequestWithdrawn` is sent for what was open.
- **Cost is a running total** for the session's process, not per turn. What it does across a restart of the process is unknown.
- **"In this project" means the git root.** An always-allow rule with that scope is written to the git root's `.claude/settings.local.json`.
- **The host binary must keep Rust's default handling of SIGPIPE** (ignored), or a write to a dead child kills the app instead of returning `Closed`.
- **Wording lives in the adapter for now:** "Always allow …", "Denied by the user", "Keep planning". Move it when the window gets its own strings.
- **A user's own `ANTHROPIC_API_KEY`** in a plain shell is passed through to `claude` on purpose and may take precedence over the claude.ai login.

## Process and structure

- **Quitting and idle-stop (plan 4)** depend on two things plan 1 left as they are: the exit report waits for the child's output pipes to close, so a descendant that inherits them delays it; and a kill reaches only the direct child. A process group on Unix fixes both.
- **Without partial messages no `TurnStarted` is given.** The app always asks for partial messages, so this path is unused.
- **The probe's `events` subcommand cannot answer a request.** A turn that asks permission waits until it is stopped.
- **`full_login` is inferred** as "signed in without the token", not read from `authMethod`. An API-key sign-in would count as a full login.
- **macOS and Windows** are untested. Tests use bash stand-ins; environment names are compared case-sensitively.

## Smaller findings left as they are

From the per-task reviews. None blocks anything.

- Workspace: no `publish = false` on the crates; a licence is named but there is no licence file.
- `mr-agent`: an image attachment serialises as a list of numbers; `StartSpec::new` defaults and error display are untested; the fake's virtual clock is `f32` and drifts over many small steps; fake events for an unknown session are dropped silently.
- `process.rs`: no test for "drop kills the child" or `close_input`; `write_line` does not reject a line containing a newline.
- `encode.rs`: file paths in "Attached files" use a lossy conversion; several small cases untested.
- Probe: a fast child's reply can be recorded before the request line; a child already dead at `initialize` is reported as a broken pipe; the scrubber does not handle the slugged home path (the fixtures were checked by hand) or a name split across two stream lines.
- `wire.rs`: a rate-limit window is dropped if its reset time is a float.
- `translate.rs`: several answer and request mismatches untested; a tracked request is never forgotten if its write fails.
- `account.rs`: only the direct child is killed on timeout; the output read is unbounded; `Debug` on the account types prints the e-mail.
- `history.rs`: fallback titles keep line breaks; the same id in two project folders picks whichever is found first; equal modification times have no tie-break.
