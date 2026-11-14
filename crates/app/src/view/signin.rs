//! The screens shown instead of the shell while the account is checked, when Claude Code is
//! not signed in, and when it is not installed.

use mr_core::Screen;
use rune::{El, Span, col, rich, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Spinner};

use crate::glue::{self, Ui};
use crate::theme::SCREEN_WIDTH;

/// The command that signs Claude Code in.
const LOGIN: &str = "claude auth login";

/// The not-installed body, and the word in it drawn in the monospace font.
const MISSING: &str = "MindRune runs the claude program and could not find it. Install Claude Code, then check again.";
const PROGRAM: &str = "claude";

/// The screen for `Checking`, `SignedOut` or `NotInstalled`, centred in the window.
pub fn screen(ui: &Ui, kit: &Kit) -> El {
    let (key, content) = match ui.app.screen {
        Screen::SignedOut => ("mr-screen-signin", signed_out(kit)),
        Screen::NotInstalled => ("mr-screen-missing", not_installed(kit)),
        // `view` shows the shell on `Main`; this arm only keeps the match whole.
        Screen::Checking | Screen::Main => ("mr-screen-checking", checking(kit)),
    };
    col()
        .key(key)
        .w_full()
        .h_full()
        .fill(kit.c.bg_base)
        .center()
        .padding(24.0)
        .child(content)
}

fn checking(kit: &Kit) -> El {
    row()
        .key("mr-checking")
        .gap(10.0)
        .center_y()
        .child(Spinner::new().size(16.0).color(kit.c.icon_muted).build(kit))
        .child(title("Checking your Claude Code sign-in…", kit))
}

fn signed_out(kit: &Kit) -> El {
    let command = row()
        .key("mr-command")
        .w_full()
        .gap(8.0)
        .pl(12.0)
        .pr(4.0)
        .py(4.0)
        .center_y()
        .radius(kit.radius.md)
        .fill(kit.c.layer_01)
        .border(1.0, kit.c.border_muted)
        .child(
            text(LOGIN)
                .key("mr-command-text")
                .font(kit.ty.mono)
                .color(kit.c.text_base)
                .selectable(true)
                .grow(1.0),
        )
        .child(
            IconButton::new(kit.glyph.copy)
                .variant(IconButtonVariant::GhostMuted)
                .help("Copy")
                .on_press_tx(|_: &mut Ui, tx| tx.copy(LOGIN))
                .build(kit)
                .key("mr-copy-command"),
        );
    card()
        .child(title("Claude Code is not signed in", kit))
        .child(body("Run this in a terminal, then check again:", kit))
        .child(command)
        .child(check_again(kit))
}

fn not_installed(kit: &Kit) -> El {
    let start = MISSING.find(PROGRAM).unwrap_or(0);
    let program = Span::new(start, start + PROGRAM.len()).mono();
    card()
        .child(title("Claude Code is not installed", kit))
        .child(
            rich(MISSING, vec![program])
                .key("mr-body")
                .font(kit.ty.ui)
                .color(kit.c.text_muted)
                .w_full(),
        )
        .child(check_again(kit))
}

/// The column a screen's words and buttons stand in.
fn card() -> El {
    col().key("mr-card").w_full().max_w(SCREEN_WIDTH).gap(12.0)
}

fn title(words: &str, kit: &Kit) -> El {
    text(words.to_string())
        .key("mr-title")
        .font(kit.ty.body_medium)
        .color(kit.c.text_base)
}

fn body(words: &str, kit: &Kit) -> El {
    text(words.to_string())
        .key("mr-body")
        .font(kit.ty.ui)
        .color(kit.c.text_muted)
        .wrap()
        .w_full()
}

fn check_again(kit: &Kit) -> El {
    row().key("mr-actions").w_full().child(
        Button::new("Check again")
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::Large)
            .on_press(|ui: &mut Ui| {
                let effects = ui.app.check_again();
                glue::run(ui, effects);
            })
            .build(kit)
            .key("mr-check-again"),
    )
}

#[cfg(test)]
mod tests {
    use mr_agent::Account;
    use mr_core::Screen;

    use crate::testkit;

    #[test]
    fn signed_out_says_what_to_run_and_checks_again() {
        let mut rt = testkit::window("signed-out");
        for part in [
            "Claude Code is not signed in",
            "Run this in a terminal, then check again:",
            "claude auth login",
        ] {
            assert!(testkit::has_text(&rt, part), "{part}");
        }
        testkit::click(&mut rt, "mr-check-again");
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.screen, Screen::SignedOut, "still signed out");
        let account = Account::SignedIn {
            name: "alex".into(),
            plan: Some("Max".into()),
            full_login: true,
        };
        rt.state.fake.as_ref().unwrap().set_account(account);
        testkit::click(&mut rt, "mr-check-again");
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.screen, Screen::Main);
        assert!(rt.find("mr-footer").is_some());
    }

    #[test]
    fn not_installed_has_its_own_words() {
        let rt = testkit::window("not-installed");
        assert!(testkit::has_text(&rt, "Claude Code is not installed"));
        assert!(rt.find("mr-check-again").is_some());
        assert!(rt.find("mr-footer").is_none());
    }

    #[test]
    fn the_command_can_be_copied() {
        let mut rt = testkit::window("signed-out");
        testkit::click(&mut rt, "mr-copy-command");
        assert_eq!(
            testkit::clipboard(&rt).as_deref(),
            Some("claude auth login")
        );
    }
}
