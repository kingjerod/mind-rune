//! The folder a new session starts in. "New session" and Ctrl+N open the folder menu: the
//! folders new sessions started in lately and those of the newest sessions, each by its name
//! with its whole path under it, then "Choose a folder…", which opens the desktop's folder
//! picker. A custom group's "New session" and "+" open the same menu, and the session started
//! is placed in that group. With no session shown, the conversation area says how to start one.
//!
//! A new session takes the focused tile's place. One whose folder is chosen with Ctrl held (Cmd
//! on macOS), in the menu or as "Choose a folder…", and every one of the menu Ctrl+Shift+N opens,
//! opens to the side instead: in a new tile right of the focused one, when there is room.
//!
//! When the picker fails, or a folder chosen is gone, a line says so under "New session": in
//! the sidebar while it shows, in the empty conversation area while that is drawn, and when
//! neither is on screen (the sidebar hidden and a session shown), floating under the sidebar's
//! toggle at the window's top left, where the menu opens then too.

use std::path::{Path, PathBuf};

use mr_core::{GroupId, Place};
use rune::{Cx, El, FileDialogOptions, KeyEvent, Tx, col, el, text};
use rune_kit::{Button, ButtonVariant, Kit, Menu, MenuItem};
use rune_theme_opencode::icons::PLUS;

use crate::glue::{self, Ui};
use crate::view::{shell, sidebar};

/// The folder menu. The kit keys its surface `mr-folder-menu` and its items `mr-folder-item{i}`.
const MENU: &str = "mr-folder";

/// The empty area's "New session".
const EMPTY_NEW: &str = "mr-empty-new";

/// What the folder picker is titled.
const PICKER_TITLE: &str = "Choose a folder for the new session";

/// The menu's least width, so that a path under a short name is seldom cut short.
const MENU_WIDTH: f32 = 280.0;

/// How far under what opened it the menu opens.
const GAP: f32 = 4.0;

/// The kit's button that shows and hides the sidebar: with the sidebar hidden and a session
/// shown, the menu and the error line go under it.
const SIDEBAR_TOGGLE: &str = "sidebar-toggle";

/// Where the menu and the error line go when nothing they belong under is on screen.
const CORNER: (f32, f32) = (16.0, 16.0);

/// How wide the floating error line is at most.
const FLOATING_WIDTH: f32 = 360.0;

/// Ctrl+N (Cmd+N on macOS): a new session.
pub fn is_new_session_key(ev: &KeyEvent) -> bool {
    let m = ev.mods;
    ev.is_command('n') && !m.shift && !m.alt && !(cfg!(target_os = "macos") && m.ctrl)
}

/// Ctrl+Shift+N (Cmd+Shift+N on macOS): a new session to the side.
pub fn is_side_session_key(ev: &KeyEvent) -> bool {
    let m = ev.mods;
    ev.is_command('n') && m.shift && !m.alt && !(cfg!(target_os = "macos") && m.ctrl)
}

/// Asks for the folder of a new session, placed in `group` when one is named, and to the side
/// with `side`: opens the folder menu with the app's choices that are folders on the disk now,
/// or, with none, the folder picker. This is a new try, so the picker's last error goes.
pub fn ask(ui: &mut Ui, tx: &mut Tx, group: Option<GroupId>, side: bool) {
    ui.folder_error = None;
    let offers: Vec<PathBuf> = ui
        .app
        .folder_choices()
        .into_iter()
        .filter(|folder| folder.is_dir())
        .collect();
    if offers.is_empty() {
        close(ui);
        pick(ui, tx, group, side_or_focused(side));
        return;
    }
    ui.folder_offers = offers;
    // The menu takes the pointer: a row being dragged never hears its release.
    ui.drag = None;
    ui.folder_menu = Some(group);
    ui.folder_side = side;
}

/// Shuts the folder menu, and with it its asking for a session to the side.
pub fn close(ui: &mut Ui) {
    ui.folder_menu = None;
    ui.folder_offers.clear();
    ui.folder_side = false;
}

/// To the side when `side`, else in the focused tile.
pub(super) fn side_or_focused(side: bool) -> Place {
    if side { Place::Side } else { Place::Focused }
}

/// Where the session of a choice made in the open folder menu goes: to the side when the menu
/// was opened for that or Ctrl is held as it is chosen.
fn chosen_place(ui: &Ui, tx: &Tx) -> Place {
    side_or_focused(ui.folder_side || tx.mods().cmd)
}

/// The desktop's folder picker, starting in the focused session's folder or else the home
/// folder.
/// The folder chosen gets a new session at `place`, placed in `group` when one is named. A
/// cancel does nothing; an error is kept, to be shown under the button.
fn pick(ui: &mut Ui, tx: &mut Tx, group: Option<GroupId>, place: Place) {
    let directory = ui.app.focused_entry().map_or_else(
        || ui.app.home().to_path_buf(),
        |entry| entry.session.folder.clone(),
    );
    let options = FileDialogOptions {
        title: PICKER_TITLE.into(),
        directory: Some(directory),
        ..FileDialogOptions::default()
    };
    tx.open_folder(
        options,
        move |ui: &mut Ui, result, tx: &mut Tx| match result {
            Ok(Some(folder)) => start_chosen(ui, tx, folder, group.as_ref(), place),
            Ok(None) => {}
            Err(error) => {
                ui.folder_error = Some(format!("The folder picker could not be opened: {error}"));
            }
        },
    );
}

/// [`start`] in a folder the user chose from the menu or in the picker, or a folder group's
/// own, when it is still a folder. This is a new try, so an earlier error goes. When it is
/// gone, nothing starts, the recent folders stay as they are, and a line says so where the
/// picker's error goes.
pub(super) fn start_chosen(
    ui: &mut Ui,
    tx: &mut Tx,
    folder: PathBuf,
    group: Option<&GroupId>,
    place: Place,
) {
    ui.folder_error = None;
    if !folder.is_dir() {
        ui.folder_error = Some(format!(
            "That folder no longer exists: {}",
            folder.display()
        ));
        return;
    }
    start(ui, tx, folder, group, place);
}

/// Starts a new session in `folder`, placed in `group` when one is named, and shows it at once
/// at `place`, as `glue::show` shows one (the conversation left does not fade out over it, and
/// it stays while text is typed in it). The session focused afterwards has its tile and its
/// input box focused: the new one, or, when the focused session is an empty one in that folder
/// already, that one, which stays.
pub fn start(ui: &mut Ui, tx: &mut Tx, folder: PathBuf, group: Option<&GroupId>, place: Place) {
    let before = ui.app.focused();
    let leaving = glue::leaving(ui, place);
    let effects = match group {
        Some(group) => ui.app.new_session_in(folder, group, place, leaving),
        None => ui.app.new_session(folder, place, leaving),
    };
    glue::run(ui, effects);
    if ui.app.focused() != before {
        tx.instant();
    }
    if let Some(key) = ui.app.focused() {
        glue::focus_tile(ui, key, true);
    }
}

/// The folder menu, while it is open, into `layer`. It opens under "New session", or for a
/// group under that group's "+".
pub fn menu(ui: &Ui, cx: &mut Cx<Ui>, kit: &Kit, layer: &mut Vec<El>) {
    let Some(group) = &ui.folder_menu else {
        return;
    };
    let (x, y) = place(ui, cx, group.as_ref());
    let mut menu = Menu::new(MENU)
        .at(x, y)
        .min_width(MENU_WIDTH)
        .open(true)
        .on_close(close);
    for folder in &ui.folder_offers {
        let chosen = folder.clone();
        let group = group.clone();
        let item = MenuItem::new(name(folder))
            .description(folder.display().to_string())
            .on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
                let place = chosen_place(ui, tx);
                close(ui);
                start_chosen(ui, tx, chosen.clone(), group.as_ref(), place);
            });
        menu = menu.item(item);
    }
    let group = group.clone();
    let choose = MenuItem::new("Choose a folder…").on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
        let place = chosen_place(ui, tx);
        close(ui);
        pick(ui, tx, group.clone(), place);
    });
    // What it hands back is its trigger's empty wrapper: nothing to draw.
    drop(menu.separator().item(choose).build(kit, layer));
}

/// The window point the menu opens at: under the left end of the group's "+", or of "New
/// session" in the sidebar when it shows, or of the empty area's "New session", or of the
/// sidebar's toggle.
fn place(ui: &Ui, cx: &mut Cx<Ui>, group: Option<&GroupId>) -> (f32, f32) {
    let under = |key: &str| under(cx, key);
    group
        .and_then(|id| under(&sidebar::add_key(id)))
        .or_else(|| {
            ui.sidebar
                .is_open()
                .then(|| under(shell::NEW_SESSION))
                .flatten()
        })
        .or_else(|| under(EMPTY_NEW))
        .or_else(|| under(SIDEBAR_TOGGLE))
        .unwrap_or(CORNER)
}

/// The window point just under the left end of the element keyed `key`, when it is laid out.
fn under(cx: &Cx<Ui>, key: &str) -> Option<(f32, f32)> {
    cx.window_layout_with(key, |r| (r[0], r[1] + r[3] + GAP))
}

/// A folder's name: its last part, or the whole path when it has none (`/`).
fn name(folder: &Path) -> String {
    folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The error line, keyed `key`: why the folder picker could not be used, or that a folder
/// chosen is gone.
fn error_line(error: &str, key: &str, kit: &Kit) -> El {
    text(error.to_string())
        .key(key.to_string())
        .font(kit.ty.small)
        .color(kit.c.danger.fg)
        .wrap()
}

/// The error line for under the sidebar's "New session", while there is one.
pub fn error(ui: &Ui, kit: &Kit) -> Option<El> {
    let error = ui.folder_error.as_ref()?;
    Some(error_line(error, "mr-folder-error", kit).w_full().px(8.0))
}

/// The error line, while there is one and neither "New session" is on screen (the sidebar is
/// hidden and a tile is shown): into `layer`, floating under the sidebar's toggle.
pub fn floating_error(ui: &Ui, cx: &mut Cx<Ui>, kit: &Kit, layer: &mut Vec<El>) {
    let Some(error) = &ui.folder_error else {
        return;
    };
    if ui.sidebar.is_open() || ui.app.focused().is_none() {
        return;
    }
    let (x, y) = under(cx, SIDEBAR_TOGGLE).unwrap_or(CORNER);
    layer.push(
        el().key("mr-folder-error-float")
            .absolute(x, y)
            .w(FLOATING_WIDTH)
            .px(10.0)
            .py(6.0)
            .radius(6.0)
            .fill(kit.c.layer_01)
            .border(1.0, kit.c.border_base)
            .child(error_line(error, "mr-folder-error-text", kit).w_full()),
    );
}

/// The conversation area while no session is shown: what to do, and a "New session" that does
/// what the sidebar's does, with the error line under it while there is one.
pub fn empty(ui: &Ui, kit: &Kit) -> El {
    let new_session = Button::new("New session")
        .icon(&PLUS)
        .variant(ButtonVariant::Neutral)
        .on_press_tx(|ui: &mut Ui, tx: &mut Tx| ask(ui, tx, None, false))
        .build(kit)
        .key(EMPTY_NEW)
        .mt(10.0);
    col()
        .key("mr-empty")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .center()
        .gap(6.0)
        .px(24.0)
        .child(
            text("No session is open.")
                .key("mr-empty-title")
                .font(kit.ty.body_medium)
                .color(kit.c.text_base),
        )
        .child(
            text("Choose a folder to start one.")
                .key("mr-empty-line")
                .font(kit.ty.body)
                .color(kit.c.text_muted),
        )
        .child(new_session)
        .children(
            ui.folder_error
                .as_ref()
                .map(|error| error_line(error, "mr-empty-error", kit).max_w(FLOATING_WIDTH)),
        )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use mr_agent::SessionCommand;
    use mr_core::{GroupId, Phase, SessionKey};
    use rune::{FileDialogKind, Key, Mods, PhysicalInput, Runtime};

    use crate::demo;
    use crate::glue::{self, Ui};
    use crate::testkit::{self, box_focused, group_of, tile_keys};

    /// The folder menu's surface, as the kit keys it.
    const MENU: &str = "mr-folder-menu";

    /// Folders named `names` made in `dir`, which exist until it goes.
    fn folders<const N: usize>(dir: &tempfile::TempDir, names: [&str; N]) -> [PathBuf; N] {
        names.map(|name| {
            let path = dir.path().join(name);
            fs::create_dir(&path).unwrap();
            path
        })
    }

    fn shown_text(path: &Path) -> String {
        path.display().to_string()
    }

    /// The texts inside the element keyed `key`, not on their way out.
    fn texts_in(rt: &Runtime<Ui>, key: &str) -> Vec<String> {
        let inside = format!("/{key}/");
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains(&inside))
            .filter_map(|e| e.text)
            .collect()
    }

    /// The surface keyed `key` is on screen and not on its way out.
    fn open(rt: &Runtime<Ui>, key: &str) -> bool {
        rt.elements()
            .iter()
            .any(|e| !e.exiting && e.key.as_deref() == Some(key))
    }

    fn rect(rt: &Runtime<Ui>, key: &str) -> [f32; 4] {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no element keyed {key:?}"))
            .rect
    }

    fn inside_the_window(rt: &Runtime<Ui>, key: &str) -> bool {
        let [x, y, w, h] = rect(rt, key);
        x >= 0.0 && y >= 0.0 && x + w <= 1200.0 && y + h <= 800.0
    }

    fn input_focused(rt: &Runtime<Ui>) -> bool {
        rt.elements()
            .iter()
            .any(|e| e.focused && e.path.contains("/mr-input/"))
    }

    fn shown(rt: &Runtime<Ui>) -> SessionKey {
        testkit::focused(rt).expect("a session is shown")
    }

    fn folder_of(rt: &Runtime<Ui>, key: SessionKey) -> PathBuf {
        rt.state
            .app
            .sessions
            .get(key)
            .unwrap()
            .session
            .folder
            .clone()
    }

    /// `key`'s session was shown with its program asked for, and the program started.
    fn started(rt: &mut Runtime<Ui>, key: SessionKey) {
        assert!(rt.state.app.sessions.get(key).unwrap().open.is_some());
        demo::pump(&mut rt.state, 0.0);
        let entry = rt.state.app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, Phase::Idle, "its program started");
    }

    /// Ctrl+N (Cmd+N on macOS).
    fn ctrl_n(rt: &mut Runtime<Ui>) {
        testkit::press_with(rt, Key::Command('n'), testkit::ctrl());
        testkit::step(rt);
    }

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

    #[test]
    fn new_session_offers_the_recent_folders() {
        let dir = tempfile::tempdir().unwrap();
        let [notes, gone, website] = folders(&dir, ["notes", "gone", "website"]);
        // No session, so the recent folders are all there is to offer.
        let mut rt = testkit::window("empty-start");
        rt.state.app.recent_folders = vec![notes.clone(), gone.clone(), website.clone()];
        fs::remove_dir(&gone).unwrap();
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_menu, Some(None));
        assert!(rt.file_dialog().is_none(), "no picker yet");
        assert_eq!(
            texts_in(&rt, MENU),
            [
                "notes".to_string(),
                shown_text(&notes),
                "website".to_string(),
                shown_text(&website),
                "Choose a folder…".to_string(),
            ],
            "the gone folder is not offered"
        );
        assert_eq!(texts_in(&rt, "mr-folder-item3"), ["Choose a folder…"]);
        let button = rect(&rt, "mr-new-session");
        let menu = rect(&rt, MENU);
        assert!(menu[1] >= button[1] + button[3] - 0.5, "under the button");
        assert!((menu[0] - button[0]).abs() < 1.0, "at its left edge");
        assert!(inside_the_window(&rt, MENU));
    }

    #[test]
    fn choosing_a_recent_folder_starts_a_session_there() {
        let dir = tempfile::tempdir().unwrap();
        let [notes, website] = folders(&dir, ["notes", "website"]);
        let mut rt = testkit::window("conversation");
        let old = shown(&rt);
        rt.state.app.recent_folders = vec![notes.clone(), website.clone()];
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        testkit::click(&mut rt, "mr-folder-item1");
        testkit::step(&mut rt);
        let key = shown(&rt);
        assert_ne!(key, old, "a new session is shown");
        assert_eq!(folder_of(&rt, key), website);
        assert_eq!(group_of(&rt, key), GroupId::Folder(website.clone()));
        assert_eq!(rt.state.app.recent_folders[0], website, "first now");
        assert_eq!(rt.state.folder_menu, None);
        assert!(!open(&rt, MENU));
        assert!(input_focused(&rt), "its input box has the focus");
        assert!(rt.find(&format!("mr-row-{}", key.0)).is_some());
        assert!(
            !testkit::sent(&rt).contains(&SessionCommand::Close),
            "the old session keeps its program"
        );
        started(&mut rt, key);
    }

    #[test]
    fn choose_a_folder_opens_the_picker() {
        let dir = tempfile::tempdir().unwrap();
        let [notes, picked] = folders(&dir, ["notes", "picked"]);
        let mut rt = testkit::window("conversation");
        let old = shown(&rt);
        rt.state.app.recent_folders = vec![notes];
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        testkit::click_text(&mut rt, "Choose a folder…");
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_menu, None, "the menu shuts");
        let (kind, options) = rt.file_dialog().expect("the picker is open");
        assert_eq!(kind, FileDialogKind::Folder);
        assert_eq!(options.title, "Choose a folder for the new session");
        assert_eq!(
            options.directory.as_deref(),
            Some(Path::new("/home/alex/project")),
            "the shown session's folder"
        );
        rt.resolve_file_dialog(None).unwrap();
        testkit::step(&mut rt);
        assert_eq!(shown(&rt), old, "cancel starts nothing");
        assert_eq!(rt.state.app.sessions.len(), 1);
        assert_eq!(rt.state.folder_error, None);

        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        testkit::click_text(&mut rt, "Choose a folder…");
        testkit::step(&mut rt);
        rt.resolve_file_dialog(Some(picked.clone())).unwrap();
        testkit::step(&mut rt);
        let key = shown(&rt);
        assert_ne!(key, old);
        assert_eq!(folder_of(&rt, key), picked);
        assert_eq!(rt.state.app.recent_folders[0], picked);
        assert!(input_focused(&rt));
        started(&mut rt, key);
    }

    #[test]
    fn with_no_recent_folder_the_button_opens_the_picker() {
        let dir = tempfile::tempdir().unwrap();
        let [picked] = folders(&dir, ["picked"]);
        let mut rt = testkit::window("empty-start");
        assert!(rt.state.app.folder_choices().is_empty());
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_menu, None);
        assert!(!open(&rt, MENU));
        let (kind, options) = rt.file_dialog().expect("the picker opens at once");
        assert_eq!(kind, FileDialogKind::Folder);
        assert_eq!(
            options.directory.as_deref(),
            Some(Path::new("/home/alex")),
            "with nothing shown, the home folder"
        );
        rt.resolve_file_dialog(Some(picked.clone())).unwrap();
        testkit::step(&mut rt);
        let key = shown(&rt);
        assert_eq!(folder_of(&rt, key), picked);
        assert!(input_focused(&rt));
        started(&mut rt, key);
    }

    #[test]
    fn a_picker_error_is_shown_under_the_button() {
        let mut rt = testkit::window("empty-start");
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        assert!(rt.file_dialog().is_some());
        // Rune opens one dialog at a time: asking again while it is open fails.
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        let error = "The folder picker could not be opened: a file dialog is already open";
        assert_eq!(rt.state.folder_error.as_deref(), Some(error));
        let shown_error = rt
            .elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some("mr-folder-error"))
            .expect("the error is drawn");
        assert_eq!(shown_error.text.as_deref(), Some(error));
        let button = rect(&rt, "mr-new-session");
        let below = rect(&rt, "mr-folder-error");
        assert!(below[1] >= button[1] + button[3] - 0.5, "under the button");
        assert!(
            below[0] + below[2] <= rt.state.sidebar.width() + 0.5,
            "in the sidebar"
        );
        assert!(testkit::focused(&rt).is_none(), "nothing started");
        // Until the next try.
        rt.resolve_file_dialog(None).unwrap();
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_error.as_deref(), Some(error), "still there");
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_error, None);
        assert!(rt.find("mr-folder-error").is_none());
        assert!(rt.file_dialog().is_some());
    }

    /// The text of the element keyed `key`, when it is drawn and on screen inside the window.
    fn on_screen(rt: &Runtime<Ui>, key: &str) -> Option<String> {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key))
            .filter(|e| {
                let [x, y, w, h] = e.visible;
                w > 0.0 && h > 0.0 && x >= 0.0 && y >= 0.0 && x + w <= 1200.0 && y + h <= 800.0
            })
            .map(|e| e.text.unwrap_or_default())
    }

    /// Hides the sidebar and lets it land.
    fn hide_sidebar(rt: &mut Runtime<Ui>) {
        rt.state.sidebar.set_open(false);
        rt.invalidate();
        rt.settle_all();
        testkit::step(rt);
        assert!(!rt.state.sidebar.is_open());
    }

    const BUSY: &str = "The folder picker could not be opened: a file dialog is already open";

    #[test]
    fn a_picker_error_shows_with_the_sidebar_hidden() {
        // Nothing shown: under the empty area's button.
        let mut rt = testkit::window("empty-start");
        hide_sidebar(&mut rt);
        testkit::click(&mut rt, "mr-empty-new");
        testkit::step(&mut rt);
        assert!(rt.file_dialog().is_some(), "no folder to offer: the picker");
        ctrl_n(&mut rt);
        assert_eq!(rt.state.folder_error.as_deref(), Some(BUSY));
        assert_eq!(on_screen(&rt, "mr-empty-error").as_deref(), Some(BUSY));
        rt.settle_all();
        testkit::step(&mut rt);
        let button = rect(&rt, "mr-empty-new");
        let line = rect(&rt, "mr-empty-error");
        assert!(
            line[1] >= button[1] + button[3] - 0.5,
            "under the button: {line:?} {button:?}"
        );
        assert_eq!(
            on_screen(&rt, "mr-folder-error"),
            None,
            "the sidebar is hidden"
        );
        assert!(rt.find("mr-folder-error-float").is_none());
        rt.resolve_file_dialog(None).unwrap();
        testkit::step(&mut rt);

        // A session shown and the sidebar hidden: no "New session" is on screen, so the line
        // floats under the sidebar's toggle. The session's folder is gone, so Ctrl+N goes
        // straight to the picker.
        let dir = tempfile::tempdir().unwrap();
        let [gone] = folders(&dir, ["gone"]);
        testkit::add_session(&mut rt, gone.to_str().unwrap());
        fs::remove_dir(&gone).unwrap();
        hide_sidebar(&mut rt);
        assert!(testkit::focused(&rt).is_some());
        ctrl_n(&mut rt);
        assert!(rt.file_dialog().is_some());
        ctrl_n(&mut rt);
        assert_eq!(rt.state.folder_error.as_deref(), Some(BUSY));
        assert!(on_screen(&rt, "mr-folder-error-float").is_some());
        assert_eq!(
            on_screen(&rt, "mr-folder-error-text").as_deref(),
            Some(BUSY)
        );
        let toggle = rect(&rt, "sidebar-toggle");
        let line = rect(&rt, "mr-folder-error-float");
        assert!(line[1] >= toggle[1] + toggle[3] - 0.5, "under the toggle");
        assert_eq!(on_screen(&rt, "mr-folder-error"), None);
        assert!(rt.find("mr-empty-error").is_none());
        // With the sidebar shown again, the line is under its button, and floats no more.
        rt.state.sidebar.set_open(true);
        rt.invalidate();
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(on_screen(&rt, "mr-folder-error").as_deref(), Some(BUSY));
        assert!(rt.find("mr-folder-error-float").is_none());
    }

    #[test]
    fn a_folder_gone_since_it_was_offered_starts_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let [notes, website] = folders(&dir, ["notes", "website"]);
        let mut rt = testkit::window("conversation");
        let old = shown(&rt);
        rt.state.app.recent_folders = vec![notes.clone(), website.clone()];
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        fs::remove_dir(&website).unwrap();
        testkit::click(&mut rt, "mr-folder-item1");
        testkit::step(&mut rt);
        let gone = format!("That folder no longer exists: {}", website.display());
        assert_eq!(shown(&rt), old, "nothing started");
        assert_eq!(rt.state.app.sessions.len(), 1);
        assert_eq!(
            rt.state.app.recent_folders,
            [notes.clone(), website.clone()]
        );
        assert_eq!(rt.state.folder_menu, None);
        assert_eq!(
            on_screen(&rt, "mr-folder-error").as_deref(),
            Some(gone.as_str())
        );

        // The picker's answer is checked the same way.
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_error, None, "a new try");
        testkit::click_text(&mut rt, "Choose a folder…");
        testkit::step(&mut rt);
        rt.resolve_file_dialog(Some(website.clone())).unwrap();
        testkit::step(&mut rt);
        assert_eq!(shown(&rt), old);
        assert_eq!(rt.state.app.sessions.len(), 1);
        assert_eq!(rt.state.app.recent_folders, [notes, website]);
        assert_eq!(
            on_screen(&rt, "mr-folder-error").as_deref(),
            Some(gone.as_str())
        );
    }

    #[test]
    fn a_new_session_with_text_typed_is_kept_when_left() {
        let dir = tempfile::tempdir().unwrap();
        let [notes, website] = folders(&dir, ["notes", "website"]);
        let mut rt = testkit::window("conversation");
        let old = shown(&rt);
        let row = |key: SessionKey| format!("mr-row-{}", key.0);
        rt.state.app.recent_folders = vec![notes.clone(), website.clone()];
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        testkit::click(&mut rt, "mr-folder-item0");
        testkit::step(&mut rt);
        let typed = shown(&rt);
        assert_ne!(typed, old);
        let draft = "Three paragraphs about the parser";
        testkit::type_text(&mut rt, draft);
        // A click on another row: it stays, with its text.
        testkit::click(&mut rt, &row(old));
        testkit::step(&mut rt);
        assert_eq!(shown(&rt), old);
        assert!(rt.state.app.sessions.get(typed).is_some(), "kept");
        assert!(rt.find(&row(typed)).is_some(), "its row is listed");
        assert!(!testkit::sent(&rt).contains(&SessionCommand::Close));
        testkit::click(&mut rt, &row(typed));
        testkit::step(&mut rt);
        assert_eq!(shown(&rt), typed);
        assert_eq!(glue::view_of(&rt.state, typed).draft.text, draft);
        // Ctrl+N into another folder: it stays too.
        ctrl_n(&mut rt);
        testkit::click(&mut rt, "mr-folder-item1");
        testkit::step(&mut rt);
        let other = shown(&rt);
        assert_eq!(folder_of(&rt, other), website);
        assert!(rt.state.app.sessions.get(typed).is_some(), "kept again");
        // Left with nothing typed, a new session is forgotten as before.
        testkit::click(&mut rt, &row(old));
        testkit::step(&mut rt);
        assert!(rt.state.app.sessions.get(other).is_none(), "forgotten");
        // White space alone is nothing typed: once emptied, the kept one goes when left.
        testkit::click(&mut rt, &row(typed));
        testkit::step(&mut rt);
        glue::view_mut(&mut rt.state, typed)
            .draft
            .replace_text(" \n\t ");
        testkit::click(&mut rt, &row(old));
        testkit::step(&mut rt);
        assert!(rt.state.app.sessions.get(typed).is_none(), "forgotten now");
        assert_eq!(rt.state.app.sessions.len(), 1);
    }

    #[test]
    fn ctrl_n_opens_the_folder_menu() {
        let dir = tempfile::tempdir().unwrap();
        let [notes] = folders(&dir, ["notes"]);
        // A request waits for the user, in a turn that runs.
        let mut rt = testkit::window("permission");
        let key = shown(&rt);
        rt.state.app.recent_folders = vec![notes.clone()];
        assert!(input_focused(&rt), "the input box has the focus");
        let before = testkit::sent(&rt).len();
        ctrl_n(&mut rt);
        assert_eq!(rt.state.folder_menu, Some(None));
        assert!(open(&rt, MENU));
        assert_eq!(
            texts_in(&rt, "mr-folder-item0"),
            ["notes".to_string(), shown_text(&notes)]
        );
        assert_eq!(
            glue::view_of(&rt.state, key).draft.text,
            "",
            "nothing typed"
        );
        // Escape shuts the menu, and stops nothing.
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_menu, None);
        assert!(!open(&rt, MENU));
        let entry = rt.state.app.sessions.get(key).unwrap();
        assert_eq!(entry.session.phase, Phase::NeedsUser, "the turn goes on");
        assert_eq!(testkit::sent(&rt)[before..], [], "no Stop, no Answer");
        // With nothing focused too.
        rt.update(|_, tx| tx.blur());
        testkit::step(&mut rt);
        ctrl_n(&mut rt);
        assert_eq!(rt.state.folder_menu, Some(None));
        assert_eq!(testkit::sent(&rt)[before..], []);
        assert!(rt.find("mr-allow-r1").is_some(), "the request still waits");
    }

    #[test]
    fn a_custom_groups_plus_asks_for_a_folder_and_places_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let [notes, picked] = folders(&dir, ["notes", "picked"]);
        let mut rt = testkit::window("sessions");
        rt.state.app.recent_folders = vec![notes.clone()];
        let side = rt
            .state
            .app
            .sidebar
            .sections
            .iter()
            .find(|s| s.name == "Side projects")
            .map(|s| s.id.clone())
            .unwrap();
        let plus = format!("mr-group-add-{}", side.key_text());
        testkit::click(&mut rt, &plus);
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_menu, Some(Some(side.clone())));
        let texts = texts_in(&rt, MENU);
        assert_eq!(texts[..2], ["notes".to_string(), shown_text(&notes)]);
        assert_eq!(texts.last().map(String::as_str), Some("Choose a folder…"));
        let at = rect(&rt, &plus);
        let menu = rect(&rt, MENU);
        assert!(menu[1] >= at[1] + at[3] - 0.5, "under the plus");
        assert!(inside_the_window(&rt, MENU));
        testkit::click(&mut rt, "mr-folder-item0");
        testkit::step(&mut rt);
        let first = shown(&rt);
        assert_eq!(folder_of(&rt, first), notes);
        assert_eq!(group_of(&rt, first), side, "placed in the group");
        assert!(input_focused(&rt));
        started(&mut rt, first);

        // The header's "New session" asks too, and the picker's folder is placed the same way.
        let header = format!("mr-group-{}", side.key_text());
        right_click(&mut rt, &header);
        let item = rt
            .elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some("mr-side-menu-item0"))
            .unwrap();
        assert!(item.focusable, "enabled");
        testkit::click(&mut rt, "mr-side-menu-item0");
        testkit::step(&mut rt);
        assert!(rt.state.side_menu.is_none());
        assert_eq!(rt.state.folder_menu, Some(Some(side.clone())));
        testkit::click_text(&mut rt, "Choose a folder…");
        testkit::step(&mut rt);
        rt.resolve_file_dialog(Some(picked.clone())).unwrap();
        testkit::step(&mut rt);
        let second = shown(&rt);
        assert_ne!(second, first);
        assert_eq!(folder_of(&rt, second), picked);
        assert_eq!(group_of(&rt, second), side);
    }

    #[test]
    fn with_nothing_shown_the_window_says_how_to_start() {
        let dir = tempfile::tempdir().unwrap();
        let [notes] = folders(&dir, ["notes"]);
        let mut rt = testkit::window("empty-start");
        assert_eq!(testkit::focused(&rt), None);
        assert_eq!(
            texts_in(&rt, "mr-empty"),
            [
                "No session is open.",
                "Choose a folder to start one.",
                "New session"
            ]
        );
        assert!(inside_the_window(&rt, "mr-empty"));
        rt.state.app.recent_folders = vec![notes.clone()];
        testkit::click(&mut rt, "mr-empty-new");
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_menu, Some(None), "as the sidebar's does");
        testkit::click(&mut rt, "mr-folder-item0");
        testkit::step(&mut rt);
        let key = shown(&rt);
        assert_eq!(folder_of(&rt, key), notes);
        assert!(rt.find("mr-empty").is_none());
        assert!(input_focused(&rt));
    }

    // Open to the side: Task 9 of plan 4.

    /// A window wide enough for a second tile beside the first.
    const WIDE: (f32, f32) = (1800.0, 900.0);

    /// Holds Ctrl (Cmd on macOS), or lets it go, and steps.
    fn hold_ctrl(rt: &mut Runtime<Ui>, held: bool) {
        let mods = if held {
            testkit::ctrl()
        } else {
            Mods::default()
        };
        rt.physical_input(PhysicalInput::Modifiers(mods));
        testkit::step(rt);
    }

    #[test]
    fn a_new_session_opens_to_the_side_with_ctrl() {
        let dir = tempfile::tempdir().unwrap();
        let [notes] = folders(&dir, ["notes"]);
        let mut rt = testkit::window_sized("conversation", WIDE);
        let old = shown(&rt);
        rt.state.app.recent_folders = vec![notes.clone()];
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        hold_ctrl(&mut rt, true);
        testkit::click(&mut rt, "mr-folder-item0");
        hold_ctrl(&mut rt, false);
        let new = shown(&rt);
        assert_ne!(new, old);
        assert_eq!(folder_of(&rt, new), notes);
        assert_eq!(tile_keys(&rt), [old, new], "beside the one there");
        assert_eq!(rt.state.folder_menu, None);
        assert!(box_focused(&rt, new), "its input box has the focus");
        assert!(!box_focused(&rt, old));
        started(&mut rt, new);
    }

    #[test]
    fn a_new_session_with_ctrl_and_no_room_takes_the_focused_tile() {
        let dir = tempfile::tempdir().unwrap();
        let [notes] = folders(&dir, ["notes"]);
        let mut rt = testkit::window_sized("conversation", (1000.0, 800.0));
        let old = shown(&rt);
        assert_eq!(tile_keys(&rt), [old]);
        assert!(!rt.state.app.room_for_tile());
        rt.state.app.recent_folders = vec![notes.clone()];
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        hold_ctrl(&mut rt, true);
        testkit::click(&mut rt, "mr-folder-item0");
        hold_ctrl(&mut rt, false);
        let new = shown(&rt);
        assert_ne!(new, old);
        assert_eq!(folder_of(&rt, new), notes);
        assert_eq!(tile_keys(&rt), [new], "one tile, holding the new session");
        assert!(box_focused(&rt, new));
        started(&mut rt, new);
    }

    #[test]
    fn a_folder_picked_with_ctrl_opens_to_the_side() {
        let dir = tempfile::tempdir().unwrap();
        let [notes, picked] = folders(&dir, ["notes", "picked"]);
        let mut rt = testkit::window_sized("conversation", WIDE);
        let old = shown(&rt);
        rt.state.app.recent_folders = vec![notes];
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        hold_ctrl(&mut rt, true);
        testkit::click_text(&mut rt, "Choose a folder…");
        hold_ctrl(&mut rt, false);
        assert_eq!(rt.state.folder_menu, None, "the menu shuts");
        assert!(!rt.state.folder_side);
        rt.resolve_file_dialog(Some(picked.clone())).unwrap();
        testkit::step(&mut rt);
        let new = shown(&rt);
        assert_eq!(folder_of(&rt, new), picked);
        assert_eq!(tile_keys(&rt), [old, new]);
        assert!(box_focused(&rt, new));
        started(&mut rt, new);
    }

    /// Ctrl+Shift+N (Cmd+Shift+N on macOS).
    fn ctrl_shift_n(rt: &mut Runtime<Ui>) {
        testkit::press_with(rt, Key::Command('n'), testkit::ctrl_shift());
        testkit::step(rt);
    }

    #[test]
    fn a_cancelled_side_menu_does_not_stick() {
        let dir = tempfile::tempdir().unwrap();
        let [notes] = folders(&dir, ["notes"]);
        let mut rt = testkit::window_sized("conversation", WIDE);
        let old = shown(&rt);
        rt.state.app.recent_folders = vec![notes.clone()];
        ctrl_shift_n(&mut rt);
        assert_eq!(rt.state.folder_menu, Some(None));
        assert!(rt.state.folder_side, "for a session to the side");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(rt.state.folder_menu, None);
        assert!(!rt.state.folder_side, "gone with the menu");
        // The picker it opens, cancelled, leaves nothing behind either.
        ctrl_shift_n(&mut rt);
        testkit::click_text(&mut rt, "Choose a folder…");
        testkit::step(&mut rt);
        rt.resolve_file_dialog(None).unwrap();
        testkit::step(&mut rt);
        assert!(!rt.state.folder_side);
        assert_eq!(tile_keys(&rt), [old], "cancel starts nothing");
        // A plain new session afterwards takes the focused tile.
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        assert!(!rt.state.folder_side);
        testkit::click(&mut rt, "mr-folder-item0");
        testkit::step(&mut rt);
        let new = shown(&rt);
        assert_ne!(new, old);
        assert_eq!(tile_keys(&rt), [new], "one tile");
    }
}
