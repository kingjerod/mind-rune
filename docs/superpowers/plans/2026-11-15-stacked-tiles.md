# Stacked Tiles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A column of the tile row can hold two tiles, one above the other, and never more. A session opens above or below a tile by a drop or from the row's menu, a tile is moved into, out of and within a column by its header or by the keyboard, the line between the two is dragged, and the arrangement comes back at the next start.

**Architecture:** `mr-core`'s `Tiles` stops being a list of tiles and becomes a list of columns, each with one tile or two and the upper one's part of the height. One function, `Tiles::rects`, says where every tile is in a row of a given size; the window, the dividers and the drop targets all read it. The window stops laying tiles out with flex shares and places every tile as a direct child of the row at its rectangle, read from the row's own size inside the view (`cx.layout_size`). A tile's element therefore never changes its parent, so Rune rebuilds nothing in it when it is stacked, taken out of a stack or swapped: its conversation, scroll position, draft and keyboard focus stay.

**Tech Stack:** Rust 1.99.1 (edition 2024), `rune` and `rune-kit` at the pinned revision, `serde`, `serde_json`. No change to dependencies.

**Spec:** `docs/superpowers/specs/2026-11-10-claude-code-ui-design.md`, section 5.2 as amended on 2026-11-15. Read these before starting, in this order: the spec's section 5.2, `docs/notes-for-later-plans.md` ("What plan 5 must know" describes the tiles as plan 4 built them), `docs/superpowers/plans/2026-11-14-tiles.md` for the decisions and rulings this plan builds on (decisions 5, 6, 12 to 15 and 22), and `docs/superpowers/plans/2026-11-14-plan-4-rulings.md` (rulings R7 to R11 and 28 to 32).

**For the controller:** Jerod's rule since 2026-11-13: every subagent (implementer, reviewer, fixer) runs on Opus 5.5 at high effort or better. Nothing less, whatever the size of the task. The research reports of plan 4 are still on disk and not in git: `.superpowers/sdd/plan-4-research/rune-facts.md` and `.superpowers/sdd/plan-4-research/window-survey.md`. The rule files of plan 4's workspace (`.superpowers/sdd/2026-11-14-tiles/implementer-rules.md`, `reviewer-rules.md`, `re-review-rules.md`, `constraints.md`, `window-context.md`) hold for this plan as they are: copy them into this plan's workspace before Task 1.

## Where this plan stands

Jerod asked for it on 2026-11-15: "is there a way to tile things in multiple rows (not just columns)?", then "for stacked tiles, it should only support 2 rows". Asked which shape that means, they chose: the middle stays a row of columns, any column can hold two tiles one above the other, and a column with one tile keeps the full height.

Plans 1 to 4 are merged into `main`. This plan comes before plan 5 (right panel, Remote Control, effort level), which is written against the tiles as this plan leaves them.

**Not in this plan, on purpose.** More than two tiles in a column. A tile that spans two columns. Reordering groups by dragging. A second window. The right panel, attachments and running in the background (plans 5 to 7).

## What this window will not do yet

Say this to Jerod before their look, so the window is judged for what it is.

- A column holds two tiles at most. A third is never added to it, by any way in.
- A column is split only while the row of tiles is at least 560 points high. In a lower window the ways to stack are disabled or give what they gave before.
- A tile cannot be dragged from one window to another; there is one window.
- While a row or a header is dragged the sidebar does not scroll by itself, as before.

## Decisions in this plan that the design does not make

The first is Jerod's. Every other is the planner's call. Jerod has not seen them; they are listed in the handover for review.

1. **Two at most (Jerod, 2026-11-15).** A column holds one tile or two, one above the other.
2. **Height.** A tile of a column of two is at least 280 points high when it is opened or when the line between the two is dragged: its header, its input box and about six lines of conversation. So a column is split only while the row is at least 560 points high. When the window itself is made lower, the split is kept as it was set, but it is drawn so that neither tile is lower than 280 while the row is at least 560 high, and in a lower row the two are drawn the same height and may go below 280; none is closed (changed after the whole-branch review). This is decision 5 of plan 4, turned on its side.
3. **A new lower or upper tile takes half its column.** The line between the two drags to change that; a double click on it gives both the same height again. A double click on a divider between columns still evens the widths only.
4. **Where a dragged row lands.** Over the middle half of a tile's width: in its upper half the session opens above that tile, in its lower half below it. Over the outer quarters, near where two columns meet, a column opens at that edge as it does today. A tile whose column is full, or a row too low to split, gives the edge over its whole width, as today. When no further column fits but the tile can be split, its whole width stacks. A session takes a tile's place by a drop only when neither a column nor a split is possible there (plan 4's decision 14, kept for that case).
5. **What shows during the drag.** For a column, the line at the edge as today. For a place above or below, an outline in the accent around the half of the tile the session will take: the same outline that today marks a tile a drop would replace.
6. **"Open below"** in a sidebar row's menu, after "Open to the side": the session opens under the focused tile. It is disabled when the session is in a tile already, when the focused tile's column is full, and when the row is too low. There is no click with a modifier for it. "Open to the side", Ctrl+click and Ctrl with a folder stay exactly as they are: they open a column, and when none fits they do what they did before, also when a tile could be split instead.
7. **Moving a tile by its header** has the same targets. A tile dropped on the upper or lower half of another tile's middle joins that tile's column. A tile of a column of two dropped at an edge gets a column of its own there, when another column fits; when none fits, no line shows. A tile dropped on the middle of its own column mate swaps places with it.
8. **Moving a tile by the keyboard.** Ctrl+Shift+PageUp and Ctrl+Shift+PageDown go on moving the focused tile one column left or right; a neighbouring column of two is passed as a whole. Pressed in a column of two, they take the focused tile out into a column of its own left or right of the one it leaves, when another column fits. New: Ctrl+Alt+PageUp and Ctrl+Alt+PageDown move the focused tile up or down. In a column of two the two tiles swap. A tile alone in its column joins the column on its left, or the one on its right when it is the first, above that column's tile for PageUp and below it for PageDown; nothing happens when that column is full or the row is too low. On macOS the modifier is Cmd, as for the other keys.
9. **F6 and Shift+F6** go column by column from the left, the upper tile before the lower, around the ends.
10. **Closing** a tile of a column of two gives its room and the focus to the other one. Closing a tile that has its column to itself moves the focus to the column on its right, else on its left, to the upper tile there (plan 4's decision 6).
11. **Widths belong to columns.** A tile that joins another column gives up its width, and the other columns share it in proportion. A tile that leaves a column of two takes an equal share of the row, as a newly opened tile does. A tile that moves with its column keeps its width.
12. **Heights stay with the tiles when two swap.** After a swap each of the two is as high as before.
13. **Saving.** The arrangement is saved with its stacks at the times it is saved today, and when the line between two tiles is let go. A build from before this plan that reads such a file shows each column's upper tile only.
14. **The live test is not changed.** Stacking is a matter of the window and the core alone; nothing about it reaches Claude Code.

## Global Constraints

- Rust toolchain is exactly `1.99.1`, edition `2024`. In an agent's shell cargo is not on `PATH`: start every command with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Rune is pinned to `rev = "2a44156b21d63b0342a366b26d9222a6edd4622c"` of `https://github.com/anomalyco/rune`. Its source is at `~/.cargo/git/checkouts/rune-c9b805e9ed8a3730/2a44156`; read it instead of guessing an API. Where this plan and the source differ, the source wins: say so in your report.
- `mr-agent` and `mr-core` depend on neither `mr-claude` nor `rune`. No type from `mr-claude` appears in `mr-core` or in any view.
- The UI thread never blocks.
- Every session intent of `App` takes the `SessionKey` it acts on. No view under a tile reads "the focused session": it draws the session it is given.
- Every `App` intent a view calls is followed by `glue::run`.
- Request buttons, a tile's close button, a divider between columns and the line between two tiles of a column never take keyboard focus and never react to Enter or Space typed in an input box.
- A request is answered only in its own session.
- The software never writes inside `~/.claude`. Tests and headless runs set `RUNE_DIRS_ROOT` to a temporary directory or use `Prefs::memory()`.
- `SAVED_VERSION` stays 1. Fields added to what is saved have serde defaults, so an older file reads as before, and a value that cannot be read costs only itself: the rest of the state is still read.
- Never assign `App.sessions`, `App.groups` or `App.tiles` wholesale after `App::new`: each counts its own changes for `App::settle`.
- Every key this app gives an element starts with `mr-`. A tile's root is `mr-tile-{n}` with the session's key number, and it is a direct child of `mr-tiles`, always. Rune's key paths match only an unbroken run of keys, so a lookup Rune makes inside a tile uses a key that carries the session's number (`mr-conversation-7`, `mr-model-7`) or climbs the real chain of keys. `testkit::tile(key, inner)` paths are for the test helpers only.
- A view never does work that grows with the length of a conversation or with the number of stored sessions beyond the rows on screen.
- Tiles come, go, move and resize at once, with no slide or fade (plan 4's decision 17).
- User-visible wording is exact where this plan quotes it. Never use a real person's name or e-mail address as test data; use `alex`, `alex@example.org`, `/home/alex`. Never copy text out of a real stored session into a test or a fixture.
- All window tests and all screenshots use `FakeAgent`, through the demo window. No agent starts the real `claude`, sets `MINDRUNE_LIVE` or runs a test with `--ignored`.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` (no warnings) and `cargo test --workspace` pass before every commit. Every test that passes today still passes after every task, changed only where this plan changes what it pins; say in the report which tests you changed and why.
- **Git.** Work on the branch `stacked-tiles`, cut from `main`. Never push. Never commit `target/` or `.superpowers/`. Commit by explicit path, not `git add -A`. End every commit message with a blank line and `Co-Authored-By: Claude <noreply@anthropic.com>`, in exactly that form.

## Review Focus

The decisions imply these conditions and no screen shows them. Each has a test in the task named.

1. **The window made low while a column is split.** Two tiles in a column, then the window shrinks until the row is 400 points high. Expected: both stay, each lower than 280; nothing is closed, nothing panics, the line between them does nothing when dragged, and every way to stack is off. Tests: Task 1, Task 4, Task 5.
2. **A saved column that no longer fits.** One of its two sessions is gone, the same id is there twice or is its own lower tile, the split is 0, 1, negative or not a number, the file is from plan 4 and has no lower tiles. Expected: the app starts, keeps what fits, and a column whose upper session is gone shows the lower one alone. Tests: Task 1, Task 2.
3. **A target that goes stale during a drag.** The tile a drop would stack on closes, or its column fills, between the last move and the release. Expected: the drop does nothing, nothing stays outlined, the next click works. Tests: Task 5, Task 6.
4. **A third tile for a column, by every way in.** A drop on a full column, "Open below" under a full column, the keys with a full neighbour, a saved file. Expected: never three; each way does what the decisions say instead. Tests: Task 1, Task 5, Task 6.
5. **Nothing in a tile is lost when it changes column.** A long conversation scrolled up, a draft typed, the keyboard in the box, a request waiting, a name being edited. Expected: all as before after the tile is stacked, unstacked or swapped, and no conversation row is built again. Tests: Task 3, Task 6.

## File Structure

```
crates/core/src/tiles.rs           Task 1: Column, Under, Level, Laid, Spot, RowSize, the arithmetic
crates/core/src/app.rs             Tasks 1, 2: the app over columns, the row's height, saving and restoring
crates/core/src/saved.rs           Task 2: a column's lower tile
crates/app/src/glue.rs             Tasks 2, 4, 5: moving to a spot, the split's press, the drop targets
crates/app/src/view/tiles.rs       Tasks 3, 4, 6: the row by rectangles, the line between two tiles, the keys
crates/app/src/view/drag.rs        Tasks 5, 6: targets above and below, the outline of a half
crates/app/src/view/sidebar.rs     Task 5: "Open below"
crates/app/src/view/mod.rs         Task 6: the window's keys
crates/app/src/demo.rs             Tasks 3, 5: scenes with a column of two
crates/app/src/testkit.rs          Task 3: helpers, if any are needed
crates/app/tests/snapshots.rs      Tasks 3, 5: the new scenes
docs/                              Task 7: notes
```

## Rune facts for the window tasks

Read from Rune's source at the pinned revision. Plan 4's "Rune facts" still hold, but for its sentence on keys, which the Global Constraints above put right. Check each of these against the source before relying on it.

- **An element is what its path says it is.** A node's identity is its path: the keys of its ancestors joined by `/`, with `#n` for an ancestor that has no key (`crates/rune/src/runtime/commit/measure.rs`, `node_id`). Scroll offsets, the keyboard focus, a text selection and the measurements of a virtual list are kept by that identity. Children of one parent that change their order keep theirs; an element that moves under another parent is a new one, and everything Rune kept for it is gone. This is why every tile stays a direct child of `mr-tiles`.
- **Reading a size inside a view.** `cx.layout_size(key)` gives `[w, h]` of an element's last layout, and runs the view again in the same commit when it changed, the first layout included ("so the first frame already has the right value", `crates/rune/src/runtime/layout_read.rs`). It gives `None` only in the pass before the element was ever laid out. `view/width.rs` reads a tile's rectangle the same way with `cx.layout_with`.
- **Placing by a rectangle.** `.absolute(left, top)` takes an element out of the flow at that point of its parent's padding box; `.size(w, h)` gives it a fixed size; `.pin(left, top, right, bottom)` pins edges. There is no height as a fraction of the parent (`w_frac` exists, no `h_frac`).
- **Tried on 2026-11-15.** In a throwaway change, `view::tiles::row` gave every tile `.absolute(x, 0.0).size(w, h)` from `cx.layout_size(ROW)` and `Tiles::widths`, with the flex shares only for the pass in which the size is not known. All 329 tests of `mindrune` passed, the test that a move keeps the scroll position included, and the `tiles` snapshot looked as before. The change was thrown away; Task 3 builds it properly.
- **Cursors.** `Cursor::ResizeRow` is the pointer for a line that drags up and down, as `Cursor::ResizeColumn` is for a divider.

## Tasks

### Task 1: The row is a row of columns

The model and its arithmetic, in the core. After this task every column still holds one tile, because nothing asks for two yet: the window and the app behave as before.

**Files:**
- Modify: `crates/core/src/tiles.rs`, `crates/core/src/app.rs`, `crates/core/src/lib.rs`
- Modify, only to keep them compiling with no change of behaviour: `crates/app/src/glue.rs`, `crates/app/src/view/tiles.rs`, `crates/app/src/view/drag.rs`, `crates/app/src/demo.rs`, `crates/app/src/testkit.rs`, `crates/app/tests/live.rs`, `crates/app/tests/replay.rs`

**Interfaces:**
- Consumes: `Tiles` as plan 4 left it.
- Produces, in `crates/core/src/tiles.rs` (`Tile`, `Tiles::list`, `Tiles::index_of` and `Tiles::moved_index` are gone):
  ```rust
  /// The least a tile of a column of two is opened or dragged to, in points.
  pub const MIN_TILE_HEIGHT: f32 = 280.0;

  /// One column of the row: one tile, or two, one above the other.
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Column {
      /// Its part of the row's width against the others'; above 0.
      pub share: f32,
      /// Its tile, or the upper of its two.
      pub top: SessionKey,
      pub under: Option<Under>,
  }
  /// The lower tile of a column of two.
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Under {
      pub key: SessionKey,
      /// The upper tile's part of the column's height; above 0 and below 1.
      pub split: f32,
  }
  /// Where a tile is in its column.
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Level { Whole, Top, Bottom }
  /// One tile as laid out: `rect` is `[x, y, w, h]` from the row's top left.
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Laid { pub key: SessionKey, pub column: usize, pub level: Level, pub rect: [f32; 4] }
  /// A place in the row a tile can go to.
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Spot {
      /// A column of its own at this edge between columns, as `edge_at` counts them.
      Edge(usize),
      /// Over this tile, in its column.
      Above(SessionKey),
      /// Under this tile, in its column.
      Below(SessionKey),
  }
  /// What is known of the row's size, in points. What is not known does not hold anything back.
  #[derive(Clone, Copy, Debug, Default, PartialEq)]
  pub struct RowSize { pub width: Option<f32>, pub height: Option<f32> }
  /// Which part of a tile a point is in, for a drop.
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Part { Left, Right, Upper, Lower }
  /// The outer quarters of `rect`'s width are `Left` and `Right`; between them its upper half
  /// is `Upper` and its lower half `Lower`. `at` is in the coordinates of `rect`.
  pub fn part_at(rect: [f32; 4], at: [f32; 2]) -> Part;

  pub enum Place { Focused, Side, At(usize), Tile(SessionKey), Above(SessionKey), Below(SessionKey) }

  impl Tiles {
      pub fn columns(&self) -> &[Column];
      /// Every tile's session, column by column from the left, the upper before the lower.
      pub fn keys(&self) -> Vec<SessionKey>;
      /// How many tiles there are.
      pub fn len(&self) -> usize;
      pub fn is_empty(&self) -> bool;
      /// The column `key`'s tile is in, and where in it.
      pub fn find(&self, key: SessionKey) -> Option<(usize, Level)>;
      /// The other tile of `key`'s column, when it has two.
      pub fn mate(&self, key: SessionKey) -> Option<SessionKey>;
      /// Each column's width in a row `row_width` wide.
      pub fn widths(&self, row_width: f32) -> Vec<f32>;
      /// Every tile's rectangle in a row of `size`, in the order of `keys`.
      pub fn rects(&self, size: [f32; 2]) -> Vec<Laid>;
      /// The tile under `at`, measured from the row's top left.
      pub fn tile_at(&self, size: [f32; 2], at: [f32; 2]) -> Option<Laid>;
      pub fn edge_at(&self, row_width: f32, x: f32) -> usize;
      pub fn fits_another(&self, row_width: f32) -> bool;
      /// A column of a row this high can hold two tiles of `MIN_TILE_HEIGHT`.
      pub fn fits_two(row_height: f32) -> bool;
      /// Whether a session that is in no tile, shown at `spot`, would open a tile there.
      pub fn takes(&self, spot: Spot, row: RowSize) -> bool;
      /// Whether moving `key`'s tile to `spot` is allowed and would change the row.
      pub fn would_move(&self, key: SessionKey, spot: Spot, row: RowSize) -> bool;
      /// The upper tile's part of its column's height, when `top` is the upper of two.
      pub fn split(&self, top: SessionKey) -> Option<f32>;
      pub(crate) fn put(&mut self, key: SessionKey, place: Place, row: RowSize) -> Put;
      pub(crate) fn move_to(&mut self, key: SessionKey, spot: Spot, row: RowSize) -> bool;
      pub(crate) fn resize_split(&mut self, top: SessionKey, height: f32, row_height: f32) -> bool;
      pub(crate) fn set_split(&mut self, top: SessionKey, split: f32) -> bool;
      pub(crate) fn restore(&mut self, saved: Vec<Column>, focused: Option<SessionKey>);
      pub(crate) fn restored(saved: Vec<Column>, focused: Option<SessionKey>) -> Tiles;
      // focused, contains, version, placement, focus, remove, resize, set_pair, even: as they are.
  }
  ```

Rules:

- **Widths are the columns'.** `widths`, `edge_at`, `fits_another`, `resize`, `set_pair`, `even` and the keeping to `MIN_TILE_WIDTH` count columns, each as they counted tiles. `resize(left, ..)` and `set_pair(left, ..)` take any tile of the left column. `even` leaves every split as it is.
- **Rectangles.** `rects` gives column `i` the x range from the sum of the widths before it to that sum plus its own, so the rectangles meet with no gap, and the last column ends at `size[0]`. A column's one tile has the full height. Of two, the upper has `split * size[1]` and the lower the rest, starting where the upper ends. With a width or height that is 0 or not finite, every length along that axis is 0 and nothing panics. `tile_at` is none outside `[0, w) x [0, h)`.
- **`takes`.** `Edge`: the width is not known, or `fits_another`. `Above(t)` and `Below(t)`: `t` is in a tile that has its column to itself, and the height is not known or `fits_two`.
- **`would_move`**, for the tile of `key`, with every edge past the number of columns counted as that number:
  - `Edge(e)`, the tile alone in column `c`: true unless `e` is `c` or `c + 1`.
  - `Edge(e)`, the tile one of two: true when the width is not known or `fits_another`: it gets a column of its own at any edge, the two beside its old column included.
  - `Above(t)` or `Below(t)`, `t` its column mate: true when the order would change (`Above` for the lower tile, `Below` for the upper).
  - `Above(t)` or `Below(t)` otherwise: `t` is another tile that has its column to itself, and the height is not known or `fits_two`.
  - False for a session with no tile, for `t == key`, and for a `t` with no tile.
- **`move_to`** does nothing and returns false when `would_move` is false. Else the tile leaves its place: a column it had to itself goes, and a column mate it leaves has the column to itself with the column's share. To `Edge(e)`: a tile that had its column keeps that share and goes in as today (at `e` when `e` is at or left of its old column, else `e - 1`); a tile out of a column of two gets the mean of the columns' shares, goes in at `e`, and the columns are then kept to `MIN_TILE_WIDTH` as when a tile is added. To `Above(t)` or `Below(t)`: it is the upper or lower tile of `t`'s column with a split of 0.5, unless the two were mates already: then they swap and the split becomes `1 - split`, so each keeps its height. The moved tile takes the focus. One real move counts once on `placement` and on `version`.
- **`put`.** A session already in a tile only has it focused, as today. `Focused`, `Side`, `At` and `Tile` do what they did, counting columns: `Side` opens a column right of the focused tile's column. `Above(t)` and `Below(t)` open a tile there with a split of 0.5 when `takes` says so, and do nothing otherwise (`Put::Nothing`). `Tile(old)` and `Focused` put the session in that tile's place, upper or lower, and leave every size.
- **`remove`.** Out of a column of two: the other has the column, and the focus if the removed tile had it. A tile with its column to itself: the column goes, and the focus goes to the upper tile of the column now at its index, else of the one before.
- **The split.** `resize_split(top, height, row_height)` makes the upper tile `height` high: clamped so that both are at least `MIN_TILE_HEIGHT`; false, and nothing changed, when `top` is not the upper of two, the height is not a number, or `row_height` is not usable or below twice `MIN_TILE_HEIGHT`. `set_split` puts a split back exactly (it is how a cancelled drag undoes itself and how a double click evens: 0.5). Both count on `version` only, as widths do.
- **`restored`.** Shares as today. A key met before is left out: when it is a column's upper tile and the column has a lower one, the lower one has the column; when it is the lower one, or the lower one is the upper one again, the column has its upper tile alone. A split that is not finite, or not above 0 and below 1, is 0.5. `restore` scales the columns' shares to a mean of 1 as today.
- **The callers.** `App` keeps `row_width: Option<f32>` in this task and passes `RowSize { width: self.row_width, height: None }`. `App::move_tile(key, edge)` keeps its signature and moves to `Spot::Edge(edge)`. `App::saved` and `restore_tiles` map a `SavedTile` to and from a column with no lower tile. Every other caller of what is gone uses the nearest of `columns`, `keys`, `len`, `find`, `would_move(key, Spot::Edge(edge), ..)`, with no change of behaviour: say in the report what each became. A `match` over `Place` in the window (`glue::leaving`) treats `Above` and `Below` as it treats `At`: no session leaves a tile.

What this task is most likely to get wrong, so the tests pin it: an edge index after a tile left a column of two (no column went, so nothing shifts); the share of a column whose tile joined another column (it is dropped, not handed to a neighbour); the focus after a removal; rectangles that leave a hair of a gap at the right or bottom edge.

- [ ] **Step 1: Write the failing tests** in `crates/core/src/tiles.rs`. The helpers `row`, `keys`, `shares` and `assert_widths` stay, over columns; add `stack(tiles, top, under, split)` for arranging a column of two. A row is 1,600 by 900 unless said.
  - Every test there today, over columns, pinning what it pinned.
  - `a_tile_opens_under_another`: `[A][B]`, `put(C, Place::Below(B), ..)` is `Put::Added`: `[A][B/C]`, split 0.5, C focused, `placement` one higher. `a_tile_opens_over_another`: `Place::Above(A)` gives `[C/A][B]`.
  - `a_column_never_holds_three`: with `[A/B]`, `put(C, Place::Below(A))`, `put(C, Place::Above(B))` and `put(C, Place::Below(B))` are `Put::Nothing`, and `takes` is false for each.
  - `a_low_row_is_not_split`: height 559 gives `Put::Nothing`; height 560 and a height not known give `Put::Added`.
  - `stacking_needs_no_room_for_a_column`: `[A][B]` in a row 900 wide: `put(C, Place::At(1))` is `Put::Nothing`, `put(C, Place::Below(A))` is `Put::Added`.
  - `rects_tile_the_row`: `[A share 1][B/C share 3, split 0.25]` in 1,600 by 800: A `[0, 0, 400, 800]` `Whole` in column 0; B `[400, 0, 1200, 200]` `Top` and C `[400, 200, 1200, 600]` `Bottom` in column 1. For shares `[1, 1, 1]` in a row 1,000 wide, the last rectangle ends at exactly 1,000.
  - `rects_without_a_size_are_empty`: sizes `[0, 900]`, `[1600, 0]`, `[NaN, NaN]`: no panic, zero lengths along that axis.
  - `the_tile_under_a_point`: in the row of `rects_tile_the_row`: (10, 10) is A, (500, 100) B, (500, 300) C, (399.9, 799.9) A; (1600, 10), (-1, 5) and (10, 800) none.
  - `the_parts_of_a_tile`: for `[400, 0, 1200, 800]`: x 500 is `Left`, x 1,500 `Right`, (1000, 399) `Upper`, (1000, 400) `Lower`, x 700 (the quarter line) not `Left`.
  - `keys_go_column_by_column`: `[A][B/C][D]` gives A, B, C, D; `len` 4; `find(C)` is `(1, Level::Bottom)`; `mate(B)` is C, `mate(A)` none.
  - `closing_one_of_two_gives_the_other_the_column`: `[A][B/C]`, C focused: `remove(C)` gives `[A][B]` with B focused and the column's share as before; the same for the upper tile.
  - `closing_a_column_moves_the_focus_right_then_left`: `[A][B/C][D]`: removing focused A focuses B; removing focused D focuses B.
  - `a_tile_leaves_its_column_for_one_of_its_own`: `[A share 1][B/C share 3]`: in a row 2,600 wide (so that no column is under 420 points and none is raised), `move_to(C, Spot::Edge(0), ..)` gives `[C][A][B]`, C with the mean share 2, A with 1, B with 3, C focused. In a row 1,600 wide the same move also raises A to 420 points, as a tile added there would be. In a row 900 wide `would_move` is false and nothing changes. `Edge(1)` and `Edge(2)` (beside its old column) both move it.
  - `a_tile_joins_another_column`: `[A][B][C]` shares 1, 2, 3: `move_to(A, Spot::Below(C), ..)` gives `[B share 2][C/A share 3]`, split 0.5, A focused.
  - `the_two_of_a_column_swap`: `[A/B]` split 0.25: `move_to(A, Spot::Below(B), ..)` gives `[B/A]` split 0.75; again is false. `move_to(A, Spot::Above(B), ..)` then gives `[A/B]` split 0.25.
  - `a_full_column_takes_no_tile`: `[A][B/C]`: `would_move(A, Spot::Above(B))` and `(A, Spot::Below(C))` are false.
  - `a_column_passes_a_column_of_two`: `[A][B/C][D]`: `move_to(A, Spot::Edge(2), ..)` gives `[B/C][A][D]`.
  - `moves_of_nothing_change_nothing`: own edges, `Above` and `Below` itself, a spot naming a session with no tile: order, focus, `version` and `placement` as before.
  - `the_split_is_dragged`: `[A/B]` in height 900: `resize_split(A, 300, 900)` gives split 1/3; 100 clamps to 280; 800 clamps to 620; `version` goes up and `placement` does not.
  - `a_low_column_does_not_drag`: height 500 is false; a height of NaN is false; `resize_split(B, ..)` for the lower tile is false.
  - `set_split_puts_a_split_back`, `evening_out_leaves_the_splits`.
  - `a_divider_moves_width_between_two_columns_of_any_shape`: `[A/B][C]`: `resize(B, ..)` and `resize(A, ..)` do the same.
  - `a_saved_row_with_columns_of_two_is_put_right`: one case for each sentence of the `restored` rule.
  - `every_change_counts`, with a put above, a move into a column, a move out and a swap each counting once on `placement`.
  - `takes_and_would_move_say_what_put_and_move_to_do`: for `[A][B/C][D]` and a new session E, every spot: `takes` is true exactly when `put` adds, and `would_move(k, spot)` exactly when `move_to` returns true, for every tile `k`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement** the model and the rules above; update the callers.
- [ ] **Step 4: Run** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`. Every test outside `tiles.rs` passes unchanged but for the names of what is gone.
- [ ] **Step 5: Commit** `feat(core): the row of tiles is a row of columns`.

### Task 2: The app stacks tiles and keeps the stacks

**Files:**
- Modify: `crates/core/src/app.rs`, `crates/core/src/saved.rs`
- Modify, to follow the changed signature: `crates/app/src/glue.rs`, `crates/app/src/view/tiles.rs`, `crates/app/src/view/drag.rs`

**Interfaces:**
- Consumes: Task 1.
- Produces:
  ```rust
  // core/src/app.rs
  /// The row's height, as the window last laid it out.
  pub fn set_row_height(&mut self, height: f32);
  pub fn row_height(&self) -> Option<f32>;
  /// What is known of the row's size.
  pub fn row_size(&self) -> RowSize;
  /// Whether a session that is in no tile could be shown at `spot` now (`Tiles::takes`).
  pub fn can_put(&self, spot: Spot) -> bool;
  /// Whether `key`'s tile could be moved to `spot` now (`Tiles::would_move`).
  pub fn can_move_tile(&self, key: SessionKey, spot: Spot) -> bool;
  /// Moves `key`'s tile to `spot`; it takes the focus and its session counts as seen. Asks
  /// to save. Nothing, and no effect, when nothing moved.
  pub fn move_tile(&mut self, key: SessionKey, spot: Spot) -> Vec<Effect>;
  /// Makes `top`'s tile `height` points high against the tile under it. Asks for no save.
  pub fn resize_split(&mut self, top: SessionKey, height: f32);
  /// Puts a split back as `tile_split` read it. Asks for no save.
  pub fn put_split(&mut self, top: SessionKey, split: f32);
  pub fn tile_split(&self, top: SessionKey) -> Option<f32>;
  /// Gives the two tiles of `top`'s column the same height; asks for a save when that
  /// changed anything.
  pub fn even_split(&mut self, top: SessionKey) -> Vec<Effect>;
  // core/src/saved.rs
  pub struct SavedTile { pub id: SessionId, pub share: f32, pub below: Option<SavedBelow> }
  /// The lower tile of a column as kept between runs.
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct SavedBelow {
      pub id: SessionId,
      /// The upper tile's part of the column's height. Missing or not a number reads as 0.5.
      pub split: f32,
  }
  // app/src/glue.rs
  pub fn move_tile(ui: &mut Ui, tx: &mut Tx, key: SessionKey, spot: Spot, typing: bool);
  ```

Rules:

- `App` holds a `RowSize` in place of `row_width`. `set_row_width` and `row_width` stay as they are for their callers. Nothing happens to the tiles when the height changes.
- `App::show` with `Place::Above` or `Place::Below` asks to save when a tile opened, as it does for a tile opened to the side. Nothing changes and no effect comes back when the place takes no tile.
- `App::resize_split` does nothing while the row's height is not known.
- **Saving.** `Saved.tiles` holds one entry for each column: its upper tile's id, the column's share, and `below` with the lower tile's id and the split. A tile whose session the store does not have is left out as today (`Session::is_stored`): when that is one of two, the other is saved as a column of one. `Saved.shown` is the focused session's id when its tile is kept, else the first kept column's upper tile.
- **The sample.** `saved.rs` compares its sample with an exact string (`SAMPLE_JSON`): the string changes with the new field, and the sample gets one column of two.
- **Reading.** `below` has a serde default of none, and a `below` that cannot be read (not an object, no id) reads as none; a `split` that is missing or not a number reads as 0.5. The rest of the state is always still read.
- **Restoring.** As today, with every lower tile counting among the tiles to wait for. A column whose upper session is not known after the complete listing shows its lower one alone; one whose lower session is not known shows the upper alone. `Tiles::restore` puts the rest right.
- `glue::move_tile` and its callers pass `Spot::Edge(edge)` where they passed an edge; nothing they do changes in this task.

- [ ] **Step 1: Write the failing tests.**
  In `crates/core/src/app.rs`:
  - `a_session_opens_under_the_focused_tile`: `show(key, Place::Below(focused), ..)` opens it there, focuses it and asks for a save.
  - `no_tile_opens_under_a_tile_of_a_low_row`: after `set_row_height(500.0)`: no effect, nothing changed, `can_put(Spot::Below(focused))` false.
  - `can_put_and_can_move_say_what_show_and_move_do`.
  - `moving_a_tile_into_a_column_asks_for_a_save`, `a_tile_moved_into_a_column_counts_as_seen`.
  - `closing_the_focused_lower_tile_focuses_the_upper`, and its session counts as seen.
  - `the_split_is_resized_without_a_save_and_evened_with_one`; `evening_an_even_split_asks_for_nothing`.
  - `a_column_of_two_is_saved_with_its_split`.
  - `a_column_whose_upper_session_is_not_stored_saves_the_lower_alone`, and the other way round.
  - `a_saved_column_of_two_comes_back`: the same columns, split and focus, with no program started.
  - `a_saved_column_with_one_session_gone_comes_back_as_one_tile`.
  - `restoring_waits_for_a_lower_tiles_session`: not known yet and the listing not complete: nothing is restored yet.
  In `crates/core/src/saved.rs`:
  - `a_tile_saved_before_stacks_reads_as_a_column_of_one`: the JSON plan 4 writes.
  - `a_below_that_cannot_be_read_is_left_out`: `"below": 5`, `"below": {"split": 0.3}`: the tile and the rest of the state are read.
  - `a_split_that_is_not_a_number_reads_as_a_half`: missing, `null`, a string.
  - `a_column_of_two_survives_a_round_trip`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(core): tiles are stacked, saved and restored in columns`.

### Task 3: The window places every tile by its rectangle

After this task a column of two is drawn. Nothing in the window makes one yet: the tests and the scene arrange it through the core.

**Files:**
- Modify: `crates/app/src/view/tiles.rs`, `crates/app/src/demo.rs`, `crates/app/tests/snapshots.rs`; `crates/app/src/testkit.rs` if a helper is needed

**Interfaces:**
- Consumes: `Tiles::rects`, `Tiles::keys`, `App::set_row_height`, `cx.layout_size`.
- Produces: `view::tiles::row` as below; the demo scene `tiles-stacked`. No new public function.

Rules:

- **The row.** `row` reads `cx.layout_size(ROW)`. With a size, every tile is a direct child of the row, in the order of `Tiles::keys`, placed with `.absolute(x, y).size(w, h)` at its rectangle from `Tiles::rects`. Without one (the pass before the row was ever laid out), the tiles are laid out by the columns' shares as today, the two of a column side by side: that pass is never drawn. Nothing else about a tile's element changes: its key, its press, its role, its header, its pane, its outline.
- **Lines.** Every tile that is not in the last column draws the line on its right edge, as today. The upper tile of two draws the same line on its lower edge.
- **Dividers.** One over each edge between two columns, the full height of the row, keyed `mr-divider-{n}` with the number of the left column's upper tile. They are built from the same size the tiles are placed from.
- **What the row reports.** `laid_out` gives the app the row's height as it gives its width, each when it changed.
- **Nothing in a tile is rebuilt** when it is stacked, taken out or swapped: this is what placing by rectangles is for, and Review Focus 5 is tested here through `glue::move_tile`.
- **The scene `tiles-stacked`**, for a window of 1,800 by 900: the `tiles` scene with its third tile moved under its second, the second's permission request still waiting (amber), the lower tile focused. Add it to `SCENES` in `tests/snapshots.rs` at `WIDE`.

What this task is most likely to get wrong: a frame in which the tiles are drawn from a stale size after the window is resized (the read must be the view's own, not `Ui.row_rect`); a tile's content not filling a fixed-size absolute element (the pane must still stretch and its scroll area must still have a bounded height); the outline or a picker's list placed from the tile's old position.

- [ ] **Step 1: Write the failing tests** in `crates/app/src/view/tiles.rs`, in a window of 1,800 by 900 unless said:
  - `a_stacked_tile_is_drawn_under_its_column_mate`: both have the same x and width; the upper's bottom is the lower's top; their heights add up to the row's.
  - `tiles_fill_the_row`: `[A][B/C][D]`: the four rectangles cover the row's rectangle with no gap and no overlap above half a point.
  - `the_first_frame_places_the_tiles`: right after the window is built, with no further frame, each tile is presented at its rectangle.
  - `a_resized_window_places_the_tiles_again`: after the window is made 1,400 by 700 every tile is at its rectangle for the new row in the next frame drawn, and the app knows the new height.
  - `stacking_keeps_the_scroll_position_and_rebuilds_no_row`: a long conversation wheeled up in A; A moved under B with `glue::move_tile`; A's conversation shows the same rows at the same offset from its top (as `a_move_keeps_the_scroll_position` measures; step frames, do not call `rt.settle_all()`, which lands a follow-the-end), and no conversation row was built again.
  - `a_draft_and_the_keyboard_stay_when_a_tile_is_stacked`, `..is_taken_out_of_its_column`, `..swaps_with_its_mate`: a character typed afterwards lands in the same box.
  - `a_name_being_edited_goes_on_when_its_tile_is_stacked`.
  - `the_outline_is_on_the_tile_it_marks`: the focused lower tile's outline has that tile's rectangle; the amber of the upper one has its own.
  - `a_press_on_a_stacked_tile_focuses_it`, with the keyboard in its box.
  - `a_lower_tiles_pickers_open_inside_the_window`: the model picker's list of a lower tile lies inside the window.
  - `a_request_in_a_stacked_tile_is_answered_in_its_own_session`: both tiles of a column hold a request with the same id.
  - `a_divider_between_columns_is_as_high_as_the_row`, and dragging it resizes a column of two as one.
  - `a_column_of_two_in_a_low_window_is_still_drawn`: a row 400 points high: both tiles drawn, each under 280, nothing panics.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: The scene.** Add `tiles-stacked`. Make its screenshot in both themes under `target/snap/stack/` and look at both: the three headers, the amber outline on the upper tile, the accent outline on the lower, the line between the two, both input boxes whole.
- [ ] **Step 5: Run** the three checks.
- [ ] **Step 6: Commit** `feat(app): every tile is placed by its rectangle`.

### Task 4: The line between two tiles is dragged

**Files:**
- Modify: `crates/app/src/view/tiles.rs`, `crates/app/src/glue.rs`

**Interfaces:**
- Consumes: `App::resize_split`, `put_split`, `tile_split`, `even_split`, `save_now`; `divider` and `drag_divider` as the model to follow.
- Produces:
  ```rust
  // app/src/glue.rs, in Ui
  /// The split of the column whose line is being dragged, as it was at the press.
  pub split_press: Option<f32>,
  // app/src/view/tiles.rs
  /// `mr-split-{n}`: the line under the tile of session `n`, the upper of two.
  fn split_key(top: SessionKey) -> String;
  ```

Rules:

- A handle 8 points high over the line between the two tiles of a column, as wide as the column, above the tiles, taking no room: role "separator", label "Tile height", `Cursor::ResizeRow`, no Tab stop, and a press on it moves neither the keyboard focus nor the tile focus (`requests::unfocusable`, as a divider).
- A drag follows `drag_divider` phase for phase: the press keeps the upper tile's height and the split; each move makes the upper tile that height plus the pointer's travel down, at once, and writes nothing; the release saves once when the height ended up different; a cancelled drag (Escape, the pointer lost) puts the split back exactly.
- A double press gives both tiles the same height and saves when that changed anything.
- Where the line meets a divider between columns, the divider has the press.
- In a row too low for two tiles of 280 the drag does nothing; a press before the row's height is known does nothing.

- [ ] **Step 1: Write the failing tests** in `crates/app/src/view/tiles.rs`, with `[A][B/C]` in a window of 1,800 by 900:
  - `the_line_between_two_tiles_drags`: dragged 100 points down, B is 100 higher and C 100 lower; the state is saved once, at the release.
  - `a_line_dragged_back_saves_nothing`.
  - `the_line_stops_at_the_least_height`: dragged far up, B is 280 high; far down, C is.
  - `escape_puts_the_line_back`, also when the split was not 0.5 before.
  - `a_double_press_on_the_line_evens_the_two`, and a second one saves nothing.
  - `the_line_takes_neither_focus`: the keyboard stays in A's box and A stays the focused tile through a drag.
  - `where_the_line_meets_a_divider_the_divider_drags`.
  - `the_line_of_a_low_row_does_nothing`: a window 500 high with a restored column of two.
  - `a_column_of_one_has_no_line`.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): the line between two tiles of a column is dragged`.

### Task 5: A session opens above or below a tile

**Files:**
- Modify: `crates/app/src/glue.rs`, `crates/app/src/view/drag.rs`, `crates/app/src/view/tiles.rs`, `crates/app/src/view/sidebar.rs`, `crates/app/src/demo.rs`, `crates/app/tests/snapshots.rs`

**Interfaces:**
- Consumes: `Tiles::tile_at`, `part_at`, `App::can_put`, `App::room_for_tile`, `Place::Above`, `Place::Below`, `drag::target_in_tiles`, `drag::overlay`, `tiles::outline`.
- Produces:
  ```rust
  // app/src/glue.rs
  pub enum DropTarget {
      Edge(usize),
      Tile(SessionKey),
      Group(GroupId),
      /// Over this tile, in its column: a new tile there for a row's drag, the dragged tile
      /// there for a header's.
      Above(SessionKey),
      /// Under this tile, in its column.
      Below(SessionKey),
  }
  // app/src/view/drag.rs
  /// The outline of the half a dropped tile will take.
  const HALF: &str = "mr-drop-half";
  ```
  The sidebar row's menu reads "Move to group", "Open to the side", "Open below", "Rename". Demo scene: `tiles-stack-drag`.

Rules:

- **The target of a row's drag** (`target_in_tiles`), for a session that is in no tile and a pointer inside the row, with `t` the tile under the pointer (`Tiles::tile_at` on the row as last laid out) and `half` being `Above(t)` when the pointer is in the upper half of `t`'s rectangle, else `Below(t)`:
  1. `t` has its column to itself, `App::can_put(Spot::Below(t))`, and the pointer is in the `Upper` or `Lower` part of `t` (`part_at`): `half`.
  2. Else, when `App::room_for_tile()`: `Edge` at the nearest edge between columns, as today.
  3. Else, when `t` has its column to itself and `App::can_put(Spot::Below(t))`: `half`.
  4. Else: `Tile(t)`, as today.
  A session that is in a tile has no target, as today.
- **The drop.** `Above(t)` shows the session at `Place::Above(t)`, `Below(t)` at `Place::Below(t)`, through `glue::show` with the keyboard in its box, as an edge's drop does. When the place takes no tile any more (the tile closed, its column filled), nothing happens.
- **What shows.** For `Above(t)` and `Below(t)`: an outline 2 points wide in the accent, keyed `mr-drop-half`, in the overlay layer, around the upper or lower half of `t`'s rectangle as last laid out; inert, at once, no line at any edge. Ruling R11 holds for these targets too: while one is shown the focused tile's accent outline is not drawn, and amber stays.
- **"Open below".** In a row's menu after "Open to the side". Disabled when the session is in a tile, when no tile is focused, and when `App::can_put(Spot::Below(focused))` is false. It shows the session at `Place::Below(focused)` with the keyboard in its box. "Open to the side" is unchanged.
- Everything that guards or ends a row's drag is unchanged and holds for these targets.
- **The scene `tiles-stack-drag`**, for a window of 1,800 by 900: as `tiles-drag`, with the pointer over the lower half of the first tile's middle and the outline around that half. Add it to `SCENES` at `WIDE`.

- [ ] **Step 1: Write the failing tests.**
  In `crates/app/src/view/drag.rs`, in a window of 1,800 by 900 with two tiles `[A][B]` and a third session's row dragged, unless said:
  - `a_row_dropped_on_the_lower_half_of_a_tile_opens_under_it`: `[A/C][B]`, C focused, the keyboard in C's box, the state saved.
  - `a_row_dropped_on_the_upper_half_of_a_tile_opens_over_it`.
  - `near_where_two_tiles_meet_a_row_still_opens_a_column`: in A's right quarter, at any height: the line at edge 1, and the drop gives `[A][C][B]`.
  - `the_half_a_tile_will_take_is_outlined`: `mr-drop-half` has the lower half of A's rectangle, no `mr-drop-edge` is shown, the focused tile's accent outline is not drawn; after the release nothing is left.
  - `a_full_column_gives_the_edge`: over the middle of `[A/C]`, with room: the line, as today.
  - `with_no_room_and_no_split_a_tile_is_replaced`: a window 1,300 wide with `[A/C][B/D]`: over D, `Tile(D)`.
  - `with_no_room_for_a_column_the_whole_tile_stacks`: a window 1,300 wide with `[A][B]`: in A's left quarter, upper half: `Above(A)`.
  - `a_low_window_stacks_nothing`: 1,800 by 500: the middle of A gives the edge.
  - `a_session_in_a_tile_has_no_target_over_a_half`, and its drop still focuses its tile.
  - `a_drop_on_a_tile_that_closed_meanwhile_does_nothing`: no tile opens, nothing stays outlined, the next click on a row works.
  - `escape_cancels_a_drop_on_a_half`.
  - `dragging_over_the_halves_rebuilds_no_conversation_row`.
  In `crates/app/src/view/sidebar.rs`:
  - `the_menu_opens_a_session_below_the_focused_tile`.
  - `open_below_is_disabled_when_it_cannot_be`: the session in a tile; the focused column full; a window 500 high; no tile at all.
  - The tests that pin the menu's entries, with "Open below" in its place.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: The scene.** Add `tiles-stack-drag`. Make its screenshot in both themes under `target/snap/stack/` and look at both: the outline must read as "the lower half of this tile" at a glance, also beside an amber outline.
- [ ] **Step 5: Run** the three checks.
- [ ] **Step 6: Commit** `feat(app): a session opens above or below a tile`.

### Task 6: A tile moves into, out of and within a column

**Files:**
- Modify: `crates/app/src/view/drag.rs`, `crates/app/src/view/tiles.rs`, `crates/app/src/view/mod.rs`

**Interfaces:**
- Consumes: `App::can_move_tile`, `glue::move_tile`, `Tiles::mate`, `Tiles::find`, `drag::move_target`, `tiles::step_move`, `tiles::step_focus`, `view::on_key`, `drag_key`.
- Produces:
  ```rust
  // app/src/view/tiles.rs
  /// Which way a key moves the focused tile in its column: Ctrl+Alt+PageUp up (-1),
  /// Ctrl+Alt+PageDown down (1). Cmd+Alt on macOS, with no Ctrl there.
  pub fn stack_step(ev: &KeyEvent) -> Option<isize>;
  /// Moves the focused tile up or down as decision 8 says. False when nothing moved.
  pub fn step_stack(ui: &mut Ui, tx: &mut Tx, step: isize) -> bool;
  ```

Rules:

- **The target of a header's drag** (`move_target`), for the tile of `key` and a pointer inside the row, with `t` the tile under the pointer:
  1. `t` is another tile and the pointer is in its `Upper` or `Lower` part: when `t` is `key`'s column mate, the spot that swaps the two (`Above(t)` for the lower tile, `Below(t)` for the upper); else `Above(t)` or `Below(t)` by the half of `t`'s rectangle the pointer is in. When `App::can_move_tile(key, spot)`: that spot.
  2. Else `Edge` at the nearest edge between columns, when `App::can_move_tile(key, Spot::Edge(edge))`.
  3. Else none.
  So over its own tile a tile that has its column shows nothing, as today; a tile of a column of two shows the line at the nearer edge of its column when another column fits.
- **The release** over `Edge`, `Above` or `Below` moves the tile there with `glue::move_tile(.., true)`. Everything else about a header's drag is as Task 15 of plan 4 left it: when it starts, what guards it, what ends it, that it sets no press to swallow.
- **What shows.** The line for an edge. For `Above(t)` and `Below(t)`, `mr-drop-half` around the place the tile will take: the half of `t`'s rectangle, or all of `t`'s rectangle when the two are mates.
- **`step_move`.** The focused tile alone in its column moves one column left or right, past a column of two as a whole; at the end of the row nothing moves. One of two moves to `Spot::Edge` at the left (`-1`) or right (`+1`) edge of its own column; when no further column fits nothing moves. The row does not wrap; the keyboard stays where it is.
- **`step_stack`.** One of two: the upper tile moves down and the lower moves up, by the swap; the other direction does nothing. Alone in column `c`: it joins the column at `c - 1`, or at `c + 1` when `c` is 0, `Above` that column's tile for `-1` and `Below` it for `1`; nothing when there is no such column, it is full, or the row is too low. The keyboard stays where it is (`glue::move_tile(.., false)`).
- **The keys** wait as the move keys wait: only on `Screen::Main`, not while a list or a menu is open, not while a row or a header is dragged; on `Screen::Main` the key is taken also when nothing moved. Ctrl+Alt+PageUp and PageDown must not also run what Ctrl+Shift+PageUp and PageDown run: `move_step` already refuses Alt.
- **`step_focus`** goes through `Tiles::keys`.

- [ ] **Step 1: Write the failing tests.**
  In `crates/app/src/view/drag.rs`, in a window of 1,800 by 900:
  - `a_header_dropped_on_the_lower_half_of_another_tile_stacks_it_there`: `[A][B][C]`, A's header let go over the lower middle of C: `[B][C/A]`, A focused with the keyboard, saved.
  - `a_tile_of_two_dragged_to_an_edge_gets_a_column`: `[A/B][C]`, B's header let go at the right end: `[A][C][B]`.
  - `with_no_room_a_tile_of_two_has_no_edge`: a window 1,300 wide with `[A/B][C]`: no line anywhere for B; over C's middle the outline shows and the drop gives `[A][C/B]` or `[A][B/C]` by the half.
  - `dropped_on_its_mate_a_tile_swaps_and_both_keep_their_heights`.
  - `over_its_own_tile_a_whole_tile_shows_nothing`.
  - `along_the_headers_edges_and_halves_both_show`: A's header moved level with the headers across `[A][B][C]`: over B's left quarter nothing (its own edge), over B's middle the outline around B's upper half, over B's right quarter the line at edge 2.
  - `a_move_into_a_column_keeps_what_is_in_the_tile`: draft, keyboard, scroll position, and no conversation row built again.
  - `a_tile_whose_target_closed_during_the_drag_is_not_moved`.
  - `escape_cancels_a_move_onto_a_half`.
  In `crates/app/src/view/tiles.rs`:
  - `ctrl_shift_page_down_passes_a_column_of_two`: `[A][B/C]`, A focused: `[B/C][A]`.
  - `ctrl_shift_page_down_takes_a_tile_out_of_its_column`: `[A/B]`, B focused: `[A][B]`; PageUp gives `[B][A]`.
  - `with_no_room_the_key_is_taken_and_nothing_moves`.
  - `ctrl_alt_page_down_puts_a_tile_under_its_left_neighbour`: `[A][B]`, B focused: `[A/B]`. `ctrl_alt_page_up_puts_it_over`: `[B/A]`.
  - `the_first_column_joins_the_second`: `[A][B]`, A focused, PageDown: `[B/A]`.
  - `in_a_column_of_two_the_keys_swap`: the upper with PageDown, the lower with PageUp; the upper with PageUp and the lower with PageDown do nothing.
  - `a_full_neighbour_takes_nothing`, `a_low_window_stacks_nothing_by_key`, `one_tile_alone_does_nothing`.
  - `the_keyboard_stays_in_the_box_when_its_tile_is_stacked_by_key`.
  - `the_stack_keys_wait`: while a menu is open, while a row is dragged, on another screen.
  - `f6_goes_column_by_column_the_upper_first`, and Shift+F6 back.
- [ ] **Step 2: Run them; they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three checks.
- [ ] **Step 5: Commit** `feat(app): a tile moves into, out of and within a column`.

### Task 7: Notes and Jerod's look

**Files:**
- Modify: `docs/notes-for-later-plans.md`

Rules:

- A new first section, "Jerod's look at stacked tiles", in the shape of "Jerod's look at plan 4": the command (`cargo run --release -p mindrune`; no live test is needed), "Before looking" from "What this window will not do yet" above, the things to try below, each ending "Seen: not yet recorded.", then every decision of this plan with what was built, then whatever the tasks left.
  1. A session dragged onto the lower half of a tile opens under it; onto the upper half, over it; near where two tiles meet, it still opens a column.
  2. "Open below" in a row's menu opens a session under the focused tile.
  3. The line between two tiles drags, stops at a sensible height, and evens out on a double click.
  4. A tile dragged by its header onto another tile joins its column; dragged to an edge it gets a column of its own; dropped on its mate the two swap.
  5. Ctrl+Shift+PageUp and PageDown, and Ctrl+Alt+PageUp and PageDown, move the focused tile as said; F6 goes through the tiles in reading order.
  6. Closing one of two gives the other the column.
  7. Close and open the app: the same columns, the same heights, the same focus.
  8. On the ultrawide: how many tiles are comfortable now, and is a half-height tile enough to work in?
- "What plan 5 must know" says what changed: the tiles are columns (`Tiles::columns`, `keys`, `rects`, `Spot`, `RowSize`), every tile is a direct child of `mr-tiles` placed by its rectangle and why, the row's height is known to the app, a file dropped on a tile can find its tile with `Tiles::tile_at`, and the keys that are now taken.
- Nothing else in the file changes.

- [ ] **Step 1: Write the notes.**
- [ ] **Step 2: Run** the three checks.
- [ ] **Step 3: Commit** `docs: stacked tiles, in the notes`.
