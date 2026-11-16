# Rulings made while the stacked tiles plan ran

The plan is `docs/superpowers/plans/2026-11-15-stacked-tiles.md`. Jerod approved it on 2026-11-15 and it ran without stopping. Where the plan did not say, or was wrong, the controller decided. Every such decision is here, in the order it was made, with what it costs if it is wrong. The plan's own fourteen decisions are in the plan and in `docs/notes-for-later-plans.md`.

## Before the first task

1. **A test of the plan asked for more than its rule gives.** `a_tile_leaves_its_column_for_one_of_its_own` asked for exact shares in a row 1,600 points wide, where the rule (a column under 420 points is raised, as when a tile is added) changes them. The test's first case got a row 2,600 wide, and a second case pins the raise at 1,600. The rule stays. If wrong: one test.
2. **A column of two in a low window is made by stacking first.** The core refuses to make one once it knows the row is too low, so the test that draws one in a low window stacks at a tall size and then makes the window low. That is also how a user gets there. If wrong: nothing.
3. **The line's drag and the divider's drag share one function.** The plan said the new drag "follows `drag_divider` phase for phase", which invites a copy. The two share `view::tiles::drag_handle`; the divider behaves as before. If wrong: a refactor of one function.

## While the tasks ran

4. **The tiles' level type is exported as `mr_core::TileLevel`.** The plain name `Level` was taken by the usage bars' type. If wrong: a rename.
5. **Two tests the review of Task 3 asked for were written in Task 4**, which works on the same line in the same file: that an upper tile draws a line on its lower edge, and that an input box lies inside its tile in a low window. If wrong: two tests in another task's commit.
6. **Three small points from the review of Task 5 were built in Task 6**, which works in the same functions: a test helper moved to one place; the outline of a half is drawn only while its target still holds; a test that a tile that needs you keeps its amber outline while it is a drop's target. If wrong: a little more in Task 6's diff.

## After the review of the whole branch

7. **In a low window the two tiles of a column are drawn even, and elsewhere each keeps 280 points.** The review found that a line dragged far from the middle, in a window then made low, drew a tile too short for its own input box, which then lay over the tile below. What is drawn now keeps both tiles at least 280 points high while the row is at least 560, and draws them the same height in a lower row. The line as you set it is kept and comes back in a taller window. This replaces the plan's decision 2, which said both shrink in proportion. If wrong: you wanted the proportion kept in a low window; it is one function, `upper_height` in `crates/core/src/tiles.rs`.
8. **A held divider or line forgets what it remembered when the row changes.** With a divider held, a key that moved a tile and then Escape put the widths kept at the press onto another pair of tiles. Now any change of which tiles are where, the focus included, makes a later Escape put nothing back. The window's keys are not held back while a handle is held: a release that never comes would leave them dead. If wrong: a key pressed in the middle of such a drag still acts, as in plan 4.
9. **Everything the final review declined to judge stands as built.** The list is in the notes under "What the final review set aside": the middle half of a tile against its outer quarters for a drop; whether Ctrl+Alt+PageUp and PageDown are free on your desktop; a lone tile joining the column on its left by the keyboard; "Open to the side" never stacking; no new session below a tile; a double click evening the line also in a low window; the line hidden where both tiles are outlined; and four things from before this plan. If wrong: each is a small change after the look.

## After the one re-review of the fixes

There is no second wave of fixes, so these three are left, each by a ruling:

10. **A line pushed against a stop where it is already drawn saves nothing until the next save.** The release compares drawn heights, and since ruling 7 the drawn height can stay the same while the kept split changes. The next save has it, at the latest when the window closes, and the old split draws the same in that window. If wrong: after a crash a line in a low window comes back as it was before one drag.
11. **A long draft can still run out of a short tile.** Ruling 7 covers the split, not the text area, which grows to ten lines in a tile of any height. It is said under "Before looking" and left for plan 6, which adds chips to the input box and must settle its height in a short tile. If wrong: it should have been fixed before your look; the fix is a limit on the text area's lines by its tile's height.
12. **One sentence of the notes was corrected by the controller** and not by a subagent: the reason given over the list of ruling 9. The notes also got the two points of rulings 10 and 11. If wrong: nothing; it is documentation of these rulings.
