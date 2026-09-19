use bevy::prelude::*;
use bevy_feathers::display::label;
use bevy_feathers::theme::ThemeTextColor;
use bevy_feathers::tokens;

/// A button's caption, painted from the button's own text token.
///
/// Feathers' `label` carries `ThemeTextColor(TEXT_MAIN)` on the text itself,
/// and a color named directly on a span beats the one a button propagates to
/// it. In the dark theme both are pale so nothing looked wrong; in the light
/// one the window's text color is dark, and every button wore dark text over a
/// background that had stayed dark. Naming the button token on the caption is
/// what puts it back — and what the theme repaints when it changes.
pub fn button_text(text: impl Into<String>) -> impl Scene {
    let text = text.into();
    bsn! {
        label(text)
        ThemeTextColor({ tokens::BUTTON_TEXT })
    }
}
