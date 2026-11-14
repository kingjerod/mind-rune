//! The real path without the real program: what `claude` sends, as the adapter's translator
//! turns it into events, taken in by the same `Session` the window draws. Nothing here starts
//! a process.

use mr_agent::{Answer, QuestionAnswer, RequestId, SessionEvent, UserRequest, UserRequestKind};
use mr_claude::{Incoming, PermissionReply, Translator, parse_line};
use mr_core::{ItemBody, Phase, Session, answer_words};
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

/// Plays one recording through the translator into a session, answering every request the
/// recording does not withdraw, and checks the conversation at the end of every turn. Returns
/// how many turns ended and how many requests were raised.
fn replay(path: &std::path::Path) -> (usize, usize) {
    let name = path.file_name().unwrap().to_string_lossy();
    let text = std::fs::read_to_string(path).unwrap();
    let mut t = Translator::new();
    let events: Vec<SessionEvent> = text.lines().flat_map(|l| t.push(parse_line(l))).collect();
    let mut session = Session::opening("/home/alex/project");
    let (mut turns, mut requests) = (0, 0);
    for (i, event) in events.iter().enumerate() {
        let turn_end = matches!(
            event,
            SessionEvent::TurnFinished(_) | SessionEvent::TurnFailed { .. }
        );
        session.apply(event.clone(), i as f64);
        if let SessionEvent::NeedsUser(request) = event {
            requests += 1;
            let withdrawn = events[i + 1..].iter().any(
                |later| matches!(later, SessionEvent::RequestWithdrawn { id } if *id == request.id),
            );
            if !withdrawn {
                let answered = session.answer(&request.id, some_answer(request));
                assert!(
                    answered.is_some(),
                    "{name}: event {i}: {request:?} can be answered"
                );
            }
        }
        if !turn_end {
            continue;
        }
        turns += 1;
        let at = format!("{name}: the turn ending at event {i}");
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
    (turns, requests)
}

#[test]
fn every_recording_ends_each_turn_settled_with_requests_drawn_once() {
    let files = recordings();
    assert!(files.len() >= 6, "{files:?}");
    let mut requests = 0;
    for path in &files {
        let (turns, raised) = replay(path);
        assert!(turns > 0, "{}: has turns", path.display());
        requests += raised;
    }
    assert!(
        requests >= 3,
        "the recordings ask permission, a question and a plan"
    );
}
