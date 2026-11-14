//! The input box under the conversation: the queued message, the text area, the model and
//! permission-mode pickers, and the send or stop button.
//!
//! Enter in the text area submits the draft and nothing else: it never reaches a request card,
//! whose buttons take no focus. While a turn runs a message is queued by the core, not sent.

use mr_agent::PermissionMode;
use mr_core::{Phase, Session, SessionKey, mode_label, model_option_label};
use rune::{El, Key, KeyEvent, TextEdit, TextStyle, Tx, col, el, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Placement, Select,
    TextArea,
};
use rune_theme_opencode::icons::{ARROW_UP, STOP};

use crate::glue::{self, Picker, SessionView, Ui};

/// The fewest and the most lines the text area shows before it scrolls.
const LINES: (usize, usize) = (1, 10);

/// The space round the box, between it and the tile's edges; matches the rows' sides.
const AREA_PAD_X: f32 = 24.0;
const AREA_PAD_BOTTOM: f32 = 16.0;

/// The space inside the box, and between its parts.
const BOX_PAD: f32 = 8.0;

/// The space between the pickers, the spacer and the send or stop button (this app's own).
const CONTROLS_GAP: f32 = 4.0;

/// The send or stop button's width. Copied from the kit, which does not export it: the side of
/// an `IconButton` of `ControlSize::Large`, in `IconButton::build`'s size table
/// (`crates/rune-kit/src/actions.rs` in the Rune checkout, `ControlSize::Large => (28.0, ..)`).
const SEND_SIZE: f32 = 28.0;

/// The least width the mode picker keeps beside a long model label: its chevron and the start
/// of its label. The model picker's label is cut short only past this. Of the 48 points, 32
/// copy the kit's inline `Select` trigger, which does not export them (`Select::field`,
/// `SelectAppearance::Inline` in `crates/rune-kit/src/select.rs` in the Rune checkout:
/// `.pl(8.0)`, `.pr(4.0)`, `.gap(4.0)` and the 16 point chevron icon); the other 16 are this
/// app's choice, room for about two letters of the label.
const MODE_MIN: f32 = 48.0;

/// The kit text area's padding and line height, where the ended wording sits.
const FIELD_PAD: f32 = 8.0;
const FIELD_LINE_HEIGHT: f32 = 17.5;

/// The text area's placeholder, and what it reads once the session has ended.
const PLACEHOLDER: &str = "Message Claude";
const PLACEHOLDER_ENDED: &str = "This session has ended";

/// The input box of `key`'s session, keyed `mr-input-area` (the space round it) and
/// `mr-input-box` (the rounded box): the queued message when there is one, the text area keyed
/// `mr-input`, then the two pickers and the send or stop button; under the box, the line that
/// says the session's folder is gone, after a message could not wake it. The pickers' lists go into
/// `layer`. The area is `width` points wide and centred, as the conversation's rows are (the
/// whole width while the tile's width is not known yet).
pub fn input(ui: &Ui, kit: &Kit, key: SessionKey, layer: &mut Vec<El>, width: Option<f32>) -> El {
    let session = ui.app.sessions.get(key).map(|entry| &entry.session);
    let view = glue::view_of(ui, key);
    let phase = session.map(|s| &s.phase);
    let ended = matches!(phase, Some(Phase::Ended { .. }));
    let mut frame = col()
        .key("mr-input-box")
        .w_full()
        .padding(BOX_PAD)
        .gap(BOX_PAD)
        .radius(kit.radius.xl)
        .fill(kit.c.layer_01)
        .border(1.0, kit.c.border_weak);
    if let Some(queued) = session.and_then(|s| s.queued.as_deref()) {
        frame = frame.child(queued_strip(key, queued, kit));
    }
    let plan_mode = ui.app.capabilities.plan_mode;
    frame = frame.child(text_area(key, view, ended, kit)).child(
        row()
            .key("mr-input-controls")
            .w_full()
            .gap(CONTROLS_GAP)
            .center_y()
            .child(model_picker(
                key,
                session,
                view,
                kit,
                layer,
                width.map(model_room),
            ))
            .child(mode_picker(key, session, view, plan_mode, kit, layer))
            .child(el().key("mr-input-gap").grow(1.0))
            .child(send_or_stop(ui, key, view, phase, kit)),
    );
    let area = col().key("mr-input-area");
    let area = match width {
        Some(width) => area.w(width).self_center(),
        None => area.w_full(),
    };
    let area = area.px(AREA_PAD_X).pb(AREA_PAD_BOTTOM).child(frame);
    match &view.folder_gone {
        Some(folder) => area.child(folder_gone(folder, kit)),
        None => area,
    }
}

/// The line under the box, keyed `mr-folder-gone`, after a message could not wake the session
/// because its folder is no longer there: in the small type and the error colour of the folder
/// picker's error line.
fn folder_gone(folder: &std::path::Path, kit: &Kit) -> El {
    text(format!(
        "This session's folder no longer exists: {}",
        folder.display()
    ))
    .key("mr-folder-gone")
    .font(kit.ty.small)
    .color(kit.c.danger.fg)
    .wrap()
    .w_full()
    .pt(6.0)
    .px(8.0)
}

/// The most width the model picker takes in an input area `width` points wide: what the box
/// holds beside the send or stop button, less [`MODE_MIN`] for the mode picker. `AREA_PAD_X`,
/// `BOX_PAD` and `CONTROLS_GAP` are this app's; `SEND_SIZE` and part of `MODE_MIN` copy the
/// kit (see each), so a change of those kit sizes makes this cap a little off. The tests
/// `in_the_smallest_window_*` measure the result.
fn model_room(width: f32) -> f32 {
    let inside = width - 2.0 * AREA_PAD_X - 2.0 * BOX_PAD;
    (inside - SEND_SIZE - 3.0 * CONTROLS_GAP - MODE_MIN).max(MODE_MIN)
}

/// The permission modes the mode picker offers: Ask and Accept edits, then Plan when the agent
/// has it, then Auto when the model in use supports it, then `current` when it is none of
/// those, so an unusual mode the session is already in stays visible. Bypass is never offered
/// otherwise: this app cannot tell whether the user's settings allow it.
pub fn mode_choices(
    current: &PermissionMode,
    plan_mode: bool,
    auto_mode: bool,
) -> Vec<PermissionMode> {
    let mut choices = vec![PermissionMode::Ask, PermissionMode::AcceptEdits];
    if plan_mode {
        choices.push(PermissionMode::Plan);
    }
    if auto_mode {
        choices.push(PermissionMode::Auto);
    }
    if !choices.contains(current) {
        choices.push(current.clone());
    }
    choices
}

/// Stops the turn of `key`'s session when one is running. True when it did, so the key that
/// asked is used up.
pub fn stop_turn(ui: &mut Ui, key: SessionKey) -> bool {
    let running = ui
        .app
        .sessions
        .get(key)
        .is_some_and(|entry| matches!(entry.session.phase, Phase::Working | Phase::NeedsUser));
    if running {
        let effects = ui.app.stop(key);
        glue::run(ui, effects);
    }
    running
}

/// Whether `ev` is Escape, with or without modifiers.
pub fn is_escape(ev: &KeyEvent) -> bool {
    matches!(&ev.key, Key::Escape)
}

/// "Queued: " and the first line of the message waiting for the turn of `key`'s session to end,
/// with "Edit", which takes it back into the draft.
fn queued_strip(key: SessionKey, queued: &str, kit: &Kit) -> El {
    let first = queued.lines().next().unwrap_or_default();
    let edit = Button::new("Edit")
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(move |ui: &mut Ui| edit_queued(ui, key))
        .build(kit)
        .key("mr-queued-edit");
    row()
        .key("mr-queued")
        .w_full()
        .gap(8.0)
        .pl(8.0)
        .center_y()
        .child(
            text(format!("Queued: {first}"))
                .key("mr-queued-text")
                .font(kit.ty.small)
                .color(kit.c.text_muted)
                .truncate()
                .grow(1.0)
                .shrink(),
        )
        .child(edit)
}

/// Puts the queued message of `key`'s session back at the start of its draft, a line break
/// between it and what was already typed, and focuses the text area.
fn edit_queued(ui: &mut Ui, key: SessionKey) {
    let taken = ui
        .app
        .sessions
        .get_mut(key)
        .and_then(|mut entry| entry.session.take_queued());
    glue::run(ui, Vec::new());
    let Some(message) = taken else {
        return;
    };
    let draft = &mut glue::view_mut(ui, key).draft;
    let text = if draft.text.is_empty() {
        message
    } else {
        format!("{message}\n{}", draft.text)
    };
    draft.replace_text(text);
    draft.focused = true;
}

/// The text area of `key`'s session, showing the draft in `view`: Enter submits to that session,
/// Shift+Enter adds a line, Escape stops its running turn.
///
/// Once the session has ended it is disabled and empty, and reads "This session has ended" where
/// the placeholder goes; the draft is kept for a restart. Those words are a text of their own
/// (`mr-input-ended`) drawn as the kit draws a placeholder, because Rune does not list a field's
/// placeholder among the window's texts; the field is named by them for assistive technology.
fn text_area(key: SessionKey, view: &SessionView, ended: bool, kit: &Kit) -> El {
    if ended {
        let style = TextStyle {
            line_height: FIELD_LINE_HEIGHT,
            ..kit.ty.ui
        };
        return TextArea::new(&TextEdit::new(""))
            .label(PLACEHOLDER_ENDED)
            .width(None)
            .auto_grow(LINES.0, LINES.1)
            .disabled(true)
            .build(kit)
            .key("mr-input")
            .child(
                text(PLACEHOLDER_ENDED)
                    .key("mr-input-ended")
                    .font(style)
                    .color(kit.c.text_faint)
                    .absolute(FIELD_PAD, FIELD_PAD)
                    .a11y_hidden(),
            );
    }
    TextArea::new(&view.draft)
        .placeholder(PLACEHOLDER)
        .label("Message")
        .width(None)
        .auto_grow(LINES.0, LINES.1)
        .on_edit(move |ui: &mut Ui| &mut glue::view_mut(ui, key).draft)
        .on_submit(move |ui: &mut Ui, _tx: &mut Tx| submit(ui, key))
        .on_key(move |ui: &mut Ui, ev: &KeyEvent, _tx: &mut Tx| is_escape(ev) && stop_turn(ui, key))
        .build(kit)
        .key("mr-input")
}

/// Puts `message`, which `key`'s session did not take, back at the end of what is typed in its
/// input box, on a line of its own when something is typed there, with the caret after it, and
/// focuses the box. What was typed stays undoable.
pub fn give_back(ui: &mut Ui, key: SessionKey, message: &str) {
    let draft = &mut glue::view_mut(ui, key).draft;
    draft.set_caret(draft.text.len());
    if draft.text.is_empty() {
        draft.insert(message);
    } else {
        draft.insert(&format!("\n{message}"));
    }
    draft.focused = true;
}

/// Sends the draft of `key`'s session to it, or queues it there while a turn runs or the
/// session wakes, and clears it. A draft that is empty after trimming, or that the session does
/// not take now (`App::takes_message`: it has ended, or a question about it waits or its check
/// in the store runs), stays as it is. So does one for a resting session whose folder is no
/// longer there: no program could start in it, so the line under the box says so instead.
/// That is checked here, as the user sends, never as the box is drawn.
fn submit(ui: &mut Ui, key: SessionKey) {
    let text = glue::view_of(ui, key).draft.text.clone();
    if !ui.app.takes_message(key) || text.trim().is_empty() {
        return;
    }
    let gone = ui
        .app
        .sessions
        .get(key)
        .filter(|entry| entry.session.phase == Phase::Resting)
        .map(|entry| entry.session.folder.clone())
        .filter(|folder| !(ui.folder_exists)(folder));
    let stays = gone.is_some();
    glue::view_mut(ui, key).folder_gone = gone;
    if stays {
        return;
    }
    let effects = ui.app.submit(key, &text);
    glue::run(ui, effects);
    glue::view_mut(ui, key).draft.clear();
}

/// Send while no turn runs (disabled while there is nothing to send), stop while one does.
///
/// Both are kit buttons, which take focus when pressed (`preserve_focus_on_press` does not hold
/// against a control that is focusable itself), so a click hands focus back to the text area and
/// the user goes on typing. A key press leaves focus on the button, as a keyboard user expects.
fn send_or_stop(
    ui: &Ui,
    key: SessionKey,
    view: &SessionView,
    phase: Option<&Phase>,
    kit: &Kit,
) -> El {
    match phase {
        Some(Phase::Working | Phase::NeedsUser) => IconButton::new(&STOP)
            .variant(IconButtonVariant::Neutral)
            .control_size(ControlSize::Large)
            .label("Stop")
            .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
                stop_turn(ui, key);
                back_to_typing(ui, key, tx);
            })
            .build(kit)
            .key("mr-stop"),
        // Disabled while the session does not take the draft (see `submit`).
        _ => {
            let enabled = ui.app.takes_message(key) && !view.draft.text.trim().is_empty();
            IconButton::new(&ARROW_UP)
                .variant(IconButtonVariant::Submit)
                .control_size(ControlSize::Large)
                .label("Send")
                .disabled(!enabled)
                .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
                    submit(ui, key);
                    back_to_typing(ui, key, tx);
                })
                .build(kit)
                .key("mr-send")
        }
    }
}

/// After a click, focus goes back to the text area of `key`'s session.
fn back_to_typing(ui: &mut Ui, key: SessionKey, tx: &Tx) {
    if !tx.by_keys() {
        glue::view_mut(ui, key).draft.focused = true;
    }
}

/// The model in use, by name, always shown; choosing another asks the agent for it. A choice
/// whose name does not say which model it is reads with that model's name beside it, in the
/// trigger and in the list. Disabled while the session opens, once it has ended, and when the
/// agent offered no models. It is at most `room` points wide (see [`model_room`]).
fn model_picker(
    key: SessionKey,
    session: Option<&Session>,
    view: &SessionView,
    kit: &Kit,
    layer: &mut Vec<El>,
    room: Option<f32>,
) -> El {
    let options: Vec<(String, String)> = session
        .map(|s| {
            s.models
                .iter()
                .map(|m| (model_option_label(m), m.id.clone()))
                .collect()
        })
        .unwrap_or_default();
    let names: Vec<&str> = options.iter().map(|(name, _)| name.as_str()).collect();
    let current = session.map_or_else(|| "Model".to_owned(), Session::model_label);
    let disabled = options.is_empty() || !picker_enabled(session);
    let picker = Select::new("mr-model", &names)
        .label("Model")
        .current(Some(&current))
        .placement(Placement::TopStart)
        .disabled(disabled)
        .open(view.open_picker == Some(Picker::Model))
        .on_toggle(move |ui: &mut Ui| toggle(ui, key, Picker::Model))
        .on_close(move |ui: &mut Ui| close(ui, key))
        .on_choose(move |ui: &mut Ui, name: &str, _tx: &mut Tx| {
            if let Some((_, id)) = options.iter().find(|(n, _)| n == name) {
                let effects = ui.app.choose_model(key, id);
                glue::run(ui, effects);
            }
            chosen(ui, key);
        })
        .build(kit, layer);
    // In a narrow window the mode picker gives up width first. A model label longer than the
    // room left beside it is cut short inside the trigger, so the send or stop button stays
    // inside the box.
    match room {
        Some(room) => picker.max_w(room),
        None => picker,
    }
}

/// The session's permission mode, always shown; choosing another asks the agent for it.
fn mode_picker(
    key: SessionKey,
    session: Option<&Session>,
    view: &SessionView,
    plan_mode: bool,
    kit: &Kit,
    layer: &mut Vec<El>,
) -> El {
    let mode = session.map_or(PermissionMode::Ask, |s| s.mode.clone());
    let auto_mode = session.is_some_and(Session::auto_mode);
    let choices = mode_choices(&mode, plan_mode, auto_mode);
    let labels: Vec<String> = choices.iter().map(mode_label).collect();
    Select::new("mr-mode", &labels)
        .label("Permission mode")
        .current(Some(&mode_label(&mode)))
        .placement(Placement::TopStart)
        .disabled(!picker_enabled(session))
        .open(view.open_picker == Some(Picker::Mode))
        .on_toggle(move |ui: &mut Ui| toggle(ui, key, Picker::Mode))
        .on_close(move |ui: &mut Ui| close(ui, key))
        .on_choose(move |ui: &mut Ui, label: &str, _tx: &mut Tx| {
            if let Some(mode) = choices.iter().find(|m| mode_label(m) == label) {
                let effects = ui.app.choose_mode(key, mode.clone());
                glue::run(ui, effects);
            }
            chosen(ui, key);
        })
        .build(kit, layer)
        // In a narrow window the mode picker gives up width, its label cut short, so the
        // model's name stays whole and the send or stop button stays inside the box.
        .shrink()
        .min_w(0.0)
}

/// The pickers work while the session can take a command: not while it opens, nor once ended.
fn picker_enabled(session: Option<&Session>) -> bool {
    session.is_some_and(|s| !matches!(s.phase, Phase::Opening | Phase::Ended { .. }))
}

/// Opens `picker` of `key`'s session, closing the other one, or closes it when it is open.
fn toggle(ui: &mut Ui, key: SessionKey, picker: Picker) {
    let view = glue::view_mut(ui, key);
    view.open_picker = (view.open_picker != Some(picker)).then_some(picker);
}

/// Escape or a press outside closed the open picker; the kit hands focus back to its trigger.
fn close(ui: &mut Ui, key: SessionKey) {
    glue::view_mut(ui, key).open_picker = None;
}

/// A choice closes the picker and puts the user back in the text area.
fn chosen(ui: &mut Ui, key: SessionKey) {
    let view = glue::view_mut(ui, key);
    view.open_picker = None;
    view.draft.focused = true;
}

#[cfg(test)]
mod tests {
    use mr_agent::{CommandKind, PermissionMode, SessionCommand, SessionEvent, TurnSummary};
    use mr_core::Session;
    use rune::{Key, Mods, Runtime};

    use super::{MODE_MIN, mode_choices};
    use crate::glue::{self, Picker, Ui};
    use crate::testkit;

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

    #[test]
    fn enter_sends_and_shift_enter_adds_a_line() {
        let mut rt = testkit::window("empty");
        testkit::type_text(&mut rt, "one");
        testkit::press_with(
            &mut rt,
            Key::Enter,
            Mods {
                shift: true,
                ..Mods::default()
            },
        );
        testkit::type_text(&mut rt, "two");
        assert_eq!(sends(&rt), Vec::<String>::new());
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(sends(&rt), vec!["one\ntwo".to_string()]);
        assert_eq!(testkit::shown_view(&rt).draft.text, "");
        assert!(testkit::has_text(&rt, "one\ntwo") || testkit::has_text(&rt, "two"));
        testkit::press(&mut rt, Key::Enter);
        testkit::type_text(&mut rt, "   ");
        testkit::press(&mut rt, Key::Enter);
        assert_eq!(sends(&rt).len(), 1, "an empty message is not sent");
    }

    #[test]
    fn while_a_turn_runs_the_button_stops_it_and_typing_queues() {
        let mut rt = testkit::window("working");
        assert!(rt.find("mr-stop").is_some() && rt.find("mr-send").is_none());
        testkit::type_text(&mut rt, "then run the tests\nand report");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(sends(&rt).len(), 1, "only the scene's own message was sent");
        assert!(testkit::has_text(&rt, "Queued: then run the tests"));
        testkit::click(&mut rt, "mr-queued-edit");
        testkit::step(&mut rt);
        assert_eq!(
            testkit::shown_view(&rt).draft.text,
            "then run the tests\nand report"
        );
        assert!(rt.find("mr-queued").is_none());
        testkit::click(&mut rt, "mr-stop");
        testkit::step(&mut rt);
        assert!(testkit::sent(&rt).contains(&SessionCommand::Stop));
    }

    #[test]
    fn a_queued_message_goes_out_when_the_turn_ends() {
        let mut rt = testkit::window("working");
        testkit::type_text(&mut rt, "next");
        testkit::press(&mut rt, Key::Enter);
        testkit::emit(
            &mut rt,
            SessionEvent::TurnFinished(TurnSummary {
                cost_usd: 0.0,
                context_used: None,
                duration_ms: 1,
                interrupted: false,
            }),
        );
        assert_eq!(sends(&rt).last().map(String::as_str), Some("next"));
        assert!(rt.find("mr-queued").is_none());
    }

    #[test]
    fn typing_enter_beside_a_waiting_request_queues_and_answers_nothing() {
        let mut rt = testkit::window("permission");
        testkit::type_text(&mut rt, "yes");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert!(
            !testkit::sent(&rt)
                .iter()
                .any(|c| matches!(c, SessionCommand::Answer { .. }))
        );
        assert!(testkit::has_text(&rt, "Queued: yes"));
        assert!(rt.find("mr-allow-r1").is_some());
    }

    #[test]
    fn escape_stops_a_turn_from_anywhere_and_does_nothing_otherwise() {
        let mut rt = testkit::window("working");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(
            testkit::sent(&rt)
                .iter()
                .filter(|c| **c == SessionCommand::Stop)
                .count(),
            1
        );
        let mut rt = testkit::window("working");
        testkit::shown_view_mut(&mut rt).draft.focused = false;
        rt.invalidate();
        testkit::step(&mut rt);
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(
            testkit::sent(&rt)
                .iter()
                .filter(|c| **c == SessionCommand::Stop)
                .count(),
            1
        );
        let mut rt = testkit::window("empty");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert!(!testkit::sent(&rt).contains(&SessionCommand::Stop));
    }

    #[test]
    fn the_model_picker_shows_the_model_and_changes_it() {
        let mut rt = testkit::window("empty");
        assert_eq!(
            rt.state.shown_session().unwrap().model_label(),
            DEFAULT_LABEL
        );
        assert!(testkit::has_text(&rt, DEFAULT_LABEL));
        testkit::click(&mut rt, "mr-model-trigger");
        testkit::step(&mut rt);
        testkit::click_text(&mut rt, "Fast");
        testkit::step(&mut rt);
        assert!(testkit::sent(&rt).contains(&SessionCommand::SetModel("fast".into())));
        assert_eq!(
            rt.state.shown_session().unwrap().model_label(),
            DEFAULT_LABEL,
            "not until the agent confirms"
        );
        testkit::emit(
            &mut rt,
            SessionEvent::ModelChanged("fake-model-fast".into()),
        );
        assert_eq!(rt.state.shown_session().unwrap().model_label(), "Fast");
        assert!(testkit::has_text(&rt, "Fast"));
        assert_eq!(testkit::shown_view(&rt).open_picker, None);
    }

    /// How the fake's default model reads: its name, and the model it stands for.
    const DEFAULT_LABEL: &str = "Default · Fake 1";

    #[test]
    fn the_model_list_names_the_model_a_choice_stands_for_and_sends_its_id() {
        let mut rt = testkit::window("empty");
        testkit::emit(
            &mut rt,
            SessionEvent::ModelChanged("fake-model-fast".into()),
        );
        assert_eq!(rt.state.shown_session().unwrap().model_label(), "Fast");
        testkit::click(&mut rt, "mr-model-trigger");
        testkit::step(&mut rt);
        let listed: Vec<String> = rt
            .elements()
            .into_iter()
            .filter(|e| e.path.contains("mr-model-content") && e.key.as_deref() == Some("label"))
            .filter_map(|e| e.text)
            .collect();
        assert_eq!(listed, vec![DEFAULT_LABEL.to_string(), "Fast".to_string()]);
        testkit::click_text(&mut rt, DEFAULT_LABEL);
        testkit::step(&mut rt);
        assert_eq!(
            testkit::sent(&rt).last(),
            Some(&SessionCommand::SetModel("default".into()))
        );
    }

    #[test]
    fn the_mode_picker_offers_the_three_modes() {
        let mut rt = testkit::window("empty");
        assert!(testkit::has_text(&rt, "Ask before edits"));
        testkit::click(&mut rt, "mr-mode-trigger");
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "Accept edits") && testkit::has_text(&rt, "Plan"));
        assert!(!testkit::has_text(&rt, "Bypass permissions"));
        testkit::click_text(&mut rt, "Plan");
        testkit::step(&mut rt);
        assert!(
            testkit::sent(&rt).contains(&SessionCommand::SetPermissionMode(PermissionMode::Plan))
        );
    }

    #[test]
    fn the_mode_choices_keep_an_unusual_current_mode_visible() {
        use PermissionMode::*;
        assert_eq!(
            mode_choices(&Ask, true, false),
            vec![Ask, AcceptEdits, Plan]
        );
        assert_eq!(mode_choices(&Ask, false, false), vec![Ask, AcceptEdits]);
        assert_eq!(
            mode_choices(&Bypass, true, false),
            vec![Ask, AcceptEdits, Plan, Bypass]
        );
        assert_eq!(
            mode_choices(&Other("dontAsk".into()), true, false),
            vec![Ask, AcceptEdits, Plan, Other("dontAsk".into())]
        );
    }

    #[test]
    fn auto_is_offered_after_plan_when_the_model_supports_it() {
        use PermissionMode::*;
        assert_eq!(
            mode_choices(&Ask, true, true),
            vec![Ask, AcceptEdits, Plan, Auto]
        );
        assert_eq!(
            mode_choices(&Ask, false, true),
            vec![Ask, AcceptEdits, Auto]
        );
        assert_eq!(
            mode_choices(&Auto, true, false),
            vec![Ask, AcceptEdits, Plan, Auto],
            "in use on a model without it: still shown"
        );
        assert_eq!(
            mode_choices(&Bypass, true, true),
            vec![Ask, AcceptEdits, Plan, Auto, Bypass]
        );
    }

    #[test]
    fn choosing_auto_asks_for_it_and_a_refusal_keeps_the_mode_in_use() {
        let mut rt = testkit::window("empty");
        testkit::click(&mut rt, "mr-mode-trigger");
        testkit::step(&mut rt);
        testkit::click_text(&mut rt, "Auto");
        testkit::step(&mut rt);
        assert_eq!(
            testkit::sent(&rt).last(),
            Some(&SessionCommand::SetPermissionMode(PermissionMode::Auto))
        );
        testkit::emit(
            &mut rt,
            SessionEvent::CommandFailed {
                what: CommandKind::SetPermissionMode,
                reason: "auto mode is not available".into(),
            },
        );
        assert!(testkit::has_text(
            &rt,
            "Could not change the permission mode: auto mode is not available"
        ));
        assert_eq!(rt.state.shown_session().unwrap().mode, PermissionMode::Ask);
        let shown = rt
            .elements()
            .into_iter()
            .find(|e| e.path.contains("/mr-mode-trigger/") && e.text.is_some())
            .and_then(|e| e.text);
        assert_eq!(shown.as_deref(), Some("Ask before edits"));
        // Accepted, it is the mode shown.
        testkit::emit(
            &mut rt,
            SessionEvent::PermissionModeChanged(PermissionMode::Auto),
        );
        assert!(testkit::texts(&rt).iter().any(|t| t == "Auto"));
    }

    #[test]
    fn auto_is_not_offered_on_a_model_without_it() {
        let mut rt = testkit::window("empty");
        testkit::emit(
            &mut rt,
            SessionEvent::ModelChanged("fake-model-fast".into()),
        );
        testkit::click(&mut rt, "mr-mode-trigger");
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "Plan"));
        assert!(!testkit::texts(&rt).iter().any(|t| t == "Auto"));
    }

    #[test]
    fn an_ended_session_cannot_be_typed_into() {
        let mut rt = testkit::window("ended");
        testkit::type_text(&mut rt, "hello");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).draft.text, "");
        assert!(testkit::has_text(&rt, "This session has ended"));
    }

    #[test]
    fn a_message_to_a_session_at_rest_opens_its_program() {
        let mut rt = testkit::window("empty");
        testkit::type_text(&mut rt, "go");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        testkit::emit(
            &mut rt,
            SessionEvent::Exited {
                clean: true,
                detail: String::new(),
            },
        );
        assert_eq!(
            rt.state.shown_session().unwrap().phase,
            mr_core::Phase::Resting
        );
        assert!(!testkit::has_text(&rt, "This session has ended"));
        testkit::type_text(&mut rt, "hello");
        testkit::press(&mut rt, Key::Enter);
        let session = rt.state.shown_session().unwrap();
        assert_eq!(session.phase, mr_core::Phase::Opening);
        assert_eq!(
            session.queued.as_deref(),
            Some("hello"),
            "queued as it wakes"
        );
        assert_eq!(testkit::shown_view(&rt).draft.text, "");
        assert_eq!(testkit::shown_open(&rt), Some(2), "a program was opened");
        testkit::step(&mut rt);
        testkit::step(&mut rt);
        let sends: Vec<String> = testkit::sent(&rt)
            .into_iter()
            .filter_map(|c| match c {
                SessionCommand::Send { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(sends, vec!["go".to_string(), "hello".to_string()]);
    }

    #[test]
    fn a_message_waits_in_the_box_while_its_question_waits() {
        let mut rt = testkit::window("empty");
        let stored = mr_agent::StoredSession {
            id: mr_agent::SessionId("a1".into()),
            folder: "/home/alex/project".into(),
            title: "Fix the parser".into(),
            last_active_unix: 1_799_990_000,
        };
        let app = &mut rt.state.app;
        let key = app.sessions.add(Session::resting(&stored), 0);
        app.sessions.get_mut(key).unwrap().prompt = Some(mr_core::Prompt::OpenElsewhere {
            text: "carry on".into(),
        });
        let effects = app.show(key, mr_core::Leaving::ForgetIfEmpty);
        glue::run(&mut rt.state, effects);
        testkit::step(&mut rt);
        testkit::type_text(&mut rt, "hello");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        testkit::click(&mut rt, "mr-send");
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).draft.text, "hello");
        assert_eq!(
            rt.state.shown_session().unwrap().phase,
            mr_core::Phase::Resting
        );
    }

    /// Whether the text area's field has keyboard focus.
    fn text_area_focused(rt: &Runtime<Ui>) -> bool {
        rt.elements()
            .iter()
            .any(|e| e.focused && e.path.contains("/mr-input/"))
    }

    fn rect(rt: &Runtime<Ui>, key: &str) -> [f32; 4] {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
            .unwrap_or_else(|| panic!("no element keyed {key:?}"))
            .rect
    }

    #[test]
    fn the_text_area_has_focus_on_the_main_screen_and_after_new_session() {
        let mut rt = testkit::window("conversation");
        assert!(text_area_focused(&rt));
        let _folder = testkit::new_session_elsewhere(&mut rt);
        assert_eq!(testkit::shown_open(&rt), Some(2));
        assert!(text_area_focused(&rt));
        // A click on send or stop leaves it focused too.
        let before = sends(&rt).len();
        testkit::type_text(&mut rt, "hello");
        testkit::step(&mut rt);
        testkit::click(&mut rt, "mr-send");
        testkit::step(&mut rt);
        assert_eq!(sends(&rt)[before..], ["hello".to_string()]);
        assert!(text_area_focused(&rt));
    }

    #[test]
    fn escape_in_an_open_picker_closes_it_and_the_turn_runs_on() {
        let mut rt = testkit::window("working");
        testkit::click(&mut rt, "mr-mode-trigger");
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).open_picker, Some(Picker::Mode));
        testkit::click(&mut rt, "mr-model-trigger");
        testkit::step(&mut rt);
        testkit::click(&mut rt, "mr-model-trigger");
        testkit::step(&mut rt);
        assert_eq!(
            testkit::shown_view(&rt).open_picker,
            Some(Picker::Model),
            "one picker open at a time"
        );
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).open_picker, None);
        assert!(!testkit::sent(&rt).contains(&SessionCommand::Stop));
    }

    #[test]
    fn a_picker_open_when_the_session_ends_does_not_open_again() {
        let mut rt = testkit::window("empty");
        testkit::click(&mut rt, "mr-model-trigger");
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).open_picker, Some(Picker::Model));
        testkit::emit(
            &mut rt,
            SessionEvent::Exited {
                clean: false,
                detail: "claude exited with status 1".into(),
            },
        );
        assert_eq!(testkit::shown_view(&rt).open_picker, None);
        // The list and its scrim fade out; let them go before clicking under them.
        let scrim = |rt: &Runtime<Ui>| {
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("mr-model-scrim"))
        };
        for _ in 0..20 {
            if !scrim(&rt) {
                break;
            }
            testkit::step(&mut rt);
        }
        testkit::click(&mut rt, "mr-restart");
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_open(&rt), Some(2));
        assert!(rt.find("mr-model-trigger").is_some());
        assert!(rt.find("mr-model-content").is_none(), "the list stays shut");
    }

    #[test]
    fn the_model_picker_shuts_when_its_list_empties() {
        let mut rt = testkit::window("empty");
        testkit::click(&mut rt, "mr-model-trigger");
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).open_picker, Some(Picker::Model));
        rt.state.shown_entry_mut().unwrap().session.models.clear();
        // Anything taken in from the agent goes through the glue.
        testkit::emit(&mut rt, SessionEvent::TitleChanged("Renamed".into()));
        assert_eq!(testkit::shown_view(&rt).open_picker, None);
        // The mode picker has its list whatever the models.
        testkit::click(&mut rt, "mr-mode-trigger");
        testkit::step(&mut rt);
        testkit::emit(&mut rt, SessionEvent::TitleChanged("Again".into()));
        assert_eq!(testkit::shown_view(&rt).open_picker, Some(Picker::Mode));
    }

    #[test]
    fn in_the_smallest_window_a_long_model_label_is_cut_inside_its_trigger() {
        let mut rt = testkit::window_sized("working", (560.0, 400.0));
        // The label the recorded list gives the default model, the longest seen.
        let mut entry = rt.state.shown_entry_mut().unwrap();
        let session = &mut entry.session;
        session.models[0].name = "Default (recommended)".into();
        session.models[0].stands_for = Some("Fable 5.2".into());
        let label = session.model_label();
        assert_eq!(label, "Default (recommended) · Fable 5.2");
        drop(entry);
        rt.invalidate();
        testkit::step(&mut rt);
        let frame = rect(&rt, "mr-input-box");
        let inside = |part: [f32; 4]| {
            part[0] >= frame[0] - 0.5 && part[0] + part[2] <= frame[0] + frame[2] + 0.5
        };
        let stop = rect(&rt, "mr-stop");
        assert!(inside(stop), "{stop:?} inside {frame:?}");
        let model = rect(&rt, "mr-model-trigger");
        assert!(inside(model), "{model:?} inside {frame:?}");
        let mode = rect(&rt, "mr-mode-trigger");
        assert!(
            model[0] + model[2] <= mode[0] + 0.5,
            "the pickers do not overlap"
        );
        assert!(
            mode[2] >= MODE_MIN - 0.5,
            "the mode picker keeps its least width"
        );
        let value = rt
            .elements()
            .into_iter()
            .find(|e| e.path.contains("/mr-model-trigger/") && e.text.as_deref() == Some(&label))
            .expect("the model picker shows the label");
        assert!(
            value.rect[0] + value.rect[2] <= model[0] + model[2] + 0.5,
            "the label is cut inside its trigger: {:?} in {model:?}",
            value.rect
        );
        let mut roomy = testkit::window("working");
        let mut entry = roomy.state.shown_entry_mut().unwrap();
        let session = &mut entry.session;
        session.models[0].name = "Default (recommended)".into();
        session.models[0].stands_for = Some("Fable 5.2".into());
        drop(entry);
        roomy.invalidate();
        testkit::step(&mut roomy);
        let whole = roomy
            .elements()
            .into_iter()
            .find(|e| e.path.contains("/mr-model-trigger/") && e.text.as_deref() == Some(&label))
            .expect("the model picker shows the label");
        assert!(
            value.rect[2] < whole.rect[2],
            "cut short at the smallest size only"
        );
    }

    #[test]
    fn in_the_smallest_window_the_model_and_the_button_stay_whole_in_the_box() {
        let mut rt = testkit::window_sized("working", (560.0, 400.0));
        testkit::type_text(
            &mut rt,
            "then run the tests and report what failed, with their names",
        );
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        let frame = rect(&rt, "mr-input-box");
        let stop = rect(&rt, "mr-stop");
        assert!(
            stop[0] + stop[2] <= frame[0] + frame[2],
            "{stop:?} inside {frame:?}"
        );
        assert!(testkit::texts(&rt).iter().any(|t| t == DEFAULT_LABEL));
        let value = rt
            .elements()
            .into_iter()
            .find(|e| e.path.contains("/mr-model-trigger/") && e.text.is_some())
            .expect("the model picker shows a name");
        let model = rect(&rt, "mr-model-trigger");
        assert!(
            value.rect[0] + value.rect[2] <= model[0] + model[2],
            "the name is inside its trigger"
        );
        let roomy = testkit::window("working");
        let whole = roomy
            .elements()
            .into_iter()
            .find(|e| e.path.contains("/mr-model-trigger/") && e.text.is_some())
            .expect("the model picker shows a name");
        assert_eq!(value.rect[2], whole.rect[2], "the name is not cut");
    }
}
