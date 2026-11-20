//! The choices a session's last turn offered, drawn above its input box under its task list:
//! the question, and a button for each answer that sends its message as the user's. The box
//! shows only while the session takes the message ([`App::choices`]), and nothing of it takes
//! the keyboard's focus: a choice is pressed with the pointer only, and the text area keeps
//! what is typed in it.
//!
//! [`App::choices`]: mr_core::App::choices

use mr_agent::Choices;
use mr_core::SessionKey;
use rune::{El, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit};

use crate::glue::{self, Ui};
use crate::view::{input, requests, tasks, tiles};

/// The height counted for the box: its padding, one line of question, one row of buttons, the
/// gap below. Its border takes no room.
pub const HEIGHT: f32 = 2.0 * PAD + QUESTION_LINE + GAP + BUTTON_HEIGHT + GAP_BELOW;

/// The space inside the box.
const PAD: f32 = 12.0;

/// The most lines of a question drawn in a tile at least [`tasks::FOLD_BELOW`] high: a longer
/// one ends in an ellipsis, and its help holds it whole.
const QUESTION_LINES: usize = 3;

/// How many lines of the question are drawn in a tile `tile_height` high: one in a tile shorter
/// than [`tasks::FOLD_BELOW`], the line [`HEIGHT`] counts, else up to [`QUESTION_LINES`]. A
/// question cut short ends in an ellipsis and is whole in its help.
fn question_lines(tile_height: Option<f32>) -> usize {
    match tile_height {
        Some(height) if height < tasks::FOLD_BELOW => 1,
        _ => QUESTION_LINES,
    }
}

/// The space between the question and the buttons.
const GAP: f32 = 10.0;

/// The height of one line of the question: a line of the kit's `ui` type, as `Kit::neutral`
/// sets it in either theme. The test `the_height_counts_what_is_drawn` measures it.
const QUESTION_LINE: f32 = 20.0;

/// The height of a button. Copied from the kit, which does not export it: the height of a
/// `Button` of `ControlSize::Regular`, in `push_metrics` (`crates/rune-kit/src/actions.rs` in
/// the Rune checkout, `ControlSize::Regular => (24.0, ..)`).
const BUTTON_HEIGHT: f32 = 24.0;

/// The space between two buttons.
const BUTTONS_GAP: f32 = 8.0;

/// The space between the box and the input box under it: the task list's.
const GAP_BELOW: f32 = tasks::GAP_BELOW;

/// The most characters of a message a button's help shows, before an ellipsis.
const HELP_MAX: usize = 200;

/// The choice box of `key`'s session, keyed `mr-choices-{n}`, with the space under it: the
/// question keyed `mr-choices-question` ([`question_lines`]), and under it a row that wraps of
/// buttons keyed `mr-choice-{i}`, the first one the agent's recommendation. None when
/// [`offered`] gives no offer.
pub fn boxed(ui: &Ui, kit: &Kit, key: SessionKey) -> Option<El> {
    let offer = offered(ui, key)?;
    let enabled = ui.app.can_choose(key);
    let question = text(offer.question.clone())
        .key("mr-choices-question")
        .font(kit.ty.ui)
        .color(kit.c.text_base)
        .max_lines(question_lines(tiles::tile_height(ui, key)))
        .help_when_truncated(offer.question.clone())
        .w_full();
    let buttons = offer
        .choices
        .iter()
        .enumerate()
        .map(|(i, choice)| button(key, i, &choice.label, &choice.message, enabled, kit));
    let boxed = col()
        .key(format!("mr-choices-{}", key.0))
        .w_full()
        .padding(PAD)
        .gap(GAP)
        .radius(kit.radius.lg)
        .fill(kit.c.bg_base)
        .border(1.0, kit.c.border_weak)
        .child(question)
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(BUTTONS_GAP)
                .children(buttons),
        );
    Some(col().w_full().pb(GAP_BELOW).child(boxed))
}

/// The height [`boxed`] is counted at: [`HEIGHT`] while a box shows, else 0.
pub fn height(ui: &Ui, key: SessionKey) -> f32 {
    if offered(ui, key).is_some() {
        HEIGHT
    } else {
        0.0
    }
}

/// The offer of `key`'s session when its box is drawn: while the session takes the message
/// ([`App::choices`](mr_core::App::choices): not deleted, no turn running, no question waiting),
/// in a tile whose height is known. Before the tile row is first laid out its tiles' heights
/// are not, and the task list over the box is not drawn then either ([`tasks::listed`]).
fn offered(ui: &Ui, key: SessionKey) -> Option<&Choices> {
    tiles::tile_height(ui, key)?;
    ui.app.choices(key)
}

/// The button of choice `i`, keyed `mr-choice-{i}`: the first in the contrast look, the others
/// neutral, greyed unless `enabled`, with the message it sends as its help ([`help_of`]). It
/// never takes the keyboard's focus.
///
/// The kit greys a button with a layer opacity, which Rune lands at once (`El::layer_opacity`),
/// and keeps its fill and its shape: a button greyed or not shows the change in the first frame.
fn button(key: SessionKey, i: usize, label: &str, message: &str, enabled: bool, kit: &Kit) -> El {
    let variant = if i == 0 {
        ButtonVariant::Contrast
    } else {
        ButtonVariant::Neutral
    };
    let button = Button::new(label)
        .variant(variant)
        .help(help_of(message))
        .disabled(!enabled)
        .on_press(move |ui: &mut Ui| press(ui, key, i))
        .build(kit)
        .key(format!("mr-choice-{i}"));
    requests::unfocusable(button)
}

/// A press on choice `i` of `key`'s session sends its message as the user's, with the chips,
/// as Enter sends what is typed. A press that cannot send does nothing. Nothing goes to a
/// resting session whose folder is gone: the line under the box says so, and the offer stays
/// ([`input::folder_there`]). What is typed in the box is left as it is.
fn press(ui: &mut Ui, key: SessionKey, i: usize) {
    if !ui.app.can_choose(key) || !input::folder_there(ui, key) {
        return;
    }
    let effects = ui.app.choose(key, i);
    glue::run(ui, effects);
}

/// What a button's help says: the message it sends, cut to [`HELP_MAX`] characters with `…`
/// after it when something was cut.
fn help_of(message: &str) -> String {
    match message.char_indices().nth(HELP_MAX) {
        Some((end, _)) => format!("{}…", &message[..end]),
        None => message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use mr_agent::{Choice, Choices, FakeStep, SessionCommand, SessionEvent, TurnSummary};
    use rune::{Key, Runtime};

    use super::*;
    use crate::{demo, testkit, theme};

    /// Runs frames until what moves or fades in has settled.
    fn settle(rt: &mut Runtime<Ui>) {
        for _ in 0..40 {
            testkit::step(rt);
        }
    }

    /// The path [`testkit::tile`] gives to `inner` of the focused tile.
    fn in_focused(rt: &Runtime<Ui>, inner: &str) -> String {
        testkit::tile(testkit::focused(rt).expect("a tile is focused"), inner)
    }

    /// The drawn rectangle of `inner` in the focused tile.
    fn rect_of(rt: &Runtime<Ui>, inner: &str) -> [f32; 4] {
        testkit::rect(rt, &in_focused(rt, inner)).unwrap_or_else(|| panic!("no {inner}"))
    }

    /// Whether the focused tile shows a text that contains `part`.
    fn shows(rt: &Runtime<Ui>, part: &str) -> bool {
        testkit::tile_has_text(rt, testkit::focused(rt).unwrap(), part)
    }

    /// The key of the focused session's box.
    fn box_key(rt: &Runtime<Ui>) -> String {
        format!("mr-choices-{}", testkit::focused(rt).unwrap().0)
    }

    /// What is drawn of a choice box, leaving ones aside.
    fn box_parts(rt: &Runtime<Ui>) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting)
            .filter_map(|e| e.key)
            .filter(|key| key.starts_with("mr-choice"))
            .collect()
    }

    /// The text of every `Send` the fake received.
    fn sends(rt: &Runtime<Ui>) -> Vec<String> {
        testkit::sent(rt)
            .into_iter()
            .filter_map(|c| match c {
                SessionCommand::Send { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    /// The fake's next turn starts and goes on.
    fn turn_goes_on(rt: &Runtime<Ui>) {
        let fake = rt.state.fake.as_ref().unwrap();
        fake.reply(vec![FakeStep::at(0.0, SessionEvent::TurnStarted)]);
    }

    fn turn_finished(interrupted: bool) -> SessionEvent {
        SessionEvent::TurnFinished(TurnSummary {
            cost_usd: 0.0,
            context_used: None,
            duration_ms: 1,
            interrupted,
        })
    }

    fn offer() -> Choices {
        let choice = |label: &str, message: &str| Choice {
            label: label.into(),
            message: message.into(),
        };
        Choices::new(
            "Go on?",
            vec![choice("Yes", "Yes, go on."), choice("No", "No, stop here.")],
        )
        .expect("an offer that can be shown")
    }

    #[test]
    fn the_choices_scene_shows_the_question_and_three_buttons_above_the_box() {
        let mut rt = testkit::window("choices");
        // Drawn from the second frame, once the tile's height is known, the box enters as
        // anything new above the input box does.
        settle(&mut rt);
        for words in [
            "The banner is resized and the page checked on a phone.",
            "Publish the new banner?",
            "Publish",
            "Show me first",
            "Not now",
        ] {
            assert!(shows(&rt, words), "{words:?} shows");
        }
        let key = box_key(&rt);
        let boxed = rect_of(&rt, &key);
        let input = rect_of(&rt, "mr-input-box");
        let frame = rect_of(&rt, "mr-conversation-frame");
        assert!(
            (boxed[1] + boxed[3] + GAP_BELOW - input[1]).abs() < 0.5,
            "{boxed:?} right over {input:?}"
        );
        assert!((boxed[0] - input[0]).abs() < 0.5 && (boxed[2] - input[2]).abs() < 0.5);
        assert!(boxed[1] >= frame[1] + frame[3] - 0.5, "under {frame:?}");
        assert!(
            (boxed[3] + GAP_BELOW - HEIGHT).abs() < 0.5,
            "as high as counted"
        );
        let question = rect_of(&rt, "mr-choices-question");
        let first = rect_of(&rt, "mr-choice-0");
        assert!(
            question[1] + question[3] <= first[1],
            "the buttons under the question"
        );
        let element = |inner: &str| testkit::element(&rt, &in_focused(&rt, inner)).unwrap();
        assert_eq!(element(&key).radius, theme::kit(true).radius.lg);
        assert_eq!(element("mr-choice-0").help.as_deref(), Some("/publish"));
        assert_eq!(
            element("mr-choice-1").help.as_deref(),
            Some("Show me the page before anything is published.")
        );
        assert_ne!(
            element("mr-choice-0").fill,
            element("mr-choice-1").fill,
            "the first stands out"
        );
        assert_eq!(element("mr-choice-1").fill, element("mr-choice-2").fill);
        let inside = format!("/{key}");
        let focusable: Vec<String> = rt
            .elements()
            .into_iter()
            .filter(|e| e.path.contains(&inside) && e.focusable)
            .map(|e| e.path)
            .collect();
        assert_eq!(
            focusable,
            Vec::<String>::new(),
            "nothing takes the keyboard"
        );
        for name in ["offer_choices", "mcp__mindrune__offer_choices"] {
            assert!(!testkit::has_text(&rt, name), "no tool line names {name}");
        }
    }

    #[test]
    fn a_click_sends_the_message_and_leaves_what_was_typed() {
        let mut rt = testkit::window("choices");
        settle(&mut rt);
        let key = testkit::focused(&rt).unwrap();
        testkit::type_text(&mut rt, "half a thought");
        turn_goes_on(&rt);
        let first = in_focused(&rt, "mr-choice-0");
        testkit::click(&mut rt, &first);
        testkit::step(&mut rt);
        let sent = testkit::sent(&rt);
        let publish = |c: &SessionCommand| {
            matches!(c, SessionCommand::Send { text, attachments }
                if text == "/publish" && attachments.is_empty())
        };
        assert!(sent.last().is_some_and(publish), "{sent:?}");
        assert_eq!(box_parts(&rt), Vec::<String>::new());
        assert_eq!(glue::view_of(&rt.state, key).draft.text, "half a thought");
        assert!(testkit::box_focused(&rt, key), "still typed in");
        assert!(shows(&rt, "/publish"), "the user's message");
    }

    #[test]
    fn typing_a_message_of_ones_own_takes_the_box_away() {
        let mut rt = testkit::window("choices");
        settle(&mut rt);
        assert!(!box_parts(&rt).is_empty());
        turn_goes_on(&rt);
        testkit::type_text(&mut rt, "wait");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(box_parts(&rt), Vec::<String>::new());
        assert_eq!(sends(&rt).last().map(String::as_str), Some("wait"));
    }

    #[test]
    fn the_box_is_not_there_while_the_turn_still_runs() {
        let mut rt = testkit::window("working");
        settle(&mut rt);
        testkit::emit(&mut rt, SessionEvent::ChoicesOffered(offer()));
        settle(&mut rt);
        assert_eq!(box_parts(&rt), Vec::<String>::new());
        let key = testkit::focused(&rt).unwrap();
        assert_eq!(height(&rt.state, key), 0.0, "nothing counted");
        testkit::emit(&mut rt, turn_finished(false));
        settle(&mut rt);
        assert!(shows(&rt, "Go on?"));
        assert!(shows(&rt, "Yes") && shows(&rt, "No"));
        assert_eq!(height(&rt.state, key), HEIGHT);
    }

    /// Whether the first button is greyed, as a screen reader hears it.
    fn first_greyed(rt: &Runtime<Ui>) -> bool {
        fn find<'a>(node: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
            if node["key"].as_str() == Some(key) {
                return Some(node);
            }
            node["children"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|child| find(child, key))
        }
        let tree = rt.accessibility_json();
        let button = find(&tree, "mr-choice-0").expect("the first button");
        button["states"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|state| state == "disabled")
    }

    /// A pasted image's chip, still being read, put on `key`'s session as a paste puts one.
    /// It is never read: no reading was asked for.
    fn chip_being_read(rt: &mut Runtime<Ui>, key: mr_core::SessionKey) {
        let source = mr_core::Source::Pasted(mr_agent::ImageData::new(testkit::tiny_png()));
        let chip = mr_core::Chip {
            id: 1,
            name: mr_core::name_of(&source),
            source,
            state: mr_core::ChipState::Reading,
        };
        rt.state
            .app
            .sessions
            .get_mut(key)
            .unwrap()
            .session
            .chips
            .push(chip);
        rt.invalidate();
    }

    #[test]
    fn the_buttons_are_greyed_while_an_attachment_is_read() {
        let mut rt = testkit::window("choices");
        settle(&mut rt);
        let key = testkit::focused(&rt).unwrap();
        let shape = |rt: &Runtime<Ui>| {
            let button = testkit::element(rt, &in_focused(rt, "mr-choice-0")).unwrap();
            // Its place moves as the chip's row grows the input box under it.
            (button.rect[2..].to_vec(), button.radius, button.fill)
        };
        let before = shape(&rt);
        assert!(!first_greyed(&rt));
        chip_being_read(&mut rt, key);
        // The first frame after the change shows the buttons greyed, in the same shape.
        rt.frame(0.016);
        assert!(!rt.state.app.can_choose(key));
        assert!(first_greyed(&rt));
        assert_eq!(shape(&rt), before);
        let sent = testkit::sent(&rt).len();
        let first = in_focused(&rt, "mr-choice-0");
        testkit::click(&mut rt, &first);
        testkit::step(&mut rt);
        assert_eq!(testkit::sent(&rt).len(), sent, "a click sends nothing");
        assert!(!box_parts(&rt).is_empty(), "the box stays");
        // Taken off again, the buttons are as they were in the next frame.
        rt.state
            .app
            .sessions
            .get_mut(key)
            .unwrap()
            .session
            .chips
            .clear();
        rt.invalidate();
        rt.frame(0.016);
        assert!(rt.state.app.can_choose(key));
        assert!(!first_greyed(&rt));
        assert_eq!(shape(&rt), before);
    }

    #[test]
    fn the_height_counts_what_is_drawn() {
        assert_eq!(HEIGHT, 86.0);
        for dark in [true, false] {
            assert_eq!(theme::kit(dark).ty.ui.line_px(), QUESTION_LINE);
        }
    }

    #[test]
    fn a_choice_to_a_session_whose_folder_is_gone_sends_nothing() {
        let mut rt = testkit::window("choices");
        settle(&mut rt);
        let key = testkit::focused(&rt).unwrap();
        // The program ends cleanly: the session rests, and the offer it showed stays.
        let gone = SessionEvent::Exited {
            clean: true,
            detail: String::new(),
        };
        testkit::emit(&mut rt, gone);
        assert_eq!(
            rt.state.app.sessions.get(key).unwrap().session.phase,
            mr_core::Phase::Resting
        );
        assert!(rt.state.app.can_choose(key));
        rt.state.folder_exists = |_| false;
        let sent = testkit::sent(&rt).len();
        let first = in_focused(&rt, "mr-choice-0");
        testkit::click(&mut rt, &first);
        testkit::step(&mut rt);
        assert_eq!(testkit::sent(&rt).len(), sent, "{:?}", testkit::sent(&rt));
        let entry = rt.state.app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, mr_core::Phase::Resting);
        assert!(entry.open.is_none(), "no program opened");
        assert!(shows(
            &rt,
            "This session's folder no longer exists: /home/alex/project"
        ));
        assert!(!box_parts(&rt).is_empty(), "the offer stays");
        // Once the folder is back, the press sends, and the line goes.
        rt.state.folder_exists = |_| true;
        turn_goes_on(&rt);
        let first = in_focused(&rt, "mr-choice-0");
        testkit::click(&mut rt, &first);
        for _ in 0..5 {
            if rt.state.app.sessions.get(key).unwrap().open.is_some() {
                break;
            }
            rt.frame(0.016);
        }
        settle(&mut rt);
        assert!(!shows(&rt, "This session's folder no longer exists"));
        assert_eq!(sends(&rt).last().map(String::as_str), Some("/publish"));
    }

    #[test]
    fn a_tile_whose_height_is_not_known_yet_draws_no_box() {
        let mut ui = demo::ui();
        let (_, scene) = demo::SCENES
            .iter()
            .find(|(name, _)| *name == "choices")
            .unwrap();
        scene(&mut ui);
        let key = ui.app.focused().unwrap();
        assert!(ui.app.choices(key).is_some());
        assert_eq!(height(&ui, key), 0.0, "nothing counted");
        assert!(boxed(&ui, theme::kit(true), key).is_none(), "nothing drawn");
        let mut rt = testkit::runtime(ui, (1200.0, 800.0));
        rt.frame(0.016);
        assert_eq!(box_parts(&rt), Vec::<String>::new());
        testkit::step(&mut rt);
        assert!(!box_parts(&rt).is_empty());
        assert_eq!(height(&rt.state, key), HEIGHT);
    }

    #[test]
    fn a_deleted_session_draws_no_box() {
        let mut rt = testkit::window("choices");
        settle(&mut rt);
        let key = testkit::focused(&rt).unwrap();
        let effects = rt.state.app.delete_session(key);
        glue::run(&mut rt.state, effects);
        testkit::step(&mut rt);
        assert!(rt.state.app.sessions.get(key).unwrap().deleted.is_some());
        assert!(boxed(&rt.state, theme::kit(true), key).is_none());
        assert_eq!(height(&rt.state, key), 0.0);
        assert_eq!(box_parts(&rt), Vec::<String>::new());
    }

    #[test]
    fn a_long_question_is_cut_at_its_end() {
        let mut rt = testkit::window("working");
        settle(&mut rt);
        let question: String = "Should the banner be resized again before it goes out? "
            .repeat(6)
            .chars()
            .take(300)
            .collect();
        let offer = Choices::new(
            &question,
            vec![Choice {
                label: "Yes".into(),
                message: "Yes.".into(),
            }],
        )
        .unwrap();
        testkit::emit(&mut rt, SessionEvent::ChoicesOffered(offer.clone()));
        testkit::emit(&mut rt, turn_finished(false));
        rt.set_viewport((420.0, 800.0), 1.0);
        settle(&mut rt);
        let boxed = rect_of(&rt, &box_key(&rt));
        let shown = rect_of(&rt, "mr-choices-question");
        assert!(
            shown[3] <= 3.0 * QUESTION_LINE + 0.5,
            "at most three lines: {shown:?}"
        );
        assert!(
            (shown[1] - (boxed[1] + PAD)).abs() < 0.5,
            "its top in sight: {shown:?} in {boxed:?}"
        );
        let element = testkit::element(&rt, &in_focused(&rt, "mr-choices-question")).unwrap();
        assert_eq!(element.help.as_deref(), Some(offer.question.as_str()));
    }

    #[test]
    fn a_press_that_cannot_send_leaves_the_folder_line_as_it_was() {
        let mut rt = testkit::window("choices");
        settle(&mut rt);
        let key = testkit::focused(&rt).unwrap();
        let gone = SessionEvent::Exited {
            clean: true,
            detail: String::new(),
        };
        testkit::emit(&mut rt, gone);
        rt.state.folder_exists = |_| false;
        chip_being_read(&mut rt, key);
        assert!(!rt.state.app.can_choose(key));
        press(&mut rt.state, key, 0);
        assert_eq!(glue::view_of(&rt.state, key).folder_gone, None);
    }

    #[test]
    fn the_open_elsewhere_card_keeps_the_box_away_until_cancel() {
        let mut rt = testkit::window("choices");
        settle(&mut rt);
        let key = testkit::focused(&rt).unwrap();
        let gone = SessionEvent::Exited {
            clean: true,
            detail: String::new(),
        };
        testkit::emit(&mut rt, gone);
        // As after a start of the app: nothing it wrote to the session is known, so the store
        // is asked whether another app changed it, and says one did a moment ago.
        let id = {
            let mut entry = rt.state.app.sessions.get_mut(key).unwrap();
            entry.last_own_unix = None;
            entry.session.id.clone()
        };
        let fake = rt.state.fake.clone().unwrap();
        fake.set_last_active(&id.unwrap(), Some(demo::NOW_UNIX - 30));
        rt.state.folder_exists = |_| true;
        let first = in_focused(&rt, "mr-choice-0");
        testkit::click(&mut rt, &first);
        settle(&mut rt);
        assert!(rt.state.app.prompt(key).is_some(), "the card asks");
        assert!(testkit::has_text(
            &rt,
            "This session may be open in another app"
        ));
        assert_eq!(
            box_parts(&rt),
            Vec::<String>::new(),
            "no box under the card"
        );
        let cancel = in_focused(&rt, "mr-elsewhere-cancel");
        testkit::click(&mut rt, &cancel);
        settle(&mut rt);
        assert!(rt.state.app.prompt(key).is_none());
        assert!(!box_parts(&rt).is_empty(), "the box is back");
        assert!(shows(&rt, "Publish the new banner?"));
    }

    #[test]
    fn a_long_message_is_cut_for_its_help() {
        assert_eq!(help_of("Leave it."), "Leave it.");
        let long = "a".repeat(201);
        let help = help_of(&long);
        assert_eq!(help.chars().count(), 201);
        assert!(help.starts_with(&"a".repeat(200)) && help.ends_with('…'));
        assert_eq!(help_of(&"é".repeat(200)), "é".repeat(200), "not cut at 200");
    }
}
