//! The message field of an input box: Rune's own multi-line text field, dressed as the kit's
//! text area (`TextArea::build` in `crates/rune-kit/src/text_input.rs` in the Rune checkout).
//!
//! The kit's text area lets no paste hook reach the field inside it, so this app builds the
//! field itself and can hear a paste of an image or of copied files. Its look copies the kit's
//! on purpose, helpers the kit keeps private included: a change of the kit's text area does
//! not reach it.

use rune::{El, RichStyle, TextEdit, TextStyle, col, text_input};
use rune_kit::Kit;

/// The space between the field's edge and its text.
pub const PAD: f32 = 8.0;

/// The height of a line of text: the kit's `13 * 1.35`, rounded to half a point.
pub const LINE_HEIGHT: f32 = 17.5;

/// The type of the field's text and placeholder.
pub fn style(kit: &Kit) -> TextStyle {
    TextStyle {
        line_height: LINE_HEIGHT,
        ..kit.ty.ui
    }
}

/// The field keyed `mr-input` showing `edit`, `lines.0` lines tall at least, growing with each
/// wrapped line up to `lines.1`, then scrolling inside with the caret kept in view after each
/// key; its height springs on the kit's control motion. The editor inside, keyed `input` in a
/// viewport keyed `scroll`, is named `label` (else `placeholder`) for assistive technology and
/// gets its handlers from `handlers`, unless `disabled`: then it takes none and the field is
/// dimmed.
pub fn field(
    kit: &Kit,
    edit: &TextEdit,
    placeholder: &str,
    label: &str,
    lines: (usize, usize),
    disabled: bool,
    handlers: impl FnOnce(El) -> El,
) -> El {
    let c = &kit.c;
    let name = if label.is_empty() { placeholder } else { label };
    let mut editor = text_input(edit, style(kit))
        .key("input")
        .w_full()
        .color(c.text_base)
        .rich_style(rich_style(kit))
        .placeholder(placeholder.to_owned(), c.text_faint)
        .caret(c.text_base, kit.selection());
    if !name.is_empty() {
        editor = editor.label(name);
    }
    editor = editor.description("").disabled(disabled);
    if !disabled {
        editor = handlers(editor);
    }
    let shell = col()
        .role("textbox")
        .padding(PAD)
        .child(rune::auto_grow(editor, lines.0, lines.1))
        .w_full();
    dress(kit, shell, disabled, edit.focused && !disabled).key("mr-input")
}

/// The kit's colours for links, code, quotes and list markers in a field (its private
/// `kit_rich_style`).
fn rich_style(kit: &Kit) -> RichStyle {
    let c = &kit.c;
    RichStyle {
        link: c.text_accent,
        code_fg: None,
        code_bg: c.layer_03,
        code_block_bg: c.layer_02,
        quote_bar: c.border_strong,
        marker: Some(c.text_muted),
    }
}

/// The kit's field shell (its private `shell`, for a field that is never invalid nor forced to
/// a state): the base fill and the input elevation, a hover tint, a 1 point focus ring just
/// inside the box in place of the shadow, and dimming when disabled.
fn dress(kit: &Kit, e: El, disabled: bool, focused: bool) -> El {
    let c = &kit.c;
    let e = e
        .radius(kit.radius.md)
        .fill(c.bg_base)
        .token_motion(kit.motion.control);
    if disabled {
        return shadows(kit, e).layer_opacity(0.5);
    }
    if focused {
        return e.outline(1.0, -1.0, c.border_focus);
    }
    shadows(kit, e).hover(c.hover)
}

/// The input elevation's shadows on `e` (the kit's private `shadows`).
fn shadows(kit: &Kit, mut e: El) -> El {
    for shadow in kit.shadow_layers(kit.elevation.input()) {
        e = e.box_shadow(shadow);
    }
    e
}
