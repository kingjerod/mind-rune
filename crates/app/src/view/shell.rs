//! The shell: the sidebar, with the new-session button, the current session's row and the
//! usage footer, beside the main area.

use mr_core::{Phase, Session, UsageLine, account_line, usage_lines};
use rune::{El, col, el, row, text};
use rune_kit::shell::{PaneState, RowStatus, Sidebar, SidebarRow, SplitView};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit};

pub use rune_kit::shell::is_sidebar_toggle;

use crate::glue::{self, Ui};
use crate::theme::{self, USAGE_BAR_HEIGHT};

/// The sidebar on the left, which the user can drag wider and collapse, and `main` beside it.
pub fn shell(ui: &Ui, kit: &Kit, main: El) -> El {
    SplitView::new(main)
        .sidebar(&ui.sidebar, sidebar_pane, sidebar(ui, kit))
        .build(kit)
}

fn sidebar_pane(ui: &mut Ui) -> &mut PaneState {
    &mut ui.sidebar
}

/// "New session": replaces the session with a new one in the same folder.
fn new_session(ui: &mut Ui) {
    let effects = ui.app.new_session();
    glue::forget_session_view(ui);
    glue::run(ui, effects);
    // Back to the text area, ready for the new session's first message.
    ui.draft.focused = true;
}

fn sidebar(ui: &Ui, kit: &Kit) -> El {
    let new_session = Button::new("New session")
        .icon(&rune_theme_opencode::icons::PLUS)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .on_press(new_session)
        .build(kit)
        .key("mr-new-session")
        .w_full();
    let mut list = Sidebar::new()
        .label("Sessions")
        .header(new_session)
        .footer(footer(ui, kit));
    if let Some(session) = &ui.app.session {
        list = list.child(session_row(session, kit));
    }
    list.build(kit)
}

/// The current session: its title and folder, with a mark while it works or waits for the user.
fn session_row(session: &Session, kit: &Kit) -> El {
    let title = session.title.as_deref().unwrap_or("New session");
    let folder = session
        .folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| session.folder.display().to_string());
    let mut row = SidebarRow::new(title).subtitle(folder).selected(true);
    match session.phase {
        Phase::Opening | Phase::Working => row = row.status(RowStatus::Busy),
        Phase::NeedsUser => row = row.status(RowStatus::Attention),
        Phase::Idle | Phase::Ended { .. } => {}
    }
    row.build(kit).key("mr-session-row")
}

/// Who is signed in, then the two usage lines, each with its bar and reset wording.
fn footer(ui: &Ui, kit: &Kit) -> El {
    let [session, weekly] = usage_lines(&ui.app.usage, ui.now_unix);
    let mut footer = col()
        .key("mr-footer")
        .w_full()
        .gap(10.0)
        .px(8.0)
        .pt(6.0)
        .pb(4.0);
    if let Some(account) = &ui.app.account {
        footer = footer.child(
            text(account_line(account))
                .key("mr-account")
                .font(kit.ty.ui_medium)
                .color(kit.c.text_base)
                .truncate()
                .w_full(),
        );
    }
    footer
        .child(usage(&session, "mr-usage-session", kit))
        .child(usage(&weekly, "mr-usage-weekly", kit))
}

/// One usage line: its text keyed `key`, a thin bar filled to the fraction used, and when the
/// window resets. An unknown line has an empty bar and no reset wording.
fn usage(line: &UsageLine, key: &str, kit: &Kit) -> El {
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
    use mr_agent::{FakeStep, SessionCommand, SessionEvent, UsageLimits};
    use rune::{Key, Mods};

    use super::new_session;
    use crate::glue::Picker;
    use crate::{demo, testkit};

    #[test]
    fn a_new_session_forgets_the_old_ones_open_picker() {
        let mut rt = testkit::window("empty");
        rt.state.open_picker = Some(Picker::Mode);
        new_session(&mut rt.state);
        assert_eq!(rt.state.app.current_open(), 2);
        assert_eq!(rt.state.open_picker, None);
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
        let mut rt = testkit::window("empty");
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.current_open(), 2);
        assert!(testkit::sent(&rt).contains(&SessionCommand::Close));
    }

    #[test]
    fn the_session_row_shows_the_sessions_title() {
        let mut rt = testkit::window("empty");
        assert!(testkit::has_text(&rt, "New session"));
        testkit::emit(&mut rt, SessionEvent::TitleChanged("Fix the parser".into()));
        assert!(testkit::has_text(&rt, "Fix the parser"));
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
        assert!(testkit::has_text(&rt, "what is in this folder?"));
        let asked: Vec<_> = testkit::sent(&rt)
            .into_iter()
            .filter(|c| matches!(c, SessionCommand::SuggestTitle { .. }))
            .collect();
        assert_eq!(asked.len(), 1);
        demo::pump(&mut rt.state, 1.0);
        rt.invalidate();
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "Listing the folder"));
        assert!(!testkit::has_text(&rt, "what is in this folder?"));
    }

    /// The texts shown inside the session's row.
    fn row_texts(rt: &rune::Runtime<crate::glue::Ui>) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.contains("mr-session-row"))
            .filter_map(|e| e.text)
            .collect()
    }

    #[test]
    fn every_demo_scene_with_a_conversation_has_a_title_in_its_row() {
        let rt = testkit::window("empty");
        assert!(row_texts(&rt).contains(&"New session".to_string()));
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
        ] {
            let rt = testkit::window(scene);
            let session = rt.state.app.session.as_ref().unwrap();
            let title = session
                .title
                .clone()
                .unwrap_or_else(|| panic!("{scene}: no title"));
            assert!(
                row_texts(&rt).contains(&title),
                "{scene}: {title:?} not in the row"
            );
            let first = session
                .conversation
                .items()
                .iter()
                .find_map(|i| match &i.body {
                    mr_core::ItemBody::User { text } => Some(text.clone()),
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

    #[test]
    fn the_kits_shortcut_hides_and_shows_the_sidebar() {
        let mut rt = testkit::window("empty");
        let toggle = Mods {
            shift: true,
            cmd: true,
            ctrl: !cfg!(target_os = "macos"),
            ..Mods::default()
        };
        testkit::press_with(&mut rt, Key::Command('b'), toggle);
        assert!(!rt.state.sidebar.is_open());
        testkit::press_with(&mut rt, Key::Command('b'), toggle);
        assert!(rt.state.sidebar.is_open());
    }
}
