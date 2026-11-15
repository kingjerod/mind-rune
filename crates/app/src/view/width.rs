//! The column the conversation and the input box share, and the two handles that set its width.
//!
//! The column is centred in the tile. The conversation's scroll still spans the whole tile, so
//! the wheel scrolls it over the empty margins too; only its rows and the input box are as wide
//! as the column.

use mr_core::{Effect, MIN_WIDTH, SessionKey, Width};
use rune::{Cursor, Cx, Drag, El, Phase, Transition, Tx, col, el, row};
use rune_kit::Kit;

use super::tiles::tile_key;
use crate::glue::{self, Ui};

/// How close to the tile's edge a drag lets go of every limit.
pub const EDGE_SNAP: f32 = 24.0;

/// A handle's width, and the width of the line it shows while hovered.
const HANDLE: f32 = 8.0;
const HANDLE_LINE: f32 = 2.0;

/// The column's width inside a tile of `tile` points.
pub fn column_width(width: Width, tile: f32) -> f32 {
    match width {
        Width::Points(points) => points.min(tile),
        Width::Unlimited => tile,
    }
}

/// The width a drag asks for, from the pointer's distance to the tile's centre line.
pub fn width_from_drag(pointer_x: f32, tile_left: f32, tile: f32) -> Width {
    let tile_right = tile_left + tile;
    if pointer_x <= tile_left + EDGE_SNAP || pointer_x >= tile_right - EDGE_SNAP {
        return Width::Unlimited;
    }
    let centre = tile_left + tile / 2.0;
    Width::Points((2.0 * (pointer_x - centre).abs()).max(MIN_WIDTH))
}

/// The column's width in this frame in `key`'s tile, for the rows and the input box: the chosen
/// width inside the tile as last laid out. `None` before the tile's first layout, which the
/// same commit follows at once with the real width.
pub fn current(ui: &Ui, cx: &mut Cx<Ui>, key: SessionKey) -> Option<f32> {
    tile(cx, key).map(|[_, tile]| column_width(ui.app.width, tile))
}

/// Wraps the conversation and the input box of `key`'s session in the column centred in its
/// tile, with its two handles.
///
/// `content` spans the tile and limits its own rows and input box to [`current`]. Over it lies
/// the column keyed `mr-column`, which takes no pointer input itself, holding a handle at each
/// edge, keyed `mr-width-left` and `mr-width-right`.
pub fn column(ui: &Ui, cx: &mut Cx<Ui>, kit: &Kit, key: SessionKey, content: El) -> El {
    let tile = tile(cx, key);
    let lane = el().key("mr-column").transition(Transition::none());
    let lane = match tile {
        Some([_, tile]) => {
            let width = column_width(ui.app.width, tile);
            lane.pin(Some((tile - width) / 2.0), Some(0.0), None, Some(0.0))
                .w(width)
        }
        None => lane.pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)),
    };
    let lane = lane
        .child(handle("mr-width-left", true, tile, kit))
        .child(handle("mr-width-right", false, tile, kit));
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(content)
        .child(lane)
}

/// The left edge of `key`'s tile in window points and its width, as last laid out.
fn tile(cx: &mut Cx<Ui>, key: SessionKey) -> Option<[f32; 2]> {
    cx.layout_with(&tile_key(key), |rect| [rect[0], rect[2]])
}

/// One of the column's handles: 8 points wide along one edge, the tile's full height, a
/// resize cursor, and a thin line that shows only while the pointer is over it. Dragging it
/// sets the width on every move and saves it on release; both edges follow, as the column
/// stays centred. It is no Tab
/// stop, takes no keys, and leaves the focused field (the input box) focused when pressed.
fn handle(key: &'static str, leading: bool, tile: Option<[f32; 2]>, kit: &Kit) -> El {
    let line = el()
        .w(HANDLE_LINE)
        .h_full()
        .fill(kit.c.border_strong)
        .group_opacity(0.0, 1.0);
    let handle = row()
        .key(key)
        .w(HANDLE)
        .justify_center()
        .role("separator")
        .label("Conversation width")
        .cursor(Cursor::ResizeColumn)
        .hover_group()
        .focusable(false)
        .preserve_focus_on_press(true)
        .transition(Transition::none())
        .on_drag(move |ui: &mut Ui, drag: &mut Drag, tx: &mut Tx| {
            if let Some([left, width]) = tile {
                drag_to(ui, drag, tx, left, width);
            }
        })
        .child(line);
    if leading {
        handle.pin(Some(0.0), Some(0.0), None, Some(0.0))
    } else {
        handle.pin(None, Some(0.0), Some(0.0), Some(0.0))
    }
}

/// One phase of a handle's drag across a tile from `tile_left`, `tile` points wide.
///
/// The press keeps the width it started from. Each move sets the width the pointer asks for at
/// once, but leaves the settings alone: a drag writes them once, on release, and only when the
/// width ends up different from where it started. A cancelled drag puts the first width back and
/// writes nothing, as instantly as the moves did.
fn drag_to(ui: &mut Ui, drag: &mut Drag, tx: &mut Tx, tile_left: f32, tile: f32) {
    let width = match drag.phase {
        Phase::Down => {
            drag.data = match ui.app.width {
                Width::Points(points) => points,
                Width::Unlimited => f32::INFINITY,
            };
            return;
        }
        Phase::Move => width_from_drag(drag.window_point()[0], tile_left, tile),
        Phase::Up if drag.cancelled() => {
            tx.instant();
            started(drag)
        }
        Phase::Up => {
            let effects = if ui.app.width == started(drag) {
                Vec::new()
            } else {
                vec![Effect::SaveWidth(ui.app.width)]
            };
            glue::run(ui, effects);
            return;
        }
    };
    let mut effects = ui.app.set_width(width);
    effects.retain(|effect| !matches!(effect, Effect::SaveWidth(_)));
    glue::run(ui, effects);
}

/// The width a drag started from, as its press kept it in [`Drag::data`].
fn started(drag: &Drag) -> Width {
    if drag.data.is_finite() {
        Width::Points(drag.data)
    } else {
        Width::Unlimited
    }
}

#[cfg(test)]
mod tests {
    use mr_core::{DEFAULT_WIDTH, MIN_WIDTH, Width};
    use rune::Runtime;

    use super::*;
    use crate::glue::Ui;
    use crate::testkit;

    #[test]
    fn the_column_is_the_chosen_width_or_the_tile() {
        assert_eq!(column_width(Width::Points(1000.0), 1600.0), 1000.0);
        assert_eq!(column_width(Width::Points(1000.0), 700.0), 700.0);
        assert_eq!(column_width(Width::Unlimited, 1600.0), 1600.0);
    }

    #[test]
    fn a_drag_sets_the_width_symmetrically() {
        // A tile from x = 300 to x = 1900: centre 1100.
        assert_eq!(width_from_drag(1500.0, 300.0, 1600.0), Width::Points(800.0));
        assert_eq!(width_from_drag(700.0, 300.0, 1600.0), Width::Points(800.0));
        assert_eq!(
            width_from_drag(1110.0, 300.0, 1600.0),
            Width::Points(MIN_WIDTH)
        );
        assert_eq!(width_from_drag(1880.0, 300.0, 1600.0), Width::Unlimited);
        assert_eq!(width_from_drag(310.0, 300.0, 1600.0), Width::Unlimited);
        assert_eq!(width_from_drag(2500.0, 300.0, 1600.0), Width::Unlimited);
        assert_eq!(
            width_from_drag(1875.0, 300.0, 1600.0),
            Width::Points(1550.0)
        );
    }

    fn drag(rt: &mut Runtime<Ui>, key: &str, to_x: f32) {
        let (x, y) = rt.find(key).unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move((x + to_x) / 2.0, y);
        rt.pointer_move(to_x, y);
        rt.pointer_up(to_x, y);
        testkit::step(rt);
    }
    fn column(rt: &Runtime<Ui>) -> [f32; 4] {
        rt.presented("mr-column").unwrap().0
    }

    /// The focused session's tile, as drawn.
    fn tile_rect(rt: &Runtime<Ui>) -> [f32; 4] {
        let key = testkit::focused(rt).unwrap();
        rt.presented(&tile_key(key)).unwrap().0
    }

    #[test]
    fn dragging_a_handle_changes_the_width_and_remembers_it() {
        let mut rt = testkit::window_sized("conversation", (2400.0, 900.0));
        let [_, _, w, _] = column(&rt);
        assert_eq!(w, 1000.0);
        let [tile_x, _, tile_w, _] = tile_rect(&rt);
        let centre = tile_x + tile_w / 2.0;
        drag(&mut rt, "mr-width-right", centre + 700.0);
        assert_eq!(rt.state.app.width, Width::Points(1400.0));
        assert_eq!(column(&rt)[2], 1400.0);
        assert_eq!(
            rt.state.prefs.get::<Width>("conversation_width"),
            Some(Width::Points(1400.0))
        );
        drag(&mut rt, "mr-width-left", tile_x + 2.0);
        assert_eq!(rt.state.app.width, Width::Unlimited);
        assert_eq!(column(&rt)[2], tile_w);
        drag(&mut rt, "mr-width-left", centre - 300.0);
        assert_eq!(rt.state.app.width, Width::Points(600.0));
    }

    #[test]
    fn the_input_box_is_as_wide_as_the_conversation() {
        let rt = testkit::window_sized("conversation", (2400.0, 900.0));
        let input = rt.presented("mr-input").unwrap().0;
        let [x, _, w, _] = column(&rt);
        assert!(input[0] >= x && input[0] + input[2] <= x + w + 0.5);
    }

    #[test]
    fn a_narrow_window_uses_all_of_its_tile() {
        let rt = testkit::window_sized("conversation", (900.0, 700.0));
        let [_, _, tile_w, _] = tile_rect(&rt);
        assert_eq!(column(&rt)[2], tile_w);
    }

    fn width_of(rt: &Runtime<Ui>, key: &str) -> f32 {
        rt.presented(key).unwrap().0[2]
    }

    #[test]
    fn the_rows_and_their_text_follow_the_width() {
        let mut rt = testkit::window_sized("conversation", (2400.0, 900.0));
        assert_eq!(width_of(&rt, "mr-item-1"), 1000.0);
        let [tile_x, _, tile_w, _] = tile_rect(&rt);
        drag(&mut rt, "mr-width-right", tile_x + tile_w / 2.0 + 700.0);
        assert_eq!(width_of(&rt, "mr-item-0"), 1400.0);
        assert_eq!(width_of(&rt, "mr-item-1"), 1400.0);
        assert!((width_of(&rt, "mr-reply-1") - (1400.0 - 48.0)).abs() < 0.5);
    }

    #[test]
    fn the_wheel_scrolls_the_conversation_over_the_margins() {
        let mut rt = testkit::window_sized("long", (2400.0, 900.0));
        let [column_x, ..] = column(&rt);
        let [tile_x, ..] = tile_rect(&rt);
        let (_, y) = rt.find(&testkit::conversation(&rt)).unwrap();
        let margin = (tile_x + column_x) / 2.0;
        assert!(margin + 20.0 < column_x);
        rt.wheel(margin, y, 600.0);
        testkit::step(&mut rt);
        assert_eq!(
            rt.scroll_at_bottom(&testkit::conversation(&rt)),
            Some(false)
        );
    }

    #[test]
    fn the_handles_take_no_keyboard_focus() {
        let mut rt = testkit::window_sized("conversation", (2400.0, 900.0));
        for _ in 0..30 {
            testkit::press(&mut rt, rune::Key::Tab);
            let focused: Vec<String> = rt
                .elements()
                .into_iter()
                .filter(|e| e.focused)
                .map(|e| e.path)
                .collect();
            assert!(
                !focused.iter().any(|path| path.contains("mr-width-")),
                "{focused:?}"
            );
        }
    }

    #[test]
    fn dragging_a_handle_leaves_the_input_box_focused() {
        let mut rt = testkit::window_sized("conversation", (2400.0, 900.0));
        let focused = |rt: &Runtime<Ui>| {
            rt.elements()
                .into_iter()
                .any(|e| e.focused && e.path.ends_with("mr-input/scroll/input"))
        };
        assert!(focused(&rt));
        let [tile_x, _, tile_w, _] = tile_rect(&rt);
        drag(&mut rt, "mr-width-right", tile_x + tile_w / 2.0 + 700.0);
        assert!(focused(&rt));
    }

    fn saved(rt: &Runtime<Ui>) -> Option<Width> {
        rt.state.prefs.get::<Width>("conversation_width")
    }

    /// Presses the handle keyed `key` and moves the pointer to `to_x`, without letting go.
    fn hold_at(rt: &mut Runtime<Ui>, key: &str, to_x: f32) -> f32 {
        let (x, y) = rt.find(key).unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move((x + to_x) / 2.0, y);
        rt.pointer_move(to_x, y);
        testkit::step(rt);
        y
    }

    #[test]
    fn a_drag_saves_the_width_once_when_it_ends() {
        let mut rt = testkit::window_sized("conversation", (2400.0, 900.0));
        let before = saved(&rt);
        assert_ne!(before, Some(Width::Points(1400.0)));
        let [tile_x, _, tile_w, _] = tile_rect(&rt);
        let to_x = tile_x + tile_w / 2.0 + 700.0;
        let y = hold_at(&mut rt, "mr-width-right", to_x);
        assert_eq!(column(&rt)[2], 1400.0);
        assert_eq!(
            saved(&rt),
            before,
            "nothing is saved while the drag goes on"
        );
        rt.pointer_up(to_x, y);
        testkit::step(&mut rt);
        assert_eq!(saved(&rt), Some(Width::Points(1400.0)));
    }

    #[test]
    fn a_cancelled_drag_puts_the_width_back_and_saves_nothing() {
        let mut rt = testkit::window_sized("conversation", (2400.0, 900.0));
        let before = saved(&rt);
        let [tile_x, _, tile_w, _] = tile_rect(&rt);
        let to_x = tile_x + tile_w / 2.0 + 700.0;
        let y = hold_at(&mut rt, "mr-width-right", to_x);
        assert_eq!(rt.state.app.width, Width::Points(1400.0));
        testkit::press(&mut rt, rune::Key::Escape);
        rt.pointer_up(to_x, y);
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.width, Width::Points(DEFAULT_WIDTH));
        assert_eq!(column(&rt)[2], DEFAULT_WIDTH);
        assert_eq!(saved(&rt), before);
    }

    #[test]
    fn a_press_without_a_move_saves_nothing() {
        let mut rt = testkit::window_sized("conversation", (2400.0, 900.0));
        let before = saved(&rt);
        testkit::click(&mut rt, "mr-width-right");
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.width, Width::Points(DEFAULT_WIDTH));
        assert_eq!(saved(&rt), before);
    }
}
