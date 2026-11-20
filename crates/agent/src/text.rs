//! The plain-text rules shared by a task list and an offer of choices.

/// The direction marks that can make a text read differently from what it holds.
const DIRECTION_MARKS: [char; 12] = [
    '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}',
    '\u{2069}', '\u{200E}', '\u{200F}', '\u{061C}',
];

/// Characters that break a line or show nothing, and so can hide what a text holds: the line
/// and paragraph separators, the zero-width space, the word joiner, the byte order mark and the
/// soft hyphen. The tag characters, U+E0000 to U+E007F, are hidden too ([`is_hidden`]). The
/// zero-width joiner of emoji and the no-break space are not.
const INVISIBLE: [char; 6] = [
    '\u{2028}', '\u{2029}', '\u{200B}', '\u{2060}', '\u{FEFF}', '\u{AD}',
];

/// The text with every run of whitespace, line breaks and tabs among them, joined to one space,
/// and its ends trimmed. A hidden character ([`is_hidden`]) is kept, though it may be
/// whitespace.
pub(crate) fn one_line(text: &str) -> String {
    text.split(|c: char| c.is_whitespace() && !is_hidden(c))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a character can be shown as plain text: it is no control character and not hidden.
pub(crate) fn is_plain(c: char) -> bool {
    !c.is_control() && !is_hidden(c)
}

/// Whether a character can make a text read differently from what it holds: a direction mark,
/// one of [`INVISIBLE`], or a tag character.
pub(crate) fn is_hidden(c: char) -> bool {
    DIRECTION_MARKS.contains(&c)
        || INVISIBLE.contains(&c)
        || ('\u{E0000}'..='\u{E007F}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_is_joined_and_marks_are_not_plain() {
        assert_eq!(one_line("  a\n\tb   c\r\n"), "a b c");
        assert_eq!(
            one_line("a\u{2028}b"),
            "a\u{2028}b",
            "kept, to be refused or removed"
        );
        assert!(is_plain('\u{200D}') && is_plain('\u{A0}'));
        assert!(is_plain('a') && is_plain(' ') && is_plain('é'));
        for c in [
            '\n',
            '\t',
            '\u{7}',
            '\u{202E}',
            '\u{2066}',
            '\u{200F}',
            '\u{061C}',
            '\u{2028}',
            '\u{E007F}',
        ] {
            assert!(!is_plain(c), "{:?}", c);
        }
    }
}
