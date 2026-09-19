//! How text is sized, in one place.
//!
//! The sizes are named for where the text sits rather than for how big it is:
//! what a caller decides is that a line is small print or a dock's title, and
//! the scale behind those names is then one edit rather than thirty.
//!
//! Sizing goes on the text itself and never on a container. Feathers' `label`
//! carries `PropagateOver<TextFont>`, so an `InheritableFont` on anything
//! above it is silently dropped and the text stays at Feathers' own 13px —
//! which is why these helpers exist rather than a size on the panel around
//! them.

use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, FontWeight};
use bevy_feathers::constants::fonts;
use bevy_feathers::display::{label, label_dim};

/// The scale text is sized from. Named for where the text sits rather than
/// for how big it is: what a caller decides is that a line is small print or
/// a dock's title.
pub mod size {
    /// Counts, notes under a truncated list, and the second line of a listing.
    pub const SMALL: f32 = 11.0;
    /// Rows of a menu or a settings screen, a step under the body text.
    pub const SECONDARY: f32 = 12.0;
    /// Feathers' own size, which a plain label already draws at.
    pub const BODY: f32 = 13.0;
    /// The dataset name on a frame's title.
    pub const FRAME_TITLE: f32 = 14.0;
    /// The name over a dock, or a heading within a screen.
    pub const DOCK_TITLE: f32 = 15.0;
    /// The name of a screen that covers the window, beside its version.
    pub const SCREEN_HEADING: f32 = 20.0;
    /// The welcome screen's own title, the largest text in the app.
    pub const SCREEN_TITLE: f32 = 32.0;
}

/// A label at `size`.
pub fn text(content: impl Into<String>, size: f32) -> impl Scene {
    let content = content.into();
    bsn! {
        label(content)
        TextFont { font_size: { FontSize::Px(size) } }
    }
}

/// A dimmed label at `size`, for anything secondary to the line above it.
pub fn text_dim(content: impl Into<String>, size: f32) -> impl Scene {
    let content = content.into();
    bsn! {
        label_dim(content)
        TextFont { font_size: { FontSize::Px(size) } }
    }
}

/// The title of a screen: [`size::SCREEN_TITLE`], and the only bold text in the app.
pub fn title(content: impl Into<String>) -> impl Scene {
    let content = content.into();
    bsn! {
        label(content)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::BOLD),
            font_size: { FontSize::Px(size::SCREEN_TITLE) },
            weight: { FontWeight::BOLD },
        }
    }
}
