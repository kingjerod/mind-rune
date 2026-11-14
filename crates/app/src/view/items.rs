//! One row of the conversation per item: the user's message, Claude's reply with its copy
//! controls, thinking, tool lines with their details, file edits as diffs, and notices.
//! Requests are drawn as cards by `requests`.

use std::hash::Hash;

use mr_agent::{ToolDetail, ToolOutcome};
use mr_core::{
    Item, ItemBody, ItemId, NoticeKind, OUTPUT_CHARS, OUTPUT_LINES, Phase, RequestState,
    ToolFamily, ToolGroup, clip, fenced, file_name, line_counts, thinking_label, unified_diff,
};
use rune::{Color, Cursor, El, Icon, Span, col, el, icon, rich, text};
use rune_kit::{
    Button, ButtonVariant, CodeFrame, IconButton, IconButtonVariant, Kit, Markdown, Spinner,
};
use rune_theme_opencode::icons::{
    BRAIN, CUBE, FILE_DOCUMENT, MAGNIFYING_GLASS, PENCIL_LINE, TERMINAL, WARNING,
};

use crate::glue::{self, QuestionDraft, Ui};
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

/// One item as a row keyed `mr-item-{id}`, its spacing as padding inside it.
pub fn row(ui: &Ui, item: &Item, kit: &'static Kit) -> El {
    let id = item.id;
    let folds = matches!(item.body, ItemBody::Thinking { .. } | ItemBody::Tools(_));
    let (content, pad_bottom) = match &item.body {
        ItemBody::User { text } => (Some(user_message(id, text, kit)), ROW_PAD_BOTTOM),
        ItemBody::Reply {
            markdown,
            streaming,
        } => (Some(reply(id, markdown, *streaming, kit)), ROW_PAD_BOTTOM),
        ItemBody::Thinking { text, seconds, .. } => (
            Some(thinking(id, text, *seconds, ui.toggled.contains(&id), kit)),
            LINE_PAD_BOTTOM,
        ),
        ItemBody::Tools(group) => (Some(tools(ui, id, group, kit)), LINE_PAD_BOTTOM),
        ItemBody::Notice { kind, text } => (
            Some(notice(id, *kind, text, offers_restart(ui, item), kit)),
            ROW_PAD_BOTTOM,
        ),
        ItemBody::Request { .. } => (Some(requests::card(ui, item, kit)), ROW_PAD_BOTTOM),
    };
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
    /// The session the conversation belongs to: a new one starts its ids and stamps again.
    open: u64,
    /// The item's own change stamp: its body, as every kind of row draws it.
    changed: u64,
    /// The user opened or closed it against its kind's default (thinking, tool groups).
    toggled: bool,
    /// The user asked to see its long text whole (tool groups).
    whole: bool,
    /// It is the notice that offers Restart: read from the session's phase and the
    /// conversation's later items.
    restart: bool,
    /// A waiting request's drafts in `ui.answers`: the options chosen and the "Something else"
    /// text with its caret and focus. `None` for every other item.
    draft: Option<Vec<QuestionDraft>>,
}

/// The version of `item`'s row. A kind of row that reads anything else from `Ui` or the
/// session adds it here.
pub fn version(ui: &Ui, item: &Item) -> impl Hash + use<> {
    RowVersion {
        open: ui.app.current_open(),
        changed: item.changed,
        toggled: ui.toggled.contains(&item.id),
        whole: ui.whole.contains(&item.id),
        restart: offers_restart(ui, item),
        draft: waiting_draft(ui, item),
    }
}

/// What the user has drafted for `item`, when it is a request still waiting.
fn waiting_draft(ui: &Ui, item: &Item) -> Option<Vec<QuestionDraft>> {
    match &item.body {
        ItemBody::Request {
            request,
            state: RequestState::Waiting,
        } => ui.answers.get(&request.id).cloned(),
        _ => None,
    }
}

/// The user's message as typed, never read as Markdown: plain selectable text in a filled,
/// rounded box at the right of the column.
fn user_message(id: ItemId, message: &str, kit: &Kit) -> El {
    let bubble = text(message.to_owned())
        .key(format!("mr-user-{}", id.0))
        .font(kit.ty.body)
        .color(kit.c.text_base)
        .wrap()
        .selectable(true)
        .max_w(USER_MAX_WIDTH)
        .px(14.0)
        .py(10.0)
        .radius(kit.radius.xl)
        .fill(kit.c.layer_02);
    rune::row().w_full().justify_end().child(bubble)
}

/// Claude's reply: Markdown over the whole text so far, each finished code block with a copy
/// button, and once the reply has finished, a copy button for its source beneath it.
fn reply(id: ItemId, markdown: &str, streaming: bool, kit: &'static Kit) -> El {
    let body = Markdown::new(markdown)
        .streaming(streaming)
        .selectable(true)
        .code_frame(move |card, frame| code_block(id, card, frame, kit))
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
            if let Some(ItemBody::Reply { markdown, .. }) = find(ui, id).map(|item| &item.body) {
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
fn code_block(id: ItemId, card: El, frame: &CodeFrame<'_>, kit: &Kit) -> El {
    let framed = col().w_full().child(card);
    if frame.open {
        return framed;
    }
    let source = frame.source.to_owned();
    let copy = IconButton::new(kit.glyph.copy)
        .variant(IconButtonVariant::GhostMuted)
        .help("Copy")
        .on_press_tx(move |_: &mut Ui, tx| tx.copy(source.clone()))
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
fn thinking(id: ItemId, thought: &str, seconds: Option<f64>, open: bool, kit: &Kit) -> El {
    let status = seconds
        .is_none()
        .then(|| spinner(kit).key(format!("mr-running-{}", id.0)));
    let label = thinking_label(seconds);
    if thought.is_empty() {
        return line(id, &BRAIN, &label, status, None, None, kit);
    }
    let mut c = col()
        .w_full()
        .child(line(id, &BRAIN, &label, status, Some(open), None, kit));
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
fn tools(ui: &Ui, id: ItemId, group: &ToolGroup, kit: &Kit) -> El {
    let open = starts_open(group) != ui.toggled.contains(&id);
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
        return line(id, icon, &summary, status, Some(false), None, kit);
    }
    let whole = ui.whole.contains(&id);
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
    let action = hidden.map(|lines| show_all(id, lines, kit));
    col()
        .w_full()
        .child(line(id, icon, &summary, status, Some(true), action, kit))
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

/// "Show all (N more lines)", or "Show all" when no line was left out whole, keyed
/// `mr-show-all-{id}`: pressing it adds `id` to `ui.whole`, so the row shows its text whole.
/// A tool group puts it at the right end of its line.
pub fn show_all(id: ItemId, hidden_lines: usize, kit: &Kit) -> El {
    let label = if hidden_lines == 0 {
        "Show all".to_owned()
    } else {
        format!("Show all ({hidden_lines} more lines)")
    };
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .on_press(move |ui: &mut Ui| {
            ui.whole.insert(id);
        })
        .build(kit)
        .key(format!("mr-show-all-{}", id.0))
}

/// A notice: muted when the user stopped the turn, in the danger colour when a turn failed or
/// the session ended, in the warning colour when a command was refused. The notice that says
/// why the session ended offers Restart while it stays ended.
fn notice(id: ItemId, kind: NoticeKind, words: &str, restart: bool, kit: &Kit) -> El {
    let ink = match kind {
        NoticeKind::Stopped => kit.c.text_muted,
        NoticeKind::TurnFailed | NoticeKind::Ended => kit.c.danger.fg,
        NoticeKind::CommandFailed => kit.c.warning.fg,
    };
    let mut c = col().w_full().gap(8.0).child(
        text(words.to_owned())
            .key(format!("mr-notice-{}", id.0))
            .font(kit.ty.ui)
            .color(ink)
            .wrap()
            .selectable(true)
            .w_full(),
    );
    if restart {
        c = c.child(
            rune::row().child(
                Button::new("Restart")
                    .on_press(|ui: &mut Ui| {
                        let effects = ui.app.restart();
                        glue::run(ui, effects);
                    })
                    .build(kit)
                    .key("mr-restart"),
            ),
        );
    }
    c
}

/// Whether `item` is the notice that offers Restart: the last `Ended` notice, while the
/// session's phase is `Ended`. Only the items after it are looked at.
fn offers_restart(ui: &Ui, item: &Item) -> bool {
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
    let Some(session) = &ui.app.session else {
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

/// The current session's item `id`. Ids grow in the order items are pushed.
pub(super) fn find(ui: &Ui, id: ItemId) -> Option<&Item> {
    let items = ui.app.session.as_ref()?.conversation.items();
    let at = items.binary_search_by_key(&id.0, |i| i.id.0).ok()?;
    items.get(at)
}

/// A thinking or tool line: icon, label, status and, at its right end, `action`. With
/// `toggle`, the line is keyed
/// `mr-toggle-{id}`, shows whether it is open and flips the item in `ui.toggled`, holding the
/// line where it is on screen while its details open or shut beneath; without, it is keyed
/// `mr-line-{id}` and does not react.
fn line(
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
            let key = format!("mr-toggle-{}", id.0);
            r = r
                .key(key.clone())
                .role("button")
                .label(label.to_owned())
                .expanded(open)
                .cursor(Cursor::Pointer)
                .radius(kit.radius.md)
                .on_press_tx(move |ui: &mut Ui, tx| {
                    if !ui.toggled.remove(&id) {
                        ui.toggled.insert(id);
                    }
                    tx.keep_on_screen(key.clone());
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

/// `text` cut to what a closed-up tool output shows, unless `whole`; adds to `cut` what was
/// left out.
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
    use mr_agent::{SessionEvent, ToolCallId, ToolDetail, ToolOutcome};
    use mr_core::Phase;
    use rune::Runtime;

    use crate::glue::Ui;
    use crate::testkit;

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
        testkit::click(&mut rt, "mr-new-session");
        testkit::step(&mut rt);
        assert_eq!(rt.state.app.current_open(), 2);
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
        assert!(rt.state.toggled.is_empty() && rt.state.whole.is_empty());
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
        assert_eq!(rt.state.app.current_open(), 2);
        assert_eq!(rt.state.app.session.as_ref().unwrap().phase, Phase::Idle);
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
}
