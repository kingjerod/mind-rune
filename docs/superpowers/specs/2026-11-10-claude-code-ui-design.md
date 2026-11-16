# MindRune: a wide-screen desktop UI for Claude Code

Design document, 2026-11-10. Status: awaiting Jerod's approval. Nothing is built yet.

The project is named MindRune (chosen 2026-11-12). The binary is `mindrune`. Until then the working name was "Rune Claude", taken from the project folder `rune_claude`, which keeps that name.

## 1. Why this exists

Jerod uses the Claude Desktop app's Code tab on an ultrawide monitor. The conversation column
there is capped at a narrow fixed width, so most of the screen stays empty. Jerod runs several
Claude Code sessions at once, in different projects, and spends a lot of time switching between
them just to see whether one has finished or is waiting for an answer.

This app is a desktop window for Claude Code that uses a wide screen properly: the conversation
is as wide as Jerod wants it, and several sessions sit side by side in one window. Apart from
that, it should feel like Claude Desktop.

## 2. How Jerod works (in their words, paraphrased)

- Looks at the sidebar to see which sessions are going.
- Switches back and forth between sessions in different projects, mostly to check whether one
  has finished or is waiting on them.
- Gives most attention to the centre conversation.
- Checks which model a session is using "a fair amount".
- Glances at environment and context now and then, not constantly.
- Uses Claude's Remote Control option.

## 3. Goals, non-goals, anti-goals

**Goals**

1. The conversation width is the user's choice, never a fixed narrow cap.
2. Several sessions are visible and usable at once, tiled side by side.
3. At any moment the window shows which sessions are working, which need the user and which
   have finished, without switching to them.
4. The model in use is always visible wherever the user types.
5. The five-hour and weekly usage limits are always visible as plain text.
6. Everything else behaves the way a Claude Desktop user expects.

**Non-goals for the first version** (see section 6 for the full list)

- Being a second agent. The app never talks to a model itself; Claude Code does all agent work.
- Supporting any agent other than Claude Code. The door is left open (section 7), nothing more.
- Feature parity with every pane of Claude Desktop.

**Anti-goals: the app has failed, even if it works, when**

- it feels slower or heavier than Claude Desktop;
- it loses track of a session, or shows a stale state for one;
- a permission request is easy to miss, or easy to answer by accident;
- it copies Anthropic's branding or passes itself off as an official app.

**Success:** Jerod uses this instead of Claude Desktop for daily work on the ultrawide.

## 4. Decisions Jerod made

These are settled. Do not reopen them while planning.

| Decision | As decided |
| --- | --- |
| UI toolkit | Rust with Rune (github.com/anomalyco/rune). |
| Agent | Claude Code, not a fork of OpenCode and not a bare model. |
| Other agents | One seam between the UI and the agent, built now. Claude Code is the only thing behind it. |
| Layout | Sessions list on the left, conversation(s) in the middle, environment and context on the right. |
| Middle | Both: a conversation width the user drags, and several sessions tiled side by side. |
| Feel | Same as Claude Desktop apart from width and tiling. |
| Attachments | Pasting images and attaching files are in the first version. |
| Remote Control | In the first version. |
| Usage limits | Always visible at the bottom left, near name and subscription, as text: "25% of current session used", "45% of weekly limit used". Not hidden in a small circle. |
| Session groups | The user can rename groups (`rune_claude` to "MindRune"), create new groups, and move sessions between groups. |
| Remote Control verification | Carry on with the design; verify Remote Control end to end as the first build step. |
| Closing the window | Closing the window keeps the app and its sessions running in the background. An explicit Quit stops everything. Sessions surviving a full quit was offered and not chosen. |

Jerod approved the full-window mockup in
`.superpowers/brainstorm/136623-1794367828/content/full-window-v2.html`. It is the reference for
arrangement. Its colours and fonts are placeholders.

## 5. The window

Everything in this section that Jerod did not state in section 4 is the designer's call, shown
to Jerod in the mockup or the conversation. Calls Jerod has not seen are marked **(call)**.

### 5.1 Sidebar (left)

- A "New session" control at the top.
- Sessions listed under **groups**.
  - By default there is one group per project folder, named after the folder.
  - A group can be renamed. Renaming changes only the label, never a folder on disk.
  - The user can create an empty group and delete a group. **(call)** Deleting a group sends
    its sessions back to their folder's default group; a default group reappears when needed.
  - A session moves between groups by dragging its row, and by a "Move to group" entry in the
    row's context menu. **(call)**
  - Because a group can hold sessions from several folders, a row whose folder differs from
    the group's own shows the folder name in muted text. **(call)**
  - Groups collapse and expand. **(call)**
- A session gets its title from its first message (Jerod, 2026-11-13). **(call)** The row
  shows the start of that message at once, and the app asks Claude Code for a short title,
  which replaces it when it arrives. This costs one small extra request per session.
- Each session row shows a state mark and the session title:
  - **working**: a turn is in flight;
  - **needs you**: a permission request, a question from Claude or a plan approval is pending;
  - **finished**: a turn ended and the user has not looked at the session since;
  - no mark: idle and seen.
- Rows for sessions currently open in a tile are highlighted.
- **Footer, always visible:** the user's name and plan, then two lines of text with a thin bar
  under each and the reset time beneath:
  - `N% of current session used` (the five-hour window)
  - `N% of weekly limit used` (the seven-day window)
  - **(call)** The bar changes colour as a limit nears: normal below 75%, warning from 75%,
    alarm from 90%. Before any numbers are known the lines read "Usage not known yet".
- The sidebar can be collapsed and resized.

**Which sessions are listed (call).** This machine holds over a thousand stored Claude Code
sessions across about twenty folders, some created by automated tools. Listing everything would
bury the live ones. The sidebar lists every session this app has started or opened, plus any
stored session with activity in the last 30 days. Each group has a "Show older" row that loads
the rest. A group can be hidden from its context menu, and hidden groups are restored from a
"Hidden groups" entry at the bottom of the list.

### 5.2 Tiles (middle)

- The middle is a single row of **tiles**. Each tile is one whole session: header,
  conversation, input box. The first version had one row only, as the designer's call.
  Jerod, 2026-11-15: a column of the row can hold two tiles, one above the other, and never
  more; a column with one tile keeps the full height.
- Tiles can be put in another order (Jerod, 2026-11-15): a tile is dragged by its header.
- Clicking a session in the sidebar shows it in the focused tile. **(call)** "Open to the
  side" opens it in a new tile: drag the row into the middle, or use the row's context menu,
  or Ctrl-click the row.
- A tile closes with its close button. Closing a tile does not stop its session.
- Dividers between tiles drag to resize. **(call)** A tile has a minimum width of about 420
  points; a new tile cannot be opened when there is no room for it.
- The **focused tile** is the one the user last clicked or typed in. It has an accent outline.
  The right panel describes the focused tile.
- A tile whose session **needs you** has an amber outline and an amber "needs you" label in
  its header, whether or not it is focused.
- **Tile header:** folder chip, session title (double-click to rename), state, a Remote
  Control indicator when it is on, close button.
- The tile arrangement (which sessions, their order and widths, which is focused) is saved and
  restored on the next launch.

### 5.3 Conversation width

- One number, the **conversation width**, set by the user and remembered. In every tile the
  text column is as wide as that number or the tile, whichever is smaller, and centred.
- When the column is narrower than its tile, a drag handle shows on each edge of the column on
  hover. Dragging either handle changes the width symmetrically. Dragging to the tile's edge
  means "no limit".
- **(call)** Default: 1000 points. Minimum: 480 points.

### 5.4 Conversation

- The user's messages, Claude's replies streaming in as formatted Markdown with coloured code,
  tables and diff fences.
- Thinking shows as a collapsed "Thought for N s" line that expands.
- Each tool call is one line that expands to its details ("Read 3 files", "Searched for …").
  Consecutive calls of the same kind fold into one line.
- File edits show as a diff, with the file name and added and removed counts.
- Commands show the command line and, expanded, their output, cut to a sensible length with a
  "show all" control.
- A **permission request** is a card at the end of the conversation naming the tool and what
  it wants to do, with three buttons: Allow once, Always allow, Deny. **(call)** The buttons
  never take keyboard focus by themselves, so typing or pressing Enter in the input box cannot
  answer a request by accident.
- **(call) What "Always allow" covers.** Claude Code sends its own suggested rule with each
  request (for example "allow `npm test` in this project"). "Always allow" accepts exactly
  that suggestion and nothing wider, which is what Claude Desktop and the terminal do. The
  button's label states the scope in words taken from the suggestion, such as "Always allow
  `npm test` in this project", so the user sees what lasting permission they are granting.
  When a request comes with no suggestion, the button is not shown. The seam's "answer a
  pending request" command therefore carries: allow or deny, and whether to accept the
  agent's suggested rule.
- **(call)** A **question from Claude** (the AskUserQuestion tool) shows its options as
  buttons with a free-text answer. A **plan approval** (leaving plan mode) shows the plan with
  Approve and Keep planning. Both put the session in the "needs you" state.
- The conversation follows new output at the bottom, and stops following when the user scrolls
  up. A "jump to latest" control appears then.
- Text can be selected and copied. Code blocks and messages have a copy control.
- Errors from Claude Code (rate limit reached, not signed in, process died) show as a plain
  message in the conversation with what to do next.

### 5.5 Input box

- Multi-line. Enter sends, Shift+Enter adds a line.
- While a turn runs, the send button becomes a stop button. Escape in the focused tile also
  stops it. A message typed during a turn is queued and sent when the turn ends.
- **Model picker**, always visible, showing the session's current model. Changing it applies
  to the next turn. A choice whose label does not name a model, such as "Default
  (recommended)", shows the model's name beside it (Jerod, 2026-11-13: the user must be able
  to tell which model they are paying for).
- **Effort level** beside the model (Jerod, 2026-11-13: "Opus 5.5 Medium or Fable 5.2
  High"): always visible for a model that has one, and changeable there. The user must be
  able to tell which effort they are paying for, as with the model.
- **Permission mode picker** beside it: Ask before edits, Accept edits, Plan, Auto. Auto was
  added on 2026-11-13 at Jerod's request and is offered when the model in use supports it.
  **(call)** Bypass mode is offered only when the user's own Claude Code settings allow it.
- **Attachments:**
  - pasting an image from the clipboard adds it as a chip;
  - dropping files onto the tile adds them as chips;
  - a paperclip button opens a file picker;
  - each chip has a remove control, and image chips show a thumbnail;
  - images are sent to Claude as images; other files are sent by path so Claude reads them
    from disk.
- Slash commands and file mentions can be typed as plain text. Their pop-up menus are not in
  the first version.

### 5.6 Right panel

Describes the focused tile. Hides and shows with one click, and resizes.

- **Environment:** folder, git branch, model, permission mode, Remote Control switch.
- **Context:** percentage of the context window used, with a bar; cost of the session so far.
- **Files changed** in this session, with added and removed counts.

### 5.7 Remote Control

- A switch per session, in the right panel. When on, the tile header shows a "Remote" mark.
- Turning it on asks Claude Code to enable Remote Control for that session, so it appears at
  claude.ai/code and in the Claude mobile app.
- If Claude Code refuses, the switch returns to off and the session shows why. The known case
  is a sign-in that is not a full claude.ai login; the message tells the user to run
  `claude auth login` in a terminal.
- **(call)** A preference "Turn Remote Control on for new sessions", off by default.

### 5.8 Starting, continuing and stopping sessions

- **New session** asks for a folder (recent folders first, then a folder picker) and opens an
  empty tile in it. **(call)** It joins that folder's default group, or the group whose "+"
  the user pressed.
- **Stored sessions** from Claude Code's own history can be opened. **(call)** Opening one
  shows its conversation from disk without starting anything; Claude Code is started for it
  only when the user sends a message. This keeps opening old sessions instant and avoids
  touching a session that may be live in another app.
- **(call) Sending to a session that may be live elsewhere.** Sending continues the session
  in place, as `claude --resume` does. If the session's history file changed in the last two
  minutes and this app did not write it, the app first asks: "This session may be open in
  another app", with Continue here, Continue as a copy and Cancel. "Continue as a copy" uses
  `--fork-session`; the copy gets a new id and appears as a new row in the same group.
- Sessions keep running when their tile is closed. Their state still shows in the sidebar.
- **Closing the window does not quit** (Jerod's decision). The app keeps running with no
  window; sessions keep working and stay reachable through Remote Control. Rune supports
  this with `App::keep_running()`.
  - There must be an obvious way to bring the window back and an obvious Quit. **(call)**
    Launching the app again shows the existing window instead of starting a second copy, and
    a tray icon offers Show and Quit where the desktop supports one. On Wayland a closed
    window is destroyed and recreated, not hidden; the builder must verify that reopening
    restores the same tiles and conversations.
  - Do not build the app in a way that rules out moving sessions into a separate background
    service later. Keeping process supervision inside the Claude adapter, behind the seam, is
    enough.
- **(call)** Quit asks first when sessions are working or have Remote Control on. Quit stops
  the app's Claude Code processes; the sessions remain in history and can be continued later.
- **(call)** A session that has been idle for a while may have its process stopped quietly to
  save memory. It restarts on the next message. The user never sees this. A session is never
  stopped this way while it is working, needs the user, or has Remote Control on.

### 5.9 Not signed in

When Claude Code reports that it is not signed in, the app shows one clear screen instead of
failing inside a tile: what is wrong, the command to run (`claude auth login`), and a "Check
again" button. The same screen covers "Claude Code is not installed" with its own text.

### 5.10 Look

**(call)** Rune's component kit (`rune-kit`) with a plain neutral theme built from the kit's
palette seed, following the system's light or dark setting. No Anthropic logos, colours or
fonts. OpenCode's theme crate is a reference for proportions only.

## 6. First version: in and out

**In:** everything in section 5.

**Left for later, by agreement with Jerod**

- Built-in terminal, browser pane, pull request tools, artifacts.
- Worktrees and cloud sessions.
- Pop-up menus for slash commands and file mentions.
- Settings screens. The app follows the user's existing Claude Code settings.

**Left for later, designer's call (Jerod has not been asked)**

- Desktop notifications when a session needs the user while the window is in the background.
- More than two tiles in a column, or more than one window. (Two tiles in a column came
  in on 2026-11-15, at Jerod's request.)
- Reordering groups by dragging. (Reordering tiles came in on 2026-11-15, at Jerod's
  request.)
- Search across sessions.
- macOS and Windows builds. Linux is first because it is Jerod's machine. Nothing in the
  design should prevent the others.

## 7. The seam between the window and the agent

The UI never mentions Claude Code. It talks to an **agent adapter** through a small
vocabulary. This is the one structural rule Jerod asked for.

**From the agent to the UI (events)**

- session started (id, folder, model, permission mode, available models and modes)
- text arriving for the current reply, piece by piece
- thinking started and finished
- a tool call started, progressed and finished (name, a short summary, structured details
  such as a diff or command output, success or failure)
- needs the user: a permission request, a question with options, or a plan to approve
- turn finished (cost, tokens, context used) or failed (reason)
- usage limits changed (per window: fraction used, reset time)
- remote access changed (off, starting, on, failed with reason)
- session title changed
- account state (signed in or not, name, plan)

**From the UI to the agent (commands)**

- start a session in a folder, or continue a stored one
- send a message with attachments
- answer a pending request
- stop the current turn
- set the model; set the permission mode
- turn remote access on or off
- rename the session
- list stored sessions and read a stored session's conversation

Rules for the seam:

- No type from the Claude Code adapter appears in UI code or in app state.
- Things an agent may not support (remote access, plan mode, usage limits) are optional
  capabilities the adapter declares. The UI hides what an adapter lacks.
- A **fake adapter** that plays scripted events is part of the build from the start. All UI
  tests and all headless screenshots run against it, with no real Claude Code process.

No second adapter is built. No plug-in system, no adapter picker in the UI.

## 8. How it talks to Claude Code

The app drives the `claude` program already installed on the machine, one child process per
live session. It uses the user's existing login, settings, permissions, skills, hooks and
session history. It does not reimplement any of the agent.

Facts below are marked by how well they are known. Version tested: `claude` 2.1.319.

**Observed live on Jerod's machine**

- `claude -p --input-format stream-json --output-format stream-json --verbose
  --include-partial-messages` runs as a long-lived process: JSON lines in on stdin, JSON lines
  out on stdout. One user message in, a stream of events out, then a `result` event; the
  process then waits for the next message.
- The first event of a turn sequence is `system`/`init` with the session id, model, permission
  mode, tools, slash commands, skills, agents and plugins.
- Reply text arrives as `stream_event` records wrapping the model's streaming events
  (`message_start`, `content_block_start`, `content_block_delta` with `text_delta`,
  `content_block_stop`, `message_delta`, `message_stop`), followed by a complete `assistant`
  record.
- A `rate_limit_event` arrives during each turn. Its `rate_limit_info.unifiedWindows` holds
  `five_hour` and `seven_day`, each with `utilization` (0 to 1), `resetsAt` (Unix seconds) and
  `status`. These are account-wide, so the newest one from any session feeds the footer.
- The `result` event carries `total_cost_usd`, `usage`, `modelUsage` (including the context
  window size), `duration_ms` and `is_error`.
- Control requests go in as `{"type":"control_request","request_id":…,"request":{…}}` and
  are answered by `{"type":"control_response","response":{"request_id":…,"subtype":"success"
  | "error",…}}`.
- `{"subtype":"remote_control","enabled":true,"name":…}` is recognised in this mode. It was
  **refused** in testing with `error_code: "remote_control_auth"`, and a `system`/
  `bridge_state` event reported `state: "failed"`, because the test shell was signed in
  through a `CLAUDE_CODE_OAUTH_TOKEN` environment variable (set in Jerod's `~/.bashrc`), not a
  full claude.ai login. A successful enable has not been observed.
- `claude auth status` prints JSON with `loggedIn` and `authMethod`.
- Stored sessions live at `~/.claude/projects/<folder path with slashes as dashes>/
  <session id>.jsonl`. Records have a `type`; the ones that matter for showing a conversation
  are `user`, `assistant`, `system` and `ai-title`. `user` and `assistant` records carry
  `message`, `cwd`, `gitBranch`, `timestamp`, `uuid`, `parentUuid` and `isSidechain`. There
  are many other record types; ignore the ones not needed.

**Read from `claude --help`**

- `--resume <id>` continues a stored session; `--session-id <uuid>` sets the id of a new one.
- `--model`, `--permission-mode` (`acceptEdits`, `auto`, `bypassPermissions`, `manual`,
  `dontAsk`, `plan`), `--effort`, `--add-dir`, `--name`.
- `--permission-prompts host` (the default with `-p`) sends permission prompts to the host
  program.
- `--replay-user-messages` echoes sent messages back for acknowledgement.

**Inferred from the program, not yet exercised. Verify each before relying on it.**

- Control request subtypes that appear to exist: `initialize`, `interrupt`, `can_use_tool`
  (sent by Claude Code to the host for a permission decision), `set_model`,
  `set_permission_mode`, `list_models`, `get_context_usage`, `get_usage`, `rename_session`,
  `get_workspace_diff`, `request_user_dialog`, `end_session`.
- Events that appear to exist: `session_title_changed`, `session_state_changed`,
  `compact_boundary`, `api_retry`, `task_started` / `task_progress`.
- User messages can carry image content blocks (base64) alongside text, as in the Anthropic
  Messages format.

The official Claude Agent SDK (TypeScript and Python) speaks this same protocol to the same
program. **Treat the SDK's transport and message types as the reference** for exact shapes,
especially for permission requests and the `initialize` handshake. Where this document and the
SDK disagree, the SDK wins.

**Sign-in and environment**

- A desktop app started from a launcher does not inherit shell variables from `~/.bashrc`.
  Started from a terminal, it does.
- **(call)** When a saved full login exists, start `claude` without `CLAUDE_CODE_OAUTH_TOKEN`
  so Remote Control works. When none exists, pass the variable through so the app still works
  for everything except Remote Control. The builder should confirm which source `claude`
  prefers when both are present.
- A shell started from inside a Claude Desktop session carries `CLAUDE_CODE_*` and
  `ANTHROPIC_*` variables that break a child `claude` (it reports "not signed in"). The app
  must never forward those to its children, and tests that run the real `claude` must start
  from a clean environment. This cost an hour during design.

### 8.x Verified live on 2026-11-10 (after this design was approved)

Plan 1's live verification ran every item on the "verify first" list against `claude` 2.1.319 with a full claude.ai login. The record is `docs/protocol-notes.md`, which wins over anything above where the two differ. What changed in this design as a result:

- **Remote Control works in this mode.** Turning it on reports `ready`, then `connected`, and returns the session's claude.ai link. One thing is still unconfirmed by a person: that the session is listed at claude.ai/code.
- **Always allow sends back one suggestion, not all of them.** A request offers several suggestions at once (save a rule, add a folder, switch to accept-edits mode). The app sends only the first, which is the one its button label describes. **(call)**
- **A new event in the seam: a request was withdrawn.** Claude Code can cancel a waiting permission request, for example when the turn is stopped. The window removes the card. **(call)**
- **A stopped turn is not a failure.** Claude Code marks it as an error; the adapter recognises it and ends the turn normally.
- **Thinking text is not sent.** Claude Code streams thinking with empty text and reports only a token estimate. The conversation shows no thinking text; plan 2 may show a "thinking" marker from the estimate.
- **The model that answers can differ from the model selected.** In plan mode a session set to Haiku answered with Sonnet. The window shows the selected model; plan 2 may also show the answering model per message.
- **Usage limits, context use and the model list can be asked for at any time**, without waiting for a turn. Plan 2 should use this to fill the usage footer on launch.
- **A session is known from the moment it opens.** The adapter chooses the session id for new and forked sessions, and reports the session as started, with the list of models to choose from, when Claude Code answers its first request. That happens before the first message, so a new, empty tile can already show its model. **(call)**
- **Always-allow labels name the tool and the rule**, for example "Always allow Bash(npm test:*) in this project". A rule saved to the shared project settings reads "for everyone in this project". **(call)**
- **A child never inherits a parent Claude session's variables,** now including every `CLAUDE_*` name except the configuration folder and the sign-in token rule.
- **"In this project" means the git root.** A saved rule lands in the git root's local settings, not the session's folder.

## 9. Shape of the program

A suggested split. The builder may change it, keeping the seam in section 7.

- **agent**: the vocabulary of section 7 as plain Rust types and one trait. No dependencies on
  Rune or on Claude Code.
- **claude adapter**: starts and supervises `claude` processes, translates its JSON lines to
  and from the vocabulary, reads stored sessions from disk, reports account state.
- **fake adapter**: plays scripted event sequences on a virtual clock.
- **app state**: sessions, groups, tiles, the conversation model for each open session,
  pending requests, usage limits, preferences. Pure data and transitions, testable without a
  window. Behaviour with modes (a session's life, a pending request, a drag) is an explicit
  state machine in the style Rune recommends (`rune::machine`).
- **persistence**: the app's own file under the user's config directory (on Linux,
  `~/.config/mindrune/`). Holds groups and their names, which session is in which group,
  hidden groups, the tile arrangement, the conversation width, panel widths and preferences.
  The app never writes inside `~/.claude`.
- **ui**: Rune views. The shell (`rune_kit::shell::SplitView`, `Sidebar`), the tile row, the
  conversation, the input box, the right panel, the not-signed-in screen.

**Threads.** Each `claude` process has a reader thread that parses lines and pushes events
onto a channel, then wakes the window with the `Waker` from `AppSpec::setup`. `poll` drains
the channel into app state and returns true only when something visible changed. No blocking
work happens in a handler or in `poll`.

**Rune facts the builder needs**

- Rune is pre-release. Pin a revision of its `dev` branch in `Cargo.toml`; the one read for
  this design is `2a44156b21d63b0342a366b26d9222a6edd4622c`. Use `rune` and `rune-kit` (with
  the `markdown` feature) from the same revision. Rune pins Rust 1.99.1.
- Read `crates/rune/README.md`, `crates/rune-kit/README.md` and `examples/hello` first.
- The kit already has: the app shell with sidebar sections, rows, renaming in place and
  drag-to-reorder; streaming Markdown with code colouring and diff fences; `DiffView`; text
  areas with pills; menus, context menus, popovers, toasts, tooltips; virtual lists.
- Rune supports pasted images (`on_input_paste` with `content.image()`), file drops
  (`on_drop_files`) and native file dialogs.
- Long conversations must use virtual rows and the growth stack so that streaming and
  scrolling stay smooth. Follow "Smooth scrolling" and "Launching fast" in Rune's README.
- Every Rune app runs headless: `--snap out.png`, `--tree tree.json`, `--script file.json`.
  Use these to check the UI without a display.
- Linux build needs `libxkbcommon-dev` and `libwayland-dev`; windowed runs need
  `libxkbcommon-x11-0`. On 2026-11-10 Jerod's machine had none of these, and no Rust
  toolchain. Vulkan drivers, `ffmpeg` and `claude` are present. The session is Wayland.

## 10. Errors and edge states

- **Claude Code missing or not signed in:** section 5.9.
- **A `claude` process exits unexpectedly:** the session shows an error line with a "Restart"
  button that continues the session. Other sessions are unaffected.
- **Usage limit reached:** the footer shows 100% in the alarm colour with the reset time, and
  the affected conversation shows Claude Code's own message.
- **An event the adapter does not recognise:** ignored and logged, never a crash. Claude Code
  updates often and adds event types.
- **A stored session file that cannot be read:** listed with its id as the title and an error
  when opened.
- **Remote Control refused:** section 5.7.
- **Attachment too large or unreadable:** the chip shows the problem and the message is not
  sent until it is removed.
- **Two tiles asked to show the same session:** focus the existing tile instead.

## 11. Testing

- App state and the seam are tested as plain Rust, driven by the fake adapter.
- The Claude adapter's translation is tested against recorded JSON-line fixtures, including
  the ones captured during design (a normal turn, a rate-limit event, a refused Remote Control
  request). Add recordings for permission requests, tool calls and errors as they are
  verified.
- UI is tested with Rune's headless `Runtime` (find by key, tap, type, read state) and with
  snapshot images for the main screens: empty, one tile, three tiles with one needing the
  user, not signed in.
- Tests that start the real `claude` are few, opt-in, use the cheapest model, and start from a
  clean environment.

## 12. Order of work and risks

**Setup that needs Jerod, before anything else**

- As of 2026-11-10 no saved full claude.ai login exists on this machine: the saved login file
  holds no tokens, and the only working sign-in is the `.bashrc` token. Jerod must run
  `claude auth login` themself. Nobody else can.
- Jerod, not the builder, confirms that a test session appears at claude.ai/code.
- The Rust toolchain and system packages are not installed and no go-ahead to install them
  has been given yet. The system packages need `sudo`, so Jerod runs that command. The plan
  must open with this checkpoint. Until it clears, nothing compiles; only reading, planning
  and writing fixtures can proceed.

**Verify first, before building UI on top**

1. Remote Control turns on with a full claude.ai login and the session appears at
   claude.ai/code. This is the least certain piece. If it cannot be made to work through the
   control request, stop and ask Jerod; do not quietly drop it.
2. The permission round trip: Claude Code asks, the host answers, the tool runs or is denied.
3. Sending an image, changing the model mid-session, and stopping a turn.

**Risks**

- The control protocol is partly undocumented and can change between `claude` versions. The
  adapter should record the version from `system`/`init` and fail soft on anything unknown.
- Rune is pre-release. Pin the revision; expect to read its source.
- The build machine needs a Rust toolchain and system packages installed before anything
  compiles. Installing them needs Jerod's go-ahead.

## 13. Left to the builder

- Exact sizes, spacing, colours, icons and motion, within "feels like Claude Desktop".
- Keyboard shortcuts beyond Enter, Shift+Enter and Escape.
- How tool calls are summarised in one line, per tool.
- Crate and module layout, as long as the seam in section 7 holds.
- How long an idle process lives, and the cut-off lengths for long output.
- The format of the app's own saved file.
