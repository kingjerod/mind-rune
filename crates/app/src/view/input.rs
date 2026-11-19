//! The input box under the conversation: the queued message, the chips, the text area, the
//! attach button, the model, effort and permission-mode pickers, and the send or stop button.
//!
//! Enter in the text area submits the draft and nothing else: it never reaches a request card,
//! whose buttons take no focus. While a turn runs a message is queued by the core, not sent.

use mr_agent::{ImageData, PermissionMode};
use mr_core::{Outgoing, Phase, Session, SessionKey, Source, mode_label, model_option_label};
use rune::{ClipboardContent, El, FileDialogOptions, Key, KeyEvent, TextEdit, Tx, col, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Placement, Select,
};
use rune_theme_opencode::icons::{ARROW_UP, PLUS, STOP};

use crate::glue::{self, Picker, SessionView, Ui};
use crate::view::{chips, field, tiles};

/// The fewest and the most lines the text area shows before it scrolls; a short tile allows
/// fewer ([`lines_for`]).
const LINES: (usize, usize) = (1, 10);

/// The space round the box, between it and the tile's edges; matches the rows' sides.
const AREA_PAD_X: f32 = 24.0;
const AREA_PAD_BOTTOM: f32 = 16.0;

/// The space inside the box, and between its parts.
const BOX_PAD: f32 = 8.0;

/// The space between the pickers and the send or stop button (this app's own).
const CONTROLS_GAP: f32 = 4.0;

/// The send or stop button's width. Copied from the kit, which does not export it: the side of
/// an `IconButton` of `ControlSize::Large`, in `IconButton::build`'s size table
/// (`crates/rune-kit/src/actions.rs` in the Rune checkout, `ControlSize::Large => (28.0, ..)`).
const SEND_SIZE: f32 = 28.0;

/// The least width the mode picker keeps beside a long model label: its chevron, and an
/// ellipsis where its label was. The model picker's label is cut short only past this. Of the
/// 48 points, 32 copy the kit's inline `Select` trigger, which does not export them
/// (`Select::field`, `SelectAppearance::Inline` in `crates/rune-kit/src/select.rs` in the Rune
/// checkout: `.pl(8.0)`, `.pr(4.0)`, `.gap(4.0)` and the 16 point chevron icon); the other 16
/// are this app's choice, which the ellipsis fills: no letter of the label shows.
const MODE_MIN: f32 = 48.0;

/// The least the effort picker shrinks to, in points: as [`MODE_MIN`], its chevron and an
/// ellipsis, with no letter of the level.
const EFFORT_MIN: f32 = 48.0;

/// The height of the queued message's strip: its Edit button's. Copied from the kit, which does
/// not export it: the height of a `Button` of `ControlSize::Small`, in `push_metrics`
/// (`crates/rune-kit/src/actions.rs` in the Rune checkout, `ControlSize::Small => (20.0, ..)`).
const QUEUED_HEIGHT: f32 = 20.0;

/// The space between the box and a line under it.
const UNDER_BOX_PAD: f32 = 6.0;

/// The height the chips row adds to the box, while there is one: the row and the gap over it.
pub(crate) const CHIPS_HEIGHT: f32 = chips::ROW_HEIGHT + BOX_PAD;

/// The height of the box round one line of text, with its bottom row and the space under it:
/// the box's padding and the field's on either side, the gap between the text and the row, the
/// row as high as the send button, and the space under the box.
const AROUND_TEXT: f32 = 2.0 * BOX_PAD + 2.0 * field::PAD + BOX_PAD + SEND_SIZE + AREA_PAD_BOTTOM;

/// The least height of conversation a tile keeps over its input box.
pub const CONVERSATION_MIN: f32 = 48.0;

/// How many lines the text may grow to in a tile `tile_height` high, when the box holds
/// `extra` points besides the text and the bottom row. Ten when the height is not known.
pub fn lines_for(tile_height: Option<f32>, extra: f32) -> usize {
    let Some(height) = tile_height else {
        return LINES.1;
    };
    let room = height - tiles::HEADER_HEIGHT - CONVERSATION_MIN - AROUND_TEXT - extra;
    // A cast saturates: no room at all is 0 lines, then the least.
    ((room / field::LINE_HEIGHT).floor() as usize).clamp(LINES.0, LINES.1)
}

/// What the box of a session holds besides its text and its bottom row, in points: the queued
/// message and the chips, each with the gap over it, and each line under the box. A line under
/// the box is counted as one line of its type, though a long one wraps.
fn extra_height(session: Option<&Session>, view: &SessionView, kit: &Kit) -> f32 {
    let mut extra = 0.0;
    if session.is_some_and(|s| s.queued.is_some()) {
        extra += QUEUED_HEIGHT + BOX_PAD;
    }
    if session.is_some_and(|s| !s.chips.is_empty()) {
        extra += CHIPS_HEIGHT;
    }
    let under = [view.folder_gone.is_some(), view.attach_error.is_some()];
    let lines = under.into_iter().filter(|shown| *shown).count();
    extra + lines as f32 * (UNDER_BOX_PAD + kit.ty.small.line_px())
}

/// The attach button's label and tooltip, and the title of the file picker it opens.
const ATTACH: &str = "Attach a file";

/// The text area's placeholder, and what it reads once the session has ended.
const PLACEHOLDER: &str = "Message Claude";
const PLACEHOLDER_ENDED: &str = "This session has ended";

/// The input box of `key`'s session, keyed `mr-input-area` (the space round it) and
/// `mr-input-box` (the rounded box): the queued message when there is one, the chips when there
/// are some, the text area keyed `mr-input`, then the attach button, the pickers and the send or
/// stop button; under the box, the line that says the session's folder is gone, after a message
/// could not wake it, and the one that says the file picker could not be used. The text grows
/// only as far as the tile's height allows ([`lines_for`]). The pickers' lists go into `layer`.
/// The area is `width` points wide and centred, as the conversation's rows are (the whole width
/// while the tile's width is not known yet).
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
    if let Some(queued) = session.and_then(|s| s.queued.as_ref()) {
        frame = frame.child(queued_strip(key, queued, kit));
    }
    if let Some(session) = session.filter(|s| !s.chips.is_empty()) {
        frame = frame.child(chips::chips(ui, kit, key, &session.chips));
    }
    let plan_mode = ui.app.capabilities.plan_mode;
    let effort = effort_picker(key, session, view, kit, layer);
    let room = width.map(|width| model_room(width, effort.is_some()));
    let mut controls = row()
        .key("mr-input-controls")
        .w_full()
        .gap(CONTROLS_GAP)
        .center_y()
        .child(attach_button(ui, key, kit))
        .child(model_picker(key, session, view, kit, layer, room));
    if let Some(effort) = effort {
        controls = controls.child(effort);
    }
    // The mode picker sits at the start of the room the others leave, and is cut short in it
    // first: the room takes what is free and gives none of it back until it is down to
    // `MODE_MIN`, so the effort picker shrinks only after that.
    let mode_slot = row()
        .key("mr-mode-slot")
        .grow(1.0)
        .min_w(MODE_MIN)
        .center_y()
        .child(mode_picker(key, session, view, plan_mode, kit, layer));
    controls = controls
        .child(mode_slot)
        .child(send_or_stop(ui, key, view, phase, kit));
    let lines = lines_for(
        tiles::tile_height(ui, key),
        extra_height(session, view, kit),
    );
    frame = frame
        .child(text_area(key, view, ended, lines, kit))
        .child(controls);
    let area = col().key("mr-input-area");
    let area = match width {
        Some(width) => area.w(width).self_center(),
        None => area.w_full(),
    };
    let area = area.px(AREA_PAD_X).pb(AREA_PAD_BOTTOM).child(frame);
    let area = match &view.folder_gone {
        Some(folder) => area.child(folder_gone(folder, kit)),
        None => area,
    };
    match &view.attach_error {
        Some(error) => area.child(under_box(error.clone(), "mr-attach-error", kit)),
        None => area,
    }
}

/// The line under the box, keyed `mr-folder-gone`, after a message could not wake the session
/// because its folder is no longer there: in the small type and the error colour of the folder
/// picker's error line.
fn folder_gone(folder: &std::path::Path, kit: &Kit) -> El {
    let words = format!(
        "This session's folder no longer exists: {}",
        folder.display()
    );
    under_box(words, "mr-folder-gone", kit)
}

/// A line under the box keyed `key`, in the small type and the error colour of the folder
/// picker's error line.
fn under_box(words: String, key: &str, kit: &Kit) -> El {
    text(words)
        .key(key.to_owned())
        .font(kit.ty.small)
        .color(kit.c.danger.fg)
        .wrap()
        .w_full()
        .pt(UNDER_BOX_PAD)
        .px(8.0)
}

/// The room the model picker has in a column `width` wide, with or without an effort picker:
/// what the box holds beside the attach button, the send or stop button and the gaps after the
/// first and before the last, less [`MODE_MIN`] for the mode picker, and with `effort` less
/// [`EFFORT_MIN`] and a gap more. So the model's name is cut last. The attach button is as
/// large as the send button.
/// `AREA_PAD_X`, `BOX_PAD` and `CONTROLS_GAP` are this app's; `SEND_SIZE` and part of
/// `MODE_MIN` copy the kit (see each), so a change of those kit sizes makes this cap a little
/// off. The tests `in_the_smallest_window_*` measure the result.
fn model_room(width: f32, effort: bool) -> f32 {
    let inside = width - 2.0 * AREA_PAD_X - 2.0 * BOX_PAD;
    let others = if effort {
        EFFORT_MIN + 4.0 * CONTROLS_GAP
    } else {
        3.0 * CONTROLS_GAP
    };
    (inside - 2.0 * SEND_SIZE - MODE_MIN - others).max(MODE_MIN)
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
/// then "· N attached" when chips wait with it ("Queued: N attached" without text), with
/// "Edit", which takes it back into the box.
fn queued_strip(key: SessionKey, queued: &Outgoing, kit: &Kit) -> El {
    let first = queued.text.lines().next().unwrap_or_default();
    let words = match queued.chips.len() {
        0 => format!("Queued: {first}"),
        count if queued.text.is_empty() => format!("Queued: {count} attached"),
        count => format!("Queued: {first} · {count} attached"),
    };
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
            text(words)
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
/// between it and what was already typed, and focuses the text area. Its chips go back on the
/// session (the core puts them there); a message of chips alone leaves the draft as it was.
/// Taking it back is an act on the session: it is no longer ready (`App::edit_queued`).
fn edit_queued(ui: &mut Ui, key: SessionKey) {
    let (effects, taken) = ui.app.edit_queued(key);
    glue::run(ui, effects);
    let Some(message) = taken else {
        return;
    };
    let draft = &mut glue::view_mut(ui, key).draft;
    if !message.is_empty() {
        let text = if draft.text.is_empty() {
            message
        } else {
            format!("{message}\n{}", draft.text)
        };
        draft.replace_text(text);
    }
    draft.focused = true;
}

/// The text area of `key`'s session, showing the draft in `view` and growing to `lines` lines
/// before it scrolls: Enter submits to that session, Shift+Enter adds a line, Escape stops its
/// running turn, and a paste of an image or of copied files adds chips to it ([`pasted`]).
/// Typing in it clears the session's ready mark (`App::attend`).
///
/// Once the session has ended it is disabled and empty, and reads "This session has ended" where
/// the placeholder goes; the draft is kept for a restart. Those words are a text of their own
/// (`mr-input-ended`) drawn as the kit draws a placeholder, because Rune does not list a field's
/// placeholder among the window's texts; the field is named by them for assistive technology.
fn text_area(key: SessionKey, view: &SessionView, ended: bool, lines: usize, kit: &Kit) -> El {
    let lines = (LINES.0, lines);
    if ended {
        let empty = TextEdit::new("");
        return field::field(kit, &empty, "", PLACEHOLDER_ENDED, lines, true, |e| e).child(
            text(PLACEHOLDER_ENDED)
                .key("mr-input-ended")
                .font(field::style(kit))
                .color(kit.c.text_faint)
                .absolute(field::PAD, field::PAD)
                .a11y_hidden(),
        );
    }
    let handlers = move |e: El| {
        e.on_edit(move |ui: &mut Ui| &mut glue::view_mut(ui, key).draft)
            // Rune calls this only for the user's own typing, paste or cut, never for text the
            // app puts in the box: typing there is an act on the session. It comes with every
            // key, so it does its work only while there is something to clear.
            .on_input_change(move |ui: &mut Ui, _tx: &mut Tx| {
                if ui.app.sessions.get(key).is_some_and(|entry| entry.unseen) {
                    let effects = ui.app.attend(key);
                    glue::run(ui, effects);
                }
            })
            .on_submit(move |ui: &mut Ui, _tx: &mut Tx| submit(ui, key))
            .on_input_key(move |ui: &mut Ui, ev: &KeyEvent, _tx: &mut Tx| {
                is_escape(ev) && stop_turn(ui, key)
            })
            .on_input_paste(
                move |ui: &mut Ui, content: &ClipboardContent, tx: &mut Tx| {
                    pasted(ui, tx, key, content)
                },
            )
    };
    field::field(
        kit,
        &view.draft,
        PLACEHOLDER,
        "Message",
        lines,
        false,
        handlers,
    )
}

/// What a paste into `key`'s box does with `content`. True when the app took it.
///
/// Copied files become chips, and the paths the clipboard may also hold as text are not pasted;
/// else text with a character other than white space is pasted as text, also beside an image;
/// else an image becomes a chip, and white space beside it is not pasted. A session that takes
/// no message now takes nothing. What is typed and the caret stay as they were.
pub fn pasted(ui: &mut Ui, tx: &mut Tx, key: SessionKey, content: &ClipboardContent) -> bool {
    take_paste(ui, tx, key, content, true)
}

/// What a paste no text field took does for `key`'s session (`view::paste`): copied files
/// become chips, else an image does, also beside text; text alone is not taken. A session that
/// takes no message now takes nothing.
pub fn pasted_outside(ui: &mut Ui, tx: &mut Tx, key: SessionKey, content: &ClipboardContent) {
    take_paste(ui, tx, key, content, false);
}

/// [`pasted`] when `text_first`, else [`pasted_outside`].
fn take_paste(
    ui: &mut Ui,
    tx: &mut Tx,
    key: SessionKey,
    content: &ClipboardContent,
    text_first: bool,
) -> bool {
    if !ui.app.takes_message(key) {
        return false;
    }
    if !content.files().is_empty() {
        let sources = content.files().iter().cloned().map(Source::Path).collect();
        glue::attach(ui, tx, key, sources);
        return true;
    }
    if text_first && !content.text().trim().is_empty() {
        return false;
    }
    let Some(image) = content.image() else {
        return false;
    };
    // Rune hands out the PNG only as a slice: this is the one copy of its bytes.
    let data = ImageData::new(image.png());
    glue::attach(ui, tx, key, vec![Source::Pasted(data)]);
    true
}

/// Puts `message`, which `key`'s session did not take, back at the end of what is typed in its
/// input box, on a line of its own when something is typed there, with the caret after it, and
/// focuses the box. What was typed stays undoable. An empty message (one of chips alone, which
/// the core puts back on the session) leaves the draft as it was.
pub fn give_back(ui: &mut Ui, key: SessionKey, message: &str) {
    let draft = &mut glue::view_mut(ui, key).draft;
    draft.focused = true;
    if message.is_empty() {
        return;
    }
    draft.set_caret(draft.text.len());
    if draft.text.is_empty() {
        draft.insert(message);
    } else {
        draft.insert(&format!("\n{message}"));
    }
}

/// Sends the draft of `key`'s session to it with the session's chips, or queues them there while
/// a turn runs or the session wakes, and clears the draft. Nothing goes unless `App::can_send`
/// says so: the session takes a message now (it has not ended, no question about it waits, its
/// check in the store does not run), every chip is ready, and there is text or a chip.
/// Otherwise the draft stays as it is. So does one for a resting session whose folder is no
/// longer there: no program could start in it, so the line under the box says so instead.
/// That is checked here, as the user sends, never as the box is drawn.
fn submit(ui: &mut Ui, key: SessionKey) {
    let text = glue::view_of(ui, key).draft.text.clone();
    if !ui.app.can_send(key, &text) {
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
            let enabled = ui.app.can_send(key, &view.draft.text);
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

/// The button that attaches a file to `key`'s session, keyed `mr-attach`: as large as the send
/// button, disabled while the session takes no message. As the send button, it is a Tab stop
/// that Space presses, and a click hands the keyboard back to the text area.
fn attach_button(ui: &Ui, key: SessionKey, kit: &Kit) -> El {
    IconButton::new(&PLUS)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .help(ATTACH)
        .disabled(!ui.app.takes_message(key))
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
            pick_file(ui, tx, key);
            back_to_typing(ui, key, tx);
        })
        .build(kit)
        .key("mr-attach")
}

/// The desktop's file picker, in the folder of `key`'s session, unless it is open already: then
/// the press does nothing. The file chosen is attached to the session and a cancel adds
/// nothing; either takes the session's error line away. An error is said under the box, in
/// place of an earlier one, until the next answer.
///
/// The answer goes to `key`'s session wherever it is: when its tile closed meanwhile, the chip
/// is added all the same and the session is kept, as one with chips is; when the session takes
/// no message any more, the file is dropped.
fn pick_file(ui: &mut Ui, tx: &mut Tx, key: SessionKey) {
    if ui.file_picker_open {
        return;
    }
    let Some(entry) = ui.app.sessions.get(key) else {
        return;
    };
    let options = FileDialogOptions {
        title: ATTACH.into(),
        directory: Some(entry.session.folder.clone()),
        ..FileDialogOptions::default()
    };
    ui.file_picker_open = true;
    tx.open_file(options, move |ui: &mut Ui, result, tx: &mut Tx| {
        ui.file_picker_open = false;
        let error = match result {
            Ok(Some(path)) => {
                glue::attach(ui, tx, key, vec![Source::Path(path)]);
                None
            }
            Ok(None) => None,
            Err(error) => Some(format!("The file picker could not be opened: {error}")),
        };
        if ui.app.sessions.get(key).is_some() {
            glue::view_mut(ui, key).attach_error = error;
        }
    });
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
    let picker = Select::new(picker_key("mr-model", key), &names)
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
    // In a narrow window the mode and effort pickers give up width first. A model label
    // longer than the room left beside them is cut short inside the trigger, so the send or
    // stop button stays inside the box.
    match room {
        Some(room) => picker.max_w(room),
        None => picker,
    }
}

/// The effort picker, or none for a model that takes no level.
///
/// It shows the level in use by name, or "Effort" while none is known, and lists the levels
/// the model in use takes, in their order. Choosing one asks for it: while a program runs the
/// trigger changes once the program says, at rest at once. It never moves the tile focus, and as
/// its list shuts the keyboard goes back to the box the user types in ([`effort_shut`]).
fn effort_picker(
    key: SessionKey,
    session: Option<&Session>,
    view: &SessionView,
    kit: &Kit,
    layer: &mut Vec<El>,
) -> Option<El> {
    let session = session?;
    let levels = session.effort_levels().to_vec();
    if levels.is_empty() {
        return None;
    }
    let labels: Vec<String> = levels.iter().map(|l| Session::effort_label(l)).collect();
    let current = session
        .effort
        .as_deref()
        .map_or_else(|| "Effort".to_owned(), Session::effort_label);
    let picker = Select::new(picker_key("mr-effort", key), &labels)
        .label("Effort")
        .current(Some(&current))
        .placement(Placement::TopStart)
        .disabled(!picker_enabled(Some(session)))
        .open(view.open_picker == Some(Picker::Effort))
        .on_toggle(move |ui: &mut Ui| toggle(ui, key, Picker::Effort))
        .on_close(move |ui: &mut Ui| effort_shut(ui, key))
        .on_choose(move |ui: &mut Ui, label: &str, _tx: &mut Tx| {
            if let Some(level) = levels.iter().find(|l| Session::effort_label(l) == label) {
                let effects = ui.app.choose_effort(key, level);
                glue::run(ui, effects);
            }
            effort_shut(ui, key);
        })
        .build(kit, layer)
        // In a narrow window it gives up width after the mode picker, its label cut short.
        .shrink()
        .min_w(EFFORT_MIN);
    Some(picker)
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
    Select::new(picker_key("mr-mode", key), &labels)
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
        // In a narrow window the mode picker gives up width first, its label cut short inside
        // the room it sits in, so the model's name stays whole and the send or stop button
        // stays inside the box.
        .shrink()
        .min_w(0.0)
}

/// `{name}-{n}`, the key of a picker of `key`'s session. The kit anchors a picker's list to its
/// trigger, and gives the focus back to it, by that key alone, the first in the window: so it
/// carries the session's number, which tells two tiles' pickers apart.
fn picker_key(name: &str, key: SessionKey) -> String {
    format!("{name}-{}", key.0)
}

/// The pickers work while the session can take a command: not while it opens, nor once ended.
fn picker_enabled(session: Option<&Session>) -> bool {
    session.is_some_and(|s| !matches!(s.phase, Phase::Opening | Phase::Ended { .. }))
}

/// Opens `picker` of `key`'s session, closing the other ones and the picker of every other
/// session's view, and focuses its tile (but for the effort picker, which never moves the
/// tile focus); or closes it when it is open.
fn toggle(ui: &mut Ui, key: SessionKey, picker: Picker) {
    let opens = glue::view_of(ui, key).open_picker != Some(picker);
    if opens {
        if picker != Picker::Effort {
            glue::focus_tile(ui, key, false);
        }
        for (other, view) in &mut ui.views {
            if *other != key {
                view.open_picker = None;
            }
        }
    }
    glue::view_mut(ui, key).open_picker = opens.then_some(picker);
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

/// The effort list of `key`'s session shuts, by a choice, Escape or a press outside, and the
/// keyboard goes back to the box the user types in: the focused tile's, which a press on the
/// effort picker leaves where it is. That is `key`'s own box when its tile is the focused one.
///
/// The kit would hand the keyboard to the picker's trigger (`focus_return`), but Rune lets a
/// text field asked for in the same commit win over that hand-back, and then does not return
/// the focus at all once it is outside the shut list.
fn effort_shut(ui: &mut Ui, key: SessionKey) {
    glue::view_mut(ui, key).open_picker = None;
    let typing = ui.app.focused().unwrap_or(key);
    glue::type_in(ui, typing);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mr_agent::{
        Attachment, CommandKind, FakeStep, PermissionMode, SessionCommand, SessionEvent,
        TurnSummary,
    };
    use mr_core::{ChipState, Leaving, Place, Session, SessionKey, Source};
    use rune::{
        ClipboardContent, ClipboardImage, FileDialogKind, FileDialogOptions, Key, Mods, Runtime, Tx,
    };

    use super::{
        AROUND_TEXT, CONVERSATION_MIN, EFFORT_MIN, MODE_MIN, extra_height, lines_for, mode_choices,
        pasted, toggle,
    };
    use crate::demo::NOW_UNIX;
    use crate::glue::{self, Picker, Ui};
    use crate::view::{field, tiles};
    use crate::{testkit, theme};

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
    fn typing_in_the_box_of_a_ready_session_clears_it() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        let unseen = |rt: &Runtime<Ui>| rt.state.app.sessions.get(key).unwrap().unseen;
        testkit::type_text(&mut rt, "go");
        testkit::press(&mut rt, Key::Enter);
        // The turn ends while another app has the keyboard: ready in the focused tile.
        rt.update(|ui, tx| glue::focus_changed(ui, false, tx));
        testkit::emit(
            &mut rt,
            SessionEvent::TurnFinished(TurnSummary {
                cost_usd: 0.0,
                context_used: None,
                duration_ms: 1,
                interrupted: false,
            }),
        );
        rt.update(|ui, tx| glue::focus_changed(ui, true, tx));
        testkit::step(&mut rt);
        assert!(unseen(&rt), "the keyboard back clears nothing");
        assert!(testkit::box_focused(&rt, key), "the box kept the keyboard");
        testkit::type_text(&mut rt, "n");
        assert!(!unseen(&rt), "typed");
        // A paste counts as typing.
        rt.state.app.sessions.get_mut(key).unwrap().unseen = true;
        rt.paste_content(ClipboardContent::new("ext"));
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).draft.text, "next");
        assert!(!unseen(&rt), "pasted");
    }

    #[test]
    fn edit_on_the_queued_message_clears_ready() {
        let mut rt = testkit::window("working");
        let key = testkit::focused(&rt).unwrap();
        testkit::type_text(&mut rt, "then run the tests");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        rt.state.app.sessions.get_mut(key).unwrap().unseen = true;
        testkit::click(&mut rt, &testkit::tile(key, "mr-queued-edit"));
        testkit::step(&mut rt);
        assert_eq!(
            testkit::shown_view(&rt).draft.text,
            "then run the tests",
            "the queued message is back in the box"
        );
        assert!(
            !rt.state.app.sessions.get(key).unwrap().unseen,
            "taking it back is the user's act"
        );
    }

    #[test]
    fn text_put_back_by_the_app_does_not_clear_ready() {
        let mut rt = testkit::window("working");
        let key = testkit::focused(&rt).unwrap();
        rt.state.app.sessions.get_mut(key).unwrap().unseen = true;
        // As the app puts a message back that was not taken: no typing, no paste.
        rt.update(|ui, _| {
            glue::view_mut(ui, key).draft.replace_text("carry on");
            glue::run(ui, Vec::new());
        });
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).draft.text, "carry on");
        assert!(rt.state.app.sessions.get(key).unwrap().unseen);
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
            rt.state.focused_session().unwrap().model_label(),
            DEFAULT_LABEL
        );
        assert!(testkit::has_text(&rt, DEFAULT_LABEL));
        let picker = testkit::model_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
        testkit::step(&mut rt);
        testkit::click_text(&mut rt, "Fast");
        testkit::step(&mut rt);
        assert!(testkit::sent(&rt).contains(&SessionCommand::SetModel("fast".into())));
        assert_eq!(
            rt.state.focused_session().unwrap().model_label(),
            DEFAULT_LABEL,
            "not until the agent confirms"
        );
        testkit::emit(
            &mut rt,
            SessionEvent::ModelChanged("fake-model-fast".into()),
        );
        assert_eq!(rt.state.focused_session().unwrap().model_label(), "Fast");
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
        assert_eq!(rt.state.focused_session().unwrap().model_label(), "Fast");
        let picker = testkit::model_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
        testkit::step(&mut rt);
        let listed: Vec<String> = rt
            .elements()
            .into_iter()
            .filter(|e| {
                e.path.contains(&testkit::model_key(&rt, "content"))
                    && e.key.as_deref() == Some("label")
            })
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
        let picker = testkit::mode_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
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
        let picker = testkit::mode_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
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
        assert_eq!(
            rt.state.focused_session().unwrap().mode,
            PermissionMode::Ask
        );
        let shown = rt
            .elements()
            .into_iter()
            .find(|e| {
                e.path
                    .contains(&format!("/{}/", testkit::mode_key(&rt, "trigger")))
                    && e.text.is_some()
            })
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
        let picker = testkit::mode_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "Plan"));
        assert!(!testkit::texts(&rt).iter().any(|t| t == "Auto"));
    }

    /// The key of `part` of `key`'s effort picker: `mr-effort-{n}-{part}`.
    fn effort(key: SessionKey, part: &str) -> String {
        format!("mr-effort-{}-{part}", key.0)
    }

    /// The key of `part` of the focused session's effort picker.
    fn effort_key(rt: &Runtime<Ui>, part: &str) -> String {
        effort(testkit::focused(rt).expect("a session is shown"), part)
    }

    /// The text the picker whose trigger is keyed `trigger` shows, and where it is drawn.
    fn shown(rt: &Runtime<Ui>, trigger: &str) -> Option<(String, [f32; 4])> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains(&format!("/{trigger}/")))
            .find_map(|e| e.text.map(|text| (text, e.rect)))
    }

    /// The text `key`'s effort picker shows.
    fn effort_shown(rt: &Runtime<Ui>, key: SessionKey) -> Option<String> {
        shown(rt, &effort(key, "trigger")).map(|(text, _)| text)
    }

    /// The commands the fake received for `key`'s session, in order.
    fn sent_to(rt: &Runtime<Ui>, key: SessionKey) -> Vec<SessionCommand> {
        let id = rt.state.app.sessions.get(key).unwrap().session.id.clone();
        let fake = rt
            .state
            .fake
            .as_ref()
            .expect("the demo window has its fake");
        fake.sent()
            .into_iter()
            .filter(|(to, _)| Some(to) == id.as_ref())
            .map(|(_, command)| command)
            .collect()
    }

    /// Opens `key`'s effort picker with a click and chooses `label` from its list.
    fn choose_effort(rt: &mut Runtime<Ui>, key: SessionKey, label: &str) {
        testkit::click(rt, &effort(key, "trigger"));
        testkit::step(rt);
        assert_eq!(
            glue::view_of(&rt.state, key).open_picker,
            Some(Picker::Effort)
        );
        testkit::click_text(rt, label);
        testkit::step(rt);
    }

    /// The labels of the open list keyed `content`, in order.
    fn listed(rt: &Runtime<Ui>, content: &str) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains(content) && e.key.as_deref() == Some("label"))
            .filter_map(|e| e.text)
            .collect()
    }

    #[test]
    fn the_effort_picker_shows_the_level_and_changes_it() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        assert_eq!(effort_shown(&rt, key).as_deref(), Some("Medium"));
        choose_effort(&mut rt, key, "High");
        assert_eq!(
            sent_to(&rt, key).last(),
            Some(&SessionCommand::SetEffort("high".into()))
        );
        assert_eq!(testkit::shown_view(&rt).open_picker, None);
        assert_eq!(
            effort_shown(&rt, key).as_deref(),
            Some("Medium"),
            "not until the program says"
        );
        testkit::emit(&mut rt, SessionEvent::EffortChanged(Some("high".into())));
        assert_eq!(effort_shown(&rt, key).as_deref(), Some("High"));
    }

    #[test]
    fn the_effort_picker_lists_the_models_levels_by_name() {
        let mut rt = testkit::window("empty");
        let trigger = effort_key(&rt, "trigger");
        testkit::click(&mut rt, &trigger);
        testkit::step(&mut rt);
        let list = effort_key(&rt, "content");
        assert_eq!(listed(&rt, &list), ["Low", "Medium", "High"]);
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        // A model that takes all five, in the order the model gives them.
        let mut entry = rt.state.focused_entry_mut().unwrap();
        entry.session.models[0].effort_levels = ["max", "low", "medium", "high", "xhigh"]
            .map(String::from)
            .to_vec();
        drop(entry);
        rt.invalidate();
        testkit::step(&mut rt);
        let trigger = effort_key(&rt, "trigger");
        testkit::click(&mut rt, &trigger);
        testkit::step(&mut rt);
        assert_eq!(
            listed(&rt, &list),
            ["Max", "Low", "Medium", "High", "Extra high"]
        );
    }

    #[test]
    fn a_model_without_levels_shows_no_effort_picker() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        testkit::click(&mut rt, &effort(key, "trigger"));
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).open_picker, Some(Picker::Effort));
        testkit::emit(
            &mut rt,
            SessionEvent::ModelChanged("fake-model-fast".into()),
        );
        assert_eq!(testkit::shown_view(&rt).open_picker, None, "its list shut");
        assert!(!testkit::drawn(&rt, &effort(key, "trigger")));
        assert!(testkit::drawn(&rt, &testkit::model_key(&rt, "trigger")));
        assert!(testkit::drawn(&rt, &testkit::mode_key(&rt, "trigger")));
        // Back on a model that takes levels, the picker is back and its list stays shut.
        testkit::emit(&mut rt, SessionEvent::ModelChanged("fake-model".into()));
        assert!(testkit::drawn(&rt, &effort(key, "trigger")));
        assert_eq!(testkit::shown_view(&rt).open_picker, None);
        assert!(!testkit::drawn(&rt, &effort(key, "content")));
    }

    #[test]
    fn an_effort_not_known_yet_reads_effort() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        rt.state.focused_entry_mut().unwrap().session.effort = None;
        rt.invalidate();
        testkit::step(&mut rt);
        assert_eq!(effort_shown(&rt, key).as_deref(), Some("Effort"));
    }

    #[test]
    fn a_resting_sessions_effort_is_chosen_without_a_program() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
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
            rt.state.focused_session().unwrap().phase,
            mr_core::Phase::Resting
        );
        let before = testkit::sent(&rt).len();
        choose_effort(&mut rt, key, "Low");
        assert_eq!(testkit::sent(&rt).len(), before, "nothing is sent");
        assert_eq!(effort_shown(&rt, key).as_deref(), Some("Low"), "at once");
        assert_eq!(
            rt.state.focused_session().unwrap().phase,
            mr_core::Phase::Resting,
            "no program was started"
        );
    }

    #[test]
    fn an_effort_that_was_not_taken_leaves_the_trigger_and_adds_a_line() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        choose_effort(&mut rt, key, "High");
        testkit::emit(
            &mut rt,
            SessionEvent::CommandFailed {
                what: CommandKind::SetEffort,
                reason: "the level is set in the environment".into(),
            },
        );
        assert!(testkit::has_text(
            &rt,
            "Could not change the effort level: the level is set in the environment"
        ));
        assert_eq!(effort_shown(&rt, key).as_deref(), Some("Medium"));
    }

    #[test]
    fn choosing_an_effort_keeps_the_keyboard_in_the_box_and_the_tile_focused() {
        let mut rt = testkit::window_sized("empty", (1800.0, 900.0));
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        assert_eq!(testkit::tile_keys(&rt), [a, b]);
        assert!(testkit::box_focused(&rt, b));
        // In the focused tile.
        choose_effort(&mut rt, b, "High");
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(b));
        assert!(testkit::box_focused(&rt, b), "the keyboard is in b's box");
        // In the other tile: the focus stays on b, and the keyboard goes back to b's box.
        choose_effort(&mut rt, a, "Low");
        testkit::step(&mut rt);
        assert_eq!(
            sent_to(&rt, a).last(),
            Some(&SessionCommand::SetEffort("low".into()))
        );
        assert_eq!(
            testkit::focused(&rt),
            Some(b),
            "the tile focus did not move"
        );
        assert!(
            testkit::box_focused(&rt, b),
            "the keyboard is back in b's box"
        );
        assert!(!testkit::box_focused(&rt, a));
        testkit::type_text(&mut rt, "typed");
        assert_eq!(glue::view_of(&rt.state, b).draft.text, "typed");
    }

    #[test]
    fn two_tiles_each_choose_their_own_effort() {
        let mut rt = testkit::window_sized("empty", (1800.0, 900.0));
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        // Opening a's effort list shuts b's model list.
        testkit::click(&mut rt, &format!("mr-model-{}-trigger", b.0));
        testkit::step(&mut rt);
        assert_eq!(glue::view_of(&rt.state, b).open_picker, Some(Picker::Model));
        toggle(&mut rt.state, a, Picker::Effort);
        rt.invalidate();
        testkit::step(&mut rt);
        assert_eq!(glue::view_of(&rt.state, b).open_picker, None);
        assert_eq!(
            glue::view_of(&rt.state, a).open_picker,
            Some(Picker::Effort)
        );
        toggle(&mut rt.state, a, Picker::Effort);
        rt.invalidate();
        for _ in 0..20 {
            testkit::step(&mut rt);
        }
        choose_effort(&mut rt, a, "High");
        assert_eq!(
            sent_to(&rt, a).last(),
            Some(&SessionCommand::SetEffort("high".into()))
        );
        assert!(
            !sent_to(&rt, b)
                .iter()
                .any(|c| matches!(c, SessionCommand::SetEffort(_)))
        );
        testkit::emit_to(&mut rt, a, SessionEvent::EffortChanged(Some("high".into())));
        choose_effort(&mut rt, b, "Low");
        assert_eq!(
            sent_to(&rt, b).last(),
            Some(&SessionCommand::SetEffort("low".into()))
        );
        testkit::emit_to(&mut rt, b, SessionEvent::EffortChanged(Some("low".into())));
        assert_eq!(effort_shown(&rt, a).as_deref(), Some("High"));
        assert_eq!(effort_shown(&rt, b).as_deref(), Some("Low"));
    }

    #[test]
    fn in_a_tile_of_420_points_the_model_and_the_effort_are_whole() {
        let rt = testkit::window_sized("empty", (688.0, 800.0));
        let key = testkit::focused(&rt).unwrap();
        let tile = rt.presented(&crate::view::tile_key(key)).unwrap().0;
        assert_eq!(tile[2], 420.0, "the tile is 420 points wide");
        let roomy = testkit::window("empty");
        let whole = |trigger: &str| shown(&roomy, trigger).expect("a label").1[2];
        let model_trigger = testkit::model_key(&rt, "trigger");
        let (model, at) = shown(&rt, &model_trigger).expect("the model's label");
        assert_eq!(model, DEFAULT_LABEL);
        assert_eq!(at[2], whole(&model_trigger), "the model's name is not cut");
        let effort_trigger = effort(key, "trigger");
        let (level, at) = shown(&rt, &effort_trigger).expect("the effort's label");
        assert_eq!(level, "Medium");
        assert_eq!(at[2], whole(&effort_trigger), "the level is not cut");
        let mode = rect(&rt, &testkit::mode_key(&rt, "trigger"));
        assert!(mode[2] >= MODE_MIN - 0.5, "the mode keeps its least width");
    }

    /// Whether the keyboard focus is on `key`'s effort trigger.
    fn effort_trigger_focused(rt: &Runtime<Ui>, key: SessionKey) -> bool {
        let trigger = effort(key, "trigger");
        rt.elements()
            .into_iter()
            .any(|e| !e.exiting && e.focused && e.key.as_deref() == Some(trigger.as_str()))
    }

    /// After a's effort list shut with b's tile focused: in the frame it shut, and after more
    /// frames (so the kit's hand-back to the trigger never comes late), b's box has the
    /// keyboard, a's trigger has not, b is still the focused tile, and typing lands in b.
    fn keyboard_back_in_b(rt: &mut Runtime<Ui>, a: SessionKey, b: SessionKey) {
        for frame in 0..3 {
            assert_eq!(glue::view_of(&rt.state, a).open_picker, None);
            assert!(testkit::box_focused(rt, b), "b's box, frame {frame}");
            assert!(
                !effort_trigger_focused(rt, a),
                "not a's trigger, frame {frame}"
            );
            assert_eq!(testkit::focused(rt), Some(b));
            testkit::step(rt);
        }
        testkit::type_text(rt, "typed");
        assert_eq!(glue::view_of(&rt.state, b).draft.text, "typed");
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "");
    }

    /// Two tiles side by side, b focused with the keyboard in its box, and a's effort list
    /// opened with a click.
    fn effort_open_in_the_other_tile() -> (Runtime<Ui>, SessionKey, SessionKey) {
        let mut rt = testkit::window_sized("empty", (1800.0, 900.0));
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        assert!(testkit::box_focused(&rt, b));
        testkit::click(&mut rt, &effort(a, "trigger"));
        testkit::step(&mut rt);
        assert_eq!(
            glue::view_of(&rt.state, a).open_picker,
            Some(Picker::Effort)
        );
        assert!(
            !testkit::box_focused(&rt, b),
            "the open list holds the keyboard"
        );
        (rt, a, b)
    }

    #[test]
    fn escape_in_the_effort_list_gives_the_keyboard_back_to_the_box() {
        let (mut rt, a, b) = effort_open_in_the_other_tile();
        testkit::press(&mut rt, Key::Escape);
        keyboard_back_in_b(&mut rt, a, b);
    }

    #[test]
    fn a_press_outside_the_effort_list_gives_the_keyboard_back_to_the_box() {
        let (mut rt, a, b) = effort_open_in_the_other_tile();
        // On the empty middle of a's own conversation, under the list's scrim: were the press
        // to reach the tile, it would focus a.
        let [x, y, w, h] = rt.presented(&crate::view::tile_key(a)).unwrap().0;
        let (x, y) = (x + w / 2.0, y + h / 3.0);
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.016);
        keyboard_back_in_b(&mut rt, a, b);
    }

    #[test]
    fn escape_in_the_effort_list_closes_it_and_the_turn_runs_on() {
        let mut rt = testkit::window("working");
        let trigger = effort_key(&rt, "trigger");
        testkit::click(&mut rt, &trigger);
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).open_picker, Some(Picker::Effort));
        // The window's keys wait while it is open.
        testkit::press_with(&mut rt, Key::Command('n'), testkit::ctrl_shift());
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_menu, None);
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).open_picker, None);
        assert!(!testkit::sent(&rt).contains(&SessionCommand::Stop));
        assert_eq!(
            rt.state.focused_session().unwrap().phase,
            mr_core::Phase::Working
        );
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
            rt.state.focused_session().unwrap().phase,
            mr_core::Phase::Resting
        );
        assert!(!testkit::has_text(&rt, "This session has ended"));
        testkit::type_text(&mut rt, "hello");
        testkit::press(&mut rt, Key::Enter);
        let session = rt.state.focused_session().unwrap();
        assert_eq!(session.phase, mr_core::Phase::Opening);
        assert_eq!(
            session.queued.as_ref().map(|queued| queued.text.as_str()),
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
        let effects = app.show(
            key,
            mr_core::Place::Focused,
            mr_core::Leaving::ForgetIfEmpty,
        );
        glue::run(&mut rt.state, effects);
        testkit::step(&mut rt);
        testkit::type_text(&mut rt, "hello");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        testkit::click(&mut rt, "mr-send");
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).draft.text, "hello");
        assert_eq!(
            rt.state.focused_session().unwrap().phase,
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

    /// The rectangles of `mr-input` and `mr-input-box` in the empty scene's window with `draft`
    /// typed, Shift+Enter between its lines: the field's one step after the typing, while its
    /// height moves, then both once it has settled.
    fn field_rects(draft: &str) -> [[f32; 4]; 3] {
        let mut rt = testkit::window("empty");
        for (n, line) in draft.lines().enumerate() {
            if n > 0 {
                testkit::press_with(
                    &mut rt,
                    Key::Enter,
                    Mods {
                        shift: true,
                        ..Mods::default()
                    },
                );
            }
            testkit::type_text(&mut rt, line);
        }
        testkit::step(&mut rt);
        let moving = rect(&rt, "mr-input");
        for _ in 0..40 {
            testkit::step(&mut rt);
        }
        [moving, rect(&rt, "mr-input"), rect(&rt, "mr-input-box")]
    }

    /// The numbers were read from the kit's `TextArea` before this app built its own field.
    #[test]
    fn the_field_is_where_the_kits_text_area_was() {
        let one = [
            [292.0, 698.5, 868.0, 33.5],
            [292.0, 698.5, 868.0, 33.5],
            [284.0, 690.5, 884.0, 85.5],
        ];
        let three = [
            [292.0, 677.875, 868.0, 57.4375],
            [292.0, 663.5, 868.0, 68.5],
            [284.0, 655.5, 884.0, 120.5],
        ];
        assert_eq!(field_rects("hello"), one);
        assert_eq!(field_rects("one\ntwo\nthree"), three);
    }

    /// The chips of `rt`'s shown session, by name.
    fn chip_names(rt: &Runtime<Ui>) -> Vec<String> {
        rt.state
            .focused_session()
            .unwrap()
            .chips
            .iter()
            .map(|chip| chip.name.clone())
            .collect()
    }

    /// The empty scene with "look" typed and the caret between its two halves.
    fn caret_in_look() -> Runtime<Ui> {
        let mut rt = testkit::window("empty");
        testkit::type_text(&mut rt, "look");
        testkit::press(&mut rt, Key::Left);
        testkit::press(&mut rt, Key::Left);
        rt
    }

    /// The draft's text and its selection's two ends.
    fn draft(rt: &Runtime<Ui>) -> (String, usize, usize) {
        let draft = &testkit::shown_view(rt).draft;
        (draft.text.clone(), draft.anchor, draft.focus)
    }

    #[test]
    fn a_pasted_image_becomes_a_chip_and_no_text() {
        let mut rt = caret_in_look();
        assert_eq!(draft(&rt), ("look".to_string(), 2, 2));
        let png = testkit::tiny_png();
        testkit::paste_image(&mut rt, png.clone());
        testkit::step(&mut rt);
        assert_eq!(chip_names(&rt), ["Pasted image"]);
        // Once read, the chip lets go of the clipboard's bytes; what it sends is the image.
        chips_read(&mut rt);
        let session = rt.state.focused_session().unwrap();
        assert!(matches!(session.chips[0].source, Source::Pasted(_)));
        let ChipState::Ready(prepared) = &session.chips[0].state else {
            panic!("the pasted image was not read");
        };
        assert!(
            matches!(&prepared.attachment, Attachment::Image { data, .. } if data.as_bytes() == png),
            "the chip sends the pasted image"
        );
        assert_eq!(draft(&rt), ("look".to_string(), 2, 2));
        assert!(text_area_focused(&rt));
    }

    #[test]
    fn pasted_files_become_chips_and_no_text() {
        let mut rt = caret_in_look();
        let paths = [
            PathBuf::from("/home/alex/project/notes.md"),
            PathBuf::from("/home/alex/project/src/main.rs"),
        ];
        testkit::paste_files(&mut rt, &paths);
        testkit::step(&mut rt);
        assert_eq!(chip_names(&rt), ["notes.md", "main.rs"]);
        // A file manager also puts the paths on the clipboard as text: the files still win.
        let more = PathBuf::from("/home/alex/project/plan.txt");
        rt.paste_content(
            ClipboardContent::new("/home/alex/project/plan.txt").with_files([more.clone()]),
        );
        testkit::step(&mut rt);
        assert_eq!(chip_names(&rt), ["notes.md", "main.rs", "plan.txt"]);
        let session = rt.state.focused_session().unwrap();
        assert_eq!(session.chips[2].source, Source::Path(more));
        assert_eq!(draft(&rt), ("look".to_string(), 2, 2));
        assert!(text_area_focused(&rt));
    }

    #[test]
    fn text_is_pasted_as_text_also_beside_an_image() {
        let mut rt = caret_in_look();
        let image = ClipboardImage::from_png(testkit::tiny_png()).unwrap();
        rt.paste_content(ClipboardContent::new("ing ah").with_image(image));
        testkit::step(&mut rt);
        assert!(chip_names(&rt).is_empty());
        assert_eq!(draft(&rt), ("loing ahok".to_string(), 8, 8));
    }

    #[test]
    fn with_no_field_focused_an_image_is_taken_also_beside_text() {
        let mut rt = testkit::window("empty");
        rt.update(|_, tx| tx.blur());
        testkit::step(&mut rt);
        assert!(!text_area_focused(&rt));
        let image = ClipboardImage::from_png(testkit::tiny_png()).unwrap();
        rt.paste_content(ClipboardContent::new("a caption").with_image(image));
        testkit::step(&mut rt);
        assert_eq!(chip_names(&rt), ["Pasted image"]);
        assert_eq!(
            testkit::shown_view(&rt).draft.text,
            "",
            "the text is not taken"
        );
        // Files still come first, and the image beside them is not taken.
        let image = ClipboardImage::from_png(testkit::tiny_png()).unwrap();
        let notes = PathBuf::from("/home/alex/project/notes.md");
        rt.paste_content(
            ClipboardContent::new("a caption")
                .with_files([notes])
                .with_image(image),
        );
        testkit::step(&mut rt);
        assert_eq!(chip_names(&rt), ["Pasted image", "notes.md"]);
    }

    #[test]
    fn an_ended_session_takes_no_paste() {
        let mut rt = testkit::window("ended");
        testkit::paste_image(&mut rt, testkit::tiny_png());
        testkit::paste_files(&mut rt, &[PathBuf::from("/home/alex/project/notes.md")]);
        testkit::step(&mut rt);
        assert!(chip_names(&rt).is_empty());
        // Nor when a paste reaches it some other way.
        let key = rt.state.app.focused().unwrap();
        let image = ClipboardImage::from_png(testkit::tiny_png()).unwrap();
        let files = ClipboardContent::new("").with_files([PathBuf::from("/home/alex/a.md")]);
        let images = ClipboardContent::new("").with_image(image);
        let mut took = Vec::new();
        rt.update(|ui, tx| {
            took.push(pasted(ui, tx, key, &files));
            took.push(pasted(ui, tx, key, &images));
        });
        assert_eq!(took, [false, false]);
        assert!(chip_names(&rt).is_empty());
        assert_eq!(testkit::shown_view(&rt).draft.text, "");
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
        let picker = testkit::mode_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).open_picker, Some(Picker::Mode));
        let picker = testkit::model_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
        testkit::step(&mut rt);
        let picker = testkit::model_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
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
        let picker = testkit::model_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
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
                .any(|e| e.key == Some(testkit::model_key(rt, "scrim")))
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
        assert!(rt.find(&testkit::model_key(&rt, "trigger")).is_some());
        assert!(
            rt.find(&testkit::model_key(&rt, "content")).is_none(),
            "the list stays shut"
        );
    }

    #[test]
    fn opening_a_picker_shuts_every_other_sessions_picker() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        glue::view_mut(&mut rt.state, a).open_picker = Some(Picker::Mode);
        toggle(&mut rt.state, b, Picker::Model);
        assert_eq!(glue::view_of(&rt.state, a).open_picker, None);
        assert_eq!(glue::view_of(&rt.state, b).open_picker, Some(Picker::Model));
        toggle(&mut rt.state, b, Picker::Model);
        assert_eq!(glue::view_of(&rt.state, b).open_picker, None, "shut again");
    }

    #[test]
    fn the_model_picker_shuts_when_its_list_empties() {
        let mut rt = testkit::window("empty");
        let picker = testkit::model_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).open_picker, Some(Picker::Model));
        rt.state.focused_entry_mut().unwrap().session.models.clear();
        // Anything taken in from the agent goes through the glue.
        testkit::emit(&mut rt, SessionEvent::TitleChanged("Renamed".into()));
        assert_eq!(testkit::shown_view(&rt).open_picker, None);
        // The mode picker has its list whatever the models.
        let picker = testkit::mode_key(&rt, "trigger");
        testkit::click(&mut rt, &picker);
        testkit::step(&mut rt);
        testkit::emit(&mut rt, SessionEvent::TitleChanged("Again".into()));
        assert_eq!(testkit::shown_view(&rt).open_picker, Some(Picker::Mode));
    }

    /// The model, effort and mode triggers of the focused session and its input box, as drawn,
    /// after checking that the attach button, the three and the stop button lie inside the box
    /// in that order and do not overlap.
    fn bottom_row(rt: &Runtime<Ui>) -> [[f32; 4]; 4] {
        let frame = rect(rt, "mr-input-box");
        let attach = rect(rt, "mr-attach");
        let model = rect(rt, &testkit::model_key(rt, "trigger"));
        let effort = rect(rt, &effort_key(rt, "trigger"));
        let mode = rect(rt, &testkit::mode_key(rt, "trigger"));
        let stop = rect(rt, "mr-stop");
        let parts = [attach, model, effort, mode, stop];
        for part in parts {
            assert!(
                part[0] >= frame[0] - 0.5 && part[0] + part[2] <= frame[0] + frame[2] + 0.5,
                "{part:?} inside {frame:?}"
            );
        }
        for pair in parts.windows(2) {
            assert!(
                pair[0][0] + pair[0][2] <= pair[1][0] + 0.5,
                "{:?} left of {:?}, not over it",
                pair[0],
                pair[1]
            );
        }
        [model, effort, mode, frame]
    }

    #[test]
    fn in_the_smallest_window_a_long_model_label_is_cut_inside_its_trigger() {
        let mut rt = testkit::window_sized("working", (560.0, 400.0));
        // The label the recorded list gives the default model, the longest seen.
        let mut entry = rt.state.focused_entry_mut().unwrap();
        let session = &mut entry.session;
        session.models[0].name = "Default (recommended)".into();
        session.models[0].stands_for = Some("Fable 5.2".into());
        let label = session.model_label();
        assert_eq!(label, "Default (recommended) · Fable 5.2");
        drop(entry);
        rt.invalidate();
        testkit::step(&mut rt);
        let [model, effort, mode, _] = bottom_row(&rt);
        assert!(
            (mode[2] - MODE_MIN).abs() <= 0.5,
            "the mode picker is at its least width: {mode:?}"
        );
        assert!(
            (effort[2] - EFFORT_MIN).abs() <= 0.5,
            "the effort picker is at its least width: {effort:?}"
        );
        let (shown_label, value) =
            shown(&rt, &testkit::model_key(&rt, "trigger")).expect("the model's label");
        assert_eq!(shown_label, label);
        assert!(
            value[0] + value[2] <= model[0] + model[2] + 0.5,
            "the label is cut inside its trigger: {value:?} in {model:?}"
        );
        let mut roomy = testkit::window("working");
        let mut entry = roomy.state.focused_entry_mut().unwrap();
        let session = &mut entry.session;
        session.models[0].name = "Default (recommended)".into();
        session.models[0].stands_for = Some("Fable 5.2".into());
        drop(entry);
        roomy.invalidate();
        testkit::step(&mut roomy);
        let (_, whole) =
            shown(&roomy, &testkit::model_key(&roomy, "trigger")).expect("the model's label");
        assert!(value[2] < whole[2], "cut short at the smallest size only");
    }

    #[test]
    fn in_the_smallest_window_the_pickers_and_the_button_stay_in_the_box() {
        let mut rt = testkit::window_sized("working", (560.0, 400.0));
        testkit::type_text(
            &mut rt,
            "then run the tests and report what failed, with their names",
        );
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        let [model, effort, mode, _] = bottom_row(&rt);
        assert!(
            (mode[2] - MODE_MIN).abs() <= 0.5,
            "the mode picker is at its least width: {mode:?}"
        );
        assert!(
            (effort[2] - EFFORT_MIN).abs() <= 0.5,
            "the effort picker is at its least width: {effort:?}"
        );
        let roomy = testkit::window("working");
        let whole = |trigger: &str| shown(&roomy, trigger).expect("a label").1;
        let model_trigger = testkit::model_key(&rt, "trigger");
        let (name, value) = shown(&rt, &model_trigger).expect("the model's name");
        assert_eq!(name, DEFAULT_LABEL);
        assert!(
            value[0] + value[2] <= model[0] + model[2] + 0.5,
            "the name is cut inside its trigger: {value:?} in {model:?}"
        );
        assert!(
            value[2] < whole(&model_trigger)[2],
            "the model's name is the one cut, last"
        );
        let (level, value) = shown(&rt, &effort_key(&rt, "trigger")).expect("the level");
        assert_eq!(level, "Medium");
        assert!(
            value[0] + value[2] <= effort[0] + effort[2] + 0.5,
            "the level is cut inside its trigger: {value:?} in {effort:?}"
        );
    }

    /// A file named `name` in `dir`, with a line in it.
    fn file(dir: &std::path::Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "a line\n").expect("a file in the test's folder");
        path
    }

    /// Runs frames until no chip of the focused session is being read.
    fn chips_read(rt: &mut Runtime<Ui>) {
        for _ in 0..20 {
            let session = rt.state.focused_session().unwrap();
            if !session.chips.iter().any(|c| c.state == ChipState::Reading) {
                break;
            }
            testkit::step(rt);
        }
        testkit::step(rt);
    }

    /// The `Send` commands the fake received, as their text and attachments.
    fn sends_with(rt: &Runtime<Ui>) -> Vec<(String, Vec<Attachment>)> {
        testkit::sent(rt)
            .into_iter()
            .filter_map(|c| match c {
                SessionCommand::Send { text, attachments } => Some((text, attachments)),
                _ => None,
            })
            .collect()
    }

    /// Whether the send button of the focused tile can be pressed.
    fn send_on(rt: &Runtime<Ui>) -> bool {
        let key = testkit::focused(rt).unwrap();
        testkit::element(rt, &testkit::tile(key, "mr-send"))
            .expect("the send button")
            .focusable
    }

    #[test]
    fn chips_alone_can_be_sent_with_enter_and_with_the_button() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(dir.path(), "notes.md");
        let sent = vec![(String::new(), vec![Attachment::File { path: path.clone() }])];
        for by_button in [false, true] {
            let mut rt = testkit::window("empty");
            assert!(!send_on(&rt), "nothing to send");
            testkit::press(&mut rt, Key::Enter);
            testkit::step(&mut rt);
            assert_eq!(sends_with(&rt), vec![]);
            testkit::attach_file(&mut rt, &path);
            chips_read(&mut rt);
            assert_eq!(chip_names(&rt), ["notes.md"]);
            assert!(send_on(&rt));
            if by_button {
                let key = testkit::focused(&rt).unwrap();
                testkit::click(&mut rt, &testkit::tile(key, "mr-send"));
            } else {
                testkit::press(&mut rt, Key::Enter);
            }
            testkit::step(&mut rt);
            assert_eq!(sends_with(&rt), sent, "by the button: {by_button}");
            assert!(chip_names(&rt).is_empty());
            assert!(text_area_focused(&rt));
        }
    }

    #[test]
    fn nothing_is_sent_while_a_chip_has_a_problem() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        rt.update(|ui, tx| {
            glue::attach(
                ui,
                tx,
                key,
                vec![Source::Path("/home/alex/no-such-file.txt".into())],
            );
        });
        chips_read(&mut rt);
        let chip = rt.state.focused_session().unwrap().chips[0].clone();
        assert!(matches!(chip.state, ChipState::Problem(_)), "{chip:?}");
        testkit::type_text(&mut rt, "hello");
        assert!(!send_on(&rt));
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(sends(&rt), Vec::<String>::new());
        assert_eq!(testkit::shown_view(&rt).draft.text, "hello");
        testkit::click(
            &mut rt,
            &testkit::tile(key, &format!("mr-chip-remove-{}", chip.id)),
        );
        testkit::step(&mut rt);
        assert!(send_on(&rt));
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(sends(&rt), ["hello".to_string()]);
    }

    #[test]
    fn the_attach_button_adds_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let first = file(dir.path(), "notes.md");
        let second = file(dir.path(), "plan.txt");
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        let button = testkit::element(&rt, &testkit::tile(key, "mr-attach")).unwrap();
        assert_eq!(button.help.as_deref(), Some("Attach a file"));
        assert_eq!(button.label.as_deref(), Some("Attach a file"));
        let send = testkit::element(&rt, &testkit::tile(key, "mr-send")).unwrap();
        assert_eq!(
            button.rect[2..],
            send.rect[2..],
            "as large as the send button"
        );
        assert!(button.rect[0] < send.rect[0]);
        testkit::click(&mut rt, &testkit::tile(key, "mr-attach"));
        let folder = rt.state.focused_session().unwrap().folder.clone();
        let (kind, options) = rt.file_dialog().expect("the file picker is open");
        assert_eq!(kind, FileDialogKind::Open);
        assert_eq!(options.title, "Attach a file");
        assert_eq!(options.directory, Some(folder));
        rt.resolve_file_dialog(Some(first.clone())).unwrap();
        testkit::step(&mut rt);
        assert_eq!(chip_names(&rt), ["notes.md"]);
        assert!(
            testkit::box_focused(&rt, key),
            "the keyboard stays in the box"
        );
        // A second press, a second file.
        testkit::attach_file(&mut rt, &second);
        assert_eq!(chip_names(&rt), ["notes.md", "plan.txt"]);
        let session = rt.state.focused_session().unwrap();
        assert_eq!(session.chips[1].source, Source::Path(second));
        assert!(testkit::box_focused(&rt, key));
    }

    /// The key of the element that has the keyboard focus.
    fn focused_key(rt: &Runtime<Ui>) -> Option<String> {
        rt.elements()
            .into_iter()
            .find(|e| e.focused && !e.exiting)
            .and_then(|e| e.key)
    }

    /// The empty scene with `notes.md` and `plan.txt` attached and read, and their chips' ids.
    fn two_chips(dir: &std::path::Path) -> (Runtime<Ui>, SessionKey, [u64; 2]) {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        testkit::attach_file(&mut rt, &file(dir, "notes.md"));
        testkit::attach_file(&mut rt, &file(dir, "plan.txt"));
        chips_read(&mut rt);
        let chips = &rt.state.focused_session().unwrap().chips;
        let ids = [chips[0].id, chips[1].id];
        assert!(text_area_focused(&rt));
        (rt, key, ids)
    }

    fn shift() -> Mods {
        Mods {
            shift: true,
            ..Mods::default()
        }
    }

    #[test]
    fn the_attach_button_and_the_chips_remove_controls_are_tab_stops() {
        let dir = tempfile::tempdir().unwrap();
        let (mut rt, _, [first, second]) = two_chips(dir.path());
        // After the text, the attach button; before it, the chips' controls, in their order.
        testkit::press(&mut rt, Key::Tab);
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-attach"));
        testkit::press_with(&mut rt, Key::Tab, shift());
        assert!(text_area_focused(&rt));
        testkit::press_with(&mut rt, Key::Tab, shift());
        let remove = |id: u64| Some(format!("mr-chip-remove-{id}"));
        assert_eq!(focused_key(&rt), remove(second));
        testkit::press_with(&mut rt, Key::Tab, shift());
        assert_eq!(focused_key(&rt), remove(first));
    }

    #[test]
    fn a_chip_removed_by_keyboard_leaves_the_keyboard_in_the_field() {
        let dir = tempfile::tempdir().unwrap();
        let (mut rt, _, [first, second]) = two_chips(dir.path());
        testkit::press_with(&mut rt, Key::Tab, shift());
        assert_eq!(focused_key(&rt), Some(format!("mr-chip-remove-{second}")));
        testkit::press(&mut rt, Key::Text(" ".into()));
        testkit::step(&mut rt);
        assert_eq!(chip_names(&rt), ["notes.md"]);
        // Space presses it, as it does a kit button; Rune gives Return to the default button.
        assert!(text_area_focused(&rt), "Space, then the field");
        testkit::press_with(&mut rt, Key::Tab, shift());
        assert_eq!(focused_key(&rt), Some(format!("mr-chip-remove-{first}")));
        testkit::press(&mut rt, Key::Text(" ".into()));
        testkit::step(&mut rt);
        assert!(chip_names(&rt).is_empty());
        assert!(text_area_focused(&rt), "again");
        assert_eq!(testkit::shown_view(&rt).draft.text, "", "nothing typed");
    }

    #[test]
    fn a_mouse_press_on_attach_or_on_a_remove_control_leaves_the_keyboard_in_the_field() {
        let dir = tempfile::tempdir().unwrap();
        let (mut rt, key, [first, _]) = two_chips(dir.path());
        testkit::click(&mut rt, &testkit::tile(key, "mr-attach"));
        assert!(rt.file_dialog().is_some(), "the picker opened");
        assert!(text_area_focused(&rt), "after attach");
        rt.resolve_file_dialog(None).unwrap();
        testkit::step(&mut rt);
        testkit::click(
            &mut rt,
            &testkit::tile(key, &format!("mr-chip-remove-{first}")),
        );
        testkit::step(&mut rt);
        assert_eq!(chip_names(&rt), ["plan.txt"]);
        assert!(text_area_focused(&rt), "after remove");
    }

    #[test]
    fn a_cancelled_dialog_adds_nothing() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        testkit::click(&mut rt, &testkit::tile(key, "mr-attach"));
        rt.resolve_file_dialog(None).unwrap();
        testkit::step(&mut rt);
        assert!(chip_names(&rt).is_empty());
        assert!(rt.file_dialog().is_none());
        assert!(testkit::element(&rt, &testkit::tile(key, "mr-attach-error")).is_none());
    }

    #[test]
    fn a_file_picker_that_cannot_open_says_so_under_the_box() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(dir.path(), "notes.md");
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        let line = |rt: &Runtime<Ui>| {
            testkit::element(rt, &testkit::tile(key, "mr-attach-error"))
                .map(|e| (e.text, e.text_color))
        };
        let busy = (
            Some("The file picker could not be opened: a file dialog is already open".to_owned()),
            Some(crate::theme::kit(true).c.danger.fg),
        );
        // Rune opens one dialog at a time: with another app's dialog up, the picker fails.
        let fail = |rt: &mut Runtime<Ui>| {
            rt.update(|_ui, tx| {
                tx.open_folder(FileDialogOptions::default(), |_: &mut Ui, _, _: &mut Tx| {});
            });
            testkit::step(rt);
            testkit::click(rt, &testkit::tile(key, "mr-attach"));
            testkit::step(rt);
            rt.resolve_file_dialog(None).unwrap();
            testkit::step(rt);
        };
        fail(&mut rt);
        assert_eq!(line(&rt), Some(busy.clone()));
        // A later cancel takes it away; the press alone does not.
        testkit::click(&mut rt, &testkit::tile(key, "mr-attach"));
        testkit::step(&mut rt);
        assert_eq!(
            line(&rt),
            Some(busy.clone()),
            "still there while the picker is up"
        );
        rt.resolve_file_dialog(None).unwrap();
        testkit::step(&mut rt);
        assert_eq!(line(&rt), None, "gone after a cancel");
        // And so does a later pick.
        fail(&mut rt);
        assert_eq!(line(&rt), Some(busy));
        testkit::attach_file(&mut rt, &path);
        assert_eq!(line(&rt), None, "gone after a pick");
        assert_eq!(chip_names(&rt), ["notes.md"]);
    }

    #[test]
    fn a_second_press_while_the_picker_is_open_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(dir.path(), "notes.md");
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        testkit::click(&mut rt, &testkit::tile(key, "mr-attach"));
        testkit::click(&mut rt, &testkit::tile(key, "mr-attach"));
        testkit::step(&mut rt);
        assert!(rt.file_dialog().is_some(), "the first picker is up");
        assert!(testkit::element(&rt, &testkit::tile(key, "mr-attach-error")).is_none());
        assert!(rt.state.file_picker_open);
        rt.resolve_file_dialog(Some(path)).unwrap();
        testkit::step(&mut rt);
        assert_eq!(
            chip_names(&rt),
            ["notes.md"],
            "the first answer still attaches"
        );
        assert!(!rt.state.file_picker_open);
        assert!(rt.file_dialog().is_none(), "no second picker comes");
        assert!(testkit::element(&rt, &testkit::tile(key, "mr-attach-error")).is_none());
    }

    #[test]
    fn the_attach_button_is_off_in_an_ended_session() {
        let mut rt = testkit::window("ended");
        let key = testkit::focused(&rt).unwrap();
        assert!(testkit::element(&rt, &testkit::tile(key, "mr-attach")).is_some());
        testkit::click(&mut rt, &testkit::tile(key, "mr-attach"));
        testkit::step(&mut rt);
        assert!(rt.file_dialog().is_none());
        assert!(chip_names(&rt).is_empty());
    }

    #[test]
    fn a_tile_with_chips_and_no_text_is_kept_when_closed() {
        // Without chips, a new session with nothing typed goes with its tile.
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        testkit::click(&mut rt, &testkit::tile(key, "mr-tile-close"));
        testkit::step(&mut rt);
        assert!(rt.state.app.sessions.get(key).is_none());
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        rt.update(|ui, tx| {
            glue::attach(
                ui,
                tx,
                key,
                vec![Source::Path("/home/alex/notes.md".into())],
            );
        });
        testkit::step(&mut rt);
        assert_eq!(
            glue::leaving(&rt.state, Place::Tile(key)),
            Leaving::Keep,
            "kept as one with text typed"
        );
        testkit::click(&mut rt, &testkit::tile(key, "mr-tile-close"));
        testkit::step(&mut rt);
        assert!(rt.state.app.tiles.keys().is_empty());
        let entry = rt.state.app.sessions.get(key).expect("kept");
        assert_eq!(entry.session.chips.len(), 1);
        assert!(!testkit::sent(&rt).contains(&SessionCommand::Close));
    }

    /// The working scene with `notes.md` attached and sent while its turn runs: queued, with
    /// no text.
    fn queued_chips_alone(dir: &std::path::Path) -> Runtime<Ui> {
        let path = file(dir, "notes.md");
        let mut rt = testkit::window("working");
        testkit::attach_file(&mut rt, &path);
        chips_read(&mut rt);
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        let session = rt.state.focused_session().unwrap();
        let queued = session.queued.as_ref().expect("queued");
        assert_eq!((queued.text.as_str(), queued.chips.len()), ("", 1));
        assert!(session.chips.is_empty());
        rt
    }

    /// What the queued strip of the focused tile says.
    fn queued_strip(rt: &Runtime<Ui>) -> Option<String> {
        let key = testkit::focused(rt).unwrap();
        testkit::element(rt, &testkit::tile(key, "mr-queued-text")).and_then(|e| e.text)
    }

    #[test]
    fn a_queued_message_of_chips_alone_counts_them() {
        let dir = tempfile::tempdir().unwrap();
        let rt = queued_chips_alone(dir.path());
        assert_eq!(queued_strip(&rt).as_deref(), Some("Queued: 1 attached"));
    }

    #[test]
    fn a_queued_message_with_chips_counts_them_after_its_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = testkit::window("working");
        testkit::attach_file(&mut rt, &file(dir.path(), "notes.md"));
        testkit::attach_file(&mut rt, &file(dir.path(), "plan.txt"));
        chips_read(&mut rt);
        testkit::type_text(&mut rt, "read these");
        testkit::press_with(
            &mut rt,
            Key::Enter,
            Mods {
                shift: true,
                ..Mods::default()
            },
        );
        testkit::type_text(&mut rt, "then the tests");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(
            queued_strip(&rt).as_deref(),
            Some("Queued: read these · 2 attached")
        );
    }

    #[test]
    fn a_queued_message_without_chips_shows_its_first_line_alone() {
        let mut rt = testkit::window("working");
        testkit::type_text(&mut rt, "then the tests");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(queued_strip(&rt).as_deref(), Some("Queued: then the tests"));
    }

    #[test]
    fn editing_a_queued_message_of_chips_alone_leaves_the_draft_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = queued_chips_alone(dir.path());
        let key = testkit::focused(&rt).unwrap();
        testkit::type_text(&mut rt, "and the tests");
        testkit::click(&mut rt, &testkit::tile(key, "mr-queued-edit"));
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).draft.text, "and the tests");
        assert_eq!(chip_names(&rt), ["notes.md"], "the chips are back");
        assert!(rt.state.focused_session().unwrap().queued.is_none());
        assert!(text_area_focused(&rt));
    }

    #[test]
    fn a_message_of_chips_alone_given_back_leaves_the_draft_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(dir.path(), "notes.md");
        let mut rt = testkit::window("resting");
        let key = testkit::focused(&rt).unwrap();
        let fake = rt.state.fake.clone().unwrap();
        let id = rt.state.focused_session().unwrap().id.clone().unwrap();
        fake.set_last_active(&id, Some(NOW_UNIX - 30));
        testkit::attach_file(&mut rt, &path);
        chips_read(&mut rt);
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert!(rt.state.app.prompt(key).is_some(), "it asks first");
        testkit::type_text(&mut rt, "and the tests");
        testkit::click(&mut rt, "mr-elsewhere-cancel");
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.prompt(key), None);
        assert_eq!(testkit::shown_view(&rt).draft.text, "and the tests");
        assert_eq!(chip_names(&rt), ["notes.md"], "the chips are back");
        assert!(text_area_focused(&rt));
    }

    #[test]
    fn white_space_beside_an_image_is_not_text() {
        let mut rt = caret_in_look();
        let image = ClipboardImage::from_png(testkit::tiny_png()).unwrap();
        rt.paste_content(ClipboardContent::new(" \n\t").with_image(image));
        testkit::step(&mut rt);
        assert_eq!(chip_names(&rt), ["Pasted image"]);
        assert_eq!(draft(&rt), ("look".to_string(), 2, 2));
    }

    #[test]
    fn files_and_an_image_pasted_together_make_chips_of_the_files_alone() {
        let mut rt = caret_in_look();
        let image = ClipboardImage::from_png(testkit::tiny_png()).unwrap();
        let paths = [
            PathBuf::from("/home/alex/project/notes.md"),
            PathBuf::from("/home/alex/project/plan.txt"),
        ];
        rt.paste_content(
            ClipboardContent::new("/home/alex/project/notes.md")
                .with_files(paths.iter().cloned())
                .with_image(image),
        );
        testkit::step(&mut rt);
        assert_eq!(chip_names(&rt), ["notes.md", "plan.txt"]);
        let session = rt.state.focused_session().unwrap();
        assert!(
            session
                .chips
                .iter()
                .all(|chip| matches!(chip.source, Source::Path(_))),
            "no chip of the image"
        );
        assert_eq!(draft(&rt), ("look".to_string(), 2, 2));
    }

    // A short tile: Task 10 of the attachments plan.

    /// The smallest window.
    const SMALLEST: (f32, f32) = (560.0, 400.0);

    #[test]
    fn the_lines_a_tile_allows() {
        assert_eq!(tiles::HEADER_HEIGHT, 36.0);
        assert_eq!(
            AROUND_TEXT, 84.0,
            "the box round one line and its bottom row"
        );
        assert_eq!(field::LINE_HEIGHT, 17.5);
        assert_eq!(lines_for(None, 0.0), 10);
        assert_eq!(lines_for(Some(172.0), 0.0), 1);
        assert_eq!(lines_for(Some(280.0), 0.0), 6);
        assert_eq!(lines_for(Some(280.0), 40.0), 4);
        assert_eq!(lines_for(Some(600.0), 0.0), 10);
        assert_eq!(lines_for(Some(20.0), 0.0), 1, "a height under the header");
        let one = tiles::HEADER_HEIGHT + CONVERSATION_MIN + AROUND_TEXT + field::LINE_HEIGHT;
        assert_eq!(lines_for(Some(one - 0.5), 0.0), 1);
        assert_eq!(lines_for(Some(one + field::LINE_HEIGHT), 0.0), 2);
    }

    /// Two sessions, `a`'s tile over `b`'s in one column, the window then made the smallest;
    /// `a`'s tile focused and its box with the keyboard. A column takes two tiles only in a
    /// taller window, and keeps them when the window is made smaller.
    fn stacked() -> (Runtime<Ui>, SessionKey, SessionKey) {
        stacked_in(SMALLEST)
    }

    /// [`stacked`], the window then made `size`.
    fn stacked_in(size: (f32, f32)) -> (Runtime<Ui>, SessionKey, SessionKey) {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        rt.update(|ui, tx| glue::show(ui, tx, b, Place::Below(a), false));
        testkit::step(&mut rt);
        rt.set_viewport(size, 1.0);
        testkit::step(&mut rt);
        rt.update(|ui, _tx| glue::focus_tile(ui, a, true));
        testkit::step(&mut rt);
        assert_eq!(testkit::columns(&rt), vec![vec![a, b]]);
        assert!(testkit::box_focused(&rt, a));
        (rt, a, b)
    }

    /// Types `count` lines into the focused box, Shift+Enter between them.
    fn type_lines(rt: &mut Runtime<Ui>, count: usize) {
        for n in 0..count {
            if n > 0 {
                let shift = Mods {
                    shift: true,
                    ..Mods::default()
                };
                testkit::press_with(rt, Key::Enter, shift);
            }
            testkit::type_text(rt, &format!("line {n}"));
        }
    }

    /// Runs frames until heights that spring have settled.
    fn settle(rt: &mut Runtime<Ui>) {
        for _ in 0..40 {
            testkit::step(rt);
        }
    }

    /// The drawn rectangle of `key`'s tile.
    fn tile_rect(rt: &Runtime<Ui>, key: SessionKey) -> [f32; 4] {
        testkit::rect(rt, &crate::view::tile_key(key)).expect("the tile is drawn")
    }

    /// The drawn rectangle of `inner` in `key`'s tile.
    fn rect_in(rt: &Runtime<Ui>, key: SessionKey, inner: &str) -> [f32; 4] {
        testkit::rect(rt, &testkit::tile(key, inner))
            .unwrap_or_else(|| panic!("no {inner} in {key:?}'s tile"))
    }

    /// Asserts that `inner` of `key`'s tile lies inside the tile, under its header.
    fn assert_under_header(rt: &Runtime<Ui>, key: SessionKey, inner: &str) {
        let [x, y, w, h] = rect_in(rt, key, inner);
        let [tx, ty, tw, th] = tile_rect(rt, key);
        let top = ty + tiles::HEADER_HEIGHT;
        assert!(
            x >= tx - 0.5 && x + w <= tx + tw + 0.5 && y >= top - 0.5 && y + h <= ty + th + 0.5,
            "{inner} at {:?} is not inside the tile {:?} under its header",
            [x, y, w, h],
            [tx, ty, tw, th]
        );
    }

    /// The paths of what is drawn of `key`'s tile inside `area`, but for its header and its
    /// outline.
    fn drawn_in(rt: &Runtime<Ui>, key: SessionKey, area: [f32; 4]) -> Vec<String> {
        let inside = format!("/{}/", crate::view::tile_key(key));
        let [ox, oy, ow, oh] = area;
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.opacity > 0.0 && e.path.contains(&inside))
            .filter(|e| !e.path.contains("/mr-tile-header") && !e.path.contains("/mr-tile-outline"))
            .filter(|e| {
                let [x, y, w, h] = e.visible;
                let across = (x + w).min(ox + ow) - x.max(ox);
                let down = (y + h).min(oy + oh) - y.max(oy);
                across > 0.5 && down > 0.5
            })
            .map(|e| e.path)
            .collect()
    }

    /// Asserts that nothing of `key`'s tile is drawn inside `other`'s tile or over its own
    /// header.
    fn assert_kept_in(rt: &Runtime<Ui>, key: SessionKey, other: SessionKey) {
        let none = Vec::<String>::new();
        assert_eq!(
            drawn_in(rt, key, tile_rect(rt, other)),
            none,
            "over the other tile"
        );
        let [x, y, w, _] = tile_rect(rt, key);
        let header = [x, y, w, tiles::HEADER_HEIGHT];
        assert_eq!(drawn_in(rt, key, header), none, "over the header");
    }

    #[test]
    fn a_long_draft_stays_inside_a_short_tile() {
        let (mut rt, a, b) = stacked();
        type_lines(&mut rt, 30);
        settle(&mut rt);
        assert!(tile_rect(&rt, a)[3] < 200.0, "a short tile");
        assert_under_header(&rt, a, "mr-input-box");
        let text = rect_in(&rt, a, "mr-input");
        assert!(
            (text[3] - (field::LINE_HEIGHT + 2.0 * field::PAD)).abs() < 0.5,
            "the text shows one line: {text:?}"
        );
        let area = rect_in(&rt, a, "mr-input-area");
        assert!(
            (area[3] - (AROUND_TEXT + field::LINE_HEIGHT)).abs() < 0.5,
            "the box round one line is as high as the constants say: {area:?}"
        );
        assert_kept_in(&rt, a, b);
    }

    #[test]
    fn the_lowest_tile_with_everything_keeps_its_text_and_its_button() {
        let dir = tempfile::tempdir().unwrap();
        let (mut rt, a, b) = stacked();
        let fake = rt.state.fake.clone().unwrap();
        fake.reply(vec![FakeStep::at(0.0, SessionEvent::TurnStarted)]);
        testkit::type_text(&mut rt, "go");
        testkit::press(&mut rt, Key::Enter);
        testkit::type_text(&mut rt, "then the tests");
        testkit::press(&mut rt, Key::Enter);
        let paths: Vec<PathBuf> = ["one.md", "two.md", "three.md"]
            .into_iter()
            .map(|name| file(dir.path(), name))
            .collect();
        testkit::paste_files(&mut rt, &paths);
        chips_read(&mut rt);
        type_lines(&mut rt, 30);
        settle(&mut rt);
        let session = rt.state.focused_session().unwrap();
        assert!(session.queued.is_some(), "a message is queued");
        assert_eq!(session.chips.len(), 3);
        // While the turn runs the stop button stands where the send button goes.
        assert_under_header(&rt, a, "mr-input");
        assert_under_header(&rt, a, "mr-stop");
        assert_kept_in(&rt, a, b);
        testkit::click(&mut rt, &testkit::tile(a, "mr-stop"));
        testkit::step(&mut rt);
        assert!(testkit::sent(&rt).contains(&SessionCommand::Stop));
        let finished = |interrupted| {
            SessionEvent::TurnFinished(TurnSummary {
                cost_usd: 0.0,
                context_used: None,
                duration_ms: 1,
                interrupted,
            })
        };
        testkit::emit_to(&mut rt, a, finished(true));
        assert_eq!(
            sends(&rt).last().map(String::as_str),
            Some("then the tests")
        );
        testkit::emit_to(&mut rt, a, finished(false));
        settle(&mut rt);
        assert_under_header(&rt, a, "mr-input");
        assert_under_header(&rt, a, "mr-send");
        assert_kept_in(&rt, a, b);
        testkit::click(&mut rt, &testkit::tile(a, "mr-send"));
        testkit::step(&mut rt);
        let (text, attachments) = sends_with(&rt).pop().expect("a message");
        assert!(
            text.starts_with("line 0\nline 1"),
            "the draft is sent: {text:?}"
        );
        assert_eq!(attachments.len(), 3, "with the chips");
    }

    #[test]
    fn the_open_elsewhere_question_can_be_answered_in_a_short_tile() {
        let (mut rt, a, b) = stacked_in((1200.0, 400.0));
        // The question is asked about a session at rest, its program gone.
        let gone = SessionEvent::Exited {
            clean: true,
            detail: String::new(),
        };
        testkit::emit_to(&mut rt, a, gone);
        assert_eq!(
            rt.state.app.sessions.get(a).unwrap().session.phase,
            mr_core::Phase::Resting
        );
        let mut entry = rt.state.app.sessions.get_mut(a).unwrap();
        entry.prompt = Some(mr_core::Prompt::OpenElsewhere {
            text: "carry on".into(),
        });
        drop(entry);
        rt.invalidate();
        settle(&mut rt);
        assert!(tile_rect(&rt, a)[3] < 200.0, "a short tile");
        for button in [
            "mr-elsewhere-here",
            "mr-elsewhere-copy",
            "mr-elsewhere-cancel",
        ] {
            assert_under_header(&rt, a, button);
        }
        assert_kept_in(&rt, a, b);
        testkit::click(&mut rt, &testkit::tile(a, "mr-elsewhere-cancel"));
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.prompt(a), None, "answered");
        assert_eq!(
            glue::view_of(&rt.state, a).draft.text,
            "carry on",
            "given back"
        );
        settle(&mut rt);
        assert_under_header(&rt, a, "mr-input");
        assert_under_header(&rt, a, "mr-send");
        assert_kept_in(&rt, a, b);
    }

    #[test]
    fn what_the_box_holds_besides_its_text_is_counted_as_drawn() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = testkit::window("working");
        let key = testkit::focused(&rt).unwrap();
        testkit::type_text(&mut rt, "then the tests");
        testkit::press(&mut rt, Key::Enter);
        testkit::paste_files(&mut rt, &[file(dir.path(), "notes.md")]);
        chips_read(&mut rt);
        let view = testkit::shown_view_mut(&mut rt);
        view.folder_gone = Some("/home/alex/gone".into());
        view.attach_error = Some("The file picker could not be opened: no portal".into());
        rt.invalidate();
        settle(&mut rt);
        let session = rt.state.focused_session().unwrap();
        assert!(session.queued.is_some() && !session.chips.is_empty());
        let extra = extra_height(Some(session), testkit::shown_view(&rt), theme::kit(true));
        let area = rect_in(&rt, key, "mr-input-area");
        assert!(
            (area[3] - (AROUND_TEXT + field::LINE_HEIGHT + extra)).abs() < 0.5,
            "{area:?} holds one line and {extra} points besides"
        );
    }

    #[test]
    fn a_tall_tile_still_grows_to_ten_lines() {
        let mut rt = testkit::window("empty");
        type_lines(&mut rt, 30);
        settle(&mut rt);
        let text = rect(&rt, "mr-input");
        assert!(
            (text[3] - (10.0 * field::LINE_HEIGHT + 2.0 * field::PAD)).abs() < 0.5,
            "the text shows ten lines: {text:?}"
        );
    }
}
