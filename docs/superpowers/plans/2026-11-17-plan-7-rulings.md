# Rulings of the plan 7 run (running in the background, and opening a picture)

Plan: `docs/superpowers/plans/2026-11-16-background-and-pictures.md`. Jerod approved it on 2026-11-17 ("Yes, build it"), to be built by subagents, each on Opus 5.5 at high effort or better, on its own branch, without pushing. While it ran, the controller decided what the plan and the spec left open, so that the run did not wait. Each ruling says what it costs if it was wrong. Nothing here was seen by Jerod before it was built. Three rulings reverse a choice the plan itself made: 12 (which folder wins for the socket), 14 (when a conversation is given back) and 21 (the quit question's default button).

The branch was `plan-7-background-pictures`, cut from `main` at `3a4b4c1`. Jerod had it merged on 2026-11-17 ("Merge now"), before their live test and look, by the merge commit `622c6ba`. At `89cf154`, its last commit of code, it has 21 commits; `cargo test --workspace` gives 1388 passed, 0 failed, 2 ignored, and `cargo fmt --all --check` and `cargo clippy --workspace --all-targets` are clean. No agent ran the real program, the real window, the tray or the live test: those are Jerod's.

## Before the first task

1. **A message refused before its program has gone is kept and not sent again to that program.** The backend refuses a send the moment a `Close` has shut the program's lane, which is before the process ends. So the failure usually comes first and the exit second. The message waits for that exit: after a clean one the session starts again at once with it, after an unclean one it goes at the restart. The plan's list by phase applies to a failure that arrives after the exit. Without this the message would have been sent again and again to the dying program. If wrong: nothing known; both orders are tested.
2. **After a program has ended, only a message that was refused is taken up again.** Any other failed command of that program is dropped, as before. If wrong: a late failed answer goes unreported, as before.
3. **Ctrl+Q is looked at first, also when something lies over the window.** Under the quit question it changes nothing. Under the large picture it asks to quit and puts the picture away. Every other key is left to what lies on top. If wrong: one key's behaviour.
4. **The app stays without a window only when it is the first copy and holds the socket.** With no way back it must not stay behind. If wrong: nothing; it is the plan's own rule, said in one place.
5. **The tray is told how the sessions stand only once it exists**, and what was said is remembered only then. If wrong: one extra menu update.
6. **A reviewer read its diff while the next implementer worked**, as in plan 6. Fix rounds waited for the running implementer. If wrong: a fix lands a task later.

## While the tasks ran

7. **Three additions of Task 1's implementer stand.** After Quit has begun, no session is started again for a message it never got. A program that ends cleanly before it has started is not started again at once. The queued message is held while an earlier one waits for a dead program. If wrong: in the second case an undelivered message waits for the user's next message.
8. **The Quit button's help says "Quit MindRune (Ctrl+Q)" on every system.** The plan quotes it and only Linux is looked at. If wrong: one wrong key name on macOS.
9. **A launch that reaches the running copy opens its folder through the same code as the folder dialog**, and does nothing once the app is quitting. The plan's signature would have needed a copy of that code. If wrong: nothing.
10. **The activation token is checked before it reaches the window system.** A token from the socket is taken only when it is printable ASCII and at most 512 bytes. The first review of Task 7 found that one with a zero byte in it would have crashed the app inside the Wayland code. The token is also never shown by a debug print. If wrong: a launcher whose token has other characters does not bring the window to the front.
11. **No socket in a shared temporary folder.** When neither the runtime folder nor the app's own folder can be had, the app runs alone and closing its window ends it. If wrong: nothing for a normal desktop.
12. **`RUNE_DIRS_ROOT` wins over `XDG_RUNTIME_DIR` for the socket's folder.** A run moved off the real folders must never find the real running copy. This reverses the plan's order. If wrong: nothing for a normal launch, where the variable is not set.
13. **The live test ticks the app's clock as the window does**, and says "not tried" in the one case where it had printed nothing. If wrong: nothing.

## The whole-branch review and its fix wave

The review found nothing critical, four Important things and fourteen Minor. One fix wave followed; a scoped re-review found all 22 of its items addressed and nothing Important new. One small correction came after it (ruling 27).

14. **A conversation not shown is given back to the store after ten minutes, and not at once.** The plan's decision 16 let it go the moment its tile showed another session, so switching between two resting sessions read each from the store every time, with the loading state and the opened rows forgotten. If wrong: memory is given back ten minutes later.
15. **Remote Control turned on for a session whose program was stopped for idling starts the program.** Before, the switch only noted the wish, and nothing started until a message was typed on this computer. A session from the history that this app has not run still waits for its first message, as plan 5 decided. If wrong: a program starts that the user did not expect.
16. **The app's own usage checks do not count as activity**, a lapsed usage window is asked about once, and no program is started for it while no window shows. The review found that these checks would have kept one program from ever being stopped, and would have started a program every five minutes through the night. If wrong: an empty or stale usage bar until the next turn.
17. **A tool call that runs in the background counts as a background task until the program's own record says otherwise.** The record was never seen with a task in it; if the program never sends it, a dev server would have been ended after half an hour. If wrong: a program that once started a background task, and whose records never come, is never stopped for idling.
18. **The live test starts one background task itself**: it asks the program to run `sleep 20` in the background and answers the one permission request, allowing only that exact command (ruling 27). It settles whether the record comes. If wrong: one more step in a test only Jerod runs.
19. **A message that keeps being refused restarts its program twice and no more.** Then the session is left ended with a notice, and Restart delivers the message. If wrong: one press on Restart.
20. **The quit question counts a waiting message in every state**: one queued, one its program never got, and one held by the "open elsewhere" question. A Quit would lose each without a word. If wrong: the question shows once more than needed.
21. **Return presses "Cancel" in the quit question, not "Quit".** The plan made "Quit" the default. The question protects running work, so its default must not be the answer that stops it. If wrong: one more key press to quit.
22. **The quit question's words are in the dialog's body, where they wrap.** As the plan had it, they ran in one line out of the dialog and off the window. The dialog keeps its name and loses its separate description. If wrong: a screen reader may not read the words by itself when the dialog opens.
23. **`mindrune --quit` says when the app asks instead of quitting**: "something is running; answer the question in its window". If wrong: nothing.
24. **The backdrop behind the large picture is black at 70% in both themes, and the dark theme's dialog backdrop is black at 60%.** The kit's own was too faint in the dark theme to read as "over the window". If wrong: a look.
25. **A session remembers which stored session its conversation was read from**, so a session continued as a copy asks for its older pictures where they are. Whether a copy's file keeps the original messages' ids was never seen. If wrong: nothing known.
26. **The lock and the socket carry the sticky bit**, so that a clean-up of the runtime folder leaves them. If wrong: nothing.
27. **After the re-review: the live test allows exactly `sleep` and a number**, nothing joined to it; and `--quit` takes a copy that has gone as a success when its answer was lost. If wrong: nothing.

Sixteen smaller choices of the fixer, each where the fix brief left one open, are in the notes ("The whole-branch review of plan 7"). The re-review found none of them wrong.

## Left, each by a ruling

Each of these fails soft. They are in the notes under "What is left".

- A model, effort or mode chosen in the second or two while an idle program is being stopped does not take; the old value shows.
- A message whose program ended cleanly before it had started waits for the user's next message, with no sign.
- This run's notices in a conversation are lost when it is given back to the store.
- A session read, dropped and read again before the first read ran is read twice.
- Reading a stored picture again is not skipped when its viewer has closed; on a session file of hundreds of megabytes a press costs about a second.
- A second launch's folder is dropped without a word before the main screen shows, or when its name is not UTF-8.
- A session this app ran is not checked again for "open elsewhere", however long it has rested.
- Images sent in this run are kept in full while their conversation is held, with no ceiling per session.
- A background tool call that was denied or failed still counts as a task until a record comes.
- A picture open over a session that a second launch pushes out of its tile closes by itself ten minutes later.
- If the one usage check after a reset fails, the usage line reads "Resets soon" until the next turn.
- The live test allows `sleep` with any number of seconds, not only the 20 it asks for. A longer one only waits longer.
- The quit dialog has a name and no separate description for a screen reader.
