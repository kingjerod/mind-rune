//! The real path without the real program: what `claude` sends, as the adapter's translator
//! turns it into events, taken in by the same `App` the window holds, one session or several
//! at once. Nothing here starts a process.

use mr_agent::{
    Account, Answer, Capabilities, QuestionAnswer, RequestId, SessionCommand, SessionEvent,
    TaskStatus, UserRequest, UserRequestKind,
};
use mr_claude::{Incoming, PermissionReply, Translator, parse_line};
use mr_core::{
    App, DEFAULT_WIDTH, Effect, FromBackend, ItemBody, Launch, Leaving, Phase, Place, Screen,
    Session, SessionKey, Width, answer_words,
};
use serde_json::json;

/// A question as `claude` asks it: one colour, and any number of sizes.
fn question_request() -> Incoming {
    let input = json!({"questions":[
        {"question":"Which colour?","options":[{"label":"Red"},{"label":"Blue"}],"multiSelect":false},
        {"question":"Which sizes?","options":[{"label":"S"},{"label":"M"}],"multiSelect":true}]});
    Incoming::ControlRequest {
        request_id: "req_q".into(),
        subtype: "can_use_tool".into(),
        body: json!({"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":input,
                     "tool_use_id":"t_q"}),
    }
}

#[test]
fn a_settled_answer_reads_as_what_claude_is_sent() {
    let mut t = Translator::new();
    let events = t.push(question_request());
    assert!(
        matches!(
            &events[..],
            [SessionEvent::NeedsUser(r)] if matches!(r.kind, UserRequestKind::Question { .. })
        ),
        "{events:?}"
    );
    let answer = Answer::Answers(vec![
        QuestionAnswer {
            prompt: "Which colour?".into(),
            chosen: vec!["Red".into()],
            free_text: Some("but darker".into()),
        },
        QuestionAnswer {
            prompt: "Which sizes?".into(),
            chosen: vec!["S".into(), "M".into()],
            free_text: Some("XL".into()),
        },
    ]);
    let reply = t
        .answer(&RequestId("req_q".into()), answer.clone())
        .unwrap();
    let PermissionReply::Allow { updated_input, .. } = reply else {
        panic!("a question is answered by allowing it: {reply:?}");
    };
    let sent: Vec<&str> = ["Which colour?", "Which sizes?"]
        .iter()
        .map(|prompt| updated_input["answers"][prompt].as_str().unwrap())
        .collect();
    assert_eq!(sent, ["Red, but darker", "S, M, XL"]);
    // The window's settled line is every question's answer, in order, as sent.
    assert_eq!(answer_words(&answer), sent.join(", "));
    assert_eq!(updated_input["questions"].as_array().map(Vec::len), Some(2));
}

/// The tools that are requests to the user, not work: `claude` asks with them, and the window
/// draws them as cards only.
const REQUEST_TOOLS: [&str; 2] = ["AskUserQuestion", "ExitPlanMode"];

/// The recordings of real `claude` sessions the adapter's own tests replay. Never edited.
fn recordings() -> Vec<std::path::PathBuf> {
    let folder =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../claude/tests/fixtures/live");
    let mut files: Vec<_> = std::fs::read_dir(&folder)
        .unwrap_or_else(|e| panic!("{}: {e}", folder.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    files.sort();
    files
}

/// What the user answers to `request` in a replay: allow, an empty answer, approve.
fn some_answer(request: &UserRequest) -> Answer {
    match request.kind {
        UserRequestKind::Permission { .. } => Answer::Allow { always: false },
        UserRequestKind::Question { .. } => Answer::Answers(vec![]),
        UserRequestKind::PlanApproval { .. } => Answer::ApprovePlan,
    }
}

/// The wall clock the replays' apps run at.
const NOW: i64 = 1_800_000_000;

/// The message each replayed session is given before its program has started. It waits in the
/// session until the recording's `Started`, so the app keeps the session when another is shown.
const FIRST_MESSAGE: &str = "Carry on.";

/// One recording, as the translator turns each of its lines into events.
struct Recording {
    name: String,
    events: Vec<SessionEvent>,
    /// Where each line's events end in `events`.
    line_ends: Vec<usize>,
}

impl Recording {
    fn read(path: &std::path::Path) -> Recording {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(path).unwrap();
        let mut t = Translator::new();
        let (mut events, mut line_ends) = (Vec::new(), Vec::new());
        for line in text.lines() {
            events.extend(t.push(parse_line(line)));
            line_ends.push(events.len());
        }
        Recording {
            name,
            events,
            line_ends,
        }
    }

    fn lines(&self) -> usize {
        self.line_ends.len()
    }

    /// The folder its session is opened in.
    fn folder(&self) -> String {
        let stem = self.name.trim_end_matches(".jsonl");
        format!("/home/alex/{stem}")
    }
}

/// An app signed in as `alex`, with no session yet.
fn signed_in_app() -> App {
    let caps = Capabilities {
        remote_access: true,
        plan_mode: true,
        usage_limits: true,
        images: true,
    };
    let (mut app, _) = App::new(caps, Width::Points(DEFAULT_WIDTH), Launch::default(), NOW);
    app.account_checked(Account::SignedIn {
        name: "alex@example.org".into(),
        plan: None,
        full_login: true,
    });
    assert_eq!(app.screen, Screen::Main);
    app
}

/// One recording played into one session of an app: the session, the number of its open,
/// and how far the recording has come.
struct Player<'r> {
    recording: &'r Recording,
    key: SessionKey,
    open: u64,
    /// Events taken in so far, and the stamp of the next.
    played: usize,
    turns: usize,
    requests: usize,
    /// Every command the app wanted sent to this session's program, in order.
    sent: Vec<SessionCommand>,
}

impl<'r> Player<'r> {
    /// A new session in `app` at `place`, as "New session" makes one, with [`FIRST_MESSAGE`]
    /// waiting in it.
    fn new(app: &mut App, recording: &'r Recording, place: Place) -> Player<'r> {
        let effects = app.new_session(recording.folder().into(), place, Leaving::ForgetIfEmpty);
        let open = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Open { open, .. } => Some(*open),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{}: no program was opened: {effects:?}", recording.name));
        let key = app.focused().expect("the new session is shown");
        let effects = app.submit(key, FIRST_MESSAGE);
        assert!(
            effects.is_empty(),
            "{}: nothing is sent before the program starts: {effects:?}",
            recording.name
        );
        Player {
            recording,
            key,
            open,
            played: 0,
            turns: 0,
            requests: 0,
            sent: Vec::new(),
        }
    }

    /// Takes in the events of the recording's line `line`, if it has one, answering every
    /// request the recording does not withdraw, and checks the conversation at the end of
    /// every turn.
    fn play_line(&mut self, app: &mut App, line: usize) {
        let Some(&end) = self.recording.line_ends.get(line) else {
            return;
        };
        while self.played < end {
            let i = self.played;
            let event = self.recording.events[i].clone();
            self.played += 1;
            let turn_end = matches!(
                event,
                SessionEvent::TurnFinished(_) | SessionEvent::TurnFailed { .. }
            );
            let request = match &event {
                SessionEvent::NeedsUser(request) => Some(request.clone()),
                _ => None,
            };
            let message = FromBackend::Event {
                open: self.open,
                event,
            };
            let effects = app.receive(message, i as f64);
            self.take_sent(effects);
            if let Some(request) = request {
                self.answer(app, i, &request);
            }
            if turn_end {
                self.turns += 1;
                self.check_settled(app, i);
            }
        }
    }

    fn answer(&mut self, app: &mut App, i: usize, request: &UserRequest) {
        self.requests += 1;
        let withdrawn = self.recording.events[i + 1..].iter().any(
            |later| matches!(later, SessionEvent::RequestWithdrawn { id } if *id == request.id),
        );
        if withdrawn {
            return;
        }
        let effects = app.answer(self.key, &request.id, some_answer(request));
        let answered = effects.iter().any(|effect| {
            matches!(
                effect,
                Effect::Send { open, command: SessionCommand::Answer { .. } } if *open == self.open
            )
        });
        assert!(
            answered,
            "{}: event {i}: {request:?} can be answered: {effects:?}",
            self.recording.name
        );
        self.take_sent(effects);
    }

    /// Keeps the commands among `effects` that go to this session's program.
    fn take_sent(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            if let Effect::Send { open, command } = effect
                && open == self.open
            {
                self.sent.push(command);
            }
        }
    }

    fn session<'a>(&self, app: &'a App) -> &'a Session {
        let entry = app.sessions.get(self.key);
        &entry
            .unwrap_or_else(|| panic!("{}: the app forgot the session", self.recording.name))
            .session
    }

    /// At the end of a turn: idle, nothing waiting, no tool still running, and no request
    /// drawn as a tool line too.
    fn check_settled(&self, app: &App, i: usize) {
        let session = self.session(app);
        let at = format!("{}: the turn ending at event {i}", self.recording.name);
        assert_eq!(session.phase, Phase::Idle, "{at}");
        assert_eq!(session.conversation.waiting(), 0, "{at}");
        for item in session.conversation.items() {
            let ItemBody::Tools(group) = &item.body else {
                continue;
            };
            assert!(!group.running(), "{at}: {group:?} still runs");
            for call in &group.calls {
                assert!(
                    !REQUEST_TOOLS.contains(&call.name.as_str()),
                    "{at}: a request drawn as a tool line too: {}",
                    group.summary()
                );
            }
        }
    }
}

/// How one recording ends when played into a session: the session, and every command the app
/// wanted sent to its program.
#[derive(Debug, PartialEq)]
struct Ending {
    session: Session,
    sent: Vec<SessionCommand>,
}

/// Plays one recording into the only session of an app. Returns how it ends, how many turns
/// ended and how many requests were raised.
fn replay_alone(recording: &Recording) -> (Ending, usize, usize) {
    let mut app = signed_in_app();
    let mut player = Player::new(&mut app, recording, Place::Focused);
    for line in 0..recording.lines() {
        player.play_line(&mut app, line);
    }
    let ending = Ending {
        session: player.session(&app).clone(),
        sent: player.sent.clone(),
    };
    (ending, player.turns, player.requests)
}

/// Plays `recordings` into as many sessions of one app at once: line by line, one line of each
/// in turn. Each session after the first is shown at `place`. Returns the app and how each
/// session ends, in the same order.
fn replay_together(recordings: &[&Recording], place: Place) -> (App, Vec<Ending>) {
    let mut app = signed_in_app();
    let mut players: Vec<Player> = Vec::new();
    for (i, recording) in recordings.iter().enumerate() {
        let place = if i == 0 { Place::Focused } else { place };
        players.push(Player::new(&mut app, recording, place));
    }
    let lines = recordings.iter().map(|r| r.lines()).max().unwrap_or(0);
    for line in 0..lines {
        for player in &mut players {
            player.play_line(&mut app, line);
        }
    }
    let endings = players
        .iter()
        .map(|player| Ending {
            session: player.session(&app).clone(),
            sent: player.sent.clone(),
        })
        .collect();
    (app, endings)
}

#[test]
fn every_recording_ends_each_turn_settled_with_requests_drawn_once() {
    let files = recordings();
    assert!(files.len() >= 6, "{files:?}");
    let mut requests = 0;
    for path in &files {
        let (_, turns, raised) = replay_alone(&Recording::read(path));
        assert!(turns > 0, "{}: has turns", path.display());
        requests += raised;
    }
    assert!(
        requests >= 3,
        "the recordings ask permission, a question and a plan"
    );
}

#[test]
fn recordings_played_into_three_sessions_at_once_end_as_each_does_alone() {
    let recordings: Vec<Recording> = recordings().iter().map(|p| Recording::read(p)).collect();
    assert!(recordings.len() >= 3, "three recordings at least");
    // Every recording is played beside two others: in threes, the last three wrapping round.
    for first in (0..recordings.len()).step_by(3) {
        let three: Vec<&Recording> = (first..first + 3)
            .map(|i| &recordings[i % recordings.len()])
            .collect();
        let (_, together) = replay_together(&three, Place::Focused);
        for (recording, ending) in three.iter().zip(together) {
            let (alone, _, _) = replay_alone(recording);
            let others: Vec<&str> = three
                .iter()
                .filter(|other| other.name != recording.name)
                .map(|other| other.name.as_str())
                .collect();
            assert!(
                ending == alone,
                "{}: played beside {others:?} it ends otherwise than alone: phase {:?} \
                 against {:?}, {} items against {}, {} commands against {}",
                recording.name,
                ending.session.phase,
                alone.session.phase,
                ending.session.conversation.items().len(),
                alone.session.conversation.items().len(),
                ending.sent.len(),
                alone.sent.len()
            );
        }
    }
}

#[test]
fn recordings_played_in_three_tiles_at_once_each_show_what_they_show_alone() {
    let recordings: Vec<Recording> = recordings().iter().map(|p| Recording::read(p)).collect();
    assert!(recordings.len() >= 3, "three recordings at least");
    // Every recording is played in a tile beside two others: in threes, the last three
    // wrapping round.
    for first in (0..recordings.len()).step_by(3) {
        let three: Vec<&Recording> = (first..first + 3)
            .map(|i| &recordings[i % recordings.len()])
            .collect();
        let (app, together) = replay_together(&three, Place::Side);
        assert_eq!(
            app.tiles.len(),
            3,
            "{}, {} and {} are in three tiles",
            three[0].name,
            three[1].name,
            three[2].name
        );
        for (recording, ending) in three.iter().zip(together) {
            let (alone, _, _) = replay_alone(recording);
            assert_eq!(
                ending.session.conversation.items(),
                alone.session.conversation.items(),
                "{}: its items in a tile beside two others are not those it has alone",
                recording.name
            );
            assert!(
                ending == alone,
                "{}: in a tile beside two others it ends otherwise than alone: phase {:?} \
                 against {:?}, {} commands against {}",
                recording.name,
                ending.session.phase,
                alone.session.phase,
                ending.sent.len(),
                alone.sent.len()
            );
        }
    }
}

/// The recording named `name` among the adapter's fixtures.
fn fixture(name: &str) -> Recording {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../claude/tests/fixtures/live")
        .join(name);
    Recording::read(&path)
}

/// Plays the lines of `player`'s recording after the first `from`, up to and including the
/// 1-based line `through`. Gives `through`: the lines played so far.
fn play_through(player: &mut Player, app: &mut App, from: usize, through: usize) -> usize {
    for line in from..through {
        player.play_line(app, line);
    }
    through
}

#[test]
fn the_recorded_offer_ends_as_two_buttons_and_a_click_sends_its_message() {
    let recording = fixture("v14-choices.jsonl");
    let mut app = signed_in_app();
    let mut player = Player::new(&mut app, &recording, Place::Focused);
    let key = player.key;
    // Line 85 is the first turn's result: the offer was made before it, while the turn ran.
    play_through(&mut player, &mut app, 0, 84);
    assert_eq!(app.choices(key), None, "the turn still runs");
    play_through(&mut player, &mut app, 84, 85);
    let offer = app.choices(key).expect("offered");
    assert_eq!(
        offer
            .choices
            .iter()
            .map(|c| c.label.as_str())
            .collect::<Vec<_>>(),
        ["Keep them", "Delete them"]
    );
    assert!(app.can_choose(key));
    let effects = app.choose(key, 0);
    let sent: Vec<&SessionCommand> = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Send { open, command } if *open == player.open => Some(command),
            _ => None,
        })
        .collect();
    assert_eq!(
        sent,
        [&SessionCommand::Send {
            text: "Keep the three text files.".into(),
            attachments: vec![],
        }]
    );
    assert_eq!(app.choices(key), None);
}

#[test]
fn the_recorded_task_list_fills_ticks_off_and_is_gone_at_the_turns_end() {
    let recording = fixture("v12-tasks.jsonl");
    let mut app = signed_in_app();
    let mut player = Player::new(&mut app, &recording, Place::Focused);
    // Line 73 is the result of the call that set the first step going.
    let played = play_through(&mut player, &mut app, 0, 73);
    let tasks = &player.session(&app).tasks;
    let statuses: Vec<&TaskStatus> = tasks.steps().iter().map(|step| &step.status).collect();
    assert_eq!(
        statuses,
        [
            &TaskStatus::InProgress,
            &TaskStatus::Pending,
            &TaskStatus::Pending
        ]
    );
    assert_eq!(tasks.done(), 0);
    // Line 211 is the first turn's result, with every step ticked.
    play_through(&mut player, &mut app, played, 211);
    assert!(player.session(&app).tasks.is_empty());
    assert_eq!(app.choices(player.key), None, "no offer was made");
}
