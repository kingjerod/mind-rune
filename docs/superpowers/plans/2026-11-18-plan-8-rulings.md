# Rulings of the plan 8 run (a Deleted view, status colours and notifications)

Plan: `docs/superpowers/plans/2026-11-17-deleted-view-colours-alerts.md`. Jerod approved it on 2026-11-17 ("Yes, build it"), to be built by subagents, each on Opus 5.5 at high effort or better, on its own branch, without pushing. While it ran, the controller decided what the plan left open, so that the run did not wait. Each ruling says what it costs if it was wrong. Nothing here was seen by Jerod before it was built. Several replace or add to something the plan itself said; each says so. The ones that change what you will see most: 14, 15, 20, 22 and 33.

The branch is `plan-8-deleted-colours-alerts`, cut from `main` at `817f075`. At `3f9c4fe`, its last commit of code, it has 30 commits; `cargo test --workspace` gives 1587 passed, 0 failed, 2 ignored, and `cargo fmt --all --check` and `cargo clippy --workspace --all-targets` are clean. No agent ran the real program, the real window, the tray, a real notification or the live test, and none touched the real store: those are Jerod's.

Every task had its own review; seven of the eleven had fix rounds. The whole-branch review found nothing critical, five Important things and twelve Minor. One fix wave followed; its re-review found all 23 items addressed and one new fault, which a last correction mended and a last re-review confirmed.

## Before the first task

1. **Until the new menus were built, the window's "Delete group" kept doing what it did.** The core's `delete_group` got its new meaning several tasks before the window asked a question for it. If wrong: nothing.
2. **The green and the orange are written down once**, as the icon's own colours, and both the window and the tray's marker are made from that one place. If wrong: nothing.
3. **A reviewer read its diff while the next implementer worked**, as in plans 6 and 7. Fix rounds waited for the running implementer. If wrong: a fix lands a task later.

## Removing a session's files

This is the first code in the project that removes a user's files, so its review's smaller findings were not left for later.

4. **Without a folder that can be found, there is no store at all.** A relative `CLAUDE_CONFIG_DIR` or a missing home folder never falls back to a guessed folder. If wrong: such a setup lists no sessions.
5. **A store that is not there is an error, for listing and for removing.** At first a removal without a store answered "already gone", which made the app forget the deletion; then the whole-branch review found that the listing did the same by answering an empty list. Now neither a deletion nor a group's place nor a name is forgotten because the folder could not be found. A fresh install with no sessions yet still lists as empty. If wrong: the sidebar is empty without a word where the store cannot be found.
6. **Links above a session's own files are followed, as reading follows them**; a link in the place of a session's own file or folder is removed as a link and never followed. A dotfiles setup links `~/.claude` itself. If wrong: a removal reaches through a link the user made, to paths named by the session's id only.
7. **The transcript's time is read again just before it is removed.** If it changed, nothing of the transcript goes. If wrong: nothing.
8. **An id that is not a UUID is an error, not "already gone".** If wrong: such a row says "Not removed" for good.

8a. **A full list of sessions that comes back empty forgets nothing.** A deleted session's file is still in the store for its week, so an empty store with deletions waiting means the store was not really there (a folder with no sessions folder in it, an empty mount point). If wrong: when the store truly became empty, old marks stay until their week's end.

## Deleting

9. **A deleted session's own program is not mistaken for a use elsewhere.** What the program writes as it ends, also when it ended a moment before the delete, does not bring the session back. If wrong: a real use elsewhere within ten seconds of the app's own exit goes unnoticed.
10. **Nothing is kept on a deleted session**: no message waiting to be sent again, no hidden "finished" mark. One that had ended is put to rest so that its conversation is let go. If wrong: nothing.
11. **A session that turns up in a deleted group of your own is deleted with it**, and starts its own week when it turns up. If wrong: an old session you had forgotten was in that group goes to the Deleted view, recoverable for a week.
12. **Recovering a group also recovers what the app had not listed yet.** If wrong: nothing.
13. **A session being removed cannot be recovered in that moment.** Otherwise it could be listed with its file gone. If wrong: a removal whose answer never comes blocks "Recover" until a restart.
14. **"Delete" asks first also when a background task runs or Remote Control is on.** The plan asked only for a running turn, a waiting request or a waiting message. If wrong: one more question.
15. **"Delete group" is not offered until the full list of sessions is in.** Then the question's count is the whole truth, and no older session of that group turns up afterwards. If wrong: a second or two after a start in which a group cannot be deleted.
16. **After a week is over, the removal does not wait for the next minute** when its moment is known: when a removal answers, and when a deleted session's program has gone. If wrong: nothing.
17. **A folder's group can never count as deleted**, whatever a damaged saved state says. If wrong: nothing.
18. **A deleted group takes nothing new.** A new session cannot be started in it, and it is not offered by "Move to group". If wrong: nothing.

## The window

19. **A tile closed by Delete goes at once**, as one closed by its button does. If wrong: nothing.
20. **The questions name what they are about, and say what is true.** Their titles are `Delete "name"?`. The session's question has three texts: it is still working; a message in it has not been sent and will be dropped; it was never sent and will not be kept. The plan's words did not cover the last two. If wrong: wording.
21. **A session never sent that holds a draft or chips asks before it is forgotten.** If wrong: one more question.
22. **A failed removal's row says "Not removed"**, not the plan's "Could not be removed", which cut the title short. If wrong: wording.
23. **Typing in a ready session's box clears "ready"**, and so do pasting into it, attaching to it and "Edit" on its queued message. The plan named typing but the first build could not hear it. A click into a box that already holds the keyboard still clears nothing until the first keystroke. If wrong: nothing.
24. **Handing the keyboard back to a tile is not your act; a click into its box is.** When a press elsewhere moves the focus away and back, the tile that holds the keyboard stays ready. The first fix for this broke the click into a ready tile's box, which the last correction mended: the window now remembers which tile held the keyboard. If wrong: a tile stays green a moment longer.
25. **Orange is one orange.** The request card inside a tile that needs input has the bar's orange border, not the toolkit's amber. In the light theme its small "Needs you" label keeps the darker amber, because the orange cannot be read on white. Usage warnings keep amber. If wrong: a look.
26. **A selected coloured row is its fill and nothing more**, and "Remote" on a coloured bar is dimmer while it connects. The run first gave the selected coloured row a ring inside it; Jerod saw it on 2026-11-18 and did not want it ("I don't want the 2nd orange border, just the inside to be orange", and the same for green), so it is gone. The selection cannot be seen on a coloured row; the arrow keys show the focus ring every row shows. If wrong: a look.
27. **The coloured row keeps the toolkit's own names for its two parts**, against the letter of the rule that every name the app gives starts with `mr-`. If wrong: nothing.

## Notifications and the tray

28. **Signals about a notification are taken only from the notification service itself.** Any program on the session could otherwise fake a press. GNOME's own source says the service answers and signals over one connection. If wrong: on a desktop that does it otherwise, presses do nothing; that check is the first place to look.
29. **A notification is tried again only when the service refused it**, never after a timeout, which could show it twice. If wrong: nothing.
30. **Notifications are taken back at quit.** If wrong: nothing.
31. **The app names three crates it already contained** (`zbus`, `tokio`, `futures-util`). The plan named two. Nothing is downloaded. If wrong: nothing.
32. **A press on a notification takes the same care of the tile it replaces as a press on a row.** If wrong: nothing.
33. **The tray's marker changes only after it has held three seconds, and a lost tray is tried again every thirty seconds.** Without the first, the icon was taken down and put up at every finished turn, also while you work in the window. Without the second, a failure could leave no tray icon, which is the way back to a closed window. If wrong: the marker is three seconds late.

## The live test

34. **It removes the sessions it made**, each only after it has seen that session's program end, and one still running is kept. It does so also when it fails early. If wrong: a test session is left behind now and then.
35. **It looks for leftovers everywhere in Claude Code's folder, by names alone**, and reads no shared file. The review's wish to count lines in Claude Code's prompt history was not followed. If wrong: whether a shared file still names a removed session stays unknown.

## Left, each by a ruling

Each of these fails soft. They are in the notes under "What is left".

- A session id that can never be removed is tried again every hour for ever.
- A removal whose answer never comes blocks "Recover" for that session until a restart.
- A deleted session open but idle in another `claude` loses its file; its time is the only sign there is.
- A title that changes while its notification is out is not updated on the desktop.
- A click into a box that already holds the keyboard does not clear "ready" until the first keystroke.
- A window that comes up without the keyboard counts as looked at until its first change of focus.
- MindRune's own file or folder picker counts as another app: a turn that ends meanwhile turns its tile green and sends a notification.
- Rows slide between the two lists when the view is switched.
- A notification the desktop took more than five seconds to show stays until the app quits.
- The tray's marker shows also while you are looking at the window.
- A start with another `CLAUDE_CONFIG_DIR` forgets the deletions and group places of the first store.
- A start in which the store was not there loses the last run's tiles and their layout (older than this plan).
- A run in which the sessions could not be listed shows an empty list and a greyed "Delete group" without a word.
- A notification can stay on the desktop when MindRune is quit while no program runs.
- A click into the box of the focused tile, when that box did not hold the keyboard, clears "ready" only with the first keystroke.
