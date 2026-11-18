//! Quit: Ctrl+Q and the sidebar's button end the app, and ask first when something would be
//! cut off ([`glue::ask_quit`]).

use mr_core::Busy;
use rune::{El, KeyEvent, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit};

use crate::glue::{self, Ui};

/// Ctrl+Q (Cmd+Q on macOS): quits.
pub fn is_quit_key(ev: &KeyEvent) -> bool {
    ev.is_command('q')
}

/// What the question says would stop: the sessions working, those with Remote Control on, and
/// what quitting does to them.
pub fn words(busy: Busy) -> String {
    let (working, remote) = (busy.working, busy.remote);
    let is = |n: usize| if n == 1 { "session is" } else { "sessions are" };
    let has = |n: usize| if n == 1 { "has" } else { "have" };
    let session = |n: usize| if n == 1 { "session" } else { "sessions" };
    let what = match (working, remote) {
        (0, 0) => String::new(),
        (working, 0) => format!("{working} {} working. ", is(working)),
        (0, remote) => format!(
            "{remote} {} {} Remote Control on. ",
            session(remote),
            has(remote)
        ),
        (working, remote) => format!(
            "{working} {} working and {remote} {} Remote Control on. ",
            is(working),
            has(remote)
        ),
    };
    format!(
        "{what}Quitting stops everything that is running. Sessions stay in the history and can \
         be continued."
    )
}

/// The question whether to quit, while it shows: the kit's dialog, its words following what
/// runs, with "Cancel", which Return presses, and "Quit". The question protects what runs, so
/// its default is the answer that stops nothing. Escape, a press outside, its close button and
/// "Cancel" put it away.
///
/// The words are the first thing in the dialog's body, not the kit's description, which is
/// drawn on one line however long it is: here they wrap inside the dialog's width, and a
/// screen reader reads them as the dialog's text.
pub fn question(ui: &Ui, kit: &Kit, layer: &mut Vec<El>) {
    if !ui.quit_ask {
        return;
    }
    // Drawn as the kit draws a description, close under the title: the header above ends with
    // 16 points of its own.
    let said = text(words(ui.app.busy()))
        .key("mr-quit-words")
        .font(kit.ty.ui.lh(20.0))
        .color(kit.c.text_muted)
        .wrap()
        .w_full()
        .px(16.0)
        .mt(-8.0)
        .pb(16.0);
    let cancel_button = Button::new("Cancel")
        .on_press(cancel)
        .build(kit)
        .key("mr-quit-cancel")
        .default_button();
    let quit_button = Button::new("Quit")
        .variant(ButtonVariant::Danger)
        .on_press_tx(glue::quit_now)
        .build(kit)
        .key("mr-quit-yes");
    let buttons = row()
        .key("mr-quit-buttons")
        .w_full()
        .justify_end()
        .gap(8.0)
        .px(16.0)
        .pb(16.0)
        .child(cancel_button)
        .child(quit_button);
    Dialog::new("mr-quit", "Quit MindRune?")
        .child(said)
        .child(buttons)
        .fit()
        .open(true)
        .on_close(cancel)
        .build(kit, layer);
}

/// Puts the question away; nothing stops.
fn cancel(ui: &mut Ui) {
    ui.quit_ask = false;
}

#[cfg(test)]
mod tests {
    use mr_agent::{RemoteAccess, SessionCommand, SessionEvent};
    use mr_core::{Busy, Phase};
    use rune::{Key, KeyEvent, Mods, Runtime, Shell};

    use super::words;
    use crate::glue::Ui;
    use crate::{testkit, view};

    #[test]
    fn the_words_say_what_would_stop() {
        let words = |working, remote| words(Busy { working, remote });
        let rest = " Quitting stops everything that is running. Sessions stay in the history and \
                    can be continued.";
        assert_eq!(words(1, 0), format!("1 session is working.{rest}"));
        assert_eq!(words(3, 0), format!("3 sessions are working.{rest}"));
        assert_eq!(
            words(0, 1),
            format!("1 session has Remote Control on.{rest}")
        );
        assert_eq!(
            words(0, 2),
            format!("2 sessions have Remote Control on.{rest}")
        );
        assert_eq!(
            words(2, 1),
            format!("2 sessions are working and 1 has Remote Control on.{rest}")
        );
    }

    /// Ctrl+Q, and a frame.
    fn ctrl_q(rt: &mut Runtime<Ui>) {
        testkit::press_with(rt, Key::Command('q'), testkit::ctrl());
        testkit::step(rt);
    }

    /// Whether the question shows.
    fn asks(rt: &Runtime<Ui>) -> bool {
        testkit::drawn(rt, "mr-quit-dialog")
    }

    /// How many times the fake was sent `command`.
    fn sent_count(rt: &Runtime<Ui>, command: &SessionCommand) -> usize {
        testkit::sent(rt).iter().filter(|c| *c == command).count()
    }

    /// The `working` scene with the question showing.
    fn asking() -> Runtime<Ui> {
        let mut rt = testkit::window("working");
        ctrl_q(&mut rt);
        assert!(asks(&rt), "the question shows");
        rt
    }

    #[test]
    fn ctrl_q_with_nothing_running_quits_at_once() {
        // With the keyboard in the message box, then with nothing focused.
        for blur in [false, true] {
            let mut rt = testkit::window("empty");
            let key = testkit::focused(&rt).unwrap();
            assert!(testkit::box_focused(&rt, key));
            if blur {
                rt.update(|_, tx| tx.blur());
                testkit::step(&mut rt);
                assert!(!testkit::box_focused(&rt, key));
            }
            ctrl_q(&mut rt);
            assert!(rt.state.quitting, "the programs were told to end");
            assert_eq!(rt.shell_request, Some(Shell::Quit), "the app quits");
            assert!(!rt.state.quit_ask);
            assert!(!asks(&rt), "no question");
            assert_eq!(sent_count(&rt, &SessionCommand::Close), 1);
        }
    }

    #[test]
    fn ctrl_q_while_a_turn_runs_asks() {
        let rt = asking();
        assert!(rt.state.quit_ask);
        assert!(testkit::has_text(&rt, "Quit MindRune?"));
        assert!(testkit::has_text(
            &rt,
            "1 session is working. Quitting stops everything that is running."
        ));
        assert!(!rt.state.quitting);
        assert_eq!(rt.shell_request, None);
        assert_eq!(sent_count(&rt, &SessionCommand::Close), 0);
    }

    #[test]
    fn ctrl_q_under_the_question_changes_nothing() {
        let mut rt = asking();
        ctrl_q(&mut rt);
        assert!(rt.state.quit_ask);
        assert!(asks(&rt));
        assert!(!rt.state.quitting);
        assert_eq!(rt.shell_request, None);
    }

    #[test]
    fn with_nothing_running_any_more_only_the_closing_words_are_left() {
        let rest = "Quitting stops everything that is running. Sessions stay in the history \
                    and can be continued.";
        assert_eq!(words(Busy::default()), rest);
        // The turn ends while the question shows: its words follow.
        let mut rt = asking();
        let key = testkit::focused(&rt).unwrap();
        let done = SessionEvent::TurnFinished(mr_agent::TurnSummary {
            cost_usd: 0.01,
            context_used: None,
            duration_ms: 1,
            interrupted: false,
        });
        testkit::emit_to(&mut rt, key, done);
        assert_eq!(rt.state.app.busy(), Busy::default());
        assert!(asks(&rt), "the question stays until it is answered");
        assert!(testkit::texts(&rt).iter().any(|text| text == rest));
        assert!(!testkit::has_text(&rt, "working"));
    }

    #[test]
    fn the_dialogs_own_close_button_and_a_press_outside_close_the_question() {
        let mut rt = asking();
        let [x, y, w, h] = rect_at(&rt, "mr-quit-dialog/header/close");
        let (px, py) = (x + w / 2.0, y + h / 2.0);
        rt.pointer_move(px, py);
        rt.pointer_down(px, py);
        rt.pointer_up(px, py);
        testkit::step(&mut rt);
        assert!(!rt.state.quit_ask);
        assert!(!asks(&rt));
        assert!(!rt.state.quitting, "nothing stopped");
        assert_eq!(sent_count(&rt, &SessionCommand::Close), 0);
        // A press outside the dialog, on the window behind it.
        let mut rt = asking();
        let dialog = testkit::rect(&rt, "mr-quit-dialog").expect("the dialog");
        assert!(dialog[0] > 40.0 && dialog[1] > 40.0, "{dialog:?}");
        rt.pointer_move(20.0, 20.0);
        rt.pointer_down(20.0, 20.0);
        rt.pointer_up(20.0, 20.0);
        testkit::step(&mut rt);
        assert!(!rt.state.quit_ask);
        assert!(!rt.state.quitting, "nothing stopped");
        assert_eq!(
            testkit::tile_keys(&rt).len(),
            1,
            "and nothing behind was pressed"
        );
    }

    #[test]
    fn cancel_gives_the_keyboard_back_to_the_focused_tiles_box() {
        // Three tiles, the middle one focused with the keyboard in its box.
        let mut rt = testkit::window_sized("tiles", (1800.0, 900.0));
        let keys = testkit::tile_keys(&rt);
        let focused = testkit::focused(&rt).unwrap();
        assert!(keys.len() > 1 && testkit::box_focused(&rt, focused));
        ctrl_q(&mut rt);
        assert!(asks(&rt));
        // While the question shows it has the keyboard: no box is drawn with it.
        for key in &keys {
            assert!(!testkit::box_focused(&rt, *key), "{key:?}");
            assert!(!crate::glue::view_of(&rt.state, *key).draft.focused);
        }
        testkit::click(&mut rt, "mr-quit-cancel");
        testkit::step(&mut rt);
        assert_eq!(
            testkit::focused(&rt),
            Some(focused),
            "the focus did not move"
        );
        assert!(testkit::box_focused(&rt, focused), "the keyboard is back");
        assert_eq!(crate::glue::typing_in(&rt.state), Some(focused));
        // Return, the question's default, does the same.
        ctrl_q(&mut rt);
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert!(!asks(&rt));
        assert!(testkit::box_focused(&rt, focused));
    }

    #[test]
    fn the_words_follow_what_runs() {
        let mut rt = asking();
        let key = testkit::focused(&rt).unwrap();
        let on = RemoteAccess::On { url: None };
        testkit::emit_to(&mut rt, key, SessionEvent::RemoteAccess(on));
        assert!(testkit::has_text(
            &rt,
            "1 session is working and 1 has Remote Control on."
        ));
    }

    #[test]
    fn cancel_closes_the_question_and_nothing_stopped() {
        let mut rt = asking();
        testkit::click(&mut rt, "mr-quit-cancel");
        testkit::step(&mut rt);
        assert!(!rt.state.quit_ask);
        assert!(!asks(&rt));
        assert!(!rt.state.quitting);
        assert_eq!(rt.shell_request, None);
        assert_eq!(sent_count(&rt, &SessionCommand::Close), 0);
        assert_eq!(sent_count(&rt, &SessionCommand::Stop), 0);
    }

    #[test]
    fn escape_closes_the_question_and_stops_no_turn() {
        let mut rt = asking();
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert!(!rt.state.quit_ask);
        assert!(!asks(&rt));
        assert_eq!(sent_count(&rt, &SessionCommand::Stop), 0, "no interrupt");
        assert_eq!(
            rt.state.focused_session().unwrap().phase,
            Phase::Working,
            "the turn still runs"
        );
        // Rune keeps the keyboard in the question, so the window's keys come after it has taken
        // Escape; were they to come first, they would leave it alone.
        let mut rt = asking();
        let escape = KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        };
        let taken = rt.update_if(|ui, tx| view::on_key(ui, &escape, tx));
        assert!(!taken);
        testkit::step(&mut rt);
        assert!(rt.state.quit_ask);
        assert_eq!(sent_count(&rt, &SessionCommand::Stop), 0, "no interrupt");
    }

    #[test]
    fn ctrl_w_under_the_question_closes_no_tile() {
        let mut rt = asking();
        let key = testkit::focused(&rt).unwrap();
        testkit::press_with(&mut rt, Key::Command('w'), testkit::ctrl());
        testkit::step(&mut rt);
        assert_eq!(testkit::tile_keys(&rt), [key]);
        assert!(rt.state.quit_ask, "the question still shows");
    }

    #[test]
    fn typing_under_the_question_reaches_no_box() {
        let mut rt = asking();
        let before = testkit::shown_view(&rt).draft.text.clone();
        testkit::type_text(&mut rt, "abc");
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).draft.text, before);
    }

    #[test]
    fn quit_in_the_question_ends_the_programs() {
        let mut rt = asking();
        let live = rt.state.app.sessions.iter().filter(|e| e.live()).count();
        assert!(live >= 1);
        testkit::click(&mut rt, "mr-quit-yes");
        testkit::step(&mut rt);
        assert!(rt.state.quitting);
        assert_eq!(rt.shell_request, Some(Shell::Quit));
        assert_eq!(
            sent_count(&rt, &SessionCommand::Close),
            live,
            "one per live session"
        );
    }

    #[test]
    fn return_presses_cancel() {
        let mut rt = asking();
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert!(!rt.state.quit_ask, "the question is gone");
        assert!(!asks(&rt));
        assert!(!rt.state.quitting, "nothing stopped");
        assert_eq!(rt.shell_request, None);
        assert_eq!(sent_count(&rt, &SessionCommand::Close), 0);
        assert_eq!(
            rt.state.focused_session().unwrap().phase,
            Phase::Working,
            "the turn still runs"
        );
    }

    /// The drawn rectangle of the one element whose key path ends with `path`.
    fn rect_at(rt: &Runtime<Ui>, path: &str) -> [f32; 4] {
        let found: Vec<[f32; 4]> = rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.ends_with(path))
            .map(|e| e.rect)
            .collect();
        assert_eq!(found.len(), 1, "{path}: {found:?}");
        found[0]
    }

    /// Whether `inner` lies wholly inside `outer`, each `[x, y, w, h]`.
    fn inside(inner: [f32; 4], outer: [f32; 4]) -> bool {
        inner[0] >= outer[0]
            && inner[1] >= outer[1]
            && inner[0] + inner[2] <= outer[0] + outer[2]
            && inner[1] + inner[3] <= outer[1] + outer[3]
    }

    #[test]
    fn the_question_lies_inside_the_window_and_its_words_wrap() {
        // The longest wording: sessions working and Remote Control on.
        let mut rt = asking();
        let key = testkit::focused(&rt).unwrap();
        let on = RemoteAccess::On { url: None };
        testkit::emit_to(&mut rt, key, SessionEvent::RemoteAccess(on));
        let window = [0.0, 0.0, 1200.0, 800.0];
        let dialog = testkit::rect(&rt, "mr-quit-dialog").expect("the dialog");
        assert!(inside(dialog, window), "{dialog:?}");
        assert!(dialog[2] <= 480.0, "the kit's width: {dialog:?}");
        let close = rect_at(&rt, "mr-quit-dialog/header/close");
        assert!(inside(close, dialog), "the close button: {close:?}");
        let words = testkit::rect(&rt, "mr-quit-words").expect("the words");
        assert!(words[2] <= dialog[2], "no wider than the dialog: {words:?}");
        assert!(inside(words, dialog), "{words:?} in {dialog:?}");
        assert!(words[3] >= 40.0, "on more than one line: {words:?}");
        for button in ["mr-quit-cancel", "mr-quit-yes"] {
            let button = testkit::rect(&rt, button).expect("a button");
            assert!(inside(button, dialog), "{button:?}");
        }
    }

    #[test]
    fn the_quit_button_in_the_sidebar_asks_the_same() {
        let mut rt = testkit::window("working");
        testkit::click(&mut rt, "mr-quit");
        testkit::step(&mut rt);
        assert!(asks(&rt));
        assert!(!rt.state.quitting);
        testkit::click(&mut rt, "mr-quit-cancel");
        testkit::step(&mut rt);
        // With nothing running it quits at once, also with no account known.
        let mut rt = testkit::window("empty");
        rt.state.app.account = None;
        rt.invalidate();
        testkit::step(&mut rt);
        testkit::click(&mut rt, "mr-quit");
        testkit::step(&mut rt);
        assert!(!asks(&rt));
        assert!(rt.state.quitting);
        assert_eq!(rt.shell_request, Some(Shell::Quit));
    }

    #[test]
    fn the_question_is_a_dialog_to_a_screen_reader() {
        let rt = asking();
        let tree = rt.accessibility_json();
        let node = find(&tree, "mr-quit-dialog").expect("the question is in the tree");
        assert_eq!(node["role"], "dialog");
        assert_eq!(node["name"], "Quit MindRune?");
    }

    /// The node keyed `key` in `tree` or under it.
    fn find<'a>(tree: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
        if tree["key"] == key {
            return Some(tree);
        }
        tree["children"]
            .as_array()?
            .iter()
            .find_map(|child| find(child, key))
    }
}
