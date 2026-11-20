//! Claude Code's task tools and the app's choice tool, whose calls are not shown as tool lines:
//! what each call does to the task list, or that it offers choices, is read from its input
//! and its result.

use mr_agent::{Choices, SessionEvent, TaskChange, TaskStatus, TaskStep, step_text};
use serde_json::Value;

use crate::choices;

/// What a call that is not shown as a tool line does.
#[derive(Debug, PartialEq)]
pub(crate) enum Hidden {
    /// Hidden, and says nothing.
    Plain,
    /// Adds a step, whose id comes with the result.
    Create {
        subject: String,
        active: Option<String>,
    },
    Change(TaskChange),
    Remove {
        id: String,
    },
    /// Offers choices: the offer read from the call's input, or none when it cannot be shown.
    /// It is announced once the call's result says it worked.
    Offer(Option<Choices>),
}

/// The task tools, which are hidden whatever they do.
const TASK_TOOLS: [&str; 4] = ["TaskCreate", "TaskUpdate", "TaskGet", "TaskList"];

/// The most characters of a step's id.
const MAX_ID: usize = 64;

/// What the call of the tool `name` with `input` is when it is not shown as a tool line;
/// `None` for an ordinary tool call.
pub(crate) fn hidden(name: &str, input: &Value) -> Option<Hidden> {
    match name {
        "TaskCreate" => Some(create(input)),
        "TaskUpdate" => Some(update(input)),
        "TaskGet" | "TaskList" => Some(Hidden::Plain),
        "ToolSearch" if loads_hidden_tools_only(input) => Some(Hidden::Plain),
        _ if name == choices::tool_name() => Some(Hidden::Offer(choices::offered(input).ok())),
        _ => None,
    }
}

/// The id of the step a `TaskCreate` made: `task.id` of the result's structured value, else
/// the id its text names in `Task #<id> created`.
pub(crate) fn created_id(structured: &Value, text: &str) -> Option<String> {
    id_of(&structured["task"]["id"]).or_else(|| {
        let (between, _) = text.strip_prefix("Task #")?.split_once(" created")?;
        id(between)
    })
}

/// What the result of a hidden call does: a call that worked adds, changes or removes a step,
/// a new step only when the result names its id, or offers the choices it carries. A failed
/// call does nothing. `structured` is the result's structured value and `text` its text, as
/// [`created_id`] reads them.
pub(crate) fn finished(
    hidden: Hidden,
    is_error: bool,
    structured: &Value,
    text: &str,
) -> Option<SessionEvent> {
    if is_error {
        return None;
    }
    match hidden {
        Hidden::Create { subject, active } => Some(SessionEvent::TaskAdded(TaskStep {
            id: created_id(structured, text)?,
            subject,
            active,
            status: TaskStatus::Pending,
        })),
        Hidden::Change(change) => Some(SessionEvent::TaskChanged(change)),
        Hidden::Remove { id } => Some(SessionEvent::TaskRemoved { id }),
        // A refused offer gives nothing: the server already told Claude why.
        Hidden::Offer(offer) => offer.map(SessionEvent::ChoicesOffered),
        Hidden::Plain => None,
    }
}

fn create(input: &Value) -> Hidden {
    match input["subject"].as_str().map(step_text) {
        Some(subject) if !subject.is_empty() => Hidden::Create {
            subject,
            active: active(input).filter(|active| !active.is_empty()),
        },
        _ => Hidden::Plain,
    }
}

fn update(input: &Value) -> Hidden {
    let Some(id) = ["taskId", "id", "task_id"]
        .into_iter()
        .find_map(|key| id_of(&input[key]))
    else {
        return Hidden::Plain;
    };
    let status = match input["status"].as_str() {
        Some("deleted") => return Hidden::Remove { id },
        Some("pending") => Some(TaskStatus::Pending),
        Some("in_progress") => Some(TaskStatus::InProgress),
        Some("completed") => Some(TaskStatus::Completed),
        _ => None,
    };
    Hidden::Change(TaskChange {
        id,
        status,
        subject: input["subject"].as_str().map(step_text),
        active: active(input),
    })
}

/// A step's active wording, from `activeForm` or, as the model may write it, `active_form`.
fn active(input: &Value) -> Option<String> {
    ["activeForm", "active_form"]
        .into_iter()
        .find_map(|key| input[key].as_str())
        .map(step_text)
}

/// Whether a `ToolSearch` only loads hidden tools: its query is `select:` and their names.
fn loads_hidden_tools_only(input: &Value) -> bool {
    let offer = choices::tool_name();
    input["query"]
        .as_str()
        .and_then(|query| query.strip_prefix("select:"))
        .is_some_and(|names| {
            names
                .split(',')
                .map(str::trim)
                .all(|name| TASK_TOOLS.contains(&name) || name == offer)
        })
}

/// An id given as a string or as a number, which is written without a fraction.
fn id_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => id(text),
        Value::Number(number) if number.is_f64() => number
            .as_f64()
            .filter(|n| n.fract() == 0.0)
            .and_then(|n| id(&format!("{n:.0}"))),
        Value::Number(number) => id(&number.to_string()),
        _ => None,
    }
}

/// An id as kept: trimmed, and none when that leaves it empty or longer than `MAX_ID`.
fn id(text: &str) -> Option<String> {
    let id = text.trim();
    (!id.is_empty() && id.chars().count() <= MAX_ID).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_task_tools_are_hidden_and_say_what_they_do() {
        assert!(
            matches!(hidden("TaskCreate", &json!({"subject":"Create a.txt","description":"x","activeForm":"Creating a.txt"})),
            Some(Hidden::Create { subject, active }) if subject == "Create a.txt" && active.as_deref() == Some("Creating a.txt"))
        );
        assert!(
            matches!(hidden("TaskCreate", &json!({"subject":"S","active_form":"Doing"})), Some(Hidden::Create { active, .. }) if active.as_deref() == Some("Doing"))
        );
        assert!(matches!(
            hidden("TaskCreate", &json!({"subject":7})),
            Some(Hidden::Plain)
        ));
        let change = |input: Value| match hidden("TaskUpdate", &input) {
            Some(Hidden::Change(c)) => c,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            change(json!({"taskId":"1","status":"in_progress"})),
            TaskChange {
                id: "1".into(),
                status: Some(TaskStatus::InProgress),
                subject: None,
                active: None
            }
        );
        assert_eq!(change(json!({"id":2,"status":"completed"})).id, "2");
        assert_eq!(
            change(json!({"task_id":"3","status":"paused","subject":"New","activeForm":"Doing"})),
            TaskChange {
                id: "3".into(),
                status: None,
                subject: Some("New".into()),
                active: Some("Doing".into())
            }
        );
        assert!(
            matches!(hidden("TaskUpdate", &json!({"taskId":"4","status":"deleted"})), Some(Hidden::Remove { id }) if id == "4")
        );
        assert!(matches!(
            hidden("TaskUpdate", &json!({"status":"completed"})),
            Some(Hidden::Plain)
        ));
        assert_eq!(
            change(json!({"taskId":" 5 ","status":"completed"})).id,
            "5",
            "an id is kept trimmed"
        );
        assert!(
            matches!(
                hidden(
                    "TaskUpdate",
                    &json!({"taskId":"x".repeat(65),"status":"completed"})
                ),
                Some(Hidden::Plain)
            ),
            "an id of 65 characters is no id"
        );
        assert!(
            matches!(
                hidden("TaskUpdate", &json!({"taskId":"   ","status":"completed"})),
                Some(Hidden::Plain)
            ),
            "an id of spaces is no id"
        );
        assert!(matches!(
            hidden("TaskList", &json!({})),
            Some(Hidden::Plain)
        ));
        assert!(matches!(
            hidden("mcp__mindrune__offer_choices", &json!({})),
            Some(Hidden::Offer(None))
        ));
        let offer = json!({"question":"Go on?","choices":[{"label":"Yes","message":"Yes."}]});
        let kept = match hidden("mcp__mindrune__offer_choices", &offer) {
            Some(Hidden::Offer(Some(choices))) => choices.question,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            kept, "Go on?",
            "an offer that can be shown is kept with its call"
        );
        assert!(hidden("Bash", &json!({"command":"ls"})).is_none());
        assert!(hidden("TodoWrite", &json!({"todos":[]})).is_none());
    }

    #[test]
    fn a_search_that_only_loads_hidden_tools_is_hidden_too() {
        let search = |query: &str| {
            hidden("ToolSearch", &json!({"query": query, "max_results": 2})).is_some()
        };
        assert!(search("select:TaskCreate,TaskUpdate"));
        assert!(search("select:mcp__mindrune__offer_choices"));
        assert!(search("select: TaskCreate , TaskList"));
        assert!(
            !search("select:TaskCreate,Bash"),
            "it loads an ordinary tool too"
        );
        assert!(!search("select:"));
        assert!(!search("task list tools"));
        assert!(hidden("ToolSearch", &json!({})).is_none());
    }

    #[test]
    fn a_new_steps_id_comes_from_the_result() {
        assert_eq!(
            created_id(
                &json!({"task":{"id":"1","subject":"Create a.txt"}}),
                "ignored"
            )
            .as_deref(),
            Some("1")
        );
        assert_eq!(
            created_id(&json!({"task":{"id":12}}), "").as_deref(),
            Some("12")
        );
        assert_eq!(
            created_id(
                &Value::Null,
                "Task #4 created successfully: Add second line to a.txt"
            )
            .as_deref(),
            Some("4")
        );
        assert_eq!(
            created_id(
                &json!([{"type":"text","text":"x"}]),
                "Task #abc created successfully: x"
            )
            .as_deref(),
            Some("abc")
        );
        assert_eq!(created_id(&Value::Null, "Updated task #1 status"), None);
        assert_eq!(created_id(&Value::Null, "Task # created"), None);
        assert_eq!(
            created_id(&json!({"task":{"id":"x".repeat(65)}}), ""),
            None,
            "an id of 65 characters is no id"
        );
        assert_eq!(
            created_id(&Value::Null, "Task #   created successfully: x"),
            None,
            "an id of spaces is no id"
        );
    }
}
