//! The sidebar's list: every group's header with its sessions' rows under it, as one flat list
//! of virtual rows drawn from the core's model (`App::sidebar`); and its menus, a row's and a
//! header's, with renaming a group or a session in place.
//!
//! Every line is one row of the list, all of one height, headers included. Only the rows near
//! the viewport are built, and a built row is built again only when the model is rebuilt: the
//! model's version (`App::sidebar_version`) is the list's version and each row's, so nothing
//! here is redone while a reply merely streams. The one line drawn from more than the model is
//! the header or the row whose name is being edited: its version holds the field too
//! ([`line_version`]).

use std::borrow::Cow;

use mr_core::{
    Effect, GroupId, Line, Mark, Place, Row, Section, SessionKey, SidebarModel, SidebarView, Spot,
};
use rune::{
    Anim, Color, Cx, El, GrowthSpec, Key, KeyEvent, PointerPress, Spring, TextEdit, TextStyle,
    Transition, Tx, col, el, icon, memo, row, text, text_input,
};
use rune_kit::shell::{ROW_HEIGHT, RowStatus, SidebarRow};
use rune_kit::{
    ControlSize, IconButton, IconButtonVariant, Kit, Menu, MenuEntry, MenuItem, Spinner,
};
use rune_theme_opencode::icons::PLUS;

use crate::glue::{self, DropTarget, MenuTarget, Renaming, SideMenu, Ui};
use crate::theme::{self, Pair};
use crate::view::{deleted, drag, folder};

/// Rows and headers sit this far in from the list's sides, as the kit's do.
const INSET: f32 = 8.0;

/// The sidebar's one menu. The kit keys its surface `mr-side-menu-menu` and its items
/// `mr-side-menu-item{i}`.
const MENU: &str = "mr-side-menu";

/// A row's "Move to group" submenu, as the kit keys it.
const SUBMENU: &str = "mr-side-menu-item0-sub-menu";

/// The field a group's name is edited in, in its header.
const RENAME: &str = "mr-group-rename";

/// The field a session's name is edited in, in its row.
const ROW_RENAME: &str = "mr-row-rename";

/// The mark on the header of the group a dragged row would drop into, and how thick it is.
const TARGET: &str = "mr-group-target";
const TARGET_WIDTH: f32 = 2.0;

/// The list of groups and sessions, for the kit's `Sidebar`, whose list it goes in: one row of
/// [`ROW_HEIGHT`] per line of the model.
///
/// The rows are the virtual rows of a growth stack of this app's own rather than
/// `Sidebar::virtual_rows`, which takes no version: without one, Rune asks every row's key on
/// every commit. The stack sits in the kit's list, so the list's focus group (one Tab stop,
/// Up and Down) holds its rows.
///
/// An empty Deleted view says so in one muted line instead.
pub fn list(ui: &Ui, kit: &'static Kit) -> El {
    let model = &ui.app.sidebar;
    if model.view == SidebarView::Deleted && model.lines.is_empty() {
        return muted_line("Nothing is deleted.", kit).key("mr-deleted-empty");
    }
    col()
        .key("mr-sidebar-rows")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            ui.app.sidebar.lines.len(),
            ROW_HEIGHT,
            |ui: &Ui, i| Cow::Owned(line_key(&ui.app.sidebar, ui.app.sidebar.lines[i])),
            move |ui: &Ui, i| {
                let line = ui.app.sidebar.lines[i];
                memo(
                    line_key(&ui.app.sidebar, line),
                    line_version(ui, line),
                    || build_line(ui, line, kit),
                )
            },
        )
        .virtual_rows_version(ui.app.sidebar_version)
}

/// What `line` is drawn from: the model; for the header or the row whose name is being
/// edited, the field, which changes with every key while the model holds the old name until it
/// is kept; and for a header, whether a dragged row would drop into its group ([`drop_group`]),
/// so that a drag moving over the list rebuilds only the headers it marks.
fn line_version(ui: &Ui, line: Line) -> (u64, Option<&TextEdit>, bool, bool) {
    let sections = &ui.app.sidebar.sections;
    let editing = match (line, &ui.renaming) {
        (Line::Header(s), Some(Renaming::Group(id))) => *id == sections[s].id,
        (Line::Row(s, r), Some(Renaming::Row(key))) => *key == sections[s].rows[r].key,
        _ => false,
    };
    let target = matches!(line, Line::Header(s) if drop_group(ui) == Some(&sections[s].id));
    let selected = matches!(line, Line::Row(s, r) if is_selected(ui, &sections[s].rows[r]));
    (
        ui.app.sidebar_version,
        editing.then_some(&ui.rename),
        target,
        selected,
    )
}

/// The row the arrow keys passed onto (see [`select`]), while no tile came, went or took the
/// focus since.
fn selected_row(ui: &Ui) -> Option<SessionKey> {
    let (key, placement) = ui.passed_row?;
    (ui.app.tiles.placement() == placement).then_some(key)
}

/// Whether `row` is the list's selected one: the row the arrow keys passed onto, else the
/// focused tile's.
fn is_selected(ui: &Ui, row: &Row) -> bool {
    selected_row(ui).map_or(row.focused, |key| key == row.key)
}

/// The group a dragged row would go into if it were let go now (`view::drag`).
fn drop_group(ui: &Ui) -> Option<&GroupId> {
    match ui.drag.as_ref()?.over.as_ref()? {
        DropTarget::Group(id) => Some(id),
        _ => None,
    }
}

/// The colour of the mark on `id`'s header while a dragged row would drop into its group: the
/// accent of the drop line and of the outline of a tile a drop would take (ruling R10).
pub(super) fn target_mark(ui: &Ui, id: &GroupId, kit: &Kit) -> Option<Color> {
    (drop_group(ui) == Some(id)).then_some(kit.c.bg_accent)
}

/// The mark on the header a drop would go into: a border over the header's whole line, with
/// its corners, taking no room in it, so nothing in the list moves as it comes and goes. It
/// comes and goes at once, as the drop line does.
fn target_ring(color: Color, kit: &Kit) -> El {
    el().key(TARGET)
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .radius(kit.radius.md)
        .border(TARGET_WIDTH, color)
        .transition(Transition::none())
        .motion(Spring::NONE)
        .a11y_hidden()
}

/// A line's key, which names its session or its group.
fn line_key(model: &SidebarModel, line: Line) -> String {
    match line {
        Line::Header(s) => header_key(&model.sections[s].id),
        Line::Row(s, r) => format!("mr-row-{}", model.sections[s].rows[r].key.0),
        Line::ShowOlder(s) => format!("mr-older-{}", model.sections[s].id.key_text()),
        Line::Loading => "mr-sessions-loading".into(),
        Line::OlderGroups => "mr-older-groups".into(),
        Line::NewGroup => "mr-new-group".into(),
        Line::HiddenGroups => "mr-hidden-groups".into(),
    }
}

fn header_key(id: &GroupId) -> String {
    format!("mr-group-{}", id.key_text())
}

fn build_line(ui: &Ui, line: Line, kit: &Kit) -> El {
    #[cfg(test)]
    LINE_BUILDS.with(|builds| builds.set(builds.get() + 1));
    let model = &ui.app.sidebar;
    if model.view == SidebarView::Deleted {
        return deleted_line(model, line, kit);
    }
    match line {
        Line::Header(s) => header(ui, &model.sections[s], kit),
        Line::Row(s, r) => {
            let row = &model.sections[s].rows[r];
            if ui.renaming == Some(Renaming::Row(row.key)) {
                return renaming_row(ui, row, kit);
            }
            // The focused tile's row is the selected one, unless the arrow keys passed onto
            // another tile's; the other tiles' rows carry the hover fill at rest, so they read
            // as open and not as selected.
            let selected = is_selected(ui, row);
            let built = if let Some(pair) = theme::pair_of(row.mark) {
                coloured_row(row, selected, pair, kit)
            } else {
                let mut built = SidebarRow::new(row.title.clone()).selected(selected);
                if let Some(folder) = &row.folder_label {
                    built = built.detail(folder.clone());
                }
                if let Some(status) = status(row.mark) {
                    built = built.status(status);
                }
                let built = built.on_select(select(row.key)).build(kit);
                if row.shown && !selected {
                    built.fill(kit.c.hover)
                } else {
                    built
                }
            };
            built
                .on_key(choose(row.key))
                .on_secondary_press(open_menu(MenuTarget::Row(row.key)))
                .on_drag(drag::row(row.key))
        }
        Line::ShowOlder(s) => {
            let section = &model.sections[s];
            let id = section.id.clone();
            press_line(format!("Show older ({})", section.older), kit).on_press_tx(
                move |ui: &mut Ui, _: &mut Tx| {
                    let effects = ui.app.show_older(&id);
                    glue::run(ui, effects);
                },
            )
        }
        Line::Loading => muted_line("Loading sessions…", kit),
        Line::OlderGroups => {
            let words = if model.older_groups_open {
                "Hide older groups".to_string()
            } else {
                format!("Older groups ({})", model.older_groups)
            };
            press_line(words, kit).on_press_tx(|ui: &mut Ui, _: &mut Tx| {
                let effects = ui.app.toggle_older_groups();
                glue::run(ui, effects);
            })
        }
        Line::NewGroup => new_group_line(kit).on_press_tx(|ui: &mut Ui, tx: &mut Tx| {
            let (id, effects) = ui.app.new_group("");
            glue::run(ui, effects);
            start_rename(ui, tx, Renaming::Group(id));
        }),
        Line::HiddenGroups => press_line(format!("Hidden groups ({})", model.hidden.len()), kit)
            .on_press_tx(|ui: &mut Ui, _: &mut Tx| {
                ui.side_menu = Some(SideMenu {
                    target: MenuTarget::HiddenGroups,
                    // A press gives no point: the menu opens under the line (see `menu`).
                    at: PointerPress { x: 0.0, y: 0.0 },
                    sub_open: false,
                });
            }),
    }
}

/// A line of the Deleted view, which holds only group headers and rows, and "Loading" until the
/// first listing has come.
fn deleted_line(model: &SidebarModel, line: Line, kit: &Kit) -> El {
    match line {
        Line::Header(s) => deleted_header(&model.sections[s], kit),
        Line::Row(s, r) => deleted_row(&model.sections[s].rows[r], kit),
        _ => muted_line("Loading sessions…", kit),
    }
}

/// A row of the Deleted view: the session's title and, as its detail, how long it has left, or
/// that removing it failed, in words as short as "6 days left" so that the title keeps its
/// room. No mark, no drag, never selected. A press, Return, Space, a
/// secondary press and the Menu key open its menu, "Recover", under it.
fn deleted_row(row: &Row, kit: &Kit) -> El {
    let detail = if row.stuck {
        "Not removed".to_string()
    } else {
        deleted::days(row.days_left.unwrap_or(0))
    };
    let target = MenuTarget::Deleted(row.key);
    SidebarRow::new(row.title.clone())
        .detail(detail)
        .on_select(press_menu(target.clone()))
        .build(kit)
        .on_key(key_menu(target.clone()))
        .on_secondary_press(open_menu(target))
}

/// A group's header in the Deleted view: its name, never folded, with no "+". Its presses and
/// keys open its menu, "Recover group", as a row's do ([`deleted_row`]). Unlike a header of the
/// sessions' list it is a stop for the list's arrow keys: they press what they reach, which here
/// does nothing.
fn deleted_header(section: &Section, kit: &Kit) -> El {
    let target = MenuTarget::DeletedGroup(section.id.clone());
    let name = text(section.name.clone())
        .key("mr-group-name")
        .font(kit.ty.text_xsmall_medium)
        .color(kit.c.text_muted)
        .truncate()
        .shrink()
        .a11y_hidden();
    let header = row()
        .label(section.name.clone())
        .w_full()
        .h(ROW_HEIGHT)
        .pt(4.0)
        .px(INSET)
        .center_y()
        .radius(kit.radius.md)
        .child(name);
    kit.interactive(header.on_press_tx(press_menu(target.clone())))
        .on_key(key_menu(target.clone()))
        .on_secondary_press(open_menu(target))
        .role("button")
        .hover_motion(Anim::INSTANT, Anim::INSTANT)
}

/// A press on a line of the Deleted view opens `target`'s menu. The list's arrow keys press each
/// line they reach: that does nothing.
fn press_menu(target: MenuTarget) -> impl Fn(&mut Ui, &mut Tx) + 'static {
    move |ui: &mut Ui, tx: &mut Tx| {
        if !tx.by_keys() {
            show_menu(ui, target.clone());
        }
    }
}

/// Return or Space on a focused line of the Deleted view opens `target`'s menu, as a press
/// does. The line's own key handler hears them before the list's focus group would press it.
fn key_menu(target: MenuTarget) -> impl Fn(&mut Ui, &KeyEvent, &mut Tx) -> bool + 'static {
    move |ui: &mut Ui, ev: &KeyEvent, tx: &mut Tx| {
        let m = ev.mods;
        let plain = !(m.shift || m.cmd || m.ctrl || m.alt || m.logo);
        let chosen = matches!(&ev.key, Key::Enter) || matches!(&ev.key, Key::Text(t) if t == " ");
        if !plain || !chosen {
            return false;
        }
        if !tx.key_repeat() {
            show_menu(ui, target.clone());
        }
        true
    }
}

/// Opens `target`'s menu with no point: it opens under its line (see [`menu`]).
fn show_menu(ui: &mut Ui, target: MenuTarget) {
    ui.drag = None;
    ui.side_menu = Some(SideMenu {
        target,
        at: PointerPress { x: 0.0, y: 0.0 },
        sub_open: false,
    });
}

#[cfg(test)]
thread_local! {
    /// How many lines of the list were built on this thread: each test runs on its own.
    static LINE_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many lines of the list have been built, rather than reused, on this thread.
#[cfg(test)]
pub(crate) fn line_builds() -> usize {
    LINE_BUILDS.with(std::cell::Cell::get)
}

/// What a row's press does: shows its session in the focused tile, or with Ctrl held (Cmd on
/// macOS) in a new tile right of it while there is room. A click also focuses the session's
/// input box. Moving with the arrow keys, which press each row they reach, leaves the focus in
/// the list, so the next one moves on, always in the focused tile. One that reaches the row of
/// a session in another tile only selects that row ([`Ui::passed_row`]): no tile takes the
/// focus, and the next one moves on in the tile being browsed. (Return and Space never get
/// here: see [`choose`].) The press Rune runs after a drag's release, when it holds the
/// pointer inside the row, does nothing (`drag::row`).
fn select(key: SessionKey) -> impl Fn(&mut Ui, &mut Tx) + 'static {
    move |ui: &mut Ui, tx: &mut Tx| {
        if std::mem::take(&mut ui.swallow_row_press) && !tx.by_keys() {
            return;
        }
        ui.passed_row = None;
        if tx.by_keys() && ui.app.tiles.contains(key) && ui.app.focused() != Some(key) {
            ui.passed_row = Some((key, ui.app.tiles.placement()));
            return;
        }
        let focus = !tx.by_keys();
        let place = folder::side_or_focused(focus && tx.mods().cmd);
        glue::show(ui, tx, key, place, focus);
    }
}

/// Return or Space on a focused row chooses its session, as a click does: it is shown and its
/// input box takes the focus. The row's own key handler hears them before the list's focus
/// group would press the row (`El::on_key`), so they are told apart from the arrows there.
///
/// While a row is dragged the rows' keys do nothing (ruling R8): this takes Return, Space and
/// the keys the focus group would move and press with, before the focus group or the window
/// hear them. Escape goes on to the window, which ends the drag (`view::on_key`).
fn choose(key: SessionKey) -> impl Fn(&mut Ui, &KeyEvent, &mut Tx) -> bool + 'static {
    move |ui: &mut Ui, ev: &KeyEvent, tx: &mut Tx| {
        if ui.drag.is_some() {
            return moves_or_chooses(ev);
        }
        let m = ev.mods;
        let plain = !(m.shift || m.cmd || m.ctrl || m.alt || m.logo);
        let chosen = matches!(&ev.key, Key::Enter) || matches!(&ev.key, Key::Text(t) if t == " ");
        if !plain || !chosen {
            return false;
        }
        if !tx.key_repeat() {
            ui.passed_row = None;
            glue::show(ui, tx, key, Place::Focused, true);
        }
        true
    }
}

/// Whether `ev` is a key a focused row acts on: Return or Space, which choose it, or a key the
/// list's focus group moves with, pressing the row it reaches.
fn moves_or_chooses(ev: &KeyEvent) -> bool {
    matches!(
        &ev.key,
        Key::Enter
            | Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
    ) || matches!(&ev.key, Key::Text(t) if t == " ")
}

/// A secondary press opens `target`'s menu where it landed. Rune calls it for the Menu key and
/// Shift+F10 too, at the bottom-left of the focused element. A row being dragged stops there:
/// the menu takes the pointer, and its release never comes.
fn open_menu(target: MenuTarget) -> impl Fn(&mut Ui, PointerPress, &mut Tx) + 'static {
    move |ui: &mut Ui, at: PointerPress, _: &mut Tx| {
        ui.drag = None;
        ui.side_menu = Some(SideMenu {
            target: target.clone(),
            at,
            sub_open: false,
        });
    }
}

/// A row whose session's name is being edited in it: the field in place of its title, then
/// its folder and its mark, on the fill it has at rest; while it is ready or needs input, on
/// its pair's fill and in its ink, with no mark ([`coloured_row`]). Built from the kit's parts as
/// `SidebarRow::build` builds one, since the kit's own field takes a key of the kit's. It is
/// no stop for the list's arrow keys while the field is there; a secondary press opens its
/// menu.
fn renaming_row(ui: &Ui, session: &Row, kit: &Kit) -> El {
    let c = &kit.c;
    let pair = theme::pair_of(session.mark);
    let mut field = name_field(&ui.rename, ROW_RENAME, "Session name", kit.ty.ui, kit)
        .focus_return("sidebar-list");
    if let Some(pair) = pair {
        field = field.color(pair.ink).caret(pair.ink, kit.selection());
    }
    let fill = match pair {
        Some(pair) => pair.fill,
        None if session.focused => c.pressed,
        None if session.shown => c.hover,
        None => Color::TRANSPARENT,
    };
    row()
        .label(session.title.clone())
        .w_full()
        .h(ROW_HEIGHT)
        .px(INSET)
        .gap(6.0)
        .center_y()
        .radius(kit.radius.md)
        .fill(fill)
        .child(field)
        .children(session.folder_label.as_ref().map(|folder| {
            text(folder.clone())
                .key("mr-row-folder")
                .font(kit.ty.small)
                .color(pair.map_or(c.text_faint, |pair| pair.ink))
                .a11y_hidden()
        }))
        .children(
            pair.is_none()
                .then(|| mark_slot(session.mark, kit))
                .flatten(),
        )
        .on_secondary_press(open_menu(MenuTarget::Row(session.key)))
        .focusable(false)
        .role("listitem")
}

/// The row of a session that is ready or needs input: filled with its pair over the selected
/// and the shown fills, its title and folder in the pair's ink, and nothing in a status slot,
/// the fill saying it. Built from the kit's parts as `SidebarRow::build` builds one, which
/// draws its title in an ink of its own; unlike the kit's row it has no hover, so the colour
/// stays as it is under the pointer. Its description says the state as the kit's does.
///
/// The fill is all of it: the selected one looks as the others do, with no ring or border of
/// its own, the tile's coloured bar saying which session it is. The one that holds the
/// keyboard's focus in the list shows the focus ring every row shows.
fn coloured_row(session: &Row, selected: bool, pair: Pair, kit: &Kit) -> El {
    let state = match session.mark {
        Mark::NeedsYou => "Needs attention",
        _ => "Unread",
    };
    let said: Vec<&str> = [session.folder_label.as_deref(), Some(state)]
        .into_iter()
        .flatten()
        .collect();
    row()
        .label(session.title.clone())
        .description(said.join(", "))
        .w_full()
        .h(ROW_HEIGHT)
        .px(INSET)
        .gap(6.0)
        .center_y()
        .radius(kit.radius.md)
        .motion(kit.motion.control)
        .focusable(true)
        .focus_selected(selected)
        .fill(pair.fill)
        .child(
            text(session.title.clone())
                .key("label")
                .font(kit.ty.ui)
                .color(pair.ink)
                .truncate()
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        )
        .children(session.folder_label.as_ref().map(|folder| {
            text(folder.clone())
                .key("detail")
                .font(kit.ty.small)
                .color(pair.ink)
                .a11y_hidden()
        }))
        .on_press_tx(select(session.key))
        // After the press handler, which would make it a button.
        .role("listitem")
}

/// The kit's status mark for a session's mark: its spinner while the session works. A session
/// that is ready or needs input has a coloured row instead ([`coloured_row`]).
fn status(mark: Mark) -> Option<RowStatus> {
    (mark == Mark::Working).then_some(RowStatus::Busy)
}

/// A group's header: a chevron, its name (or, while it is being renamed, the field), while it
/// is collapsed the strongest mark among its sessions, and a "+" for a new session in it. A
/// press folds or unfolds it, but not while it is being renamed, so a click in the field only
/// moves the caret. A secondary press opens its menu. While a dragged row would drop into its
/// group, it has the kit's selected fill and a border in the accent ([`target_ring`]).
///
/// Built from the kit's parts as `SidebarHeader::build` builds one, so that it can hold a
/// text field. Like the kit's, it is for the pointer only: the list's arrow keys pass over it
/// to the next session's row, and since they press what they reach they would fold groups as
/// they passed.
fn header(ui: &Ui, section: &Section, kit: &Kit) -> El {
    let c = &kit.c;
    let renaming = matches!(&ui.renaming, Some(Renaming::Group(id)) if *id == section.id);
    let glyph = if section.collapsed {
        kit.glyph.triangle_right
    } else {
        kit.glyph.triangle_down
    };
    let name = if renaming {
        name_field(
            &ui.rename,
            RENAME,
            "Group name",
            kit.ty.text_xsmall_medium,
            kit,
        )
        .focus_return("sidebar-list")
    } else {
        text(section.name.clone())
            .key("mr-group-name")
            .font(kit.ty.text_xsmall_medium)
            .color(c.text_muted)
            .truncate()
            .shrink()
    };
    let title = row()
        .key("mr-group-title")
        .gap(4.0)
        .center_y()
        .shrink()
        .child(
            icon(glyph, 12.0)
                .key("mr-disclosure")
                .color(c.icon_faint)
                .a11y_hidden(),
        )
        .child(name);
    let mut header = row()
        .label(section.name.clone())
        .w_full()
        .h(ROW_HEIGHT)
        .pt(4.0)
        .px(INSET)
        .center_y()
        .radius(kit.radius.md);
    let mark = target_mark(ui, &section.id, kit);
    if mark.is_some() {
        header = header.fill(c.pressed);
    }
    // The field fills the header and must be reachable by assistive technology; the name it
    // replaces only repeats the header's label.
    header = if renaming {
        header.child(title.grow(1.0))
    } else {
        header
            .child(title.a11y_hidden())
            .child(el().key("mr-spacer").grow(1.0).a11y_hidden())
    };
    if section.collapsed
        && let Some(mark) = mark_slot(section.mark, kit)
    {
        header = header.child(mark);
    }
    header = header.child(add_button(section, kit));
    if let Some(color) = mark {
        header = header.child(target_ring(color, kit));
    }
    let menu = open_menu(MenuTarget::Group(section.id.clone()));
    if renaming {
        return header.on_secondary_press(menu).focusable(false);
    }
    let id: GroupId = section.id.clone();
    let collapsed = section.collapsed;
    kit.interactive(header.on_press_tx(move |ui: &mut Ui, _: &mut Tx| {
        let effects = ui.app.collapse_group(&id, !collapsed);
        glue::run(ui, effects);
    }))
    .on_secondary_press(menu)
    .role("button")
    .expanded(!collapsed)
    .hover_motion(Anim::INSTANT, Anim::INSTANT)
    .focusable(false)
}

/// The window's one-line field a name is edited in, keyed `key`, for a group's header, a
/// session's row or a tile's header: Return keeps the name, Escape drops the change, and
/// neither does anything else (`glue::finish_rename`). When it loses the focus otherwise,
/// [`rename_watch`] keeps the name. In the sidebar, as the kit's row rename does, it gives the
/// focus back to the list when it goes (`El::focus_return`).
pub(super) fn name_field(
    edit: &TextEdit,
    key: &'static str,
    label: &'static str,
    font: TextStyle,
    kit: &Kit,
) -> El {
    let c = &kit.c;
    text_input(edit, font)
        .single_line()
        .key(key)
        .label(label)
        .focus_selected(true)
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .color(c.text_base)
        .caret(c.text_base, kit.selection())
        .on_edit(|ui: &mut Ui| &mut ui.rename)
        .on_submit(|ui: &mut Ui, _: &mut Tx| glue::finish_rename(ui, true))
        .on_input_key(|ui: &mut Ui, ev: &KeyEvent, _: &mut Tx| {
            if ev.key != Key::Escape {
                return false;
            }
            glue::finish_rename(ui, false);
            true
        })
}

/// The "+" at a header's end: what its menu's "New session" does. It takes its own presses, so
/// it never folds the group.
fn add_button(section: &Section, kit: &Kit) -> El {
    let id = section.id.clone();
    IconButton::new(&PLUS)
        .variant(IconButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .label("New session")
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
            let side = tx.mods().cmd;
            new_session(ui, tx, &id, side);
        })
        .build(kit)
        .key(add_key(&section.id))
        .focusable(false)
}

/// The key of `id`'s "+".
pub fn add_key(id: &GroupId) -> String {
    format!("mr-group-add-{}", id.key_text())
}

/// A new session in group `id`, to the side with `side`: in a folder's group, in that folder at
/// once while it is still a folder (see `folder::start_chosen`); in a custom group, in the
/// folder the folder menu is asked for.
fn new_session(ui: &mut Ui, tx: &mut Tx, id: &GroupId, side: bool) {
    match id {
        GroupId::Folder(folder) => {
            let place = folder::side_or_focused(side);
            folder::start_chosen(ui, tx, folder.clone(), Some(id), place);
        }
        GroupId::Custom(_) => folder::ask(ui, tx, Some(id.clone()), side),
    }
}

/// The colour of a session's mark: the kit's spinner colour while it works, the fill of its
/// pair while it is ready or needs input; none for no mark.
pub(super) fn mark_color(mark: Mark, kit: &Kit) -> Option<Color> {
    match mark {
        Mark::Working => Some(kit.c.icon_muted),
        _ => theme::pair_of(mark).map(|pair| pair.fill),
    }
}

/// A header's mark, keyed `mr-mark`: the kit's spinner while a session works, else a dot in
/// the fill of the mark's pair.
fn mark_slot(mark: Mark, kit: &Kit) -> Option<El> {
    let color = mark_color(mark, kit)?;
    let drawn = match mark {
        Mark::Working => Spinner::new().size(12.0).color(color).build(kit),
        _ => el().size(7.0, 7.0).radius(3.5).fill(color),
    };
    Some(
        row()
            .key("mr-mark")
            .size(20.0, 20.0)
            .center()
            .child(drawn)
            .a11y_hidden(),
    )
}

/// A line of muted text, as wide as a row and as high.
fn muted_line(words: impl Into<String>, kit: &Kit) -> El {
    row().w_full().h(ROW_HEIGHT).px(INSET).center_y().child(
        text(words.into())
            .key("mr-side-line-text")
            .font(kit.ty.small)
            .color(kit.c.text_muted)
            .truncate()
            .shrink(),
    )
}

/// A muted line the pointer presses. Like a header, it is not a stop for the list's arrow keys,
/// which press each row they reach.
fn press_line(words: String, kit: &Kit) -> El {
    pressable(muted_line(words, kit), kit)
}

fn pressable(line: El, kit: &Kit) -> El {
    kit.interactive(line.radius(kit.radius.md))
        .role("button")
        .hover_motion(Anim::INSTANT, Anim::INSTANT)
        .focusable(false)
}

/// "New group", muted, with a plus before it.
fn new_group_line(kit: &Kit) -> El {
    let line = row()
        .label("New group")
        .w_full()
        .h(ROW_HEIGHT)
        .px(INSET)
        .gap(6.0)
        .center_y()
        .child(
            icon(&PLUS, 14.0)
                .key("mr-side-line-icon")
                .color(kit.c.icon_muted)
                .a11y_hidden(),
        )
        .child(
            text("New group")
                .key("mr-side-line-text")
                .font(kit.ty.small)
                .color(kit.c.text_muted)
                .truncate()
                .shrink()
                .a11y_hidden(),
        );
    pressable(line, kit)
}

/// Starts editing a group's name in its header or a session's in its row (`glue::start_rename`),
/// the line scrolled into view first: only the rows near the viewport are built, and a line far
/// off has no field to focus.
fn start_rename(ui: &mut Ui, tx: &mut Tx, what: Renaming) {
    match &what {
        Renaming::Group(id) => tx.scroll_into_view(header_key(id)),
        Renaming::Row(key) => tx.scroll_into_view(format!("mr-row-{}", key.0)),
        Renaming::Header(_) => {}
    }
    glue::start_rename(ui, what);
}

/// Shuts the menu as one of its items is chosen. The focus left the field of the name being
/// edited when the menu opened: the name typed so far is kept now, before the item changes the
/// groups or starts another rename.
fn chosen(ui: &mut Ui) {
    ui.side_menu = None;
    glue::end_rename(ui, true);
}

/// While a name is edited, in a group's header, a session's row or a tile's header, in a field
/// that has lost the focus otherwise than to the sidebar's menu (a click elsewhere, Tab, F6),
/// the name typed so far is kept.
///
/// Rune has no focus-change callback, but it keeps the field's `TextEdit::focused` up to date
/// on each commit, and the view runs again when it changes. So this puts into `layer`, only
/// while that is so, an empty element that keeps the name once it is laid out.
pub fn rename_watch(ui: &Ui, layer: &mut Vec<El>) {
    if ui.renaming.is_none() || ui.rename.focused || ui.side_menu.is_some() {
        return;
    }
    layer.push(
        el().key("mr-rename-lost")
            .absolute(0.0, 0.0)
            .size(0.0, 0.0)
            .on_layout(|ui: &mut Ui, _| glue::end_rename(ui, true)),
    );
}

/// The sidebar's menu, while one is open, into `layer`: one kit `Menu` placed at the point it
/// was opened at, with no trigger. A press on the "Hidden groups" line gives no point, so that
/// menu opens under the line; so do the menus of the Deleted view, however they were opened.
pub fn menu(ui: &Ui, cx: &mut Cx<Ui>, kit: &Kit, layer: &mut Vec<El>) {
    let Some(open) = &ui.side_menu else {
        return;
    };
    let at = (open.at.x, open.at.y);
    let under = |key: &str| {
        cx.window_layout_with(key, |r| (r[0], r[1] + r[3]))
            .unwrap_or(at)
    };
    let (x, y) = match &open.target {
        MenuTarget::HiddenGroups => under("mr-hidden-groups"),
        MenuTarget::Deleted(key) => under(&format!("mr-row-{}", key.0)),
        MenuTarget::DeletedGroup(id) => under(&header_key(id)),
        MenuTarget::Row(_) | MenuTarget::Group(_) => at,
    };
    let menu = Menu::new(MENU)
        .at(x, y)
        .open(true)
        .on_close(|ui: &mut Ui| ui.side_menu = None);
    let menu = match &open.target {
        MenuTarget::Row(key) => row_menu(ui, menu, *key, open.sub_open),
        MenuTarget::Group(id) => group_menu(ui, menu, id),
        MenuTarget::HiddenGroups => hidden_menu(ui, menu),
        MenuTarget::Deleted(key) => recover_menu(menu, *key),
        MenuTarget::DeletedGroup(id) => recover_group_menu(menu, id),
    };
    // What it hands back is its trigger's empty wrapper: nothing to draw.
    if let Some(menu) = menu {
        drop(menu.build(kit, layer));
    }
}

/// A row's menu: "Move to group", whose submenu lists every group but the row's own, in the
/// sidebar's order and the hidden ones after them, then "New group…"; "Open to the side",
/// which cannot be chosen while there is no room for a tile or the session is in one already;
/// "Open below", which opens it under the focused tile and cannot be chosen while the session
/// is in a tile, no tile is focused or the focused tile's column could not take another;
/// "Rename", which edits the session's name in its row; and, after a separator, "Delete", which
/// moves it to the Deleted view at once, or asks first when that would cut something off
/// (`glue::ask_delete_session`).
fn row_menu(ui: &Ui, menu: Menu, key: SessionKey, sub_open: bool) -> Option<Menu> {
    let entry = ui.app.sessions.get(key)?;
    let own = ui.app.groups.group_of(entry);
    let model = &ui.app.sidebar;
    let listed = model.sections.iter().map(|s| (&s.id, &s.name));
    let hidden = model.hidden.iter().map(|(id, name)| (id, name));
    let mut groups: Vec<MenuEntry> = listed
        .chain(hidden)
        .filter(|(id, _)| **id != own)
        .map(|(id, name)| {
            let id = id.clone();
            MenuEntry::item(MenuItem::new(name.clone()).on_select(move |ui: &mut Ui| {
                chosen(ui);
                let effects = ui.app.move_session(key, &id);
                glue::run(ui, effects);
            }))
        })
        .collect();
    groups.push(MenuEntry::Separator);
    groups.push(MenuEntry::item(MenuItem::new("New group…").on_select_tx(
        move |ui: &mut Ui, tx: &mut Tx| {
            chosen(ui);
            let (id, mut effects) = ui.app.new_group("");
            // Moving it saves again, with the new group.
            effects.retain(|effect| !matches!(effect, Effect::SaveState(_)));
            effects.extend(ui.app.move_session(key, &id));
            glue::run(ui, effects);
            start_rename(ui, tx, Renaming::Group(id));
        },
    )));
    let move_to = MenuItem::new("Move to group")
        .submenu(groups, sub_open)
        .on_select_tx(|ui: &mut Ui, tx: &mut Tx| {
            let Some(menu) = &mut ui.side_menu else {
                return;
            };
            menu.sub_open = !menu.sub_open;
            // The kit moves nothing into a submenu: from the keyboard, the focus goes to its
            // first group, as Right arrow would in a native menu. Its surface is mounted after
            // this handler, and a focus asked for here is found in the tree that has it.
            if menu.sub_open && tx.by_keys() {
                tx.focus(SUBMENU, tx.focus_visible());
            }
        });
    let side = MenuItem::new("Open to the side")
        .disabled(!ui.app.room_for_tile() || ui.app.tiles.contains(key))
        .on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
            chosen(ui);
            glue::show(ui, tx, key, Place::Side, true);
        });
    let under = |focused: SessionKey| ui.app.can_put(Spot::Below(focused));
    let below = MenuItem::new("Open below")
        .disabled(ui.app.tiles.contains(key) || !ui.app.focused().is_some_and(under))
        .on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
            chosen(ui);
            if let Some(focused) = ui.app.focused() {
                glue::show(ui, tx, key, Place::Below(focused), true);
            }
        });
    let rename = MenuItem::new("Rename").on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
        chosen(ui);
        start_rename(ui, tx, Renaming::Row(key));
    });
    let delete =
        MenuItem::new("Delete")
            .danger(true)
            .on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
                chosen(ui);
                glue::ask_delete_session(ui, tx, key);
            });
    Some(
        menu.item(move_to)
            .item(side)
            .item(below)
            .item(rename)
            .separator()
            .item(delete),
    )
}

/// A header's menu: "New session", "Rename", "Hide group"; for a group the user made "Ungroup",
/// which lets the group go and sends its sessions home; and, after a separator, "Delete group",
/// which asks before the group and every session of it go to the Deleted view
/// (`glue::ask_delete_group`), for a group the user made or one that holds a session. It is
/// greyed out until the full list of sessions is in (`SidebarModel::complete`): before that the
/// question's count would not be the whole truth.
fn group_menu(ui: &Ui, menu: Menu, id: &GroupId) -> Option<Menu> {
    let section = ui.app.sidebar.sections.iter().find(|s| s.id == *id)?;
    let new_session = {
        let id = id.clone();
        MenuItem::new("New session").on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
            chosen(ui);
            let side = tx.mods().cmd;
            self::new_session(ui, tx, &id, side);
        })
    };
    let rename = {
        let id = id.clone();
        MenuItem::new("Rename").on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
            chosen(ui);
            start_rename(ui, tx, Renaming::Group(id.clone()));
        })
    };
    let hide = {
        let id = id.clone();
        MenuItem::new("Hide group").on_select(move |ui: &mut Ui| {
            chosen(ui);
            let effects = ui.app.hide_group(&id, true);
            glue::run(ui, effects);
        })
    };
    let mut menu = menu.item(new_session).item(rename).item(hide);
    if section.custom {
        let id = id.clone();
        menu = menu.item(MenuItem::new("Ungroup").on_select(move |ui: &mut Ui| {
            // A name typed for the group that goes goes with it.
            if ui.renaming == Some(Renaming::Group(id.clone())) {
                glue::end_rename(ui, false);
            }
            chosen(ui);
            let effects = ui.app.ungroup(&id);
            glue::run(ui, effects);
        }));
    }
    if section.custom || !ui.app.sessions_of(id).is_empty() {
        let id = id.clone();
        let delete = MenuItem::new("Delete group")
            .danger(true)
            .disabled(!ui.app.sidebar.complete)
            .on_select(move |ui: &mut Ui| {
                chosen(ui);
                glue::ask_delete_group(ui, id.clone());
            });
        menu = menu.separator().item(delete);
    }
    Some(menu)
}

/// The menu of a row in the Deleted view: "Recover", which puts the session back in the list,
/// and its group with it when that was deleted. The Deleted view still shows.
fn recover_menu(menu: Menu, key: SessionKey) -> Option<Menu> {
    Some(
        menu.item(MenuItem::new("Recover").on_select(move |ui: &mut Ui| {
            chosen(ui);
            let effects = ui.app.recover_session(key);
            glue::run(ui, effects);
        })),
    )
}

/// The menu of a group's header in the Deleted view: "Recover group", which puts the group back
/// with every deleted session of it. The Deleted view still shows.
fn recover_group_menu(menu: Menu, id: &GroupId) -> Option<Menu> {
    let id = id.clone();
    Some(menu.item(
        MenuItem::new("Recover group").on_select(move |ui: &mut Ui| {
            chosen(ui);
            let effects = ui.app.recover_group(&id);
            glue::run(ui, effects);
        }),
    ))
}

/// The "Hidden groups" menu: one item per hidden group, which shows it again.
fn hidden_menu(ui: &Ui, menu: Menu) -> Option<Menu> {
    let hidden = &ui.app.sidebar.hidden;
    if hidden.is_empty() {
        return None;
    }
    Some(hidden.iter().fold(menu, |menu, (id, name)| {
        let id = id.clone();
        menu.item(MenuItem::new(name.clone()).on_select(move |ui: &mut Ui| {
            chosen(ui);
            let effects = ui.app.hide_group(&id, false);
            glue::run(ui, effects);
        }))
    }))
}

#[cfg(test)]
mod tests {
    use mr_agent::{
        FakeStep, RequestId, SessionCommand, SessionEvent, SessionId, StoredSession, ToolCallId,
        ToolDetail, UserRequest, UserRequestKind,
    };
    use mr_core::{FromBackend, GroupId, Line, SessionKey};

    use super::line_key;
    use rune::{Color, Key, Mods, PhysicalInput, Runtime};

    use super::super::conversation;
    use crate::demo::{self, RUNE_CLAUDE, SKETCHES};
    use crate::glue::{self, MenuTarget, Renaming, Ui};
    use crate::testkit::{
        self, FINISHED, GARDEN, NEEDS_YOU, WEBSITE, WORKING, box_focused, columns, group_named,
        group_of, tile_keys, titled,
    };
    use crate::theme;

    fn row(key: SessionKey) -> String {
        format!("mr-row-{}", key.0)
    }

    fn group(folder: &str) -> String {
        format!("mr-group-{}", GroupId::Folder(folder.into()).key_text())
    }

    /// The texts the list shows, top to bottom.
    fn list_texts(rt: &Runtime<Ui>) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.visible[2] > 0.0 && e.visible[3] > 0.0)
            .filter(|e| e.path.contains("/mr-sidebar-rows/"))
            .filter_map(|e| e.text)
            .collect()
    }

    /// The texts inside the line keyed `key`.
    fn texts_in(rt: &Runtime<Ui>, key: &str) -> Vec<String> {
        let inside = format!("/{key}/");
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains(&inside))
            .filter_map(|e| e.text)
            .collect()
    }

    /// The mark the line keyed `key` shows: "working" (the kit's spinner in a row's status slot
    /// or the app's `mr-mark` of a header), "needs you" (orange) or "finished" (green), a row by
    /// its fill and a header by its mark's dot. A fill comes in on a spring: settle the window
    /// first.
    fn mark(rt: &Runtime<Ui>, key: &str) -> Option<&'static str> {
        let inside = format!("/{key}/");
        let parts: Vec<_> = rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting)
            .filter(|e| {
                e.key.as_deref() == Some(key)
                    || e.path.contains(&inside)
                        && (e.path.contains("/status-mark/") || e.path.contains("/mr-mark/"))
            })
            .collect();
        if parts.iter().any(|e| e.role == "progressbar") {
            return Some("working");
        }
        parts.iter().find_map(|e| {
            if e.fill == theme::needs_input().fill {
                Some("needs you")
            } else if e.fill == theme::ready().fill {
                Some("finished")
            } else {
                None
            }
        })
    }

    /// The fill of the line keyed `key`, once the window has settled.
    fn fill(rt: &mut Runtime<Ui>, key: &str) -> Color {
        rt.settle_all();
        testkit::step(rt);
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key))
            .expect("the line")
            .fill
    }

    fn selected(rt: &Runtime<Ui>, key: &str) -> bool {
        rt.elements()
            .iter()
            .any(|e| !e.exiting && e.key.as_deref() == Some(key) && e.item_selected)
    }

    fn focused_key(rt: &Runtime<Ui>) -> Option<String> {
        rt.elements()
            .into_iter()
            .find(|e| e.focused && !e.exiting)
            .and_then(|e| e.key)
    }

    fn input_focused(rt: &Runtime<Ui>) -> bool {
        rt.elements()
            .iter()
            .any(|e| e.focused && e.path.contains("/mr-input/"))
    }

    /// What `key`'s session was sent from `from` on, among every command the fake received.
    fn sent_to(rt: &Runtime<Ui>, key: SessionKey, from: usize) -> Vec<SessionCommand> {
        let id = rt.state.app.sessions.get(key).unwrap().session.id.clone();
        let fake = rt.state.fake.as_ref().unwrap();
        fake.sent()[from..]
            .iter()
            .filter(|(to, _)| Some(to) == id.as_ref())
            .map(|(_, command)| command.clone())
            .collect()
    }

    fn sent_count(rt: &Runtime<Ui>) -> usize {
        rt.state.fake.as_ref().unwrap().sent().len()
    }

    #[test]
    fn groups_and_their_sessions_are_listed() {
        let rt = testkit::window("sessions");
        let model = &rt.state.app.sidebar;
        let mut names = Vec::new();
        for line in &model.lines {
            match *line {
                Line::Header(s) => names.push(model.sections[s].name.clone()),
                Line::Row(s, r) => names.push(model.sections[s].rows[r].title.clone()),
                _ => {}
            }
        }
        let texts = list_texts(&rt);
        let in_order: Vec<String> = texts
            .iter()
            .filter(|t| names.contains(t))
            .cloned()
            .collect();
        assert_eq!(in_order, names, "in the model's order");
        assert_eq!(
            texts,
            [
                "rune_claude",
                FINISHED,
                NEEDS_YOU,
                WORKING,
                "sketches",
                "Side projects",
                GARDEN,
                "garden",
                "website",
                "Fixing the contact form",
                "Show older (4)",
                "New group",
                "Older groups (2)",
            ]
        );
    }

    #[test]
    fn a_row_shows_its_sessions_mark() {
        let rt = testkit::window("sessions");
        assert_eq!(mark(&rt, &row(titled(&rt, FINISHED))), Some("finished"));
        assert_eq!(mark(&rt, &row(titled(&rt, NEEDS_YOU))), Some("needs you"));
        assert_eq!(mark(&rt, &row(titled(&rt, WORKING))), Some("working"));
        assert_eq!(mark(&rt, &row(titled(&rt, GARDEN))), None, "shown and idle");
        let resting = titled(&rt, "Fixing the contact form");
        assert_eq!(mark(&rt, &row(resting)), None, "resting and seen");
    }

    #[test]
    fn a_question_out_of_sight_marks_its_row() {
        let mut rt = testkit::window("elsewhere");
        let asked = testkit::focused(&rt).expect("the parser is shown");
        let other = testkit::add_session(&mut rt, "/home/alex/notes");
        assert_eq!(testkit::focused(&rt), Some(other));
        assert_eq!(mark(&rt, &row(asked)), Some("needs you"));
        testkit::show(&mut rt, asked);
        testkit::click(&mut rt, "mr-elsewhere-cancel");
        testkit::step(&mut rt);
        assert_eq!(mark(&rt, &row(asked)), None, "answered, the mark goes");
    }

    #[test]
    fn clicking_a_row_shows_that_session() {
        let mut rt = testkit::window("sessions");
        let garden = titled(&rt, GARDEN);
        assert!(selected(&rt, &row(garden)));
        let asking = titled(&rt, NEEDS_YOU);
        assert!(!testkit::has_text(&rt, "Run the window tests"));
        testkit::click(&mut rt, &row(asking));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(asking));
        assert!(testkit::has_text(&rt, "Run the window tests"));
        assert!(!testkit::has_text(&rt, "Which beds get the most sun"));
        assert!(selected(&rt, &row(asking)));
        assert!(!selected(&rt, &row(garden)));
        assert!(input_focused(&rt), "a click focuses its input box");
    }

    #[test]
    fn a_finished_mark_goes_when_the_session_is_shown() {
        let mut rt = testkit::window("sessions");
        let finished = titled(&rt, FINISHED);
        assert_eq!(mark(&rt, &row(finished)), Some("finished"));
        testkit::click(&mut rt, &row(finished));
        testkit::step(&mut rt);
        assert_eq!(mark(&rt, &row(finished)), None);
        let garden = row(titled(&rt, GARDEN));
        testkit::click(&mut rt, &garden);
        testkit::step(&mut rt);
        assert_eq!(mark(&rt, &row(finished)), None, "seen, it stays unmarked");
    }

    #[test]
    fn a_turn_in_a_hidden_session_changes_only_its_row() {
        let mut rt = testkit::window("sessions");
        rt.settle_all();
        testkit::step(&mut rt);
        let finished = titled(&rt, FINISHED);
        let before = conversation::all_row_builds();
        let shown = testkit::texts(&rt)
            .into_iter()
            .filter(|t| !list_texts(&rt).contains(t))
            .collect::<Vec<_>>();
        testkit::emit_to(&mut rt, finished, SessionEvent::TurnStarted);
        testkit::emit_to(
            &mut rt,
            finished,
            SessionEvent::TextDelta("Out of sight".into()),
        );
        assert_eq!(mark(&rt, &row(finished)), Some("working"));
        assert!(!testkit::has_text(&rt, "Out of sight"));
        assert!(testkit::has_text(&rt, "Which beds get the most sun"));
        assert_eq!(conversation::all_row_builds(), before, "no row built again");
        let after = testkit::texts(&rt)
            .into_iter()
            .filter(|t| !list_texts(&rt).contains(t))
            .collect::<Vec<_>>();
        assert_eq!(after, shown, "the conversation on screen is unchanged");
    }

    /// Claude asks `key`'s session whether it may run `ls`, as request r1.
    fn ask_to_list(rt: &mut Runtime<Ui>, key: SessionKey) {
        testkit::emit_to(rt, key, SessionEvent::TurnStarted);
        let detail = ToolDetail::Command {
            command: "ls".into(),
        };
        testkit::emit_to(
            rt,
            key,
            SessionEvent::ToolStarted {
                id: ToolCallId("call-r1".into()),
                name: "Bash".into(),
                detail: detail.clone(),
            },
        );
        testkit::emit_to(
            rt,
            key,
            SessionEvent::NeedsUser(UserRequest {
                id: RequestId("r1".into()),
                kind: UserRequestKind::Permission {
                    tool: "Bash".into(),
                    detail,
                    always_allow_label: Some("Always allow Bash(ls) in this project".into()),
                },
            }),
        );
    }

    #[test]
    fn a_request_in_a_hidden_session_waits_for_the_user() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::show(&mut rt, a);
        let from = sent_count(&rt);
        ask_to_list(&mut rt, b);
        // The mark's colour comes in on a spring.
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(mark(&rt, &row(b)), Some("needs you"));
        assert!(rt.find("mr-allow-r1").is_none(), "no card on screen");
        assert!(input_focused(&rt));
        rt.state
            .fake
            .as_ref()
            .unwrap()
            .reply(vec![FakeStep::at(0.0, SessionEvent::TurnStarted)]);
        testkit::type_text(&mut rt, "Go on");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(sent_to(&rt, b, from), vec![], "nothing goes to b");
        assert!(
            sent_to(&rt, a, from)
                .iter()
                .any(|c| matches!(c, SessionCommand::Send { .. })),
            "the message went to a"
        );
        testkit::click(&mut rt, &row(b));
        testkit::step(&mut rt);
        for button in ["mr-allow-r1", "mr-always-r1", "mr-deny-r1"] {
            assert!(rt.find(button).is_some(), "{button}");
        }
        assert_eq!(sent_to(&rt, b, from), vec![]);
    }

    #[test]
    fn escape_stops_nothing_in_a_hidden_session() {
        let mut rt = testkit::window("empty");
        let a = testkit::add_session(&mut rt, "/home/alex/a");
        let b = testkit::add_session(&mut rt, "/home/alex/b");
        testkit::emit_to(&mut rt, b, SessionEvent::TurnStarted);
        testkit::show(&mut rt, a);
        let from = sent_count(&rt);
        // In a's input box, then with nothing focused.
        testkit::press(&mut rt, Key::Escape);
        rt.update(|_, tx| tx.blur());
        testkit::step(&mut rt);
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert!(!sent_to(&rt, b, from).contains(&SessionCommand::Stop));
        assert_eq!(sent_to(&rt, a, from), vec![], "a had no turn to stop");
        let phase = &rt.state.app.sessions.get(b).unwrap().session.phase;
        assert_eq!(*phase, mr_core::Phase::Working);
    }

    #[test]
    fn folding_a_group_hides_its_rows_and_shows_its_mark() {
        let mut rt = testkit::window("sessions");
        let header = group(RUNE_CLAUDE);
        let rows: Vec<String> = [FINISHED, NEEDS_YOU, WORKING]
            .iter()
            .map(|title| row(titled(&rt, title)))
            .collect();
        assert_eq!(mark(&rt, &header), None, "open, the rows show the marks");
        assert_eq!(mark(&rt, &group(SKETCHES)), Some("working"));
        assert!(!list_texts(&rt).contains(&"Redrawing the logo".to_string()));
        testkit::click(&mut rt, &header);
        testkit::step(&mut rt);
        for key in &rows {
            assert!(rt.find(key).is_none(), "{key} folded away");
        }
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(mark(&rt, &header), Some("needs you"), "the strongest");
        testkit::click(&mut rt, &header);
        testkit::step(&mut rt);
        for key in &rows {
            assert!(rt.find(key).is_some(), "{key} back");
        }
        assert_eq!(mark(&rt, &header), None);
    }

    #[test]
    fn a_row_in_another_group_names_its_folder() {
        let rt = testkit::window("sessions");
        assert_eq!(
            texts_in(&rt, &row(titled(&rt, GARDEN))),
            vec![GARDEN.to_string(), "garden".to_string()]
        );
        assert_eq!(
            texts_in(&rt, &row(titled(&rt, WORKING))),
            vec![WORKING.to_string()],
            "in its own folder's group"
        );
    }

    #[test]
    fn show_older_lists_the_rest() {
        let mut rt = testkit::window("sessions");
        let older = [
            "Moving to a static host",
            "Adding a dark theme",
            "Compressing the photos",
            "Writing the about page",
        ];
        for title in older {
            assert!(!testkit::has_text(&rt, title), "{title}");
        }
        let key = format!("mr-older-{}", GroupId::Folder(WEBSITE.into()).key_text());
        testkit::click(&mut rt, &key);
        testkit::step(&mut rt);
        let texts = list_texts(&rt);
        let at = texts.iter().position(|t| t == "website").unwrap();
        assert_eq!(
            texts[at + 1..at + 6],
            [
                "Fixing the contact form",
                older[0],
                older[1],
                older[2],
                older[3]
            ]
        );
        assert!(!testkit::has_text(&rt, "Show older"));
        assert!(rt.find(&key).is_none());
    }

    #[test]
    fn older_groups_can_be_shown_and_hidden() {
        let mut rt = testkit::window("sessions");
        assert!(!list_texts(&rt).contains(&"recipes".to_string()));
        testkit::click(&mut rt, "mr-older-groups");
        testkit::step(&mut rt);
        let texts = list_texts(&rt);
        for name in ["recipes", "thesis", "Hide older groups"] {
            assert!(texts.contains(&name.to_string()), "{name}: {texts:?}");
        }
        assert!(!texts.contains(&"Older groups (2)".to_string()));
        testkit::click(&mut rt, "mr-older-groups");
        testkit::step(&mut rt);
        let texts = list_texts(&rt);
        assert!(!texts.contains(&"recipes".to_string()));
        assert!(texts.contains(&"Older groups (2)".to_string()));
    }

    #[test]
    fn a_long_list_builds_only_what_is_on_screen() {
        let mut rt = testkit::window("empty");
        testkit::long_list(&mut rt, 2_000);
        let model = &rt.state.app.sidebar;
        let rows: usize = model.sections.iter().map(|s| s.rows.len()).sum();
        assert!(rows > 2_000, "every session has its row in the model");
        let built = rt
            .elements()
            .iter()
            .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("mr-row-")))
            .count();
        assert!(built > 0 && built < 200, "{built} rows built");
        assert!(testkit::has_text(&rt, "Archived session 0"));
    }

    #[test]
    fn arrow_keys_move_through_the_rows() {
        let mut rt = testkit::window("sessions");
        let [finished, asking, working, garden] =
            [FINISHED, NEEDS_YOU, WORKING, GARDEN].map(|title| titled(&rt, title));
        testkit::click(&mut rt, &row(finished));
        testkit::step(&mut rt);
        assert!(input_focused(&rt), "a click focuses the input box");
        // As Tab into the list does: it lands on the selected row.
        rt.update(|_, tx| tx.focus(row(finished), true));
        testkit::step(&mut rt);
        assert_eq!(focused_key(&rt), Some(row(finished)));
        for next in [asking, working, garden] {
            testkit::press(&mut rt, Key::Down);
            testkit::step(&mut rt);
            assert_eq!(testkit::focused(&rt), Some(next));
            assert_eq!(
                focused_key(&rt),
                Some(row(next)),
                "the focus stays in the list"
            );
            assert!(!input_focused(&rt));
        }
        testkit::press(&mut rt, Key::Up);
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(working), "over the headers");
        assert!(selected(&rt, &row(working)));
    }

    #[test]
    fn return_or_space_on_a_row_moves_into_its_input_box() {
        let mut rt = testkit::window("sessions");
        let [finished, asking, working] =
            [FINISHED, NEEDS_YOU, WORKING].map(|title| titled(&rt, title));
        for (key, chosen) in [(Key::Enter, working), (Key::Text(" ".into()), asking)] {
            testkit::click(&mut rt, &row(finished));
            testkit::step(&mut rt);
            rt.update(|_, tx| tx.focus(row(finished), true));
            testkit::step(&mut rt);
            let from = sent_count(&rt);
            let downs = if chosen == working { 2 } else { 1 };
            for _ in 0..downs {
                testkit::press(&mut rt, Key::Down);
            }
            testkit::step(&mut rt);
            assert_eq!(testkit::focused(&rt), Some(chosen));
            assert!(!input_focused(&rt), "moving leaves the focus in the list");
            testkit::press(&mut rt, key.clone());
            testkit::step(&mut rt);
            assert_eq!(testkit::focused(&rt), Some(chosen), "{key:?}");
            assert!(input_focused(&rt), "{key:?} moves into the input box");
            assert!(
                glue::view_of(&rt.state, chosen).draft.text.is_empty(),
                "{key:?}"
            );
            assert_eq!(sent_count(&rt), from, "{key:?} sends nothing");
        }
    }

    /// The texts shown inside the shown session's row.
    fn shown_row_texts(rt: &Runtime<Ui>) -> Vec<String> {
        let key = testkit::focused(rt).expect("a session is shown");
        texts_in(rt, &row(key))
    }

    #[test]
    fn the_session_row_shows_the_sessions_title() {
        let mut rt = testkit::window("empty");
        assert_eq!(shown_row_texts(&rt), vec!["New session".to_string()]);
        testkit::emit(&mut rt, SessionEvent::TitleChanged("Fix the parser".into()));
        assert_eq!(shown_row_texts(&rt), vec!["Fix the parser".to_string()]);
    }

    #[test]
    fn the_row_shows_the_first_message_at_once_and_then_the_title() {
        let mut rt = testkit::window("empty");
        let fake = rt.state.fake.clone().unwrap();
        fake.title_reply(vec![FakeStep::at(
            1.0,
            SessionEvent::TitleChanged("Listing the folder".into()),
        )]);
        testkit::type_text(&mut rt, "  what is   in this folder?");
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        testkit::press_with(&mut rt, Key::Enter, shift);
        testkit::type_text(&mut rt, "and why");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        // Only the row has it with one space: the message keeps what was typed.
        assert_eq!(
            shown_row_texts(&rt),
            vec!["what is in this folder?".to_string()]
        );
        let asked: Vec<_> = testkit::sent(&rt)
            .into_iter()
            .filter(|c| matches!(c, SessionCommand::SuggestTitle { .. }))
            .collect();
        assert_eq!(asked.len(), 1);
        demo::pump(&mut rt.state, 1.0);
        rt.invalidate();
        testkit::step(&mut rt);
        assert_eq!(shown_row_texts(&rt), vec!["Listing the folder".to_string()]);
    }

    #[test]
    fn every_demo_scene_with_a_conversation_has_a_title_in_its_row() {
        for scene in [
            "conversation",
            "streaming",
            "long",
            "tools",
            "working",
            "ended",
            "permission",
            "permission-plain",
            "permission-long",
            "permission-edit",
            "question",
            "plan",
            "sessions",
        ] {
            let rt = testkit::window(scene);
            let session = rt.state.focused_session().unwrap();
            let title = session
                .title
                .clone()
                .unwrap_or_else(|| panic!("{scene}: no title"));
            assert!(
                shown_row_texts(&rt).contains(&title),
                "{scene}: {title:?} not in the row"
            );
            let first = session
                .conversation
                .items()
                .iter()
                .find_map(|i| match &i.body {
                    mr_core::ItemBody::User { text, .. } => Some(text.clone()),
                    _ => None,
                });
            if let Some(first) = first {
                assert!(
                    !first.starts_with(&title),
                    "{scene}: the agent's title, not the stand-in"
                );
            }
        }
    }

    // Menus: Task 11.

    /// The texts inside the sidebar's menu, or inside its submenu.
    const MENU: &str = "mr-side-menu-menu";
    const SUBMENU: &str = "mr-side-menu-item0-sub-menu";

    /// A row's menu, as it reads.
    const ROW_MENU: [&str; 5] = [
        "Move to group",
        "Open to the side",
        "Open below",
        "Rename",
        "Delete",
    ];

    /// Right-clicks the middle of the element keyed `key`, then steps. Rows that moved are
    /// landed first, so the click hits where `key` is laid out.
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

    /// The surface keyed `key` is on screen and not on its way out.
    fn open(rt: &Runtime<Ui>, key: &str) -> bool {
        rt.elements()
            .iter()
            .any(|e| !e.exiting && e.key.as_deref() == Some(key))
    }

    /// The rect of the element keyed `key`.
    fn rect(rt: &Runtime<Ui>, key: &str) -> [f32; 4] {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no element keyed {key:?}"))
            .rect
    }

    fn header_of(id: &GroupId) -> String {
        format!("mr-group-{}", id.key_text())
    }

    /// The group whose name is being edited.
    fn renamed_group(rt: &Runtime<Ui>) -> GroupId {
        match &rt.state.renaming {
            Some(Renaming::Group(id)) => id.clone(),
            other => panic!("renaming no group: {other:?}"),
        }
    }

    /// Something was written under the state key since it was removed.
    fn saved(rt: &Runtime<Ui>) -> bool {
        rt.state.prefs.contains(glue::STATE_KEY)
    }

    /// Opens the menu of the header keyed `header` and chooses its item `i`.
    fn header_item(rt: &mut Runtime<Ui>, header: &str, i: usize) {
        right_click(rt, header);
        assert!(open(rt, MENU), "{header}'s menu");
        testkit::click(rt, &format!("mr-side-menu-item{i}"));
        testkit::step(rt);
    }

    #[test]
    fn a_right_click_on_a_row_opens_its_menu() {
        let mut rt = testkit::window("sessions");
        let working = titled(&rt, WORKING);
        let shown = testkit::focused(&rt);
        let version = rt.state.app.sidebar_version;
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        right_click(&mut rt, &row(working));
        assert_eq!(
            rt.state.side_menu.as_ref().map(|m| m.target.clone()),
            Some(MenuTarget::Row(working))
        );
        assert_eq!(texts_in(&rt, MENU), ROW_MENU);
        let focused = focused_key(&rt).unwrap_or_default();
        assert!(
            focused.starts_with("mr-side-menu"),
            "the menu holds the focus: {focused}"
        );
        let [x, y, w, h] = rect(&rt, MENU);
        assert!(x >= 0.0 && y >= 0.0 && x + w <= 1200.0 && y + h <= 800.0);
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        assert!(!open(&rt, MENU));
        assert_eq!(rt.state.app.sidebar_version, version, "nothing changed");
        assert_eq!(testkit::focused(&rt), shown);
        assert!(!saved(&rt));
    }

    #[test]
    fn move_to_group_lists_the_other_groups() {
        let mut rt = testkit::window("sessions");
        let working = titled(&rt, WORKING);
        // A hidden group is offered too, after the others.
        let ui = &mut rt.state;
        let effects = ui.app.hide_group(&GroupId::Folder(WEBSITE.into()), true);
        glue::run(ui, effects);
        rt.invalidate();
        testkit::step(&mut rt);
        assert!(!list_texts(&rt).contains(&"website".to_string()));
        right_click(&mut rt, &row(working));
        assert!(!open(&rt, SUBMENU), "closed until chosen");
        testkit::click(&mut rt, "mr-side-menu-item0");
        testkit::step(&mut rt);
        assert_eq!(
            texts_in(&rt, SUBMENU),
            ["sketches", "Side projects", "website", "New group…"]
        );
        let item = rect(&rt, "mr-side-menu-item0");
        let sub = rect(&rt, SUBMENU);
        assert!(
            sub[0] >= item[0] + item[2] - 0.5,
            "beside the item, not over it"
        );
    }

    #[test]
    fn choosing_a_group_moves_the_session() {
        let mut rt = testkit::window("sessions");
        let working = titled(&rt, WORKING);
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        right_click(&mut rt, &row(working));
        testkit::click(&mut rt, "mr-side-menu-item0");
        testkit::step(&mut rt);
        // sketches, Side projects, website.
        testkit::click(&mut rt, "mr-side-menu-item0-sub-item2");
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        assert!(!open(&rt, MENU) && !open(&rt, SUBMENU));
        assert_eq!(group_of(&rt, working), GroupId::Folder(WEBSITE.into()));
        let texts = list_texts(&rt);
        let at = texts.iter().position(|t| t == "website").unwrap();
        assert_eq!(texts[at + 1..at + 3], [WORKING, "rune_claude"], "{texts:?}");
        assert_eq!(
            texts_in(&rt, &row(working)),
            [WORKING.to_string(), "rune_claude".to_string()]
        );
        assert!(saved(&rt), "a state was saved");
    }

    #[test]
    fn new_group_from_a_row_moves_it_and_starts_renaming() {
        let mut rt = testkit::window("sessions");
        let working = titled(&rt, WORKING);
        right_click(&mut rt, &row(working));
        testkit::click(&mut rt, "mr-side-menu-item0");
        testkit::step(&mut rt);
        // Three groups and a separator before it.
        testkit::click(&mut rt, "mr-side-menu-item0-sub-item4");
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        let id = renamed_group(&rt);
        assert!(matches!(id, GroupId::Custom(_)));
        assert_eq!(group_of(&rt, working), id, "the session moved into it");
        let rename = &rt.state.rename;
        assert_eq!(rename.text, "New group");
        assert_eq!(
            (rename.anchor, rename.focus),
            (0, "New group".len()),
            "all selected"
        );
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-group-rename"));
        testkit::type_text(&mut rt, "Errands");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(texts_in(&rt, &header_of(&id)), ["Errands"]);
        let texts = list_texts(&rt);
        let at = texts.iter().position(|t| t == "Errands").unwrap();
        assert_eq!(texts[at + 1..at + 3], [WORKING, "rune_claude"], "{texts:?}");
    }

    #[test]
    fn a_group_is_renamed_from_its_menu() {
        let mut rt = testkit::window("sessions");
        let header = group(RUNE_CLAUDE);
        let id = GroupId::Folder(RUNE_CLAUDE.into());
        let rows = |rt: &Runtime<Ui>| {
            let model = &rt.state.app.sidebar;
            let section = model.sections.iter().find(|s| s.id == id).unwrap();
            section.rows.iter().map(|r| r.key).collect::<Vec<_>>()
        };
        let before = rows(&rt);
        header_item(&mut rt, &header, 1);
        assert_eq!(rt.state.renaming, Some(Renaming::Group(id.clone())));
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-group-rename"));
        testkit::type_text(&mut rt, "Mind");
        assert_eq!(
            texts_in(&rt, &header),
            ["Mind"],
            "what is typed shows at once"
        );
        testkit::type_text(&mut rt, "Rune");
        assert_eq!(texts_in(&rt, &header), ["MindRune"]);
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(texts_in(&rt, &header), ["MindRune"]);
        assert_eq!(rows(&rt), before, "the same sessions");
        for key in &before {
            let folder = &rt.state.app.sessions.get(*key).unwrap().session.folder;
            assert_eq!(
                folder,
                std::path::Path::new(RUNE_CLAUDE),
                "no folder was touched"
            );
        }

        // Escape leaves the old name.
        header_item(&mut rt, &header, 1);
        testkit::type_text(&mut rt, "Other");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(texts_in(&rt, &header), ["MindRune"]);

        // Clicking another row keeps what was typed.
        header_item(&mut rt, &header, 1);
        testkit::type_text(&mut rt, "Kept");
        let asking = titled(&rt, NEEDS_YOU);
        testkit::click(&mut rt, &row(asking));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(asking));
        assert_eq!(rt.state.renaming, None);
        assert_eq!(texts_in(&rt, &header), ["Kept"]);
        assert_eq!(rows(&rt).len(), before.len());
    }

    #[test]
    fn a_click_on_the_rename_field_does_not_fold_the_group() {
        let mut rt = testkit::window("sessions");
        let header = group(RUNE_CLAUDE);
        header_item(&mut rt, &header, 1);
        testkit::click(&mut rt, "mr-group-rename");
        testkit::step(&mut rt);
        let id = GroupId::Folder(RUNE_CLAUDE.into());
        let section = rt.state.app.sidebar.sections.iter().find(|s| s.id == id);
        assert!(!section.unwrap().collapsed);
        assert_eq!(
            rt.state.renaming,
            Some(Renaming::Group(id)),
            "still renaming"
        );
    }

    #[test]
    fn starting_another_rename_keeps_the_change() {
        let mut rt = testkit::window("sessions");
        header_item(&mut rt, &group(RUNE_CLAUDE), 1);
        testkit::type_text(&mut rt, "Kept");
        let website = GroupId::Folder(WEBSITE.into());
        header_item(&mut rt, &header_of(&website), 1);
        assert_eq!(rt.state.renaming, Some(Renaming::Group(website)));
        assert_eq!(texts_in(&rt, &group(RUNE_CLAUDE)), ["Kept"]);
    }

    #[test]
    fn two_groups_can_have_the_same_name() {
        let mut rt = testkit::window("sessions");
        let side = group_named(&rt, "Side projects");
        header_item(&mut rt, &header_of(&side), 1);
        testkit::type_text(&mut rt, "sketches");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        let named = list_texts(&rt)
            .into_iter()
            .filter(|t| t == "sketches")
            .count();
        assert_eq!(named, 2, "both are drawn");
        assert!(rt.find(&header_of(&side)).is_some());
        assert!(rt.find(&group(SKETCHES)).is_some());
        let renamed: Vec<String> = rt
            .elements()
            .into_iter()
            .filter_map(|e| e.key)
            .filter(|k| k.contains("#dup"))
            .collect();
        assert!(renamed.is_empty(), "duplicate keys: {renamed:?}");
    }

    #[test]
    fn hiding_a_group_removes_it_and_hidden_groups_brings_it_back() {
        let mut rt = testkit::window("sessions");
        let website = GroupId::Folder(WEBSITE.into());
        assert!(
            !list_texts(&rt)
                .iter()
                .any(|t| t.starts_with("Hidden groups"))
        );
        header_item(&mut rt, &header_of(&website), 2);
        let texts = list_texts(&rt);
        assert!(!texts.contains(&"website".to_string()), "{texts:?}");
        assert!(!texts.contains(&"Fixing the contact form".to_string()));
        assert!(
            texts.contains(&"Hidden groups (1)".to_string()),
            "{texts:?}"
        );
        testkit::click(&mut rt, "mr-hidden-groups");
        testkit::step(&mut rt);
        assert_eq!(
            rt.state.side_menu.as_ref().map(|m| m.target.clone()),
            Some(MenuTarget::HiddenGroups)
        );
        assert_eq!(texts_in(&rt, MENU), ["website"]);
        let menu = rect(&rt, MENU);
        let line = rect(&rt, "mr-hidden-groups");
        assert!(menu[1] >= line[1], "under its line: {menu:?} {line:?}");
        testkit::click(&mut rt, "mr-side-menu-item0");
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        let texts = list_texts(&rt);
        assert!(texts.contains(&"website".to_string()), "{texts:?}");
        assert!(!texts.iter().any(|t| t.starts_with("Hidden groups")));
    }

    #[test]
    fn the_new_group_line_creates_a_group_and_starts_renaming() {
        let mut rt = testkit::window("sessions");
        assert!(list_texts(&rt).contains(&"New group".to_string()));
        testkit::click(&mut rt, "mr-new-group");
        testkit::step(&mut rt);
        let id = renamed_group(&rt);
        assert!(rt.state.app.sidebar.sections.iter().any(|s| s.id == id));
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-group-rename"));
        testkit::type_text(&mut rt, "Errands");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(texts_in(&rt, &header_of(&id)), ["Errands"]);
    }

    #[test]
    fn ungroup_sends_a_custom_groups_sessions_home() {
        let mut rt = testkit::window("sessions");
        let garden = titled(&rt, GARDEN);
        let side = group_named(&rt, "Side projects");
        right_click(&mut rt, &header_of(&side));
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
        assert!(rt.state.side_menu.is_none());
        let texts = list_texts(&rt);
        assert!(!texts.contains(&"Side projects".to_string()), "{texts:?}");
        assert_eq!(
            group_of(&rt, garden),
            GroupId::Folder("/home/alex/garden".into())
        );
        let at = texts.iter().position(|t| t == "garden").unwrap();
        assert_eq!(texts[at + 1], GARDEN);
        assert_eq!(
            texts_in(&rt, &row(garden)),
            [GARDEN],
            "home: no folder label"
        );
    }

    #[test]
    fn the_plus_on_a_folder_group_starts_a_session_there() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = testkit::window("sessions");
        let there = testkit::add_session(&mut rt, dir.path().to_str().unwrap());
        let id = GroupId::Folder(dir.path().into());
        assert_eq!(group_of(&rt, there), id);
        rt.state.folder_error = Some("an earlier error".into());
        testkit::click(&mut rt, &format!("mr-group-add-{}", id.key_text()));
        testkit::step(&mut rt);
        let shown = testkit::focused(&rt).expect("a session is shown");
        assert_ne!(shown, there, "a new session is shown");
        assert_eq!(rt.state.folder_error, None, "a new try");
        let entry = rt.state.app.sessions.get(shown).unwrap();
        assert_eq!(entry.session.folder, dir.path());
        assert_eq!(group_of(&rt, shown), id);
        let section = rt.state.app.sidebar.sections.iter().find(|s| s.id == id);
        assert!(!section.unwrap().collapsed, "the group did not fold");
        assert!(rt.find(&row(shown)).is_some());
        assert!(input_focused(&rt));
        demo::pump(&mut rt.state, 0.0);
        let entry = rt.state.app.sessions.get(shown).unwrap();
        assert!(entry.open.is_some(), "its program was started");
        assert_eq!(entry.session.phase, mr_core::Phase::Idle, "and it started");
    }

    #[test]
    fn the_plus_on_the_group_of_a_folder_that_is_gone_starts_nothing() {
        let mut rt = testkit::window("sessions");
        // The demo's folders are not on this disk: like a worktree removed since.
        let id = GroupId::Folder(RUNE_CLAUDE.into());
        let (shown, count) = (testkit::focused(&rt), rt.state.app.sessions.len());
        let recent = rt.state.app.recent_folders.clone();
        let gone = format!("That folder no longer exists: {RUNE_CLAUDE}");
        testkit::click(&mut rt, &format!("mr-group-add-{}", id.key_text()));
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), shown, "nothing started");
        assert_eq!(rt.state.app.sessions.len(), count);
        assert_eq!(rt.state.app.recent_folders, recent, "not remembered");
        assert_eq!(rt.state.folder_error.as_deref(), Some(gone.as_str()));
        assert!(testkit::has_text(&rt, &gone));
        // The header menu's "New session" does the same.
        rt.state.folder_error = None;
        header_item(&mut rt, &header_of(&id), 0);
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), shown);
        assert_eq!(rt.state.app.sessions.len(), count);
        assert_eq!(rt.state.folder_error.as_deref(), Some(gone.as_str()));
    }

    #[test]
    fn the_folder_error_goes_by_itself() {
        let mut rt = testkit::window("sessions");
        let id = GroupId::Folder(RUNE_CLAUDE.into());
        let gone = format!("That folder no longer exists: {RUNE_CLAUDE}");
        let add = format!("mr-group-add-{}", id.key_text());
        testkit::click(&mut rt, &add);
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, &gone));
        // It stays long enough to be read.
        for _ in 0..6 {
            rt.frame(1.0);
        }
        assert!(testkit::has_text(&rt, &gone), "six seconds on");
        // A try that fails again starts its time again.
        testkit::click(&mut rt, &add);
        for _ in 0..6 {
            rt.frame(1.0);
        }
        assert!(
            testkit::has_text(&rt, &gone),
            "six seconds after the second"
        );
        for _ in 0..3 {
            rt.frame(1.0);
        }
        assert_eq!(rt.state.folder_error, None, "nothing was done, and it went");
        assert!(!testkit::has_text(&rt, &gone));
    }

    #[test]
    fn the_menu_key_opens_the_menu_of_the_focused_row() {
        let mut rt = testkit::window("sessions");
        let finished = titled(&rt, FINISHED);
        testkit::click(&mut rt, &row(finished));
        testkit::step(&mut rt);
        rt.update(|_, tx| tx.focus(row(finished), true));
        testkit::step(&mut rt);
        testkit::press(&mut rt, Key::ContextMenu);
        testkit::step(&mut rt);
        let menu = rt.state.side_menu.clone().expect("the row's menu opened");
        assert_eq!(menu.target, MenuTarget::Row(finished));
        let [x, y, _, h] = rect(&rt, &row(finished));
        assert_eq!(
            (menu.at.x, menu.at.y),
            (x, y + h),
            "at the row's bottom-left"
        );
        assert!(open(&rt, MENU));
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        rt.update(|_, tx| tx.focus(row(finished), true));
        testkit::step(&mut rt);
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        testkit::press_with(&mut rt, Key::Function(10), shift);
        testkit::step(&mut rt);
        assert_eq!(
            rt.state.side_menu.as_ref().map(|m| m.target.clone()),
            Some(MenuTarget::Row(finished))
        );
    }

    #[test]
    fn menu_items_never_answer_a_request() {
        let mut rt = testkit::window("sessions");
        let asking = titled(&rt, NEEDS_YOU);
        testkit::click(&mut rt, &row(asking));
        testkit::step(&mut rt);
        assert!(rt.find("mr-allow-r1").is_some(), "the card waits");
        let from = sent_count(&rt);
        right_click(&mut rt, &row(asking));
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-side-menu-item0"));
        // The menu opens at the pointer, just above and left of its first item: no item is
        // highlighted, and Rune lets Return and Space choose nothing until one is (a row the
        // user cannot see highlighted chooses nothing).
        for key in [Key::Enter, Key::Text(" ".into())] {
            testkit::press(&mut rt, key);
            testkit::step(&mut rt);
            assert!(!open(&rt, SUBMENU));
            assert_eq!(group_of(&rt, asking), GroupId::Folder(RUNE_CLAUDE.into()));
        }
        // Down shows the highlight where the focus is, on "Move to group"; Return opens the
        // submenu and moves into it; Down moves on and Return chooses.
        testkit::press(&mut rt, Key::Down);
        testkit::step(&mut rt);
        assert!(!open(&rt, SUBMENU));
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-side-menu-item0"));
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert!(open(&rt, SUBMENU));
        assert_eq!(
            focused_key(&rt).as_deref(),
            Some("mr-side-menu-item0-sub-item0")
        );
        for _ in 0..2 {
            testkit::press(&mut rt, Key::Down);
            testkit::step(&mut rt);
        }
        assert_eq!(
            focused_key(&rt).as_deref(),
            Some("mr-side-menu-item0-sub-item2")
        );
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        let website = GroupId::Folder(WEBSITE.into());
        assert_eq!(group_of(&rt, asking), website);
        // Rune gives the focus back where it was before the menu opened: the input box,
        // which the click on the row focused. Space there types a space.
        assert!(input_focused(&rt), "back where it was");
        testkit::press(&mut rt, Key::Text(" ".into()));
        testkit::step(&mut rt);
        assert_eq!(glue::view_of(&rt.state, asking).draft.text, " ");
        // Return in the rename field keeps the name, and nothing else.
        header_item(&mut rt, &header_of(&website), 1);
        testkit::type_text(&mut rt, "Site");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(texts_in(&rt, &header_of(&website)), ["Site"]);
        // Down then Return on a header's first item chooses "New session": the demo's folder
        // is not on this disk, so it says so instead of starting one.
        right_click(&mut rt, &header_of(&website));
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-side-menu-item0"));
        for key in [Key::Down, Key::Enter] {
            testkit::press(&mut rt, key);
            testkit::step(&mut rt);
        }
        assert_eq!(
            rt.state.folder_error,
            Some(format!("That folder no longer exists: {WEBSITE}")),
            "New session was chosen"
        );
        let answered = sent_to(&rt, asking, from)
            .into_iter()
            .any(|c| matches!(c, SessionCommand::Answer { .. }));
        assert!(!answered, "no Answer was sent");
        let session = &rt.state.app.sessions.get(asking).unwrap().session;
        assert_eq!(session.conversation.waiting(), 1);
    }

    #[test]
    fn escape_on_a_menu_opened_by_the_menu_key_gives_the_focus_back_to_the_row() {
        let mut rt = testkit::window("sessions");
        let finished = titled(&rt, FINISHED);
        // As Tab into the list does once the row is shown: it lands on the selected row.
        testkit::click(&mut rt, &row(finished));
        testkit::step(&mut rt);
        rt.update(|_, tx| tx.focus(row(finished), true));
        testkit::step(&mut rt);
        testkit::press(&mut rt, Key::ContextMenu);
        testkit::step(&mut rt);
        assert!(open(&rt, MENU));
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        assert_eq!(focused_key(&rt), Some(row(finished)));
        // The list's arrows work again.
        let asking = titled(&rt, NEEDS_YOU);
        testkit::press(&mut rt, Key::Down);
        testkit::step(&mut rt);
        assert_eq!(focused_key(&rt), Some(row(asking)));
    }

    #[test]
    fn a_session_is_moved_with_the_keyboard_alone() {
        let mut rt = testkit::window("sessions");
        let working = titled(&rt, WORKING);
        // As Tab into the list does once the row is shown: it lands on the selected row.
        testkit::click(&mut rt, &row(working));
        testkit::step(&mut rt);
        rt.update(|_, tx| tx.focus(row(working), true));
        testkit::step(&mut rt);
        testkit::press(&mut rt, Key::ContextMenu);
        testkit::step(&mut rt);
        // "Move to group": its submenu opens, with the focus on its first group.
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(
            texts_in(&rt, SUBMENU),
            ["sketches", "Side projects", "website", "New group…"]
        );
        testkit::press(&mut rt, Key::Down);
        testkit::step(&mut rt);
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        let side = group_named(&rt, "Side projects");
        assert_eq!(group_of(&rt, working), side, "moved by the keyboard");
        assert!(!open(&rt, MENU) && !open(&rt, SUBMENU));
        // The row is on screen in its new place, and has the focus.
        let texts = list_texts(&rt);
        let at = texts.iter().position(|t| t == "Side projects").unwrap();
        assert_eq!(texts[at + 1], WORKING, "{texts:?}");
        assert_eq!(focused_key(&rt), Some(row(working)));
    }

    #[test]
    fn nothing_is_saved_while_a_name_is_typed() {
        let mut rt = testkit::window("sessions");
        let header = group(RUNE_CLAUDE);
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        header_item(&mut rt, &header, 1);
        testkit::type_text(&mut rt, "Mind");
        testkit::type_text(&mut rt, "Rune");
        assert_eq!(texts_in(&rt, &header), ["MindRune"]);
        assert!(!saved(&rt), "nothing saved per key");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert!(saved(&rt), "saved once the name is kept");
    }

    #[test]
    fn a_new_group_whose_header_is_off_screen_is_renamed_in_view() {
        let mut rt = testkit::window("empty");
        let now = rt.state.app.now_unix;
        // Sixty groups of one recent session each: far longer than the sidebar.
        let listed: Vec<StoredSession> = (0..60)
            .map(|n| StoredSession {
                id: SessionId(format!("project-{n}")),
                folder: format!("/home/alex/project-{n:02}").into(),
                title: format!("Work on project {n}"),
                last_active_unix: now - 3_600 * (n + 1),
            })
            .collect();
        let ui = &mut rt.state;
        let effects = ui.app.receive(
            FromBackend::Sessions {
                since_unix: Some(0),
                result: Ok(listed),
            },
            0.0,
        );
        glue::run(ui, effects);
        rt.invalidate();
        testkit::step(&mut rt);
        // To the end of the list, where "New group" is; a new group is listed first.
        rt.update(|_, tx| tx.scroll_into_view("mr-new-group"));
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(rt.find("mr-new-group").is_some());
        assert!(!list_texts(&rt).contains(&"Work on project 0".to_string()));
        testkit::click(&mut rt, "mr-new-group");
        testkit::step(&mut rt);
        let id = renamed_group(&rt);
        let first = rt.state.app.sidebar.lines[0];
        assert_eq!(
            line_key(&rt.state.app.sidebar, first),
            header_of(&id),
            "first"
        );
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(
            rt.state.renaming,
            Some(Renaming::Group(id.clone())),
            "still renaming"
        );
        assert!(
            rt.elements().iter().any(|e| !e.exiting
                && e.key.as_deref() == Some("mr-group-rename")
                && e.visible[3] > 0.0),
            "the field is on screen"
        );
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-group-rename"));
        testkit::type_text(&mut rt, "Errands");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(texts_in(&rt, &header_of(&id)), ["Errands"]);
    }

    #[test]
    fn the_group_menu_scene_shows_a_rows_submenu_inside_the_window() {
        let rt = testkit::window("group-menu");
        assert_eq!(texts_in(&rt, MENU), ROW_MENU);
        assert_eq!(
            texts_in(&rt, SUBMENU),
            ["sketches", "Side projects", "website", "New group…"]
        );
        for key in [MENU, SUBMENU] {
            let [x, y, w, h] = rect(&rt, key);
            assert!(
                x >= 0.0 && y >= 0.0 && x + w <= 1200.0 && y + h <= 800.0,
                "{key}"
            );
        }
        let item = rect(&rt, "mr-side-menu-item0");
        let sub = rect(&rt, SUBMENU);
        assert!(
            sub[0] >= item[0] + item[2] - 0.5,
            "beside its item, not over it"
        );
    }

    // Open to the side: Task 9 of plan 4.

    /// A window wide enough for a second tile beside the first.
    const WIDE: (f32, f32) = (1800.0, 900.0);

    /// The "Open to the side" item of a row's menu.
    const SIDE_ITEM: &str = "mr-side-menu-item1";

    /// Clicks the element keyed `key` with Ctrl held, then lets Ctrl go and steps.
    fn ctrl_click(rt: &mut Runtime<Ui>, key: &str) {
        rt.physical_input(PhysicalInput::Modifiers(testkit::ctrl()));
        testkit::step(rt);
        testkit::click(rt, key);
        rt.physical_input(PhysicalInput::Modifiers(Mods::default()));
        testkit::step(rt);
    }

    /// Whether the menu item keyed `key` can be chosen: the kit's disabled items take no focus.
    fn enabled(rt: &Runtime<Ui>, key: &str) -> bool {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no element keyed {key:?}"))
            .focusable
    }

    #[test]
    fn a_ctrl_click_opens_the_session_to_the_side() {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let garden = titled(&rt, GARDEN);
        assert_eq!(tile_keys(&rt), [garden]);
        let finished = titled(&rt, FINISHED);
        ctrl_click(&mut rt, &row(finished));
        assert_eq!(tile_keys(&rt), [garden, finished]);
        assert_eq!(testkit::focused(&rt), Some(finished));
        assert!(box_focused(&rt, finished), "its input box has the focus");
        assert!(!box_focused(&rt, garden));
        // A plain click shows a session in the focused tile, as before.
        let asking = titled(&rt, NEEDS_YOU);
        testkit::click(&mut rt, &row(asking));
        testkit::step(&mut rt);
        assert_eq!(tile_keys(&rt), [garden, asking]);
        assert_eq!(testkit::focused(&rt), Some(asking));
    }

    #[test]
    fn an_arrow_passes_the_row_of_another_tile() {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let [asking, working, garden] = [NEEDS_YOU, WORKING, GARDEN].map(|t| titled(&rt, t));
        testkit::click(&mut rt, &row(asking));
        testkit::step(&mut rt);
        ctrl_click(&mut rt, &row(working));
        // Back in the first tile, and in the list on its row, as Tab into the list lands.
        testkit::click(&mut rt, &row(asking));
        testkit::step(&mut rt);
        rt.update(|_, tx| tx.focus(row(asking), true));
        testkit::step(&mut rt);
        assert_eq!(
            (tile_keys(&rt), testkit::focused(&rt)),
            (vec![asking, working], Some(asking))
        );
        let from = sent_count(&rt);
        testkit::press(&mut rt, Key::Down);
        testkit::step(&mut rt);
        assert_eq!(
            focused_key(&rt),
            Some(row(working)),
            "the list is on its row"
        );
        assert!(selected(&rt, &row(working)) && !selected(&rt, &row(asking)));
        assert_eq!(
            (tile_keys(&rt), testkit::focused(&rt)),
            (vec![asking, working], Some(asking)),
            "nothing else changed"
        );
        testkit::press(&mut rt, Key::Down);
        testkit::step(&mut rt);
        assert_eq!(focused_key(&rt), Some(row(garden)));
        assert!(selected(&rt, &row(garden)) && !selected(&rt, &row(working)));
        assert_eq!(
            (tile_keys(&rt), testkit::focused(&rt)),
            (vec![garden, working], Some(garden)),
            "on in the tile that was being browsed"
        );
        // Up passes it as well; Return on its row focuses its tile.
        testkit::press(&mut rt, Key::Up);
        testkit::step(&mut rt);
        assert_eq!(focused_key(&rt), Some(row(working)));
        assert_eq!(testkit::focused(&rt), Some(garden));
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(
            (tile_keys(&rt), testkit::focused(&rt)),
            (vec![garden, working], Some(working))
        );
        assert!(box_focused(&rt, working));
        assert_eq!(sent_count(&rt), from, "nothing was sent");
    }

    #[test]
    fn the_menu_opens_a_session_to_the_side() {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let garden = titled(&rt, GARDEN);
        let finished = titled(&rt, FINISHED);
        right_click(&mut rt, &row(finished));
        assert_eq!(texts_in(&rt, MENU), ROW_MENU);
        assert!(enabled(&rt, SIDE_ITEM));
        testkit::click(&mut rt, SIDE_ITEM);
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        assert!(!open(&rt, MENU));
        assert_eq!(tile_keys(&rt), [garden, finished]);
        assert_eq!(testkit::focused(&rt), Some(finished));
        assert!(box_focused(&rt, finished), "its input box has the focus");
    }

    #[test]
    fn without_room_the_menu_entry_is_disabled_and_a_ctrl_click_swaps() {
        let mut rt = testkit::window_sized("sessions", (1000.0, 800.0));
        let garden = titled(&rt, GARDEN);
        assert_eq!(tile_keys(&rt), [garden]);
        assert!(!rt.state.app.room_for_tile());
        let finished = titled(&rt, FINISHED);
        right_click(&mut rt, &row(finished));
        assert_eq!(texts_in(&rt, MENU), ROW_MENU);
        assert!(!enabled(&rt, SIDE_ITEM), "no room");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        ctrl_click(&mut rt, &row(finished));
        assert_eq!(tile_keys(&rt), [finished], "shown in the focused tile");
        assert_eq!(testkit::focused(&rt), Some(finished));
        assert!(box_focused(&rt, finished));
    }

    #[test]
    fn a_session_in_a_tile_cannot_be_opened_twice() {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let garden = titled(&rt, GARDEN);
        let finished = titled(&rt, FINISHED);
        ctrl_click(&mut rt, &row(finished));
        assert_eq!(tile_keys(&rt), [garden, finished]);
        assert!(rt.state.app.room_for_tile(), "there is room for a third");
        right_click(&mut rt, &row(garden));
        assert!(!enabled(&rt, SIDE_ITEM), "in a tile already");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        ctrl_click(&mut rt, &row(garden));
        assert_eq!(tile_keys(&rt), [garden, finished], "no second tile for it");
        assert_eq!(testkit::focused(&rt), Some(garden), "its tile is focused");
        assert!(box_focused(&rt, garden));
    }

    #[test]
    fn the_move_to_group_submenu_still_opens() {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let working = titled(&rt, WORKING);
        right_click(&mut rt, &row(working));
        assert!(!open(&rt, "mr-side-menu-item0-sub-menu"));
        testkit::click(&mut rt, "mr-side-menu-item0");
        testkit::step(&mut rt);
        assert!(open(&rt, "mr-side-menu-item0-sub-menu"));
        assert_eq!(
            texts_in(&rt, "mr-side-menu-item0-sub-menu"),
            ["sketches", "Side projects", "website", "New group…"]
        );
    }

    #[test]
    fn a_ctrl_click_on_a_groups_plus_opens_its_session_to_the_side() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = testkit::window_sized("sessions", WIDE);
        let there = testkit::add_session(&mut rt, dir.path().to_str().unwrap());
        let id = GroupId::Folder(dir.path().into());
        ctrl_click(&mut rt, &format!("mr-group-add-{}", id.key_text()));
        let new = testkit::focused(&rt).expect("a session is shown");
        assert_ne!(new, there);
        assert_eq!(tile_keys(&rt), [there, new]);
        assert_eq!(group_of(&rt, new), id);
        assert!(box_focused(&rt, new));
    }

    // Open below: Task 5 of the stacked tiles plan.

    /// The "Open below" item of a row's menu.
    const BELOW_ITEM: &str = "mr-side-menu-item2";

    /// Opens the menu of `key`'s row: whether "Open below" can be chosen there. The menu is
    /// left open.
    fn below_enabled(rt: &mut Runtime<Ui>, key: SessionKey) -> bool {
        right_click(rt, &row(key));
        assert_eq!(texts_in(rt, MENU), ROW_MENU);
        enabled(rt, BELOW_ITEM)
    }

    #[test]
    fn the_menu_opens_a_session_below_the_focused_tile() {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let garden = titled(&rt, GARDEN);
        let finished = titled(&rt, FINISHED);
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        assert!(below_enabled(&mut rt, finished));
        testkit::click(&mut rt, BELOW_ITEM);
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        assert!(!open(&rt, MENU));
        assert_eq!(columns(&rt), [vec![garden, finished]]);
        assert_eq!(testkit::focused(&rt), Some(finished));
        assert!(box_focused(&rt, finished), "its input box has the focus");
        assert!(saved(&rt), "the state is saved");
    }

    #[test]
    fn open_below_is_disabled_when_it_cannot_be() {
        // The session in a tile already.
        let mut rt = testkit::window_sized("sessions", WIDE);
        let garden = titled(&rt, GARDEN);
        let finished = titled(&rt, FINISHED);
        let asking = titled(&rt, NEEDS_YOU);
        assert!(!below_enabled(&mut rt, garden), "in a tile already");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        // The focused tile's column full.
        assert!(below_enabled(&mut rt, finished));
        testkit::click(&mut rt, BELOW_ITEM);
        testkit::step(&mut rt);
        assert_eq!(columns(&rt), [vec![garden, finished]]);
        assert!(!below_enabled(&mut rt, asking), "the column is full");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        // A row too low for two.
        let mut rt = testkit::window_sized("sessions", (1800.0, 500.0));
        let finished = titled(&rt, FINISHED);
        assert!(!below_enabled(&mut rt, finished), "too low");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        // No tile at all.
        let mut rt = testkit::window_sized("sessions", WIDE);
        let garden = titled(&rt, GARDEN);
        let finished = titled(&rt, FINISHED);
        rt.update(|ui, _| {
            let effects = ui.app.close_tile(garden, mr_core::Leaving::Keep);
            glue::run(ui, effects);
        });
        testkit::step(&mut rt);
        assert!(tile_keys(&rt).is_empty());
        assert!(!below_enabled(&mut rt, finished), "no tile");
    }

    // Renaming a session in its row: Task 10 of plan 4.

    const RESTING: &str = "Fixing the contact form";

    /// The "Rename" item of a row's menu.
    const RENAME_ITEM: &str = "mr-side-menu-item3";

    /// Opens the menu of `key`'s row and chooses "Rename".
    fn rename_row(rt: &mut Runtime<Ui>, key: SessionKey) {
        right_click(rt, &row(key));
        testkit::click(rt, RENAME_ITEM);
        testkit::step(rt);
        assert_eq!(rt.state.renaming, Some(Renaming::Row(key)));
    }

    #[test]
    fn the_rows_menu_renames_in_the_row() {
        let mut rt = testkit::window("sessions");
        let resting = titled(&rt, RESTING);
        let entry = rt.state.app.sessions.get(resting).unwrap();
        assert!(entry.open.is_none() && !rt.state.app.tiles.contains(resting));
        let id = entry.session.id.clone().expect("a stored session");
        let shown = testkit::focused(&rt);
        let from = sent_count(&rt);
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        rename_row(&mut rt, resting);
        assert!(rt.state.side_menu.is_none());
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-row-rename"));
        let edit = &rt.state.rename;
        assert_eq!(edit.text, RESTING);
        assert_eq!(
            (edit.anchor, edit.focus),
            (0, RESTING.len()),
            "all selected"
        );
        assert_eq!(texts_in(&rt, &row(resting)), [RESTING]);
        testkit::type_text(&mut rt, "Contact");
        assert_eq!(texts_in(&rt, &row(resting)), ["Contact"], "shown as typed");
        testkit::type_text(&mut rt, " form");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(texts_in(&rt, &row(resting)), ["Contact form"]);
        assert_eq!(sent_to(&rt, resting, from), [], "nothing was sent");
        let saved: mr_core::Saved = rt.state.prefs.get(glue::STATE_KEY).expect("saved");
        assert!(saved.names.contains(&(id, "Contact form".into())));
        assert_eq!(testkit::focused(&rt), shown, "the tile focus did not move");
        // Back in the list, on its selected row, as Tab into it lands.
        assert_eq!(focused_key(&rt), shown.map(row));
    }

    #[test]
    fn a_row_rename_leaves_the_tiles_alone() {
        let mut rt = testkit::window_sized("sessions", WIDE);
        let garden = titled(&rt, GARDEN);
        let finished = titled(&rt, FINISHED);
        ctrl_click(&mut rt, &row(finished));
        assert_eq!(tile_keys(&rt), [garden, finished]);
        assert!(box_focused(&rt, finished));
        let resting = titled(&rt, RESTING);
        rename_row(&mut rt, resting);
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(
            focused_key(&rt).as_deref(),
            Some("mr-row-rename"),
            "not given back"
        );
        assert_eq!(
            testkit::focused(&rt),
            Some(finished),
            "the tile focus stays"
        );
        testkit::type_text(&mut rt, "Contact form");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert!(list_texts(&rt).contains(&RESTING.to_string()), "dropped");
        assert_eq!(testkit::focused(&rt), Some(finished));
    }

    #[test]
    fn renaming_a_group_still_works() {
        let mut rt = testkit::window("sessions");
        let header = group(RUNE_CLAUDE);
        header_item(&mut rt, &header, 1);
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-group-rename"));
        testkit::type_text(&mut rt, "MindRune");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, None);
        assert_eq!(texts_in(&rt, &header), ["MindRune"]);
    }

    #[test]
    fn one_name_is_edited_at_a_time() {
        let mut rt = testkit::window("sessions");
        let header = group(RUNE_CLAUDE);
        header_item(&mut rt, &header, 1);
        testkit::type_text(&mut rt, "MindRune");
        let resting = titled(&rt, RESTING);
        rename_row(&mut rt, resting);
        assert_eq!(
            texts_in(&rt, &header),
            ["MindRune"],
            "the group's name is kept"
        );
        assert!(rt.find("mr-group-rename").is_none());
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-row-rename"));
        testkit::type_text(&mut rt, "Contact form");
        // And a group's rename ends the row's, keeping it.
        header_item(&mut rt, &header, 1);
        assert_eq!(
            rt.state.renaming,
            Some(Renaming::Group(GroupId::Folder(RUNE_CLAUDE.into())))
        );
        assert_eq!(texts_in(&rt, &row(resting)), ["Contact form"]);
    }

    #[test]
    fn a_row_rename_ends_when_the_row_goes() {
        let mut rt = testkit::window("sessions");
        let resting = titled(&rt, RESTING);
        rename_row(&mut rt, resting);
        testkit::type_text(&mut rt, "Contact form");
        let ui = &mut rt.state;
        let effects = ui.app.hide_group(&GroupId::Folder(WEBSITE.into()), true);
        glue::run(ui, effects);
        assert_eq!(rt.state.renaming, None, "its row is no longer listed");
        rt.invalidate();
        testkit::step(&mut rt);
        let title = &rt.state.app.sessions.get(resting).unwrap().session.title;
        assert_eq!(title.as_deref(), Some(RESTING), "dropped");
    }

    #[test]
    fn orange_wins_on_a_selected_row() {
        let mut rt = testkit::window("sessions");
        let asking = row(titled(&rt, NEEDS_YOU));
        testkit::click(&mut rt, &asking);
        testkit::step(&mut rt);
        assert!(selected(&rt, &asking));
        assert_eq!(fill(&mut rt, &asking), theme::needs_input().fill);
        // The row it passed back to is selected and grey again.
        let garden = row(titled(&rt, GARDEN));
        testkit::click(&mut rt, &garden);
        testkit::step(&mut rt);
        assert_eq!(fill(&mut rt, &garden), theme::kit(true).c.pressed);
        assert_eq!(fill(&mut rt, &asking), theme::needs_input().fill);
        // Under the pointer it keeps its colour, while a grey row takes the hover.
        let hovered = |rt: &mut Runtime<Ui>, key: &str| {
            let (x, y) = testkit::find(rt, key).unwrap();
            rt.pointer_move(x, y);
            testkit::step(rt);
            rt.settle_all();
            let line = rt
                .elements()
                .into_iter()
                .find(|e| !e.exiting && e.key.as_deref() == Some(key))
                .unwrap();
            (line.hover, line.fill)
        };
        let grey = row(titled(&rt, WORKING));
        assert!(
            hovered(&mut rt, &grey).0 > 0.0,
            "a grey row takes the hover"
        );
        let (hover, colour) = hovered(&mut rt, &asking);
        assert_eq!(hover, 0.0, "no hover over the colour");
        assert_eq!(colour, theme::needs_input().fill);
    }

    /// What is drawn inside the row keyed `key`: the keys of its parts.
    fn parts(rt: &mut Runtime<Ui>, key: &str) -> Vec<Option<String>> {
        rt.settle_all();
        testkit::step(rt);
        let inside = format!("/{key}/");
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains(&inside))
            .map(|e| e.key)
            .collect()
    }

    #[test]
    fn a_selected_coloured_row_is_its_fill_and_nothing_more() {
        let mut rt = testkit::window("sessions");
        let asking = row(titled(&rt, NEEDS_YOU));
        let resting = parts(&mut rt, &asking);
        assert!(!resting.is_empty(), "its title is in it");

        // Selected by a press: orange still, with nothing added inside the row.
        testkit::click(&mut rt, &asking);
        testkit::step(&mut rt);
        assert!(selected(&rt, &asking));
        assert_eq!(fill(&mut rt, &asking), theme::needs_input().fill);
        assert_eq!(parts(&mut rt, &asking), resting, "no ring, no border");

        // The arrow keys come onto it and select it: the same.
        let garden = row(titled(&rt, GARDEN));
        testkit::click(&mut rt, &garden);
        testkit::step(&mut rt);
        rt.update(|_, tx| tx.focus(garden.clone(), true));
        testkit::step(&mut rt);
        testkit::press(&mut rt, Key::Up);
        testkit::step(&mut rt);
        testkit::press(&mut rt, Key::Up);
        testkit::step(&mut rt);
        assert_eq!(focused_key(&rt), Some(asking.clone()), "the arrow is on it");
        assert_eq!(fill(&mut rt, &asking), theme::needs_input().fill);
        assert_eq!(parts(&mut rt, &asking), resting);
    }

    #[test]
    fn delete_group_waits_for_the_full_list() {
        let mut rt = testkit::window("sessions");
        let header = group(RUNE_CLAUDE);
        // A folder group's "Delete group": after "New session", "Rename", "Hide group" and the
        // separator.
        let item = |rt: &Runtime<Ui>| {
            rt.elements()
                .into_iter()
                .find(|e| !e.exiting && e.key.as_deref() == Some("mr-side-menu-item4"))
                .expect("the entry")
        };
        // Only the recent sessions are listed yet.
        let ui = &mut rt.state;
        ui.app.listed_all = false;
        glue::run(ui, Vec::new());
        assert!(!ui.app.sidebar.complete);
        rt.invalidate();
        right_click(&mut rt, &header);
        assert_eq!(
            texts_in(&rt, MENU),
            ["New session", "Rename", "Hide group", "Delete group"]
        );
        assert!(
            !item(&rt).focusable,
            "greyed out until every session is known"
        );
        testkit::click(&mut rt, "mr-side-menu-item4");
        testkit::step(&mut rt);
        assert_eq!(rt.state.delete_ask, None, "a press asks nothing");
        // Nor does the question come another way.
        rt.update(|ui, _| glue::ask_delete_group(ui, GroupId::Folder(RUNE_CLAUDE.into())));
        assert_eq!(rt.state.delete_ask, None);
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);

        // The full list is in.
        let ui = &mut rt.state;
        ui.side_menu = None;
        ui.app.listed_all = true;
        glue::run(ui, Vec::new());
        assert!(ui.app.sidebar.complete);
        rt.invalidate();
        right_click(&mut rt, &header);
        assert!(item(&rt).focusable);
        testkit::click(&mut rt, "mr-side-menu-item4");
        testkit::step(&mut rt);
        assert_eq!(
            rt.state.delete_ask,
            Some(glue::DeleteAsk::Group(GroupId::Folder(RUNE_CLAUDE.into())))
        );
    }

    #[test]
    fn a_collapsed_group_shows_the_strongest_colour() {
        let mut rt = testkit::window("sessions");
        let dot = |rt: &mut Runtime<Ui>, header: &str| {
            rt.settle_all();
            testkit::step(rt);
            let inside = format!("/{header}/");
            rt.elements()
                .into_iter()
                .find(|e| !e.exiting && e.path.contains(&inside) && e.path.contains("/mr-mark/"))
                .map(|e| e.fill)
        };
        let header = group(RUNE_CLAUDE);
        testkit::click(&mut rt, &header);
        testkit::step(&mut rt);
        assert_eq!(dot(&mut rt, &header), Some(theme::needs_input().fill));
        assert_eq!(mark(&rt, &group(SKETCHES)), Some("working"));
        // A group whose strongest is a ready session shows the green.
        let resting = titled(&rt, "Fixing the contact form");
        let ui = &mut rt.state;
        ui.app.sessions.get_mut(resting).unwrap().unseen = true;
        glue::run(ui, Vec::new());
        let website = group(WEBSITE);
        testkit::click(&mut rt, &website);
        testkit::step(&mut rt);
        assert_eq!(dot(&mut rt, &website), Some(theme::ready().fill));
    }

    #[test]
    fn the_row_being_renamed_keeps_its_colour() {
        let mut rt = testkit::window("sessions");
        for (title, pair) in [
            (NEEDS_YOU, theme::needs_input()),
            (FINISHED, theme::ready()),
        ] {
            let key = titled(&rt, title);
            assert_eq!(fill(&mut rt, &row(key)), pair.fill);
            rename_row(&mut rt, key);
            assert_eq!(fill(&mut rt, &row(key)), pair.fill, "{title}");
            let field = rt
                .elements()
                .into_iter()
                .find(|e| !e.exiting && e.key.as_deref() == Some("mr-row-rename"))
                .expect("the field");
            assert_eq!(field.text_color, Some(pair.ink));
            testkit::press(&mut rt, Key::Escape);
            testkit::step(&mut rt);
        }
    }
}
