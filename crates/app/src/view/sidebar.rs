//! The sidebar's list: every group's header with its sessions' rows under it, as one flat list
//! of virtual rows drawn from the core's model (`App::sidebar`); and its menus, a row's and a
//! header's, with renaming a group in place.
//!
//! Every line is one row of the list, all of one height, headers included. Only the rows near
//! the viewport are built, and a built row is built again only when the model is rebuilt: the
//! model's version (`App::sidebar_version`) is the list's version and each row's, so nothing
//! here is redone while a reply merely streams. The one line drawn from more than the model is
//! the header whose name is being edited: its version holds the field too ([`line_version`]).

use std::borrow::Cow;

use mr_core::{Effect, GroupId, Line, Mark, Section, SessionKey, SidebarModel};
use rune::{
    Anim, Cx, El, GrowthSpec, Key, KeyEvent, PointerPress, TextEdit, Tx, col, el, icon, memo, row,
    text, text_input,
};
use rune_kit::shell::{ROW_HEIGHT, RowStatus, SidebarRow};
use rune_kit::{
    ControlSize, IconButton, IconButtonVariant, Kit, Menu, MenuEntry, MenuItem, Spinner,
};
use rune_theme_opencode::icons::PLUS;

use crate::glue::{self, MenuTarget, SideMenu, Ui};
use crate::view::folder;

/// Rows and headers sit this far in from the list's sides, as the kit's do.
const INSET: f32 = 8.0;

/// The sidebar's one menu. The kit keys its surface `mr-side-menu-menu` and its items
/// `mr-side-menu-item{i}`.
const MENU: &str = "mr-side-menu";

/// A row's "Move to group" submenu, as the kit keys it.
const SUBMENU: &str = "mr-side-menu-item0-sub-menu";

/// The field a group's name is edited in, in its header.
const RENAME: &str = "mr-group-rename";

/// The list of groups and sessions, for the kit's `Sidebar`, whose list it goes in: one row of
/// [`ROW_HEIGHT`] per line of the model.
///
/// The rows are the virtual rows of a growth stack of this app's own rather than
/// `Sidebar::virtual_rows`, which takes no version: without one, Rune asks every row's key on
/// every commit. The stack sits in the kit's list, so the list's focus group (one Tab stop,
/// Up and Down) holds its rows.
pub fn list(ui: &Ui, kit: &'static Kit) -> El {
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

/// What `line` is drawn from: the model, and for the header whose name is being edited, the
/// field, which changes with every key while the model holds the old name until it is kept.
fn line_version(ui: &Ui, line: Line) -> (u64, Option<&TextEdit>) {
    let editing = match line {
        Line::Header(s) => ui.renaming.as_ref() == Some(&ui.app.sidebar.sections[s].id),
        _ => false,
    };
    (ui.app.sidebar_version, editing.then_some(&ui.rename))
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
    let model = &ui.app.sidebar;
    match line {
        Line::Header(s) => header(ui, &model.sections[s], kit),
        Line::Row(s, r) => {
            let row = &model.sections[s].rows[r];
            let mut built = SidebarRow::new(row.title.clone()).selected(row.shown);
            if let Some(folder) = &row.folder_label {
                built = built.detail(folder.clone());
            }
            if let Some(status) = status(row.mark) {
                built = built.status(status);
            }
            built
                .on_select(select(row.key))
                .build(kit)
                .on_key(choose(row.key))
                .on_secondary_press(open_menu(MenuTarget::Row(row.key)))
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
            start_rename(ui, tx, id);
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

/// What a row's press does: shows its session. A click also focuses the session's input box.
/// Moving with the arrow keys, which press each row they reach, leaves the focus in the list,
/// so the next one moves on. (Return and Space never get here: see [`choose`].)
fn select(key: SessionKey) -> impl Fn(&mut Ui, &mut Tx) + 'static {
    move |ui: &mut Ui, tx: &mut Tx| {
        let focus = !tx.by_keys();
        glue::show(ui, tx, key, focus);
    }
}

/// Return or Space on a focused row chooses its session, as a click does: it is shown and its
/// input box takes the focus. The row's own key handler hears them before the list's focus
/// group would press the row (`El::on_key`), so they are told apart from the arrows there.
fn choose(key: SessionKey) -> impl Fn(&mut Ui, &KeyEvent, &mut Tx) -> bool + 'static {
    move |ui: &mut Ui, ev: &KeyEvent, tx: &mut Tx| {
        let m = ev.mods;
        let plain = !(m.shift || m.cmd || m.ctrl || m.alt || m.logo);
        let chosen = matches!(&ev.key, Key::Enter) || matches!(&ev.key, Key::Text(t) if t == " ");
        if !plain || !chosen {
            return false;
        }
        if !tx.key_repeat() {
            glue::show(ui, tx, key, true);
        }
        true
    }
}

/// A secondary press opens `target`'s menu where it landed. Rune calls it for the Menu key and
/// Shift+F10 too, at the bottom-left of the focused element.
fn open_menu(target: MenuTarget) -> impl Fn(&mut Ui, PointerPress, &mut Tx) + 'static {
    move |ui: &mut Ui, at: PointerPress, _: &mut Tx| {
        ui.side_menu = Some(SideMenu {
            target: target.clone(),
            at,
            sub_open: false,
        });
    }
}

/// The kit's status mark for a session's mark.
fn status(mark: Mark) -> Option<RowStatus> {
    match mark {
        Mark::None => None,
        Mark::Working => Some(RowStatus::Busy),
        Mark::NeedsYou => Some(RowStatus::Attention),
        Mark::Finished => Some(RowStatus::Unread),
    }
}

/// A group's header: a chevron, its name (or, while it is being renamed, the field), while it
/// is collapsed the strongest mark among its sessions, and a "+" for a new session in it. A
/// press folds or unfolds it, but not while it is being renamed, so a click in the field only
/// moves the caret. A secondary press opens its menu.
///
/// Built from the kit's parts as `SidebarHeader::build` builds one, so that it can hold a
/// text field. Like the kit's, it is for the pointer only: the list's arrow keys pass over it
/// to the next session's row, and since they press what they reach they would fold groups as
/// they passed.
fn header(ui: &Ui, section: &Section, kit: &Kit) -> El {
    let c = &kit.c;
    let renaming = ui.renaming.as_ref() == Some(&section.id);
    let glyph = if section.collapsed {
        kit.glyph.triangle_right
    } else {
        kit.glyph.triangle_down
    };
    let name = if renaming {
        rename_field(&ui.rename, kit)
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

/// The one-line field a group's name is edited in: Return keeps the name, Escape drops the
/// change. When it loses the focus otherwise, [`rename_watch`] keeps the name. As the kit's
/// row rename does, it gives the focus back to the list when it goes.
fn rename_field(edit: &TextEdit, kit: &Kit) -> El {
    let c = &kit.c;
    text_input(edit, kit.ty.text_xsmall_medium)
        .single_line()
        .key(RENAME)
        .label("Group name")
        .focus_selected(true)
        .focus_return("sidebar-list")
        .grow(1.0)
        .shrink()
        .color(c.text_base)
        .caret(c.text_base, kit.selection())
        .on_edit(|ui: &mut Ui| &mut ui.rename)
        .on_submit(|ui: &mut Ui, _: &mut Tx| end_rename(ui, true))
        .on_input_key(|ui: &mut Ui, ev: &KeyEvent, _: &mut Tx| {
            if ev.key != Key::Escape {
                return false;
            }
            end_rename(ui, false);
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
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| new_session(ui, tx, &id))
        .build(kit)
        .key(add_key(&section.id))
        .focusable(false)
}

/// The key of `id`'s "+".
pub fn add_key(id: &GroupId) -> String {
    format!("mr-group-add-{}", id.key_text())
}

/// A new session in group `id`: in a folder's group, in that folder at once while it is still
/// a folder (see `folder::start_chosen`); in a custom group, in the folder the folder menu is
/// asked for.
fn new_session(ui: &mut Ui, tx: &mut Tx, id: &GroupId) {
    match id {
        GroupId::Folder(folder) => folder::start_chosen(ui, tx, folder.clone(), Some(id)),
        GroupId::Custom(_) => folder::ask(ui, tx, Some(id.clone())),
    }
}

/// A header's mark, keyed `mr-mark`, drawn as a row's status slot draws it.
fn mark_slot(mark: Mark, kit: &Kit) -> Option<El> {
    let c = &kit.c;
    let dot = |color| el().size(7.0, 7.0).radius(3.5).fill(color);
    let drawn = match mark {
        Mark::None => return None,
        Mark::Working => Spinner::new().size(12.0).color(c.icon_muted).build(kit),
        Mark::NeedsYou => dot(c.warning.icon),
        Mark::Finished => dot(c.bg_accent),
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

/// Starts editing `id`'s name in its header: the field holds the name, all selected, and takes
/// the focus. A change being made to another group's name is dropped.
fn start_rename(ui: &mut Ui, tx: &mut Tx, id: GroupId) {
    let mut edit = TextEdit::new(ui.app.groups.name(&id));
    edit.anchor = 0;
    edit.focused = true;
    ui.rename = edit;
    // Only the rows near the viewport are built: a header far off has no field to focus.
    tx.scroll_into_view(header_key(&id));
    ui.renaming = Some(id);
}

/// Ends the rename going on, if any: with `keep`, the name typed is given to the group, unless
/// it is the name it has.
fn end_rename(ui: &mut Ui, keep: bool) {
    let Some(id) = ui.renaming.take() else {
        return;
    };
    let edit = std::mem::replace(&mut ui.rename, TextEdit::new(""));
    if keep && edit.text.trim() != ui.app.groups.name(&id) {
        let effects = ui.app.rename_group(&id, &edit.text);
        glue::run(ui, effects);
    }
}

/// Shuts the menu as one of its items is chosen. The focus left the rename field going on when
/// the menu opened: unless the item starts another rename, the name typed so far is kept now,
/// before the item changes the groups.
fn chosen(ui: &mut Ui, keep_rename: bool) {
    ui.side_menu = None;
    if keep_rename {
        end_rename(ui, true);
    }
}

/// While a group's name is edited in a field that has lost the focus otherwise than to the
/// sidebar's menu (a click elsewhere, Tab), the name typed so far is kept.
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
            .on_layout(|ui: &mut Ui, _| end_rename(ui, true)),
    );
}

/// The sidebar's menu, while one is open, into `layer`: one kit `Menu` placed at the point it
/// was opened at, with no trigger. A press on the "Hidden groups" line gives no point, so that
/// menu opens under the line.
pub fn menu(ui: &Ui, cx: &mut Cx<Ui>, kit: &Kit, layer: &mut Vec<El>) {
    let Some(open) = &ui.side_menu else {
        return;
    };
    let at = (open.at.x, open.at.y);
    let (x, y) = match open.target {
        MenuTarget::HiddenGroups => cx
            .window_layout_with("mr-hidden-groups", |r| (r[0], r[1] + r[3]))
            .unwrap_or(at),
        _ => at,
    };
    let menu = Menu::new(MENU)
        .at(x, y)
        .open(true)
        .on_close(|ui: &mut Ui| ui.side_menu = None);
    let menu = match &open.target {
        MenuTarget::Row(key) => row_menu(ui, menu, *key, open.sub_open),
        MenuTarget::Group(id) => group_menu(ui, menu, id),
        MenuTarget::HiddenGroups => hidden_menu(ui, menu),
    };
    // What it hands back is its trigger's empty wrapper: nothing to draw.
    if let Some(menu) = menu {
        drop(menu.build(kit, layer));
    }
}

/// A row's menu: "Move to group", whose submenu lists every group but the row's own, in the
/// sidebar's order and the hidden ones after them, then "New group…".
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
                chosen(ui, true);
                let effects = ui.app.move_session(key, &id);
                glue::run(ui, effects);
            }))
        })
        .collect();
    groups.push(MenuEntry::Separator);
    groups.push(MenuEntry::item(MenuItem::new("New group…").on_select_tx(
        move |ui: &mut Ui, tx: &mut Tx| {
            chosen(ui, false);
            let (id, mut effects) = ui.app.new_group("");
            // Moving it saves again, with the new group.
            effects.retain(|effect| !matches!(effect, Effect::SaveState(_)));
            effects.extend(ui.app.move_session(key, &id));
            glue::run(ui, effects);
            start_rename(ui, tx, id);
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
    Some(menu.item(move_to))
}

/// A header's menu: "New session", "Rename", "Hide group", "Delete group".
fn group_menu(ui: &Ui, menu: Menu, id: &GroupId) -> Option<Menu> {
    let section = ui.app.sidebar.sections.iter().find(|s| s.id == *id)?;
    let new_session = {
        let id = id.clone();
        MenuItem::new("New session").on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
            chosen(ui, true);
            self::new_session(ui, tx, &id);
        })
    };
    let rename = {
        let id = id.clone();
        MenuItem::new("Rename").on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
            chosen(ui, false);
            start_rename(ui, tx, id.clone());
        })
    };
    let hide = {
        let id = id.clone();
        MenuItem::new("Hide group").on_select(move |ui: &mut Ui| {
            chosen(ui, true);
            let effects = ui.app.hide_group(&id, true);
            glue::run(ui, effects);
        })
    };
    let delete = {
        let id = id.clone();
        MenuItem::new("Delete group")
            .danger(true)
            .disabled(!section.can_delete)
            .on_select(move |ui: &mut Ui| {
                // A name typed for the group deleted goes with it.
                let keep = ui.renaming.as_ref() != Some(&id);
                chosen(ui, keep);
                if !keep {
                    end_rename(ui, false);
                }
                let effects = ui.app.delete_group(&id);
                glue::run(ui, effects);
            })
    };
    Some(menu.item(new_session).item(rename).item(hide).item(delete))
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
            chosen(ui, true);
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
    use rune::{Key, Mods, Runtime};

    use super::super::conversation;
    use crate::demo::{self, RUNE_CLAUDE, SKETCHES};
    use crate::glue::{self, MenuTarget, Ui};
    use crate::{testkit, theme};

    const FINISHED: &str = "Tidying the sidebar keys";
    const NEEDS_YOU: &str = "Running the window tests";
    const WORKING: &str = "Profiling the long scene";
    const GARDEN: &str = "Planning the spring beds";
    const WEBSITE: &str = "/home/alex/website";

    fn row(key: SessionKey) -> String {
        format!("mr-row-{}", key.0)
    }

    fn group(folder: &str) -> String {
        format!("mr-group-{}", GroupId::Folder(folder.into()).key_text())
    }

    /// The session titled `title`.
    fn titled(rt: &Runtime<Ui>, title: &str) -> SessionKey {
        rt.state
            .app
            .sessions
            .iter()
            .find(|entry| entry.session.title.as_deref() == Some(title))
            .map(|entry| entry.key)
            .unwrap_or_else(|| panic!("no session titled {title:?}"))
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

    /// The mark the line keyed `key` shows: "working" (the kit's spinner), "needs you" (a dot in
    /// the warning colour) or "finished" (a dot in the accent colour), in the kit's status slot
    /// of a row or the app's `mr-mark` of a header. A dot's colour comes in on a spring: settle
    /// the window first.
    fn mark(rt: &Runtime<Ui>, key: &str) -> Option<&'static str> {
        let kit = theme::kit(true);
        let inside = format!("/{key}/");
        let parts: Vec<_> = rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains(&inside))
            .filter(|e| e.path.contains("/status-mark/") || e.path.contains("/mr-mark/"))
            .collect();
        if parts.iter().any(|e| e.role == "progressbar") {
            return Some("working");
        }
        parts.iter().find_map(|e| {
            if e.fill == kit.c.warning.icon {
                Some("needs you")
            } else if e.fill == kit.c.bg_accent {
                Some("finished")
            } else {
                None
            }
        })
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
        let asked = rt.state.app.shown.expect("the parser is shown");
        let other = testkit::add_session(&mut rt, "/home/alex/notes");
        assert_eq!(rt.state.app.shown, Some(other));
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
        assert_eq!(rt.state.app.shown, Some(asking));
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
        let before = conversation::row_builds();
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
        assert_eq!(conversation::row_builds(), before, "no row built again");
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
        let archive = "/home/alex/archive";
        let now = rt.state.app.now_unix;
        let listed: Vec<StoredSession> = (0..2_000)
            .map(|n| StoredSession {
                id: SessionId(format!("archived-{n}")),
                folder: archive.into(),
                title: format!("Archived session {n}"),
                last_active_unix: now - (40 + n) * 86_400,
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
        let id = GroupId::Folder(archive.into());
        let mut effects = ui.app.toggle_older_groups();
        effects.extend(ui.app.show_older(&id));
        glue::run(ui, effects);
        rt.invalidate();
        testkit::step(&mut rt);
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
            assert_eq!(rt.state.app.shown, Some(next));
            assert_eq!(
                focused_key(&rt),
                Some(row(next)),
                "the focus stays in the list"
            );
            assert!(!input_focused(&rt));
        }
        testkit::press(&mut rt, Key::Up);
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.shown, Some(working), "over the headers");
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
            assert_eq!(rt.state.app.shown, Some(chosen));
            assert!(!input_focused(&rt), "moving leaves the focus in the list");
            testkit::press(&mut rt, key.clone());
            testkit::step(&mut rt);
            assert_eq!(rt.state.app.shown, Some(chosen), "{key:?}");
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
        let key = rt.state.app.shown.expect("a session is shown");
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
            let session = rt.state.shown_session().unwrap();
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

    /// The group listed as `name`.
    fn group_named(rt: &Runtime<Ui>, name: &str) -> GroupId {
        rt.state
            .app
            .sidebar
            .sections
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| panic!("no group named {name:?}"))
    }

    fn header_of(id: &GroupId) -> String {
        format!("mr-group-{}", id.key_text())
    }

    /// The group `key`'s session is in.
    fn group_of(rt: &Runtime<Ui>, key: SessionKey) -> GroupId {
        let entry = rt.state.app.sessions.get(key).unwrap();
        rt.state.app.groups.group_of(entry)
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
        let shown = rt.state.app.shown;
        let version = rt.state.app.sidebar_version;
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        right_click(&mut rt, &row(working));
        assert_eq!(
            rt.state.side_menu.as_ref().map(|m| m.target.clone()),
            Some(MenuTarget::Row(working))
        );
        assert_eq!(texts_in(&rt, MENU), ["Move to group"]);
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
        assert_eq!(rt.state.app.shown, shown);
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
        let id = rt.state.renaming.clone().expect("renaming the new group");
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
        assert_eq!(rt.state.renaming, Some(id.clone()));
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
        assert_eq!(rt.state.app.shown, Some(asking));
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
        assert_eq!(rt.state.renaming, Some(id), "still renaming");
    }

    #[test]
    fn starting_another_rename_drops_the_change() {
        let mut rt = testkit::window("sessions");
        header_item(&mut rt, &group(RUNE_CLAUDE), 1);
        testkit::type_text(&mut rt, "Dropped");
        let website = GroupId::Folder(WEBSITE.into());
        header_item(&mut rt, &header_of(&website), 1);
        assert_eq!(rt.state.renaming, Some(website));
        assert_eq!(texts_in(&rt, &group(RUNE_CLAUDE)), ["rune_claude"]);
        assert!(!list_texts(&rt).contains(&"Dropped".to_string()));
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
        let id = rt.state.renaming.clone().expect("renaming the new group");
        assert!(rt.state.app.sidebar.sections.iter().any(|s| s.id == id));
        assert_eq!(focused_key(&rt).as_deref(), Some("mr-group-rename"));
        testkit::type_text(&mut rt, "Errands");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(texts_in(&rt, &header_of(&id)), ["Errands"]);
    }

    #[test]
    fn deleting_a_custom_group_sends_its_sessions_home() {
        let mut rt = testkit::window("sessions");
        let garden = titled(&rt, GARDEN);
        let side = group_named(&rt, "Side projects");
        right_click(&mut rt, &header_of(&side));
        assert_eq!(
            texts_in(&rt, MENU),
            ["New session", "Rename", "Hide group", "Delete group"]
        );
        let kit = theme::kit(true);
        let danger = rt
            .elements()
            .into_iter()
            .find(|e| e.path.contains("/mr-side-menu-item3/") && e.text.is_some())
            .and_then(|e| e.text_color);
        assert_eq!(danger, Some(kit.c.danger.fg), "in the danger colour");
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
    fn a_folder_group_with_nothing_to_reset_cannot_be_deleted() {
        let mut rt = testkit::window("sessions");
        let header = group(RUNE_CLAUDE);
        rt.state.prefs.remove(glue::STATE_KEY).unwrap();
        right_click(&mut rt, &header);
        let item = |rt: &Runtime<Ui>| {
            rt.elements()
                .into_iter()
                .find(|e| !e.exiting && e.key.as_deref() == Some("mr-side-menu-item3"))
                .unwrap()
        };
        assert!(!item(&rt).focusable, "disabled");
        testkit::click(&mut rt, "mr-side-menu-item3");
        testkit::step(&mut rt);
        assert!(list_texts(&rt).contains(&"rune_claude".to_string()));
        assert!(!saved(&rt));
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        // Once it has a name of its own, deleting gives it its folder's name back.
        header_item(&mut rt, &header, 1);
        testkit::type_text(&mut rt, "MindRune");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        right_click(&mut rt, &header);
        assert!(item(&rt).focusable, "something to reset");
        testkit::click(&mut rt, "mr-side-menu-item3");
        testkit::step(&mut rt);
        assert_eq!(texts_in(&rt, &header), ["rune_claude"]);
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
        let shown = rt.state.app.shown.expect("a session is shown");
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
        let (shown, count) = (rt.state.app.shown, rt.state.app.sessions.len());
        let recent = rt.state.app.recent_folders.clone();
        let gone = format!("That folder no longer exists: {RUNE_CLAUDE}");
        testkit::click(&mut rt, &format!("mr-group-add-{}", id.key_text()));
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.shown, shown, "nothing started");
        assert_eq!(rt.state.app.sessions.len(), count);
        assert_eq!(rt.state.app.recent_folders, recent, "not remembered");
        assert_eq!(rt.state.folder_error.as_deref(), Some(gone.as_str()));
        assert!(testkit::has_text(&rt, &gone));
        // The header menu's "New session" does the same.
        rt.state.folder_error = None;
        header_item(&mut rt, &header_of(&id), 0);
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.shown, shown);
        assert_eq!(rt.state.app.sessions.len(), count);
        assert_eq!(rt.state.folder_error.as_deref(), Some(gone.as_str()));
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
        let id = rt.state.renaming.clone().expect("renaming the new group");
        let first = rt.state.app.sidebar.lines[0];
        assert_eq!(
            line_key(&rt.state.app.sidebar, first),
            header_of(&id),
            "first"
        );
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(rt.state.renaming, Some(id.clone()), "still renaming");
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
        assert_eq!(texts_in(&rt, MENU), ["Move to group"]);
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
}
