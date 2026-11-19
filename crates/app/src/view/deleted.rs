//! Deleting: the questions asked before a session that would be cut off or a whole group goes
//! to the Deleted view, and before "Delete All" removes everything there for good
//! ([`glue::ask_delete_session`], [`glue::ask_delete_group`], [`glue::ask_delete_all`]), and
//! how long a deleted session has left.

use mr_core::Deleting;
use rune::{El, Tx, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit};

use crate::glue::{self, DeleteAsk, Ui};

/// What the question about `ask` says: its title, its text and its button. The title names
/// what it is about: the session by the title its row shows, the group by its name. A session is
/// asked about only while deleting it would lose something ([`glue::delete_asks`]).
pub fn words(ui: &Ui, ask: &DeleteAsk) -> (String, String, &'static str) {
    match ask {
        DeleteAsk::Session(key) => {
            let name = ui.app.title_of(*key).unwrap_or_default();
            (
                format!("Delete \"{name}\"?"),
                session_text(ui.app.deleting(*key)).into(),
                "Delete",
            )
        }
        DeleteAsk::Group(id) => {
            let keys = ui.app.sessions_of(id);
            (
                format!("Delete \"{}\"?", ui.app.groups.name(id)),
                group_text(keys.len(), ui.app.in_flight(&keys)),
                "Delete",
            )
        }
        DeleteAsk::All => (
            "Delete everything for good?".into(),
            all_text(ui.app.sidebar.deleted),
            "Delete All",
        ),
    }
}

/// What deleting a session does to it, as the core says it would (`App::deleting`). Only what
/// is true of it is promised: one the store has nothing of cannot be recovered. A session whose
/// turn ended while the question showed has nothing left to lose but can still be recovered.
fn session_text(deleting: Option<Deleting>) -> &'static str {
    match deleting {
        Some(Deleting::Stops) => {
            "It is still working. Deleting stops it. It can be recovered for 7 days."
        }
        Some(Deleting::DropsMessage) => {
            "A message in it has not been sent and will be dropped. It can be recovered for 7 \
             days."
        }
        Some(Deleting::Forgotten { .. }) => "It was never sent and will not be kept.",
        Some(Deleting::Kept) | None => "It can be recovered for 7 days.",
    }
}

/// What deleting a group with `sessions` sessions, `working` of them in flight, takes with it.
fn group_text(sessions: usize, working: usize) -> String {
    let goes = match sessions {
        0 => "It has no sessions.".to_string(),
        1 => "1 session goes with it.".to_string(),
        n => format!("{n} sessions go with it."),
    };
    let stops = match working {
        0 => String::new(),
        1 => " 1 of them is still working and will be stopped.".to_string(),
        n => format!(" {n} of them are still working and will be stopped."),
    };
    let back = if sessions == 0 { "It" } else { "They" };
    format!("{goes}{stops} {back} can be recovered for 7 days.")
}

/// What "Delete All" does to `sessions` sessions.
fn all_text(sessions: usize) -> String {
    let what = match sessions {
        1 => "1 session".to_string(),
        n => format!("{n} sessions"),
    };
    format!("{what} will be removed from this computer. This cannot be undone.")
}

/// How long a deleted session has left before it is removed for good, `days_left` being whole
/// days, 0 on its last.
pub fn days(days_left: u8) -> String {
    match days_left {
        0 => "today".into(),
        1 => "1 day left".into(),
        n => format!("{n} days left"),
    }
}

/// The question about deleting, while one shows: the kit's dialog, as the question whether to
/// quit is drawn (`view::quit`), with "Cancel", which Return presses, and the button that
/// deletes. Its default is the answer that deletes nothing. Escape, a press outside, its close
/// button and "Cancel" put it away; the button that deletes does what it asks and puts it away.
pub fn question(ui: &Ui, kit: &Kit, layer: &mut Vec<El>) {
    let Some(ask) = &ui.delete_ask else {
        return;
    };
    let (title, said, button) = words(ui, ask);
    let said = text(said)
        .key("mr-delete-words")
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
        .key("mr-delete-cancel")
        .default_button();
    let delete_button = Button::new(button)
        .variant(ButtonVariant::Danger)
        .on_press_tx(|ui: &mut Ui, tx: &mut Tx| {
            if let Some(ask) = ui.delete_ask.clone() {
                glue::delete(ui, tx, ask);
            }
        })
        .build(kit)
        .key("mr-delete-yes");
    let buttons = row()
        .key("mr-delete-buttons")
        .w_full()
        .justify_end()
        .gap(8.0)
        .px(16.0)
        .pb(16.0)
        .child(cancel_button)
        .child(delete_button);
    Dialog::new("mr-delete", title)
        .child(said)
        .child(buttons)
        .fit()
        .open(true)
        .on_close(cancel)
        .build(kit, layer);
}

/// Puts the question away; nothing is deleted.
fn cancel(ui: &mut Ui) {
    ui.delete_ask = None;
}

#[cfg(test)]
mod tests {
    use mr_agent::{SessionCommand, SessionId};
    use mr_core::{
        GroupId, Leaving, Outgoing, Phase, Place, Prompt, Remote, SessionKey, SidebarView,
        Undelivered,
    };
    use rune::{Key, KeyEvent, Mods, Runtime};

    use super::{all_text, days, group_text, words};
    use crate::demo;
    use crate::glue::{self, DeleteAsk, MenuTarget, Ui};
    use crate::testkit::{
        self, CONTACT, FINISHED, GARDEN, NEEDS_YOU, WORKING, box_focused, group_named, tile_keys,
        titled,
    };
    use crate::{theme, view};

    const MENU: &str = "mr-side-menu-menu";
    const DIALOG: &str = "mr-delete-dialog";
    /// A row's "Delete": after "Move to group", "Open to the side", "Open below", "Rename" and
    /// the separator.
    const DELETE_ITEM: &str = "mr-side-menu-item5";
    /// A custom group's "Delete group": after "New session", "Rename", "Hide group", "Ungroup"
    /// and the separator.
    const DELETE_GROUP_ITEM: &str = "mr-side-menu-item5";
    const WIDE: (f32, f32) = (1800.0, 900.0);

    #[test]
    fn the_words_say_what_goes() {
        let rest = " They can be recovered for 7 days.";
        assert_eq!(
            group_text(0, 0),
            "It has no sessions. It can be recovered for 7 days."
        );
        assert_eq!(group_text(1, 0), format!("1 session goes with it.{rest}"));
        assert_eq!(
            group_text(1, 1),
            format!(
                "1 session goes with it. 1 of them is still working and will be stopped.{rest}"
            )
        );
        assert_eq!(group_text(3, 0), format!("3 sessions go with it.{rest}"));
        assert_eq!(
            group_text(3, 1),
            format!("3 sessions go with it. 1 of them is still working and will be stopped.{rest}")
        );
        assert_eq!(
            group_text(3, 2),
            format!(
                "3 sessions go with it. 2 of them are still working and will be stopped.{rest}"
            )
        );
        assert_eq!(
            all_text(1),
            "1 session will be removed from this computer. This cannot be undone."
        );
        assert_eq!(
            all_text(4),
            "4 sessions will be removed from this computer. This cannot be undone."
        );
        // As the questions put them, each naming what it is about.
        let mut rt = testkit::window("sessions");
        let working = titled(&rt, WORKING);
        assert_eq!(
            words(&rt.state, &DeleteAsk::Session(working)),
            (
                format!("Delete \"{WORKING}\"?"),
                "It is still working. Deleting stops it. It can be recovered for 7 days."
                    .to_string(),
                "Delete"
            )
        );
        let side = group_named(&rt, "Side projects");
        assert_eq!(
            words(&rt.state, &DeleteAsk::Group(side)),
            (
                "Delete \"Side projects\"?".to_string(),
                format!("1 session goes with it.{rest}"),
                "Delete"
            )
        );
        let contact = titled(&rt, CONTACT);
        delete_now(&mut rt, contact);
        assert_eq!(
            words(&rt.state, &DeleteAsk::All),
            (
                "Delete everything for good?".to_string(),
                all_text(1),
                "Delete All"
            )
        );
    }

    /// What the question about deleting `key`'s session says under its title.
    fn said_of(rt: &Runtime<Ui>, key: SessionKey) -> String {
        words(&rt.state, &DeleteAsk::Session(key)).1
    }

    #[test]
    fn the_session_question_says_what_is_true_of_it() {
        const STOPS: &str =
            "It is still working. Deleting stops it. It can be recovered for 7 days.";
        const DROPS: &str = "A message in it has not been sent and will be dropped. It can be \
                             recovered for 7 days.";
        const NEVER_SENT: &str = "It was never sent and will not be kept.";
        let mut rt = testkit::window("sessions");

        // Its program would be stopped: a turn runs, a request waits, a background task runs,
        // Remote Control is on.
        assert_eq!(said_of(&rt, titled(&rt, WORKING)), STOPS);
        assert_eq!(said_of(&rt, titled(&rt, NEEDS_YOU)), STOPS);
        let contact = titled(&rt, CONTACT);
        let change = |rt: &mut Runtime<Ui>, change: &dyn Fn(&mut mr_core::Entry)| {
            change(&mut rt.state.app.sessions.get_mut(contact).unwrap());
        };
        change(&mut rt, &|entry| entry.session.background = 1);
        assert_eq!(said_of(&rt, contact), STOPS, "a background task");
        change(&mut rt, &|entry| {
            entry.session.background = 0;
            entry.session.remote = Remote::On { url: None };
        });
        assert_eq!(said_of(&rt, contact), STOPS, "Remote Control");
        change(&mut rt, &|entry| entry.session.remote = Remote::Off);

        // A stored session at rest that holds a message not sent yet, and nothing else.
        change(&mut rt, &|entry| {
            entry.prompt = Some(Prompt::OpenElsewhere {
                text: "carry on".into(),
            });
        });
        assert_eq!(said_of(&rt, contact), DROPS, "held by the question");
        assert_eq!(
            words(&rt.state, &DeleteAsk::Session(contact)).0,
            format!("Delete \"{CONTACT}\"?")
        );
        rt.update(|ui, tx| glue::ask_delete_session(ui, tx, contact));
        testkit::step(&mut rt);
        assert!(asks(&rt), "it asks first");
        assert!(testkit::texts(&rt).contains(&DROPS.to_string()));
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        change(&mut rt, &|entry| {
            entry.prompt = None;
            entry.session.queued = Some(Outgoing {
                text: "carry on".into(),
                chips: Vec::new(),
            });
        });
        assert_eq!(said_of(&rt, contact), DROPS, "queued");
        change(&mut rt, &|entry| {
            entry.session.queued = None;
            entry.session.undelivered = vec![Undelivered {
                text: "carry on".into(),
                attachments: Vec::new(),
            }];
        });
        assert_eq!(said_of(&rt, contact), DROPS, "never delivered");
        change(&mut rt, &|entry| entry.session.undelivered.clear());

        // A session the store has nothing of would be forgotten: nothing can be recovered.
        let ui = &mut rt.state;
        let effects = ui
            .app
            .new_session("/home/alex/notes".into(), Place::Focused, Leaving::Keep);
        glue::run(ui, effects);
        let fresh = ui.app.focused().unwrap();
        rt.invalidate();
        testkit::step(&mut rt);
        assert_eq!(said_of(&rt, fresh), NEVER_SENT);
        assert_eq!(
            words(&rt.state, &DeleteAsk::Session(fresh)).0,
            "Delete \"New session\"?"
        );
    }

    /// A new session the store has nothing of, in the focused tile of a wide window.
    fn never_sent() -> (Runtime<Ui>, SessionKey) {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let ui = &mut rt.state;
        let effects = ui.app.new_session(
            "/home/alex/notes".into(),
            Place::Side,
            Leaving::ForgetIfEmpty,
        );
        glue::run(ui, effects);
        let fresh = ui.app.focused().unwrap();
        rt.invalidate();
        testkit::step(&mut rt);
        (rt, fresh)
    }

    #[test]
    fn delete_on_a_session_never_sent_asks_when_something_is_in_it() {
        const NEVER_SENT: &str = "It was never sent and will not be kept.";
        // A draft typed in its box.
        let (mut rt, fresh) = never_sent();
        assert!(box_focused(&rt, fresh));
        testkit::type_text(&mut rt, "half a thought");
        delete_from_menu(&mut rt, fresh);
        assert!(asks(&rt), "the draft would be lost");
        assert!(testkit::texts(&rt).contains(&NEVER_SENT.to_string()));
        assert!(testkit::has_text(&rt, "Delete \"New session\"?"));
        assert!(rt.state.app.sessions.get(fresh).is_some(), "not yet");
        testkit::click(&mut rt, "mr-delete-yes");
        testkit::step(&mut rt);
        assert!(rt.state.app.sessions.get(fresh).is_none(), "forgotten");
        assert_eq!(rt.state.app.sidebar.deleted, 0, "nothing to recover");

        // A chip.
        let (mut rt, fresh) = never_sent();
        testkit::paste_image(&mut rt, testkit::tiny_png());
        testkit::step(&mut rt);
        let chips = &rt.state.app.sessions.get(fresh).unwrap().session.chips;
        assert_eq!(chips.len(), 1);
        rt.update(|ui, tx| glue::ask_delete_session(ui, tx, fresh));
        testkit::step(&mut rt);
        assert!(asks(&rt), "the chip would be lost");
        assert!(testkit::texts(&rt).contains(&NEVER_SENT.to_string()));

        // A first message still queued.
        let (mut rt, fresh) = never_sent();
        rt.state.app.sessions.get_mut(fresh).unwrap().session.queued = Some(Outgoing {
            text: "early".into(),
            chips: Vec::new(),
        });
        rt.update(|ui, tx| glue::ask_delete_session(ui, tx, fresh));
        testkit::step(&mut rt);
        assert!(asks(&rt), "the message would be lost");
        assert!(testkit::texts(&rt).contains(&NEVER_SENT.to_string()));
    }

    #[test]
    fn a_delete_that_closes_no_tile_lets_the_question_fade() {
        let leaving = |rt: &Runtime<Ui>| {
            rt.elements()
                .iter()
                .any(|e| e.exiting && e.key.as_deref() == Some(DIALOG))
        };
        // "Delete All" closes no tile: the question goes as a question does.
        let mut rt = testkit::window("sessions");
        let contact = titled(&rt, CONTACT);
        delete_now(&mut rt, contact);
        show_deleted(&mut rt);
        testkit::click(&mut rt, "mr-delete-all");
        testkit::step(&mut rt);
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(asks(&rt));
        testkit::click(&mut rt, "mr-delete-yes");
        assert_eq!(rt.state.delete_ask, None);
        assert!(leaving(&rt), "it fades out");

        // A delete that closes a tile is at once, so the tiles take their widths in one frame.
        let (mut rt, working) = asking();
        rt.settle_all();
        testkit::step(&mut rt);
        testkit::click(&mut rt, "mr-delete-yes");
        assert!(is_deleted(&rt, working));
        assert!(!leaving(&rt), "gone at once");
    }

    #[test]
    fn the_days_are_said_plainly() {
        assert_eq!(days(6), "6 days left");
        assert_eq!(days(2), "2 days left");
        assert_eq!(days(1), "1 day left");
        assert_eq!(days(0), "today");
    }

    /// A right-click on the centre of the element keyed `key`, then a step. Rows that moved are
    /// landed first.
    fn right_click(rt: &mut Runtime<Ui>, key: &str) {
        rt.settle_all();
        testkit::step(rt);
        let Some((x, y)) = rt.find(key) else {
            panic!("no element keyed {key:?}");
        };
        rt.pointer_move(x, y);
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        testkit::step(rt);
    }

    /// The texts inside the element keyed `key`.
    fn texts_in(rt: &Runtime<Ui>, key: &str) -> Vec<String> {
        let inside = format!("/{key}/");
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains(&inside))
            .filter_map(|e| e.text)
            .collect()
    }

    /// The texts the sidebar's list shows, top to bottom.
    fn list_texts(rt: &Runtime<Ui>) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.visible[2] > 0.0 && e.visible[3] > 0.0)
            .filter(|e| e.path.contains("/mr-sidebar-rows/"))
            .filter_map(|e| e.text)
            .collect()
    }

    fn row(key: SessionKey) -> String {
        testkit::row_key(key)
    }

    fn header(id: &GroupId) -> String {
        format!("mr-group-{}", id.key_text())
    }

    /// Whether a question about deleting shows.
    fn asks(rt: &Runtime<Ui>) -> bool {
        testkit::drawn(rt, DIALOG)
    }

    fn is_deleted(rt: &Runtime<Ui>, key: SessionKey) -> bool {
        rt.state
            .app
            .sessions
            .get(key)
            .is_some_and(|entry| entry.deleted.is_some())
    }

    /// The id of `key`'s session.
    fn id_of(rt: &Runtime<Ui>, key: SessionKey) -> SessionId {
        let entry = rt.state.app.sessions.get(key).unwrap();
        entry.session.id.clone().expect("a stored session")
    }

    /// How many times `id`'s session was sent `Close`.
    fn closes(rt: &Runtime<Ui>, id: &SessionId) -> usize {
        let fake = rt.state.fake.as_ref().unwrap();
        fake.sent()
            .iter()
            .filter(|(to, command)| to == id && *command == SessionCommand::Close)
            .count()
    }

    /// Chooses "Delete" in `key`'s row's menu.
    fn delete_from_menu(rt: &mut Runtime<Ui>, key: SessionKey) {
        right_click(rt, &row(key));
        assert_eq!(
            texts_in(rt, MENU).last().map(String::as_str),
            Some("Delete")
        );
        testkit::click(rt, DELETE_ITEM);
        testkit::step(rt);
    }

    /// Deletes `key`'s session as "Delete" in its row's menu does, then steps.
    fn delete_now(rt: &mut Runtime<Ui>, key: SessionKey) {
        rt.update(|ui, tx| glue::ask_delete_session(ui, tx, key));
        testkit::step(rt);
        assert!(is_deleted(rt, key), "deleted at once");
    }

    /// Switches the sidebar to the Deleted view with its switch.
    fn show_deleted(rt: &mut Runtime<Ui>) {
        testkit::click(rt, "mr-view-deleted");
        testkit::step(rt);
        assert_eq!(rt.state.app.sidebar_view(), SidebarView::Deleted);
    }

    /// Whether the element keyed `key` can be pressed: a disabled one takes no focus.
    fn enabled(rt: &Runtime<Ui>, key: &str) -> bool {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no element keyed {key:?}"))
            .focusable
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

    /// The states assistive technology is told of the element keyed `key`.
    fn states(rt: &Runtime<Ui>, key: &str) -> Vec<String> {
        let tree = rt.accessibility_json();
        let node = find(&tree, key).unwrap_or_else(|| panic!("{key} is in the tree"));
        node["states"].as_array().map_or(Vec::new(), |states| {
            states
                .iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
    }

    /// Pumps the demo's fake until `done` holds, then draws.
    fn pump_until(rt: &mut Runtime<Ui>, done: impl Fn(&Ui) -> bool) {
        for _ in 0..50 {
            if done(&rt.state) {
                break;
            }
            demo::pump(&mut rt.state, 0.0);
        }
        assert!(done(&rt.state), "never got there");
        rt.invalidate();
        testkit::step(rt);
    }

    #[test]
    fn delete_on_a_resting_row_moves_it_to_the_deleted_view_at_once() {
        let mut rt = testkit::window("sessions");
        let contact = titled(&rt, CONTACT);
        assert!(list_texts(&rt).contains(&CONTACT.to_string()));
        assert_eq!(texts_in(&rt, "mr-view-deleted"), ["Deleted"]);
        delete_from_menu(&mut rt, contact);
        assert!(rt.state.side_menu.is_none());
        assert!(!asks(&rt), "no question");
        assert_eq!(rt.state.delete_ask, None);
        assert!(is_deleted(&rt, contact));
        assert!(!list_texts(&rt).contains(&CONTACT.to_string()));
        assert_eq!(texts_in(&rt, "mr-view-deleted"), ["Deleted (1)"]);
        assert_eq!(rt.state.app.sidebar_view(), SidebarView::Sessions);
    }

    #[test]
    fn delete_on_a_working_row_asks_and_cancel_keeps_it() {
        let mut rt = testkit::window("sessions");
        let working = titled(&rt, WORKING);
        let id = id_of(&rt, working);
        delete_from_menu(&mut rt, working);
        assert!(asks(&rt));
        assert_eq!(rt.state.delete_ask, Some(DeleteAsk::Session(working)));
        assert!(testkit::has_text(&rt, &format!("Delete \"{WORKING}\"?")));
        assert_eq!(
            texts_in(&rt, DIALOG)
                .into_iter()
                .filter(|t| t.starts_with("It is"))
                .collect::<Vec<_>>(),
            ["It is still working. Deleting stops it. It can be recovered for 7 days."]
        );
        assert_eq!(texts_in(&rt, "mr-delete-cancel"), ["Cancel"]);
        assert_eq!(texts_in(&rt, "mr-delete-yes"), ["Delete"]);
        // Return presses "Cancel".
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert!(!asks(&rt));
        assert_eq!(rt.state.delete_ask, None);
        assert!(!is_deleted(&rt, working));
        assert_eq!(closes(&rt, &id), 0);
        let entry = rt.state.app.sessions.get(working).unwrap();
        assert_eq!(entry.session.phase, Phase::Working, "it still works");
        assert!(list_texts(&rt).contains(&WORKING.to_string()));
    }

    #[test]
    fn delete_in_the_question_stops_and_deletes_it() {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let garden = titled(&rt, GARDEN);
        let working = titled(&rt, WORKING);
        let id = id_of(&rt, working);
        testkit::show_side(&mut rt, working);
        assert_eq!(tile_keys(&rt), [garden, working]);
        assert!(box_focused(&rt, working));
        delete_from_menu(&mut rt, working);
        assert!(asks(&rt));
        testkit::click(&mut rt, "mr-delete-yes");
        testkit::step(&mut rt);
        assert!(!asks(&rt));
        assert_eq!(rt.state.delete_ask, None);
        assert!(is_deleted(&rt, working));
        assert_eq!(closes(&rt, &id), 1, "its program was stopped");
        assert_eq!(tile_keys(&rt), [garden], "its tile closed");
        // The keyboard goes where closing the tile sends it.
        assert_eq!(testkit::focused(&rt), Some(garden));
        assert!(box_focused(&rt, garden));
        assert!(!list_texts(&rt).contains(&WORKING.to_string()));
        assert_eq!(texts_in(&rt, "mr-view-deleted"), ["Deleted (1)"]);
    }

    /// A group "Errands" holding the finished session, the contact form's and the working one.
    fn errands(rt: &mut Runtime<Ui>) -> GroupId {
        let keys = [
            titled(rt, FINISHED),
            titled(rt, CONTACT),
            titled(rt, WORKING),
        ];
        let ui = &mut rt.state;
        let (id, effects) = ui.app.new_group("Errands");
        glue::run(ui, effects);
        for key in keys {
            let effects = ui.app.move_session(key, &id);
            glue::run(ui, effects);
        }
        rt.invalidate();
        testkit::step(rt);
        id
    }

    #[test]
    fn delete_group_asks_with_the_count() {
        let mut rt = testkit::window("sessions");
        let id = errands(&mut rt);
        let keys = rt.state.app.sessions_of(&id);
        assert_eq!(keys.len(), 3);
        right_click(&mut rt, &header(&id));
        assert_eq!(
            texts_in(&rt, MENU),
            [
                "New session",
                "Rename",
                "Hide group",
                "Ungroup",
                "Delete group"
            ]
        );
        let kit = theme::kit(true);
        let danger = rt
            .elements()
            .into_iter()
            .find(|e| e.path.contains(&format!("/{DELETE_GROUP_ITEM}/")) && e.text.is_some())
            .and_then(|e| e.text_color);
        assert_eq!(danger, Some(kit.c.danger.fg), "in the danger colour");
        testkit::click(&mut rt, DELETE_GROUP_ITEM);
        testkit::step(&mut rt);
        assert!(asks(&rt), "a group always asks");
        assert!(testkit::has_text(&rt, "Delete \"Errands\"?"));
        assert!(
            testkit::texts(&rt).contains(
                &"3 sessions go with it. 1 of them is still working and will be stopped. They can \
              be recovered for 7 days."
                    .to_string()
            )
        );
        assert!(keys.iter().all(|key| !is_deleted(&rt, *key)), "not yet");
        testkit::click(&mut rt, "mr-delete-yes");
        testkit::step(&mut rt);
        assert!(!asks(&rt));
        assert!(keys.iter().all(|key| is_deleted(&rt, *key)));
        assert!(!list_texts(&rt).contains(&"Errands".to_string()));
        assert_eq!(texts_in(&rt, "mr-view-deleted"), ["Deleted (3)"]);
        show_deleted(&mut rt);
        let texts = list_texts(&rt);
        assert_eq!(
            texts.first().map(String::as_str),
            Some("Errands"),
            "{texts:?}"
        );
        for title in [FINISHED, CONTACT, WORKING] {
            assert!(texts.contains(&title.to_string()), "{title}");
        }
    }

    #[test]
    fn ungroup_is_offered_only_for_a_group_you_made() {
        let mut rt = testkit::window("sessions");
        let garden = titled(&rt, GARDEN);
        // A folder's group: no "Ungroup".
        right_click(&mut rt, &header(&GroupId::Folder(demo::RUNE_CLAUDE.into())));
        assert_eq!(
            texts_in(&rt, MENU),
            ["New session", "Rename", "Hide group", "Delete group"]
        );
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        // A group the user made: "Ungroup" sends its sessions home and deletes nothing.
        let side = group_named(&rt, "Side projects");
        right_click(&mut rt, &header(&side));
        assert_eq!(
            texts_in(&rt, MENU),
            [
                "New session",
                "Rename",
                "Hide group",
                "Ungroup",
                "Delete group"
            ]
        );
        testkit::click(&mut rt, "mr-side-menu-item3");
        testkit::step(&mut rt);
        assert!(!asks(&rt), "no question");
        assert!(!list_texts(&rt).contains(&"Side projects".to_string()));
        assert_eq!(
            testkit::group_of(&rt, garden),
            GroupId::Folder("/home/alex/garden".into())
        );
        assert!(!is_deleted(&rt, garden));
        assert_eq!(rt.state.app.sidebar.deleted, 0);
    }

    #[test]
    fn the_switch_shows_the_deleted_view() {
        let mut rt = testkit::window("sessions");
        let contact = titled(&rt, CONTACT);
        assert!(testkit::drawn(&rt, "mr-view-switch"));
        assert!(testkit::drawn(&rt, "mr-new-session"));
        assert!(states(&rt, "mr-view-sessions").contains(&"checked".to_string()));
        assert!(states(&rt, "mr-view-deleted").contains(&"unchecked".to_string()));
        // Both are Tab stops.
        assert!(enabled(&rt, "mr-view-sessions") && enabled(&rt, "mr-view-deleted"));
        // Above "New session", on one line.
        let sessions = testkit::rect(&rt, "mr-view-sessions").unwrap();
        let deleted = testkit::rect(&rt, "mr-view-deleted").unwrap();
        let new_session = testkit::rect(&rt, "mr-new-session").unwrap();
        assert_eq!(sessions[1], deleted[1], "side by side");
        assert!(sessions[1] + sessions[3] <= new_session[1]);
        delete_now(&mut rt, contact);
        show_deleted(&mut rt);
        assert!(!testkit::drawn(&rt, "mr-new-session"));
        assert_eq!(
            texts_in(&rt, "mr-sidebar-header")
                .into_iter()
                .filter(|t| t.starts_with("Removed"))
                .collect::<Vec<_>>(),
            ["Removed for good after 7 days."]
        );
        assert!(testkit::drawn(&rt, "mr-deleted-note"));
        assert_eq!(texts_in(&rt, "mr-delete-all"), ["Delete All"]);
        assert!(enabled(&rt, "mr-delete-all"));
        assert_eq!(list_texts(&rt), ["website", CONTACT, "6 days left"]);
        assert_eq!(texts_in(&rt, "mr-view-deleted"), ["Deleted (1)"]);
        assert!(states(&rt, "mr-view-deleted").contains(&"checked".to_string()));
        assert!(states(&rt, "mr-view-sessions").contains(&"unchecked".to_string()));
        // No mark, no "+", not selected.
        assert!(!testkit::drawn(
            &rt,
            &crate::view::sidebar::add_key(&GroupId::Folder(testkit::WEBSITE.into()))
        ));
        // And back.
        testkit::click(&mut rt, "mr-view-sessions");
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.sidebar_view(), SidebarView::Sessions);
        assert!(testkit::drawn(&rt, "mr-new-session"));
        assert!(!testkit::drawn(&rt, "mr-delete-all"));
        assert!(!list_texts(&rt).contains(&CONTACT.to_string()));
    }

    #[test]
    fn the_deleted_view_says_when_it_is_empty() {
        let mut rt = testkit::window("sessions");
        show_deleted(&mut rt);
        assert!(testkit::drawn(&rt, "mr-deleted-empty"));
        assert!(testkit::has_text(&rt, "Nothing is deleted."));
        assert_eq!(texts_in(&rt, "mr-view-deleted"), ["Deleted"]);
        assert!(!enabled(&rt, "mr-delete-all"), "nothing to delete");
        assert!(states(&rt, "mr-delete-all").contains(&"disabled".to_string()));
        testkit::click(&mut rt, "mr-delete-all");
        testkit::step(&mut rt);
        assert!(!asks(&rt));
    }

    #[test]
    fn a_press_on_a_deleted_row_offers_recover() {
        let mut rt = testkit::window("sessions");
        let contact = titled(&rt, CONTACT);
        delete_now(&mut rt, contact);
        show_deleted(&mut rt);
        let offers = |rt: &mut Runtime<Ui>| {
            assert_eq!(
                rt.state.side_menu.as_ref().map(|m| m.target.clone()),
                Some(MenuTarget::Deleted(contact))
            );
            assert_eq!(texts_in(rt, MENU), ["Recover"]);
            // Under its row.
            rt.settle_all();
            testkit::step(rt);
            let line = testkit::rect(rt, &row(contact)).unwrap();
            let menu = testkit::rect(rt, MENU).unwrap();
            assert!(
                menu[1] >= line[1] + line[3] - 1.0,
                "{menu:?} under {line:?}"
            );
            testkit::press(rt, Key::Escape);
            testkit::step(rt);
            assert!(rt.state.side_menu.is_none());
        };
        testkit::click(&mut rt, &row(contact));
        offers(&mut rt);
        right_click(&mut rt, &row(contact));
        offers(&mut rt);
        rt.update(|_, tx| tx.focus(row(contact), true));
        testkit::step(&mut rt);
        testkit::press(&mut rt, Key::ContextMenu);
        testkit::step(&mut rt);
        offers(&mut rt);
        rt.update(|_, tx| tx.focus(row(contact), true));
        testkit::step(&mut rt);
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        offers(&mut rt);
        // Nothing was shown, nothing recovered.
        assert!(!tile_keys(&rt).contains(&contact));
        assert!(is_deleted(&rt, contact));
    }

    #[test]
    fn recover_puts_the_row_back() {
        let mut rt = testkit::window("sessions");
        let contact = titled(&rt, CONTACT);
        delete_now(&mut rt, contact);
        show_deleted(&mut rt);
        testkit::click(&mut rt, &row(contact));
        testkit::click(&mut rt, "mr-side-menu-item0");
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        assert!(!is_deleted(&rt, contact));
        assert_eq!(
            rt.state.app.sidebar_view(),
            SidebarView::Deleted,
            "the Deleted view still shows"
        );
        assert!(testkit::has_text(&rt, "Nothing is deleted."));
        assert_eq!(texts_in(&rt, "mr-view-deleted"), ["Deleted"]);
        testkit::click(&mut rt, "mr-view-sessions");
        testkit::step(&mut rt);
        let texts = list_texts(&rt);
        let at = texts.iter().position(|t| t == "website").unwrap();
        assert_eq!(texts[at + 1], CONTACT, "{texts:?}");
    }

    #[test]
    fn recover_group_puts_the_group_back() {
        let mut rt = testkit::window("sessions");
        let garden = titled(&rt, GARDEN);
        let side = group_named(&rt, "Side projects");
        right_click(&mut rt, &header(&side));
        testkit::click(&mut rt, DELETE_GROUP_ITEM);
        testkit::step(&mut rt);
        assert!(
            testkit::texts(&rt)
                .contains(&"1 session goes with it. They can be recovered for 7 days.".to_string())
        );
        testkit::click(&mut rt, "mr-delete-yes");
        testkit::step(&mut rt);
        assert!(is_deleted(&rt, garden));
        assert!(tile_keys(&rt).is_empty(), "its tile closed");
        assert!(!list_texts(&rt).contains(&"Side projects".to_string()));
        show_deleted(&mut rt);
        assert_eq!(list_texts(&rt), ["Side projects", GARDEN, "6 days left"]);
        testkit::click(&mut rt, &header(&side));
        assert_eq!(
            rt.state.side_menu.as_ref().map(|m| m.target.clone()),
            Some(MenuTarget::DeletedGroup(side.clone()))
        );
        assert_eq!(texts_in(&rt, MENU), ["Recover group"]);
        testkit::click(&mut rt, "mr-side-menu-item0");
        testkit::step(&mut rt);
        assert!(!is_deleted(&rt, garden));
        assert_eq!(rt.state.app.sidebar_view(), SidebarView::Deleted);
        assert!(testkit::has_text(&rt, "Nothing is deleted."));
        testkit::click(&mut rt, "mr-view-sessions");
        testkit::step(&mut rt);
        let texts = list_texts(&rt);
        let at = texts.iter().position(|t| t == "Side projects").unwrap();
        assert_eq!(texts[at + 1], GARDEN, "{texts:?}");
    }

    #[test]
    fn delete_all_asks_and_removes_for_good() {
        let mut rt = testkit::window("sessions");
        let contact = titled(&rt, CONTACT);
        let garden = titled(&rt, GARDEN);
        let ids = [id_of(&rt, contact), id_of(&rt, garden)];
        delete_now(&mut rt, contact);
        delete_now(&mut rt, garden);
        show_deleted(&mut rt);
        testkit::click(&mut rt, "mr-delete-all");
        testkit::step(&mut rt);
        assert!(asks(&rt));
        assert_eq!(rt.state.delete_ask, Some(DeleteAsk::All));
        assert!(testkit::has_text(&rt, "Delete everything for good?"));
        assert!(testkit::texts(&rt).contains(
            &"2 sessions will be removed from this computer. This cannot be undone.".to_string()
        ));
        assert_eq!(texts_in(&rt, "mr-delete-yes"), ["Delete All"]);
        let fake = rt.state.fake.clone().unwrap();
        assert!(fake.removed().is_empty(), "nothing yet");
        testkit::click(&mut rt, "mr-delete-yes");
        testkit::step(&mut rt);
        assert!(!asks(&rt));
        pump_until(&mut rt, |ui| ui.app.sidebar.deleted == 0);
        let removed = fake.removed();
        for id in &ids {
            assert!(removed.contains(id), "{id:?} in {removed:?}");
        }
        assert!(rt.state.app.sessions.get(contact).is_none(), "forgotten");
        assert!(testkit::has_text(&rt, "Nothing is deleted."));
        assert!(!enabled(&rt, "mr-delete-all"));
    }

    #[test]
    fn a_row_whose_removal_failed_says_so() {
        let mut rt = testkit::window("sessions");
        let contact = titled(&rt, CONTACT);
        let fake = rt.state.fake.clone().unwrap();
        fake.fail_remove("the disk is read-only");
        delete_now(&mut rt, contact);
        show_deleted(&mut rt);
        assert_eq!(texts_in(&rt, &row(contact)), [CONTACT, "6 days left"]);
        testkit::click(&mut rt, "mr-delete-all");
        testkit::click(&mut rt, "mr-delete-yes");
        testkit::step(&mut rt);
        pump_until(&mut rt, |ui| {
            ui.app.sessions.get(contact).is_some_and(|entry| {
                entry
                    .deleted
                    .is_some_and(|deleted| deleted.failed_unix.is_some())
            })
        });
        assert_eq!(fake.removed().len(), 1);
        assert_eq!(texts_in(&rt, &row(contact)), [CONTACT, "Not removed"]);
    }

    #[test]
    fn deleting_the_session_whose_picture_is_open_closes_the_picture() {
        let mut rt = testkit::window("viewer");
        let key = testkit::focused(&rt).unwrap();
        assert!(rt.state.viewing.is_some());
        assert!(testkit::drawn(&rt, "mr-viewer-image"));
        // Its program still runs: it has not let its conversation go yet.
        rt.update(|ui, tx| glue::ask_delete_session(ui, tx, key));
        testkit::step(&mut rt);
        assert!(is_deleted(&rt, key));
        assert!(rt.state.viewing.is_none(), "the picture closed");
        assert!(!testkit::drawn(&rt, "mr-viewer-image"));
    }

    /// The `sessions` scene with the question about the working session showing.
    /// It is in the focused tile, its box focused before the menu opened.
    fn asking() -> (Runtime<Ui>, SessionKey) {
        let mut rt = testkit::window("sessions");
        let working = titled(&rt, WORKING);
        testkit::show(&mut rt, working);
        assert_eq!(tile_keys(&rt), [working]);
        assert!(box_focused(&rt, working));
        delete_from_menu(&mut rt, working);
        assert!(asks(&rt));
        (rt, working)
    }

    #[test]
    fn escape_closes_the_question_and_deletes_nothing() {
        let (mut rt, working) = asking();
        let id = id_of(&rt, working);
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert!(!asks(&rt));
        assert_eq!(rt.state.delete_ask, None);
        assert!(!is_deleted(&rt, working));
        assert_eq!(closes(&rt, &id), 0);
        assert!(
            testkit::sent(&rt)
                .iter()
                .all(|c| *c != SessionCommand::Stop)
        );
        // Rune keeps the keyboard in the question, so the window's keys come after it has taken
        // Escape; were they to come first, they would leave it alone and stop no turn.
        let (mut rt, working) = asking();
        let escape = KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        };
        let taken = rt.update_if(|ui, tx| view::on_key(ui, &escape, tx));
        assert!(!taken);
        testkit::step(&mut rt);
        assert!(asks(&rt));
        assert!(
            testkit::sent(&rt)
                .iter()
                .all(|c| *c != SessionCommand::Stop),
            "no interrupt"
        );
        let entry = rt.state.app.sessions.get(working).unwrap();
        assert_eq!(entry.session.phase, Phase::Working, "the turn still runs");
        // The quit question puts it away, and it puts the quit question away.
        let (mut rt, _) = asking();
        rt.update(|ui, tx| {
            glue::ask_quit(ui, tx);
        });
        testkit::step(&mut rt);
        assert!(rt.state.quit_ask && rt.state.delete_ask.is_none());
        assert!(!asks(&rt));
        rt.update(|ui, _| glue::ask_delete_group(ui, GroupId::Folder(demo::RUNE_CLAUDE.into())));
        testkit::step(&mut rt);
        assert!(!rt.state.quit_ask && asks(&rt));
    }

    #[test]
    fn ctrl_w_under_the_question_closes_no_tile() {
        let (mut rt, working) = asking();
        testkit::press_with(&mut rt, Key::Command('w'), testkit::ctrl());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), [working]);
        assert!(asks(&rt), "the question still shows");
    }

    #[test]
    fn typing_under_the_question_reaches_no_box() {
        let (mut rt, _) = asking();
        let before = testkit::shown_view(&rt).draft.text.clone();
        testkit::type_text(&mut rt, "abc");
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).draft.text, before);
        assert!(asks(&rt));
    }

    #[test]
    fn delete_on_a_session_never_sent_just_makes_the_row_go() {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let ui = &mut rt.state;
        let effects = ui.app.new_session(
            "/home/alex/notes".into(),
            Place::Side,
            Leaving::ForgetIfEmpty,
        );
        glue::run(ui, effects);
        let fresh = ui.app.focused().unwrap();
        rt.invalidate();
        testkit::step(&mut rt);
        assert!(
            !rt.state
                .app
                .sessions
                .get(fresh)
                .unwrap()
                .session
                .is_stored()
        );
        assert!(testkit::drawn(&rt, &row(fresh)));
        delete_from_menu(&mut rt, fresh);
        assert!(!asks(&rt), "no question");
        assert!(rt.state.app.sessions.get(fresh).is_none(), "forgotten");
        assert!(!testkit::drawn(&rt, &row(fresh)), "its row is gone");
        assert_eq!(texts_in(&rt, "mr-view-deleted"), ["Deleted"], "no count");
        assert_eq!(rt.state.app.sidebar.deleted, 0);
    }

    #[test]
    fn after_a_direct_delete_the_keyboard_is_in_the_other_tiles_box() {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let garden = titled(&rt, GARDEN);
        let contact = titled(&rt, CONTACT);
        testkit::show_side(&mut rt, contact);
        assert_eq!(tile_keys(&rt), [garden, contact]);
        assert!(box_focused(&rt, contact));
        let tile = view::tile_key(garden);
        let before = testkit::rect(&rt, &tile).unwrap();
        right_click(&mut rt, &row(contact));
        // One frame after the press the tile left has its new width, as after its close button.
        testkit::click(&mut rt, DELETE_ITEM);
        let now = testkit::rect(&rt, &tile).unwrap();
        assert!(now[2] > before[2], "{now:?} wider than {before:?}");
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(testkit::rect(&rt, &tile).unwrap(), now, "and keeps it");
        assert!(!asks(&rt), "a resting session goes at once");
        assert!(is_deleted(&rt, contact));
        assert_eq!(tile_keys(&rt), [garden]);
        assert_eq!(testkit::focused(&rt), Some(garden));
        assert!(box_focused(&rt, garden));
    }

    #[test]
    fn delete_is_drawn_as_a_danger_entry() {
        let mut rt = testkit::window("sessions");
        let contact = titled(&rt, CONTACT);
        right_click(&mut rt, &row(contact));
        let kit = theme::kit(true);
        let colour = rt
            .elements()
            .into_iter()
            .find(|e| e.path.contains(&format!("/{DELETE_ITEM}/")) && e.text.is_some())
            .and_then(|e| (e.text.as_deref() == Some("Delete")).then_some(e.text_color))
            .flatten();
        assert_eq!(colour, Some(kit.c.danger.fg));
    }

    #[test]
    fn the_question_is_a_dialog_to_a_screen_reader() {
        let (rt, _) = asking();
        let tree = rt.accessibility_json();
        let node = find(&tree, DIALOG).expect("the question is in the tree");
        assert_eq!(node["role"], "dialog");
        assert_eq!(node["name"], format!("Delete \"{WORKING}\"?"));
    }
}
