//! The root view: the sign-in screens until Claude Code is signed in, then the shell, with a
//! row of tiles that each hold one session's conversation area.

use mr_core::{Screen, SessionKey};
use rune::{Cx, El, KeyEvent, Tx, col, el};
use rune_kit::Kit;

use crate::glue::{self, Ui};
use crate::theme;

mod conversation;
mod drag;
mod folder;
mod input;
mod items;
mod requests;
mod resting;
mod shell;
mod sidebar;
mod signin;
mod tiles;
mod width;

pub(crate) use conversation::conversation_key;
pub(crate) use items::row_key;
#[cfg(test)]
pub(crate) use tiles::{ROW, tile_key};

pub fn view(ui: &Ui, cx: &mut Cx<Ui>) -> El {
    let kit = theme::kit(cx.system_dark());
    let mut layer: Vec<El> = Vec::new();
    let base = match ui.app.screen {
        Screen::Main => {
            // The one place the conversation area reads which sessions are in tiles: `pane`
            // and everything under it draw the session they are given. Other views read the
            // focused one: `on_key` below (Escape stops the focused session's turn) and, in
            // `folder`, `pick` (where the picker starts), `start` (whether a new session was
            // shown, to focus its box) and `floating_error` (whether a session is on screen).
            let main = tiles::row(ui, cx, kit, &mut layer);
            let shell = shell::shell(ui, kit, main);
            sidebar::menu(ui, cx, kit, &mut layer);
            folder::menu(ui, cx, kit, &mut layer);
            folder::floating_error(ui, cx, kit, &mut layer);
            sidebar::rename_watch(ui, &mut layer);
            tiles::focus_watch(ui, &mut layer);
            drag::overlay(ui, kit, &mut layer);
            shell
        }
        Screen::Checking | Screen::SignedOut | Screen::NotInstalled => signin::screen(ui, kit),
    };
    // The app's own root, under the kit's: what a screenshot's tree is checked for.
    let base = el().key("mr-root").child(base.w_full().h_full());
    rune_kit::root(base, layer)
}

/// The conversation area for one session: its conversation, and below it its input box, whose
/// pickers put their lists in `layer`, both in the column whose width the user drags. While the
/// app asks before joining this session where another app may have it open, the card that asks
/// sits between the two.
fn pane(ui: &Ui, cx: &mut Cx<Ui>, kit: &'static Kit, key: SessionKey, layer: &mut Vec<El>) -> El {
    let width = width::current(ui, cx, key);
    let content = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(conversation::conversation(ui, kit, key, width))
        .children(resting::elsewhere(ui, key, kit, width))
        .child(input::input(ui, kit, key, layer, width));
    width::column(ui, cx, kit, key, content)
}

/// `AppSpec::on_key`, for keys no focused text field took: the kit's sidebar shortcut shows and
/// hides the sidebar; on the main screen, Ctrl+N asks for a new session's folder, and
/// Ctrl+Shift+N for one to the side, F6 and Shift+F6 move to the next and the previous tile, and
/// Ctrl+W closes the focused one, once per press, the last three unless a picker's list or a
/// menu is open (a text field leaves all of them alone); Ctrl+Shift+PageUp and
/// Ctrl+Shift+PageDown move the focused tile one place along the row, and Ctrl+Alt+PageUp and
/// Ctrl+Alt+PageDown up or down in a column, also held, on the same terms, and are taken on the
/// main screen even when nothing moves; and Escape shuts the folder menu when it is open, or
/// else stops the focused tile's running turn (a focused text area catches its own Escape,
/// which never gets here). With neither, Escape is left alone. An open picker or menu, and the
/// field a name is edited in, take Escape before this. Ctrl+N and
/// Ctrl+Shift+N keep a name being edited first; F6 keeps it as the field loses the keyboard,
/// and Ctrl+W drops it with the tile whose header it was edited in. While a sidebar row or a
/// tile's header is dragged, the drag takes them first ([`drag_key`]).
pub fn on_key(ui: &mut Ui, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if shell::is_sidebar_toggle(ev) {
        ui.sidebar.toggle();
        return true;
    }
    if ui.drag.is_some() {
        return drag_key(ui, ev);
    }
    if folder::is_new_session_key(ev) {
        if ui.app.screen == Screen::Main && !tx.key_repeat() {
            // The folder picker leaves the field where it is: the name is kept first, before
            // the new session can take the place of the one renamed in its header.
            glue::end_rename(ui, true);
            folder::ask(ui, tx, None, false);
        }
        return true;
    }
    if folder::is_side_session_key(ev) {
        if ui.app.screen == Screen::Main && !tx.key_repeat() && !list_open(ui) {
            glue::end_rename(ui, true);
            folder::ask(ui, tx, None, true);
        }
        return true;
    }
    if let Some(step) = tiles::tile_step(ev) {
        return ui.app.screen == Screen::Main && (list_open(ui) || tiles::step_focus(ui, tx, step));
    }
    if let Some(step) = tiles::move_step(ev) {
        if ui.app.screen != Screen::Main {
            return false;
        }
        if !list_open(ui) {
            tiles::step_move(ui, tx, step);
        }
        return true;
    }
    if let Some(step) = tiles::stack_step(ev) {
        if ui.app.screen != Screen::Main {
            return false;
        }
        if !list_open(ui) {
            tiles::step_stack(ui, tx, step);
        }
        return true;
    }
    if tiles::is_close_key(ev)
        && ui.app.screen == Screen::Main
        && let Some(key) = ui.app.focused()
    {
        if !tx.key_repeat() && !list_open(ui) {
            glue::close_tile(ui, tx, key);
        }
        return true;
    }
    if input::is_escape(ev) && ui.folder_menu.is_some() {
        folder::close(ui);
        return true;
    }
    input::is_escape(ev)
        && ui
            .app
            .focused()
            .is_some_and(|key| input::stop_turn(ui, key))
}

/// A key while a sidebar row or a tile's header is dragged (ruling R8): Escape ends the drag
/// and does nothing else, also when Rune cannot cancel it itself because the pressed row has
/// left the list; the window's other keys (Ctrl+N, Ctrl+Shift+N, F6, Shift+F6, Ctrl+W and the
/// four that move a tile) wait. True when the key was one of these.
fn drag_key(ui: &mut Ui, ev: &KeyEvent) -> bool {
    if input::is_escape(ev) {
        ui.drag = None;
        return true;
    }
    folder::is_new_session_key(ev)
        || folder::is_side_session_key(ev)
        || tiles::tile_step(ev).is_some()
        || tiles::move_step(ev).is_some()
        || tiles::stack_step(ev).is_some()
        || tiles::is_close_key(ev)
}

/// A picker's list or a menu is open. It holds the keyboard until it shuts: Ctrl+Shift+N, F6 and
/// Ctrl+W wait.
fn list_open(ui: &Ui) -> bool {
    ui.side_menu.is_some()
        || ui.folder_menu.is_some()
        || ui.views.values().any(|view| view.open_picker.is_some())
}

#[cfg(test)]
mod tests {
    use mr_agent::{
        Question, QuestionOption, RequestId, SessionEvent, ToolCallId, ToolDetail, ToolOutcome,
        UserRequest, UserRequestKind,
    };
    use mr_core::{ItemId, Place, SessionKey};
    use rune::{Key, Runtime};

    use super::conversation;
    use crate::glue::{self, Picker, Ui};
    use crate::testkit;

    /// What the input box's field shows.
    fn input_text(rt: &Runtime<Ui>) -> String {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.path.ends_with("mr-input/scroll/input"))
            .and_then(|e| e.text)
            .unwrap_or_default()
    }

    /// A finished reply `text` in `key`'s session.
    fn reply(rt: &mut Runtime<Ui>, key: SessionKey, text: &str) {
        testkit::emit_to(rt, key, SessionEvent::TextDelta(text.into()));
        testkit::emit_to(rt, key, SessionEvent::BlockFinished);
    }

    /// The id of the last item of `key`'s session.
    fn last_id(rt: &Runtime<Ui>, key: SessionKey) -> ItemId {
        let entry = rt.state.app.sessions.get(key).unwrap();
        entry.session.conversation.items().last().unwrap().id
    }

    #[test]
    fn each_session_keeps_its_own_draft() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        testkit::type_text(&mut rt, "abc");
        assert_eq!(input_text(&rt), "abc");
        testkit::show(&mut rt, b);
        assert_eq!(input_text(&rt), "", "the box is empty");
        testkit::type_text(&mut rt, "xyz");
        testkit::show(&mut rt, a);
        assert_eq!(input_text(&rt), "abc");
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "abc");
        assert_eq!(glue::view_of(&rt.state, b).draft.text, "xyz");
    }

    /// Whether a visible text of the conversation on screen contains `part`. The sidebar's rows
    /// show every session's title, which may be its first message.
    fn in_conversation(rt: &Runtime<Ui>, part: &str) -> bool {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.visible[2] > 0.0 && e.visible[3] > 0.0)
            .filter(|e| e.path.contains("/mr-conversation-"))
            .filter_map(|e| e.text)
            .any(|t| t.contains(part))
    }

    #[test]
    fn switching_never_shows_the_other_sessions_rows() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        reply(&mut rt, a, "The reply in a");
        reply(&mut rt, b, "The reply in b");
        assert_eq!(last_id(&rt, a), last_id(&rt, b), "the same item id in both");
        // Twice, so the second round meets the rows the first one memoised.
        for _ in 0..2 {
            testkit::show(&mut rt, b);
            assert!(testkit::has_text(&rt, "The reply in b"));
            assert!(!testkit::has_text(&rt, "The reply in a"));
            assert!(in_conversation(&rt, "Open /home/alex/b"));
            assert!(!in_conversation(&rt, "Open /home/alex/a"));
            testkit::show(&mut rt, a);
            assert!(testkit::has_text(&rt, "The reply in a"));
            assert!(!testkit::has_text(&rt, "The reply in b"));
            assert!(!in_conversation(&rt, "Open /home/alex/b"));
        }
    }

    /// A finished `ls` in `key`'s session.
    fn listing(rt: &mut Runtime<Ui>, key: SessionKey) {
        testkit::emit_to(
            rt,
            key,
            SessionEvent::ToolStarted {
                id: ToolCallId("c1".into()),
                name: "Bash".into(),
                detail: ToolDetail::Command {
                    command: "ls".into(),
                },
            },
        );
        testkit::emit_to(
            rt,
            key,
            SessionEvent::ToolFinished {
                id: ToolCallId("c1".into()),
                outcome: ToolOutcome::Ok {
                    output: "notes.txt\n".into(),
                },
            },
        );
    }

    #[test]
    fn opened_rows_belong_to_their_session() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        listing(&mut rt, a);
        listing(&mut rt, b);
        let id = last_id(&rt, a);
        assert_eq!(id, last_id(&rt, b));
        let details = format!("mr-details-{}", id.0);
        testkit::show(&mut rt, a);
        testkit::click(&mut rt, &format!("mr-toggle-{}", id.0));
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(rt.find(&details).is_some(), "opened in a");
        testkit::show(&mut rt, b);
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(rt.find(&details).is_none(), "closed in b");
        assert!(glue::view_of(&rt.state, b).toggled.is_empty());
        testkit::show(&mut rt, a);
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(rt.find(&details).is_some(), "still open in a");
    }

    /// Claude asks `key`'s session which colour, as request q1.
    fn ask_colour(rt: &mut Runtime<Ui>, key: SessionKey) {
        let option = |label: &str| QuestionOption {
            label: label.into(),
            description: None,
        };
        testkit::emit_to(
            rt,
            key,
            SessionEvent::NeedsUser(UserRequest {
                id: RequestId("q1".into()),
                kind: UserRequestKind::Question {
                    questions: vec![Question {
                        header: None,
                        prompt: "Which colour?".into(),
                        options: vec![option("Red"), option("Blue")],
                        multi_select: false,
                    }],
                },
            }),
        );
    }

    #[test]
    fn a_question_draft_belongs_to_its_session() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        ask_colour(&mut rt, a);
        ask_colour(&mut rt, b);
        let q1 = RequestId("q1".into());
        testkit::show(&mut rt, a);
        testkit::click(&mut rt, "mr-option-q1-0-0");
        assert!(rt.find("mr-chosen-q1-0-0").is_some(), "Red, in a");
        testkit::show(&mut rt, b);
        assert!(rt.find("mr-chosen-q1-0-0").is_none(), "nothing chosen in b");
        // An event for b goes through the glue while a's request still waits, hidden.
        reply(&mut rt, b, "Take your time");
        let kept = &glue::view_of(&rt.state, a).answers[&q1];
        assert_eq!(kept[0].chosen, vec!["Red".to_string()], "a's draft is kept");
        testkit::show(&mut rt, a);
        assert!(rt.find("mr-chosen-q1-0-0").is_some(), "Red, in a again");
    }

    #[test]
    fn events_for_a_hidden_session_do_not_draw() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        reply(&mut rt, a, "The reply in a");
        listing(&mut rt, a);
        rt.settle_all();
        testkit::step(&mut rt);
        let built = conversation::row_builds(a);
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        testkit::emit_to(&mut rt, b, SessionEvent::TextDelta("Streaming in b".into()));
        testkit::emit_to(&mut rt, b, SessionEvent::TextDelta(", and on".into()));
        assert!(!testkit::has_text(&rt, "Streaming in b"));
        assert!(testkit::has_text(&rt, "The reply in a"));
        assert_eq!(
            conversation::row_builds(a),
            built,
            "a's rows are not built again"
        );
    }

    #[test]
    fn a_session_shown_again_keeps_its_place_and_a_new_one_starts_at_its_end() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        for n in 1..=200 {
            let delta = SessionEvent::TextDelta(format!("Line {n} of a"));
            crate::demo::emit_to(&rt.state, a, delta);
            crate::demo::emit_to(&rt.state, a, SessionEvent::BlockFinished);
        }
        testkit::step(&mut rt);
        rt.settle_all();
        testkit::step(&mut rt);
        let scroll = format!("mr-conversation-{}", a.0);
        assert_eq!(rt.scroll_at_bottom(&scroll), Some(true));
        let (x, y) = rt.find(&scroll).unwrap();
        for _ in 0..5 {
            rt.wheel(x, y, 600.0);
            testkit::step(&mut rt);
        }
        assert_eq!(rt.scroll_at_bottom(&scroll), Some(false));
        let seen: Vec<String> = testkit::texts(&rt)
            .into_iter()
            .filter(|t| t.ends_with(" of a"))
            .collect();
        assert!(!seen.is_empty() && !seen.contains(&"Line 200 of a".to_string()));
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        reply(&mut rt, b, "The reply in b");
        let b_scroll = format!("mr-conversation-{}", b.0);
        assert_eq!(
            rt.scroll_at_bottom(&b_scroll),
            Some(true),
            "b starts at its end"
        );
        testkit::show(&mut rt, a);
        testkit::step(&mut rt);
        assert_eq!(
            rt.scroll_at_bottom(&scroll),
            Some(false),
            "a keeps its place"
        );
        assert!(rt.find("mr-jump-latest").is_some());
        let first = &seen[0];
        assert!(testkit::has_text(&rt, first), "{first} is on screen again");
        assert!(!testkit::has_text(&rt, "Line 200 of a"));
    }

    #[test]
    fn a_session_shown_without_focus_leaves_the_focus_where_it_is() {
        let input_focused = |rt: &Runtime<Ui>| {
            rt.elements()
                .iter()
                .any(|e| e.focused && e.path.contains("/mr-input/"))
        };
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        assert!(
            glue::view_of(&rt.state, a).draft.focused,
            "a new session's box"
        );
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        assert!(input_focused(&rt));
        assert!(
            !glue::view_of(&rt.state, a).draft.focused,
            "a, left, is not"
        );
        // The focus goes elsewhere, as to the sidebar's list.
        rt.update(|_, tx| tx.blur());
        testkit::step(&mut rt);
        assert!(!input_focused(&rt));
        rt.update(|ui, tx| glue::show(ui, tx, a, Place::Focused, false));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(a));
        assert!(!input_focused(&rt));
        assert!(!glue::view_of(&rt.state, a).draft.focused);
        testkit::show(&mut rt, b);
        assert!(input_focused(&rt), "shown by a click");
        assert!(glue::view_of(&rt.state, b).draft.focused);
    }

    #[test]
    fn ctrl_shift_n_asks_for_a_session_to_the_side() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir(&notes).unwrap();
        let mut rt = testkit::window_sized("conversation", (1800.0, 900.0));
        let old = testkit::focused(&rt).expect("the scene's session");
        rt.state.app.recent_folders = vec![notes.clone()];
        let ctrl_shift_n = |rt: &mut Runtime<Ui>| {
            testkit::press_with(rt, Key::Command('n'), testkit::ctrl_shift());
            testkit::step(rt);
        };
        // While a picker's list is open it waits, as F6 and Ctrl+W do.
        testkit::shown_view_mut(&mut rt).open_picker = Some(Picker::Mode);
        rt.invalidate();
        testkit::step(&mut rt);
        ctrl_shift_n(&mut rt);
        assert_eq!(rt.state.folder_menu, None);
        assert!(!rt.state.folder_side);
        testkit::shown_view_mut(&mut rt).open_picker = None;
        rt.invalidate();
        testkit::step(&mut rt);
        // From the input box, where the user types.
        testkit::click(&mut rt, &testkit::tile(old, "mr-input"));
        assert!(glue::view_of(&rt.state, old).draft.focused);
        ctrl_shift_n(&mut rt);
        assert_eq!(rt.state.folder_menu, Some(None), "the folder menu");
        assert!(rt.state.folder_side, "for a session to the side");
        testkit::click(&mut rt, "mr-folder-item0");
        testkit::step(&mut rt);
        let new = testkit::focused(&rt).expect("the new session");
        assert_ne!(new, old);
        assert_eq!(testkit::tile_keys(&rt), [old, new]);
        assert!(!rt.state.folder_side);
        assert!(glue::view_of(&rt.state, new).draft.focused, "its box");
        assert!(
            !glue::view_of(&rt.state, old).draft.focused,
            "not the old one's"
        );
    }
}
