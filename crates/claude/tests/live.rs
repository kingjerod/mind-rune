//! Starts the real `claude`. Opt in with `MINDRUNE_LIVE=1` and `--ignored`; it needs a
//! signed-in `claude` on the PATH and spends a little of the user's usage.

use std::path::PathBuf;

use mr_agent::{Agent, SessionCommand, SessionEvent, StartSpec};
use mr_claude::{ClaudeAgent, root_from};

/// Claude Code's folder, where the program itself looks.
fn dirs_root() -> PathBuf {
    root_from(
        std::env::var_os("CLAUDE_CONFIG_DIR").as_deref(),
        std::env::home_dir().as_deref(),
    )
    .expect("Claude Code's folder is known")
}

#[test]
#[ignore = "starts the real claude; run with MINDRUNE_LIVE=1"]
fn one_real_turn_on_the_cheapest_model() {
    if std::env::var("MINDRUNE_LIVE").as_deref() != Ok("1") {
        return;
    }
    let keep = [
        "HOME",
        "PATH",
        "USER",
        "LANG",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "CLAUDE_CODE_OAUTH_TOKEN",
    ];
    let env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| keep.contains(&k.as_str()))
        .collect();
    let agent = ClaudeAgent::with_paths("claude".into(), dirs_root(), env);
    let dir = tempfile::tempdir().unwrap();
    let mut spec = StartSpec::new(dir.path());
    spec.model = Some("haiku".into());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = agent
        .open(
            spec,
            Box::new(move |e| {
                let _ = tx.send(e);
            }),
        )
        .unwrap();
    handle
        .send(SessionCommand::Send {
            text: "Reply with the single word: ok".into(),
            attachments: vec![],
        })
        .unwrap();
    let mut saw_limits = false;
    loop {
        match rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("no event in two minutes")
        {
            SessionEvent::UsageLimits(_) => saw_limits = true,
            SessionEvent::TurnFinished(_) => break,
            SessionEvent::TurnFailed { reason } => panic!("turn failed: {reason}"),
            SessionEvent::Exited { detail, .. } => panic!("claude exited: {detail}"),
            _ => {}
        }
    }
    assert!(saw_limits, "usage limits should arrive during a turn");
    handle.send(SessionCommand::Close).unwrap();
}
