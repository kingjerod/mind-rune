//! The conversation: the current session's items as virtual rows in a scroll that follows the
//! newest text until the user scrolls up.

use std::borrow::Cow;

use mr_core::{Item, ItemBody};
use rune::{El, GrowthSpec, ScrollSpec, col, memo, row, text};
use rune_kit::{Button, ButtonVariant, Kit};
use rune_theme_opencode::icons::ARROW_DOWN_TO_LINE;

use crate::glue::Ui;
use crate::view::items;

/// The height a row is taken to have until it is first laid out.
const ESTIMATED_ROW_HEIGHT: f32 = 64.0;

/// The space above the first row, inside the list; each row has its own space below it.
const LIST_PAD_TOP: f32 = 16.0;

/// How far above the list's bottom edge "Jump to latest" floats.
const JUMP_INSET: f32 = 16.0;

/// The current session's conversation: a scroll keyed `mr-conversation` taking the height left
/// in its column, holding one virtual row per item, with the empty hint or "Jump to latest"
/// floating over it.
///
/// The scroll spans the tile, so the wheel works over the margins too; its rows are `width`
/// points wide and centred (the whole width while the tile's width is not known yet).
///
/// Only the rows near the viewport are built, and a finished row is built again only when what
/// it draws from changes, so the cost of a frame does not grow with the conversation. The width
/// reaches the rows through the list's own width alone, so a new width lays the built rows out
/// again without building them again.
pub fn conversation(ui: &Ui, kit: &'static Kit, width: Option<f32>) -> El {
    let count = items_of(ui).len();
    let rows = col().key("mr-rows");
    let rows = match width {
        Some(width) => rows.w(width).self_center(),
        None => rows.w_full(),
    };
    let rows = rows
        .pt(LIST_PAD_TOP)
        .growth(GrowthSpec::default())
        .virtual_rows(
            count,
            ESTIMATED_ROW_HEIGHT,
            |ui: &Ui, i| Cow::Owned(items::row_key(items_of(ui)[i].id)),
            move |ui: &Ui, i| build_row(ui, &items_of(ui)[i], kit),
        )
        // Keys come from item ids, which never change and are only appended, so they change
        // only with the row count (which Rune always checks) or with a new session.
        .virtual_rows_version(ui.app.current_open());
    let list = col()
        .key("mr-conversation")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: true })
        .on_at_bottom(|ui: &mut Ui, at: bool| ui.following = at)
        .child(rows);
    let mut frame = col()
        .key("mr-conversation-frame")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(list);
    if count == 0 {
        frame = frame.child(empty_hint(ui, kit));
    } else if !ui.following {
        frame = frame.child(jump_to_latest(kit));
    }
    frame
}

/// A streaming reply grows in place, marked as the stack's streaming row; any other row is
/// kept by `memo` until what it draws from changes.
fn build_row(ui: &Ui, item: &Item, kit: &'static Kit) -> El {
    if matches!(
        item.body,
        ItemBody::Reply {
            streaming: true,
            ..
        }
    ) {
        return items::row(ui, item, kit).streaming(true);
    }
    memo(items::row_key(item.id), items::version(ui, item), || {
        items::row(ui, item, kit)
    })
}

fn items_of(ui: &Ui) -> &[Item] {
    ui.app
        .session
        .as_ref()
        .map_or(&[], |session| session.conversation.items())
}

/// One centred line over the empty list, naming the session's folder.
fn empty_hint(ui: &Ui, kit: &Kit) -> El {
    let folder = ui
        .app
        .session
        .as_ref()
        .map_or(ui.app.folder.as_path(), |session| session.folder.as_path());
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string());
    row()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .center()
        .px(24.0)
        .child(
            text(format!("What should Claude do in {name}?"))
                .key("mr-empty-hint")
                .font(kit.ty.body)
                .color(kit.c.text_muted)
                .truncate(),
        )
}

/// "Jump to latest", floating over the bottom of the list while the user reads further up.
fn jump_to_latest(kit: &Kit) -> El {
    let button = Button::new("Jump to latest")
        .icon(&ARROW_DOWN_TO_LINE)
        .variant(ButtonVariant::Neutral)
        .on_press_tx(|_: &mut Ui, tx| tx.scroll_to_bottom("mr-conversation"))
        .build(kit)
        .key("mr-jump-latest");
    row()
        .pin(Some(0.0), None, Some(0.0), Some(JUMP_INSET))
        .justify_center()
        .child(button)
}

#[cfg(test)]
mod tests {
    use mr_agent::SessionEvent;
    use rune::Runtime;

    use crate::glue::Ui;
    use crate::testkit;

    fn item_keys(rt: &Runtime<Ui>) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter_map(|e| e.key)
            .filter(|k| k.starts_with("mr-item-"))
            .collect()
    }

    #[test]
    fn an_empty_session_asks_what_to_do() {
        let rt = testkit::window("empty");
        assert!(rt.find("mr-empty-hint").is_some());
        assert!(
            testkit::texts(&rt)
                .iter()
                .any(|t| t.starts_with("What should Claude do in "))
        );
    }

    #[test]
    fn the_users_message_is_shown_as_typed_and_the_reply_as_markdown() {
        let rt = testkit::window("conversation");
        assert!(testkit::has_text(&rt, "**hi**, what is in this folder?"));
        assert!(rt.find("mr-empty-hint").is_none());
        assert!(
            !testkit::texts(&rt).iter().any(|t| t.contains("```")),
            "a fence must not show as text"
        );
    }

    #[test]
    fn a_reply_cut_off_inside_a_code_block_still_draws() {
        let rt = testkit::window("streaming");
        assert_eq!(item_keys(&rt).len(), 2);
    }

    #[test]
    fn two_thousand_items_build_only_what_is_on_screen() {
        let rt = testkit::window("long");
        let keys = item_keys(&rt);
        assert!(keys.len() < 100, "{} rows were built", keys.len());
        assert!(testkit::has_text(&rt, "Reply 2000"));
        assert!(!testkit::has_text(&rt, "Reply 1000"));
    }

    #[test]
    fn the_list_follows_new_text_until_the_user_scrolls_up() {
        let mut rt = testkit::window("long");
        assert_eq!(rt.scroll_at_bottom("mr-conversation"), Some(true));
        assert!(rt.find("mr-jump-latest").is_none());
        testkit::emit(
            &mut rt,
            SessionEvent::TextDelta("A new line at the end".into()),
        );
        assert_eq!(rt.scroll_at_bottom("mr-conversation"), Some(true));
        assert!(testkit::has_text(&rt, "A new line at the end"));
        let (x, y) = rt.find("mr-conversation").unwrap();
        rt.wheel(x, y, 600.0);
        testkit::step(&mut rt);
        assert_eq!(rt.scroll_at_bottom("mr-conversation"), Some(false));
        testkit::emit(&mut rt, SessionEvent::TextDelta(" and more".into()));
        assert_eq!(
            rt.scroll_at_bottom("mr-conversation"),
            Some(false),
            "new text must not pull the view down"
        );
        testkit::click(&mut rt, "mr-jump-latest");
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(rt.scroll_at_bottom("mr-conversation"), Some(true));
    }
}
