//! The interface between a data source and the frames that display it.
//!
//! Every format is a Bevy plugin. On build it registers itself as a *source
//! entity*, which carries the handful of things a panel needs to know about
//! whatever it is showing: a name, the render layer its geometry is drawn on,
//! how much world it occupies, and a line of status for the overlay.
//!
//! Panels refer to a source by entity rather than by a format enum, which is
//! what keeps `panel` and `hud` from knowing anything about OME-Zarr or
//! Scatterbrain. Swapping what a frame displays is then a matter of pointing it
//! at a different entity and moving its camera onto that source's layer.

pub mod hover;
pub mod properties;

use bevy::prelude::*;

use crate::view::ViewLimits;

/// A loaded dataset that a frame can display.
#[derive(Component, Debug, Clone)]
pub struct DataSource {
    /// Shown as the first line of the panel overlay.
    pub name: String,
    /// Physical unit of the source's coordinates, for reporting zoom.
    pub unit: String,
    /// What kind of data this is, for listings that show more than the name.
    pub detail: String,
    /// The headline figure for this dataset, e.g. its point count.
    pub stat: String,
    /// Render layer this source's geometry is drawn on. Allocated at
    /// registration so that two sources can never collide.
    pub layer: usize,
}

/// What a plugin declares about its dataset when registering.
pub struct SourceInfo {
    pub name: String,
    pub unit: String,
    pub detail: String,
    pub stat: String,
}

/// How much world a source occupies, in display coordinates, used to frame a
/// panel that opens onto it.
#[derive(Component, Clone, Copy)]
pub struct SourceExtent {
    pub centre: Vec2,
    pub size: Vec2,
    /// Finest meaningful detail, in world units per pixel. Sets how far a
    /// panel may zoom in before it is magnifying beyond the data.
    pub finest: f32,
}

impl SourceExtent {
    pub fn limits(&self, viewport: Vec2) -> ViewLimits {
        ViewLimits::fit(self.centre, self.size.x, self.size.y, viewport, self.finest)
    }
}

/// Status text for the overlay, rewritten each frame by the owning plugin.
///
/// Deliberately excludes anything view-dependent: one source may be shown in
/// several frames at different zooms, so the panel-specific lines are added by
/// the overlay rather than baked in here.
#[derive(Component, Default)]
pub struct SourceStatus(pub String);

/// Hands out render layers, one per source.
///
/// Layer 0 is left alone: it is the default layer, and anything spawned without
/// an explicit `RenderLayers` lands there and would show up in every frame.
#[derive(Resource)]
pub struct SourceRegistry {
    next_layer: usize,
}

impl Default for SourceRegistry {
    fn default() -> Self {
        SourceRegistry { next_layer: 1 }
    }
}

impl SourceRegistry {
    pub fn allocate_layer(&mut self) -> usize {
        let layer = self.next_layer;
        self.next_layer += 1;
        layer
    }
}

/// Register a source and spawn its entity, returning it so the plugin can bind
/// its streamer to it.
pub fn register(app: &mut App, info: SourceInfo, extent: SourceExtent) -> Entity {
    let layer = app
        .world_mut()
        .get_resource_or_init::<SourceRegistry>()
        .allocate_layer();

    app.world_mut()
        .spawn((
            DataSource {
                name: info.name,
                unit: info.unit,
                detail: info.detail,
                stat: info.stat,
                layer,
            },
            extent,
            SourceStatus::default(),
        ))
        .id()
}

/// Format a count the way dataset listings do: 3_739_961 as "3.74M".
pub fn compact_count(value: u64) -> String {
    match value {
        n if n >= 1_000_000_000 => format!("{:.2}B", n as f64 / 1e9),
        n if n >= 1_000_000 => format!("{:.2}M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.1}K", n as f64 / 1e3),
        n => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_are_unique_and_never_the_default() {
        let mut registry = SourceRegistry::default();
        let layers: Vec<usize> = (0..8).map(|_| registry.allocate_layer()).collect();

        // Layer 0 catches anything spawned without an explicit layer, so a
        // source must never be handed it.
        assert!(!layers.contains(&0));

        let unique: std::collections::HashSet<_> = layers.iter().collect();
        assert_eq!(
            unique.len(),
            layers.len(),
            "sources would bleed into each other"
        );
    }

    fn extent() -> SourceExtent {
        SourceExtent {
            centre: Vec2::ZERO,
            size: Vec2::splat(10.0),
            finest: 0.01,
        }
    }

    fn info(name: &str, unit: &str) -> SourceInfo {
        SourceInfo {
            name: name.into(),
            unit: unit.into(),
            detail: "test".into(),
            stat: "0".into(),
        }
    }

    #[test]
    fn counts_read_the_way_dataset_listings_show_them() {
        assert_eq!(compact_count(3_739_961), "3.74M");
        assert_eq!(compact_count(4_042_976), "4.04M");
        assert_eq!(compact_count(1_420_000), "1.42M");
        assert_eq!(compact_count(12_345), "12.3K");
        assert_eq!(compact_count(999), "999");
    }

    #[test]
    fn registering_a_source_spawns_it_with_its_own_layer() {
        let mut app = App::new();
        let first = register(&mut app, info("First", "mm"), extent());
        let second = register(&mut app, info("Second", "um"), extent());

        let world = app.world();
        let first = world.get::<DataSource>(first).unwrap();
        let second = world.get::<DataSource>(second).unwrap();

        assert_eq!(first.name, "First");
        assert_eq!(second.unit, "um");
        // Two plugins registering back to back must not share a layer, or each
        // would draw into the other's frames.
        assert_ne!(first.layer, second.layer);
        assert_ne!(first.layer, 0);
    }

    #[test]
    fn a_registered_source_can_report_status_immediately() {
        // The overlay reads this component every frame, so it has to exist from
        // registration rather than appearing once the plugin first runs.
        let mut app = App::new();
        let source = register(&mut app, info("Thing", "mm"), extent());
        assert!(app.world().get::<SourceStatus>(source).is_some());
        assert!(app.world().get::<SourceExtent>(source).is_some());
    }

    #[test]
    fn extent_frames_the_whole_source() {
        let extent = SourceExtent {
            centre: Vec2::new(5.0, -5.0),
            size: Vec2::new(40.0, 40.0),
            finest: 0.001,
        };
        let limits = extent.limits(Vec2::new(800.0, 400.0));
        assert_eq!(limits.centre, Vec2::new(5.0, -5.0));
        // Limited by height: 40 world units across 400 pixels.
        assert!((limits.fit_scale - 0.1).abs() < 1e-6);
    }
}
