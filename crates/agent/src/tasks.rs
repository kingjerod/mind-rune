//! A session's task list: the steps the agent says it is working through.

use serde::{Deserialize, Serialize};

use crate::text::{is_hidden, is_plain, one_line};

/// The most steps a list keeps.
pub const MAX_STEPS: usize = 100;
/// The most characters kept of a step's subject or of its active wording.
pub const MAX_STEP_TEXT: usize = 300;

/// Where a step stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

/// One step of a task list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskStep {
    /// The agent's own id for the step.
    pub id: String,
    /// What the step is, such as "Create a.txt".
    pub subject: String,
    /// What to say while it is in progress, such as "Creating a.txt".
    pub active: Option<String>,
    pub status: TaskStatus,
}

/// A change to one step. A part left `None` stays as it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskChange {
    pub id: String,
    pub status: Option<TaskStatus>,
    pub subject: Option<String>,
    /// An empty wording clears the step's.
    pub active: Option<String>,
}

/// The steps of a session, in the order added, held within the limits above.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskList {
    steps: Vec<TaskStep>,
}

impl TaskList {
    /// A list of the steps that `add` takes, in their order.
    pub fn from_steps(steps: Vec<TaskStep>) -> TaskList {
        let mut list = TaskList::default();
        for step in steps {
            list.add(step);
        }
        list
    }

    pub fn steps(&self) -> &[TaskStep] {
        &self.steps
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Adds a step at the end, its texts made one plain line and cut. A full list first makes
    /// room by removing its oldest completed step. Refuses, changing nothing, a step with no id
    /// or no subject, an id already in the list, and any step while the list is full of open
    /// steps.
    pub fn add(&mut self, step: TaskStep) -> bool {
        let subject = step_text(&step.subject);
        if step.id.is_empty() || subject.is_empty() || self.steps.iter().any(|s| s.id == step.id) {
            return false;
        }
        if self.steps.len() >= MAX_STEPS {
            let Some(oldest) = self
                .steps
                .iter()
                .position(|s| s.status == TaskStatus::Completed)
            else {
                return false;
            };
            self.steps.remove(oldest);
        }
        self.steps.push(TaskStep {
            id: step.id,
            subject,
            active: step.active.as_deref().and_then(wording),
            status: step.status,
        });
        true
    }

    /// Sets what the change names on its step. False for an unknown id or when nothing differs.
    pub fn change(&mut self, change: &TaskChange) -> bool {
        let Some(step) = self.steps.iter_mut().find(|s| s.id == change.id) else {
            return false;
        };
        let mut changed = false;
        if let Some(status) = change.status
            && status != step.status
        {
            step.status = status;
            changed = true;
        }
        if let Some(subject) = change.subject.as_deref().map(step_text)
            && !subject.is_empty()
            && subject != step.subject
        {
            step.subject = subject;
            changed = true;
        }
        if let Some(active) = change.active.as_deref().map(wording)
            && active != step.active
        {
            step.active = active;
            changed = true;
        }
        changed
    }

    /// Removes the step with this id. False when there is none.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.steps.len();
        self.steps.retain(|s| s.id != id);
        self.steps.len() != before
    }

    /// How many steps are completed.
    pub fn done(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.status == TaskStatus::Completed)
            .count()
    }

    /// The index of the last step in progress, else of the first one pending: new work wins
    /// over a step an earlier turn left in progress.
    pub fn current(&self) -> Option<usize> {
        let steps = &self.steps;
        steps
            .iter()
            .rposition(|s| s.status == TaskStatus::InProgress)
            .or_else(|| steps.iter().position(|s| s.status == TaskStatus::Pending))
    }

    /// At a turn's end, clears a list that has no open step left. True when it cleared one.
    pub fn turn_ended(&mut self) -> bool {
        if self.is_empty() || self.current().is_some() {
            return false;
        }
        self.steps.clear();
        true
    }
}

/// A step's text as a list keeps it: one line, with no control character and no hidden one
/// (a direction mark, an invisible character), trimmed and cut to `MAX_STEP_TEXT` characters.
pub fn step_text(text: &str) -> String {
    let plain: String = text
        .chars()
        .filter(|&c| !is_hidden(c) && (c.is_whitespace() || is_plain(c)))
        .collect();
    one_line(&plain).chars().take(MAX_STEP_TEXT).collect()
}

/// An active wording as kept: none when it is empty.
fn wording(text: &str) -> Option<String> {
    Some(step_text(text)).filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, subject: &str) -> TaskStep {
        TaskStep {
            id: id.into(),
            subject: subject.into(),
            active: None,
            status: TaskStatus::Pending,
        }
    }
    fn status(id: &str, status: TaskStatus) -> TaskChange {
        TaskChange {
            id: id.into(),
            status: Some(status),
            subject: None,
            active: None,
        }
    }

    #[test]
    fn steps_are_kept_in_the_order_added_and_an_id_only_once() {
        let mut list = TaskList::default();
        assert!(list.add(step("1", "Create a.txt")));
        assert!(list.add(step("2", "Create b.txt")));
        assert!(!list.add(step("1", "Again")), "the id is there");
        assert!(!list.add(step("3", "   ")), "no subject");
        assert!(!list.add(step("", "No id")));
        let subjects: Vec<&str> = list.steps().iter().map(|s| s.subject.as_str()).collect();
        assert_eq!(subjects, ["Create a.txt", "Create b.txt"]);
    }

    #[test]
    fn a_change_sets_only_what_it_names_and_an_unknown_step_is_ignored() {
        let mut list = TaskList::from_steps(vec![step("1", "Create a.txt")]);
        assert!(list.change(&status("1", TaskStatus::InProgress)));
        assert!(
            !list.change(&status("1", TaskStatus::InProgress)),
            "nothing differs"
        );
        assert!(!list.change(&status("9", TaskStatus::Completed)));
        let wording = TaskChange {
            id: "1".into(),
            status: None,
            subject: Some("  ".into()),
            active: Some(" Creating a.txt ".into()),
        };
        assert!(list.change(&wording));
        assert_eq!(
            list.steps()[0].subject,
            "Create a.txt",
            "an empty subject is not taken"
        );
        assert_eq!(list.steps()[0].active.as_deref(), Some("Creating a.txt"));
        assert_eq!(list.steps()[0].status, TaskStatus::InProgress);
    }

    #[test]
    fn the_current_step_is_the_last_in_progress_or_else_the_first_pending() {
        let mut list = TaskList::from_steps(vec![step("1", "a"), step("2", "b"), step("3", "c")]);
        assert_eq!(list.current(), Some(0));
        list.change(&status("1", TaskStatus::Completed));
        assert_eq!((list.current(), list.done()), (Some(1), 1));
        list.change(&status("3", TaskStatus::InProgress));
        assert_eq!(
            list.current(),
            Some(2),
            "in progress wins over an earlier pending one"
        );
    }

    #[test]
    fn a_step_left_in_progress_gives_way_to_a_later_one() {
        let mut list = TaskList::from_steps(vec![step("1", "a"), step("2", "b"), step("3", "c")]);
        list.change(&status("1", TaskStatus::InProgress));
        list.change(&status("2", TaskStatus::Completed));
        list.change(&status("3", TaskStatus::InProgress));
        assert_eq!(list.current(), Some(2), "the stale step 1 is not current");
    }

    #[test]
    fn a_turns_end_clears_only_a_list_with_nothing_open() {
        let mut list = TaskList::from_steps(vec![step("1", "a"), step("2", "b")]);
        list.change(&status("1", TaskStatus::Completed));
        assert!(!list.turn_ended());
        assert_eq!(
            list.steps().len(),
            2,
            "the ticked step stays with the open one"
        );
        list.change(&status("2", TaskStatus::Completed));
        assert!(list.turn_ended());
        assert!(list.is_empty() && list.current().is_none());
        assert!(!TaskList::default().turn_ended(), "nothing to clear");
    }

    #[test]
    fn long_texts_are_cut_and_the_list_is_bounded() {
        let long = "é".repeat(400);
        let mut list = TaskList::default();
        list.add(TaskStep {
            active: Some(long.clone()),
            ..step("1", &long)
        });
        assert_eq!(list.steps()[0].subject.chars().count(), MAX_STEP_TEXT);
        assert_eq!(
            list.steps()[0].active.as_ref().unwrap().chars().count(),
            MAX_STEP_TEXT
        );
        for n in 2..=200 {
            list.add(step(&n.to_string(), "x"));
        }
        assert_eq!(list.steps().len(), MAX_STEPS);
        assert_eq!(list.steps()[MAX_STEPS - 1].id, "100", "all open: refused");
        assert!(list.remove("1") && !list.remove("1"));
    }

    #[test]
    fn a_full_list_makes_room_by_its_oldest_ticked_step() {
        let mut list =
            TaskList::from_steps((1..=MAX_STEPS).map(|n| step(&n.to_string(), "x")).collect());
        assert!(!list.add(step("new", "x")), "every step is open");
        list.change(&status("5", TaskStatus::Completed));
        list.change(&status("9", TaskStatus::Completed));
        assert!(list.add(step("new", "x")));
        assert_eq!(list.steps().len(), MAX_STEPS);
        let ids: Vec<&str> = list.steps().iter().map(|s| s.id.as_str()).collect();
        assert!(
            !ids.contains(&"5") && ids.contains(&"9"),
            "the oldest ticked one goes"
        );
        assert_eq!(ids.last(), Some(&"new"));
        assert!(!list.add(step("9", "x")), "a known id is still refused");
        assert!(
            list.steps().iter().any(|s| s.id == "9"),
            "and makes no room"
        );
    }

    #[test]
    fn a_steps_texts_are_kept_as_one_clean_line() {
        let mut list = TaskList::default();
        list.add(TaskStep {
            active: Some(" Creating\n a.txt\u{7} ".into()),
            ..step("1", "Create\n\u{202E}a.txt\t now")
        });
        assert_eq!(list.steps()[0].subject, "Create a.txt now");
        assert_eq!(list.steps()[0].active.as_deref(), Some("Creating a.txt"));
        list.change(&TaskChange {
            id: "1".into(),
            status: None,
            subject: Some("Make\r\nb\u{200B}.txt\u{2066}\u{2028}\u{E0041}".into()),
            active: Some("\u{200F}\n".into()),
        });
        assert_eq!(list.steps()[0].subject, "Make b.txt");
        assert_eq!(list.steps()[0].active, None, "nothing plain is left of it");
    }
}
