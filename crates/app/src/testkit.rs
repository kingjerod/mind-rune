//! Helpers for window tests: open a demo scene in a headless runtime, act on it
//! like a person would, and read what it shows.

#![allow(dead_code, reason = "helpers for the view tests of Tasks 11 to 17")]

use std::cell::RefCell;

use mr_agent::{FakeStep, SessionCommand, SessionEvent, SessionId, StoredSession, TurnSummary};
use mr_core::{FromBackend, GroupId, Leaving, Phase, Place, SessionKey};
use rune::clipboard::MemoryClipboard;
use rune::{ElementInfo, Env, Key, KeyEvent, Mods, Runtime};

use crate::glue::{self, SessionView, Ui};
use crate::{demo, view};

/// The frame step, in seconds.
const FRAME: f64 = 0.016;

thread_local! {
    /// The in-memory clipboard of the last window `window` opened on this thread. Each test
    /// runs on its own thread, so tests never see each other's.
    static CLIPBOARD: RefCell<Option<MemoryClipboard>> = const { RefCell::new(None) };
}

/// The demo window at 1200 by 800, dark, scale 1, with `scene` run and three frames stepped.
pub fn window(scene: &str) -> Runtime<Ui> {
    window_sized(scene, (1200.0, 800.0))
}

/// [`window`] at `size`, width by height in points.
pub fn window_sized(scene: &str, size: (f32, f32)) -> Runtime<Ui> {
    let Some((_, run_scene)) = demo::SCENES.iter().find(|(name, _)| *name == scene) else {
        panic!("no scene {scene:?}");
    };
    let mut ui = demo::ui();
    run_scene(&mut ui);
    let mut rt = runtime(ui, size);
    step(&mut rt);
    rt
}

/// The window over `ui` at `size`, as [`window`] opens it, with no frame run yet.
pub fn runtime(ui: Ui, size: (f32, f32)) -> Runtime<Ui> {
    let env = Env {
        system_dark: true,
        scale: 1.0,
    };
    let mut rt = Runtime::new(ui, view::view, size, env);
    rt.poll = Some(glue::poll);
    rt.on_key = Some(view::on_key);
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    CLIPBOARD.with(|c| *c.borrow_mut() = Some(clipboard));
    rt
}

/// `mr-tile-{n}/{inner}`, the path [`find`], [`rect`] and [`click`] take to something inside
/// `key`'s tile. Rune takes a key path only when its leading keys are the element's nearest
/// keyed ancestors, which a tile seldom is: never hand this to a Rune lookup.
pub fn tile(key: SessionKey, inner: &str) -> String {
    format!("{}/{inner}", view::tile_key(key))
}

/// The drawn rectangle `[x, y, w, h]` of the element `key` names, as [`find`] picks it.
pub fn rect(rt: &Runtime<Ui>, key: &str) -> Option<[f32; 4]> {
    match key.split_once('/') {
        Some((tile, _)) if tile.starts_with("mr-tile-") => element(rt, key).map(|e| e.rect),
        _ => rt.presented(key).map(|(rect, _)| rect),
    }
}

/// The element a path from [`tile`] names, as [`find`] picks it.
pub fn element(rt: &Runtime<Ui>, key: &str) -> Option<ElementInfo> {
    let (tile, inner) = key.split_once('/')?;
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && under_tile(&e.path, tile, inner))
}

/// The centre of the element `key` names: as `Runtime::find` takes it, or, for a path from
/// [`tile`], the first element inside that tile whose own key path ends with `inner`. Rune's
/// key paths name an element by its nearest keyed ancestors only, which a tile seldom is.
pub fn find(rt: &Runtime<Ui>, key: &str) -> Option<(f32, f32)> {
    match key.split_once('/') {
        Some((tile, _)) if tile.starts_with("mr-tile-") => {
            rect(rt, key).map(|[x, y, w, h]| (x + w / 2.0, y + h / 2.0))
        }
        _ => rt.find(key),
    }
}

/// Whether the element at `path` sits inside the tile keyed `tile` and its keyed ancestors end
/// with the keys of `inner`, as in `mr-input/scroll/input`.
fn under_tile(path: &str, tile: &str, inner: &str) -> bool {
    let Some((_, below)) = path.split_once(&format!("/{tile}/")) else {
        return false;
    };
    if below
        .rsplit('/')
        .next()
        .is_some_and(|own| own.starts_with('#'))
    {
        return false;
    }
    let mut keyed = below
        .rsplit('/')
        .filter(|segment| !segment.starts_with('#'));
    inner.rsplit('/').all(|want| keyed.next() == Some(want))
}

/// The sessions in the tiles, left to right.
pub fn tile_keys(rt: &Runtime<Ui>) -> Vec<SessionKey> {
    rt.state.app.tiles.keys()
}

/// The row's columns, each its tiles from the top.
pub fn columns(rt: &Runtime<Ui>) -> Vec<Vec<SessionKey>> {
    let columns = rt.state.app.tiles.columns();
    columns
        .iter()
        .map(|column| {
            let under = column.under.map(|under| under.key);
            std::iter::once(column.top).chain(under).collect()
        })
        .collect()
}

/// Whether the keyboard focus is in `key`'s input box.
pub fn box_focused(rt: &Runtime<Ui>, key: SessionKey) -> bool {
    let inside = format!("/{}/", view::tile_key(key));
    rt.elements().into_iter().any(|e| {
        !e.exiting && e.focused && e.path.contains(&inside) && e.path.contains("/mr-input/")
    })
}

/// Ctrl as it is held off macOS, Cmd on macOS.
pub fn ctrl() -> Mods {
    Mods {
        cmd: true,
        ctrl: !cfg!(target_os = "macos"),
        ..Mods::default()
    }
}

/// Ctrl+Shift as they are held off macOS, Cmd+Shift on macOS.
pub fn ctrl_shift() -> Mods {
    Mods {
        shift: true,
        ..ctrl()
    }
}

/// Whether a visible text inside `key`'s tile contains `part`.
pub fn tile_has_text(rt: &Runtime<Ui>, key: SessionKey, part: &str) -> bool {
    let inside = format!("/{}/", view::tile_key(key));
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.visible[2] > 0.0 && e.visible[3] > 0.0)
        .filter(|e| e.path.contains(&inside))
        .filter_map(|e| e.text)
        .any(|t| t.contains(part))
}

/// Moves the pointer onto the element `key` names ([`find`]), presses and releases, and runs a
/// frame.
pub fn click(rt: &mut Runtime<Ui>, key: &str) {
    let Some((x, y)) = find(rt, key) else {
        panic!("no element keyed {key:?}");
    };
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(FRAME);
}

/// The key of `key`'s row in the sidebar.
pub fn row_key(key: SessionKey) -> String {
    format!("mr-row-{}", key.0)
}

/// Presses on the centre of `key`'s sidebar row and moves the pointer to `to` in eight even
/// steps, a frame after each: the row is held there, its release still to come.
pub fn hold_row(rt: &mut Runtime<Ui>, key: SessionKey, to: (f32, f32)) {
    let row = row_key(key);
    let Some(from) = find(rt, &row) else {
        panic!("no element keyed {row:?}");
    };
    rt.pointer_move(from.0, from.1);
    rt.pointer_down(from.0, from.1);
    rt.frame(FRAME);
    move_held(rt, to);
}

/// Moves the pointer from where it is to `to` in eight even steps, a frame after each.
pub fn move_held(rt: &mut Runtime<Ui>, to: (f32, f32)) {
    let from = rt.pointer().expect("the pointer is in the window");
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        rt.pointer_move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        rt.frame(FRAME);
    }
}

/// Lets the held pointer go where it is, then steps.
pub fn release(rt: &mut Runtime<Ui>) {
    let (x, y) = rt.pointer().expect("the pointer is in the window");
    rt.pointer_up(x, y);
    step(rt);
}

/// Drags `key`'s sidebar row to `to` ([`hold_row`]) and lets it go there ([`release`]).
pub fn drag_row(rt: &mut Runtime<Ui>, key: SessionKey, to: (f32, f32)) {
    hold_row(rt, key, to);
    release(rt);
}

/// `count` stored sessions in `/home/alex/archive`, each older than the last, listed whole: the
/// older groups shown and the archive's older sessions with them. Then steps.
pub fn long_list(rt: &mut Runtime<Ui>, count: i64) {
    let archive = "/home/alex/archive";
    let now = rt.state.app.now_unix;
    let listed: Vec<StoredSession> = (0..count)
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
    step(rt);
}

/// Titles of sessions of the `sessions` scene, and the folder of its website group.
pub const FINISHED: &str = "Tidying the sidebar keys";
pub const NEEDS_YOU: &str = "Running the window tests";
pub const WORKING: &str = "Profiling the long scene";
pub const GARDEN: &str = "Planning the spring beds";
pub const CONTACT: &str = "Fixing the contact form";
pub const WEBSITE: &str = "/home/alex/website";

/// The session titled `title`.
pub fn titled(rt: &Runtime<Ui>, title: &str) -> SessionKey {
    rt.state
        .app
        .sessions
        .iter()
        .find(|entry| entry.session.title.as_deref() == Some(title))
        .map(|entry| entry.key)
        .unwrap_or_else(|| panic!("no session titled {title:?}"))
}

/// The listed group named `name`.
pub fn group_named(rt: &Runtime<Ui>, name: &str) -> GroupId {
    rt.state
        .app
        .sidebar
        .sections
        .iter()
        .find(|section| section.name == name)
        .map(|section| section.id.clone())
        .unwrap_or_else(|| panic!("no group named {name:?}"))
}

/// The group `key`'s session is in.
pub fn group_of(rt: &Runtime<Ui>, key: SessionKey) -> GroupId {
    let entry = rt.state.app.sessions.get(key).unwrap();
    rt.state.app.groups.group_of(entry)
}

/// Whether an element keyed exactly `key` is drawn and not on its way out.
pub fn drawn(rt: &Runtime<Ui>, key: &str) -> bool {
    rt.elements()
        .into_iter()
        .any(|e| !e.exiting && e.key.as_deref() == Some(key))
}

/// Clicks the centre of the visible element whose text is exactly `text`, leaving out a
/// picker's trigger (which shows the chosen option's text too), and runs a frame. When several
/// match, the last in tree order is clicked: an open picker's list, in the overlay layer, comes
/// after everything under it.
pub fn click_text(rt: &mut Runtime<Ui>, text: &str) {
    let Some(target) = rt
        .elements()
        .into_iter()
        .rev()
        .filter(|e| !e.exiting && e.visible[2] > 0.0 && e.visible[3] > 0.0)
        .filter(|e| {
            !e.path
                .split('/')
                .any(|segment| segment.ends_with("-trigger"))
        })
        .find(|e| e.text.as_deref() == Some(text))
    else {
        panic!("no visible element shows {text:?}");
    };
    let [x, y, w, h] = target.visible;
    let (x, y) = (x + w / 2.0, y + h / 2.0);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(FRAME);
}

/// Types `text` into whatever has focus, one key per character, and runs a frame.
pub fn type_text(rt: &mut Runtime<Ui>, text: &str) {
    for c in text.chars() {
        rt.key(KeyEvent {
            key: Key::Text(c.to_string()),
            mods: Mods::default(),
        });
    }
    rt.frame(FRAME);
}

/// Presses `key` with no modifiers and runs a frame.
pub fn press(rt: &mut Runtime<Ui>, key: Key) {
    press_with(rt, key, Mods::default());
}

/// Presses `key` with `mods` held and runs a frame.
pub fn press_with(rt: &mut Runtime<Ui>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    rt.frame(FRAME);
}

/// Three frames of 16 ms.
pub fn step(rt: &mut Runtime<Ui>) {
    for _ in 0..3 {
        rt.frame(FRAME);
    }
}

/// The text of every visible element, in tree order.
pub fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.visible[2] > 0.0 && e.visible[3] > 0.0)
        .filter_map(|e| e.text)
        .collect()
}

/// Whether a visible element's text contains `part`.
pub fn has_text(rt: &Runtime<Ui>, part: &str) -> bool {
    texts(rt).iter().any(|t| t.contains(part))
}

/// The text on the window's in-memory clipboard. The clipboard is the one `window` gave the
/// last window it opened on this thread, which in a test is `_rt`'s.
pub fn clipboard(_rt: &Runtime<Ui>) -> Option<String> {
    CLIPBOARD.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(MemoryClipboard::text)
            .map(|t| t.to_string())
    })
}

/// Every command the fake received, in order, without the session it went to.
pub fn sent(rt: &Runtime<Ui>) -> Vec<SessionCommand> {
    fake(rt).sent().into_iter().map(|(_, c)| c).collect()
}

/// The session in the focused tile.
pub fn focused(rt: &Runtime<Ui>) -> Option<SessionKey> {
    rt.state.app.focused()
}

/// The number of the focused session's current open.
pub fn shown_open(rt: &Runtime<Ui>) -> Option<u64> {
    rt.state.app.focused_entry().and_then(|entry| entry.open)
}

/// Emits `event` to the focused session, then steps.
pub fn emit(rt: &mut Runtime<Ui>, event: SessionEvent) {
    let key = focused(rt).expect("a session is shown");
    emit_to(rt, key, event);
}

/// Emits `event` to `key`'s session, shown or not, then steps.
pub fn emit_to(rt: &mut Runtime<Ui>, key: SessionKey, event: SessionEvent) {
    demo::emit_to(&rt.state, key, event);
    step(rt);
}

/// The focused session's items.
pub fn shown_session_items(rt: &Runtime<Ui>) -> &[mr_core::Item] {
    rt.state
        .focused_session()
        .map_or(&[], |s| s.conversation.items())
}

/// The key of the focused session's conversation scroll.
pub fn conversation(rt: &Runtime<Ui>) -> String {
    view::conversation_key(focused(rt).expect("a session is shown"))
}

/// How the focused session is shown: its draft, its open rows, its question drafts.
pub fn shown_view(rt: &Runtime<Ui>) -> &SessionView {
    let key = focused(rt).expect("a session is shown");
    glue::view_of(&rt.state, key)
}

/// How the focused session is shown, lent out to change.
pub fn shown_view_mut(rt: &mut Runtime<Ui>) -> &mut SessionView {
    let key = focused(rt).expect("a session is shown");
    glue::view_mut(&mut rt.state, key)
}

/// A new session in `folder`, shown as "New session" shows one, its program started and one
/// message, "Open {folder}", handed over in a turn that ends at once with nothing said. So it is
/// a session the app keeps when another is shown; the scene's own session, which has nothing
/// said yet, is forgotten the first time this leaves it.
pub fn add_session(rt: &mut Runtime<Ui>, folder: &str) -> SessionKey {
    fake(rt).reply(vec![
        FakeStep::at(0.0, SessionEvent::TurnStarted),
        FakeStep::at(
            0.0,
            SessionEvent::TurnFinished(TurnSummary {
                cost_usd: 0.0,
                context_used: None,
                duration_ms: 1,
                interrupted: false,
            }),
        ),
    ]);
    let ui = &mut rt.state;
    let effects = ui
        .app
        .new_session(folder.into(), Place::Focused, Leaving::ForgetIfEmpty);
    glue::run(ui, effects);
    let key = ui.app.focused().expect("the new session is shown");
    let idle = |ui: &Ui| {
        ui.app
            .sessions
            .get(key)
            .is_some_and(|entry| entry.session.phase == Phase::Idle)
    };
    pump_until(ui, idle);
    let effects = ui.app.submit(key, &format!("Open {folder}"));
    glue::run(ui, effects);
    pump_until(ui, |ui| {
        idle(ui)
            && ui
                .app
                .sessions
                .get(key)
                .is_some_and(|entry| entry.session.is_stored())
    });
    rt.invalidate();
    step(rt);
    key
}

/// A new session in a new empty folder, started as the user starts one: "New session", then
/// "Choose a folder…" when the folder menu opens, and the folder chosen in the picker. Then
/// steps. The folder lasts as long as what this returns.
pub fn new_session_elsewhere(rt: &mut Runtime<Ui>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary folder");
    click(rt, "mr-new-session");
    step(rt);
    if rt.state.folder_menu.is_some() {
        click_text(rt, "Choose a folder…");
        step(rt);
    }
    rt.resolve_file_dialog(Some(dir.path().to_path_buf()))
        .expect("the folder picker is open");
    step(rt);
    dir
}

/// Shows `key`'s session as a click on its row does, its input box focused, then steps.
pub fn show(rt: &mut Runtime<Ui>, key: SessionKey) {
    rt.update(|ui, tx| glue::show(ui, tx, key, Place::Focused, true));
    step(rt);
}

/// Shows `key`'s session in a new tile right of the focused one, as Ctrl+click on its row does,
/// its input box focused, then steps.
pub fn show_side(rt: &mut Runtime<Ui>, key: SessionKey) {
    rt.update(|ui, tx| glue::show(ui, tx, key, Place::Side, true));
    step(rt);
}

/// The key of `part` of the focused session's model picker: `mr-model-{n}-{part}`.
pub fn model_key(rt: &Runtime<Ui>, part: &str) -> String {
    let key = focused(rt).expect("a session is shown");
    format!("mr-model-{}-{part}", key.0)
}

/// The key of `part` of the focused session's permission-mode picker: `mr-mode-{n}-{part}`.
pub fn mode_key(rt: &Runtime<Ui>, part: &str) -> String {
    let key = focused(rt).expect("a session is shown");
    format!("mr-mode-{}-{part}", key.0)
}

/// Pumps the demo's fake until `done` holds. Panics when it never does.
fn pump_until(ui: &mut Ui, done: impl Fn(&Ui) -> bool) {
    for _ in 0..50 {
        if done(ui) {
            return;
        }
        demo::pump(ui, 0.0);
    }
    assert!(done(ui), "the session never got there");
}

fn fake(rt: &Runtime<Ui>) -> &mr_agent::FakeAgent {
    rt.state
        .fake
        .as_ref()
        .expect("the demo window has its fake")
}
