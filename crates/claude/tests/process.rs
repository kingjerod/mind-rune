use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use mr_agent::AgentError;
use mr_claude::{ClaudeProcess, ExitInfo, SpawnConfig};

const WAIT: Duration = Duration::from_secs(5);

fn run(arg: &str) -> (ClaudeProcess, Receiver<String>, Receiver<ExitInfo>) {
    let (line_tx, line_rx) = channel();
    let (exit_tx, exit_rx) = channel();
    let config = SpawnConfig {
        program: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/stub/stub.sh"),
        args: vec![arg.to_string()],
        cwd: std::env::temp_dir(),
        env: vec![
            ("PATH".to_string(), std::env::var("PATH").unwrap()),
            ("ONLY_THIS".to_string(), "1".to_string()),
        ],
    };
    let process = ClaudeProcess::spawn(
        config,
        move |line| {
            let _ = line_tx.send(line);
        },
        move |info| {
            let _ = exit_tx.send(info);
        },
    )
    .expect("the stub starts");
    (process, line_rx, exit_rx)
}

fn recv(lines: &Receiver<String>) -> String {
    lines.recv_timeout(WAIT).expect("a line within 5 seconds")
}

fn recv_exit(exit: &Receiver<ExitInfo>) -> ExitInfo {
    exit.recv_timeout(WAIT).expect("an exit within 5 seconds")
}

#[test]
fn lines_go_in_and_come_back() {
    let (p, lines, _exit) = run("echo");
    p.write_line("hello").unwrap();
    assert_eq!(recv(&lines), "got:hello");
}

#[test]
fn the_child_sees_only_the_environment_it_was_given() {
    let (_p, lines, exit) = run("env");
    recv_exit(&exit);
    let all: Vec<String> = lines.try_iter().collect();
    assert!(all.contains(&"ONLY_THIS=1".to_string()));
    // cargo sets CARGO_* in the test process, so their absence shows nothing was inherited
    assert!(!all.iter().any(|l| l.starts_with("CARGO_")));
}

#[test]
fn a_five_megabyte_line_arrives_whole() {
    let (_p, lines, _exit) = run("big");
    assert_eq!(recv(&lines).len(), 5 * 1024 * 1024);
}

#[test]
fn bytes_that_are_not_utf8_do_not_stop_the_stream() {
    let (_p, lines, _exit) = run("badutf8");
    assert!(recv(&lines).ends_with(" ok"));
    assert_eq!(recv(&lines), "second");
}

#[test]
fn a_crash_reports_its_code_and_stderr_and_closes_the_handle() {
    let (p, lines, exit) = run("crash");
    assert_eq!(recv(&lines), "before");
    let info = recv_exit(&exit);
    assert_eq!(info.code, Some(3));
    assert!(info.stderr_tail.contains("boom"));
    assert!(matches!(p.write_line("late"), Err(AgentError::Closed)));
}

#[test]
fn a_missing_program_is_not_installed() {
    let config = SpawnConfig {
        program: "/nonexistent/claude".into(),
        args: vec![],
        cwd: std::env::temp_dir(),
        env: vec![],
    };
    assert!(matches!(
        ClaudeProcess::spawn(config, |_| {}, |_| {}),
        Err(AgentError::NotInstalled)
    ));
}
