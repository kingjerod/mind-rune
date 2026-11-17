//! What a stored session shows that a live one does not: its history on its way, or why it
//! could not be read, over the empty conversation; and the card that asks before the app joins
//! a session another app may have open.
//!
//! The card's buttons are request buttons (`requests::request_button`): no key ever presses
//! one, so only a click answers it.

use mr_core::{HistoryState, Prompt, PromptChoice, Session, SessionKey};
use rune::{Align, El, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit};

use crate::glue::{self, Ui};
use crate::view::{input, requests};

/// What the conversation area says while the history is read.
const LOADING: &str = "Loading the conversation…";

/// What it says when the history could not be read, above the reason.
const FAILED: &str = "This conversation could not be read.";

/// The card's title and what it explains.
const ELSEWHERE_TITLE: &str = "This session may be open in another app";
const ELSEWHERE_BODY: &str =
    "Its history changed less than two minutes ago. Continuing here adds to the same session.";

/// The space round the card, matching the rows' sides and the input box's.
const CARD_MARGIN_X: f32 = 24.0;
const CARD_MARGIN_BOTTOM: f32 = 12.0;

/// The space inside the card, all round, as a request card's.
const CARD_PAD: f32 = 16.0;

/// The width of the card's border, as a waiting request's.
const CARD_BORDER: f32 = 1.5;

/// The space either side of the words in the middle of the conversation area.
const MIDDLE_PAD_X: f32 = 24.0;

/// The widest the words in the middle of the conversation area run before they wrap.
const MIDDLE_MAX_WIDTH: f32 = 480.0;

/// Over the empty conversation of `session`, in its middle: that its history is on its way
/// (keyed `mr-history-loading`), or that it could not be read, with the reason under it in
/// muted text (keyed `mr-history-failed`). `None` once the history is in, or when there is none.
pub fn history(session: &Session, kit: &Kit) -> Option<El> {
    let words = match &session.history {
        HistoryState::Loaded => return None,
        HistoryState::NotLoaded | HistoryState::Loading => text(LOADING)
            .key("mr-history-loading")
            .font(kit.ty.body)
            .color(kit.c.text_muted)
            .truncate(),
        HistoryState::Failed(reason) => col()
            .key("mr-history-failed")
            .max_w(MIDDLE_MAX_WIDTH)
            .gap(4.0)
            .items_center()
            .child(middle_line(FAILED, kit.c.text_base, kit))
            .child(middle_line(reason, kit.c.text_muted, kit).font(kit.ty.ui)),
    };
    Some(
        row()
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .center()
            .px(MIDDLE_PAD_X)
            .child(words),
    )
}

/// One line of words in the middle of the conversation area, wrapped and centred.
fn middle_line(words: &str, ink: rune::Color, kit: &Kit) -> El {
    text(words.to_owned())
        .font(kit.ty.body)
        .color(ink)
        .wrap()
        .align(Align::Center)
        .selectable(true)
}

/// The card that asks what to do with the message sent to `key`'s session while another app may
/// have it open, keyed `mr-elsewhere`, while that question waits about the session: its title,
/// what continuing here means, and "Continue here", "Continue as a copy" and "Cancel". It sits
/// between the conversation and the input box, `width` points wide and centred as they are (the
/// whole width while the tile's width is not known yet).
pub fn elsewhere(ui: &Ui, key: SessionKey, kit: &Kit, width: Option<f32>) -> Option<El> {
    if !asks_about(ui, key) {
        return None;
    }
    let buttons = [
        choice(
            key,
            "Continue here",
            "mr-elsewhere-here",
            ButtonVariant::Contrast,
            PromptChoice::ContinueHere,
            kit,
        ),
        choice(
            key,
            "Continue as a copy",
            "mr-elsewhere-copy",
            ButtonVariant::Neutral,
            PromptChoice::ContinueAsCopy,
            kit,
        ),
        choice(
            key,
            "Cancel",
            "mr-elsewhere-cancel",
            ButtonVariant::Ghost,
            PromptChoice::Cancel,
            kit,
        ),
    ];
    let card = col()
        .key("mr-elsewhere")
        .w_full()
        .padding(CARD_PAD)
        .gap(12.0)
        .radius(kit.radius.lg)
        .fill(kit.c.bg_base)
        .border(CARD_BORDER, kit.c.warning.icon)
        .child(
            col()
                .w_full()
                .gap(4.0)
                .child(
                    text(ELSEWHERE_TITLE)
                        .key("mr-elsewhere-title")
                        .font(kit.ty.body_medium)
                        .color(kit.c.text_base)
                        .wrap()
                        .w_full(),
                )
                .child(
                    text(ELSEWHERE_BODY)
                        .key("mr-elsewhere-body")
                        .font(kit.ty.ui)
                        .color(kit.c.text_muted)
                        .wrap()
                        .w_full(),
                ),
        )
        .child(
            row()
                .key("mr-elsewhere-buttons")
                .w_full()
                .flex_wrap()
                .gap(8.0)
                .children(buttons),
        );
    let area = col();
    let area = match width {
        Some(width) => area.w(width).self_center(),
        None => area.w_full(),
    };
    Some(area.px(CARD_MARGIN_X).pb(CARD_MARGIN_BOTTOM).child(card))
}

/// Whether a question waits about `key`'s session.
fn asks_about(ui: &Ui, key: SessionKey) -> bool {
    matches!(ui.app.prompt(key), Some(Prompt::OpenElsewhere { .. }))
}

/// One of the card's buttons, keyed `button_key`: a click answers the question with `answer`.
fn choice(
    key: SessionKey,
    label: &str,
    button_key: &str,
    variant: ButtonVariant,
    answer: PromptChoice,
    kit: &Kit,
) -> El {
    let button = Button::new(label)
        .variant(variant)
        .on_press(move |ui: &mut Ui| choose(ui, key, answer));
    requests::request_button(button, button_key.to_owned(), kit)
}

/// Answers the question about `key`'s session with `answer`. Cancel puts the message back into
/// that session's input box. A press that comes when no question waits about it answers
/// nothing.
fn choose(ui: &mut Ui, key: SessionKey, answer: PromptChoice) {
    let (effects, back) = ui.app.answer_prompt(key, answer);
    glue::run(ui, effects);
    if let Some(message) = back {
        input::give_back(ui, key, &message);
    }
}

#[cfg(test)]
mod tests {
    use mr_agent::{
        FakeAgent, FakeStep, HistoryItem, PermissionMode, SessionCommand, SessionEvent, SessionId,
        StoredHistory, StoredSession,
    };
    use mr_core::{HistoryState, ItemBody, ItemId, Phase, Place, Session, SessionKey};
    use rune::{Key, Runtime};

    use crate::demo::{
        DAY, ELSEWHERE_MESSAGE, HISTORY_FAILURE, NOW_UNIX, PARSER_ID, PARSER_QUESTION, PARSER_REPLY,
    };
    use crate::glue::{self, Ui};
    use crate::testkit;
    use crate::theme;
    use crate::view::conversation;

    /// The card's buttons, by key.
    const CARD_BUTTONS: [&str; 3] = [
        "mr-elsewhere-here",
        "mr-elsewhere-copy",
        "mr-elsewhere-cancel",
    ];

    const CARD_TITLE: &str = "This session may be open in another app";
    const CARD_BODY: &str =
        "Its history changed less than two minutes ago. Continuing here adds to the same session.";

    fn fake(rt: &Runtime<Ui>) -> FakeAgent {
        rt.state.fake.clone().expect("the demo window has its fake")
    }

    /// The key of the parser's stored session, the one the scenes show.
    fn parser(rt: &Runtime<Ui>) -> SessionKey {
        let id = SessionId(PARSER_ID.into());
        rt.state
            .app
            .sessions
            .by_id(&id)
            .expect("the parser is listed")
    }

    fn session(rt: &Runtime<Ui>, key: SessionKey) -> &Session {
        &rt.state
            .app
            .sessions
            .get(key)
            .expect("the session is there")
            .session
    }

    /// No program was opened for any session, and so nothing was sent to one.
    fn nothing_opened(rt: &Runtime<Ui>) -> bool {
        rt.state
            .app
            .sessions
            .iter()
            .all(|entry| entry.open.is_none())
            && fake(rt).sent().is_empty()
    }

    /// The texts of the `Send` commands the fake received, in order.
    fn sends(rt: &Runtime<Ui>) -> Vec<String> {
        testkit::sent(rt)
            .into_iter()
            .filter_map(|c| match c {
                SessionCommand::Send { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    /// The text a picker's trigger, keyed `trigger`, shows.
    fn shown_by(rt: &Runtime<Ui>, trigger: &str) -> Option<String> {
        let inside = format!("/{trigger}/");
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.path.contains(&inside) && e.text.is_some())
            .and_then(|e| e.text)
    }

    /// The element keyed `key`, on screen.
    fn element(rt: &Runtime<Ui>, key: &str) -> rune::ElementInfo {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no element keyed {key:?}"))
    }

    /// The visible element whose text is exactly `text`.
    fn text_element(rt: &Runtime<Ui>, text: &str) -> rune::ElementInfo {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.visible[2] > 0.0 && e.text.as_deref() == Some(text))
            .unwrap_or_else(|| panic!("nothing shows {text:?}"))
    }

    fn centre(rect: [f32; 4]) -> (f32, f32) {
        (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0)
    }

    /// Whether `key`'s element sits in the middle of the conversation area.
    fn in_the_middle(rt: &Runtime<Ui>, key: &str) -> bool {
        let (x, y) = centre(element(rt, key).rect);
        let (fx, fy) = centre(element(rt, "mr-conversation-frame").rect);
        (x - fx).abs() < 1.0 && (y - fy).abs() < 1.0
    }

    /// Whether the input box's field has the keyboard focus.
    fn input_focused(rt: &Runtime<Ui>) -> bool {
        rt.elements()
            .iter()
            .any(|e| e.focused && e.path.contains("/mr-input/"))
    }

    /// The next message sent starts a turn that goes on.
    fn turn_goes_on(rt: &Runtime<Ui>) {
        fake(rt).reply(vec![FakeStep::at(0.0, SessionEvent::TurnStarted)]);
    }

    /// The parser's session is told that another app changed it a moment ago; then `message`
    /// is typed into its box and sent with Enter.
    fn send_while_open_elsewhere(rt: &mut Runtime<Ui>, message: &str) {
        fake(rt).set_last_active(&SessionId(PARSER_ID.into()), Some(NOW_UNIX - 30));
        testkit::type_text(rt, message);
        testkit::press(rt, Key::Enter);
        testkit::step(rt);
    }

    /// A stored session `id` in `/home/alex/notes` whose history is `history`, listed as the
    /// store lists it, not shown yet.
    fn stored(rt: &mut Runtime<Ui>, id: &str, history: StoredHistory) -> SessionKey {
        let id = SessionId(id.into());
        fake(rt).set_history(&id, history);
        let stored = StoredSession {
            id,
            folder: "/home/alex/notes".into(),
            title: "Notes".into(),
            last_active_unix: NOW_UNIX - DAY,
        };
        let key = rt
            .state
            .app
            .sessions
            .add(Session::resting(&stored), stored.last_active_unix);
        glue::run(&mut rt.state, Vec::new());
        key
    }

    #[test]
    fn a_stored_session_shows_its_history() {
        let rt = testkit::window("resting");
        let key = parser(&rt);
        assert_eq!(testkit::focused(&rt), Some(key));
        assert_eq!(session(&rt, key).phase, Phase::Resting);
        assert!(
            testkit::has_text(&rt, PARSER_QUESTION),
            "the user's message"
        );
        assert!(testkit::has_text(&rt, PARSER_REPLY), "the reply");
        assert!(testkit::has_text(&rt, "Read parse.rs"), "the tool line");
        assert!(rt.find("mr-empty-hint").is_none());
        assert!(rt.find("mr-history-loading").is_none());
        assert!(nothing_opened(&rt), "{:?}", fake(&rt).sent());
    }

    #[test]
    fn loading_and_failure_are_said_plainly() {
        let rt = testkit::window("history-loading");
        assert_eq!(
            session(&rt, parser(&rt)).history,
            HistoryState::Loading,
            "the read is still on its way"
        );
        assert_eq!(
            element(&rt, "mr-history-loading").text.as_deref(),
            Some("Loading the conversation…")
        );
        assert!(in_the_middle(&rt, "mr-history-loading"));
        assert!(rt.find("mr-empty-hint").is_none(), "no hint while it loads");

        let rt = testkit::window("history-failed");
        let reason = format!("i/o error: {HISTORY_FAILURE}");
        assert_eq!(
            session(&rt, parser(&rt)).history,
            HistoryState::Failed(reason.clone())
        );
        assert!(rt.find("mr-history-failed").is_some());
        assert!(in_the_middle(&rt, "mr-history-failed"));
        let said = text_element(&rt, "This conversation could not be read.");
        let why = text_element(&rt, &reason);
        assert!(
            why.rect[1] >= said.rect[1] + said.rect[3] - 0.5,
            "the reason under it"
        );
        let kit = theme::kit(true);
        assert_eq!(
            why.text_color,
            Some(kit.c.text_muted),
            "the reason is muted"
        );
        assert!(rt.find("mr-empty-hint").is_none());
        assert!(rt.find("mr-history-loading").is_none());
    }

    #[test]
    fn the_two_notices_are_drawn() {
        let rt = testkit::window("resting");
        let kit = theme::kit(true);
        for words in [
            "Earlier messages are not shown.",
            "Earlier messages were summarised here to save room.",
        ] {
            let notice = text_element(&rt, words);
            assert_eq!(notice.text_color, Some(kit.c.text_muted), "{words}");
            // The row the notice is in: its key is the item's.
            let row = rt
                .elements()
                .into_iter()
                .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("mr-item-")))
                .find(|e| notice.path.starts_with(&e.path))
                .expect("the notice is in its row");
            let (x, _) = centre(notice.rect);
            let (row_x, _) = centre(row.rect);
            assert!((x - row_x).abs() < 1.0, "{words} is centred");
            assert!(
                notice.rect[2] < row.rect[2] - 100.0,
                "{words} is not the row's width"
            );
        }
    }

    #[test]
    fn a_stored_message_keeps_a_slot_for_each_image() {
        let mut rt = testkit::window("resting");
        assert!(testkit::has_text(&rt, "This is what it prints now."));
        assert!(!testkit::texts(&rt).iter().any(|t| t == "1 image"));
        let user_pictures = |rt: &Runtime<Ui>, key: SessionKey| -> Vec<(ItemId, Vec<u64>)> {
            session(rt, key)
                .conversation
                .items()
                .iter()
                .filter_map(|item| match &item.body {
                    ItemBody::User { pictures, .. } => Some((item.id, pictures.clone())),
                    _ => None,
                })
                .collect()
        };
        // The parser's screenshot: the store gave no bytes for it, so its slot waits for good.
        let parser = parser(&rt);
        let (_, shot) = user_pictures(&rt, parser)
            .into_iter()
            .find(|(_, pictures)| !pictures.is_empty())
            .expect("the message with a screenshot");
        assert_eq!(shot.len(), 1);
        let wait = format!("mr-user-picture-wait-{}", shot[0]);
        assert!(testkit::element(&rt, &testkit::tile(parser, &wait)).is_some());
        let history = StoredHistory {
            items: vec![
                HistoryItem::User {
                    text: String::new(),
                    image_count: 3,
                    files: Vec::new(),
                    images: vec![None; 3],
                },
                HistoryItem::AssistantText("Three screenshots of the same error.".into()),
            ],
            ..StoredHistory::default()
        };
        let key = stored(&mut rt, "stored-notes", history);
        testkit::show(&mut rt, key);
        assert!(testkit::has_text(&rt, "Three screenshots"));
        let [(first, pictures)] = &user_pictures(&rt, key)[..] else {
            panic!("one message of the user's");
        };
        assert_eq!(pictures.len(), 3);
        for picture in pictures {
            let wait = format!("mr-user-picture-wait-{picture}");
            assert!(testkit::element(&rt, &testkit::tile(key, &wait)).is_some());
        }
        assert!(!testkit::has_text(&rt, "3 images"));
        assert!(
            rt.find(&format!("mr-user-{}", first.0)).is_none(),
            "a message with no text shows only its pictures"
        );
    }

    #[test]
    fn a_stored_thought_reads_thought_and_opens() {
        let mut rt = testkit::window("resting");
        let thought = rt
            .elements()
            .into_iter()
            .filter(|e| e.label.as_deref() == Some("Thought"))
            .filter_map(|e| e.key)
            .find(|k| k.starts_with("mr-toggle-"))
            .expect("the stored thought is a line that opens");
        assert!(!testkit::has_text(&rt, "nothing after it is looked at"));
        testkit::click(&mut rt, &thought);
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "nothing after it is looked at"));
    }

    #[test]
    fn the_pickers_show_what_the_session_will_start_with() {
        let mut rt = testkit::window("resting");
        assert_eq!(
            shown_by(&rt, &testkit::model_key(&rt, "trigger")).as_deref(),
            Some("Fast")
        );
        assert_eq!(
            shown_by(&rt, &testkit::mode_key(&rt, "trigger")).as_deref(),
            Some("Plan")
        );
        let picker = testkit::mode_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
        testkit::step(&mut rt);
        testkit::click_text(&mut rt, "Accept edits");
        testkit::step(&mut rt);
        assert!(nothing_opened(&rt), "{:?}", fake(&rt).sent());
        assert_eq!(
            shown_by(&rt, &testkit::mode_key(&rt, "trigger")).as_deref(),
            Some("Accept edits")
        );
        let key = parser(&rt);
        assert_eq!(session(&rt, key).mode, PermissionMode::AcceptEdits);
        assert_eq!(session(&rt, key).phase, Phase::Resting);
    }

    #[test]
    fn with_no_models_the_model_picker_shows_the_models_id_and_is_disabled() {
        let mut rt = testkit::window("resting");
        let key = parser(&rt);
        rt.state.app.known_models.clear();
        rt.state
            .app
            .sessions
            .get_mut(key)
            .unwrap()
            .session
            .models
            .clear();
        glue::run(&mut rt.state, Vec::new());
        rt.invalidate();
        testkit::step(&mut rt);
        assert_eq!(
            shown_by(&rt, &testkit::model_key(&rt, "trigger")).as_deref(),
            Some("fake-model-fast")
        );
        let picker = testkit::model_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
        testkit::step(&mut rt);
        assert!(
            rt.find(&testkit::model_key(&rt, "content")).is_none(),
            "disabled"
        );
        rt.state.app.sessions.get_mut(key).unwrap().session.model = None;
        glue::run(&mut rt.state, Vec::new());
        rt.invalidate();
        testkit::step(&mut rt);
        assert_eq!(
            shown_by(&rt, &testkit::model_key(&rt, "trigger")).as_deref(),
            Some("Model")
        );
    }

    #[test]
    fn sending_wakes_the_session() {
        let mut rt = testkit::window("resting");
        let key = parser(&rt);
        let picker = testkit::mode_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
        testkit::step(&mut rt);
        testkit::click_text(&mut rt, "Accept edits");
        testkit::step(&mut rt);
        assert!(
            element(&rt, &testkit::mode_key(&rt, "trigger")).focusable,
            "usable while it rests"
        );
        assert!(
            element(&rt, &testkit::model_key(&rt, "trigger")).focusable,
            "usable while it rests"
        );
        turn_goes_on(&rt);
        testkit::type_text(&mut rt, "Now add a test for it");
        testkit::press(&mut rt, Key::Enter);
        // The store is asked first, then the program is opened.
        for _ in 0..5 {
            if rt.state.app.sessions.get(key).unwrap().open.is_some() {
                break;
            }
            rt.frame(0.016);
        }
        // Opening, the message waits for the program, and the pickers wait with it.
        assert_eq!(session(&rt, key).phase, Phase::Opening);
        assert!(testkit::has_text(&rt, "Queued: Now add a test for it"));
        assert!(
            rt.state.app.sessions.get(key).unwrap().open.is_some(),
            "opened"
        );
        assert!(
            !element(&rt, &testkit::mode_key(&rt, "trigger")).focusable,
            "the mode picker waits"
        );
        assert!(
            !element(&rt, &testkit::model_key(&rt, "trigger")).focusable,
            "the model picker waits"
        );
        assert_eq!(glue::view_of(&rt.state, key).draft.text, "");
        testkit::step(&mut rt);
        testkit::step(&mut rt);
        let woken = session(&rt, key);
        assert_eq!(woken.phase, Phase::Working);
        assert_eq!(
            woken.id,
            Some(SessionId(PARSER_ID.into())),
            "its program resumed the stored id"
        );
        assert_eq!(
            woken.mode,
            PermissionMode::AcceptEdits,
            "the program started in the mode chosen"
        );
        assert_eq!(sends(&rt), vec!["Now add a test for it".to_string()]);
        assert!(
            !testkit::sent(&rt)
                .iter()
                .any(|c| matches!(c, SessionCommand::SetPermissionMode(_))),
            "the mode went with the start, not after it"
        );
        assert!(!testkit::has_text(&rt, "Queued:"));
    }

    #[test]
    fn a_session_open_elsewhere_asks_before_sending() {
        let mut rt = testkit::window("resting");
        let key = parser(&rt);
        send_while_open_elsewhere(&mut rt, "Carry on");
        assert!(rt.find("mr-elsewhere").is_some());
        for words in [
            CARD_TITLE,
            CARD_BODY,
            "Continue here",
            "Continue as a copy",
            "Cancel",
        ] {
            assert!(testkit::texts(&rt).iter().any(|t| t == words), "{words}");
        }
        assert!(nothing_opened(&rt), "{:?}", fake(&rt).sent());
        assert_eq!(session(&rt, key).phase, Phase::Resting);
        assert_eq!(
            glue::view_of(&rt.state, key).draft.text,
            "",
            "the box is empty"
        );
        assert!(!testkit::has_text(&rt, "Queued:"));
    }

    #[test]
    fn the_card_is_drawn_only_with_its_session() {
        let mut rt = testkit::window("elsewhere");
        let key = parser(&rt);
        let other = stored(&mut rt, "stored-notes", StoredHistory::default());
        testkit::show(&mut rt, other);
        assert!(rt.find("mr-elsewhere").is_none());
        assert!(!testkit::has_text(&rt, CARD_TITLE));
        assert!(rt.state.app.prompt(key).is_some(), "still asked");
        testkit::show(&mut rt, key);
        assert!(rt.find("mr-elsewhere").is_some());
    }

    #[test]
    fn another_sessions_box_sends_while_a_question_waits() {
        let mut rt = testkit::window("elsewhere");
        let key = parser(&rt);
        let other = stored(&mut rt, "stored-notes", StoredHistory::default());
        testkit::show(&mut rt, other);
        turn_goes_on(&rt);
        testkit::type_text(&mut rt, "Tidy the notes");
        assert!(
            element(&rt, "mr-send").focusable,
            "Send is usable in the other session"
        );
        testkit::press(&mut rt, Key::Enter);
        for _ in 0..5 {
            if rt.state.app.sessions.get(other).unwrap().open.is_some() {
                break;
            }
            rt.frame(0.016);
        }
        testkit::step(&mut rt);
        testkit::step(&mut rt);
        assert_eq!(sends(&rt), vec!["Tidy the notes".to_string()]);
        assert_eq!(glue::view_of(&rt.state, other).draft.text, "");
        assert_eq!(session(&rt, other).phase, Phase::Working);
        assert!(rt.state.app.prompt(key).is_some(), "the parser still asks");
        assert_eq!(session(&rt, key).phase, Phase::Resting);
        testkit::show(&mut rt, key);
        assert!(rt.find("mr-elsewhere").is_some());
    }

    #[test]
    fn cancel_puts_the_message_back() {
        let mut rt = testkit::window("elsewhere");
        let key = parser(&rt);
        testkit::type_text(&mut rt, "and the tests");
        testkit::click(&mut rt, "mr-elsewhere-cancel");
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.prompt(key), None);
        assert!(rt.find("mr-elsewhere").is_none());
        let draft = &glue::view_of(&rt.state, key).draft;
        assert_eq!(draft.text, format!("and the tests\n{ELSEWHERE_MESSAGE}"));
        assert_eq!(
            draft.range(),
            (draft.text.len(), draft.text.len()),
            "at its end"
        );
        assert!(input_focused(&rt));
        assert!(nothing_opened(&rt));
        assert_eq!(session(&rt, key).phase, Phase::Resting);
        // Sent again, it asks again.
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert!(rt.find("mr-elsewhere").is_some());
    }

    #[test]
    fn continue_here_sends_it() {
        let mut rt = testkit::window("elsewhere");
        let key = parser(&rt);
        turn_goes_on(&rt);
        testkit::click(&mut rt, "mr-elsewhere-here");
        testkit::step(&mut rt);
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.prompt(key), None);
        assert!(rt.find("mr-elsewhere").is_none());
        assert_eq!(sends(&rt), vec![ELSEWHERE_MESSAGE.to_string()]);
        let joined = session(&rt, key);
        assert_eq!(
            joined.id,
            Some(SessionId(PARSER_ID.into())),
            "the same session"
        );
        assert_eq!(joined.phase, Phase::Working);
        assert_eq!(testkit::focused(&rt), Some(key));
    }

    #[test]
    fn continue_as_a_copy_shows_a_new_row_in_the_same_group() {
        let mut rt = testkit::window("resting");
        let original = parser(&rt);
        let (group, effects) = rt.state.app.new_group("Parser work");
        glue::run(&mut rt.state, effects);
        let effects = rt.state.app.move_session(original, &group);
        glue::run(&mut rt.state, effects);
        send_while_open_elsewhere(&mut rt, "Carry on");
        testkit::click(&mut rt, "mr-elsewhere-copy");
        testkit::step(&mut rt);
        testkit::step(&mut rt);
        let copy = testkit::focused(&rt).expect("a session is shown");
        assert_ne!(copy, original, "the copy is shown");
        let section_of = |key: SessionKey| {
            rt.state
                .app
                .sidebar
                .sections
                .iter()
                .find(|section| section.rows.iter().any(|row| row.key == key))
                .map(|section| section.id.clone())
        };
        assert_eq!(section_of(copy), Some(group.clone()));
        assert_eq!(section_of(original), Some(group));
        assert!(rt.find(&format!("mr-row-{}", copy.0)).is_some());
        assert!(rt.find(&format!("mr-row-{}", original.0)).is_some());
        assert_eq!(sends(&rt), vec!["Carry on".to_string()]);
        let forked = session(&rt, copy);
        assert_ne!(forked.id, Some(SessionId(PARSER_ID.into())), "a new id");
        assert!(
            testkit::has_text(&rt, PARSER_QUESTION),
            "with the same history"
        );
        let left = rt.state.app.sessions.get(original).unwrap();
        assert_eq!(left.session.phase, Phase::Resting);
        assert!(left.open.is_none(), "the original rests");
        assert!(rt.find("mr-elsewhere").is_none());
    }

    /// Whether one of the card's buttons has the keyboard focus.
    fn a_card_button_has_focus(rt: &Runtime<Ui>) -> bool {
        rt.elements()
            .iter()
            .any(|e| e.focused && e.key.as_deref().is_some_and(|k| CARD_BUTTONS.contains(&k)))
    }

    /// Whether the walk leaves the focused control unpressed, as the request cards' walk does:
    /// Space on "New session" opens the folder menu, and Enter on a picker's trigger opens a
    /// list that keeps the walk going round it.
    fn leave_unpressed(rt: &Runtime<Ui>) -> bool {
        let focused = rt
            .elements()
            .into_iter()
            .find(|e| e.focused)
            .and_then(|e| e.key);
        let controls = [
            "mr-new-session".to_string(),
            testkit::model_key(rt, "trigger"),
            testkit::mode_key(rt, "trigger"),
        ];
        focused.is_some_and(|key| controls.contains(&key))
    }

    #[test]
    fn no_key_presses_the_cards_buttons() {
        let mut rt = testkit::window("elsewhere");
        let key = parser(&rt);
        for button in CARD_BUTTONS {
            assert!(!element(&rt, button).focusable, "{button}");
        }
        assert!(input_focused(&rt));
        testkit::type_text(&mut rt, "x");
        testkit::press(&mut rt, Key::Enter);
        testkit::press(&mut rt, Key::Text(" ".into()));
        testkit::step(&mut rt);
        assert!(
            rt.state.app.prompt(key).is_some(),
            "Enter and Space chose nothing"
        );
        assert_eq!(glue::view_of(&rt.state, key).draft.text, "x ");
        for _ in 0..30 {
            testkit::press(&mut rt, Key::Tab);
            assert!(!a_card_button_has_focus(&rt));
            if leave_unpressed(&rt) {
                continue;
            }
            testkit::press(&mut rt, Key::Enter);
            testkit::press(&mut rt, Key::Text(" ".into()));
        }
        testkit::step(&mut rt);
        assert!(rt.state.app.prompt(key).is_some(), "still asked");
        assert!(nothing_opened(&rt), "{:?}", fake(&rt).sent());
    }

    #[test]
    fn a_message_to_a_session_whose_folder_is_gone_stays_in_the_box() {
        let mut rt = testkit::window("resting");
        let key = parser(&rt);
        rt.state.folder_exists = |_| false;
        testkit::type_text(&mut rt, "Carry on");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert!(nothing_opened(&rt), "{:?}", fake(&rt).sent());
        assert_eq!(session(&rt, key).phase, Phase::Resting);
        assert_eq!(session(&rt, key).queued, None, "not queued");
        assert_eq!(glue::view_of(&rt.state, key).draft.text, "Carry on");
        let line = element(&rt, "mr-folder-gone");
        assert_eq!(
            line.text.as_deref(),
            Some("This session's folder no longer exists: /home/alex/parser")
        );
        let kit = theme::kit(true);
        assert_eq!(
            line.text_color,
            Some(kit.c.danger.fg),
            "as the picker's error"
        );
        let field = element(&rt, "mr-input-box");
        assert!(
            line.rect[1] >= field.rect[1] + field.rect[3] - 0.5,
            "under the box"
        );
        // Once the folder is back, the next try sends, and the line goes.
        rt.state.folder_exists = |_| true;
        turn_goes_on(&rt);
        testkit::press(&mut rt, Key::Enter);
        for _ in 0..5 {
            if rt.state.app.sessions.get(key).unwrap().open.is_some() {
                break;
            }
            rt.frame(0.016);
        }
        testkit::step(&mut rt);
        assert!(rt.find("mr-folder-gone").is_none());
        assert_eq!(sends(&rt), vec!["Carry on".to_string()]);
    }

    #[test]
    fn a_failed_history_can_still_be_sent_to() {
        let mut rt = testkit::window("history-failed");
        let detail = "No conversation found with session ID: stored-parser";
        fake(&rt).fail_next_open(detail);
        testkit::type_text(&mut rt, "Are you there?");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, detail), "said as any other error");
        let key = parser(&rt);
        assert!(matches!(session(&rt, key).phase, Phase::Ended { .. }));
    }

    #[test]
    fn a_very_long_history_scrolls_smoothly() {
        let mut rt = testkit::window("resting");
        let items = (1..=1_000)
            .flat_map(|n| {
                [
                    HistoryItem::User {
                        text: format!("Question {n}"),
                        image_count: 0,
                        files: Vec::new(),
                        images: Vec::new(),
                    },
                    HistoryItem::AssistantText(format!("Answer {n}")),
                ]
            })
            .collect();
        let history = StoredHistory {
            items,
            ..StoredHistory::default()
        };
        let key = stored(&mut rt, "stored-long", history);
        let built = conversation::row_builds(key);
        rt.update(|ui, tx| glue::show(ui, tx, key, Place::Focused, true));
        rt.frame(0.016);
        assert_eq!(session(&rt, key).conversation.items().len(), 2_000);
        let first = conversation::row_builds(key) - built;
        assert!(first < 100, "the first frame built {first} rows");
        testkit::step(&mut rt);
        let all = conversation::row_builds(key) - built;
        assert!(all < 100, "{all} rows were built");
        let scroll = conversation::conversation_key(key);
        assert_eq!(rt.scroll_at_bottom(&scroll), Some(true), "at its end");
        assert!(testkit::has_text(&rt, "Answer 1000"));
        assert!(!testkit::has_text(&rt, "Answer 500"));
    }
}
