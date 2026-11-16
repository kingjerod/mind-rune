//! The right panel: the kit's trailing pane beside the row of tiles, which describes the
//! focused tile's session, and the button in the band above the tiles that shows and hides it.

use std::path::{Path, PathBuf};

use mr_agent::{ContextKind, ContextPart};
use mr_core::{
    Branch, FileChange, HistoryState, ItemBody, Level, NoticeKind, Phase, Remote, Session,
    SessionKey, UsageLine, mode_label,
};
use rune::{Color, El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::shell::INSPECTOR;
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Switch};

use super::requests::unfocusable;
use super::shell;
use super::tiles::NEW_SESSION;
use crate::glue::{self, Ui};

/// The key of the panel's content.
const PANEL: &str = "mr-panel";
/// The key of the button that shows and hides the panel.
const TOGGLE: &str = "mr-panel-toggle";
/// The keys of the panel's heading, and of what it says with no tile.
const TITLE: &str = "mr-panel-title";
const EMPTY: &str = "mr-panel-empty";

/// The keys of the rows of "Environment", and of the label and the value each row holds.
const FOLDER: &str = "mr-panel-folder";
const BRANCH: &str = "mr-panel-branch";
const MODEL: &str = "mr-panel-model";
const EFFORT: &str = "mr-panel-effort";
const MODE: &str = "mr-panel-mode";
const LABEL: &str = "mr-panel-label";
const VALUE: &str = "mr-panel-value";

/// The keys of the Remote Control switch, of the line under it, of "Copy link" and of the
/// switch for new sessions.
const REMOTE: &str = "mr-panel-remote";
const REMOTE_STATUS: &str = "mr-panel-remote-status";
const REMOTE_COPY: &str = "mr-panel-remote-copy";
const REMOTE_NEW: &str = "mr-panel-remote-new";

/// The keys of the context line (its bar is keyed `mr-panel-context-bar`) and of the cost line.
const CONTEXT: &str = "mr-panel-context";
const COST: &str = "mr-panel-cost";

/// The keys of the name and of the tokens in a row of what the context is made of; the row
/// itself is keyed `mr-panel-context-part-{i}`.
const PART_NAME: &str = "mr-panel-context-part-name";
const PART_TOKENS: &str = "mr-panel-context-part-tokens";

/// The keys of "Files changed", of the number beside its heading, of what it says with no file
/// (or that they are not known yet) and of its line about a history not loaded whole; of a
/// row's path and counts.
const FILES: &str = "mr-panel-files";
const FILES_COUNT: &str = "mr-panel-files-count";
const NO_FILES: &str = "mr-panel-files-none";
const NOT_LISTED: &str = "mr-panel-files-earlier";
const PATH: &str = "mr-panel-file-path";
const ADDED: &str = "mr-panel-file-added";
const REMOVED: &str = "mr-panel-file-removed";

/// How wide the labels of "Environment" are, so that the values start in one column.
const LABEL_WIDTH: f32 = 64.0;

/// The corner radius of the kit's switch track at its default size, Mini: 16 points high.
const TRACK_RADIUS: f32 = 8.0;

/// How opaque the kit draws a disabled switch.
const DISABLED_OPACITY: f32 = 0.5;

/// The shortcut the button's tooltip names: `shell::is_panel_toggle`.
const SHORTCUT: &str = if cfg!(target_os = "macos") {
    "⌥⌘B"
} else {
    "Ctrl+Alt+B"
};

/// What the right panel shows: the focused tile's session, or that there is none.
///
/// For a session: its title, then "Environment" (its folder, its branch, its model, its effort
/// and its mode), then the Remote Control switch with the line under it, "Copy link" while it is
/// on with an address, and the switch for new sessions, which shows with no session too; then
/// "Context" (the share of the context window in use, what it is made of and the cost so far)
/// and "Files changed".
///
/// It reads `App::focused()` at every build, so it follows the tile focus as it moves and when
/// the focused tile closes, and shows what the session's events changed, a branch read late
/// included: the window runs the view again whenever its poll takes a message in. A press
/// anywhere in it leaves the keyboard and the tile focus where they are. Its content scrolls as
/// one column when it is higher than the window.
pub fn panel(ui: &Ui, kit: &'static Kit) -> El {
    let c = &kit.c;
    let content = match ui.app.focused_entry() {
        Some(entry) => {
            let key = entry.key;
            let session = &entry.session;
            let title = session
                .title
                .clone()
                .unwrap_or_else(|| NEW_SESSION.to_owned());
            col()
                .w_full()
                .gap(20.0)
                .child(
                    text(title)
                        .key(TITLE)
                        .role("heading")
                        .font(kit.ty.body_medium)
                        .color(c.text_base)
                        .wrap()
                        .w_full(),
                )
                .child(environment(ui, key, session, kit))
                .child(remote_control(ui, key, session, kit))
                .child(context(session, kit))
                .child(files(session, kit))
        }
        None => col()
            .w_full()
            .gap(20.0)
            .child(
                text("No session open.")
                    .key(EMPTY)
                    .font(kit.ty.ui)
                    .color(c.text_muted)
                    .wrap()
                    .w_full(),
            )
            .child(for_new_sessions(ui, kit)),
    };
    let panel = col()
        .key(PANEL)
        .w_full()
        .h_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        // A press that no element takes blurs the focused field, whatever its ancestors ask
        // for: the panel takes every press on its plain content and does nothing with it.
        .on_press(|_: &mut Ui| {})
        // After the press handler, which would make it a button.
        .role("group")
        .child(col().w_full().padding(16.0).child(content));
    unfocusable(panel)
}

/// "Environment": the folder, the branch when there is one, the model, the effort when the
/// model takes a level and one is known, and the permission mode.
fn environment(ui: &Ui, key: SessionKey, session: &Session, kit: &Kit) -> El {
    let folder = &session.folder;
    let shown = home_relative(folder, ui.app.home());
    let branch = ui.app.branch_of(key).map(|branch| match branch {
        Branch::Named(name) => name.clone(),
        Branch::Detached(commit) => format!("detached at {commit}"),
    });
    let effort = session
        .effort
        .as_deref()
        .filter(|_| !session.effort_levels().is_empty())
        .map(Session::effort_label);
    col()
        .w_full()
        .gap(6.0)
        .child(heading("Environment", kit))
        .child(line(
            FOLDER,
            "Folder",
            text(shown)
                .truncate_middle()
                .help(folder.display().to_string()),
            kit,
        ))
        .children(branch.map(|branch| line(BRANCH, "Branch", text(branch).truncate(), kit)))
        .child(line(
            MODEL,
            "Model",
            text(session.model_label()).truncate(),
            kit,
        ))
        .children(effort.map(|effort| line(EFFORT, "Effort", text(effort).truncate(), kit)))
        .child(line(
            MODE,
            "Mode",
            text(mode_label(&session.mode)).truncate(),
            kit,
        ))
}

/// A heading of a part of the panel.
fn heading(words: &str, kit: &Kit) -> El {
    text(words)
        .role("heading")
        .font(kit.ty.small_medium)
        .color(kit.c.text_muted)
}

/// A row of "Environment" keyed `key`: `label`, muted, and `value`, one line the caller cuts.
fn line(key: &'static str, label: &str, value: El, kit: &Kit) -> El {
    row()
        .key(key)
        .w_full()
        .gap(8.0)
        .child(
            text(label)
                .key(LABEL)
                .font(kit.ty.ui)
                .color(kit.c.text_muted)
                .w(LABEL_WIDTH),
        )
        .child(
            value
                .key(VALUE)
                .font(kit.ty.ui)
                .color(kit.c.text_base)
                .grow(1.0)
                .min_w(0.0),
        )
}

/// `folder` with the home folder `home` at its start written `~`.
fn home_relative(folder: &Path, home: &Path) -> String {
    if home.parent().is_none() {
        return folder.display().to_string();
    }
    match folder.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => PathBuf::from("~").join(rest).display().to_string(),
        Err(_) => folder.display().to_string(),
    }
}

/// The Remote Control switch for `key`'s session, the line under it with "Copy link" beside it
/// while the session is reachable, and the switch for new sessions.
///
/// The switch shows on while Remote Control is asked for or the program says it is not off, so
/// it never disagrees with the mark in the tile's header; a press asks for the opposite. It is
/// disabled when the session has ended or the agent has no Remote Control, and the line says
/// so. The address is copied, never drawn.
fn remote_control(ui: &Ui, key: SessionKey, session: &Session, kit: &Kit) -> El {
    let available = ui.app.capabilities.remote_access;
    let ended = matches!(session.phase, Phase::Ended { .. });
    let disabled = !available || ended;
    let switch = Switch::new(shows_on(session))
        .label("Remote Control")
        .hide_label()
        .disabled(disabled)
        .on_toggle_tx(move |ui: &mut Ui, tx: &mut Tx| {
            let on = ui
                .app
                .sessions
                .get(key)
                .is_some_and(|entry| !shows_on(&entry.session));
            let effects = ui.app.set_remote(key, on);
            glue::run(ui, effects);
            back_to_typing(ui, tx);
        })
        .build(kit)
        .key(REMOTE);
    let switch = edged(switch, REMOTE, disabled, kit);
    let said = if !available {
        "Remote Control is not available.".to_owned()
    } else if ended {
        "This session has ended.".to_owned()
    } else {
        remote_line(session)
    };
    let reachable = matches!(session.remote, Remote::On { url: Some(_) });
    let copy = reachable.then(|| {
        Button::new("Copy link")
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
                let entry = ui.app.sessions.get(key);
                if let Some(Remote::On { url: Some(url) }) = entry.map(|e| &e.session.remote) {
                    tx.copy(url.clone());
                }
                back_to_typing(ui, tx);
            })
            .build(kit)
            .key(REMOTE_COPY)
    });
    col()
        .w_full()
        .gap(6.0)
        .child(labelled("Remote Control", switch, kit))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text(said)
                        .key(REMOTE_STATUS)
                        .font(kit.ty.small)
                        .color(kit.c.text_muted)
                        .wrap()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .children(copy),
        )
        .child(for_new_sessions(ui, kit))
}

/// Whether the Remote Control switch of `session` shows on: it is asked for, or the program
/// says it is not off.
fn shows_on(session: &Session) -> bool {
    session.remote_wanted || session.remote != Remote::Off
}

/// The line under the Remote Control switch of a session that takes it: why it last failed,
/// that it waits for the program to start, or what the program says of it.
fn remote_line(session: &Session) -> String {
    if let Some(said) = &session.remote_said {
        return said.clone();
    }
    let waits = matches!(session.phase, Phase::Resting | Phase::Opening);
    let words = match &session.remote {
        _ if session.remote_wanted && waits => "Turns on when the session starts.",
        // Asked of a running program, which has not answered yet.
        Remote::Off if session.remote_wanted => "Connecting…",
        Remote::Starting => "Connecting…",
        Remote::Reconnecting => "Reconnecting…",
        Remote::On { .. } => "On. Reachable from claude.ai/code and the Claude app.",
        Remote::Off => "Off.",
    };
    words.to_owned()
}

/// "On for new sessions": whether a session made with "New session" starts with Remote Control
/// asked for. It is kept between runs.
fn for_new_sessions(ui: &Ui, kit: &Kit) -> El {
    let disabled = !ui.app.capabilities.remote_access;
    let switch = Switch::new(ui.app.remote_for_new)
        .label("On for new sessions")
        .hide_label()
        .disabled(disabled)
        .on_toggle_tx(|ui: &mut Ui, tx: &mut Tx| {
            let effects = ui.app.set_remote_for_new(!ui.app.remote_for_new);
            glue::run(ui, effects);
            back_to_typing(ui, tx);
        })
        .build(kit)
        .key(REMOTE_NEW);
    let switch = edged(switch, REMOTE_NEW, disabled, kit);
    labelled("On for new sessions", switch, kit)
}

/// `switch`, the kit's switch keyed `key` with its label hidden, under a 1-point edge round its
/// track, keyed `{key}-edge`. The kit draws an off track in `layer_04`, which in the light
/// theme is as light as the panel under it: the edge shows where the switch is. It is drawn
/// over the track's outer point, by this app, and is inert: the switch keeps its look on and
/// off, its hit area, its place in the Tab order and what assistive tools hear. It takes no
/// room, and is faded with the switch while that is disabled.
fn edged(switch: El, key: &str, disabled: bool, kit: &Kit) -> El {
    let edge = el()
        .key(format!("{key}-edge"))
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .radius(TRACK_RADIUS)
        .border(1.0, edge_color(kit))
        .opacity(if disabled { DISABLED_OPACITY } else { 1.0 })
        .inert()
        .a11y_hidden();
    el().child(switch).child(edge)
}

/// The colour of the edge round a switch's track: the theme's muted icon colour. No border
/// colour of the theme stands out from the panel in the light theme (each is black at a tenth
/// or so), but for the focus ring's, which would read as focus.
fn edge_color(kit: &Kit) -> Color {
    kit.c.icon_muted
}

/// `words` on the left and `control` on the right of one row. The words are only drawn: the
/// control carries them for assistive tools.
fn labelled(words: &str, control: El, kit: &Kit) -> El {
    row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            text(words)
                .font(kit.ty.ui)
                .color(kit.c.text_base)
                .grow(1.0)
                .min_w(0.0)
                .truncate()
                .a11y_hidden(),
        )
        .child(control)
}

/// After a press with the pointer on a control of the panel, which as a Tab stop took the
/// keyboard, the keyboard goes back to the box of the focused tile, where the user types. A
/// press by keys leaves it on the control, as a keyboard user expects.
fn back_to_typing(ui: &mut Ui, tx: &Tx) {
    if tx.by_keys() {
        return;
    }
    if let Some(key) = ui.app.focused() {
        glue::type_in(ui, key);
    }
}

/// "Context": the share of the context window the session uses, over a bar built and coloured
/// as the sidebar's usage bars are; under it what the context is made of, part by part, when
/// the program said; and what the session has cost so far. The share and the cost are what the
/// program said when it last finished a turn, the share before that the breakdown's, and "not
/// known yet" with neither.
fn context(session: &Session, kit: &Kit) -> El {
    let used = match session.context_share() {
        Some(share) => {
            let percent = mr_agent::percent(share);
            let line = UsageLine {
                text: format!("{percent}% of context used"),
                fraction: Some(share.clamp(0.0, 1.0)),
                level: Level::of(percent),
                resets: None,
            };
            shell::usage(&line, CONTEXT, kit)
        }
        None => small("Context use is not known yet.", kit).key(CONTEXT),
    };
    let parts = session.context_parts.iter().flat_map(|breakdown| {
        breakdown
            .parts
            .iter()
            .filter(|part| part.tokens > 0)
            .enumerate()
            .map(|(i, part)| context_part(i, part, breakdown.window, kit))
    });
    let cost = match session.cost_usd {
        Some(cost) => format!("Cost so far: {}", dollars(cost)),
        None => "Cost is not known yet.".to_owned(),
    };
    col()
        .w_full()
        .gap(6.0)
        .child(heading("Context", kit))
        .child(used)
        .children(parts)
        .child(small(cost, kit).key(COST))
}

/// The row `i` of what the context is made of: the name of `part` on the left, cut when too
/// long; on the right its tokens and its share of `window`, `<1%` for a share that would read
/// 0%. The buffer and the free space are muted.
fn context_part(i: usize, part: &ContextPart, window: u64, kit: &Kit) -> El {
    let percent = mr_agent::percent(part.tokens as f32 / window.max(1) as f32);
    let share = if percent == 0 {
        "<1%".to_owned()
    } else {
        format!("{percent}%")
    };
    let color = match part.kind {
        ContextKind::Used => kit.c.text_base,
        ContextKind::Buffer | ContextKind::Free => kit.c.text_muted,
    };
    row()
        .key(format!("mr-panel-context-part-{i}"))
        .w_full()
        .gap(8.0)
        .child(
            text(part.name.clone())
                .key(PART_NAME)
                .font(kit.ty.ui)
                .color(color)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(
            text(format!("{} ({share})", token_count(part.tokens)))
                .key(PART_TOKENS)
                .font(kit.ty.ui)
                .color(color),
        )
}

/// `tokens` as Claude Code's `/context` writes them: the number itself below 1,000, then
/// thousands with one decimal and `k`, then millions with one decimal and `M`, rounded half up
/// at the decimal.
fn token_count(tokens: u64) -> String {
    // In tenths of a thousand, and of a million: 999,950 reads 1.0M, not 1000.0k.
    let tenths = |unit: u64| tokens.saturating_add(unit / 20) / (unit / 10);
    match tokens {
        0..1_000 => tokens.to_string(),
        1_000..999_950 => {
            let t = tenths(1_000);
            format!("{}.{}k", t / 10, t % 10)
        }
        _ => {
            let t = tenths(1_000_000);
            format!("{}.{}M", t / 10, t % 10)
        }
    }
}

/// `cost` in dollars with two decimals, or "less than $0.01" for an amount above nothing that
/// would read $0.00.
fn dollars(cost: f64) -> String {
    if cost > 0.0 && cost < 0.01 {
        "less than $0.01".to_owned()
    } else {
        format!("${:.2}", cost.max(0.0))
    }
}

/// "Files changed", with their number beside it: a row for each file the session's edits and
/// writes changed, in the order the conversation counted them, with its lines added and
/// removed. The rows are the conversation's own count, kept as its calls finish: nothing here
/// walks the conversation. With no row it says that none changed, or, while the session's
/// stored history is not read, that they are not known yet. A last line says when the start
/// of the session's history is not loaded, since its edits are not counted.
fn files(session: &Session, kit: &Kit) -> El {
    let changed = session.conversation.changes().files();
    let count = (!changed.is_empty()).then(|| {
        text(changed.len().to_string())
            .key(FILES_COUNT)
            .font(kit.ty.small_medium)
            .color(kit.c.text_faint)
    });
    let title = row()
        .gap(6.0)
        .center_y()
        .child(heading("Files changed", kit))
        .children(count);
    let none = changed.is_empty().then(|| {
        let words = match session.history {
            HistoryState::Loaded => "No files changed yet.",
            HistoryState::NotLoaded | HistoryState::Loading | HistoryState::Failed(_) => {
                "Files changed are not known yet."
            }
        };
        small(words, kit).key(NO_FILES)
    });
    let earlier = earlier_left_out(session).then(|| {
        small(
            "Changes in earlier messages that are not loaded are not listed.",
            kit,
        )
        .key(NOT_LISTED)
    });
    col()
        .key(FILES)
        .w_full()
        .gap(6.0)
        .child(title)
        .children(none)
        .children(
            changed
                .iter()
                .enumerate()
                .map(|(i, file)| changed_file(i, file, &session.folder, kit)),
        )
        .children(earlier)
}

/// The row `i` of "Files changed": the path of `file`, relative to `folder` when it lies in it
/// and cut at its start when too long, the whole path as its tooltip; then `+added` and
/// `−removed` in a diff's colours, a count of 0 left out.
fn changed_file(i: usize, file: &FileChange, folder: &Path, kit: &Kit) -> El {
    let shown = match Path::new(&file.path).strip_prefix(folder) {
        Ok(rest) if !rest.as_os_str().is_empty() => rest.display().to_string(),
        _ => file.path.clone(),
    };
    let count = |key: &'static str, words: String, color: Color| {
        text(words).key(key).font(kit.ty.ui).color(color)
    };
    let added =
        (file.added > 0).then(|| count(ADDED, format!("+{}", file.added), kit.c.success.fg));
    let removed =
        (file.removed > 0).then(|| count(REMOVED, format!("−{}", file.removed), kit.c.danger.fg));
    row()
        .key(format!("mr-panel-file-{i}"))
        .w_full()
        .gap(6.0)
        .child(
            text(shown)
                .key(PATH)
                .font(kit.ty.ui)
                .color(kit.c.text_base)
                .truncate_head()
                .help(file.path.clone())
                .grow(1.0)
                .min_w(0.0),
        )
        .children(added)
        .children(removed)
}

/// Whether the start of `session`'s history was left out when it was read: the conversation
/// then opens with the notice that says so.
fn earlier_left_out(session: &Session) -> bool {
    session.conversation.items().first().is_some_and(|item| {
        matches!(
            item.body,
            ItemBody::Notice {
                kind: NoticeKind::EarlierLeftOut,
                ..
            }
        )
    })
}

/// A line of the panel in the small type, muted, wrapped to its width.
fn small(words: impl Into<String>, kit: &Kit) -> El {
    text(words)
        .font(kit.ty.small)
        .color(kit.c.text_muted)
        .wrap()
        .w_full()
}

/// The icon button in the band above the tiles that shows and hides the panel: "Show panel"
/// while it is hidden, "Hide panel" and pressed-looking while it shows. It is no Tab stop, and
/// a press on it leaves the keyboard where it is.
pub fn button(ui: &Ui, kit: &Kit) -> El {
    let open = ui.panel.is_open();
    let button = IconButton::new(&INSPECTOR)
        .variant(IconButtonVariant::GhostMuted)
        .expanded(open)
        .label(if open { "Hide panel" } else { "Show panel" })
        .shortcut(SHORTCUT)
        .on_press(toggle)
        .build(kit)
        .key(TOGGLE);
    unfocusable(button)
}

/// Shows the panel when it is hidden and hides it when it shows, and saves that at once.
pub fn toggle(ui: &mut Ui) {
    ui.panel.toggle();
    let effects = ui.app.save_now();
    glue::run(ui, effects);
}

#[cfg(test)]
mod tests;
