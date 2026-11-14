//! Tool calls as lines a person can read: folding, summaries, output clipping
//! and diff line counts.

use mr_agent::{ToolCallId, ToolDetail, ToolOutcome};
use similar::{ChangeTag, TextDiff};

/// One tool call, from its start until its outcome arrives.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: String,
    pub detail: ToolDetail,
    pub outcome: Option<ToolOutcome>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolFamily {
    Read,
    Search,
    Command,
    Edit,
    Other,
}

/// Calls shown together as one line.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolGroup {
    pub family: ToolFamily,
    pub calls: Vec<ToolCall>,
}

const SEARCH_CHARS: usize = 60;
const COMMAND_CHARS: usize = 80;

impl ToolGroup {
    pub fn start(call: ToolCall) -> ToolGroup {
        ToolGroup {
            family: family(&call.detail),
            calls: vec![call],
        }
    }

    /// Adds `call` when it folds into this group; gives it back when it does not.
    // The call comes back whole so the caller can start a new group with it.
    #[allow(clippy::result_large_err)]
    pub fn try_add(&mut self, call: ToolCall) -> Result<(), ToolCall> {
        let folds = match self.family {
            ToolFamily::Read | ToolFamily::Search => family(&call.detail) == self.family,
            ToolFamily::Other => {
                family(&call.detail) == ToolFamily::Other
                    && self
                        .calls
                        .first()
                        .is_some_and(|first| first.name == call.name)
            }
            ToolFamily::Command | ToolFamily::Edit => false,
        };
        if folds {
            self.calls.push(call);
            Ok(())
        } else {
            Err(call)
        }
    }

    pub fn summary(&self) -> String {
        let Some(first) = self.calls.first() else {
            return String::new();
        };
        let n = self.calls.len();
        let several = n > 1;
        match &first.detail {
            ToolDetail::FileRead { path } => {
                if several {
                    format!("Read {n} files")
                } else {
                    format!("Read {}", file_name(path))
                }
            }
            ToolDetail::Search { query } => {
                if several {
                    format!("Searched {n} times")
                } else {
                    format!("Searched for {}", cut_chars(query, SEARCH_CHARS))
                }
            }
            ToolDetail::Command { command } => {
                let first_line = command.lines().next().unwrap_or("");
                format!("Ran {}", cut_chars(first_line, COMMAND_CHARS))
            }
            ToolDetail::FileEdit { path, .. } => format!("Edited {}", file_name(path)),
            ToolDetail::FileWrite { path, .. } => format!("Wrote {}", file_name(path)),
            ToolDetail::Other { .. } => {
                if several {
                    format!("{}, {n} times", first.name)
                } else {
                    first.name.clone()
                }
            }
        }
    }

    /// A call has no outcome yet.
    pub fn running(&self) -> bool {
        self.calls.iter().any(|call| call.outcome.is_none())
    }

    pub fn failed(&self) -> bool {
        self.calls
            .iter()
            .any(|call| matches!(call.outcome, Some(ToolOutcome::Failed { .. })))
    }
}

pub fn family(detail: &ToolDetail) -> ToolFamily {
    match detail {
        ToolDetail::FileRead { .. } => ToolFamily::Read,
        ToolDetail::Search { .. } => ToolFamily::Search,
        ToolDetail::Command { .. } => ToolFamily::Command,
        ToolDetail::FileEdit { .. } | ToolDetail::FileWrite { .. } => ToolFamily::Edit,
        ToolDetail::Other { .. } => ToolFamily::Other,
    }
}

/// The last component of a path, or the whole text when it has none.
pub fn file_name(path: &str) -> &str {
    match path.trim_end_matches('/').rsplit('/').next() {
        Some(name) if !name.is_empty() => name,
        _ => path,
    }
}

/// `text` cut to `max` characters, with `…` after it when something was cut.
fn cut_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

pub const OUTPUT_LINES: usize = 20;
pub const OUTPUT_CHARS: usize = 4000;

#[derive(Debug, PartialEq)]
pub struct Clip<'a> {
    pub shown: &'a str,
    pub cut: bool,
    pub hidden_lines: usize,
}

/// Shows at most `max_lines` lines and at most `max_chars` characters of
/// `text`, whichever ends first, always cutting on a character boundary.
/// `hidden_lines` counts the lines not shown at all.
pub fn clip(text: &str, max_lines: usize, max_chars: usize) -> Clip<'_> {
    let mut end = text.len();
    // Whether the cut falls where a line starts, so no line is partly shown.
    let mut at_line_start = false;
    if max_lines == 0 || max_chars == 0 {
        end = 0;
        at_line_start = true;
    } else {
        let (mut chars, mut lines, mut previous) = (0, 0, '\n');
        for (i, c) in text.char_indices() {
            if chars == max_chars {
                end = i;
                at_line_start = previous == '\n';
                break;
            }
            if c == '\n' {
                lines += 1;
                if lines == max_lines {
                    end = i;
                    break;
                }
            }
            chars += 1;
            previous = c;
        }
    }
    let (shown, rest) = text.split_at(end);
    // The line the cut falls in is partly shown, so the lines not shown at
    // all start after its line break (a line limit cuts at that break).
    let unseen = if at_line_start {
        rest
    } else {
        rest.split_once('\n').map_or("", |(_, after)| after)
    };
    // A trailing line break is not text that was left out.
    let cut = if at_line_start {
        !rest.is_empty()
    } else {
        !(rest.is_empty() || rest == "\n")
    };
    Clip {
        shown,
        cut,
        hidden_lines: unseen.lines().count(),
    }
}

/// Lines added and lines removed going from `old` to `new`.
pub fn line_counts(old: &str, new: &str) -> (usize, usize) {
    TextDiff::from_lines(old, new)
        .iter_all_changes()
        .fold((0, 0), |(added, removed), change| match change.tag() {
            ChangeTag::Insert => (added + 1, removed),
            ChangeTag::Delete => (added, removed + 1),
            ChangeTag::Equal => (added, removed),
        })
}

/// The lines of context a diff shows around each change.
const DIFF_CONTEXT: usize = 3;

/// What stands between two hunks of a diff, on a line of its own.
const HUNK_GAP: &str = "\u{22ef}";

/// The changed lines of the two texts with three lines of context around each change, each
/// line marked `-`, `+` or a space, without file headers.
///
/// Hunks have no `@@` line: an edit's texts are pieces of a file, not whole files, so the
/// line numbers there would count from the piece and look like positions in the file. Where
/// there is more than one hunk, a line `⋯` stands between them. Every line ends with a line
/// break, also when the texts do not end with one.
pub fn unified_diff(old: &str, new: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut out = String::new();
    for (n, hunk) in diff
        .unified_diff()
        .context_radius(DIFF_CONTEXT)
        .iter_hunks()
        .enumerate()
    {
        if n > 0 {
            out.push_str(HUNK_GAP);
            out.push('\n');
        }
        for change in hunk.iter_changes() {
            out.push_str(&change.tag().to_string());
            out.push_str(change.value());
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

/// `body` inside a Markdown code fence that `body` cannot close early.
///
/// The fence is one backtick longer than the longest run of backticks in `body`, and at
/// least three long. A line break is added after `body` when it has none.
pub fn fenced(language: &str, body: &str) -> String {
    let longest = body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    let end = if body.is_empty() || body.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    format!("{fence}{language}\n{body}{end}{fence}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, name: &str, detail: ToolDetail) -> ToolCall {
        ToolCall {
            id: ToolCallId(id.into()),
            name: name.into(),
            detail,
            outcome: None,
        }
    }
    fn read(id: &str, path: &str) -> ToolCall {
        call(id, "Read", ToolDetail::FileRead { path: path.into() })
    }

    #[test]
    fn reads_fold_and_a_command_does_not() {
        let mut group = ToolGroup::start(read("1", "/p/src/main.rs"));
        assert_eq!(group.summary(), "Read main.rs");
        assert!(group.try_add(read("2", "/p/a.rs")).is_ok());
        assert!(group.try_add(read("3", "b.rs")).is_ok());
        assert_eq!(group.summary(), "Read 3 files");
        let command = call(
            "4",
            "Bash",
            ToolDetail::Command {
                command: "cargo test\n--workspace".into(),
            },
        );
        let back = group.try_add(command.clone()).unwrap_err();
        assert_eq!(back, command);
        let mut commands = ToolGroup::start(command.clone());
        assert_eq!(commands.summary(), "Ran cargo test");
        assert!(commands.try_add(command).is_err());
    }

    #[test]
    fn other_tools_fold_only_with_the_same_name() {
        let other = |id: &str, name: &str| {
            call(
                id,
                name,
                ToolDetail::Other {
                    input_json: "{}".into(),
                },
            )
        };
        let mut group = ToolGroup::start(other("1", "WebFetch"));
        assert_eq!(group.summary(), "WebFetch");
        assert!(group.try_add(other("2", "WebFetch")).is_ok());
        assert!(group.try_add(other("3", "TodoWrite")).is_err());
        assert_eq!(group.summary(), "WebFetch, 2 times");
    }

    #[test]
    fn the_other_summaries_read_as_written() {
        let s = |name: &str, d: ToolDetail| ToolGroup::start(call("1", name, d)).summary();
        assert_eq!(
            s(
                "Grep",
                ToolDetail::Search {
                    query: "TODO".into()
                }
            ),
            "Searched for TODO"
        );
        assert_eq!(
            s(
                "Grep",
                ToolDetail::Search {
                    query: "x".repeat(61)
                }
            ),
            format!("Searched for {}…", "x".repeat(60))
        );
        assert_eq!(
            s(
                "Edit",
                ToolDetail::FileEdit {
                    path: "/p/main.rs".into(),
                    old_text: "a".into(),
                    new_text: "b".into()
                }
            ),
            "Edited main.rs"
        );
        assert_eq!(
            s(
                "Write",
                ToolDetail::FileWrite {
                    path: "notes.md".into(),
                    content: "x".into()
                }
            ),
            "Wrote notes.md"
        );
        assert_eq!(
            s(
                "Bash",
                ToolDetail::Command {
                    command: "y".repeat(81)
                }
            ),
            format!("Ran {}…", "y".repeat(80))
        );
    }

    #[test]
    fn running_and_failed_follow_the_outcomes() {
        let mut group = ToolGroup::start(read("1", "a"));
        assert!(group.running() && !group.failed());
        group.calls[0].outcome = Some(ToolOutcome::Failed {
            output: "no such file".into(),
        });
        assert!(!group.running() && group.failed());
    }

    #[test]
    fn long_output_is_cut_on_a_line_or_a_character() {
        let many: String = (0..50_000).map(|n| format!("line {n}\n")).collect();
        let c = clip(&many, OUTPUT_LINES, OUTPUT_CHARS);
        assert_eq!(c.shown.lines().count(), 20);
        assert_eq!((c.cut, c.hidden_lines), (true, 49_980));
        let wide = "é".repeat(1_000_000);
        let c = clip(&wide, OUTPUT_LINES, OUTPUT_CHARS);
        assert_eq!(c.shown.chars().count(), 4000);
        assert_eq!((c.cut, c.hidden_lines), (true, 0));
        let short = clip("one\ntwo", OUTPUT_LINES, OUTPUT_CHARS);
        assert_eq!(
            short,
            Clip {
                shown: "one\ntwo",
                cut: false,
                hidden_lines: 0
            }
        );
        assert_eq!(clip("", OUTPUT_LINES, OUTPUT_CHARS).shown, "");
    }

    #[test]
    fn a_cut_on_a_line_boundary_hides_whole_lines() {
        let c = clip("abc\nxyz\n", OUTPUT_LINES, 4);
        assert_eq!((c.shown, c.cut, c.hidden_lines), ("abc\n", true, 1));
        let c = clip("abc\nxyz\nqq\n", OUTPUT_LINES, 4);
        assert_eq!((c.shown, c.cut, c.hidden_lines), ("abc\n", true, 2));
        let c = clip("a\n\nb", 2, 100);
        assert_eq!((c.shown, c.cut, c.hidden_lines), ("a\n", true, 1));
        // Cut in the middle of a line: that line is partly shown, not hidden.
        let c = clip("abcd\nxyz\n", OUTPUT_LINES, 2);
        assert_eq!((c.shown, c.cut, c.hidden_lines), ("ab", true, 1));
        // Nothing allowed: every line is hidden.
        let c = clip("a\nb", 0, 100);
        assert_eq!((c.shown, c.cut, c.hidden_lines), ("", true, 2));
    }

    #[test]
    fn a_diff_shows_what_changed_with_context() {
        assert_eq!(
            unified_diff("a\nb\nc\n", "a\nx\nc\n"),
            " a\n-b\n+x\n c\n",
            "no file headers and no hunk header"
        );
        assert_eq!(unified_diff("same\n", "same\n"), "");
        assert_eq!(
            unified_diff("a", "b"),
            "-a\n+b\n",
            "every line ends with a break"
        );
    }

    #[test]
    fn hunks_are_parted_by_one_line_without_numbers() {
        let old: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let new = old
            .replace("line 2\n", "line two\n")
            .replace("line 18\n", "line eighteen\n");
        let diff = unified_diff(&old, &new);
        assert!(!diff.contains("@@"), "{diff}");
        assert_eq!(diff.matches("\u{22ef}\n").count(), 1, "{diff}");
        let (first, second) = diff.split_once("\u{22ef}\n").unwrap();
        assert!(first.contains("-line 2\n+line two\n") && first.ends_with(" line 5\n"));
        assert!(second.starts_with(" line 15\n") && second.contains("+line eighteen\n"));
    }

    #[test]
    fn a_fence_is_longer_than_any_run_of_backticks_inside() {
        assert_eq!(fenced("diff", "+x\n"), "```diff\n+x\n```\n");
        assert_eq!(fenced("", "no newline"), "```\nno newline\n```\n");
        let inner = "text\n```rust\nfn f() {}\n```\n````\n";
        let out = fenced("diff", inner);
        assert!(out.starts_with("`````diff\n") && out.ends_with("`````\n"));
    }

    #[test]
    fn line_counts_count_added_and_removed_lines() {
        assert_eq!(line_counts("a\nb\nc\n", "a\nx\nc\nd\n"), (2, 1));
        assert_eq!(line_counts("", "one\ntwo\n"), (2, 0));
        assert_eq!(line_counts("same\n", "same\n"), (0, 0));
    }
}
