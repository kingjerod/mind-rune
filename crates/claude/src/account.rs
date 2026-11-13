use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use mr_agent::Account;
use serde_json::Value;

use crate::env::child_env;

/// How long one run of `claude auth status` may take before it counts as logged out.
const STATUS_TIMEOUT: Duration = Duration::from_secs(10);
const TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// What `claude auth status` says about the sign-in.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusInfo {
    pub logged_in: bool,
    pub method: String,
    pub email: Option<String>,
    pub subscription: Option<String>,
}

/// Reads the JSON `claude auth status` prints. Input that is not a JSON
/// object reads as logged out.
pub fn parse_status(json: &str) -> StatusInfo {
    let value: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    StatusInfo {
        logged_in: value.get("loggedIn").and_then(Value::as_bool) == Some(true),
        method: text("authMethod").unwrap_or_default(),
        email: text("email"),
        subscription: text("subscriptionType"),
    }
}

/// The label shown for a subscription type, or `None` for one not known.
pub fn plan_label(subscription: Option<&str>) -> Option<String> {
    let label = match subscription? {
        "max" => "Max plan",
        "pro" => "Pro plan",
        "team" => "Team plan",
        "enterprise" => "Enterprise plan",
        _ => return None,
    };
    Some(label.to_string())
}

/// Finds out whether and how the user is signed in by running
/// `<program> auth status`. Blocks while the program runs (up to ten seconds
/// per run, twice at most), so call it from a worker thread.
///
/// The first run has the OAuth token removed: if it is logged in, a saved full
/// login exists. Otherwise, when the parent environment holds a token, the
/// second run keeps it: if that one is logged in, the token is the only
/// sign-in. Without a token the second run would repeat the first, so it is
/// skipped. A program that cannot be started is `Missing`.
pub fn detect(program: &Path, parent_env: Vec<(String, String)>) -> Account {
    let without_token =
        match run_status(program, child_env(parent_env.clone(), true), STATUS_TIMEOUT) {
            Some(status) => status,
            None => return Account::Missing,
        };
    if without_token.logged_in {
        return signed_in(&without_token, true);
    }
    if !parent_env.iter().any(|(key, _)| key == TOKEN) {
        return Account::SignedOut;
    }
    match run_status(program, child_env(parent_env, false), STATUS_TIMEOUT) {
        Some(status) if status.logged_in => signed_in(&status, false),
        Some(_) => Account::SignedOut,
        None => Account::Missing,
    }
}

fn signed_in(status: &StatusInfo, full_login: bool) -> Account {
    Account::SignedIn {
        name: status
            .email
            .clone()
            .unwrap_or_else(|| "Signed in".to_string()),
        plan: plan_label(status.subscription.as_deref()),
        full_login,
    }
}

/// Runs `<program> auth status` with exactly `env`. `None` when the program
/// cannot be started. A run that fails, prints nothing or outlives `timeout`
/// (it is then killed) reads as logged out.
fn run_status(program: &Path, env: Vec<(String, String)>, timeout: Duration) -> Option<StatusInfo> {
    let mut child = Command::new(program)
        .args(["auth", "status"])
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let logged_out = parse_status("");
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });

    let deadline = Instant::now() + timeout;
    let exit = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Some(logged_out);
            }
        }
    };
    if !exit.success() {
        return Some(logged_out);
    }
    let left = deadline.saturating_duration_since(Instant::now());
    Some(match receiver.recv_timeout(left) {
        Ok(bytes) => parse_status(&String::from_utf8_lossy(&bytes)),
        Err(_) => logged_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_status_outputs_seen_live() {
        let out = parse_status(
            r#"{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}"#,
        );
        assert!(!out.logged_in);
        let token = parse_status(
            r#"{"loggedIn": true, "authMethod": "oauth_token", "apiProvider": "firstParty"}"#,
        );
        assert!(token.logged_in);
        assert_eq!(token.method, "oauth_token");
        assert_eq!(token.email, None);
        assert!(!parse_status("garbage").logged_in);
    }

    #[test]
    fn plan_labels() {
        assert_eq!(plan_label(Some("max")).as_deref(), Some("Max plan"));
        assert_eq!(plan_label(Some("pro")).as_deref(), Some("Pro plan"));
        assert_eq!(plan_label(Some("team")).as_deref(), Some("Team plan"));
        assert_eq!(
            plan_label(Some("enterprise")).as_deref(),
            Some("Enterprise plan")
        );
        assert_eq!(plan_label(Some("mystery")), None);
        assert_eq!(plan_label(None), None);
    }

    fn detect_with(mode: &str, token: bool) -> Account {
        let mut env = vec![
            ("PATH".to_string(), std::env::var("PATH").unwrap()),
            ("STUB_AUTH".to_string(), mode.to_string()),
        ];
        if token {
            env.push(("CLAUDE_CODE_OAUTH_TOKEN".to_string(), "t".to_string()));
        }
        detect(&auth_stub(), env)
    }

    fn auth_stub() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/stub/auth_stub.sh")
    }

    #[test]
    fn the_status_check_does_not_see_a_parent_sessions_variables() {
        let parent = vec![
            ("PATH".to_string(), std::env::var("PATH").unwrap()),
            ("STUB_AUTH".to_string(), "leak".to_string()),
            ("CLAUDECODE".to_string(), "1".to_string()),
            ("ANTHROPIC_CANARY".to_string(), "1".to_string()),
        ];
        assert!(matches!(
            detect(&auth_stub(), parent),
            Account::SignedIn {
                full_login: true,
                ..
            }
        ));
    }

    #[test]
    fn a_status_check_that_hangs_is_killed_and_reads_as_logged_out() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let env = vec![
            ("PATH".to_string(), std::env::var("PATH").unwrap()),
            ("STUB_AUTH".to_string(), "sleep".to_string()),
            ("STUB_PID_FILE".to_string(), pid_file.display().to_string()),
        ];
        let began = Instant::now();
        let status = run_status(&auth_stub(), env, Duration::from_millis(300));
        assert!(began.elapsed() < Duration::from_secs(5));
        assert_eq!(status, Some(parse_status("")));
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let alive = Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(!alive.success(), "the stub {} was not killed", pid.trim());
    }

    fn runs_counted(token: bool) -> usize {
        let dir = tempfile::tempdir().unwrap();
        let count_file = dir.path().join("count");
        let mut env = vec![
            ("PATH".to_string(), std::env::var("PATH").unwrap()),
            ("STUB_AUTH".to_string(), "count".to_string()),
            (
                "STUB_COUNT_FILE".to_string(),
                count_file.display().to_string(),
            ),
        ];
        if token {
            env.push(("CLAUDE_CODE_OAUTH_TOKEN".to_string(), "t".to_string()));
        }
        assert_eq!(detect(&auth_stub(), env), Account::SignedOut);
        std::fs::read_to_string(&count_file)
            .unwrap()
            .lines()
            .count()
    }

    #[test]
    fn the_second_check_runs_only_when_there_is_a_token_to_try() {
        assert_eq!(runs_counted(false), 1);
        assert_eq!(runs_counted(true), 2);
    }

    #[test]
    fn a_saved_login_is_a_full_login() {
        assert_eq!(
            detect_with("full", true),
            Account::SignedIn {
                name: "user@example.com".into(),
                plan: Some("Max plan".into()),
                full_login: true
            }
        );
    }

    #[test]
    fn a_token_alone_signs_in_without_a_full_login() {
        assert_eq!(
            detect_with("token", true),
            Account::SignedIn {
                name: "Signed in".into(),
                plan: None,
                full_login: false
            }
        );
        assert_eq!(detect_with("token", false), Account::SignedOut);
    }

    #[test]
    fn nothing_is_signed_out_and_no_program_is_missing() {
        assert_eq!(detect_with("none", true), Account::SignedOut);
        assert_eq!(
            detect(Path::new("/nonexistent/claude"), vec![]),
            Account::Missing
        );
    }

    #[test]
    fn a_program_that_prints_nothing_or_fails_is_signed_out() {
        // `true` exits 0 with no output; `false` exits 1.
        assert_eq!(
            detect(Path::new("/usr/bin/true"), vec![]),
            Account::SignedOut
        );
        assert_eq!(
            detect(Path::new("/usr/bin/false"), vec![]),
            Account::SignedOut
        );
    }
}
