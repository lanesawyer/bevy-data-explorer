//! Icons, drawn as glyphs from the Lucide icon font.
//!
//! A font rather than images: glyphs are rasterised at the physical size, so
//! they stay sharp at any scale factor, and they take the text colour a button
//! propagates, so hover, disabled and the light theme repaint them for free.
//! Feathers' own three icons are Lucide drawings, so the two sets match.
//!
//! The whole font is embedded so adding an icon is one variant here. Its
//! codepoints come from `font/info.json` in the `lucide-static` release the
//! font was taken from (1.47.0); they are not stable across releases, so take
//! both from the same one.

use bevy::app::PropagateOver;
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, FontWeight};
use bevy_feathers::theme::ThemeTextColor;
use bevy_feathers::tokens;

const FONT: &str = "embedded://bevy_data_explorer/widgets/assets/lucide.ttf";
const SIZE: f32 = 15.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Icon {
    Bug,
    Camera,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    CircleHelp,
    Copy,
    CopyPlus,
    Cube,
    Ellipsis,
    ExternalLink,
    FilterX,
    Info,
    Moon,
    Palette,
    PanelLeftClose,
    PanelLeftOpen,
    RotateCcw,
    ScrollText,
    Square,
    Sun,
    Trash,
    X,
}

impl Icon {
    #[cfg(test)]
    const ALL: [Icon; 23] = [
        Icon::Bug,
        Icon::Camera,
        Icon::ChevronDown,
        Icon::ChevronRight,
        Icon::ChevronUp,
        Icon::CircleHelp,
        Icon::Copy,
        Icon::CopyPlus,
        Icon::Cube,
        Icon::Ellipsis,
        Icon::ExternalLink,
        Icon::FilterX,
        Icon::Info,
        Icon::Moon,
        Icon::Palette,
        Icon::PanelLeftClose,
        Icon::PanelLeftOpen,
        Icon::RotateCcw,
        Icon::ScrollText,
        Icon::Square,
        Icon::Sun,
        Icon::Trash,
        Icon::X,
    ];

    /// The icon's character in the Lucide font, as a string so it can be
    /// compared against and written into a `Text` directly.
    pub fn glyph(self) -> &'static str {
        match self {
            Icon::Bug => "\u{e20c}",
            Icon::Camera => "\u{e064}",
            Icon::ChevronDown => "\u{e06d}",
            Icon::ChevronRight => "\u{e06f}",
            Icon::ChevronUp => "\u{e070}",
            Icon::CircleHelp => "\u{e082}",
            Icon::Copy => "\u{e09e}",
            Icon::CopyPlus => "\u{e3fd}",
            Icon::Cube => "\u{e061}",
            Icon::Ellipsis => "\u{e0b6}",
            Icon::ExternalLink => "\u{e0b9}",
            Icon::FilterX => "\u{e3b5}",
            Icon::Info => "\u{e0f9}",
            Icon::Moon => "\u{e11e}",
            Icon::Palette => "\u{e1dd}",
            Icon::PanelLeftClose => "\u{e21c}",
            Icon::PanelLeftOpen => "\u{e21d}",
            Icon::RotateCcw => "\u{e148}",
            Icon::ScrollText => "\u{e45f}",
            Icon::Square => "\u{e167}",
            Icon::Sun => "\u{e178}",
            Icon::Trash => "\u{e18e}",
            Icon::X => "\u{e1b2}",
        }
    }
}

/// An icon with no colour of its own, for a caller to name one.
///
/// `PropagateOver` for the same reason Feathers' `label` carries it: a button's
/// `InheritableFont` propagates Fira Sans to the text under it, which has no
/// glyph at these codepoints.
pub fn icon_text(icon: Icon) -> impl Scene {
    bsn! {
        Text({ icon.glyph().to_string() })
        TextFont {
            font: FontSourceTemplate::Handle(FONT),
            font_size: FontSize::Px(SIZE),
            weight: FontWeight::NORMAL,
        }
        PropagateOver<TextFont>
    }
}

/// An icon as a button's caption, painted from the button's text token for
/// the reason `button_text` is.
pub fn button_icon(icon: Icon) -> impl Scene {
    bsn! {
        icon_text(icon)
        ThemeTextColor({ tokens::BUTTON_TEXT })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_its_own_glyph_in_the_private_use_area() {
        let glyphs: std::collections::HashSet<&str> =
            Icon::ALL.iter().map(|icon| icon.glyph()).collect();
        assert_eq!(glyphs.len(), Icon::ALL.len());
        for glyph in glyphs {
            let c = glyph.chars().next().unwrap();
            assert!(('\u{e000}'..='\u{f8ff}').contains(&c));
        }
    }
}
