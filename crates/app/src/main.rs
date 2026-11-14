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
    folder: PathBuf,
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
    Ui::new(agent, None, launch.folder.clone(), Prefs::open("mindrune"))
}

/// `[FOLDER] [--demo]`. The folder defaults to the current one and must exist.
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
        Some(folder) => folder,
        None => std::env::current_dir().map_err(|e| format!("no current folder: {e}"))?,
    };
    if !folder.is_dir() {
        return Err(format!("not a folder: {}", folder.display()));
    }
    let folder = std::path::absolute(&folder).unwrap_or(folder);
    Ok(Launch { folder, demo })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn the_folder_must_exist() {
        assert_eq!(
            parse(&args(&["/no/such/folder"])),
            Err("not a folder: /no/such/folder".into())
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert_eq!(
            parse(&args(&["--demo", path])),
            Ok(Launch {
                folder: dir.path().into(),
                demo: true
            })
        );
        let here = parse(&[]).unwrap();
        assert_eq!(here.folder, std::env::current_dir().unwrap());
        assert!(!here.demo);
    }
}
