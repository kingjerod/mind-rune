//! Helpers for window tests: open a demo scene in a headless runtime, act on it
//! like a person would, and read what it shows.

#![allow(dead_code, reason = "helpers for the view tests of Tasks 11 to 17")]

use std::cell::RefCell;

use mr_agent::{FakeStep, SessionCommand, SessionEvent, TurnSummary};
use mr_core::{Leaving, Phase, SessionKey};
use rune::clipboard::MemoryClipboard;
use rune::{Env, Key, KeyEvent, Mods, Runtime};

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
    step(&mut rt);
    rt
}

/// Moves the pointer onto the element keyed `key`, presses and releases, and runs a frame.
pub fn click(rt: &mut Runtime<Ui>, key: &str) {
    let Some((x, y)) = rt.find(key) else {
        panic!("no element keyed {key:?}");
    };
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(FRAME);
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

/// The number of the shown session's current open.
pub fn shown_open(rt: &Runtime<Ui>) -> Option<u64> {
    rt.state.app.shown_entry().and_then(|entry| entry.open)
}

/// Emits `event` to the shown session, then steps.
pub fn emit(rt: &mut Runtime<Ui>, event: SessionEvent) {
    let key = rt.state.app.shown.expect("a session is shown");
    emit_to(rt, key, event);
}

/// Emits `event` to `key`'s session, shown or not, then steps.
pub fn emit_to(rt: &mut Runtime<Ui>, key: SessionKey, event: SessionEvent) {
    demo::emit_to(&rt.state, key, event);
    step(rt);
}

/// The shown session's items.
pub fn shown_session_items(rt: &Runtime<Ui>) -> &[mr_core::Item] {
    rt.state
        .shown_session()
        .map_or(&[], |s| s.conversation.items())
}

/// The key of the shown session's conversation scroll.
pub fn conversation(rt: &Runtime<Ui>) -> String {
    view::conversation_key(rt.state.app.shown.expect("a session is shown"))
}

/// How the shown session is shown: its draft, its open rows, its question drafts.
pub fn shown_view(rt: &Runtime<Ui>) -> &SessionView {
    let key = rt.state.app.shown.expect("a session is shown");
    glue::view_of(&rt.state, key)
}

/// How the shown session is shown, lent out to change.
pub fn shown_view_mut(rt: &mut Runtime<Ui>) -> &mut SessionView {
    let key = rt.state.app.shown.expect("a session is shown");
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
    let effects = ui.app.new_session(folder.into(), Leaving::ForgetIfEmpty);
    glue::run(ui, effects);
    let key = ui.app.shown.expect("the new session is shown");
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
    rt.update(|ui, tx| glue::show(ui, tx, key, true));
    step(rt);
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
