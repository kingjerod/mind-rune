//! Requests for the user: a permission to use a tool, a question from Claude, a plan to
//! approve. Impossible to miss, and impossible to answer by accident: no request button ever
//! takes keyboard focus, so only a click answers one.

use mr_agent::{
    Answer, Question, QuestionAnswer, QuestionOption, RequestId, ToolDetail, UserRequest,
    UserRequestKind,
};
use mr_core::{Item, ItemBody, ItemId, RequestState, SessionKey, file_name};
use rune::{Color, Cursor, El, TextEdit, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Markdown, TextInput};

use crate::glue::{self, QuestionDraft, Ui};
use crate::view::items::{self, call_details, show_all};

/// The width of the border that marks a request waiting for the user.
const WAITING_BORDER: f32 = 1.5;

/// The space inside a waiting card, all round.
const CARD_PAD: f32 = 16.0;

/// The widest a question's options and its "Something else" field grow.
const OPTIONS_MAX_WIDTH: f32 = 560.0;

/// The size of an option's mark and of a settled request's icon.
const MARK: f32 = 14.0;

/// The height of a settled request's line, as a tool line's.
const SETTLED_HEIGHT: f32 = 28.0;

/// The height of a one-line "Always allow" button, as a kit button's (Regular).
const ALWAYS_MIN_HEIGHT: f32 = 24.0;

/// What a withdrawn request says in place of an answer.
const NO_LONGER_NEEDED: &str = "No longer needed";

/// A request of `key`'s session as a card. While it waits, a card bordered in the warning colour
/// under the label "Needs you", with its buttons; once answered or withdrawn, one line: its
/// title, then what became of it. No button on it ever takes keyboard focus.
pub fn card(ui: &Ui, key: SessionKey, item: &Item, kit: &Kit) -> El {
    let ItemBody::Request { request, state } = &item.body else {
        return col();
    };
    let title = title(&request.kind);
    match state {
        RequestState::Waiting => waiting(ui, key, item.id, request, &title, kit),
        RequestState::Answered(words) => settled(&request.id, &title, words, true, kit),
        RequestState::Withdrawn => settled(&request.id, &title, NO_LONGER_NEEDED, false, kit),
    }
}

/// The headline of a permission card.
pub fn permission_title(tool: &str, detail: &ToolDetail) -> String {
    match detail {
        ToolDetail::Command { .. } => "Run a command".to_owned(),
        ToolDetail::FileEdit { path, .. } => format!("Edit {}", file_name(path)),
        ToolDetail::FileWrite { path, .. } => format!("Write {}", file_name(path)),
        ToolDetail::FileRead { path } => format!("Read {}", file_name(path)),
        ToolDetail::Search { query } => format!("Search for {query}"),
        ToolDetail::Other { .. } => format!("Use {tool}"),
    }
}

/// The answers to send, or `None` while a question has neither a choice nor text.
pub fn question_answers(
    questions: &[Question],
    drafts: &[QuestionDraft],
) -> Option<Vec<QuestionAnswer>> {
    if drafts.len() < questions.len() {
        return None;
    }
    questions
        .iter()
        .zip(drafts)
        .map(|(question, draft)| {
            let chosen: Vec<String> = question
                .options
                .iter()
                .filter(|option| draft.chosen.contains(&option.label))
                .map(|option| option.label.clone())
                .collect();
            let free = draft.other.text.trim();
            let free_text = (!free.is_empty()).then(|| free.to_owned());
            (!chosen.is_empty() || free_text.is_some()).then(|| QuestionAnswer {
                prompt: question.prompt.clone(),
                chosen,
                free_text,
            })
        })
        .collect()
}

/// Chooses or un-chooses `label`: one choice at most unless the question allows several. With
/// one choice, the option and the "Something else" field are the answer in turn: choosing an
/// option clears the field, and [`typed`] un-chooses the option.
pub fn choose(draft: &mut QuestionDraft, question: &Question, label: &str) {
    if let Some(at) = draft.chosen.iter().position(|chosen| chosen == label) {
        draft.chosen.remove(at);
    } else if question.multi_select {
        draft.chosen.push(label.to_owned());
    } else {
        draft.chosen = vec![label.to_owned()];
        draft.other.clear();
    }
}

/// The "Something else" field of `draft` was edited: when the question allows one choice
/// and the field now holds words, they are the answer, so the option chosen is un-chosen.
pub fn typed(draft: &mut QuestionDraft, question: &Question) {
    if !question.multi_select && !draft.other.text.trim().is_empty() {
        draft.chosen.clear();
    }
}

/// What a request's card is headed with.
fn title(kind: &UserRequestKind) -> String {
    match kind {
        UserRequestKind::Permission { tool, detail, .. } => permission_title(tool, detail),
        UserRequestKind::Question { .. } => "Claude has a question".to_owned(),
        UserRequestKind::PlanApproval { .. } => "Claude's plan".to_owned(),
    }
}

/// A request that no longer waits: one line with its title and `words`, what became of it.
fn settled(id: &RequestId, title: &str, words: &str, answered: bool, kit: &Kit) -> El {
    let glyph = if answered {
        kit.glyph.check
    } else {
        kit.glyph.dash
    };
    row()
        .key(format!("mr-request-{}", id.0))
        .w_full()
        .h(SETTLED_HEIGHT)
        .gap(8.0)
        .center_y()
        .child(icon(glyph, MARK).color(kit.c.icon_muted).a11y_hidden())
        .child(
            text(title.to_owned())
                .font(kit.ty.ui)
                .color(kit.c.text_muted)
                .truncate()
                .shrink(),
        )
        .child(
            text(words.to_owned())
                .key(format!("mr-request-words-{}", id.0))
                .font(kit.ty.ui_medium)
                .color(kit.c.text_base)
                .truncate()
                .shrink(),
        )
}

/// A request of `key`'s session waiting for the user: "Needs you", its title, what it is about,
/// and its buttons, in a card bordered in the warning colour.
fn waiting(
    ui: &Ui,
    key: SessionKey,
    item: ItemId,
    request: &UserRequest,
    title: &str,
    kit: &Kit,
) -> El {
    let id = &request.id;
    let view = glue::view_of(ui, key);
    let mut action = None;
    let (body, buttons): (Vec<El>, Vec<El>) = match &request.kind {
        UserRequestKind::Permission {
            detail,
            always_allow_label,
            ..
        } => {
            let shown = call_details(kit, detail, None, view.whole.contains(&item));
            action = shown.cut.map(|lines| show_all(key, item, lines, kit));
            let mut buttons = vec![answer_button(
                key,
                id,
                "Allow once",
                "mr-allow-",
                ButtonVariant::Contrast,
                Answer::Allow { always: false },
                kit,
            )];
            if let Some(label) = always_allow_label {
                buttons.push(always_button(key, id, label, kit));
            }
            buttons.push(answer_button(
                key,
                id,
                "Deny",
                "mr-deny-",
                ButtonVariant::Danger,
                Answer::Deny { message: None },
                kit,
            ));
            let body = shown.el.key(format!("mr-request-body-{}", id.0));
            (vec![body], buttons)
        }
        UserRequestKind::Question { questions } => {
            let drafts = view.answers.get(id).map_or(&[][..], Vec::as_slice);
            let body = questions
                .iter()
                .enumerate()
                .map(|(q, question)| {
                    question_part(key, item, id, questions, q, question, drafts, kit)
                })
                .collect();
            let complete = question_answers(questions, drafts).is_some();
            let send = {
                let button_key = format!("mr-answer-{}", id.0);
                let (id, questions) = (id.clone(), questions.clone());
                request_button(
                    Button::new("Send answer")
                        .variant(ButtonVariant::Contrast)
                        .disabled(!complete)
                        .on_press(move |ui: &mut Ui| send_answers(ui, key, &id, &questions)),
                    button_key,
                    kit,
                )
            };
            (body, vec![send])
        }
        UserRequestKind::PlanApproval { plan_markdown } => {
            let plan = Markdown::new(plan_markdown.as_str())
                .selectable(true)
                .build(kit)
                .key(format!("mr-plan-{}", id.0))
                .w_full();
            let buttons = vec![
                answer_button(
                    key,
                    id,
                    "Approve",
                    "mr-approve-",
                    ButtonVariant::Contrast,
                    Answer::ApprovePlan,
                    kit,
                ),
                answer_button(
                    key,
                    id,
                    "Keep planning",
                    "mr-keep-",
                    ButtonVariant::Neutral,
                    Answer::KeepPlanning { feedback: None },
                    kit,
                ),
            ];
            (vec![plan], buttons)
        }
    };
    let head = col()
        .gap(2.0)
        .shrink()
        .min_w(0.0)
        .child(
            text("Needs you")
                .key(format!("mr-needs-you-{}", id.0))
                .font(kit.ty.small_medium)
                .color(kit.c.warning.fg),
        )
        .child(
            text(title.to_owned())
                .key(format!("mr-request-title-{}", id.0))
                .font(kit.ty.body_medium)
                .color(kit.c.text_base)
                // A file name in the title can be one long word.
                .wrap_anywhere()
                .w_full(),
        )
        .children(changed_file(&request.kind).map(|path| {
            text(path.to_owned())
                .key(format!("mr-request-path-{}", id.0))
                .font(kit.ty.mono)
                .color(kit.c.text_base)
                // A path has few word breaks, and a long file name none: break anywhere.
                .wrap_anywhere()
                .selectable(true)
                .w_full()
        }));
    let mut top = row().w_full().gap(8.0).child(head);
    if let Some(action) = action {
        top = top.child(el().grow(1.0)).child(action);
    }
    col()
        .key(format!("mr-request-{}", id.0))
        .w_full()
        .padding(CARD_PAD)
        .gap(12.0)
        .radius(kit.radius.lg)
        .fill(kit.c.bg_base)
        .border(WAITING_BORDER, kit.c.warning.icon)
        .child(top)
        .children(body)
        .child(
            row()
                .key(format!("mr-request-buttons-{}", id.0))
                .w_full()
                .flex_wrap()
                .gap(8.0)
                .children(buttons),
        )
}

/// The whole path of the file a permission asks to edit or write. The title names only the
/// file, and the same name in another folder is another decision: the card shows the path
/// whole, under its title.
fn changed_file(kind: &UserRequestKind) -> Option<&str> {
    match kind {
        UserRequestKind::Permission {
            detail: ToolDetail::FileEdit { path, .. } | ToolDetail::FileWrite { path, .. },
            ..
        } => Some(path),
        _ => None,
    }
}

/// One question of a question card of `key`'s session: its header, its prompt, one choice per
/// option and a "Something else" field. Enter in the field sends the answer once every question
/// has one.
#[expect(clippy::too_many_arguments, reason = "one question's place and look")]
fn question_part(
    key: SessionKey,
    item: ItemId,
    id: &RequestId,
    questions: &[Question],
    q: usize,
    question: &Question,
    drafts: &[QuestionDraft],
    kit: &Kit,
) -> El {
    let draft = drafts.get(q);
    let mut part = col().w_full().gap(8.0);
    if let Some(header) = &question.header {
        part = part.child(
            text(header.clone())
                .key(format!("mr-header-{}-{q}", id.0))
                .font(kit.ty.small_medium)
                .color(kit.c.text_muted),
        );
    }
    part = part.child(
        text(question.prompt.clone())
            .key(format!("mr-prompt-{}-{q}", id.0))
            .font(kit.ty.ui_medium)
            .color(kit.c.text_base)
            .wrap()
            .w_full(),
    );
    let mut choices = col().w_full().max_w(OPTIONS_MAX_WIDTH).gap(6.0);
    for (o, option) in question.options.iter().enumerate() {
        let chosen = draft.is_some_and(|d| d.chosen.contains(&option.label));
        choices = choices.child(choice(key, item, id, questions, q, o, option, chosen, kit));
    }
    let empty = TextEdit::new("");
    let edit = draft.map_or(&empty, |d| &d.other);
    let count = questions.len();
    let field = {
        let field_key = format!("mr-other-{}-{q}", id.0);
        let (id, change_id, submit_id) = (id.clone(), id.clone(), id.clone());
        let (question, questions) = (question.clone(), questions.to_vec());
        TextInput::new(edit)
            .placeholder("Something else")
            .width(None)
            .on_edit(move |ui: &mut Ui| &mut drafts_mut(ui, key, &id, count)[q].other)
            .on_change(move |ui: &mut Ui, _tx| {
                typed(&mut drafts_mut(ui, key, &change_id, count)[q], &question);
            })
            .on_submit(move |ui: &mut Ui, _tx| send_answers(ui, key, &submit_id, &questions))
            .build(kit)
            .key(field_key)
    };
    part.child(choices.child(field))
}

/// One option of a question: its label with its description beneath, marked when chosen.
/// Pressing it chooses or un-chooses it, and focuses the tile.
#[expect(clippy::too_many_arguments, reason = "one option's place and look")]
fn choice(
    key: SessionKey,
    item: ItemId,
    id: &RequestId,
    questions: &[Question],
    q: usize,
    o: usize,
    option: &QuestionOption,
    chosen: bool,
    kit: &Kit,
) -> El {
    let multi = questions[q].multi_select;
    let mut words = col().gap(2.0).shrink().min_w(0.0).child(
        text(option.label.clone())
            .font(kit.ty.ui_medium)
            .color(kit.c.text_base)
            .wrap(),
    );
    if let Some(description) = &option.description {
        words = words.child(
            text(description.clone())
                .font(kit.ty.ui)
                .color(kit.c.text_muted)
                .wrap(),
        );
    }
    let (edge, fill) = if chosen {
        (kit.c.icon_accent, kit.c.layer_02)
    } else {
        (kit.c.border_muted, Color::TRANSPARENT)
    };
    let press = {
        let (id, question, label) = (id.clone(), questions[q].clone(), option.label.clone());
        let count = questions.len();
        move |ui: &mut Ui| {
            if is_waiting(ui, key, item) {
                glue::focus_tile(ui, key, false);
                choose(&mut drafts_mut(ui, key, &id, count)[q], &question, &label);
            }
        }
    };
    let r = row()
        .key(format!("mr-option-{}-{q}-{o}", id.0))
        .role(if multi { "checkbox" } else { "radio" })
        .label(option.label.clone())
        .checked(chosen)
        .w_full()
        .gap(10.0)
        .px(12.0)
        .py(8.0)
        .radius(kit.radius.md)
        .fill(fill)
        .border(1.0, edge)
        .cursor(Cursor::Pointer)
        .child(mark(
            chosen.then(|| format!("mr-chosen-{}-{q}-{o}", id.0)),
            multi,
            kit,
        ))
        .child(words)
        .on_press(press);
    unfocusable(kit.interactive(r))
}

/// The box or circle at the start of an option: filled, with a tick keyed `chosen`, when chosen.
fn mark(chosen: Option<String>, multi: bool, kit: &Kit) -> El {
    let radius = if multi { kit.radius.sm } else { MARK / 2.0 };
    let m = el()
        .size(MARK, MARK)
        .mt(2.0)
        .radius(radius)
        .center()
        .a11y_hidden();
    if let Some(key) = chosen {
        m.fill(kit.c.icon_accent).child(
            icon(kit.glyph.check_small, MARK - 2.0)
                .key(key)
                .color(kit.c.white),
        )
    } else {
        m.border(1.0, unchosen_ring(kit))
    }
}

/// The ring of an option's mark while it is not chosen: the muted icon colour, which stands
/// out from the card in both themes (the kit's own strong border is faint in the dark one).
fn unchosen_ring(kit: &Kit) -> Color {
    kit.c.icon_muted
}

/// A button that sends `answer` to request `id` of `key`'s session, keyed `{prefix}{id}`.
fn answer_button(
    session: SessionKey,
    id: &RequestId,
    label: &str,
    prefix: &str,
    variant: ButtonVariant,
    answer: Answer,
    kit: &Kit,
) -> El {
    let key = format!("{prefix}{}", id.0);
    let id = id.clone();
    request_button(
        Button::new(label)
            .variant(variant)
            .on_press(move |ui: &mut Ui| {
                let effects = ui.app.answer(session, &id, answer.clone());
                glue::run(ui, effects);
            }),
        key,
        kit,
    )
}

/// "Always allow", labelled with the rule word for word, keyed `mr-always-{id}`. It looks
/// like a Neutral kit button, but its label wraps: the end of the rule is its scope ("in this
/// project"), so the whole of it must be readable before the click. A kit `Button` keeps one
/// line and a fixed height, so a long rule would run out of the card.
fn always_button(key: SessionKey, id: &RequestId, label: &str, kit: &Kit) -> El {
    let rid = id.clone();
    let font = kit.ty.ui_medium;
    // One line is exactly as tall as the kit buttons beside it; more lines add their height.
    let pad_y = ((ALWAYS_MIN_HEIGHT - font.line_px()) / 2.0).max(2.0);
    let words = text(label.to_owned())
        .font(font)
        .color(kit.c.text_base)
        .wrap()
        .shrink()
        .min_w(0.0)
        .a11y_hidden();
    let r = row()
        .key(format!("mr-always-{}", id.0))
        .role("button")
        .label(label.to_owned())
        .min_h(ALWAYS_MIN_HEIGHT)
        .px(12.0)
        .py(pad_y)
        .center_y()
        .shrink()
        .min_w(0.0)
        .radius(kit.radius.md)
        .fill(kit.c.bg_button_neutral)
        .cursor(Cursor::Pointer)
        .child(words)
        .on_press(move |ui: &mut Ui| {
            let effects = ui.app.answer(key, &rid, Answer::Allow { always: true });
            glue::run(ui, effects);
        });
    unfocusable(kit.interactive(kit.elevate(r, &kit.elevation.button_neutral)))
}

/// A request's button, keyed `key`: pressed by a click only. The card that asks before joining a
/// session open elsewhere has its buttons made here too.
pub(super) fn request_button(button: Button, key: String, kit: &Kit) -> El {
    unfocusable(button.build(kit).key(key))
}

/// Never a keyboard focus stop, so neither Tab, Enter nor Space can reach it, and a click on it
/// leaves whatever field has focus focused. It is never the window's default button either:
/// nothing here calls `default_button`. A request's buttons and a tile's close button are made
/// so.
pub(super) fn unfocusable(e: El) -> El {
    e.focusable(false).preserve_focus_on_press(true)
}

/// Sends the answers drafted for question request `id` of `key`'s session, once every question
/// has one.
fn send_answers(ui: &mut Ui, key: SessionKey, id: &RequestId, questions: &[Question]) {
    let Some(answers) = glue::view_of(ui, key)
        .answers
        .get(id)
        .and_then(|drafts| question_answers(questions, drafts))
    else {
        return;
    };
    let effects = ui.app.answer(key, id, Answer::Answers(answers));
    glue::run(ui, effects);
}

/// The drafts of request `id` of `key`'s session, one per question, made empty when there are
/// none yet.
fn drafts_mut<'a>(
    ui: &'a mut Ui,
    key: SessionKey,
    id: &RequestId,
    count: usize,
) -> &'a mut Vec<QuestionDraft> {
    let drafts = glue::view_mut(ui, key)
        .answers
        .entry(id.clone())
        .or_default();
    if drafts.len() < count {
        drafts.resize_with(count, QuestionDraft::default);
    }
    drafts
}

/// Whether item `item` of `key`'s session is a request still waiting.
fn is_waiting(ui: &Ui, key: SessionKey, item: ItemId) -> bool {
    items::find(ui, key, item).is_some_and(|item| {
        matches!(
            item.body,
            ItemBody::Request {
                state: RequestState::Waiting,
                ..
            }
        )
    })
}

#[cfg(test)]
mod tests {
    use mr_agent::{
        Answer, Question, QuestionAnswer, QuestionOption, RequestId, SessionCommand, SessionEvent,
        ToolDetail, UserRequest, UserRequestKind,
    };
    use mr_core::Phase;
    use rune::{Key, Runtime, TextEdit};

    use super::{choose, permission_title, question_answers, typed, unchosen_ring};
    use crate::glue::{self, QuestionDraft, Ui};
    use crate::testkit;
    use crate::theme;

    const BUTTONS: [&str; 7] = [
        "mr-allow-",
        "mr-always-",
        "mr-deny-",
        "mr-option-",
        "mr-answer-",
        "mr-approve-",
        "mr-keep-",
    ];
    fn a_request_button_has_focus(rt: &Runtime<Ui>) -> bool {
        rt.elements().iter().any(|e| {
            e.focused
                && e.key
                    .as_deref()
                    .is_some_and(|k| BUTTONS.iter().any(|b| k.starts_with(b)))
        })
    }
    fn focused_key(rt: &Runtime<Ui>) -> Option<String> {
        rt.elements()
            .into_iter()
            .find(|e| e.focused)
            .and_then(|e| e.key)
    }
    /// Whether the keyboard walks leave the focused control unpressed, because pressing it does
    /// that control's own job, which has nothing to do with requests: Space on "New session"
    /// asks for a new session's folder, and Enter on a picker's trigger opens its list, which then holds
    /// focus (Enter there picks the first option and sends focus back to the text area), so
    /// the walk would go round the picker for good and never reach the controls after it.
    fn leave_unpressed(rt: &Runtime<Ui>) -> bool {
        let controls = [
            "mr-new-session".to_string(),
            testkit::model_key(rt, "trigger"),
            testkit::mode_key(rt, "trigger"),
        ];
        focused_key(rt).is_some_and(|key| controls.contains(&key))
    }
    fn answers(rt: &Runtime<Ui>) -> Vec<Answer> {
        testkit::sent(rt)
            .into_iter()
            .filter_map(|c| match c {
                SessionCommand::Answer { answer, .. } => Some(answer),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_permission_card_names_the_tool_and_offers_three_answers() {
        let mut rt = testkit::window("permission");
        for part in [
            "Needs you",
            "Run a command",
            "touch a.txt",
            "Allow once",
            "Always allow Bash(touch a.txt) in this project",
            "Deny",
        ] {
            assert!(testkit::has_text(&rt, part), "{part}");
        }
        assert_eq!(rt.state.focused_session().unwrap().phase, Phase::NeedsUser);
        testkit::click(&mut rt, "mr-always-r1");
        testkit::step(&mut rt);
        assert_eq!(answers(&rt), vec![Answer::Allow { always: true }]);
        assert!(testkit::has_text(&rt, "Always allowed"));
        assert!(rt.find("mr-allow-r1").is_none() && rt.find("mr-deny-r1").is_none());
    }

    #[test]
    fn without_a_suggested_rule_there_is_no_always_button() {
        let mut rt = testkit::window("permission-plain");
        assert!(rt.find("mr-always-r2").is_none());
        testkit::click(&mut rt, "mr-deny-r2");
        testkit::step(&mut rt);
        assert_eq!(answers(&rt), vec![Answer::Deny { message: None }]);
        assert!(testkit::has_text(&rt, "Denied"));
    }

    #[test]
    fn the_keyboard_cannot_answer_a_request() {
        let mut rt = testkit::window("permission");
        assert!(!a_request_button_has_focus(&rt));
        for _ in 0..30 {
            testkit::press(&mut rt, Key::Tab);
            assert!(!a_request_button_has_focus(&rt));
            if leave_unpressed(&rt) {
                continue;
            }
            testkit::press(&mut rt, Key::Enter);
            testkit::press(&mut rt, Key::Text(" ".into()));
        }
        testkit::step(&mut rt);
        assert_eq!(answers(&rt), vec![]);
        assert_eq!(
            rt.state.focused_session().unwrap().conversation.waiting(),
            1
        );
    }

    #[test]
    fn a_request_that_dies_with_its_session_stops_offering_buttons() {
        let mut rt = testkit::window("permission");
        testkit::emit(
            &mut rt,
            SessionEvent::Exited {
                clean: false,
                detail: "killed".into(),
            },
        );
        assert!(rt.find("mr-allow-r1").is_none());
        assert!(testkit::has_text(&rt, "No longer needed"));
        assert!(rt.find("mr-restart").is_some());
        assert_eq!(answers(&rt), vec![]);
    }

    #[test]
    fn a_withdrawn_request_stops_offering_buttons() {
        let mut rt = testkit::window("permission");
        testkit::emit(
            &mut rt,
            SessionEvent::RequestWithdrawn {
                id: RequestId("r1".into()),
            },
        );
        assert!(rt.find("mr-allow-r1").is_none());
        assert!(testkit::has_text(&rt, "No longer needed"));
    }

    #[test]
    fn a_question_is_sent_only_when_every_part_is_answered() {
        let mut rt = testkit::window("question");
        for part in [
            "Claude has a question",
            "Colour",
            "Which colour?",
            "Red",
            "Warm",
            "Which sizes?",
        ] {
            assert!(testkit::has_text(&rt, part), "{part}");
        }
        testkit::click(&mut rt, "mr-answer-q1");
        testkit::click(&mut rt, "mr-option-q1-0-0");
        testkit::click(&mut rt, "mr-option-q1-0-1");
        testkit::click(&mut rt, "mr-answer-q1");
        testkit::step(&mut rt);
        assert_eq!(
            answers(&rt),
            vec![],
            "the second question has no answer yet"
        );
        testkit::click(&mut rt, "mr-option-q1-1-2");
        testkit::click(&mut rt, "mr-option-q1-1-0");
        testkit::click(&mut rt, "mr-answer-q1");
        testkit::step(&mut rt);
        assert_eq!(
            answers(&rt),
            vec![Answer::Answers(vec![
                QuestionAnswer {
                    prompt: "Which colour?".into(),
                    chosen: vec!["Blue".into()],
                    free_text: None
                },
                QuestionAnswer {
                    prompt: "Which sizes?".into(),
                    chosen: vec!["S".into(), "L".into()],
                    free_text: None
                },
            ])]
        );
        assert!(testkit::has_text(&rt, "Blue, S, L"));
    }

    #[test]
    fn free_text_counts_as_an_answer() {
        let q = |multi| Question {
            header: None,
            prompt: "p".into(),
            options: vec![QuestionOption {
                label: "A".into(),
                description: None,
            }],
            multi_select: multi,
        };
        let mut draft = QuestionDraft {
            chosen: vec![],
            other: TextEdit::new(""),
        };
        assert_eq!(
            question_answers(&[q(false)], std::slice::from_ref(&draft)),
            None
        );
        draft.other = TextEdit::new("  my own  ");
        assert_eq!(
            question_answers(&[q(false)], std::slice::from_ref(&draft)),
            Some(vec![QuestionAnswer {
                prompt: "p".into(),
                chosen: vec![],
                free_text: Some("my own".into())
            }])
        );
        choose(&mut draft, &q(false), "A");
        choose(&mut draft, &q(false), "A");
        assert!(draft.chosen.is_empty(), "choosing again un-chooses");
    }

    /// The contrast ratio of `ink` (with its alpha, laid over `ground`) against `ground`.
    fn contrast(ink: rune::Color, ground: rune::Color) -> f32 {
        let laid = ground.mix(ink.alpha(1.0), ink.a);
        let (a, b) = (laid.luminance() + 0.05, ground.luminance() + 0.05);
        a.max(b) / a.min(b)
    }

    #[test]
    fn an_unchosen_mark_is_clearly_visible_in_both_themes() {
        for dark in [false, true] {
            let kit = theme::kit(dark);
            let ratio = contrast(unchosen_ring(kit), kit.c.bg_base);
            // 3:1 is what WCAG asks of the parts of a control a user must see to use it.
            assert!(ratio >= 3.0, "dark {dark}: {ratio:.2}:1");
        }
    }

    fn one_of(multi: bool) -> Question {
        Question {
            header: None,
            prompt: "p".into(),
            options: ["A", "B"]
                .map(|label| QuestionOption {
                    label: label.into(),
                    description: None,
                })
                .into(),
            multi_select: multi,
        }
    }

    #[test]
    fn with_one_choice_typing_and_choosing_replace_each_other() {
        let single = one_of(false);
        let mut draft = QuestionDraft::default();
        choose(&mut draft, &single, "A");
        draft.other = TextEdit::new("my own");
        typed(&mut draft, &single);
        assert!(draft.chosen.is_empty(), "typing un-chooses");
        choose(&mut draft, &single, "B");
        assert_eq!(draft.chosen, vec!["B".to_string()]);
        assert_eq!(draft.other.text, "", "choosing clears the field");
        draft.other = TextEdit::new("  ");
        typed(&mut draft, &single);
        assert_eq!(
            draft.chosen,
            vec!["B".to_string()],
            "blanks are not an answer"
        );
    }

    #[test]
    fn with_several_choices_typing_and_choosing_both_stay() {
        let multi = one_of(true);
        let mut draft = QuestionDraft::default();
        choose(&mut draft, &multi, "A");
        draft.other = TextEdit::new("XL");
        typed(&mut draft, &multi);
        choose(&mut draft, &multi, "B");
        assert_eq!(draft.chosen, vec!["A".to_string(), "B".to_string()]);
        assert_eq!(draft.other.text, "XL");
    }

    #[test]
    fn a_typed_answer_replaces_a_single_choice_and_joins_several() {
        let mut rt = testkit::window("question");
        testkit::click(&mut rt, "mr-option-q1-0-0");
        testkit::click(&mut rt, "mr-other-q1-0");
        testkit::type_text(&mut rt, "Crimson");
        assert!(!chosen(&rt, 0, 0), "typing un-chose Red");
        testkit::click(&mut rt, "mr-option-q1-0-0");
        assert!(chosen(&rt, 0, 0));
        assert_eq!(
            testkit::shown_view(&rt).answers[&q1()][0].other.text,
            "",
            "choosing Red cleared the field"
        );
        testkit::click(&mut rt, "mr-other-q1-0");
        testkit::type_text(&mut rt, "Crimson");
        testkit::click(&mut rt, "mr-option-q1-1-0");
        testkit::click(&mut rt, "mr-other-q1-1");
        testkit::type_text(&mut rt, "XL");
        assert!(chosen(&rt, 1, 0), "S stays chosen beside a typed size");
        testkit::click(&mut rt, "mr-answer-q1");
        testkit::step(&mut rt);
        let sent = answers(&rt);
        assert_eq!(
            sent,
            vec![Answer::Answers(vec![
                QuestionAnswer {
                    prompt: "Which colour?".into(),
                    chosen: vec![],
                    free_text: Some("Crimson".into())
                },
                QuestionAnswer {
                    prompt: "Which sizes?".into(),
                    chosen: vec!["S".into()],
                    free_text: Some("XL".into())
                },
            ])]
        );
        let settled = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("mr-request-words-q1"))
            .and_then(|e| e.text);
        assert_eq!(settled.as_deref(), Some("Crimson, S, XL"));
        assert_eq!(settled.unwrap(), mr_core::answer_words(&sent[0]));
    }

    #[test]
    fn a_plan_is_approved_or_sent_back() {
        let mut rt = testkit::window("plan");
        assert!(testkit::has_text(&rt, "Claude's plan"));
        testkit::click(&mut rt, "mr-keep-p1");
        testkit::step(&mut rt);
        assert_eq!(answers(&rt), vec![Answer::KeepPlanning { feedback: None }]);
        assert!(testkit::has_text(&rt, "Kept planning"));
        let mut rt = testkit::window("plan");
        testkit::click(&mut rt, "mr-approve-p1");
        testkit::step(&mut rt);
        assert_eq!(answers(&rt), vec![Answer::ApprovePlan]);
    }

    #[test]
    fn the_titles_read_as_written() {
        let t = |d| permission_title("Tool", &d);
        assert_eq!(
            t(ToolDetail::FileEdit {
                path: "/p/a.rs".into(),
                old_text: String::new(),
                new_text: String::new()
            }),
            "Edit a.rs"
        );
        assert_eq!(
            t(ToolDetail::FileWrite {
                path: "b.md".into(),
                content: String::new()
            }),
            "Write b.md"
        );
        assert_eq!(t(ToolDetail::FileRead { path: "c".into() }), "Read c");
        assert_eq!(t(ToolDetail::Search { query: "x".into() }), "Search for x");
        assert_eq!(
            t(ToolDetail::Other {
                input_json: "{}".into()
            }),
            "Use Tool"
        );
    }

    /// Whether option `o` of question `q` of request q1 shows as chosen.
    fn chosen(rt: &Runtime<Ui>, q: usize, o: usize) -> bool {
        rt.find(&format!("mr-chosen-q1-{q}-{o}")).is_some()
    }

    fn q1() -> RequestId {
        RequestId("q1".into())
    }

    #[test]
    fn a_chosen_option_shows_as_chosen_at_once_and_unchosen_again() {
        let mut rt = testkit::window("question");
        assert!(!chosen(&rt, 0, 0));
        testkit::click(&mut rt, "mr-option-q1-0-0");
        assert!(chosen(&rt, 0, 0), "chosen after one frame");
        testkit::click(&mut rt, "mr-option-q1-0-1");
        assert!(!chosen(&rt, 0, 0), "one colour at most");
        assert!(chosen(&rt, 0, 1));
        testkit::click(&mut rt, "mr-option-q1-1-0");
        testkit::click(&mut rt, "mr-option-q1-1-2");
        assert!(chosen(&rt, 1, 0) && chosen(&rt, 1, 2), "several sizes");
        testkit::click(&mut rt, "mr-option-q1-1-0");
        assert!(
            !chosen(&rt, 1, 0) && chosen(&rt, 1, 2),
            "choosing again un-chooses"
        );
    }

    #[test]
    fn something_else_keeps_its_caret_and_enter_sends_only_a_complete_answer() {
        let mut rt = testkit::window("question");
        testkit::click(&mut rt, "mr-other-q1-1");
        testkit::type_text(&mut rt, "extra large");
        assert!(testkit::has_text(&rt, "extra large"), "a space is a space");
        let caret = rt
            .elements()
            .into_iter()
            .any(|e| e.focused && e.caret.is_some());
        assert!(caret, "the field keeps focus and its caret while typing");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(answers(&rt), vec![], "the colour has no answer yet");
        testkit::click(&mut rt, "mr-option-q1-0-0");
        testkit::click(&mut rt, "mr-other-q1-1");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        assert_eq!(
            answers(&rt),
            vec![Answer::Answers(vec![
                QuestionAnswer {
                    prompt: "Which colour?".into(),
                    chosen: vec!["Red".into()],
                    free_text: None
                },
                QuestionAnswer {
                    prompt: "Which sizes?".into(),
                    chosen: vec![],
                    free_text: Some("extra large".into())
                },
            ])]
        );
        assert!(testkit::has_text(&rt, "Red, extra large"));
        assert!(
            testkit::shown_view(&rt).answers.is_empty(),
            "an answered request's drafts are dropped"
        );
    }

    #[test]
    fn a_withdrawn_question_drops_its_drafts() {
        let mut rt = testkit::window("question");
        testkit::click(&mut rt, "mr-option-q1-0-0");
        assert!(testkit::shown_view(&rt).answers.contains_key(&q1()));
        testkit::emit(&mut rt, SessionEvent::RequestWithdrawn { id: q1() });
        assert!(testkit::shown_view(&rt).answers.is_empty());
        assert!(testkit::has_text(&rt, "No longer needed"));
    }

    #[test]
    fn a_new_session_that_reuses_a_request_id_starts_from_an_empty_draft() {
        let mut rt = testkit::window("question");
        let asking = testkit::focused(&rt).unwrap();
        testkit::click(&mut rt, "mr-option-q1-0-0");
        testkit::click(&mut rt, "mr-other-q1-1");
        testkit::type_text(&mut rt, "XL");
        let _folder = testkit::new_session_elsewhere(&mut rt);
        assert_eq!(testkit::shown_open(&rt), Some(2));
        assert!(
            testkit::shown_view(&rt).answers.is_empty(),
            "the new session has no drafts"
        );
        assert!(
            glue::view_of(&rt.state, asking).answers.contains_key(&q1()),
            "the old session still asks, and keeps its own"
        );
        let asked = rt
            .state
            .focused_session()
            .unwrap()
            .conversation
            .items()
            .len();
        assert_eq!(asked, 0);
        let same = SessionEvent::NeedsUser(UserRequest {
            id: q1(),
            kind: UserRequestKind::Question {
                questions: vec![Question {
                    header: None,
                    prompt: "Which colour?".into(),
                    options: vec![
                        QuestionOption {
                            label: "Red".into(),
                            description: None,
                        },
                        QuestionOption {
                            label: "Blue".into(),
                            description: None,
                        },
                    ],
                    multi_select: false,
                }],
            },
        });
        testkit::emit(&mut rt, same);
        assert!(rt.find("mr-answer-q1").is_some());
        // Drawing the field makes an empty draft (Rune syncs focus into it): empty is the point.
        let drafts = testkit::shown_view(&rt)
            .answers
            .get(&q1())
            .map_or(&[][..], Vec::as_slice);
        assert!(
            drafts
                .iter()
                .all(|d| d.chosen.is_empty() && d.other.text.is_empty())
        );
        assert!(!testkit::has_text(&rt, "XL"));
        assert!(!chosen(&rt, 0, 0), "Red is not chosen");
    }

    #[test]
    fn the_keyboard_cannot_answer_a_question() {
        let mut rt = testkit::window("question");
        testkit::click(&mut rt, "mr-option-q1-0-0");
        for _ in 0..30 {
            testkit::press(&mut rt, Key::Tab);
            assert!(!a_request_button_has_focus(&rt));
            if leave_unpressed(&rt) {
                continue;
            }
            testkit::press(&mut rt, Key::Enter);
            testkit::press(&mut rt, Key::Text(" ".into()));
        }
        testkit::step(&mut rt);
        assert_eq!(answers(&rt), vec![]);
        let session = rt.state.focused_session().unwrap();
        assert_eq!(session.conversation.waiting(), 1);
    }

    /// The rule the `permission-long` scene offers (see demo.rs).
    const LONG_RULE: &str = "Always allow Bash(cargo build:*), Bash(cargo test:*), Read(/home/alex/projects/very-long-folder-name/src/**) for everyone in this project";

    /// The window's smallest size (`AppSpec::min_size`).
    const SMALLEST: (f32, f32) = (560.0, 400.0);

    fn rect(rt: &Runtime<Ui>, key: &str) -> [f32; 4] {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
            .unwrap_or_else(|| panic!("no element keyed {key:?}"))
            .rect
    }

    /// Whether rect `inner` lies inside rect `outer` (x, y, width, height), to half a point.
    fn inside(inner: [f32; 4], outer: [f32; 4]) -> bool {
        let e = 0.5;
        inner[0] >= outer[0] - e
            && inner[1] >= outer[1] - e
            && inner[0] + inner[2] <= outer[0] + outer[2] + e
            && inner[1] + inner[3] <= outer[1] + outer[3] + e
    }

    /// Every element inside the element keyed `card` that runs past its left or right side.
    fn overflowing(rt: &Runtime<Ui>, card: &str) -> Vec<String> {
        let els = rt.elements();
        let at = els
            .iter()
            .position(|e| e.key.as_deref() == Some(card) && !e.exiting)
            .unwrap_or_else(|| panic!("no element keyed {card:?}"));
        let [left, _, width, _] = els[at].rect;
        let within = |mut i: usize| loop {
            match els[i].parent {
                Some(p) if p == at => return true,
                Some(p) => i = p,
                None => return false,
            }
        };
        els.iter()
            .enumerate()
            .filter(|&(i, e)| {
                within(i)
                    && !e.exiting
                    && (e.rect[0] < left - 0.5 || e.rect[0] + e.rect[2] > left + width + 0.5)
            })
            .map(|(_, e)| format!("{:?} {:?} {:?}", e.key, e.text, e.rect))
            .collect()
    }

    #[test]
    fn a_long_rule_wraps_inside_the_card_and_is_read_whole() {
        for (width, height) in [SMALLEST, (1200.0, 800.0)] {
            let mut rt = testkit::window_sized("permission-long", (width, height));
            let card = rect(&rt, "mr-request-r3");
            assert!(
                inside(rect(&rt, "mr-always-r3"), card),
                "{width}x{height}: {:?} outside {card:?}",
                rect(&rt, "mr-always-r3")
            );
            assert_eq!(overflowing(&rt, "mr-request-r3"), Vec::<String>::new());
            assert!(
                testkit::texts(&rt).iter().any(|t| t == LONG_RULE),
                "{width}x{height}: the whole rule shows"
            );
            testkit::click(&mut rt, "mr-always-r3");
            testkit::step(&mut rt);
            assert_eq!(answers(&rt), vec![Answer::Allow { always: true }]);
            // At the smallest size the card is taller than the conversation's viewport (the
            // tile's header and the input box take the rest), so its settled line starts above
            // the viewport and comes into view as the card's buttons finish leaving: the window
            // settles first.
            rt.settle_all();
            testkit::step(&mut rt);
            assert!(testkit::has_text(&rt, "Always allowed"));
        }
    }

    #[test]
    fn a_short_rule_keeps_one_line() {
        let rt = testkit::window("permission");
        let [_, _, _, always] = rect(&rt, "mr-always-r1");
        let [_, _, _, allow] = rect(&rt, "mr-allow-r1");
        assert!((always - allow).abs() < 1.0, "{always} against {allow}");
    }

    /// Emits a permission request `id` for `tool` to change the file at `path`.
    fn file_request(rt: &mut Runtime<Ui>, id: &str, tool: &str, path: &str) {
        let detail = if tool == "Write" {
            ToolDetail::FileWrite {
                path: path.into(),
                content: "Host example\n".into(),
            }
        } else {
            ToolDetail::FileEdit {
                path: path.into(),
                old_text: "a\n".into(),
                new_text: "b\n".into(),
            }
        };
        testkit::emit(
            rt,
            SessionEvent::NeedsUser(UserRequest {
                id: RequestId(id.into()),
                kind: UserRequestKind::Permission {
                    tool: tool.into(),
                    detail,
                    always_allow_label: None,
                },
            }),
        );
        // A new row climbs into view; let it land.
        rt.settle_all();
        testkit::step(rt);
    }

    /// The text of the element keyed `key`.
    fn text_of(rt: &Runtime<Ui>, key: &str) -> Option<String> {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
            .and_then(|e| e.text)
    }

    #[test]
    fn a_card_to_change_a_file_shows_its_whole_path() {
        let mut rt = testkit::window("empty");
        // Outside the session's folder, /home/alex/project.
        file_request(&mut rt, "w1", "Write", "/home/alex/.ssh/config");
        assert!(testkit::has_text(&rt, "Write config"), "the title stays");
        assert_eq!(
            text_of(&rt, "mr-request-path-w1").as_deref(),
            Some("/home/alex/.ssh/config")
        );
        let [_, title_y, _, title_h] = rect(&rt, "mr-request-title-w1");
        let [_, path_y, _, _] = rect(&rt, "mr-request-path-w1");
        assert!(path_y >= title_y + title_h - 0.5, "under the title");
        let [_, body_y, _, _] = rect(&rt, "mr-request-body-w1");
        assert!(path_y < body_y, "above what it would write");
        testkit::click(&mut rt, "mr-deny-w1");
        testkit::step(&mut rt);
        assert!(
            rt.find("mr-request-path-w1").is_none(),
            "only while it waits"
        );
    }

    #[test]
    fn a_long_path_wraps_inside_the_card() {
        let (width, height) = SMALLEST;
        let mut rt = testkit::window_sized("empty", (width, height));
        let path =
            "/home/alex/projects/very-long-folder-name/with/many/levels/below/it/src/main.rs";
        file_request(&mut rt, "e1", "Edit", path);
        // The card is taller than the conversation here, so its title may be scrolled away.
        assert_eq!(
            text_of(&rt, "mr-request-title-e1").as_deref(),
            Some("Edit main.rs")
        );
        assert_eq!(text_of(&rt, "mr-request-path-e1").as_deref(), Some(path));
        let card = rect(&rt, "mr-request-e1");
        let shown = rect(&rt, "mr-request-path-e1");
        assert!(inside(shown, card), "{shown:?} outside {card:?}");
        let [_, _, _, one_line] = rect(&rt, "mr-request-title-e1");
        assert!(
            shown[3] > one_line * 1.5,
            "on more than one line: {shown:?}"
        );
        assert_eq!(overflowing(&rt, "mr-request-e1"), Vec::<String>::new());
    }

    #[test]
    fn a_long_file_name_breaks_anywhere_inside_the_card() {
        let mut rt = testkit::window_sized("empty", SMALLEST);
        let name = format!("{}.rs", "generated_bindings_for_the_whole_api".repeat(3));
        let path = format!("/home/alex/project/{name}");
        file_request(&mut rt, "e2", "Edit", &path);
        assert_eq!(
            text_of(&rt, "mr-request-path-e2").as_deref(),
            Some(path.as_str())
        );
        let card = rect(&rt, "mr-request-e2");
        let kit = crate::theme::kit(true);
        // The title names the file too.
        for (key, font) in [
            ("mr-request-path-e2", kit.ty.mono),
            ("mr-request-title-e2", kit.ty.body_medium),
        ] {
            let shown = rect(&rt, key);
            assert!(inside(shown, card), "{key}: {shown:?} outside {card:?}");
            // The element is as wide as the card either way; its height tells how its text was
            // broken. At word breaks only, the name would be one line running past the card.
            let text = text_of(&rt, key).unwrap();
            let measured = |options: rune::text::RichLayoutOptions| {
                rune::text::measure_with(&text, &[], font, Some(shown[2]), options)
            };
            let anywhere = measured(rune::text::RichLayoutOptions::new().wrap_anywhere());
            let at_words = measured(rune::text::RichLayoutOptions::default());
            assert!(at_words.0 > shown[2], "{key}: at words it would not fit");
            assert!(anywhere.0 <= shown[2], "{key}: broken anywhere it fits");
            assert!(
                (shown[3] - anywhere.1).abs() < 1.0,
                "{key}: broken anywhere: {} tall, {} expected",
                shown[3],
                anywhere.1
            );
        }
    }

    #[test]
    fn a_card_for_other_tools_shows_no_path_line() {
        let rt = testkit::window("permission");
        assert!(rt.find("mr-request-path-r1").is_none());
    }

    #[test]
    fn long_questions_wrap_inside_the_card() {
        let (width, height) = SMALLEST;
        let mut rt = testkit::window_sized("empty", (width, height));
        let long = |what: &str| format!("{what} {}", "that goes on and on ".repeat(8));
        testkit::emit(
            &mut rt,
            SessionEvent::NeedsUser(UserRequest {
                id: RequestId("q9".into()),
                kind: UserRequestKind::Question {
                    questions: vec![Question {
                        header: Some(long("A header")),
                        prompt: long("A prompt"),
                        options: vec![QuestionOption {
                            label: long("An option"),
                            description: Some(long("A description")),
                        }],
                        multi_select: false,
                    }],
                },
            }),
        );
        assert_eq!(overflowing(&rt, "mr-request-q9"), Vec::<String>::new());
    }
}
