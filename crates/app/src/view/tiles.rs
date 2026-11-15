//! The row of tiles in the middle of the shell: one tile per session the app holds in its row,
//! side by side, each as wide as its share of the row and holding a header over that session's
//! pane.

use std::path::Path;

use mr_core::{Entry, Mark, SessionKey};
use rune::{
    Color, Cursor, Cx, Drag, El, Key, KeyEvent, Phase, Spring, Transition, Tx, col, el,
    row as row_el, text,
};
use rune_kit::{ControlSize, IconButton, IconButtonVariant, Kit};

use super::{drag, folder, requests, sidebar};
use crate::glue::{self, DropTarget, Renaming, Ui};

/// The key of the row.
pub const ROW: &str = "mr-tiles";

/// The keys of a tile's header and of its parts, the same in every tile.
const HEADER: &str = "mr-tile-header";
const FOLDER: &str = "mr-tile-folder";
const TITLE: &str = "mr-tile-title";
const STATE: &str = "mr-tile-state";
const CLOSE: &str = "mr-tile-close";

/// The key of the field a session's name is edited in, in its tile's header.
const RENAME: &str = "mr-tile-rename";

/// The key of the outline drawn over a tile while it has one.
const OUTLINE: &str = "mr-tile-outline";

/// How thick a tile's outline is.
const OUTLINE_WIDTH: f32 = 2.0;

/// The height of a tile's header.
const HEADER_HEIGHT: f32 = 36.0;

/// The widest the folder's chip grows before its name is cut.
const CHIP_MAX_WIDTH: f32 = 160.0;

/// How wide a divider's handle is, centred on the edge between two tiles.
const DIVIDER_WIDTH: f32 = 8.0;

/// The title of a session that has none yet.
const NEW_SESSION: &str = "New session";

/// `mr-tile-{n}`.
pub fn tile_key(key: SessionKey) -> String {
    format!("mr-tile-{}", key.0)
}

/// `mr-divider-{n}`: the divider right of the tile of session `n`.
fn divider_key(key: SessionKey) -> String {
    format!("mr-divider-{}", key.0)
}

/// The row of tiles, or the empty area when there is none.
///
/// Each tile, keyed [`tile_key`], takes its share of the row's width and holds its session's
/// pane, whose pickers put their lists in `layer`, under its outline when it has one. Each
/// tile but the last draws a line on its right edge; once the row's width is known, a
/// divider over each such edge drags it ([`divider`]).
/// Tiles come and go at once, with no fade. A press on a tile's plain content or its header
/// focuses the tile, with the keyboard in its input box unless the press selected text there;
/// the tile stays a group, no Tab stop.
/// The row tells the window state where it was laid out and the app how wide it is.
pub fn row(ui: &Ui, cx: &mut Cx<Ui>, kit: &'static Kit, layer: &mut Vec<El>) -> El {
    let row = row_el()
        .key(ROW)
        .w_full()
        .h_full()
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .fill(kit.c.bg_base)
        .on_layout(laid_out);
    let tiles = ui.app.tiles.list();
    if tiles.is_empty() {
        return row.child(
            col()
                .w_full()
                .h_full()
                .grow(1.0)
                .min_h(0.0)
                .child(folder::empty(ui, kit)),
        );
    }
    let last = tiles.len() - 1;
    let dividers = ui.app.row_width().map(|row_width| {
        let widths = ui.app.tiles.widths(row_width);
        let mut edge = 0.0;
        tiles[..last]
            .iter()
            .zip(widths)
            .map(|(tile, width)| {
                edge += width;
                divider(tile.key, edge)
            })
            .collect::<Vec<El>>()
    });
    row.children(tiles.iter().enumerate().map(|(index, tile)| {
        let key = tile.key;
        let edge = if index < last { 1.0 } else { 0.0 };
        col()
            .key(tile_key(key))
            .grow(tile.share)
            .min_w(0.0)
            .h_full()
            .min_h(0.0)
            .border_sides([0.0, edge, 0.0, 0.0], kit.c.border_weak)
            .transition(Transition::none())
            .on_press_tx(press_tile(key))
            // After the press handler, which would make it a button.
            .role("group")
            .focusable(false)
            .child(header(ui, kit, key))
            .child(super::pane(ui, cx, kit, key, layer))
            .children(outline(ui, key, kit).map(ring))
    }))
    .children(dividers.into_iter().flatten())
}

/// What a press on a tile's plain content or its header does: the tile takes the focus, and its
/// input box the keyboard. A word or a line a double or triple press selected in a reply stays
/// selected: then the box does not take the keyboard.
fn press_tile(key: SessionKey) -> impl Fn(&mut Ui, &mut Tx) + 'static {
    move |ui: &mut Ui, tx: &mut Tx| {
        tx.instant();
        let typing = tx.text_selection().is_none();
        glue::focus_tile(ui, key, typing);
    }
}

/// The divider over the edge `edge` points from the row's left, right of `left`'s tile: a
/// handle that takes no room, above the tiles, with a resize cursor. A drag moves width between
/// the two tiles ([`drag_divider`]); a double press gives every tile the same width. It is no
/// Tab stop, and a press on it moves neither the keyboard focus nor the tile focus.
fn divider(left: SessionKey, edge: f32) -> El {
    let handle = el()
        .key(divider_key(left))
        .pin(Some(edge - DIVIDER_WIDTH / 2.0), Some(0.0), None, Some(0.0))
        .w(DIVIDER_WIDTH)
        .role("separator")
        .label("Tile width")
        .cursor(Cursor::ResizeColumn)
        .transition(Transition::none())
        .on_drag(move |ui: &mut Ui, drag: &mut Drag, tx: &mut Tx| {
            drag_divider(ui, drag, tx, left);
        })
        .on_double_press_tx(|ui: &mut Ui, tx: &mut Tx| {
            tx.instant();
            let effects = ui.app.even_tiles();
            glue::run(ui, effects);
        });
    requests::unfocusable(handle)
}

/// One phase of a divider's drag, `left` being the tile on its left.
///
/// The press keeps the tile's width, and the two tiles' shares. Each move makes it that width
/// plus the pointer's travel, at once, and writes nothing; the release writes the arrangement
/// once, when the width ends up different from where it started. A cancelled drag puts the
/// two shares back as they were, also for a tile the window had made narrower than a drag may.
/// The tiles never animate.
fn drag_divider(ui: &mut Ui, drag: &mut Drag, tx: &mut Tx, left: SessionKey) {
    tx.instant();
    let width = match drag.phase {
        Phase::Down => {
            drag.data = tile_width(ui, left).unwrap_or(f32::NAN);
            ui.divider_press = ui.app.tile_pair(left);
            return;
        }
        Phase::Move => drag.data + drag.dx,
        Phase::Up if drag.cancelled() => {
            if let Some(shares) = ui.divider_press.take() {
                ui.app.put_tile_pair(left, shares);
            }
            glue::run(ui, Vec::new());
            return;
        }
        Phase::Up => {
            ui.divider_press = None;
            let effects = if tile_width(ui, left) == Some(drag.data) {
                Vec::new()
            } else {
                ui.app.save_now()
            };
            glue::run(ui, effects);
            return;
        }
    };
    // A NaN width, from a press before the row's width was known, changes nothing.
    ui.app.resize_tile(left, width);
    glue::run(ui, Vec::new());
}

/// `key`'s tile's width in the row as last laid out.
fn tile_width(ui: &Ui, key: SessionKey) -> Option<f32> {
    let index = ui.app.tiles.index_of(key)?;
    let widths = ui.app.tiles.widths(ui.app.row_width()?);
    widths.get(index).copied()
}

/// The colour of `key`'s outline. While a row dragged over the row would take its place, the
/// accent, also over the amber of a session that needs the user (ruling R9): it is the one
/// thing that says which session the drop replaces. Else the amber of the sidebar's attention
/// mark while its session needs the user, whether its tile is focused or not; else the accent
/// while its tile is the focused one of two or more, unless a dragged row or header has a
/// target in the row (ruling R11): the accent then marks only where the drop lands, the line at
/// an edge or the tile it replaces. None otherwise: one tile alone is always the focused one.
pub(super) fn outline(ui: &Ui, key: SessionKey, kit: &Kit) -> Option<Color> {
    let over = ui.drag.as_ref().and_then(|drag| drag.over.as_ref());
    if over == Some(&DropTarget::Tile(key)) {
        return Some(kit.c.bg_accent);
    }
    let mark = ui.app.sessions.get(key).map_or(Mark::None, Entry::mark);
    if mark == Mark::NeedsYou {
        return sidebar::mark_color(mark, kit);
    }
    let in_row = matches!(over, Some(DropTarget::Edge(_) | DropTarget::Tile(_)));
    let focused = ui.app.focused() == Some(key) && ui.app.tiles.list().len() > 1;
    (focused && !in_row).then_some(kit.c.bg_accent)
}

/// The outline, over the whole tile and taking no room in it, so the tile does not move as it
/// gains or loses one. It comes, goes and changes colour at once, as the tiles do.
fn ring(color: Color) -> El {
    el().key(OUTLINE)
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .border(OUTLINE_WIDTH, color)
        .transition(Transition::none())
        .motion(Spring::NONE)
}

/// Pushes, while a text field of a tile that is not the focused one has the keyboard focus,
/// an element whose layout focuses that tile (see `sidebar::rename_watch`).
///
/// The rule (ruling R7): while a text field of a tile holds the keyboard focus, that tile is
/// the focused one. This watch serves a field taking the keyboard focus, which Rune does
/// without the app's code running; `glue::run` gives the focus back when an intent moved it
/// away from the tile the user types in.
///
/// The fields are a tile's input box, the "Something else" fields of its questions and the
/// field its header's name is edited in (`glue::typing_in`). Rune writes the real focus into each field's `focused` on every
/// commit, after the view that pushed the element ran: so the element looks again before it
/// acts, and a field another commit has just taken the focus from is left alone.
pub fn focus_watch(ui: &Ui, layer: &mut Vec<El>) {
    let Some(key) = glue::typing_in(ui).filter(|key| ui.app.focused() != Some(*key)) else {
        return;
    };
    layer.push(
        el().key(format!("mr-focus-watch-{}", key.0))
            .absolute(0.0, 0.0)
            .size(0.0, 0.0)
            .on_layout(move |ui: &mut Ui, _| {
                if glue::typing_in(ui) == Some(key) && ui.app.focused() != Some(key) {
                    glue::focus_tile(ui, key, false);
                }
            }),
    );
}

/// How many tiles along a key moves the focus: F6 one to the right, Shift+F6 one to the left.
pub fn tile_step(ev: &KeyEvent) -> Option<isize> {
    let m = ev.mods;
    if ev.key != Key::Function(6) || m.cmd || m.ctrl || m.alt || m.logo {
        return None;
    }
    Some(if m.shift { -1 } else { 1 })
}

/// Ctrl+W (Cmd+W on macOS): closes the focused tile.
pub fn is_close_key(ev: &KeyEvent) -> bool {
    let m = ev.mods;
    ev.is_command('w') && !m.shift && !m.alt && !(cfg!(target_os = "macos") && m.ctrl)
}

/// How many places a key moves the focused tile: Ctrl+Shift+PageUp one to the left,
/// Ctrl+Shift+PageDown one to the right.
///
/// The modifiers are held as for [`is_close_key`]: Cmd+Shift on macOS, with no Ctrl there.
pub fn move_step(ev: &KeyEvent) -> Option<isize> {
    let m = ev.mods;
    if !m.cmd || !m.shift || m.alt || (cfg!(target_os = "macos") && m.ctrl) {
        return None;
    }
    match ev.key {
        Key::PageUp => Some(-1),
        Key::PageDown => Some(1),
        _ => None,
    }
}

/// Moves the focused tile `step` places along the row. False when it is at that end.
///
/// The row does not wrap. The keyboard focus stays where it is (`glue::move_tile`).
pub fn step_move(ui: &mut Ui, tx: &mut Tx, step: isize) -> bool {
    let Some(key) = ui.app.focused() else {
        return false;
    };
    let Some(at) = ui.app.tiles.index_of(key) else {
        return false;
    };
    let to = at as isize + step;
    if step == 0 || to < 0 || to >= ui.app.tiles.list().len() as isize {
        return false;
    }
    // A tile goes in at the edge left of the tile it takes the place of going left, right of
    // it going right (`Tiles::moved_index`).
    let edge = if step > 0 {
        to as usize + 1
    } else {
        to as usize
    };
    glue::move_tile(ui, tx, key, edge, false);
    true
}

/// Moves the focus `step` tiles along the row, round its ends, with the keyboard in that
/// tile's input box. With fewer than two tiles nothing moves: false.
pub fn step_focus(ui: &mut Ui, tx: &mut Tx, step: isize) -> bool {
    let keys: Vec<SessionKey> = ui.app.tiles.list().iter().map(|tile| tile.key).collect();
    let Some(at) = ui
        .app
        .focused()
        .and_then(|key| keys.iter().position(|k| *k == key))
        .filter(|_| keys.len() > 1)
    else {
        return false;
    };
    let next = (at as isize + step).rem_euclid(keys.len() as isize) as usize;
    tx.instant();
    glue::focus_tile(ui, keys[next], true);
    true
}

/// `key`'s header, one line of a fixed height: its folder's name as a chip, its title taking
/// the room the others leave, what its session is doing in a word, and the button that closes
/// the tile. It reads the session's folder, title and mark, nothing of its conversation.
///
/// A double press on the title edits the session's name there, in the window's one name field
/// (`glue::start_rename`); while it does, a press in the field only edits it. The title takes
/// the presses of the tile itself ([`press_tile`]), and the first press of the pair, which
/// focuses the tile, leaves the title where it is: Rune pairs two presses on one element path.
///
/// The header drags its tile along the row (`drag::header`). Rune gives a press, and the drag
/// that follows it, to the one topmost element with a handler for either: so the title, which
/// has presses of its own, takes the drag too, from the same function as the header around it,
/// and the close button, which has its own press, starts none. For the same reason the header
/// takes the tile's press itself. It is no Tab stop, and a press on it leaves the keyboard
/// where it is: a click then puts it in the tile's box, and a drag let go over nothing or
/// cancelled leaves it where it was (a name edited in the header goes on being edited).
fn header(ui: &Ui, kit: &'static Kit, key: SessionKey) -> El {
    let c = &kit.c;
    let entry = ui.app.sessions.get(key);
    let mark = entry.map_or(Mark::None, Entry::mark);
    let title = if ui.renaming == Some(Renaming::Header(key)) {
        sidebar::name_field(&ui.rename, RENAME, "Session name", kit.ty.ui_medium, kit)
    } else {
        let title = entry
            .and_then(|entry| entry.session.title.clone())
            .unwrap_or_else(|| NEW_SESSION.to_owned());
        text(title)
            .key(TITLE)
            .font(kit.ty.ui_medium)
            .color(c.text_base)
            .truncate()
            .grow(1.0)
            .min_w(0.0)
            .on_press_tx(press_tile(key))
            .on_double_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
                tx.instant();
                glue::start_rename(ui, Renaming::Header(key));
            })
            .on_drag(drag::header(key))
    };
    let header = row_el()
        .key(HEADER)
        .w_full()
        .h(HEADER_HEIGHT)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .border_sides([0.0, 0.0, 1.0, 0.0], c.border_weak)
        .on_press_tx(press_tile(key))
        .on_drag(drag::header(key))
        // After the press handler, which would make it a button.
        .role("group");
    requests::unfocusable(header)
        .children(entry.map(|entry| folder_chip(&entry.session.folder, kit)))
        .child(title)
        .children(state_word(mark, kit))
        .child(close_button(key, kit))
}

/// The chip that names `folder` by its last part, the whole path its label for assistive tools.
fn folder_chip(folder: &Path, kit: &Kit) -> El {
    let c = &kit.c;
    let name = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    row_el()
        .key(FOLDER)
        .role("label")
        .label(folder.display().to_string())
        .h(20.0)
        .px(6.0)
        .max_w(CHIP_MAX_WIDTH)
        // In a narrow tile the chip gives way first, so the state word and the close button
        // stay whole and inside.
        .shrink()
        .center_y()
        .radius(kit.radius.xs)
        .fill(c.layer_02)
        .border(0.5, c.border_base)
        .child(
            text(name)
                .font(kit.ty.small_medium)
                .color(c.text_muted)
                .truncate()
                .shrink()
                .a11y_hidden(),
        )
}

/// What the session is doing, in the colour its mark has in the sidebar: nothing while it has
/// no mark.
fn state_word(mark: Mark, kit: &Kit) -> Option<El> {
    let word = match mark {
        Mark::None => return None,
        Mark::Working => "Working",
        Mark::NeedsYou => "Needs you",
        Mark::Finished => "Finished",
    };
    let color = sidebar::mark_color(mark, kit)?;
    Some(text(word).key(STATE).font(kit.ty.small_medium).color(color))
}

/// The button that closes `key`'s tile. Pressed by a click only: it is no Tab stop, and a click
/// on it leaves the focused field focused.
fn close_button(key: SessionKey, kit: &Kit) -> El {
    let button = IconButton::new(kit.glyph.close)
        .variant(IconButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .label("Close tile")
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| glue::close_tile(ui, tx, key))
        .build(kit)
        .key(CLOSE);
    requests::unfocusable(button)
}

/// Keeps where the row was laid out, and gives the app its width, each when it changed.
fn laid_out(ui: &mut Ui, rect: [f32; 4]) {
    if ui.row_rect != Some(rect) {
        ui.row_rect = Some(rect);
    }
    if ui.app.row_width() != Some(rect[2]) {
        ui.app.set_row_width(rect[2]);
        glue::run(ui, Vec::new());
    }
}

#[cfg(test)]
mod tests {
    use mr_agent::{
        RequestId, SessionCommand, SessionEvent, SessionId, StoredSession, ToolCallId, ToolDetail,
        ToolOutcome, TurnSummary, UserRequest, UserRequestKind,
    };
    use mr_core::{ItemBody, ItemId, Mark, SAVED_VERSION, Saved, SavedTile, SessionKey, Width};
    use rune::prefs::Prefs;
    use rune::{Color, Key, KeyEvent, Mods, Runtime};

    use super::{ROW, divider_key, tile_key};
    use crate::demo;
    use crate::glue::{self, Picker, Renaming, Ui};
    use crate::testkit::{self, box_focused, tile, tile_keys};
    use crate::theme;
    use crate::view::{conversation, items};

    /// The window every test here opens: 1,800 by 900, the sidebar open.
    const SIZE: (f32, f32) = (1800.0, 900.0);

    /// Two sessions, `a` in the first tile and `b` in a second one to its side, which has the
    /// focus.
    fn two_tiles() -> (Runtime<Ui>, SessionKey, SessionKey) {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        (rt, a, b)
    }

    /// The drawn rectangle of `key`'s tile.
    fn tile_rect(rt: &Runtime<Ui>, key: SessionKey) -> [f32; 4] {
        rt.presented(&tile_key(key)).expect("the tile is drawn").0
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

    /// A finished `ls` in `key`'s session.
    fn listing(rt: &mut Runtime<Ui>, key: SessionKey) {
        let id = ToolCallId("c1".into());
        let started = SessionEvent::ToolStarted {
            id: id.clone(),
            name: "Bash".into(),
            detail: ToolDetail::Command {
                command: "ls".into(),
            },
        };
        testkit::emit_to(rt, key, started);
        let outcome = ToolOutcome::Ok {
            output: "notes.txt\n".into(),
        };
        testkit::emit_to(rt, key, SessionEvent::ToolFinished { id, outcome });
    }

    #[test]
    fn two_sessions_are_on_screen_side_by_side() {
        let (mut rt, a, b) = two_tiles();
        reply(&mut rt, a, "The reply in a");
        reply(&mut rt, b, "The reply in b");
        assert!(testkit::tile_has_text(&rt, a, "The reply in a"));
        assert!(testkit::tile_has_text(&rt, b, "The reply in b"));
        assert!(!testkit::tile_has_text(&rt, a, "The reply in b"));
        assert!(!testkit::tile_has_text(&rt, b, "The reply in a"));
        let [ax, _, aw, _] = tile_rect(&rt, a);
        let [bx, ..] = tile_rect(&rt, b);
        assert!(
            ax + aw <= bx + 0.5,
            "a ends before b starts: {ax} + {aw}, {bx}"
        );
    }

    #[test]
    fn each_tile_has_its_own_input_box() {
        let (mut rt, a, b) = two_tiles();
        testkit::click(&mut rt, &tile(a, "mr-input"));
        testkit::type_text(&mut rt, "for a");
        testkit::click(&mut rt, &tile(b, "mr-input"));
        testkit::type_text(&mut rt, "for b");
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "for a");
        assert_eq!(glue::view_of(&rt.state, b).draft.text, "for b");
    }

    /// Whether `key`'s session holds a message of the user's that reads `text`.
    fn said(rt: &Runtime<Ui>, key: SessionKey, text: &str) -> bool {
        let entry = rt.state.app.sessions.get(key).unwrap();
        let items = entry.session.conversation.items();
        items
            .iter()
            .any(|item| matches!(&item.body, ItemBody::User { text: t, .. } if t == text))
    }

    #[test]
    fn enter_sends_in_the_tile_it_was_typed_in() {
        let (mut rt, a, b) = two_tiles();
        testkit::click(&mut rt, &tile(a, "mr-input"));
        testkit::type_text(&mut rt, "Hello a");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert!(said(&rt, a, "Hello a"));
        assert!(!said(&rt, b, "Hello a"));
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "");
    }

    /// Claude asks `key`'s session to run `touch a.txt`, as request r1.
    fn ask_permission(rt: &mut Runtime<Ui>, key: SessionKey) {
        let request = UserRequest {
            id: RequestId("r1".into()),
            kind: UserRequestKind::Permission {
                tool: "Bash".into(),
                detail: ToolDetail::Command {
                    command: "touch a.txt".into(),
                },
                always_allow_label: None,
            },
        };
        testkit::emit_to(rt, key, SessionEvent::NeedsUser(request));
    }

    fn waiting(rt: &Runtime<Ui>, key: SessionKey) -> usize {
        let entry = rt.state.app.sessions.get(key).unwrap();
        entry.session.conversation.waiting()
    }

    #[test]
    fn a_request_is_answered_only_in_its_own_tile() {
        let (mut rt, a, b) = two_tiles();
        ask_permission(&mut rt, a);
        ask_permission(&mut rt, b);
        assert_eq!((waiting(&rt, a), waiting(&rt, b)), (1, 1));
        // The cards grow in; a press lands once they are whole.
        rt.settle_all();
        testkit::step(&mut rt);
        testkit::click(&mut rt, &tile(b, "mr-allow-r1"));
        testkit::step(&mut rt);
        assert_eq!(waiting(&rt, b), 0, "b's is answered");
        assert_eq!(waiting(&rt, a), 1, "a's still waits");
    }

    #[test]
    fn opening_a_row_in_one_tile_leaves_the_other_closed() {
        let (mut rt, a, b) = two_tiles();
        listing(&mut rt, a);
        listing(&mut rt, b);
        let id = last_id(&rt, a);
        assert_eq!(id, last_id(&rt, b), "the same item in both");
        let toggle = format!("mr-toggle-{}", id.0);
        let details = format!("mr-details-{}", id.0);
        testkit::click(&mut rt, &tile(b, &toggle));
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(
            testkit::rect(&rt, &tile(b, &details)).is_some(),
            "open in b"
        );
        assert!(
            testkit::rect(&rt, &tile(a, &details)).is_none(),
            "closed in a"
        );
        assert!(glue::view_of(&rt.state, a).toggled.is_empty());
        // The row the press keeps on screen is the one in b's tile.
        let (x, _) = rt.find(&items::toggle_path(b, id)).expect("named");
        let [bx, _, bw, _] = tile_rect(&rt, b);
        assert!(
            x > bx && x < bx + bw,
            "{x} in b's tile from {bx}, {bw} wide"
        );
    }

    fn model(key: SessionKey, part: &str) -> String {
        format!("mr-model-{}-{part}", key.0)
    }

    #[test]
    fn a_picker_opens_under_its_own_tile() {
        let (mut rt, a, b) = two_tiles();
        testkit::click(&mut rt, &model(b, "trigger"));
        testkit::step(&mut rt);
        assert_eq!(glue::view_of(&rt.state, b).open_picker, Some(Picker::Model));
        let ([x, _, w, _], _) = rt.presented(&model(b, "content")).expect("b's list");
        let [bx, _, bw, _] = tile_rect(&rt, b);
        assert!(x >= bx && x + w <= bx + bw, "{x}, {w} inside {bx}, {bw}");
        // The first press outside b's list shuts it; the next opens a's.
        for _ in 0..2 {
            if glue::view_of(&rt.state, a).open_picker.is_none() {
                testkit::click(&mut rt, &model(a, "trigger"));
                testkit::step(&mut rt);
            }
        }
        assert_eq!(glue::view_of(&rt.state, a).open_picker, Some(Picker::Model));
        assert_eq!(glue::view_of(&rt.state, b).open_picker, None);
        let ([x, ..], _) = rt.presented(&model(a, "content")).expect("a's list");
        let [ax, _, aw, _] = tile_rect(&rt, a);
        assert!(x >= ax && x < ax + aw);
    }

    /// Presses the element `key` names and lets go at `to_x`.
    fn drag(rt: &mut Runtime<Ui>, key: &str, to_x: f32) {
        let (x, y) = testkit::find(rt, key).unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move((x + to_x) / 2.0, y);
        rt.pointer_move(to_x, y);
        rt.pointer_up(to_x, y);
        testkit::step(rt);
    }

    fn column_width(rt: &Runtime<Ui>, key: SessionKey) -> f32 {
        testkit::rect(rt, &tile(key, "mr-column")).unwrap()[2]
    }

    #[test]
    fn the_text_column_follows_its_tile() {
        let (mut rt, a, b) = two_tiles();
        rt.update(|ui, _| {
            let effects = ui.app.set_width(Width::Points(1200.0));
            glue::run(ui, effects);
        });
        testkit::step(&mut rt);
        let (wa, wb) = (tile_rect(&rt, a)[2], tile_rect(&rt, b)[2]);
        assert!(
            wa < 1200.0 && wb < 1200.0,
            "each tile is narrower than the width"
        );
        assert_eq!(column_width(&rt, a), wa);
        assert_eq!(column_width(&rt, b), wb);
        let [bx, _, bw, _] = tile_rect(&rt, b);
        drag(&mut rt, &tile(b, "mr-width-left"), bx + bw / 2.0 - 250.0);
        assert_eq!(rt.state.app.width, Width::Points(500.0));
        assert_eq!(column_width(&rt, a), 500.0);
        assert_eq!(column_width(&rt, b), 500.0);
    }

    #[test]
    fn events_for_one_tile_do_not_rebuild_the_other() {
        let (mut rt, a, b) = two_tiles();
        reply(&mut rt, a, "The reply in a");
        listing(&mut rt, a);
        rt.settle_all();
        testkit::step(&mut rt);
        let built = conversation::row_builds(a);
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        testkit::emit_to(&mut rt, b, SessionEvent::TextDelta("Streaming in b".into()));
        testkit::emit_to(&mut rt, b, SessionEvent::TextDelta(", and on".into()));
        assert!(testkit::tile_has_text(&rt, b, "Streaming in b"));
        assert!(testkit::tile_has_text(&rt, a, "The reply in a"));
        assert_eq!(
            conversation::row_builds(a),
            built,
            "a's rows are not built again"
        );
    }

    /// The keys of the tiles drawn and not on their way out.
    fn drawn_tiles(rt: &Runtime<Ui>) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting)
            .filter_map(|e| e.key)
            .filter(|k| {
                k.strip_prefix("mr-tile-")
                    .is_some_and(|n| n.parse::<u64>().is_ok())
            })
            .collect()
    }

    #[test]
    fn two_saved_tiles_come_back_in_the_window() {
        let stored = |id: &str, title: &str| StoredSession {
            id: SessionId(id.into()),
            folder: format!("/home/alex/{id}").into(),
            title: title.into(),
            last_active_unix: demo::NOW_UNIX - demo::DAY,
        };
        let tile_of = |id: &str, share: f32| SavedTile {
            id: SessionId(id.into()),
            share,
        };
        let mut prefs = Prefs::memory();
        let saved = Saved {
            version: SAVED_VERSION,
            shown: Some(SessionId("b1".into())),
            tiles: vec![tile_of("a1", 2.0), tile_of("b1", 1.0)],
            ..Saved::default()
        };
        prefs.set(glue::STATE_KEY, &saved).unwrap();
        let mut ui = demo::ui_with(None, prefs);
        let fake = ui.fake.clone().unwrap();
        fake.set_sessions(vec![
            stored("a1", "Fix the parser"),
            stored("b1", "Tidy the notes"),
        ]);
        glue::setup(&mut ui, std::sync::Arc::new(|| {}));
        let mut rt = testkit::runtime(ui, SIZE);
        // Frame by frame until the listing has come and the tiles are drawn. What each frame
        // shows of the tiles: their rectangles and opacities, and their input boxes'.
        let shown = |rt: &Runtime<Ui>, keys: &[SessionKey]| {
            keys.iter()
                .map(|&key| {
                    let input = testkit::rect(rt, &tile(key, "mr-input"));
                    (rt.presented(&tile_key(key)), input)
                })
                .collect::<Vec<_>>()
        };
        let mut first = None;
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
            let keys = tile_keys(&rt);
            if keys.len() == 2
                && keys
                    .iter()
                    .all(|&key| rt.presented(&tile_key(key)).is_some())
            {
                first = Some(shown(&rt, &keys));
                break;
            }
        }
        let first = first.expect("the saved tiles came back");
        let keys = tile_keys(&rt);
        let id_of = |rt: &Runtime<Ui>, key: SessionKey| {
            let entry = rt.state.app.sessions.get(key).unwrap();
            entry.session.id.clone().unwrap().0
        };
        assert_eq!([id_of(&rt, keys[0]), id_of(&rt, keys[1])], ["a1", "b1"]);
        for _ in 0..20 {
            testkit::step(&mut rt);
        }
        assert_eq!(first, shown(&rt, &keys), "nothing animates in");
        let (a, b) = (tile_rect(&rt, keys[0]), tile_rect(&rt, keys[1]));
        assert_eq!(rt.presented(&tile_key(keys[0])).unwrap().1, 1.0);
        assert!(
            (a[2] / b[2] - 2.0).abs() < 0.05,
            "widths {} and {}",
            a[2],
            b[2]
        );
        assert_eq!(testkit::focused(&rt), Some(keys[1]));
        assert!(box_focused(&rt, keys[1]), "its input box has the keyboard");
        assert!(!box_focused(&rt, keys[0]));
    }

    #[test]
    fn the_empty_area_shows_with_no_tile() {
        let rt = testkit::window_sized("empty-start", SIZE);
        assert!(rt.state.app.tiles.list().is_empty());
        assert!(drawn_tiles(&rt).is_empty());
        let empty = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("mr-empty"))
            .expect("the empty area");
        assert!(empty.path.contains(&format!("/{ROW}/")), "{}", empty.path);
        assert!(testkit::has_text(&rt, "No session is open."));
    }

    #[test]
    fn a_tile_whose_session_is_gone_closes() {
        let (mut rt, a, b) = two_tiles();
        // As when the program could not start because Claude Code is not installed, with the
        // screen kept: the window's next poll, due in the demo every 50 ms, settles the app.
        rt.state.app.sessions.remove(b);
        rt.frame(0.06);
        assert_eq!(drawn_tiles(&rt), vec![tile_key(a)]);
        let row = rt.presented(ROW).unwrap().0;
        assert_eq!(tile_rect(&rt, a)[2], row[2], "as wide as the row, at once");
        assert_eq!(testkit::focused(&rt), Some(a));
        assert!(!rt.state.views.contains_key(&b));
    }

    #[test]
    fn the_row_reports_its_width() {
        let (rt, ..) = two_tiles();
        // The shell's main card, which the row fills: the window less the sidebar and the
        // card's own margin.
        let main = rt.presented("main-pane").unwrap().0;
        let beside = SIZE.0 - rt.state.sidebar.width();
        assert!(main[2] <= beside && main[2] > beside - 20.0, "{main:?}");
        assert_eq!(rt.state.app.row_width(), Some(main[2]));
        assert_eq!(rt.state.row_rect, Some(rt.presented(ROW).unwrap().0));
    }

    #[test]
    fn a_session_that_leaves_its_tile_shuts_its_picker() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        testkit::click(&mut rt, &model(b, "trigger"));
        testkit::step(&mut rt);
        assert_eq!(glue::view_of(&rt.state, b).open_picker, Some(Picker::Model));
        testkit::show(&mut rt, c);
        assert_eq!(tile_keys(&rt), vec![a, c]);
        let view = glue::view_of(&rt.state, b);
        assert_eq!(view.open_picker, None);
        assert!(!view.draft.focused);
    }

    #[test]
    fn a_tile_that_only_loses_the_focus_keeps_its_picker_shut_or_open() {
        let (mut rt, a, b) = two_tiles();
        testkit::click(&mut rt, &model(b, "trigger"));
        testkit::step(&mut rt);
        assert_eq!(glue::view_of(&rt.state, b).open_picker, Some(Picker::Model));
        rt.update(|ui, _| {
            let effects = ui.app.focus_tile(a);
            glue::run(ui, effects);
        });
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(a));
        assert_eq!(
            glue::view_of(&rt.state, b).open_picker,
            Some(Picker::Model),
            "open"
        );
        assert_eq!(glue::view_of(&rt.state, a).open_picker, None, "shut");
    }

    /// The text the part `inner` of `key`'s header shows: its own, or the first inside it.
    fn header_text(rt: &Runtime<Ui>, key: SessionKey, inner: &str) -> Option<String> {
        header_part(rt, key, inner).map(|(text, _)| text)
    }

    /// The text the part `inner` of `key`'s header shows, and that text's colour.
    fn header_part(rt: &Runtime<Ui>, key: SessionKey, inner: &str) -> Option<(String, Color)> {
        let tile = format!("/{}/", tile_key(key));
        let (within, own) = (format!("/{inner}/"), format!("/{inner}"));
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains(&tile))
            .filter(|e| e.path.contains(&within) || e.path.ends_with(&own))
            .find_map(|e| Some((e.text?, e.text_color?)))
    }

    /// A turn that ends at once with nothing said.
    fn turn_finished() -> SessionEvent {
        SessionEvent::TurnFinished(TurnSummary {
            cost_usd: 0.0,
            context_used: None,
            duration_ms: 1,
            interrupted: false,
        })
    }

    #[test]
    fn a_header_names_the_folder_and_the_session() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let garden = testkit::add_session(&mut rt, "/home/alex/garden");
        let title = SessionEvent::TitleChanged("Planning the spring beds".into());
        testkit::emit_to(&mut rt, garden, title);
        let folder = header_text(&rt, garden, "mr-tile-folder");
        assert_eq!(folder.as_deref(), Some("garden"));
        let chip = testkit::element(&rt, &tile(garden, "mr-tile-folder")).expect("the chip");
        assert_eq!(chip.label.as_deref(), Some("/home/alex/garden"));
        let title = header_text(&rt, garden, "mr-tile-title");
        assert_eq!(title.as_deref(), Some("Planning the spring beds"));
    }

    #[test]
    fn a_session_without_a_title_reads_new_session() {
        let rt = testkit::window_sized("empty", SIZE);
        let key = testkit::focused(&rt).expect("the scene's new session");
        let entry = rt.state.app.sessions.get(key).unwrap();
        assert_eq!(entry.session.title, None);
        let title = header_text(&rt, key, "mr-tile-title");
        assert_eq!(title.as_deref(), Some("New session"));
    }

    #[test]
    fn the_header_says_what_its_session_is_doing() {
        let (mut rt, a, b) = two_tiles();
        let c = theme::kit(true).c;
        // A text's colour comes in on a spring: the window settles first.
        let state = |rt: &mut Runtime<Ui>, key| {
            rt.settle_all();
            testkit::step(rt);
            header_part(rt, key, "mr-tile-state")
        };
        assert_eq!(state(&mut rt, a), None);
        assert_eq!(state(&mut rt, b), None);
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        assert_eq!(state(&mut rt, b), Some(("Working".into(), c.icon_muted)));
        ask_permission(&mut rt, b);
        assert_eq!(
            state(&mut rt, b),
            Some(("Needs you".into(), c.warning.icon))
        );
        testkit::emit_to(&mut rt, a, SessionEvent::TurnStarted);
        testkit::emit_to(&mut rt, a, turn_finished());
        assert_eq!(state(&mut rt, a), Some(("Finished".into(), c.bg_accent)));
        rt.update(|ui, _| {
            let effects = ui.app.focus_tile(a);
            glue::run(ui, effects);
        });
        testkit::step(&mut rt);
        assert_eq!(state(&mut rt, a), None, "idle and focused: no word");
    }

    /// Whether `key`'s row in the sidebar shows the kit's busy mark, its spinner.
    fn busy_in_sidebar(rt: &Runtime<Ui>, key: SessionKey) -> bool {
        let inside = format!("/mr-row-{}/", key.0);
        rt.elements().into_iter().any(|e| {
            !e.exiting
                && e.path.contains(&inside)
                && e.path.contains("/status-mark/")
                && e.role == "progressbar"
        })
    }

    #[test]
    fn closing_a_tile_leaves_the_session_working() {
        let (mut rt, a, b) = two_tiles();
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        assert!(busy_in_sidebar(&rt, b));
        let before = testkit::sent(&rt).len();
        testkit::click(&mut rt, &tile(b, "mr-tile-close"));
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a]);
        assert_eq!(drawn_tiles(&rt), vec![tile_key(a)]);
        assert!(busy_in_sidebar(&rt, b), "b still works");
        let sent = &testkit::sent(&rt)[before..];
        assert!(!sent.iter().any(|c| matches!(c, SessionCommand::Close)));
    }

    #[test]
    fn closing_the_last_tile_shows_the_empty_area() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        assert_eq!(tile_keys(&rt), vec![a]);
        testkit::click(&mut rt, &tile(a, "mr-tile-close"));
        testkit::step(&mut rt);
        assert!(tile_keys(&rt).is_empty());
        assert!(drawn_tiles(&rt).is_empty());
        assert!(testkit::has_text(&rt, "No session is open."));
        assert!(rt.state.app.sessions.get(a).is_some(), "a is kept");
    }

    #[test]
    fn closing_a_tile_with_text_typed_keeps_a_new_session() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let new = testkit::focused(&rt).expect("the scene's new session");
        testkit::click(&mut rt, &tile(new, "mr-input"));
        testkit::type_text(&mut rt, "Plan the beds");
        testkit::click(&mut rt, &tile(new, "mr-tile-close"));
        testkit::step(&mut rt);
        assert!(tile_keys(&rt).is_empty());
        assert!(rt.state.app.sessions.get(new).is_some(), "kept");
        assert_eq!(glue::view_of(&rt.state, new).draft.text, "Plan the beds");
        // With nothing typed, it is forgotten.
        let mut rt = testkit::window_sized("empty", SIZE);
        let new = testkit::focused(&rt).expect("the scene's new session");
        testkit::click(&mut rt, &tile(new, "mr-tile-close"));
        testkit::step(&mut rt);
        assert!(tile_keys(&rt).is_empty());
        assert!(rt.state.app.sessions.get(new).is_none(), "forgotten");
    }

    #[test]
    fn the_close_button_never_takes_the_keyboard_focus() {
        // A closed while b's tile is focused, then while a's is.
        for a_focused in [false, true] {
            let (mut rt, a, b) = two_tiles();
            if a_focused {
                testkit::show(&mut rt, a);
                assert_eq!(testkit::focused(&rt), Some(a));
                assert!(box_focused(&rt, a));
            } else {
                assert_eq!(testkit::focused(&rt), Some(b));
                assert!(box_focused(&rt, b));
            }
            let close = testkit::element(&rt, &tile(a, "mr-tile-close")).expect("a's button");
            assert!(!close.focusable, "no Tab stop");
            testkit::click(&mut rt, &tile(a, "mr-tile-close"));
            testkit::step(&mut rt);
            assert_eq!(tile_keys(&rt), vec![b]);
            assert_eq!(testkit::focused(&rt), Some(b));
            assert!(box_focused(&rt, b), "in b's box (a focused: {a_focused})");
            assert!(glue::view_of(&rt.state, b).draft.focused);
            testkit::press(&mut rt, Key::Enter);
            testkit::step(&mut rt);
            assert_eq!(tile_keys(&rt), vec![b], "Enter closes nothing");
            assert!(rt.state.app.sessions.get(b).is_some());
        }
    }

    #[test]
    fn a_long_title_does_not_push_the_close_button_out() {
        // Two tiles of 420 points: the window less the sidebar and the main card's 8-point
        // margin is 840.
        let mut rt = testkit::window_sized("empty", (260.0 + 8.0 + 840.0, 900.0));
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        let [tx, _, tw, _] = tile_rect(&rt, b);
        assert!((tw - 420.0).abs() < 0.5, "b is 420 wide: {tw}");
        let long: String = "A title that goes on and on "
            .chars()
            .cycle()
            .take(300)
            .collect();
        testkit::emit_to(&mut rt, b, SessionEvent::TitleChanged(long));
        let [cx, _, cw, _] = testkit::rect(&rt, &tile(b, "mr-tile-close")).expect("b's button");
        assert!(
            cx >= tx && cx + cw <= tx + tw + 0.5,
            "{cx}, {cw} inside {tx}, {tw}"
        );
        let [title_x, _, title_w, _] = testkit::rect(&rt, &tile(b, "mr-tile-title")).unwrap();
        assert!(title_x + title_w <= cx, "the title ends before the button");
    }

    #[test]
    fn a_session_opened_to_the_side_again_takes_the_keyboard() {
        // b's box had the keyboard as b left its first tile (`add_session(b)`, then `show(a)`):
        // opened to the side again, it takes the keyboard from a's.
        let (mut rt, a, b) = two_tiles();
        assert!(box_focused(&rt, b), "in b's box");
        assert!(!box_focused(&rt, a));
        assert!(glue::view_of(&rt.state, b).draft.focused);
        assert!(!glue::view_of(&rt.state, a).draft.focused);
        testkit::type_text(&mut rt, "for b");
        assert_eq!(glue::view_of(&rt.state, b).draft.text, "for b");
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "");
    }

    /// The colour of the outline round `key`'s tile, while one is drawn. Rune shows tests no
    /// border colours, so the drawn element is checked to be there, over the whole tile, and the
    /// colour is the one the view worked out for it.
    fn outlined(rt: &Runtime<Ui>, key: SessionKey) -> Option<Color> {
        let drawn = testkit::element(rt, &tile(key, "mr-tile-outline"));
        let colour = super::outline(&rt.state, key, theme::kit(true));
        assert_eq!(drawn.is_some(), colour.is_some(), "drawn as worked out");
        if let Some(drawn) = drawn {
            assert_eq!(drawn.rect, tile_rect(rt, key), "over the whole tile");
        }
        colour
    }

    /// Focuses `key`'s tile as a click on its text does, then steps.
    fn focus(rt: &mut Runtime<Ui>, key: SessionKey) {
        rt.update(|ui, _| glue::focus_tile(ui, key, true));
        testkit::step(rt);
        assert_eq!(testkit::focused(rt), Some(key));
    }

    #[test]
    fn one_tile_has_no_outline() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        assert_eq!(tile_keys(&rt), vec![a]);
        assert_eq!(testkit::focused(&rt), Some(a));
        assert_eq!(outlined(&rt, a), None);
    }

    #[test]
    fn the_focused_one_of_two_is_outlined() {
        let (mut rt, a, b) = two_tiles();
        let accent = theme::kit(true).c.bg_accent;
        assert_eq!(outlined(&rt, b), Some(accent));
        assert_eq!(outlined(&rt, a), None);
        let header = testkit::rect(&rt, &tile(a, "mr-tile-header")).unwrap();
        focus(&mut rt, a);
        assert_eq!(outlined(&rt, a), Some(accent));
        assert_eq!(outlined(&rt, b), None);
        let outlined_header = testkit::rect(&rt, &tile(a, "mr-tile-header")).unwrap();
        assert_eq!(outlined_header, header, "the outline takes no room");
    }

    #[test]
    fn a_tile_that_needs_the_user_is_outlined_in_amber() {
        let amber = theme::kit(true).c.warning.icon;
        // Alone.
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        testkit::emit_to(&mut rt, a, SessionEvent::TurnStarted);
        ask_permission(&mut rt, a);
        assert_eq!(outlined(&rt, a), Some(amber));
        // As the one of two that is not focused, and as the focused one: amber wins.
        let (mut rt, a, b) = two_tiles();
        testkit::emit_to(&mut rt, a, SessionEvent::TurnStarted);
        ask_permission(&mut rt, a);
        assert_eq!(testkit::focused(&rt), Some(b));
        assert_eq!(outlined(&rt, a), Some(amber));
        assert_eq!(outlined(&rt, b), Some(theme::kit(true).c.bg_accent));
        focus(&mut rt, a);
        assert_eq!(outlined(&rt, a), Some(amber));
        assert_eq!(outlined(&rt, b), None);
    }

    /// Clicks the middle of the visible text in `key`'s tile that contains `part`.
    fn click_text_in(rt: &mut Runtime<Ui>, key: SessionKey, part: &str) {
        let inside = format!("/{}/", tile_key(key));
        let [x, y, w, h] = rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains(&inside))
            .filter(|e| e.visible[2] > 0.0 && e.visible[3] > 0.0)
            .find(|e| e.text.as_deref().is_some_and(|t| t.contains(part)))
            .unwrap_or_else(|| panic!("no {part:?} in the tile"))
            .visible;
        let (x, y) = (x + w / 2.0, y + h / 2.0);
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        testkit::step(rt);
    }

    /// Double-clicks at the middle of the visible text in `key`'s tile that contains `part`.
    fn double_click_text_in(rt: &mut Runtime<Ui>, key: SessionKey, part: &str) {
        let inside = format!("/{}/", tile_key(key));
        let [x, y, w, h] = rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains(&inside))
            .filter(|e| e.visible[2] > 0.0 && e.visible[3] > 0.0)
            .find(|e| e.text.as_deref().is_some_and(|t| t.contains(part)))
            .unwrap_or_else(|| panic!("no {part:?} in the tile"))
            .visible;
        let (x, y) = (x + w / 2.0, y + h / 2.0);
        rt.pointer_move(x, y);
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0 / 120.0);
        }
        testkit::step(rt);
    }

    #[test]
    fn a_word_selected_in_a_reply_stays_selected() {
        let (mut rt, a, b) = two_tiles();
        reply(&mut rt, b, "Seventeen tomatoes");
        rt.settle_all();
        testkit::step(&mut rt);
        focus(&mut rt, a);
        assert!(box_focused(&rt, a));
        double_click_text_in(&mut rt, b, "Seventeen tomatoes");
        assert_eq!(testkit::focused(&rt), Some(b), "b takes the tile focus");
        let selected = rt.selected_text().expect("a word is selected");
        assert!(
            ["Seventeen", "tomatoes"].contains(&selected.trim()),
            "{selected:?}"
        );
        assert!(!box_focused(&rt, b), "b's box does not take the keyboard");
        assert!(!box_focused(&rt, a), "nor does a's keep it");
        testkit::press_with(&mut rt, Key::Command('c'), testkit::ctrl());
        assert_eq!(testkit::clipboard(&rt), Some(selected), "Ctrl+C copies it");
        assert_eq!(testkit::focused(&rt), Some(b));
    }

    #[test]
    fn answering_in_another_tile_leaves_the_typing_tile_focused() {
        let (mut rt, a, b) = two_tiles();
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        ask_permission(&mut rt, b);
        rt.settle_all();
        testkit::step(&mut rt);
        testkit::click(&mut rt, &tile(a, "mr-input"));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(a));
        assert!(box_focused(&rt, a));
        // `click` runs one frame after the release: the tile focus never leaves a.
        testkit::click(&mut rt, &tile(b, "mr-allow-r1"));
        assert_eq!(waiting(&rt, b), 0, "b's request is answered");
        assert_eq!(testkit::focused(&rt), Some(a), "right after the click");
        assert!(box_focused(&rt, a));
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(a), "once the window settles");
        assert!(box_focused(&rt, a));
        // With no field holding the keyboard, the same click focuses b.
        let (mut rt, a, b) = two_tiles();
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        ask_permission(&mut rt, b);
        rt.settle_all();
        testkit::step(&mut rt);
        focus(&mut rt, a);
        rt.update(|_, tx| tx.blur());
        testkit::step(&mut rt);
        assert!(!box_focused(&rt, a));
        testkit::click(&mut rt, &tile(b, "mr-allow-r1"));
        assert_eq!(waiting(&rt, b), 0);
        assert_eq!(testkit::focused(&rt), Some(b));
    }

    #[test]
    fn a_click_on_a_tiles_text_focuses_it() {
        let (mut rt, a, b) = two_tiles();
        reply(&mut rt, b, "The reply in b");
        focus(&mut rt, a);
        assert!(box_focused(&rt, a));
        click_text_in(&mut rt, b, "The reply in b");
        assert_eq!(testkit::focused(&rt), Some(b));
        let root = rt
            .elements()
            .into_iter()
            .find(|e| e.key == Some(tile_key(b)));
        let root = root.expect("b's tile");
        assert_eq!(root.role, "group", "the tile stays a group");
        assert!(!root.focusable, "no Tab stop");
        assert!(glue::view_of(&rt.state, b).draft.focused);
        assert!(box_focused(&rt, b));
        testkit::type_text(&mut rt, "for b");
        assert_eq!(glue::view_of(&rt.state, b).draft.text, "for b");
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "");
        // And on its header.
        testkit::click(&mut rt, &tile(a, "mr-tile-title"));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(a));
        assert!(box_focused(&rt, a));
    }

    #[test]
    fn a_click_in_a_tiles_input_box_focuses_it() {
        let (mut rt, a, b) = two_tiles();
        testkit::click(&mut rt, &tile(a, "mr-input"));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(a));
        assert!(box_focused(&rt, a));
        assert_eq!(outlined(&rt, b), None);
        testkit::click(&mut rt, &tile(b, "mr-input"));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(b));
        assert!(box_focused(&rt, b));
    }

    #[test]
    fn a_click_in_a_questions_own_answer_focuses_its_tile() {
        let (mut rt, a, b) = two_tiles();
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        let option = |label: &str| mr_agent::QuestionOption {
            label: label.into(),
            description: None,
        };
        let question = mr_agent::Question {
            header: None,
            prompt: "Which colour?".into(),
            options: vec![option("Red"), option("Blue")],
            multi_select: false,
        };
        let request = UserRequest {
            id: RequestId("q1".into()),
            kind: UserRequestKind::Question {
                questions: vec![question],
            },
        };
        testkit::emit_to(&mut rt, b, SessionEvent::NeedsUser(request));
        rt.settle_all();
        testkit::step(&mut rt);
        focus(&mut rt, a);
        testkit::click(&mut rt, &tile(b, "mr-other-q1-0"));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(b));
        let q1 = RequestId("q1".into());
        assert!(glue::view_of(&rt.state, b).answers[&q1][0].other.focused);
        assert!(!glue::view_of(&rt.state, a).draft.focused);
    }

    #[test]
    fn using_a_control_focuses_its_tile() {
        let (mut rt, a, b) = two_tiles();
        listing(&mut rt, b);
        let id = last_id(&rt, b);
        focus(&mut rt, a);
        // The row grows in; a press lands once it is whole.
        rt.settle_all();
        testkit::step(&mut rt);
        testkit::click(&mut rt, &tile(b, &format!("mr-toggle-{}", id.0)));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(b), "a row opened");
        assert!(glue::view_of(&rt.state, b).toggled.contains(&id));
        focus(&mut rt, a);
        testkit::click(&mut rt, &model(b, "trigger"));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(b), "a picker opened");
        assert_eq!(glue::view_of(&rt.state, b).open_picker, Some(Picker::Model));
    }

    /// Sends `text` as the user's message in `key`'s session and returns its item.
    fn say_in(rt: &mut Runtime<Ui>, key: SessionKey, text: &str) -> ItemId {
        rt.update(|ui, _| {
            let effects = ui.app.submit(key, text);
            glue::run(ui, effects);
        });
        let entry = rt.state.app.sessions.get(key).unwrap();
        let items = entry.session.conversation.items();
        let said = items
            .iter()
            .rev()
            .find(|item| matches!(item.body, ItemBody::User { .. }));
        said.expect("the message is in the conversation").id
    }

    #[test]
    fn show_all_in_another_tile_shows_it_there() {
        let (mut rt, a, b) = two_tiles();
        let long: String = (1..=30).map(|n| format!("line {n}\n")).collect();
        let in_a = say_in(&mut rt, a, &long);
        let in_b = say_in(&mut rt, b, &long);
        focus(&mut rt, a);
        rt.update(|_, tx| tx.blur());
        // The rows grow in; a press lands once they are whole.
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(
            glue::typing_in(&rt.state),
            None,
            "no field has the keyboard"
        );
        assert_eq!(testkit::focused(&rt), Some(a), "a is the focused tile");
        let a_button = tile(a, &format!("mr-show-all-{}", in_a.0));
        assert!(testkit::element(&rt, &a_button).is_some(), "a's is cut too");
        testkit::click(&mut rt, &tile(b, &format!("mr-show-all-{}", in_b.0)));
        testkit::step(&mut rt);
        assert!(glue::view_of(&rt.state, b).whole.contains(&in_b));
        assert!(!glue::view_of(&rt.state, a).whole.contains(&in_a));
        assert!(testkit::tile_has_text(&rt, b, "line 30"));
        assert!(!testkit::tile_has_text(&rt, a, "line 30"));
        assert_eq!(testkit::focused(&rt), Some(b));
    }

    /// Whether `key`'s row in the sidebar shows a status mark of any kind.
    fn marked_in_sidebar(rt: &Runtime<Ui>, key: SessionKey) -> bool {
        let inside = format!("/mr-row-{}/", key.0);
        rt.elements().into_iter().any(|e| {
            !e.exiting && e.path.contains(&inside) && e.key.as_deref() == Some("status-mark")
        })
    }

    #[test]
    fn focusing_a_tile_clears_its_finished_mark() {
        let (mut rt, a, b) = two_tiles();
        focus(&mut rt, a);
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        reply(&mut rt, b, "The reply in b");
        testkit::emit_to(&mut rt, b, turn_finished());
        assert_eq!(
            header_text(&rt, b, "mr-tile-state").as_deref(),
            Some("Finished")
        );
        assert!(marked_in_sidebar(&rt, b));
        click_text_in(&mut rt, b, "The reply in b");
        assert_eq!(testkit::focused(&rt), Some(b));
        let entry = rt.state.app.sessions.get(b).unwrap();
        assert_eq!(entry.mark(), Mark::None);
        assert_eq!(header_text(&rt, b, "mr-tile-state"), None);
        assert!(!marked_in_sidebar(&rt, b));
    }

    /// The commands `key`'s session was sent from `from` on.
    fn sent_to(rt: &Runtime<Ui>, key: SessionKey, from: usize) -> Vec<SessionCommand> {
        let id = rt.state.app.sessions.get(key).unwrap().session.id.clone();
        let fake = rt.state.fake.as_ref().unwrap();
        fake.sent()[from..]
            .iter()
            .filter(|(to, _)| Some(to) == id.as_ref())
            .map(|(_, command)| command.clone())
            .collect()
    }

    #[test]
    fn escape_stops_only_the_focused_tiles_turn() {
        let (mut rt, a, b) = two_tiles();
        testkit::emit_to(&mut rt, a, SessionEvent::TurnStarted);
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        assert_eq!(testkit::focused(&rt), Some(b));
        rt.update(|_, tx| tx.focus("sidebar-list", false));
        testkit::step(&mut rt);
        assert!(!box_focused(&rt, a) && !box_focused(&rt, b), "in the list");
        let from = rt.state.fake.as_ref().unwrap().sent().len();
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert!(
            sent_to(&rt, b, from).contains(&SessionCommand::Stop),
            "b stops"
        );
        assert!(
            !sent_to(&rt, a, from).contains(&SessionCommand::Stop),
            "a goes on"
        );
    }

    #[test]
    fn f6_goes_round_the_tiles() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        testkit::show_side(&mut rt, c);
        assert_eq!(tile_keys(&rt), vec![a, b, c]);
        focus(&mut rt, a);
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        let rounds = [(Mods::default(), [b, c, a]), (shift, [c, b, a])];
        for (mods, order) in rounds {
            for key in order {
                testkit::press_with(&mut rt, Key::Function(6), mods);
                testkit::step(&mut rt);
                assert_eq!(testkit::focused(&rt), Some(key));
                assert!(box_focused(&rt, key), "in its box");
                testkit::type_text(&mut rt, "x");
            }
        }
        // Each box was focused twice, once each way round.
        for key in [a, b, c] {
            assert_eq!(glue::view_of(&rt.state, key).draft.text, "xx");
        }
    }

    #[test]
    fn f6_passes_a_tile_whose_session_has_ended() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        testkit::show_side(&mut rt, c);
        assert_eq!(tile_keys(&rt), vec![a, b, c]);
        let ended = SessionEvent::Exited {
            clean: false,
            detail: "The program stopped".into(),
        };
        testkit::emit_to(&mut rt, b, ended);
        focus(&mut rt, a);
        assert!(box_focused(&rt, a));
        testkit::press(&mut rt, Key::Function(6));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(b), "on b, whose box is shut");
        for key in [a, b, c] {
            assert!(!box_focused(&rt, key), "no box has the keyboard");
            assert!(!glue::view_of(&rt.state, key).draft.focused);
        }
        testkit::press(&mut rt, Key::Function(6));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(c));
        assert!(box_focused(&rt, c), "in c's box");
        testkit::type_text(&mut rt, "for c");
        assert_eq!(glue::view_of(&rt.state, c).draft.text, "for c");
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "");
        // A press on b's header does the same.
        testkit::click(&mut rt, &tile(b, "mr-tile-title"));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(b));
        assert!(!box_focused(&rt, c) && !glue::view_of(&rt.state, c).draft.focused);
    }

    #[test]
    fn f6_and_ctrl_w_wait_while_a_picker_is_open() {
        let (mut rt, a, b) = two_tiles();
        testkit::click(&mut rt, &model(b, "trigger"));
        testkit::step(&mut rt);
        assert_eq!(glue::view_of(&rt.state, b).open_picker, Some(Picker::Model));
        testkit::press(&mut rt, Key::Function(6));
        rt.key(ctrl_w());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        assert_eq!(testkit::focused(&rt), Some(b));
        assert_eq!(glue::view_of(&rt.state, b).open_picker, Some(Picker::Model));
    }

    #[test]
    fn f6_does_nothing_with_one_tile() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        testkit::press(&mut rt, Key::Function(6));
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a]);
        assert_eq!(testkit::focused(&rt), Some(a));
    }

    /// Ctrl+W, or Cmd+W on macOS.
    fn ctrl_w() -> KeyEvent {
        KeyEvent {
            key: Key::Command('w'),
            mods: testkit::ctrl(),
        }
    }

    #[test]
    fn ctrl_w_closes_the_focused_tile() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        testkit::show_side(&mut rt, c);
        focus(&mut rt, b);
        rt.key(ctrl_w());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c]);
        assert_eq!(testkit::focused(&rt), Some(c));
        assert!(
            box_focused(&rt, c),
            "the keyboard in the box of the tile focused now"
        );
        // Held, it is not repeated.
        rt.repeat_key(ctrl_w());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c]);
        // With one tile, the empty area shows.
        rt.key(ctrl_w());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a]);
        rt.key(ctrl_w());
        testkit::step(&mut rt);
        assert!(tile_keys(&rt).is_empty());
        assert!(drawn_tiles(&rt).is_empty());
        assert!(testkit::has_text(&rt, "No session is open."));
    }

    /// The divider right of `key`'s tile, drawn and not on its way out.
    fn divider_element(rt: &Runtime<Ui>, key: SessionKey) -> Option<rune::ElementInfo> {
        let key = divider_key(key);
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key.as_str()))
    }

    /// Each tile's width as the app has it, left to right.
    fn widths(rt: &Runtime<Ui>) -> Vec<f32> {
        let row_width = rt.state.app.row_width().expect("the row was laid out");
        rt.state.app.tiles.widths(row_width)
    }

    fn assert_near(found: f32, expected: f32, what: &str) {
        assert!(
            (found - expected).abs() <= 1.0,
            "{what}: {found}, not {expected}"
        );
    }

    /// Presses the divider right of `key`'s tile and moves the pointer `by` points across,
    /// without letting go. Gives where the pointer is.
    fn hold_divider(rt: &mut Runtime<Ui>, key: SessionKey, by: f32) -> (f32, f32) {
        let (x, y) = rt.find(&divider_key(key)).expect("the divider is drawn");
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move(x + by / 2.0, y);
        rt.pointer_move(x + by, y);
        testkit::step(rt);
        (x + by, y)
    }

    /// Drags the divider right of `key`'s tile `by` points across and lets go.
    fn drag_divider(rt: &mut Runtime<Ui>, key: SessionKey, by: f32) {
        let (x, y) = hold_divider(rt, key, by);
        rt.pointer_up(x, y);
        testkit::step(rt);
    }

    #[test]
    fn dragging_a_divider_moves_width_between_two_tiles() {
        let (mut rt, a, b) = two_tiles();
        let half = rt.state.app.row_width().unwrap() / 2.0;
        let handle = divider_element(&rt, a).expect("a's divider");
        assert_eq!(handle.role, "separator");
        assert_eq!(handle.label.as_deref(), Some("Tile width"));
        assert_near(handle.rect[2], 8.0, "the handle's width");
        let [ax, _, aw, _] = tile_rect(&rt, a);
        assert_near(handle.rect[0] + 4.0, ax + aw, "centred on the edge");
        assert!(divider_element(&rt, b).is_none(), "none after the last");
        drag_divider(&mut rt, a, -200.0);
        let [left, right] = widths(&rt)[..] else {
            panic!("two tiles")
        };
        assert_near(left, half - 200.0, "a");
        assert_near(right, half + 200.0, "b");
        assert_near(tile_rect(&rt, a)[2], half - 200.0, "a's tile");
        assert_near(tile_rect(&rt, b)[2], half + 200.0, "b's tile");
        let [ax, _, aw, _] = tile_rect(&rt, a);
        let handle = testkit::rect(&rt, &divider_key(a)).unwrap();
        assert_near(handle[0] + 4.0, ax + aw, "the handle follows the edge");
    }

    #[test]
    fn a_tile_stops_at_420_points() {
        let (mut rt, a, _) = two_tiles();
        drag_divider(&mut rt, a, -2000.0);
        assert_near(widths(&rt)[0], 420.0, "a");
        assert_near(tile_rect(&rt, a)[2], 420.0, "a's tile");
    }

    #[test]
    fn escape_puts_the_widths_back() {
        let (mut rt, a, b) = two_tiles();
        let half = rt.state.app.row_width().unwrap() / 2.0;
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        let (x, y) = hold_divider(&mut rt, a, -200.0);
        assert_near(widths(&rt)[0], half - 200.0, "while held");
        testkit::press(&mut rt, Key::Escape);
        rt.pointer_up(x, y);
        testkit::step(&mut rt);
        assert_near(widths(&rt)[0], half, "a");
        assert_near(tile_rect(&rt, a)[2], half, "a's tile");
        assert_near(tile_rect(&rt, b)[2], half, "b's tile");
        assert_eq!(saved_shares(&rt), None, "nothing written");
    }

    #[test]
    fn escape_puts_a_narrow_tile_back_as_it_was() {
        let (mut rt, a, b) = two_tiles();
        drag_divider(&mut rt, a, -2000.0);
        assert_near(widths(&rt)[0], 420.0, "a at its least");
        // The window is made narrower: the tiles shrink in proportion, a under 420.
        rt.set_viewport((1200.0, 900.0), 1.0);
        rt.settle_all();
        testkit::step(&mut rt);
        let before = widths(&rt);
        assert!(before[0] < 420.0, "{before:?}");
        let drawn = (tile_rect(&rt, a), tile_rect(&rt, b));
        let (x, y) = hold_divider(&mut rt, a, 100.0);
        assert_ne!(widths(&rt), before, "moved while held");
        testkit::press(&mut rt, Key::Escape);
        rt.pointer_up(x, y);
        testkit::step(&mut rt);
        assert_eq!(widths(&rt), before, "both as before the press");
        assert_eq!(
            (tile_rect(&rt, a), tile_rect(&rt, b)),
            drawn,
            "and drawn so"
        );
    }

    /// The tiles' shares as last written to the settings.
    fn saved_shares(rt: &Runtime<Ui>) -> Option<Vec<f32>> {
        let saved: mr_core::Saved = rt.state.prefs.get(glue::STATE_KEY)?;
        Some(saved.tiles.iter().map(|tile| tile.share).collect())
    }

    #[test]
    fn letting_go_saves_once() {
        let (mut rt, a, _) = two_tiles();
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        let (x, y) = hold_divider(&mut rt, a, -200.0);
        rt.pointer_move(x - 50.0, y);
        testkit::step(&mut rt);
        assert_eq!(saved_shares(&rt), None, "nothing written during the moves");
        rt.pointer_up(x - 50.0, y);
        testkit::step(&mut rt);
        let shares: Vec<f32> = rt.state.app.tiles.list().iter().map(|t| t.share).collect();
        assert_ne!(shares, vec![1.0, 1.0]);
        assert_eq!(saved_shares(&rt), Some(shares), "written at the release");
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        testkit::step(&mut rt);
        assert_eq!(saved_shares(&rt), None, "and only then");
        // A press that moves nothing writes nothing.
        let (x, y) = rt.find(&divider_key(a)).unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        testkit::step(&mut rt);
        assert_eq!(saved_shares(&rt), None, "a press alone");
    }

    #[test]
    fn a_double_press_evens_the_tiles() {
        let (mut rt, a, b) = two_tiles();
        let half = rt.state.app.row_width().unwrap() / 2.0;
        drag_divider(&mut rt, a, -200.0);
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        let (x, y) = rt.find(&divider_key(a)).unwrap();
        rt.pointer_move(x, y);
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0 / 120.0);
        }
        testkit::step(&mut rt);
        assert_near(widths(&rt)[0], half, "a");
        assert_near(tile_rect(&rt, a)[2], half, "a's tile");
        assert_near(tile_rect(&rt, b)[2], half, "b's tile");
        assert_eq!(saved_shares(&rt), Some(vec![1.0, 1.0]), "saved");
    }

    #[test]
    fn a_narrow_window_keeps_every_tile() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        testkit::show_side(&mut rt, c);
        rt.set_viewport((700.0, 600.0), 1.0);
        rt.settle_all();
        testkit::step(&mut rt);
        let three = vec![tile_key(a), tile_key(b), tile_key(c)];
        assert_eq!(drawn_tiles(&rt), three);
        assert!(widths(&rt).iter().all(|w| *w < 420.0), "{:?}", widths(&rt));
        drag_divider(&mut rt, a, 100.0);
        assert_eq!(drawn_tiles(&rt), three);
        let shares: Vec<f32> = rt.state.app.tiles.list().iter().map(|t| t.share).collect();
        assert_eq!(shares, vec![1.0, 1.0, 1.0], "nothing changes");
    }

    #[test]
    fn a_divider_does_not_take_the_keyboard_focus() {
        let (mut rt, a, b) = two_tiles();
        assert_eq!(testkit::focused(&rt), Some(b));
        assert!(box_focused(&rt, b));
        let handle = divider_element(&rt, a).unwrap();
        assert!(!handle.focusable, "no Tab stop");
        drag_divider(&mut rt, a, -200.0);
        assert_eq!(testkit::focused(&rt), Some(b), "the focused tile stays");
        assert!(box_focused(&rt, b));
        testkit::type_text(&mut rt, "for b");
        assert_eq!(glue::view_of(&rt.state, b).draft.text, "for b");
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "");
    }

    /// Clicks `key`'s row in the sidebar.
    fn click_row(rt: &mut Runtime<Ui>, key: SessionKey) {
        testkit::click(rt, &format!("mr-row-{}", key.0));
        testkit::step(rt);
    }

    #[test]
    fn a_click_on_a_sidebar_row_shows_it_in_the_focused_tile() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        assert_eq!(testkit::focused(&rt), Some(b));
        click_row(&mut rt, c);
        assert_eq!(tile_keys(&rt), vec![a, c]);
        assert_eq!(testkit::focused(&rt), Some(c));
        assert!(box_focused(&rt, c));
    }

    /// The sidebar's row of `key`'s session.
    fn sidebar_row(rt: &Runtime<Ui>, key: SessionKey) -> rune::ElementInfo {
        let row = format!("mr-row-{}", key.0);
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(row.as_str()))
            .expect("the row is drawn")
    }

    #[test]
    fn the_rows_of_tiles_are_marked_in_the_sidebar() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        assert_eq!(testkit::focused(&rt), Some(b));
        rt.settle_all();
        testkit::step(&mut rt);
        let hover = theme::kit(true).c.hover;
        let (a_row, b_row, c_row) = (
            sidebar_row(&rt, a),
            sidebar_row(&rt, b),
            sidebar_row(&rt, c),
        );
        assert!(b_row.item_selected, "b's row is the selected one");
        assert!(!a_row.item_selected && !c_row.item_selected);
        assert_eq!(a_row.fill, hover, "a's row has the hover fill at rest");
        assert_eq!(c_row.fill.a, 0.0, "c's has none");
    }

    #[test]
    fn a_narrow_tile_keeps_its_close_button_inside() {
        let folder = "/home/alex/planting-schedules-for-the-community-bed";
        assert_eq!(folder.rsplit('/').next().unwrap().chars().count(), 40);
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, folder);
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        testkit::show_side(&mut rt, c);
        assert_eq!(tile_keys(&rt), vec![a, b, c]);
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        ask_permission(&mut rt, b);
        rt.set_viewport((1000.0, 700.0), 1.0);
        rt.settle_all();
        testkit::step(&mut rt);
        let [tx, _, tw, _] = tile_rect(&rt, b);
        assert!(
            tw < 300.0,
            "b is narrower than the header's parts at rest: {tw}"
        );
        let [cx, _, cw, _] = testkit::rect(&rt, &tile(b, "mr-tile-close")).expect("b's button");
        assert!(
            cx >= tx && cx + cw <= tx + tw + 0.5,
            "{cx}, {cw} inside {tx}, {tw}"
        );
        let [sx, _, sw, _] = testkit::rect(&rt, &tile(b, "mr-tile-state")).expect("b's word");
        assert!(sx + sw <= cx, "the word ends before the button");
        let [fx, _, fw, _] = testkit::rect(&rt, &tile(b, "mr-tile-folder")).unwrap();
        assert!(fx >= tx && fx + fw <= sx, "the chip ends before the word");
    }

    // Renaming in the header: Task 10 of plan 4.

    const GARDEN: &str = "Planning the spring beds";

    /// Double-clicks the element `key` names, as a person does: two presses at one point with a
    /// frame between them, then steps.
    fn double_click(rt: &mut Runtime<Ui>, key: &str) {
        let (x, y) = testkit::find(rt, key).unwrap_or_else(|| panic!("no {key:?}"));
        rt.pointer_move(x, y);
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0 / 120.0);
        }
        testkit::step(rt);
    }

    /// Two tiles as [`two_tiles`] opens them, `a` titled [`GARDEN`]; `b` has the focus.
    fn titled_tiles() -> (Runtime<Ui>, SessionKey, SessionKey) {
        let (mut rt, a, b) = two_tiles();
        testkit::emit_to(&mut rt, a, SessionEvent::TitleChanged(GARDEN.into()));
        assert_eq!(testkit::focused(&rt), Some(b));
        (rt, a, b)
    }

    /// Whether the keyboard focus is in the name field of `key`'s header.
    fn name_field_focused(rt: &Runtime<Ui>, key: SessionKey) -> bool {
        let inside = format!("/{}/", tile_key(key));
        rt.elements().into_iter().any(|e| {
            !e.exiting
                && e.focused
                && e.path.contains(&inside)
                && e.key.as_deref() == Some("mr-tile-rename")
        })
    }

    /// The title `key`'s row in the sidebar shows.
    fn row_title(rt: &Runtime<Ui>, key: SessionKey) -> Option<String> {
        let inside = format!("/mr-row-{}/", key.0);
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.path.contains(&inside) && e.text.is_some())
            .and_then(|e| e.text)
    }

    /// Starts renaming `a` in its header with a double press on its title, which is not the
    /// focused tile's.
    fn edit_title(rt: &mut Runtime<Ui>, a: SessionKey) {
        double_click(rt, &tile(a, "mr-tile-title"));
        assert_eq!(rt.state.renaming, Some(Renaming::Header(a)));
    }

    #[test]
    fn a_double_press_on_a_title_edits_it() {
        let (mut rt, a, _) = titled_tiles();
        let from = testkit::sent(&rt).len();
        edit_title(&mut rt, a);
        assert_eq!(testkit::focused(&rt), Some(a), "its tile is focused");
        assert!(name_field_focused(&rt, a), "the field has the keyboard");
        assert_eq!(
            header_text(&rt, a, "mr-tile-rename").as_deref(),
            Some(GARDEN)
        );
        let edit = &rt.state.rename;
        assert_eq!((edit.anchor, edit.focus), (0, GARDEN.len()), "all selected");
        // Neither the watch nor the give-back takes the keyboard from it.
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(name_field_focused(&rt, a));
        assert_eq!(testkit::focused(&rt), Some(a));
        testkit::type_text(&mut rt, "Garden");
        assert_eq!(
            header_text(&rt, a, "mr-tile-rename").as_deref(),
            Some("Garden")
        );
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(
            header_text(&rt, a, "mr-tile-title").as_deref(),
            Some("Garden")
        );
        assert_eq!(row_title(&rt, a).as_deref(), Some("Garden"));
        let rename = SessionCommand::Rename("Garden".into());
        assert!(
            sent_to(&rt, a, 0).contains(&rename),
            "{:?}",
            &testkit::sent(&rt)[from..]
        );
        assert!(box_focused(&rt, a), "the keyboard goes to its input box");
        assert_eq!(testkit::focused(&rt), Some(a));
    }

    #[test]
    fn escape_keeps_the_old_title() {
        let (mut rt, a, _) = titled_tiles();
        testkit::emit_to(&mut rt, a, SessionEvent::TurnStarted);
        let from = rt.state.fake.as_ref().unwrap().sent().len();
        edit_title(&mut rt, a);
        testkit::type_text(&mut rt, "Garden");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(
            header_text(&rt, a, "mr-tile-title").as_deref(),
            Some(GARDEN)
        );
        assert_eq!(row_title(&rt, a).as_deref(), Some(GARDEN));
        assert_eq!(sent_to(&rt, a, from), [], "no Rename, and its turn goes on");
        assert!(box_focused(&rt, a), "the keyboard goes to its input box");
        assert_eq!(testkit::focused(&rt), Some(a));
    }

    #[test]
    fn a_click_elsewhere_keeps_the_new_name() {
        let (mut rt, a, b) = titled_tiles();
        edit_title(&mut rt, a);
        testkit::type_text(&mut rt, "Garden");
        testkit::click(&mut rt, &tile(b, "mr-input"));
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(
            header_text(&rt, a, "mr-tile-title").as_deref(),
            Some("Garden")
        );
        assert_eq!(
            testkit::focused(&rt),
            Some(b),
            "the click went where it was made"
        );
        assert!(box_focused(&rt, b));
        let rename = SessionCommand::Rename("Garden".into());
        assert!(sent_to(&rt, a, 0).contains(&rename));
    }

    #[test]
    fn f6_keeps_the_new_name() {
        let (mut rt, a, b) = titled_tiles();
        edit_title(&mut rt, a);
        testkit::type_text(&mut rt, "Garden");
        testkit::press(&mut rt, Key::Function(6));
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(
            header_text(&rt, a, "mr-tile-title").as_deref(),
            Some("Garden")
        );
        assert_eq!(testkit::focused(&rt), Some(b));
        assert!(box_focused(&rt, b));
    }

    #[test]
    fn showing_another_session_in_the_tile_keeps_the_new_name() {
        let (mut rt, a, b) = titled_tiles();
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        assert_eq!(tile_keys(&rt), vec![a, c]);
        testkit::show(&mut rt, b);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        edit_title(&mut rt, a);
        testkit::type_text(&mut rt, "Garden");
        testkit::click(&mut rt, &format!("mr-row-{}", c.0));
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![c, b], "c took a's tile");
        assert_eq!(rt.state.renaming, None);
        assert_eq!(row_title(&rt, a).as_deref(), Some("Garden"));
    }

    #[test]
    fn a_new_session_from_ctrl_n_keeps_the_new_name() {
        let (mut rt, a, _) = titled_tiles();
        edit_title(&mut rt, a);
        testkit::type_text(&mut rt, "Garden");
        // The demo's folders are not on this disk: Ctrl+N opens the folder picker, which
        // leaves the keyboard in the field.
        testkit::press_with(&mut rt, Key::Command('n'), testkit::ctrl());
        testkit::step(&mut rt);
        let dir = tempfile::tempdir().unwrap();
        rt.resolve_file_dialog(Some(dir.path().to_path_buf()))
            .expect("the folder picker is open");
        testkit::step(&mut rt);
        assert!(
            !tile_keys(&rt).contains(&a),
            "the new session took a's tile"
        );
        assert_eq!(rt.state.renaming, None);
        assert_eq!(row_title(&rt, a).as_deref(), Some("Garden"));
    }

    #[test]
    fn closing_the_tile_ends_its_rename() {
        // By its close button, and by Ctrl+W.
        for by_key in [false, true] {
            let (mut rt, a, b) = titled_tiles();
            let from = rt.state.fake.as_ref().unwrap().sent().len();
            edit_title(&mut rt, a);
            testkit::type_text(&mut rt, "Garden");
            if by_key {
                rt.key(ctrl_w());
                testkit::step(&mut rt);
            } else {
                testkit::click(&mut rt, &tile(a, "mr-tile-close"));
                testkit::step(&mut rt);
            }
            rt.settle_all();
            testkit::step(&mut rt);
            assert_eq!(tile_keys(&rt), vec![b], "by key: {by_key}");
            assert_eq!(rt.state.renaming, None);
            assert_eq!(row_title(&rt, a).as_deref(), Some(GARDEN), "dropped");
            assert_eq!(sent_to(&rt, a, from), []);
            assert_eq!(testkit::focused(&rt), Some(b));
            assert!(box_focused(&rt, b));
        }
    }

    #[test]
    fn an_empty_name_keeps_the_title() {
        let (mut rt, a, _) = titled_tiles();
        let from = rt.state.fake.as_ref().unwrap().sent().len();
        edit_title(&mut rt, a);
        testkit::press(&mut rt, Key::Backspace);
        testkit::type_text(&mut rt, "  ");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(
            header_text(&rt, a, "mr-tile-title").as_deref(),
            Some(GARDEN)
        );
        assert_eq!(sent_to(&rt, a, from), []);
    }

    #[test]
    fn a_press_in_the_field_keeps_editing() {
        let (mut rt, a, _) = titled_tiles();
        edit_title(&mut rt, a);
        testkit::type_text(&mut rt, "Gar");
        double_click(&mut rt, &tile(a, "mr-tile-rename"));
        assert_eq!(
            rt.state.renaming,
            Some(Renaming::Header(a)),
            "not started again"
        );
        assert_eq!(rt.state.rename.text, "Gar");
        assert!(name_field_focused(&rt, a));
    }

    #[test]
    fn the_name_field_fits_a_narrow_tile() {
        let mut rt = testkit::window_sized("empty", (260.0 + 8.0 + 840.0, 900.0));
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        let long: String = "A title that goes on and on "
            .chars()
            .cycle()
            .take(300)
            .collect();
        testkit::emit_to(&mut rt, b, SessionEvent::TitleChanged(long));
        double_click(&mut rt, &tile(b, "mr-tile-title"));
        assert_eq!(rt.state.renaming, Some(Renaming::Header(b)));
        let [tx, _, tw, _] = tile_rect(&rt, b);
        assert!((tw - 420.0).abs() < 0.5, "b is 420 wide: {tw}");
        let [fx, _, fw, _] = testkit::rect(&rt, &tile(b, "mr-tile-rename")).expect("the field");
        let [cx, _, cw, _] = testkit::rect(&rt, &tile(b, "mr-tile-close")).expect("b's button");
        assert!(
            fx >= tx && fx + fw <= cx,
            "the field {fx}, {fw} ends before the button {cx}"
        );
        assert!(cx + cw <= tx + tw + 0.5, "{cx}, {cw} inside {tx}, {tw}");
    }

    // Moving a tile with the keyboard: Task 15 of plan 4.

    /// Three sessions in tiles `[a, b, c]`, `c` focused.
    fn three_tiles() -> (Runtime<Ui>, SessionKey, SessionKey, SessionKey) {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        testkit::show_side(&mut rt, c);
        assert_eq!(tile_keys(&rt), vec![a, b, c]);
        (rt, a, b, c)
    }

    /// Ctrl+Shift+PageUp, or Cmd+Shift+PageUp on macOS.
    fn move_left() -> KeyEvent {
        KeyEvent {
            key: Key::PageUp,
            mods: testkit::ctrl_shift(),
        }
    }

    /// Ctrl+Shift+PageDown, or Cmd+Shift+PageDown on macOS.
    fn move_right() -> KeyEvent {
        KeyEvent {
            key: Key::PageDown,
            mods: testkit::ctrl_shift(),
        }
    }

    #[test]
    fn ctrl_shift_page_down_moves_the_focused_tile_right() {
        let (mut rt, a, b, c) = three_tiles();
        focus(&mut rt, a);
        rt.key(move_right());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, a, c]);
        assert_eq!(testkit::focused(&rt), Some(a));
        // Held, it moves on.
        rt.repeat_key(move_right());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, c, a]);
        assert_eq!(testkit::focused(&rt), Some(a));
        let [bx, ..] = tile_rect(&rt, b);
        let [cx, ..] = tile_rect(&rt, c);
        let [ax, ..] = tile_rect(&rt, a);
        assert!(
            bx < cx && cx < ax,
            "drawn in the new order: {bx}, {cx}, {ax}"
        );
    }

    #[test]
    fn ctrl_shift_page_up_moves_it_left() {
        let (mut rt, a, b, c) = three_tiles();
        assert_eq!(testkit::focused(&rt), Some(c));
        rt.key(move_left());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c, b]);
        rt.key(move_left());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![c, a, b]);
        assert_eq!(testkit::focused(&rt), Some(c));
    }

    #[test]
    fn at_the_end_of_the_row_the_key_is_taken_and_nothing_moves() {
        let (mut rt, a, b, c) = three_tiles();
        let version = rt.state.app.tiles.version();
        rt.update(|ui, tx| assert!(super::super::on_key(ui, &move_right(), tx), "taken"));
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b, c], "no wrap");
        assert_eq!(rt.state.app.tiles.version(), version);
        focus(&mut rt, a);
        let version = rt.state.app.tiles.version();
        rt.update(|ui, tx| assert!(super::super::on_key(ui, &move_left(), tx), "taken"));
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b, c], "no wrap");
        assert_eq!(rt.state.app.tiles.version(), version);
        assert_eq!(testkit::focused(&rt), Some(a));
    }

    #[test]
    fn the_move_keys_pass_on_another_screen() {
        let (mut rt, a, b, c) = three_tiles();
        rt.update(|ui, _| {
            let effects = ui.app.account_checked(mr_agent::Account::Missing);
            glue::run(ui, effects);
        });
        testkit::step(&mut rt);
        assert_ne!(rt.state.app.screen, mr_core::Screen::Main);
        for ev in [move_left(), move_right()] {
            rt.update(|ui, tx| assert!(!super::super::on_key(ui, &ev, tx), "not taken"));
            testkit::step(&mut rt);
        }
        assert_eq!(tile_keys(&rt), vec![a, b, c], "nothing moves");
        assert_eq!(testkit::focused(&rt), Some(c));
    }

    #[test]
    fn the_keyboard_stays_in_the_box_when_its_tile_moves() {
        let (mut rt, a, b, c) = three_tiles();
        focus(&mut rt, b);
        testkit::type_text(&mut rt, "A draft");
        rt.key(move_right());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c, b]);
        assert!(box_focused(&rt, b), "the same field has the keyboard");
        assert_eq!(glue::view_of(&rt.state, b).draft.text, "A draft");
        testkit::type_text(&mut rt, "!");
        assert_eq!(glue::view_of(&rt.state, b).draft.text, "A draft!");
        assert_eq!(glue::view_of(&rt.state, c).draft.text, "");
    }

    #[test]
    fn the_move_keys_wait_while_a_menu_is_open() {
        let (mut rt, a, b, c) = three_tiles();
        testkit::click(&mut rt, &model(c, "trigger"));
        testkit::step(&mut rt);
        assert_eq!(glue::view_of(&rt.state, c).open_picker, Some(Picker::Model));
        rt.key(move_left());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b, c], "a picker's list is open");
        assert_eq!(glue::view_of(&rt.state, c).open_picker, Some(Picker::Model));
        rt.update(|ui, _| glue::view_mut(ui, c).open_picker = None);
        testkit::step(&mut rt);
        let (x, y) = rt.find(&format!("mr-row-{}", a.0)).unwrap();
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_some(), "the row's menu is open");
        rt.key(move_left());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b, c], "a menu is open");
        assert_eq!(testkit::focused(&rt), Some(c));
    }

    #[test]
    fn a_request_in_a_moved_tile_is_answered_in_its_own_session() {
        let (mut rt, a, b) = two_tiles();
        ask_permission(&mut rt, a);
        ask_permission(&mut rt, b);
        assert_eq!((waiting(&rt, a), waiting(&rt, b)), (1, 1));
        assert_eq!(testkit::focused(&rt), Some(b));
        rt.key(move_left());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, a]);
        rt.settle_all();
        testkit::step(&mut rt);
        testkit::click(&mut rt, &tile(a, "mr-allow-r1"));
        testkit::step(&mut rt);
        assert_eq!(waiting(&rt, a), 0, "a's is answered");
        assert_eq!(waiting(&rt, b), 1, "b's still waits");
    }

    #[test]
    fn a_name_being_edited_goes_on_through_a_move() {
        let (mut rt, a, b) = titled_tiles();
        edit_title(&mut rt, a);
        testkit::type_text(&mut rt, "Gar");
        rt.key(move_right());
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, a]);
        assert_eq!(
            rt.state.renaming,
            Some(Renaming::Header(a)),
            "neither kept nor dropped"
        );
        assert!(name_field_focused(&rt, a));
        testkit::type_text(&mut rt, "den");
        assert_eq!(rt.state.rename.text, "Garden");
    }
}
