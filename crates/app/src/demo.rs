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
    Account, FakeAgent, FakeStep, LimitWindow, Question, QuestionOption, RequestId, SessionEvent,
    ToolCallId, ToolDetail, ToolOutcome, TurnSummary, UsageLimits, UserRequest, UserRequestKind,
};
use mr_core::{ItemBody, Phase, Screen};
use rune::prefs::Prefs;

use crate::glue::{self, Ui};

/// The folder the demo's session is opened in.
const FOLDER: &str = "/home/alex/project";

/// The demo's fixed wall clock, in Unix seconds.
const NOW_UNIX: i64 = 1_800_000_000;

/// The most pumps a scene waits for a state before it gives up.
const MAX_PUMPS: usize = 50;

/// A window state over a `FakeAgent` signed in as alex on the Max plan, with settings kept in memory.
pub fn ui() -> Ui {
    let fake = FakeAgent::new();
    fake.set_account(Account::SignedIn {
        name: "alex".into(),
        plan: Some("Max".into()),
        full_login: true,
    });
    let mut ui = Ui::new(
        Arc::new(fake.clone()),
        Some(fake),
        FOLDER.into(),
        Prefs::memory(),
    );
    ui.unix_clock = || NOW_UNIX;
    ui.now_unix = NOW_UNIX;
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
    let effects = ui.app.submit(message);
    glue::run(ui, effects);
}

/// Emits `event` to the current session.
fn emit(ui: &Ui, event: SessionEvent) {
    let id = ui
        .app
        .session
        .as_ref()
        .and_then(|s| s.id.clone())
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
        ui.app
            .session
            .as_ref()
            .is_some_and(|s| s.phase == Phase::Idle)
    });
    usage(ui, 0.37, 0.10);
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
        ui.app
            .session
            .as_ref()
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
        ui.app.session.as_ref().is_some_and(|s| {
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
        .app
        .session
        .as_ref()
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
        ui.app.session.as_ref().is_some_and(|s| {
            matches!(
                s.conversation.items().last().map(|item| &item.body),
                Some(ItemBody::Thinking { seconds: None, .. })
            )
        })
    });
    pump(ui, t);
    pump_until(ui, "the finished turn", |ui| {
        ui.app
            .session
            .as_ref()
            .is_some_and(|s| s.phase == Phase::Idle)
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
        ui.app.session.as_ref().is_some_and(|s| {
            s.phase == Phase::Working
                && matches!(
                    s.conversation.items().last().map(|item| &item.body),
                    Some(ItemBody::Thinking { seconds: None, .. })
                )
        })
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
        ui.app
            .session
            .as_ref()
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
        ui.app
            .session
            .as_ref()
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
        ui.app
            .session
            .as_ref()
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
    let now = ui.now_unix;
    emit(
        ui,
        SessionEvent::UsageLimits(UsageLimits {
            session: Some(LimitWindow {
                used: session,
                resets_at_unix: now + 2 * 3600 + 14 * 60,
            }),
            weekly: Some(LimitWindow {
                used: weekly,
                resets_at_unix: now + 3 * 86_400 + 4 * 3600,
            }),
        }),
    );
    pump(ui, 0.0);
}
