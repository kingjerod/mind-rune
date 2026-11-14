//! The root view: the sign-in screens until Claude Code is signed in, then the shell.

use mr_core::Screen;
use rune::{Cx, El, KeyEvent, Tx, col, el};
use rune_kit::Kit;

use crate::glue::Ui;
use crate::theme;

mod conversation;
mod input;
mod items;
mod requests;
mod shell;
mod signin;
mod width;

pub fn view(ui: &Ui, cx: &mut Cx<Ui>) -> El {
    let kit = theme::kit(cx.system_dark());
    let mut layer: Vec<El> = Vec::new();
    let base = match ui.app.screen {
        Screen::Main => shell::shell(ui, kit, tile(ui, cx, kit, &mut layer)),
        Screen::Checking | Screen::SignedOut | Screen::NotInstalled => signin::screen(ui, kit),
    };
    // The app's own root, under the kit's: what a screenshot's tree is checked for.
    let base = el().key("mr-root").child(base.w_full().h_full());
    rune_kit::root(base, layer)
}

/// The middle of the shell: the conversation, and below it the input box, whose pickers put
/// their lists in `layer`, both in the column whose width the user drags.
fn tile(ui: &Ui, cx: &mut Cx<Ui>, kit: &'static Kit, layer: &mut Vec<El>) -> El {
    let width = width::current(ui, cx);
    let content = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(conversation::conversation(ui, kit, width))
        .child(input::input(ui, kit, layer, width));
    col()
        .key("mr-tile")
        .w_full()
        .h_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(kit.c.bg_base)
        .child(width::column(ui, cx, kit, content))
}

/// `AppSpec::on_key`, for keys no focused text field took: the kit's sidebar shortcut shows and
/// hides the sidebar, and Escape stops a running turn (a focused text area catches its own
/// Escape, which never gets here). With no turn running Escape is left alone.
pub fn on_key(ui: &mut Ui, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    if shell::is_sidebar_toggle(ev) {
        ui.sidebar.toggle();
        return true;
    }
    input::is_escape(ev) && input::stop_turn(ui)
}
