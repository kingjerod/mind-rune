//! A picture open large over the whole window: a press on a sent picture or on a chip's picture
//! opens it (`glue::open_picture`), in the kit's viewer, which zooms and pans it and closes on
//! Escape, a press beside the picture or its close button.

use rune::{Cx, El, Image, SurfaceFit, el, row, text};
use rune_kit::{Kit, Preview};
use rune_theme_opencode::icons::{MINUS, PLUS};

use crate::glue::{self, Ui};
use crate::theme;

/// What shows when the full picture cannot be had or cannot be decoded.
const CANNOT_READ: &str = "The full picture cannot be read.";

/// How far above the window's bottom edge that line sits: over the viewer's zoom bar.
const ERROR_BOTTOM: f32 = 56.0;

/// The open picture, last in `layer`: a dark scrim over the whole window (`mr-viewer-scrim`,
/// [`theme::viewer_scrim`]), then the kit's viewer over it, which shows the full image once it
/// is decoded and the small picture until then, a pixel a point. A screen reader is told the
/// picture is an image, enlarged. With `failed`, the line `mr-viewer-error` says that the full
/// picture cannot be read, near the bottom edge.
pub fn layer(ui: &Ui, cx: &Cx<Ui>, kit: &Kit, layer: &mut Vec<El>) {
    let Some(viewing) = &ui.viewing else {
        return;
    };
    let shown = viewing
        .full
        .as_ref()
        .filter(|full| full.is_ready() && full.error().is_none())
        .or_else(|| ui.thumbs.get(&viewing.picture));
    let natural = shown
        .and_then(Image::size)
        .map_or([1.0, 1.0], |[width, height]| [width as f32, height as f32]);
    let mut image = el()
        .key("mr-viewer-image")
        .role("img")
        .label("Image, enlarged");
    if let Some(shown) = shown {
        image = image.pixels_fit(shown.pixels(), SurfaceFit::Fit);
    }
    let window = cx.layout_size("mr-root").unwrap_or_default();
    layer.push(
        el().key("mr-viewer-scrim")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(theme::viewer_scrim())
            .a11y_hidden(),
    );
    layer.push(
        Preview::new(|ui: &mut Ui| &mut ui.zoom, natural, image)
            .icons(&MINUS, &PLUS)
            .on_close(|ui: &mut Ui, _| glue::close_picture(ui))
            .build(&ui.zoom, kit, window, cx.is_reduced_motion()),
    );
    if viewing.failed {
        // It lies outside the viewer, so a press on it closes the picture as a press beside the
        // picture does. It is plain text to a screen reader: not inert, which reads as disabled.
        layer.push(
            row()
                .key("mr-viewer-error-row")
                .pin(Some(0.0), None, Some(0.0), Some(ERROR_BOTTOM))
                .justify_center()
                .child(
                    text(CANNOT_READ)
                        .key("mr-viewer-error")
                        .font(kit.ty.body)
                        .color(kit.c.text_base)
                        .px(10.0)
                        .py(6.0)
                        .radius(6.0)
                        .fill(kit.c.layer_01),
                ),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mr_agent::{
        FakeAgent, FakeStep, HistoryItem, ImageData, SessionCommand, SessionEvent, SessionId,
        StoredHistory, StoredSession,
    };
    use mr_core::{ChipState, Session, SessionKey, Source};
    use rune::{ImageCache, Key, Runtime};

    use crate::glue::{self, Ui};
    use crate::{demo, testkit};

    /// Waits for the backend's threads, then runs frames, so every answer they gave is in.
    fn settle_backend(rt: &mut Runtime<Ui>) {
        rt.state.backend.as_ref().expect("the backend").flush();
        testkit::step(rt);
    }

    /// Attaches a pasted picture of `width` by `height` pixels to the focused session and waits
    /// for its chip and its thumbnail: the session and the chip's id.
    fn chip(rt: &mut Runtime<Ui>, width: u32, height: u32) -> (SessionKey, u64) {
        rt.state.images = Some(ImageCache::new(Arc::new(|| {})));
        let key = testkit::focused(rt).expect("a session is shown");
        let source = Source::Pasted(ImageData::new(testkit::png(width, height)));
        rt.update(|ui, tx| glue::attach(ui, tx, key, vec![source]));
        settle_backend(rt);
        let chips = &rt.state.app.sessions.get(key).unwrap().session.chips;
        let [held] = &chips[..] else {
            panic!("one chip");
        };
        assert!(matches!(held.state, ChipState::Ready(_)));
        let id = held.id;
        rt.state.thumbs[&id]
            .wait()
            .expect("a thumbnail made in the test");
        testkit::step(rt);
        (key, id)
    }

    /// In the `empty` scene, a message sent with a picture whose drawn picture is made: its
    /// session and the picture's id. The fake answers nothing, so the turn runs on.
    fn sent(rt: &mut Runtime<Ui>) -> (SessionKey, u64) {
        let (key, picture) = chip(rt, 4, 3);
        let started = FakeStep::at(0.0, SessionEvent::TurnStarted);
        rt.state.fake.as_ref().unwrap().reply(vec![started]);
        rt.update(|ui, _| {
            let effects = ui.app.submit(key, "Look");
            glue::run(ui, effects);
        });
        rt.settle_all();
        testkit::step(rt);
        assert_eq!(pictures(rt, key), [picture]);
        (key, picture)
    }

    /// The picture ids of `key`'s conversation.
    fn pictures(rt: &Runtime<Ui>, key: SessionKey) -> Vec<u64> {
        let session = &rt.state.app.sessions.get(key).unwrap().session;
        session.conversation.pictures().collect()
    }

    /// A stored session `id` whose one message of the user's, stored as `m1`, has an image of
    /// each of `images`' bytes, shown in the focused tile with its pictures come.
    fn stored(rt: &mut Runtime<Ui>, id: &str, images: Vec<Vec<u8>>) -> (SessionKey, Vec<u64>) {
        stored_in(rt, id, images, false)
    }

    /// [`stored`], shown in a tile to the side of the focused one when `side`.
    fn stored_in(
        rt: &mut Runtime<Ui>,
        id: &str,
        images: Vec<Vec<u8>>,
        side: bool,
    ) -> (SessionKey, Vec<u64>) {
        rt.state.images = Some(ImageCache::new(Arc::new(|| {})));
        let id = SessionId(id.into());
        let history = StoredHistory {
            items: vec![
                HistoryItem::User {
                    text: "Look".into(),
                    image_count: images.len(),
                    files: Vec::new(),
                    images: images
                        .into_iter()
                        .map(|bytes| Some(ImageData::new(bytes)))
                        .collect(),
                    stored: Some("m1".into()),
                },
                HistoryItem::AssistantText("Seen.".into()),
            ],
            ..StoredHistory::default()
        };
        rt.state.fake.as_ref().unwrap().set_history(&id, history);
        let stored = StoredSession {
            id,
            folder: "/home/alex/notes".into(),
            title: "Notes".into(),
            last_active_unix: demo::NOW_UNIX - demo::DAY,
        };
        let key = rt
            .state
            .app
            .sessions
            .add(Session::resting(&stored), stored.last_active_unix);
        glue::run(&mut rt.state, Vec::new());
        if side {
            testkit::show_side(rt, key);
        } else {
            testkit::show(rt, key);
        }
        settle_backend(rt);
        let ids = pictures(rt, key);
        for id in &ids {
            if let Some(handle) = rt.state.thumbs.get(id) {
                let _ = handle.clone().wait();
            }
        }
        testkit::step(rt);
        (key, ids)
    }

    /// Pauses the fake's reads and takes it out of the window, so that its frames do not wait
    /// for the backend, which waits for the read: the demo's poll flushes the backend.
    fn pause_reads(rt: &mut Runtime<Ui>) -> FakeAgent {
        let fake = rt.state.fake.take().unwrap();
        fake.pause_reads();
        fake
    }

    /// Resumes the reads `pause_reads` paused and gives the fake back to the window.
    fn resume_reads(rt: &mut Runtime<Ui>, fake: FakeAgent) {
        fake.resume_reads();
        rt.state.fake = Some(fake);
        settle_backend(rt);
    }

    /// Presses the drawn picture `picture` of `key`'s tile.
    fn press_picture(rt: &mut Runtime<Ui>, key: SessionKey, picture: u64) {
        testkit::click(
            rt,
            &testkit::tile(key, &format!("mr-user-picture-{picture}")),
        );
        testkit::step(rt);
    }

    /// Waits for the open picture's full image to be decoded, and runs frames.
    fn full_comes(rt: &mut Runtime<Ui>) {
        let full = rt.state.viewing.as_ref().unwrap().full.clone();
        let _ = full.expect("the full image's handle").wait();
        testkit::step(rt);
    }

    /// The picture open large, as its session and id.
    fn viewing(rt: &Runtime<Ui>) -> Option<(SessionKey, u64)> {
        rt.state.viewing.as_ref().map(|v| (v.key, v.picture))
    }

    fn stops(rt: &Runtime<Ui>) -> usize {
        testkit::sent(rt)
            .iter()
            .filter(|c| **c == SessionCommand::Stop)
            .count()
    }

    #[test]
    fn a_press_on_a_sent_picture_opens_it_over_the_whole_window() {
        let mut rt = testkit::window("empty");
        let (key, picture) = sent(&mut rt);
        press_picture(&mut rt, key, picture);
        assert_eq!(viewing(&rt), Some((key, picture)));
        let window = [0.0, 0.0, 1200.0, 800.0];
        assert_eq!(testkit::rect(&rt, "mr-viewer-scrim"), Some(window));
        assert_eq!(testkit::rect(&rt, "image-preview"), Some(window));
        // Its bytes were at hand: the full image shows.
        full_comes(&mut rt);
        let viewing = rt.state.viewing.as_ref().unwrap();
        assert_eq!(viewing.full.as_ref().unwrap().size(), Some([4, 3]));
        assert!(!viewing.failed);
        assert!(testkit::drawn(&rt, "mr-viewer-image"));
        assert!(!testkit::drawn(&rt, "mr-viewer-error"));
    }

    #[test]
    fn escape_closes_the_picture_and_stops_no_turn() {
        let mut rt = testkit::window("empty");
        let (key, picture) = sent(&mut rt);
        assert!(rt.state.app.busy().working > 0, "the turn runs");
        press_picture(&mut rt, key, picture);
        assert!(viewing(&rt).is_some());
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(viewing(&rt), None);
        assert!(!testkit::drawn(&rt, "mr-viewer-scrim"));
        assert_eq!(stops(&rt), 0, "no interrupt");
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(stops(&rt), 1, "the second Escape stops the turn");
    }

    #[test]
    fn a_press_beside_the_picture_closes_it() {
        let mut rt = testkit::window("empty");
        let (key, picture) = sent(&mut rt);
        press_picture(&mut rt, key, picture);
        assert!(viewing(&rt).is_some());
        // In the band over the picture, where nothing of the viewer's is.
        rt.pointer_move(600.0, 20.0);
        rt.pointer_down(600.0, 20.0);
        rt.pointer_up(600.0, 20.0);
        testkit::step(&mut rt);
        assert_eq!(viewing(&rt), None);
        assert_eq!(stops(&rt), 0);
    }

    #[test]
    fn ctrl_w_under_the_picture_closes_no_tile() {
        let mut rt = testkit::window("empty");
        let (key, picture) = sent(&mut rt);
        press_picture(&mut rt, key, picture);
        testkit::press_with(&mut rt, Key::Command('w'), testkit::ctrl());
        testkit::step(&mut rt);
        assert_eq!(testkit::tile_keys(&rt), [key]);
        assert_eq!(
            viewing(&rt),
            Some((key, picture)),
            "the picture still shows"
        );
    }

    #[test]
    fn ctrl_q_under_the_picture_asks_to_quit_and_puts_it_away() {
        let mut rt = testkit::window("empty");
        let (key, picture) = sent(&mut rt);
        press_picture(&mut rt, key, picture);
        testkit::press_with(&mut rt, Key::Command('q'), testkit::ctrl());
        testkit::step(&mut rt);
        assert_eq!(viewing(&rt), None, "the picture is put away");
        assert!(!testkit::drawn(&rt, "image-preview"));
        assert!(rt.state.quit_ask);
        assert!(testkit::drawn(&rt, "mr-quit-dialog"), "the question shows");
        assert!(!rt.state.quitting);
    }

    #[test]
    fn a_chip_picture_opens_what_will_be_sent() {
        let mut rt = testkit::window("empty");
        // Larger than a thumbnail, which is at most 320 pixels a side.
        let (key, id) = chip(&mut rt, 400, 300);
        assert_eq!(rt.state.thumbs[&id].size(), Some([320, 240]));
        testkit::click(&mut rt, &testkit::tile(key, &format!("mr-chip-thumb-{id}")));
        testkit::step(&mut rt);
        assert_eq!(viewing(&rt), Some((key, id)));
        full_comes(&mut rt);
        let full = rt.state.viewing.as_ref().unwrap().full.clone().unwrap();
        assert_eq!(full.size(), Some([400, 300]), "the image to be sent, whole");
        // The chip stays.
        let chips = &rt.state.app.sessions.get(key).unwrap().session.chips;
        assert_eq!(chips.len(), 1);
    }

    #[test]
    fn space_on_a_focused_chip_picture_opens_it() {
        let mut rt = testkit::window("empty");
        let (key, id) = chip(&mut rt, 4, 3);
        let thumb = format!("mr-chip-thumb-{id}");
        rt.update(|_, tx| tx.focus(thumb.clone(), true));
        testkit::step(&mut rt);
        let focused = rt.elements().into_iter().find(|e| e.focused);
        assert_eq!(
            focused.and_then(|e| e.key).as_deref(),
            Some(thumb.as_str()),
            "a Tab stop"
        );
        testkit::press(&mut rt, Key::Text(" ".into()));
        testkit::step(&mut rt);
        assert_eq!(viewing(&rt), Some((key, id)));
    }

    #[test]
    fn a_stored_picture_shows_small_until_the_store_answers() {
        let mut rt = testkit::window("resting");
        let (key, ids) = stored(&mut rt, "stored-one", vec![testkit::png(6, 3)]);
        let fake = pause_reads(&mut rt);
        press_picture(&mut rt, key, ids[0]);
        assert_eq!(viewing(&rt), Some((key, ids[0])));
        assert!(rt.state.viewing.as_ref().unwrap().full.is_none());
        let small = testkit::rect(&rt, "mr-viewer-image").expect("the small one shows");
        assert_eq!(&small[2..], &[6.0, 3.0], "a pixel a point");
        resume_reads(&mut rt, fake);
        full_comes(&mut rt);
        let viewing = rt.state.viewing.as_ref().unwrap();
        assert_eq!(viewing.full.as_ref().unwrap().size(), Some([6, 3]));
        assert!(!viewing.failed);
    }

    #[test]
    fn a_picture_the_store_cannot_give_says_so() {
        let mut rt = testkit::window("resting");
        let (key, ids) = stored(&mut rt, "stored-one", vec![testkit::png(6, 3)]);
        rt.state.fake.as_ref().unwrap().fail_image("gone");
        press_picture(&mut rt, key, ids[0]);
        settle_backend(&mut rt);
        assert!(rt.state.viewing.as_ref().unwrap().failed);
        assert!(testkit::has_text(&rt, "The full picture cannot be read."));
        let [x, y, w, h] = testkit::rect(&rt, "mr-viewer-error").expect("the line");
        assert!(y + h > 700.0 && y + h < 800.0, "near the bottom edge");
        assert_eq!(viewing(&rt), Some((key, ids[0])), "it stays open");
        // A screen reader reads the line as text, not as something switched off.
        let tree = rt.accessibility_json();
        let line = node(&tree, "mr-viewer-error").expect("the line is in the tree");
        assert_eq!(line["name"], "The full picture cannot be read.");
        assert!(line.get("states").is_none(), "{line}");
        // A press on the line closes the picture, as a press beside the picture does.
        let (px, py) = (x + w / 2.0, y + h / 2.0);
        rt.pointer_move(px, py);
        rt.pointer_down(px, py);
        rt.pointer_up(px, py);
        testkit::step(&mut rt);
        assert_eq!(viewing(&rt), None);
    }

    /// The node keyed `key` in `tree` or under it, its children left out.
    fn node(tree: &serde_json::Value, key: &str) -> Option<serde_json::Value> {
        if tree["key"] == key {
            let mut found = tree.clone();
            found.as_object_mut()?.remove("children");
            return Some(found);
        }
        tree["children"]
            .as_array()?
            .iter()
            .find_map(|child| node(child, key))
    }

    #[test]
    fn the_large_picture_has_a_name_for_a_screen_reader() {
        let mut rt = testkit::window("empty");
        let (key, picture) = sent(&mut rt);
        press_picture(&mut rt, key, picture);
        full_comes(&mut rt);
        let tree = rt.accessibility_json();
        let image = node(&tree, "mr-viewer-image").expect("the picture is in the tree");
        assert_eq!(image["role"], "image");
        assert_eq!(image["name"], "Image, enlarged");
    }

    #[test]
    fn bytes_that_are_no_image_say_so() {
        let mut rt = testkit::window("resting");
        let (key, ids) = stored(&mut rt, "stored-one", vec![vec![1, 2, 3, 4]]);
        assert!(!rt.state.thumbs.contains_key(&ids[0]), "no small picture");
        rt.update(|ui, _| glue::open_picture(ui, key, ids[0]));
        settle_backend(&mut rt);
        let full = rt.state.viewing.as_ref().unwrap().full.clone();
        assert!(full.expect("the bytes came").wait().is_err());
        testkit::step(&mut rt);
        assert!(rt.state.viewing.as_ref().unwrap().failed);
        assert!(testkit::has_text(&rt, "The full picture cannot be read."));
    }

    #[test]
    fn an_answer_for_another_picture_is_dropped() {
        let mut rt = testkit::window("empty");
        let (here, picture) = sent(&mut rt);
        let (beside, ids) = stored_in(&mut rt, "stored-one", vec![testkit::png(6, 3)], true);
        // The first picture's answer says it cannot be read.
        rt.state.fake.as_ref().unwrap().fail_image("gone");
        let fake = pause_reads(&mut rt);
        press_picture(&mut rt, beside, ids[0]);
        assert_eq!(viewing(&rt), Some((beside, ids[0])));
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(viewing(&rt), None);
        press_picture(&mut rt, here, picture);
        resume_reads(&mut rt, fake);
        assert_eq!(viewing(&rt), Some((here, picture)));
        full_comes(&mut rt);
        let viewing = rt.state.viewing.as_ref().unwrap();
        assert!(!viewing.failed);
        assert_eq!(viewing.full.as_ref().unwrap().size(), Some([4, 3]));
        assert!(!testkit::has_text(&rt, "The full picture cannot be read."));
    }

    #[test]
    fn a_picture_not_loaded_does_not_open() {
        let mut rt = testkit::window("empty");
        let (key, picture) = sent(&mut rt);
        rt.state.thumbs.remove(&picture);
        rt.invalidate();
        testkit::step(&mut rt);
        let wait = testkit::tile(key, &format!("mr-user-picture-wait-{picture}"));
        testkit::click(&mut rt, &wait);
        testkit::step(&mut rt);
        assert_eq!(viewing(&rt), None);
    }

    #[test]
    fn closing_the_window_closes_the_picture() {
        let mut rt = testkit::window("empty");
        let (key, picture) = sent(&mut rt);
        press_picture(&mut rt, key, picture);
        assert!(viewing(&rt).is_some());
        rt.update(glue::closing);
        assert_eq!(viewing(&rt), None);
    }

    #[test]
    fn typing_under_the_picture_reaches_no_field() {
        let mut rt = testkit::window("empty");
        let (key, picture) = sent(&mut rt);
        assert!(testkit::box_focused(&rt, key), "the keyboard is in its box");
        let before = testkit::shown_view(&rt).draft.text.clone();
        press_picture(&mut rt, key, picture);
        assert_eq!(viewing(&rt), Some((key, picture)));
        testkit::type_text(&mut rt, "abc");
        testkit::step(&mut rt);
        assert_eq!(testkit::shown_view(&rt).draft.text, before);
        assert_eq!(viewing(&rt), Some((key, picture)), "it still shows");
        // Closed, the keyboard is back where it was.
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(viewing(&rt), None);
        testkit::type_text(&mut rt, "abc");
        testkit::step(&mut rt);
        assert_eq!(
            testkit::shown_view(&rt).draft.text,
            format!("{before}abc"),
            "typed into the box again"
        );
    }

    #[test]
    fn closing_the_picture_lets_go_of_the_full_image() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        let mut rt = testkit::window("empty");
        let (key, picture) = sent(&mut rt);
        // A cache that counts what it decodes: each decode wakes once. It dedupes by content
        // while anything holds the image, so bytes loaded again are decoded again only when
        // nothing held them any more.
        let decodes = Arc::new(AtomicUsize::new(0));
        let counted = decodes.clone();
        rt.state.images = Some(ImageCache::new(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        })));
        let decoded = |count: usize| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while decodes.load(Ordering::SeqCst) < count && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            decodes.load(Ordering::SeqCst)
        };
        press_picture(&mut rt, key, picture);
        full_comes(&mut rt);
        assert_eq!(decoded(1), 1, "the full image was decoded");
        let (bytes, _) = rt.state.app.full_image(key, picture);
        let bytes = bytes.expect("a picture sent in this run is at hand");
        // While it is open, the same bytes give the image that is held: nothing is decoded.
        let cache = |rt: &Runtime<Ui>| rt.state.images.as_ref().unwrap().load(bytes.shared());
        assert!(cache(&rt).is_ready());
        assert_eq!(decoded(1), 1);
        testkit::press(&mut rt, Key::Escape);
        testkit::step(&mut rt);
        assert_eq!(viewing(&rt), None);
        // The viewer fades out: while its element is still drawn, that element holds the
        // pixels. Frames run until it is gone.
        let drawn = |rt: &Runtime<Ui>| {
            rt.elements()
                .into_iter()
                .any(|e| e.key.as_deref() == Some("mr-viewer-image"))
        };
        for _ in 0..200 {
            if !drawn(&rt) {
                break;
            }
            rt.settle_all();
            testkit::step(&mut rt);
        }
        assert!(!drawn(&rt), "the viewer is gone from the window");
        // Closed, nothing holds it: the same bytes are decoded anew.
        let again = cache(&rt);
        again.wait().expect("an image made in the test");
        assert_eq!(decoded(2), 2, "its handle was dropped with the viewer");
    }

    #[test]
    fn a_picture_whose_session_let_go_closes() {
        let mut rt = testkit::window("resting");
        let (one, ids) = stored(&mut rt, "stored-one", vec![testkit::png(4, 3)]);
        press_picture(&mut rt, one, ids[0]);
        assert_eq!(viewing(&rt), Some((one, ids[0])));
        let (_two, _) = stored(&mut rt, "stored-two", vec![testkit::png(3, 4)]);
        let session = &rt.state.app.sessions.get(one).unwrap().session;
        assert_eq!(session.history, mr_core::HistoryState::Loaded);
        assert_eq!(viewing(&rt), Some((one, ids[0])), "held for now");
        // Ten minutes out of every tile, the tick lets the conversation go.
        rt.state.unix_clock = || demo::NOW_UNIX + mr_core::LET_GO_SECONDS;
        let effects = rt.state.app.tick((rt.state.unix_clock)());
        glue::run(&mut rt.state, effects);
        let session = &rt.state.app.sessions.get(one).unwrap().session;
        assert_eq!(session.history, mr_core::HistoryState::NotLoaded);
        assert_eq!(viewing(&rt), None);
    }
}
