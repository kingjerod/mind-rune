//! The tray icon: a click shows the window; its menu says how the sessions stand and offers
//! Show and Quit. The tray runs on its own thread and reaches the app only through its handle.

use std::sync::mpsc::{self, Receiver, Sender};

use mr_agent::ImageData;
use mr_core::Standing;
use rune::{AppHandle, Tray, TrayEvent, TrayIcon, TrayItem, TraySpec};

use crate::glue::Ui;

/// The tray's icon, built into the program.
const ICON: &[u8] = include_bytes!("../../../assets/icon/mindrune-64.png");

/// What the menu says of the sessions: the counts that are not zero, joined by ` · `.
/// `None` when every count is zero.
pub fn status_line(standing: Standing) -> Option<String> {
    let mut parts = Vec::new();
    if standing.working > 0 {
        parts.push(format!("{} working", standing.working));
    }
    match standing.needs_you {
        0 => {}
        1 => parts.push("1 needs you".to_string()),
        many => parts.push(format!("{many} need you")),
    }
    if standing.finished > 0 {
        parts.push(format!("{} finished", standing.finished));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The tray's menu: the status line, when there is one, which cannot be picked; then Show and
/// Quit.
pub fn menu(standing: Standing) -> Vec<TrayItem> {
    let mut items = Vec::new();
    if let Some(line) = status_line(standing) {
        items.push(TrayItem::action("status", line).enabled(false));
        items.push(TrayItem::Separator);
    }
    items.extend([
        TrayItem::action("show", "Show MindRune"),
        TrayItem::Separator,
        TrayItem::action("quit", "Quit"),
    ]);
    items
}

/// Starts the tray on its own thread. The sender tells it a new standing.
pub fn start(handle: AppHandle<Ui>) -> Sender<Standing> {
    let (tell, told) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("mindrune-tray".into())
        .spawn(move || serve(handle, told));
    if let Err(error) = spawned {
        eprintln!("mindrune: no tray icon: {error}");
    }
    tell
}

/// The tray's thread: puts the icon up, then brings its menu up to date with each standing told,
/// until the sender is gone. Where no icon can be put up, it says so once and ends.
fn serve(handle: AppHandle<Ui>, told: Receiver<Standing>) {
    let Some(icon) = icon() else {
        eprintln!("mindrune: no tray icon: its picture cannot be read");
        return;
    };
    let spec = TraySpec::new("mindrune", "MindRune", icon)
        .tooltip("MindRune")
        .menu(menu(Standing::default()));
    let tray = match Tray::new(spec, move |event| on_event(&handle, event)) {
        Ok(tray) => tray,
        Err(error) => {
            eprintln!("mindrune: no tray icon: {error}");
            return;
        }
    };
    for standing in told {
        tray.set_menu(menu(standing));
    }
}

/// The icon's pixels.
fn icon() -> Option<TrayIcon> {
    let picture = mr_core::picture_of(&ImageData::new(ICON))?;
    Some(TrayIcon::rgba(
        picture.width,
        picture.height,
        picture.rgba.to_vec(),
    ))
}

/// Runs on the tray's thread, and only hands work over: a click on the icon or Show shows the
/// window, Quit asks to quit as `mindrune --quit` does.
fn on_event(handle: &AppHandle<Ui>, event: TrayEvent) {
    match event {
        TrayEvent::Activate { token } => crate::show(handle, token),
        TrayEvent::Menu(id) if id == "show" => crate::show(handle, None),
        // The tray does not wait to hear whether the app asks first: the window shows it.
        TrayEvent::Menu(id) if id == "quit" => drop(crate::ask_to_quit(handle)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_icon_can_be_read() {
        // Unread, the tray would not start.
        assert!(icon().is_some());
    }

    #[test]
    fn the_status_line_names_what_is_not_zero() {
        let all = Standing {
            working: 2,
            needs_you: 1,
            finished: 1,
        };
        assert_eq!(
            status_line(all).as_deref(),
            Some("2 working · 1 needs you · 1 finished")
        );
        let waiting = Standing {
            needs_you: 3,
            ..Standing::default()
        };
        assert_eq!(status_line(waiting).as_deref(), Some("3 need you"));
        let finished = Standing {
            working: 1,
            finished: 4,
            ..Standing::default()
        };
        assert_eq!(
            status_line(finished).as_deref(),
            Some("1 working · 4 finished")
        );
        assert_eq!(status_line(Standing::default()), None);
    }

    /// The ids of `menu`'s actions, in order, with whether each is enabled.
    fn actions(menu: &[TrayItem]) -> Vec<(&str, bool)> {
        menu.iter()
            .filter_map(|item| match item {
                TrayItem::Action { id, enabled, .. } => Some((id.as_str(), *enabled)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_menu_has_show_and_quit_and_a_status_only_when_there_is_one() {
        let quiet = menu(Standing::default());
        assert_eq!(actions(&quiet), vec![("show", true), ("quit", true)]);
        assert_eq!(
            quiet,
            vec![
                TrayItem::action("show", "Show MindRune"),
                TrayItem::Separator,
                TrayItem::action("quit", "Quit"),
            ]
        );
        let busy = menu(Standing {
            working: 1,
            ..Standing::default()
        });
        assert_eq!(
            actions(&busy),
            vec![("status", false), ("show", true), ("quit", true)]
        );
        assert_eq!(
            busy[..2],
            [
                TrayItem::action("status", "1 working").enabled(false),
                TrayItem::Separator,
            ]
        );
    }
}
