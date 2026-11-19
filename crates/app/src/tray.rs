//! The tray icon: a click shows the window; its menu says how the sessions stand and offers
//! Show and Quit; a dot on it says whether a session is ready or needs input. The tray runs on
//! its own thread and reaches the app only through its handle.
//!
//! Rune cannot change a tray's icon, so a new marker takes a new tray, which blinks. The icon
//! is therefore calm ([`Plan`]): it changes only once the marker wanted has held for
//! [`MARKER_HOLD`], so a session that becomes ready and is attended at once never touches it.
//! The menu's first line follows every standing at once. A tray that could not be put up is
//! tried again every [`TRAY_RETRY`], by the thread's own timer.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use mr_agent::ImageData;
use mr_core::Standing;
use rune::{AppHandle, Tray, TrayError, TrayEvent, TrayIcon, TrayItem, TraySpec};

use crate::glue::Ui;
use crate::theme;

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

/// How long the marker wanted must stay the same before the icon is made anew with it.
const MARKER_HOLD: Duration = Duration::from_secs(3);
/// How long after a tray could not be put up it is tried again.
const TRAY_RETRY: Duration = Duration::from_secs(30);
/// How long the first tray waits for the first standing, so that it goes up with the right
/// marker.
const FIRST_WAIT: Duration = Duration::from_millis(500);

/// What the tray's thread does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Put a tray up with this marker, in place of the one that is up, if any.
    PutUp(Marker),
    /// Wait for the next standing: at most this long, or as long as it takes.
    Wait(Option<Duration>),
}

/// Which marker the tray's icon shows, and when: what the thread decides from. It holds no
/// tray and reads no clock, and its times are counted from the thread's start.
#[derive(Debug, Default)]
struct Plan {
    /// The newest standing told.
    standing: Standing,
    /// Since when the marker wanted has been the one it is.
    wanted_since: Duration,
    /// The marker of the tray that is up; none while there is no tray.
    up: Option<Marker>,
    /// When a tray was last tried in vain; none before the first try and while one is up.
    failed: Option<Duration>,
}

impl Plan {
    /// Takes in a standing told at `now`.
    fn told(&mut self, standing: Standing, now: Duration) {
        if marker(standing) != marker(self.standing) {
            self.wanted_since = now;
        }
        self.standing = standing;
    }

    /// What to do at `now`. A tray that shows the marker wanted is left alone. One that shows
    /// another is made anew once the marker wanted has held for [`MARKER_HOLD`]. With no tray,
    /// one is put up at once the first time and [`TRAY_RETRY`] after each try that failed,
    /// with the marker wanted then.
    fn next(&self, now: Duration) -> Step {
        let wanted = marker(self.standing);
        let due = |at: Duration| match at.checked_sub(now) {
            Some(left) if !left.is_zero() => Step::Wait(Some(left)),
            _ => Step::PutUp(wanted),
        };
        match (self.up, self.failed) {
            (Some(drawn), _) if drawn == wanted => Step::Wait(None),
            (Some(_), _) => due(self.wanted_since + MARKER_HOLD),
            (None, Some(failed)) => due(failed + TRAY_RETRY),
            (None, None) => Step::PutUp(wanted),
        }
    }

    /// A tray with `marker` was tried, the one before it taken down first; the try ended at
    /// `now`.
    fn tried(&mut self, marker: Marker, went_up: bool, now: Duration) {
        self.up = went_up.then_some(marker);
        self.failed = (!went_up).then_some(now);
    }
}

/// The tray's thread: puts the icon up and does what its [`Plan`] says, until the sender is
/// gone. The menu is brought up to date with each standing told, at once. Where the platform
/// has no tray, or the icon's picture cannot be read, it says so once and ends; a tray that
/// could not be put up for another reason is said once and tried again.
fn serve(handle: AppHandle<Ui>, told: Receiver<Standing>) {
    if icon(Marker::None).is_none() {
        eprintln!("mindrune: no tray icon: its picture cannot be read");
        return;
    }
    let started = Instant::now();
    let mut plan = Plan::default();
    let mut tray: Option<Tray> = None;
    // Whether the last try failed and said so.
    let mut said = false;
    // The first tray waits a moment for the first standing: a session may be ready already.
    let mut wait = Step::Wait(Some(FIRST_WAIT));
    loop {
        match wait {
            Step::Wait(limit) => {
                let got = match limit {
                    Some(limit) => told.recv_timeout(limit),
                    None => told.recv().map_err(|_| RecvTimeoutError::Disconnected),
                };
                match got {
                    Ok(first) => {
                        // Only the newest of a burst counts.
                        let standing = told.try_iter().last().unwrap_or(first);
                        plan.told(standing, started.elapsed());
                        if let Some(tray) = &tray {
                            tray.set_menu(menu(standing));
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            Step::PutUp(marker) => {
                // A second tray with the same id would be a second icon: the old one goes
                // first.
                drop(tray.take());
                let put = match icon(marker) {
                    Some(icon) => put_up(&handle, icon, plan.standing),
                    None => Err(TrayError::Backend("its picture cannot be read".into())),
                };
                match put {
                    Ok(new) => {
                        tray = Some(new);
                        said = false;
                    }
                    Err(TrayError::Unsupported) => {
                        eprintln!("mindrune: no tray icon: {}", TrayError::Unsupported);
                        return;
                    }
                    Err(error) if !said => {
                        eprintln!("mindrune: no tray icon: {error}");
                        said = true;
                    }
                    Err(_) => {}
                }
                plan.tried(marker, tray.is_some(), started.elapsed());
            }
        }
        wait = plan.next(started.elapsed());
    }
}

/// Puts the icon up with the menu for `standing`. It can take up to five seconds.
fn put_up(handle: &AppHandle<Ui>, icon: TrayIcon, standing: Standing) -> Result<Tray, TrayError> {
    let spec = TraySpec::new("mindrune", "MindRune", icon)
        .tooltip("MindRune")
        .menu(menu(standing));
    let handle = handle.clone();
    Tray::new(spec, move |event| on_event(&handle, event))
}

/// The mark on the tray's icon: how the sessions that want the user stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    None,
    Ready,
    NeedsInput,
}

/// The mark for a standing: needs input wins over ready.
pub fn marker(standing: Standing) -> Marker {
    if standing.needs_you > 0 {
        Marker::NeedsInput
    } else if standing.finished > 0 {
        Marker::Ready
    } else {
        Marker::None
    }
}

/// The icon's pixels with the marker drawn over them: straight RGBA, `side` by `side`.
pub fn marked(pixels: &[u8], side: u32, marker: Marker) -> Vec<u8> {
    let mut out = pixels.to_vec();
    let fill = match marker {
        Marker::None => return out,
        Marker::Ready => theme::READY_RGB,
        Marker::NeedsInput => theme::NEEDS_INPUT_RGB,
    };
    let inner = 0.22 * side as f32;
    let outer = inner + side as f32 / 20.0;
    let centre = side as f32 - outer;
    for y in 0..side {
        for x in 0..side {
            let ring = cover(x, y, centre, outer);
            if ring == 0.0 {
                continue;
            }
            // In `usize`: in `u32` the product overflows for a very large icon.
            let at = (y as usize * side as usize + x as usize) * 4;
            let Some(pixel) = out.get_mut(at..at + 4) else {
                continue;
            };
            // The ring lies under the whole dot; the fill then covers all but the ring.
            over(pixel, [0, 0, 0], 0.6 * ring);
            over(pixel, fill, cover(x, y, centre, inner));
        }
    }
    out
}

/// How much of the pixel at `x`, `y` lies within `radius` of (`centre`, `centre`), from four by
/// four samples.
fn cover(x: u32, y: u32, centre: f32, radius: f32) -> f32 {
    let mut inside = 0;
    for i in 0..4 {
        for j in 0..4 {
            let dx = x as f32 + (i as f32 + 0.5) / 4.0 - centre;
            let dy = y as f32 + (j as f32 + 0.5) / 4.0 - centre;
            if dx * dx + dy * dy <= radius * radius {
                inside += 1;
            }
        }
    }
    inside as f32 / 16.0
}

/// Paints `rgb` at `alpha` over a straight RGBA pixel, source over.
fn over(pixel: &mut [u8], rgb: [u8; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let (colour, rest) = pixel.split_at_mut(3);
    let below = rest[0] as f32 / 255.0;
    let kept = below * (1.0 - alpha);
    let total = alpha + kept;
    for (channel, paint) in colour.iter_mut().zip(rgb) {
        *channel = ((paint as f32 * alpha + *channel as f32 * kept) / total).round() as u8;
    }
    rest[0] = (total * 255.0).round() as u8;
}

/// The icon's pixels with `marker` drawn over them. `None` when the picture cannot be read, or
/// when a marker is asked of a picture that is not square.
fn icon(marker: Marker) -> Option<TrayIcon> {
    let picture = mr_core::picture_of(&ImageData::new(ICON))?;
    let (width, height) = (picture.width, picture.height);
    if marker != Marker::None && width != height {
        return None;
    }
    Some(TrayIcon::rgba(
        width,
        height,
        marked(&picture.rgba, width, marker),
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
        // Unread, the tray would not start; unmarked, it would stay as it is.
        for marker in [Marker::None, Marker::Ready, Marker::NeedsInput] {
            assert!(icon(marker).is_some());
        }
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

    #[test]
    fn the_marker_follows_the_standing() {
        let both = Standing {
            working: 1,
            needs_you: 1,
            finished: 2,
        };
        assert_eq!(marker(both), Marker::NeedsInput);
        let ready = Standing {
            working: 1,
            finished: 1,
            ..Standing::default()
        };
        assert_eq!(marker(ready), Marker::Ready);
        let working = Standing {
            working: 3,
            ..Standing::default()
        };
        assert_eq!(marker(working), Marker::None);
        assert_eq!(marker(Standing::default()), Marker::None);
    }

    /// A `side` by `side` icon of one straight RGBA colour.
    fn plain(side: u32, rgba: [u8; 4]) -> Vec<u8> {
        rgba.repeat((side * side) as usize)
    }

    /// The straight RGBA bytes of the pixel at `x`, `y` of a `side` wide icon.
    fn pixel(pixels: &[u8], side: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * side + x) * 4) as usize;
        pixels[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn no_marker_leaves_the_icon_as_it_is() {
        let pixels: Vec<u8> = (0..64u32 * 64 * 4).map(|i| (i % 251) as u8).collect();
        assert_eq!(marked(&pixels, 64, Marker::None), pixels);
    }

    #[test]
    fn the_marker_is_a_dot_at_the_lower_right() {
        let grey = [200, 200, 200, 255];
        let icon = plain(64, grey);
        for (marker, rgb) in [
            (Marker::Ready, crate::theme::READY_RGB),
            (Marker::NeedsInput, crate::theme::NEEDS_INPUT_RGB),
        ] {
            let dotted = marked(&icon, 64, marker);
            assert_eq!(pixel(&dotted, 64, 52, 52), [rgb[0], rgb[1], rgb[2], 255]);
            assert_eq!(pixel(&dotted, 64, 8, 8), grey);
            // The ring's outer edge touches the right and bottom edges: 44% plus two twentieths.
            let outer = (0.44 + 0.1) * 64.0 / 2.0;
            let area = std::f64::consts::PI * outer * outer;
            let changed = dotted
                .as_chunks::<4>()
                .0
                .iter()
                .zip(icon.as_chunks::<4>().0)
                .filter(|(after, before)| after != before)
                .count() as f64;
            assert!(
                (changed - area).abs() <= area * 0.2,
                "{changed} pixels changed, the dot covers {area:.0}"
            );
        }
        // Over a clear icon the ring is black at 60%, in straight alpha.
        let clear = marked(&plain(64, [0; 4]), 64, Marker::Ready);
        assert_eq!(pixel(&clear, 64, 62, 46), [0, 0, 0, 153]);
        assert_eq!(pixel(&clear, 64, 8, 8), [0; 4]);
    }

    #[test]
    fn the_dot_reaches_the_right_and_bottom_edges() {
        let icon = plain(64, [200, 200, 200, 255]);
        for marker in [Marker::Ready, Marker::NeedsInput] {
            let dotted = marked(&icon, 64, marker);
            let changed = |x, y| pixel(&dotted, 64, x, y) != pixel(&icon, 64, x, y);
            assert!(changed(63, 46), "the right edge");
            assert!(changed(46, 63), "the bottom edge");
            assert!(!changed(63, 63), "the corner is outside the circle");
        }
    }

    const fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn ready() -> Standing {
        Standing {
            finished: 1,
            ..Standing::default()
        }
    }

    fn waiting() -> Standing {
        Standing {
            needs_you: 1,
            ..Standing::default()
        }
    }

    /// A plan whose first tray went up unmarked at the thread's start.
    fn up_unmarked() -> Plan {
        let mut plan = Plan::default();
        assert_eq!(plan.next(secs(0)), Step::PutUp(Marker::None));
        plan.tried(Marker::None, true, secs(0));
        assert_eq!(plan.next(secs(0)), Step::Wait(None));
        plan
    }

    #[test]
    fn the_first_tray_goes_up_with_the_marker_wanted_then() {
        let mut plan = Plan::default();
        plan.told(waiting(), Duration::from_millis(200));
        // No hold for the first: there is no icon to blink.
        assert_eq!(
            plan.next(Duration::from_millis(200)),
            Step::PutUp(Marker::NeedsInput)
        );
    }

    #[test]
    fn a_marker_must_hold_before_it_is_drawn() {
        let mut plan = up_unmarked();
        plan.told(ready(), secs(10));
        assert_eq!(plan.next(secs(10)), Step::Wait(Some(MARKER_HOLD)));
        assert_eq!(plan.next(secs(12)), Step::Wait(Some(secs(1))));
        assert_eq!(plan.next(secs(13)), Step::PutUp(Marker::Ready));
        plan.tried(Marker::Ready, true, secs(14));
        assert_eq!(plan.next(secs(14)), Step::Wait(None));

        // A standing that leaves the wanted marker as it is does not start the hold again.
        let mut plan = up_unmarked();
        plan.told(ready(), secs(10));
        let two = Standing {
            working: 1,
            finished: 2,
            ..Standing::default()
        };
        plan.told(two, secs(12));
        assert_eq!(plan.next(secs(13)), Step::PutUp(Marker::Ready));

        // Another marker wanted meanwhile starts it again: the icon is made once, not twice.
        let mut plan = up_unmarked();
        plan.told(ready(), secs(10));
        plan.told(waiting(), secs(12));
        assert_eq!(plan.next(secs(13)), Step::Wait(Some(secs(2))));
        assert_eq!(plan.next(secs(15)), Step::PutUp(Marker::NeedsInput));
    }

    #[test]
    fn a_marker_attended_at_once_never_reaches_the_tray() {
        let mut plan = up_unmarked();
        // A turn ends in a tile the user is not in, and they click it a second later.
        plan.told(ready(), secs(10));
        assert_eq!(plan.next(secs(10)), Step::Wait(Some(MARKER_HOLD)));
        plan.told(Standing::default(), secs(11));
        assert_eq!(plan.next(secs(11)), Step::Wait(None));
        assert_eq!(plan.next(secs(60)), Step::Wait(None), "nothing is pending");

        // Again and again, each attended within the hold: never a new tray.
        for round in 0..20 {
            let at = secs(100 + 2 * round);
            plan.told(ready(), at);
            assert!(matches!(plan.next(at), Step::Wait(Some(_))));
            plan.told(Standing::default(), at + secs(1));
            assert_eq!(plan.next(at + secs(1)), Step::Wait(None));
        }
        // And the first seconds of an earlier one do not count for a later one.
        plan.told(ready(), secs(200));
        plan.told(Standing::default(), secs(202));
        plan.told(ready(), secs(204));
        assert_eq!(plan.next(secs(205)), Step::Wait(Some(secs(2))));
    }

    #[test]
    fn a_lost_tray_is_tried_again_on_a_timer() {
        // The marker changed, the old tray was taken down and the new one could not go up.
        let mut plan = up_unmarked();
        plan.told(ready(), secs(10));
        assert_eq!(plan.next(secs(13)), Step::PutUp(Marker::Ready));
        plan.tried(Marker::Ready, false, secs(18));
        // No standing need come: the thread wakes by itself.
        assert_eq!(plan.next(secs(18)), Step::Wait(Some(TRAY_RETRY)));
        assert_eq!(plan.next(secs(40)), Step::Wait(Some(secs(8))));
        assert_eq!(plan.next(secs(48)), Step::PutUp(Marker::Ready));
        plan.tried(Marker::Ready, false, secs(53));
        assert_eq!(plan.next(secs(53)), Step::Wait(Some(TRAY_RETRY)));
        // A standing in between does not bring the try forward, and the try that comes takes
        // the marker wanted then, without a hold: there is no icon to blink.
        plan.told(waiting(), secs(82));
        assert_eq!(plan.next(secs(82)), Step::Wait(Some(secs(1))));
        assert_eq!(plan.next(secs(83)), Step::PutUp(Marker::NeedsInput));
        plan.tried(Marker::NeedsInput, true, secs(84));
        assert_eq!(plan.next(secs(84)), Step::Wait(None));

        // A first tray that could not go up is tried again the same way.
        let mut plan = Plan::default();
        assert_eq!(plan.next(secs(0)), Step::PutUp(Marker::None));
        plan.tried(Marker::None, false, secs(5));
        assert_eq!(plan.next(secs(5)), Step::Wait(Some(TRAY_RETRY)));
        assert_eq!(plan.next(secs(35)), Step::PutUp(Marker::None));
    }

    #[test]
    fn the_marked_icon_keeps_its_size() {
        for side in [22, 64] {
            let icon = plain(side, [10, 20, 30, 255]);
            for marker in [Marker::None, Marker::Ready, Marker::NeedsInput] {
                assert_eq!(marked(&icon, side, marker).len(), icon.len());
            }
        }
    }
}
