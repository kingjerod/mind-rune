//! A session's task list, drawn above its input box: the steps Claude says it is working
//! through, open as a checklist or folded to one line. It does not scroll with the
//! conversation, and nothing of it takes the keyboard's focus: its head line is pressed with
//! the pointer only.

use std::ops::Range;

use mr_agent::{TaskList, TaskStatus, TaskStep};
use mr_core::{Phase, SessionKey};
use rune::{Color, Cursor, El, Spring, col, el, icon, row, text};
use rune_kit::Kit;

use crate::glue::{self, SessionView, Ui};
use crate::view::{choices, items, requests, tiles};

/// The height of one line of the list.
pub const ROW: f32 = 22.0;

/// How many steps an open list draws at once; a line says how many more there are.
pub const ROWS_MAX: usize = 6;

/// A tile shorter than this starts with its list folded.
pub const FOLD_BELOW: f32 = 480.0;

/// The space between the list and what is under it.
pub const GAP_BELOW: f32 = 8.0;

/// The size of a step's mark and of the head's triangle.
const MARK: f32 = 14.0;

/// The space between a mark and its words.
const MARK_GAP: f32 = 8.0;

/// The key of the folded line's mark, the same whichever step is current, so it lands at once
/// as the work moves on, as an open step's mark does ([`look`]).
const FOLDED_MARK: &str = "mr-tasks-mark";

/// The task list of `key`'s session as drawn, keyed `mr-tasks-{n}`: open, a head line keyed
/// `mr-tasks-head` with the count of steps done, the steps in sight ([`window`]) and a line for
/// the rest; folded, the head line alone, saying where the work is ([`folded_words`]). A press
/// on the head line opens or folds it. None when [`listed`] gives no list.
pub fn strip(ui: &Ui, kit: &Kit, key: SessionKey) -> Option<El> {
    let entry = ui.app.sessions.get(key)?;
    let list = listed(ui, key)?;
    let open = is_open(glue::view_of(ui, key), tiles::tile_height(ui, key));
    let working = matches!(entry.session.phase, Phase::Working | Phase::NeedsUser);
    let steps = list.steps();
    let current = list.current();
    let label = format!("Tasks, {} of {} done", list.done(), steps.len());
    let strip = col()
        .key(format!("mr-tasks-{}", key.0))
        .role("list")
        .label(label)
        .w_full()
        .pb(GAP_BELOW);
    if !open {
        let (mark, ink) = look(current.map(|i| &steps[i]), working, kit);
        let words = folded_words(list, working);
        let line = head(key, false, words.clone(), kit)
            .child(mark.key(FOLDED_MARK).a11y_hidden())
            .child(words_text(words, ink, kit));
        return Some(strip.child(line));
    }
    let count = format!("{} of {}", list.done(), steps.len());
    let line = head(key, true, "Tasks".to_owned(), kit)
        .child(
            text("Tasks")
                .font(kit.ty.small_medium)
                .color(kit.c.text_muted)
                .a11y_hidden(),
        )
        .child(
            text(count)
                .font(kit.ty.small)
                .color(kit.c.text_faint)
                .a11y_hidden(),
        );
    let shown = window(steps.len(), current, ROWS_MAX);
    let left = steps.len() - shown.len();
    let lines = shown.map(|i| step_line(i, &steps[i], working && current == Some(i), kit));
    let more = (left > 0).then(|| {
        row()
            .key("mr-tasks-more")
            .w_full()
            .h(ROW)
            .pl(MARK + MARK_GAP)
            .center_y()
            .child(
                text(format!("{left} more"))
                    .font(kit.ty.small)
                    .color(kit.c.text_faint),
            )
    });
    Some(strip.child(line).children(lines).children(more))
}

/// The task list of `key`'s session when one is drawn: not empty, not of a deleted session, in a
/// tile whose height is known, and not in a tile shorter than [`FOLD_BELOW`] while a choice box
/// shows there: the two do not fit such a tile together, and the box is what waits for the
/// user. Before the tile row is first laid out its tiles' heights are not known, and a list
/// drawn then in a short tile would show open for a frame and fold.
pub fn listed(ui: &Ui, key: SessionKey) -> Option<&TaskList> {
    let entry = ui.app.sessions.get(key)?;
    let list = &entry.session.tasks;
    let height = tiles::tile_height(ui, key)?;
    let crowded = height < FOLD_BELOW && choices::height(ui, key) > 0.0;
    // A deleted session is in no tile, so its tile's height already rules it out, and a delete
    // that lets its conversation go takes the list too: no test can tell this check apart. It
    // says outright what the plan asks.
    let drawn = entry.deleted.is_none() && !list.is_empty() && !crowded;
    drawn.then_some(list)
}

/// The height [`strip`] draws `list` at, open or folded, with the space under it: 0 for an
/// empty list.
pub fn height(list: &TaskList, open: bool) -> f32 {
    let len = list.steps().len();
    if len == 0 {
        return 0.0;
    }
    if !open {
        return ROW + GAP_BELOW;
    }
    let drawn = window(len, list.current(), ROWS_MAX).len();
    let more = usize::from(drawn < len);
    ROW * (1 + drawn + more) as f32 + GAP_BELOW
}

/// Whether the list of a session shown as `view` is drawn open in a tile `tile_height` high:
/// as the user chose when they did, else open unless the tile is known to be shorter than
/// [`FOLD_BELOW`].
pub fn is_open(view: &SessionView, tile_height: Option<f32>) -> bool {
    view.tasks_open
        .unwrap_or_else(|| tile_height.is_none_or(|height| height >= FOLD_BELOW))
}

/// The places of the steps an open list of `len` steps draws, at most `max` in a row: all of
/// them when they fit; else a run that holds the `current` step with one step before it where
/// there is one, kept inside the list; with no current step, the last ones.
pub fn window(len: usize, current: Option<usize>, max: usize) -> Range<usize> {
    if len <= max {
        return 0..len;
    }
    let start = current.map_or(len - max, |c| c.saturating_sub(1).min(len - max));
    start..start + max
}

/// What a folded list says: the current step's place counted from one, the number of steps,
/// and, while a turn is `working`, its active wording or else its subject, as
/// `3 of 5: Creating c.txt`; outside a turn its subject, as the open list has it. With no open
/// step, `5 of 5 done`.
pub fn folded_words(list: &TaskList, working: bool) -> String {
    let len = list.steps().len();
    match list.current().map(|i| (i, &list.steps()[i])) {
        Some((i, step)) => format!("{} of {len}: {}", i + 1, words_of(step, working)),
        None => format!("{} of {len} done", list.done()),
    }
}

/// The head line of `key`'s list, keyed `mr-tasks-head`, labelled `label` and showing that it
/// is `open` or folded with its triangle; what it holds after that is the caller's. A press
/// shows the list the other way and focuses the tile. It never takes the keyboard's focus.
fn head(key: SessionKey, open: bool, label: String, kit: &Kit) -> El {
    let arrow = if open {
        kit.glyph.triangle_down
    } else {
        kit.glyph.triangle_right
    };
    let r = row()
        .key("mr-tasks-head")
        .role("button")
        .label(label)
        .expanded(open)
        .w_full()
        .h(ROW)
        .gap(MARK_GAP)
        .center_y()
        .radius(kit.radius.md)
        .cursor(Cursor::Pointer)
        .child(icon(arrow, MARK).color(kit.c.icon_weaker).a11y_hidden())
        .on_press(move |ui: &mut Ui| {
            glue::focus_tile(ui, key, false);
            glue::view_mut(ui, key).tasks_open = Some(!open);
        });
    requests::unfocusable(kit.interactive(r))
}

/// The line of step `i`, keyed `mr-task-{i}` with its mark keyed `mr-task-mark-{i}`: the mark,
/// then the words on one line, cut with an ellipsis. With `spinning`, the step is the current
/// one of a turn under way and shows its active wording.
fn step_line(i: usize, step: &TaskStep, spinning: bool, kit: &Kit) -> El {
    let (mark, ink) = look(Some(step), spinning, kit);
    let words = words_of(step, spinning);
    row()
        .key(format!("mr-task-{i}"))
        .role("listitem")
        .label(format!("{words}, {}", status_words(step.status)))
        .w_full()
        .h(ROW)
        .gap(MARK_GAP)
        .center_y()
        .child(mark.key(mark_key(i)).a11y_hidden())
        .child(words_text(words.to_owned(), ink, kit))
}

/// The mark of `step`, and the colour of its words: a done step's tick in the success colour
/// and faint words; with `spinning`, the tile's spinner and the base text colour; else a ring
/// and muted words. A folded list with no open step shows a tick too.
///
/// A step's mark keeps its key as it changes from one look to another, so each lands at once:
/// its corners and colours (a ring would otherwise round off from the spinner's square), and
/// the spinner's dots it gains or loses (they would otherwise fade over the tick).
fn look(step: Option<&TaskStep>, spinning: bool, kit: &Kit) -> (El, Color) {
    let (mark, ink) = match step {
        None => done_look(kit),
        Some(step) if step.status == TaskStatus::Completed => done_look(kit),
        Some(_) if spinning => (items::spinner(kit), kit.c.text_base),
        Some(_) => (ring(kit), kit.c.text_muted),
    };
    (mark.motion(Spring::NONE).instant_changes(), ink)
}

/// A done step's tick and the colour of its words.
fn done_look(kit: &Kit) -> (El, Color) {
    let tick = icon(kit.glyph.check, MARK).color(kit.c.success.fg);
    (tick, kit.c.text_faint)
}

/// The mark of a step not done, or not under way: an empty circle.
fn ring(kit: &Kit) -> El {
    el().size(MARK, MARK)
        .radius(MARK / 2.0)
        .border(1.0, kit.c.icon_muted)
}

/// A step's words, or a folded list's, on one line cut with an ellipsis.
fn words_text(words: String, ink: Color, kit: &Kit) -> El {
    text(words)
        .font(kit.ty.ui)
        .color(ink)
        .truncate()
        .shrink()
        .a11y_hidden()
}

/// What a step says: while a turn is `working` on it, its active wording or else its subject;
/// otherwise its subject.
fn words_of(step: &TaskStep, working: bool) -> &str {
    match &step.active {
        Some(active) if working => active,
        _ => &step.subject,
    }
}

/// What a screen reader says of a step after its words.
fn status_words(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Completed => "Done",
        TaskStatus::InProgress => "In progress",
        TaskStatus::Pending => "Not started",
    }
}

/// The key of the mark of step `i`.
fn mark_key(i: usize) -> String {
    format!("mr-task-mark-{i}")
}

#[cfg(test)]
mod tests {
    use mr_agent::{SessionEvent, TaskChange, TaskStatus, TaskStep, TurnSummary};
    use rune::Runtime;

    use super::*;
    use crate::glue::SessionView;
    use crate::{demo, testkit};

    fn task(id: &str, subject: &str, active: Option<&str>, status: TaskStatus) -> TaskStep {
        TaskStep {
            id: id.into(),
            subject: subject.into(),
            active: active.map(Into::into),
            status,
        }
    }

    /// The `tasks` scene's five steps, two done and the third in progress with `active` as its
    /// wording.
    fn five(active: Option<&str>) -> TaskList {
        TaskList::from_steps(vec![
            task(
                "1",
                "Measure the current banner",
                None,
                TaskStatus::Completed,
            ),
            task("2", "Resize the image", None, TaskStatus::Completed),
            task("3", "Update the stylesheet", active, TaskStatus::InProgress),
            task("4", "Check the page on a phone", None, TaskStatus::Pending),
            task("5", "Write the change note", None, TaskStatus::Pending),
        ])
    }

    #[test]
    fn the_window_keeps_the_current_step_in_sight() {
        assert_eq!(window(5, Some(2), 6), 0..5);
        assert_eq!(window(10, Some(0), 6), 0..6);
        assert_eq!(
            window(10, Some(4), 6),
            3..9,
            "one step before the current one"
        );
        assert_eq!(window(10, Some(9), 6), 4..10, "kept inside the list");
        assert_eq!(window(10, None, 6), 4..10);
        assert_eq!(window(0, None, 6), 0..0);
    }

    #[test]
    fn the_folded_line_says_where_the_work_is() {
        let list = five(Some("Updating the stylesheet"));
        assert_eq!(folded_words(&list, true), "3 of 5: Updating the stylesheet");
        assert_eq!(
            folded_words(&list, false),
            "3 of 5: Update the stylesheet",
            "nothing is being done outside a turn"
        );
        let plain = five(None);
        assert_eq!(folded_words(&plain, true), "3 of 5: Update the stylesheet");
        assert_eq!(folded_words(&plain, false), "3 of 5: Update the stylesheet");
        let mut done = five(None);
        for id in ["3", "4", "5"] {
            done.change(&TaskChange {
                id: id.into(),
                status: Some(TaskStatus::Completed),
                subject: None,
                active: None,
            });
        }
        assert_eq!(folded_words(&done, true), "5 of 5 done");
        assert_eq!(folded_words(&done, false), "5 of 5 done");
    }

    #[test]
    fn the_height_counts_what_is_drawn() {
        let five = five(None);
        let ten = TaskList::from_steps(
            (1..=10)
                .map(|n| task(&n.to_string(), "Step", None, TaskStatus::Pending))
                .collect(),
        );
        assert_eq!(height(&TaskList::default(), true), 0.0);
        assert_eq!(height(&five, false), 30.0);
        assert_eq!(height(&five, true), 6.0 * 22.0 + 8.0);
        assert_eq!(
            height(&ten, true),
            8.0 * 22.0 + 8.0,
            "the head, six steps and the line for the rest"
        );
    }

    #[test]
    fn a_short_tile_starts_folded_and_the_users_choice_wins() {
        let mut view = SessionView::new(0);
        assert!(is_open(&view, None));
        assert!(is_open(&view, Some(480.0)));
        assert!(!is_open(&view, Some(479.0)));
        view.tasks_open = Some(true);
        assert!(is_open(&view, Some(280.0)));
        view.tasks_open = Some(false);
        assert!(!is_open(&view, Some(900.0)));
    }

    /// One frame's step, in seconds, as `testkit` runs them.
    const FRAME: f64 = 0.016;

    /// Runs frames until what moves or fades in has settled.
    fn settle(rt: &mut Runtime<Ui>) {
        for _ in 0..40 {
            testkit::step(rt);
        }
    }

    /// How many elements are drawn inside the mark of step `i`, leaving and coming ones too.
    fn inside_mark(rt: &Runtime<Ui>, i: usize) -> usize {
        let inside = format!("/mr-task-mark-{i}/");
        rt.elements()
            .into_iter()
            .filter(|e| e.path.contains(&inside) && e.opacity > 0.0)
            .count()
    }

    /// The path [`testkit::tile`] gives to `inner` of the focused tile.
    fn in_focused(rt: &Runtime<Ui>, inner: &str) -> String {
        testkit::tile(testkit::focused(rt).expect("a tile is focused"), inner)
    }

    /// The drawn rectangle of `inner` in the focused tile.
    fn rect_of(rt: &Runtime<Ui>, inner: &str) -> [f32; 4] {
        testkit::rect(rt, &in_focused(rt, inner)).unwrap_or_else(|| panic!("no {inner}"))
    }

    /// Whether the focused tile shows a text that contains `part`.
    fn shows(rt: &Runtime<Ui>, part: &str) -> bool {
        testkit::tile_has_text(rt, testkit::focused(rt).unwrap(), part)
    }

    /// The keys of what is drawn of a task list in the focused tile.
    fn list_keys(rt: &Runtime<Ui>) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting)
            .filter_map(|e| e.key)
            .filter(|key| key.starts_with("mr-task"))
            .collect()
    }

    /// The number of step lines drawn.
    fn step_lines(rt: &Runtime<Ui>) -> usize {
        list_keys(rt)
            .iter()
            .filter(|key| {
                key.strip_prefix("mr-task-")
                    .is_some_and(|n| n.parse::<usize>().is_ok())
            })
            .count()
    }

    fn change(id: &str, status: TaskStatus) -> SessionEvent {
        SessionEvent::TaskChanged(TaskChange {
            id: id.into(),
            status: Some(status),
            subject: None,
            active: None,
        })
    }

    fn turn_finished(interrupted: bool) -> SessionEvent {
        SessionEvent::TurnFinished(TurnSummary {
            cost_usd: 0.0,
            context_used: None,
            duration_ms: 1,
            interrupted,
        })
    }

    #[test]
    fn the_tasks_scene_shows_the_list_above_the_box() {
        let mut rt = testkit::window("tasks");
        // Drawn from the second frame, once the tile's height is known, the list enters as
        // anything new above the box does.
        settle(&mut rt);
        let key = testkit::focused(&rt).unwrap();
        for words in [
            "Tasks",
            "2 of 5",
            "Measure the current banner",
            "Updating the stylesheet",
            "Write the change note",
        ] {
            assert!(shows(&rt, words), "{words:?} shows");
        }
        assert!(
            !shows(&rt, "Update the stylesheet"),
            "the current step shows its active wording"
        );
        let list = rect_of(&rt, &format!("mr-tasks-{}", key.0));
        let boxed = rect_of(&rt, "mr-input-box");
        assert!(
            list[1] + list[3] <= boxed[1] + 0.5,
            "{list:?} over {boxed:?}"
        );
        assert!((list[0] - boxed[0]).abs() < 0.5 && (list[2] - boxed[2]).abs() < 0.5);
        let tasks = &rt.state.focused_session().unwrap().tasks;
        assert_eq!(list[3], height(tasks, true), "as high as counted");
        let mark = testkit::element(&rt, &in_focused(&rt, "mr-task-mark-2")).unwrap();
        assert_eq!(mark.role, "progressbar", "the spinner");
        for name in ["TaskCreate", "TaskUpdate", "ToolSearch"] {
            assert!(!testkit::has_text(&rt, name), "no tool line names {name}");
        }
    }

    #[test]
    fn a_press_on_the_head_folds_and_opens_the_list() {
        let mut rt = testkit::window("tasks");
        let head = in_focused(&rt, "mr-tasks-head");
        testkit::click(&mut rt, &head);
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).tasks_open, Some(false));
        assert!(shows(&rt, "3 of 5: Updating the stylesheet"));
        assert!(!shows(&rt, "Write the change note"));
        testkit::click(&mut rt, &head);
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).tasks_open, Some(true));
        assert!(shows(&rt, "Write the change note"));
        assert!(!shows(&rt, "3 of 5: Updating the stylesheet"));
        testkit::click(&mut rt, &head);
        testkit::emit(&mut rt, turn_finished(true));
        assert!(
            shows(&rt, "3 of 5: Update the stylesheet"),
            "a stopped turn"
        );
    }

    #[test]
    fn the_folded_scene_starts_folded_in_its_short_tile() {
        let rt = testkit::window("tasks-folded");
        let key = testkit::focused(&rt).unwrap();
        let columns = testkit::columns(&rt);
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].len(), 2);
        assert_eq!(columns[0][0], key, "the upper tile");
        assert_eq!(testkit::shown_view(&rt).tasks_open, None);
        assert!(shows(&rt, "3 of 5: Updating the stylesheet"));
        let list = rect_of(&rt, &format!("mr-tasks-{}", key.0));
        assert_eq!(list[3], ROW + GAP_BELOW, "as high as counted");
        for words in ["Measure the current banner", "Write the change note"] {
            assert!(!shows(&rt, words), "{words:?} is folded away");
        }
    }

    #[test]
    fn outside_a_turn_the_current_step_has_no_spinner() {
        let mut rt = testkit::window("tasks");
        let key = testkit::focused(&rt).unwrap();
        demo::emit_to(&rt.state, key, turn_finished(true));
        rt.frame(FRAME);
        assert_eq!(
            rt.state.focused_session().unwrap().phase,
            mr_core::Phase::Idle
        );
        assert!(shows(&rt, "Update the stylesheet"), "the list stays");
        let mark = testkit::element(&rt, &in_focused(&rt, "mr-task-mark-2")).unwrap();
        assert_ne!(mark.role, "progressbar");
        assert_eq!(
            mark.radius, 7.0,
            "a ring in the first frame, never a square"
        );
        assert_eq!(inside_mark(&rt, 2), 0, "no dot of the spinner is left");
    }

    #[test]
    fn a_step_that_changes_its_mark_shows_the_new_one_at_once() {
        let mut rt = testkit::window("tasks");
        let key = testkit::focused(&rt).unwrap();
        // The third step is done and the fourth one is under way: a spinner gives way to a
        // tick, and a ring to a spinner.
        demo::emit_to(&rt.state, key, change("3", TaskStatus::Completed));
        rt.frame(FRAME);
        let mark = |i: usize| {
            testkit::element(&rt, &in_focused(&rt, &format!("mr-task-mark-{i}"))).unwrap()
        };
        assert_eq!(mark(2).role, "icon", "the tick");
        assert_eq!(
            inside_mark(&rt, 2),
            0,
            "no dot of the spinner fades over it"
        );
        assert_eq!(mark(3).role, "progressbar");
        assert_eq!(mark(3).radius, 0.0, "no ring left round it");
        assert!(inside_mark(&rt, 3) > 0);
    }

    #[test]
    fn the_folded_mark_keeps_one_key_as_the_work_moves_on() {
        let mut rt = testkit::window("tasks-folded");
        settle(&mut rt);
        let key = testkit::focused(&rt).unwrap();
        // Whatever is drawn as a mark in the focused tile, leaving ones too.
        let tile = format!("/{}/", crate::view::tile_key(key));
        let marks = |rt: &Runtime<Ui>| {
            rt.elements()
                .into_iter()
                .filter(|e| e.path.contains(&tile))
                .filter(|e| e.key.as_deref().is_some_and(|k| k.contains("mark")))
                .map(|e| (e.key.unwrap(), e.exiting, e.role))
                .collect::<Vec<_>>()
        };
        demo::emit_to(&rt.state, key, change("3", TaskStatus::Completed));
        demo::emit_to(&rt.state, key, change("4", TaskStatus::InProgress));
        rt.frame(FRAME);
        assert!(shows(&rt, "4 of 5"));
        assert_eq!(
            marks(&rt),
            [(FOLDED_MARK.to_string(), false, "progressbar")],
            "the same mark, at once"
        );
        demo::emit_to(&rt.state, key, change("4", TaskStatus::Completed));
        demo::emit_to(&rt.state, key, change("5", TaskStatus::Completed));
        rt.frame(FRAME);
        assert!(shows(&rt, "5 of 5 done"));
        assert_eq!(
            marks(&rt),
            [(FOLDED_MARK.to_string(), false, "icon")],
            "the tick lands at once"
        );
        let inside = format!("/{FOLDED_MARK}/");
        assert!(
            !rt.elements()
                .iter()
                .any(|e| e.path.contains(&inside) && e.opacity > 0.0),
            "no dot of the spinner fades over it"
        );
    }

    #[test]
    fn a_tile_whose_height_is_not_known_yet_draws_no_list() {
        let mut ui = demo::ui();
        let (_, scene) = demo::SCENES
            .iter()
            .find(|(name, _)| *name == "tasks-folded")
            .unwrap();
        scene(&mut ui);
        let mut rt = testkit::runtime(ui, (1200.0, 800.0));
        rt.frame(FRAME);
        assert_eq!(step_lines(&rt), 0, "never the step lines");
        let folded = !list_keys(&rt).is_empty();
        assert!(!folded || shows(&rt, "3 of 5: Updating the stylesheet"));
        rt.frame(FRAME);
        assert_eq!(step_lines(&rt), 0);
        assert!(shows(&rt, "3 of 5: Updating the stylesheet"), "folded");
    }

    #[test]
    fn the_list_goes_when_its_turn_ends_ticked() {
        let mut rt = testkit::window("tasks");
        let head = in_focused(&rt, "mr-tasks-head");
        testkit::click(&mut rt, &head);
        assert_eq!(testkit::shown_view(&rt).tasks_open, Some(false));
        for id in ["3", "4", "5"] {
            testkit::emit(&mut rt, change(id, TaskStatus::Completed));
        }
        assert!(!list_keys(&rt).is_empty(), "kept until the turn ends");
        testkit::emit(&mut rt, turn_finished(false));
        testkit::step(&mut rt);
        assert_eq!(list_keys(&rt), Vec::<String>::new());
        assert_eq!(testkit::shown_view(&rt).tasks_open, None);
    }

    #[test]
    fn a_hundred_steps_draw_six_and_long_words_stay_on_one_line() {
        let mut rt = testkit::window("tasks");
        let key = testkit::focused(&rt).unwrap();
        let long: String = "word ".repeat(60).chars().take(299).chain(['s']).collect();
        assert_eq!(long.chars().count(), 300);
        for n in 6..=100 {
            let subject = if n == 50 {
                long.clone()
            } else {
                format!("Step {n}")
            };
            let step = task(&n.to_string(), &subject, None, TaskStatus::Pending);
            demo::emit_to(&rt.state, key, SessionEvent::TaskAdded(step));
        }
        demo::emit_to(&rt.state, key, change("3", TaskStatus::Completed));
        demo::emit_to(&rt.state, key, change("50", TaskStatus::InProgress));
        testkit::step(&mut rt);
        let tasks = &rt.state.focused_session().unwrap().tasks;
        assert_eq!((tasks.steps().len(), tasks.current()), (100, Some(49)));
        assert_eq!(step_lines(&rt), 6);
        assert!(shows(&rt, "94 more"));
        assert_eq!(rect_of(&rt, "mr-task-49")[3], ROW);
        let words = rt
            .elements()
            .into_iter()
            .find(|e| !e.exiting && e.text.as_deref() == Some(long.as_str()))
            .expect("the long step's words");
        assert!(words.rect[3] <= ROW, "one line: {:?}", words.rect);
    }
}
