//! One row of the conversation per item: the user's message, Claude's reply with its copy
//! controls, thinking, tool lines with their details, file edits as diffs, and notices.
//! Requests are drawn as cards by `requests`. A control here that calls no core intent (a
//! row's toggle, "Show all", a copy button) focuses its tile itself; the core's intents do so
//! for the others.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;

use mr_agent::{ToolDetail, ToolOutcome};
use mr_core::{
    Item, ItemBody, ItemId, NoticeKind, OUTPUT_CHARS, OUTPUT_LINES, Phase, RequestState,
    SessionKey, ThinkingTime, ToolFamily, ToolGroup, clip, fenced, file_name, line_counts,
    thinking_label, unified_diff,
};
use rune::{Align, Color, Cursor, El, Icon, Image, Span, SurfaceFit, col, el, icon, rich, text};
use rune_kit::{
    Button, ButtonVariant, CodeFrame, IconButton, IconButtonVariant, Kit, Markdown, Spinner,
};
use rune_theme_opencode::icons::{
    BRAIN, CUBE, FILE_DOCUMENT, MAGNIFYING_GLASS, PENCIL_LINE, PHOTO, TERMINAL, WARNING,
};

use crate::glue::{self, QuestionDraft, SessionView, Ui};
use crate::view::requests;

/// The space below each row's content, inside the row. None goes above it, so a streaming
/// row that climbs into view from nothing shows its first line at once rather than blank space.
const ROW_PAD_BOTTOM: f32 = 16.0;

/// The space below a thinking or tool line, so that consecutive lines read as one list.
const LINE_PAD_BOTTOM: f32 = 2.0;

/// The space either side of each row's content, inside the row.
const ROW_PAD_X: f32 = 24.0;

/// The widest a user's message box grows before its text wraps.
const USER_MAX_WIDTH: f32 = 560.0;

/// The height of the row of a message's pictures, and of each picture in it, whether it has
/// come or not: a picture that comes never moves the conversation.
const PICTURE_HEIGHT: f32 = 120.0;

/// The narrowest and the widest a picture is drawn; a shape outside them shows bands.
const PICTURE_MIN_WIDTH: f32 = 60.0;
const PICTURE_MAX_WIDTH: f32 = 240.0;

/// The space between two pictures.
const PICTURE_GAP: f32 = 6.0;

/// The corners of a picture, and of the slot that waits for one.
const PICTURE_RADIUS: f32 = 6.0;

/// The size of the icon in a slot that waits for its picture.
const PICTURE_ICON: f32 = 24.0;

/// The height of a thinking or tool line.
const LINE_HEIGHT: f32 = 28.0;

/// The size of a line's icon, spinner and marks.
const LINE_ICON: f32 = 14.0;

/// How far a line's details are set in from its left edge: under its summary, past its icon.
const DETAILS_INSET: f32 = 22.0;

/// The key of the row that shows the item `id`.
pub fn row_key(id: ItemId) -> String {
    format!("mr-item-{}", id.0)
}

/// The `rune::memo` id of the row that shows item `id` of `key`'s session, its items numbered in
/// `generation`. Every session numbers its items from 0, and a generation numbers them again,
/// so the item's id alone could name another session's row, or an older one.
pub fn memo_id(key: SessionKey, generation: u64, id: ItemId) -> String {
    format!("mr-item-{}-g{generation}-{}", id.0, key.0)
}

/// The key path of the line that opens and closes item `id` in `key`'s conversation. Rune names
/// an element by its nearest keyed ancestors, so the path climbs to the conversation, whose key
/// carries the session's number: the same item of another tile's session is not named.
pub(crate) fn toggle_path(key: SessionKey, id: ItemId) -> String {
    format!(
        "{}/{}/{}/{}",
        super::conversation_key(key),
        super::conversation::ROWS,
        row_key(id),
        toggle_key(id)
    )
}

/// `mr-toggle-{id}`.
fn toggle_key(id: ItemId) -> String {
    format!("mr-toggle-{}", id.0)
}

/// One item of `key`'s session as a row keyed `mr-item-{id}`, its spacing as padding inside it.
pub fn row(ui: &Ui, key: SessionKey, item: &Item, kit: &'static Kit) -> El {
    let id = item.id;
    let view = glue::view_of(ui, key);
    let folds = matches!(item.body, ItemBody::Thinking { .. } | ItemBody::Tools(_));
    let (content, pad_bottom) = match &item.body {
        ItemBody::User {
            text,
            files,
            pictures,
            ..
        } => {
            let whole = view.whole.contains(&id);
            let sent = Sent {
                text,
                pictures,
                files,
            };
            (
                Some(user_message(&ui.thumbs, key, id, sent, whole, kit)),
                ROW_PAD_BOTTOM,
            )
        }
        ItemBody::Reply {
            markdown,
            streaming,
        } => (
            Some(reply(key, id, markdown, *streaming, kit)),
            ROW_PAD_BOTTOM,
        ),
        ItemBody::Thinking { text, time, .. } => {
            let open = view.toggled.contains(&id);
            (
                Some(thinking(key, id, text, *time, open, kit)),
                LINE_PAD_BOTTOM,
            )
        }
        ItemBody::Tools(group) => (Some(tools(view, key, id, group, kit)), LINE_PAD_BOTTOM),
        ItemBody::Notice { kind, text } => {
            let restart = offers_restart(ui, key, item);
            (
                Some(notice(key, id, *kind, text, restart, kit)),
                ROW_PAD_BOTTOM,
            )
        }
        ItemBody::Request { .. } => (Some(requests::card(ui, key, item, kit)), ROW_PAD_BOTTOM),
    };
    // `toggle_path` names the chain of keys from the conversation down to a row's toggle: a
    // keyed element added between this row and its toggle, or above it, must go there too, or
    // `keep_on_screen` silently names nothing.
    let r = col()
        .key(row_key(id))
        .w_full()
        .px(ROW_PAD_X)
        .pb(pad_bottom)
        .children(content);
    // A row that opens and shuts glides like a disclosure, not at a stream's pace.
    if folds {
        r.fold_glide(kit.motion.disclosure)
    } else {
        r
    }
}

/// What a finished row draws from, for `rune::memo`. It never walks the item's body, so it
/// costs the same for a megabyte of output as for one line.
#[derive(Hash)]
struct RowVersion {
    /// The key number of the session the conversation belongs to: each session has its own
    /// ids and stamps.
    session: u64,
    /// The session's `generation`: its items were numbered again when it went up.
    generation: u64,
    /// The item's own change stamp: its body, as every kind of row draws it.
    changed: u64,
    /// The user opened or closed it against its kind's default (thinking, tool groups).
    toggled: bool,
    /// The user asked to see its long text whole (tool groups, the user's messages).
    whole: bool,
    /// It is the notice that offers Restart: read from the session's phase and the
    /// conversation's later items.
    restart: bool,
    /// A waiting request's drafts in its session's view: the options chosen and the
    /// "Something else" text with its caret and focus. `None` for every other item.
    draft: Option<Vec<QuestionDraft>>,
    /// The size of each picture of a user's message whose handle is made, in order: a
    /// picture's width follows it. Empty for every other item.
    pictures: Vec<Option<[u32; 2]>>,
}

/// The version of the row of `item`, of `key`'s session. A kind of row that reads anything else
/// from `Ui` or the session adds it here.
pub fn version(ui: &Ui, key: SessionKey, item: &Item) -> impl Hash + use<> {
    let view = glue::view_of(ui, key);
    RowVersion {
        session: key.0,
        generation: ui
            .app
            .sessions
            .get(key)
            .map_or(0, |entry| entry.session.generation),
        changed: item.changed,
        toggled: view.toggled.contains(&item.id),
        whole: view.whole.contains(&item.id),
        restart: offers_restart(ui, key, item),
        draft: waiting_draft(view, item),
        pictures: match &item.body {
            ItemBody::User { pictures, .. } => pictures
                .iter()
                .map(|picture| ui.thumbs.get(picture).and_then(Image::size))
                .collect(),
            _ => Vec::new(),
        },
    }
}

/// What the user has drafted for `item`, when it is a request still waiting.
fn waiting_draft(view: &SessionView, item: &Item) -> Option<Vec<QuestionDraft>> {
    match &item.body {
        ItemBody::Request {
            request,
            state: RequestState::Waiting,
        } => view.answers.get(&request.id).cloned(),
        _ => None,
    }
}

/// What a message of the user's was sent with.
struct Sent<'a> {
    text: &'a str,
    pictures: &'a [u64],
    files: &'a [PathBuf],
}

/// The user's message as typed, never read as Markdown: plain selectable text in a filled,
/// rounded box at the right of the column. A long message is cut as long tool output is, unless
/// `whole`, with [`show_all`] under it. A message that had images shows them over its text
/// ([`pictures`]), and one sent with files names them under it ([`files_line`]); one with no
/// text shows only those.
fn user_message(
    thumbs: &HashMap<u64, Image>,
    key: SessionKey,
    id: ItemId,
    sent: Sent<'_>,
    whole: bool,
    kit: &Kit,
) -> El {
    let Sent {
        text: message,
        pictures: shown_pictures,
        files,
    } = sent;
    let mut cut = None;
    let shown = clipped(message, whole, &mut cut);
    let words = text(shown.to_owned())
        .key(format!("mr-user-{}", id.0))
        .font(kit.ty.body)
        .color(kit.c.text_base)
        .wrap()
        .selectable(true);
    let more = cut.map(|lines| show_all(key, id, lines, kit));
    let bubble = if shown_pictures.is_empty() && files.is_empty() && more.is_none() {
        words.max_w(USER_MAX_WIDTH)
    } else {
        col()
            .max_w(USER_MAX_WIDTH)
            .gap(4.0)
            .children(pictures(thumbs, key, id, shown_pictures, kit))
            .children((!message.is_empty()).then_some(words))
            .children(more)
            .children(files_line(id, files, kit))
    };
    let bubble = bubble
        .px(14.0)
        .py(10.0)
        .radius(kit.radius.xl)
        .fill(kit.c.layer_02);
    rune::row().w_full().justify_end().child(bubble)
}

/// The pictures of the user's message `id`, one per picture id, in a row keyed
/// `mr-user-pictures-{id}` that is [`PICTURE_HEIGHT`] high whatever has come, scrolls sideways
/// and never wraps. A picture whose handle is made is drawn whole in a box its shape asks for
/// (`mr-user-picture-{picture}`), and a press on it opens it large (`glue::open_picture`); until
/// then, and for good when it never comes, a slot of the bubble's own colour, outlined, with the
/// image icon stands in its place (`mr-user-picture-wait-{picture}`). A press on the row
/// focuses `key`'s tile and leaves the keyboard where it was; a press on a picture hands the
/// keyboard to the viewer until it closes. Neither is a Tab stop. None for a message with no
/// image.
fn pictures(
    thumbs: &HashMap<u64, Image>,
    key: SessionKey,
    id: ItemId,
    pictures: &[u64],
    kit: &Kit,
) -> Option<El> {
    if pictures.is_empty() {
        return None;
    }
    let count = pictures.len();
    let slots = pictures.iter().enumerate().map(|(n, &picture)| {
        let label = format!("Image {} of {count}", n + 1);
        let made = thumbs
            .get(&picture)
            .and_then(|image| Some((image, image.size()?)));
        match made {
            // A press opens it large; the keyboard stays where it is.
            Some((image, [width, height])) => requests::unfocusable(
                el().key(format!("mr-user-picture-{picture}"))
                    .size(picture_width(width, height), PICTURE_HEIGHT)
                    .radius(PICTURE_RADIUS)
                    .pixels_fit(image.pixels(), SurfaceFit::Fit)
                    .role("button")
                    .label(format!("Open image {} of {count}", n + 1))
                    .on_press(move |ui: &mut Ui| glue::open_picture(ui, key, picture)),
            ),
            None => rune::row()
                .key(format!("mr-user-picture-wait-{picture}"))
                .size(PICTURE_HEIGHT, PICTURE_HEIGHT)
                .center()
                .radius(PICTURE_RADIUS)
                .border(1.0, kit.c.border_weak)
                .role("img")
                .label(format!("{label}, not loaded"))
                .child(icon(&PHOTO, PICTURE_ICON).color(kit.c.icon_muted)),
        }
    });
    let row = col()
        .key(format!("mr-user-pictures-{}", id.0))
        .w_full()
        .h(PICTURE_HEIGHT)
        .scroll_x()
        // A press focuses the tile, as on the rest of the message, and leaves the keyboard
        // where it is.
        .on_press(move |ui: &mut Ui| glue::focus_tile(ui, key, false))
        .child(
            rune::row()
                .h(PICTURE_HEIGHT)
                .gap(PICTURE_GAP)
                .children(slots),
        );
    // To a screen reader the row is nothing, and its pictures belong to the message: a group
    // with a press handler would be announced as a button.
    Some(requests::unfocusable(row).role("none"))
}

/// The width of a picture of `width` by `height` pixels drawn [`PICTURE_HEIGHT`] high, kept
/// between [`PICTURE_MIN_WIDTH`] and [`PICTURE_MAX_WIDTH`].
fn picture_width(width: u32, height: u32) -> f32 {
    (PICTURE_HEIGHT * width as f32 / height.max(1) as f32)
        .clamp(PICTURE_MIN_WIDTH, PICTURE_MAX_WIDTH)
}

/// The names of the files a message was sent with, the last part of each path, ", " between
/// them: one muted line keyed `mr-user-files-{id}`, whose tooltip has the whole paths, one a
/// line. None for a message sent with no file.
fn files_line(id: ItemId, files: &[PathBuf], kit: &Kit) -> Option<El> {
    let (first, rest) = files.split_first()?;
    let names: Vec<String> = files
        .iter()
        .map(|path| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        })
        .collect();
    let line = text(names.join(", "))
        .key(format!("mr-user-files-{}", id.0))
        .font(kit.ty.ui)
        .color(kit.c.text_muted)
        .wrap()
        .help(first.display().to_string());
    Some(
        rest.iter()
            .fold(line, |line, path| line.help_row(path.display().to_string())),
    )
}

/// Claude's reply: Markdown over the whole text so far, each finished code block with a copy
/// button, and once the reply has finished, a copy button for its source beneath it.
fn reply(key: SessionKey, id: ItemId, markdown: &str, streaming: bool, kit: &'static Kit) -> El {
    let body = Markdown::new(markdown)
        .streaming(streaming)
        .selectable(true)
        .code_frame(move |card, frame| code_block(key, id, card, frame, kit))
        .build(kit)
        .key(format!("mr-reply-{}", id.0))
        .w_full();
    if streaming {
        return body;
    }
    let copy = IconButton::new(kit.glyph.copy)
        .variant(IconButtonVariant::GhostMuted)
        .help("Copy")
        .on_press_tx(move |ui: &mut Ui, tx| {
            glue::focus_tile(ui, key, false);
            let item = find(ui, key, id);
            if let Some(ItemBody::Reply { markdown, .. }) = item.map(|item| &item.body) {
                tx.copy(markdown.clone());
            }
        })
        .build(kit)
        .key(format!("mr-copy-{}", id.0));
    col()
        .w_full()
        .gap(4.0)
        .child(body)
        .child(rune::row().w_full().child(copy))
}

/// A code block of a reply, with a button at its top right that copies its text once the
/// block has arrived whole.
fn code_block(key: SessionKey, id: ItemId, card: El, frame: &CodeFrame<'_>, kit: &Kit) -> El {
    let framed = col().w_full().child(card);
    if frame.open {
        return framed;
    }
    let source = frame.source.to_owned();
    let copy = IconButton::new(kit.glyph.copy)
        .variant(IconButtonVariant::GhostMuted)
        .help("Copy")
        .on_press_tx(move |ui: &mut Ui, tx| {
            glue::focus_tile(ui, key, false);
            tx.copy(source.clone());
        })
        .build(kit)
        .key(format!("mr-code-copy-{}-{}", id.0, frame.index));
    framed.child(
        rune::row()
            .pin(None, Some(4.0), Some(4.0), None)
            .child(copy),
    )
}

/// A thought: one muted line, with a spinner while it goes on. When it has text, the line
/// opens and closes it, beneath; when it has none, the line does not react.
fn thinking(
    key: SessionKey,
    id: ItemId,
    thought: &str,
    time: ThinkingTime,
    open: bool,
    kit: &Kit,
) -> El {
    let status =
        (time == ThinkingTime::Going).then(|| spinner(kit).key(format!("mr-running-{}", id.0)));
    let label = thinking_label(time);
    if thought.is_empty() {
        return line(key, id, &BRAIN, &label, status, None, None, kit);
    }
    let mut c = col()
        .w_full()
        .child(line(key, id, &BRAIN, &label, status, Some(open), None, kit));
    if open {
        c = c.child(
            col()
                .key(format!("mr-details-{}", id.0))
                .w_full()
                .pl(DETAILS_INSET)
                .pb(8.0)
                .child(
                    text(thought.to_owned())
                        .font(kit.ty.ui)
                        .color(kit.c.text_muted)
                        .wrap()
                        .selectable(true)
                        .w_full(),
                ),
        );
    }
    c
}

/// A tool group: one line with its family's icon and summary, a spinner while a call runs or
/// a warning mark when one failed. The line opens and closes the details beneath; an edit
/// starts open once one of its calls has an outcome, and every other family closed. Before
/// that an edit may be waiting for permission, and its card shows the diff. Closed details
/// are not built.
fn tools(view: &SessionView, key: SessionKey, id: ItemId, group: &ToolGroup, kit: &Kit) -> El {
    let open = starts_open(group) != view.toggled.contains(&id);
    let status = if group.running() {
        Some(spinner(kit).key(format!("mr-running-{}", id.0)))
    } else if group.failed() {
        Some(
            icon(&WARNING, LINE_ICON)
                .key(format!("mr-failed-{}", id.0))
                .color(kit.c.danger.fg)
                .role("img")
                .label("Failed"),
        )
    } else {
        None
    };
    let summary = group.summary();
    let icon = family_icon(group.family);
    if !open {
        return line(key, id, icon, &summary, status, Some(false), None, kit);
    }
    let whole = view.whole.contains(&id);
    let mut hidden: Option<usize> = None;
    let mut details = col()
        .key(format!("mr-details-{}", id.0))
        .w_full()
        .pl(DETAILS_INSET)
        .pt(2.0)
        .pb(10.0)
        .gap(10.0);
    for (n, call) in group.calls.iter().enumerate() {
        let shown = call_details(kit, &call.detail, call.outcome.as_ref(), whole);
        if let Some(lines) = shown.cut {
            hidden = Some(hidden.unwrap_or(0) + lines);
        }
        details = details.child(shown.el.key(format!("mr-call-{n}")));
    }
    // On the line, not under the output: the line is on screen the moment the details start
    // to open, and stays put while they glide open beneath it.
    let action = hidden.map(|lines| show_all(key, id, lines, kit));
    col()
        .w_full()
        .child(line(
            key,
            id,
            icon,
            &summary,
            status,
            Some(true),
            action,
            kit,
        ))
        .child(details)
}

/// Whether a tool group's details start open: an edit's once it is done, so a diff that waits
/// for permission is shown once, on the request's card.
fn starts_open(group: &ToolGroup) -> bool {
    group.family == ToolFamily::Edit && group.calls.iter().any(|call| call.outcome.is_some())
}

/// One tool call drawn by [`call_details`]: the element, and when part of its text was left
/// out, how many lines were not shown at all.
pub struct CallDetails {
    pub el: El,
    /// `Some` when text was cut, with the number of lines left out whole (0 when the cut fell
    /// inside a line).
    pub cut: Option<usize>,
}

/// What one tool call does and what came of it, as a tool group's details show it and a
/// permission card's body can (with no outcome):
///
/// - Read: the path.
/// - Search: the query, then the output.
/// - Command: the whole command, then the output.
/// - Edit (`FileEdit`): the file name, `+added −removed`, then the diff.
/// - Edit (`FileWrite`): the file name, `+lines`, then the content.
/// - Other: the input JSON, then the output.
///
/// A read or an edit shows its output only when it failed; a failed call's output is in the
/// danger colour. Output, content and diffs are in the monospace font and cut to
/// [`OUTPUT_LINES`] lines and [`OUTPUT_CHARS`] characters unless `whole`; the caller offers
/// [`show_all`] when anything was cut. Only the shown part is laid out.
pub fn call_details(
    kit: &Kit,
    detail: &ToolDetail,
    outcome: Option<&ToolOutcome>,
    whole: bool,
) -> CallDetails {
    let mut cut = None;
    let mut c = col().w_full().gap(6.0);
    let failed = matches!(outcome, Some(ToolOutcome::Failed { .. }));
    let output_shown = match detail {
        ToolDetail::FileRead { path } => {
            c = c.child(mono(path, kit.c.text_muted, kit));
            failed
        }
        ToolDetail::Search { query } => {
            c = c.child(mono(query, kit.c.text_base, kit));
            true
        }
        ToolDetail::Command { command } => {
            c = c.child(mono(command, kit.c.text_base, kit).selectable(true));
            true
        }
        ToolDetail::FileEdit {
            path,
            old_text,
            new_text,
        } => {
            let (added, removed) = line_counts(old_text, new_text);
            c = c.child(file_head(path, added, Some(removed), kit));
            let diff = unified_diff(old_text, new_text);
            let shown = clipped(&diff, whole, &mut cut);
            if !shown.is_empty() {
                c = c.child(
                    Markdown::new(fenced("diff", shown))
                        .selectable(true)
                        .build(kit)
                        .key("mr-diff")
                        .w_full(),
                );
            }
            failed
        }
        ToolDetail::FileWrite { path, content } => {
            c = c.child(file_head(path, content.lines().count(), None, kit));
            let shown = clipped(content, whole, &mut cut);
            if !shown.is_empty() {
                c = c.child(output_box(shown, kit.c.text_base, kit));
            }
            failed
        }
        ToolDetail::Other { input_json } => {
            let shown = clipped(input_json, whole, &mut cut);
            c = c.child(output_box(shown, kit.c.text_muted, kit));
            true
        }
    };
    let output = match outcome {
        Some(ToolOutcome::Ok { output } | ToolOutcome::Failed { output }) if output_shown => {
            Some(output.as_str())
        }
        _ => None,
    };
    if let Some(output) = output.filter(|o| !o.trim().is_empty()) {
        let ink = if failed {
            kit.c.danger.fg
        } else {
            kit.c.text_base
        };
        let shown = clipped(output, whole, &mut cut);
        c = c.child(output_box(shown, ink, kit).key("mr-output"));
    }
    CallDetails { el: c, cut }
}

/// "Show all (N more lines)", "Show all (1 more line)" for one, or "Show all" when no line
/// was left out whole, keyed `mr-show-all-{id}`: pressing it focuses the tile and adds `id` to
/// the `whole` of `key`'s view, so the row shows its text whole. A tool group puts it at the
/// right end of its line, a message of the user's under its text.
pub fn show_all(key: SessionKey, id: ItemId, hidden_lines: usize, kit: &Kit) -> El {
    let label = match hidden_lines {
        0 => "Show all".to_owned(),
        1 => "Show all (1 more line)".to_owned(),
        _ => format!("Show all ({hidden_lines} more lines)"),
    };
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .on_press(move |ui: &mut Ui| {
            glue::focus_tile(ui, key, false);
            glue::view_mut(ui, key).whole.insert(id);
        })
        .build(kit)
        .key(format!("mr-show-all-{}", id.0))
}

/// A notice: muted when the user stopped the turn, in the danger colour when a turn failed or
/// the session ended, in the warning colour when a command was refused. The two notices of a
/// stored history (earlier messages left out, or summarised) are muted and centred: they mark a
/// place in the conversation rather than say what happened in it. The notice that says why the
/// session ended offers Restart while it stays ended.
fn notice(
    key: SessionKey,
    id: ItemId,
    kind: NoticeKind,
    words: &str,
    restart: bool,
    kit: &Kit,
) -> El {
    let ink = match kind {
        NoticeKind::Stopped | NoticeKind::EarlierLeftOut | NoticeKind::Compacted => {
            kit.c.text_muted
        }
        NoticeKind::TurnFailed | NoticeKind::Ended => kit.c.danger.fg,
        NoticeKind::CommandFailed => kit.c.warning.fg,
    };
    let words = text(words.to_owned())
        .key(format!("mr-notice-{}", id.0))
        .font(kit.ty.ui)
        .color(ink)
        .wrap()
        .selectable(true);
    if matches!(kind, NoticeKind::EarlierLeftOut | NoticeKind::Compacted) {
        return rune::row()
            .w_full()
            .justify_center()
            .child(words.align(Align::Center).shrink().min_w(0.0));
    }
    let mut c = col().w_full().gap(8.0).child(words.w_full());
    if restart {
        c = c.child(
            rune::row().child(
                Button::new("Restart")
                    .on_press(move |ui: &mut Ui| {
                        let effects = ui.app.restart(key);
                        glue::run(ui, effects);
                    })
                    .build(kit)
                    .key("mr-restart"),
            ),
        );
    }
    c
}

/// Whether `item` is the notice that offers Restart: the last `Ended` notice, while the phase of
/// `key`'s session is `Ended`. Only the items after it are looked at.
fn offers_restart(ui: &Ui, key: SessionKey, item: &Item) -> bool {
    let is_ended = |item: &Item| {
        matches!(
            item.body,
            ItemBody::Notice {
                kind: NoticeKind::Ended,
                ..
            }
        )
    };
    if !is_ended(item) {
        return false;
    }
    let Some(session) = ui.app.sessions.get(key).map(|entry| &entry.session) else {
        return false;
    };
    if !matches!(session.phase, Phase::Ended { .. }) {
        return false;
    }
    let items = session.conversation.items();
    match items.binary_search_by_key(&item.id.0, |i| i.id.0) {
        Ok(at) => !items[at + 1..].iter().any(is_ended),
        Err(_) => false,
    }
}

/// Item `id` of `key`'s session. Ids grow in the order items are pushed.
pub(super) fn find(ui: &Ui, key: SessionKey, id: ItemId) -> Option<&Item> {
    let items = ui.app.sessions.get(key)?.session.conversation.items();
    let at = items.binary_search_by_key(&id.0, |i| i.id.0).ok()?;
    items.get(at)
}

/// A thinking or tool line of item `id` of `key`'s session: icon, label, status and, at its
/// right end, `action`. With `toggle`, the line is keyed `mr-toggle-{id}`, shows whether it is
/// open and flips the item in the `toggled` of the session's view, holding the line where it is
/// on screen while its details open or shut beneath; without, it is keyed `mr-line-{id}` and
/// does not react.
#[expect(clippy::too_many_arguments, reason = "one line's place and look")]
fn line(
    key: SessionKey,
    id: ItemId,
    glyph: &'static Icon,
    label: &str,
    status: Option<El>,
    toggle: Option<bool>,
    action: Option<El>,
    kit: &Kit,
) -> El {
    let mut r = rune::row()
        .w_full()
        .h(LINE_HEIGHT)
        .gap(8.0)
        .center_y()
        .child(icon(glyph, LINE_ICON).color(kit.c.icon_muted).a11y_hidden())
        .child(
            text(label.to_owned())
                .font(kit.ty.ui)
                .color(kit.c.text_muted)
                .truncate()
                .shrink(),
        )
        .children(status);
    if let Some(open) = toggle {
        let arrow = if open {
            kit.glyph.triangle_down
        } else {
            kit.glyph.triangle_right
        };
        r = r.child(
            icon(arrow, LINE_ICON)
                .color(kit.c.icon_weaker)
                .a11y_hidden(),
        );
    }
    if let Some(action) = action {
        r = r.child(el().grow(1.0)).child(action);
    }
    match toggle {
        None => r.key(format!("mr-line-{}", id.0)),
        Some(open) => {
            // Named by `toggle_path`, through every keyed ancestor up to the conversation.
            r = r
                .key(toggle_key(id))
                .role("button")
                .label(label.to_owned())
                .expanded(open)
                .cursor(Cursor::Pointer)
                .radius(kit.radius.md)
                .on_press_tx(move |ui: &mut Ui, tx| {
                    glue::focus_tile(ui, key, false);
                    let toggled = &mut glue::view_mut(ui, key).toggled;
                    if !toggled.remove(&id) {
                        toggled.insert(id);
                    }
                    tx.keep_on_screen(toggle_path(key, id));
                });
            kit.interactive(r)
        }
    }
}

fn spinner(kit: &Kit) -> El {
    Spinner::new()
        .size(LINE_ICON)
        .color(kit.c.icon_muted)
        .build(kit)
}

fn family_icon(family: ToolFamily) -> &'static Icon {
    match family {
        ToolFamily::Read => &FILE_DOCUMENT,
        ToolFamily::Search => &MAGNIFYING_GLASS,
        ToolFamily::Command => &TERMINAL,
        ToolFamily::Edit => &PENCIL_LINE,
        ToolFamily::Other => &CUBE,
    }
}

/// An edited or written file's name, then its counts: `+added −removed`, or `+lines` for a
/// file written whole, in the success and danger colours.
fn file_head(path: &str, added: usize, removed: Option<usize>, kit: &Kit) -> El {
    let plus = format!("+{added}");
    let counts = match removed {
        Some(removed) => format!("{plus} −{removed}"),
        None => plus.clone(),
    };
    let mut spans = vec![Span::new(0, plus.len()).color(kit.c.success.fg)];
    if counts.len() > plus.len() {
        spans.push(Span::new(plus.len() + 1, counts.len()).color(kit.c.danger.fg));
    }
    rune::row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            text(file_name(path).to_owned())
                .font(kit.ty.ui_medium)
                .color(kit.c.text_base)
                .truncate()
                .shrink(),
        )
        .child(rich(counts, spans).font(kit.ty.ui).key("mr-counts"))
}

/// `text` cut to what long tool output and a long message of the user's show, unless `whole`;
/// adds to `cut` what was left out.
fn clipped<'a>(text: &'a str, whole: bool, cut: &mut Option<usize>) -> &'a str {
    if whole {
        return text;
    }
    let c = clip(text, OUTPUT_LINES, OUTPUT_CHARS);
    if c.cut {
        *cut = Some(cut.unwrap_or(0) + c.hidden_lines);
    }
    c.shown
}

/// One line of monospace text, wrapped.
fn mono(words: &str, ink: Color, kit: &Kit) -> El {
    text(words.to_owned())
        .font(kit.ty.mono)
        .color(ink)
        .wrap()
        .w_full()
}

/// Tool output or file content: monospace, selectable, in a filled box. Trailing line breaks
/// are left out, so the box ends at its last line.
fn output_box(output: &str, ink: Color, kit: &Kit) -> El {
    col()
        .w_full()
        .px(10.0)
        .py(8.0)
        .radius(kit.radius.md)
        .fill(kit.c.layer_01)
        .border(0.5, kit.c.border_muted)
        .child(mono(output.trim_end_matches('\n'), ink, kit).selectable(true))
}

#[cfg(test)]
mod tests {
    use mr_agent::{
        HistoryItem, SessionEvent, SessionId, StoredHistory, StoredSession, ToolCallId, ToolDetail,
        ToolOutcome,
    };
    use std::path::PathBuf;
    use std::sync::Arc;

    use mr_agent::ImageData;
    use mr_core::{ChipState, ItemBody, ItemId, Phase, Session, SessionKey, Source};
    use rune::{ElementInfo, ImageCache, Runtime};

    use crate::demo::{DAY, NOW_UNIX};
    use crate::glue::{self, Ui};
    use crate::testkit;
    use crate::theme;

    /// The `mr-toggle-` key of the line that reads `summary`.
    fn toggle_key(rt: &Runtime<Ui>, summary: &str) -> String {
        rt.elements()
            .into_iter()
            .filter(|e| e.label.as_deref() == Some(summary))
            .filter_map(|e| e.key)
            .find(|k| k.starts_with("mr-toggle-"))
            .unwrap_or_else(|| panic!("no line toggles {summary:?}"))
    }

    #[test]
    fn tool_calls_are_single_lines_until_opened() {
        let mut rt = testkit::window("tools");
        for part in [
            "Thought for 4 s",
            "Read 3 files",
            "Searched for TODO",
            "Ran cargo test",
            "Edited main.rs",
            "Read missing.rs",
        ] {
            assert!(testkit::has_text(&rt, part), "{part}");
        }
        assert!(
            !testkit::has_text(&rt, "line 3"),
            "command output starts closed"
        );
        let command = toggle_key(&rt, "Ran cargo test");
        testkit::click(&mut rt, &command);
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "line 3"));
        assert!(!testkit::has_text(&rt, "line 400"));
        assert!(testkit::has_text(&rt, "Show all (480 more lines)"));
    }

    #[test]
    fn an_edit_shows_its_counts_and_its_diff_without_being_opened() {
        let rt = testkit::window("tools");
        assert!(testkit::has_text(&rt, "+3 −2"));
    }

    #[test]
    fn an_edit_in_two_places_shows_both_parted_by_one_line() {
        let mut rt = testkit::window("empty");
        let old: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let new = old
            .replace("line 2\n", "line two\n")
            .replace("line 18\n", "line eighteen\n");
        let id = ToolCallId("e9".into());
        testkit::emit(
            &mut rt,
            SessionEvent::ToolStarted {
                id: id.clone(),
                name: "Edit".into(),
                detail: ToolDetail::FileEdit {
                    path: "/home/alex/project/notes.txt".into(),
                    old_text: old,
                    new_text: new,
                },
            },
        );
        testkit::emit(
            &mut rt,
            SessionEvent::ToolFinished {
                id,
                outcome: ToolOutcome::Ok {
                    output: "The file was updated.".into(),
                },
            },
        );
        // The edit's details glide open; let them land.
        rt.settle_all();
        testkit::step(&mut rt);
        let texts = testkit::texts(&rt);
        for line in [
            "-line 2",
            "+line two",
            "\u{22ef}",
            "-line 18",
            "+line eighteen",
        ] {
            assert!(texts.iter().any(|t| t == line), "{line:?} in {texts:?}");
        }
        assert!(!texts.iter().any(|t| t.starts_with("@@")), "{texts:?}");
    }

    #[test]
    fn a_new_session_starts_with_every_row_as_its_kind_starts() {
        let mut rt = testkit::window("tools");
        let command = toggle_key(&rt, "Ran cargo test");
        let edit = toggle_key(&rt, "Edited main.rs");
        testkit::click(&mut rt, &command);
        testkit::step(&mut rt);
        let id = command.trim_start_matches("mr-toggle-").to_owned();
        testkit::click(&mut rt, &format!("mr-show-all-{id}"));
        testkit::click(&mut rt, &edit);
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "line 500"), "shown whole");
        assert!(!testkit::has_text(&rt, "+3 −2"), "the edit was closed");
        let _folder = testkit::new_session_elsewhere(&mut rt);
        assert_eq!(testkit::shown_open(&rt), Some(2));
        // The same turn again: its items get the same ids in the new conversation.
        crate::demo::tools_turn(&mut rt.state);
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(toggle_key(&rt, "Ran cargo test"), command);
        assert!(
            !testkit::has_text(&rt, "line 3"),
            "the command starts closed"
        );
        assert!(testkit::has_text(&rt, "+3 −2"), "the edit starts open");
        assert!(
            testkit::shown_view(&rt).toggled.is_empty()
                && testkit::shown_view(&rt).whole.is_empty()
        );
    }

    #[test]
    fn an_edit_waiting_for_permission_shows_its_diff_once_on_the_card() {
        let mut rt = testkit::window("permission-edit");
        rt.settle_all();
        testkit::step(&mut rt);
        let line = toggle_key(&rt, "Edited main.rs");
        let id = line.trim_start_matches("mr-toggle-").to_owned();
        assert!(rt.find(&format!("mr-running-{id}")).is_some());
        assert!(
            rt.find(&format!("mr-details-{id}")).is_none(),
            "the line starts closed"
        );
        let added = |rt: &Runtime<Ui>| {
            testkit::texts(rt)
                .iter()
                .filter(|t| t.as_str() == "+    run(n)?;")
                .count()
        };
        assert_eq!(added(&rt), 1, "the diff shows once, on the card");
        testkit::click(&mut rt, "mr-allow-e1");
        testkit::emit(
            &mut rt,
            SessionEvent::ToolFinished {
                id: ToolCallId("call-e1".into()),
                outcome: ToolOutcome::Ok {
                    output: "The file was updated.".into(),
                },
            },
        );
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(
            rt.find(&format!("mr-details-{id}")).is_some(),
            "done, the edit opens"
        );
        assert_eq!(
            added(&rt),
            1,
            "the settled card is one line; the edit shows the diff"
        );
    }

    #[test]
    fn a_running_tool_and_a_running_thought_say_so() {
        let rt = testkit::window("working");
        assert!(testkit::has_text(&rt, "Thinking…"));
        assert!(testkit::has_text(&rt, "Ran sleep 30"));
    }

    #[test]
    fn a_dead_session_offers_a_restart() {
        let mut rt = testkit::window("ended");
        assert!(testkit::has_text(&rt, "claude exited with status 1"));
        testkit::click(&mut rt, "mr-restart");
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_open(&rt), Some(2));
        assert_eq!(rt.state.focused_session().unwrap().phase, Phase::Idle);
        assert!(rt.find("mr-restart").is_none());
    }

    #[test]
    fn a_reply_can_be_copied_as_its_source() {
        let mut rt = testkit::window("conversation");
        let key = rt
            .elements()
            .into_iter()
            .filter_map(|e| e.key)
            .find(|k| k.starts_with("mr-copy-"))
            .unwrap();
        testkit::click(&mut rt, &key);
        assert!(testkit::clipboard(&rt).unwrap().contains("```rust"));
    }

    /// Emits a finished `Bash` call running `command` whose output is `output`.
    fn command(rt: &mut Runtime<Ui>, id: &str, command: &str, output: String) {
        testkit::emit(
            rt,
            SessionEvent::ToolStarted {
                id: ToolCallId(id.into()),
                name: "Bash".into(),
                detail: ToolDetail::Command {
                    command: command.into(),
                },
            },
        );
        testkit::emit(
            rt,
            SessionEvent::ToolFinished {
                id: ToolCallId(id.into()),
                outcome: ToolOutcome::Ok { output },
            },
        );
    }

    #[test]
    fn show_all_shows_the_whole_output_and_keeps_it_open() {
        let mut rt = testkit::window("tools");
        let command = toggle_key(&rt, "Ran cargo test");
        testkit::click(&mut rt, &command);
        testkit::step(&mut rt);
        let id = command.trim_start_matches("mr-toggle-");
        testkit::click(&mut rt, &format!("mr-show-all-{id}"));
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "line 500"));
        assert!(!testkit::has_text(&rt, "Show all"));
        assert!(rt.find(&format!("mr-details-{id}")).is_some(), "still open");
    }

    #[test]
    fn huge_output_opens_clipped_and_a_megabyte_can_be_shown_whole() {
        let mut rt = testkit::window("empty");
        let many: String = (0..50_000).map(|n| format!("row {n}\n")).collect();
        command(&mut rt, "c1", "seq 50000", many);
        command(&mut rt, "c2", "head -c 1000000", "x".repeat(1_000_000));
        for (summary, offer) in [
            ("Ran seq 50000", "Show all (49980 more lines)"),
            ("Ran head -c 1000000", "Show all"),
        ] {
            let key = toggle_key(&rt, summary);
            testkit::click(&mut rt, &key);
            testkit::step(&mut rt);
            assert!(testkit::has_text(&rt, offer), "{summary}");
        }
        // Slow for a megabyte, and accepted; it must not panic.
        let id = toggle_key(&rt, "Ran head -c 1000000").replace("mr-toggle-", "");
        testkit::click(&mut rt, &format!("mr-show-all-{id}"));
        testkit::step(&mut rt);
        assert!(rt.find(&format!("mr-show-all-{id}")).is_none());
        assert!(testkit::texts(&rt).iter().any(|t| t.len() == 1_000_000));
    }

    #[test]
    fn a_thought_with_text_opens_and_one_without_does_not() {
        let mut rt = testkit::window("tools");
        let quiet = rt
            .elements()
            .into_iter()
            .filter_map(|e| e.key)
            .filter(|k| k.starts_with("mr-line-"))
            .count();
        assert_eq!(quiet, 1, "the 4 s thought has no text and does not toggle");
        testkit::emit(&mut rt, SessionEvent::ThinkingStarted);
        testkit::emit(
            &mut rt,
            SessionEvent::ThinkingDelta("Maybe the parser is at fault".into()),
        );
        assert!(testkit::has_text(&rt, "Thinking…"));
        assert!(!testkit::has_text(&rt, "Maybe the parser"));
        let key = toggle_key(&rt, "Thinking…");
        testkit::click(&mut rt, &key);
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "Maybe the parser is at fault"));
    }

    #[test]
    fn a_code_block_can_be_copied_as_its_text() {
        let mut rt = testkit::window("conversation");
        let key = rt
            .elements()
            .into_iter()
            .filter_map(|e| e.key)
            .find(|k| k.starts_with("mr-code-copy-"))
            .unwrap();
        testkit::click(&mut rt, &key);
        let copied = testkit::clipboard(&rt).unwrap();
        assert!(copied.starts_with("fn main() {"), "{copied}");
        assert!(!copied.contains("```"));
    }

    #[test]
    fn a_command_cut_off_by_the_end_of_the_session_stops_spinning() {
        let mut rt = testkit::window("working");
        let key = toggle_key(&rt, "Ran sleep 30");
        let id = key.replace("mr-toggle-", "");
        assert!(rt.find(&format!("mr-running-{id}")).is_some());
        testkit::emit(
            &mut rt,
            SessionEvent::Exited {
                clean: false,
                detail: "claude exited with status 1".into(),
            },
        );
        assert!(testkit::has_text(&rt, "Ran sleep 30"));
        assert!(rt.find(&format!("mr-running-{id}")).is_none());
        assert!(rt.find(&format!("mr-failed-{id}")).is_some());
        testkit::click(&mut rt, &key);
        // The details glide open; let them land so the check does not hang on timing.
        rt.settle_all();
        testkit::step(&mut rt);
        assert!(testkit::has_text(&rt, "Did not finish"));
    }

    /// "line 1" to "line {count}", one per line.
    fn numbered_lines(count: usize) -> String {
        (1..=count)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Sends `message` as the user from the focused tile and lets its row land; returns the
    /// tile's session and the message's item.
    fn say(rt: &mut Runtime<Ui>, message: &str) -> (SessionKey, ItemId) {
        let key = testkit::focused(rt).expect("a session is shown");
        rt.update(|ui, _tx| {
            let effects = ui.app.submit(key, message);
            glue::run(ui, effects);
        });
        // The row grows in; it is read, and pressed, once it is whole.
        rt.settle_all();
        testkit::step(rt);
        (key, last_message(rt))
    }

    /// The last message of the user's in the focused session.
    fn last_message(rt: &Runtime<Ui>) -> ItemId {
        testkit::shown_session_items(rt)
            .iter()
            .rev()
            .find(|item| matches!(item.body, ItemBody::User { .. }))
            .expect("the user said something")
            .id
    }

    /// The text that the message `id` in `key`'s tile shows.
    fn shown_message(rt: &Runtime<Ui>, key: SessionKey, id: ItemId) -> String {
        testkit::element(rt, &testkit::tile(key, &format!("mr-user-{}", id.0)))
            .and_then(|e| e.text)
            .expect("the message is drawn")
    }

    #[test]
    fn a_long_message_is_cut_at_twenty_lines() {
        let mut rt = testkit::window("empty");
        let (key, id) = say(&mut rt, &numbered_lines(500));
        assert!(testkit::has_text(&rt, "Show all (480 more lines)"));
        let shown = shown_message(&rt, key, id);
        assert!(shown.ends_with("line 20"), "{shown:?}");
        assert!(!testkit::has_text(&rt, "line 21"));
    }

    #[test]
    fn one_line_more_is_said_in_the_singular() {
        let offers = |rt: &Runtime<Ui>| -> Vec<String> {
            let texts = testkit::texts(rt);
            texts
                .into_iter()
                .filter(|t| t.starts_with("Show all"))
                .collect()
        };
        let mut rt = testkit::window("empty");
        say(&mut rt, &numbered_lines(21));
        assert_eq!(offers(&rt), ["Show all (1 more line)"]);
        // And for a tool's output.
        command(&mut rt, "c1", "seq 21", numbered_lines(21));
        let toggle = toggle_key(&rt, "Ran seq 21");
        testkit::click(&mut rt, &toggle);
        testkit::step(&mut rt);
        assert_eq!(offers(&rt), ["Show all (1 more line)"; 2]);
        // Two lines or more read as before.
        let mut rt = testkit::window("empty");
        say(&mut rt, &numbered_lines(22));
        assert_eq!(offers(&rt), ["Show all (2 more lines)"]);
    }

    #[test]
    fn a_message_of_one_very_long_line_is_cut_at_4000_characters() {
        let mut rt = testkit::window("empty");
        let (key, id) = say(&mut rt, &"alex ".repeat(5_200));
        assert_eq!(shown_message(&rt, key, id).chars().count(), 4_000);
        assert!(testkit::texts(&rt).iter().any(|t| t == "Show all"));
    }

    #[test]
    fn show_all_shows_the_whole_message() {
        let mut rt = testkit::window("empty");
        let (key, id) = say(&mut rt, &numbered_lines(500));
        testkit::click(&mut rt, &format!("mr-show-all-{}", id.0));
        testkit::step(&mut rt);
        assert_eq!(shown_message(&rt, key, id), numbered_lines(500));
        assert!(!testkit::has_text(&rt, "Show all"));
        assert!(glue::view_of(&rt.state, key).whole.contains(&id));
    }

    #[test]
    fn a_short_message_has_no_button() {
        let mut rt = testkit::window("empty");
        let (key, id) = say(&mut rt, "Hello from alex");
        assert_eq!(shown_message(&rt, key, id), "Hello from alex");
        assert!(!testkit::drawn(&rt, &format!("mr-show-all-{}", id.0)));
    }

    #[test]
    fn a_cut_never_splits_a_character() {
        let mut rt = testkit::window("empty");
        let message = format!("{}é{}", "a".repeat(3_999), "b".repeat(100));
        let (key, id) = say(&mut rt, &message);
        let shown = shown_message(&rt, key, id);
        assert_eq!(shown.chars().count(), 4_000);
        assert_eq!(shown.chars().last(), Some('é'));
    }

    #[test]
    fn a_stored_long_message_is_cut_too() {
        let mut rt = testkit::window("resting");
        let id = SessionId("stored-long-message".into());
        let history = StoredHistory {
            items: vec![
                HistoryItem::User {
                    text: numbered_lines(500),
                    image_count: 2,
                    files: Vec::new(),
                    images: vec![None; 2],
                    stored: None,
                },
                HistoryItem::AssistantText("Read them all.".into()),
            ],
            ..StoredHistory::default()
        };
        rt.state
            .fake
            .as_ref()
            .expect("the demo window has its fake")
            .set_history(&id, history);
        let stored = StoredSession {
            id,
            folder: "/home/alex/notes".into(),
            title: "Notes".into(),
            last_active_unix: NOW_UNIX - DAY,
        };
        let key = rt
            .state
            .app
            .sessions
            .add(Session::resting(&stored), stored.last_active_unix);
        glue::run(&mut rt.state, Vec::new());
        testkit::show(&mut rt, key);
        let id = last_message(&rt);
        assert!(testkit::has_text(&rt, "Show all (480 more lines)"));
        assert!(shown_message(&rt, key, id).ends_with("line 20"));
        assert!(!testkit::has_text(&rt, "line 21"));
        let below = |inner: &str| testkit::rect(&rt, &testkit::tile(key, inner)).unwrap()[1];
        let button = below(&format!("mr-show-all-{}", id.0));
        let pictures = below(&format!("mr-user-pictures-{}", id.0));
        assert!(
            pictures < below(&format!("mr-user-{}", id.0))
                && below(&format!("mr-user-{}", id.0)) < button,
            "the pictures, the text, then the button"
        );
    }

    /// Attaches `sources` to the focused session, sends them with `message` once they are
    /// read, and lets the message's row land; returns the session and the message's item.
    fn say_with(rt: &mut Runtime<Ui>, message: &str, sources: Vec<Source>) -> (SessionKey, ItemId) {
        let key = testkit::focused(rt).expect("a session is shown");
        rt.update(|ui, tx| glue::attach(ui, tx, key, sources));
        for _ in 0..20 {
            let chips = &rt.state.app.sessions.get(key).unwrap().session.chips;
            if chips.iter().all(|chip| chip.state != ChipState::Reading) {
                break;
            }
            testkit::step(rt);
        }
        say(rt, message)
    }

    #[test]
    fn a_sent_message_names_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = ["notes.md", "plan.txt"]
            .into_iter()
            .map(|name| {
                let path = dir.path().join(name);
                std::fs::write(&path, "a line\n").unwrap();
                path
            })
            .collect();
        let mut rt = testkit::window("empty");
        let sources = paths.iter().cloned().map(Source::Path).collect();
        let (key, id) = say_with(&mut rt, "Read these", sources);
        let files = testkit::element(&rt, &testkit::tile(key, &format!("mr-user-files-{}", id.0)))
            .expect("the files line");
        assert_eq!(files.text.as_deref(), Some("notes.md, plan.txt"));
        let tooltip = format!("{}\n{}", paths[0].display(), paths[1].display());
        assert_eq!(files.help.as_deref(), Some(tooltip.as_str()));
        assert_eq!(files.text_color, Some(theme::kit(true).c.text_muted));
        assert_eq!(shown_message(&rt, key, id), "Read these");
        let text = testkit::rect(&rt, &testkit::tile(key, &format!("mr-user-{}", id.0))).unwrap();
        assert!(text[1] < files.rect[1], "under the text");
        assert!(!testkit::drawn(&rt, &format!("mr-user-pictures-{}", id.0)));
    }

    /// Pastes an image of each of `shapes` (width by height) into the focused session, with a
    /// cache to make their handles in, sends them with `message`, and waits for the handles to
    /// be made; returns the session and the message's item.
    fn say_pictures(
        rt: &mut Runtime<Ui>,
        message: &str,
        shapes: &[(u32, u32)],
    ) -> (SessionKey, ItemId) {
        rt.state.images = Some(ImageCache::new(Arc::new(|| {})));
        let sources = shapes
            .iter()
            .map(|&(width, height)| Source::Pasted(ImageData::new(testkit::png(width, height))))
            .collect();
        let (key, id) = say_with(rt, message, sources);
        for picture in pictures_of(rt, id) {
            let handle = rt.state.thumbs.get(&picture).expect("a handle").clone();
            handle.wait().expect("a picture made in the test is made");
        }
        testkit::step(rt);
        (key, id)
    }

    /// The picture ids of the user's message `id` in the focused session.
    fn pictures_of(rt: &Runtime<Ui>, id: ItemId) -> Vec<u64> {
        testkit::shown_session_items(rt)
            .iter()
            .find_map(|item| match &item.body {
                ItemBody::User { pictures, .. } if item.id == id => Some(pictures.clone()),
                _ => None,
            })
            .expect("the user's message")
    }

    /// Whether `inner` lies within `outer`, both `[x, y, w, h]`.
    fn within(inner: [f32; 4], outer: [f32; 4]) -> bool {
        inner[0] >= outer[0]
            && inner[1] >= outer[1]
            && inner[0] + inner[2] <= outer[0] + outer[2]
            && inner[1] + inner[3] <= outer[1] + outer[3]
    }

    /// The element whose child `child` is.
    fn parent_of(rt: &Runtime<Ui>, child: &ElementInfo) -> ElementInfo {
        let index = child.parent.expect("a parent");
        rt.elements().swap_remove(index)
    }

    /// The row of the conversation that holds `inner`.
    fn row_of(rt: &Runtime<Ui>, inner: &ElementInfo) -> ElementInfo {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting)
            .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("mr-item-")))
            .find(|e| inner.path.starts_with(&format!("{}/", e.path)))
            .expect("in a row")
    }

    #[test]
    fn a_sent_message_shows_its_pictures() {
        let mut rt = testkit::window("empty");
        let (key, id) = say_pictures(&mut rt, "Which one?", &[(4, 3), (1, 8), (8, 1)]);
        let ids = pictures_of(&rt, id);
        assert_eq!(ids.len(), 3);
        let row = line(&rt, key, format!("mr-user-pictures-{}", id.0)).expect("the row");
        assert_eq!(row.rect[3], 120.0);
        let mut widths = Vec::new();
        for (n, picture) in ids.iter().enumerate() {
            let shown = line(&rt, key, format!("mr-user-picture-{picture}")).expect("drawn");
            assert_eq!(shown.rect[3], 120.0);
            assert!(
                within(shown.rect, row.rect),
                "{:?} in {:?}",
                shown.rect,
                row.rect
            );
            assert_eq!(shown.label, Some(format!("Open image {} of 3", n + 1)));
            assert_eq!(shown.radius, 6.0);
            assert_eq!(shown.fill.a, 0.0, "the bands are the bubble's own colour");
            assert!(!shown.focusable);
            widths.push(shown.rect[2]);
        }
        assert_eq!(widths, [160.0, 60.0, 240.0], "the shape, within 60 and 240");
        let text = line(&rt, key, format!("mr-user-{}", id.0)).expect("the text");
        assert!(
            row.rect[1] + row.rect[3] <= text.rect[1],
            "the pictures over the text"
        );
        let bubble = parent_of(&rt, &row);
        assert!(within(row.rect, bubble.rect) && within(text.rect, bubble.rect));
        assert!(!testkit::has_text(&rt, "3 images"));
    }

    /// Every node of the accessibility tree under `node`, each with the role and key of its
    /// ancestors ("role key"), outermost first.
    fn a11y_nodes<'a>(
        node: &'a serde_json::Value,
        above: &mut Vec<String>,
        out: &mut Vec<(&'a serde_json::Value, Vec<String>)>,
    ) {
        out.push((node, above.clone()));
        let role = node["role"].as_str().unwrap_or_default();
        above.push(format!(
            "{role} {}",
            node["key"].as_str().unwrap_or_default()
        ));
        for child in node["children"].as_array().into_iter().flatten() {
            a11y_nodes(child, above, out);
        }
        above.pop();
    }

    #[test]
    fn a_screen_reader_hears_a_button_that_opens_the_picture() {
        let mut rt = testkit::window("empty");
        let (_, id) = say_pictures(&mut rt, "Which one?", &[(4, 3), (3, 4)]);
        let ids = pictures_of(&rt, id);
        // The second has not come.
        rt.state.thumbs.remove(&ids[1]);
        rt.invalidate();
        testkit::step(&mut rt);
        let tree = rt.accessibility_json();
        let mut nodes = Vec::new();
        a11y_nodes(&tree, &mut Vec::new(), &mut nodes);
        let keyed = |key: &str| {
            nodes
                .iter()
                .find(|(node, _)| node["key"].as_str() == Some(key))
                .unwrap_or_else(|| panic!("no node keyed {key:?} in {tree:#}"))
        };
        let (drawn, above_drawn) = keyed(&format!("mr-user-picture-{}", ids[0]));
        let (waiting, above_waiting) = keyed(&format!("mr-user-picture-wait-{}", ids[1]));
        assert_eq!(drawn["role"], "button");
        assert_eq!(drawn["name"], "Open image 1 of 2");
        assert_eq!(waiting["role"], "image");
        assert_eq!(waiting["name"], "Image 2 of 2, not loaded");
        // Within its row of the conversation, nothing around either is a button: the tile
        // around it is not this task's.
        for above in [above_drawn, above_waiting] {
            let row = above
                .iter()
                .rposition(|node| node.contains(" mr-item-"))
                .expect("in a row");
            assert!(
                !above[row..].iter().any(|node| node.starts_with("button")),
                "a picture is inside {above:?}"
            );
        }
        let row = format!("mr-user-pictures-{}", id.0);
        assert!(
            nodes
                .iter()
                .filter(|(node, _)| node["key"].as_str() == Some(row.as_str()))
                .all(|(node, _)| !node["role"].as_str().unwrap_or_default().contains("button")),
            "the row is no button"
        );
        let images = nodes
            .iter()
            .filter(|(node, _)| {
                node["key"]
                    .as_str()
                    .is_some_and(|key| key.starts_with("mr-user-picture-"))
            })
            .count();
        assert_eq!(images, 2, "two pictures, two nodes");
    }

    #[test]
    fn a_picture_that_has_not_come_shows_a_waiting_slot() {
        let mut rt = testkit::window("empty");
        rt.state.images = None;
        let png = Source::Pasted(ImageData::new(testkit::tiny_png()));
        let (key, id) = say_with(&mut rt, "Look", vec![png]);
        let [picture] = pictures_of(&rt, id)[..] else {
            panic!("one picture");
        };
        assert!(rt.state.thumbs.is_empty(), "no cache, no handle");
        let slot = line(&rt, key, format!("mr-user-picture-wait-{picture}")).expect("the slot");
        assert_eq!(&slot.rect[2..], &[120.0, 120.0]);
        assert_eq!(slot.fill.a, 0.0, "the bubble's own colour");
        assert_eq!(slot.radius, 6.0);
        assert!(line(&rt, key, format!("mr-user-picture-{picture}")).is_none());
        let elements = rt.elements();
        let index = elements
            .iter()
            .position(|e| e.path == slot.path)
            .expect("the slot is drawn");
        let inside: Vec<&ElementInfo> = elements
            .iter()
            .filter(|e| e.parent == Some(index))
            .collect();
        assert_eq!(inside.len(), 1, "the icon alone");
        let glyph = inside[0];
        assert_eq!(glyph.role, "icon");
        assert!(glyph.text.is_none(), "no words");
        let middle = |r: [f32; 4]| (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        assert_eq!(middle(glyph.rect), middle(slot.rect), "in its middle");
        assert!(!testkit::has_text(&rt, "1 image"));
    }

    #[test]
    fn the_row_is_as_high_before_and_after_a_picture_comes() {
        let mut rt = testkit::window("empty");
        rt.state.images = None;
        let png = Source::Pasted(ImageData::new(testkit::tiny_png()));
        let (key, id) = say_with(&mut rt, "Look", vec![png]);
        let [picture] = pictures_of(&rt, id)[..] else {
            panic!("one picture");
        };
        let text = |rt: &Runtime<Ui>| line(rt, key, format!("mr-user-{}", id.0)).expect("text");
        let before = text(&rt);
        let row_before = row_of(&rt, &before).rect;
        assert!(line(&rt, key, format!("mr-user-picture-wait-{picture}")).is_some());
        // The picture comes.
        let images = ImageCache::new(Arc::new(|| {}));
        let handle = images.rgba(4, 3, vec![200u8; 4 * 3 * 4]);
        handle.wait().expect("made");
        rt.state.images = Some(images);
        rt.state.thumbs.insert(picture, handle);
        glue::run(&mut rt.state, Vec::new());
        rt.invalidate();
        testkit::step(&mut rt);
        assert!(
            rt.state.thumbs.contains_key(&picture),
            "kept: the message holds it"
        );
        let shown = line(&rt, key, format!("mr-user-picture-{picture}")).expect("drawn");
        assert_eq!(&shown.rect[2..], &[160.0, 120.0]);
        let after = text(&rt);
        assert_eq!(after.rect[1], before.rect[1], "the text did not move");
        assert_eq!(after.rect[3], before.rect[3]);
        let row_after = row_of(&rt, &after).rect;
        assert_eq!(row_after[1], row_before[1]);
        assert_eq!(row_after[3], row_before[3], "the row is as high");
    }

    #[test]
    fn many_pictures_scroll_and_do_not_wrap() {
        let mut rt = testkit::window("empty");
        let (key, long) = say(&mut rt, &"alex ".repeat(200));
        let widest = line(&rt, key, format!("mr-user-{}", long.0)).expect("the long message");
        let (key, id) = say_pictures(&mut rt, "Eight", &[(4, 3); 8]);
        let row = line(&rt, key, format!("mr-user-pictures-{}", id.0)).expect("the row");
        assert_eq!(row.rect[3], 120.0);
        let bubble = parent_of(&rt, &row);
        assert!(
            bubble.rect[2] <= widest.rect[2],
            "{} is no wider than {}",
            bubble.rect[2],
            widest.rect[2]
        );
        let shown: Vec<ElementInfo> = pictures_of(&rt, id)
            .iter()
            .map(|picture| line(&rt, key, format!("mr-user-picture-{picture}")).expect("drawn"))
            .collect();
        assert_eq!(shown.len(), 8);
        assert!(shown.iter().all(|e| e.rect[1] == row.rect[1]), "one line");
        for pair in shown.windows(2) {
            assert_eq!(pair[1].rect[0], pair[0].rect[0] + pair[0].rect[2] + 6.0);
        }
        let last = shown.last().unwrap().rect;
        assert!(
            last[0] + last[2] > row.rect[0] + row.rect[2],
            "the rest is scrolled to"
        );
    }

    #[test]
    fn a_message_of_images_alone_shows_the_row_and_no_text() {
        let mut rt = testkit::window("empty");
        let (key, id) = say_pictures(&mut rt, "", &[(4, 3)]);
        let [picture] = pictures_of(&rt, id)[..] else {
            panic!("one picture");
        };
        assert!(line(&rt, key, format!("mr-user-pictures-{}", id.0)).is_some());
        assert!(line(&rt, key, format!("mr-user-picture-{picture}")).is_some());
        assert!(
            line(&rt, key, format!("mr-user-{}", id.0)).is_none(),
            "no text"
        );
        assert!(!testkit::drawn(&rt, &format!("mr-user-files-{}", id.0)));
        assert!(!testkit::has_text(&rt, "1 image"));
    }

    #[test]
    fn a_press_on_a_picture_leaves_the_keyboard_where_it_was() {
        let mut rt = testkit::window("empty");
        let (key, id) = say_pictures(&mut rt, "Look", &[(4, 3)]);
        let [picture] = pictures_of(&rt, id)[..] else {
            panic!("one picture");
        };
        testkit::type_text(&mut rt, "and then");
        testkit::press(&mut rt, rune::Key::Left);
        testkit::press(&mut rt, rune::Key::Left);
        assert!(testkit::box_focused(&rt, key));
        let caret = glue::view_of(&rt.state, key).draft.focus;
        assert_eq!(caret, "and th".len());
        let effects_before = testkit::sent(&rt).len();
        let (x, y) = testkit::find(
            &rt,
            &testkit::tile(key, &format!("mr-user-picture-{picture}")),
        )
        .expect("the picture is drawn");
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.frame(1.0 / 60.0);
        assert!(
            testkit::box_focused(&rt, key),
            "the box keeps the keyboard while held"
        );
        rt.pointer_up(x, y);
        testkit::step(&mut rt);
        // The picture opens large over the window; put away, it gives the keyboard back.
        close_picture(&mut rt, picture);
        assert!(testkit::box_focused(&rt, key), "the box keeps the keyboard");
        let draft = &glue::view_of(&rt.state, key).draft;
        assert_eq!(draft.text, "and then");
        assert_eq!(
            (draft.anchor, draft.focus),
            (caret, caret),
            "the caret where it was"
        );
        assert_eq!(testkit::sent(&rt).len(), effects_before, "nothing is sent");
        let row = line(&rt, key, format!("mr-user-pictures-{}", id.0)).expect("the row");
        assert!(!row.focusable, "no Tab stop");
    }

    #[test]
    fn a_press_on_a_picture_focuses_its_tile() {
        let mut rt = testkit::window("chips-problem");
        let [asks, sent] = testkit::tile_keys(&rt)[..] else {
            panic!("two tiles");
        };
        let session = &rt.state.app.sessions.get(sent).unwrap().session;
        let picture = session.conversation.pictures().next().expect("a picture");
        let shown = testkit::tile(sent, &format!("mr-user-picture-{picture}"));
        assert_eq!(testkit::focused(&rt), Some(asks));
        // While the other tile's box has the keyboard, it keeps it, and its tile the focus.
        let draft = glue::view_of(&rt.state, asks).draft.clone();
        testkit::click(&mut rt, &shown);
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(asks));
        close_picture(&mut rt, picture);
        assert!(testkit::box_focused(&rt, asks));
        assert_eq!(testkit::focused(&rt), Some(asks));
        let after = &glue::view_of(&rt.state, asks).draft;
        assert_eq!((&after.text, after.focus), (&draft.text, draft.focus));
        // With no field holding the keyboard, the picture's tile takes the focus.
        rt.update(|_, tx| tx.blur());
        testkit::step(&mut rt);
        assert_eq!(glue::typing_in(&rt.state), None);
        testkit::click(&mut rt, &shown);
        testkit::step(&mut rt);
        assert_eq!(testkit::focused(&rt), Some(sent));
        close_picture(&mut rt, picture);
        assert_eq!(testkit::focused(&rt), Some(sent));
        assert!(
            !testkit::box_focused(&rt, sent),
            "the keyboard stays where it was"
        );
    }

    /// Puts away picture `picture`, which a press opened large, with Escape.
    fn close_picture(rt: &mut Runtime<Ui>, picture: u64) {
        assert_eq!(rt.state.viewing.as_ref().map(|v| v.picture), Some(picture));
        testkit::press(rt, rune::Key::Escape);
        testkit::step(rt);
        assert!(rt.state.viewing.is_none());
    }

    /// A file named `name` in `dir`, with a line in it.
    fn file(dir: &std::path::Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "a line\n").unwrap();
        path
    }

    /// The element keyed `inner` in `key`'s tile.
    fn line(rt: &Runtime<Ui>, key: SessionKey, inner: String) -> Option<ElementInfo> {
        testkit::element(rt, &testkit::tile(key, &inner))
    }

    #[test]
    fn the_pictures_go_over_the_text_and_the_files_under_it() {
        let dir = tempfile::tempdir().unwrap();
        let notes = file(dir.path(), "notes.md");
        let mut rt = testkit::window("empty");
        let png = testkit::tiny_png();
        let sources = vec![
            Source::Pasted(ImageData::new(png.as_slice())),
            Source::Path(notes),
        ];
        let (key, id) = say_with(&mut rt, "Look", sources);
        let pictures = line(&rt, key, format!("mr-user-pictures-{}", id.0)).expect("the row");
        let text = line(&rt, key, format!("mr-user-{}", id.0)).expect("the text");
        let files = line(&rt, key, format!("mr-user-files-{}", id.0)).expect("the files line");
        assert_eq!(files.text.as_deref(), Some("notes.md"));
        assert!(
            pictures.rect[1] + pictures.rect[3] <= text.rect[1],
            "the pictures over the text"
        );
        assert!(
            text.rect[1] + text.rect[3] <= files.rect[1],
            "the files under the text"
        );
        assert!(!testkit::has_text(&rt, "1 image"));
    }

    #[test]
    fn a_message_of_files_alone_shows_only_their_line() {
        let dir = tempfile::tempdir().unwrap();
        let notes = file(dir.path(), "notes.md");
        let mut rt = testkit::window("empty");
        let (key, id) = say_with(&mut rt, "", vec![Source::Path(notes)]);
        let files = line(&rt, key, format!("mr-user-files-{}", id.0)).and_then(|e| e.text);
        assert_eq!(files.as_deref(), Some("notes.md"));
        assert!(
            line(&rt, key, format!("mr-user-{}", id.0)).is_none(),
            "no text"
        );
        assert!(line(&rt, key, format!("mr-user-pictures-{}", id.0)).is_none());
    }

    #[test]
    fn a_stored_message_names_its_files() {
        let mut rt = testkit::window("resting");
        let id = SessionId("stored-with-files".into());
        let history = StoredHistory {
            items: vec![
                HistoryItem::User {
                    text: "Read these".into(),
                    image_count: 0,
                    files: vec![
                        "/home/alex/notes/plan.md".into(),
                        "/home/alex/notes/todo.txt".into(),
                    ],
                    images: Vec::new(),
                    stored: None,
                },
                HistoryItem::AssistantText("Read them.".into()),
            ],
            ..StoredHistory::default()
        };
        rt.state
            .fake
            .as_ref()
            .expect("the demo window has its fake")
            .set_history(&id, history);
        let stored = StoredSession {
            id,
            folder: "/home/alex/notes".into(),
            title: "Notes".into(),
            last_active_unix: NOW_UNIX - DAY,
        };
        let key = rt
            .state
            .app
            .sessions
            .add(Session::resting(&stored), stored.last_active_unix);
        glue::run(&mut rt.state, Vec::new());
        testkit::show(&mut rt, key);
        let id = last_message(&rt);
        let files = line(&rt, key, format!("mr-user-files-{}", id.0)).expect("the files line");
        assert_eq!(files.text.as_deref(), Some("plan.md, todo.txt"));
        assert_eq!(
            files.help.as_deref(),
            Some("/home/alex/notes/plan.md\n/home/alex/notes/todo.txt")
        );
    }
}
