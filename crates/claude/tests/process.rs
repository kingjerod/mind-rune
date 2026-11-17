use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use mr_agent::AgentError;
use mr_claude::{ClaudeProcess, ExitInfo, SpawnConfig};

const WAIT: Duration = Duration::from_secs(5);

fn config(stub: &str, arg: &str) -> SpawnConfig {
    SpawnConfig {
        program: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/stub")
            .join(stub),
        args: vec![arg.to_string()],
        cwd: std::env::temp_dir(),
        env: vec![
            ("PATH".to_string(), std::env::var("PATH").unwrap()),
            ("ONLY_THIS".to_string(), "1".to_string()),
        ],
    }
}

fn run(arg: &str) -> (ClaudeProcess, Receiver<String>, Receiver<ExitInfo>) {
    let (line_tx, line_rx) = channel();
    let (exit_tx, exit_rx) = channel();
    let process = ClaudeProcess::spawn(
        config("stub.sh", arg),
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

#[test]
fn a_large_line_and_the_reader_do_not_lock_each_other() {
    // The stub floods its output only once this file exists, which the test makes after it has
    // queued the large line.
    let folder = tempfile::tempdir().expect("a folder for the marker");
    let marker = folder.path().join("queued");
    let (line_tx, lines) = channel();
    // The reader answers through the process, which exists only once `spawn` returns.
    let link: Arc<OnceLock<Weak<ClaudeProcess>>> = Arc::new(OnceLock::new());
    let reader_link = Arc::clone(&link);
    let process = ClaudeProcess::spawn(
        config("flood.sh", marker.to_str().expect("a UTF-8 temporary path")),
        move |line| {
            if line == "ready" {
                // As the reader answers a request it refuses.
                if let Some(process) = reader_link.get().and_then(Weak::upgrade) {
                    let _ = process.write_line("pong");
                }
            }
            let _ = line_tx.send(line);
        },
        |_| {},
    )
    .expect("the stub starts");
    let process = Arc::new(process);
    let _ = link.set(Arc::downgrade(&process));

    // A thread of its own, so the test can give up at its deadline. It holds the process, so
    // a test that gives up does not drop it while the write is blocked.
    let writer = Arc::clone(&process);
    thread::spawn(move || {
        let _ = writer.write_line(&"x".repeat(2_000_000));
        let _ = std::fs::write(&marker, "");
    });

    // This used to hang: the large write held the lock on the child's input while the stub,
    // not reading, filled its output, and the reader waited for that same lock to answer
    // "ready" instead of reading on.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut lengths = Vec::new();
    while lengths.len() < 2 {
        let left = deadline.saturating_duration_since(Instant::now());
        let line = lines
            .recv_timeout(left)
            .expect("both lengths within 10 seconds");
        if line.parse::<usize>().is_ok() {
            lengths.push(line);
        }
    }
    assert_eq!(lengths, ["2000000", "4"]);
}

#[test]
fn lines_are_written_in_the_order_they_were_queued() {
    let (p, lines, _exit) = run("echo");
    thread::scope(|scope| {
        for name in ["a", "b"] {
            let p = &p;
            scope.spawn(move || {
                for n in 0..100 {
                    p.write_line(&format!("{name}{n}")).unwrap();
                }
            });
        }
    });
    let got: Vec<String> = (0..200).map(|_| recv(&lines)).collect();
    for name in ["a", "b"] {
        let own: Vec<&str> = got
            .iter()
            .filter_map(|line| line.strip_prefix("got:"))
            .filter(|line| line.starts_with(name))
            .collect();
        let expected: Vec<String> = (0..100).map(|n| format!("{name}{n}")).collect();
        assert_eq!(own, expected);
    }
}

#[test]
fn closing_the_input_writes_what_was_queued_first() {
    let (p, lines, exit) = run("echo");
    for n in 0..3 {
        p.write_line(&format!("line{n}")).unwrap();
    }
    p.close_input();
    assert!(matches!(p.write_line("late"), Err(AgentError::Closed)));
    for n in 0..3 {
        assert_eq!(recv(&lines), format!("got:line{n}"));
    }
    // The stub exits at the end of its input.
    assert_eq!(recv_exit(&exit).code, Some(0));
}

#[test]
fn a_line_after_the_child_has_gone_is_refused() {
    let (p, _lines, exit) = run("echo");
    p.kill();
    recv_exit(&exit);
    assert!(matches!(p.write_line("late"), Err(AgentError::Closed)));
}
