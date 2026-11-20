//! A question the agent offers the user, with a few answers to pick from.

use serde::{Deserialize, Serialize};

use crate::text::{is_plain, one_line};

/// The most choices an offer holds.
pub const MAX_CHOICES: usize = 3;
/// The most characters of a question.
pub const MAX_QUESTION: usize = 300;
/// The most characters of a choice's label.
pub const MAX_LABEL: usize = 40;
/// The most characters of a choice's message.
pub const MAX_MESSAGE: usize = 200;

/// One answer the user can pick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    /// What its button says.
    pub label: String,
    /// What is sent as the user's message when it is picked.
    pub message: String,
}

/// A question with one to three choices, the agent's recommendation first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choices {
    pub question: String,
    pub choices: Vec<Choice>,
}

impl Choices {
    /// An offer with its question and labels made one line and its messages trimmed, or the
    /// sentence that says why it cannot be shown. The sentences, in the order they are checked:
    /// `A question is needed.`; `The question is at most 300 characters.`; `One to three choices
    /// are needed.`; `Only plain text can be shown.`; `Every choice needs a label and a
    /// message.`; `A label is at most 40 characters.`; `A message is at most 200 characters.`
    pub fn new(question: &str, choices: Vec<Choice>) -> Result<Choices, String> {
        let question = one_line(question);
        if question.is_empty() {
            return Err("A question is needed.".into());
        }
        if question.chars().count() > MAX_QUESTION {
            return Err(format!(
                "The question is at most {MAX_QUESTION} characters."
            ));
        }
        if choices.is_empty() || choices.len() > MAX_CHOICES {
            return Err("One to three choices are needed.".into());
        }
        let choices: Vec<Choice> = choices
            .iter()
            .map(|c| Choice {
                label: one_line(&c.label),
                message: c.message.trim().into(),
            })
            .collect();
        let plain = |text: &str| text.chars().all(is_plain);
        if !plain(&question)
            || choices
                .iter()
                .any(|c| !plain(&c.label) || !plain(&c.message))
        {
            return Err("Only plain text can be shown.".into());
        }
        if choices
            .iter()
            .any(|c| c.label.is_empty() || c.message.is_empty())
        {
            return Err("Every choice needs a label and a message.".into());
        }
        if choices.iter().any(|c| c.label.chars().count() > MAX_LABEL) {
            return Err(format!("A label is at most {MAX_LABEL} characters."));
        }
        if choices
            .iter()
            .any(|c| c.message.chars().count() > MAX_MESSAGE)
        {
            return Err(format!("A message is at most {MAX_MESSAGE} characters."));
        }
        Ok(Choices { question, choices })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(label: &str, message: &str) -> Choice {
        Choice {
            label: label.into(),
            message: message.into(),
        }
    }

    #[test]
    fn choices_are_trimmed_and_kept_in_order() {
        let offer = Choices::new(
            " Apply now? ",
            vec![
                choice(" Apply ", " /apply "),
                choice("Not now", "Leave it for now."),
            ],
        )
        .unwrap();
        assert_eq!(offer.question, "Apply now?");
        assert_eq!(
            offer.choices,
            [
                choice("Apply", "/apply"),
                choice("Not now", "Leave it for now.")
            ]
        );
    }

    #[test]
    fn what_cannot_be_shown_is_refused_with_a_sentence() {
        let one = || vec![choice("Yes", "Yes.")];
        assert_eq!(
            Choices::new("  ", one()).unwrap_err(),
            "A question is needed."
        );
        assert_eq!(
            Choices::new(&"q".repeat(301), one()).unwrap_err(),
            "The question is at most 300 characters."
        );
        assert_eq!(
            Choices::new("Q?", vec![]).unwrap_err(),
            "One to three choices are needed."
        );
        assert_eq!(
            Choices::new("Q?", vec![choice("a", "a"); 4]).unwrap_err(),
            "One to three choices are needed."
        );
        assert_eq!(
            Choices::new("Q?", vec![choice(" ", "x")]).unwrap_err(),
            "Every choice needs a label and a message."
        );
        assert_eq!(
            Choices::new("Q?", vec![choice("x", "")]).unwrap_err(),
            "Every choice needs a label and a message."
        );
        assert_eq!(
            Choices::new("Q?", vec![choice(&"l".repeat(41), "x")]).unwrap_err(),
            "A label is at most 40 characters."
        );
        assert_eq!(
            Choices::new("Q?", vec![choice("x", &"m".repeat(201))]).unwrap_err(),
            "A message is at most 200 characters."
        );
        assert!(
            Choices::new(
                &"é".repeat(300),
                vec![choice(&"é".repeat(40), &"é".repeat(200))]
            )
            .is_ok(),
            "counted in characters"
        );
    }

    #[test]
    fn only_plain_text_can_be_shown() {
        let offer = Choices::new(
            "Apply\n the  change?\t",
            vec![choice("Apply\nnow", "Apply it.")],
        )
        .unwrap();
        assert_eq!(offer.question, "Apply the change?", "shown on one line");
        assert_eq!(offer.choices[0].label, "Apply now");
        let refused = "Only plain text can be shown.";
        assert_eq!(
            Choices::new("Q?", vec![choice("Yes", "Yes.\nAnd more.")]).unwrap_err(),
            refused,
            "a message keeps its line breaks, so it is refused"
        );
        assert_eq!(
            Choices::new("Q?", vec![choice("Yes\u{202E}", "Yes.")]).unwrap_err(),
            refused
        );
        assert_eq!(
            Choices::new("Ring\u{7}?", vec![choice("Yes", "Yes.")]).unwrap_err(),
            refused
        );
        assert_eq!(
            Choices::new("Q\u{2066}?", vec![]).unwrap_err(),
            "One to three choices are needed.",
            "checked after the count"
        );
        assert_eq!(
            Choices::new("Q?", vec![choice("\u{200E}", "")]).unwrap_err(),
            refused,
            "checked before an empty label or message"
        );
    }

    #[test]
    fn hidden_characters_are_not_plain_and_a_joined_emoji_is() {
        let refused = "Only plain text can be shown.";
        for hidden in [
            '\u{200B}',
            '\u{2028}',
            '\u{2060}',
            '\u{FEFF}',
            '\u{AD}',
            '\u{E0041}',
        ] {
            let message = format!("Yes{hidden}.");
            assert_eq!(
                Choices::new("Q?", vec![choice("Yes", &message)]).unwrap_err(),
                refused,
                "{hidden:?}"
            );
        }
        assert_eq!(
            Choices::new("Go\u{2029}on?", vec![choice("Yes", "Yes.")]).unwrap_err(),
            refused,
            "a paragraph separator is not joined as a line break is"
        );
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        let offer = Choices::new("Q?", vec![choice(&format!("{family} Yes"), "Yes.\u{A0}Go")]);
        assert_eq!(offer.unwrap().choices[0].label, format!("{family} Yes"));
    }

    #[test]
    fn the_hand_written_sentence_matches_the_limit() {
        assert_eq!(
            MAX_CHOICES, 3,
            "the sentence \"One to three choices are needed.\" names the limit: change both"
        );
    }
}
