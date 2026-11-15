//! MindRune: a desktop window for Claude Code. `mindrune [FOLDER] [--demo]`.

mod demo;
mod glue;
#[cfg(test)]
mod testkit;
mod theme;
mod view;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use mr_claude::ClaudeAgent;
use rune::AppSpec;
use rune::prefs::Prefs;

use crate::glue::Ui;

/// What the command line asked for.
#[derive(Debug, PartialEq)]
struct Launch {
    /// The folder to start a new session in; with none, the session shown when the last run
    /// ended is shown again, or the window says how to start one.
    folder: Option<PathBuf>,
    demo: bool,
}

static LAUNCH: OnceLock<Launch> = OnceLock::new();

fn main() {
    // Rune's own flags (`--scene`, `--snap`, ...) are not in `extra_args`.
    let launch = parse(rune::extra_args()).unwrap_or_else(|message| {
        eprintln!("mindrune: {message}");
        std::process::exit(2);
    });
    let _ = LAUNCH.set(launch);
    // As `rune::run`, with a hook that ends the session when the window closes.
    rune::App::new(AppSpec {
        min_size: Some((560.0, 400.0)),
        scenes: demo::SCENES,
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(glue::setup),
        poll: Some(glue::poll),
        on_key: Some(view::on_key),
        ..AppSpec::new("MindRune", (1200.0, 800.0), init, view::view)
    })
    .on_close(glue::closing)
    .run();
}

fn init() -> Ui {
    let launch = LAUNCH.get().expect("main reads the arguments first");
    if launch.demo {
        return demo::ui();
    }
    let agent = Arc::new(ClaudeAgent::from_environment());
    let launch = core_launch(launch, rune::dirs::home());
    Ui::new(agent, None, launch, Prefs::open("mindrune"))
}

/// What the app starts with: the command line's folder, if any, and the user's `home` folder,
/// where a look starts when no session is known; the current folder stands in for a home that
/// was not found. It looks: without a folder, a session is started in the background to learn
/// the models and the usage limits.
fn core_launch(launch: &Launch, home: Option<PathBuf>) -> mr_core::Launch {
    let home = home
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"));
    mr_core::Launch {
        folder: launch.folder.clone(),
        look: true,
        home,
        ..mr_core::Launch::default()
    }
}

/// `[FOLDER] [--demo]`. The folder, when one is given, must exist.
fn parse(args: &[String]) -> Result<Launch, String> {
    let mut demo = false;
    let mut folder = None;
    for arg in args {
        if arg == "--demo" {
            demo = true;
        } else if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else if folder.is_none() {
            folder = Some(PathBuf::from(arg));
        } else {
            return Err(format!("only one folder can be opened: {arg}"));
        }
    }
    let folder = match folder {
        Some(folder) if !folder.is_dir() => {
            return Err(format!("not a folder: {}", folder.display()));
        }
        Some(folder) => Some(std::path::absolute(&folder).unwrap_or(folder)),
        None => None,
    };
    Ok(Launch { folder, demo })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_agent::{Account, Capabilities, SessionId, StoredSession};
    use mr_core::{App, Effect, FromBackend, SAVED_VERSION, Saved, Width};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn the_command_line_folder_is_optional() {
        assert_eq!(
            parse(&[]),
            Ok(Launch {
                folder: None,
                demo: false
            })
        );
        assert_eq!(
            parse(&args(&["--demo"])),
            Ok(Launch {
                folder: None,
                demo: true
            })
        );
        assert_eq!(
            parse(&args(&["/no/such/folder"])),
            Err("not a folder: /no/such/folder".into())
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert_eq!(
            parse(&args(&["--demo", path])),
            Ok(Launch {
                folder: Some(dir.path().into()),
                demo: true
            })
        );
    }

    fn caps() -> Capabilities {
        Capabilities {
            remote_access: true,
            plan_mode: true,
            usage_limits: true,
            images: true,
        }
    }

    fn signed_in() -> Account {
        Account::SignedIn {
            name: "alex".into(),
            plan: None,
            full_login: true,
        }
    }

    #[test]
    fn without_a_folder_the_last_session_is_shown_again_and_the_app_looks() {
        let launch = parse(&[]).unwrap();
        let mut core = core_launch(&launch, Some("/home/alex".into()));
        assert_eq!(core.folder, None);
        assert!(core.look);
        assert_eq!(core.home, PathBuf::from("/home/alex"));
        core.saved = Saved {
            version: SAVED_VERSION,
            shown: Some(SessionId("a1".into())),
            ..Saved::default()
        };
        let (mut app, _) = App::new(caps(), Width::Unlimited, core, 1_800_000_000);
        let effects = app.account_checked(signed_in());
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::Open { .. })),
            "no session starts by itself: {effects:?}"
        );
        assert_eq!(app.focused(), None);
        let stored = StoredSession {
            id: SessionId("a1".into()),
            folder: "/home/alex/notes".into(),
            title: "Tidy the notes".into(),
            last_active_unix: 1_800_000_000 - 60,
        };
        let effects = app.receive(
            FromBackend::Sessions {
                since_unix: Some(0),
                result: Ok(vec![stored]),
            },
            0.0,
        );
        let shown = app
            .focused_entry()
            .expect("the last session is shown again");
        assert_eq!(shown.session.folder, PathBuf::from("/home/alex/notes"));
        assert!(shown.open.is_none(), "without starting its program");
        assert!(
            effects.contains(&Effect::Look {
                folder: "/home/alex/notes".into()
            }),
            "{effects:?}"
        );
    }

    #[test]
    fn with_a_folder_a_session_starts_there() {
        let dir = tempfile::tempdir().unwrap();
        let launch = parse(&args(&[dir.path().to_str().unwrap()])).unwrap();
        let core = core_launch(&launch, Some("/home/alex".into()));
        assert_eq!(core.folder.as_deref(), Some(dir.path()));
        let (mut app, _) = App::new(caps(), Width::Unlimited, core, 1_800_000_000);
        let effects = app.account_checked(signed_in());
        let opened = effects.iter().find_map(|e| match e {
            Effect::Open { spec, .. } => Some(spec.folder.clone()),
            _ => None,
        });
        assert_eq!(opened.as_deref(), Some(dir.path()));
        let shown = app.focused_entry().expect("its session is shown");
        assert_eq!(shown.session.folder, dir.path());
    }

    #[test]
    fn without_a_home_the_current_folder_stands_in() {
        let core = core_launch(&parse(&[]).unwrap(), None);
        assert_eq!(core.home, std::env::current_dir().unwrap());
    }
}
