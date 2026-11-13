//! Turns a probe recording into a test fixture. Works on the text of each
//! line, never on parsed JSON, so key order and spacing are untouched.

use std::collections::HashMap;

const FIXTURE_HOME: &str = "/home/user";
const FIXTURE_EMAIL: &str = "user@example.com";
const UUID_LEN: usize = 36;

/// Keeps the lines `claude` wrote (those the probe recorded with `< `), minus
/// hook noise, and removes what identifies the person or the machine: each
/// distinct UUID becomes a numbered placeholder in order of first appearance,
/// e-mail addresses become `user@example.com`, and the home directory `home`
/// becomes `/home/user`.
pub fn scrub_lines(input: &str, home: &str) -> String {
    let mut uuids: HashMap<String, usize> = HashMap::new();
    let mut out = String::new();
    for line in input.lines() {
        let Some(body) = line.strip_prefix("< ") else {
            continue;
        };
        if is_hook_record(body) {
            continue;
        }
        let body = replace_home(body, home);
        let body = replace_emails(&body);
        out.push_str(&replace_uuids(&body, &mut uuids));
        out.push('\n');
    }
    out
}

fn is_hook_record(line: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    value["type"] == "system"
        && value["subtype"]
            .as_str()
            .is_some_and(|subtype| subtype.starts_with("hook_"))
}

fn replace_home(line: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find(home) {
        let after = &rest[at + home.len()..];
        // `/home/alex` must not eat the start of `/home/alexx`.
        let whole = !continues_name(after);
        out.push_str(&rest[..at]);
        out.push_str(if whole { FIXTURE_HOME } else { home });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Whether `after` carries on the directory name, as in `/home/alexx` or
/// `/home/alex.bak`. A period that ends a sentence does not.
fn continues_name(after: &str) -> bool {
    let mut chars = after.chars();
    match chars.next() {
        Some(c) if c.is_alphanumeric() || matches!(c, '_' | '-') => true,
        Some('.') => chars.next().is_some_and(char::is_alphanumeric),
        _ => false,
    }
}

fn replace_emails(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut copied = 0;
    let mut search = 0;
    while let Some(offset) = line[search..].find('@') {
        let at = search + offset;
        search = at + 1;
        let mut start = at;
        while start > copied && is_local_byte(bytes[start - 1]) {
            start -= 1;
        }
        // In JSON text `\nalex@x.com` has the escape letter `n` glued to the name.
        if start > copied
            && bytes[start - 1] == b'\\'
            && start < at
            && is_escape_letter(bytes[start])
        {
            start += 1;
        }
        let mut end = at + 1;
        while end < bytes.len() && is_domain_byte(bytes[end]) {
            end += 1;
        }
        while end > at + 1 && matches!(bytes[end - 1], b'.' | b'-') {
            end -= 1;
        }
        let domain = &line[at + 1..end];
        let has_tld = domain
            .rsplit_once('.')
            .is_some_and(|(host, tld)| !host.is_empty() && tld.len() >= 2);
        if start < at && has_tld {
            out.push_str(&line[copied..start]);
            out.push_str(FIXTURE_EMAIL);
            copied = end;
            search = end;
        }
    }
    out.push_str(&line[copied..]);
    out
}

fn is_local_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_domain_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')
}

fn replace_uuids(line: &str, seen: &mut HashMap<String, usize>) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut copied = 0;
    let mut i = 0;
    while i + UUID_LEN <= bytes.len() {
        let candidate = &bytes[i..i + UUID_LEN];
        let bounded = starts_token(bytes, i)
            && bytes
                .get(i + UUID_LEN)
                .is_none_or(|b| !b.is_ascii_alphanumeric());
        if bounded && looks_like_uuid(candidate) {
            let key = line[i..i + UUID_LEN].to_ascii_lowercase();
            let next = seen.len() + 1;
            let number = *seen.entry(key).or_insert(next);
            out.push_str(&line[copied..i]);
            out.push_str(&format!("00000000-0000-4000-8000-{number:012}"));
            i += UUID_LEN;
            copied = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&line[copied..]);
    out
}

/// Whether a token may start at `i`: not glued to a letter or digit, except the
/// letter of a JSON escape such as `\n` or `\t`.
fn starts_token(bytes: &[u8], i: usize) -> bool {
    match i.checked_sub(1).map(|before| bytes[before]) {
        None => true,
        Some(b) if !b.is_ascii_alphanumeric() => true,
        Some(b) => i >= 2 && bytes[i - 2] == b'\\' && is_escape_letter(b),
    }
}

fn is_escape_letter(b: u8) -> bool {
    matches!(b, b'n' | b't' | b'r' | b'b' | b'f')
}

fn looks_like_uuid(bytes: &[u8]) -> bool {
    bytes.iter().enumerate().all(|(i, b)| match i {
        8 | 13 | 18 | 23 => *b == b'-',
        _ => b.is_ascii_hexdigit(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_becomes_a_fixture() {
        let rec = concat!(
            "> {\"type\":\"user\"}\n",
            "< {\"type\":\"system\",\"subtype\":\"hook_started\",\"uuid\":\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\"}\n",
            "< {\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb\",\"cwd\":\"/home/alex/x\"}\n",
            "< {\"type\":\"result\",\"session_id\":\"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb\",\"who\":\"alex@example.org\"}\n"
        );
        assert_eq!(
            scrub_lines(rec, "/home/alex"),
            concat!(
                "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"00000000-0000-4000-8000-000000000001\",\"cwd\":\"/home/user/x\"}\n",
                "{\"type\":\"result\",\"session_id\":\"00000000-0000-4000-8000-000000000001\",\"who\":\"user@example.com\"}\n"
            )
        );
    }

    #[test]
    fn the_same_uuid_in_other_case_is_the_same_placeholder_and_others_count_on() {
        let rec = "< {\"a\":\"AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA\",\"b\":\"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb\",\"c\":\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\"}\n";
        assert_eq!(
            scrub_lines(rec, ""),
            "{\"a\":\"00000000-0000-4000-8000-000000000001\",\"b\":\"00000000-0000-4000-8000-000000000002\",\"c\":\"00000000-0000-4000-8000-000000000001\"}\n"
        );
    }

    #[test]
    fn the_home_directory_is_replaced_only_as_a_whole_path_start() {
        let rec = "< {\"a\":\"/home/alex\",\"b\":\"/home/alexx/y\",\"c\":\"/home/alex/\"}\n";
        assert_eq!(
            scrub_lines(rec, "/home/alex/"),
            "{\"a\":\"/home/user\",\"b\":\"/home/alexx/y\",\"c\":\"/home/user/\"}\n"
        );
    }

    #[test]
    fn emails_are_found_in_prose_and_after_json_escapes() {
        let rec = "< {\"t\":\"mail a.b+c@mail.example.org, or\\nme@x.io.\"}\n";
        assert_eq!(
            scrub_lines(rec, ""),
            "{\"t\":\"mail user@example.com, or\\nuser@example.com.\"}\n"
        );
    }

    #[test]
    fn an_at_sign_that_is_not_an_email_stays() {
        let rec = "< {\"t\":\"@mention and a@b and x @ y.com\"}\n";
        assert_eq!(scrub_lines(rec, ""), &rec[2..]);
    }

    #[test]
    fn lines_without_the_incoming_prefix_are_dropped() {
        assert_eq!(
            scrub_lines("> {\"a\":1}\nnoise\n< {\"b\":2}\n", ""),
            "{\"b\":2}\n"
        );
    }

    #[test]
    fn a_uuid_after_a_json_escape_or_at_the_start_of_a_string_is_replaced() {
        let rec = r#"< {"t":"files:\n3b241101-e2bb-4255-8caf-4136c566a962.jsonl\n\tbbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb","u":"3b241101-e2bb-4255-8caf-4136c566a962"}"#;
        assert_eq!(
            scrub_lines(&format!("{rec}\n"), ""),
            concat!(
                r#"{"t":"files:\n00000000-0000-4000-8000-000000000001.jsonl\n\t00000000-0000-4000-8000-000000000002","u":"00000000-0000-4000-8000-000000000001"}"#,
                "\n"
            )
        );
    }

    #[test]
    fn a_letter_before_a_uuid_without_a_backslash_still_blocks_it() {
        let rec = "< {\"t\":\"xaaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\"}\n";
        assert_eq!(scrub_lines(rec, ""), &rec[2..]);
    }

    #[test]
    fn the_home_directory_before_a_sentence_period_is_replaced() {
        let rec =
            "< {\"t\":\"cwd is /home/alex. Also /home/alex, and /home/alex.bak and /home/alex\"}\n";
        assert_eq!(
            scrub_lines(rec, "/home/alex"),
            "{\"t\":\"cwd is /home/user. Also /home/user, and /home/alex.bak and /home/user\"}\n"
        );
    }
}
