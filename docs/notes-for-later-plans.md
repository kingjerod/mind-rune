# Notes for later plans

Carried over from plan 1 (foundations and the Claude adapter), plan 2 (one session in a window), plan 3 (many sessions) and plan 4 (tiles). Everything here was found while building or reviewing those plans and deliberately left for a later plan. Read it with `docs/protocol-notes.md` before writing the next plan.

Plan 4's sections come first, then plan 3's. From "Jerod's first look" on, the file is what plan 2 left, kept as it was; where plan 3 settled something there, an added sentence in italics says so.

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

Plan 5 adds attachments, the right panel, Remote Control and running in the background. What plan 4 built that it meets:

- **The tiles are in the core.** `crates/core/src/tiles.rs` holds `Tiles`: the tiles left to right, each a session's key and a share of the row, and the focused tile. `App.tiles` is one `Tiles`, never assigned after `App::new` (`Tiles::restore` replaces it in place when the saved arrangement comes back). Two counters say what changed: `version` with every change, `placement` with every change but of widths; `App::settle` rebuilds the sidebar on `placement` only. `Place` says where a session is shown (`Focused`, `Side`, `At(i)`, `Tile(key)`); `App::show(key, place, leaving)` is the one way in, and a session already in a tile only has its tile focused. `App::focused()` is the focused tile's session. Room is `MIN_TILE_WIDTH` (420 points) per tile, against the row's width the window reports (`App::set_row_width`); `App::room_for_tile` says yes while that width is not known. `App::resize_tile`, `even_tiles`, `put_tile_pair`, `move_tile` and `close_tile` change the row. A tile is moved to an edge of the row as it is before the move, counted as `Tiles::edge_at` counts them: `Tiles::moved_index` says where it would sit, and none at its own two edges; `Tiles::move_to` moves it there with its share and focuses it, counting once on `placement`. `App::move_tile` also counts its session as seen and asks to save. A turn that ends in a tile that is not focused sets `Entry.unseen`, which is `Mark::Finished` until that tile is focused.
- **What is kept.** `Saved.tiles` holds the tiles by the agent's id with their shares, `Saved.shown` the focused one, and `Saved.names` the names the user gave that no program has confirmed yet. `App::saved` keeps a tile only when the store has its session (`Session::is_stored`: it came from the store, or a message was handed over), since a new session nothing was sent in has an id the store does not know and would hold the restore back until the complete listing; `Saved.shown` is the focused session's id when its tile is kept, else the first kept tile's id, else none. `SAVED_VERSION` stays 1; a share that cannot be read reads as 1, the share of every tile in an even row (the tile is kept), a name entry that cannot be read is left out. Shares are scaled to a mean of 1 when restored.
- **A tile's header** (`view::tiles::header`) is one row: the folder chip, the title (or its name field while it is renamed), the state word, the close button. The place for the Remote Control mark is between the state word and the close button: the title is the part that shrinks (`grow(1)`, `min_w(0)`), so a mark there takes room from the title, never from the close button. The folder chip has a fixed largest width.
- **The right panel describes `App::focused()`.** Nothing under a tile reads the focused session: every view takes the key of the session it draws, and every handler captures it. The right panel is the one place that should follow the focus; it changes when `Tiles::placement` changes. The tile focus moves on a press on a tile's plain content or header, on F6, on a field taking the keyboard, and on a control used in a tile while no field holds the keyboard; a control that takes no keyboard focus, used in another tile while the user types, leaves the focus where the user types (`glue::focus_tile`, `glue::typing_in`, `view::tiles::focus_watch`, the give-back at the end of `glue::run`).
- **Dropping files onto a tile** can reuse the drop target of the row drag: `view::drag::target_in_tiles` finds the edge or the tile under a point from `Ui.row_rect`, and `view::tiles::outline` draws a tile as the target. How Rune reports files dropped from outside the window was not looked at in this plan.
- **The limits of the drag** (`crates/app/src/view/drag.rs`). It is built by hand on the pressed row's `on_drag`: past 4 points of movement it puts `Ui.drag` in state, a chip follows the pointer in the overlay layer, and the target is worked out on every move. Rune gives no move or release when the pressed row is gone (scrolled out, its group collapsed) or a modal opens mid-press, so a watcher under the whole window counts the moves the drag did not hear and ends it at the second. The count differs from the plan's letter: Rune runs the watcher's move before the drag's own, so every move counts as missed until the drag hears it, as the kit's own grip counts. While a row is dragged, Escape only cancels the drag, the window's keys and the sidebar rows' own keys do nothing, and a menu opening ends it (`tidy_sidebar` also clears a drag whenever a menu is open). Every drop target is in the accent: the line at an edge, the outline of the tile replaced (over amber), a ring on a group's header that takes no room. While the target is in the tile row the focused tile's accent outline is not drawn (`view::tiles::outline`), so the line beside it can be seen; amber stays at an edge target. Rune runs a row's press after the release of its own drag; `Ui.swallow_row_press`, set at every release past the slop and cleared by `glue::poll`, swallows it. A tile's header drags its tile through the same code (`view::drag::header`), with `RowDrag.from` set to `DragFrom::Header`, so the slop, the chip, the line, the watcher, Escape, the keys that wait and the menus that end it are the row's. Its target is only ever an edge of the tile row (`view::drag::move_target`); the sidebar is never asked, and a tile let go there does nothing. It ends when its tile closes, sets no press to swallow, and does not start with one tile in the row or while the header's name is edited. Rune gives a press and its drag to the topmost element with a handler for either, so the header and its title both carry the handler; the close button starts none. A press on the header leaves the keyboard where it is, so a drag let go over nothing or cancelled leaves it there. The sidebar does not scroll by itself near its edges, groups cannot be reordered, and the chip is not kept inside the window.
- **Keys inside a tile.** Rune's key paths match only an unbroken run of keys (`a/b` names `b` only when `a` is its nearest keyed ancestor), so `mr-tile-7/mr-send` names nothing in a Rune lookup. A lookup Rune makes inside a tile uses a key that carries the session's number (`mr-conversation-7`, `mr-model-7`, `mr-mode-7`) or climbs the real chain of keys (`items::toggle_path`). `testkit::tile(key, inner)` paths are for the test helpers only, which resolve them themselves. The plan's Global Constraint and its Rune fact on keys were wrong on this point.
- **Focus in the window.** `glue::run` ends by handing the tile focus back to the tile whose field holds the keyboard, in the same handler, so no other tile's outline flashes. A field that leaves the screen while it has the keyboard is rebuilt with the same text and caret (`glue::let_go`), losing its undo history: Rune would otherwise take the returning field for the old one.
- **Renaming.** `App::rename` names a session at once and sends `Rename` to its program when one runs; else the name is kept in `Saved.names` and told to the next program that starts. A title the program sends later is ignored until a restart; a listing that gives the user's name confirms it, and a complete listing drops the kept names of sessions it does not give. The window has one name field, `Ui.rename`, for whatever `Ui.renaming` says (a group, a row, a header), with `glue::start_rename`, `end_rename` and `finish_rename`. Starting another rename keeps what was typed in the first, groups included. A typed name is lost by Escape, or by closing its tile while its field still has the keyboard. When the place of a session's rename goes (its row or its tile), the name is kept if its field had already lost the keyboard and no sidebar menu is open, and dropped otherwise; a group's rename is always dropped when its group is no longer listed (`glue::tidy_sidebar`).

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

- **Remote Control, seen by a person.** Turning it on works at the protocol level and returns a claude.ai link. Nobody has yet confirmed the session is listed at claude.ai/code and can be driven from there. Do this before plan 4 builds the switch.
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
