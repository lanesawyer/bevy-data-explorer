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

    /// The box around every part of the volume that `rays` pass through, or
    /// `None` when none of them reach it.
    ///
    /// What a view can see of a volume is not a slab at some depth but every
    /// point along every ray it casts, since a projection through the volume
    /// draws all of them. Rays from across a frame's viewport, clipped to the
    /// volume, are therefore what bound the part worth reading in detail —
    /// zoomed in head-on that is a narrow column through every slice, and
    /// turned side-on it is most of the specimen.
    pub fn seen_by(&self, rays: impl IntoIterator<Item = Ray3d>) -> Option<(Vec3, Vec3)> {
        let (min, max) = self.bounds();
        let mut seen: Option<(Vec3, Vec3)> = None;
        for ray in rays {
            let Some((near, far)) = crossing(&ray, min, max) else {
                continue;
            };
            for point in [ray.get_point(near), ray.get_point(far)] {
                let point = point.clamp(min, max);
                seen = Some(match seen {
                    Some((low, high)) => (low.min(point), high.max(point)),
                    None => (point, point),
                });
            }
        }
        seen
    }
}

/// Where a ray is inside the box from `min` to `max`, as distances along it,
/// counting only what lies ahead of its origin.
fn crossing(ray: &Ray3d, min: Vec3, max: Vec3) -> Option<(f32, f32)> {
    let inverse = ray.direction.as_vec3().recip();
    let a = (min - ray.origin) * inverse;
    let b = (max - ray.origin) * inverse;
    let near = a.min(b).max_element().max(0.0);
    let far = a.max(b).min_element();
    (far >= near).then_some((near, far))
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

    fn stack() -> SourceVolume {
        SourceVolume {
            centre: Vec3::ZERO,
            size: Vec3::new(14.0, 10.0, 14.0),
            layer: 1,
        }
    }

    fn ray(origin: Vec3, towards: Vec3) -> Ray3d {
        Ray3d::new(origin, Dir3::new(towards - origin).unwrap())
    }

    #[test]
    fn a_head_on_view_sees_a_column_through_every_slice() {
        // Looking straight down z at a small patch: narrow in x and y, but the
        // whole depth, since every slice along those rays is drawn.
        let volume = stack();
        let rays = [-1.0f32, 1.0].into_iter().flat_map(|x| {
            [-1.0f32, 1.0]
                .into_iter()
                .map(move |y| ray(Vec3::new(x, y, 50.0), Vec3::new(x, y, 0.0)))
        });
        let (low, high) = volume.seen_by(rays).unwrap();
        assert!((high.x - low.x - 2.0).abs() < 1e-4);
        assert!((high.y - low.y - 2.0).abs() < 1e-4);
        assert!((high.z - low.z - 14.0).abs() < 1e-4);
    }

    #[test]
    fn rays_that_miss_see_nothing() {
        let volume = stack();
        let away = ray(Vec3::new(0.0, 0.0, 50.0), Vec3::new(0.0, 0.0, 100.0));
        let beside = ray(Vec3::new(40.0, 0.0, 50.0), Vec3::new(40.0, 0.0, 0.0));
        assert!(volume.seen_by([away, beside]).is_none());
    }

    #[test]
    fn from_inside_only_what_lies_ahead_is_seen() {
        let volume = stack();
        let forward = ray(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0));
        let (low, high) = volume.seen_by([forward]).unwrap();
        assert!((high.z - 0.0).abs() < 1e-4);
        assert!((low.z + 7.0).abs() < 1e-4);
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
