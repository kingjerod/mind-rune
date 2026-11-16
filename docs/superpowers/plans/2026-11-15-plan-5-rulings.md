# Rulings made while plan 5 ran

The plan is `docs/superpowers/plans/2026-11-15-panel-remote-effort.md`. Jerod approved it on 2026-11-15 and it ran without stopping. Where the plan did not say, or was wrong, the controller decided. Every such decision is here, in the order it was made, with what it costs if it is wrong. The plan's own seventeen decisions are in the plan and in `docs/notes-for-later-plans.md`.

## Before the first task

1. **The panel must leave nothing a frame behind.** The panel takes room from the row of tiles, and the stacked tiles run measured that such a frame already uses both of Rune's re-run passes. The plan had no test for it. Task 7 got one more test (`opening_the_panel_leaves_nothing_a_frame_behind`), and the panel makes no layout read that depends on a tile. If wrong: one test.
2. **The session focused when the app starts gets its branch read.** The plan said "when the focus comes to a session" and did not say whether the tile restored at start counts. It does. If wrong: one file read at start.
3. **The run used the stacked tiles run's rule files**, with the branch name changed. The plan named plan 4's, which are the same without that run's corrections. If wrong: nothing.
4. **Where a research report differs from the code, the code wins.** Both reports were written before stacked tiles were built. If wrong: nothing.

## While the tasks ran

5. **The effort level is said only when it is new.** The plan disagreed with itself: Task 1 promised an event in answer to every change of effort, Task 2 gave one only on a first reading or a change. Task 2's rule stands, since an event that repeats an unchanged level buys nothing and the core never waits for an answer. The seam's two doc comments were reworded. If wrong: two doc comments.
6. **A task was built while the task before it was under review**, when it consumed nothing of that task and shared no file with it. A fix round for the earlier task waited until the later implementer was done. If wrong: one interleaved commit range.
7. **A session starts again with the level it shows.** The plan's words would pass no level, or the history's older one, for a session whose program had said a level in this run and then rested. What the picker shows must be what the next message uses. If wrong: one condition in `start_spec`.
8. **Tasks 9 and 8 were built out of the plan's order**, while Task 4 was under review and re-review. They consume nothing of the tasks before them. If wrong: nothing.
9. **`tempfile` is a dev-dependency of `mr-core`.** The plan says "no change to dependencies" and also names `tempfile` for tests; the branch reader's tests need a temporary folder. If wrong: one line of `Cargo.toml`.
10. **The probe's print of events hides a Remote Control address.** It printed every event whole, which would have printed a real address. The raw recording keeps raw lines, which is its purpose. If wrong: one match arm in the probe.
11. **The switch shows on whenever Remote Control is on.** The plan said the switch shows what was asked. Remote Control may come on by a way other than the switch, and the switch must never disagree with the mark in a tile's header. So it shows on when it is wanted or when the program says it is not off, and a press asks for the opposite of what it shows. An old reason for a failure is cleared when the program says it is connecting or on. If wrong: one condition in the view and three lines in the core.
12. **A copy made by "Continue as a copy" keeps the wish** the user switched on for the resting original: it is the same conversation going on in the same tile. The preference for new sessions never reaches a copy. If wrong: one line in `answer_prompt`.
13. **Task 12 was split**: the probe and the live test first, the notes last. If wrong: nothing; the commits are the ones the plan names.
14. **The panel slides open as the sidebar does**, over about 0.6 seconds. The motion is the kit's own, the plan says the pane's grip and collapse are not rebuilt, and the tiles follow the drawn row in every frame. If wrong: `tx.instant()` on the key's path, and the button needs `on_press_tx`.
15. **The probe's terminal echo hides the session's address.** Its `raw` mode printed every line of the program, the reply that holds the address included. The echo now hides three values; the recording file keeps raw lines. If wrong: one replace in the echo.
16. **Each of the panel's switches has a visible edge in both themes.** In the light theme an off switch was white on near-white. The edge is drawn by the app around the kit's switch, in the theme's muted icon colour, since no border colour of the light theme reaches the contrast a control needs. The kit's on and off colours stay. If wrong: a look that is changed in one place.
17. **The plan ended without Task 13**, by its own rule: no recording was on disk. If wrong: nothing; the task waits for the recording.
18. **"Files changed are not known yet."** is what the panel says while a stored session's history is not read or its read failed. The plan gave no wording for that case, and "No files changed yet." stated as fact what was not known. If wrong: one string.
19. **The notes' own review ran beside the whole-branch review**, and its findings joined the one fix wave. If wrong: nothing; both reviews happened.

## After the whole-branch review

20. **A session remembers what it last asked its program about Remote Control.** The review found that an "off" pressed before the program had answered an "on" was never sent. Ruling 11 was incomplete there. A press is now compared with the last ask while one is on its way. If wrong: one field and its clearing rules.
21. **Which Minor findings the fix wave took.** Fixed: a false failure line after two quick effort choices and a choice taken back that was not sent; the live test's line about a resumed session's effort; the wider hiding in the probe's echo; context that ignores records that are no reply; a Branch row in the `panel` scene without the demo reading the real disk; the missing tests. Left, each with its reason in the notes: "Connecting…" without end when the program never answers; which copy of a refusal decides the words; kept branches never pruned; stored edits diffed when a history loads; the reader thread writing to the program's input. If wrong: each is a small change later.
22. **Everything the final review declined to judge stands as built**, ten lines, each with its ruling in the notes. Two are put to Jerod: the cost line on a subscription is Claude Code's own total and not money spent, and with "On for new sessions" set an empty new session shows at claude.ai/code while its program runs. If wrong: wording, or one condition each.

## After the re-review of the fix wave

The re-review found every finding addressed and nothing critical or important. There is no second fix wave, so what it found open is parked here.

23. **Four fast presses on the switch (on, off, on, off) can leave Remote Control on.** The memory of the last ask is cleared by the value the program answers with, so the first "on"'s answer settles the second. The switch and the header show that it is on, and one more press turns it off. Left: it needs four presses within about two connection times. If wrong: the asks need counting, not a single remembered value.
24. **Three quick effort choices can end on the choice before the last, or show one false "Could not change the effort level" line.** The picker shows the level really in use; choosing again corrects it. Left for the same reason. If wrong: the same counting.
25. **A request the program never answers leaves its control inert until the session restarts.** After an unanswered "off" the switch stays on and further presses send nothing; the same for an effort level asked for and never answered. Before the fix wave each press sent again. Left: it needs a program that drops a request, which was never seen, and the live test prints `no answer in 30 s` if it happens. If wrong: a deadline on the memory, a few lines.
26. **A session driven only from another device reads "New session"** in its header and sidebar row until Claude Code names it. It is kept and resumed correctly since the fix wave. Left: a title would have to be made from a prompt this app never saw, which is Task 13's ground. If wrong: nothing until Task 13.
27. **The probe's echo hides the address by field name only.** A line that carried the address inside a string, or under another name, would be echoed. No recorded line has that shape. The notes tell Jerod to treat the terminal like the recording file. If wrong: the echo would need to search values too.
28. **One sentence of the notes was wrong and the wave's own three leftovers were missing from them.** The controller corrected the notes in the commit that added this file. If wrong: wording.

## The context breakdown, after Jerod's look

On 2026-11-16 Jerod looked at the window and asked for one more thing: under "Context", what the context is made of (system prompt, system tools, messages and so on). It was built on this branch after `91555e4` by one implementer and passed one review with nothing critical or important.

29. **Built on the plan 5 branch, without a plan document.** It changes one section of the panel Jerod was deciding about, and the request it needs (`get_context_usage`) was seen working in V9. If wrong: the commits after `91555e4` are cut off or reverted.
30. **Rows the program keeps out of the window are not shown.** Its "deferred" tool descriptions are left out of its own sums, and the panel says what the context is made of. If wrong: one condition in the adapter and a muted row.
31. **The figure over the bar stays the one plan 5 shows.** The breakdown gives it only while a session has not finished a turn here. After a turn the figure and the sum of the rows can differ a little. If wrong: one function.
32. **Only names, token counts and kinds are read from the reply.** Its lists of memory files, tools and skills hold paths and are not read. If wrong: nothing lost today.
33. **The breakdown is asked for when a session starts, too.** That moment was never seen; a refusal is silent. If wrong: rows show only after a turn, and the live test's line says so.
34. **A share that rounds to 1% reads "1%".** The brief's example row contradicted its own rule; the rule was followed. If wrong: wording.
35. **The rows are shaped like the "Files changed" rows**, not like the panel's label and value rows, whose label is too narrow for "Autocompact buffer". If wrong: a look in one function.
36. **Three Minors of the review are left.** A name too long for the panel is cut without a tooltip (no name the program gives is that long at 240 points or wider); no test pins the half-percent boundary; a window of 0 is handled in two slightly different ways, and the adapter never lets one through. If wrong: a tooltip and two tests.
