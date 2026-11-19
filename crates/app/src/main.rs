//! MindRune: a desktop window for Claude Code. `mindrune [FOLDER] [--demo]`, `mindrune --quit`.

mod demo;
mod glue;
mod instance;
mod notify;
#[cfg(test)]
mod testkit;
mod theme;
mod tray;
mod view;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, OnceLock};

use mr_claude::ClaudeAgent;
use mr_core::IDLE_SECONDS;
use rune::prefs::Prefs;
use rune::{AppHandle, AppSpec, Tx};

use crate::glue::Ui;
use crate::instance::{Ask, Claim, Listener, Told};

/// What the command line asked for.
#[derive(Debug, PartialEq)]
struct Launch {
    /// The folder to start a new session in; with none, the session shown when the last run
    /// ended is shown again, or the window says how to start one.
    folder: Option<PathBuf>,
    demo: bool,
    /// `--quit`: the running copy is asked to quit, and nothing starts.
    quit: bool,
}

static LAUNCH: OnceLock<Launch> = OnceLock::new();

/// The id the window reports to a Linux desktop, which finds the window's icon through the
/// entry of that name: `assets/linux/mindrune.desktop`, put in place by
/// `scripts/install-desktop-entry.sh`.
const APP_ID: &str = "mindrune";

/// The app's icon where the program sets it itself: the Dock on macOS, the windows and the
/// taskbar on Windows.
const ICON: &[u8] = include_bytes!("../../../assets/icon/mindrune-512.png");

fn main() {
    // Rune's own flags (`--scene`, `--snap`, ...) are not in `extra_args`.
    let launch = parse(rune::extra_args()).unwrap_or_else(|message| {
        eprintln!("mindrune: {message}");
        std::process::exit(2);
    });
    let headless = rune::is_headless();
    // Before anything of the app is built: a second copy ends without touching its state.
    let first = if claims(&launch, headless) {
        one_copy(&launch)
    } else {
        not_claimed(&launch)
    };
    let stays = stays_without_window(&launch, headless, first.is_some());
    let notifies = notifies(&launch, headless);
    let _ = LAUNCH.set(launch);
    rune::set_app_id(APP_ID);
    if let Err(message) = rune::set_app_icon(ICON) {
        eprintln!("mindrune: {message}");
    }
    // As `rune::run`, with hooks for the window's close and the app's quit.
    let app = rune::App::new(AppSpec {
        min_size: Some((560.0, 400.0)),
        scenes: demo::SCENES,
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(glue::setup),
        poll: Some(glue::poll),
        on_key: Some(view::on_key),
        ..AppSpec::new("MindRune", (1200.0, 800.0), init, view::view)
    })
    // Whether the user is in this app or another, for a turn that ends in the focused tile.
    .on_focus(glue::focus_changed);
    let app = if stays {
        app.keep_running()
            .on_close(glue::closing)
            .on_quit(glue::quitting)
            .on_visibility(glue::shown)
    } else {
        app.on_close(glue::quitting)
    };
    if notifies {
        let handle = app.handle();
        if let Some(notifier) = notify::start(handle.clone()) {
            handle.update(move |ui, _tx| ui.notifier = Some(notifier));
        }
    }
    if let Some(listener) = first {
        let handle = app.handle();
        if stays {
            let tray = tray::start(handle.clone());
            handle.update(move |ui, _tx| ui.tray = Some(tray));
            // So the next poll tells the tray how the sessions stand, not one half a minute on.
            handle.wake();
        }
        listen(listener, handle);
    }
    app.run();
}

/// Whether the app, its sessions with it, keeps running when its window closes: on Unix,
/// outside the demo and outside a headless run, and only as the `first` copy, which a launch
/// can find again. Otherwise closing the window ends the app.
fn stays_without_window(launch: &Launch, headless: bool, first: bool) -> bool {
    cfg!(unix) && !launch.demo && !headless && first
}

/// Whether this launch looks for a running copy: not in the demo and not in a headless run.
fn claims(launch: &Launch, headless: bool) -> bool {
    !launch.demo && !headless
}

/// Whether desktop notifications are sent: outside the demo and outside a headless run, also
/// when the app does not stay without its window.
fn notifies(launch: &Launch, headless: bool) -> bool {
    !launch.demo && !headless
}

/// Claims the socket: the first copy gets its listener. A second copy tells the running one
/// what was asked and ends here, as `--quit` does. Where there is no way back to this copy,
/// it runs alone.
fn one_copy(launch: &Launch) -> Option<Listener> {
    let Some(dir) = instance::folder() else {
        return not_claimed(launch);
    };
    let claim = instance::claim(&dir).unwrap_or_else(|error| {
        eprintln!("mindrune: {}: {error}", dir.display());
        Claim::Alone
    });
    match claim {
        Claim::First(_) if launch.quit => not_running(),
        Claim::First(listener) => Some(listener),
        Claim::Second => {
            // A quit whose answer did not come is no failure when the copy has gone.
            let told = if launch.quit {
                instance::quit(&dir)
            } else {
                instance::tell(
                    &dir,
                    &Ask::Show {
                        token: activation_token(),
                        folder: launch.folder.clone(),
                    },
                )
            };
            let Ok(told) = told else {
                let lock = instance::lock_path(&dir);
                eprintln!(
                    "mindrune: a running copy does not answer ({})",
                    lock.display()
                );
                std::process::exit(1);
            };
            if let Some(line) = said_after(told, launch.quit) {
                eprintln!("{line}");
            }
            std::process::exit(0);
        }
        Claim::Alone => not_claimed(launch),
    }
}

/// What a later launch says on stderr once the running copy has answered: that its window is
/// shown, or, when a quit was asked for and the app asks the user first, where the question is.
/// Nothing for a quit that was carried out.
fn said_after(told: Told, quit: bool) -> Option<&'static str> {
    match (told, quit) {
        (Told::Asked, _) => {
            Some("mindrune: something is running; answer the question in its window")
        }
        (Told::Ok, false) => Some("mindrune: already running; its window is shown"),
        (Told::Ok, true) => None,
    }
}

/// A launch that found no running copy and is not the first: `--quit` ends here.
fn not_claimed(launch: &Launch) -> Option<Listener> {
    if launch.quit {
        not_running();
    }
    None
}

fn not_running() -> ! {
    eprintln!("mindrune: not running");
    std::process::exit(0);
}

/// The token the desktop gave this launch to bring a window to the front, passed on to the
/// running copy when it is [one](instance::is_token). Never printed.
fn activation_token() -> Option<String> {
    activation_token_from(|key| std::env::var(key).ok())
}

fn activation_token_from(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"]
        .into_iter()
        .find_map(|key| var(key).filter(|token| instance::is_token(token)))
}

/// Serves the copies launched after this one: a launch shows the window, opening a session in
/// its folder, and `--quit` quits as Quit does, asking first when something would be cut off.
/// The window opens for that question, and the copy that asked is told of it.
fn listen(listener: Listener, handle: AppHandle<Ui>) {
    listener.serve(move |ask| match ask {
        Ask::Show { token, folder } => {
            handle.update(move |ui, tx| glue::launched_again(ui, tx, folder));
            show(&handle, token);
            None
        }
        Ask::Quit => Some(ask_to_quit(&handle)),
    });
}

/// Shows the window in front from another thread (a later launch, the tray), with the
/// desktop's `token` when there is one. The token is never printed.
fn show(handle: &AppHandle<Ui>, token: Option<String>) {
    handle.open();
    if let Some(token) = token {
        handle.activate(token);
    }
}

/// Asks for Quit from another thread (a later launch's `--quit`, the tray's Quit): the app
/// quits, or the window opens for the question when something would be cut off. Which of the
/// two comes on the receiver, from the UI thread.
fn ask_to_quit(handle: &AppHandle<Ui>) -> Receiver<Told> {
    let (say, word) = mpsc::channel();
    let again = handle.clone();
    handle.update(move |ui, tx| {
        // Nobody may be listening: the tray asks and does not wait.
        let asks = quit_for(ui, tx, |told| {
            let _ = say.send(told);
        });
        if asks {
            again.open();
        }
    });
    word
}

/// A quit asked for from outside the window, on the UI thread. What the asker is told is said
/// before anything is done: a quit that is carried out may wait for the programs to end, and
/// ends the listener with the app. True when the question shows.
fn quit_for(ui: &mut Ui, tx: &mut Tx, say: impl FnOnce(Told)) -> bool {
    say(if glue::quit_asks(ui) {
        Told::Asked
    } else {
        Told::Ok
    });
    glue::ask_quit(ui, tx)
}

fn init() -> Ui {
    let launch = LAUNCH.get().expect("main reads the arguments first");
    if launch.demo {
        return demo::ui();
    }
    let agent = Arc::new(ClaudeAgent::from_environment());
    let launch = core_launch(launch, rune::dirs::home());
    let mut ui = Ui::new(agent, None, launch, Prefs::open("mindrune"));
    let idle = std::env::var_os("MINDRUNE_IDLE_SECONDS");
    let idle = idle.as_deref().map(|value| value.to_string_lossy());
    ui.app.set_idle_after(idle_seconds(idle.as_deref()));
    ui
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

/// The idle length `MINDRUNE_IDLE_SECONDS` asks for when it has `value`, in seconds: a whole
/// number is taken; anything else is said on stderr, and the usual length stays.
fn idle_seconds(value: Option<&str>) -> i64 {
    let Some(value) = value else {
        return IDLE_SECONDS;
    };
    value.parse().unwrap_or_else(|_| {
        eprintln!(
            "mindrune: MINDRUNE_IDLE_SECONDS is not a number of seconds; using {IDLE_SECONDS}"
        );
        IDLE_SECONDS
    })
}

/// `[FOLDER] [--demo]` or `--quit`. The folder, when one is given, must exist.
fn parse(args: &[String]) -> Result<Launch, String> {
    let mut demo = false;
    let mut quit = false;
    let mut folder = None;
    for arg in args {
        if arg == "--demo" {
            demo = true;
        } else if arg == "--quit" {
            quit = true;
        } else if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else if folder.is_none() {
            folder = Some(PathBuf::from(arg));
        } else {
            return Err(format!("only one folder can be opened: {arg}"));
        }
    }
    if quit && folder.is_some() {
        return Err("--quit takes no folder".into());
    }
    let folder = match folder {
        Some(folder) if !folder.is_dir() => {
            return Err(format!("not a folder: {}", folder.display()));
        }
        Some(folder) => Some(std::path::absolute(&folder).unwrap_or(folder)),
        None => None,
    };
    Ok(Launch { folder, demo, quit })
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
                demo: false,
                quit: false
            })
        );
        assert_eq!(
            parse(&args(&["--demo"])),
            Ok(Launch {
                folder: None,
                demo: true,
                quit: false
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
                demo: true,
                quit: false
            })
        );
    }

    #[test]
    fn quit_takes_no_folder() {
        assert_eq!(
            parse(&args(&["--quit"])),
            Ok(Launch {
                folder: None,
                demo: false,
                quit: true
            })
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert_eq!(
            parse(&args(&["--quit", path])),
            Err("--quit takes no folder".into())
        );
        assert_eq!(
            parse(&args(&[path, "--quit"])),
            Err("--quit takes no folder".into())
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

    /// The value of `key` in a desktop entry.
    fn entry_value<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
        entry
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
    }

    #[test]
    fn the_desktop_entry_names_the_id_the_window_reports() {
        // A Linux desktop matches a window to its entry, and so to its icon, by this id.
        let entry = include_str!("../../../assets/linux/mindrune.desktop");
        assert_eq!(APP_ID, "mindrune");
        assert_eq!(entry_value(entry, "StartupWMClass"), Some("mindrune"));
        assert_eq!(entry_value(entry, "Icon"), Some("mindrune"));
    }

    #[test]
    fn the_app_icon_is_a_square_png_of_256_pixels_or_more() {
        // What `rune::set_app_icon` asks for on Windows.
        assert_eq!(&ICON[..8], b"\x89PNG\r\n\x1a\n");
        let side = |at: usize| u32::from_be_bytes(ICON[at..at + 4].try_into().unwrap());
        let (width, height) = (side(16), side(20));
        assert_eq!(width, height);
        assert!(width >= 256, "{width}");
    }

    #[test]
    fn without_a_home_the_current_folder_stands_in() {
        let core = core_launch(&parse(&[]).unwrap(), None);
        assert_eq!(core.home, std::env::current_dir().unwrap());
    }

    #[test]
    fn the_app_stays_without_its_window_only_outside_the_demo_and_a_headless_run() {
        let app = parse(&[]).unwrap();
        let demo = parse(&args(&["--demo"])).unwrap();
        assert_eq!(stays_without_window(&app, false, true), cfg!(unix));
        assert!(!stays_without_window(&app, true, true));
        assert!(!stays_without_window(&demo, false, true));
        assert!(!stays_without_window(&demo, true, true));
    }

    #[test]
    fn only_the_first_copy_stays_without_its_window() {
        let app = parse(&[]).unwrap();
        assert!(!stays_without_window(&app, false, false));
    }

    #[test]
    fn the_demo_and_a_headless_run_claim_nothing() {
        let app = parse(&[]).unwrap();
        let quit = parse(&args(&["--quit"])).unwrap();
        let demo = parse(&args(&["--demo"])).unwrap();
        assert!(claims(&app, false));
        assert!(claims(&quit, false));
        assert!(!claims(&app, true));
        assert!(!claims(&demo, false));
        assert!(!claims(&demo, true));
    }

    #[test]
    fn the_demo_and_a_headless_run_send_no_notifications() {
        let app = parse(&[]).unwrap();
        let demo = parse(&args(&["--demo"])).unwrap();
        // Also as a copy that does not stay without its window.
        assert!(!stays_without_window(&app, false, false));
        assert!(notifies(&app, false));
        assert!(!notifies(&app, true));
        assert!(!notifies(&demo, false));
        assert!(!notifies(&demo, true));
    }

    #[test]
    fn only_a_printable_token_is_passed_on() {
        let token = |pairs: &[(&str, &str)]| {
            activation_token_from(|key| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.to_string())
            })
        };
        assert_eq!(
            token(&[("XDG_ACTIVATION_TOKEN", "abc_1")]),
            Some("abc_1".into())
        );
        assert_eq!(
            token(&[("DESKTOP_STARTUP_ID", "id_TIME1")]),
            Some("id_TIME1".into())
        );
        assert_eq!(token(&[("XDG_ACTIVATION_TOKEN", "a\nb")]), None);
        assert_eq!(
            token(&[("XDG_ACTIVATION_TOKEN", "a b"), ("DESKTOP_STARTUP_ID", "")]),
            None
        );
        assert_eq!(token(&[]), None);
    }

    #[test]
    fn a_later_launch_says_what_the_running_copy_answered() {
        assert_eq!(
            said_after(Told::Ok, false),
            Some("mindrune: already running; its window is shown")
        );
        assert_eq!(said_after(Told::Ok, true), None, "it quit: nothing to say");
        let asked = Some("mindrune: something is running; answer the question in its window");
        assert_eq!(said_after(Told::Asked, true), asked);
        assert_eq!(said_after(Told::Asked, false), asked);
    }

    #[test]
    fn a_quit_from_outside_says_whether_the_app_asks_first() {
        // A turn runs: the app asks, and says so.
        let mut rt = testkit::window("working");
        let mut told = None;
        let asks = rt.update_if(|ui, tx| quit_for(ui, tx, |word| told = Some(word)));
        assert!(asks);
        assert_eq!(told, Some(Told::Asked));
        assert!(rt.state.quit_ask && !rt.state.quitting);
        // Asked again while the question shows: the same answer, and nothing else changes.
        let mut told = None;
        assert!(rt.update_if(|ui, tx| quit_for(ui, tx, |word| told = Some(word))));
        assert_eq!(told, Some(Told::Asked));
        assert!(rt.state.quit_ask && !rt.state.quitting);
        // Nothing runs: the quit is carried out.
        let mut rt = testkit::window("empty");
        let mut told = None;
        let asks = rt.update_if(|ui, tx| quit_for(ui, tx, |word| told = Some(word)));
        assert!(!asks);
        assert_eq!(told, Some(Told::Ok));
        assert!(rt.state.quitting && !rt.state.quit_ask);
        assert_eq!(rt.shell_request, Some(rune::Shell::Quit));
    }

    #[test]
    fn the_idle_length_is_read_from_the_environment() {
        assert_eq!(idle_seconds(Some("120")), 120);
        assert_eq!(idle_seconds(Some("soon")), IDLE_SECONDS);
        assert_eq!(idle_seconds(None), IDLE_SECONDS);
    }
}
