//! Real turns through the same `App` and `Backend` the window uses, against the real `claude`.
//! Opt-in: it is ignored by default and returns at once unless `MINDRUNE_LIVE=1`.
//!
//! ```bash
//! MINDRUNE_LIVE=1 cargo test -p mindrune --test live -- --ignored --nocapture
//! ```
//!
//! It needs Claude Code installed and signed in. It runs the app twice, as the window would be
//! opened, closed and opened again, and starts the real program six times in all: the session
//! of the first run, the look the first run takes, a second session opened to the side in the
//! first run, the first session resumed in the second run, the same session started again
//! there after its program was stopped for idling, and a session for attachments opened to the
//! side in the second run. The account is checked once; the second run is handed what the
//! first found.
//!
//! The first run opens with no folder, so it lists the stored sessions and looks, as the window
//! does; the test then starts a new session in a fresh temporary folder, asks for a one-word
//! reply and checks it. After the reply it prints what came of the requests read from the
//! program but not yet seen on the wire: the session's title, the model's label, and whether
//! `claude` accepts the Auto permission mode. Then it prints how long the two listings took,
//! what the look gave, and how long the largest of a few recent stored histories took to read.
//! None of these fails the test, except not getting back to "Ask before edits" after trying
//! Auto. Then it prints what was read in the program for plan 5 and never seen working, none of
//! it a way to fail: the effort level the session started with, what came of choosing another
//! of the model's levels and of going back, the share of the context window in use and whether
//! the cost is known, what the context was made of at the start and after the turn (how many
//! parts and of which kinds, never their names), and what came of turning Remote Control on and
//! off again. Then a second
//! session opens to the side, in the same folder; once it is ready, and before its first
//! message, Remote Control is turned on and off in it too, and what came of that is printed.
//! Then it is sent one message: each session must hold its own reply and not the other's. The
//! first session is renamed: its program must confirm the name, and a listing of the store must
//! then give it. The first run then closes, as the window closes, keeping what it would save:
//! both tiles, the first focused.
//!
//! The second run starts with what the first kept, so it shows the test's session again from
//! the store. Both tiles must come back, the first focused and under its new name. The first
//! message and its reply must be in the history read back, a second message must wake the
//! session, resuming the same id, and get its reply; it prints the effort level the app passed
//! when it resumed the session and the level the program then said. What the first run kept
//! also says that this app closed the session, so the second run is not expected to ask
//! whether another app has it open; it prints whether it did.
//!
//! Then the session is left alone, the idle length set to a minute, until the app stops its
//! program unnoticed, as it stops one idle for half an hour. It prints whether the program
//! ended cleanly, then sends one more message, which starts the session again, and prints
//! whether it answered and whether its model, effort and mode are those it had. None of this
//! fails the test.
//!
//! Then, in the second run, a session of its own opens to the side and is sent a message with a
//! red image made by the test and a file the test writes into its folder, each attached as the
//! window attaches it and read on the preparing thread. It prints whether the answer names the
//! colour and the file's first word, what the stored history gives back for that message,
//! whether that message has its own id in the store and what reading its image again by that
//! id gives, and whether a second message, with an image of noise a little under the largest sent as it is,
//! was answered. None of that fails the test, nor does anything that session's program does.
//!
//! Then that session is asked to run `sleep 20` in the background with its shell tool and to
//! answer with one word. The program asks for permission, and the test, which is the user
//! here, answers through the app as a press on "Allow" does: it allows a request of the shell
//! tool for the command `sleep` and a number, exactly, and denies anything else. It prints whether
//! a background task record that counted at least one task arrived, and whether the adapter's
//! own guard counted the tool call. The `sleep` ends by itself. Last it prints how many
//! background task records the test's programs sent in both runs, and how many tasks the last
//! one counted.
//!
//! At the very end the test removes from the store the three sessions it made, by their ids,
//! through the adapter's `remove_session`, as the app removes a deleted session after its
//! week, and prints how many it removed. It removes a session only once it has seen that
//! session's program end, waiting up to a limit for it; one whose program still runs is kept,
//! with a line that says so. It removes nothing else: no session it did not make and none whose
//! id it does not know. Then it looks through Claude Code's folder for any path whose name
//! still holds the id of a session that is gone, by names alone and never opening a file, at
//! most [`WALK_DEPTH`] levels deep and [`WALK_NAMES`] names, and prints how many it found and
//! the first-level folders they sit under, never a deeper name and never an id. A first-level
//! folder that is a link is not looked into, and the test prints how many there are. None of
//! that fails the test.
//!
//! Every way out of the test, a failed assertion included, closes its sessions, so no `claude`
//! is left running, and removes from the store those of its sessions whose programs it has
//! seen end ([`Tidy`]): a failed run leaves no test session behind but one whose program may
//! still run. Every wait has a deadline that says what it waited for. Of the user's
//! other stored sessions it prints only counts and times: never a title, a folder, a message
//! or an id. It writes one file, into its own temporary folder; only `claude` itself stores the
//! test's three sessions, and only the removal at the end changes the store. What it prints of
//! the session sent attachments holds no path, no id and no image's bytes.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mr_agent::{
    Account, Agent, AgentError, Answer, Attachment, CommandKind, ContextBreakdown, ContextKind,
    HistoryItem, ImageData, PermissionMode, RemoteAccess, RemoteFailure, Removal, RequestId,
    SessionCommand, SessionEvent, SessionId, ToolDetail, UserRequestKind,
};
use mr_claude::{ClaudeAgent, root_from};
use mr_core::{
    App, Backend, ChipState, DEFAULT_WIDTH, Effect, FromBackend, HISTORY_ITEMS, HistoryState,
    IDLE_SECONDS, ItemBody, Launch, Leaving, MAX_BYTES, MAX_SIDE, OWN_EXIT_SLACK_SECONDS, Phase,
    Place, Prompt, PromptChoice, RequestState, Saved, Screen, Session, SessionKey, Source,
    ToBackend, Width, mode_label,
};

/// The whole run, from the account check to the reply; also a resumed session's reply.
const TURN_LIMIT: Duration = Duration::from_secs(120);

/// How long the session may take to end once it was told to close.
const CLOSE_LIMIT: Duration = Duration::from_secs(20);

/// How long the loop sleeps between two looks at the backend.
const TICK: Duration = Duration::from_millis(20);

/// How long the title may take to replace the stand-in, after the reply.
const TITLE_LIMIT: Duration = Duration::from_secs(30);

/// How long a permission mode change may take to be confirmed or refused.
const MODE_LIMIT: Duration = Duration::from_secs(10);

/// How long both listings of the store may take, from the first turn's end.
const LIST_LIMIT: Duration = Duration::from_secs(60);

/// How long the look may take once it was asked for: its own two limits and a margin.
const LOOK_LIMIT: Duration = Duration::from_secs(
    Backend::LOOK_START_LIMIT.as_secs() + Backend::LOOK_USAGE_LIMIT.as_secs() + 10,
);

/// How long one stored history may take to be read.
const HISTORY_LIMIT: Duration = Duration::from_secs(30);

/// How many of the most recent stored sessions have their history read.
const HISTORIES_READ: usize = 5;

/// How long the second run may take to show the test's session again, its history read.
const READ_BACK_LIMIT: Duration = Duration::from_secs(120);

/// How long the store may take to say when the session last changed, once a message woke it.
const ACTIVITY_LIMIT: Duration = Duration::from_secs(30);

const MESSAGE: &str = "Reply with the single word: ok";

const SECOND_MESSAGE: &str = "Reply with the single word: again";

/// The message to the session opened to the side.
const SIDE_MESSAGE: &str = "Reply with the single word: side";

/// The name the test's session is given.
const NAME: &str = "MindRune live test";

/// How long the program may take to confirm a name.
const RENAME_LIMIT: Duration = Duration::from_secs(30);

/// How long a listing of the store may take to give the new name, asked again each second.
const NAMED_LIMIT: Duration = Duration::from_secs(30);

/// How long the program may take to say the effort level, at start or after a choice.
const EFFORT_LIMIT: Duration = Duration::from_secs(30);

/// How long the program may take to say what the context is made of after the first turn.
const BREAKDOWN_LIMIT: Duration = Duration::from_secs(10);

/// How long Remote Control may take to be on or refused, and to be off again.
const REMOTE_LIMIT: Duration = Duration::from_secs(30);

/// How long the test goes on listening after a refusal of Remote Control, for the other copies
/// of it, which may give another kind of failure.
const REFUSAL_LISTEN: Duration = Duration::from_secs(2);

/// The message sent with an image and a file, in a session of its own.
const ATTACHED_MESSAGE: &str = "What colour is the image, and what is the first word of the file?";

/// The image sent with it: this many pixels a side, all red.
const RED_SIDE: u32 = 16;

/// The file sent with it, written into the test's folder: one made-up sentence, and its first
/// word as the answer would name it.
const NOTE_NAME: &str = "note.txt";
const NOTE: &str = "Pelican boats drift past the old harbour at dawn.\n";
const NOTE_WORD: &str = "pelican";

/// The message sent with the image of noise.
const NOISE_MESSAGE: &str = "Reply with the single word: noise";

/// The sides of the image of noise: three bytes of noise a pixel make a PNG of about 3.9 MB, a
/// little under `MAX_BYTES`, so it is sent byte for byte, as a line of about 5.2 MB.
const NOISE_SIZE: (u32, u32) = (1120, 1160);

/// How long the chips of a message may take to be read.
const PREPARE_LIMIT: Duration = Duration::from_secs(30);

/// The message that starts the test's session again once its program was stopped for idling.
const QUIET_MESSAGE: &str = "Reply with the single word: back";

/// The idle length the test sets: the shortest the app takes, a minute.
const QUIET_AFTER: i64 = 60;

/// How long the test waits for the app to stop the idle program: the idle length, the events
/// its program may still send after a turn, and a margin.
const QUIET_LIMIT: Duration = Duration::from_secs(180);

/// What the session for attachments is asked last: one command in the background, which ends
/// by itself, and a short answer.
const BACKGROUND_MESSAGE: &str = "Run the shell command `sleep 20` in the background with your \
    shell tool (set run_in_background), do not wait for it, and then reply with the single word: \
    started";

/// The shell tool's name, as its permission request gives it.
const SHELL_TOOL: &str = "Bash";

/// How the one command the test allows starts; only digits may follow ([`allowed_command`]).
const ALLOWED_COMMAND: &str = "sleep ";

/// How long after the turn's end a background task record is still waited for.
const RECORD_LIMIT: Duration = Duration::from_secs(5);

/// The picture number the test reads a stored image under: one no session holds, so the
/// answer is for the test alone.
const READ_PICTURE: u64 = u64::MAX;

/// How many levels under Claude Code's folder the look for leftover paths goes: a file in a
/// session's own folder in a project folder is at the fourth.
const WALK_DEPTH: usize = 4;

/// How many names the look for leftover paths reads at most.
const WALK_NAMES: usize = 200_000;

#[test]
#[ignore = "starts the real claude; run with MINDRUNE_LIVE=1 and --ignored"]
fn a_real_turn_the_store_the_look_and_the_session_resumed() {
    if std::env::var("MINDRUNE_LIVE").as_deref() != Ok("1") {
        eprintln!("live: skipped; set MINDRUNE_LIVE=1 to run it");
        return;
    }
    let dir = tempfile::tempdir().expect("a temporary folder");
    let folder = dir.path().to_path_buf();
    let agent: Arc<dyn Agent> = Arc::new(ClaudeAgent::from_environment());
    // Before the runs, so that it is dropped after them: on every way out it removes the
    // test's sessions whose programs were seen to end.
    let made = Rc::new(RefCell::new(Made::default()));
    let mut tidy = Tidy {
        agent: agent.clone(),
        made: made.clone(),
        done: false,
    };

    // The first run: the window opened with no folder, and a new session in the test's folder.
    let launch = Launch {
        folder: None,
        look: true,
        home: folder.clone(),
        ..Launch::default()
    };
    let mut live = Live::start(agent.clone(), launch, &folder, None, made.clone());
    let stand_in = live.first_turn();
    live.report_title_model_and_auto_mode(stand_in);
    live.report_listings();
    live.report_look();
    live.report_histories();
    live.report_effort();
    live.report_context_and_cost();
    live.report_breakdown();
    live.report_remote(live.key.expect("the test's session"), "remote control");
    live.side_turn();
    live.rename();
    let id = live.id().expect("the session has the agent's id");
    let side_id = live
        .side_id()
        .expect("the session to the side has the agent's id");
    let saved = live.close_app();
    let account = live.app.account.clone().expect("the account was checked");
    let background = std::mem::take(&mut live.background);
    drop(live);

    // The second run: the window opened again with what the first kept.
    let launch = Launch {
        folder: None,
        look: false,
        home: folder.clone(),
        saved,
    };
    let mut again = Live::start(agent.clone(), launch, &folder, Some(account), made);
    again.background = background;
    again.read_back(&id);
    again.restored(&id, &side_id);
    again.resume(&id);
    again.stop_while_idle();
    again.attachments();
    again.report_background();
    again.close_app();
    // A session of the test's is removed only once the test has seen its program end: no
    // program runs for it any more, or the app holds it no more. `close_app` waited for the
    // first two; the session for attachments was waited for only up to a limit, so the test
    // waits again here, up to `CLOSE_LIMIT`, and leaves any session whose program still runs.
    // The time given to the removal is the moment that wait ended plus the app's own slack for
    // a program's last writes, as the app gives it for a session it deleted. It guards little
    // here, since the removal follows at once: what keeps a running program's files is the wait.
    let ids = [Some(id), Some(side_id), again.attached_id()];
    let running = again.wait_until_ended(&ids);
    let ended_unix = unix_now();
    drop(again);
    // From here the removal is this one: the guard has nothing left to do.
    tidy.done = true;
    remove_own_sessions(
        agent.as_ref(),
        &ids,
        &running,
        ended_unix.saturating_add(OWN_EXIT_SLACK_SECONDS),
    );
}

/// The sessions the test made in the real store, by the agent's id, each with when its program
/// was last seen to end (Unix seconds), or `None` while a program runs for it or may. Both runs
/// write here ([`Live::track`]); [`Tidy`] reads it.
#[derive(Debug, Default)]
struct Made {
    sessions: Vec<(SessionId, Option<i64>)>,
}

impl Made {
    /// Takes in that a program runs for `id` at `now_unix`, or that none does. A session is
    /// only known here once a program was seen running for it, so that an id the test merely
    /// read somewhere is never taken for its own; and a program that runs again (a resume)
    /// takes back what was said of the one before.
    fn see(&mut self, id: &SessionId, runs: bool, now_unix: i64) {
        match self.sessions.iter_mut().find(|(known, _)| known == id) {
            Some((_, ended)) if runs => *ended = None,
            Some((_, ended)) => {
                ended.get_or_insert(now_unix);
            }
            None if runs => self.sessions.push((id.clone(), None)),
            None => {}
        }
    }

    /// The sessions whose program was seen to end and has not run since, with when.
    fn ended(&self) -> Vec<(SessionId, i64)> {
        self.sessions
            .iter()
            .filter_map(|(id, ended)| Some((id.clone(), (*ended)?)))
            .collect()
    }
}

/// Removes the test's sessions from the store when the test leaves before its end (a failed
/// assertion, a deadline): a failed run is when strays pile up in the user's real store. It is
/// declared before the runs, so it is dropped after them, when each has closed its sessions and
/// waited for them ([`Live`]'s own drop); it waits for nothing itself.
///
/// It removes only a session whose program the test saw end, and nothing else, for these
/// reasons. A program that still runs, or may, could write its files again after they were
/// removed, and what it then leaves is a partial session nobody made, in the user's own list:
/// worse than a whole test session left behind. A session whose id the test never learned
/// cannot be named. And nothing the test did not make is ever its to remove.
struct Tidy {
    agent: Arc<dyn Agent>,
    made: Rc<RefCell<Made>>,
    /// The test reached its own removal at its end.
    done: bool,
}

impl Tidy {
    /// Removes each session whose program was seen to end, unless its transcript changed later
    /// than that exit, with the app's own slack for a program's last writes, as at the test's
    /// end. Gives how many were removed, of how many, and how many were kept because a program
    /// may still run. None while the record is being written. Waits for nothing.
    fn remove_ended(&self) -> Option<(usize, usize, usize)> {
        let made = self.made.try_borrow().ok()?;
        let ended = made.ended();
        let kept = made.sessions.len() - ended.len();
        let removed = ended
            .iter()
            .filter(|(id, ended_unix)| {
                let unless_after = ended_unix.saturating_add(OWN_EXIT_SLACK_SECONDS);
                matches!(
                    self.agent.remove_session(id, unless_after),
                    Ok(Removal::Removed)
                )
            })
            .count();
        Some((removed, ended.len(), kept))
    }
}

impl Drop for Tidy {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        // This runs while the test's own panic unwinds, where a second panic that got out
        // would abort the process and lose the first one's words: what the adapter might do is
        // caught, and the line is written without `eprintln!`, which panics when stderr is
        // gone.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            if let Some((removed, of, kept)) = self.remove_ended() {
                let _ = writeln!(
                    std::io::stderr(),
                    "live: the test left early; its sessions removed from the store: {removed} \
                     of {of}, kept because a program may still run: {kept}"
                );
            }
        }));
    }
}

/// Removes for good, through the adapter's `remove_session`, the stored sessions the test made
/// in this run, `ids` (`None` for one whose id it does not know, which is left alone), but not
/// those in `running`, whose programs had not ended, and not one whose transcript changed later
/// than `unless_after_unix`. Prints how many were removed, then how many paths anywhere in
/// Claude Code's folder are still named by the id of a session that is gone ([`leftovers`]).
/// Fails on nothing.
fn remove_own_sessions(
    agent: &dyn Agent,
    ids: &[Option<SessionId>],
    running: &[SessionId],
    unless_after_unix: i64,
) {
    let mut known: Vec<&SessionId> = Vec::new();
    for id in ids.iter().flatten() {
        if !known.contains(&id) {
            known.push(id);
        }
    }
    let unknown = ids.iter().filter(|id| id.is_none()).count();
    if unknown > 0 {
        eprintln!("live: sessions of the test's whose id is not known, not removed: {unknown}");
    }
    let mut removed = 0;
    // The sessions the store no longer has: removed, or not there under their id.
    let mut gone: Vec<&SessionId> = Vec::new();
    for id in &known {
        if running.contains(id) {
            eprintln!("live: a test session was still running; kept");
            continue;
        }
        match agent.remove_session(id, unless_after_unix) {
            Ok(Removal::Removed) => {
                removed += 1;
                gone.push(id);
            }
            Ok(Removal::Missing) => {
                eprintln!("live: a session of the test's was not in the store");
                gone.push(id);
            }
            Ok(Removal::Changed) => {
                eprintln!("live: a session of the test's changed after the test closed it; kept");
            }
            // The error's words may name a path: only its kind is printed.
            Err(error) => eprintln!(
                "live: a session of the test's could not be removed ({})",
                error_kind(&error)
            ),
        }
    }
    eprintln!(
        "live: the test's own sessions removed from the store: {removed} of {}",
        known.len()
    );
    const LINE: &str = "live: paths still named by a removed session's id:";
    let Some(root) = root_from(
        std::env::var_os("CLAUDE_CONFIG_DIR").as_deref(),
        std::env::home_dir().as_deref(),
    ) else {
        eprintln!("{LINE} not looked at (no store)");
        return;
    };
    if gone.is_empty() {
        eprintln!("{LINE} not looked at (no session was removed)");
        return;
    }
    let Some(left) = leftovers(&root, &gone, WALK_DEPTH, WALK_NAMES) else {
        eprintln!("{LINE} not looked at (the folder could not be listed)");
        return;
    };
    eprintln!("{LINE} {}", left.count);
    if !left.under.is_empty() {
        let under: Vec<&str> = left.under.iter().map(String::as_str).collect();
        eprintln!("live: they sit under: {}", under.join(", "));
    }
    if left.cut {
        eprintln!(
            "live: the look stopped after {WALK_NAMES} names; not all of the folder was seen"
        );
    }
    if left.unlisted > 0 {
        eprintln!(
            "live: folders that could not be listed, and so not looked into: {}",
            left.unlisted
        );
    }
    if left.links > 0 {
        eprintln!("live: folders not looked into, being links: {}", left.links);
    }
}

/// What a look through Claude Code's folder found of sessions that are gone.
#[derive(Debug, Default, PartialEq)]
struct Leftovers {
    /// How many paths have a name that holds the id of one of them.
    count: usize,
    /// The first-level folders those paths sit under (`projects`, `file-history` and so on).
    /// Never a deeper name and never an id: a first-level name that itself holds an id is
    /// given as [`AT_THE_TOP`].
    under: BTreeSet<String>,
    /// The look stopped at its limit of names, so not all was seen.
    cut: bool,
    /// How many folders below the first could not be listed.
    unlisted: usize,
    /// How many first-level folders are links, and so were not looked into: with `projects`
    /// linked elsewhere, a count of 0 would say nothing of what is there.
    links: usize,
}

/// What stands for a first-level name that holds an id.
const AT_THE_TOP: &str = "(the top of the folder)";

/// Looks through `root`, Claude Code's folder, for every path whose name holds one of `ids`:
/// at most `depth` levels down and `names` names in all. Only names are read, with what kind
/// of thing each is: no file is opened, no link is followed, and a path found is counted and
/// not looked into. This finds a kind of path nobody knew a session leaves; it cannot see an
/// id written inside a file. None when `root` itself cannot be listed.
fn leftovers(root: &Path, ids: &[&SessionId], depth: usize, names: usize) -> Option<Leftovers> {
    // An id too short to be one would be found in every name.
    let ids: Vec<&str> = ids
        .iter()
        .map(|id| id.0.as_str())
        .filter(|id| id.len() >= 8)
        .collect();
    let holds_id = |name: &str| ids.iter().any(|id| name.contains(id));
    let mut found = Leftovers::default();
    let mut seen = 0;
    // Each folder still to list: its path, its level, and the first-level name it is under.
    let mut todo: Vec<(PathBuf, usize, Option<String>)> = vec![(root.to_path_buf(), 1, None)];
    while let Some((folder, level, first)) = todo.pop() {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(_) if level == 1 => return None,
            Err(_) => {
                found.unlisted += 1;
                continue;
            }
        };
        for entry in entries.flatten() {
            if seen == names {
                found.cut = true;
                return Some(found);
            }
            seen += 1;
            let name = entry.file_name().to_string_lossy().into_owned();
            let first = first.clone().unwrap_or_else(|| name.clone());
            if holds_id(&name) {
                found.count += 1;
                let under = if holds_id(&first) {
                    AT_THE_TOP.to_string()
                } else {
                    first
                };
                found.under.insert(under);
                continue;
            }
            // `file_type` does not follow a link: a linked folder is not looked into. At the
            // first level it is counted, so that the test can say it did not look there; only
            // what kind of thing the link points to is asked, nothing is opened.
            let kind = entry.file_type();
            if level == 1 && kind.as_ref().is_ok_and(|kind| kind.is_symlink()) {
                found.links += usize::from(entry.path().is_dir());
            }
            if level < depth && kind.is_ok_and(|kind| kind.is_dir()) {
                todo.push((entry.path(), level + 1, Some(first)));
            }
        }
    }
    Some(found)
}

/// Runs in the ordinary suite: it looks through a folder the test makes.
#[test]
fn leftovers_are_found_by_name_anywhere_and_named_by_their_first_folder() {
    let store = tempfile::tempdir().expect("a temporary folder");
    let root = store.path();
    let gone = SessionId("0b5e1a52-8c3f-4d2a-9e61-7f0c2d4b8a19".into());
    let other = "1c6f2b63-9d40-4e3b-af72-801d3e5c9b2a";
    let id = gone.0.as_str();
    let make = |path: String| std::fs::create_dir_all(root.join(path)).expect("a folder");
    let write = |path: String| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
        std::fs::write(path, "x").expect("a file");
    };
    // Nothing of it is left: another session's paths do not count.
    write(format!("projects/-home-alex-project/{other}.jsonl"));
    make(format!("file-history/{other}"));
    write("history.jsonl".into());
    let ids = [&gone];
    assert_eq!(
        leftovers(root, &ids, WALK_DEPTH, WALK_NAMES),
        Some(Leftovers::default())
    );

    // The kinds removing knows, and kinds nobody knew of, at every level looked at.
    write(format!("projects/-home-alex-project/{id}.jsonl"));
    write(format!(
        "projects/-home-alex-project/{id}/subagents/agent-a1.jsonl"
    ));
    make(format!("session-env/{id}"));
    write(format!("todos/{id}-agent-{id}.json"));
    write(format!("tasks/by-day/2026/{id}.lock"));
    write(format!("{id}.json"));
    let found = leftovers(root, &ids, WALK_DEPTH, WALK_NAMES).expect("listed");
    assert_eq!(found.count, 6, "each path once, and none inside one found");
    let under: Vec<&str> = found.under.iter().map(String::as_str).collect();
    assert_eq!(
        under,
        [AT_THE_TOP, "projects", "session-env", "tasks", "todos"]
    );
    assert!(!found.cut && found.unlisted == 0 && found.links == 0);
    // What is printed holds no id and no project folder's name.
    let printed = under.join(", ");
    assert!(!printed.contains(id) && !printed.contains("-home-alex-project"));

    // Not deeper than asked: the lock is at the fourth level.
    let shallow = leftovers(root, &ids, 3, WALK_NAMES).expect("listed");
    assert_eq!(shallow.count, 5);
    assert!(!shallow.under.contains("tasks"));
    // Not more names than asked, and it says that it stopped.
    let few = leftovers(root, &ids, WALK_DEPTH, 3).expect("listed");
    assert!(few.cut && few.count <= 3);
    // An id that is none finds nothing, and a folder that is not there is not looked at.
    let empty = SessionId(String::new());
    assert_eq!(
        leftovers(root, &[&empty], WALK_DEPTH, WALK_NAMES),
        Some(Leftovers::default())
    );
    assert_eq!(
        leftovers(&root.join("not-there"), &ids, WALK_DEPTH, WALK_NAMES),
        None
    );
}

/// Runs in the ordinary suite: no link is followed out of the folder.
#[cfg(unix)]
#[test]
fn the_look_for_leftovers_follows_no_link() {
    let store = tempfile::tempdir().expect("a temporary folder");
    let elsewhere = tempfile::tempdir().expect("a temporary folder");
    let gone = SessionId("0b5e1a52-8c3f-4d2a-9e61-7f0c2d4b8a19".into());
    std::fs::create_dir_all(elsewhere.path().join(&gone.0)).expect("a folder");
    std::fs::write(elsewhere.path().join("a-file"), "x").expect("a file");
    let link = |to: PathBuf, at: &str| std::os::unix::fs::symlink(to, store.path().join(at));
    link(elsewhere.path().to_path_buf(), "linked").expect("a link");
    // A linked file and a link to nowhere are no folders: only the linked folder is counted.
    link(elsewhere.path().join("a-file"), "file").expect("a link");
    link(elsewhere.path().join("not-there"), "gone").expect("a link");
    // A link below the first level is not looked into either, and not counted.
    std::fs::create_dir_all(store.path().join("projects")).expect("a folder");
    link(elsewhere.path().to_path_buf(), "projects/linked").expect("a link");
    assert_eq!(
        leftovers(store.path(), &[&gone], WALK_DEPTH, WALK_NAMES),
        Some(Leftovers {
            links: 1,
            ..Leftovers::default()
        }),
        "nothing found there, and it says that it did not look"
    );
}

/// Runs in the ordinary suite, against the fake: nothing real is removed.
#[test]
fn a_test_that_leaves_early_removes_only_what_it_saw_end() {
    let id = |text: &str| SessionId(text.into());
    let mut made = Made::default();
    // Never seen running: not known to be the test's.
    made.see(&id("read-somewhere"), false, 100);
    assert!(made.sessions.is_empty());
    made.see(&id("first"), true, 100);
    made.see(&id("side"), true, 101);
    made.see(&id("attached"), true, 102);
    assert_eq!(made.ended(), vec![], "all three run");
    made.see(&id("first"), false, 110);
    made.see(&id("first"), false, 120);
    made.see(&id("side"), false, 111);
    assert_eq!(
        made.ended(),
        vec![(id("first"), 110), (id("side"), 111)],
        "when each was first seen ended"
    );
    // Resumed: its program runs again, so it is not one to remove until that one ends too.
    made.see(&id("first"), true, 130);
    assert_eq!(made.ended(), vec![(id("side"), 111)]);
    made.see(&id("first"), false, 140);
    assert_eq!(made.ended(), vec![(id("first"), 140), (id("side"), 111)]);

    let stored = |text: &str| mr_agent::StoredSession {
        id: id(text),
        folder: "/home/alex/project".into(),
        title: text.into(),
        last_active_unix: 100,
    };
    let fake = Arc::new(mr_agent::FakeAgent::new());
    fake.set_sessions(vec![stored("first"), stored("side"), stored("attached")]);
    let made = Rc::new(RefCell::new(made));
    // The test reached its own removal: the guard does nothing when it is dropped.
    let tidy = Tidy {
        agent: fake.clone(),
        made: made.clone(),
        done: true,
    };
    drop(tidy);
    assert_eq!(fake.removed(), vec![]);
    // It left early: the two seen to end go, the one that may still run stays. (The guard's
    // drop does this and prints a line; here it is marked done, to keep the output clean.)
    let mut tidy = Tidy {
        agent: fake.clone(),
        made: made.clone(),
        done: false,
    };
    assert_eq!(tidy.remove_ended(), Some((2, 2, 1)));
    assert_eq!(fake.removed(), vec![id("first"), id("side")]);
    // A record that is being written is left alone, without a panic.
    let writing = made.borrow_mut();
    assert_eq!(tidy.remove_ended(), None);
    drop(writing);
    assert_eq!(fake.removed().len(), 2);
    tidy.done = true;
}

/// The kind of `error`, without its words, which may name a path.
fn error_kind(error: &AgentError) -> &'static str {
    match error {
        AgentError::NotInstalled => "not installed",
        AgentError::Closed => "closed",
        AgentError::Io(_) => "an error reading or removing",
        AgentError::Protocol(_) => "a protocol error",
    }
}

/// When something was asked of the store and what came back.
struct Answered {
    took: Duration,
    /// How many it gave, or `None` when it failed.
    count: Option<usize>,
}

/// One run of the `App` and the `Backend` over the real agent, the way the window holds them.
/// Dropping it before the test's session closed cleanly (a failed assertion, a timeout) closes
/// the session, and writes down which of the test's sessions were seen to end.
struct Live {
    app: App,
    backend: Backend,
    /// The sessions the test made, with whether a program runs for each ([`Live::track`]).
    made: Rc<RefCell<Made>>,
    /// The test's own folder: the only place its sessions and its look start in.
    folder: PathBuf,
    /// The test's session in this run, once there is one.
    key: Option<SessionKey>,
    /// The session opened to the side of it, once there is one.
    side: Option<SessionKey>,
    /// A new session is still to be started in `folder` once the account is signed in.
    start_new: bool,
    started: Instant,
    /// The test's session's program is meant to be running: until it closes, it may not rest.
    running: bool,
    closed: bool,
    /// Every command of the test's session the agent refused, with its reason, in order. The
    /// core keeps a refused title request to itself, so the test reads them as they come.
    refusals: Vec<(CommandKind, String)>,
    /// Every title the test's session was given, in order, as the agent sent it.
    titles: Vec<String>,
    /// Every permission mode the agent confirmed for the test's session, in order. Unlike the
    /// session's `mode`, this shows a confirmation of the mode already in use.
    modes: Vec<PermissionMode>,
    /// The test's session's own program reported the session's usage window.
    session_usage: bool,
    /// When each listing was asked for: the recent one, and the one of every stored session.
    recent_asked: Option<Instant>,
    all_asked: Option<Instant>,
    recent: Option<Answered>,
    all: Option<Answered>,
    look_asked: Option<Instant>,
    /// What the look gave: how long it took, how many models, whether it knew the usage.
    looked: Option<(Duration, usize, bool)>,
    /// When each history was asked for, by the entry it is for.
    reading: HashMap<SessionKey, Instant>,
    /// Each history read, with how many items it had.
    read: HashMap<SessionKey, Answered>,
    /// What the store said about when the test's session last changed: Unix seconds.
    activity: Option<Option<i64>>,
    /// The spec of the latest program opened for the test's session.
    opened: Option<mr_agent::StartSpec>,
    /// A listing was asked for to see the test's session's new name.
    asked_for_name: bool,
    /// Whether the first listing that answered since gave that name.
    listed_name: Option<bool>,
    /// Every effort level the agent said for the test's session, in order, with when.
    efforts: Vec<(Instant, Option<String>)>,
    /// What the programs of the test's sessions said of Remote Control, in order, with when.
    remotes: Vec<(SessionKey, Instant, RemoteSaid)>,
    /// Whether a turn of the test's session has ended in this run.
    turn_ended: bool,
    /// Every breakdown of the context the test's session was given, in order, each with whether
    /// a turn had ended before it came.
    breakdowns: Vec<(bool, ContextBreakdown)>,
    /// The session that is sent attachments, once there is one. Unlike the others, nothing its
    /// program does fails the test.
    attached: Option<SessionKey>,
    /// How each turn of that session ended, in order: true when it finished, false when it
    /// failed.
    attached_ends: Vec<bool>,
    /// That session's stored history was asked for, and its answer is for the test alone.
    stored_asked: bool,
    /// What the read of that history gave: its items, or `None` when it failed.
    stored: Option<Option<Vec<HistoryItem>>>,
    /// The picture ids of that history's first message of the user's.
    stored_pictures: Vec<u64>,
    /// How many of them a picture answer came for: the reader gave their bytes.
    stored_read: usize,
    /// The picture number a stored image was asked for under, until its answer came.
    image_asked: Option<u64>,
    /// What that read gave: the image's bytes, or why it failed.
    image_read: Option<Result<ImageData, String>>,
    /// How many tasks each background task record counted, in order, of every program the test
    /// ran; the first run's are handed to the second.
    background: Vec<usize>,
    /// What the session for attachments' program said about background tasks while it was
    /// asked to start one ([`Live::background_task`]).
    tasks: Tasks,
    /// Every program's exit, by its open number, with whether it was clean.
    exits: Vec<(u64, bool)>,
    /// Every open the app sent `Close`, in order.
    closes: Vec<u64>,
    /// The test's session is past the steps that check it: from the idle stop on it is
    /// reported on and never failed on, and closing the app takes it as closed once its program
    /// has ended, cleanly or not.
    quiet: bool,
}

/// What came of asking a program to start a background task. The adapter gives a count of
/// tasks in two cases, which look the same to the app: from its own guard, in the same breath
/// as the tool call it saw, and from the program's record, on its own. The test tells them
/// apart by that: a count that comes directly after a shell call is the guard's.
#[derive(Clone, Copy, Debug, Default)]
struct Tasks {
    /// The step runs: the events of the session for attachments are looked at.
    watching: bool,
    /// How many calls of the shell tool were seen.
    shell_calls: usize,
    /// The event before was a shell call.
    after_shell_call: bool,
    /// A count of at least one came directly after a shell call.
    guard_counted: bool,
    /// A count of at least one came on its own.
    record_counted: bool,
}

impl Tasks {
    /// Takes in one event of the watched program.
    fn see(&mut self, event: &SessionEvent) {
        let after_shell_call = std::mem::take(&mut self.after_shell_call);
        match event {
            SessionEvent::ToolStarted {
                detail: ToolDetail::Command { .. },
                ..
            } => {
                self.shell_calls += 1;
                self.after_shell_call = true;
            }
            SessionEvent::BackgroundTasks { running } if after_shell_call => {
                self.guard_counted |= *running >= 1;
            }
            SessionEvent::BackgroundTasks { running } => self.record_counted |= *running >= 1,
            _ => {}
        }
    }
}

/// What a program said of Remote Control, without the address it may carry.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RemoteSaid {
    Off,
    Starting,
    On,
    Reconnecting,
    Failed(RemoteFailure),
    /// A command about it was refused.
    Refused,
}

impl RemoteSaid {
    fn from_event(event: &SessionEvent) -> Option<RemoteSaid> {
        match event {
            SessionEvent::RemoteAccess(access) => Some(match access {
                RemoteAccess::Off => RemoteSaid::Off,
                RemoteAccess::Starting => RemoteSaid::Starting,
                RemoteAccess::On { .. } => RemoteSaid::On,
                RemoteAccess::Reconnecting => RemoteSaid::Reconnecting,
                RemoteAccess::Failed { why, .. } => RemoteSaid::Failed(*why),
            }),
            SessionEvent::CommandFailed {
                what: CommandKind::SetRemoteAccess,
                ..
            } => Some(RemoteSaid::Refused),
            _ => None,
        }
    }
}

impl Live {
    /// The app as the window starts it with `launch`. With `account`, that account is taken in
    /// at once instead of checking it again; without, the account is checked and a new session
    /// is started in `folder` once it is signed in.
    fn start(
        agent: Arc<dyn Agent>,
        launch: Launch,
        folder: &Path,
        account: Option<Account>,
        made: Rc<RefCell<Made>>,
    ) -> Live {
        let (app, mut effects) = App::new(
            agent.capabilities(),
            Width::Points(DEFAULT_WIDTH),
            launch,
            unix_now(),
        );
        let backend = Backend::start(agent, Arc::new(|| {}));
        let mut live = Live {
            app,
            backend,
            made,
            folder: folder.to_path_buf(),
            key: None,
            side: None,
            start_new: account.is_none(),
            started: Instant::now(),
            running: false,
            closed: false,
            refusals: Vec::new(),
            titles: Vec::new(),
            modes: Vec::new(),
            session_usage: false,
            recent_asked: None,
            all_asked: None,
            recent: None,
            all: None,
            look_asked: None,
            looked: None,
            reading: HashMap::new(),
            read: HashMap::new(),
            activity: None,
            opened: None,
            asked_for_name: false,
            listed_name: None,
            efforts: Vec::new(),
            remotes: Vec::new(),
            turn_ended: false,
            breakdowns: Vec::new(),
            attached: None,
            attached_ends: Vec::new(),
            stored_asked: false,
            stored: None,
            stored_pictures: Vec::new(),
            stored_read: 0,
            image_asked: None,
            image_read: None,
            background: Vec::new(),
            tasks: Tasks::default(),
            exits: Vec::new(),
            closes: Vec::new(),
            quiet: false,
        };
        if let Some(account) = account {
            effects.retain(|effect| *effect != Effect::CheckAccount);
            effects.extend(live.app.receive(FromBackend::Account(account), 0.0));
        }
        live.carry_out(effects);
        live
    }

    /// Waits for the session to be ready, sends the message once, waits for the turn to end and
    /// for the session's own usage, and checks the reply. Returns the stand-in title the
    /// session had once the message was sent.
    fn first_turn(&mut self) -> Option<String> {
        let deadline = self.started + TURN_LIMIT;
        let mut sent = false;
        let mut stand_in = None;
        loop {
            self.take_in();
            self.fail_on_a_dead_end();
            let phase = self.phase();
            if phase == Some(Phase::Idle) {
                if !sent {
                    let key = self.key.expect("the session was started");
                    let effects = self.app.submit(key, MESSAGE);
                    assert!(!effects.is_empty(), "the message was not sent: {effects:?}");
                    self.running = true;
                    self.carry_out(effects);
                    sent = true;
                    stand_in = self.title();
                } else if self.session_usage {
                    break;
                }
            }
            if Instant::now() >= deadline {
                if !sent {
                    panic!(
                        "the session never reached Idle within {} s: screen {:?}, phase {phase:?}",
                        TURN_LIMIT.as_secs(),
                        self.app.screen
                    );
                }
                if phase == Some(Phase::Idle) {
                    panic!(
                        "the turn ended but the session's usage limits never arrived within {} s",
                        TURN_LIMIT.as_secs()
                    );
                }
                panic!(
                    "no reply within {} s (phase {phase:?}); conversation:\n{:#?}",
                    TURN_LIMIT.as_secs(),
                    self.items()
                );
            }
            std::thread::sleep(TICK);
        }

        let items = self.items();
        let replies = finished_replies(&items);
        assert_eq!(
            replies.len(),
            1,
            "one finished reply was expected; conversation:\n{items:#?}"
        );
        assert!(
            replies[0].to_lowercase().contains("ok"),
            "the reply does not say ok: {:?}",
            replies[0]
        );
        assert!(self.app.usage.session.is_some(), "no session usage");
        let label = self.session().unwrap().model_label();
        assert_ne!(label, "Model", "the model was never named");
        eprintln!("live: replied {:?} with {label}", replies[0]);
        stand_in
    }

    /// The title, the model's label and the Auto mode, as plan 2's live test printed them.
    fn report_title_model_and_auto_mode(&mut self, stand_in: Option<String>) {
        self.report_title(stand_in);
        let label = self.session().unwrap().model_label();
        eprintln!("live: model label {label:?}");
        self.try_auto_mode();
    }

    /// Prints how many sessions each listing gave and how long it took, from asking to the
    /// answer. Fails on nothing.
    fn report_listings(&mut self) {
        let answered = self.wait_for(LIST_LIMIT, |live| {
            live.recent.is_some() && live.all.is_some()
        });
        match (&self.recent, &self.all) {
            (
                Some(Answered {
                    took: recent_took,
                    count: Some(recent),
                }),
                Some(Answered {
                    took: all_took,
                    count: Some(all),
                }),
            ) => eprintln!(
                "live: listed {recent} recent sessions in {} ms, {all} in all in {} ms",
                recent_took.as_millis(),
                all_took.as_millis()
            ),
            (recent, all) => eprintln!(
                "live: the listings did not both answer{}: recent {}, all {}",
                if answered {
                    String::new()
                } else {
                    format!(" within {} s", LIST_LIMIT.as_secs())
                },
                listing_word(recent, self.recent_asked),
                listing_word(all, self.all_asked)
            ),
        }
    }

    /// Prints what the look gave. Fails on nothing.
    fn report_look(&mut self) {
        if self.look_asked.is_none() {
            eprintln!("live: the app asked for no look");
            return;
        }
        self.wait_for(LOOK_LIMIT, |live| live.looked.is_some());
        match self.looked {
            Some((took, models, usage)) => eprintln!(
                "live: the look gave {models} models; usage known: {} ({} ms)",
                if usage { "yes" } else { "no" },
                took.as_millis()
            ),
            None => eprintln!(
                "live: the look gave no answer within {} s of the first turn's end",
                LOOK_LIMIT.as_secs()
            ),
        }
    }

    /// Shows the [`HISTORIES_READ`] most recent stored sessions one after the other, as a click
    /// on their rows would, and prints the largest history read, by its items, and how long
    /// that read took. The listing gives no sizes, so the largest is the largest of these. Then
    /// the test's session is shown again. Fails on nothing.
    fn report_histories(&mut self) {
        let mut stored: Vec<(i64, SessionKey)> = self
            .app
            .sessions
            .iter()
            .filter(|entry| {
                Some(entry.key) != self.key
                    && entry.session.history == HistoryState::NotLoaded
                    && entry.session.id.is_some()
            })
            .map(|entry| (entry.last_active_unix, entry.key))
            .collect();
        stored.sort_by_key(|&(last_active, _)| std::cmp::Reverse(last_active));
        stored.truncate(HISTORIES_READ);
        let mut unanswered = false;
        for &(_, key) in &stored {
            let effects = self.app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
            self.carry_out(effects);
            if !self.wait_for(HISTORY_LIMIT, |live| live.read.contains_key(&key)) {
                // The library thread is held up: what is asked next would only wait behind it.
                unanswered = true;
                break;
            }
        }
        let key = self.key.expect("the test's session");
        let effects = self.app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
        self.carry_out(effects);

        let largest = stored
            .iter()
            .filter_map(|(_, key)| self.read.get(key))
            .filter_map(|answer| Some((answer.count?, answer.took)))
            .max_by_key(|&(items, _)| items);
        let failed = stored
            .iter()
            .filter(|(_, key)| self.read.get(key).is_some_and(|a| a.count.is_none()))
            .count();
        match largest {
            Some((items, took)) => eprintln!(
                "live: read the largest of the {} most recent histories ({items} items) in {} ms",
                stored.len(),
                took.as_millis()
            ),
            None if stored.is_empty() => eprintln!("live: no stored session to read"),
            None => eprintln!("live: no recent history was read"),
        }
        if failed > 0 {
            eprintln!("live: {failed} of them could not be read");
        }
        if unanswered {
            eprintln!(
                "live: a history read gave no answer within {} s",
                HISTORY_LIMIT.as_secs()
            );
        }
    }

    /// Opens a second session to the side of the test's, in the same folder, as "Open to the
    /// side" does, turns Remote Control on and off in it once it is ready (see
    /// [`Live::report_remote`]), then sends it [`SIDE_MESSAGE`] and waits for its reply. Each
    /// session must hold its own message and reply only. The test's tile is focused again
    /// afterwards.
    fn side_turn(&mut self) {
        let key = self.key.expect("the test's session");
        let asked = Instant::now();
        let effects =
            self.app
                .new_session(self.folder.clone(), Place::Side, Leaving::ForgetIfEmpty);
        let side = self.app.focused().expect("the new session is shown");
        assert_ne!(side, key, "a second session opened");
        assert_eq!(
            self.tile_keys(),
            vec![key, side],
            "it opened in a tile right of the test's"
        );
        self.side = Some(side);
        self.carry_out(effects);
        let ready = self.wait_for(TURN_LIMIT, |live| live.phase_of(side) == Some(Phase::Idle));
        assert!(
            ready,
            "the session to the side never reached Idle within {} s: phase {:?}",
            TURN_LIMIT.as_secs(),
            self.phase_of(side)
        );
        // Not counted in the time to the reply.
        let paused = Instant::now();
        self.report_remote(side, "remote control before a first turn");
        let paused = paused.elapsed();
        let effects = self.app.submit(side, SIDE_MESSAGE);
        assert!(!effects.is_empty(), "the message to the side was not sent");
        self.carry_out(effects);
        let replied = |live: &Live| {
            live.phase_of(side) == Some(Phase::Idle)
                && !finished_replies(&live.items_of(side)).is_empty()
        };
        if !self.wait_for(TURN_LIMIT, replied) {
            panic!(
                "no reply in the session to the side within {} s (phase {:?}); conversation:\n{:#?}",
                TURN_LIMIT.as_secs(),
                self.phase_of(side),
                self.items_of(side)
            );
        }
        let took = asked.elapsed().saturating_sub(paused);

        let said = |items: &[ItemBody], message: &str| {
            items
                .iter()
                .any(|body| matches!(body, ItemBody::User { text, .. } if text.contains(message)))
        };
        let (first, second) = (self.items(), self.items_of(side));
        let side_replies = finished_replies(&second);
        assert_eq!(
            side_replies.len(),
            1,
            "one reply in the session to the side; conversation:\n{second:#?}"
        );
        assert!(
            side_replies[0].to_lowercase().contains("side"),
            "the reply to the side does not say side: {:?}",
            side_replies[0]
        );
        assert!(
            said(&second, SIDE_MESSAGE) && !said(&second, MESSAGE),
            "the session to the side holds the test's message; conversation:\n{second:#?}"
        );
        assert!(
            !said(&first, SIDE_MESSAGE) && finished_replies(&first).len() == 1,
            "the test's session holds the message or the reply to the side; conversation:\n\
             {first:#?}"
        );
        eprintln!(
            "live: the session to the side replied in {} ms; each reply is in its own session",
            took.as_millis()
        );
        let effects = self.app.focus_tile(key);
        self.carry_out(effects);
    }

    /// Renames the test's session [`NAME`], as a double click on its tile's title does. Its
    /// program must confirm the name, and a listing of the store must then give it: one is
    /// asked for each second until one does.
    fn rename(&mut self) {
        let key = self.key.expect("the test's session");
        let id = self.id().expect("the test's session has its id");
        let (titles_before, refused_before) = (self.titles.len(), self.refusals.len());
        let asked = Instant::now();
        let effects = self.app.rename(key, NAME);
        let open = self.open().expect("the test's session's program runs");
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::Send { open: to, command: SessionCommand::Rename(_) } if *to == open
            )),
            "the rename was not sent to the program: {effects:?}"
        );
        self.carry_out(effects);
        let confirmed = |live: &Live| live.titles[titles_before..].iter().any(|t| t == NAME);
        let refused = |live: &Live| {
            live.refusals[refused_before..]
                .iter()
                .any(|(what, _)| *what == CommandKind::Rename)
        };
        self.wait_for(RENAME_LIMIT, |live| confirmed(live) || refused(live));
        assert!(
            confirmed(self),
            "the program did not confirm the name within {} s; refused: {:?}",
            RENAME_LIMIT.as_secs(),
            self.refusal(CommandKind::Rename)
        );
        let took = asked.elapsed();

        let listed_from = Instant::now();
        let deadline = listed_from + NAMED_LIMIT;
        loop {
            self.asked_for_name = true;
            self.listed_name = None;
            self.backend
                .send(ToBackend::ListSessions { since_unix: None });
            let answered = self
                .wait_for(deadline.saturating_duration_since(Instant::now()), |live| {
                    live.listed_name.is_some()
                });
            if self.listed_name == Some(true) {
                break;
            }
            assert!(
                answered && Instant::now() + Duration::from_secs(1) < deadline,
                "no listing gave the new name within {} s of its confirmation",
                NAMED_LIMIT.as_secs()
            );
            std::thread::sleep(Duration::from_secs(1));
        }
        self.asked_for_name = false;
        assert_eq!(self.id(), Some(id), "the same session was renamed");
        eprintln!(
            "live: the name was confirmed in {} ms; a listing gave it {} ms later",
            took.as_millis(),
            listed_from.elapsed().as_millis()
        );
    }

    /// Prints the effort level the test's session started with, from the agent's first word on
    /// it. Then chooses another of the model's levels, as the effort picker does, and the first
    /// level again, printing what came of each. Fails on nothing.
    fn report_effort(&mut self) {
        self.wait_for(EFFORT_LIMIT, |live| !live.efforts.is_empty());
        let start = match self.efforts.first() {
            Some((_, level)) => {
                eprintln!(
                    "live: effort at start: {}",
                    level.as_deref().unwrap_or("none")
                );
                level.clone()
            }
            None => {
                eprintln!(
                    "live: effort at start: not said within {} s",
                    EFFORT_LIMIT.as_secs()
                );
                None
            }
        };
        let levels = self
            .session()
            .map(|session| session.effort_levels().to_vec())
            .unwrap_or_default();
        if levels.is_empty() {
            eprintln!("live: effort: the app knows no levels for this model");
            return;
        }
        let Some(other) = levels
            .into_iter()
            .find(|level| Some(level) != start.as_ref())
        else {
            eprintln!("live: effort did not change: the model lists no other level");
            return;
        };
        self.change_effort(&other);
        match start {
            Some(start) if self.session().and_then(|s| s.effort.as_ref()) == Some(&start) => {
                eprintln!("live: effort did not change: {start} is still in use");
            }
            Some(start) => self.change_effort(&start),
            None => eprintln!("live: effort did not change: there is no level to go back to"),
        }
    }

    /// Chooses `level` for the test's session and prints the level the agent then said and
    /// how long that took, or why it did not change. Fails on nothing.
    fn change_effort(&mut self, level: &str) {
        let key = self.key.expect("the test's session");
        let (said_before, refused_before) = (self.efforts.len(), self.refusals.len());
        let asked = Instant::now();
        let effects = self.app.choose_effort(key, level);
        let sent = effects.iter().any(|effect| {
            matches!(
                effect,
                Effect::Send {
                    command: SessionCommand::SetEffort(_),
                    ..
                }
            )
        });
        self.carry_out(effects);
        if !sent {
            eprintln!("live: effort did not change: the app sent nothing for {level}");
            return;
        }
        let refused = |live: &Live| {
            live.refusals[refused_before..]
                .iter()
                .any(|(what, _)| *what == CommandKind::SetEffort)
        };
        self.wait_for(EFFORT_LIMIT, |live| {
            live.efforts.len() > said_before || refused(live)
        });
        if refused(self) {
            let reason = self.refusal(CommandKind::SetEffort).unwrap_or_default();
            eprintln!("live: effort did not change: {reason}");
        } else if let Some((at, said)) = self.efforts.get(said_before) {
            eprintln!(
                "live: effort changed to {} in {} ms",
                said.as_deref().unwrap_or("none"),
                at.duration_since(asked).as_millis()
            );
        } else {
            eprintln!(
                "live: effort did not change: no answer in {} s",
                EFFORT_LIMIT.as_secs()
            );
        }
    }

    /// Prints the share of the context window in use and whether the cost is known, as the
    /// first turn left them. Fails on nothing.
    fn report_context_and_cost(&self) {
        let session = self.session().expect("the test's session");
        let context = match session.context_used {
            Some(used) => format!("{}%", (used * 100.0).round().clamp(0.0, 100.0) as u8),
            None => "unknown".to_string(),
        };
        eprintln!(
            "live: context {context} and cost known: {}",
            if session.cost_usd.is_some() {
                "yes"
            } else {
                "no"
            }
        );
    }

    /// Prints what the program said the context was made of after the session started and
    /// after its first turn: how many parts and of which kinds, or none. Waits
    /// [`BREAKDOWN_LIMIT`] for the one after the turn. Names no part. Fails on nothing.
    fn report_breakdown(&mut self) {
        self.wait_for(BREAKDOWN_LIMIT, |live| {
            live.breakdowns.iter().any(|(after, _)| *after)
        });
        for (after, lead) in [(false, "at start"), (true, "after a turn")] {
            let found = self
                .breakdowns
                .iter()
                .find(|(turn_ended, _)| *turn_ended == after)
                .map(|(_, breakdown)| breakdown);
            match found {
                Some(breakdown) => {
                    let mut kinds: Vec<&str> = Vec::new();
                    for part in &breakdown.parts {
                        let kind = match part.kind {
                            ContextKind::Used => "used",
                            ContextKind::Buffer => "buffer",
                            ContextKind::Free => "free",
                        };
                        if !kinds.contains(&kind) {
                            kinds.push(kind);
                        }
                    }
                    eprintln!(
                        "live: context breakdown {lead}: {} parts (kinds: {})",
                        breakdown.parts.len(),
                        kinds.join(", ")
                    );
                }
                None => eprintln!("live: context breakdown {lead}: none"),
            }
        }
    }

    /// Turns Remote Control on for `key`'s session, as its switch does, and prints whether the
    /// program said it is on and how long that took, or that it refused and why, or that it did
    /// not answer. A refusal can come more than once, as a state and as a reply: the test
    /// listens [`REFUSAL_LISTEN`] longer and prints every kind of failure it heard, in order.
    /// Then turns it off and prints what came of that. Every line starts with `lead`. The
    /// address is never printed: only the state's name. Fails on nothing.
    fn report_remote(&mut self, key: SessionKey, lead: &str) {
        let from = self.remotes.len();
        let asked = Instant::now();
        let effects = self.app.set_remote(key, true);
        let sent = sends_remote(&effects);
        self.carry_out(effects);
        if !sent {
            eprintln!("live: {lead}: the app sent nothing to turn it on");
            return;
        }
        let settled = |said: RemoteSaid| matches!(said, RemoteSaid::On | RemoteSaid::Failed(_));
        self.wait_for(REMOTE_LIMIT, |live| {
            live.remote_said(key, from)
                .into_iter()
                .any(|(_, said)| settled(said))
        });
        let said = self.remote_said(key, from);
        match said.iter().find(|&&(_, said)| settled(said)) {
            Some((at, RemoteSaid::On)) => eprintln!(
                "live: {lead}: on in {} ms",
                at.duration_since(asked).as_millis()
            ),
            Some((_, RemoteSaid::Failed(_))) => {
                self.wait_for(REFUSAL_LISTEN, |_| false);
                let mut kinds: Vec<String> = Vec::new();
                for (_, said) in self.remote_said(key, from) {
                    if let RemoteSaid::Failed(why) = said {
                        let kind = format!("{why:?}");
                        if !kinds.contains(&kind) {
                            kinds.push(kind);
                        }
                    }
                }
                eprintln!("live: {lead}: refused ({})", kinds.join(", then "));
            }
            _ => match said.last() {
                Some((_, last)) => eprintln!(
                    "live: {lead}: no answer in {} s (last: {last:?})",
                    REMOTE_LIMIT.as_secs()
                ),
                None => eprintln!("live: {lead}: no answer in {} s", REMOTE_LIMIT.as_secs()),
            },
        }
        self.remote_off(key, lead);
    }

    /// Turns Remote Control off for `key`'s session and prints what came of it. A program that
    /// has not answered being turned on is asked too, by the app itself: a late "on" must not
    /// outlive the check.
    fn remote_off(&mut self, key: SessionKey, lead: &str) {
        let from = self.remotes.len();
        let effects = self.app.set_remote(key, false);
        if !sends_remote(&effects) {
            self.carry_out(effects);
            eprintln!("live: {lead}: off, as it already was");
            return;
        }
        self.carry_out(effects);
        let settled = |said: RemoteSaid| matches!(said, RemoteSaid::Off | RemoteSaid::Refused);
        self.wait_for(REMOTE_LIMIT, |live| {
            live.remote_said(key, from)
                .into_iter()
                .any(|(_, said)| settled(said))
        });
        let said = self.remote_said(key, from);
        match said.iter().find(|&&(_, said)| settled(said)) {
            Some((_, RemoteSaid::Off)) => eprintln!("live: {lead}: off"),
            Some(_) => eprintln!("live: {lead}: turning it off was refused"),
            None => eprintln!(
                "live: {lead}: turning it off had no answer in {} s",
                REMOTE_LIMIT.as_secs()
            ),
        }
    }

    /// What `key`'s program said of Remote Control from the `from`th word noted on, with when.
    fn remote_said(&self, key: SessionKey, from: usize) -> Vec<(Instant, RemoteSaid)> {
        self.remotes[from..]
            .iter()
            .filter(|(of, _, _)| *of == key)
            .map(|&(_, at, said)| (at, said))
            .collect()
    }

    /// The test's session whose program has the open number `open`.
    fn key_of(&self, open: u64) -> Option<SessionKey> {
        self.ours()
            .into_iter()
            .find(|key| self.app.sessions.get(*key).and_then(|entry| entry.open) == Some(open))
    }

    /// Checks that the second run brought back both tiles of the first, `id`'s session (named
    /// [`NAME`]) focused on the left and `side_id`'s on its right.
    fn restored(&mut self, id: &SessionId, side_id: &SessionId) {
        let ids: Vec<Option<SessionId>> = self
            .tile_keys()
            .into_iter()
            .map(|key| {
                self.app
                    .sessions
                    .get(key)
                    .and_then(|e| e.session.id.clone())
            })
            .collect();
        assert_eq!(
            ids,
            vec![Some(id.clone()), Some(side_id.clone())],
            "the second run did not bring back both tiles in their order"
        );
        assert_eq!(
            self.app.focused(),
            self.tile_keys().first().copied(),
            "the left tile, the test's, is focused"
        );
        assert_eq!(
            self.title().as_deref(),
            Some(NAME),
            "the test's session is not shown under its new name"
        );
        // The session to the side rests in this run: no program of its is started to close.
        eprintln!("live: restored 2 tiles, the focus on the first");
    }

    /// Writes down, for each of the test's sessions that has the agent's id, whether a program
    /// runs for it now: the app holds an open for it from the start of its program to the
    /// program's exit. Called after everything that can start or end one, so [`Tidy`] knows
    /// which sessions it may remove if the test leaves early. Never panics: it also runs while
    /// a panic unwinds.
    fn track(&self) {
        let Ok(mut made) = self.made.try_borrow_mut() else {
            return;
        };
        let now_unix = unix_now();
        for key in self.ours().into_iter().chain(self.attached) {
            if let Some(entry) = self.app.sessions.get(key)
                && let Some(id) = &entry.session.id
            {
                made.see(id, entry.open.is_some(), now_unix);
            }
        }
    }

    /// The sessions in the tiles, left to right.
    fn tile_keys(&self) -> Vec<SessionKey> {
        self.app.tiles.keys()
    }

    /// The test's sessions in this run: its own, and the one to the side once there is one.
    fn ours(&self) -> Vec<SessionKey> {
        self.key.into_iter().chain(self.side).collect()
    }

    /// Waits up to [`CLOSE_LIMIT`] until no program runs for any of the sessions `ids` names
    /// (the app holds it no more, or its open has gone), and gives the ids of those whose
    /// program still runs then. Only takes in what comes; fails on nothing.
    fn wait_until_ended(&mut self, ids: &[Option<SessionId>]) -> Vec<SessionId> {
        let running = |live: &Live| -> Vec<SessionId> {
            ids.iter()
                .flatten()
                .filter(|id| {
                    live.app
                        .sessions
                        .iter()
                        .any(|entry| entry.session.id.as_ref() == Some(*id) && entry.open.is_some())
                })
                .cloned()
                .collect()
        };
        let deadline = Instant::now() + CLOSE_LIMIT;
        loop {
            self.take_in();
            let still = running(self);
            if still.is_empty() || Instant::now() >= deadline {
                return still;
            }
            std::thread::sleep(TICK);
        }
    }

    /// The id of the session sent attachments, once it has one.
    fn attached_id(&self) -> Option<SessionId> {
        self.attached
            .and_then(|key| self.app.sessions.get(key))
            .and_then(|entry| entry.session.id.clone())
    }

    fn side_id(&self) -> Option<SessionId> {
        self.side
            .and_then(|key| self.app.sessions.get(key))
            .and_then(|entry| entry.session.id.clone())
    }

    /// Waits for the second run to show the test's session `id` again, as kept by the first,
    /// with its history read from the store, and checks that the first message and its reply
    /// are in it.
    fn read_back(&mut self, id: &SessionId) {
        let shown_id = |live: &Live| live.app.focused_entry().and_then(|e| e.session.id.clone());
        let read = self.wait_for(READ_BACK_LIMIT, |live| {
            live.app.focused_entry().is_some_and(|entry| {
                entry.session.id.as_ref() == Some(id)
                    && matches!(
                        entry.session.history,
                        HistoryState::Loaded | HistoryState::Failed(_)
                    )
            })
        });
        if !read {
            let shown = shown_id(self);
            panic!(
                "the second run did not show the session again with its history within {} s: \
                 listing {:?}; the session shown {}; its history {:?}",
                READ_BACK_LIMIT.as_secs(),
                self.app.listing,
                match shown {
                    Some(shown) if shown == *id => "is the test's",
                    Some(_) => "is another",
                    None => "is none",
                },
                self.app.focused_entry().map(|entry| &entry.session.history)
            );
        }
        self.key = self.app.focused();
        let session = self.session().expect("the test's session");
        if let HistoryState::Failed(reason) = &session.history {
            panic!("the session's history could not be read back from the store: {reason}");
        }
        assert_eq!(
            session.phase,
            Phase::Resting,
            "it rests, its program not started"
        );
        let items = self.items();
        let first = items
            .iter()
            .any(|body| matches!(body, ItemBody::User { text, .. } if text.contains(MESSAGE)));
        let replies = finished_replies(&items);
        assert!(
            first
                && replies
                    .iter()
                    .any(|reply| reply.to_lowercase().contains("ok")),
            "the history read back lacks the first message or its reply; conversation:\n\
             {items:#?}"
        );
        eprintln!(
            "live: read the session back from the store ({} items)",
            items.len()
        );
    }

    /// Sends the second message to the resting session and waits for the reply. The program
    /// must resume the same id. The store is asked first when the session last changed; the
    /// first run's saved state says this app closed the session then, so the app is not
    /// expected to ask whether another app has it open. It prints which happened, and when it
    /// did ask (the program wrote more than `OWN_EXIT_SLACK_SECONDS` after the first run
    /// closed) it continues here.
    fn resume(&mut self, id: &SessionId) {
        let key = self.key.expect("the test's session");
        let effects = self.app.tick(unix_now());
        self.carry_out(effects);
        let effects = self.app.submit(key, SECOND_MESSAGE);
        assert!(!effects.is_empty(), "the second message woke nothing");
        self.carry_out(effects);

        // A session this run has not run yet is first checked in the store.
        let decided = self.wait_for(ACTIVITY_LIMIT, |live| {
            live.app.prompt(key).is_some() || live.opened.is_some()
        });
        assert!(
            decided,
            "neither the program opened nor did the app ask within {} s of the second \
             message; the store answered when the session changed: {}",
            ACTIVITY_LIMIT.as_secs(),
            if self.activity.is_some() { "yes" } else { "no" }
        );
        let changed = match self.activity {
            Some(Some(unix)) => format!("{} s before", self.app.now_unix - unix),
            Some(None) => "never, by the store".into(),
            None => "unknown".into(),
        };
        if let Some(Prompt::OpenElsewhere { .. }) = self.app.prompt(key) {
            eprintln!("live: the session changed {changed}; the app asked first, continued here");
            let (effects, back) = self.app.answer_prompt(key, PromptChoice::ContinueHere);
            assert_eq!(back, None, "continuing gives no text back");
            self.carry_out(effects);
        } else {
            eprintln!("live: the session changed {changed}; the app did not ask");
        }
        let spec = self.opened.clone().expect("a program was opened");
        assert_eq!(
            spec.resume.as_ref(),
            Some(id),
            "it resumes the same session"
        );
        assert!(!spec.fork, "it resumes, not forks");
        self.running = true;

        if !self.wait_for(TURN_LIMIT, |live| live.has_reply_to(SECOND_MESSAGE)) {
            panic!(
                "no reply to the second message within {} s (phase {:?}); conversation:\n{:#?}",
                TURN_LIMIT.as_secs(),
                self.phase(),
                self.items()
            );
        }
        let session = self.session().expect("the test's session");
        assert_eq!(session.id.as_ref(), Some(id), "the same session carried on");
        let items = self.items();
        let replies = finished_replies(&items);
        let last = replies.last().expect("a reply");
        assert!(
            last.to_lowercase().contains("again"),
            "the second reply does not say again: {last:?}"
        );
        let label = session.model_label();
        assert_ne!(label, "Model", "the model was never named");
        eprintln!(
            "live: resumed with {label} in {}",
            mode_label(&session.mode)
        );
        self.report_resumed_effort(&spec);
    }

    /// Prints the level the app passed with `--effort` when it resumed the session, from its
    /// stored history, and the first level its program then said. Fails on nothing.
    fn report_resumed_effort(&mut self, spec: &mr_agent::StartSpec) {
        let passed = spec.effort.as_deref().unwrap_or("none").to_string();
        self.wait_for(EFFORT_LIMIT, |live| !live.efforts.is_empty());
        let says = match self.efforts.first() {
            Some((_, level)) => level.as_deref().unwrap_or("none").to_string(),
            None => format!("nothing within {} s", EFFORT_LIMIT.as_secs()),
        };
        eprintln!("live: resumed with effort {passed}, the program says {says}");
    }

    /// Opens a session of its own to the side, in the test's folder, as "Open to the side"
    /// does, and sends it [`ATTACHED_MESSAGE`] with a red image made here, pasted, and
    /// [`NOTE_NAME`], written into the test's folder and attached by its path, as the window
    /// attaches them: read on the preparing thread, then sent through the backend's writer.
    /// Prints what the message went out with, whether the answer names the colour and the
    /// file's word, then what that session's stored history gives back for the message, then
    /// sends [`NOISE_MESSAGE`] with an image of noise a little under the largest sent as it is
    /// and prints whether that turn was answered. Then the session is closed. Fails on nothing
    /// its program does, nor on a note it cannot write or a session that is not made: it says so
    /// and leaves the attachments out.
    fn attachments(&mut self) {
        let note = self.folder.join(NOTE_NAME);
        if let Err(error) = std::fs::write(&note, NOTE) {
            eprintln!(
                "live: the note could not be written ({:?}); attachments are not tried",
                error.kind()
            );
            return;
        }
        let known: Vec<SessionKey> = self.app.sessions.iter().map(|entry| entry.key).collect();
        let effects =
            self.app
                .new_session(self.folder.clone(), Place::Side, Leaving::ForgetIfEmpty);
        let made = self
            .app
            .sessions
            .iter()
            .map(|entry| entry.key)
            .find(|key| !known.contains(key));
        self.attached = made;
        self.carry_out(effects);
        let Some(key) = made else {
            eprintln!("live: no new session was made for attachments; they are not tried");
            return;
        };
        let ready = self.wait_for(TURN_LIMIT, |live| {
            matches!(live.phase_of(key), Some(Phase::Idle | Phase::Ended { .. }))
        });
        if !ready || self.phase_of(key) != Some(Phase::Idle) {
            eprintln!(
                "live: the session for attachments was not ready within {} s",
                TURN_LIMIT.as_secs()
            );
            self.close_attached(key);
            return;
        }

        let sources = vec![Source::Path(note.clone()), Source::Pasted(red_png())];
        if let Some((before, attachments)) = self.send_with(key, ATTACHED_MESSAGE, sources, None) {
            let images = attachments
                .iter()
                .filter(|attachment| matches!(attachment, Attachment::Image { .. }))
                .count();
            let files: Vec<&Path> = attachments
                .iter()
                .filter_map(|attachment| match attachment {
                    Attachment::File { path } => Some(path.as_path()),
                    Attachment::Image { .. } => None,
                })
                .collect();
            let sent = attachments.iter().find_map(|attachment| match attachment {
                Attachment::Image { data, .. } => Some(data.clone()),
                Attachment::File { .. } => None,
            });
            if images == 1 && files == [note.as_path()] {
                eprintln!("live: image and file sent");
            } else {
                eprintln!(
                    "live: the message went out with {images} image(s) and {} file(s), not the \
                     image and the file attached",
                    files.len()
                );
            }
            let finished = self.wait_for_end(key, before);
            let answer = self.answer_after(key, ATTACHED_MESSAGE).to_lowercase();
            let names = |word: &str| {
                finished
                    && answer
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|part| part == word)
            };
            eprintln!("live: the answer names the colour: {}", yes(names("red")));
            eprintln!(
                "live: the answer names the file's word: {}",
                yes(names(NOTE_WORD))
            );
            self.report_stored(key, sent);
        }

        let noise = noise_png();
        let bytes = noise.len();
        let sources = vec![Source::Pasted(noise)];
        if let Some((before, attachments)) =
            self.send_with(key, NOISE_MESSAGE, sources, Some(bytes))
        {
            let kept = attachments.iter().any(|attachment| {
                matches!(attachment, Attachment::Image { data, .. } if data.len() == bytes)
            });
            let finished = self.wait_for_end(key, before);
            if kept {
                eprintln!("live: a 3 MB line went through: {}", yes(finished));
            } else {
                eprintln!("live: the noise image was made smaller, so no 3 MB line was sent");
            }
        }
        self.background_task(key);
        self.close_attached(key);
    }

    /// Asks `key`'s session, the one sent attachments, to run `sleep 20` in the background
    /// and to answer with one word, and prints whether a background task record that counted
    /// at least one task arrived and whether the adapter's own guard counted the tool call.
    /// The program asks before it runs a command: the test answers as the user would, through
    /// the app, and allows only the shell tool with `sleep` and a number as its whole command. Any
    /// other request is denied, and the first line then reads "not tried". Nothing of a
    /// request is printed. Every wait has a limit, and the `sleep` ends by itself. Fails on
    /// nothing.
    fn background_task(&mut self, key: SessionKey) {
        const RECORD: &str =
            "live: a background task record that counted at least one task arrived";
        if self.phase_of(key) != Some(Phase::Idle) {
            eprintln!("{RECORD}: not tried (the session was not ready for a message)");
            return;
        }
        self.tasks = Tasks {
            watching: true,
            ..Tasks::default()
        };
        let before = self.attached_ends.len();
        let effects = self.app.submit(key, BACKGROUND_MESSAGE);
        self.carry_out(effects);
        let deadline = Instant::now() + TURN_LIMIT;
        let mut other = false;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let settled = |live: &Live| {
                live.attached_ends.len() > before
                    || !matches!(live.phase_of(key), Some(Phase::Working))
            };
            self.wait_for(left, settled);
            if self.phase_of(key) != Some(Phase::NeedsUser) {
                break;
            }
            for (id, allowed) in self.waiting_requests(key) {
                other |= !allowed;
                let answer = if allowed {
                    Answer::Allow { always: false }
                } else {
                    Answer::Deny { message: None }
                };
                let effects = self.app.answer(key, &id, answer);
                self.carry_out(effects);
            }
            // A request that cannot be answered so (a question, a plan), or time that is up.
            if self.phase_of(key) == Some(Phase::NeedsUser) || left.is_zero() {
                other |= self.phase_of(key) == Some(Phase::NeedsUser);
                let effects = self.app.stop(key);
                self.carry_out(effects);
                self.wait_for(CLOSE_LIMIT, |live| {
                    live.attached_ends.len() > before
                        || !matches!(live.phase_of(key), Some(Phase::NeedsUser | Phase::Working))
                });
                break;
            }
        }
        if other {
            self.tasks.watching = false;
            eprintln!("{RECORD}: not tried (the program asked for something else)");
            return;
        }
        if self.attached_ends.len() == before {
            eprintln!(
                "live: the turn that starts a background task did not end within {} s",
                TURN_LIMIT.as_secs()
            );
        }
        // The record may come a moment after the call's own result.
        self.wait_for(RECORD_LIMIT, |live| live.tasks.record_counted);
        self.tasks.watching = false;
        if self.tasks.shell_calls == 0 {
            eprintln!("{RECORD}: not tried (the program ran no shell command)");
            return;
        }
        eprintln!("{RECORD}: {}", yes(self.tasks.record_counted));
        eprintln!(
            "live: the adapter's own guard counted the tool call: {}",
            yes(self.tasks.guard_counted)
        );
    }

    /// The requests that wait in `key`'s session, each with whether the test allows it: a
    /// permission request of the shell tool for a command [`allowed_command`] takes.
    fn waiting_requests(&self, key: SessionKey) -> Vec<(RequestId, bool)> {
        self.items_of(key)
            .into_iter()
            .filter_map(|body| match body {
                ItemBody::Request {
                    request,
                    state: RequestState::Waiting,
                } => {
                    let allowed = matches!(
                        &request.kind,
                        UserRequestKind::Permission {
                            tool,
                            detail: ToolDetail::Command { command },
                            ..
                        } if tool == SHELL_TOOL && allowed_command(command)
                    );
                    Some((request.id, allowed))
                }
                _ => None,
            })
            .collect()
    }

    /// Attaches `sources` to `key`'s session, waits until every chip is read, and sends `text`
    /// with them. Returns how many turns of the session had ended before and the attachments the
    /// `Send` carried, once the message was sent; prints why and returns `None` when it was not.
    /// With `bytes`, prints a line when the one image was not kept at that many bytes. Fails on
    /// nothing.
    fn send_with(
        &mut self,
        key: SessionKey,
        text: &str,
        sources: Vec<Source>,
        bytes: Option<usize>,
    ) -> Option<(usize, Vec<Attachment>)> {
        let lead = if bytes.is_some() {
            "the noise image"
        } else {
            "the image and the file"
        };
        let effects = self.app.attach(key, sources);
        self.carry_out(effects);
        let chips = |live: &Live| {
            live.app
                .sessions
                .get(key)
                .map(|entry| entry.session.chips.clone())
                .unwrap_or_default()
        };
        let read = self.wait_for(PREPARE_LIMIT, |live| {
            chips(live)
                .iter()
                .all(|chip| chip.state != ChipState::Reading)
        });
        if !read {
            eprintln!(
                "live: {lead}: not read within {} s; nothing was sent",
                PREPARE_LIMIT.as_secs()
            );
            return None;
        }
        let held = chips(self);
        if let Some(problem) = held.iter().find_map(|chip| match chip.state {
            ChipState::Problem(problem) => Some(problem),
            _ => None,
        }) {
            eprintln!(
                "live: {lead}: a chip says {:?}; nothing was sent",
                problem.words()
            );
            return None;
        }
        if let Some(bytes) = bytes {
            let kept = held.iter().any(|chip| {
                matches!(
                    &chip.state,
                    ChipState::Ready(prepared) if matches!(
                        &prepared.attachment,
                        Attachment::Image { data, .. } if data.len() == bytes
                    )
                )
            });
            if !kept {
                eprintln!("live: the noise image was made smaller; the line is shorter than meant");
            }
        }
        let before = self.attached_ends.len();
        let effects = self.app.submit(key, text);
        let sent = effects.iter().find_map(|effect| match effect {
            Effect::Send {
                command: SessionCommand::Send { attachments, .. },
                ..
            } => Some(attachments.clone()),
            _ => None,
        });
        self.carry_out(effects);
        let Some(attachments) = sent else {
            eprintln!("live: {lead}: the app sent no message");
            return None;
        };
        Some((before, attachments))
    }

    /// Waits for the turn of `key`'s session after the `before`th to end, and says whether it
    /// finished. A turn that asks the user for something is stopped, and one whose program ends
    /// does not finish; each prints a line. Fails on nothing.
    fn wait_for_end(&mut self, key: SessionKey, before: usize) -> bool {
        let settled = |live: &Live| {
            live.attached_ends.len() > before
                || matches!(
                    live.phase_of(key),
                    Some(Phase::NeedsUser | Phase::Ended { .. })
                )
        };
        let ended = self.wait_for(TURN_LIMIT, settled);
        match self.phase_of(key) {
            Some(Phase::NeedsUser) => {
                eprintln!("live: the turn asked the user for something; it is stopped");
                let effects = self.app.stop(key);
                self.carry_out(effects);
                self.wait_for(TURN_LIMIT, |live| live.attached_ends.len() > before);
                return false;
            }
            Some(Phase::Ended { clean, .. }) => {
                eprintln!("live: the session's program ended in the turn (clean: {clean})");
                return false;
            }
            _ => {}
        }
        if !ended {
            eprintln!(
                "live: the turn did not end within {} s",
                TURN_LIMIT.as_secs()
            );
            return false;
        }
        let finished = self.attached_ends[before];
        if !finished {
            eprintln!("live: the turn failed");
        }
        finished
    }

    /// The finished replies in `key`'s session after the user's message that starts with
    /// `message`, joined.
    fn answer_after(&self, key: SessionKey, message: &str) -> String {
        let items = self.items_of(key);
        let asked = items.iter().rposition(
            |body| matches!(body, ItemBody::User { text, .. } if text.starts_with(message)),
        );
        let after = asked.map_or(&items[..0], |at| &items[at..]);
        finished_replies(after)
            .into_iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Reads `key`'s session's stored history through the backend, as a stored session's is
    /// read, and prints what it gives for the first message of the user's: how many images and
    /// files, and whether its text is the message as typed. Then whether that message has its
    /// own id in the store, and what reading its first image again by that id gives, compared
    /// with `sent`, the image the message went out with. Fails on nothing.
    fn report_stored(&mut self, key: SessionKey, sent: Option<ImageData>) {
        let Some(id) = self
            .app
            .sessions
            .get(key)
            .and_then(|entry| entry.session.id.clone())
        else {
            eprintln!("live: the session for attachments has no id; its history is not read");
            return;
        };
        self.stored = None;
        self.stored_asked = true;
        self.backend.send(ToBackend::ReadHistory {
            key,
            id: id.clone(),
            max_items: HISTORY_ITEMS,
        });
        self.wait_for(HISTORY_LIMIT, |live| live.stored.is_some());
        self.stored_asked = false;
        let items = match self.stored.take() {
            Some(Some(items)) => items,
            Some(None) => {
                eprintln!("live: the stored history could not be read");
                return;
            }
            None => {
                eprintln!(
                    "live: the stored history gave no answer within {} s",
                    HISTORY_LIMIT.as_secs()
                );
                return;
            }
        };
        let users: Vec<(&String, usize, usize, Option<&String>)> = items
            .iter()
            .filter_map(|item| match item {
                HistoryItem::User {
                    text,
                    image_count,
                    files,
                    stored,
                    ..
                } => Some((text, *image_count, files.len(), stored.as_ref())),
                _ => None,
            })
            .collect();
        eprintln!(
            "live: the stored history holds {} message(s) of the user's",
            users.len()
        );
        match users.first() {
            Some((text, images, files, stored)) => {
                eprintln!(
                    "live: the stored message shows {images} image(s) and {files} file(s), text \
                     kept whole: {}",
                    yes(text.as_str() == ATTACHED_MESSAGE)
                );
                // Each image read comes back as a picture; one not read never does.
                let images = *images;
                self.wait_for(HISTORY_LIMIT, |live| live.stored_read >= images);
                eprintln!(
                    "live: the stored message's images with bytes the app could read: {} of \
                     {images}",
                    self.stored_read
                );
                eprintln!(
                    "live: the stored message has an id: {}",
                    yes(stored.is_some())
                );
                match (stored, &sent) {
                    (Some(message), Some(sent)) => {
                        let message = (*message).clone();
                        self.report_image_read(id, message, sent);
                    }
                    (Some(_), None) => eprintln!(
                        "live: its image read again by that id: not tried (no image was sent)"
                    ),
                    (None, _) => {}
                }
            }
            None => eprintln!("live: the stored history holds no message of the user's"),
        }
    }

    /// Reads again, through the backend, the first image of the stored message `message` of
    /// session `id`, as a press on a picture of a reopened session asks for it, and prints how
    /// many bytes came and whether they are `sent`'s. Fails on nothing.
    fn report_image_read(&mut self, id: SessionId, message: String, sent: &ImageData) {
        self.image_asked = Some(READ_PICTURE);
        self.image_read = None;
        self.backend.send(ToBackend::ReadImage {
            picture: READ_PICTURE,
            id,
            message,
            index: 0,
        });
        self.wait_for(HISTORY_LIMIT, |live| live.image_read.is_some());
        self.image_asked = None;
        match self.image_read.take() {
            Some(Ok(read)) => eprintln!(
                "live: its image read again by that id: {} bytes, the same as were sent: {}",
                read.len(),
                yes(read.as_bytes() == sent.as_bytes())
            ),
            Some(Err(reason)) => {
                eprintln!("live: its image read again by that id: not read ({reason})");
            }
            None => eprintln!(
                "live: its image read again by that id: no answer within {} s",
                HISTORY_LIMIT.as_secs()
            ),
        }
    }

    /// Leaves the test's session idle, the idle length set to [`QUIET_AFTER`] and the app's
    /// clock ticked as the window ticks it, until the app stops its program unnoticed; prints
    /// whether the program ended cleanly. Then sends [`QUIET_MESSAGE`], which starts the
    /// session again, and prints whether it answered and whether its model, effort and mode are
    /// those it had before the stop. From here on the test's session is reported on and never
    /// failed on (`quiet`): every wait ends by its limit and says what it found, and the steps
    /// after this one run whatever happened. The idle length is set back to half an hour once
    /// the idle `Close` is seen, before the session is started again, and in any case after.
    fn stop_while_idle(&mut self) {
        self.quiet = true;
        self.app.set_idle_after(QUIET_AFTER);
        self.stop_and_start_again();
        self.app.set_idle_after(IDLE_SECONDS);
    }

    /// [`Live::stop_while_idle`]'s steps, each of which may end the part with a line.
    fn stop_and_start_again(&mut self) {
        let key = self.key.expect("the test's session");
        let shown = |live: &Live| {
            live.session()
                .map(|s| (s.model_label(), s.effort.clone(), s.mode.clone()))
        };
        let before = shown(self);
        let Some(open) = self.open() else {
            eprintln!("live: the test's session has no program running; no idle stop is tried");
            return;
        };
        let exit = |live: &Live| {
            live.exits
                .iter()
                .find(|(exited, _)| *exited == open)
                .map(|(_, clean)| *clean)
        };
        // `take_in` ticks the clock, and the tick sends the idle `Close`.
        self.wait_for(QUIET_LIMIT, |live| {
            live.closes.contains(&open) || exit(live).is_some()
        });
        if !self.closes.contains(&open) {
            match exit(self) {
                Some(clean) => eprintln!(
                    "live: the program ended by itself before the idle stop (clean: {clean})"
                ),
                None => eprintln!(
                    "live: the program was not stopped for idling within {} s",
                    QUIET_LIMIT.as_secs()
                ),
            }
            return;
        }
        // The idle `Close` is seen: the usual length again, so the program that is started
        // next is not stopped for idling before it has answered.
        self.app.set_idle_after(IDLE_SECONDS);
        self.wait_for(CLOSE_LIMIT, |live| exit(live).is_some());
        let Some(clean) = exit(self) else {
            eprintln!(
                "live: a program closed while idle had not ended {} s after Close",
                CLOSE_LIMIT.as_secs()
            );
            return;
        };
        eprintln!(
            "live: a program closed while idle ended cleanly: {}",
            yes(clean)
        );
        match self.phase() {
            Some(Phase::Resting) => {}
            Some(Phase::Ended { clean, .. }) => {
                eprintln!(
                    "live: after the stop the session ended (clean: {clean}) instead of resting; \
                     it is not started again"
                );
                return;
            }
            _ => {
                eprintln!(
                    "live: after the stop the session does not rest; it is not started again"
                );
                return;
            }
        }

        self.opened = None;
        let effects = self.app.submit(key, QUIET_MESSAGE);
        self.carry_out(effects);
        let decided = self.wait_for(ACTIVITY_LIMIT, |live| {
            live.app.prompt(key).is_some() || live.opened.is_some()
        });
        if let Some(Prompt::OpenElsewhere { .. }) = self.app.prompt(key) {
            eprintln!(
                "live: started again: the app asked whether another app has it; continued here"
            );
            let (effects, _) = self.app.answer_prompt(key, PromptChoice::ContinueHere);
            self.carry_out(effects);
        } else if !decided {
            eprintln!(
                "live: started again, the session answered: no (no program was opened within {} \
                 s)",
                ACTIVITY_LIMIT.as_secs()
            );
            eprintln!("live: started again with the model, effort and mode it had: no");
            return;
        }
        let settled = |live: &Live| {
            live.has_reply_to(QUIET_MESSAGE)
                || matches!(
                    live.phase(),
                    Some(Phase::NeedsUser | Phase::Ended { .. } | Phase::Resting)
                )
        };
        self.wait_for(TURN_LIMIT, settled);
        let why = if self.has_reply_to(QUIET_MESSAGE) {
            None
        } else {
            Some(match self.phase() {
                Some(Phase::NeedsUser) => {
                    let effects = self.app.stop(key);
                    self.carry_out(effects);
                    self.wait_for(TURN_LIMIT, |live| {
                        !matches!(live.phase(), Some(Phase::NeedsUser | Phase::Working))
                    });
                    "it asked the user; the turn is stopped".to_string()
                }
                Some(Phase::Ended { clean, .. }) => format!("its program ended, clean: {clean}"),
                Some(Phase::Resting) => "its program ended cleanly before it answered".into(),
                _ => format!("no reply within {} s", TURN_LIMIT.as_secs()),
            })
        };
        match &why {
            None => eprintln!("live: started again, the session answered: yes"),
            Some(why) => eprintln!("live: started again, the session answered: no ({why})"),
        }
        eprintln!(
            "live: started again with the model, effort and mode it had: {}",
            yes(why.is_none() && shown(self) == before)
        );
    }

    /// Whether the test's session idles with a finished reply after its latest message that
    /// contains `message`.
    fn has_reply_to(&self, message: &str) -> bool {
        self.phase() == Some(Phase::Idle) && {
            let items = self.items();
            let asked = items.iter().rposition(
                |body| matches!(body, ItemBody::User { text, .. } if text.contains(message)),
            );
            asked.is_some_and(|at| !finished_replies(&items[at..]).is_empty())
        }
    }

    /// Prints how many counts of background tasks came from the test's programs in both runs,
    /// and how many tasks the last one counted. A count comes with each of the program's
    /// records, and once from the adapter's own guard for a tool call run in the background
    /// ([`Live::background_task`] says which came).
    fn report_background(&self) {
        eprintln!(
            "live: background task records that arrived: {}",
            self.background.len()
        );
        if let Some(last) = self.background.last() {
            eprintln!("live: the last one counted {last} task(s)");
        }
    }

    /// Closes the program of `key`'s session, the one sent attachments, and waits for it to
    /// rest. Prints a line when it does not. Fails on nothing.
    fn close_attached(&mut self, key: SessionKey) {
        if let Some(open) = self.attached_open() {
            self.backend.send(ToBackend::Send {
                open,
                command: SessionCommand::Close,
            });
        }
        let rested = self.wait_for(CLOSE_LIMIT, |live| {
            matches!(
                live.phase_of(key),
                Some(Phase::Resting | Phase::Ended { .. }) | None
            )
        });
        match self.phase_of(key) {
            Some(Phase::Ended { clean, .. }) => {
                eprintln!("live: the session for attachments ended (clean: {clean})");
            }
            _ if !rested => eprintln!(
                "live: the session for attachments had not ended {} s after Close",
                CLOSE_LIMIT.as_secs()
            ),
            _ => {}
        }
    }

    /// The number of the current open of the session sent attachments, while its program runs.
    fn attached_open(&self) -> Option<u64> {
        self.attached
            .and_then(|key| self.app.sessions.get(key))
            .and_then(|entry| entry.open)
    }

    /// The test's session in this run.
    fn session(&self) -> Option<&Session> {
        self.key
            .and_then(|key| self.app.sessions.get(key))
            .map(|entry| &entry.session)
    }

    /// The number of the test's session's current open, while its program runs.
    fn open(&self) -> Option<u64> {
        self.key
            .and_then(|key| self.app.sessions.get(key))
            .and_then(|entry| entry.open)
    }

    fn id(&self) -> Option<SessionId> {
        self.session().and_then(|s| s.id.clone())
    }

    /// Seconds since the start: what events are stamped with.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Takes every message waiting from the backend into the `App`, as the window's `poll`
    /// does, and carries out what comes back. Notes what the test reports on as it passes.
    fn take_in(&mut self) {
        // As the window's poll does: the clock first, so what is taken in is stamped with it.
        let effects = self.app.tick(unix_now());
        self.carry_out(effects);
        while let Some(message) = self.backend.try_recv() {
            // The App would load it over the session's live conversation.
            if let FromBackend::History {
                key,
                result,
                pictures,
            } = &message
                && self.stored_asked
                && Some(*key) == self.attached
            {
                self.stored_asked = false;
                self.stored = Some(result.as_ref().ok().map(|history| history.items.clone()));
                self.stored_pictures = pictures.first().cloned().unwrap_or_default();
                self.stored_read = 0;
                continue;
            }
            // The App would drop it: no picture is open large.
            if let FromBackend::Image { picture, result } = &message
                && Some(*picture) == self.image_asked
            {
                self.image_asked = None;
                self.image_read = Some(result.clone().map_err(|error| error.to_string()));
                continue;
            }
            // Only an image whose bytes the reader gave is answered with a picture.
            if let FromBackend::Picture { id, .. } = &message
                && self.stored_pictures.contains(id)
            {
                self.stored_read += 1;
            }
            self.note(&message);
            let now = self.now();
            let effects = self.app.receive(message, now);
            self.carry_out(effects);
            if self.start_new && self.app.screen == Screen::Main {
                // As "New session" does, in the test's folder, before any listing is merged.
                self.start_new = false;
                let effects = self.app.new_session(
                    self.folder.clone(),
                    Place::Focused,
                    Leaving::ForgetIfEmpty,
                );
                self.key = self.app.focused();
                self.carry_out(effects);
            }
        }
    }

    /// Notes what `message` says that the test reports on, before the `App` takes it.
    fn note(&mut self, message: &FromBackend) {
        if let FromBackend::Event { open, event } = message {
            match event {
                SessionEvent::BackgroundTasks { running } => self.background.push(*running),
                SessionEvent::Exited { clean, .. } => self.exits.push((*open, *clean)),
                _ => {}
            }
            if self.tasks.watching && Some(*open) == self.attached_open() {
                self.tasks.see(event);
            }
        }
        if let FromBackend::Event { open, event } = message
            && let Some(key) = self.key_of(*open)
            && let Some(said) = RemoteSaid::from_event(event)
        {
            self.remotes.push((key, Instant::now(), said));
        }
        match message {
            FromBackend::Event { open, event } if Some(*open) == self.open() => match event {
                SessionEvent::CommandFailed { what, reason } => {
                    self.refusals.push((*what, reason.clone()));
                }
                SessionEvent::TitleChanged(title) => self.titles.push(title.clone()),
                SessionEvent::PermissionModeChanged(mode) => self.modes.push(mode.clone()),
                SessionEvent::EffortChanged(level) => {
                    self.efforts.push((Instant::now(), level.clone()));
                }
                SessionEvent::UsageLimits(limits) if limits.session.is_some() => {
                    self.session_usage = true;
                }
                SessionEvent::TurnFinished(_) | SessionEvent::TurnFailed { .. } => {
                    self.turn_ended = true;
                }
                SessionEvent::ContextBreakdown(breakdown) => {
                    self.breakdowns.push((self.turn_ended, breakdown.clone()));
                }
                _ => {}
            },
            FromBackend::Sessions { since_unix, result } => {
                if self.asked_for_name && self.listed_name.is_none() {
                    let id = self.id();
                    self.listed_name = Some(result.as_ref().is_ok_and(|listed| {
                        listed
                            .iter()
                            .any(|stored| Some(&stored.id) == id.as_ref() && stored.title == NAME)
                    }));
                }
                let (asked, answered) = match since_unix {
                    Some(_) => (self.recent_asked, &mut self.recent),
                    None => (self.all_asked, &mut self.all),
                };
                if answered.is_none()
                    && let Some(asked) = asked
                {
                    *answered = Some(Answered {
                        took: asked.elapsed(),
                        count: result.as_ref().ok().map(Vec::len),
                    });
                }
            }
            FromBackend::Look { models, usage, .. } => {
                if let Some(asked) = self.look_asked {
                    self.looked = Some((asked.elapsed(), models.len(), usage.is_some()));
                }
            }
            FromBackend::History { key, result, .. } => {
                if let Some(asked) = self.reading.get(key) {
                    self.read.insert(
                        *key,
                        Answered {
                            took: asked.elapsed(),
                            count: result.as_ref().ok().map(|history| history.items.len()),
                        },
                    );
                }
            }
            FromBackend::Activity { key, result } if Some(*key) == self.key => {
                self.activity = Some(result.as_ref().ok().copied().flatten());
            }
            FromBackend::Event { open, event } if Some(*open) == self.attached_open() => {
                match event {
                    SessionEvent::TurnFinished(_) => self.attached_ends.push(true),
                    SessionEvent::TurnFailed { .. } => self.attached_ends.push(false),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// Takes messages in until `done` holds or `limit` has passed. True when `done` held.
    fn wait_for(&mut self, limit: Duration, done: impl Fn(&Live) -> bool) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            self.take_in();
            self.fail_on_a_dead_end();
            if done(self) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(TICK);
        }
    }

    /// The latest refusal of a command of this kind.
    fn refusal(&self, kind: CommandKind) -> Option<&str> {
        self.refusals
            .iter()
            .rev()
            .find(|(what, _)| *what == kind)
            .map(|(_, reason)| reason.as_str())
    }

    fn title(&self) -> Option<String> {
        self.session().and_then(|s| s.title.clone())
    }

    fn mode(&self) -> Option<PermissionMode> {
        self.session().map(|s| s.mode.clone())
    }

    /// Waits for the title `claude` gives the session in place of the stand-in made from the
    /// message, and prints it, or that none came (with the reason when it was refused). A title
    /// that `claude` sent counts even when it reads the same as the stand-in.
    fn report_title(&mut self, stand_in: Option<String>) {
        let named = self.wait_for(TITLE_LIMIT, |live| {
            !live.titles.is_empty() || live.title() != stand_in
        });
        if named {
            let title = self.titles.last().cloned().or_else(|| self.title());
            eprintln!("live: title {:?}", title.unwrap_or_default());
            return;
        }
        let seconds = TITLE_LIMIT.as_secs();
        match self.refusal(CommandKind::SuggestTitle) {
            Some(reason) => eprintln!("live: no title after {seconds} s (refused: {reason})"),
            None => eprintln!("live: no title after {seconds} s"),
        }
    }

    /// Chooses Auto when it is offered and prints whether `claude` accepted it, then goes back to
    /// "Ask before edits". Only failing to get back fails the test.
    ///
    /// Accepted: Ask is chosen through the app, which sends it because the mode in use is Auto.
    /// Refused: the mode never left Ask, so nothing is sent. No answer: `claude` may still accept
    /// Auto later, and the app would send nothing for Ask (it is the mode it knows), so the test
    /// sends `SetPermissionMode(Ask)` itself and passes only when `claude` confirms Ask.
    fn try_auto_mode(&mut self) {
        let offered = self.session().is_some_and(Session::auto_mode);
        if !offered {
            eprintln!("live: auto mode not offered");
            return;
        }
        let key = self.key.expect("the test's session");
        let refused_before = self.refusals.len();
        let effects = self.app.choose_mode(key, PermissionMode::Auto);
        self.carry_out(effects);
        let refused = |live: &Live| {
            live.refusals[refused_before..]
                .iter()
                .any(|(what, _)| *what == CommandKind::SetPermissionMode)
        };
        let answered = self.wait_for(MODE_LIMIT, |live| {
            live.mode() == Some(PermissionMode::Auto) || refused(live)
        });
        if self.mode() == Some(PermissionMode::Auto) {
            eprintln!("live: auto mode accepted");
        } else if answered {
            let reason = self
                .refusal(CommandKind::SetPermissionMode)
                .unwrap_or_default();
            eprintln!("live: auto mode refused: {reason}");
        } else {
            eprintln!(
                "live: auto mode: no answer after {} s",
                MODE_LIMIT.as_secs()
            );
        }
        if answered {
            let effects = self.app.choose_mode(key, PermissionMode::Ask);
            self.carry_out(effects);
            let back = self.wait_for(MODE_LIMIT, |live| live.mode() == Some(PermissionMode::Ask));
            assert!(
                back,
                "the session did not get back to \"Ask before edits\" within {} s: {:?}",
                MODE_LIMIT.as_secs(),
                self.mode()
            );
            return;
        }
        let (confirmed_before, refused_before) = (self.modes.len(), self.refusals.len());
        let open = self.open().expect("the session's program still runs");
        self.carry_out(vec![Effect::Send {
            open,
            command: SessionCommand::SetPermissionMode(PermissionMode::Ask),
        }]);
        let confirmed = |live: &Live| live.modes[confirmed_before..].contains(&PermissionMode::Ask);
        let refused = |live: &Live| {
            live.refusals[refused_before..]
                .iter()
                .any(|(what, _)| *what == CommandKind::SetPermissionMode)
        };
        self.wait_for(MODE_LIMIT, |live| confirmed(live) || refused(live));
        assert!(
            confirmed(self),
            "\"Ask before edits\" was not confirmed within {} s of asking for it again; \
             modes confirmed since: {:?}; refusals since: {:?}",
            MODE_LIMIT.as_secs(),
            &self.modes[confirmed_before..],
            &self.refusals[refused_before..]
        );
    }

    /// Sends what the `App` asks of the agent to the backend, noting when the test's reports
    /// start. Neither the width nor the state kept between runs is saved. A look is run in the
    /// test's folder only: the app asks for it in the folder of its newest session, which is
    /// the test's unless a session of the user's changed since the app started.
    fn carry_out(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            let message = match effect {
                Effect::SaveWidth(_) | Effect::SaveState(_) => continue,
                Effect::CheckAccount => ToBackend::CheckAccount,
                Effect::Open { open, spec } => {
                    if Some(open) == self.open() {
                        self.opened = Some(spec.clone());
                    }
                    ToBackend::Open { open, spec }
                }
                Effect::Send { open, command } => {
                    if matches!(command, SessionCommand::Close) {
                        self.closes.push(open);
                    }
                    ToBackend::Send { open, command }
                }
                Effect::ListSessions { since_unix } => {
                    let asked = match since_unix {
                        Some(_) => &mut self.recent_asked,
                        None => &mut self.all_asked,
                    };
                    asked.get_or_insert_with(Instant::now);
                    ToBackend::ListSessions { since_unix }
                }
                Effect::ReadHistory { key, id, max_items } => {
                    self.reading.insert(key, Instant::now());
                    ToBackend::ReadHistory { key, id, max_items }
                }
                Effect::DropHistory { key } => ToBackend::DropHistory { key },
                Effect::CheckActivity { key, id } => ToBackend::CheckActivity { key, id },
                Effect::ReadBranch { folder } => ToBackend::ReadBranch { folder },
                Effect::Prepare { chip, source } => ToBackend::Prepare { chip, source },
                Effect::ReadImage {
                    picture,
                    id,
                    message,
                    index,
                } => ToBackend::ReadImage {
                    picture,
                    id,
                    message,
                    index,
                },
                Effect::RemoveStored {
                    id,
                    unless_after_unix,
                } => ToBackend::RemoveStored {
                    id,
                    unless_after_unix,
                },
                // Task 9 passes these on to the desktop; the test asks for none.
                Effect::Notify { .. } | Effect::Unnotify { .. } | Effect::UnnotifyAll => continue,
                Effect::Look { folder } => {
                    if folder != self.folder {
                        eprintln!(
                            "live: the app asked for its look in the folder of a session newer \
                             than the test's; it runs in the test's folder instead"
                        );
                    }
                    self.look_asked.get_or_insert_with(Instant::now);
                    ToBackend::Look {
                        folder: self.folder.clone(),
                    }
                }
            };
            self.backend.send(message);
        }
        self.track();
    }

    fn phase(&self) -> Option<Phase> {
        self.key.and_then(|key| self.phase_of(key))
    }

    fn phase_of(&self, key: SessionKey) -> Option<Phase> {
        self.app
            .sessions
            .get(key)
            .map(|entry| entry.session.phase.clone())
    }

    fn items(&self) -> Vec<ItemBody> {
        self.key.map_or_else(Vec::new, |key| self.items_of(key))
    }

    fn items_of(&self, key: SessionKey) -> Vec<ItemBody> {
        self.app.sessions.get(key).map_or_else(Vec::new, |entry| {
            entry
                .session
                .conversation
                .items()
                .iter()
                .map(|i| i.body.clone())
                .collect()
        })
    }

    /// Fails at once on a state the run cannot get out of.
    fn fail_on_a_dead_end(&self) {
        match self.app.screen {
            Screen::NotInstalled => panic!("Claude Code is not installed (no `claude` found)"),
            Screen::SignedOut => panic!("Claude Code is not signed in: run `claude auth login`"),
            Screen::Checking | Screen::Main => {}
        }
        for key in self.ours() {
            if self.quiet && Some(key) == self.key {
                continue;
            }
            match self.phase_of(key) {
                Some(Phase::Ended { clean, detail }) => panic!(
                    "a session ended before the reply (clean: {clean}): {detail}; \
                     conversation:\n{:#?}",
                    self.items_of(key)
                ),
                Some(Phase::Resting) if self.running && !self.closed => panic!(
                    "a session's program ended cleanly before the reply; conversation:\n{:#?}",
                    self.items_of(key)
                ),
                Some(Phase::NeedsUser) => panic!(
                    "a turn asked the user for something; conversation:\n{:#?}",
                    self.items_of(key)
                ),
                _ => {}
            }
        }
    }

    /// Sends `Close` to each of the test's sessions, the one sent attachments included, once,
    /// unless its program has ended already.
    fn send_close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        for key in self.ours().into_iter().chain(self.attached) {
            if let Some(open) = self.app.sessions.get(key).and_then(|entry| entry.open) {
                self.backend.send(ToBackend::Send {
                    open,
                    command: SessionCommand::Close,
                });
            }
        }
    }

    /// Closes the app as the window does, and waits until each of the test's sessions has ended
    /// cleanly: with a message in it, it then rests. Past the idle stop (`quiet`) the test's own
    /// session counts once its program has ended, cleanly or not. Returns what the app would
    /// save.
    fn close_app(&mut self) -> Saved {
        self.closed = true;
        let effects = self.app.close(unix_now());
        let saved = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::SaveState(saved) => Some((**saved).clone()),
                _ => None,
            })
            .expect("closing saves the state first");
        self.carry_out(effects);
        let deadline = Instant::now() + CLOSE_LIMIT;
        loop {
            self.take_in();
            let reported = |live: &Live, key: SessionKey| live.quiet && Some(key) == live.key;
            let phases: Vec<(SessionKey, Option<Phase>)> = self
                .ours()
                .into_iter()
                .map(|key| (key, self.phase_of(key)))
                .collect();
            let done = phases.iter().all(|(key, phase)| match phase {
                Some(Phase::Resting) => true,
                Some(Phase::Ended { .. }) => reported(self, *key),
                _ => false,
            });
            if done {
                return saved;
            }
            for (key, phase) in &phases {
                if let Some(Phase::Ended { clean, detail }) = phase
                    && !reported(self, *key)
                {
                    panic!("a session did not rest after closing (clean: {clean}): {detail}")
                }
            }
            assert!(
                Instant::now() < deadline,
                "a session never ended within {} s of Close: phases {phases:?}",
                CLOSE_LIMIT.as_secs()
            );
            std::thread::sleep(TICK);
        }
    }
}

impl Drop for Live {
    /// Closes the test's sessions if the test left without closing them, and gives them a moment
    /// to end, so a failure never leaves a `claude` running. Dropping the `Backend` afterwards
    /// also passes `Close` to every session its threads still hold, the look's included.
    fn drop(&mut self) {
        self.track();
        if self.closed {
            return;
        }
        self.send_close();
        let deadline = Instant::now() + CLOSE_LIMIT;
        while Instant::now() < deadline {
            // Taken in only to see the session end; nothing more is sent.
            while let Some(message) = self.backend.try_recv() {
                let now = self.now();
                let _ = self.app.receive(message, now);
            }
            // So that a session seen to end here can be removed ([`Tidy`]).
            self.track();
            let ended = self.ours().into_iter().chain(self.attached).all(|key| {
                matches!(
                    self.phase_of(key),
                    Some(Phase::Resting | Phase::Ended { .. }) | None
                )
            });
            if ended {
                return;
            }
            std::thread::sleep(TICK);
        }
        eprintln!(
            "live: a session had not ended {} s after Close",
            CLOSE_LIMIT.as_secs()
        );
    }
}

/// The finished replies among `items`, in order.
fn finished_replies(items: &[ItemBody]) -> Vec<&String> {
    items
        .iter()
        .filter_map(|body| match body {
            ItemBody::Reply {
                markdown,
                streaming: false,
            } => Some(markdown),
            _ => None,
        })
        .collect()
}

/// "yes" or "no".
fn yes(said: bool) -> &'static str {
    if said { "yes" } else { "no" }
}

/// A PNG of [`RED_SIDE`] pixels a side, all red.
fn red_png() -> ImageData {
    let image = image::RgbImage::from_pixel(RED_SIDE, RED_SIDE, image::Rgb([255, 0, 0]));
    ImageData::new(png(&image))
}

/// A PNG of noise from a fixed seed, [`NOISE_SIZE`] pixels: a little under [`MAX_BYTES`] and
/// within [`MAX_SIDE`], so the app sends it byte for byte.
fn noise_png() -> ImageData {
    let (width, height) = NOISE_SIZE;
    debug_assert!(width <= MAX_SIDE && height <= MAX_SIDE);
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let bytes: Vec<u8> = (0..width * height * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect();
    let image = image::RgbImage::from_raw(width, height, bytes).expect("as many bytes as pixels");
    let png = png(&image);
    if png.len() > MAX_BYTES {
        eprintln!(
            "live: the noise image came out {} bytes, over the limit of {MAX_BYTES}: it will be made \
             smaller",
            png.len()
        );
    }
    ImageData::new(png)
}

/// `image` encoded as a PNG.
fn png(image: &image::RgbImage) -> Vec<u8> {
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("an image made here encodes");
    png
}

/// Whether `effects` send a Remote Control command.
fn sends_remote(effects: &[Effect]) -> bool {
    effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::Send {
                command: SessionCommand::SetRemoteAccess { .. },
                ..
            }
        )
    })
}

/// How a listing stands, in a word: how many it gave, failed, not answered or not asked.
fn listing_word(answer: &Option<Answered>, asked: Option<Instant>) -> String {
    match (answer, asked) {
        (Some(Answered { count: Some(n), .. }), _) => format!("{n} sessions"),
        (Some(Answered { count: None, .. }), _) => "failed".into(),
        (None, Some(_)) => "no answer".into(),
        (None, None) => "not asked".into(),
    }
}

/// Whether `command` is the one the test allows: with the blanks round it taken off, `sleep `
/// and a whole number of seconds, and nothing else. So nothing can ride along with it: no
/// second command, no option, no substitution, no second line.
fn allowed_command(command: &str) -> bool {
    command
        .trim()
        .strip_prefix(ALLOWED_COMMAND)
        .is_some_and(|seconds| !seconds.is_empty() && seconds.bytes().all(|b| b.is_ascii_digit()))
}

/// Runs in the ordinary suite: it starts nothing.
#[test]
fn only_a_sleep_of_some_seconds_is_allowed() {
    for allowed in ["sleep 20", "sleep 5", " sleep 20 ", "sleep 20\n"] {
        assert!(allowed_command(allowed), "{allowed:?}");
    }
    for refused in [
        "sleep 20 && touch x",
        "sleep 20; touch x",
        "sleep 20 | touch x",
        "sleep 20 & touch x",
        "sleep $(touch x)",
        "sleep `touch x`",
        "sleep 20\ntouch x",
        "sleep 20 30",
        "sleep -h",
        "sleep --help",
        "sleep 20s",
        "sleep 2.5",
        "sleep  20",
        "sleep\t20",
        "sleep ",
        "sleep",
        "sleeper 20",
        "/bin/sleep 20",
        "touch x",
        "",
    ] {
        assert!(!allowed_command(refused), "{refused:?}");
    }
}

/// The wall clock in Unix seconds.
fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
