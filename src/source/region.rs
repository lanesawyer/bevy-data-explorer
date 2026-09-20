//! A rectangle of a frame, asked of a source and answered in its own
//! coordinates.
//!
//! This is part of the source plugin surface, alongside [`crate::source`], and
//! it meets halfway exactly as [`super::hover`] does. The grid knows what the
//! user dragged out and which frame it was dragged in; only a format plugin
//! knows where that lands in the dataset, because only it knows how its points
//! were laid out — a single cloud draws them at their own coordinates, while a
//! sectioned one pulls its slides apart into a grid.
//!
//! So the grid writes a [`RegionProbe`] onto the source entity of the frame the
//! rectangle was drawn in, and the plugin answers with a [`SelectedRegion`] in
//! the coordinates whatever counts those cells indexed them by. A plugin that
//! cannot answer never writes one, and nothing is counted for its frames.

use bevy::prelude::*;

/// A rectangle dragged out over a frame, in display coordinates — the space the
/// source's geometry is spawned in, with y already negated.
///
/// Present on at most one source at a time, and removed as soon as the
/// selection is cleared. Its absence is the signal to stop answering, which is
/// why plugins query for it rather than for a flag inside it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct RegionProbe {
    /// The frame it was drawn in. Two frames can show one source at different
    /// zooms, so a region belongs to a frame rather than to a source.
    pub panel: Entity,
    pub min: Vec2,
    pub max: Vec2,
}

/// A region in the dataset's own coordinates, as a service counting its cells
/// names them.
///
/// Written by the owning plugin and left alone by everything else.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct SelectedRegion {
    /// What the counting service knows this set of coordinates by. Only the
    /// format can say: it is read from the dataset's own metadata, and a
    /// service that indexes several visualizations of one dataset tells them
    /// apart by it.
    pub key: String,
    pub min: Vec2,
    pub max: Vec2,
}

impl SelectedRegion {
    /// The region as a corner pair, whichever way round it was dragged.
    pub fn corners(&self) -> ([f32; 2], [f32; 2]) {
        (
            [self.min.x.min(self.max.x), self.min.y.min(self.max.y)],
            [self.min.x.max(self.max.x), self.min.y.max(self.max.y)],
        )
    }

    /// Whether it covers enough to be worth counting. A click that moved a
    /// pixel is a click, not a selection, and counting it would ask a service
    /// for an empty answer.
    pub fn is_empty(&self) -> bool {
        let (min, max) = self.corners();
        max[0] - min[0] <= f32::EPSILON || max[1] - min[1] <= f32::EPSILON
    }
}

/// Turn a probe in display coordinates into the dataset's own, for a source
/// whose points are drawn where their coordinates put them.
///
/// Display space negates y, so the two corners swap over in y as well as being
/// mirrored — which is why this is worth doing in one place rather than in each
/// format that needs it.
pub fn region_of(probe: &RegionProbe, offset: Vec2, key: impl Into<String>) -> SelectedRegion {
    let a = probe.min - offset;
    let b = probe.max - offset;
    let low = a.min(b);
    let high = a.max(b);
    SelectedRegion {
        key: key.into(),
        // Negating y swaps which corner is the low one, so the two are put
        // back in order rather than negated where they stand.
        min: Vec2::new(low.x, -high.y),
        max: Vec2::new(high.x, -low.y),
    }
}

/// The set source plugins answer a region in.
///
/// The grid writes the probe before it and the counting reads the answers
/// after it, so a plugin only has to declare membership rather than order
/// itself against systems in two other modules.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegionResolving;

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(min: Vec2, max: Vec2) -> RegionProbe {
        RegionProbe {
            panel: Entity::PLACEHOLDER,
            min,
            max,
        }
    }

    #[test]
    fn a_region_is_mirrored_back_out_of_display_space() {
        // A drag from the top left of the screen downward covers world y from
        // -30 to -10, which is dataset y from 10 to 30. Forgetting the flip
        // would ask a service about a band the data has nothing in.
        let region = region_of(
            &probe(Vec2::new(5.0, -10.0), Vec2::new(15.0, -30.0)),
            Vec2::ZERO,
            "V",
        );
        assert_eq!(region.min, Vec2::new(5.0, 10.0));
        assert_eq!(region.max, Vec2::new(15.0, 30.0));
        assert_eq!(region.key, "V");
    }

    #[test]
    fn dragging_either_way_round_gives_the_same_region() {
        let down = region_of(
            &probe(Vec2::new(5.0, -10.0), Vec2::new(15.0, -30.0)),
            Vec2::ZERO,
            "V",
        );
        let up = region_of(
            &probe(Vec2::new(15.0, -30.0), Vec2::new(5.0, -10.0)),
            Vec2::ZERO,
            "V",
        );
        assert_eq!(down.corners(), up.corners());
    }

    #[test]
    fn a_laid_out_slide_is_brought_back_to_its_own_coordinates() {
        // A sectioned dataset pulls its slides apart, so the same drag over
        // two slides has to mean each slide's own coordinates.
        let offset = Vec2::new(100.0, -40.0);
        let region = region_of(
            &probe(Vec2::new(105.0, -50.0), Vec2::new(115.0, -70.0)),
            offset,
            "V",
        );
        assert_eq!(region.min, Vec2::new(5.0, 10.0));
        assert_eq!(region.max, Vec2::new(15.0, 30.0));
    }

    #[test]
    fn a_drag_that_went_nowhere_is_not_a_selection() {
        let click = region_of(
            &probe(Vec2::new(5.0, -10.0), Vec2::new(5.0, -10.0)),
            Vec2::ZERO,
            "V",
        );
        assert!(click.is_empty());
        assert!(!region_of(&probe(Vec2::ZERO, Vec2::new(1.0, -1.0)), Vec2::ZERO, "V").is_empty());
    }
}
