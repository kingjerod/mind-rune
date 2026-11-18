//! The shell: the sidebar, with the new-session button, the list of groups and sessions and the
//! footer (the account, Quit and the usage), the main area beside it, and the right panel beside
//! that.

use mr_core::{UsageLine, account_line, usage_lines};
use rune::{El, KeyEvent, Tx, col, el, row, text};
use rune_kit::shell::{PaneState, Sidebar, SplitView, Toolbar};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit};

pub use rune_kit::shell::is_sidebar_toggle;

use crate::glue::{self, Ui};
use crate::theme::{self, USAGE_BAR_HEIGHT};
use crate::view::{folder, panel, sidebar};

/// The key of the sidebar's "New session".
pub const NEW_SESSION: &str = "mr-new-session";

/// Ctrl+Alt+B (Cmd+Alt+B on macOS): shows or hides the right panel.
pub fn is_panel_toggle(ev: &KeyEvent) -> bool {
    let m = ev.mods;
    ev.is_command('b') && m.alt && !m.shift && !(cfg!(target_os = "macos") && m.ctrl)
}

/// The sidebar on the left and the right panel, each of which the user can drag wider and
/// collapse, and `main` between them, under the band that holds the panel's button at its
/// right end.
pub fn shell(ui: &Ui, kit: &'static Kit, main: El) -> El {
    SplitView::new(main)
        .sidebar(&ui.sidebar, sidebar_pane, sidebar(ui, kit))
        .inspector(&ui.panel, panel_pane, panel::panel(ui, kit))
        .toolbar(Toolbar::new().trailing(panel::button(ui, kit)))
        .build(kit)
}

fn sidebar_pane(ui: &mut Ui) -> &mut PaneState {
    &mut ui.sidebar
}

fn panel_pane(ui: &mut Ui) -> &mut PaneState {
    &mut ui.panel
}

/// The sidebar: "New session", which asks for the new session's folder (`folder::ask`), with
/// the folder picker's error under it while there is one; the groups and their sessions; and
/// the usage footer.
fn sidebar(ui: &Ui, kit: &'static Kit) -> El {
    let new_session = Button::new("New session")
        .icon(&rune_theme_opencode::icons::PLUS)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .on_press_tx(|ui: &mut Ui, tx: &mut Tx| folder::ask(ui, tx, None, false))
        .build(kit)
        .key(NEW_SESSION)
        .w_full();
    let header = col()
        .key("mr-sidebar-header")
        .w_full()
        .gap(4.0)
        .child(new_session)
        .children(folder::error(ui, kit));
    Sidebar::new()
        .label("Sessions")
        .header(header)
        .child(sidebar::list(ui, kit))
        .footer(footer(ui, kit))
        .build(kit)
}

/// Who is signed in, with the button that quits at the right end of its line, then the two
/// usage lines, each with its bar and reset wording. The button shows also when no account is
/// known.
fn footer(ui: &Ui, kit: &Kit) -> El {
    let [session, weekly] = usage_lines(&ui.app.usage, ui.app.now_unix);
    let mut account = row()
        .key("mr-account-line")
        .w_full()
        .gap(8.0)
        .items_center();
    account = match &ui.app.account {
        Some(known) => account.child(
            text(account_line(known))
                .key("mr-account")
                .font(kit.ty.ui_medium)
                .color(kit.c.text_base)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        ),
        None => account.child(el().grow(1.0)),
    };
    let quit = Button::new("Quit")
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .help("Quit MindRune (Ctrl+Q)")
        .on_press_tx(|ui: &mut Ui, tx: &mut Tx| {
            glue::ask_quit(ui, tx);
        })
        .build(kit)
        .key("mr-quit");
    col()
        .key("mr-footer")
        .w_full()
        .gap(10.0)
        .px(8.0)
        .pt(6.0)
        .pb(4.0)
        .child(account.child(quit))
        .child(usage(&session, "mr-usage-session", kit))
        .child(usage(&weekly, "mr-usage-weekly", kit))
}

/// One usage line: its text keyed `key`, a thin bar keyed `{key}-bar` filled to the fraction
/// used, and when the window resets. An unknown line has an empty bar and no reset wording. The
/// text and the bar take the colour of the line's level. The right panel draws its context line
/// with it too.
pub(super) fn usage(line: &UsageLine, key: &str, kit: &Kit) -> El {
    let color = theme::level_color(kit, line.level);
    let mut bar = row()
        .key(format!("{key}-bar"))
        .w_full()
        .h(USAGE_BAR_HEIGHT)
        .radius(USAGE_BAR_HEIGHT / 2.0)
        .fill(kit.c.layer_03)
        .clip_soft(0.0);
    if let Some(fraction) = line.fraction {
        bar = bar.child(
            el().key("mr-fill")
                .h_full()
                .w_frac(fraction)
                .radius(USAGE_BAR_HEIGHT / 2.0)
                .fill(color),
        );
    }
    let mut group = col()
        .key(format!("{key}-line"))
        .w_full()
        .gap(4.0)
        .child(
            text(line.text.clone())
                .key(key.to_string())
                .font(kit.ty.small)
                .color(color)
                .truncate()
                .w_full(),
        )
        .child(bar);
    if let Some(resets) = &line.resets {
        group = group.child(
            text(resets.clone())
                .key(format!("{key}-resets"))
                .font(kit.ty.small)
                .color(kit.c.text_faint)
                .truncate()
                .w_full(),
        );
    }
    group
}

#[cfg(test)]
mod tests {
    use mr_agent::{SessionCommand, UsageLimits};
    use rune::Key;

    use mr_core::{Place, SessionKey};
    use rune::{Runtime, Tx};

    use super::super::folder;
    use crate::glue::{Picker, Ui};
    use crate::testkit;

    /// A new session in the folder of `from`'s session, as choosing that folder starts one.
    fn new_session(ui: &mut Ui, tx: &mut Tx, from: SessionKey) {
        let folder = ui.app.sessions.get(from).unwrap().session.folder.clone();
        folder::start(ui, tx, folder, None, Place::Focused);
    }

    /// Clicks "New session" and chooses a folder that is there, the only recent one.
    fn new_session_in_a_folder(rt: &mut Runtime<Ui>, dir: &tempfile::TempDir) {
        rt.state.app.recent_folders = vec![dir.path().to_path_buf()];
        testkit::click(rt, "mr-new-session");
        testkit::step(rt);
        testkit::click(rt, "mr-folder-item0");
    }

    #[test]
    fn a_new_session_forgets_the_old_ones_open_picker() {
        let mut rt = testkit::window("conversation");
        let old = testkit::focused(&rt).unwrap();
        testkit::shown_view_mut(&mut rt).open_picker = Some(Picker::Mode);
        rt.update(|ui, tx| new_session(ui, tx, old));
        assert_eq!(testkit::shown_open(&rt), Some(2));
        assert_eq!(testkit::shown_view(&rt).open_picker, None);
    }

    #[test]
    fn new_session_keeps_a_session_with_nothing_said_yet() {
        let mut rt = testkit::window("empty");
        let old = testkit::focused(&rt).unwrap();
        testkit::shown_view_mut(&mut rt).open_picker = Some(Picker::Mode);
        rt.update(|ui, tx| new_session(ui, tx, old));
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_open(&rt), Some(1));
        assert_eq!(
            testkit::shown_view(&rt).open_picker,
            Some(Picker::Mode),
            "the same session"
        );
        assert!(!testkit::sent(&rt).contains(&SessionCommand::Close));
    }

    #[test]
    fn the_footer_always_shows_who_and_how_much() {
        let rt = testkit::window("empty");
        for part in [
            "alex · Max",
            "37% of current session used",
            "Resets in 2 h 14 min",
            "10% of weekly limit used",
            "Resets in 3 d 4 h",
        ] {
            assert!(testkit::has_text(&rt, part), "{part}");
        }
        let rt = testkit::window("limit");
        assert!(testkit::has_text(&rt, "100% of current session used"));
    }

    #[test]
    fn before_any_numbers_the_footer_says_so() {
        let mut rt = testkit::window("empty");
        rt.state.app.usage = UsageLimits {
            session: None,
            weekly: None,
        };
        rt.invalidate();
        testkit::step(&mut rt);
        let unknown = testkit::texts(&rt)
            .iter()
            .filter(|t| t.as_str() == "Usage not known yet")
            .count();
        assert_eq!(unknown, 2);
    }

    #[test]
    fn new_session_replaces_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = testkit::window("conversation");
        assert!(testkit::has_text(&rt, "A small Rust workspace"));
        new_session_in_a_folder(&mut rt, &dir);
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_open(&rt), Some(2));
        assert!(
            rt.find("mr-empty-hint").is_some(),
            "the new session is shown"
        );
        assert!(!testkit::has_text(&rt, "A small Rust workspace"));
        assert!(
            !testkit::sent(&rt).contains(&SessionCommand::Close),
            "the old session keeps its program"
        );
    }

    #[test]
    fn a_new_session_shows_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = testkit::window("conversation");
        let reply = "A small Rust workspace";
        assert!(testkit::has_text(&rt, reply));
        new_session_in_a_folder(&mut rt, &dir);
        let fading = rt
            .elements()
            .into_iter()
            .filter_map(|e| e.text)
            .any(|t| t.contains(reply));
        assert!(
            !fading,
            "the old conversation does not fade out over the new one"
        );
    }

    #[test]
    fn the_kits_shortcut_hides_and_shows_the_sidebar() {
        let mut rt = testkit::window("empty");
        let toggle = testkit::ctrl_shift();
        testkit::press_with(&mut rt, Key::Command('b'), toggle);
        assert!(!rt.state.sidebar.is_open());
        testkit::press_with(&mut rt, Key::Command('b'), toggle);
        assert!(rt.state.sidebar.is_open());
    }
}
