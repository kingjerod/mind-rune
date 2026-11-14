# Notes for plans 3 and 4

Carried over from plan 1 (foundations and the Claude adapter) and plan 2 (one session in a window). Everything here was found while building or reviewing those plans and deliberately left for a later plan. Read it with `docs/protocol-notes.md` before writing plan 3.

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
- **Closing the window waits for one session only.** It sends `Close` to the current session and waits up to 2.5 seconds for its program to end. A session replaced earlier by "New session" whose program has not ended yet is not waited for, nor is a session still starting (the first start can spend up to 20 seconds checking the sign-in). Its wait also keeps the closing window on screen, unresponsive, for that long at most.

Found by the final review of plan 2 and not fixed:

- **Restart can resume a session that has nothing stored.** A session that started and died before its first message has no conversation on disk, so `claude --resume` fails and Restart fails for ever, with a different error each time. Restart should resume only when the conversation holds a user message, and otherwise start afresh in the same folder. The fake resumes anything, so no test sees it.
- **A session that ends by itself with status 0 offers no Restart.** The Restart button lives on the "ended" notice, which is only added for an unclean exit; the input box then says "This session has ended" and only "New session" goes on.
- **A failed send that is not `Closed` leaves the session working for ever.** `Session::submit` sets the phase to working before the send; when the send fails the app adds a notice but leaves the phase. It should return to idle when the failed command was a message.
- **Two "ended" notices are possible** when `open` both returns an error and reports `Exited` (the adapter can); the second should be skipped when the session has already ended.
- **The usage footer goes stale after a reset.** Usage is asked for when a session starts and otherwise arrives with turns, so after a window's reset time passes with no turn, the line keeps its old percentage with "Resets soon". The 30 second tick should ask again when it sees a reset time in the past.
- **A queued message is lost by "New session".** Only the input box's draft survives.
- **A question whose input could not be read** shows a card with no questions and an enabled "Send answer" that does nothing; Stop is the only way out.
- **Claude can offer its own "Something else" option** (the question recording has one), which then sits directly above the field with the same words.
- **Diffs are computed on the window's thread without a time limit** (`unified_diff` and `line_counts`). Edits are small in practice; `similar` has a deadline setting.
- **Each Restart leaves the dead session's handle in the backend worker's map**, with its closed pipes, until the app quits.
- **The live test uses the user's default model**, where the design asks for the cheapest; one model choice before the message would satisfy both.
- **The window redraws for every message taken in**, including stale ones from a replaced session that change nothing (`glue::poll`).
- **Tool lines never show a file's folder** ("Edited main.rs"); only a request card shows the whole path, where the user decides.
- **The saved conversation width is not checked against the minimum** when it is read, so a hand-edited settings file can give a 100 point column.

Left as they are, for Jerod to decide:

- **(Jerod's judgment) The footer shows the account's e-mail address as the name**, because that is all the adapter has; the design says "name".
- **(Jerod's judgment) "New session" during a turn closes the working session without asking.** Plan 3 replaces this behaviour.
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
- **The `Backend` has one worker thread, shared by all sessions.** A slow `open` or `send` for one session holds up all the others. Plan 3, with many sessions, may want one thread per open.
- **`Backend` edge cases.** A panic in an agent call ends the worker silently and drops its sessions without `Close`. A session that exits by itself stays in the worker's map until a send to it fails. Two opens with the same number would drop the first handle without `Close`.
- **`FakeAgent` has no per-open identity.** An old handle of a resumed session id works again, where the real adapter's stays closed.
- **The conversation list's version for Rune is the number of the current open.** That relies on items only ever being appended. A change that inserts or removes items in the middle of a conversation must also change that version.
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

- **Keep the replay.** `crates/app/tests/replay.rs` plays every recording of a real `claude` session through the adapter's translator into the core's `Session` and checks the end of every turn. It is the only automatic check of the real path that runs without `claude`. Add a recording whenever a live run shows something new, and make every demo scene something a recording could contain.
- **Put one session's view state in one struct before plan 3.** Which rows are open, which outputs are shown whole, question drafts, the open picker, whether the list follows the end and the draft are loose fields on `Ui` today; with many sessions each must be per session anyway, and replacing a session then becomes one assignment.
- **Carry the tool call's id on a request.** `claude`'s permission request names the tool call it is about (`tool_use_id`). With it on `UserRequest`, the window could draw a request and its tool line as one thing instead of relying on their order.
- **Decide quitting once, in the adapter.** A process group and a kill on shutdown (plan 4 needs both anyway) would make closing the window a few lines and cover every session, not only the current one.
- **For the reviews:** give the reviewer of each view task the adapter's recordings, and check the plan's wording tables for what the user needs to know to decide, not only for wording.

## What plan 3 must know about the window

- **Keys in use.** Enter sends the message (or queues it while a turn runs); Shift+Enter adds a line. Escape stops a running turn, from the input box or from anywhere else in the window; a picker list that is open takes Escape first and closes. Ctrl+Shift+B (Cmd+Shift+B on macOS, also Ctrl+Cmd+S there) shows and hides the sidebar; this is the kit's shortcut. Tab moves between the input box, the two pickers, the send or stop button, the sidebar toggle and the sidebar's buttons. No key answers a request: request buttons never take focus and ignore Enter and Space.
- **One session.** `App` holds one `Option<Session>`, and `Ui` holds the state of that one session's view (the draft, which rows are open, which outputs are shown whole, question drafts, whether the list follows the end). Plan 3 needs many sessions: each of these becomes per session, and the stale-open rule (everything from the backend carries the number of its open) must become per session too.
- **The session list grows from `mr-session-row`.** The sidebar shows one `SidebarRow` keyed `mr-session-row`, with the session's title (or "New session"), the folder name as its subtitle, a spinner while a turn runs and a dot when a request waits. The list of sessions goes there, in the kit's `Sidebar`, above the usage footer.
- **Every key this app gives an element starts with `mr-`.** The root of the window is `mr-root`; the conversation's middle is `mr-tile`; rows are `mr-item-{id}`. The screenshot test checks every screen for `mr-root` and for Rune's duplicate-key warnings.
- **Screens are tested from scripted scenes.** `crates/app/src/demo.rs` holds the scenes (`--demo --scene NAME`); `cargo test -p mindrune --test snapshots` writes a picture of each main screen in both themes to `target/snap/final`.

## Open checks that need Jerod

- **Remote Control, seen by a person.** Turning it on works at the protocol level and returns a claude.ai link. Nobody has yet confirmed the session is listed at claude.ai/code and can be driven from there. Do this before plan 4 builds the switch.
- **Listing speed on the real history.** `stored_sessions` was never timed against the real `~/.claude/projects` (over 1,000 files). A permission check refused the run for an agent. Plan 3's sidebar assumes it finishes in well under 10 seconds. Listing reads only file metadata, and `describe` reads at most about 512 KB per file.
- **The repository.** On 2026-11-11 Jerod had the git data removed from this folder and set the repository up again; plan 1's work is the first commit on `main`, and plan 2 is on the branch `plan-2-one-session`, not pushed. Commit ids and the branch name `plan-1-foundations` in older notes point at history that no longer exists. Keep `.gitignore`: it keeps `target/` and `.superpowers/` out of the repository, and `target/rec/` holds raw session recordings that must not be committed.

## The seam: additions the window will need

Plan 1 kept the seam to what its own tasks needed. The whole-branch review named these as likely next additions. Plan 2 made the thinking events, the usage query, the question details and the typed command failure; what it did not make is under "Left by plan 2". Decide each of these when the plan that needs it is written.

| Needed for | Addition | Why |
| --- | --- | --- |
| Plan 3 | A cheap "last modified" for one session | The "live elsewhere" check in the design (5.8). |
| Plan 3 | Paging or a limit on `list_sessions` and `read_session` | Over 1,000 sessions; histories of tens of megabytes. |
| Plan 3 | Reading history along the `parentUuid` chain | File order would show abandoned branches and compaction summaries. |
| Plan 3 | How a rename is stored on disk | `describe` reads only `ai-title` records. A renamed session may list under its old title. Not checked. |
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
