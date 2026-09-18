//! Sources that can be seen in three dimensions.
//!
//! Most data here is flat, and much of what looks deep is not: a sectioned
//! point cloud can hold dozens of slices without saying where any of them lay
//! in the specimen, and stacking those at a guessed spacing would draw a
//! reconstruction that never existed. So nothing is offered in 3D by default. A
//! format advertises a [`SourceVolume`] only when its own metadata places every
//! slice in space, and a frame offers to turn its view into an orbit only over
//! a source that carries one.
//!
//! What a volume looks like is the format's business. It draws its 3D form on
//! the render layer the volume was given, a layer of its own so that a frame
//! looking at it in 3D sees none of the flat geometry the same source streams
//! for its 2D frames, and nothing here knows whether that form is ray-marched
//! voxels, points or meshes.

use bevy::prelude::*;

use super::SourceRegistry;

/// A source whose data occupies real depth, and where its 3D form is drawn.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct SourceVolume {
    /// Middle of the data, in display coordinates: x right, y up, and z out of
    /// the screen, so that looking down -z is looking at it as a frame does.
    pub centre: Vec3,
    /// Extent along each axis, in the source's unit.
    pub size: Vec3,
    /// Render layer the 3D form is drawn on, apart from the source's own.
    pub layer: usize,
}

impl SourceVolume {
    /// The smallest and largest corners.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        (self.centre - self.size * 0.5, self.centre + self.size * 0.5)
    }

    /// Radius of the sphere that holds the whole volume, which is what a camera
    /// orbiting it has to keep in view whichever way it faces.
    pub fn radius(&self) -> f32 {
        (self.size.length() * 0.5).max(f32::MIN_POSITIVE)
    }
}

/// Mark `source` as occupying `size` of real depth around `centre`, and give its
/// 3D form a render layer of its own.
pub fn advertise(world: &mut World, source: Entity, centre: Vec3, size: Vec3) -> SourceVolume {
    let layer = world
        .get_resource_or_init::<SourceRegistry>()
        .allocate_layer();
    let volume = SourceVolume {
        centre,
        size,
        layer,
    };
    world.entity_mut(source).insert(volume);
    volume
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{DataSource, SourceExtent, SourceInfo, register_in};

    fn source(app: &mut App) -> Entity {
        register_in(
            app.world_mut(),
            SourceInfo {
                name: "Stack".into(),
                unit: "mm".into(),
                detail: "test".into(),
                stat: "0".into(),
            },
            SourceExtent {
                centre: Vec2::ZERO,
                size: Vec2::splat(10.0),
                finest: 0.01,
            },
        )
    }

    #[test]
    fn a_volume_draws_on_a_layer_no_source_uses() {
        // Sharing the source's own layer would put its flat tiles inside the
        // volume a 3D frame is looking at.
        let mut app = App::new();
        let stack = source(&mut app);
        let volume = advertise(app.world_mut(), stack, Vec3::ZERO, Vec3::ONE);
        let other = source(&mut app);

        let world = app.world();
        let own = world.get::<DataSource>(stack).unwrap().layer;
        let next = world.get::<DataSource>(other).unwrap().layer;
        assert_ne!(volume.layer, own);
        assert_ne!(volume.layer, next);
        assert_ne!(volume.layer, 0);
        assert_eq!(world.get::<SourceVolume>(stack), Some(&volume));
    }

    #[test]
    fn bounds_and_radius_enclose_the_whole_volume() {
        let volume = SourceVolume {
            centre: Vec3::new(1.0, -2.0, -7.0),
            size: Vec3::new(14.0, 10.0, 14.2),
            layer: 1,
        };
        let (min, max) = volume.bounds();
        assert_eq!((min + max) * 0.5, volume.centre);
        assert!((max - min - volume.size).abs().max_element() < 1e-5);
        // Every corner lies on or inside the sphere a camera frames.
        assert!((max - volume.centre).length() <= volume.radius() + 1e-4);
    }
}
