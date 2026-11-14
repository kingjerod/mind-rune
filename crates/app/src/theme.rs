//! Colours and sizes. The theme is the kit's neutral one, following the system's light or
//! dark setting; nothing here names a colour of its own.

use std::sync::OnceLock;

use mr_core::Level;
use rune::Color;
use rune_kit::Kit;

/// The height of a usage line's bar.
pub const USAGE_BAR_HEIGHT: f32 = 4.0;

/// The widest the sign-in screens' text runs.
pub const SCREEN_WIDTH: f32 = 420.0;

/// The kit's neutral theme for this setting, built once.
pub fn kit(dark: bool) -> &'static Kit {
    static DARK: OnceLock<Kit> = OnceLock::new();
    static LIGHT: OnceLock<Kit> = OnceLock::new();
    let cell = if dark { &DARK } else { &LIGHT };
    cell.get_or_init(|| Kit::neutral(dark))
}

/// A usage line's colour: muted while normal, then the kit's warning and danger colours.
pub fn level_color(kit: &Kit, level: Level) -> Color {
    match level {
        Level::Normal => kit.c.text_muted,
        Level::Warning => kit.c.warning.fg,
        Level::Alarm => kit.c.danger.fg,
    }
}
