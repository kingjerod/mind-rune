//! The tests of the right panel.

use std::fs;

use mr_agent::{
    ContextBreakdown, ContextKind, ContextPart, HistoryItem, LimitWindow, PermissionMode,
    RemoteAccess, RemoteFailure, SessionCommand, SessionEvent, SessionId, StoredHistory,
    ToolCallId, ToolDetail, ToolOutcome, TurnSummary, UsageLimits,
};
use mr_core::{Branch, FromBackend, HistoryState, Level, Remote, SessionKey};
use rune::{Color, ElementInfo, Key, Mods, Runtime};
use serde_json::Value;

use super::{
    BRANCH, EFFORT, FOLDER, MODE, MODEL, PART_NAME, PART_TOKENS, REMOTE, REMOTE_COPY, REMOTE_NEW,
    REMOTE_STATUS, TOGGLE,
};
use crate::glue::{self, STATE_KEY, Ui};
use crate::view::{ROW, conversation, tile_key};
use crate::{demo, testkit, theme};

/// A window wide enough for three tiles beside the sidebar.
const WIDE: (f32, f32) = (1800.0, 900.0);

/// The kit's key of the panel's pane.
const PANE: &str = "inspector-pane";

/// The window background the kit's main card leaves on its right while no pane borders it.
const FRAME: f32 = 8.0;

/// Rune draws to 1/64 of a point.
const DRAWN: f32 = 1.0 / 64.0;

/// How far a tile may be from its place while the pane's spring moves the row: the tiles
/// land a few frames before the row's own spring does, by less than this.
const MOVING: f32 = 1.0 / 32.0;

/// Ctrl+Alt (Cmd+Alt on macOS).
fn ctrl_alt() -> Mods {
    Mods {
        alt: true,
        ..testkit::ctrl()
    }
}

/// Presses Ctrl+Alt+B.
fn ctrl_alt_b(rt: &mut Runtime<Ui>) {
    testkit::press_with(rt, Key::Command('b'), ctrl_alt());
}

/// Lands every animation, then steps.
fn settle(rt: &mut Runtime<Ui>) {
    rt.settle_all();
    testkit::step(rt);
}

/// The drawn rectangle of the element keyed `key`.
fn drawn(rt: &Runtime<Ui>, key: &str) -> [f32; 4] {
    rt.presented(key)
        .unwrap_or_else(|| panic!("{key} is drawn"))
        .0
}

/// The text of the element keyed `key`, when one is drawn and not on its way out.
fn text_of(rt: &Runtime<Ui>, key: &str) -> Option<String> {
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some(key))
        .and_then(|e| e.text)
}

/// The panel's heading.
fn heading(rt: &Runtime<Ui>) -> Option<String> {
    text_of(rt, "mr-panel-title")
}

/// The title of the focused tile's session.
fn focused_title(rt: &Runtime<Ui>) -> String {
    rt.state
        .focused_session()
        .and_then(|session| session.title.clone())
        .expect("the focused session has a title")
}

/// The width of each tile, in the order of the row.
fn widths(rt: &Runtime<Ui>) -> Vec<f32> {
    testkit::tile_keys(rt)
        .into_iter()
        .map(|key| drawn(rt, &tile_key(key))[2])
        .collect()
}

/// `inner` lies inside `outer`, give or take what Rune's drawing rounds.
fn inside(inner: [f32; 4], outer: [f32; 4]) -> bool {
    let ([ix, iy, iw, ih], [x, y, w, h]) = (inner, outer);
    ix >= x - DRAWN && iy >= y - DRAWN && ix + iw <= x + w + DRAWN && iy + ih <= y + h + DRAWN
}

/// Every tile is drawn at the rectangle `Tiles::rects` gives it in the row as it is drawn,
/// give or take `within`, and its input box's pickers and its send or stop button lie
/// inside it.
fn assert_all_in_place(rt: &Runtime<Ui>, what: &str, within: f32) {
    let [x, y, w, h] = drawn(rt, ROW);
    let elements = rt.elements();
    for laid in rt.state.app.tiles.rects([w, h]) {
        let key = laid.key;
        let [lx, ly, lw, lh] = laid.rect;
        let want = [x + lx, y + ly, lw, lh];
        let tile = drawn(rt, &tile_key(key));
        let off = tile.iter().zip(want).any(|(d, l)| (d - l).abs() > within);
        assert!(!off, "{what}: {key:?} drawn at {tile:?}, not {want:?}");
        for control in controls(rt, &elements, key) {
            assert!(
                inside(control, tile),
                "{what}: a control of {key:?} at {control:?} is outside its tile {tile:?}"
            );
        }
    }
}

/// The drawn rectangles of `key`'s pickers and of its send or stop button, the button
/// found among `elements`.
fn controls(rt: &Runtime<Ui>, elements: &[ElementInfo], key: SessionKey) -> Vec<[f32; 4]> {
    let picker = |name: &str| rt.presented(&format!("{name}-{}-trigger", key.0));
    let model = picker("mr-model").expect("a model picker");
    let mode = picker("mr-mode").expect("a mode picker");
    let inside_tile = format!("/{}/", tile_key(key));
    let button = elements
        .iter()
        .find(|e| {
            !e.exiting
                && e.path.contains(&inside_tile)
                && matches!(e.key.as_deref(), Some("mr-send" | "mr-stop"))
        })
        .expect("a send or stop button");
    let mut found = vec![model.0, mode.0, button.rect];
    found.extend(picker("mr-effort").map(|(rect, _)| rect));
    found
}

#[test]
fn the_panel_is_hidden_at_first_and_the_button_shows_it() {
    let mut rt = testkit::window_sized("tiles", WIDE);
    assert!(!rt.state.panel.is_open());
    assert_eq!(drawn(&rt, PANE)[2], 0.0, "no room is taken");
    let before = drawn(&rt, ROW);
    testkit::click(&mut rt, TOGGLE);
    settle(&mut rt);
    assert!(rt.state.panel.is_open());
    let pane = drawn(&rt, PANE);
    let row = drawn(&rt, ROW);
    assert_eq!(pane[2], 300.0, "its width at first");
    assert!(
        (pane[0] - (row[0] + row[2])).abs() <= DRAWN,
        "the pane {pane:?} starts where the row {row:?} ends"
    );
    assert!(
        (pane[0] + pane[2] - WIDE.0).abs() <= DRAWN,
        "at the window's edge"
    );
    // The kit's main card keeps a frame on its right only while no pane borders it.
    assert!(
        (before[2] - row[2] - (pane[2] - FRAME)).abs() <= DRAWN,
        "the row {row:?} is narrower than {before:?} by the pane's width"
    );
    assert_eq!(heading(&rt), Some(focused_title(&rt)));
}

#[test]
fn ctrl_alt_b_shows_and_hides_it() {
    let mut rt = testkit::window("conversation");
    let key = testkit::focused(&rt).unwrap();
    assert!(testkit::box_focused(&rt, key), "the keyboard is in the box");
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    assert!(rt.state.panel.is_open());
    assert!(rt.state.sidebar.is_open(), "the sidebar stays");
    assert!(testkit::box_focused(&rt, key), "the box keeps the keyboard");
    assert!(testkit::has_text(&rt, &focused_title(&rt)));
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    assert!(!rt.state.panel.is_open());
    assert!(testkit::box_focused(&rt, key), "the box keeps the keyboard");
    testkit::type_text(&mut rt, "still here");
    assert_eq!(
        crate::glue::view_of(&rt.state, key).draft.text,
        "still here"
    );
}

#[test]
fn the_button_says_what_it_will_do() {
    let mut rt = testkit::window("conversation");
    let button = |rt: &Runtime<Ui>| {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(TOGGLE))
            .expect("the button is drawn")
    };
    let shown = button(&rt);
    assert_eq!(shown.label.as_deref(), Some("Show panel"));
    assert_eq!(shown.expanded, Some(false));
    testkit::click(&mut rt, TOGGLE);
    settle(&mut rt);
    let shown = button(&rt);
    assert_eq!(shown.label.as_deref(), Some("Hide panel"));
    assert_eq!(shown.expanded, Some(true), "pressed-looking");
    testkit::click(&mut rt, TOGGLE);
    settle(&mut rt);
    assert_eq!(button(&rt).label.as_deref(), Some("Show panel"));
    assert!(!rt.state.panel.is_open());
}

#[test]
fn the_panel_names_the_focused_session_and_follows_the_focus() {
    let mut rt = testkit::window_sized("tiles", WIDE);
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    let first = focused_title(&rt);
    assert_eq!(heading(&rt), Some(first.clone()));
    testkit::press(&mut rt, Key::Function(6));
    settle(&mut rt);
    let next = focused_title(&rt);
    assert_ne!(next, first, "F6 moved the focus");
    assert_eq!(heading(&rt), Some(next));

    // A session with no title yet.
    let mut rt = testkit::window("empty");
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    assert_eq!(heading(&rt).as_deref(), Some("New session"));
}

#[test]
fn with_no_tile_it_says_no_session_open() {
    let mut rt = testkit::window("empty-start");
    assert_eq!(testkit::focused(&rt), None);
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    assert_eq!(
        text_of(&rt, "mr-panel-empty").as_deref(),
        Some("No session open.")
    );
    assert_eq!(heading(&rt), None);
    assert!(testkit::has_text(&rt, "No session open."));
}

#[test]
fn closing_the_focused_tile_shows_the_next_one() {
    let mut rt = testkit::window_sized("tiles", WIDE);
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    let closed = testkit::focused(&rt).unwrap();
    let before = focused_title(&rt);
    testkit::press_with(&mut rt, Key::Command('w'), testkit::ctrl());
    settle(&mut rt);
    let now = testkit::focused(&rt).unwrap();
    assert_ne!(now, closed);
    assert_ne!(focused_title(&rt), before);
    assert_eq!(heading(&rt), Some(focused_title(&rt)));
}

#[test]
fn opening_the_panel_closes_no_tile() {
    let mut rt = testkit::window_sized("tiles", WIDE);
    let keys = testkit::tile_keys(&rt);
    assert_eq!(keys.len(), 3);
    let before = widths(&rt);
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    assert_eq!(testkit::tile_keys(&rt), keys, "the same three tiles");
    let after = widths(&rt);
    for (narrower, wider) in after.iter().zip(&before) {
        assert!(narrower < wider, "{after:?} each narrower than {before:?}");
    }
}

#[test]
fn a_press_in_the_panel_keeps_the_keyboard_and_the_focused_tile() {
    let mut rt = testkit::window_sized("tiles", WIDE);
    let focused = testkit::focused(&rt).unwrap();
    assert!(testkit::box_focused(&rt, focused));
    testkit::click(&mut rt, TOGGLE);
    settle(&mut rt);
    assert!(testkit::box_focused(&rt, focused), "the button keeps it");
    for at in ["mr-panel-title", "mr-panel"] {
        testkit::click(&mut rt, at);
        settle(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(focused), "a press on {at}");
        assert!(testkit::box_focused(&rt, focused), "a press on {at}");
    }
    // Another tile's box had the keyboard before: it keeps it too.
    testkit::press(&mut rt, Key::Function(6));
    settle(&mut rt);
    let other = testkit::focused(&rt).unwrap();
    assert_ne!(other, focused);
    testkit::click(&mut rt, "mr-panel");
    settle(&mut rt);
    assert_eq!(testkit::focused(&rt), Some(other));
    assert!(testkit::box_focused(&rt, other));
    testkit::click(&mut rt, TOGGLE);
    settle(&mut rt);
    assert!(!rt.state.panel.is_open());
    assert!(testkit::box_focused(&rt, other), "hiding it keeps it");
}

#[test]
fn both_panes_open_in_the_smallest_window() {
    let size = (560.0, 400.0);
    let mut rt = testkit::window_sized("tiles", size);
    let keys = testkit::tile_keys(&rt);
    assert!(rt.state.sidebar.is_open());
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    assert!(rt.state.panel.is_open());
    assert_eq!(testkit::tile_keys(&rt), keys, "no tile closed");
    let button = drawn(&rt, TOGGLE);
    assert!(
        inside(button, [0.0, 0.0, size.0, size.1]),
        "the button {button:?} is inside the window"
    );
    testkit::click(&mut rt, TOGGLE);
    settle(&mut rt);
    assert!(!rt.state.panel.is_open(), "the button hides the panel");
    assert_eq!(testkit::tile_keys(&rt), keys);
}

#[test]
fn the_tiles_start_where_they_started() {
    let mut rt = testkit::window_sized("tiles", WIDE);
    assert_eq!(drawn(&rt, ROW), [260.0, 48.0, 1_532.0, 844.0]);
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    let [x, y, _, h] = drawn(&rt, ROW);
    assert_eq!([x, y, h], [260.0, 48.0, 844.0], "the band keeps its height");
}

/// Presses Ctrl+Alt+B and checks every frame of the pane's spring, a second of frames. No
/// commit is forced ([`settle`] would force one), so a layout read left stale would show.
fn toggle_and_watch(rt: &mut Runtime<Ui>, what: &str) {
    ctrl_alt_b(rt);
    // The kit moves the pane's width on its spring: the tiles follow the row as it is
    // drawn, frame by frame.
    for frame in 0..60 {
        assert_all_in_place(rt, &format!("{what}, frame {frame}"), MOVING);
        rt.frame(0.016);
    }
}

#[test]
fn opening_the_panel_leaves_nothing_a_frame_behind() {
    let mut rt = testkit::window_sized("tiles-stacked", WIDE);
    assert_eq!(
        testkit::columns(&rt).len(),
        2,
        "three tiles, two of them a column"
    );
    assert_eq!(testkit::tile_keys(&rt).len(), 3);
    assert_all_in_place(&rt, "at first", DRAWN);
    let wide = drawn(&rt, ROW)[2];
    toggle_and_watch(&mut rt, "opening");
    assert!(
        (drawn(&rt, ROW)[2] - (wide - (300.0 - FRAME))).abs() <= DRAWN,
        "the narrower row"
    );
    assert_all_in_place(&rt, "with the panel", DRAWN);
    toggle_and_watch(&mut rt, "closing");
    assert!(
        (drawn(&rt, ROW)[2] - wide).abs() <= DRAWN,
        "the wider row again"
    );
    assert_all_in_place(&rt, "without the panel", DRAWN);
}

// The environment and Remote Control: Task 10 of plan 5.

/// The address tests use for a session reachable by Remote Control. It is never put in a
/// test's message.
const ADDRESS: &str = "https://example.org/session";

/// What the line under the switch says while the program connects, and once it is on.
const CONNECTING: &str = "Connecting…";
const ON: &str = "On. Reachable from claude.ai/code and the Claude app.";

/// What the line says after a refusal for want of a full sign-in.
const SIGN_IN: &str = "Remote Control needs a full claude.ai sign-in. Run `claude auth \
                       login` in a terminal, then turn it on again.";

/// The window `scene` at [`WIDE`], with the panel open.
fn with_panel(scene: &str) -> Runtime<Ui> {
    let mut rt = testkit::window_sized(scene, WIDE);
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    rt
}

/// What the row `key` of "Environment" says, when it is drawn.
fn row_value(rt: &Runtime<Ui>, key: &str) -> Option<String> {
    value_of(rt, key).and_then(|value| value.text)
}

/// The value of the row `key` of "Environment", when it is drawn.
fn value_of(rt: &Runtime<Ui>, key: &str) -> Option<ElementInfo> {
    let own = format!("/{key}/mr-panel-value");
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.path.ends_with(&own))
}

/// The line under the Remote Control switch.
fn line(rt: &Runtime<Ui>) -> Option<String> {
    text_of(rt, REMOTE_STATUS)
}

/// The node keyed `key` in `tree`, the window's accessibility tree, or under it.
fn node<'a>(tree: &'a Value, key: &str) -> Option<&'a Value> {
    if tree["key"] == key {
        return Some(tree);
    }
    tree["children"]
        .as_array()?
        .iter()
        .find_map(|child| node(child, key))
}

/// Whether the accessibility tree gives the element keyed `key` the state `state`.
fn has_state(rt: &Runtime<Ui>, key: &str, state: &str) -> bool {
    let tree = rt.accessibility_json();
    let found = node(&tree, key).unwrap_or_else(|| panic!("{key} is in the tree"));
    found["states"]
        .as_array()
        .is_some_and(|states| states.iter().any(|s| s == state))
}

/// Whether the switch keyed `key` shows on.
fn shows_on(rt: &Runtime<Ui>, key: &str) -> bool {
    has_state(rt, key, "checked")
}

/// Every Remote Control command the fake received, with the session it went to.
fn remote_sent(rt: &Runtime<Ui>) -> Vec<(SessionId, bool)> {
    let fake = rt
        .state
        .fake
        .as_ref()
        .expect("the demo window has its fake");
    fake.sent()
        .into_iter()
        .filter_map(|(id, command)| match command {
            SessionCommand::SetRemoteAccess { on } => Some((id, on)),
            _ => None,
        })
        .collect()
}

/// The agent's id of `key`'s session.
fn id_of(rt: &Runtime<Ui>, key: SessionKey) -> SessionId {
    let entry = rt.state.app.sessions.get(key).expect("the session is held");
    entry.session.id.clone().expect("the session has started")
}

/// The program says `state` of Remote Control to `key`'s session.
fn says(rt: &mut Runtime<Ui>, key: SessionKey, state: RemoteAccess) {
    testkit::emit_to(rt, key, SessionEvent::RemoteAccess(state));
}

/// Remote Control on, reachable at [`ADDRESS`].
fn reachable() -> RemoteAccess {
    RemoteAccess::On {
        url: Some(ADDRESS.into()),
    }
}

/// The branch of `key`'s folder is read as `branch`, then steps.
fn branch_read(rt: &mut Runtime<Ui>, key: SessionKey, branch: Option<Branch>) {
    let folder = rt
        .state
        .app
        .sessions
        .get(key)
        .unwrap()
        .session
        .folder
        .clone();
    rt.update(|ui, _| {
        let effects = ui.app.receive(FromBackend::Branch { folder, branch }, 0.0);
        glue::run(ui, effects);
    });
    testkit::step(rt);
}

#[test]
fn the_environment_names_folder_branch_model_effort_and_mode() {
    let mut rt = with_panel("empty");
    let key = testkit::focused(&rt).unwrap();
    branch_read(&mut rt, key, Some(Branch::Named("main".into())));
    assert!(testkit::has_text(&rt, "Environment"));
    for (key, label, value) in [
        (FOLDER, "Folder", "~/project"),
        (BRANCH, "Branch", "main"),
        (MODEL, "Model", "Default · Fake 1"),
        (EFFORT, "Effort", "Medium"),
        (MODE, "Mode", "Ask before edits"),
    ] {
        assert_eq!(row_value(&rt, key).as_deref(), Some(value), "{label}");
        let own = format!("/{key}/mr-panel-label");
        let shown = rt
            .elements()
            .into_iter()
            .find(|e| !e.exiting && e.path.ends_with(&own))
            .and_then(|e| e.text);
        assert_eq!(shown.as_deref(), Some(label));
    }
    let folder = value_of(&rt, FOLDER).unwrap();
    assert_eq!(folder.help.as_deref(), Some("/home/alex/project"));
}

#[test]
fn a_folder_under_home_is_written_with_a_tilde() {
    let mut rt = with_panel("empty");
    for (folder, shown) in [
        ("/home/alex/garden", "~/garden"),
        ("/home/alex", "~"),
        ("/home/alexandra/notes", "/home/alexandra/notes"),
        ("/srv/notes", "/srv/notes"),
    ] {
        testkit::add_session(&mut rt, folder);
        assert_eq!(row_value(&rt, FOLDER).as_deref(), Some(shown), "{folder}");
        let help = value_of(&rt, FOLDER).and_then(|value| value.help);
        assert_eq!(help.as_deref(), Some(folder), "the whole path");
    }
    // A long one is cut in the middle, inside the panel.
    let long = "/home/alex/projects/clients/the-community-garden/planting-schedules/2027";
    testkit::add_session(&mut rt, long);
    let value = value_of(&rt, FOLDER).unwrap();
    let pane = drawn(&rt, PANE);
    assert!(inside(value.rect, pane), "{:?} in {pane:?}", value.rect);
    assert_eq!(value.help.as_deref(), Some(long));
}

#[test]
fn no_branch_no_row() {
    let mut rt = with_panel("empty");
    let key = testkit::focused(&rt).unwrap();
    assert_eq!(
        row_value(&rt, BRANCH),
        None,
        "the folder is in no repository"
    );
    assert!(!testkit::has_text(&rt, "Branch"));
    branch_read(&mut rt, key, Some(Branch::Named("main".into())));
    assert_eq!(row_value(&rt, BRANCH).as_deref(), Some("main"));
    branch_read(&mut rt, key, None);
    assert_eq!(row_value(&rt, BRANCH), None);
    assert!(!testkit::has_text(&rt, "Branch"));
}

#[test]
fn a_detached_head_reads_detached_at() {
    let mut rt = with_panel("empty");
    let key = testkit::focused(&rt).unwrap();
    branch_read(&mut rt, key, Some(Branch::Detached("0123abc".into())));
    assert_eq!(
        row_value(&rt, BRANCH).as_deref(),
        Some("detached at 0123abc")
    );
}

#[test]
fn a_model_without_levels_has_no_effort_row() {
    let mut rt = with_panel("empty");
    assert_eq!(row_value(&rt, EFFORT).as_deref(), Some("Medium"));
    testkit::emit(
        &mut rt,
        SessionEvent::ModelChanged("fake-model-fast".into()),
    );
    assert_eq!(row_value(&rt, MODEL).as_deref(), Some("Fast"));
    assert_eq!(row_value(&rt, EFFORT), None);
    assert!(!testkit::has_text(&rt, "Effort"));
}

#[test]
fn the_rows_follow_a_change() {
    let mut rt = with_panel("empty");
    let key = testkit::focused(&rt).unwrap();
    rt.update(|ui, _| {
        let mut effects = ui.app.choose_effort(key, "high");
        effects.extend(ui.app.choose_mode(key, PermissionMode::AcceptEdits));
        glue::run(ui, effects);
    });
    testkit::step(&mut rt);
    testkit::emit(&mut rt, SessionEvent::EffortChanged(Some("high".into())));
    testkit::emit(
        &mut rt,
        SessionEvent::PermissionModeChanged(PermissionMode::AcceptEdits),
    );
    assert_eq!(row_value(&rt, EFFORT).as_deref(), Some("High"));
    assert_eq!(row_value(&rt, MODE).as_deref(), Some("Accept edits"));
    rt.update(|ui, _| {
        let effects = ui.app.choose_model(key, "fast");
        glue::run(ui, effects);
    });
    testkit::emit(
        &mut rt,
        SessionEvent::ModelChanged("fake-model-fast".into()),
    );
    assert_eq!(row_value(&rt, MODEL).as_deref(), Some("Fast"));
}

#[test]
fn the_branch_shows_when_its_answer_arrives() {
    let mut rt = with_panel("empty");
    // A folder of this test's own, so its branch is read from the disk.
    rt.state.reads_branches = true;
    let dir = testkit::new_session_elsewhere(&mut rt);
    let key = testkit::focused(&rt).unwrap();
    assert_eq!(row_value(&rt, BRANCH), None, "no repository yet");
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/garden\n").unwrap();
    // A turn that ends in the focused tile reads the branch again, on the library thread:
    // the answer is taken in by the next frame's poll, the focus staying where it is.
    demo::emit_to(&rt.state, key, SessionEvent::TurnStarted);
    demo::emit_to(
        &rt.state,
        key,
        SessionEvent::TurnFinished(mr_agent::TurnSummary {
            cost_usd: 0.0,
            context_used: None,
            duration_ms: 1,
            interrupted: false,
        }),
    );
    rt.frame(0.016);
    assert_eq!(testkit::focused(&rt), Some(key));
    assert_eq!(row_value(&rt, BRANCH).as_deref(), Some("garden"));
}

#[test]
fn the_panel_scene_shows_a_branch_the_demo_did_not_read() {
    let rt = with_panel("panel");
    assert_eq!(row_value(&rt, BRANCH).as_deref(), Some("main"));
    assert!(
        !rt.state.reads_branches,
        "the demo's folders are made up: no branch is read from the disk"
    );
}

#[test]
fn the_switch_turns_remote_control_on_for_the_session_shown() {
    let mut rt = with_panel("tiles");
    let key = testkit::focused(&rt).unwrap();
    assert!(!shows_on(&rt, REMOTE));
    assert_eq!(line(&rt).as_deref(), Some("Off."));
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    assert_eq!(remote_sent(&rt), vec![(id_of(&rt, key), true)]);
    assert_eq!(testkit::focused(&rt), Some(key), "the focus stays");
    assert!(shows_on(&rt, REMOTE), "at once");
    assert_eq!(line(&rt).as_deref(), Some(CONNECTING));
    says(&mut rt, key, RemoteAccess::Starting);
    assert_eq!(line(&rt).as_deref(), Some(CONNECTING));
    says(&mut rt, key, reachable());
    assert_eq!(line(&rt).as_deref(), Some(ON));
    assert!(shows_on(&rt, REMOTE));
    says(&mut rt, key, RemoteAccess::Reconnecting);
    assert_eq!(line(&rt).as_deref(), Some("Reconnecting…"));
    assert_eq!(remote_sent(&rt).len(), 1, "to no other session");
}

#[test]
fn a_second_press_before_the_answer_turns_it_off() {
    let mut rt = with_panel("tiles");
    let key = testkit::focused(&rt).unwrap();
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    let id = id_of(&rt, key);
    assert_eq!(
        remote_sent(&rt),
        vec![(id.clone(), true), (id.clone(), false)],
        "the off is sent although the program has not answered the on"
    );
    says(&mut rt, key, RemoteAccess::Starting);
    says(&mut rt, key, reachable());
    says(&mut rt, key, RemoteAccess::Off);
    settle(&mut rt);
    assert!(!shows_on(&rt, REMOTE), "off, as last pressed");
    assert_eq!(line(&rt).as_deref(), Some("Off."));
    assert_eq!(remote_sent(&rt).len(), 2);
}

#[test]
fn the_switch_shows_on_whenever_remote_control_is_on() {
    let mut rt = with_panel("tiles");
    let key = testkit::focused(&rt).unwrap();
    says(&mut rt, key, reachable());
    let session = &rt.state.app.sessions.get(key).unwrap().session;
    assert!(!session.remote_wanted, "nothing asked for it here");
    assert!(shows_on(&rt, REMOTE));
    assert_eq!(line(&rt).as_deref(), Some(ON));
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    assert_eq!(remote_sent(&rt), vec![(id_of(&rt, key), false)]);
}

#[test]
fn a_refusal_puts_the_switch_back_and_says_why_under_it() {
    let mut rt = with_panel("tiles");
    let key = testkit::focused(&rt).unwrap();
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    says(&mut rt, key, RemoteAccess::Starting);
    says(
        &mut rt,
        key,
        RemoteAccess::Failed {
            reason: "/login".into(),
            why: RemoteFailure::Login,
        },
    );
    settle(&mut rt);
    assert!(!shows_on(&rt, REMOTE), "off again");
    assert_eq!(line(&rt).as_deref(), Some(SIGN_IN));
    assert!(testkit::tile_has_text(
        &rt,
        key,
        "needs a full claude.ai sign-in"
    ));
    // A press clears the reason and asks again.
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    assert_eq!(line(&rt).as_deref(), Some(CONNECTING));
}

#[test]
fn a_resting_session_says_it_turns_on_when_it_starts() {
    let mut rt = with_panel("resting");
    let key = testkit::focused(&rt).unwrap();
    assert_eq!(line(&rt).as_deref(), Some("Off."));
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    assert!(shows_on(&rt, REMOTE));
    assert_eq!(
        line(&rt).as_deref(),
        Some("Turns on when the session starts.")
    );
    assert_eq!(remote_sent(&rt), vec![], "nothing is sent");
    let session = &rt.state.app.sessions.get(key).unwrap().session;
    assert!(session.remote_wanted);
}

#[test]
fn the_switch_follows_the_focus() {
    let mut rt = with_panel("tiles");
    let first = testkit::focused(&rt).unwrap();
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    says(&mut rt, first, reachable());
    assert!(shows_on(&rt, REMOTE));
    testkit::press(&mut rt, Key::Function(6));
    settle(&mut rt);
    let next = testkit::focused(&rt).unwrap();
    assert_ne!(next, first);
    assert!(!shows_on(&rt, REMOTE), "the other session's state");
    assert_eq!(line(&rt).as_deref(), Some("Off."));
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    assert_eq!(
        remote_sent(&rt),
        vec![(id_of(&rt, first), true), (id_of(&rt, next), true)]
    );
    let session = |key| rt.state.app.sessions.get(key).unwrap().session.clone();
    assert!(matches!(session(first).remote, Remote::On { .. }));
    assert!(session(next).remote_wanted);
}

#[test]
fn copy_link_copies_the_address_and_shows_it_nowhere() {
    let mut rt = with_panel("tiles");
    let key = testkit::focused(&rt).unwrap();
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    assert!(!testkit::drawn(&rt, REMOTE_COPY), "not while it connects");
    says(&mut rt, key, RemoteAccess::On { url: None });
    assert!(!testkit::drawn(&rt, REMOTE_COPY), "not without an address");
    says(&mut rt, key, reachable());
    settle(&mut rt);
    assert!(testkit::drawn(&rt, REMOTE_COPY));
    let sent = testkit::sent(&rt).len();
    testkit::click(&mut rt, REMOTE_COPY);
    settle(&mut rt);
    assert!(
        testkit::clipboard(&rt).as_deref() == Some(ADDRESS),
        "the address is on the clipboard"
    );
    assert_eq!(testkit::sent(&rt).len(), sent, "nothing is sent");
    assert_eq!(testkit::focused(&rt), Some(key));
    assert!(testkit::box_focused(&rt, key), "the box keeps the keyboard");
    let elements = rt.elements();
    for e in &elements {
        let held = [&e.text, &e.label, &e.help, &e.selected];
        assert!(
            !held
                .iter()
                .any(|t| t.as_deref().is_some_and(|t| t.contains(ADDRESS))),
            "an element at {} holds the address",
            e.path
        );
    }
    let tree = rt.accessibility_json().to_string();
    assert!(!tree.contains(ADDRESS), "the accessibility tree holds it");
}

#[test]
fn an_ended_session_has_a_disabled_switch() {
    let mut rt = with_panel("ended");
    assert!(has_state(&rt, REMOTE, "disabled"));
    assert_eq!(line(&rt).as_deref(), Some("This session has ended."));
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    assert!(!shows_on(&rt, REMOTE));
    assert_eq!(remote_sent(&rt), vec![]);
}

#[test]
fn without_remote_control_the_switches_are_disabled() {
    let mut rt = with_panel("conversation");
    rt.state.app.capabilities.remote_access = false;
    rt.invalidate();
    settle(&mut rt);
    assert!(has_state(&rt, REMOTE, "disabled"));
    assert!(has_state(&rt, REMOTE_NEW, "disabled"));
    assert_eq!(
        line(&rt).as_deref(),
        Some("Remote Control is not available.")
    );
}

#[test]
fn on_for_new_sessions_is_kept_and_a_new_session_starts_with_it() {
    let mut rt = with_panel("empty-start");
    assert_eq!(testkit::focused(&rt), None);
    assert!(!shows_on(&rt, REMOTE_NEW), "off at first");
    testkit::click(&mut rt, REMOTE_NEW);
    settle(&mut rt);
    assert!(shows_on(&rt, REMOTE_NEW));
    assert!(rt.state.app.remote_for_new);
    let saved: mr_core::Saved = rt.state.prefs.get(STATE_KEY).expect("saved");
    assert!(saved.remote_for_new, "kept");
    let key = testkit::add_session(&mut rt, "/home/alex/garden");
    settle(&mut rt);
    assert_eq!(remote_sent(&rt), vec![(id_of(&rt, key), true)]);
    assert!(shows_on(&rt, REMOTE), "the new session's switch");
    assert!(shows_on(&rt, REMOTE_NEW));
    testkit::click(&mut rt, REMOTE_NEW);
    settle(&mut rt);
    let saved: mr_core::Saved = rt.state.prefs.get(STATE_KEY).expect("saved");
    assert!(!saved.remote_for_new);
}

#[test]
fn a_press_on_a_switch_keeps_the_keyboard_in_the_box() {
    let mut rt = with_panel("tiles");
    let key = testkit::focused(&rt).unwrap();
    assert!(testkit::box_focused(&rt, key));
    for switch in [REMOTE, REMOTE_NEW] {
        let reached = rt
            .elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(switch))
            .is_some_and(|e| e.focusable);
        assert!(reached, "{switch} is a Tab stop");
        testkit::click(&mut rt, switch);
        settle(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(key), "{switch}");
        assert!(testkit::box_focused(&rt, key), "{switch}");
    }
    testkit::type_text(&mut rt, "still here");
    assert_eq!(glue::view_of(&rt.state, key).draft.text, "still here");
}

/// The relative luminance of `color`, which Rune keeps linear, drawn opaque over `under`.
fn luminance_over(color: Color, under: Color) -> f32 {
    let mix = |c: f32, u: f32| c * color.a + u * (1.0 - color.a);
    let [r, g, b] = [
        mix(color.r, under.r),
        mix(color.g, under.g),
        mix(color.b, under.b),
    ];
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The contrast of `color`, drawn over `under`, against `under`, as WCAG works it out.
fn contrast(color: Color, under: Color) -> f32 {
    let (a, b) = (
        luminance_over(color, under) + 0.05,
        luminance_over(under, under) + 0.05,
    );
    a.max(b) / a.min(b)
}

#[test]
fn each_switch_has_an_edge_in_both_themes() {
    let mut rt = with_panel("tiles");
    for dark in [true, false] {
        rt.set_system_dark(dark);
        settle(&mut rt);
        let tree = rt.accessibility_json();
        for switch in [REMOTE, REMOTE_NEW] {
            let edge = format!("{switch}-edge");
            let drawn_edge = rt
                .elements()
                .into_iter()
                .find(|e| !e.exiting && e.key.as_deref() == Some(edge.as_str()))
                .unwrap_or_else(|| panic!("{edge} is drawn, dark {dark}"));
            assert_eq!(drawn_edge.rect, drawn(&rt, switch), "round {switch}");
            assert!(
                drawn_edge.inert && !drawn_edge.focusable,
                "{edge} takes nothing"
            );
            assert!(node(&tree, &edge).is_none(), "{edge} is not in the tree");
            let found = node(&tree, switch).unwrap_or_else(|| panic!("{switch} in the tree"));
            assert_eq!(found["role"], "switch", "{switch} is still a switch");
        }
        assert!(!shows_on(&rt, REMOTE), "the switch is off");
    }
    // The edge's colour is a token of the theme that stands out from the panel, the
    // window's own background, in the light theme too.
    for dark in [true, false] {
        let kit = theme::kit(dark);
        let ratio = contrast(super::edge_color(kit), kit.c.bg_deep);
        assert!(ratio >= 3.0, "{ratio}:1 against the panel, dark {dark}");
    }
    // A click on the switch reaches it through the edge.
    testkit::click(&mut rt, REMOTE);
    settle(&mut rt);
    assert!(shows_on(&rt, REMOTE));
}

// Context, cost and the files changed: Task 11 of plan 5.

/// The keys of the context line and its bar, of the cost line, and of "Files changed".
const CONTEXT: &str = "mr-panel-context";
const CONTEXT_BAR: &str = "mr-panel-context-bar";
const COST: &str = "mr-panel-cost";
const FILES: &str = "mr-panel-files";

/// The keys inside "Files changed": the number beside its heading, what it says with no file,
/// its last line about a history not loaded whole, and a row's path and counts.
const FILES_COUNT: &str = "mr-panel-files-count";
const NO_FILES: &str = "mr-panel-files-none";
const NOT_LISTED: &str = "mr-panel-files-earlier";
const PATH: &str = "mr-panel-file-path";
const ADDED: &str = "mr-panel-file-added";
const REMOVED: &str = "mr-panel-file-removed";

/// The folder of the `empty` scene's session.
const PROJECT: &str = "/home/alex/project";

/// The focused session's program finishes a turn, saying it has cost `cost` so far and uses
/// `context` of its context window.
fn turn_ends(rt: &mut Runtime<Ui>, cost: f64, context: Option<f32>) {
    testkit::emit(rt, SessionEvent::TurnStarted);
    testkit::emit(
        rt,
        SessionEvent::TurnFinished(TurnSummary {
            cost_usd: cost,
            context_used: context,
            duration_ms: 1,
            interrupted: false,
        }),
    );
}

/// An edit of `path` from `old` to `new`.
fn edit(path: &str, old: &str, new: &str) -> ToolDetail {
    ToolDetail::FileEdit {
        path: path.into(),
        old_text: old.into(),
        new_text: new.into(),
    }
}

/// `key`'s session starts the call `id`, an edit or a write, without stepping.
fn call_starts(rt: &Runtime<Ui>, key: SessionKey, id: &str, detail: ToolDetail) {
    let name = match detail {
        ToolDetail::FileWrite { .. } => "Write",
        _ => "Edit",
    };
    let started = SessionEvent::ToolStarted {
        id: ToolCallId(id.into()),
        name: name.into(),
        detail,
    };
    demo::emit_to(&rt.state, key, started);
}

/// `key`'s call `id` ends: it succeeded when `ok`, else it was denied. Does not step.
fn call_ends(rt: &Runtime<Ui>, key: SessionKey, id: &str, ok: bool) {
    let outcome = if ok {
        ToolOutcome::Ok {
            output: "The file was updated.".into(),
        }
    } else {
        ToolOutcome::Failed {
            output: "The user doesn't want to proceed with this tool use.".into(),
        }
    };
    let finished = SessionEvent::ToolFinished {
        id: ToolCallId(id.into()),
        outcome,
    };
    demo::emit_to(&rt.state, key, finished);
}

/// The focused session's call `id` runs `detail` and ends, succeeding when `ok`; then steps.
fn call(rt: &mut Runtime<Ui>, id: &str, detail: ToolDetail, ok: bool) {
    let key = testkit::focused(rt).expect("a session is shown");
    call_starts(rt, key, id, detail);
    call_ends(rt, key, id, ok);
    testkit::step(rt);
}

/// The element keyed `key`, when one is drawn and not on its way out.
fn element(rt: &Runtime<Ui>, key: &str) -> Option<ElementInfo> {
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some(key))
}

/// The element keyed `inner` directly inside the element keyed `outer`, when one is drawn.
fn inside_of(rt: &Runtime<Ui>, outer: &str, inner: &str) -> Option<ElementInfo> {
    let own = format!("/{outer}/{inner}");
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.path.ends_with(&own))
}

/// The key of the row `i` of "Files changed".
fn file_key(i: usize) -> String {
    format!("mr-panel-file-{i}")
}

/// What the row `i` of "Files changed" shows: its path, then each count it draws.
fn file_row(rt: &Runtime<Ui>, i: usize) -> Option<(String, Option<String>, Option<String>)> {
    let row = file_key(i);
    let path = inside_of(rt, &row, PATH)?.text?;
    let count = |key| inside_of(rt, &row, key).and_then(|e| e.text);
    Some((path, count(ADDED), count(REMOVED)))
}

/// Every row of "Files changed", in order.
fn file_rows(rt: &Runtime<Ui>) -> Vec<(String, Option<String>, Option<String>)> {
    (0..).map_while(|i| file_row(rt, i)).collect()
}

/// A row of "Files changed" as the test expects it.
fn shows(
    path: &str,
    added: Option<&str>,
    removed: Option<&str>,
) -> (String, Option<String>, Option<String>) {
    (
        path.to_owned(),
        added.map(str::to_owned),
        removed.map(str::to_owned),
    )
}

#[test]
fn context_and_cost_are_not_known_until_a_turn_ends() {
    let mut rt = with_panel("empty");
    forget_breakdown(&mut rt);
    assert!(
        testkit::texts(&rt).contains(&"Context".to_owned()),
        "its heading"
    );
    let unknown = |rt: &Runtime<Ui>| {
        assert_eq!(
            text_of(rt, CONTEXT).as_deref(),
            Some("Context use is not known yet.")
        );
        assert!(!testkit::drawn(rt, CONTEXT_BAR), "no bar");
        assert_eq!(text_of(rt, COST).as_deref(), Some("Cost is not known yet."));
    };
    unknown(&rt);
    testkit::emit(&mut rt, SessionEvent::TurnStarted);
    unknown(&rt);
    testkit::emit(
        &mut rt,
        SessionEvent::TurnFinished(TurnSummary {
            cost_usd: 0.42,
            context_used: Some(0.37),
            duration_ms: 1,
            interrupted: false,
        }),
    );
    assert_eq!(
        text_of(&rt, CONTEXT).as_deref(),
        Some("37% of context used")
    );
    assert!(testkit::drawn(&rt, CONTEXT_BAR));
    assert_eq!(text_of(&rt, COST).as_deref(), Some("Cost so far: $0.42"));
    // A share is rounded to a whole number.
    turn_ends(&mut rt, 1.234, Some(0.4249));
    assert_eq!(
        text_of(&rt, CONTEXT).as_deref(),
        Some("42% of context used")
    );
    assert_eq!(text_of(&rt, COST).as_deref(), Some("Cost so far: $1.23"));
}

#[test]
fn the_context_bar_changes_colour_as_the_sidebars_do() {
    let mut rt = with_panel("empty");
    let kit = theme::kit(true);
    let fill = |rt: &Runtime<Ui>, bar: &str| {
        inside_of(rt, bar, "mr-fill").unwrap_or_else(|| panic!("{bar} has its fill"))
    };
    let mut seen = Vec::new();
    for (share, level) in [
        (0.74, Level::Normal),
        (0.75, Level::Warning),
        (0.90, Level::Alarm),
    ] {
        turn_ends(&mut rt, 0.1, Some(share));
        // The sidebar's line for the same share.
        rt.state.app.usage = UsageLimits {
            session: Some(LimitWindow {
                used: share,
                resets_at_unix: demo::NOW_UNIX + 3_600,
            }),
            weekly: None,
        };
        rt.invalidate();
        // The colours move to their new level: land them.
        settle(&mut rt);
        let ours = fill(&rt, CONTEXT_BAR);
        let sidebars = fill(&rt, "mr-usage-session-bar");
        let colour = theme::level_color(kit, level);
        assert_eq!(ours.fill, colour, "{share}");
        assert_eq!(ours.fill, sidebars.fill, "{share}");
        let line = element(&rt, CONTEXT).expect("the context line");
        let sidebar_line = element(&rt, "mr-usage-session").expect("the sidebar's");
        assert_eq!(line.text_color, Some(colour), "{share}");
        assert_eq!(line.text_color, sidebar_line.text_color, "{share}");
        // Built as the sidebar's: as high, filled to the share.
        let (bar, sidebar_bar) = (drawn(&rt, CONTEXT_BAR), drawn(&rt, "mr-usage-session-bar"));
        assert_eq!(bar[3], sidebar_bar[3], "{share}");
        assert!((ours.rect[2] - bar[2] * share).abs() <= 0.5, "{share}");
        seen.push(colour);
    }
    seen.dedup();
    assert_eq!(seen.len(), 3, "three colours");
}

#[test]
fn a_cost_below_a_cent_says_so() {
    let mut rt = with_panel("empty");
    for cost in [0.004, 0.0099] {
        turn_ends(&mut rt, cost, None);
        assert_eq!(
            text_of(&rt, COST).as_deref(),
            Some("Cost so far: less than $0.01"),
            "{cost}"
        );
    }
    turn_ends(&mut rt, 0.01, None);
    assert_eq!(text_of(&rt, COST).as_deref(), Some("Cost so far: $0.01"));
}

#[test]
fn a_cost_of_nothing_reads_zero() {
    let mut rt = with_panel("empty");
    forget_breakdown(&mut rt);
    turn_ends(&mut rt, 0.0, None);
    assert_eq!(text_of(&rt, COST).as_deref(), Some("Cost so far: $0.00"));
    assert_eq!(
        text_of(&rt, CONTEXT).as_deref(),
        Some("Context use is not known yet."),
        "a turn that says nothing of the context"
    );
}

/// The focused session forgets the breakdown the fake gave when it started, as a session
/// whose program never answered the request.
fn forget_breakdown(rt: &mut Runtime<Ui>) {
    let key = testkit::focused(rt).expect("a session is shown");
    rt.state
        .app
        .sessions
        .get_mut(key)
        .unwrap()
        .session
        .context_parts = None;
    rt.invalidate();
    testkit::step(rt);
}

/// The focused session's program says its context is made of `parts`, of a window of 200,000.
fn breakdown_is(rt: &mut Runtime<Ui>, parts: &[(&str, u64, ContextKind)]) {
    let parts = parts
        .iter()
        .map(|(name, tokens, kind)| ContextPart {
            name: (*name).into(),
            tokens: *tokens,
            kind: *kind,
        })
        .collect();
    let breakdown = ContextBreakdown {
        parts,
        window: 200_000,
    };
    testkit::emit(rt, SessionEvent::ContextBreakdown(breakdown));
}

/// The key of the row `i` of the breakdown.
fn part_key(i: usize) -> String {
    format!("mr-panel-context-part-{i}")
}

/// The name and the tokens the row `i` of the breakdown shows.
fn part_row(rt: &Runtime<Ui>, i: usize) -> Option<(String, String)> {
    let row = part_key(i);
    let name = inside_of(rt, &row, PART_NAME)?.text?;
    let tokens = inside_of(rt, &row, PART_TOKENS)?.text?;
    Some((name, tokens))
}

/// Every row of the breakdown, in order.
fn part_rows(rt: &Runtime<Ui>) -> Vec<(String, String)> {
    (0..).map_while(|i| part_row(rt, i)).collect()
}

/// A row of the breakdown as the test expects it.
fn part_shows(name: &str, tokens: &str) -> (String, String) {
    (name.to_owned(), tokens.to_owned())
}

#[test]
fn the_breakdown_lists_each_part_with_tokens_and_share() {
    let mut rt = with_panel("empty");
    forget_breakdown(&mut rt);
    breakdown_is(
        &mut rt,
        &[
            ("System tools", 12_000, ContextKind::Used),
            ("MCP server instructions", 717, ContextKind::Used),
            ("Autocompact buffer", 33_000, ContextKind::Buffer),
            ("Free space", 141_000, ContextKind::Free),
        ],
    );
    assert_eq!(
        part_rows(&rt),
        vec![
            part_shows("System tools", "12.0k (6%)"),
            part_shows("MCP server instructions", "717 (<1%)"),
            part_shows("Autocompact buffer", "33.0k (17%)"),
            part_shows("Free space", "141.0k (71%)"),
        ]
    );
    // Before any turn, the figure over the bar is the used parts' share.
    assert_eq!(text_of(&rt, CONTEXT).as_deref(), Some("6% of context used"));
    let tops: Vec<f32> = (0..4).map(|i| drawn(&rt, &part_key(i))[1]).collect();
    assert!(tops.windows(2).all(|w| w[0] < w[1]), "one under the other");
    assert!(tops[0] > drawn(&rt, CONTEXT_BAR)[1], "under the bar");
    assert!(tops[3] < drawn(&rt, COST)[1], "over the cost");
    // The buffer and the free space are muted; the used parts are not. A row that was another
    // part before moves to its colour: land it.
    settle(&mut rt);
    let kit = theme::kit(true);
    let colours = |i: usize| {
        let row = part_key(i);
        let name = inside_of(&rt, &row, PART_NAME).unwrap().text_color;
        let tokens = inside_of(&rt, &row, PART_TOKENS).unwrap().text_color;
        assert_eq!(name, tokens, "row {i}");
        name
    };
    assert_eq!(colours(0), Some(kit.c.text_base));
    assert_eq!(colours(1), Some(kit.c.text_base));
    assert_eq!(colours(2), Some(kit.c.text_muted));
    assert_eq!(colours(3), Some(kit.c.text_muted));
    // The next reading replaces the rows, and a turn's own figure then holds over the bar.
    turn_ends(&mut rt, 0.1, Some(0.42));
    breakdown_is(&mut rt, &[("Messages", 1_234_567, ContextKind::Used)]);
    assert_eq!(part_rows(&rt), vec![part_shows("Messages", "1.2M (100%)")]);
    assert_eq!(
        text_of(&rt, CONTEXT).as_deref(),
        Some("42% of context used")
    );
}

#[test]
fn parts_without_tokens_are_not_listed() {
    let mut rt = with_panel("empty");
    breakdown_is(
        &mut rt,
        &[
            ("System prompt", 3_000, ContextKind::Used),
            ("Skills", 0, ContextKind::Used),
            ("Messages", 8_000, ContextKind::Used),
            ("Free space", 0, ContextKind::Free),
        ],
    );
    assert_eq!(
        part_rows(&rt),
        vec![
            part_shows("System prompt", "3.0k (2%)"),
            part_shows("Messages", "8.0k (4%)"),
        ]
    );
    assert!(element(&rt, &part_key(2)).is_none());
}

#[test]
fn no_breakdown_shows_no_rows() {
    let mut rt = with_panel("empty");
    assert!(!part_rows(&rt).is_empty(), "the fake gives one at start");
    forget_breakdown(&mut rt);
    assert!(part_rows(&rt).is_empty());
    assert!(element(&rt, &part_key(0)).is_none());
    turn_ends(&mut rt, 0.1, Some(0.2));
    assert!(part_rows(&rt).is_empty(), "a turn gives none by itself");
    assert_eq!(
        text_of(&rt, CONTEXT).as_deref(),
        Some("20% of context used")
    );
}

#[test]
fn the_panel_scene_lists_what_its_37_percent_is_made_of() {
    let mut rt = testkit::window_sized("panel", WIDE);
    settle(&mut rt);
    assert_eq!(
        text_of(&rt, CONTEXT).as_deref(),
        Some("37% of context used")
    );
    let session = rt.state.focused_session().expect("a session is shown");
    let breakdown = session.context_parts.as_ref().expect("a breakdown");
    let used: u64 = breakdown
        .parts
        .iter()
        .filter(|part| part.kind == ContextKind::Used)
        .map(|part| part.tokens)
        .sum();
    assert_eq!((used, breakdown.window), (74_000, 200_000));
    assert_eq!(part_rows(&rt).len(), 7);
}

#[test]
fn token_counts_read_as_claude_codes_own() {
    for (tokens, words) in [
        (0, "0"),
        (999, "999"),
        (1_000, "1.0k"),
        (5_870, "5.9k"),
        (999_949, "999.9k"),
        (999_950, "1.0M"),
        (1_234_567, "1.2M"),
    ] {
        assert_eq!(super::token_count(tokens), words, "{tokens}");
    }
}

#[test]
fn the_breakdown_leaves_files_changed_in_reach_in_the_smallest_window() {
    let mut rt = testkit::window_sized("panel", (560.0, 400.0));
    settle(&mut rt);
    assert!(rt.state.panel.is_open(), "the scene opens it");
    assert!(element(&rt, &part_key(0)).is_some(), "the rows are there");
    let visible = |rt: &Runtime<Ui>, key: &str| {
        element(rt, key).is_some_and(|e| e.visible[2] > 0.0 && e.visible[3] > 0.0)
    };
    assert!(!visible(&rt, FILES), "below the panel's end at first");
    let pane = drawn(&rt, PANE);
    let (x, y) = (pane[0] + pane[2] / 2.0, pane[1] + pane[3] / 2.0);
    for _ in 0..10 {
        rt.wheel(x, y, -600.0);
        testkit::step(&mut rt);
    }
    assert!(visible(&rt, FILES), "the wheel brings it into view");
    let last = file_key(3);
    let row = element(&rt, &last).expect("the last file");
    assert!(inside(row.visible, pane), "{:?} in {pane:?}", row.visible);
    assert!(row.visible[3] > 0.0, "the last file is in view");
}

#[test]
fn the_files_a_session_changed_are_listed_with_their_counts() {
    let mut rt = with_panel("empty");
    let main = format!("{PROJECT}/src/main.rs");
    let notes = format!("{PROJECT}/notes.txt");
    let beds = "/home/alex/garden/beds.md";
    testkit::emit(&mut rt, SessionEvent::TurnStarted);
    call(&mut rt, "e1", edit(&main, "a\nb\n", "a\nc\nd\n"), true);
    let write = ToolDetail::FileWrite {
        path: notes.clone(),
        content: "one\ntwo\nthree\n".into(),
    };
    call(&mut rt, "w1", write, true);
    call(&mut rt, "e2", edit(beds, "sun\nshade\n", "sun\n"), true);
    assert_eq!(
        file_rows(&rt),
        [
            shows("src/main.rs", Some("+2"), Some("−1")),
            shows("notes.txt", Some("+3"), None),
            shows(beds, None, Some("−1")),
        ]
    );
    assert_eq!(text_of(&rt, FILES_COUNT).as_deref(), Some("3"));
    assert!(testkit::texts(&rt).contains(&"Files changed".to_owned()));
    // The whole path is each row's tooltip, and the counts have a diff's colours.
    let kit = theme::kit(true);
    for (i, whole) in [main.as_str(), notes.as_str(), beds]
        .into_iter()
        .enumerate()
    {
        let path = inside_of(&rt, &file_key(i), PATH).unwrap();
        assert_eq!(path.help.as_deref(), Some(whole));
    }
    let added = inside_of(&rt, &file_key(0), ADDED).unwrap();
    let removed = inside_of(&rt, &file_key(0), REMOVED).unwrap();
    assert_eq!(added.text_color, Some(kit.c.success.fg));
    assert_eq!(removed.text_color, Some(kit.c.danger.fg));
    // The rows are inside the section, inside the panel.
    let pane = drawn(&rt, PANE);
    assert!(inside(drawn(&rt, FILES), pane));
    // A path too long for the row is cut at its start, inside the panel.
    let long = format!("{PROJECT}/src/planting/schedules/beds/the-long-border/autumn/bulbs.rs");
    call(&mut rt, "e3", edit(&long, "", "tulips\n"), true);
    let path = inside_of(&rt, &file_key(3), PATH).unwrap();
    assert!(inside(path.rect, pane), "{:?} in {pane:?}", path.rect);
    assert_eq!(path.help.as_deref(), Some(long.as_str()));
    assert!(testkit::has_text(
        &rt,
        "src/planting/schedules/beds/the-long-border/autumn/bulbs.rs"
    ));
}

#[test]
fn the_list_grows_as_edits_finish() {
    let mut rt = with_panel("empty");
    let key = testkit::focused(&rt).unwrap();
    let main = format!("{PROJECT}/src/main.rs");
    let lib = format!("{PROJECT}/src/lib.rs");
    testkit::emit(&mut rt, SessionEvent::TurnStarted);
    call_starts(&rt, key, "e1", edit(&main, "a\n", "b\n"));
    testkit::step(&mut rt);
    assert_eq!(file_rows(&rt), [], "not finished yet");
    call_ends(&rt, key, "e1", true);
    testkit::step(&mut rt);
    assert_eq!(
        file_rows(&rt),
        [shows("src/main.rs", Some("+1"), Some("−1"))]
    );
    call(&mut rt, "e2", edit(&lib, "", "pub fn run() {}\n"), true);
    call(&mut rt, "e3", edit(&main, "b\n", "b\nc\n"), true);
    assert_eq!(
        file_rows(&rt),
        [
            shows("src/main.rs", Some("+2"), Some("−1")),
            shows("src/lib.rs", Some("+1"), None),
        ]
    );
    assert_eq!(text_of(&rt, FILES_COUNT).as_deref(), Some("2"));
    // An edit that is denied adds nothing.
    call(&mut rt, "e4", edit(&main, "c\n", "d\n"), false);
    let denied = format!("{PROJECT}/src/denied.rs");
    call(&mut rt, "e5", edit(&denied, "", "x\n"), false);
    assert_eq!(
        file_rows(&rt),
        [
            shows("src/main.rs", Some("+2"), Some("−1")),
            shows("src/lib.rs", Some("+1"), None),
        ]
    );
    assert_eq!(text_of(&rt, FILES_COUNT).as_deref(), Some("2"));
}

#[test]
fn no_files_changed_yet() {
    let mut rt = with_panel("empty");
    assert_eq!(
        text_of(&rt, NO_FILES).as_deref(),
        Some("No files changed yet.")
    );
    assert!(testkit::texts(&rt).contains(&"Files changed".to_owned()));
    assert!(
        !testkit::drawn(&rt, FILES_COUNT),
        "no number beside the heading"
    );
    assert_eq!(file_rows(&rt), []);
    // A read changes nothing.
    testkit::emit(&mut rt, SessionEvent::TurnStarted);
    let key = testkit::focused(&rt).unwrap();
    let read = SessionEvent::ToolStarted {
        id: ToolCallId("r1".into()),
        name: "Read".into(),
        detail: ToolDetail::FileRead {
            path: format!("{PROJECT}/src/main.rs"),
        },
    };
    demo::emit_to(&rt.state, key, read);
    call_ends(&rt, key, "r1", true);
    testkit::step(&mut rt);
    assert_eq!(
        text_of(&rt, NO_FILES).as_deref(),
        Some("No files changed yet.")
    );
    // The first edit takes its place.
    call(
        &mut rt,
        "e1",
        edit(&format!("{PROJECT}/a.txt"), "", "a\n"),
        true,
    );
    assert!(!testkit::drawn(&rt, NO_FILES));
    assert_eq!(text_of(&rt, FILES_COUNT).as_deref(), Some("1"));
}

/// The line "Files changed" shows with no row.
fn no_files_line(rt: &Runtime<Ui>) -> Option<String> {
    text_of(rt, NO_FILES)
}

/// `key`'s session's history is in the state `state`, then the window runs again.
fn history_is(rt: &mut Runtime<Ui>, key: SessionKey, state: HistoryState) {
    rt.state.app.sessions.get_mut(key).unwrap().session.history = state;
    rt.invalidate();
    settle(rt);
}

#[test]
fn files_are_not_known_until_the_history_is_read() {
    const NOT_KNOWN: &str = "Files changed are not known yet.";
    let mut rt = with_panel("empty");
    let key = testkit::focused(&rt).unwrap();
    for state in [
        HistoryState::NotLoaded,
        HistoryState::Loading,
        HistoryState::Failed("the file is gone".into()),
    ] {
        history_is(&mut rt, key, state.clone());
        assert_eq!(no_files_line(&rt).as_deref(), Some(NOT_KNOWN), "{state:?}");
        let line = element(&rt, NO_FILES).unwrap();
        let kit = theme::kit(true);
        assert_eq!(line.text_color, Some(kit.c.text_muted), "muted: {state:?}");
    }
    // Once read, what it says is known.
    history_is(&mut rt, key, HistoryState::Loading);
    {
        let mut entry = rt.state.app.sessions.get_mut(key).unwrap();
        entry.session.load_history(StoredHistory::default(), &[]);
    }
    rt.invalidate();
    settle(&mut rt);
    assert_eq!(no_files_line(&rt).as_deref(), Some("No files changed yet."));
    // An edit made while the history is still read shows as a row, with no such line.
    history_is(&mut rt, key, HistoryState::Loading);
    testkit::emit(&mut rt, SessionEvent::TurnStarted);
    call(
        &mut rt,
        "e1",
        edit(&format!("{PROJECT}/a.txt"), "", "a\n"),
        true,
    );
    assert!(!testkit::drawn(&rt, NO_FILES));
    assert_eq!(file_rows(&rt), [shows("a.txt", Some("+1"), None)]);
}

#[test]
fn a_history_with_an_earlier_part_says_what_is_not_listed() {
    let words = "Changes in earlier messages that are not loaded are not listed.";
    let rt = with_panel("resting");
    let line = element(&rt, NOT_LISTED).expect("the line is drawn");
    assert_eq!(line.text.as_deref(), Some(words));
    let kit = theme::kit(true);
    assert_eq!(line.text_color, Some(kit.c.text_muted), "muted");
    let none = drawn(&rt, NO_FILES);
    assert!(line.rect[1] > none[1], "the last line");
    // A history read whole, or none, says nothing of the kind.
    let mut rt = with_panel("empty");
    assert!(!testkit::drawn(&rt, NOT_LISTED));
    assert!(!testkit::has_text(&rt, words));
    // Under the rows, when there are some: a file changed in this run, then the end of the
    // session's history put before it.
    let key = testkit::focused(&rt).unwrap();
    testkit::emit(&mut rt, SessionEvent::TurnStarted);
    call(
        &mut rt,
        "e1",
        edit(&format!("{PROJECT}/a.txt"), "", "a\n"),
        true,
    );
    let history = StoredHistory {
        items: vec![HistoryItem::User {
            text: "Plant the bulbs".into(),
            image_count: 0,
            files: Vec::new(),
            images: Vec::new(),
        }],
        earlier_left_out: true,
        ..StoredHistory::default()
    };
    {
        let mut entry = rt.state.app.sessions.get_mut(key).unwrap();
        entry.session.history = HistoryState::NotLoaded;
        entry.session.load_history(history, &[]);
    }
    rt.invalidate();
    settle(&mut rt);
    let line = drawn(&rt, NOT_LISTED);
    assert!(line[1] > drawn(&rt, &file_key(0))[1], "under the rows");
}

#[test]
fn the_panel_builds_no_conversation_row() {
    let mut rt = testkit::window_sized("tiles", WIDE);
    settle(&mut rt);
    let rows = conversation::all_row_builds();
    ctrl_alt_b(&mut rt);
    settle(&mut rt);
    assert!(rt.state.panel.is_open());
    assert!(testkit::drawn(&rt, FILES), "the panel is drawn");
    assert_eq!(conversation::all_row_builds(), rows, "opening it");
    let focused = testkit::focused(&rt).unwrap();
    let other = testkit::tile_keys(&rt)
        .into_iter()
        .find(|key| *key != focused)
        .expect("another tile");
    testkit::emit_to(
        &mut rt,
        other,
        SessionEvent::ModelChanged("fake-model-fast".into()),
    );
    settle(&mut rt);
    assert_eq!(
        conversation::all_row_builds(),
        rows,
        "an event in another session"
    );
}

#[test]
fn fifty_files_scroll_with_the_panel() {
    let mut rt = with_panel("tiles");
    let key = testkit::focused(&rt).unwrap();
    for n in 0..50 {
        let id = format!("e{n}");
        call_starts(
            &rt,
            key,
            &id,
            edit(&format!("/home/alex/website/page-{n}.html"), "", "<p>\n"),
        );
        call_ends(&rt, key, &id, true);
    }
    testkit::step(&mut rt);
    settle(&mut rt);
    assert_eq!(text_of(&rt, FILES_COUNT).as_deref(), Some("50"));
    let last = file_key(49);
    let visible = |rt: &Runtime<Ui>, key: &str| {
        element(rt, key).is_some_and(|e| e.visible[2] > 0.0 && e.visible[3] > 0.0)
    };
    assert!(
        !visible(&rt, &last),
        "the last row is below the panel's end"
    );
    assert!(visible(&rt, "mr-panel-title"));
    // How far each tile's conversation is scrolled.
    let scrolled = |rt: &Runtime<Ui>| {
        testkit::tile_keys(rt)
            .into_iter()
            .map(|key| {
                let rows = format!("/{}/mr-rows", crate::view::conversation_key(key));
                let list = rt
                    .elements()
                    .into_iter()
                    .find(|e| !e.exiting && e.path.ends_with(&rows))
                    .expect("a conversation's rows");
                (key, list.scrolled)
            })
            .collect::<Vec<_>>()
    };
    let before = scrolled(&rt);
    let pane = drawn(&rt, PANE);
    let (x, y) = (pane[0] + pane[2] / 2.0, pane[1] + pane[3] / 2.0);
    for _ in 0..5 {
        rt.wheel(x, y, -600.0);
        testkit::step(&mut rt);
    }
    let row = element(&rt, &last).expect("the last row");
    assert!(
        visible(&rt, &last),
        "the wheel brings the last row into view"
    );
    assert!(inside(row.visible, pane), "{:?} in {pane:?}", row.visible);
    assert!(
        !visible(&rt, "mr-panel-title"),
        "the heading scrolled away with it"
    );
    assert_eq!(scrolled(&rt), before, "no conversation scrolled");
}
