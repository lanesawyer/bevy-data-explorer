//! Feathers' dark theme, with the button states pushed further apart.

use bevy::prelude::*;
use bevy_feathers::theme::ThemeProps;

/// Widen the button state tokens.
///
/// Feathers' hover is a five percent lift in lightness, which is hard to see at
/// all over a frame's imagery and reads as a button that does not respond. The
/// tokens are widened rather than each button being styled by hand, so every
/// control in the app moves together.
pub fn dark() -> ThemeProps {
    use bevy_feathers::{dark_theme::create_dark_theme, palette, tokens};

    let mut theme = create_dark_theme();
    theme
        .color
        .insert(tokens::BUTTON_BG_HOVER, palette::GRAY_3.lighter(0.14));
    theme
        .color
        .insert(tokens::BUTTON_BG_PRESSED, palette::ACCENT.darker(0.12));
    theme.color.insert(
        tokens::BUTTON_PRIMARY_BG_HOVER,
        palette::ACCENT.lighter(0.08),
    );
    theme
}
