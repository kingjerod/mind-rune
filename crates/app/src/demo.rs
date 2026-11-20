//! The scripted demo window: a `FakeAgent` signed in as alex, and the named
//! scenes that tests and screenshots open. Nothing here reaches the real agent.
//!
//! How a scene is built: it replaces the state with `fresh` and starts the
//! backend with `start`, so it never touches the real agent. The fake does not answer `RefreshUsage` by itself,
//! so a scene emits the `UsageLimits` event once the session has started. A
//! scene that shows a turn scripts it first with `FakeAgent::reply` (a `Send`
//! with nothing scripted fails the turn), sends the user's message with `say`,
//! which also scripts the title Claude gives the session, then pumps. Steps scripted for later than the
//! scene pumps stay undelivered, which is how a scene stops in the middle of a
//! turn. Events outside a turn are given with `FakeAgent::emit`.

use std::sync::Arc;

use mr_agent::{
    Account, Attachment, Choice, Choices, ContextBreakdown, ContextKind, ContextPart, FakeAgent,
    FakeStep, HistoryItem, ImageData, LimitWindow, ModelOption, PermissionMode, Question,
    QuestionOption, RemoteAccess, RemoteFailure, RequestId, SessionCommand, SessionEvent,
    SessionId, StoredHistory, StoredSession, TaskChange, TaskStatus, TaskStep, ToolCallId,
    ToolDetail, ToolOutcome, TurnSummary, UsageLimits, UserRequest, UserRequestKind,
};
use mr_core::{
    AttachKind, Branch, Chip, ChipState, Effect, FromBackend, GroupId, HistoryState, ItemBody,
    Launch, Leaving, Mark, Phase, Place, Prepared, Problem, Remote, Screen, SessionKey,
    SidebarView, Source, Spot, THUMB_SIDE, ThinkingTime, Thumb,
};
use rune::prefs::Prefs;
use rune::{PointerPress, TextEdit};

use crate::glue::{
    self, DragFrom, DropTarget, MenuTarget, Picker, Renaming, RowDrag, SideMenu, Ui,
};

/// The folder the demo's session is opened in.
const FOLDER: &str = "/home/alex/project";

/// The demo's fixed wall clock, in Unix seconds.
pub(crate) const NOW_UNIX: i64 = 1_800_000_000;

/// The most pumps a scene waits for a state before it gives up.
const MAX_PUMPS: usize = 50;

/// A window state over a `FakeAgent` signed in as alex on the Max plan, with settings kept in
/// memory, started in the demo's folder.
pub fn ui() -> Ui {
    ui_in(Some(FOLDER))
}

/// [`ui`], started in `folder`, or as `mindrune` with no folder starts.
fn ui_in(folder: Option<&str>) -> Ui {
    ui_with(folder, Prefs::memory())
}

/// [`ui_in`], with the settings `prefs` holds: what a last run kept, say. Only tests call it
/// with their own.
pub(crate) fn ui_with(folder: Option<&str>, prefs: Prefs) -> Ui {
    let fake = FakeAgent::new();
    fake.set_account(Account::SignedIn {
        name: "alex".into(),
        plan: Some("Max".into()),
        full_login: true,
    });
    // No look: with the fake, a look starts only when the fake is advanced, and the demo's
    // flush would wait for it.
    let launch = Launch {
        folder: folder.map(Into::into),
        look: false,
        home: "/home/alex".into(),
        ..Launch::default()
    };
    let mut ui = Ui::new(Arc::new(fake.clone()), Some(fake), launch, prefs);
    ui.unix_clock = || NOW_UNIX;
    ui.app.now_unix = NOW_UNIX;
    ui.folder_exists = |_| true;
    ui.reads_branches = false;
    ui
}

/// Advances the fake by `seconds` and takes in everything it produced. Demo and tests only.
///
/// The backend is flushed before the fake advances and after every batch taken in, so
/// whatever the worker queues in answer is taken in by this same pump: the demo is
/// deterministic although the backend has a thread. Returns true when at least one
/// message was taken in.
pub fn pump(ui: &mut Ui, seconds: f32) -> bool {
    let Some(backend) = &ui.backend else {
        return false;
    };
    backend.flush();
    if let Some(fake) = &ui.fake {
        fake.advance(seconds);
    }
    ui.fake_time += f64::from(seconds);
    let stamp = ui.fake_time;
    let mut took = false;
    while glue::take_in(ui, stamp) {
        took = true;
        if let Some(backend) = &ui.backend {
            backend.flush();
        }
    }
    took
}

/// A scene: replaces the window's state with a named one.
pub type Scene = fn(&mut Ui);

pub const SCENES: &[(&str, Scene)] = &[
    ("checking", checking),
    ("signed-out", signed_out),
    ("not-installed", not_installed),
    ("empty", empty),
    ("effort", effort),
    ("limit", limit),
    ("conversation", conversation),
    ("streaming", streaming),
    ("long", long),
    ("tools", tools),
    ("working", working),
    ("ended", ended),
    ("permission", permission),
    ("permission-plain", permission_plain),
    ("permission-long", permission_long),
    ("permission-edit", permission_edit),
    ("question", question),
    ("plan", plan),
    ("sessions", sessions),
    ("group-menu", group_menu),
    ("folders", folders),
    ("empty-start", empty_start),
    ("resting", resting),
    ("history-loading", history_loading),
    ("history-failed", history_failed),
    ("elsewhere", elsewhere),
    ("tiles", tiles),
    ("tiles-needs-you", tiles_needs_you),
    ("tiles-drag", tiles_drag),
    ("tiles-stack-drag", tiles_stack_drag),
    ("tiles-move", tiles_move),
    ("tiles-stacked", tiles_stacked),
    ("tile-rename", tile_rename),
    ("panel", panel),
    ("panel-remote", panel_remote),
    ("panel-refused", panel_refused),
    ("chips", chips),
    ("chips-problem", chips_problem),
    ("quit-ask", quit_ask),
    ("viewer", viewer),
    ("viewer-failed", viewer_failed),
    ("colours", colours),
    ("deleted", deleted),
    ("deleted-empty", deleted_empty),
    ("delete-group-ask", delete_group_ask),
    ("delete-all-ask", delete_all_ask),
    ("tasks", tasks),
    ("tasks-folded", tasks_folded),
    ("choices", choices),
];

/// Replaces the state with a fresh demo state.
fn fresh(ui: &mut Ui) {
    *ui = self::ui();
}

/// Starts the backend with a waker that does nothing: in the demo, `pump` takes
/// everything in, so nothing needs waking.
fn start(ui: &mut Ui) {
    glue::setup(ui, Arc::new(|| {}));
}

/// Pumps 0 seconds at a time until `done` holds. Panics when it never does.
fn pump_until(ui: &mut Ui, what: &str, done: impl Fn(&Ui) -> bool) {
    for _ in 0..MAX_PUMPS {
        if done(ui) {
            return;
        }
        pump(ui, 0.0);
    }
    assert!(done(ui), "the demo never reached {what}");
}

fn fake(ui: &Ui) -> &FakeAgent {
    ui.fake.as_ref().expect("a demo state has its fake")
}

/// The user sends `message` as the session's first, and Claude names the session `title` when
/// the core asks, at once. A turn must be scripted first, as for any `Send`.
fn say(ui: &mut Ui, message: &str, title: &str) {
    fake(ui).title_reply(vec![FakeStep::at(
        0.0,
        SessionEvent::TitleChanged(title.into()),
    )]);
    let key = ui.app.focused().expect("a session is shown");
    let effects = ui.app.submit(key, message);
    glue::run(ui, effects);
}

/// Emits `event` to the focused session.
fn emit(ui: &Ui, event: SessionEvent) {
    let key = ui.app.focused().expect("a session is shown");
    emit_to(ui, key, event);
}

/// Emits `event` to `key`'s session, shown or not. A scene may hold several sessions.
pub(crate) fn emit_to(ui: &Ui, key: SessionKey, event: SessionEvent) {
    let id = ui
        .app
        .sessions
        .get(key)
        .and_then(|entry| entry.session.id.clone())
        .expect("the session has started");
    fake(ui).emit(&id, event);
}

/// The account check has not come back: it is never sent, so poll cannot answer it.
fn checking(ui: &mut Ui) {
    fresh(ui);
    ui.forget_waiting();
    start(ui);
}

fn signed_out(ui: &mut Ui) {
    account_scene(ui, Account::SignedOut, Screen::SignedOut);
}

fn not_installed(ui: &mut Ui) {
    account_scene(ui, Account::Missing, Screen::NotInstalled);
}

fn account_scene(ui: &mut Ui, account: Account, screen: Screen) {
    fresh(ui);
    // Before the backend starts, which asks for the account at once.
    fake(ui).set_account(account);
    start(ui);
    pump_until(ui, "its account screen", |ui| ui.app.screen == screen);
}

/// A started session with nothing said yet; the usage request has been answered.
fn empty(ui: &mut Ui) {
    fresh(ui);
    start(ui);
    pump_until(ui, "an idle session", |ui| {
        ui.focused_session().is_some_and(|s| s.phase == Phase::Idle)
    });
    usage(ui, 0.37, 0.10);
}

/// Like `empty`, with the effort picker's list open on Low, Medium and High, Medium in use.
fn effort(ui: &mut Ui) {
    empty(ui);
    pump_until(ui, "the effort in use", |ui| {
        ui.focused_session()
            .is_some_and(|s| s.effort.as_deref() == Some("medium"))
    });
    let key = ui.app.focused().expect("a session is shown");
    glue::view_mut(ui, key).open_picker = Some(Picker::Effort);
}

/// Like `empty`, with the five-hour window used up and the weekly one at 82%.
fn limit(ui: &mut Ui) {
    empty(ui);
    usage(ui, 1.0, 0.82);
}

/// The user's message in the `conversation` and `streaming` scenes. Its asterisks must show.
const USER_MESSAGE: &str = "**hi**, what is in this folder?";

/// The title Claude gives the session in the `conversation` and `streaming` scenes.
const CONVERSATION_TITLE: &str = "Listing the folder";

/// Claude's reply in the `conversation` scene: a heading, a list, a table and a Rust code block.
const REPLY: &str = r#"## What is in this folder

A small Rust workspace with two crates and one binary.

- `crates/core` holds what the window knows, as plain data.
- `crates/app` draws the window and carries out what the core asks for.
- `Cargo.toml` ties the two together.

| Path | What it holds | Lines |
| :--- | :--- | ---: |
| `crates/core` | The conversation and the session | 2,140 |
| `crates/app` | The window and its views | 1,310 |

The binary starts like this:

```rust
fn main() {
    let folder = std::env::current_dir().expect("a working folder");
    let window = Window::open(&folder);
    println!("Opened {} in {} ms", folder.display(), window.ready_ms());
}
```

Ask me to open any of these files."#;

/// Where the `streaming` scene cuts the reply off: inside the code block.
const CUT: &str = "    let window";

/// How many replies the `long` scene holds.
const LONG_REPLIES: usize = 2_000;

/// A finished turn, as `TurnFinished` reports it.
fn finished() -> SessionEvent {
    SessionEvent::TurnFinished(TurnSummary {
        cost_usd: 0.02,
        context_used: None,
        duration_ms: 1_800,
        interrupted: false,
    })
}

/// The user asks what is in the folder and Claude's whole reply arrives.
fn conversation(ui: &mut Ui) {
    empty(ui);
    fake(ui).reply(vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(0.0, SessionEvent::TextDelta(REPLY.into())),
        FakeStep::at(0.0, SessionEvent::BlockFinished),
        FakeStep::at(0.0, finished()),
    ]);
    say(ui, USER_MESSAGE, CONVERSATION_TITLE);
    pump_until(ui, "the finished reply", |ui| {
        ui.focused_session()
            .is_some_and(|s| s.phase == Phase::Idle && s.conversation.items().len() == 2)
    });
}

/// The same turn, with the reply stopped inside its code block and still streaming. The rest
/// is scripted a minute later, so a scene that pumps no time never reaches it.
fn streaming(ui: &mut Ui) {
    empty(ui);
    let cut = REPLY
        .find(CUT)
        .expect("the reply has the cut in its code block");
    fake(ui).reply(vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(0.0, SessionEvent::TextDelta(REPLY[..cut].into())),
        FakeStep::at(60.0, SessionEvent::TextDelta(REPLY[cut..].into())),
        FakeStep::at(60.0, SessionEvent::BlockFinished),
        FakeStep::at(60.0, finished()),
    ]);
    say(ui, USER_MESSAGE, CONVERSATION_TITLE);
    pump_until(ui, "the reply cut off in its code block", |ui| {
        ui.focused_session().is_some_and(|s| {
            matches!(
                s.conversation.items().last().map(|item| &item.body),
                Some(ItemBody::Reply {
                    streaming: true,
                    ..
                })
            )
        })
    });
}

/// Two thousand finished replies, `Reply 1` to `Reply 2000`. Every event is emitted first and
/// then taken in by one pump, so the scene stays quick to build.
fn long(ui: &mut Ui) {
    empty(ui);
    emit(
        ui,
        SessionEvent::TitleChanged("Two thousand replies".into()),
    );
    for n in 1..=LONG_REPLIES {
        emit(ui, SessionEvent::TextDelta(format!("Reply {n}")));
        emit(ui, SessionEvent::BlockFinished);
    }
    pump(ui, 0.0);
    let items = ui
        .focused_session()
        .map_or(0, |s| s.conversation.items().len());
    assert_eq!(
        items, LONG_REPLIES,
        "the long scene's replies were all taken in"
    );
}

/// The user's message in the `tools` scene.
const TOOLS_MESSAGE: &str = "Run the tests and fix what fails";

/// How long the `tools` scene's thinking lasts, in seconds.
const TOOLS_THINKING: f32 = 4.0;

/// How long after its `Send` the rest of a scene's turn is scripted, so that a scene, a test
/// or a snapshot that pumps less time never reaches it.
const LATER: f32 = 60.0;

/// The `tools` scene's command output: 500 numbered lines.
fn test_output() -> String {
    (1..=500).map(|n| format!("line {n}\n")).collect()
}

fn tool(id: &str, name: &str, detail: ToolDetail) -> SessionEvent {
    SessionEvent::ToolStarted {
        id: ToolCallId(id.into()),
        name: name.into(),
        detail,
    }
}

fn done(id: &str, output: &str) -> SessionEvent {
    SessionEvent::ToolFinished {
        id: ToolCallId(id.into()),
        outcome: ToolOutcome::Ok {
            output: output.into(),
        },
    }
}

fn read(id: &str, path: &str) -> SessionEvent {
    tool(id, "Read", ToolDetail::FileRead { path: path.into() })
}

/// A finished turn of every kind of tool line: a 4 s thought with no text, three reads that
/// fold into one line, a search, a command with 500 lines of output, an edit, a read that
/// failed, and the reply.
fn tools(ui: &mut Ui) {
    empty(ui);
    tools_turn(ui);
}

/// The `tools` scene's turn, in the session as it is: sent, played and finished.
pub(crate) fn tools_turn(ui: &mut Ui) {
    let t = TOOLS_THINKING;
    fake(ui).reply(vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(0.0, SessionEvent::ThinkingStarted),
        FakeStep::at(t, SessionEvent::BlockFinished),
        FakeStep::at(t, read("r1", "/home/alex/project/Cargo.toml")),
        FakeStep::at(t, read("r2", "/home/alex/project/src/main.rs")),
        FakeStep::at(t, read("r3", "/home/alex/project/src/lib.rs")),
        FakeStep::at(t, done("r1", "[package]\nname = \"project\"\n")),
        FakeStep::at(t, done("r2", "fn main() {}\n")),
        FakeStep::at(t, done("r3", "pub fn run() {}\n")),
        FakeStep::at(
            t,
            tool(
                "s1",
                "Grep",
                ToolDetail::Search {
                    query: "TODO".into(),
                },
            ),
        ),
        FakeStep::at(
            t,
            done(
                "s1",
                "src/main.rs:3:    // TODO: report the error\nsrc/lib.rs:9:    // TODO: keep going\n",
            ),
        ),
        FakeStep::at(
            t,
            tool(
                "c1",
                "Bash",
                ToolDetail::Command {
                    command: "cargo test".into(),
                },
            ),
        ),
        FakeStep::at(t, done("c1", &test_output())),
        FakeStep::at(
            t,
            tool(
                "e1",
                "Edit",
                ToolDetail::FileEdit {
                    path: "/home/alex/project/src/main.rs".into(),
                    old_text: "    let n = parse(&input);\n    run(n);\n".into(),
                    new_text: "    let n = parse(&input)?;\n    run(n)?;\n    Ok(())\n".into(),
                },
            ),
        ),
        FakeStep::at(t, done("e1", "The file was updated.")),
        FakeStep::at(t, read("r4", "/home/alex/project/missing.rs")),
        FakeStep::at(
            t,
            SessionEvent::ToolFinished {
                id: ToolCallId("r4".into()),
                outcome: ToolOutcome::Failed {
                    output: "No such file".into(),
                },
            },
        ),
        FakeStep::at(
            t,
            SessionEvent::TextDelta(
                "The tests pass now: `main` returns the parse error instead of panicking.".into(),
            ),
        ),
        FakeStep::at(t, SessionEvent::BlockFinished),
        FakeStep::at(t, finished()),
    ]);
    say(ui, TOOLS_MESSAGE, "Fixing the failing tests");
    pump_until(ui, "the thinking", |ui| {
        ui.focused_session().is_some_and(|s| {
            matches!(
                s.conversation.items().last().map(|item| &item.body),
                Some(ItemBody::Thinking {
                    time: ThinkingTime::Going,
                    ..
                })
            )
        })
    });
    pump(ui, t);
    pump_until(ui, "the finished turn", |ui| {
        ui.focused_session().is_some_and(|s| s.phase == Phase::Idle)
    });
}

/// A turn in flight: a command still running, then a thought still going on. The rest of the
/// turn is scripted a minute later.
fn working(ui: &mut Ui) {
    empty(ui);
    fake(ui).reply(vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(
            0.0,
            tool(
                "c1",
                "Bash",
                ToolDetail::Command {
                    command: "sleep 30".into(),
                },
            ),
        ),
        FakeStep::at(0.0, SessionEvent::ThinkingStarted),
        FakeStep::at(LATER, SessionEvent::BlockFinished),
        FakeStep::at(LATER, done("c1", "")),
        FakeStep::at(LATER, finished()),
    ]);
    say(
        ui,
        "Wait half a minute, then tell me",
        "Waiting half a minute",
    );
    pump_until(ui, "the running command and thought", |ui| {
        ui.focused_session().is_some_and(|s| {
            s.phase == Phase::Working
                && matches!(
                    s.conversation.items().last().map(|item| &item.body),
                    Some(ItemBody::Thinking {
                        time: ThinkingTime::Going,
                        ..
                    })
                )
        })
    });
}

/// The user's message in the `tasks` scenes, and the title Claude gives the session.
const TASKS_MESSAGE: &str = "Make the banner fit a phone screen";
const TASKS_TITLE: &str = "Resizing the banner";

/// The steps of the `tasks` scenes' list: id, subject and active wording.
const TASKS: [(&str, &str, &str); 5] = [
    (
        "1",
        "Measure the current banner",
        "Measuring the current banner",
    ),
    ("2", "Resize the image", "Resizing the image"),
    ("3", "Update the stylesheet", "Updating the stylesheet"),
    (
        "4",
        "Check the page on a phone",
        "Checking the page on a phone",
    ),
    ("5", "Write the change note", "Writing the change note"),
];

/// A step added to the task list, not started yet.
fn task_added((id, subject, active): (&str, &str, &str)) -> SessionEvent {
    SessionEvent::TaskAdded(TaskStep {
        id: id.into(),
        subject: subject.into(),
        active: Some(active.into()),
        status: TaskStatus::Pending,
    })
}

/// Step `id` of the task list now stands at `status`.
fn task_now(id: &str, status: TaskStatus) -> SessionEvent {
    SessionEvent::TaskChanged(TaskChange {
        id: id.into(),
        status: Some(status),
        subject: None,
        active: None,
    })
}

/// A turn in flight that works through a list of five steps: two done, the third, "Update the
/// stylesheet", under way, and a thought still going on. The rest of the turn is scripted a
/// minute later, as in `working`.
fn tasks(ui: &mut Ui) {
    empty(ui);
    let command = |id: &str, command: &str| {
        let detail = ToolDetail::Command {
            command: command.into(),
        };
        tool(id, "Bash", detail)
    };
    let mut steps = vec![
        SessionEvent::TurnStarted,
        SessionEvent::TextDelta("I will work through this in five steps.".into()),
        SessionEvent::BlockFinished,
    ];
    steps.extend(TASKS.map(task_added));
    steps.extend([
        task_now("1", TaskStatus::InProgress),
        command("c1", "identify images/banner.png"),
        done("c1", "images/banner.png PNG 1600x400"),
        task_now("1", TaskStatus::Completed),
        task_now("2", TaskStatus::InProgress),
        command(
            "c2",
            "convert images/banner.png -resize 800x images/banner-800.png",
        ),
        done("c2", ""),
        task_now("2", TaskStatus::Completed),
        task_now("3", TaskStatus::InProgress),
        read("r1", "/home/alex/project/styles/banner.css"),
        done("r1", ".banner {\n    width: 1600px;\n}\n"),
        SessionEvent::ThinkingStarted,
    ]);
    let mut script: Vec<FakeStep> = steps
        .into_iter()
        .map(|event| FakeStep::at(0.0, event))
        .collect();
    script.extend([
        FakeStep::at(LATER, SessionEvent::BlockFinished),
        FakeStep::at(LATER, finished()),
    ]);
    fake(ui).reply(script);
    say(ui, TASKS_MESSAGE, TASKS_TITLE);
    pump_until(ui, "the third step under way and a thought", |ui| {
        ui.focused_session().is_some_and(|s| {
            s.phase == Phase::Working
                && s.tasks.current() == Some(2)
                && s.tasks.steps()[2].status == TaskStatus::InProgress
                && matches!(
                    s.conversation.items().last().map(|item| &item.body),
                    Some(ItemBody::Thinking {
                        time: ThinkingTime::Going,
                        ..
                    })
                )
        })
    });
}

/// The `tasks` scene in the upper tile of a column of two, for a window of 1,200 by 800, where
/// the tile is short enough that its list starts folded; the lower tile holds a session with
/// nothing said yet. The upper one is focused.
fn tasks_folded(ui: &mut Ui) {
    tasks(ui);
    let upper = ui.app.focused().expect("a session is shown");
    let effects = ui
        .app
        .new_session(SKETCHES.into(), Place::Below(upper), Leaving::ForgetIfEmpty);
    glue::run(ui, effects);
    pump_until(ui, "the lower session started", |ui| {
        ui.app.focused() != Some(upper)
            && ui.focused_session().is_some_and(|s| s.phase == Phase::Idle)
    });
    focus_tile(ui, upper);
}

/// Claude's reply in the `choices` scene, and the question its turn ends with.
const CHOICES_REPLY: &str = "I resized the image to 800 pixels wide and let the stylesheet scale it down. The banner is resized and the page checked on a phone.";
const CHOICES_QUESTION: &str = "Publish the new banner?";

/// The answers of the `choices` scene: each label, and the message it sends.
const CHOICES: [(&str, &str); 3] = [
    ("Publish", "/publish"),
    (
        "Show me first",
        "Show me the page before anything is published.",
    ),
    ("Not now", "Leave it unpublished for now."),
];

/// The session "Resizing the banner" after a finished turn whose reply is followed by an offer
/// of three answers, the first one Claude's recommendation. The offer comes inside the turn,
/// as the agent gives it, and shows once the turn has ended.
fn choices(ui: &mut Ui) {
    empty(ui);
    let answers = CHOICES
        .map(|(label, message)| Choice {
            label: label.into(),
            message: message.into(),
        })
        .to_vec();
    let offer = Choices::new(CHOICES_QUESTION, answers).expect("an offer that can be shown");
    fake(ui).reply(vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(0.0, SessionEvent::TextDelta(CHOICES_REPLY.into())),
        FakeStep::at(0.0, SessionEvent::BlockFinished),
        FakeStep::at(0.0, SessionEvent::ChoicesOffered(offer)),
        FakeStep::at(0.0, finished()),
    ]);
    say(ui, TASKS_MESSAGE, TASKS_TITLE);
    pump_until(ui, "the finished turn and its offer", |ui| {
        let key = ui.app.focused().expect("a session is shown");
        ui.app.choices(key).is_some()
    });
}

/// A reply, then the program dies.
fn ended(ui: &mut Ui) {
    empty(ui);
    fake(ui).reply(vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(0.0, SessionEvent::TextDelta("Done.".into())),
        FakeStep::at(0.0, SessionEvent::BlockFinished),
        FakeStep::at(0.0, finished()),
    ]);
    say(ui, "Tidy up the README", "Tidying up the README");
    pump_until(ui, "the reply", |ui| {
        ui.focused_session()
            .is_some_and(|s| s.phase == Phase::Idle && s.conversation.items().len() == 2)
    });
    emit(
        ui,
        SessionEvent::Exited {
            clean: false,
            detail: "claude exited with status 1".into(),
        },
    );
    pump_until(ui, "the ended session", |ui| {
        ui.focused_session()
            .is_some_and(|s| matches!(s.phase, Phase::Ended { .. }))
    });
}

/// The user sends `message` and Claude's turn stops at `request`, waiting for the user. Nothing
/// more is scripted, so the session stays in `NeedsUser` until the request is answered.
///
/// The turn is what the adapter sends: a permission's tool call starts first (`claude` reports
/// the call, then asks whether it may run), while a question or a plan comes as the request
/// alone (the adapter does not report those tools as calls).
fn ask(ui: &mut Ui, message: &str, title: &str, request: UserRequest) {
    empty(ui);
    let mut steps = vec![FakeStep::at(0.0, SessionEvent::TurnStarted)];
    if let Some(call) = call_asked_for(&request) {
        steps.push(FakeStep::at(0.0, call));
    }
    steps.push(FakeStep::at(0.0, SessionEvent::NeedsUser(request)));
    fake(ui).reply(steps);
    say(ui, message, title);
    pump_until(ui, "the request waiting", |ui| {
        ui.focused_session()
            .is_some_and(|s| s.phase == Phase::NeedsUser && s.conversation.waiting() == 1)
    });
}

/// The tool call a permission request asks to make, as `claude` reports it before asking:
/// its id is the request's own, prefixed `call-`.
fn call_asked_for(request: &UserRequest) -> Option<SessionEvent> {
    match &request.kind {
        UserRequestKind::Permission { tool, detail, .. } => Some(SessionEvent::ToolStarted {
            id: ToolCallId(format!("call-{}", request.id.0)),
            name: tool.clone(),
            detail: detail.clone(),
        }),
        UserRequestKind::Question { .. } | UserRequestKind::PlanApproval { .. } => None,
    }
}

/// Claude asks to run `touch a.txt`, with `label` as the rule "Always allow" would add.
fn touch(id: &str, label: Option<&str>) -> UserRequest {
    UserRequest {
        id: RequestId(id.into()),
        kind: UserRequestKind::Permission {
            tool: "Bash".into(),
            detail: ToolDetail::Command {
                command: "touch a.txt".into(),
            },
            always_allow_label: label.map(str::to_owned),
        },
    }
}

/// The user's message in the `permission` scenes, and the title Claude gives the session.
const PERMISSION_MESSAGE: &str = "Create an empty a.txt";
const PERMISSION_TITLE: &str = "Creating an empty a.txt";

/// Claude asks to run a command, offering a rule to always allow it.
fn permission(ui: &mut Ui) {
    let label = "Always allow Bash(touch a.txt) in this project";
    ask(
        ui,
        PERMISSION_MESSAGE,
        PERMISSION_TITLE,
        touch("r1", Some(label)),
    );
}

/// The same request with no rule to offer.
fn permission_plain(ui: &mut Ui) {
    ask(ui, PERMISSION_MESSAGE, PERMISSION_TITLE, touch("r2", None));
}

/// Claude asks to make a small edit to a file of the project, offering to allow every edit for
/// the session.
fn permission_edit(ui: &mut Ui) {
    let request = UserRequest {
        id: RequestId("e1".into()),
        kind: UserRequestKind::Permission {
            tool: "Edit".into(),
            detail: ToolDetail::FileEdit {
                path: "/home/alex/project/src/main.rs".into(),
                old_text: "    let n = parse(&input);\n    run(n);\n".into(),
                new_text: "    let n = parse(&input)?;\n    run(n)?;\n".into(),
            },
            always_allow_label: Some("Allow all edits for this session".into()),
        },
    };
    ask(
        ui,
        "Make main return the parse error",
        "Returning the parse error",
        request,
    );
}

fn option(label: &str, description: Option<&str>) -> QuestionOption {
    QuestionOption {
        label: label.into(),
        description: description.map(str::to_owned),
    }
}

/// The rule "Always allow" offers in the `permission-long` scene: several rules joined, with a
/// path, longer than the card is wide at the window's smallest.
const LONG_RULE: &str = "Always allow Bash(cargo build:*), Bash(cargo test:*), Read(/home/alex/projects/very-long-folder-name/src/**) for everyone in this project";

/// The same request, offering a rule too long for one line.
fn permission_long(ui: &mut Ui) {
    ask(
        ui,
        PERMISSION_MESSAGE,
        PERMISSION_TITLE,
        touch("r3", Some(LONG_RULE)),
    );
}

/// Claude asks two questions at once: one colour, and any number of sizes.
fn question(ui: &mut Ui) {
    let request = UserRequest {
        id: RequestId("q1".into()),
        kind: UserRequestKind::Question {
            questions: vec![
                Question {
                    header: Some("Colour".into()),
                    prompt: "Which colour?".into(),
                    options: vec![option("Red", Some("Warm")), option("Blue", None)],
                    multi_select: false,
                },
                Question {
                    header: None,
                    prompt: "Which sizes?".into(),
                    options: vec![option("S", None), option("M", None), option("L", None)],
                    multi_select: true,
                },
            ],
        },
    };
    ask(
        ui,
        "Add a shirt to the shop",
        "Adding a shirt to the shop",
        request,
    );
}

/// The plan in the `plan` scene: a heading and three steps.
const PLAN: &str = "## Split the parser

1. Move the lexer into `src/lexer.rs`.
2. Give the parser its own error type.
3. Run `cargo test` and fix what fails.";

/// Claude has a plan and asks for it to be approved.
fn plan(ui: &mut Ui) {
    let request = UserRequest {
        id: RequestId("p1".into()),
        kind: UserRequestKind::PlanApproval {
            plan_markdown: PLAN.into(),
        },
    };
    ask(
        ui,
        "Plan how to split the parser",
        "Planning the parser split",
        request,
    );
}

/// Reports the two usage windows as `session` and `weekly` used, resetting in 2 h 14 min and
/// in 3 d 4 h, and takes the report in.
fn usage(ui: &mut Ui, session: f32, weekly: f32) {
    let limits = limits(ui.app.now_unix, session, weekly);
    emit(ui, SessionEvent::UsageLimits(limits));
    pump(ui, 0.0);
}

/// The two usage windows as `session` and `weekly` used, resetting in 2 h 14 min and in
/// 3 d 4 h from `now`.
fn limits(now: i64, session: f32, weekly: f32) -> UsageLimits {
    UsageLimits {
        session: Some(LimitWindow {
            used: session,
            resets_at_unix: now + 2 * 3600 + 14 * 60,
        }),
        weekly: Some(LimitWindow {
            used: weekly,
            resets_at_unix: now + 3 * 86_400 + 4 * 3600,
        }),
    }
}

/// One day, in seconds.
pub(crate) const DAY: i64 = 86_400;

/// The folder of the `sessions` scene's main group.
pub(crate) const RUNE_CLAUDE: &str = "/home/alex/rune_claude";

/// The folder of the `sessions` scene's collapsed group.
pub(crate) const SKETCHES: &str = "/home/alex/sketches";

/// The stored session the `sessions` scene shows, placed in "Side projects".
const GARDEN_ID: &str = "stored-garden";

/// The stored sessions of the `sessions` scene: the id, the folder, the title and how many days
/// before the demo's clock each was last active. Anything older than 30 days is listed only on
/// request.
const STORED: &[(&str, &str, &str, i64)] = &[
    (
        GARDEN_ID,
        "/home/alex/garden",
        "Planning the spring beds",
        2,
    ),
    (
        "stored-site-1",
        "/home/alex/website",
        "Fixing the contact form",
        5,
    ),
    (
        "stored-site-2",
        "/home/alex/website",
        "Moving to a static host",
        40,
    ),
    (
        "stored-site-3",
        "/home/alex/website",
        "Adding a dark theme",
        45,
    ),
    (
        "stored-site-4",
        "/home/alex/website",
        "Compressing the photos",
        50,
    ),
    (
        "stored-site-5",
        "/home/alex/website",
        "Writing the about page",
        60,
    ),
    (
        "stored-recipes",
        "/home/alex/recipes",
        "Sorting the soup recipes",
        90,
    ),
    (
        "stored-thesis",
        "/home/alex/thesis",
        "Formatting the bibliography",
        120,
    ),
];

/// Many sessions in their groups: "rune_claude" with one session working, one that needs the
/// user and one finished out of sight; "Side projects", a custom group holding a stored
/// session from another folder, which is shown; "sketches", collapsed, with a session working;
/// "website", with four sessions too old to list until asked; and two older groups.
fn sessions(ui: &mut Ui) {
    sessions_with(ui, &[]);
}

/// The `sessions` scene with `extra` stored too, given as [`STORED`] gives its own.
fn sessions_with(ui: &mut Ui, extra: &[(&str, &str, &str, i64)]) {
    fresh(ui);
    let stored: Vec<StoredSession> = STORED
        .iter()
        .chain(extra)
        .map(|&(id, folder, title, days)| StoredSession {
            id: SessionId(id.into()),
            folder: folder.into(),
            title: title.into(),
            last_active_unix: NOW_UNIX - days * DAY,
        })
        .collect();
    fake(ui).set_sessions(stored);
    fake(ui).set_history(
        &SessionId(GARDEN_ID.into()),
        StoredHistory {
            items: vec![
                HistoryItem::User {
                    text: "Which beds get the most sun in spring?".into(),
                    image_count: 0,
                    files: Vec::new(),
                    images: Vec::new(),
                    stored: None,
                },
                HistoryItem::AssistantText(
                    "The two beds by the south fence get sun from morning to late afternoon. \
                     Put the tomatoes and the beans there, and the lettuce in the shade of the shed."
                        .into(),
                ),
            ],
            ..StoredHistory::default()
        },
    );
    start(ui);
    pump_until(ui, "an idle session and the listing", |ui| {
        ui.focused_session().is_some_and(|s| s.phase == Phase::Idle) && ui.app.listed_all
    });
    usage(ui, 0.37, 0.10);

    // Its turn ends a second from now, out of sight.
    let finished_later = vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(
            1.0,
            SessionEvent::TextDelta("The keys are tidy now.".into()),
        ),
        FakeStep::at(1.0, SessionEvent::BlockFinished),
        FakeStep::at(1.0, finished()),
    ];
    live(
        ui,
        RUNE_CLAUDE,
        "Tidy the sidebar's keys",
        "Tidying the sidebar keys",
        finished_later,
    );
    let tests = UserRequest {
        id: RequestId("r1".into()),
        kind: UserRequestKind::Permission {
            tool: "Bash".into(),
            detail: ToolDetail::Command {
                command: "cargo test -p mindrune".into(),
            },
            always_allow_label: Some("Always allow Bash(cargo test:*) in this project".into()),
        },
    };
    let mut asking = vec![FakeStep::at(0.0, SessionEvent::TurnStarted)];
    asking.extend(call_asked_for(&tests).map(|call| FakeStep::at(0.0, call)));
    asking.push(FakeStep::at(0.0, SessionEvent::NeedsUser(tests)));
    live(
        ui,
        RUNE_CLAUDE,
        "Run the window tests",
        "Running the window tests",
        asking,
    );
    live(
        ui,
        RUNE_CLAUDE,
        "Profile the long scene",
        "Profiling the long scene",
        thinking_on(),
    );
    live(
        ui,
        SKETCHES,
        "Draw the logo again",
        "Redrawing the logo",
        thinking_on(),
    );

    let garden = key_of(ui, GARDEN_ID);
    let (side, effects) = ui.app.new_group("Side projects");
    glue::run(ui, effects);
    let effects = ui.app.move_session(garden, &side);
    glue::run(ui, effects);
    let effects = ui
        .app
        .collapse_group(&GroupId::Folder(SKETCHES.into()), true);
    glue::run(ui, effects);
    let effects = ui.app.show(garden, Place::Focused, Leaving::ForgetIfEmpty);
    glue::run(ui, effects);
    pump(ui, 1.0);
    pump_until(ui, "the finished turn and the shown history", |ui| {
        let marks = ui.app.sessions.iter().filter(|e| e.unseen).count();
        marks == 1
            && ui
                .focused_session()
                .is_some_and(|s| s.conversation.items().len() == 2)
    });
}

/// Where the `group-menu` scene's menu opens: over the row of the session that needs the user,
/// in a window of 1200 by 800, as a right-click there opens it.
const GROUP_MENU_AT: PointerPress = PointerPress { x: 150.0, y: 182.0 };

/// The `sessions` scene with the menu of the row "Running the window tests" open on "Move to
/// group": the other groups, then "New group…".
fn group_menu(ui: &mut Ui) {
    sessions(ui);
    let key = key_of_title(ui, "Running the window tests");
    // As after a right-click: the open menu holds the focus, not the focused session's box.
    if let Some(focused) = ui.app.focused() {
        glue::view_mut(ui, focused).draft.focused = false;
    }
    ui.side_menu = Some(SideMenu {
        target: MenuTarget::Row(key),
        at: GROUP_MENU_AT,
        sub_open: true,
    });
}

/// The `sessions` scene with the folder menu of "New session" open: the scene's recent folders,
/// then the folders of its newest sessions, then "Choose a folder…". The menu offers the
/// folders as they are: they are made up, so the check against the disk that opening it makes
/// (`view::folder::ask`) would offer none of them.
fn folders(ui: &mut Ui) {
    sessions(ui);
    // As after a press on the button: the open menu holds the focus, not the focused session's box.
    if let Some(focused) = ui.app.focused() {
        glue::view_mut(ui, focused).draft.focused = false;
    }
    ui.folder_offers = ui.app.folder_choices();
    ui.folder_menu = Some(None);
}

/// As `mindrune` with no folder starts the first time: signed in, nothing stored, so nothing is
/// shown and the conversation area says how to start.
fn empty_start(ui: &mut Ui) {
    *ui = ui_in(None);
    start(ui);
    pump_until(ui, "the main screen and the listing", |ui| {
        ui.app.screen == Screen::Main && ui.app.listed_all
    });
}

/// The stored session of the `resting`, `history-loading`, `history-failed` and `elsewhere`
/// scenes: its id, its folder and its title.
pub(crate) const PARSER_ID: &str = "stored-parser";
const PARSER_FOLDER: &str = "/home/alex/parser";
const PARSER_TITLE: &str = "Reporting every parse error";

/// How many days before the demo's clock the parser's session was last active.
const PARSER_DAYS: i64 = 3;

/// The first message of the parser's stored history, and the start of its first reply.
pub(crate) const PARSER_QUESTION: &str = "Why does the parser stop at the first error?";
pub(crate) const PARSER_REPLY: &str = "The parser returns on the first error it meets.";

/// Why the `history-failed` scene's history could not be read, as the fake says it.
pub(crate) const HISTORY_FAILURE: &str = "the session file is gone";

/// The message the `elsewhere` scene sends, which waits on the user's answer.
pub(crate) const ELSEWHERE_MESSAGE: &str = "Carry on with the error list";

/// The parser's stored history: only its end is read, so it starts with the notice that earlier
/// messages are not shown. A question, a thought (whose time the store does not keep), a read,
/// the reply; then the summary the agent made to save room, a message with a screenshot, and
/// the last reply. It names the model that last answered and the mode it was in.
pub(crate) fn parser_history() -> StoredHistory {
    StoredHistory {
        items: vec![
            HistoryItem::User {
                text: PARSER_QUESTION.into(),
                image_count: 0,
                files: Vec::new(),
                images: Vec::new(),
                stored: None,
            },
            HistoryItem::Thinking(
                "It returns as soon as one statement fails, so nothing after it is looked at."
                    .into(),
            ),
            HistoryItem::Tool {
                id: ToolCallId("h1".into()),
                name: "Read".into(),
                detail: ToolDetail::FileRead {
                    path: "/home/alex/parser/src/parse.rs".into(),
                },
                outcome: Some(ToolOutcome::Ok {
                    output: "pub fn parse(input: &str) -> Result<Ast, Error> {\n".into(),
                }),
            },
            HistoryItem::AssistantText(format!(
                "{PARSER_REPLY} Collecting the errors in a `Vec` and going on with the next \
                 statement lets `parse` report them all at once."
            )),
            HistoryItem::Compacted,
            HistoryItem::User {
                text: "This is what it prints now.".into(),
                image_count: 1,
                files: Vec::new(),
                images: vec![None; 1],
                stored: None,
            },
            HistoryItem::AssistantText(
                "Three errors, each with its line: the parser goes on after each one now.".into(),
            ),
        ],
        earlier_left_out: true,
        model: Some("fake-model-fast".into()),
        permission_mode: Some(PermissionMode::Plan),
        effort: None,
        tasks: Vec::new(),
        choices: None,
    }
}

/// The models a look at launch reports in the demo: the fake's own list, which every program it
/// opens reports too (`fake_models` in `mr-agent`'s fake, which it does not export).
fn look_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            id: "default".into(),
            name: "Default".into(),
            resolved: "fake-model".into(),
            stands_for: Some("Fake 1".into()),
            auto_mode: true,
            effort_levels: vec!["low".into(), "medium".into(), "high".into()],
        },
        ModelOption {
            id: "fast".into(),
            name: "Fast".into(),
            resolved: "fake-model-fast".into(),
            stands_for: None,
            auto_mode: false,
            effort_levels: Vec::new(),
        },
    ]
}

/// As `mindrune` with no folder starts once a look has given the models and the usage: the
/// parser's stored session listed, its history read as `history` says (a detail is a read that
/// fails with it), nothing shown and no program opened. The demo's own look is off (see
/// `ui_in`), so its answer is taken in here as the backend would hand it over. Returns the
/// parser's key.
fn parser_listed(ui: &mut Ui, history: Result<StoredHistory, &str>) -> SessionKey {
    *ui = ui_in(None);
    let id = SessionId(PARSER_ID.into());
    fake(ui).set_sessions(vec![StoredSession {
        id: id.clone(),
        folder: PARSER_FOLDER.into(),
        title: PARSER_TITLE.into(),
        last_active_unix: NOW_UNIX - PARSER_DAYS * DAY,
    }]);
    match history {
        Ok(history) => fake(ui).set_history(&id, history),
        Err(detail) => fake(ui).fail_history(&id, detail),
    }
    start(ui);
    pump_until(ui, "the main screen and the listing", |ui| {
        ui.app.screen == Screen::Main && ui.app.listed_all
    });
    let look = FromBackend::Look {
        models: look_models(),
        model: None,
        usage: Some(limits(NOW_UNIX, 0.37, 0.10)),
    };
    let effects = ui.app.receive(look, ui.fake_time);
    glue::run(ui, effects);
    key_of(ui, PARSER_ID)
}

/// Whether the focused session's history is `state`.
fn history_is(ui: &Ui, state: &HistoryState) -> bool {
    ui.focused_session().is_some_and(|s| s.history == *state)
}

/// A stored session shown from its history, as the app restores one at launch: its program is
/// not started, and the pickers show the model and the mode it will start with.
fn resting(ui: &mut Ui) {
    let key = parser_listed(ui, Ok(parser_history()));
    let effects = ui.app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
    glue::run(ui, effects);
    pump_until(ui, "the stored history", |ui| {
        history_is(ui, &HistoryState::Loaded)
    });
}

/// The same session shown while its history is being read. The read is never carried out, so
/// the scene stays there.
fn history_loading(ui: &mut Ui) {
    let key = parser_listed(ui, Ok(parser_history()));
    let mut effects = ui.app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
    effects.retain(|effect| !matches!(effect, Effect::ReadHistory { .. }));
    glue::run(ui, effects);
}

/// The same session shown when its history cannot be read.
fn history_failed(ui: &mut Ui) {
    let key = parser_listed(ui, Err(HISTORY_FAILURE));
    let effects = ui.app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
    glue::run(ui, effects);
    pump_until(ui, "the failed read", |ui| {
        ui.focused_session()
            .is_some_and(|s| matches!(s.history, HistoryState::Failed(_)))
    });
}

/// The `resting` scene after a message was sent to it while another app had just changed it:
/// the app asks before it joins the session, holding the message.
fn elsewhere(ui: &mut Ui) {
    resting(ui);
    let id = SessionId(PARSER_ID.into());
    fake(ui).set_last_active(&id, Some(NOW_UNIX - 40));
    let key = ui.app.focused().expect("a session is shown");
    let effects = ui.app.submit(key, ELSEWHERE_MESSAGE);
    glue::run(ui, effects);
    pump_until(ui, "the question", |ui| ui.app.prompt(key).is_some());
}

/// The key of the session titled `title`.
fn key_of_title(ui: &Ui, title: &str) -> SessionKey {
    ui.app
        .sessions
        .iter()
        .find(|entry| entry.session.title.as_deref() == Some(title))
        .map(|entry| entry.key)
        .unwrap_or_else(|| panic!("no session titled {title:?}"))
}

/// A turn that starts and goes on thinking; the rest of it is scripted a minute later.
fn thinking_on() -> Vec<FakeStep> {
    vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(0.0, SessionEvent::ThinkingStarted),
        FakeStep::at(LATER, SessionEvent::BlockFinished),
        FakeStep::at(LATER, finished()),
    ]
}

/// A new session in `folder`, shown, whose first message `message` plays `steps`; Claude names
/// it `title`. Pumps until the turn has started and the fake has the message, so the reply a
/// later scene scripts is not taken for this one's.
fn live(ui: &mut Ui, folder: &str, message: &str, title: &str, steps: Vec<FakeStep>) {
    live_at(ui, Place::Focused, folder, message, title, steps);
}

/// [`live`], with the new session shown at `place`.
fn live_at(
    ui: &mut Ui,
    place: Place,
    folder: &str,
    message: &str,
    title: &str,
    steps: Vec<FakeStep>,
) {
    let effects = ui
        .app
        .new_session(folder.into(), place, Leaving::ForgetIfEmpty);
    glue::run(ui, effects);
    pump_until(ui, "the new session started", |ui| {
        ui.focused_session().is_some_and(|s| s.phase == Phase::Idle)
    });
    fake(ui).reply(steps);
    say(ui, message, title);
    let id = ui.focused_session().and_then(|s| s.id.clone());
    pump_until(ui, "its turn under way", |ui| {
        let sent = fake(ui)
            .sent()
            .into_iter()
            .any(|(to, command)| Some(to) == id && matches!(command, SessionCommand::Send { .. }));
        sent && ui
            .focused_session()
            .is_some_and(|s| matches!(s.phase, Phase::Working | Phase::NeedsUser))
    });
}

/// A turn that asks to run `touch a.txt` (request r1), offering a rule to always allow it.
fn asking_turn() -> Vec<FakeStep> {
    let mut asking = vec![FakeStep::at(0.0, SessionEvent::TurnStarted)];
    let request = touch("r1", Some("Always allow Bash(touch a.txt) in this project"));
    asking.extend(call_asked_for(&request).map(|call| FakeStep::at(0.0, call)));
    asking.push(FakeStep::at(0.0, SessionEvent::NeedsUser(request)));
    asking
}

/// Three tiles side by side, for a window of 1,800 by 900: the `conversation` scene's finished
/// conversation, then a session whose permission request waits, then one at work. The middle
/// one is focused, its input box with the keyboard focus.
fn tiles(ui: &mut Ui) {
    conversation(ui);
    live_at(
        ui,
        Place::Side,
        "/home/alex/website",
        PERMISSION_MESSAGE,
        PERMISSION_TITLE,
        asking_turn(),
    );
    let asks = ui.app.focused().expect("the second tile is focused");
    let working = vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(
            0.0,
            tool(
                "c1",
                "Bash",
                ToolDetail::Command {
                    command: "sleep 30".into(),
                },
            ),
        ),
        FakeStep::at(0.0, SessionEvent::ThinkingStarted),
        FakeStep::at(LATER, SessionEvent::BlockFinished),
        FakeStep::at(LATER, done("c1", "")),
        FakeStep::at(LATER, finished()),
    ];
    live_at(
        ui,
        Place::Side,
        SKETCHES,
        "Wait half a minute, then tell me",
        "Waiting half a minute",
        working,
    );
    focus_tile(ui, asks);
}

/// Focuses `key`'s tile, its input box with the keyboard focus. Each tile's box asked for the
/// keyboard focus as its tile opened: only the focused one's keeps asking.
fn focus_tile(ui: &mut Ui, key: SessionKey) {
    let effects = ui.app.focus_tile(key);
    glue::run(ui, effects);
    let tiled: Vec<SessionKey> = ui.app.tiles.keys();
    for tiled in tiled {
        glue::view_mut(ui, tiled).draft.focused = tiled == key;
    }
}

/// The user's message in the middle tile of the `tiles-needs-you` scene, Claude's reply, and
/// the title Claude gives the session.
const README_MESSAGE: &str = "What does the README say?";
const README_REPLY: &str = "It says how to build the sketches and how to run each one.";
const README_TITLE: &str = "Reading the README";

/// How long the middle tile's turn in the `tiles-needs-you` scene lasts, in seconds.
const README_TURN: f32 = 2.0;

/// Three tiles side by side, for a window of 1,800 by 900: the `conversation` scene's finished
/// conversation, focused; a session whose turn finished while its tile was not the focused
/// one, so that it says "Finished"; and a session whose permission request waits.
fn tiles_needs_you(ui: &mut Ui) {
    conversation(ui);
    let first = ui.app.focused().expect("the first tile is focused");
    let reading = vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(README_TURN, SessionEvent::TextDelta(README_REPLY.into())),
        FakeStep::at(README_TURN, SessionEvent::BlockFinished),
        FakeStep::at(README_TURN, finished()),
    ];
    live_at(
        ui,
        Place::Side,
        SKETCHES,
        README_MESSAGE,
        README_TITLE,
        reading,
    );
    let reads = ui.app.focused().expect("the second tile is focused");
    live_at(
        ui,
        Place::Side,
        "/home/alex/website",
        PERMISSION_MESSAGE,
        PERMISSION_TITLE,
        asking_turn(),
    );
    focus_tile(ui, first);
    pump(ui, README_TURN);
    pump_until(ui, "the middle tile's turn finished out of sight", |ui| {
        ui.app
            .sessions
            .get(reads)
            .is_some_and(|entry| entry.mark() == Mark::Finished)
    });
}

/// Where the `tiles-drag` scene's pointer is, in window points: in the tile row of a window of
/// 1,800 by 900, just right of the edge between its two tiles.
const DRAG_AT: [f32; 2] = [1_040.0, 380.0];

/// The tile row of a window of 1,800 by 900, as the window lays it out: x, y, width, height.
/// The snapshot test runs `tiles-drag`, `tiles-stack-drag` and `tiles-move` at that size
/// (`WIDE` in `tests/snapshots.rs`).
const WIDE_ROW: [f32; 4] = [260.0, 48.0, 1_532.0, 844.0];

/// Two tiles, for a window of 1,800 by 900: the `tiles` scene with its middle tile closed, and
/// that tile's session, whose permission request waits, dragged by its sidebar row half way
/// to the tile row, where the line marks the edge between the two tiles.
fn tiles_drag(ui: &mut Ui) {
    tiles(ui);
    let asks = ui.app.focused().expect("the middle tile is focused");
    let effects = ui.app.close_tile(asks, Leaving::Keep);
    glue::run(ui, effects);
    let focused = ui
        .app
        .focused()
        .expect("the tile on its right takes the focus");
    focus_tile(ui, focused);
    // The line is placed from the row's last layout, which a snapshot's one frame comes too
    // late for. No pointer moves come in a snapshot, so the watcher of a lost release never
    // ends the drag.
    ui.row_rect = Some(WIDE_ROW);
    ui.app.set_row_width(WIDE_ROW[2]);
    ui.drag = Some(RowDrag {
        key: asks,
        at: DRAG_AT,
        over: Some(DropTarget::Edge(1)),
        heard: true,
        missed: 0,
        from: DragFrom::Row,
    });
}

/// Where the `tiles-stack-drag` scene's pointer is, in window points: in the tile row of a
/// window of 1,800 by 900, over the lower half of the first tile's middle.
const STACK_AT: [f32; 2] = [643.0, 680.0];

/// The `tiles-drag` scene with the row dragged on, over the lower half of the first tile's
/// middle, where the outline marks the half a tile would open in.
fn tiles_stack_drag(ui: &mut Ui) {
    tiles_drag(ui);
    let first = ui.app.tiles.columns()[0].top;
    let drag = ui.drag.as_mut().expect("the row is dragged");
    drag.at = STACK_AT;
    drag.over = Some(DropTarget::Below(first));
}

/// Where the `tiles-move` scene's pointer is, in window points: in the headers of a window of
/// 1,800 by 900, a little right of the edge between the second tile and the third.
const MOVE_AT: [f32; 2] = [1_300.0, 66.0];

/// The `tiles` scene's three tiles, for a window of 1,800 by 900, the first dragged by its
/// header to the edge between the second and the third, where the line shows it would go.
fn tiles_move(ui: &mut Ui) {
    tiles(ui);
    let first = ui.app.tiles.columns()[0].top;
    // As for `tiles-drag`: the line is placed from the row's last layout, which a snapshot's
    // one frame comes too late for.
    ui.row_rect = Some(WIDE_ROW);
    ui.app.set_row_width(WIDE_ROW[2]);
    ui.drag = Some(RowDrag {
        key: first,
        at: MOVE_AT,
        over: Some(DropTarget::Edge(2)),
        heard: true,
        missed: 0,
        from: DragFrom::Header,
    });
}

/// The `tiles` scene's three tiles, for a window of 1,800 by 900, the third moved under the
/// second: a column of two whose upper tile's permission request waits, its lower tile, at
/// work, focused.
fn tiles_stacked(ui: &mut Ui) {
    tiles(ui);
    let columns = ui.app.tiles.columns();
    let (asks, working) = (columns[1].top, columns[2].top);
    let effects = ui.app.move_tile(working, Spot::Below(asks));
    glue::run(ui, effects);
    focus_tile(ui, working);
}

/// The `conversation` scene with its title being edited in the tile's header, as a double click
/// on the title starts it: the whole title selected.
fn tile_rename(ui: &mut Ui) {
    conversation(ui);
    let key = ui.app.focused().expect("a session is shown");
    glue::start_rename(ui, Renaming::Header(key));
}

/// The message the `panel` scene's focused session goes on with, and the reply that ends it.
const EDITS_MESSAGE: &str = "Make the parser say where each error is";
const EDITS_REPLY: &str = "Each error now names its line and column, and the notes say so.";

/// The `tiles` scene's three tiles, for a window of 1,800 by 900, with the right panel open
/// beside them, naming the focused session. That is the first tile's, focused, which has gone
/// on with a turn that changed four files, one of them outside its folder: the panel shows 37%
/// of its context used and what that is made of, $0.42 spent and the four files.
fn panel(ui: &mut Ui) {
    tiles(ui);
    let first = ui.app.tiles.columns()[0].top;
    focus_tile(ui, first);
    let edit = |id: &str, path: &str, old: &str, new: &str| {
        let detail = ToolDetail::FileEdit {
            path: path.into(),
            old_text: old.into(),
            new_text: new.into(),
        };
        [tool(id, "Edit", detail), done(id, "The file was updated.")]
    };
    let write = ToolDetail::FileWrite {
        path: format!("{FOLDER}/tests/errors.rs"),
        content: (1..=12).map(|n| format!("// case {n}\n")).collect(),
    };
    let mut steps = vec![SessionEvent::TurnStarted];
    steps.extend(edit(
        "e1",
        &format!("{FOLDER}/src/parser.rs"),
        "    Err(Error::new(message))\n",
        "    let at = self.position();\n    Err(Error::new(message).at(at.line, at.column))\n",
    ));
    steps.extend(edit(
        "e2",
        &format!("{FOLDER}/src/error.rs"),
        "pub struct Error {\n    message: String,\n}\n",
        "pub struct Error {\n    message: String,\n    line: usize,\n    column: usize,\n}\n",
    ));
    steps.extend([tool("w1", "Write", write), done("w1", "File created.")]);
    steps.extend(edit(
        "e3",
        "/home/alex/notes/parser.md",
        "",
        "Errors name their line and column.\nEach one has a test in tests/errors.rs.\n",
    ));
    steps.extend([
        SessionEvent::TextDelta(EDITS_REPLY.into()),
        SessionEvent::BlockFinished,
        SessionEvent::TurnFinished(TurnSummary {
            cost_usd: 0.42,
            context_used: Some(0.37),
            duration_ms: 24_000,
            interrupted: false,
        }),
    ]);
    fake(ui).reply(steps.into_iter().map(|e| FakeStep::at(0.0, e)).collect());
    let effects = ui.app.submit(first, EDITS_MESSAGE);
    glue::run(ui, effects);
    pump_until(ui, "the turn that changed four files", |ui| {
        ui.focused_session()
            .is_some_and(|s| s.phase == Phase::Idle && s.conversation.changes().files().len() == 4)
    });
    // What the program counted after the turn: 74,000 of 200,000 used, the 37% above.
    let breakdown = panel_breakdown();
    emit_to(ui, first, SessionEvent::ContextBreakdown(breakdown.clone()));
    pump_until(ui, "the breakdown of the context", |ui| {
        ui.focused_session()
            .is_some_and(|s| s.context_parts.as_ref() == Some(&breakdown))
    });
    // What the library thread would answer for a repository on `main`.
    let branch = FromBackend::Branch {
        folder: FOLDER.into(),
        branch: Some(Branch::Named("main".into())),
    };
    let effects = ui.app.receive(branch, ui.fake_time);
    glue::run(ui, effects);
    ui.panel.set_open(true);
}

/// What the `panel` scene's session's context is made of: its used parts sum to 74,000 of a
/// window of 200,000.
fn panel_breakdown() -> ContextBreakdown {
    let part = |name: &str, tokens: u64, kind: ContextKind| ContextPart {
        name: name.into(),
        tokens,
        kind,
    };
    ContextBreakdown {
        parts: vec![
            part("System prompt", 3_000, ContextKind::Used),
            part("System tools", 12_000, ContextKind::Used),
            part("Memory files", 700, ContextKind::Used),
            part("Skills", 2_000, ContextKind::Used),
            part("Messages", 56_300, ContextKind::Used),
            part("Autocompact buffer", 33_000, ContextKind::Buffer),
            part("Free space", 93_000, ContextKind::Free),
        ],
        window: 200_000,
    }
}

/// The address the demo's sessions are reachable at by Remote Control. It is never shown.
const REMOTE_ADDRESS: &str = "https://example.org/session";

/// The `panel` scene with Remote Control turned on for the focused tile's session, as its
/// switch turns it on: the program connects, then says it is reachable. The tile's header says
/// "Remote", and the panel offers "Copy link".
fn panel_remote(ui: &mut Ui) {
    let key = remote_asked(ui);
    emit_to(ui, key, SessionEvent::RemoteAccess(RemoteAccess::Starting));
    emit_to(
        ui,
        key,
        SessionEvent::RemoteAccess(RemoteAccess::On {
            url: Some(REMOTE_ADDRESS.into()),
        }),
    );
    pump_until(ui, "Remote Control on", |ui| {
        ui.focused_session()
            .is_some_and(|s| matches!(s.remote, Remote::On { .. }))
    });
}

/// The `panel` scene with Remote Control asked for in the focused tile's session and refused
/// for want of a full claude.ai sign-in: the switch is off again, and the reason is under it
/// and in the conversation.
fn panel_refused(ui: &mut Ui) {
    let key = remote_asked(ui);
    emit_to(ui, key, SessionEvent::RemoteAccess(RemoteAccess::Starting));
    let refused = RemoteAccess::Failed {
        reason: "/login".into(),
        why: RemoteFailure::Login,
    };
    emit_to(ui, key, SessionEvent::RemoteAccess(refused));
    pump_until(ui, "Remote Control refused", |ui| {
        ui.focused_session()
            .is_some_and(|s| s.remote_said.is_some())
    });
}

/// The `panel` scene, its focused tile's session asking for Remote Control. Returns that
/// session.
fn remote_asked(ui: &mut Ui) -> SessionKey {
    panel(ui);
    let key = ui.app.focused().expect("a session is shown");
    let effects = ui.app.set_remote(key, true);
    glue::run(ui, effects);
    key
}

/// The key of the session the agent knows as `id`.
fn key_of(ui: &Ui, id: &str) -> SessionKey {
    ui.app
        .sessions
        .iter()
        .find(|entry| entry.session.id.as_ref().is_some_and(|i| i.0 == id))
        .map(|entry| entry.key)
        .unwrap_or_else(|| panic!("no session {id}"))
}

/// The ids of the chips the scenes put in place: far above those `App::attach` gives, which
/// count from 1, so a chip attached in the demo window never takes one of theirs.
const SCENE_CHIPS: u64 = 9_000;

/// The draft of the `chips` scene, two lines.
const CHIPS_DRAFT: &str =
    "Does the login form in the screenshot match the notes?\nThe folder has the form's code.";

/// A focused tile in the demo's folder with a draft of two lines and four chips: a screenshot
/// ready to send, its thumbnail made in code; a file; a folder; and a pasted image still being
/// read, so the send button is off.
fn chips(ui: &mut Ui) {
    empty(ui);
    let key = ui.app.focused().expect("a session is shown");
    let pasted = SCENE_CHIPS + 4;
    put_chips(
        ui,
        key,
        vec![
            image_chip(
                SCENE_CHIPS + 1,
                "/home/alex/project/screenshots/login-form.png",
            ),
            path_chip(
                SCENE_CHIPS + 2,
                "/home/alex/project/notes/login.md",
                AttachKind::File,
            ),
            path_chip(
                SCENE_CHIPS + 3,
                "/home/alex/project/src/login",
                AttachKind::Folder,
            ),
            Chip {
                id: pasted,
                name: mr_core::name_of(&pasted_source()),
                source: pasted_source(),
                state: ChipState::Reading,
            },
        ],
    );
    glue::view_mut(ui, key).draft = TextEdit::new(CHIPS_DRAFT);
    focus_tile(ui, key);
}

/// The message the right tile of the `chips-problem` scene was sent with its three chips,
/// Claude's reply, and the title Claude gives the session.
const SKETCH_MESSAGE: &str = "Does the sketch follow the plan?";
const SKETCH_REPLY: &str = "Mostly. The whiteboard draws the three shapes the plan lists, but not the spiral that shapes.txt adds.";
const SKETCH_TITLE: &str = "Checking the sketch against the plan";

/// The draft beside the chip that cannot be sent in the `chips-problem` scene.
const PANORAMA_DRAFT: &str = "Which path in this photo is the shortest?";

/// Two tiles. The left one, focused, has a draft and one chip, a photo too large to send, so the
/// send button is off. The right one's last message went with an image and two files, and
/// Claude has answered it: the message shows the image, its chip's thumbnail made in code, and
/// names the two files.
fn chips_problem(ui: &mut Ui) {
    empty(ui);
    let asks = ui.app.focused().expect("a session is shown");
    let effects = ui
        .app
        .new_session(SKETCHES.into(), Place::Side, Leaving::ForgetIfEmpty);
    glue::run(ui, effects);
    pump_until(ui, "the new session started", |ui| {
        ui.focused_session().is_some_and(|s| s.phase == Phase::Idle)
    });
    let sent = ui.app.focused().expect("the new tile is focused");
    put_chips(
        ui,
        sent,
        vec![
            image_chip(SCENE_CHIPS + 1, "/home/alex/sketches/whiteboard.png"),
            path_chip(
                SCENE_CHIPS + 2,
                "/home/alex/sketches/plan.md",
                AttachKind::File,
            ),
            path_chip(
                SCENE_CHIPS + 3,
                "/home/alex/sketches/shapes.txt",
                AttachKind::File,
            ),
        ],
    );
    fake(ui).reply(vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(0.0, SessionEvent::TextDelta(SKETCH_REPLY.into())),
        FakeStep::at(0.0, SessionEvent::BlockFinished),
        FakeStep::at(0.0, finished()),
    ]);
    say(ui, SKETCH_MESSAGE, SKETCH_TITLE);
    pump_until(ui, "the reply to the message with chips", |ui| {
        ui.app.sessions.get(sent).is_some_and(|entry| {
            entry.session.phase == Phase::Idle && entry.session.conversation.items().len() == 2
        })
    });
    let panorama = "/home/alex/project/photos/harbour-panorama.jpg";
    put_chips(
        ui,
        asks,
        vec![Chip {
            id: SCENE_CHIPS + 4,
            name: mr_core::name_of(&Source::Path(panorama.into())),
            source: Source::Path(panorama.into()),
            state: ChipState::Problem(Problem::TooLarge),
        }],
    );
    glue::view_mut(ui, asks).draft = TextEdit::new(PANORAMA_DRAFT);
    focus_tile(ui, asks);
}

/// Puts `chips` on `key`'s session, as their answers from the preparing thread would have left
/// them, and makes the thumbnail of each ready image as `glue` makes one for a real answer. A
/// scene waits for each: a thumbnail still being made paints nothing in a snapshot. The cache
/// is the one `start` made; the window's own setup makes another after the scene, and a handle
/// made here stays drawn.
fn put_chips(ui: &mut Ui, key: SessionKey, chips: Vec<Chip>) {
    for chip in &chips {
        let ChipState::Ready(Prepared {
            thumb: Some(thumb), ..
        }) = &chip.state
        else {
            continue;
        };
        let images = ui
            .images
            .as_ref()
            .expect("the scene has started the backend");
        let image = images.rgba(thumb.width, thumb.height, thumb.rgba.clone());
        image.wait().expect("a thumbnail made in code is made");
        ui.thumbs.insert(chip.id, image);
    }
    let mut entry = ui.app.sessions.get_mut(key).expect("the session is there");
    entry.session.chips.extend(chips);
}

/// A chip of `path`, ready to go by its path as a file or a folder.
fn path_chip(id: u64, path: &str, kind: AttachKind) -> Chip {
    let source = Source::Path(path.into());
    Chip {
        id,
        name: mr_core::name_of(&source),
        source,
        state: ChipState::Ready(Prepared {
            kind,
            attachment: Attachment::File { path: path.into() },
            thumb: None,
        }),
    }
}

/// A chip of the PNG image at `path`, ready to send, with a thumbnail made in code. Its bytes
/// are a stand-in: nothing in the demo decodes them, and nothing is read from the disk.
fn image_chip(id: u64, path: &str) -> Chip {
    let source = Source::Path(path.into());
    Chip {
        id,
        name: mr_core::name_of(&source),
        source,
        state: ChipState::Ready(Prepared {
            kind: AttachKind::Image,
            attachment: Attachment::Image {
                media_type: "image/png".into(),
                data: ImageData::new(&b"\x89PNG\r\n\x1a\n"[..]),
            },
            thumb: Some(window_thumb()),
        }),
    }
}

/// A pasted image: a stand-in for the PNG bytes a clipboard gives.
fn pasted_source() -> Source {
    Source::Pasted(ImageData::new(&b"\x89PNG\r\n\x1a\n"[..]))
}

/// A thumbnail made in code, as wide as a chip's thumbnail may be and two thirds as high: a
/// window over a sky that darkens downwards, with two lines of text in it.
fn window_thumb() -> Thumb {
    let (width, height) = (THUMB_SIDE, THUMB_SIDE * 2 / 3);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let in_window = (20..76).contains(&x) && (14..50).contains(&y);
            let in_line = (28..68).contains(&x) && ((24..28).contains(&y) || (34..38).contains(&y));
            let pixel = if in_line {
                [96, 108, 128]
            } else if in_window {
                [240, 243, 248]
            } else {
                let down = y as f32 / height as f32;
                [
                    (120.0 - 70.0 * down) as u8,
                    (180.0 - 80.0 * down) as u8,
                    (235.0 - 60.0 * down) as u8,
                ]
            };
            rgba.extend_from_slice(&pixel);
            rgba.push(255);
        }
    }
    Thumb {
        width,
        height,
        rgba: rgba.into(),
    }
}

/// Two tiles and the question whether to quit over them, as Ctrl+Q asks it: on the left the
/// `conversation` scene's session with Remote Control on, on the right, focused, a session at
/// work. `glue::ask_quit` puts away what lies over the window and shows the question when
/// something runs; a scene has no `Tx` to call it with, so it does the same itself.
fn quit_ask(ui: &mut Ui) {
    conversation(ui);
    let remote = ui.app.focused().expect("a session is shown");
    let effects = ui.app.set_remote(remote, true);
    glue::run(ui, effects);
    emit_to(
        ui,
        remote,
        SessionEvent::RemoteAccess(RemoteAccess::Starting),
    );
    emit_to(
        ui,
        remote,
        SessionEvent::RemoteAccess(RemoteAccess::On {
            url: Some(REMOTE_ADDRESS.into()),
        }),
    );
    pump_until(ui, "Remote Control on", |ui| {
        ui.app
            .sessions
            .get(remote)
            .is_some_and(|entry| matches!(entry.session.remote, Remote::On { .. }))
    });
    live_at(
        ui,
        Place::Side,
        SKETCHES,
        "Wait half a minute, then tell me",
        "Waiting half a minute",
        thinking_on(),
    );
    glue::put_away(ui);
    ui.quit_ask = true;
    // As the window is while the question shows: the question has the keyboard, so no input
    // box is drawn with it. The first tile's box still had it from the scene's first message.
    for tiled in ui.app.tiles.keys() {
        glue::view_mut(ui, tiled).draft.focused = false;
    }
}

/// The message the `viewer` scene's picture was sent with, Claude's reply, and the title Claude
/// gives the session.
const BLOCKS_MESSAGE: &str = "Which of these colours would suit the buttons?";
const BLOCKS_REPLY: &str = "The blue of the second row reads well on both themes.";
const BLOCKS_TITLE: &str = "Choosing a colour for the buttons";

/// How large the `viewer` scene's picture is, in pixels.
const VIEWER_SIZE: (u32, u32) = (900, 600);

/// A message sent with a picture of coloured blocks, 900 by 600 pixels and made in code, and
/// Claude's reply; then a press on the sent picture opened it large over the window. A picture
/// sent in this run is at hand in full, so it shows at once: the scene waits for it to be
/// decoded, as a snapshot paints nothing of an image still being decoded.
fn viewer(ui: &mut Ui) {
    empty(ui);
    let key = ui.app.focused().expect("a session is shown");
    let picture = SCENE_CHIPS + 1;
    let (width, height) = VIEWER_SIZE;
    let rgb = blocks(width, height);
    let path = "/home/alex/project/colours.png";
    let source = Source::Path(path.into());
    let chip = Chip {
        id: picture,
        name: mr_core::name_of(&source),
        source,
        state: ChipState::Ready(Prepared {
            kind: AttachKind::Image,
            attachment: Attachment::Image {
                media_type: "image/png".into(),
                data: ImageData::new(png(width, height, &rgb)),
            },
            thumb: Some(thumb_of(width, height, &rgb)),
        }),
    };
    put_chips(ui, key, vec![chip]);
    fake(ui).reply(vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(0.0, SessionEvent::TextDelta(BLOCKS_REPLY.into())),
        FakeStep::at(0.0, SessionEvent::BlockFinished),
        FakeStep::at(0.0, finished()),
    ]);
    say(ui, BLOCKS_MESSAGE, BLOCKS_TITLE);
    pump_until(ui, "the reply to the picture", |ui| {
        ui.focused_session()
            .is_some_and(|s| s.phase == Phase::Idle && s.conversation.items().len() == 2)
    });
    glue::open_picture(ui, key, picture);
    let full = ui
        .viewing
        .as_ref()
        .and_then(|viewing| viewing.full.clone())
        .expect("a picture sent in this run is at hand");
    full.wait().expect("a picture made in code decodes");
}

/// Why the `viewer-failed` scene's picture cannot be read again, as the store would say it.
const IMAGE_FAILURE: &str = "the message is not in the session";

/// The `resting` scene's stored session, whose message with a screenshot kept the screenshot's
/// bytes, and a press on its picture: the full image is asked of the store, which cannot give
/// it. The small picture stays, with the line that says so.
fn viewer_failed(ui: &mut Ui) {
    let mut history = parser_history();
    for item in &mut history.items {
        if let HistoryItem::User { images, stored, .. } = item
            && !images.is_empty()
        {
            let (width, height) = (640, 400);
            *images = vec![Some(ImageData::new(png(
                width,
                height,
                &terminal(width, height),
            )))];
            *stored = Some("stored-screenshot".into());
        }
    }
    let key = parser_listed(ui, Ok(history));
    fake(ui).fail_image(IMAGE_FAILURE);
    let effects = ui.app.show(key, Place::Focused, Leaving::ForgetIfEmpty);
    glue::run(ui, effects);
    pump_until(ui, "the stored history and its picture", |ui| {
        ui.focused_session().is_some_and(|s| {
            s.history == HistoryState::Loaded
                && s.conversation
                    .pictures()
                    .any(|id| ui.thumbs.contains_key(&id))
        })
    });
    let picture = ui
        .focused_session()
        .and_then(|s| {
            s.conversation
                .pictures()
                .find(|id| ui.thumbs.contains_key(id))
        })
        .expect("the picture is drawn");
    ui.thumbs[&picture]
        .wait()
        .expect("a picture made in code is drawn");
    glue::open_picture(ui, key, picture);
    pump_until(ui, "the failed read", |ui| {
        ui.viewing.as_ref().is_some_and(|viewing| viewing.failed)
    });
}

/// The folder of the `colours` scene's collapsed group.
const RECIPES: &str = "/home/alex/recipes";

/// Three tiles side by side, for a window of 1,800 by 900, in the colours of their states: a
/// session whose turn finished while its tile was not the focused one (green), one whose
/// permission request waits (orange), and one at work, focused. In the sidebar their rows take
/// the same colours, and "recipes", collapsed, shows the green dot of a session that finished
/// in no tile. The arrow keys have passed onto the orange row, as they do in the list: it is the
/// selected one, and its fill is all it shows.
fn colours(ui: &mut Ui) {
    empty(ui);
    pump_until(ui, "the listing", |ui| ui.app.listed_all);
    // Its turn ends a second from now, after another session took its tile.
    let sorting = vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(1.0, SessionEvent::TextDelta("Soups first.".into())),
        FakeStep::at(1.0, SessionEvent::BlockFinished),
        FakeStep::at(1.0, finished()),
    ];
    live(
        ui,
        RECIPES,
        "Sort the soup recipes",
        "Sorting the soup recipes",
        sorting,
    );
    let sorted = ui.app.focused().expect("the recipes session is shown");
    let reading = vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(README_TURN, SessionEvent::TextDelta(README_REPLY.into())),
        FakeStep::at(README_TURN, SessionEvent::BlockFinished),
        FakeStep::at(README_TURN, finished()),
    ];
    live(ui, SKETCHES, README_MESSAGE, README_TITLE, reading);
    let reads = ui.app.focused().expect("the first tile is focused");
    live_at(
        ui,
        Place::Side,
        "/home/alex/website",
        PERMISSION_MESSAGE,
        PERMISSION_TITLE,
        asking_turn(),
    );
    let asks = ui.app.focused().expect("the second tile is focused");
    live_at(
        ui,
        Place::Side,
        RUNE_CLAUDE,
        "Profile the long scene",
        "Profiling the long scene",
        thinking_on(),
    );
    let working = ui.app.focused().expect("the third tile is focused");
    focus_tile(ui, working);
    let effects = ui
        .app
        .collapse_group(&GroupId::Folder(RECIPES.into()), true);
    glue::run(ui, effects);
    pump(ui, README_TURN);
    pump_until(ui, "both turns finished out of sight", |ui| {
        [sorted, reads].iter().all(|key| {
            ui.app
                .sessions
                .get(*key)
                .is_some_and(|entry| entry.mark() == Mark::Finished)
        })
    });
    // As an arrow key in the list leaves it on the row of a session in another tile.
    ui.passed_row = Some((asks, ui.app.tiles.placement()));
}

/// Two stored sessions of the `deleted` scene's, in "website" with the scene's own: the id, the
/// folder, the title and how many days before the demo's clock each was last active.
const DELETED_EXTRA: &[(&str, &str, &str, i64)] = &[
    (
        "stored-site-banner",
        "/home/alex/website",
        "Resizing the banner",
        1,
    ),
    (
        "stored-site-fonts",
        "/home/alex/website",
        "Choosing the fonts",
        4,
    ),
];

/// Why the `deleted` scene's one removal failed, as the fake says it.
const REMOVE_FAILURE: &str = "permission denied";

/// The `sessions` scene, with the finished session shown in place of the garden's, and the
/// Deleted view: "website", with a session deleted now (6 days left) and one whose removal
/// failed ("Not removed"), and "Side projects", deleted as a group two days ago with
/// the garden's session (4 days left). That one gets there by the app's own path: deleted, then
/// "Delete All" while the fake refuses to remove. The other two are deleted with the demo's
/// clock set back and then forward again, as time passes between them.
fn deleted(ui: &mut Ui) {
    sessions_with(ui, DELETED_EXTRA);
    let finished = key_of_title(ui, "Tidying the sidebar keys");
    let effects = ui
        .app
        .show(finished, Place::Focused, Leaving::ForgetIfEmpty);
    glue::run(ui, effects);

    let fonts = key_of(ui, "stored-site-fonts");
    fake(ui).fail_remove(REMOVE_FAILURE);
    let effects = ui.app.delete_session(fonts);
    glue::run(ui, effects);
    let effects = ui.app.delete_all();
    glue::run(ui, effects);
    pump_until(ui, "the failed removal", |ui| {
        ui.app
            .sessions
            .get(fonts)
            .and_then(|entry| entry.deleted)
            .is_some_and(|deleted| deleted.failed_unix.is_some())
    });
    fake(ui).allow_remove();

    let garden = key_of(ui, GARDEN_ID);
    let side = ui
        .app
        .sessions
        .get(garden)
        .map(|entry| ui.app.groups.group_of(entry))
        .expect("the garden's session is listed");
    ui.app.now_unix = NOW_UNIX - 2 * DAY;
    let effects = ui.app.delete_group(&side);
    glue::run(ui, effects);
    ui.app.now_unix = NOW_UNIX;
    let banner = key_of(ui, "stored-site-banner");
    let effects = ui.app.delete_session(banner);
    glue::run(ui, effects);
    let effects = ui.app.set_sidebar_view(SidebarView::Deleted);
    glue::run(ui, effects);
    pump(ui, 0.0);
}

/// The `sessions` scene with the Deleted view shown and nothing deleted.
fn deleted_empty(ui: &mut Ui) {
    sessions(ui);
    let effects = ui.app.set_sidebar_view(SidebarView::Deleted);
    glue::run(ui, effects);
}

/// The `sessions` scene with the question whether to delete the group "rune_claude" over it:
/// three sessions go with it, two of them still working. As the window is while the question
/// shows, no input box has the keyboard.
fn delete_group_ask(ui: &mut Ui) {
    sessions(ui);
    glue::ask_delete_group(ui, GroupId::Folder(RUNE_CLAUDE.into()));
    unfocus_boxes(ui);
}

/// The `deleted` scene with the question whether to remove everything there for good.
fn delete_all_ask(ui: &mut Ui) {
    deleted(ui);
    glue::ask_delete_all(ui);
    unfocus_boxes(ui);
}

/// As the window is while a question shows: the question has the keyboard, so no tile's input
/// box is drawn with it.
fn unfocus_boxes(ui: &mut Ui) {
    for tiled in ui.app.tiles.keys() {
        glue::view_mut(ui, tiled).draft.focused = false;
    }
}

/// `width` by `height` pixels of coloured blocks, six across and four down, the hue changing
/// across and the shade down, with thin pale lines between them and round the whole picture:
/// as RGB, row by row.
fn blocks(width: u32, height: u32) -> Vec<u8> {
    const HUES: [[f32; 3]; 6] = [
        [220.0, 70.0, 60.0],
        [235.0, 150.0, 40.0],
        [225.0, 200.0, 60.0],
        [70.0, 170.0, 90.0],
        [50.0, 120.0, 210.0],
        [140.0, 80.0, 190.0],
    ];
    let (across, down) = (6, 4);
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let (column, row) = (x * across / width, y * down / height);
            // On the left and top of each block, and along the picture's right and bottom
            // edge, so that the picture is framed all round and does not look cut off.
            let line = (x * across) % width < across * 4
                || (y * down) % height < down * 4
                || x + 4 >= width
                || y + 4 >= height;
            let pixel = if line {
                [236, 238, 242]
            } else {
                let shade = 1.0 - 0.18 * row as f32;
                HUES[column as usize].map(|c| (c * shade) as u8)
            };
            rgb.extend_from_slice(&pixel);
        }
    }
    rgb
}

/// `width` by `height` pixels of a terminal: a dark ground and three lines of output, each a
/// red mark and a pale run of text, as RGB, row by row.
fn terminal(width: u32, height: u32) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let line = (0..3).find(|n| (60 + n * 60..84 + n * 60).contains(&y));
            let pixel = match line {
                Some(_) if (40..64).contains(&x) => [225, 85, 75],
                Some(n) if (80..440 - n * 70).contains(&x) => [210, 214, 222],
                _ => [30, 32, 38],
            };
            rgb.extend_from_slice(&pixel);
        }
    }
    rgb
}

/// The thumbnail of `rgb`, `width` by `height` pixels: as wide as a chip's thumbnail may be, its
/// pixels the nearest of the picture's.
fn thumb_of(width: u32, height: u32, rgb: &[u8]) -> Thumb {
    let (thumb_width, thumb_height) = (THUMB_SIDE, THUMB_SIDE * height / width);
    let mut rgba = Vec::with_capacity((thumb_width * thumb_height * 4) as usize);
    for y in 0..thumb_height {
        for x in 0..thumb_width {
            let at = ((y * height / thumb_height) * width + x * width / thumb_width) as usize * 3;
            rgba.extend_from_slice(&rgb[at..at + 3]);
            rgba.push(255);
        }
    }
    Thumb {
        width: thumb_width,
        height: thumb_height,
        rgba: rgba.into(),
    }
}

/// `rgb`, `width` by `height` pixels, as a PNG whose rows are stored as they are, without
/// compression: the demo makes its pictures in code, and the app has no image encoder of its
/// own to make them with.
fn png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgb.len() + height as usize);
    for row in rgb.chunks(width as usize * 3) {
        // Each row starts with its filter: none.
        raw.push(0);
        raw.extend_from_slice(row);
    }
    // A zlib stream of stored blocks of at most 65,535 bytes, ended by the data's Adler-32.
    let mut zlib = vec![0x78, 0x01];
    let count = raw.chunks(65_535).count();
    for (n, block) in raw.chunks(65_535).enumerate() {
        zlib.push(u8::from(n + 1 == count));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // Eight bits a channel, RGB, deflate, the standard filters, not interlaced.
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png_chunk(&mut png, b"IHDR", &header);
    png_chunk(&mut png, b"IDAT", &zlib);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

/// Adds a chunk of `kind` holding `data` to `png`: its length, its kind, the data, and the
/// CRC-32 of kind and data.
fn png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let mut crc = !0u32;
    for &byte in &png[start..] {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    png.extend_from_slice(&(!crc).to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    #[test]
    fn the_chips_scene_shows_four_chips_and_one_thumbnail() {
        let rt = testkit::window("chips");
        let key = testkit::focused(&rt).expect("a tile is focused");
        let chips = &rt.state.app.sessions.get(key).unwrap().session.chips;
        assert_eq!(chips.len(), 4);
        let shown = |inner: String| testkit::element(&rt, &testkit::tile(key, &inner)).is_some();
        let [image, file, folder, reading] = [0, 1, 2, 3].map(|n| chips[n].id);
        assert!(shown(format!("mr-chip-thumb-{image}")));
        assert!(rt.state.thumbs[&image].is_ready());
        assert!(shown(format!("mr-chip-file-{file}")));
        assert!(shown(format!("mr-chip-folder-{folder}")));
        assert!(shown(format!("mr-chip-photo-{reading}")));
        assert!(shown(format!("mr-chip-reading-{reading}")));
        assert_eq!(chips[3].state, ChipState::Reading);
        let draft = &glue::view_of(&rt.state, key).draft.text;
        assert_eq!(draft.lines().count(), 2);
        assert!(testkit::box_focused(&rt, key));
        assert!(
            !rt.state.app.can_send(key, draft),
            "an image is still being read"
        );
    }

    #[test]
    fn the_chips_problem_scene_cannot_send_and_its_neighbour_names_its_files() {
        let rt = testkit::window("chips-problem");
        let tiles = testkit::tile_keys(&rt);
        assert_eq!(tiles.len(), 2);
        let key = testkit::focused(&rt).expect("a tile is focused");
        assert_eq!(key, tiles[0]);
        let chips = &rt.state.app.sessions.get(key).unwrap().session.chips;
        assert_eq!(chips.len(), 1);
        assert!(testkit::tile_has_text(&rt, key, "Too large to send"));
        let draft = &glue::view_of(&rt.state, key).draft.text;
        assert!(!draft.is_empty());
        assert!(!rt.state.app.can_send(key, draft));
        let sent = tiles[1];
        let picture = SCENE_CHIPS + 1;
        let shown = testkit::element(
            &rt,
            &testkit::tile(sent, &format!("mr-user-picture-{picture}")),
        )
        .expect("the sent message shows its picture");
        assert_eq!(shown.rect[3], 120.0);
        assert!(rt.state.thumbs[&picture].is_ready());
        assert!(!testkit::tile_has_text(&rt, sent, "1 image"));
        assert!(testkit::tile_has_text(&rt, sent, "plan.md, shapes.txt"));
    }

    #[test]
    fn a_png_made_here_decodes_to_its_pixels() {
        let rgb: Vec<u8> = (0..5 * 3)
            .flat_map(|n| [n as u8, 100, 255 - n as u8])
            .collect();
        let decoded = image::load_from_memory(&png(5, 3, &rgb)).expect("the PNG decodes");
        assert_eq!((decoded.width(), decoded.height()), (5, 3));
        assert_eq!(decoded.to_rgb8().into_raw(), rgb);
    }

    #[test]
    fn a_png_larger_than_one_stored_block_decodes() {
        let (width, height) = (300, 200);
        let rgb: Vec<u8> = (0..width * height * 3).map(|n| (n % 251) as u8).collect();
        let decoded = image::load_from_memory(&png(width, height, &rgb)).expect("the PNG decodes");
        assert_eq!(decoded.to_rgb8().into_raw(), rgb);
    }

    #[test]
    fn the_quit_ask_scene_asks_over_a_working_tile_and_a_remote_one() {
        let rt = testkit::window("quit-ask");
        assert_eq!(testkit::tile_keys(&rt).len(), 2);
        assert!(rt.state.quit_ask);
        assert_eq!(
            rt.state.app.busy(),
            mr_core::Busy {
                working: 1,
                remote: 1
            }
        );
        assert!(testkit::has_text(
            &rt,
            "1 session is working and 1 has Remote Control on."
        ));
        assert!(testkit::find(&rt, "mr-quit-yes").is_some());
        // No input box is drawn with the keyboard under the question.
        let working = testkit::focused(&rt).expect("a focused tile");
        for key in testkit::tile_keys(&rt) {
            assert!(!glue::view_of(&rt.state, key).draft.focused, "{key:?}");
        }
        // Cancel leaves the focus on the working tile, and the keyboard in no other tile's box.
        let mut rt = rt;
        testkit::click(&mut rt, "mr-quit-cancel");
        testkit::step(&mut rt);
        assert!(!rt.state.quit_ask);
        assert_eq!(testkit::focused(&rt), Some(working));
        let typing = glue::typing_in(&rt.state);
        assert!(typing.is_none_or(|key| key == working), "{typing:?}");
    }

    #[test]
    fn the_viewer_scene_shows_a_sent_picture_in_full() {
        let rt = testkit::window("viewer");
        let viewing = rt.state.viewing.as_ref().expect("a picture is open");
        assert!(!viewing.failed);
        let full = viewing.full.as_ref().expect("its full image is at hand");
        assert_eq!(full.size(), Some([VIEWER_SIZE.0, VIEWER_SIZE.1]));
        assert!(rt.state.thumbs[&viewing.picture].is_ready());
        assert!(testkit::find(&rt, "mr-viewer-image").is_some());
        assert!(testkit::find(&rt, "mr-viewer-error").is_none());
    }

    #[test]
    fn the_viewer_failed_scene_shows_the_small_picture_and_says_why() {
        let rt = testkit::window("viewer-failed");
        let viewing = rt.state.viewing.as_ref().expect("a picture is open");
        assert!(viewing.failed);
        assert!(viewing.full.is_none());
        assert!(rt.state.thumbs[&viewing.picture].is_ready());
        let session = &rt.state.app.sessions.get(viewing.key).unwrap().session;
        assert_eq!(session.phase, Phase::Resting);
        assert!(testkit::has_text(&rt, "The full picture cannot be read."));
    }

    /// The texts of the sidebar's list that are drawn, top to bottom.
    fn list_texts(rt: &rune::Runtime<Ui>) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains("/mr-sidebar-rows/"))
            .filter_map(|e| e.text)
            .collect()
    }

    #[test]
    fn the_colours_scene_has_a_ready_a_waiting_and_a_working_tile() {
        let rt = testkit::window_sized("colours", (1800.0, 900.0));
        let tiles = testkit::tile_keys(&rt);
        let marks: Vec<Mark> = tiles
            .iter()
            .map(|key| rt.state.app.sessions.get(*key).unwrap().mark())
            .collect();
        assert_eq!(marks, [Mark::Finished, Mark::NeedsYou, Mark::Working]);
        assert_eq!(testkit::focused(&rt), Some(tiles[2]));
        let recipes = GroupId::Folder(RECIPES.into());
        let section = rt
            .state
            .app
            .sidebar
            .sections
            .iter()
            .find(|s| s.id == recipes);
        assert!(section.is_some_and(|s| s.collapsed));
        // Its header shows the green dot of the ready session in it.
        let header = format!("/mr-group-{}/", recipes.key_text());
        let dot = rt
            .elements()
            .into_iter()
            .find(|e| !e.exiting && e.path.contains(&header) && e.path.contains("/mr-mark/"))
            .map(|e| e.fill);
        assert_eq!(dot, Some(crate::theme::ready().fill));
        let sorted = key_of_title(&rt.state, "Sorting the soup recipes");
        assert!(!rt.state.app.tiles.contains(sorted));
        assert_eq!(
            rt.state.app.sessions.get(sorted).unwrap().mark(),
            Mark::Finished
        );
    }

    #[test]
    fn the_deleted_scene_lists_two_groups_with_their_days_and_a_failure() {
        let rt = testkit::window("deleted");
        assert_eq!(rt.state.app.sidebar_view(), SidebarView::Deleted);
        assert_eq!(rt.state.app.sidebar.deleted, 3);
        assert!(testkit::has_text(&rt, "Deleted (3)"));
        assert!(testkit::has_text(&rt, "Removed for good after 7 days."));
        assert_eq!(
            list_texts(&rt),
            [
                "website",
                "Resizing the banner",
                "6 days left",
                "Choosing the fonts",
                "Not removed",
                "Side projects",
                "Planning the spring beds",
                "4 days left",
            ]
        );
        // The failure came by the app's own path: the fake was asked once and refused.
        let fake = rt.state.fake.as_ref().unwrap();
        assert_eq!(fake.removed(), [SessionId("stored-site-fonts".into())]);
        // A tile still shows a session that is not deleted.
        assert_eq!(testkit::tile_keys(&rt).len(), 1);
    }

    #[test]
    fn the_deleted_empty_scene_says_nothing_is_deleted() {
        let rt = testkit::window("deleted-empty");
        assert_eq!(rt.state.app.sidebar_view(), SidebarView::Deleted);
        assert!(testkit::has_text(&rt, "Nothing is deleted."));
        // "Delete All" is there and cannot be pressed: a disabled button takes no focus.
        let delete_all = rt
            .elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some("mr-delete-all"))
            .expect("Delete All is drawn");
        assert!(!delete_all.focusable);
    }

    #[test]
    fn the_delete_group_ask_scene_asks_over_the_list() {
        let rt = testkit::window("delete-group-ask");
        assert_eq!(rt.state.app.sidebar_view(), SidebarView::Sessions);
        assert_eq!(
            rt.state.delete_ask,
            Some(glue::DeleteAsk::Group(GroupId::Folder(RUNE_CLAUDE.into())))
        );
        assert!(testkit::has_text(
            &rt,
            "3 sessions go with it. 2 of them are still working and will be stopped. They can be \
             recovered for 7 days."
        ));
        assert!(testkit::find(&rt, "mr-delete-yes").is_some());
        assert!(glue::typing_in(&rt.state).is_none());
    }

    /// Runs frames until a task list or a choice box, drawn from the second frame of the app's
    /// life, has slid and faded in, as a screenshot of the scene takes it.
    fn settled(scene: &str) -> rune::Runtime<Ui> {
        let mut rt = testkit::window(scene);
        for _ in 0..40 {
            testkit::step(&mut rt);
        }
        rt
    }

    /// Whether the element keyed `key` in `tile`'s tile is drawn at rest, fully shown.
    fn at_rest(rt: &rune::Runtime<Ui>, tile: SessionKey, key: &str) -> bool {
        testkit::element(rt, &testkit::tile(tile, key))
            .is_some_and(|e| !e.exiting && e.opacity == 1.0)
    }

    /// The status of each step of `key`'s task list, in order.
    fn statuses(rt: &rune::Runtime<Ui>, key: SessionKey) -> Vec<TaskStatus> {
        let tasks = &rt.state.app.sessions.get(key).unwrap().session.tasks;
        tasks.steps().iter().map(|step| step.status).collect()
    }

    #[test]
    fn the_tasks_scene_is_at_its_third_step_of_five_in_a_working_turn() {
        let rt = settled("tasks");
        let key = testkit::focused(&rt).expect("a tile is focused");
        let entry = rt.state.app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, Phase::Working);
        assert_eq!(entry.mark(), Mark::Working);
        use TaskStatus::{Completed, InProgress, Pending};
        assert_eq!(
            statuses(&rt, key),
            [Completed, Completed, InProgress, Pending, Pending]
        );
        assert_eq!(entry.session.tasks.current(), Some(2));
        assert!(rt.state.app.choices(key).is_none());
        // Open in a tile of the whole window: the head, five steps, and no line for more.
        assert_eq!(glue::view_of(&rt.state, key).tasks_open, None);
        assert!(at_rest(&rt, key, &format!("mr-tasks-{}", key.0)));
        for i in 0..5 {
            assert!(at_rest(&rt, key, &format!("mr-task-{i}")), "step {i}");
        }
        assert!(testkit::element(&rt, &testkit::tile(key, "mr-tasks-more")).is_none());
    }

    #[test]
    fn the_tasks_folded_scene_folds_the_list_of_its_upper_tile() {
        let rt = settled("tasks-folded");
        let columns = testkit::columns(&rt);
        assert_eq!(columns.len(), 1);
        let [upper, lower] = columns[0][..] else {
            panic!("one column of two: {columns:?}");
        };
        assert_eq!(testkit::focused(&rt), Some(upper));
        let entry = |key: SessionKey| rt.state.app.sessions.get(key).unwrap();
        assert_eq!(entry(upper).session.phase, Phase::Working);
        assert_eq!(entry(upper).mark(), Mark::Working);
        assert_eq!(entry(upper).session.tasks.current(), Some(2));
        assert_eq!(statuses(&rt, upper).len(), 5);
        assert_eq!(entry(lower).session.phase, Phase::Idle);
        assert_eq!(entry(lower).mark(), Mark::None);
        assert!(entry(lower).session.tasks.is_empty());
        assert!(entry(lower).session.conversation.items().is_empty());
        // Folded by the tile's height, not by a press: the head line alone, at rest.
        assert_eq!(glue::view_of(&rt.state, upper).tasks_open, None);
        assert!(at_rest(&rt, upper, &format!("mr-tasks-{}", upper.0)));
        assert!(at_rest(&rt, upper, "mr-tasks-head"));
        assert!(testkit::element(&rt, &testkit::tile(upper, "mr-task-0")).is_none());
        assert!(testkit::tile_has_text(
            &rt,
            upper,
            "3 of 5: Updating the stylesheet"
        ));
        assert!(testkit::element(&rt, &testkit::tile(lower, "mr-tasks-head")).is_none());
    }

    #[test]
    fn the_choices_scene_ends_its_turn_with_an_offer_of_three() {
        let rt = settled("choices");
        let key = testkit::focused(&rt).expect("a tile is focused");
        let entry = rt.state.app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, Phase::Idle);
        // The turn ended in the tile the user looks at: nothing marks it.
        assert_eq!(entry.mark(), Mark::None);
        assert!(entry.session.tasks.is_empty());
        assert_eq!(entry.session.conversation.waiting(), 0);
        let offer = rt.state.app.choices(key).expect("the offer shows");
        assert_eq!(offer.question, CHOICES_QUESTION);
        let labels: Vec<&str> = offer.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, CHOICES.map(|(label, _)| label));
        assert!(rt.state.app.can_choose(key));
        assert!(at_rest(&rt, key, &format!("mr-choices-{}", key.0)));
        for i in 0..3 {
            assert!(at_rest(&rt, key, &format!("mr-choice-{i}")), "button {i}");
        }
    }

    #[test]
    fn the_delete_all_ask_scene_asks_over_the_deleted_view() {
        let rt = testkit::window("delete-all-ask");
        assert_eq!(rt.state.app.sidebar_view(), SidebarView::Deleted);
        assert_eq!(rt.state.delete_ask, Some(glue::DeleteAsk::All));
        assert!(testkit::has_text(
            &rt,
            "3 sessions will be removed from this computer. This cannot be undone."
        ));
        assert!(testkit::has_text(&rt, "Delete All"));
    }
}
