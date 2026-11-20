//! Colours and sizes. The theme is the kit's neutral one, following the system's light or
//! dark setting; the only colours named here are the black of the two scrims, the grey of a
//! neutral button in either setting and the icon's green and orange, with what is written on
//! them.

use std::sync::OnceLock;

use mr_core::{Level, Mark};
use rune::color::srgb_decode;
use rune::{Color, oklch};
use rune_kit::Kit;

/// The height of a usage line's bar.
pub const USAGE_BAR_HEIGHT: f32 = 4.0;

/// The widest the sign-in screens' text runs.
pub const SCREEN_WIDTH: f32 = 420.0;

/// How strong the scrim under a dialog is in the dark setting: black at this share.
const DARK_SCRIM: f32 = 0.6;

/// How light a neutral button is in the dark setting, as `oklch` takes it. The kit's own is a
/// card's fill (0.232), so a button on a card showed by its faint edge alone.
const DARK_BUTTON: f32 = 0.34;

/// How light a neutral button is in the light setting: the kit's own is a card's white.
const LIGHT_BUTTON: f32 = 0.92;

/// The kit's neutral theme for this setting, built once. In the dark setting the scrim under a
/// dialog is stronger than the kit's own: black at 30% hardly shows on a dark window, and the
/// dialog's edge was hard to find. A neutral button is a grey that stands apart from a card in
/// either setting ([`DARK_BUTTON`], [`LIGHT_BUTTON`]), in the kit's hue.
pub fn kit(dark: bool) -> &'static Kit {
    static DARK: OnceLock<Kit> = OnceLock::new();
    static LIGHT: OnceLock<Kit> = OnceLock::new();
    let cell = if dark { &DARK } else { &LIGHT };
    cell.get_or_init(|| {
        let mut kit = Kit::neutral(dark);
        if dark {
            kit.c.scrim = Color::BLACK.alpha(DARK_SCRIM);
        }
        let button = if dark { DARK_BUTTON } else { LIGHT_BUTTON };
        kit.c.bg_button_neutral = oklch(button, 0.005, 265.0);
        kit
    })
}

/// What lies between the window and a picture open large over it: black at 70%, the same in
/// both settings, so the window behind reads as behind.
pub fn viewer_scrim() -> Color {
    Color::BLACK.alpha(0.7)
}

/// A usage line's colour: muted while normal, then the kit's warning and danger colours.
pub fn level_color(kit: &Kit, level: Level) -> Color {
    match level {
        Level::Normal => kit.c.text_muted,
        Level::Warning => kit.c.warning.fg,
        Level::Alarm => kit.c.danger.fg,
    }
}

/// The green of the icon's hexagon, in sRGB: a session that is ready.
pub const READY_RGB: [u8; 3] = [0x24, 0x6b, 0x43];

/// The orange of the icon's rays, in sRGB: a session that needs input.
pub const NEEDS_INPUT_RGB: [u8; 3] = [0xf3, 0x92, 0x2e];

/// The near white written on the green.
const LIGHT_INK_RGB: [u8; 3] = [0xf7, 0xf7, 0xf7];

/// The near black written on the orange.
const DARK_INK_RGB: [u8; 3] = [0x1f, 0x1f, 0x1f];

/// A fill and the colour of what is written on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pair {
    pub fill: Color,
    pub ink: Color,
}

/// A session that is ready: the green of the icon's hexagon, with light writing.
pub fn ready() -> Pair {
    Pair {
        fill: srgb(READY_RGB),
        ink: srgb(LIGHT_INK_RGB),
    }
}

/// A session that needs input: the orange of the icon's rays, with dark writing.
pub fn needs_input() -> Pair {
    Pair {
        fill: srgb(NEEDS_INPUT_RGB),
        ink: srgb(DARK_INK_RGB),
    }
}

/// The pair of a mark, when it has one. Working and no mark have none.
pub fn pair_of(mark: Mark) -> Option<Pair> {
    match mark {
        Mark::Finished => Some(ready()),
        Mark::NeedsYou => Some(needs_input()),
        Mark::Working | Mark::None => None,
    }
}

/// The contrast between two colours, as WCAG counts it: from 1 for the same colour to 21 for
/// black on white. A `Color` holds linear light, so its luminance is WCAG's relative one.
pub fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (a.luminance() + 0.05, b.luminance() + 0.05);
    a.max(b) / a.min(b)
}

/// The kit of this setting with its muted icon colour set to the ink of `mark`'s pair, built
/// once for each pair and setting; none for a mark that has no pair. The kit's icon buttons
/// take their icon's colour from the kit, so one on a bar the pair fills, a tile's close button,
/// is built from this. It is keyed by the mark, so no other pair can be asked for.
pub fn inked(dark: bool, mark: Mark) -> Option<&'static Kit> {
    static KITS: [OnceLock<Kit>; 4] = [const { OnceLock::new() }; 4];
    let pair = pair_of(mark)?;
    let slot = usize::from(dark) * 2 + usize::from(mark == Mark::NeedsYou);
    Some(KITS[slot].get_or_init(|| {
        let mut inked = kit(dark).clone();
        inked.c.icon_muted = pair.ink;
        inked
    }))
}

/// The opaque colour of sRGB bytes.
fn srgb(bytes: [u8; 3]) -> Color {
    let [r, g, b] = bytes.map(|byte| srgb_decode(f32::from(byte) / 255.0));
    Color { r, g, b, a: 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scrims_are_dark_enough_to_set_what_lies_over_the_window_apart() {
        let black = |color: Color| (color.r, color.g, color.b) == (0.0, 0.0, 0.0);
        let (dark, light) = (kit(true).c.scrim, kit(false).c.scrim);
        assert!(black(dark) && black(light));
        assert!(dark.a >= 0.5, "the dialog's edge shows on a dark window");
        assert_eq!(light, Kit::neutral(false).c.scrim, "the kit's own");
        // Under a picture, the same in both settings and stronger than under a dialog.
        let viewer = viewer_scrim();
        assert!(black(viewer));
        assert!((viewer.a - 0.7).abs() < 1e-6);
        assert!(viewer.a > dark.a && viewer.a > light.a);
    }

    #[test]
    fn a_neutral_button_stands_apart_in_either_setting() {
        let dark = kit(true);
        let button = dark.c.bg_button_neutral;
        for under in [dark.c.bg_base, dark.c.page] {
            assert!(
                button.luminance() > under.luminance(),
                "lighter than {under:?}"
            );
            let ratio = contrast(button, under);
            assert!(ratio >= 1.3, "a contrast of {ratio} with {under:?}");
        }
        assert!(contrast(button, dark.c.text_base) >= 4.5, "its label reads");
        // In the light setting it is darker than a card, and than the page beside one.
        let light = kit(false);
        let button = light.c.bg_button_neutral;
        assert!(contrast(button, light.c.bg_base) >= 1.2);
        assert!(button.luminance() < light.c.page.luminance());
        assert!(
            contrast(button, light.c.text_base) >= 4.5,
            "its label reads"
        );
    }

    #[test]
    fn the_pairs_can_be_read() {
        for pair in [ready(), needs_input()] {
            let ratio = contrast(pair.fill, pair.ink);
            assert!(ratio >= 4.5, "{pair:?} has a contrast of {ratio}");
            assert_eq!(contrast(pair.ink, pair.fill), ratio, "either way round");
        }
        assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 1e-3);
        assert!(ready().ink.luminance() > 0.8, "light writing on the green");
        assert!(
            needs_input().ink.luminance() < 0.05,
            "dark writing on the orange"
        );
    }

    #[test]
    fn the_fills_are_the_icons_colours() {
        let bytes = |color: Color| {
            assert_eq!(color.a, 1.0);
            [color.r, color.g, color.b].map(|c| rune::color::srgb_encode(c) * 255.0)
        };
        let icon: [(Color, [u8; 3]); 2] = [
            (ready().fill, [0x24, 0x6b, 0x43]),
            (needs_input().fill, [0xf3, 0x92, 0x2e]),
        ];
        for (fill, icon) in icon {
            for (found, want) in bytes(fill).into_iter().zip(icon) {
                assert!(
                    (found - f32::from(want)).abs() < 0.5,
                    "{fill:?} is {icon:02x?}"
                );
            }
        }
    }

    #[test]
    fn the_inked_kits_are_made_once() {
        let of = |dark, mark| inked(dark, mark).expect("a mark with a pair");
        for dark in [true, false] {
            for mark in [Mark::Finished, Mark::NeedsYou] {
                let pair = pair_of(mark).unwrap();
                let inked = of(dark, mark);
                assert!(std::ptr::eq(inked, of(dark, mark)), "made once");
                assert_eq!(inked.c.icon_muted, pair.ink);
                assert_eq!(inked.dark, dark);
                assert_eq!(inked.c.hover, kit(dark).c.hover, "the rest is the theme's");
            }
            assert!(!std::ptr::eq(
                of(dark, Mark::Finished),
                of(dark, Mark::NeedsYou)
            ));
            // A mark with no pair has no kit of its own: nothing can land in another's slot.
            assert!(inked(dark, Mark::Working).is_none());
            assert!(inked(dark, Mark::None).is_none());
        }
        assert!(!std::ptr::eq(
            of(true, Mark::Finished),
            of(false, Mark::Finished)
        ));
    }

    #[test]
    fn only_ready_and_needs_input_have_a_pair() {
        assert_eq!(pair_of(Mark::Finished), Some(ready()));
        assert_eq!(pair_of(Mark::NeedsYou), Some(needs_input()));
        assert_eq!(pair_of(Mark::Working), None);
        assert_eq!(pair_of(Mark::None), None);
    }
}
