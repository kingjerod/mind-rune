//! Colours and sizes. The theme is the kit's neutral one, following the system's light or
//! dark setting; the only colours named here are the black of the two scrims.

use std::sync::OnceLock;

use mr_core::Level;
use rune::Color;
use rune_kit::Kit;

/// The height of a usage line's bar.
pub const USAGE_BAR_HEIGHT: f32 = 4.0;

/// The widest the sign-in screens' text runs.
pub const SCREEN_WIDTH: f32 = 420.0;

/// How strong the scrim under a dialog is in the dark setting: black at this share.
const DARK_SCRIM: f32 = 0.6;

/// The kit's neutral theme for this setting, built once. In the dark setting the scrim under a
/// dialog is stronger than the kit's own: black at 30% hardly shows on a dark window, and the
/// dialog's edge was hard to find.
pub fn kit(dark: bool) -> &'static Kit {
    static DARK: OnceLock<Kit> = OnceLock::new();
    static LIGHT: OnceLock<Kit> = OnceLock::new();
    let cell = if dark { &DARK } else { &LIGHT };
    cell.get_or_init(|| {
        let mut kit = Kit::neutral(dark);
        if dark {
            kit.c.scrim = Color::BLACK.alpha(DARK_SCRIM);
        }
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
}
