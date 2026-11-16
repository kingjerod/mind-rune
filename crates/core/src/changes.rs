//! The files a conversation's edits changed, counted as each edit finishes.

use std::collections::HashMap;

use mr_agent::{ToolDetail, ToolOutcome};

use crate::tools::{ToolCall, line_counts};

/// One file a conversation changed, with the lines its edits added and removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub added: usize,
    pub removed: usize,
}

/// The files a conversation's edits changed, in the order each was first changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    files: Vec<FileChange>,
    /// The counted calls behind each of `files`, at the same place.
    calls: Vec<usize>,
    /// Where each path is in `files`.
    at: HashMap<String, usize>,
}

impl Changes {
    pub fn files(&self) -> &[FileChange] {
        &self.files
    }

    /// Lines added and removed over all files.
    pub fn totals(&self) -> (usize, usize) {
        self.files.iter().fold((0, 0), |(added, removed), file| {
            (added + file.added, removed + file.removed)
        })
    }

    /// Counts `call` when it is an edit or a write that succeeded.
    pub(crate) fn add(&mut self, call: &ToolCall) {
        if counts(call)
            && let Some((path, added, removed)) = lines(&call.detail)
        {
            self.count(path, added, removed, 1);
        }
    }

    /// Takes back what [`Changes::add`] counted for `call`, whatever its outcome is now. A file
    /// left with no counted call leaves the list.
    pub(crate) fn take_back(&mut self, call: &ToolCall) {
        let Some((path, added, removed)) = lines(&call.detail) else {
            return;
        };
        let Some(&at) = self.at.get(path) else {
            return;
        };
        self.calls[at] -= 1;
        if self.calls[at] > 0 {
            let file = &mut self.files[at];
            file.added = file.added.saturating_sub(added);
            file.removed = file.removed.saturating_sub(removed);
            return;
        }
        self.files.remove(at);
        self.calls.remove(at);
        self.at.remove(path);
        for later in self.at.values_mut().filter(|later| **later > at) {
            *later -= 1;
        }
    }

    /// These changes with `later`'s after them: counts of a path both have add up.
    pub(crate) fn then(mut self, later: Changes) -> Changes {
        for (file, calls) in later.files.into_iter().zip(later.calls) {
            self.count(&file.path, file.added, file.removed, calls);
        }
        self
    }

    fn count(&mut self, path: &str, added: usize, removed: usize, calls: usize) {
        if let Some(&at) = self.at.get(path) {
            let file = &mut self.files[at];
            file.added += added;
            file.removed += removed;
            self.calls[at] += calls;
            return;
        }
        self.at.insert(path.to_string(), self.files.len());
        self.files.push(FileChange {
            path: path.to_string(),
            added,
            removed,
        });
        self.calls.push(calls);
    }
}

/// Whether `call` is one [`Changes`] counts: an edit or a write that succeeded.
pub(crate) fn counts(call: &ToolCall) -> bool {
    matches!(call.outcome, Some(ToolOutcome::Ok { .. }))
        && matches!(
            call.detail,
            ToolDetail::FileEdit { .. } | ToolDetail::FileWrite { .. }
        )
}

/// The path an edit or a write changed and the lines it added and removed, as the call's row
/// shows them.
fn lines(detail: &ToolDetail) -> Option<(&str, usize, usize)> {
    match detail {
        ToolDetail::FileEdit {
            path,
            old_text,
            new_text,
        } => {
            let (added, removed) = line_counts(old_text, new_text);
            Some((path, added, removed))
        }
        ToolDetail::FileWrite { path, content } => Some((path, content.lines().count(), 0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_agent::ToolCallId;

    fn edit(path: &str, old_text: &str, new_text: &str) -> ToolCall {
        ToolCall {
            id: ToolCallId(format!("edit {path}")),
            name: "Edit".into(),
            detail: ToolDetail::FileEdit {
                path: path.into(),
                old_text: old_text.into(),
                new_text: new_text.into(),
            },
            outcome: Some(ToolOutcome::Ok {
                output: "updated".into(),
            }),
        }
    }

    fn change(path: &str, added: usize, removed: usize) -> FileChange {
        FileChange {
            path: path.into(),
            added,
            removed,
        }
    }

    #[test]
    fn two_edits_of_one_file_add_up() {
        let mut changes = Changes::default();
        changes.add(&edit("/home/alex/app/src/main.rs", "a\nb\n", "a\nc\nd\n"));
        changes.add(&edit("/home/alex/app/src/main.rs", "x\n", ""));
        assert_eq!(
            changes.files(),
            [change("/home/alex/app/src/main.rs", 2, 2)]
        );
    }

    #[test]
    fn files_come_in_the_order_they_were_first_changed() {
        let mut changes = Changes::default();
        changes.add(&edit("/home/alex/app/src/b.rs", "", "one\n"));
        changes.add(&edit("/home/alex/app/src/a.rs", "", "one\n"));
        changes.add(&edit("/home/alex/app/src/b.rs", "one\n", "two\n"));
        let paths: Vec<_> = changes.files().iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["/home/alex/app/src/b.rs", "/home/alex/app/src/a.rs"]
        );
    }

    #[test]
    fn the_totals_add_every_file() {
        let mut changes = Changes::default();
        assert_eq!(changes.totals(), (0, 0));
        changes.add(&edit("/home/alex/app/src/a.rs", "a\nb\n", "a\nc\nd\n"));
        changes.add(&edit(
            "/home/alex/app/README.md",
            "old\nold\nold\n",
            "new\n",
        ));
        assert_eq!(changes.totals(), (3, 4));
    }

    #[test]
    fn a_call_taken_back_leaves_its_file_when_it_was_the_only_one() {
        let mut changes = Changes::default();
        let first = edit("/home/alex/app/src/a.rs", "", "one\n");
        let second = edit("/home/alex/app/src/b.rs", "", "one\ntwo\n");
        changes.add(&first);
        changes.add(&second);
        changes.add(&first);
        changes.take_back(&first);
        assert_eq!(
            changes.files(),
            [
                change("/home/alex/app/src/a.rs", 1, 0),
                change("/home/alex/app/src/b.rs", 2, 0)
            ]
        );
        changes.take_back(&first);
        assert_eq!(changes.files(), [change("/home/alex/app/src/b.rs", 2, 0)]);
        // The file left is still found by its path.
        changes.add(&second);
        assert_eq!(changes.files(), [change("/home/alex/app/src/b.rs", 4, 0)]);
    }

    #[test]
    fn later_changes_come_after_and_add_to_a_path_both_have() {
        let mut earlier = Changes::default();
        earlier.add(&edit("/home/alex/app/src/a.rs", "", "one\n"));
        let mut later = Changes::default();
        later.add(&edit("/home/alex/app/src/b.rs", "", "one\n"));
        later.add(&edit("/home/alex/app/src/a.rs", "one\n", "two\n"));
        assert_eq!(
            earlier.then(later).files(),
            [
                change("/home/alex/app/src/a.rs", 2, 1),
                change("/home/alex/app/src/b.rs", 1, 0)
            ]
        );
    }
}
