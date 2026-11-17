//! The chips row of the input box: what the user attached to the next message and has not sent
//! yet, one chip each, with its picture, its name and a control that takes it off.
//!
//! What is drawn comes from the session's chips and the thumbnails in `Ui::thumbs` alone, so a
//! scene can put chips and thumbnails in place itself.

use mr_core::{AttachKind, Chip, ChipState, SessionKey, Source};
use rune::{Color, El, SurfaceFit, col, el, icon, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant, Kit};
use rune_theme_opencode::icons::{FILE_DOCUMENT, FOLDER, PHOTO};

use crate::glue::{self, Ui};

/// The height of the row, and of each chip in it.
pub const ROW_HEIGHT: f32 = 32.0;

/// The space between two chips.
const GAP: f32 = 6.0;

/// The side of a chip's picture, and the size of the icon that stands for one.
const PICTURE: f32 = 24.0;
const PICTURE_ICON: f32 = 16.0;

/// The corners of a thumbnail.
const THUMB_RADIUS: f32 = 4.0;

/// The widest a chip's name is drawn; a longer one is cut in the middle.
const NAME_MAX: f32 = 160.0;

/// What a chip being read says after its name, and when the pointer rests on it.
const READING: &str = "Reading…";

/// How far in from an edge of the row that hides chips the fade reaches, and how far from that
/// edge a chip scrolled into view stops.
const EDGE: f32 = 16.0;

/// The key of chip `id`. Chips' ids are never given twice in a run, so it is unique in the
/// window and a scroll request finds the chip by it alone.
pub fn chip_key(id: u64) -> String {
    format!("mr-chip-{id}")
}

/// The chips of `key`'s session in a row keyed `mr-chips`, which scrolls sideways and never
/// wraps, and fades at an edge that hides chips. A chip just attached, and one whose answer is
/// a problem, is scrolled into view in it (`glue::attach`, `glue::poll`).
pub fn chips(ui: &Ui, kit: &Kit, key: SessionKey, chips: &[Chip]) -> El {
    let chips = chips.iter().map(|held| chip(ui, kit, key, held));
    col()
        .key("mr-chips")
        .w_full()
        .h(ROW_HEIGHT)
        .scroll_x()
        .scroll_edge_fade(EDGE)
        .scroll_margin(EDGE)
        .child(row().h(ROW_HEIGHT).gap(GAP).center_y().children(chips))
}

/// One chip, keyed `mr-chip-{id}`: its picture, its name, "Reading…" while it is read
/// (`mr-chip-reading-{id}`) or its problem when it has one (`mr-chip-problem-{id}`), and the
/// control that takes it off.
fn chip(ui: &Ui, kit: &Kit, key: SessionKey, held: &Chip) -> El {
    let id = held.id;
    let c = &kit.c;
    let whole = match &held.source {
        Source::Path(path) => path.display().to_string(),
        Source::Pasted(_) => held.name.clone(),
    };
    let (help, name_color) = match &held.state {
        ChipState::Reading => (vec![READING.to_owned()], c.text_muted),
        ChipState::Ready(_) => (vec![whole], c.text_base),
        ChipState::Problem(problem) => (vec![whole, problem.words().to_owned()], c.text_base),
    };
    let note = match &held.state {
        ChipState::Reading => Some(
            text(READING)
                .key(format!("mr-chip-reading-{id}"))
                .font(kit.ty.small)
                .color(c.text_muted),
        ),
        ChipState::Problem(problem) => Some(
            text(problem.words())
                .key(format!("mr-chip-problem-{id}"))
                .font(kit.ty.small)
                .color(c.danger.fg),
        ),
        ChipState::Ready(_) => None,
    };
    let name = text(held.name.clone())
        .key(format!("mr-chip-name-{id}"))
        .font(kit.ty.small)
        .color(name_color)
        .truncate_middle()
        .max_w(NAME_MAX);
    // A Tab stop that Space presses, as the send button is.
    let remove = IconButton::new(kit.glyph.close)
        .variant(IconButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .label(format!("Remove {}", held.name))
        .on_press(move |ui: &mut Ui| remove(ui, key, id))
        .build(kit)
        .key(format!("mr-chip-remove-{id}"));
    let mut chip = row()
        .key(chip_key(id))
        .h(ROW_HEIGHT)
        .pl(4.0)
        .pr(4.0)
        .gap(GAP)
        .center_y()
        .radius(kit.radius.sm)
        .fill(c.layer_02)
        .border(1.0, border(held, kit))
        .child(picture(ui, kit, held))
        .child(name)
        .children(note)
        .child(remove);
    let mut lines = help.into_iter();
    if let Some(first) = lines.next() {
        chip = chip.help(first);
    }
    lines.fold(chip, El::help_row)
}

/// The colour of a chip's border: the danger colour while it has a problem.
fn border(held: &Chip, kit: &Kit) -> Color {
    match held.state {
        ChipState::Problem(_) => kit.c.danger.fg,
        _ => kit.c.border_base,
    }
}

/// A chip's picture: an image's thumbnail filling the square (`mr-chip-thumb-{id}`), or the
/// icon of what it is, in a square keyed by it: `mr-chip-photo-{id}`, `mr-chip-folder-{id}` or
/// `mr-chip-file-{id}`.
fn picture(ui: &Ui, kit: &Kit, held: &Chip) -> El {
    let (glyph, what) = match &held.state {
        ChipState::Ready(prepared) => match prepared.kind {
            AttachKind::Image => match ui.thumbs.get(&held.id) {
                Some(thumb) if thumb.error().is_none() => {
                    return el()
                        .key(format!("mr-chip-thumb-{}", held.id))
                        .size(PICTURE, PICTURE)
                        .radius(THUMB_RADIUS)
                        .pixels_fit(thumb.pixels(), SurfaceFit::Fill)
                        .role("img")
                        .label(held.name.clone());
                }
                _ => (&PHOTO, "photo"),
            },
            AttachKind::Folder => (&FOLDER, "folder"),
            AttachKind::File => (&FILE_DOCUMENT, "file"),
        },
        _ => match held.source {
            Source::Path(_) => (&FILE_DOCUMENT, "file"),
            Source::Pasted(_) => (&PHOTO, "photo"),
        },
    };
    row()
        .key(format!("mr-chip-{what}-{}", held.id))
        .size(PICTURE, PICTURE)
        .center()
        .child(icon(glyph, PICTURE_ICON).color(kit.c.text_muted))
}

/// Takes chip `id` off `key`'s session. The keyboard goes back to the text area, from a click
/// as from a key: the control pressed is gone.
fn remove(ui: &mut Ui, key: SessionKey, id: u64) {
    ui.app.detach(key, id);
    glue::run(ui, Vec::new());
    glue::type_in(ui, key);
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use mr_core::{AttachKind, Chip, ChipState, Problem, SessionKey, Source, ToBackend};
    use rune::{ElementInfo, ImageCache, Key, Runtime};

    use crate::glue::{self, Ui};
    use crate::testkit;
    use crate::theme;
    use crate::view::input::CHIPS_HEIGHT;
    use crate::view::tiles;

    /// A file named `name` in `dir`, with a line in it.
    fn file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "a line\n").expect("a file in the test's folder");
        path
    }

    /// Attaches `sources` to `key`'s session as the window does, and runs frames until no chip
    /// is being read.
    fn attach(rt: &mut Runtime<Ui>, key: SessionKey, sources: Vec<Source>) {
        rt.update(|ui, tx| glue::attach(ui, tx, key, sources));
        settle(rt, key);
    }

    /// Runs frames until no chip of `key`'s session is being read.
    fn settle(rt: &mut Runtime<Ui>, key: SessionKey) {
        for _ in 0..20 {
            let reading = chips_of(rt, key)
                .iter()
                .any(|chip| chip.state == ChipState::Reading);
            if !reading {
                break;
            }
            testkit::step(rt);
        }
        testkit::step(rt);
    }

    fn chips_of(rt: &Runtime<Ui>, key: SessionKey) -> Vec<Chip> {
        rt.state
            .app
            .sessions
            .get(key)
            .unwrap()
            .session
            .chips
            .clone()
    }

    /// Puts `chip` on `key`'s session as it is, as a scene does, and draws it.
    fn put(rt: &mut Runtime<Ui>, key: SessionKey, chip: Chip) {
        rt.state
            .app
            .sessions
            .get_mut(key)
            .unwrap()
            .session
            .chips
            .push(chip);
        glue::run(&mut rt.state, Vec::new());
        rt.invalidate();
        testkit::step(rt);
    }

    /// The element keyed `inner` inside `key`'s tile.
    fn inside(rt: &Runtime<Ui>, key: SessionKey, inner: &str) -> ElementInfo {
        testkit::element(rt, &testkit::tile(key, inner))
            .unwrap_or_else(|| panic!("no {inner:?} in the tile"))
    }

    fn drawn(rt: &Runtime<Ui>, key: SessionKey, inner: &str) -> bool {
        testkit::element(rt, &testkit::tile(key, inner)).is_some()
    }

    #[test]
    fn a_chip_shows_its_name_and_can_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(dir.path(), "notes.md");
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        assert!(!drawn(&rt, key, "mr-chips"), "no row without chips");
        testkit::type_text(&mut rt, "look");
        attach(&mut rt, key, vec![Source::Path(path.clone())]);
        let id = chips_of(&rt, key)[0].id;
        assert!(matches!(chips_of(&rt, key)[0].state, ChipState::Ready(_)));
        let chip = inside(&rt, key, &format!("mr-chip-{id}"));
        assert!(chip.path.contains("/mr-chips/"), "{}", chip.path);
        assert_eq!(
            chip.help.as_deref(),
            Some(path.display().to_string().as_str())
        );
        let name = inside(&rt, key, &format!("mr-chip-name-{id}"));
        assert_eq!(name.text.as_deref(), Some("notes.md"));
        assert_eq!(name.text_color, Some(theme::kit(true).c.text_base));
        let remove = inside(&rt, key, &format!("mr-chip-remove-{id}"));
        assert_eq!(remove.label.as_deref(), Some("Remove notes.md"));
        assert!(testkit::box_focused(&rt, key));
        testkit::click(
            &mut rt,
            &testkit::tile(key, &format!("mr-chip-remove-{id}")),
        );
        testkit::step(&mut rt);
        assert!(chips_of(&rt, key).is_empty());
        assert!(!drawn(&rt, key, "mr-chips"));
        assert!(
            testkit::box_focused(&rt, key),
            "the keyboard stays in the box"
        );
        assert_eq!(testkit::shown_view(&rt).draft.text, "look");
    }

    #[test]
    fn a_reading_chip_says_so() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        let chip = Chip {
            id: 900,
            name: "plan.txt".into(),
            source: Source::Path("/home/alex/plan.txt".into()),
            state: ChipState::Reading,
        };
        put(&mut rt, key, chip);
        let name = inside(&rt, key, "mr-chip-name-900");
        assert_eq!(name.text.as_deref(), Some("plan.txt"));
        assert_eq!(name.text_color, Some(theme::kit(true).c.text_muted));
        assert_eq!(
            inside(&rt, key, "mr-chip-900").help.as_deref(),
            Some("Reading…")
        );
        let reading = inside(&rt, key, "mr-chip-reading-900");
        assert_eq!(reading.text.as_deref(), Some("Reading…"));
        assert_eq!(reading.text_color, Some(theme::kit(true).c.text_muted));
        assert!(
            name.rect[0] + name.rect[2] <= reading.rect[0],
            "after the name"
        );
        assert!(!drawn(&rt, key, "mr-chip-problem-900"));
        // Once read, it says nothing more.
        let mut chips = chips_of(&rt, key);
        chips[0].state = ChipState::Problem(Problem::Unreadable);
        rt.state.app.sessions.get_mut(key).unwrap().session.chips = chips;
        rt.invalidate();
        testkit::step(&mut rt);
        assert!(!drawn(&rt, key, "mr-chip-reading-900"));
        assert!(drawn(&rt, key, "mr-chip-problem-900"));
    }

    #[test]
    fn a_problem_shows_its_words() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        let chip = Chip {
            id: 901,
            name: "huge.png".into(),
            source: Source::Path("/home/alex/huge.png".into()),
            state: ChipState::Problem(Problem::TooLarge),
        };
        put(&mut rt, key, chip);
        let problem = inside(&rt, key, "mr-chip-problem-901");
        assert_eq!(problem.text.as_deref(), Some("Too large to send"));
        assert_eq!(problem.text_color, Some(theme::kit(true).c.danger.fg));
        let name = inside(&rt, key, "mr-chip-name-901");
        assert!(
            name.rect[0] + name.rect[2] <= problem.rect[0],
            "after the name"
        );
        assert_eq!(
            inside(&rt, key, "mr-chip-901").help.as_deref(),
            Some("/home/alex/huge.png\nToo large to send")
        );
        assert!(
            !testkit::element(&rt, &testkit::tile(key, "mr-send"))
                .unwrap()
                .focusable,
            "nothing can be sent"
        );
    }

    #[test]
    fn an_image_chip_shows_its_thumb() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        rt.state.images = Some(ImageCache::new(Arc::new(|| {})));
        testkit::paste_image(&mut rt, testkit::tiny_png());
        settle(&mut rt, key);
        let id = chips_of(&rt, key)[0].id;
        let thumb = rt.state.thumbs.get(&id).expect("a thumbnail").clone();
        assert_eq!(thumb.wait().ok(), Some([4, 3]));
        rt.invalidate();
        testkit::step(&mut rt);
        let picture = inside(&rt, key, &format!("mr-chip-thumb-{id}"));
        assert_eq!(&picture.rect[2..], &[24.0, 24.0]);
        assert_eq!(picture.radius, 4.0);
        assert_eq!(
            inside(&rt, key, &format!("mr-chip-{id}")).help.as_deref(),
            Some("Pasted image")
        );
        // Without its handle the chip shows the image icon instead.
        rt.state.thumbs.clear();
        rt.invalidate();
        testkit::step(&mut rt);
        assert!(!drawn(&rt, key, &format!("mr-chip-thumb-{id}")));
        // A handle is dropped once its chip is on no session.
        rt.state.thumbs.insert(id, thumb);
        testkit::click(
            &mut rt,
            &testkit::tile(key, &format!("mr-chip-remove-{id}")),
        );
        testkit::step(&mut rt);
        assert!(rt.state.thumbs.is_empty());
    }

    #[test]
    fn a_long_name_is_cut_and_the_tooltip_has_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let long = "the-minutes-of-every-meeting-about-the-parser-since-the-spring.md";
        let path = file(dir.path(), long);
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        attach(&mut rt, key, vec![Source::Path(path.clone())]);
        let id = chips_of(&rt, key)[0].id;
        let name = inside(&rt, key, &format!("mr-chip-name-{id}"));
        assert!(name.rect[2] <= 160.5, "cut at 160 points: {:?}", name.rect);
        assert!(name.rect[2] > 100.0, "{:?}", name.rect);
        assert_eq!(
            inside(&rt, key, &format!("mr-chip-{id}")).help.as_deref(),
            Some(path.display().to_string().as_str())
        );
    }

    #[test]
    fn many_chips_scroll_and_the_box_is_no_higher() {
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        let height = |rt: &Runtime<Ui>| inside(rt, key, "mr-input-box").rect[3];
        let path = |n: usize| Source::Path(format!("/home/alex/project/file-{n}.txt").into());
        let none = height(&rt);
        attach(&mut rt, key, vec![path(0)]);
        rt.settle_all();
        testkit::step(&mut rt);
        let one = height(&rt);
        assert_eq!(one - none, CHIPS_HEIGHT);
        attach(&mut rt, key, (1..12).map(path).collect());
        rt.settle_all();
        testkit::step(&mut rt);
        assert_eq!(chips_of(&rt, key).len(), 12);
        assert_eq!(height(&rt), one, "the box is no higher with twelve");
        let row = inside(&rt, key, "mr-chips");
        let frame = inside(&rt, key, "mr-input-box");
        assert_eq!(row.rect[3], 32.0);
        assert!(row.rect[0] + row.rect[2] <= frame.rect[0] + frame.rect[2]);
        let ids: Vec<u64> = chips_of(&rt, key).iter().map(|chip| chip.id).collect();
        let first = inside(&rt, key, &format!("mr-chip-{}", ids[0]));
        let second = inside(&rt, key, &format!("mr-chip-{}", ids[1]));
        let last = inside(&rt, key, &format!("mr-chip-{}", ids[11]));
        assert_eq!(second.rect[0] - (first.rect[0] + first.rect[2]), 6.0);
        assert_eq!(last.rect[1], first.rect[1], "one line: never wrapped");
        // The row scrolled to the last chip attached; the first lies before its start.
        assert!(
            last.rect[0] + last.rect[2] <= row.rect[0] + row.rect[2] + 0.5,
            "the last chip shows: {:?} in {:?}",
            last.rect,
            row.rect
        );
        assert!(
            first.rect[0] < row.rect[0],
            "scrolled past the first: {:?} in {:?}",
            first.rect,
            row.rect
        );
    }

    /// A window whose one tile is about 420 points wide, where two chips fit in the row.
    fn narrow() -> (Runtime<Ui>, SessionKey) {
        let rt = testkit::window_sized("empty", (690.0, 700.0));
        let key = testkit::focused(&rt).unwrap();
        let tile = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(tiles::tile_key(key).as_str()))
            .expect("the tile")
            .rect;
        assert!(
            (400.0..440.0).contains(&tile[2]),
            "a tile of about 420: {tile:?}"
        );
        (rt, key)
    }

    /// Whether chip `id` of `key`'s session lies inside its row as the row shows it.
    fn in_view(rt: &Runtime<Ui>, key: SessionKey, id: u64) -> bool {
        let row = inside(rt, key, "mr-chips").rect;
        let chip = inside(rt, key, &format!("mr-chip-{id}")).rect;
        chip[0] >= row[0] - 0.5 && chip[0] + chip[2] <= row[0] + row[2] + 0.5
    }

    const NAMES: [&str; 3] = [
        "meeting-notes.md",
        "the-parser-plan.txt",
        "release-checklist.md",
    ];

    #[test]
    fn a_new_chip_is_scrolled_into_view() {
        let dir = tempfile::tempdir().unwrap();
        let (mut rt, key) = narrow();
        for name in NAMES {
            attach(&mut rt, key, vec![Source::Path(file(dir.path(), name))]);
        }
        let ids: Vec<u64> = chips_of(&rt, key).iter().map(|chip| chip.id).collect();
        assert!(in_view(&rt, key, ids[2]), "the third chip shows");
        assert!(!in_view(&rt, key, ids[0]), "for which the row scrolled");
    }

    #[test]
    fn a_chip_with_a_problem_out_of_sight_is_brought_into_view() {
        let dir = tempfile::tempdir().unwrap();
        let (mut rt, key) = narrow();
        let gone = dir.path().join("gone.txt");
        let chip = Chip {
            id: 900,
            name: "gone.txt".into(),
            source: Source::Path(gone.clone()),
            state: ChipState::Reading,
        };
        put(&mut rt, key, chip);
        for name in NAMES {
            attach(&mut rt, key, vec![Source::Path(file(dir.path(), name))]);
        }
        rt.update(|_, tx| tx.scroll_to_x("mr-chips", 10_000.0));
        testkit::step(&mut rt);
        assert!(!in_view(&rt, key, 900), "out of sight at the row's start");
        rt.state
            .backend
            .as_ref()
            .expect("the demo's backend")
            .send(ToBackend::Prepare {
                chip: 900,
                source: Source::Path(gone),
            });
        settle(&mut rt, key);
        assert_eq!(
            chips_of(&rt, key)[0].state,
            ChipState::Problem(Problem::Unreadable)
        );
        assert!(in_view(&rt, key, 900), "brought into view");
    }

    #[test]
    fn a_chips_picture_shows_what_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let notes = file(dir.path(), "notes.md");
        let folder = dir.path().join("src");
        std::fs::create_dir(&folder).unwrap();
        let mut rt = testkit::window("empty");
        let key = testkit::focused(&rt).unwrap();
        rt.state.images = None;
        attach(
            &mut rt,
            key,
            vec![Source::Path(notes), Source::Path(folder)],
        );
        testkit::paste_image(&mut rt, testkit::tiny_png());
        settle(&mut rt, key);
        let chips = chips_of(&rt, key);
        let kinds: Vec<_> = chips
            .iter()
            .map(|chip| match &chip.state {
                ChipState::Ready(prepared) => prepared.kind,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            [AttachKind::File, AttachKind::Folder, AttachKind::Image]
        );
        let [file, folder, image] = [chips[0].id, chips[1].id, chips[2].id];
        assert!(drawn(&rt, key, &format!("mr-chip-file-{file}")));
        assert!(drawn(&rt, key, &format!("mr-chip-folder-{folder}")));
        assert!(rt.state.thumbs.is_empty(), "no cache, no thumbnail");
        assert!(drawn(&rt, key, &format!("mr-chip-photo-{image}")));
        assert!(!drawn(&rt, key, &format!("mr-chip-thumb-{image}")));
        // Not read yet, a path shows a file and a paste an image.
        let reading = |id: u64, source: Source| Chip {
            id,
            name: "waiting".into(),
            source,
            state: ChipState::Reading,
        };
        put(
            &mut rt,
            key,
            reading(910, Source::Path("/home/alex/src".into())),
        );
        let pasted = Source::Pasted(mr_agent::ImageData::new(testkit::tiny_png()));
        put(&mut rt, key, reading(911, pasted));
        assert!(drawn(&rt, key, "mr-chip-file-910"));
        assert!(drawn(&rt, key, "mr-chip-photo-911"));
    }

    #[test]
    fn a_problem_chip_has_the_danger_border() {
        let kit = theme::kit(true);
        let chip = |state| Chip {
            id: 1,
            name: "huge.png".into(),
            source: Source::Path("/home/alex/huge.png".into()),
            state,
        };
        assert_eq!(
            super::border(&chip(ChipState::Problem(Problem::TooLarge)), kit),
            kit.c.danger.fg
        );
        assert_eq!(
            super::border(&chip(ChipState::Reading), kit),
            kit.c.border_base
        );
    }

    #[test]
    fn the_row_lies_between_the_queued_message_and_the_text() {
        let mut rt = testkit::window("working");
        let key = testkit::focused(&rt).unwrap();
        testkit::type_text(&mut rt, "then the tests");
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        attach(
            &mut rt,
            key,
            vec![Source::Path("/home/alex/notes.md".into())],
        );
        rt.settle_all();
        testkit::step(&mut rt);
        let [queued, row, text] =
            ["mr-queued", "mr-chips", "mr-input"].map(|k| inside(&rt, key, k).rect);
        assert!(queued[1] + queued[3] <= row[1], "{queued:?} over {row:?}");
        assert!(row[1] + row[3] <= text[1], "{row:?} over {text:?}");
    }

    #[test]
    fn a_queued_image_keeps_its_thumb() {
        let mut rt = testkit::window("working");
        let key = testkit::focused(&rt).unwrap();
        rt.state.images = Some(ImageCache::new(Arc::new(|| {})));
        testkit::paste_image(&mut rt, testkit::tiny_png());
        settle(&mut rt, key);
        let id = chips_of(&rt, key)[0].id;
        assert!(rt.state.thumbs.contains_key(&id));
        testkit::press(&mut rt, Key::Enter);
        testkit::step(&mut rt);
        let session = &rt.state.app.sessions.get(key).unwrap().session;
        assert!(session.chips.is_empty());
        assert_eq!(session.queued.as_ref().map(|q| q.chips.len()), Some(1));
        glue::run(&mut rt.state, Vec::new());
        let thumb = rt.state.thumbs.get(&id).expect("kept while queued").clone();
        thumb.wait().unwrap();
        testkit::click(&mut rt, &testkit::tile(key, "mr-queued-edit"));
        testkit::step(&mut rt);
        assert_eq!(chips_of(&rt, key).len(), 1, "the chip is back");
        assert!(drawn(&rt, key, &format!("mr-chip-thumb-{id}")));
    }
}
