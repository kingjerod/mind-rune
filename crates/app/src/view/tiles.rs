//! The row of tiles in the middle of the shell: one tile per session the app holds in its row,
//! in columns side by side, each column as wide as its share of the row and holding one tile or
//! two, one above the other. Each tile holds a header over that session's pane.

use std::path::{Path, PathBuf};

use mr_core::{App, Entry, Laid, Mark, Remote, SessionKey, Source, Spot, TileLevel};
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
const REMOTE: &str = "mr-tile-remote";

/// The key of the field a session's name is edited in, in its tile's header.
const RENAME: &str = "mr-tile-rename";

/// The key of the outline drawn over a tile while it has one.
const OUTLINE: &str = "mr-tile-outline";

/// The keys of the veil over a tile's pane while files dragged from another app are held over
/// the tile, and of its words.
const DROP_VEIL: &str = "mr-drop-files";
const DROP_VEIL_TEXT: &str = "mr-drop-files-text";

/// How thick a tile's outline is.
const OUTLINE_WIDTH: f32 = 2.0;

/// The height of a tile's header.
pub(super) const HEADER_HEIGHT: f32 = 36.0;

/// The widest the folder's chip grows before its name is cut.
const CHIP_MAX_WIDTH: f32 = 160.0;

/// How wide a divider's handle is, centred on the edge between two tiles.
const DIVIDER_WIDTH: f32 = 8.0;

/// How high the handle over the line between the two tiles of a column is, centred on it.
const SPLIT_HEIGHT: f32 = 8.0;

/// The title of a session that has none yet.
pub(super) const NEW_SESSION: &str = "New session";

/// `mr-tile-{n}`.
pub fn tile_key(key: SessionKey) -> String {
    format!("mr-tile-{}", key.0)
}

/// `mr-divider-{n}`: the divider right of the column whose tile, or upper tile, is session `n`'s.
fn divider_key(key: SessionKey) -> String {
    format!("mr-divider-{}", key.0)
}

/// `mr-split-{n}`: the line under the tile of session `n`, the upper of two.
fn split_key(top: SessionKey) -> String {
    format!("mr-split-{}", top.0)
}

/// The row of tiles, or the empty area when there is none.
///
/// Each tile, keyed [`tile_key`], is a direct child of the row, in the order of `Tiles::keys`,
/// placed at its rectangle in the row's size as the row last laid out (`Tiles::rects`), so a
/// tile that moves, joins or leaves a column keeps everything Rune keeps for it. In the pass
/// before the row was ever laid out, which is never drawn, the tiles take the columns' shares
/// of its width, the two of a column side by side. Each tile holds its session's pane, whose
/// pickers put their lists in `layer`, under its outline when it has one. Each tile left of
/// the last column draws a line on its right edge, and the upper tile of two one on its lower
/// edge; once the size is known, a divider over each edge between two columns drags it
/// ([`divider`]), and a handle over the line between two tiles of a column drags that
/// ([`split`]).
/// Tiles come and go at once, with no fade. A press on a tile's plain content or its header
/// focuses the tile, with the keyboard in its input box unless the press selected text there;
/// the tile stays a group, no Tab stop.
/// The row tells the window state where it was laid out and the app how wide and high it is.
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
    let columns = ui.app.tiles.columns();
    if columns.is_empty() {
        return row.child(
            col()
                .w_full()
                .h_full()
                .grow(1.0)
                .min_h(0.0)
                .child(folder::empty(ui, kit)),
        );
    }
    let last = columns.len() - 1;
    // The view's own read of the size, so a resized window is placed right in the same frame.
    let size = cx.layout_size(ROW);
    let laid = ui.app.tiles.rects(size.unwrap_or_default());
    let handles = size.map(|_| {
        let lines = laid
            .iter()
            .filter(|laid| laid.level == TileLevel::Top)
            .map(|laid| split(laid.key, laid.rect));
        let dividers = laid
            .iter()
            .filter(|laid| laid.column < last && laid.level != TileLevel::Bottom)
            .map(|laid| divider(laid.key, laid.rect[0] + laid.rect[2]));
        // The dividers after the lines, so that where the two meet the divider has the press.
        lines.chain(dividers).collect::<Vec<El>>()
    });
    row.children(laid.iter().map(|laid| {
        let tile = tile(ui, cx, kit, laid, last, layer);
        match size {
            Some(_) => {
                let [x, y, w, h] = laid.rect;
                tile.absolute(x, y).size(w, h)
            }
            None => {
                let column = &columns[laid.column];
                let parts = if column.under.is_some() { 2.0 } else { 1.0 };
                tile.grow(column.share / parts).h_full()
            }
        }
    }))
    .children(handles.into_iter().flatten())
}

/// The tile `laid` places, without its place in the row: its header over its session's pane,
/// under its outline when it has one, and its lines (see [`row`]), `last` being the index of
/// the row's last column.
///
/// While its session takes a message, the tile takes files dropped from another app
/// ([`drop_files`]), and shows its veil while they are held over it ([`veil`]). Otherwise the
/// drag passes it by. Whether it takes them is read as the view is built: Rune asks during the
/// drag, with no state to read.
fn tile(
    ui: &Ui,
    cx: &mut Cx<Ui>,
    kit: &'static Kit,
    laid: &Laid,
    last: usize,
    layer: &mut Vec<El>,
) -> El {
    let key = laid.key;
    let right = if laid.column < last { 1.0 } else { 0.0 };
    let bottom = if laid.level == TileLevel::Top {
        1.0
    } else {
        0.0
    };
    let takes = ui.app.takes_message(key);
    col()
        .key(tile_key(key))
        .min_w(0.0)
        .min_h(0.0)
        .border_sides([0.0, right, bottom, 0.0], kit.c.border_weak)
        .transition(Transition::none())
        .on_press_tx(press_tile(key))
        // After the press handler, which would make it a button.
        .role("group")
        .focusable(false)
        .on_drop_files(drop_files(key))
        .accepts_files(move |_| takes)
        .on_drop_hover(move |ui: &mut Ui, over: bool, _: &mut Tx| {
            if over {
                ui.file_drop = Some(key);
            } else if ui.file_drop == Some(key) {
                ui.file_drop = None;
            }
        })
        .child(header(ui, kit, key))
        .child(super::pane(ui, cx, kit, key, layer))
        .children((takes && ui.file_drop == Some(key)).then(|| veil(kit)))
        .children(outline(ui, key, kit).map(ring))
}

/// What files dropped on `key`'s tile from another app do: each becomes a chip of its session,
/// and the tile takes the focus, with the keyboard in its input box. A session that stopped
/// taking messages since its tile was drawn takes nothing, and the focus stays where it is.
fn drop_files(key: SessionKey) -> impl Fn(&mut Ui, &[PathBuf], &mut Tx) + 'static {
    move |ui: &mut Ui, paths: &[PathBuf], tx: &mut Tx| {
        if !ui.app.takes_message(key) {
            return;
        }
        tx.instant();
        let sources = paths.iter().cloned().map(Source::Path).collect();
        glue::attach(ui, tx, key, sources);
        glue::focus_tile(ui, key, true);
    }
}

/// The veil over a tile's pane under its header while files are held over the tile: the accent
/// faintly, with "Drop to attach" in its middle. It takes no room and no input, and comes and
/// goes at once.
fn veil(kit: &Kit) -> El {
    col()
        .key(DROP_VEIL)
        .pin(Some(0.0), Some(HEADER_HEIGHT), Some(0.0), Some(0.0))
        .center()
        .fill(kit.c.bg_accent.alpha(0.12))
        .transition(Transition::none())
        .motion(Spring::NONE)
        .inert()
        .child(
            text("Drop to attach")
                .key(DROP_VEIL_TEXT)
                .font(kit.ty.ui_medium)
                .color(kit.c.text_base),
        )
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

/// The divider over the edge `edge` points from the row's left, right of `left`'s column: a
/// handle the row's full height that takes no room, above the tiles, with a resize cursor. A
/// drag moves width between the two columns ([`DIVIDER_DRAG`]); a double press gives every
/// column the same width. It is no Tab stop, and a press on it moves neither the keyboard focus
/// nor the tile focus.
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
            drag_handle(ui, drag, tx, left, &DIVIDER_DRAG);
        })
        .on_double_press_tx(|ui: &mut Ui, tx: &mut Tx| {
            tx.instant();
            let effects = ui.app.even_tiles();
            glue::run(ui, effects);
        });
    requests::unfocusable(handle)
}

/// The handle over the line under `top`'s tile, the upper of two, at `rect` in the row: as
/// wide as the column and centred on the line, taking no room, above the tiles, with a resize
/// cursor. A drag moves height between the two tiles ([`SPLIT_DRAG`]); a double press gives
/// both the same height. It is no Tab stop, and a press on it moves neither the keyboard focus
/// nor the tile focus.
fn split(top: SessionKey, rect: [f32; 4]) -> El {
    let [x, y, w, h] = rect;
    let handle = el()
        .key(split_key(top))
        .absolute(x, y + h - SPLIT_HEIGHT / 2.0)
        .size(w, SPLIT_HEIGHT)
        .role("separator")
        .label("Tile height")
        .cursor(Cursor::ResizeRow)
        .transition(Transition::none())
        .on_drag(move |ui: &mut Ui, drag: &mut Drag, tx: &mut Tx| {
            drag_handle(ui, drag, tx, top, &SPLIT_DRAG);
        })
        .on_double_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
            tx.instant();
            let effects = ui.app.even_split(top);
            glue::run(ui, effects);
        });
    requests::unfocusable(handle)
}

/// What the drag of a handle between two tiles changes: a length of one tile against its
/// neighbour along the drag, and `P`, what a cancelled drag puts back.
struct HandleDrag<P> {
    /// The tile's length along the drag in the row as last laid out.
    length: fn(&Ui, SessionKey) -> Option<f32>,
    /// The pointer's travel along the drag since the press.
    travel: fn(&Drag) -> f32,
    /// Where the window state keeps `P` from the press to the release.
    press: fn(&mut Ui) -> &mut Option<P>,
    /// `P` as it is now.
    read: fn(&App, SessionKey) -> Option<P>,
    /// Puts `P` back.
    put: fn(&mut App, SessionKey, P),
    /// Makes the tile that long; asks for no save.
    resize: fn(&mut App, SessionKey, f32),
}

/// A divider's drag: the width of the column on its left, against the column on its right.
const DIVIDER_DRAG: HandleDrag<[f32; 2]> = HandleDrag {
    length: tile_width,
    travel: |drag| drag.dx,
    press: |ui| &mut ui.divider_press,
    read: App::tile_pair,
    put: App::put_tile_pair,
    resize: App::resize_tile,
};

/// A line's drag: the height of the tile above it, against the tile under it.
const SPLIT_DRAG: HandleDrag<f32> = HandleDrag {
    length: tile_height,
    travel: |drag| drag.dy,
    press: |ui| &mut ui.split_press,
    read: App::tile_split,
    put: App::put_split,
    resize: App::resize_split,
};

/// One phase of the drag of a handle between two tiles, `key` being the tile `handle` sizes:
/// for a divider the tile on its left, for a line the tile above it.
///
/// The press keeps the tile's length, and what a cancel puts back. Each move makes it that
/// length plus the pointer's travel, at once, and writes nothing; the release writes the
/// arrangement once, when the length ends up different from where it started ([`unchanged`]).
/// A cancelled drag puts back exactly what the press kept, also for a tile the window had made
/// smaller than a drag may. The tiles never animate.
fn drag_handle<P>(
    ui: &mut Ui,
    drag: &mut Drag,
    tx: &mut Tx,
    key: SessionKey,
    handle: &HandleDrag<P>,
) {
    tx.instant();
    let length = match drag.phase {
        Phase::Down => {
            drag.data = (handle.length)(ui, key).unwrap_or(f32::NAN);
            let kept = (handle.read)(&ui.app, key);
            *(handle.press)(ui) = kept;
            return;
        }
        Phase::Move => drag.data + (handle.travel)(drag),
        Phase::Up if drag.cancelled() => {
            if let Some(kept) = (handle.press)(ui).take() {
                (handle.put)(&mut ui.app, key, kept);
            }
            glue::run(ui, Vec::new());
            return;
        }
        Phase::Up => {
            *(handle.press)(ui) = None;
            let effects = if unchanged(drag.data, (handle.length)(ui, key)) {
                Vec::new()
            } else {
                ui.app.save_now()
            };
            glue::run(ui, effects);
            return;
        }
    };
    // A NaN length, from a press before the row's size was known, changes nothing.
    (handle.resize)(&mut ui.app, key, length);
    glue::run(ui, Vec::new());
}

/// Whether the drag of a handle that started at the length `start` and ends at `now` changed
/// nothing: also when `start` is not a number, from a press before the row's size was known,
/// whose moves change nothing.
fn unchanged(start: f32, now: Option<f32>) -> bool {
    start.is_nan() || now == Some(start)
}

/// `key`'s tile's width in the row as last laid out.
fn tile_width(ui: &Ui, key: SessionKey) -> Option<f32> {
    let (index, _) = ui.app.tiles.find(key)?;
    let widths = ui.app.tiles.widths(ui.app.row_width()?);
    widths.get(index).copied()
}

/// `key`'s tile's height in the row as last laid out.
pub(super) fn tile_height(ui: &Ui, key: SessionKey) -> Option<f32> {
    let size = [ui.app.row_width()?, ui.app.row_height()?];
    let laid = ui.app.tiles.rects(size);
    laid.iter()
        .find(|laid| laid.key == key)
        .map(|laid| laid.rect[3])
}

/// The colour of `key`'s outline. While a row dragged over the row would take its place, the
/// accent, also over the amber of a session that needs the user (ruling R9): it is the one
/// thing that says which session the drop replaces. Else the amber of the sidebar's attention
/// mark while its session needs the user, whether its tile is focused or not; else the accent
/// while its tile is the focused one of two or more, unless a dragged row or header has a
/// target in the row (ruling R11): the accent then marks only where the drop lands, the line at
/// an edge, the half of a tile a new one takes or the tile it replaces. None otherwise: one
/// tile alone is always the focused one.
pub(super) fn outline(ui: &Ui, key: SessionKey, kit: &Kit) -> Option<Color> {
    let over = ui.drag.as_ref().and_then(|drag| drag.over.as_ref());
    if over == Some(&DropTarget::Tile(key)) {
        return Some(kit.c.bg_accent);
    }
    let mark = ui.app.sessions.get(key).map_or(Mark::None, Entry::mark);
    if mark == Mark::NeedsYou {
        return sidebar::mark_color(mark, kit);
    }
    let in_row = over.is_some_and(|over| !matches!(over, DropTarget::Group(_)));
    let focused = ui.app.focused() == Some(key) && ui.app.tiles.len() > 1;
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

/// Moves the focused tile `step` columns along the row, past a column of two as a whole; one
/// of two gets a column of its own at the edge of its column on that side, when another
/// column fits. False when nothing moved: at that end of the row, or with no room.
///
/// The row does not wrap. The keyboard focus stays where it is (`glue::move_tile`).
pub fn step_move(ui: &mut Ui, tx: &mut Tx, step: isize) -> bool {
    let Some(key) = ui.app.focused() else {
        return false;
    };
    let Some((at, level)) = ui.app.tiles.find(key) else {
        return false;
    };
    let edge = if level != TileLevel::Whole {
        match step.signum() {
            -1 => at,
            1 => at + 1,
            _ => return false,
        }
    } else {
        let to = at as isize + step;
        if step == 0 || to < 0 || to >= ui.app.tiles.columns().len() as isize {
            return false;
        }
        // A tile goes in at the edge left of the column it takes the place of going left,
        // right of it going right (`Tiles::move_to`).
        if step > 0 {
            to as usize + 1
        } else {
            to as usize
        }
    };
    let spot = Spot::Edge(edge);
    if !ui.app.can_move_tile(key, spot) {
        return false;
    }
    glue::move_tile(ui, tx, key, spot, false);
    true
}

/// Which way a key moves the focused tile in its column: Ctrl+Alt+PageUp up (-1),
/// Ctrl+Alt+PageDown down (1). Cmd+Alt on macOS, with no Ctrl there.
pub fn stack_step(ev: &KeyEvent) -> Option<isize> {
    let m = ev.mods;
    if !m.cmd || !m.alt || m.shift || (cfg!(target_os = "macos") && m.ctrl) {
        return None;
    }
    match ev.key {
        Key::PageUp => Some(-1),
        Key::PageDown => Some(1),
        _ => None,
    }
}

/// Moves the focused tile up or down as decision 8 says. False when nothing moved.
///
/// One of two swaps with its mate: the upper going down, the lower going up. One alone in its
/// column joins the column on its left, or on its right when it is the first, over that
/// column's tile going up and under it going down; nothing when that column is full or the
/// row is too low. The keyboard focus stays where it is (`glue::move_tile`).
pub fn step_stack(ui: &mut Ui, tx: &mut Tx, step: isize) -> bool {
    let Some(key) = ui.app.focused() else {
        return false;
    };
    let tiles = &ui.app.tiles;
    let Some((at, level)) = tiles.find(key) else {
        return false;
    };
    let spot = match (level, step.signum()) {
        (TileLevel::Top, 1) => tiles.mate(key).map(Spot::Below),
        (TileLevel::Bottom, -1) => tiles.mate(key).map(Spot::Above),
        (TileLevel::Whole, -1 | 1) => {
            let to = if at == 0 { 1 } else { at - 1 };
            let top = tiles.columns().get(to).map(|column| column.top);
            top.map(|top| {
                if step < 0 {
                    Spot::Above(top)
                } else {
                    Spot::Below(top)
                }
            })
        }
        _ => None,
    };
    let Some(spot) = spot.filter(|spot| ui.app.can_move_tile(key, *spot)) else {
        return false;
    };
    glue::move_tile(ui, tx, key, spot, false);
    true
}

/// Moves the focus `step` tiles along the row, round its ends, with the keyboard in that
/// tile's input box. With fewer than two tiles nothing moves: false.
pub fn step_focus(ui: &mut Ui, tx: &mut Tx, step: isize) -> bool {
    let keys: Vec<SessionKey> = ui.app.tiles.keys();
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
/// the room the others leave, what its session is doing in a word, "Remote" while Remote
/// Control is on ([`remote_mark`]), and the button that closes the tile. It reads the session's
/// folder, title, mark and Remote Control, nothing of its conversation.
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
        .children(entry.and_then(|entry| remote_mark(&entry.session.remote, kit)))
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

/// "Remote" while Remote Control is not off: in the accent's text colour while it is on, muted
/// while it connects or reconnects. It never shrinks, so it takes its room from the title, and
/// has no press of its own: a press on it is one on the header.
fn remote_mark(remote: &Remote, kit: &Kit) -> Option<El> {
    let color = match remote {
        Remote::Off => return None,
        Remote::On { .. } => kit.c.text_accent,
        Remote::Starting | Remote::Reconnecting => kit.c.text_muted,
    };
    Some(
        text("Remote")
            .key(REMOTE)
            .font(kit.ty.small_medium)
            .color(color),
    )
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

/// Keeps where the row was laid out, and gives the app its width and its height, each when it
/// changed.
fn laid_out(ui: &mut Ui, rect: [f32; 4]) {
    if ui.row_rect != Some(rect) {
        ui.row_rect = Some(rect);
    }
    let [_, _, width, height] = rect;
    let mut changed = false;
    if ui.app.row_width() != Some(width) {
        ui.app.set_row_width(width);
        changed = true;
    }
    if ui.app.row_height() != Some(height) {
        ui.app.set_row_height(height);
        changed = true;
    }
    if changed {
        glue::run(ui, Vec::new());
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mr_agent::{
        RemoteAccess, RemoteFailure, RequestId, SessionCommand, SessionEvent, SessionId,
        StoredSession, ToolCallId, ToolDetail, ToolOutcome, TurnSummary, UserRequest,
        UserRequestKind,
    };
    use mr_core::{
        ItemBody, ItemId, Mark, SAVED_VERSION, Saved, SavedTile, SessionKey, Source, Spot,
        TileLevel, Width,
    };
    use rune::prefs::Prefs;
    use rune::{Color, Key, KeyEvent, Mods, Runtime};

    use super::{ROW, divider_key, split_key, tile_key, unchanged};
    use crate::demo;
    use crate::glue::{self, DragFrom, Picker, Renaming, Ui};
    use crate::testkit::{self, box_focused, columns, tile, tile_keys};
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
            below: None,
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
        assert!(rt.state.app.tiles.is_empty());
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
        let shares: Vec<f32> = rt
            .state
            .app
            .tiles
            .columns()
            .iter()
            .map(|c| c.share)
            .collect();
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
        let shares: Vec<f32> = rt
            .state
            .app
            .tiles
            .columns()
            .iter()
            .map(|c| c.share)
            .collect();
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

    // A column of two: Task 3 of the stacked tiles plan.

    /// Moves `key`'s tile to `spot` as the window moves a tile, the keyboard left where it is,
    /// then steps.
    fn move_to(rt: &mut Runtime<Ui>, key: SessionKey, spot: Spot) {
        rt.update(|ui, tx| glue::move_tile(ui, tx, key, spot, false));
        testkit::step(rt);
    }

    /// Moves `key`'s tile under `other`'s, into its column.
    fn stack(rt: &mut Runtime<Ui>, key: SessionKey, other: SessionKey) {
        move_to(rt, key, Spot::Below(other));
        let (column, _) = rt.state.app.tiles.find(other).unwrap();
        assert_eq!(
            rt.state.app.tiles.find(key),
            Some((column, TileLevel::Bottom))
        );
    }

    /// The drawn rectangle of the row.
    fn row_rect(rt: &Runtime<Ui>) -> [f32; 4] {
        rt.presented(ROW).expect("the row is drawn").0
    }

    /// Where `key`'s tile belongs in the row as it is drawn, in window points.
    fn laid_rect(rt: &Runtime<Ui>, key: SessionKey) -> [f32; 4] {
        let [x, y, w, h] = row_rect(rt);
        let laid = rt.state.app.tiles.rects([w, h]);
        let [lx, ly, lw, lh] = laid.iter().find(|l| l.key == key).expect("in the row").rect;
        [x + lx, y + ly, lw, lh]
    }

    /// Every tile is drawn at its rectangle in the row as it is drawn.
    fn assert_placed(rt: &Runtime<Ui>) {
        for key in tile_keys(rt) {
            let (drawn, laid) = (tile_rect(rt, key), laid_rect(rt, key));
            let off = drawn.iter().zip(laid).any(|(d, l)| (d - l).abs() > 0.01);
            assert!(!off, "{key:?} drawn at {drawn:?}, not {laid:?}");
        }
    }

    /// The `tiles` scene's state, its third tile moved under its second, the window not built.
    fn stacked_scene() -> (Ui, SessionKey, SessionKey) {
        let mut ui = demo::ui();
        let (_, scene) = demo::SCENES
            .iter()
            .find(|(name, _)| *name == "tiles")
            .unwrap();
        scene(&mut ui);
        let columns = ui.app.tiles.columns();
        let (second, third) = (columns[1].top, columns[2].top);
        let effects = ui.app.move_tile(third, Spot::Below(second));
        glue::run(&mut ui, effects);
        assert_eq!(ui.app.tiles.mate(second), Some(third));
        (ui, second, third)
    }

    #[test]
    fn a_stacked_tile_is_drawn_under_its_column_mate() {
        let (mut rt, a, b) = two_tiles();
        stack(&mut rt, b, a);
        let [ax, ay, aw, ah] = tile_rect(&rt, a);
        let [bx, by, bw, bh] = tile_rect(&rt, b);
        let row = row_rect(&rt);
        assert_eq!((ax, aw), (bx, bw), "the same x and width");
        assert!(
            (ay + ah - by).abs() < 0.01,
            "a ends where b starts: {ay} + {ah}, {by}"
        );
        assert!(
            (ah + bh - row[3]).abs() < 0.01,
            "{ah} + {bh} is the row's {}",
            row[3]
        );
        assert!((ay - row[1]).abs() < 0.01, "a at the row's top");
        assert_placed(&rt);
    }

    #[test]
    fn tiles_fill_the_row() {
        let (mut rt, a, b, c) = three_tiles();
        stack(&mut rt, c, b);
        let d = testkit::add_session(&mut rt, "/home/alex/d");
        testkit::show(&mut rt, c);
        testkit::show_side(&mut rt, d);
        assert_eq!(tile_keys(&rt), vec![a, b, c, d]);
        assert_eq!(rt.state.app.tiles.columns().len(), 3, "[a][b/c][d]");
        let [rx, ry, rw, rh] = row_rect(&rt);
        let rects: Vec<[f32; 4]> = [a, b, c, d].iter().map(|&k| tile_rect(&rt, k)).collect();
        for [x, y, w, h] in &rects {
            assert!(
                *x >= rx - 0.5
                    && *y >= ry - 0.5
                    && x + w <= rx + rw + 0.5
                    && y + h <= ry + rh + 0.5,
                "{:?} inside the row {:?}",
                [x, y, w, h],
                [rx, ry, rw, rh]
            );
        }
        for (i, p) in rects.iter().enumerate() {
            for q in &rects[i + 1..] {
                let across = (p[0] + p[2]).min(q[0] + q[2]) - p[0].max(q[0]);
                let down = (p[1] + p[3]).min(q[1] + q[3]) - p[1].max(q[1]);
                assert!(across <= 0.5 || down <= 0.5, "{p:?} and {q:?} overlap");
            }
        }
        // No gap: just past each edge of a tile that is not the row's own, another tile lies.
        let in_a_tile = |x: f32, y: f32| {
            rects
                .iter()
                .any(|[l, t, w, h]| x >= *l && x <= l + w && y >= *t && y <= t + h)
        };
        for &[x, y, w, h] in &rects {
            for part in [0.1, 0.5, 0.9] {
                let (along_x, along_y) = (x + w * part, y + h * part);
                for (px, py) in [
                    (x - 0.51, along_y),
                    (x + w + 0.51, along_y),
                    (along_x, y - 0.51),
                    (along_x, y + h + 0.51),
                ] {
                    let in_row = px > rx && px < rx + rw && py > ry && py < ry + rh;
                    assert!(!in_row || in_a_tile(px, py), "a gap at {px}, {py}");
                }
            }
        }
        assert_placed(&rt);
    }

    #[test]
    fn the_first_frame_places_the_tiles() {
        let (ui, second, third) = stacked_scene();
        let mut rt = testkit::runtime(ui, SIZE);
        rt.frame(1.0 / 60.0);
        assert_eq!(tile_keys(&rt).len(), 3);
        let [_, uy, _, uh] = tile_rect(&rt, second);
        assert!((uy + uh - tile_rect(&rt, third)[1]).abs() < 0.01, "stacked");
        assert_placed(&rt);
    }

    #[test]
    fn a_resized_window_places_the_tiles_again() {
        let (mut rt, a, b, _) = three_tiles();
        stack(&mut rt, b, a);
        assert_placed(&rt);
        let before = row_rect(&rt);
        rt.set_viewport((1400.0, 700.0), 1.0);
        rt.frame(1.0 / 60.0);
        let row = row_rect(&rt);
        assert!(
            row[2] < before[2] && row[3] < before[3],
            "{before:?}, then {row:?}"
        );
        assert_placed(&rt);
        // The row tells the app once it was laid out.
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.app.row_height(), Some(row[3]), "the app knows");
        assert_eq!(rt.state.app.row_width(), Some(row[2]));
    }

    #[test]
    fn stacking_keeps_the_scroll_position_and_rebuilds_no_row() {
        let (mut rt, a, b, c) = three_tiles();
        for n in 1..=200 {
            let delta = SessionEvent::TextDelta(format!("Line {n} of a"));
            demo::emit_to(&rt.state, a, delta);
            demo::emit_to(&rt.state, a, SessionEvent::BlockFinished);
        }
        testkit::step(&mut rt);
        rt.settle_all();
        testkit::step(&mut rt);
        let scroll = conversation::conversation_key(a);
        let (x, y) = rt.find(&scroll).unwrap();
        for _ in 0..5 {
            rt.wheel(x, y, 600.0);
            testkit::step(&mut rt);
        }
        assert_eq!(rt.scroll_at_bottom(&scroll), Some(false));
        // The first line of a's on screen, and how far below its tile's top it is.
        let first = |rt: &Runtime<Ui>| {
            let [_, top, _, _] = tile_rect(rt, a);
            rt.elements()
                .into_iter()
                .filter(|e| !e.exiting && e.visible[2] > 0.0 && e.visible[3] > 0.0)
                .filter(|e| e.path.contains(&format!("/{scroll}/")))
                .filter(|e| e.text.as_deref().is_some_and(|t| t.ends_with(" of a")))
                .min_by(|p, q| p.rect[1].total_cmp(&q.rect[1]))
                .map(|e| (e.text.unwrap(), e.rect[1] - top))
                .expect("a line of a's on screen")
        };
        let before = first(&rt);
        assert_ne!(before.0, "Line 200 of a", "scrolled up");
        let rows = conversation::all_row_builds();
        stack(&mut rt, a, b);
        assert_eq!(tile_keys(&rt), vec![b, a, c]);
        assert_placed(&rt);
        assert_eq!(rt.scroll_at_bottom(&scroll), Some(false));
        let after = first(&rt);
        assert_eq!(after.0, before.0, "the same line first");
        assert!(
            (after.1 - before.1).abs() < 0.5,
            "{before:?}, then {after:?}"
        );
        assert_eq!(conversation::all_row_builds(), rows, "no conversation row");
    }

    /// Types "A draft" in `key`'s box, focused by a click on its text as a person does.
    fn draft_in(rt: &mut Runtime<Ui>, key: SessionKey) {
        testkit::click(rt, &tile(key, "mr-input"));
        testkit::type_text(rt, "A draft");
        assert!(box_focused(rt, key));
    }

    /// The box of `key` has the keyboard and its draft, and a character typed lands there.
    fn assert_still_typing(rt: &mut Runtime<Ui>, key: SessionKey) {
        assert!(box_focused(rt, key), "the same field has the keyboard");
        assert_eq!(glue::view_of(&rt.state, key).draft.text, "A draft");
        testkit::type_text(rt, "!");
        assert_eq!(glue::view_of(&rt.state, key).draft.text, "A draft!");
        for other in tile_keys(rt).into_iter().filter(|k| *k != key) {
            assert_eq!(glue::view_of(&rt.state, other).draft.text, "");
        }
    }

    #[test]
    fn a_draft_and_the_keyboard_stay_when_a_tile_is_stacked() {
        let (mut rt, a, b, c) = three_tiles();
        draft_in(&mut rt, a);
        stack(&mut rt, a, b);
        assert_eq!(tile_keys(&rt), vec![b, a, c]);
        assert_still_typing(&mut rt, a);
    }

    #[test]
    fn a_draft_and_the_keyboard_stay_when_a_tile_is_taken_out_of_its_column() {
        let (mut rt, a, b, c) = three_tiles();
        stack(&mut rt, b, a);
        draft_in(&mut rt, b);
        move_to(&mut rt, b, Spot::Edge(2));
        assert_eq!(tile_keys(&rt), vec![a, c, b]);
        assert_eq!(rt.state.app.tiles.find(b), Some((2, TileLevel::Whole)));
        assert_still_typing(&mut rt, b);
    }

    #[test]
    fn a_draft_and_the_keyboard_stay_when_a_tile_swaps_with_its_mate() {
        let (mut rt, a, b) = two_tiles();
        stack(&mut rt, b, a);
        draft_in(&mut rt, a);
        move_to(&mut rt, a, Spot::Below(b));
        assert_eq!(tile_keys(&rt), vec![b, a]);
        assert_eq!(rt.state.app.tiles.find(a), Some((0, TileLevel::Bottom)));
        assert_still_typing(&mut rt, a);
    }

    #[test]
    fn a_name_being_edited_goes_on_when_its_tile_is_stacked() {
        let (mut rt, a, b) = titled_tiles();
        edit_title(&mut rt, a);
        testkit::type_text(&mut rt, "Gar");
        stack(&mut rt, a, b);
        assert_eq!(tile_keys(&rt), vec![b, a]);
        assert_eq!(rt.state.renaming, Some(Renaming::Header(a)));
        assert!(name_field_focused(&rt, a));
        testkit::type_text(&mut rt, "den");
        assert_eq!(rt.state.rename.text, "Garden");
    }

    #[test]
    fn the_outline_is_on_the_tile_it_marks() {
        let (mut rt, a, b) = two_tiles();
        testkit::emit_to(&mut rt, a, SessionEvent::TurnStarted);
        ask_permission(&mut rt, a);
        stack(&mut rt, b, a);
        assert_eq!(testkit::focused(&rt), Some(b));
        assert_placed(&rt);
        // `outlined` checks that each outline lies over its own tile's drawn rectangle.
        assert_eq!(outlined(&rt, b), Some(theme::kit(true).c.bg_accent));
        assert_eq!(outlined(&rt, a), Some(theme::kit(true).c.warning.icon));
        assert_ne!(tile_rect(&rt, a), tile_rect(&rt, b));
    }

    /// Presses and lets go at the middle of `key`'s tile, below its header, then steps.
    fn press_middle(rt: &mut Runtime<Ui>, key: SessionKey) {
        let [x, y, w, h] = tile_rect(rt, key);
        let (x, y) = (x + w / 2.0, y + h / 2.0);
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        testkit::step(rt);
    }

    #[test]
    fn a_press_on_a_stacked_tile_focuses_it() {
        let (mut rt, a, b) = two_tiles();
        stack(&mut rt, b, a);
        assert_eq!(testkit::focused(&rt), Some(b));
        press_middle(&mut rt, a);
        assert_eq!(testkit::focused(&rt), Some(a));
        assert!(box_focused(&rt, a), "the keyboard in its box");
        press_middle(&mut rt, b);
        assert_eq!(testkit::focused(&rt), Some(b));
        assert!(box_focused(&rt, b));
        testkit::click(&mut rt, &tile(a, "mr-tile-header"));
        assert_eq!(testkit::focused(&rt), Some(a));
        assert!(box_focused(&rt, a));
    }

    #[test]
    fn a_lower_tiles_pickers_open_inside_the_window() {
        let (mut rt, a, b) = two_tiles();
        stack(&mut rt, b, a);
        testkit::click(&mut rt, &model(b, "trigger"));
        testkit::step(&mut rt);
        assert_eq!(glue::view_of(&rt.state, b).open_picker, Some(Picker::Model));
        let ([x, y, w, h], _) = rt.presented(&model(b, "content")).expect("b's list");
        assert!(
            x >= 0.0 && y >= 0.0 && x + w <= SIZE.0 && y + h <= SIZE.1,
            "{:?} inside the window",
            [x, y, w, h]
        );
        let [bx, _, bw, _] = tile_rect(&rt, b);
        assert!(x >= bx && x + w <= bx + bw, "{x}, {w} inside {bx}, {bw}");
    }

    #[test]
    fn a_request_in_a_stacked_tile_is_answered_in_its_own_session() {
        let (mut rt, a, b) = two_tiles();
        ask_permission(&mut rt, a);
        ask_permission(&mut rt, b);
        assert_eq!((waiting(&rt, a), waiting(&rt, b)), (1, 1));
        stack(&mut rt, b, a);
        rt.settle_all();
        testkit::step(&mut rt);
        testkit::click(&mut rt, &tile(b, "mr-allow-r1"));
        testkit::step(&mut rt);
        assert_eq!(waiting(&rt, b), 0, "b's is answered");
        assert_eq!(waiting(&rt, a), 1, "a's still waits");
        testkit::click(&mut rt, &tile(a, "mr-allow-r1"));
        testkit::step(&mut rt);
        assert_eq!(waiting(&rt, a), 0, "then a's");
    }

    #[test]
    fn a_divider_between_columns_is_as_high_as_the_row() {
        let (mut rt, a, b, c) = three_tiles();
        stack(&mut rt, b, a);
        let row = row_rect(&rt);
        let handle = divider_element(&rt, a).expect("the divider right of a's column");
        assert_near(handle.rect[1], row[1], "the handle's top");
        assert_near(handle.rect[3], row[3], "the handle's height");
        let [ax, _, aw, _] = tile_rect(&rt, a);
        assert_near(handle.rect[0] + 4.0, ax + aw, "centred on the edge");
        assert!(divider_element(&rt, b).is_none(), "none for a lower tile");
        assert!(divider_element(&rt, c).is_none(), "none after the last");
        // Dragged, it resizes the column of two as one.
        let half = row[2] / 2.0;
        drag_divider(&mut rt, a, -200.0);
        let [ax, _, aw, _] = tile_rect(&rt, a);
        let [bx, _, bw, _] = tile_rect(&rt, b);
        assert_near(aw, half - 200.0, "a's tile");
        assert_eq!((ax, aw), (bx, bw), "b's tile with it");
        assert_near(tile_rect(&rt, c)[2], half + 200.0, "c's tile");
        assert_placed(&rt);
    }

    #[test]
    fn a_column_of_two_in_a_low_window_is_still_drawn() {
        let (mut rt, a, b) = two_tiles();
        stack(&mut rt, b, a);
        // The window is made lower than a column of two may be opened in: a row 400 high.
        let row = row_rect(&rt);
        rt.set_viewport((SIZE.0, SIZE.1 - (row[3] - 400.0)), 1.0);
        rt.settle_all();
        testkit::step(&mut rt);
        assert_near(row_rect(&rt)[3], 400.0, "the row's height");
        assert_eq!(drawn_tiles(&rt), vec![tile_key(a), tile_key(b)]);
        let (ah, bh) = (tile_rect(&rt, a)[3], tile_rect(&rt, b)[3]);
        assert!(ah < 280.0 && bh < 280.0, "{ah} and {bh}");
        assert!(ah > 0.0 && bh > 0.0, "{ah} and {bh}");
        assert_placed(&rt);
        for key in [a, b] {
            let [x, y, w, h] = tile_rect(&rt, key);
            let input = testkit::rect(&rt, &tile(key, "mr-input")).expect("a box");
            let [ix, iy, iw, ih] = input;
            assert!(
                ix >= x - 0.5 && iy >= y - 0.5 && ix + iw <= x + w + 0.5 && iy + ih <= y + h + 0.5,
                "{key:?}'s box {input:?} inside its tile {:?}",
                [x, y, w, h]
            );
        }
    }

    // The line between the two tiles of a column: Task 4 of the stacked tiles plan.

    /// `inner` lies inside `outer`, give or take half a point.
    fn inside(inner: [f32; 4], outer: [f32; 4]) -> bool {
        let ([ix, iy, iw, ih], [x, y, w, h]) = (inner, outer);
        ix >= x - 0.5 && iy >= y - 0.5 && ix + iw <= x + w + 0.5 && iy + ih <= y + h + 0.5
    }

    #[test]
    fn a_line_dragged_to_its_stop_keeps_both_boxes_in_their_tiles() {
        let (mut rt, _, b, c) = column_of_two();
        drag_split(&mut rt, b, -2000.0);
        let dragged = height(&rt, b);
        assert_near(dragged, 280.0, "b at its stop");
        let split = rt.state.app.tile_split(b);
        for window in [500.0, 400.0, SIZE.1] {
            rt.set_viewport((SIZE.0, window), 1.0);
            rt.settle_all();
            testkit::step(&mut rt);
            assert_placed(&rt);
            for (key, mate) in [(b, c), (c, b)] {
                let area = testkit::rect(&rt, &tile(key, "mr-input-area")).expect("a box");
                let (own, other) = (tile_rect(&rt, key), tile_rect(&rt, mate));
                assert!(
                    inside(area, own),
                    "in a window {window} high, {key:?}'s box {area:?} inside its tile {own:?}"
                );
                let down = (area[1] + area[3]).min(other[1] + other[3]) - area[1].max(other[1]);
                assert!(
                    down <= 0.5,
                    "in a window {window} high, {key:?}'s box {area:?} reaches into {other:?}"
                );
            }
            assert_eq!(rt.state.app.tile_split(b), split, "the split as dragged");
        }
        assert_near(height(&rt, b), dragged, "b as high as it was dragged");
    }

    /// `[a][b/c]`: three sessions, `c`'s tile under `b`'s, `c` focused.
    fn column_of_two() -> (Runtime<Ui>, SessionKey, SessionKey, SessionKey) {
        let (mut rt, a, b, c) = three_tiles();
        stack(&mut rt, c, b);
        assert_eq!(rt.state.app.tiles.columns().len(), 2);
        (rt, a, b, c)
    }

    /// The line under `key`'s tile, drawn and not on its way out.
    fn split_element(rt: &Runtime<Ui>, key: SessionKey) -> Option<rune::ElementInfo> {
        let key = split_key(key);
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key.as_str()))
    }

    /// The keys of every line drawn and not on its way out.
    fn drawn_lines(rt: &Runtime<Ui>) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting)
            .filter_map(|e| e.key)
            .filter(|k| k.starts_with("mr-split-"))
            .collect()
    }

    /// Presses the line under `key`'s tile and moves the pointer `by` points down, without
    /// letting go. Gives where the pointer is.
    fn hold_split(rt: &mut Runtime<Ui>, key: SessionKey, by: f32) -> (f32, f32) {
        let (x, y) = rt.find(&split_key(key)).expect("the line is drawn");
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move(x, y + by / 2.0);
        rt.pointer_move(x, y + by);
        testkit::step(rt);
        (x, y + by)
    }

    /// Drags the line under `key`'s tile `by` points down and lets go.
    fn drag_split(rt: &mut Runtime<Ui>, key: SessionKey, by: f32) {
        let (x, y) = hold_split(rt, key, by);
        rt.pointer_up(x, y);
        testkit::step(rt);
    }

    /// Presses the line under `key`'s tile twice, as a double click does, then steps.
    fn double_press_split(rt: &mut Runtime<Ui>, key: SessionKey) {
        let (x, y) = rt.find(&split_key(key)).expect("the line is drawn");
        rt.pointer_move(x, y);
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0 / 120.0);
        }
        testkit::step(rt);
    }

    /// The height of `key`'s tile as drawn.
    fn height(rt: &Runtime<Ui>, key: SessionKey) -> f32 {
        tile_rect(rt, key)[3]
    }

    /// Each column's split as last written to the settings: none for a column of one.
    fn saved_splits(rt: &Runtime<Ui>) -> Option<Vec<Option<f32>>> {
        let saved: mr_core::Saved = rt.state.prefs.get(glue::STATE_KEY)?;
        let splits = saved
            .tiles
            .iter()
            .map(|t| t.below.as_ref().map(|b| b.split));
        Some(splits.collect())
    }

    #[test]
    fn the_line_between_two_tiles_drags() {
        let (mut rt, a, b, c) = column_of_two();
        let handle = split_element(&rt, b).expect("b's line");
        assert_eq!(handle.role, "separator");
        assert_eq!(handle.label.as_deref(), Some("Tile height"));
        assert!(!handle.focusable, "no Tab stop");
        let [bx, by, bw, bh] = tile_rect(&rt, b);
        assert_near(handle.rect[3], 8.0, "the handle's height");
        assert_near(handle.rect[0], bx, "the handle's left");
        assert_near(handle.rect[2], bw, "as wide as the column");
        assert_near(handle.rect[1] + 4.0, by + bh, "centred on the line");
        let (upper, lower) = (height(&rt, b), height(&rt, c));
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        let (x, y) = hold_split(&mut rt, b, 100.0);
        assert_near(height(&rt, b), upper + 100.0, "b while held");
        assert_eq!(saved_splits(&rt), None, "nothing written during the moves");
        rt.pointer_up(x, y);
        testkit::step(&mut rt);
        assert_near(height(&rt, b), upper + 100.0, "b");
        assert_near(height(&rt, c), lower - 100.0, "c");
        assert_placed(&rt);
        let [_, by, _, bh] = tile_rect(&rt, b);
        let handle = testkit::rect(&rt, &split_key(b)).unwrap();
        assert_near(handle[1] + 4.0, by + bh, "the handle follows the line");
        let split = rt.state.app.tile_split(b);
        assert_ne!(split, Some(0.5));
        assert_eq!(
            saved_splits(&rt),
            Some(vec![None, split]),
            "written at the release"
        );
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        testkit::step(&mut rt);
        assert_eq!(saved_splits(&rt), None, "and only then");
        assert_eq!(
            tile_rect(&rt, a)[3],
            row_rect(&rt)[3],
            "a as high as the row"
        );
    }

    #[test]
    fn a_line_dragged_back_saves_nothing() {
        let (mut rt, _, b, c) = column_of_two();
        let before = (rt.state.app.tile_split(b), height(&rt, b), height(&rt, c));
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        let (x, y) = hold_split(&mut rt, b, 100.0);
        assert_ne!(rt.state.app.tile_split(b), before.0, "moved while held");
        rt.pointer_move(x, y - 100.0);
        testkit::step(&mut rt);
        rt.pointer_up(x, y - 100.0);
        testkit::step(&mut rt);
        let after = (rt.state.app.tile_split(b), height(&rt, b), height(&rt, c));
        assert_eq!(after, before);
        assert_eq!(saved_splits(&rt), None, "nothing written");
        // A press that moves nothing writes nothing either.
        let (x, y) = rt.find(&split_key(b)).unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        testkit::step(&mut rt);
        assert_eq!(saved_splits(&rt), None, "a press alone");
    }

    #[test]
    fn the_line_stops_at_the_least_height() {
        let (mut rt, _, b, c) = column_of_two();
        let row = row_rect(&rt)[3];
        drag_split(&mut rt, b, -2000.0);
        assert_near(height(&rt, b), 280.0, "b at its least");
        assert_near(height(&rt, c), row - 280.0, "c");
        drag_split(&mut rt, b, 4000.0);
        assert_near(height(&rt, c), 280.0, "c at its least");
        assert_near(height(&rt, b), row - 280.0, "b");
        assert_placed(&rt);
    }

    #[test]
    fn escape_puts_the_line_back() {
        let (mut rt, _, b, c) = column_of_two();
        // From the even split, then from one a drag left 130 points lower.
        for first in [None, Some(130.0)] {
            if let Some(by) = first {
                drag_split(&mut rt, b, by);
            }
            let before = (
                rt.state.app.tile_split(b),
                tile_rect(&rt, b),
                tile_rect(&rt, c),
            );
            rt.state.prefs.remove(glue::STATE_KEY).unwrap();
            let (x, y) = hold_split(&mut rt, b, -100.0);
            assert_ne!(rt.state.app.tile_split(b), before.0, "moved while held");
            testkit::press(&mut rt, Key::Escape);
            rt.pointer_up(x, y);
            testkit::step(&mut rt);
            let after = (
                rt.state.app.tile_split(b),
                tile_rect(&rt, b),
                tile_rect(&rt, c),
            );
            assert_eq!(after, before, "exactly as before the press, from {first:?}");
            assert_eq!(saved_splits(&rt), None, "nothing written");
        }
        assert_ne!(rt.state.app.tile_split(b), Some(0.5));
    }

    #[test]
    fn a_double_press_on_the_line_evens_the_two() {
        let (mut rt, _, b, c) = column_of_two();
        drag_split(&mut rt, b, 150.0);
        assert_ne!(rt.state.app.tile_split(b), Some(0.5));
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        double_press_split(&mut rt, b);
        assert_eq!(rt.state.app.tile_split(b), Some(0.5));
        assert_near(height(&rt, b), height(&rt, c), "the same height");
        assert_placed(&rt);
        assert_eq!(saved_splits(&rt), Some(vec![None, Some(0.5)]), "saved");
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        rt.frame(1.0);
        double_press_split(&mut rt, b);
        assert_eq!(rt.state.app.tile_split(b), Some(0.5));
        assert_eq!(saved_splits(&rt), None, "a second one changes nothing");
    }

    #[test]
    fn the_line_takes_neither_focus() {
        let (mut rt, a, b, _) = column_of_two();
        testkit::click(&mut rt, &tile(a, "mr-input"));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(a));
        assert!(box_focused(&rt, a));
        let (x, y) = hold_split(&mut rt, b, 100.0);
        assert_eq!(testkit::focused(&rt), Some(a), "a stays focused while held");
        assert!(box_focused(&rt, a));
        rt.pointer_up(x, y);
        testkit::step(&mut rt);
        double_press_split(&mut rt, b);
        assert_eq!(testkit::focused(&rt), Some(a), "the focused tile stays");
        assert!(box_focused(&rt, a));
        testkit::type_text(&mut rt, "for a");
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "for a");
        assert_eq!(glue::view_of(&rt.state, b).draft.text, "");
    }

    #[test]
    fn where_the_line_meets_a_divider_the_divider_drags() {
        let (mut rt, a, b, _) = column_of_two();
        let half = rt.state.app.row_width().unwrap() / 2.0;
        let split = rt.state.app.tile_split(b);
        let [bx, by, _, bh] = tile_rect(&rt, b);
        // Over both handles: the divider's right half and the line's left end.
        let (x, y) = (bx + 2.0, by + bh);
        let inside = |[l, t, w, h]: [f32; 4]| x > l && x < l + w && y > t && y < t + h;
        assert!(
            inside(testkit::rect(&rt, &divider_key(a)).unwrap()),
            "on the divider"
        );
        assert!(
            inside(testkit::rect(&rt, &split_key(b)).unwrap()),
            "on the line"
        );
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move(x - 50.0, y + 50.0);
        rt.pointer_move(x - 100.0, y + 100.0);
        testkit::step(&mut rt);
        rt.pointer_up(x - 100.0, y + 100.0);
        testkit::step(&mut rt);
        assert_near(widths(&rt)[0], half - 100.0, "a's column");
        assert_eq!(rt.state.app.tile_split(b), split, "the line stays");
        assert_placed(&rt);
    }

    #[test]
    fn dragging_the_line_rebuilds_no_row() {
        let (mut rt, _, b, c) = column_of_two();
        for key in [b, c] {
            for n in 1..=200 {
                demo::emit_to(&rt.state, key, SessionEvent::TextDelta(format!("Line {n}")));
                demo::emit_to(&rt.state, key, SessionEvent::BlockFinished);
            }
        }
        testkit::step(&mut rt);
        rt.settle_all();
        testkit::step(&mut rt);
        let (upper, rows) = (height(&rt, b), conversation::all_row_builds());
        assert!(rows > 0, "rows on screen in both");
        let (x, y) = rt.find(&split_key(b)).expect("the line is drawn");
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        for n in 1..=10 {
            rt.pointer_move(x, y + 10.0 * n as f32);
            testkit::step(&mut rt);
        }
        assert_near(height(&rt, b), upper + 100.0, "b while held");
        rt.pointer_up(x, y + 100.0);
        testkit::step(&mut rt);
        assert_near(height(&rt, b), upper + 100.0, "b");
        assert_eq!(conversation::all_row_builds(), rows, "no conversation row");
    }

    #[test]
    fn a_drag_pressed_before_the_size_was_known_changed_nothing() {
        // A press before the row was laid out keeps a length that is not a number.
        assert!(unchanged(f32::NAN, Some(300.0)));
        assert!(unchanged(f32::NAN, None));
        assert!(unchanged(300.0, Some(300.0)));
        assert!(!unchanged(300.0, Some(301.0)));
        assert!(!unchanged(300.0, None));
    }

    #[test]
    fn the_line_of_a_low_row_does_nothing() {
        let stored = |id: &str| StoredSession {
            id: SessionId(id.into()),
            folder: format!("/home/alex/{id}").into(),
            title: format!("Session {id}"),
            last_active_unix: demo::NOW_UNIX - demo::DAY,
        };
        let below = mr_core::SavedBelow {
            id: SessionId("b1".into()),
            split: 0.6,
        };
        let column = SavedTile {
            id: SessionId("a1".into()),
            share: 1.0,
            below: Some(below),
        };
        let mut prefs = Prefs::memory();
        let saved = Saved {
            version: SAVED_VERSION,
            shown: Some(SessionId("a1".into())),
            tiles: vec![column],
            ..Saved::default()
        };
        prefs.set(glue::STATE_KEY, &saved).unwrap();
        let mut ui = demo::ui_with(None, prefs);
        let fake = ui.fake.clone().unwrap();
        fake.set_sessions(vec![stored("a1"), stored("b1")]);
        glue::setup(&mut ui, std::sync::Arc::new(|| {}));
        let mut rt = testkit::runtime(ui, (SIZE.0, 500.0));
        for _ in 0..20 {
            testkit::step(&mut rt);
        }
        let keys = tile_keys(&rt);
        let [a, b] = keys[..] else {
            panic!("the column came back: {keys:?}")
        };
        assert_eq!(rt.state.app.tiles.mate(a), Some(b));
        assert!(row_rect(&rt)[3] < 560.0, "{:?}", row_rect(&rt));
        let before = (
            rt.state.app.tile_split(a),
            tile_rect(&rt, a),
            tile_rect(&rt, b),
        );
        assert_eq!(before.0, Some(0.6));
        assert_eq!(before.1[3], before.2[3], "drawn even in a row this low");
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        for by in [-60.0, 60.0] {
            drag_split(&mut rt, a, by);
            let after = (
                rt.state.app.tile_split(a),
                tile_rect(&rt, a),
                tile_rect(&rt, b),
            );
            assert_eq!(after, before, "dragged {by}");
        }
        assert_eq!(saved_splits(&rt), None, "nothing written");
    }

    #[test]
    fn a_column_of_one_has_no_line() {
        let (mut rt, a, b, c) = three_tiles();
        assert!(drawn_lines(&rt).is_empty(), "{:?}", drawn_lines(&rt));
        stack(&mut rt, c, b);
        assert_eq!(drawn_lines(&rt), vec![split_key(b)]);
        assert!(split_element(&rt, a).is_none() && split_element(&rt, c).is_none());
        move_to(&mut rt, c, Spot::Edge(2));
        assert!(drawn_lines(&rt).is_empty(), "{:?}", drawn_lines(&rt));
    }

    /// The window as last drawn, rendered on the GPU: 8-bit RGBA rows, one pixel a point.
    fn drawn_pixels(rt: &mut Runtime<Ui>) -> Vec<u8> {
        use rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer};
        let gpu = Gpu::headless();
        let size = (SIZE.0 as u32, SIZE.1 as u32);
        let format = if rt.srgb_blend() {
            OFFSCREEN_FORMAT.remove_srgb_suffix()
        } else {
            OFFSCREEN_FORMAT
        };
        let mut renderer = Renderer::new(&gpu.device, format);
        renderer.set_srgb_blend(rt.srgb_blend());
        let target = Offscreen::with_format(&gpu.device, size.0, size.1, format);
        renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
        renderer.prepare_surfaces(&gpu.device, &gpu.queue, &rt.surfaces, size);
        renderer.prepare(
            &gpu.device,
            &gpu.queue,
            &mut rt.text.atlas,
            &rt.instances,
            size,
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.draw(&mut encoder, &target.view, rt.clear_color());
        target.copy(&mut encoder);
        renderer.submit(&gpu.queue, encoder);
        target.read(&gpu.device)
    }

    #[test]
    fn the_upper_tile_of_two_draws_a_line_on_its_lower_edge() {
        let (mut rt, a, b, c) = column_of_two();
        let d = testkit::add_session(&mut rt, "/home/alex/d");
        testkit::show(&mut rt, c);
        testkit::show_side(&mut rt, d);
        // [a][b/c][d], d focused: neither tile of the column, nor a, has an outline.
        assert_eq!(testkit::focused(&rt), Some(d));
        for key in [a, b, c] {
            assert_eq!(outlined(&rt, key), None);
        }
        let pixels = drawn_pixels(&mut rt);
        let at = |x: f32, y: f32| {
            let i = (y as usize * SIZE.0 as usize + x as usize) * 4;
            <[u8; 4]>::try_from(&pixels[i..i + 4]).unwrap()
        };
        // Along the lower edge of a tile, 30 points in from its left: the edge's last row of
        // pixels, and rows 3 points above and below it.
        let edge = |key: SessionKey| {
            let [x, y, _, h] = tile_rect(&rt, key);
            let (x, bottom) = (x + 30.0, y + h);
            (
                at(x, bottom - 3.0),
                at(x, bottom - 0.5),
                at(x, bottom + 3.0),
            )
        };
        let (above, line, below) = edge(b);
        assert_eq!(above, below, "the tiles' own ground on either side");
        assert_ne!(line, above, "b's lower edge is a line");
        let (above, last, _) = edge(a);
        assert_eq!(last, above, "a, alone in its column, has none");
        let (above, last, _) = edge(c);
        assert_eq!(last, above, "nor has c, the lower of two");
    }

    // A tile moved into, out of and within a column by the keyboard: Task 6 of the stacked
    // tiles plan.

    /// Ctrl+Alt+PageUp, or Cmd+Alt+PageUp on macOS.
    fn stack_up() -> KeyEvent {
        KeyEvent {
            key: Key::PageUp,
            mods: Mods {
                alt: true,
                ..testkit::ctrl()
            },
        }
    }

    /// Ctrl+Alt+PageDown, or Cmd+Alt+PageDown on macOS.
    fn stack_down() -> KeyEvent {
        KeyEvent {
            key: Key::PageDown,
            mods: Mods {
                alt: true,
                ..testkit::ctrl()
            },
        }
    }

    /// Gives `ev` to the window's keys: it is taken, and nothing in the row changes.
    fn taken_and_nothing_moves(rt: &mut Runtime<Ui>, ev: KeyEvent) {
        let (before, version) = (columns(rt), rt.state.app.tiles.version());
        rt.update(|ui, tx| assert!(super::super::on_key(ui, &ev, tx), "taken: {ev:?}"));
        testkit::step(rt);
        assert_eq!(columns(rt), before, "nothing moves: {ev:?}");
        assert_eq!(rt.state.app.tiles.version(), version, "{ev:?}");
    }

    /// Presses `ev` as the user does, then steps.
    fn key(rt: &mut Runtime<Ui>, ev: KeyEvent) {
        rt.key(ev);
        testkit::step(rt);
    }

    #[test]
    fn ctrl_shift_page_down_passes_a_column_of_two() {
        let (mut rt, a, b, c) = three_tiles();
        stack(&mut rt, c, b);
        focus(&mut rt, a);
        key(&mut rt, move_right());
        assert_eq!(columns(&rt), vec![vec![b, c], vec![a]]);
        assert_eq!(testkit::focused(&rt), Some(a));
        key(&mut rt, move_left());
        assert_eq!(columns(&rt), vec![vec![a], vec![b, c]], "and back");
    }

    #[test]
    fn ctrl_shift_page_down_takes_a_tile_out_of_its_column() {
        let (mut rt, a, b) = two_tiles();
        stack(&mut rt, b, a);
        assert_eq!(testkit::focused(&rt), Some(b));
        key(&mut rt, move_right());
        assert_eq!(columns(&rt), vec![vec![a], vec![b]]);
        assert_eq!(testkit::focused(&rt), Some(b));
        stack(&mut rt, b, a);
        key(&mut rt, move_left());
        assert_eq!(columns(&rt), vec![vec![b], vec![a]]);
        assert_eq!(testkit::focused(&rt), Some(b));
    }

    #[test]
    fn with_no_room_the_key_is_taken_and_nothing_moves() {
        let mut rt = testkit::window_sized("empty", (1300.0, 900.0));
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        stack(&mut rt, b, a);
        testkit::show_side(&mut rt, c);
        assert_eq!(columns(&rt), vec![vec![a, b], vec![c]]);
        assert!(!rt.state.app.room_for_tile());
        for key in [a, b] {
            focus(&mut rt, key);
            taken_and_nothing_moves(&mut rt, move_left());
            taken_and_nothing_moves(&mut rt, move_right());
            assert_eq!(testkit::focused(&rt), Some(key));
        }
    }

    #[test]
    fn ctrl_alt_page_down_puts_a_tile_under_its_left_neighbour() {
        let (mut rt, a, b) = two_tiles();
        key(&mut rt, stack_down());
        assert_eq!(columns(&rt), vec![vec![a, b]]);
        assert_eq!(testkit::focused(&rt), Some(b));
        assert_placed(&rt);
    }

    #[test]
    fn ctrl_alt_page_up_puts_it_over() {
        let (mut rt, a, b) = two_tiles();
        key(&mut rt, stack_up());
        assert_eq!(columns(&rt), vec![vec![b, a]]);
        assert_eq!(testkit::focused(&rt), Some(b));
    }

    #[test]
    fn the_first_column_joins_the_second() {
        for (ev, column) in [(stack_down(), [1, 0]), (stack_up(), [0, 1])] {
            let (mut rt, a, b) = two_tiles();
            focus(&mut rt, a);
            key(&mut rt, ev.clone());
            let keys = [a, b];
            assert_eq!(
                columns(&rt),
                vec![column.map(|i| keys[i]).to_vec()],
                "{ev:?}"
            );
            assert_eq!(testkit::focused(&rt), Some(a));
        }
    }

    #[test]
    fn in_a_column_of_two_the_keys_swap() {
        let (mut rt, a, b) = two_tiles();
        stack(&mut rt, b, a);
        focus(&mut rt, a);
        // The upper: down swaps, up does nothing.
        taken_and_nothing_moves(&mut rt, stack_up());
        key(&mut rt, stack_down());
        assert_eq!(columns(&rt), vec![vec![b, a]]);
        assert_eq!(testkit::focused(&rt), Some(a));
        // Now the lower: down does nothing, up swaps.
        taken_and_nothing_moves(&mut rt, stack_down());
        key(&mut rt, stack_up());
        assert_eq!(columns(&rt), vec![vec![a, b]]);
        assert_eq!(testkit::focused(&rt), Some(a));
    }

    #[test]
    fn a_full_neighbour_takes_nothing() {
        let (mut rt, a, b, c) = three_tiles();
        stack(&mut rt, b, a);
        focus(&mut rt, c);
        taken_and_nothing_moves(&mut rt, stack_up());
        taken_and_nothing_moves(&mut rt, stack_down());
        assert_eq!(columns(&rt), vec![vec![a, b], vec![c]]);
        assert_eq!(testkit::focused(&rt), Some(c));
    }

    #[test]
    fn a_low_window_stacks_nothing_by_key() {
        let mut rt = testkit::window_sized("empty", (1800.0, 500.0));
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        assert!(!mr_core::Tiles::fits_two(row_rect(&rt)[3]));
        for key in [b, a] {
            focus(&mut rt, key);
            taken_and_nothing_moves(&mut rt, stack_up());
            taken_and_nothing_moves(&mut rt, stack_down());
        }
        assert_eq!(columns(&rt), vec![vec![a], vec![b]]);
    }

    #[test]
    fn one_tile_alone_does_nothing() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        taken_and_nothing_moves(&mut rt, stack_up());
        taken_and_nothing_moves(&mut rt, stack_down());
        assert_eq!(columns(&rt), vec![vec![a]]);
    }

    #[test]
    fn the_keyboard_stays_in_the_box_when_its_tile_is_stacked_by_key() {
        let (mut rt, a, b) = two_tiles();
        draft_in(&mut rt, b);
        key(&mut rt, stack_down());
        assert_eq!(columns(&rt), vec![vec![a, b]]);
        assert_still_typing(&mut rt, b);
        key(&mut rt, stack_up());
        assert_eq!(columns(&rt), vec![vec![b, a]], "the swap, from the box");
        assert!(box_focused(&rt, b));
    }

    #[test]
    fn the_stack_keys_wait() {
        let (mut rt, a, b, c) = three_tiles();
        let all = [stack_up(), stack_down()];
        // While a picker's list is open.
        testkit::click(&mut rt, &model(c, "trigger"));
        testkit::step(&mut rt);
        assert_eq!(glue::view_of(&rt.state, c).open_picker, Some(Picker::Model));
        for ev in all.clone() {
            taken_and_nothing_moves(&mut rt, ev);
        }
        rt.update(|ui, _| glue::view_mut(ui, c).open_picker = None);
        testkit::step(&mut rt);
        // While a row's menu is open.
        let (x, y) = rt.find(&format!("mr-row-{}", a.0)).unwrap();
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_some(), "the row's menu is open");
        for ev in all.clone() {
            taken_and_nothing_moves(&mut rt, ev);
        }
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        // While a row is dragged.
        let [x, y, w, h] = row_rect(&rt);
        testkit::hold_row(&mut rt, a, (x + w / 2.0, y + h / 2.0));
        assert!(rt.state.drag.is_some(), "a row is dragged");
        for ev in all.clone() {
            taken_and_nothing_moves(&mut rt, ev);
        }
        assert!(rt.state.drag.is_some(), "the drag goes on");
        testkit::press(&mut rt, Key::Escape);
        testkit::release(&mut rt);
        assert_eq!(columns(&rt), vec![vec![a], vec![b], vec![c]]);
        // On another screen they are not taken.
        rt.update(|ui, _| {
            let effects = ui.app.account_checked(mr_agent::Account::Missing);
            glue::run(ui, effects);
        });
        testkit::step(&mut rt);
        for ev in all {
            rt.update(|ui, tx| assert!(!super::super::on_key(ui, &ev, tx), "not taken"));
            testkit::step(&mut rt);
        }
        assert_eq!(
            columns(&rt),
            vec![vec![a], vec![b], vec![c]],
            "nothing moves"
        );
    }

    #[test]
    fn f6_goes_column_by_column_the_upper_first() {
        let (mut rt, a, b, c) = three_tiles();
        stack(&mut rt, b, a);
        assert_eq!(columns(&rt), vec![vec![a, b], vec![c]]);
        focus(&mut rt, a);
        for next in [b, c, a] {
            testkit::press(&mut rt, Key::Function(6));
            testkit::step(&mut rt);
            assert_eq!(testkit::focused(&rt), Some(next), "F6");
            assert!(box_focused(&rt, next));
        }
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        for next in [c, b, a] {
            testkit::press_with(&mut rt, Key::Function(6), shift);
            testkit::step(&mut rt);
            assert_eq!(testkit::focused(&rt), Some(next), "Shift+F6");
            assert!(box_focused(&rt, next));
        }
    }

    // A key that moves the tiles while a handle is held: the fix wave after the whole-branch
    // review of the stacked tiles plan.

    /// Each column's share, left to right.
    fn shares(rt: &Runtime<Ui>) -> Vec<f32> {
        let columns = rt.state.app.tiles.columns();
        columns.iter().map(|column| column.share).collect()
    }

    #[test]
    fn a_held_divider_puts_nothing_back_after_a_key_moved_the_tiles() {
        let (mut rt, a, b, c) = three_tiles();
        focus(&mut rt, a);
        let (x, y) = hold_divider(&mut rt, a, -60.0);
        assert!(rt.state.divider_press.is_some(), "kept at the press");
        key(&mut rt, move_right());
        assert_eq!(columns(&rt), vec![vec![b], vec![a], vec![c]]);
        assert_eq!(rt.state.divider_press, None, "forgotten");
        let moved = shares(&rt);
        testkit::press(&mut rt, Key::Escape);
        rt.pointer_up(x, y);
        testkit::step(&mut rt);
        assert_eq!(shares(&rt), moved, "nothing put back");
        let before = widths(&rt);
        drag_divider(&mut rt, b, -100.0);
        assert_near(widths(&rt)[0], before[0] - 100.0, "the next drag");
        assert_placed(&rt);
    }

    #[test]
    fn a_held_line_puts_nothing_back_after_a_key_moved_the_tiles() {
        let (mut rt, _, b, c) = column_of_two();
        focus(&mut rt, b);
        let (x, y) = hold_split(&mut rt, b, 100.0);
        assert!(rt.state.split_press.is_some(), "kept at the press");
        key(&mut rt, stack_down());
        assert_eq!(columns(&rt)[1], vec![c, b], "swapped");
        assert_eq!(rt.state.split_press, None, "forgotten");
        key(&mut rt, stack_up());
        assert_eq!(columns(&rt)[1], vec![b, c], "and back");
        let moved = rt.state.app.tile_split(b);
        testkit::press(&mut rt, Key::Escape);
        rt.pointer_up(x, y);
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.tile_split(b), moved, "nothing put back");
        let before = height(&rt, b);
        drag_split(&mut rt, b, -100.0);
        assert_near(height(&rt, b), before - 100.0, "the next drag");
        assert_placed(&rt);
    }

    // The mark of Remote Control in a header: Task 10 of plan 5.

    /// `key`'s session asks for Remote Control, as its switch in the panel does, then steps.
    fn ask_remote(rt: &mut Runtime<Ui>, key: SessionKey) {
        rt.update(|ui, _| {
            let effects = ui.app.set_remote(key, true);
            glue::run(ui, effects);
        });
        testkit::step(rt);
    }

    /// The program says `state` of Remote Control to `key`'s session.
    fn remote_says(rt: &mut Runtime<Ui>, key: SessionKey, state: RemoteAccess) {
        testkit::emit_to(rt, key, SessionEvent::RemoteAccess(state));
    }

    /// Remote Control on, reachable at the address tests use.
    fn reachable() -> RemoteAccess {
        RemoteAccess::On {
            url: Some("https://example.org/session".into()),
        }
    }

    /// The mark in `key`'s header and its colour, once the window has settled.
    fn mark_in(rt: &mut Runtime<Ui>, key: SessionKey) -> Option<(String, Color)> {
        // A text's colour comes in on a spring.
        rt.settle_all();
        testkit::step(rt);
        header_part(rt, key, "mr-tile-remote")
    }

    #[test]
    fn a_tile_says_remote_while_it_is_on() {
        let (mut rt, a, b) = two_tiles();
        let c = theme::kit(true).c;
        let muted = Some(("Remote".to_owned(), c.text_muted));
        let on = Some(("Remote".to_owned(), c.text_accent));
        assert_eq!(mark_in(&mut rt, b), None);
        ask_remote(&mut rt, b);
        remote_says(&mut rt, b, RemoteAccess::Starting);
        assert_eq!(mark_in(&mut rt, b), muted, "while it connects");
        remote_says(&mut rt, b, reachable());
        assert_eq!(mark_in(&mut rt, b), on);
        assert_eq!(mark_in(&mut rt, a), None, "b's alone");
        remote_says(&mut rt, b, RemoteAccess::Reconnecting);
        assert_eq!(mark_in(&mut rt, b), muted, "while it reconnects");
        remote_says(&mut rt, b, RemoteAccess::Off);
        assert_eq!(mark_in(&mut rt, b), None, "off");
        ask_remote(&mut rt, b);
        remote_says(&mut rt, b, RemoteAccess::Starting);
        let refused = RemoteAccess::Failed {
            reason: "/login".into(),
            why: RemoteFailure::Login,
        };
        remote_says(&mut rt, b, refused);
        assert_eq!(mark_in(&mut rt, b), None, "refused");
    }

    #[test]
    fn the_mark_takes_its_room_from_the_title() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let key = testkit::focused(&rt).unwrap();
        let long = "Planning the spring beds for the community garden down by the river";
        testkit::emit(&mut rt, SessionEvent::TitleChanged(long.into()));
        testkit::emit(&mut rt, SessionEvent::TurnStarted);
        ask_remote(&mut rt, key);
        remote_says(&mut rt, key, reachable());
        let part = |rt: &Runtime<Ui>, inner: &str| {
            testkit::rect(rt, &tile(key, inner)).unwrap_or_else(|| panic!("{inner} is drawn"))
        };
        rt.settle_all();
        testkit::step(&mut rt);
        let [_, _, wide_title, _] = part(&rt, "mr-tile-title");
        let whole = ["mr-tile-state", "mr-tile-remote", "mr-tile-close"].map(|p| part(&rt, p)[2]);
        // A row of 420 points: the window less the sidebar and the main card's frame.
        rt.set_viewport((260.0 + 8.0 + 420.0, SIZE.1), 1.0);
        rt.settle_all();
        testkit::step(&mut rt);
        let [tx, _, tw, _] = tile_rect(&rt, key);
        assert!((tw - 420.0).abs() <= 1.0, "a tile of 420 points: {tw}");
        let [title_x, _, title_w, _] = part(&rt, "mr-tile-title");
        let [state_x, _, state_w, _] = part(&rt, "mr-tile-state");
        let [mark_x, _, mark_w, _] = part(&rt, "mr-tile-remote");
        let [close_x, _, close_w, _] = part(&rt, "mr-tile-close");
        assert_eq!([state_w, mark_w, close_w], whole, "the three are whole");
        assert!(
            title_w < wide_title,
            "the title is cut: {title_w} of {wide_title}"
        );
        assert!(
            title_x + title_w <= state_x,
            "the title ends before the word"
        );
        assert!(state_x + state_w <= mark_x, "the word ends before the mark");
        assert!(
            mark_x + mark_w <= close_x,
            "the mark ends before the button"
        );
        assert!(close_x + close_w <= tx + tw + 0.5, "the button is inside");
    }

    #[test]
    fn a_press_on_the_mark_is_a_press_on_the_header() {
        let (mut rt, a, b) = two_tiles();
        ask_remote(&mut rt, a);
        remote_says(&mut rt, a, reachable());
        assert_eq!(testkit::focused(&rt), Some(b));
        let mark = tile(a, "mr-tile-remote");
        testkit::click(&mut rt, &mark);
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(a), "the tile is focused");
        assert!(box_focused(&rt, a), "its box has the keyboard");
        // A drag from the mark moves the tile, as one from the header does.
        let (x, y) = testkit::find(&rt, &mark).unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        testkit::step(&mut rt);
        let [rx, ry, rw, rh] = rt.presented(ROW).unwrap().0;
        testkit::move_held(&mut rt, (rx + rw - 20.0, ry + rh / 2.0));
        let dragged = rt.state.drag.as_ref().map(|drag| (drag.key, drag.from));
        assert_eq!(dragged, Some((a, DragFrom::Header)));
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, a], "moved");
    }

    /// How many chips `key`'s session holds.
    fn chips(rt: &Runtime<Ui>, key: SessionKey) -> usize {
        rt.state.app.sessions.get(key).unwrap().session.chips.len()
    }

    /// The centre of `key`'s tile, which is in its conversation.
    fn tile_middle(rt: &Runtime<Ui>, key: SessionKey) -> (f32, f32) {
        let [x, y, w, h] = tile_rect(rt, key);
        (x + w / 2.0, y + h / 2.0)
    }

    /// The drop veil drawn in `key`'s tile, if any.
    fn veil(rt: &Runtime<Ui>, key: SessionKey) -> Option<rune::ElementInfo> {
        testkit::element(rt, &tile(key, "mr-drop-files"))
    }

    /// No mark of a dragged row or header is drawn.
    fn no_drag_marks(rt: &Runtime<Ui>) {
        for mark in ["mr-drag-chip", "mr-drop-edge", "mr-drop-half"] {
            assert!(!testkit::drawn(rt, mark), "{mark} is not drawn");
        }
    }

    #[test]
    fn files_held_over_a_tile_show_the_veil_and_leaving_takes_it_away() {
        let (mut rt, a, b) = two_tiles();
        let outlines = (outlined(&rt, a), outlined(&rt, b));
        let paths = vec![PathBuf::from("/home/alex/a/notes.md")];
        let at = tile_middle(&rt, a);
        rt.drag_files(paths, Some(at));
        testkit::step(&mut rt);
        assert_eq!(rt.drop_hovered(), Some(tile_key(a)), "a is the target");
        assert_eq!(rt.state.file_drop, Some(a));
        let shown = veil(&rt, a).expect("a shows the veil");
        assert!(veil(&rt, b).is_none(), "b shows none");
        let [x, y, w, h] = tile_rect(&rt, a);
        assert_eq!(
            shown.rect,
            [x, y + super::HEADER_HEIGHT, w, h - super::HEADER_HEIGHT],
            "over the pane under the header"
        );
        assert!(shown.inert, "it takes no presses");
        let accent = theme::kit(true).c.bg_accent;
        assert_eq!(shown.fill, accent.alpha(0.12));
        let words = testkit::element(&rt, &tile(a, "mr-drop-files/mr-drop-files-text"))
            .expect("the veil has its words");
        assert_eq!(words.text.as_deref(), Some("Drop to attach"));
        let [wx, wy, ww, wh] = words.rect;
        let middle = (wx + ww / 2.0, wy + wh / 2.0);
        let centre = (
            x + w / 2.0,
            y + super::HEADER_HEIGHT + (h - super::HEADER_HEIGHT) / 2.0,
        );
        assert!(
            (middle.0 - centre.0).abs() < 1.0 && (middle.1 - centre.1).abs() < 1.0,
            "in the middle: {middle:?}, {centre:?}"
        );
        assert_eq!(
            (outlined(&rt, a), outlined(&rt, b)),
            outlines,
            "no outline changes"
        );
        no_drag_marks(&rt);
        // Over b, the veil moves to b.
        let over_b = tile_middle(&rt, b);
        rt.drag_files_to(Some(over_b));
        testkit::step(&mut rt);
        assert_eq!(rt.state.file_drop, Some(b));
        assert!(veil(&rt, a).is_none() && veil(&rt, b).is_some());
        // Leaving the window takes it away.
        rt.drag_files_leave();
        testkit::step(&mut rt);
        assert_eq!(rt.state.file_drop, None);
        assert!(veil(&rt, a).is_none() && veil(&rt, b).is_none());
        assert_eq!(chips(&rt, a) + chips(&rt, b), 0, "nothing was dropped");
    }

    #[test]
    fn a_tile_that_goes_takes_its_veil() {
        let (mut rt, a, b) = two_tiles();
        let at = tile_middle(&rt, a);
        rt.drag_files(vec![PathBuf::from("/home/alex/a/notes.md")], Some(at));
        testkit::step(&mut rt);
        assert_eq!(rt.state.file_drop, Some(a));
        rt.update(|ui, tx| glue::close_tile(ui, tx, a));
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b]);
        assert_eq!(rt.state.file_drop, None);
        assert!(
            veil(&rt, b).is_none(),
            "the tile that took its room shows none"
        );
    }

    #[test]
    fn dropped_files_become_chips_of_the_tile_under_the_pointer() {
        let (mut rt, a, b) = two_tiles();
        assert_eq!(testkit::focused(&rt), Some(b));
        let folder = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = ["notes.md", "plan.txt"]
            .iter()
            .map(|name| folder.path().join(name))
            .collect();
        for path in &paths {
            std::fs::write(path, "made by the test").unwrap();
        }
        let at = tile_middle(&rt, a);
        rt.drag_files(paths.clone(), Some(at));
        testkit::step(&mut rt);
        assert!(veil(&rt, a).is_some());
        rt.drop_files(paths.clone(), Some(at));
        testkit::step(&mut rt);
        assert_eq!(chips(&rt, a), 2, "a has the chips");
        assert_eq!(chips(&rt, b), 0, "b has none");
        let session = &rt.state.app.sessions.get(a).unwrap().session;
        let sources: Vec<Source> = session.chips.iter().map(|c| c.source.clone()).collect();
        assert_eq!(
            sources,
            paths.iter().cloned().map(Source::Path).collect::<Vec<_>>()
        );
        assert_eq!(testkit::focused(&rt), Some(a), "a has the focus");
        assert!(box_focused(&rt, a), "and its box the keyboard");
        assert_eq!(rt.state.file_drop, None);
        assert!(veil(&rt, a).is_none(), "the veil goes with the drop");
        // A drop on the sidebar does nothing.
        let (x, y) = testkit::find(&rt, &testkit::row_key(b)).unwrap();
        let elsewhere = vec![PathBuf::from("/home/alex/b/elsewhere.md")];
        rt.drop_files(elsewhere, Some((x, y)));
        testkit::step(&mut rt);
        assert_eq!((chips(&rt, a), chips(&rt, b)), (2, 0));
        assert_eq!(testkit::focused(&rt), Some(a));
    }

    #[test]
    fn a_drop_on_a_session_that_stopped_taking_messages_moves_no_focus() {
        let (mut rt, a, b) = two_tiles();
        assert_eq!(testkit::focused(&rt), Some(b));
        let paths = vec![PathBuf::from("/home/alex/a/notes.md")];
        let at = tile_middle(&rt, a);
        rt.drag_files(paths.clone(), Some(at));
        testkit::step(&mut rt);
        assert!(veil(&rt, a).is_some(), "a took the drag");
        // Between the frame that drew a's tile and the drop, a stops taking messages: its check
        // in the store begins. The tile drawn still accepts the files.
        rt.state.app.sessions.get_mut(a).unwrap().checking = true;
        rt.drop_files(paths, Some(at));
        testkit::step(&mut rt);
        assert_eq!(chips(&rt, a), 0);
        assert_eq!(testkit::focused(&rt), Some(b), "the focus stays");
        assert!(box_focused(&rt, b), "and the keyboard with it");
        assert_eq!(rt.state.file_drop, None);
        assert!(veil(&rt, a).is_none(), "the veil goes with the drop");
    }

    #[test]
    fn an_ended_sessions_tile_takes_no_files() {
        let (mut rt, a, b) = two_tiles();
        let ended = SessionEvent::Exited {
            clean: false,
            detail: "The program stopped".into(),
        };
        testkit::emit_to(&mut rt, a, ended);
        let paths = vec![PathBuf::from("/home/alex/a/notes.md")];
        let at = tile_middle(&rt, a);
        rt.drag_files(paths.clone(), Some(at));
        testkit::step(&mut rt);
        assert_eq!(rt.drop_hovered(), None, "the drag passes it by");
        assert_eq!(rt.state.file_drop, None);
        assert!(veil(&rt, a).is_none());
        rt.drop_files(paths, Some(at));
        testkit::step(&mut rt);
        assert_eq!((chips(&rt, a), chips(&rt, b)), (0, 0));
        assert_eq!(testkit::focused(&rt), Some(b), "the focus stays");
    }

    #[test]
    fn a_dragged_row_shows_no_veil() {
        let (mut rt, a, b) = two_tiles();
        let over_b = tile_middle(&rt, b);
        testkit::hold_row(&mut rt, a, over_b);
        assert!(rt.state.drag.is_some(), "a row is dragged");
        assert_eq!(rt.state.file_drop, None);
        assert!(veil(&rt, a).is_none() && veil(&rt, b).is_none());
        testkit::press(&mut rt, Key::Escape);
        testkit::release(&mut rt);
        // Nor does a dragged header.
        let (x, y) = testkit::find(&rt, &tile(a, "mr-tile-title")).unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        testkit::step(&mut rt);
        testkit::move_held(&mut rt, over_b);
        let dragged = rt.state.drag.as_ref().map(|drag| (drag.key, drag.from));
        assert_eq!(dragged, Some((a, DragFrom::Header)));
        assert_eq!(rt.state.file_drop, None);
        assert!(veil(&rt, a).is_none() && veil(&rt, b).is_none());
        testkit::press(&mut rt, Key::Escape);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b], "nothing moved");
    }

    #[test]
    fn a_paste_with_no_box_focused_goes_to_the_focused_tile() {
        let (mut rt, a, b) = two_tiles();
        rt.update(|ui, _| {
            for key in [a, b] {
                glue::view_mut(ui, key).draft.focused = false;
            }
        });
        rt.invalidate();
        testkit::step(&mut rt);
        assert!(
            !box_focused(&rt, a) && !box_focused(&rt, b),
            "no box has the keyboard"
        );
        assert_eq!(testkit::focused(&rt), Some(b));
        testkit::paste_image(&mut rt, testkit::tiny_png());
        testkit::step(&mut rt);
        assert_eq!((chips(&rt, a), chips(&rt, b)), (0, 1), "the image is b's");
        testkit::paste_files(&mut rt, &[PathBuf::from("/home/alex/b/notes.md")]);
        testkit::step(&mut rt);
        assert_eq!((chips(&rt, a), chips(&rt, b)), (0, 2), "the file is b's");
        // Text pasted with no box focused does nothing.
        rt.paste("some words");
        testkit::step(&mut rt);
        assert_eq!((chips(&rt, a), chips(&rt, b)), (0, 2));
        for key in [a, b] {
            assert_eq!(glue::view_of(&rt.state, key).draft.text, "");
        }
    }
}
