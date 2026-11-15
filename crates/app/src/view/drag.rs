//! A sidebar row dragged into the row of tiles or onto a group, and a tile dragged along the
//! row by its header. Rune has no drag and drop between elements, so it is built here by hand:
//! from the row's or the header's `on_drag`, the tile row's rectangle as last laid out
//! (`Ui::row_rect`) and the tiles' widths, what Rune finds under the pointer in the sidebar's
//! list (`Tx::element_at_in`), and the overlay layer. One drag is held at a time, in
//! `Ui::drag`, whichever it took hold of.
//!
//! A drag can fail to end as drags do: Escape and a lost pointer end it with a cancelled
//! release, and a row that unmounts (scrolled out of the list), a tile that closes or a menu
//! opening (which takes the pointer) end it with no release at all: the watcher, the window's
//! Escape (`view::on_key`) and `glue::tidy_sidebar` end those. Each ends it with nothing left
//! behind: no chip, no line, no outline, and no press swallowed later.

use mr_core::{GroupId, Place, SessionKey};
use rune::{Drag, El, Phase, Picked, PointerPress, Transition, Tx, el, row as row_el, text};
use rune_kit::Kit;

use crate::glue::{self, DragFrom, DropTarget, Renaming, RowDrag, Ui};

/// How far the pointer travels from the press before a press on a row or a header becomes a
/// drag.
pub const SLOP: f32 = 4.0;

/// The key of the sidebar's list, whose lines a drop on a group is found among.
const LIST: &str = "mr-sidebar-rows";

/// The keys of the title by the pointer, of the line where a tile would open and of the
/// watcher.
const CHIP: &str = "mr-drag-chip";
const EDGE: &str = "mr-drop-edge";
const WATCH: &str = "mr-drag-watch";

/// How thick the line where a tile would open is.
const EDGE_WIDTH: f32 = 2.0;

/// Where the chip sits from the pointer, right and below it.
const CHIP_OFFSET: [f32; 2] = [14.0, 10.0];

/// The widest the chip grows before its title is cut.
const CHIP_MAX_WIDTH: f32 = 240.0;

/// The title of a session that has none yet, as its row reads.
const NEW_SESSION: &str = "New session";

/// `Drag::data` once the press has passed the slop: the drag started, and a press that ends
/// it may have a press still to swallow, even after the drag ended some other way.
const STARTED: f32 = 1.0;

/// The handler for a row's `on_drag`.
///
/// A press becomes a drag once the pointer is more than [`SLOP`] from where it went down; until
/// then nothing shows and the release is a click. Each move puts the pointer and what a drop
/// there would do into `Ui::drag`, and nothing else: in the tile row [`target_in_tiles`],
/// elsewhere the group of the sidebar line under the pointer, once Rune says which ([`ask`]).
/// A drag that ended before its release (the watcher, a menu, its session gone) does not start
/// again in the same press.
///
/// The release does what the drag is over, and over a group moves the session there; with
/// nothing under it, a session in a tile dropped on the tile row focuses that tile. It sets
/// `swallow_row_press` (see there): Rune runs the row's own press after it when it holds the
/// pointer inside the row, and that press must not show the session again. A cancelled release
/// (Escape, the pointer lost) does nothing, and no press follows it.
pub fn row(key: SessionKey) -> impl Fn(&mut Ui, &mut Drag, &mut Tx) + 'static {
    move |ui: &mut Ui, drag: &mut Drag, tx: &mut Tx| held(ui, drag, tx, DragFrom::Row, key)
}

/// The handler for a tile header's `on_drag`.
///
/// As a row's ([`row`]), with the same slop, guards and overlay, and the same `Ui::drag`, but
/// it moves the tile along the row: over the tile row the target is the nearest edge, unless
/// the tile would stay where it is there ([`move_target`]); elsewhere there is none, and the
/// sidebar is never asked. The release over an edge moves the tile there, focuses it and puts
/// the keyboard in its input box; over nothing, or cancelled, it does nothing. With one tile
/// in the row, or while the header's name is edited, no drag starts.
///
/// It sets no press to swallow: the press Rune runs after a release inside the header (when
/// the drag came back to where it started, so nothing moved) only focuses the tile, as a click.
pub fn header(key: SessionKey) -> impl Fn(&mut Ui, &mut Drag, &mut Tx) + 'static {
    move |ui: &mut Ui, drag: &mut Drag, tx: &mut Tx| held(ui, drag, tx, DragFrom::Header, key)
}

/// One phase of the drag of `key`'s row or header, as `from` says.
fn held(ui: &mut Ui, drag: &mut Drag, tx: &mut Tx, from: DragFrom, key: SessionKey) {
    tx.instant();
    match drag.phase {
        // Every pointer press on a row or a header comes here first: it is no press after a
        // release (`glue::poll` has cleared that flag by now in any case), and a drag whose
        // release was lost ends.
        Phase::Down => {
            ui.swallow_row_press = false;
            ui.drag = None;
        }
        Phase::Move => {
            let ours = ui
                .drag
                .as_ref()
                .is_some_and(|d| d.key == key && d.from == from);
            if drag.data != STARTED {
                if drag.dx.hypot(drag.dy) <= SLOP || !can_start(ui, from, key) {
                    return;
                }
                drag.data = STARTED;
            } else if !ours {
                return;
            }
            if menu_open(ui) {
                ui.drag = None;
                return;
            }
            let at = drag.window_point();
            let over = match from {
                DragFrom::Row => {
                    // Outside the tile row, the sidebar's line under the pointer says
                    // (`answer`).
                    if !in_row(ui, at) {
                        ask(tx, key, at);
                    }
                    target_in_tiles(ui, key, at)
                }
                DragFrom::Header => move_target(ui, key, at),
            };
            ui.drag = Some(RowDrag {
                key,
                at,
                over,
                heard: true,
                missed: 0,
                from,
            });
        }
        Phase::Up => {
            let held = ui.drag.take().filter(|d| d.key == key && d.from == from);
            if drag.data != STARTED || drag.cancelled() {
                return;
            }
            match from {
                DragFrom::Row => drop_row(ui, tx, key, held),
                DragFrom::Header => {
                    if let Some(RowDrag {
                        over: Some(DropTarget::Edge(edge)),
                        ..
                    }) = held
                    {
                        glue::move_tile(ui, tx, key, edge, true);
                    }
                }
            }
        }
    }
}

/// Whether a press on `key`'s row or header that passed the slop starts a drag: a row's always;
/// a header's while there is another tile to move past and its name is not being edited.
fn can_start(ui: &Ui, from: DragFrom, key: SessionKey) -> bool {
    match from {
        DragFrom::Row => true,
        DragFrom::Header => {
            ui.app.tiles.contains(key)
                && ui.app.tiles.list().len() > 1
                && ui.renaming != Some(Renaming::Header(key))
        }
    }
}

/// The release of `key`'s row, past the slop and not cancelled, `held` being its drag as it was
/// then (none when it ended before).
fn drop_row(ui: &mut Ui, tx: &mut Tx, key: SessionKey, held: Option<RowDrag>) {
    ui.swallow_row_press = true;
    let Some(held) = held else {
        return;
    };
    let place = match held.over {
        Some(DropTarget::Edge(i)) => Place::At(i),
        Some(DropTarget::Tile(k)) => Place::Tile(k),
        Some(DropTarget::Group(id)) => {
            let effects = ui.app.move_session(key, &id);
            glue::run(ui, effects);
            return;
        }
        // A session in a tile, dropped on the tile row: its tile takes the focus.
        None if in_row(ui, held.at) && ui.app.tiles.contains(key) => Place::Focused,
        None => return,
    };
    glue::show(ui, tx, key, place, true);
}

/// Asks Rune which line of the sidebar's list is under `at`, for the drag of `key`'s row. The
/// answer comes after the handler that asks ([`answer`]).
fn ask(tx: &mut Tx, key: SessionKey, at: [f32; 2]) {
    let answered = move |ui: &mut Ui, picked: Option<Picked>, _: &mut Tx| {
        answer(ui, key, at, picked.as_ref().map(Picked::path));
    };
    tx.element_at_in(LIST, at[0], at[1], answered);
}

/// What Rune found under `at` in the sidebar's list, at `path`, for the drag of `key`'s row: a
/// drop there moves the session to the line's group, unless that is the group it is in.
///
/// The answer is dropped when it is no longer the drag's: the drag ended meanwhile, is another
/// row's, or the pointer has moved on from `at`, so that a slow answer never marks a group the
/// pointer has left.
fn answer(ui: &mut Ui, key: SessionKey, at: [f32; 2], path: Option<&str>) {
    let ours = |d: &RowDrag| d.key == key && d.from == DragFrom::Row && d.at == at;
    if !ui.drag.as_ref().is_some_and(ours) {
        return;
    }
    let Some(entry) = ui.app.sessions.get(key) else {
        return;
    };
    let own = ui.app.groups.group_of(entry);
    let over = path
        .and_then(|path| group_of_path(ui, path))
        .filter(|id| *id != own)
        .map(DropTarget::Group);
    if let Some(drag) = &mut ui.drag {
        drag.over = over;
    }
}

/// The group of the sidebar line whose element path is `path`, when it is a header, a row
/// or a "Show older" line of a listed group.
///
/// The lines are the children of the list, keyed as `sidebar::list` keys them; a point inside
/// one gives the path of what is drawn there, under the line.
pub fn group_of_path(ui: &Ui, path: &str) -> Option<GroupId> {
    let mut segments = path.split('/');
    segments.find(|segment| *segment == LIST)?;
    // Unkeyed elements between (`#n`) are not lines, as Rune's own key paths skip them.
    let line = segments.find(|segment| !segment.starts_with('#'))?;
    let model = &ui.app.sidebar;
    let listed = |id: GroupId| model.sections.iter().any(|s| s.id == id).then_some(id);
    if let Some(number) = line.strip_prefix("mr-row-") {
        let entry = ui.app.sessions.get(SessionKey(number.parse().ok()?))?;
        return listed(ui.app.groups.group_of(entry));
    }
    let text = line
        .strip_prefix("mr-group-")
        .or_else(|| line.strip_prefix("mr-older-"))?;
    model
        .sections
        .iter()
        .find(|section| section.id.key_text() == text)
        .map(|section| section.id.clone())
}

/// What a drop at `at` would do in the tile row, when `at` is inside it: nothing for a session
/// already in a tile; with room for another tile, one at the nearest edge; else the session
/// in the tile under the pointer takes its place.
pub fn target_in_tiles(ui: &Ui, key: SessionKey, at: [f32; 2]) -> Option<DropTarget> {
    let [x, _, width, _] = ui.row_rect.filter(|_| in_row(ui, at))?;
    let tiles = &ui.app.tiles;
    if tiles.contains(key) {
        return None;
    }
    if ui.app.room_for_tile() {
        return Some(DropTarget::Edge(tiles.edge_at(width, at[0] - x)));
    }
    let index = tiles.tile_at(width, at[0] - x)?;
    Some(DropTarget::Tile(tiles.list()[index].key))
}

/// Where a drop at `at` would move `key`'s tile, when `at` is inside the tile row: the nearest
/// edge, unless the tile would stay where it is (`Tiles::moved_index`). Whether another tile
/// would fit plays no part: no tile is added.
pub fn move_target(ui: &Ui, key: SessionKey, at: [f32; 2]) -> Option<DropTarget> {
    let [x, _, width, _] = ui.row_rect.filter(|_| in_row(ui, at))?;
    let edge = ui.app.tiles.edge_at(width, at[0] - x);
    ui.app.tiles.moved_index(key, edge)?;
    Some(DropTarget::Edge(edge))
}

/// Whether `at` is inside the tile row as last laid out.
fn in_row(ui: &Ui, at: [f32; 2]) -> bool {
    ui.row_rect
        .is_some_and(|[x, y, w, h]| (x..x + w).contains(&at[0]) && (y..y + h).contains(&at[1]))
}

/// A menu is open: it takes the pointer, and a drag under it ends.
fn menu_open(ui: &Ui) -> bool {
    ui.side_menu.is_some() || ui.folder_menu.is_some()
}

/// The title that follows the pointer, and the watcher for a release that never came.
///
/// While a row or a header is dragged, `layer` gets its session's title in a small raised chip
/// right of and below the pointer; for a drop at an edge (a tile opened there, or the dragged
/// one moved there), a line in the accent colour at that edge of the tile row, its full height
/// (a tile the drop would take shows its outline, `tiles::outline`). Both are inert: they take
/// no press and no hover from what is under them.
///
/// Last, the watcher: an element over the whole window that has only `on_pointer_move`, which
/// Rune hands every move of a held pointer to, under the rest of the window so it takes no
/// pointer input from it. A drag whose release was lost (its row unmounted) hears no moves
/// any more; the watcher ends it on the second move the drag does not hear ([`watch`]).
pub fn overlay(ui: &Ui, kit: &Kit, layer: &mut Vec<El>) {
    let Some(drag) = ui.drag.as_ref().filter(|_| !menu_open(ui)) else {
        return;
    };
    let c = &kit.c;
    let title = ui
        .app
        .sessions
        .get(drag.key)
        .and_then(|entry| entry.session.title.clone())
        .unwrap_or_else(|| NEW_SESSION.to_owned());
    let chip = row_el()
        .key(CHIP)
        .absolute(drag.at[0] + CHIP_OFFSET[0], drag.at[1] + CHIP_OFFSET[1])
        .h(24.0)
        .max_w(CHIP_MAX_WIDTH)
        .px(8.0)
        .center_y()
        .radius(kit.radius.md)
        .fill(c.layer_01)
        .border(0.5, c.border_base)
        .child(
            text(title)
                .font(kit.ty.small_medium)
                .color(c.text_base)
                .truncate()
                .shrink(),
        );
    layer.push(
        kit.elevate(chip, &kit.elevation.raised)
            .transition(Transition::none())
            .inert(),
    );
    if let (Some(DropTarget::Edge(i)), Some([x, y, width, height])) = (&drag.over, ui.row_rect) {
        // `over` may have been worked out over a row with more tiles than it has now.
        let edge: f32 = ui.app.tiles.widths(width).iter().take(*i).sum();
        let left = (x + edge - EDGE_WIDTH / 2.0).clamp(x, x + width - EDGE_WIDTH);
        layer.push(
            el().key(EDGE)
                .absolute(left, y)
                .size(EDGE_WIDTH, height)
                .fill(c.bg_accent)
                .transition(Transition::none())
                .inert(),
        );
    }
    layer.push(
        el().key(WATCH)
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .paint_order(-1)
            .transition(Transition::none())
            .on_pointer_move(|ui: &mut Ui, _: PointerPress| watch(ui)),
    );
}

/// The watcher's part of the kit's guard (`rune-kit/src/grip.rs`), which is private to the
/// kit: each move counts as missed until the drag's own move says it heard it, and the second
/// move missed in a row ends the drag.
///
/// Rune queues the watcher's move before the drag's own (`move_pointer`, in
/// `crates/rune/src/runtime/input/pointer.rs` in the Rune checkout), so a live drag has always
/// heard the move before this one, and the move in hand at most is still unheard.
fn watch(ui: &mut Ui) {
    let Some(drag) = &mut ui.drag else {
        return;
    };
    if drag.heard {
        drag.missed = 0;
    }
    drag.missed += 1;
    drag.heard = false;
    if drag.missed >= 2 {
        ui.drag = None;
    }
}

#[cfg(test)]
mod tests {
    use mr_agent::{SessionCommand, SessionEvent, SessionId, StoredSession};
    use mr_core::{FromBackend, GroupId, SessionKey};
    use rune::{Key, KeyEvent, Mods, Runtime};
    use rune_kit::shell::ROW_HEIGHT;

    use super::super::{conversation, sidebar, tiles};
    use crate::glue::{self, DragFrom, DropTarget, MenuTarget, Renaming, Ui};
    use crate::testkit::{
        self, CONTACT, FINISHED, GARDEN, NEEDS_YOU, WEBSITE, WORKING, box_focused, group_named,
        group_of, row_key, tile, tile_keys, titled,
    };
    use crate::theme;

    /// The window most tests here open: 1,800 by 900, the sidebar open.
    const SIZE: (f32, f32) = (1800.0, 900.0);

    /// The key of the title by the pointer, of the line where a tile would open.
    const CHIP: &str = "mr-drag-chip";
    const EDGE: &str = "mr-drop-edge";

    /// Three sessions in a window of `size`: `a` in the first tile and `b` in a second to its
    /// side, which has the focus; `c` only in the sidebar.
    fn three(size: (f32, f32)) -> (Runtime<Ui>, SessionKey, SessionKey, SessionKey) {
        let mut rt = testkit::window_sized("empty", size);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        let c = testkit::add_session(&mut rt, "/home/alex/c");
        testkit::show(&mut rt, a);
        testkit::show_side(&mut rt, b);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        assert_eq!(testkit::focused(&rt), Some(b));
        (rt, a, b, c)
    }

    /// The tile row as last laid out: x, y, width, height.
    fn row_rect(rt: &Runtime<Ui>) -> [f32; 4] {
        rt.state.row_rect.expect("the tile row is laid out")
    }

    /// The window x of edge `i` of the tile row: 0 is its left end.
    fn edge_x(rt: &Runtime<Ui>, i: usize) -> f32 {
        let [x, _, w, _] = row_rect(rt);
        x + rt.state.app.tiles.widths(w)[..i].iter().sum::<f32>()
    }

    /// Half way down the tile row.
    fn mid_y(rt: &Runtime<Ui>) -> f32 {
        let [_, y, _, h] = row_rect(rt);
        y + h / 2.0
    }

    /// Where the drag is said to be going.
    fn over(rt: &Runtime<Ui>) -> Option<DropTarget> {
        rt.state.drag.as_ref().and_then(|drag| drag.over.clone())
    }

    /// Nothing of a drag is left: none in state, no chip, no line, no watcher.
    fn nothing_left(rt: &Runtime<Ui>) {
        assert_eq!(rt.state.drag, None, "no drag in state");
        assert!(!testkit::drawn(rt, CHIP), "no chip");
        assert!(!testkit::drawn(rt, EDGE), "no line");
        assert!(!testkit::drawn(rt, "mr-drag-watch"), "no watcher");
    }

    /// The groups and the sessions in each, in order.
    fn arrangement(rt: &Runtime<Ui>) -> Vec<Vec<SessionKey>> {
        let model = &rt.state.app.sidebar;
        model
            .sections
            .iter()
            .map(|section| section.rows.iter().map(|row| row.key).collect())
            .collect()
    }

    #[test]
    fn a_small_movement_is_still_a_click() {
        let (mut rt, a, _, c) = three(SIZE);
        let (x, y) = testkit::find(&rt, &row_key(c)).unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        testkit::step(&mut rt);
        rt.pointer_move(x + 3.0, y);
        testkit::step(&mut rt);
        assert_eq!(rt.state.drag, None, "no drag within the slop");
        assert!(!testkit::drawn(&rt, CHIP));
        rt.pointer_up(x + 3.0, y);
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c], "shown in the focused tile");
        assert_eq!(testkit::focused(&rt), Some(c));
        assert!(box_focused(&rt, c));
    }

    #[test]
    fn a_row_dropped_between_two_tiles_opens_a_tile_there() {
        let (mut rt, a, b, c) = three(SIZE);
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::drag_row(&mut rt, c, to);
        assert_eq!(tile_keys(&rt), vec![a, c, b]);
        assert_eq!(testkit::focused(&rt), Some(c));
        assert!(box_focused(&rt, c), "the new tile's box has the keyboard");
        nothing_left(&rt);
    }

    #[test]
    fn a_row_dropped_at_the_right_end_opens_the_last_tile() {
        let (mut rt, a, b, c) = three(SIZE);
        let [x, _, w, _] = row_rect(&rt);
        let to = (x + w - 20.0, mid_y(&rt));
        testkit::drag_row(&mut rt, c, to);
        assert_eq!(tile_keys(&rt), vec![a, b, c]);
        assert_eq!(testkit::focused(&rt), Some(c));
        assert!(box_focused(&rt, c));
        nothing_left(&rt);
    }

    #[test]
    fn the_line_shows_where_the_tile_will_open() {
        let (mut rt, _, _, c) = three(SIZE);
        let edge = edge_x(&rt, 1);
        let to = (edge - 30.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        assert_eq!(over(&rt), Some(DropTarget::Edge(1)));
        let [lx, ly, lw, lh] = testkit::rect(&rt, EDGE).expect("the line is drawn");
        let [_, y, _, h] = row_rect(&rt);
        assert!((lx + lw / 2.0 - edge).abs() <= 1.0, "at the edge: {lx}");
        assert_eq!(lw, 2.0);
        assert_eq!((ly, lh), (y, h), "the row's full height");
        testkit::release(&mut rt);
        nothing_left(&rt);
    }

    #[test]
    fn without_room_the_drop_swaps_the_tile_under_the_pointer() {
        let (mut rt, a, b, c) = three((1300.0, 900.0));
        assert!(!rt.state.app.room_for_tile());
        rt.update(|ui, _| glue::focus_tile(ui, a, true));
        testkit::step(&mut rt);
        let to = ((edge_x(&rt, 1) + edge_x(&rt, 2)) / 2.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        assert_eq!(over(&rt), Some(DropTarget::Tile(b)));
        let kit = theme::kit(true);
        assert_eq!(tiles::outline(&rt.state, b, kit), Some(kit.c.bg_accent));
        assert!(testkit::element(&rt, &tile(b, "mr-tile-outline")).is_some());
        assert_eq!(
            tiles::outline(&rt.state, a, kit),
            None,
            "the target is the one tile outlined"
        );
        assert!(!testkit::drawn(&rt, EDGE), "no line");
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c]);
        assert_eq!(testkit::focused(&rt), Some(c));
        assert!(box_focused(&rt, c));
        nothing_left(&rt);
    }

    #[test]
    fn a_drop_on_the_sidebar_does_nothing() {
        // Outside the groups: a drop on one moves the session (`a_row_dropped_on_a_header_..`).
        let (mut rt, a, b, c) = three(SIZE);
        let before = arrangement(&rt);
        let new_group = testkit::find(&rt, "mr-new-group").unwrap();
        testkit::drag_row(&mut rt, c, new_group);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        assert_eq!(testkit::focused(&rt), Some(b));
        assert_eq!(arrangement(&rt), before);
        nothing_left(&rt);
    }

    #[test]
    fn a_session_already_in_a_tile_is_only_focused() {
        let (mut rt, a, b, _) = three(SIZE);
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, a, to);
        assert!(rt.state.drag.is_some());
        assert_eq!(over(&rt), None);
        assert!(!testkit::drawn(&rt, EDGE), "no line");
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        assert_eq!(testkit::focused(&rt), Some(a));
        assert!(box_focused(&rt, a));
        nothing_left(&rt);
    }

    #[test]
    fn the_drop_is_not_also_a_click() {
        let (mut rt, a, b, c) = three(SIZE);
        let home = testkit::find(&rt, &row_key(c)).unwrap();
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        testkit::move_held(&mut rt, home);
        assert_eq!(over(&rt), None);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b], "nothing shown anew");
        assert_eq!(testkit::focused(&rt), Some(b));
        nothing_left(&rt);
        assert!(!rt.state.swallow_row_press, "the press was swallowed");
        testkit::click(&mut rt, &row_key(c));
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c], "the next click works");
        assert_eq!(testkit::focused(&rt), Some(c));
    }

    #[test]
    fn escape_cancels_the_drag() {
        let (mut rt, a, b, c) = three(SIZE);
        // A turn runs in the focused tile: Escape ends the drag and nothing else.
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        let working = |rt: &Runtime<Ui>| {
            let entry = rt.state.app.sessions.get(b).unwrap();
            entry.session.phase == mr_core::Phase::Working
        };
        assert!(working(&rt));
        let sent = testkit::sent(&rt).len();
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        assert!(testkit::drawn(&rt, CHIP) && testkit::drawn(&rt, EDGE));
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        testkit::step(&mut rt);
        nothing_left(&rt);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b], "nothing opens");
        assert_eq!(testkit::focused(&rt), Some(b));
        nothing_left(&rt);
        assert!(working(&rt), "the turn goes on");
        assert!(
            !testkit::sent(&rt)[sent..].contains(&SessionCommand::Stop),
            "no Stop was sent"
        );
    }

    #[test]
    fn a_drop_line_past_the_last_tile_does_not_panic() {
        let (mut rt, a, b, c) = three(SIZE);
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        assert_eq!(over(&rt), Some(DropTarget::Edge(1)));
        // An edge worked out over a row that had more tiles than it has now.
        rt.update(|ui, _| ui.drag.as_mut().unwrap().over = Some(DropTarget::Edge(5)));
        // The line glides from where it was to where it goes.
        for _ in 0..20 {
            testkit::step(&mut rt);
        }
        assert_eq!(over(&rt), Some(DropTarget::Edge(5)));
        let [x, _, w, _] = row_rect(&rt);
        let [lx, _, lw, _] = testkit::rect(&rt, EDGE).expect("the line is drawn");
        assert!((lx + lw - (x + w)).abs() < 0.5, "at the row's right end");
        assert_eq!(tile_keys(&rt), vec![a, b]);
    }

    #[test]
    fn a_drag_whose_row_scrolled_away_ends_by_itself() {
        let mut rt = testkit::window_sized("empty", SIZE);
        testkit::long_list(&mut rt, 2_000);
        let tiled = tile_keys(&rt);
        let model = &rt.state.app.sidebar;
        let first = model
            .lines
            .iter()
            .find_map(|line| match *line {
                mr_core::Line::Row(s, r) => Some(model.sections[s].rows[r].key),
                _ => None,
            })
            .expect("a row");
        let list = testkit::find(&rt, &row_key(first)).expect("the first row is drawn");
        let [x, _, w, _] = row_rect(&rt);
        let to = (x + w / 2.0, mid_y(&rt));
        testkit::hold_row(&mut rt, first, to);
        assert!(rt.state.drag.is_some());
        rt.wheel(list.0, list.1, -20_000.0);
        testkit::step(&mut rt);
        assert!(!testkit::drawn(&rt, &row_key(first)), "the row is gone");
        let (px, py) = rt.pointer().unwrap();
        for dx in [10.0, 20.0] {
            rt.pointer_move(px + dx, py);
            testkit::step(&mut rt);
        }
        nothing_left(&rt);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), tiled, "the release does nothing");
        nothing_left(&rt);
    }

    #[test]
    fn the_title_follows_the_pointer() {
        let (mut rt, _, _, c) = three(SIZE);
        let title = rt.state.app.sessions.get(c).unwrap().session.title.clone();
        let title = title.unwrap_or_else(|| "New session".into());
        let near = |rt: &Runtime<Ui>| {
            let [x, y, _, _] = testkit::rect(rt, CHIP).expect("the chip is drawn");
            let (px, py) = rt.pointer().unwrap();
            ((x - px).powi(2) + (y - py).powi(2)).sqrt()
        };
        testkit::hold_row(&mut rt, c, (900.0, 300.0));
        assert!(near(&rt) <= 40.0, "{} points away", near(&rt));
        let shown = rt
            .elements()
            .into_iter()
            .any(|e| e.path.contains(&format!("/{CHIP}/")) && e.text.as_deref() == Some(&title));
        assert!(shown, "the chip shows {title:?}");
        testkit::move_held(&mut rt, (1400.0, 700.0));
        assert!(near(&rt) <= 40.0, "{} points away", near(&rt));
        testkit::release(&mut rt);
        nothing_left(&rt);
    }

    #[test]
    fn a_release_outside_the_tiles_and_the_sidebar_does_nothing() {
        let (mut rt, a, b, c) = three(SIZE);
        let before = arrangement(&rt);
        let footer = testkit::find(&rt, "mr-usage-session").expect("the usage footer");
        testkit::hold_row(&mut rt, c, footer);
        assert_eq!(over(&rt), None);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b], "no tile opens");
        assert_eq!(testkit::focused(&rt), Some(b));
        assert_eq!(arrangement(&rt), before, "nothing moves");
        nothing_left(&rt);
    }

    #[test]
    fn a_menu_opening_ends_the_drag() {
        let (mut rt, a, b, c) = three(SIZE);
        let home = testkit::find(&rt, &row_key(c)).unwrap();
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        testkit::move_held(&mut rt, home);
        rt.secondary_down(home.0, home.1);
        rt.secondary_up(home.0, home.1);
        testkit::step(&mut rt);
        let menu = rt.state.side_menu.as_ref().map(|menu| menu.target.clone());
        assert_eq!(menu, Some(MenuTarget::Row(c)), "its menu is open");
        nothing_left(&rt);
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::move_held(&mut rt, to);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b], "the release does nothing");
        assert_eq!(testkit::focused(&rt), Some(b));
        nothing_left(&rt);
    }

    #[test]
    fn a_drag_whose_session_is_gone_ends() {
        let (mut rt, a, b, c) = three(SIZE);
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        assert!(rt.state.drag.is_some());
        // As when a message from the backend takes it away: the glue runs, and the window
        // redraws.
        rt.update(|ui, _| {
            ui.app.sessions.remove(c);
            glue::run(ui, Vec::new());
        });
        testkit::step(&mut rt);
        nothing_left(&rt);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        assert_eq!(testkit::focused(&rt), Some(b));
    }

    #[test]
    fn a_drop_into_the_tiles_leaves_no_press_to_swallow() {
        let (mut rt, a, b, c) = three(SIZE);
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::drag_row(&mut rt, c, to);
        assert_eq!(tile_keys(&rt), vec![a, c, b]);
        assert!(
            !rt.state.swallow_row_press,
            "no press follows a release outside the row"
        );
    }

    #[test]
    fn a_release_outside_the_window_does_nothing() {
        let (mut rt, a, b, c) = three(SIZE);
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        assert_eq!(over(&rt), Some(DropTarget::Edge(1)));
        let y = mid_y(&rt);
        rt.pointer_move(-50.0, y);
        testkit::step(&mut rt);
        rt.pointer_up(-50.0, y);
        testkit::step(&mut rt);
        nothing_left(&rt);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        assert_eq!(testkit::focused(&rt), Some(b));
    }

    #[test]
    fn the_windows_keys_wait_while_a_row_is_dragged() {
        let (mut rt, a, b, c) = three(SIZE);
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        testkit::press_with(&mut rt, Key::Command('n'), testkit::ctrl());
        testkit::press_with(&mut rt, Key::Command('n'), testkit::ctrl_shift());
        assert_eq!(rt.state.folder_menu, None, "no folder menu");
        assert!(rt.resolve_file_dialog(None).is_err(), "no folder picker");
        testkit::press(&mut rt, Key::Function(6));
        testkit::press_with(
            &mut rt,
            Key::Function(6),
            Mods {
                shift: true,
                ..Mods::default()
            },
        );
        testkit::press_with(&mut rt, Key::Command('w'), testkit::ctrl());
        assert_eq!(tile_keys(&rt), vec![a, b], "no tile closed");
        assert_eq!(testkit::focused(&rt), Some(b), "the focus stays");
        assert_eq!(over(&rt), Some(DropTarget::Edge(1)), "the drag goes on");
        assert!(testkit::drawn(&rt, CHIP) && testkit::drawn(&rt, EDGE));
        testkit::move_held(&mut rt, (to.0 + 5.0, to.1));
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c, b], "and still drops");
        assert_eq!(testkit::focused(&rt), Some(c));
        nothing_left(&rt);
    }

    #[test]
    fn escape_ends_a_drag_whose_row_scrolled_away() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        testkit::long_list(&mut rt, 2_000);
        testkit::emit_to(&mut rt, a, SessionEvent::TurnStarted);
        let working = |rt: &Runtime<Ui>| {
            let entry = rt.state.app.sessions.get(a).unwrap();
            entry.session.phase == mr_core::Phase::Working
        };
        assert!(working(&rt));
        let sent = testkit::sent(&rt).len();
        let model = &rt.state.app.sidebar;
        let archived = model
            .lines
            .iter()
            .find_map(|line| match *line {
                mr_core::Line::Row(s, r) if model.sections[s].rows[r].key != a => {
                    Some(model.sections[s].rows[r].key)
                }
                _ => None,
            })
            .expect("an archived row");
        let list = testkit::find(&rt, &row_key(archived)).expect("its row is drawn");
        let [x, _, w, _] = row_rect(&rt);
        let to = (x + w / 2.0, mid_y(&rt));
        testkit::hold_row(&mut rt, archived, to);
        assert!(rt.state.drag.is_some());
        rt.wheel(list.0, list.1, -20_000.0);
        testkit::step(&mut rt);
        assert!(!testkit::drawn(&rt, &row_key(archived)), "the row is gone");
        testkit::press(&mut rt, Key::Escape);
        nothing_left(&rt);
        assert!(working(&rt), "the turn was not stopped");
        assert_eq!(testkit::sent(&rt).len(), sent, "nothing was sent");
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a]);
        nothing_left(&rt);
    }

    #[test]
    fn a_menu_open_in_state_ends_a_drag() {
        let (mut rt, _, _, c) = three(SIZE);
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        for open in [true, false] {
            testkit::hold_row(&mut rt, c, to);
            assert!(rt.state.drag.is_some());
            rt.update(|ui, _| {
                if open {
                    ui.folder_menu = Some(None);
                } else {
                    ui.side_menu = Some(glue::SideMenu {
                        target: MenuTarget::Row(c),
                        at: rune::PointerPress { x: 0.0, y: 0.0 },
                        sub_open: false,
                    });
                }
                glue::run(ui, Vec::new());
            });
            testkit::step(&mut rt);
            nothing_left(&rt);
            testkit::release(&mut rt);
            rt.update(|ui, _| {
                ui.folder_menu = None;
                ui.side_menu = None;
            });
            testkit::step(&mut rt);
            nothing_left(&rt);
        }
    }

    #[test]
    fn a_tile_that_needs_the_user_shows_it_is_the_drop_target() {
        let (mut rt, a, b, c) = three((1300.0, 900.0));
        let request = mr_agent::UserRequest {
            id: mr_agent::RequestId("r1".into()),
            kind: mr_agent::UserRequestKind::Permission {
                tool: "Bash".into(),
                detail: mr_agent::ToolDetail::Command {
                    command: "touch a.txt".into(),
                },
                always_allow_label: None,
            },
        };
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        testkit::emit_to(&mut rt, b, SessionEvent::NeedsUser(request));
        let kit = theme::kit(true);
        let amber = sidebar::mark_color(mr_core::Mark::NeedsYou, kit);
        assert_eq!(tiles::outline(&rt.state, b, kit), amber);
        let to = ((edge_x(&rt, 1) + edge_x(&rt, 2)) / 2.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        assert_eq!(over(&rt), Some(DropTarget::Tile(b)));
        assert_eq!(tiles::outline(&rt.state, b, kit), Some(kit.c.bg_accent));
        assert!(testkit::element(&rt, &tile(b, "mr-tile-outline")).is_some());
        assert_eq!(tiles::outline(&rt.state, a, kit), None);
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        testkit::step(&mut rt);
        nothing_left(&rt);
        assert_eq!(tiles::outline(&rt.state, b, kit), amber, "amber comes back");
        testkit::release(&mut rt);
    }

    #[test]
    fn the_focus_outline_gives_way_to_the_drop_line() {
        let (mut rt, a, b, c) = three(SIZE);
        let kit = theme::kit(true);
        let outlined = |rt: &Runtime<Ui>, key: SessionKey| {
            testkit::element(rt, &tile(key, "mr-tile-outline")).is_some()
        };
        assert!(outlined(&rt, b), "the focused tile is outlined");
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        assert_eq!(over(&rt), Some(DropTarget::Edge(1)));
        assert!(testkit::drawn(&rt, EDGE), "the line is drawn");
        assert_eq!(tiles::outline(&rt.state, b, kit), None);
        assert!(!outlined(&rt, b), "the focused tile's outline gave way");
        assert!(!outlined(&rt, a));
        // Back over the sidebar: no target, and the outline is back.
        let (x, y) = testkit::find(&rt, &row_key(a)).unwrap();
        testkit::move_held(&mut rt, (x, y + 200.0));
        assert_eq!(over(&rt), None);
        assert!(!testkit::drawn(&rt, EDGE));
        assert_eq!(tiles::outline(&rt.state, b, kit), Some(kit.c.bg_accent));
        assert!(outlined(&rt, b), "the outline is back");
        testkit::move_held(&mut rt, to);
        assert_eq!(over(&rt), Some(DropTarget::Edge(1)));
        testkit::release(&mut rt);
        nothing_left(&rt);
        assert_eq!(tile_keys(&rt), vec![a, c, b]);
        assert_eq!(testkit::focused(&rt), Some(c));
        assert_eq!(tiles::outline(&rt.state, c, kit), Some(kit.c.bg_accent));
        assert!(
            outlined(&rt, c),
            "the new tile is outlined as the focused one"
        );
        assert!(!outlined(&rt, b));
    }

    /// The key of the element that has the keyboard focus.
    fn focus(rt: &Runtime<Ui>) -> Option<String> {
        rt.elements()
            .into_iter()
            .find(|e| e.focused && !e.exiting)
            .and_then(|e| e.key)
    }

    #[test]
    fn a_rows_keys_wait_while_a_row_is_dragged() {
        // The focus lands on the selected row, b's: Return and Space would move it into b's
        // input box, and the arrows, Home, End and the pages would show another session.
        let (mut rt, a, b, c) = three(SIZE);
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        rt.update(|_, tx| tx.focus(row_key(b), true));
        testkit::step(&mut rt);
        assert_eq!(focus(&rt), Some(row_key(b)));
        let keys = [
            Key::Enter,
            Key::Text(" ".into()),
            Key::Up,
            Key::Down,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
        ];
        for key in keys {
            testkit::press(&mut rt, key.clone());
            assert_eq!(tile_keys(&rt), vec![a, b], "{key:?} shows nothing");
            assert_eq!(
                testkit::focused(&rt),
                Some(b),
                "{key:?} moves no tile focus"
            );
            assert_eq!(
                focus(&rt),
                Some(row_key(b)),
                "{key:?} leaves the row focused"
            );
        }
        assert_eq!(over(&rt), Some(DropTarget::Edge(1)), "the drag goes on");
        testkit::move_held(&mut rt, (to.0 + 5.0, to.1));
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c, b], "and still drops");
        nothing_left(&rt);
    }

    /// A listing of one stored session, `id` in `folder`, last active `days` from now, as the
    /// backend sends one; the window draws it.
    fn listed(rt: &mut Runtime<Ui>, id: &str, folder: &str, days: i64) {
        let session = StoredSession {
            id: SessionId(id.into()),
            folder: folder.into(),
            title: format!("Listed {id}"),
            last_active_unix: rt.state.app.now_unix + days * 86_400,
        };
        rt.update(|ui, _| {
            let result = Ok(vec![session]);
            let since_unix = Some(0);
            let effects = ui
                .app
                .receive(FromBackend::Sessions { since_unix, result }, 0.0);
            glue::run(ui, effects);
        });
        testkit::step(rt);
    }

    #[test]
    fn a_row_that_moved_under_the_pointer_is_not_clicked_by_the_release() {
        // The row leaves a still pointer: Rune still holds the pointer inside it and runs its
        // press after the release.
        let (mut rt, a, b, c) = three(SIZE);
        let home = testkit::find(&rt, &row_key(c)).unwrap();
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        testkit::hold_row(&mut rt, c, to);
        testkit::move_held(&mut rt, home);
        listed(&mut rt, "newer", "/home/alex/newer", 1);
        let [_, y, _, h] = testkit::rect(&rt, &row_key(c)).unwrap();
        assert!(!(y..y + h).contains(&home.1), "the row moved down");
        // A host can send the pointer again where it already is: Rune keeps what it holds then.
        rt.pointer_move(home.0, home.1);
        testkit::step(&mut rt);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b], "nothing shown");
        assert_eq!(testkit::focused(&rt), Some(b));
        assert!(!rt.state.swallow_row_press, "the press was swallowed");
        nothing_left(&rt);

        // The row comes under a still pointer, two lines below it, where nothing is listed:
        // Rune still holds the pointer outside it and runs no press, so none is left to swallow.
        let (mut rt, a, b, c) = three(SIZE);
        let home = testkit::find(&rt, &row_key(c)).unwrap();
        let below = (home.0, home.1 + 2.0 * ROW_HEIGHT);
        testkit::hold_row(&mut rt, c, to);
        testkit::move_held(&mut rt, below);
        assert_eq!(over(&rt), None);
        listed(&mut rt, "newer", "/home/alex/newer", 1);
        let [_, y, _, h] = testkit::rect(&rt, &row_key(c)).unwrap();
        assert!(
            (y..y + h).contains(&below.1),
            "the row moved under the pointer"
        );
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b], "nothing shown");
        assert_eq!(testkit::focused(&rt), Some(b));
        assert!(!rt.state.swallow_row_press, "no press is left to swallow");
        nothing_left(&rt);
    }

    #[test]
    fn dragging_rebuilds_no_conversation_row() {
        // With room the drag passes edges; without, it passes over tiles it would take.
        for size in [SIZE, (1300.0, 900.0)] {
            let (mut rt, a, b, c) = three(size);
            for key in [a, b] {
                testkit::emit_to(&mut rt, key, SessionEvent::TextDelta("A reply".into()));
                testkit::emit_to(&mut rt, key, SessionEvent::BlockFinished);
            }
            rt.settle_all();
            testkit::step(&mut rt);
            let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
            testkit::hold_row(&mut rt, c, to);
            let rows = conversation::all_row_builds();
            let lines = sidebar::line_builds();
            let [x, _, w, _] = row_rect(&rt);
            let to = (x + 40.0, mid_y(&rt));
            testkit::move_held(&mut rt, to);
            let target = if size == SIZE {
                DropTarget::Edge(0)
            } else {
                DropTarget::Tile(a)
            };
            assert_eq!(over(&rt), Some(target));
            let to = (x + w - 40.0, mid_y(&rt) + 100.0);
            testkit::move_held(&mut rt, to);
            testkit::move_held(&mut rt, (100.0, 200.0));
            assert!(rt.state.drag.is_some());
            assert_eq!(conversation::all_row_builds(), rows, "no conversation row");
            assert_eq!(sidebar::line_builds(), lines, "no sidebar line");
            testkit::release(&mut rt);
        }
    }

    /// The `sessions` scene at [`SIZE`]: rune_claude's three sessions, sketches folded, Side
    /// projects with the session in the one tile, website with one row and "Show older (4)",
    /// and "Older groups (2)".
    fn sessions() -> Runtime<Ui> {
        testkit::window_sized("sessions", SIZE)
    }

    fn website() -> GroupId {
        GroupId::Folder(WEBSITE.into())
    }

    /// The key of `id`'s header and of its "Show older" line.
    fn header(id: &GroupId) -> String {
        format!("mr-group-{}", id.key_text())
    }

    fn older(id: &GroupId) -> String {
        format!("mr-older-{}", id.key_text())
    }

    /// Whether `id` is listed folded.
    fn collapsed(rt: &Runtime<Ui>, id: &GroupId) -> bool {
        let sections = &rt.state.app.sidebar.sections;
        sections.iter().any(|s| s.id == *id && s.collapsed)
    }

    /// Something was written under the state key since it was removed.
    fn saved(rt: &Runtime<Ui>) -> bool {
        rt.state.prefs.contains(glue::STATE_KEY)
    }

    /// The centre of the element keyed `key`.
    fn at(rt: &Runtime<Ui>, key: &str) -> (f32, f32) {
        testkit::find(rt, key).unwrap_or_else(|| panic!("no element keyed {key:?}"))
    }

    /// Whether the header of `id` is drawn with the kit's selected fill.
    fn marked(rt: &Runtime<Ui>, id: &GroupId) -> bool {
        let key = header(id);
        let header = rt
            .elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(&key))
            .expect("the header is drawn");
        header.fill == theme::kit(true).c.pressed
    }

    /// The element path of what is drawn at the centre of `key` in the sidebar's list.
    fn path_at(rt: &Runtime<Ui>, key: &str) -> String {
        let (x, y) = at(rt, key);
        let picked = rt.element_at_in("mr-sidebar-rows", x, y).expect("a line");
        picked.path().to_owned()
    }

    #[test]
    fn a_row_dropped_on_a_header_moves_to_that_group() {
        let mut rt = sessions();
        let finished = titled(&rt, FINISHED);
        let tiles = tile_keys(&rt);
        let focused = testkit::focused(&rt);
        let to = at(&rt, &header(&website()));
        testkit::drag_row(&mut rt, finished, to);
        assert_eq!(group_of(&rt, finished), website());
        let sections = &rt.state.app.sidebar.sections;
        let listed = sections
            .iter()
            .find(|s| s.id == website())
            .is_some_and(|s| s.rows.iter().any(|row| row.key == finished));
        assert!(listed, "its row is listed under website");
        assert_eq!(tile_keys(&rt), tiles, "no tile opens");
        assert_eq!(testkit::focused(&rt), focused);
        nothing_left(&rt);
    }

    #[test]
    fn a_row_dropped_on_another_groups_row_moves_to_that_group() {
        let mut rt = sessions();
        let [finished, asking, contact] = [FINISHED, NEEDS_YOU, CONTACT].map(|t| titled(&rt, t));
        let own = group_of(&rt, finished);
        // Its "Show older" line is website's too.
        let to = at(&rt, &older(&website()));
        testkit::hold_row(&mut rt, finished, to);
        assert_eq!(over(&rt), Some(DropTarget::Group(website())));
        let to = at(&rt, &row_key(contact));
        testkit::move_held(&mut rt, to);
        assert_eq!(over(&rt), Some(DropTarget::Group(website())));
        testkit::release(&mut rt);
        assert_eq!(group_of(&rt, finished), website());
        assert_eq!(group_of(&rt, asking), own, "the others in its group stay");
        assert_eq!(group_of(&rt, contact), website());
        nothing_left(&rt);
    }

    #[test]
    fn a_row_dropped_in_its_own_group_stays() {
        let mut rt = sessions();
        let [finished, asking] = [FINISHED, NEEDS_YOU].map(|t| titled(&rt, t));
        let own = group_of(&rt, finished);
        let tiles = tile_keys(&rt);
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        for line in [row_key(asking), header(&own)] {
            let to = at(&rt, &line);
            testkit::hold_row(&mut rt, finished, to);
            assert_eq!(over(&rt), None, "over {line}");
            assert!(!marked(&rt, &own), "its own header is not marked");
            testkit::release(&mut rt);
            assert_eq!(group_of(&rt, finished), own);
            assert!(!collapsed(&rt, &own), "a drop is no press on the header");
            assert_eq!(tile_keys(&rt), tiles);
            assert!(!saved(&rt), "no save is written");
            nothing_left(&rt);
        }
    }

    #[test]
    fn the_header_under_the_pointer_is_marked() {
        let mut rt = sessions();
        let [finished, asking] = [FINISHED, NEEDS_YOU].map(|t| titled(&rt, t));
        let own = group_of(&rt, finished);
        let side = group_named(&rt, "Side projects");
        assert!(!marked(&rt, &website()));
        let to = at(&rt, &header(&website()));
        testkit::hold_row(&mut rt, finished, to);
        assert_eq!(over(&rt), Some(DropTarget::Group(website())));
        assert!(marked(&rt, &website()), "the kit's selected fill");
        assert!(!marked(&rt, &own) && !marked(&rt, &side), "no other header");
        let to = at(&rt, &row_key(asking));
        testkit::move_held(&mut rt, to);
        assert_eq!(over(&rt), None);
        assert!(
            !marked(&rt, &website()),
            "no longer once the pointer left it"
        );
        let to = at(&rt, &header(&side));
        testkit::move_held(&mut rt, to);
        assert!(marked(&rt, &side));
        let footer = at(&rt, "mr-usage-session");
        testkit::move_held(&mut rt, footer);
        assert_eq!(over(&rt), None);
        assert!(!marked(&rt, &side), "nor once it left the sidebar");
        testkit::release(&mut rt);
        assert_eq!(group_of(&rt, finished), own);
        nothing_left(&rt);
    }

    /// The colour of the accent mark on `id`'s header, while one is drawn. Rune shows tests no
    /// border colours, so, as for a tile's outline, the drawn mark is checked to be there, over
    /// the header's whole line, and the colour is the one the view worked out for it.
    fn header_mark(rt: &Runtime<Ui>, id: &GroupId) -> Option<rune::Color> {
        let key = header(id);
        let inside = format!("/{key}/");
        let elements = rt.elements();
        let line = elements
            .iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(&key))
            .expect("the header is drawn");
        let drawn = elements.iter().find(|e| {
            !e.exiting && e.key.as_deref() == Some("mr-group-target") && e.path.contains(&inside)
        });
        let colour = sidebar::target_mark(&rt.state, id, theme::kit(true));
        assert_eq!(drawn.is_some(), colour.is_some(), "drawn as worked out");
        if let Some(drawn) = drawn {
            assert_eq!(drawn.rect, line.rect, "over the header's whole line");
        }
        colour
    }

    #[test]
    fn the_target_header_is_marked_in_the_accent() {
        let mut rt = sessions();
        let [finished, asking, contact] = [FINISHED, NEEDS_YOU, CONTACT].map(|t| titled(&rt, t));
        let own = group_of(&rt, finished);
        let side = group_named(&rt, "Side projects");
        let accent = theme::kit(true).c.bg_accent;
        let rects = |rt: &Runtime<Ui>| {
            let line = |key: &str| testkit::rect(rt, key).expect("drawn");
            (line(&header(&website())), line(&row_key(contact)))
        };
        let before = rects(&rt);
        assert_eq!(header_mark(&rt, &website()), None);
        let to = at(&rt, &header(&website()));
        testkit::hold_row(&mut rt, finished, to);
        assert_eq!(header_mark(&rt, &website()), Some(accent));
        assert!(marked(&rt, &website()), "over the kit's selected fill");
        for other in [&own, &side] {
            assert_eq!(header_mark(&rt, other), None, "{other:?}");
        }
        assert_eq!(rects(&rt), before, "nothing in the list moves");
        let to = at(&rt, &row_key(asking));
        testkit::move_held(&mut rt, to);
        assert_eq!(header_mark(&rt, &website()), None);
        assert_eq!(rects(&rt), before, "nothing moves as it goes");
        testkit::release(&mut rt);
        nothing_left(&rt);
    }

    #[test]
    fn a_drop_on_a_collapsed_group_moves_and_keeps_it_collapsed() {
        let mut rt = sessions();
        let finished = titled(&rt, FINISHED);
        let sketches = group_named(&rt, "sketches");
        assert!(collapsed(&rt, &sketches));
        let to = at(&rt, &header(&sketches));
        testkit::hold_row(&mut rt, finished, to);
        assert_eq!(over(&rt), Some(DropTarget::Group(sketches.clone())));
        testkit::release(&mut rt);
        assert_eq!(group_of(&rt, finished), sketches);
        assert!(collapsed(&rt, &sketches), "still folded");
        assert!(
            !testkit::drawn(&rt, &row_key(finished)),
            "its row is folded away"
        );
        nothing_left(&rt);
    }

    #[test]
    fn the_older_groups_line_takes_no_drop() {
        let mut rt = sessions();
        let finished = titled(&rt, FINISHED);
        let own = group_of(&rt, finished);
        // Hidden, website is listed in "Hidden groups (1)", which takes no drop either.
        rt.update(|ui, _| {
            let effects = ui.app.hide_group(&website(), true);
            glue::run(ui, effects);
        });
        testkit::step(&mut rt);
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        for line in ["mr-older-groups", "mr-hidden-groups"] {
            let to = at(&rt, line);
            testkit::hold_row(&mut rt, finished, to);
            assert_eq!(over(&rt), None, "over {line}");
            testkit::release(&mut rt);
            assert_eq!(group_of(&rt, finished), own, "{line}");
            assert!(!saved(&rt), "{line}: nothing is saved");
            assert!(rt.state.side_menu.is_none(), "{line}: no menu opens");
            nothing_left(&rt);
        }
    }

    #[test]
    fn an_answer_that_comes_after_the_release_changes_nothing() {
        let mut rt = sessions();
        let [finished, asking] = [FINISHED, NEEDS_YOU].map(|t| titled(&rt, t));
        let own = group_of(&rt, finished);
        let tiles = tile_keys(&rt);
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        let home = at(&rt, &row_key(asking));
        let over_header = at(&rt, &header(&website()));

        // Rune answers a question asked from an answer only in a later frame, after the first
        // input there. Asked two answers deep, the drag's question about website's header is
        // answered in the frame after the release.
        testkit::hold_row(&mut rt, finished, home);
        let point = [over_header.0, over_header.1];
        rt.update(move |_, tx| {
            let deeper = move |_: &mut Ui, _: Option<rune::Picked>, tx: &mut rune::Tx| {
                let asked = move |_: &mut Ui, _: Option<rune::Picked>, tx: &mut rune::Tx| {
                    super::ask(tx, finished, point);
                };
                tx.element_at_in("mr-sidebar-rows", point[0], point[1], asked);
            };
            tx.element_at_in("mr-sidebar-rows", point[0], point[1], deeper);
        });
        testkit::release(&mut rt);
        assert_eq!(group_of(&rt, finished), own);
        nothing_left(&rt);

        // The answer to the drag's last question, as if a header were there by then. The guard
        // here is structural: with no drag in state an answer has nothing to change.
        testkit::hold_row(&mut rt, finished, home);
        let last = rt.state.drag.as_ref().unwrap().at;
        testkit::release(&mut rt);
        let path = path_at(&rt, &header(&website()));
        rt.update(|ui, _| super::answer(ui, finished, last, Some(&path)));
        testkit::step(&mut rt);
        assert_eq!(group_of(&rt, finished), own);
        assert_eq!(tile_keys(&rt), tiles);
        assert!(!saved(&rt), "nothing is saved");
        assert!(!marked(&rt, &website()));
        nothing_left(&rt);
    }

    #[test]
    fn an_answer_the_pointer_has_left_or_for_another_row_marks_nothing() {
        let mut rt = sessions();
        let [finished, asking, working] = [FINISHED, NEEDS_YOU, WORKING].map(|t| titled(&rt, t));
        let path = path_at(&rt, &header(&website()));
        let left = at(&rt, &header(&website()));
        let to = at(&rt, &row_key(asking));
        testkit::hold_row(&mut rt, finished, to);
        let now = rt.state.drag.as_ref().unwrap().at;
        // A slow answer about where the pointer was before.
        rt.update(|ui, _| super::answer(ui, finished, [left.0, left.1], Some(&path)));
        testkit::step(&mut rt);
        assert_eq!(over(&rt), None, "the pointer has moved on");
        // An answer for another row's drag.
        rt.update(|ui, _| super::answer(ui, working, now, Some(&path)));
        testkit::step(&mut rt);
        assert_eq!(over(&rt), None, "another row's");
        assert!(!marked(&rt, &website()));
        // The drag's own answer to where it is still marks.
        rt.update(|ui, _| super::answer(ui, finished, now, Some(&path)));
        testkit::step(&mut rt);
        assert_eq!(over(&rt), Some(DropTarget::Group(website())));
        testkit::release(&mut rt);
        nothing_left(&rt);
    }

    #[test]
    fn group_of_path_names_the_lines_of_listed_groups() {
        let rt = sessions();
        let [finished, contact] = [FINISHED, CONTACT].map(|t| titled(&rt, t));
        let own = group_of(&rt, finished);
        let sketches = group_named(&rt, "sketches");
        let of = |key: &str| super::group_of_path(&rt.state, &path_at(&rt, key));
        assert_eq!(of(&header(&website())), Some(website()));
        assert_eq!(of(&row_key(contact)), Some(website()));
        assert_eq!(of(&older(&website())), Some(website()));
        assert_eq!(of(&row_key(finished)), Some(own));
        assert_eq!(of(&header(&sketches)), Some(sketches));
        for line in ["mr-older-groups", "mr-new-group"] {
            assert_eq!(of(line), None, "{line}");
        }
        // Inside a header: its name, its "+".
        let name = format!(
            "{}/mr-group-title/mr-group-name",
            path_at(&rt, &header(&website()))
        );
        assert_eq!(super::group_of_path(&rt.state, &name), Some(website()));
        assert_eq!(super::group_of_path(&rt.state, "kit-root/mr-root"), None);
        let unkeyed = format!("{}/#0/{}/label", super::LIST, row_key(contact));
        assert_eq!(super::group_of_path(&rt.state, &unkeyed), Some(website()));
    }

    #[test]
    fn the_move_is_kept() {
        let mut rt = sessions();
        let finished = titled(&rt, FINISHED);
        let entry = rt.state.app.sessions.get(finished).unwrap();
        let id = entry.session.id.clone().expect("the agent's id");
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        let to = at(&rt, &header(&website()));
        testkit::drag_row(&mut rt, finished, to);
        let saved: mr_core::Saved = rt.state.prefs.get(glue::STATE_KEY).expect("a save");
        assert!(
            saved.placed.contains(&(id, website())),
            "{:?}",
            saved.placed
        );
    }

    #[test]
    fn dragging_over_groups_rebuilds_only_the_headers_it_marks() {
        let mut rt = sessions();
        let [finished, asking, working, garden, contact] =
            [FINISHED, NEEDS_YOU, WORKING, GARDEN, CONTACT].map(|t| titled(&rt, t));
        let side = group_named(&rt, "Side projects");
        let to = at(&rt, &row_key(asking));
        testkit::hold_row(&mut rt, finished, to);
        rt.settle_all();
        testkit::step(&mut rt);
        let rows = conversation::all_row_builds();
        // Each step is one pointer move, so no line between is passed over; with the lines
        // rebuilt by it: only a header that is marked or no longer marked.
        let steps = [
            (header(&website()), 1),
            (row_key(contact), 0),
            (older(&website()), 0),
            (header(&side), 2),
            (row_key(garden), 0),
            (row_key(working), 1),
            (row_key(asking), 0),
            (header(&website()), 1),
            ("mr-usage-session".to_string(), 1),
        ];
        for (key, built) in steps {
            let lines = sidebar::line_builds();
            let (x, y) = at(&rt, &key);
            rt.pointer_move(x, y);
            testkit::step(&mut rt);
            assert_eq!(sidebar::line_builds() - lines, built, "over {key}");
        }
        let lines = sidebar::line_builds();
        let [x, _, w, _] = row_rect(&rt);
        rt.pointer_move(x + w / 2.0, mid_y(&rt));
        testkit::step(&mut rt);
        assert!(matches!(over(&rt), Some(DropTarget::Edge(_))));
        assert_eq!(
            sidebar::line_builds(),
            lines,
            "the tile row asks the sidebar nothing"
        );
        assert_eq!(conversation::all_row_builds(), rows, "no conversation row");
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        testkit::step(&mut rt);
        testkit::release(&mut rt);
        nothing_left(&rt);
    }

    // Moving a tile by its header: Task 15 of plan 4.

    /// [`three`] with `c` shown to the side too: the tiles `[a, b, c]`, `c` focused, its input
    /// box with the keyboard.
    fn three_tiles() -> (Runtime<Ui>, SessionKey, SessionKey, SessionKey) {
        let (mut rt, a, b, c) = three(SIZE);
        testkit::show_side(&mut rt, c);
        assert_eq!(tile_keys(&rt), vec![a, b, c]);
        assert_eq!(testkit::focused(&rt), Some(c));
        (rt, a, b, c)
    }

    /// Presses on the centre of `inner`, a part of `key`'s header, and moves the pointer to
    /// `to` in eight even steps ([`testkit::move_held`]): held there, its release still to come.
    fn hold_header_part(rt: &mut Runtime<Ui>, key: SessionKey, inner: &str, to: (f32, f32)) {
        let part = tile(key, inner);
        let from = testkit::find(rt, &part).unwrap_or_else(|| panic!("no {part:?}"));
        rt.pointer_move(from.0, from.1);
        rt.pointer_down(from.0, from.1);
        testkit::step(rt);
        testkit::move_held(rt, to);
    }

    /// [`hold_header_part`] on `key`'s title, which has presses of its own.
    fn hold_header(rt: &mut Runtime<Ui>, key: SessionKey, to: (f32, f32)) {
        hold_header_part(rt, key, "mr-tile-title", to);
    }

    /// A little left of the tile row's right end, half way down.
    fn right_end(rt: &Runtime<Ui>) -> (f32, f32) {
        let [x, _, w, _] = row_rect(rt);
        (x + w - 20.0, mid_y(rt))
    }

    #[test]
    fn a_header_dragged_past_the_others_moves_its_tile() {
        let (mut rt, a, b, c) = three_tiles();
        let to = right_end(&rt);
        hold_header(&mut rt, a, to);
        assert_eq!(over(&rt), Some(DropTarget::Edge(3)));
        assert_eq!(
            rt.state.drag.as_ref().map(|d| d.from),
            Some(DragFrom::Header)
        );
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, c, a]);
        assert_eq!(testkit::focused(&rt), Some(a));
        assert!(box_focused(&rt, a), "the keyboard in a's box");
        nothing_left(&rt);
    }

    #[test]
    fn a_header_dragged_left_moves_its_tile_between_the_others() {
        // By the folder's chip, a plain part of the header.
        let (mut rt, a, b, c) = three_tiles();
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        hold_header_part(&mut rt, c, "mr-tile-folder", to);
        assert_eq!(over(&rt), Some(DropTarget::Edge(1)));
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c, b]);
        assert_eq!(testkit::focused(&rt), Some(c));
        assert!(box_focused(&rt, c));
        nothing_left(&rt);
    }

    #[test]
    fn a_small_movement_on_a_header_is_still_a_click() {
        let (mut rt, a, b, c) = three_tiles();
        let (x, y) = testkit::find(&rt, &tile(a, "mr-tile-title")).unwrap();
        hold_header(&mut rt, a, (x + 3.0, y));
        assert_eq!(rt.state.drag, None, "no drag within the slop");
        assert!(!testkit::drawn(&rt, CHIP));
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b, c], "the order stays");
        assert_eq!(testkit::focused(&rt), Some(a), "the click focused a");
        assert!(box_focused(&rt, a));
        nothing_left(&rt);
    }

    #[test]
    fn the_line_shows_where_the_tile_will_go() {
        let (mut rt, a, b, c) = three_tiles();
        let edge = edge_x(&rt, 2);
        let to = (edge - 30.0, mid_y(&rt));
        hold_header(&mut rt, a, to);
        assert_eq!(over(&rt), Some(DropTarget::Edge(2)));
        let [lx, _, lw, _] = testkit::rect(&rt, EDGE).expect("the line is drawn");
        assert!((lx + lw / 2.0 - edge).abs() <= 1.0, "at the edge: {lx}");
        let [x, y, _, _] = testkit::rect(&rt, CHIP).expect("the chip is drawn");
        let (px, py) = rt.pointer().unwrap();
        let near = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
        assert!(near <= 40.0, "{near} points away");
        // Over the dragged tile itself, whose own edges move nothing.
        let [ax, _, aw, _] = rt.presented(&tiles::tile_key(a)).unwrap().0;
        let to = (ax + aw / 2.0, mid_y(&rt));
        testkit::move_held(&mut rt, to);
        assert!(rt.state.drag.is_some());
        assert_eq!(over(&rt), None);
        assert!(!testkit::drawn(&rt, EDGE), "no line");
        testkit::release(&mut rt);
        nothing_left(&rt);
        assert_eq!(tile_keys(&rt), vec![a, b, c]);
    }

    #[test]
    fn a_tile_moves_in_a_row_with_no_room_for_another() {
        let (mut rt, a, b, _) = three((1300.0, 900.0));
        assert!(!rt.state.app.room_for_tile());
        let to = right_end(&rt);
        hold_header(&mut rt, a, to);
        assert_eq!(over(&rt), Some(DropTarget::Edge(2)));
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, a]);
        assert_eq!(testkit::focused(&rt), Some(a));
        nothing_left(&rt);
    }

    #[test]
    fn a_header_dropped_on_the_sidebar_does_nothing() {
        let (mut rt, a, b, c) = three_tiles();
        let before = arrangement(&rt);
        let group = group_of(&rt, a);
        let kit = theme::kit(true);
        let marks = |rt: &Runtime<Ui>| {
            let sections = &rt.state.app.sidebar.sections;
            sections
                .iter()
                .filter(|section| sidebar::target_mark(&rt.state, &section.id, kit).is_some())
                .count()
        };
        // Over another group's row, then its header.
        let to = testkit::find(&rt, &row_key(b)).unwrap();
        hold_header(&mut rt, a, to);
        assert!(rt.state.drag.is_some());
        assert_eq!(over(&rt), None);
        assert_eq!(marks(&rt), 0, "no group is marked");
        let header = format!("mr-group-{}", group_of(&rt, b).key_text());
        let to = testkit::find(&rt, &header).unwrap();
        testkit::move_held(&mut rt, to);
        assert_eq!(over(&rt), None);
        assert_eq!(marks(&rt), 0, "no group is marked");
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b, c]);
        assert_eq!(testkit::focused(&rt), Some(c));
        assert!(box_focused(&rt, c), "the keyboard stays where it was");
        assert_eq!(group_of(&rt, a), group);
        assert_eq!(arrangement(&rt), before);
        nothing_left(&rt);
    }

    #[test]
    fn escape_cancels_a_tile_move() {
        let (mut rt, a, b, c) = three_tiles();
        // A turn runs in the focused tile, the one dragged: Escape ends the drag and nothing
        // else.
        testkit::emit_to(&mut rt, c, SessionEvent::TurnStarted);
        let working = |rt: &Runtime<Ui>| {
            let entry = rt.state.app.sessions.get(c).unwrap();
            entry.session.phase == mr_core::Phase::Working
        };
        assert!(working(&rt));
        let sent = testkit::sent(&rt).len();
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        hold_header(&mut rt, c, to);
        assert!(testkit::drawn(&rt, CHIP) && testkit::drawn(&rt, EDGE));
        testkit::press(&mut rt, Key::Escape);
        nothing_left(&rt);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b, c], "nothing moves");
        assert_eq!(testkit::focused(&rt), Some(c));
        assert!(box_focused(&rt, c), "the keyboard stays where it was");
        nothing_left(&rt);
        assert!(working(&rt), "the turn goes on");
        assert!(
            !testkit::sent(&rt)[sent..].contains(&SessionCommand::Stop),
            "no Stop was sent"
        );
    }

    #[test]
    fn one_tile_alone_is_not_dragged() {
        let mut rt = testkit::window_sized("empty", SIZE);
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        assert_eq!(tile_keys(&rt), vec![a]);
        let to = right_end(&rt);
        hold_header(&mut rt, a, to);
        assert_eq!(rt.state.drag, None);
        assert!(!testkit::drawn(&rt, CHIP));
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a]);
        nothing_left(&rt);
    }

    /// Double-clicks the element `key` names: two presses at one point with a frame between
    /// them, then steps.
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

    #[test]
    fn a_header_whose_name_is_edited_is_not_dragged() {
        let (mut rt, a, b, c) = three_tiles();
        double_click(&mut rt, &tile(a, "mr-tile-title"));
        assert_eq!(rt.state.renaming, Some(Renaming::Header(a)));
        let to = right_end(&rt);
        // In the name field, then on the header's plain part.
        for part in ["mr-tile-rename", "mr-tile-folder"] {
            hold_header_part(&mut rt, a, part, to);
            assert_eq!(rt.state.drag, None, "{part}");
            assert!(!testkit::drawn(&rt, CHIP), "{part}");
            if part == "mr-tile-rename" {
                assert_eq!(rt.state.renaming, Some(Renaming::Header(a)), "still edited");
            }
            testkit::release(&mut rt);
            assert_eq!(tile_keys(&rt), vec![a, b, c], "{part}");
            nothing_left(&rt);
        }
    }

    #[test]
    fn the_header_still_closes_and_renames() {
        let (mut rt, a, b, c) = three_tiles();
        // A press on the close button that moves drags nothing.
        let to = right_end(&rt);
        hold_header_part(&mut rt, b, "mr-tile-close", to);
        assert_eq!(rt.state.drag, None);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, b, c]);
        testkit::click(&mut rt, &tile(b, "mr-tile-close"));
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c], "the click closed b's tile");
        double_click(&mut rt, &tile(a, "mr-tile-title"));
        assert_eq!(rt.state.renaming, Some(Renaming::Header(a)));
        assert_eq!(testkit::focused(&rt), Some(a));
    }

    #[test]
    fn a_move_keeps_the_draft_and_rebuilds_no_conversation_row() {
        let (mut rt, a, b, c) = three_tiles();
        for key in [a, b, c] {
            testkit::emit_to(&mut rt, key, SessionEvent::TextDelta("A reply".into()));
            testkit::emit_to(&mut rt, key, SessionEvent::BlockFinished);
        }
        testkit::click(&mut rt, &tile(a, "mr-input"));
        testkit::type_text(&mut rt, "A draft");
        rt.settle_all();
        testkit::step(&mut rt);
        let rows = conversation::all_row_builds();
        let to = right_end(&rt);
        hold_header(&mut rt, a, to);
        testkit::release(&mut rt);
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, c, a]);
        assert_eq!(conversation::all_row_builds(), rows, "no conversation row");
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "A draft");
        let shown = rt.elements().into_iter().any(|e| {
            !e.exiting
                && e.path.contains(&format!("/{}/", tiles::tile_key(a)))
                && e.text.as_deref() == Some("A draft")
        });
        assert!(shown, "a's box shows the draft");
        assert!(box_focused(&rt, a));
        testkit::type_text(&mut rt, "!");
        assert_eq!(glue::view_of(&rt.state, a).draft.text, "A draft!");
    }

    #[test]
    fn a_move_keeps_the_scroll_position() {
        let (mut rt, a, b, c) = three_tiles();
        for n in 1..=200 {
            let delta = SessionEvent::TextDelta(format!("Line {n} of a"));
            crate::demo::emit_to(&rt.state, a, delta);
            crate::demo::emit_to(&rt.state, a, SessionEvent::BlockFinished);
        }
        testkit::step(&mut rt);
        rt.settle_all();
        testkit::step(&mut rt);
        let scroll = format!("mr-conversation-{}", a.0);
        let (x, y) = rt.find(&scroll).unwrap();
        for _ in 0..5 {
            rt.wheel(x, y, 600.0);
            testkit::step(&mut rt);
        }
        assert_eq!(rt.scroll_at_bottom(&scroll), Some(false));
        // The first line of a's on screen, and how far below its tile's top it is.
        let first = |rt: &Runtime<Ui>| {
            let [_, top, _, _] = rt.presented(&tiles::tile_key(a)).expect("drawn").0;
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
        let to = right_end(&rt);
        hold_header(&mut rt, a, to);
        testkit::release(&mut rt);
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, c, a]);
        assert_eq!(rt.scroll_at_bottom(&scroll), Some(false));
        let after = first(&rt);
        assert_eq!(after.0, before.0, "the same line first");
        assert!(
            (after.1 - before.1).abs() < 0.5,
            "{before:?}, then {after:?}"
        );
    }

    #[test]
    fn the_widths_stay_with_their_tiles() {
        let (mut rt, a, b, c) = three_tiles();
        let divider = format!("mr-divider-{}", a.0);
        let (x, y) = rt.find(&divider).expect("a's divider");
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move(x + 60.0, y);
        rt.pointer_move(x + 120.0, y);
        testkit::step(&mut rt);
        rt.pointer_up(x + 120.0, y);
        testkit::step(&mut rt);
        let width = |rt: &Runtime<Ui>, key: SessionKey| {
            rt.presented(&tiles::tile_key(key)).expect("drawn").0[2]
        };
        let before = [a, b, c].map(|key| width(&rt, key));
        assert!(before[0] > before[2] + 50.0, "a is wider: {before:?}");
        let to = right_end(&rt);
        hold_header(&mut rt, a, to);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, c, a]);
        let after = [a, b, c].map(|key| width(&rt, key));
        for (was, is) in before.iter().zip(after) {
            assert!((was - is).abs() < 0.5, "{before:?}, then {after:?}");
        }
    }

    #[test]
    fn a_tile_closed_during_its_drag_ends_the_drag() {
        let (mut rt, a, b, c) = three_tiles();
        let to = right_end(&rt);
        hold_header(&mut rt, a, to);
        assert!(rt.state.drag.is_some());
        rt.update(|ui, _| {
            let effects = ui.app.close_tile(a, mr_core::Leaving::Keep);
            glue::run(ui, effects);
        });
        testkit::step(&mut rt);
        nothing_left(&rt);
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![b, c]);
        nothing_left(&rt);
    }

    #[test]
    fn the_windows_keys_wait_while_a_header_is_dragged() {
        let (mut rt, a, b, c) = three_tiles();
        let to = (edge_x(&rt, 1) + 10.0, mid_y(&rt));
        hold_header(&mut rt, c, to);
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        testkit::press(&mut rt, Key::Function(6));
        testkit::press_with(&mut rt, Key::Function(6), shift);
        testkit::press_with(&mut rt, Key::Command('w'), testkit::ctrl());
        testkit::press_with(&mut rt, Key::Command('n'), testkit::ctrl());
        testkit::press_with(&mut rt, Key::Command('n'), testkit::ctrl_shift());
        testkit::press_with(&mut rt, Key::PageUp, testkit::ctrl_shift());
        testkit::press_with(&mut rt, Key::PageDown, testkit::ctrl_shift());
        assert_eq!(rt.state.folder_menu, None, "no folder menu");
        assert!(rt.resolve_file_dialog(None).is_err(), "no folder picker");
        assert_eq!(tile_keys(&rt), vec![a, b, c], "nothing closed or moved");
        assert_eq!(testkit::focused(&rt), Some(c), "the focus stays");
        assert_eq!(over(&rt), Some(DropTarget::Edge(1)), "the drag goes on");
        testkit::move_held(&mut rt, (to.0 + 5.0, to.1));
        testkit::release(&mut rt);
        assert_eq!(tile_keys(&rt), vec![a, c, b], "and still drops");
        nothing_left(&rt);
    }

    #[test]
    fn a_header_drag_leaves_no_press_to_swallow() {
        let (mut rt, a, b, c) = three(SIZE);
        let home = testkit::find(&rt, &tile(a, "mr-tile-title")).unwrap();
        let to = right_end(&rt);
        hold_header(&mut rt, a, to);
        assert!(rt.state.drag.is_some());
        // Let go inside its own header: Rune runs the title's press after the release.
        testkit::move_held(&mut rt, home);
        assert_eq!(over(&rt), None);
        // One frame only: `glue::poll` clears the flag before the next frame's input.
        rt.pointer_up(home.0, home.1);
        rt.frame(0.016);
        assert!(!rt.state.swallow_row_press, "nothing to swallow");
        testkit::step(&mut rt);
        nothing_left(&rt);
        assert_eq!(tile_keys(&rt), vec![a, b]);
        assert_eq!(
            testkit::focused(&rt),
            Some(a),
            "the press focused a, as a click"
        );
        testkit::click(&mut rt, &row_key(c));
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), vec![c, b], "the click shows c");
        assert_eq!(testkit::focused(&rt), Some(c));
    }

    #[test]
    fn a_move_is_saved() {
        let (mut rt, a, b, c) = three_tiles();
        let id = |rt: &Runtime<Ui>, key: SessionKey| {
            let entry = rt.state.app.sessions.get(key).unwrap();
            entry.session.id.clone().expect("the agent's id")
        };
        let ids = [b, c, a].map(|key| id(&rt, key));
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        let to = right_end(&rt);
        hold_header(&mut rt, a, to);
        assert!(!saved(&rt), "nothing written during the drag");
        testkit::release(&mut rt);
        let saved: mr_core::Saved = rt.state.prefs.get(glue::STATE_KEY).expect("a save");
        let order: Vec<SessionId> = saved.tiles.iter().map(|tile| tile.id.clone()).collect();
        assert_eq!(order, ids.to_vec());
        assert_eq!(saved.shown, Some(id(&rt, a)));
    }
}
