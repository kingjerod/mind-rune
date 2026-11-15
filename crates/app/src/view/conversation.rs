//! The conversation: a session's items as virtual rows in a scroll that follows the newest
//! text until the user scrolls up.

use std::borrow::Cow;
use std::path::Path;

use mr_core::{Item, ItemBody, SessionKey};
use rune::{El, GrowthSpec, ScrollSpec, col, memo, row, text};
use rune_kit::{Button, ButtonVariant, Kit};
use rune_theme_opencode::icons::ARROW_DOWN_TO_LINE;

use crate::glue::{self, Ui};
use crate::view::{items, resting};

/// The height a row is taken to have until it is first laid out.
const ESTIMATED_ROW_HEIGHT: f32 = 64.0;

/// The space above the first row, inside the list; each row has its own space below it.
const LIST_PAD_TOP: f32 = 16.0;

/// How far above the list's bottom edge "Jump to latest" floats.
const JUMP_INSET: f32 = 16.0;

/// The key of the list of rows inside a conversation's scroll.
pub const ROWS: &str = "mr-rows";

/// The key of `key`'s conversation scroll, so that each session keeps its own place.
pub fn conversation_key(key: SessionKey) -> String {
    format!("mr-conversation-{}", key.0)
}

/// `key`'s conversation: a scroll keyed [`conversation_key`] taking the height left in its
/// column, holding one virtual row per item, with "Jump to latest" floating over it. Over an
/// empty one floats, in its middle, what a stored session says of its history while it is not
/// in (see [`resting::history`]), or else the empty hint.
///
/// The scroll spans the tile, so the wheel works over the margins too; its rows are `width`
/// points wide and centred (the whole width while the tile's width is not known yet).
///
/// Only the rows near the viewport are built, and a finished row is built again only when what
/// it draws from changes, so the cost of a frame does not grow with the conversation. The width
/// reaches the rows through the list's own width alone, so a new width lays the built rows out
/// again without building them again.
pub fn conversation(ui: &Ui, kit: &'static Kit, key: SessionKey, width: Option<f32>) -> El {
    let session = ui.app.sessions.get(key).map(|entry| &entry.session);
    let view = glue::view_of(ui, key);
    let count = items_of(ui, key).len();
    let generation = session.map_or(0, |s| s.generation);
    // `items::toggle_path` climbs from a row's toggle through this list to the scroll keyed
    // `conversation_key`: a keyed wrapper put between them must go into that path too.
    let rows = col().key(ROWS);
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
            move |ui: &Ui, i| Cow::Owned(items::row_key(items_of(ui, key)[i].id)),
            move |ui: &Ui, i| build_row(ui, key, &items_of(ui, key)[i], kit),
        )
        // Keys come from item ids, which only grow, so they change with the row count (which
        // Rune always checks), with another session drawn here, or when its items are
        // numbered again.
        .virtual_rows_version((key.0, generation))
        .on_visible_rows(move |ui: &mut Ui, shown| {
            let first = items_of(ui, key).get(shown.start).map(|item| item.id);
            glue::view_mut(ui, key).first_seen = first;
            false
        });
    let list = col()
        .key(conversation_key(key))
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: true })
        .on_at_bottom(move |ui: &mut Ui, at: bool| glue::view_mut(ui, key).following = at)
        .child(rows);
    let mut frame = col()
        .key("mr-conversation-frame")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(list);
    if count == 0 {
        if let Some(session) = session {
            let over =
                resting::history(session, kit).unwrap_or_else(|| empty_hint(&session.folder, kit));
            frame = frame.child(over);
        }
    } else if !view.following {
        frame = frame.child(jump_to_latest(key, kit));
    }
    frame
}

/// A streaming reply grows in place, marked as the stack's streaming row; any other row is
/// kept by `memo` until what it draws from changes. The memo is named by the session and its
/// generation as well as the item, since every session numbers its items from 0.
fn build_row(ui: &Ui, key: SessionKey, item: &Item, kit: &'static Kit) -> El {
    if matches!(
        item.body,
        ItemBody::Reply {
            streaming: true,
            ..
        }
    ) {
        return items::row(ui, key, item, kit).streaming(true);
    }
    let generation = ui
        .app
        .sessions
        .get(key)
        .map_or(0, |entry| entry.session.generation);
    let id = items::memo_id(key, generation, item.id);
    // The row's own key is the list's (`items::row_key`), which Rune gives every row it builds.
    memo(id.clone(), items::version(ui, key, item), move || {
        #[cfg(test)]
        ROW_BUILDS.with(|builds| *builds.borrow_mut().entry(key).or_default() += 1);
        items::row(ui, key, item, kit).key(id)
    })
}

#[cfg(test)]
thread_local! {
    /// How many memoised rows of each session were built on this thread: each test runs on its
    /// own.
    static ROW_BUILDS: std::cell::RefCell<std::collections::HashMap<SessionKey, usize>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// How many memoised rows of `key`'s session have been built, rather than reused, on this
/// thread.
#[cfg(test)]
pub(crate) fn row_builds(key: SessionKey) -> usize {
    ROW_BUILDS.with(|builds| builds.borrow().get(&key).copied().unwrap_or(0))
}

/// How many memoised rows of every session have been built, rather than reused, on this
/// thread.
#[cfg(test)]
pub(crate) fn all_row_builds() -> usize {
    ROW_BUILDS.with(|builds| builds.borrow().values().sum())
}

fn items_of(ui: &Ui, key: SessionKey) -> &[Item] {
    ui.app
        .sessions
        .get(key)
        .map_or(&[], |entry| entry.session.conversation.items())
}

/// One centred line over the empty list, naming the session's `folder`.
fn empty_hint(folder: &Path, kit: &Kit) -> El {
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

/// "Jump to latest", floating over the bottom of `key`'s list while the user reads further up.
/// Pressing it also focuses the tile.
fn jump_to_latest(key: SessionKey, kit: &Kit) -> El {
    let button = Button::new("Jump to latest")
        .icon(&ARROW_DOWN_TO_LINE)
        .variant(ButtonVariant::Neutral)
        .on_press_tx(move |ui: &mut Ui, tx| {
            glue::focus_tile(ui, key, false);
            tx.scroll_to_bottom(conversation_key(key));
        })
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
        assert_eq!(rt.scroll_at_bottom(&testkit::conversation(&rt)), Some(true));
        assert!(rt.find("mr-jump-latest").is_none());
        testkit::emit(
            &mut rt,
            SessionEvent::TextDelta("A new line at the end".into()),
        );
        assert_eq!(rt.scroll_at_bottom(&testkit::conversation(&rt)), Some(true));
        assert!(testkit::has_text(&rt, "A new line at the end"));
        let (x, y) = rt.find(&testkit::conversation(&rt)).unwrap();
        rt.wheel(x, y, 600.0);
        testkit::step(&mut rt);
        assert_eq!(
            rt.scroll_at_bottom(&testkit::conversation(&rt)),
            Some(false)
        );
        testkit::emit(&mut rt, SessionEvent::TextDelta(" and more".into()));
        assert_eq!(
            rt.scroll_at_bottom(&testkit::conversation(&rt)),
            Some(false),
            "new text must not pull the view down"
        );
        testkit::click(&mut rt, "mr-jump-latest");
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(rt.scroll_at_bottom(&testkit::conversation(&rt)), Some(true));
    }
}
