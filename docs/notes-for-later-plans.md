# Notes for plans 2 to 4

Carried over from plan 1 (foundations and the Claude adapter). Everything here was found while building or reviewing plan 1 and deliberately left for a later plan. Read it with `docs/protocol-notes.md` before writing plan 2.

## Fix first in plan 2

Found by the last review of plan 1, after the one fix wave the process allows. The behaviour is right; two sentences that describe it are wrong.

- **`Started` is not guaranteed.** The doc comments in `crates/agent/src/agent.rs` and `crates/agent/src/event.rs` say `Started` comes exactly once. It comes at most once. If `claude` exits before it answers its first request (a bad session id to resume, a bad flag), the window gets only `Exited`, and `open` may also return an error while the sink still receives that `Exited`. Correct the two comments, and give `FakeAgent` a way to play "exits without starting" so the window's handling of it can be tested.
- **The fake and the real adapter still differ in two small ways.** A second `Close` before the program has exited returns `Ok` from the real adapter and `Closed` from the fake. An `Answer` for a request that is not waiting returns `Protocol` from the real adapter; the fake accepts anything.
- **A wrong first value is corrected late or never.** When no model was asked for, `Started` names the `default` entry's model; if the user's own settings choose another, `ModelChanged` corrects it only at the first turn. A session id in a later `init` that differs from the one the adapter chose is ignored.
- **Test gaps.** The replay test over the recordings would catch doubled text but not lost text, and it runs only the older start-up path; assert per turn that the streamed text equals the text of that turn's complete records. The test that kills a hung sign-in check uses a 300 ms limit and could fail on a very busy machine.

## Open checks that need Jerod

- **Remote Control, seen by a person.** Turning it on works at the protocol level and returns a claude.ai link. Nobody has yet confirmed the session is listed at claude.ai/code and can be driven from there. Do this before plan 4 builds the switch.
- **Listing speed on the real history.** `stored_sessions` was never timed against the real `~/.claude/projects` (over 1,000 files). A permission check refused the run for an agent. Plan 3's sidebar assumes it finishes in well under 10 seconds. Listing reads only file metadata, and `describe` reads at most about 512 KB per file.
- **No git history.** On 2026-11-11 Jerod had the git data removed from this folder and will set up the repository and make the first commit themself. Commit ids and the branch name `plan-1-foundations` in older notes point at history that no longer exists. Keep `.gitignore`: it keeps `target/` and `.superpowers/` out of the new repository, and `target/rec/` holds raw session recordings that must not be committed. Do not run `git init` or commit here unless Jerod asks.

## The seam: additions the window will need

Plan 1 kept the seam to what its own tasks needed. The whole-branch review named these as likely next additions. Decide each when the plan that needs it is written.

| Needed for | Addition | Why |
| --- | --- | --- |
| Plan 2 | An event for "thinking started / finished", or a token estimate | Thinking text is never sent, so today a thinking phase is invisible. `system`/`thinking_tokens` carries an estimate. |
| Plan 2 | A usage query on the agent | The footer should be filled on launch. `get_usage` works at any time; today limits arrive only during a turn. Note its numbers are percentages and ISO times, unlike the event's fractions and Unix times. |
| Plan 2 | The answering model per message | `init` names the selected model. Plan mode answered with a different one. `assistant.message.model` has it. |
| Plan 2 | Option descriptions and the header on a `Question` | Dropped today; only labels are kept. |
| Plan 2 | A way to decline a question | A question can only be answered. Stop works as a way out. |
| Plan 2 | A typed `CommandFailed.what` | It is a free string the window would have to match on. |
| Plan 2 | Tool input while it streams | `ToolStarted` waits for the complete input. A long `Write` shows nothing until it is whole. |
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
- **`translate.rs` is about 1,500 lines**, most of it tests. Split it into a `translate/` module (turns, requests, control replies) the next time it is touched.
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
