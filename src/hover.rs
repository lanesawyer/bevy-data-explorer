//! What is under the pointer, asked of a source and reported back to its frame.
//!
//! This is part of the source plugin surface, alongside [`crate::datasource`].
//! The grid knows where the pointer is and which frame it is in; only a format
//! plugin knows what lives at that spot. So the two meet halfway: the grid
//! writes a [`HoverProbe`] onto the source entity of whichever frame the
//! pointer is over, each plugin answers with a [`HoverInfo`], and the overlay
//! draws whatever came back without knowing what kind of dataset produced it.
//!
//! A plugin that cannot answer simply never writes [`HoverInfo`], and no
//! tooltip appears for its frames.

use bevy::prelude::*;

/// Where the pointer is, in the world of the source a frame is showing.
///
/// Present on at most one source at a time — the one the pointer is actually
/// over — and removed as soon as the pointer leaves the grid or moves onto
/// chrome. Its absence is the signal to stop resolving, which is why plugins
/// query for it rather than for a flag inside it.
#[derive(Component, Clone, Copy, Debug)]
pub struct HoverProbe {
    /// The frame the pointer is in. Two frames can show one source at
    /// different zooms, so the answer belongs to a frame rather than a source.
    pub panel: Entity,
    /// Pointer position in display coordinates — the same space the source's
    /// geometry is spawned in, with y already negated.
    pub world: Vec2,
    /// World units per logical screen pixel in that frame, so a plugin can
    /// size its pick radius in screen terms rather than in data units.
    pub units_per_px: f32,
}

impl HoverProbe {
    /// How far from the pointer to look, given a radius in logical pixels.
    pub fn radius(&self, pixels: f32) -> f32 {
        pixels * self.units_per_px
    }
}

/// What a source found under the pointer, drawn in its frame's bottom corner.
///
/// Written by the owning plugin and left alone by everything else. An empty
/// `title` means nothing was found, which is distinct from the plugin having no
/// opinion at all — that is the component being absent.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct HoverInfo {
    /// The identifier of whatever is under the pointer, on its own line.
    pub title: String,
    /// Supporting detail, as label and value pairs.
    pub rows: Vec<(String, String)>,
}

impl HoverInfo {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.rows.is_empty()
    }

    pub fn row(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.rows.push((label.into(), value.into()));
        self
    }

    pub fn titled(title: impl Into<String>) -> Self {
        HoverInfo {
            title: title.into(),
            rows: Vec::new(),
        }
    }

    /// The tooltip's text, one row per line.
    pub fn lines(&self) -> String {
        let mut out = String::new();
        if !self.title.is_empty() {
            out.push_str(&self.title);
        }
        for (label, value) in &self.rows {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(label);
            out.push_str("  ");
            out.push_str(value);
        }
        out
    }
}

/// The set source plugins resolve the probe in.
///
/// The grid writes the probe before it and the overlay reads the answers after
/// it, so a plugin only has to declare membership rather than order itself
/// against systems in two other modules.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HoverProbing;

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> HoverProbe {
        HoverProbe {
            panel: Entity::PLACEHOLDER,
            world: Vec2::new(10.0, -20.0),
            units_per_px: 0.5,
        }
    }

    #[test]
    fn a_pick_radius_is_asked_for_in_screen_pixels() {
        // Zoomed out, a few pixels covers a lot of world; zoomed in, very
        // little. Picking has to feel the same at both.
        assert_eq!(probe().radius(6.0), 3.0);
    }

    #[test]
    fn nothing_found_is_distinct_from_a_source_with_no_opinion() {
        assert!(HoverInfo::default().is_empty());
        assert!(!HoverInfo::titled("cell 4").is_empty());
    }

    #[test]
    fn the_tooltip_reads_as_a_title_over_its_detail() {
        let info = HoverInfo::titled("L2/3 IT")
            .row("node", "0246")
            .row("x", "1.25");
        assert_eq!(info.lines(), "L2/3 IT\nnode  0246\nx  1.25");
    }

    #[test]
    fn detail_alone_still_renders_without_a_leading_blank_line() {
        // The image has no identifier to headline with, only coordinates.
        let info = HoverInfo::default().row("px", "120, 400");
        assert_eq!(info.lines(), "px  120, 400");
    }
}
