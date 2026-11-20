# Rulings of the plan 9 run (a task checklist and a choice box)

Plan: `docs/superpowers/plans/2026-11-19-tasks-and-choices.md`. Jerod approved it on 2026-11-19 ("Yes, build it"), to be built by subagents, each on Opus 5.5 at high effort or better, on its own branch, without pushing. While it ran, the controller decided what the plan left open, so that the run did not wait. Each ruling says what it costs if it was wrong. Nothing here was seen by Jerod before it was built. Several replace or add to something the plan itself said; each says so. The ones that change what you will see most: 9, 10, 14, 15, 17, 18 and 20.

The branch is `plan-9-tasks-and-choices`, cut from `main` at `7d88a48`. It was merged into `main` on 2026-11-20 as one commit; the commit ids named here are the branch's, from before that. At `bcf6e29`, its last commit of code, `cargo test --workspace` gives 1705 passed, 0 failed, 2 ignored, and `cargo fmt --all --check` and `cargo clippy --workspace --all-targets` are clean. No agent ran the real program, the real window or the live test, and none touched Claude Code's folder: those are Jerod's.

Every task had its own review; two of the nine had a fix round. The whole-branch review found nothing critical, five Important things and fourteen Minor. One fix wave followed; its re-review found every item addressed and two sentences in the notes that were not true, which a last correction closed in code and in words.

## Before the first task

1. **A tool result that comes after its turn has ended adds no step.** Two of the plan's tests contradicted each other about such a result; it is now treated as any result whose call the app never saw. If wrong: one stray event that the core drops.
2. **A reviewer read its diff while the next implementer worked**, as in plans 6 to 8. Fix rounds waited for the running implementer. If wrong: a fix lands a task later.

## The task list

3. **A step's id is trusted no more than its words.** It is taken only when it is not empty and at most 64 characters; the plan gave ids no limit. An update names its step by `taskId`, or by one of the two misspellings the documentation warns of, whichever holds a usable id first. If wrong: a step with an absurd id is not shown.
4. **A step's words are cleaned**: line breaks and tabs become one space, and invisible characters are taken out (see 15). If wrong: nothing.
5. **The current step is the last one in progress**, not the first as the plan had it. A step left in progress by a turn you stopped then no longer holds the spinner once Claude marks a newer step in progress. If wrong: when Claude runs two steps at once, the spinner is on the later one.
6. **A full list makes room.** At 100 steps the oldest ticked step goes when a new one comes; the plan would have refused the new one silently. If wrong: nothing.
7. **The list's turn also ends where a message could not be sent** and the session went back to idle, not only at a finished or failed turn and at the program's exit. If wrong: nothing.
8. **In the stored conversation a message of yours does not drop a step whose result follows it.** A message the program took in mid-turn sits between a call and its result there. If wrong: nothing seen.
9. **A conversation that is let go takes its list and its box with it, and both are read again from the store.** This replaces the plan's rule ("what this run saw is never overwritten") for a session at rest whose conversation the app has dropped from memory. A session carried on in a terminal meanwhile then does not keep an old list over its new conversation. If wrong: in a very long session, a list loses its oldest steps after the idle stop, as it does after a restart.
10. **There is no way yet to clear a stuck list by hand.** A step Claude never ticks keeps the list until a turn ends with everything ticked. The whole-branch review asked for a small control; that is a new thing in the window, so it is a question for Jerod, not a ruling.

## The choice box

11. **A click on a session at rest, then "Cancel" at the "open elsewhere" question, leaves you where you were**: the buttons are back and nothing is put into your typed text. The plan had not thought of this path; without the ruling the choice's message landed in your draft and the buttons were gone. A typed message of your own is given back as before. If wrong: someone who typed a choice's exact message and cancels finds the button in place of their text.
12. **After "Continue as a copy" the original keeps its box**: nothing was said in the original. If wrong: one line to change.
13. **A click first makes the check a typed message makes**: when a resting session's folder is gone, nothing is sent and the line under the box says so. If wrong: nothing.
14. **A choice's message is at most 200 characters**, not 2,000. Pointing at a button shows 200, so with the plan's limit most of a long message would have been sent unseen. If wrong: Claude is refused a long message and shortens it.
15. **Only plain text can be shown.** A question, a label or a message that holds a control character, a mark that turns the reading direction, or one of a few invisible characters is refused with that sentence; a question's and a label's line breaks become one space. The text comes from Claude and may be shaped by a page it read; what you read must be what is sent. The joiner inside emoji stays allowed. If wrong: three flag emoji cannot be in a button's text.
16. **The box shows only when the tool's call succeeded.** The plan announced it at the call; a call the program refused would have left a box for a question Claude was told had failed. If wrong: nothing seen; the box shows at the turn's end either way.
17. **A button that sent a slash command stays answered after a restart.** Claude Code stores a command in a wrapper the reader skipped, so the box would have come back. How the program stores a command was never seen; item 21 of the look tries it. If wrong: the box is back once.
18. **Claude is told to name the decision in its reply.** The plan's instruction ended "do not ask the same question again in your reply". Over Remote Control, in a tile too short for the box, or after a message queued mid-turn, the question would then have been nowhere in words. The last sentence is now: "Your reply must still make sense to someone who cannot see the buttons: end it by naming the decision in a few words." The tool's description names no project's command and states its limits. This wording is not what the recording was made with. If wrong: the decision is named twice in the tile, once in the reply and once in the box.

## The window

19. **Marks land at once.** The empty circle showed as a square for a moment when a turn stopped, and the spinner faded over the tick. No mark fades or changes shape now; the list and the box as a whole still slide and fade in. If wrong: a look.
20. **In a short tile the box comes first.** In a tile under 480 points the list is not drawn while a box shows, and the question shows one line (three in a taller tile), cut at its end, whole when you point at it. A folded list, the box and the message box do not fit 280 points together. At exactly 280 points with an attached file and a notice line as well, the box's top border is 1.5 points under the title bar; the question stays whole. If wrong: an open step is out of sight in a short tile until the choice is answered.
21. **Nothing is drawn before the tile's height is known**, which is only the very first frame of the app. A list in a short tile would otherwise open and fold at start. If wrong: the list appears one frame late.
22. **Folded, the line says what the open list says**: the step's "doing" wording only while a turn runs, its plain name otherwise. If wrong: nothing.
23. **The box is counted at 86 points**, not the plan's 96: the plan's sum was wrong. If wrong: nothing.
24. **Look choices of the implementers**: the folded line's words take the current step's colour; with nothing open the folded mark is the tick; the head line has the hover look of a tool line's toggle; "4 more" is indented under the steps' words.

## The live test

25. **Its step leaves one step open**, and it prints how many steps the list held at the end of the first run. The second run's count from the store should equal it. As the plan had it (both steps ticked) the second run printed 0 whether the list could be rebuilt or not. If wrong: the test's session keeps an open list, which harms nothing.
26. **The live test does not settle whether the tool is loaded up front** (`alwaysLoad`): the adapter hides the search that would show it before the app sees it. A recording with the probe does (`cargo run -p mr-probe -- raw --cwd DIR --choices`), and nothing you see depends on it. If no session starts at all with this build, that setting is the first suspect.

## Left, each by a ruling

- A way to clear a stuck list by hand (10): for Jerod.
- "2 of 5" on the open head counts steps done; "3 of 5: …" on the folded line is the current step's place. The wording is the plan's: for Jerod's look.
- In the dark theme the two plainer buttons of the box had only a faint edge. Settled by Jerod on 2026-11-20 ("Make the grey lighter"): a neutral button is a lighter grey in the dark theme, everywhere in the window (`theme::kit`), and at their word the same day a light grey in the light theme.
- A reply to the program's server request that came before the app has its handle on the program would be dropped, now with one line on stderr. The window for it is microseconds while the program needs far longer to start; never seen.
- The stored conversation knows a turn's end only by a message of yours, so the box of a turn that failed may come back after a restart or after the conversation was read again.
- A copy starts with the original's list; whether the forked program keeps the same list and numbering, and what `/clear` and compaction do to the list, was never seen.
