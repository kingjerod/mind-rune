//! The `events` subcommand: one session through any `Agent`, printing each
//! agent-neutral event. Typed commands: `say TEXT`, `stop`, `rc on|off`, `quit`.

use std::io::{self, BufRead};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mr_agent::{Agent, SessionCommand, SessionEvent, StartSpec};

use crate::repl::{Cmd, parse_command};

/// How long to wait for the session to end after `Close`, which itself kills
/// a child that has not exited after two seconds.
const EXIT_WAIT: Duration = Duration::from_secs(5);

enum Message {
    Typed(String),
    InputEnded,
    SessionExited,
}

pub fn run(agent: &dyn Agent, cwd: PathBuf, model: Option<String>) -> Result<(), String> {
    let (tx, inbox) = mpsc::channel();
    let mut spec = StartSpec::new(cwd);
    spec.model = model;
    let sink_tx = tx.clone();
    let session = agent
        .open(
            spec,
            Box::new(move |event| {
                println!("{event:?}");
                if matches!(event, SessionEvent::Exited { .. }) {
                    let _ = sink_tx.send(Message::SessionExited);
                }
            }),
        )
        .map_err(|e| format!("cannot open a session: {e}"))?;

    thread::spawn(move || read_typed_lines(tx));

    while let Ok(message) = inbox.recv() {
        let line = match message {
            Message::Typed(line) => line,
            Message::InputEnded => break,
            Message::SessionExited => return Ok(()),
        };
        if line.trim().is_empty() {
            continue;
        }
        let command = match parse_command(&line) {
            Ok(Cmd::Quit) => break,
            Ok(Cmd::Say(text)) => SessionCommand::Send {
                text,
                attachments: vec![],
            },
            Ok(Cmd::Stop) => SessionCommand::Stop,
            Ok(Cmd::Remote(on)) => SessionCommand::SetRemoteAccess { on },
            Ok(_) => {
                eprintln!("probe: `events` understands only say, stop, rc and quit");
                continue;
            }
            Err(message) => {
                eprintln!("probe: {message}");
                continue;
            }
        };
        if let Err(e) = session.send(command) {
            eprintln!("probe: {e}");
        }
    }

    // Quit or end of input: let the session finish and report how it ended.
    // A session that already ended answers `Closed`, and its exit is on the way.
    let _ = session.send(SessionCommand::Close);
    while let Ok(message) = inbox.recv_timeout(EXIT_WAIT) {
        if matches!(message, Message::SessionExited) {
            break;
        }
    }
    Ok(())
}

fn read_typed_lines(messages: mpsc::Sender<Message>) {
    let mut stdin = io::stdin().lock();
    let mut line = String::new();
    loop {
        line.clear();
        match stdin.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if messages.send(Message::Typed(line.clone())).is_err() {
                    return;
                }
            }
        }
    }
    let _ = messages.send(Message::InputEnded);
}
