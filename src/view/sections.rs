//! A frame showing a stack's three cross-sections in 3D, where they cross.
//!
//! Neuroglancer's fourth panel: not the volume, which a light-sheet brain is
//! far too large to draw whole, but the three slices the other frames are
//! showing, each laid in space where it cuts the specimen, so turning them
//! shows how the planes meet. It is what "All views" opens beside the three
//! flat frames.
//!
//! A frame asks for it by carrying [`CrossSections`]. The slices are found
//! among the open sources as the frame's own dataset cut each way — the same
//! address with a different `#plane=` after it — so the frame needs to know
//! nothing about how they were opened, and a bookmark restores it by asking
//! again. Each slice is drawn by a camera of its own into an image, the way a
//! layer is, which also has its source stream the tiles for it; the image is
//! laid on a flat quad in 3D, on a render layer the frame's orbit looks at.
//! The slice a quad shows and where it lies both follow the source's stack,
//! so paging any linked frame moves its plane here.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::sprite_render::{AlphaMode2d, ColorMaterial};

use crate::formats::image::store::split_plane;
use crate::source::stack::{SliceStack, SourceAxes};
use crate::source::volume::SourceVolume;
use crate::source::{DataSource, ShowsSource, SourceExtent, SourceRegistry, SourceUrl};

use super::{Orbit, Panel, View};

/// The longer edge of the image a slice is drawn into, in pixels. This panel
/// is for seeing where the planes are rather than for reading them, and the
/// frames beside it hold the detail.
const SLICE_PX: u32 = 1024;

/// Before every frame's camera, so a quad shows this frame's slice rather
/// than the last.
const SLICE_CAMERA_ORDER: isize = -64;

/// A frame drawing its stack's cross-sections in 3D.
#[derive(Component, Default)]
pub struct CrossSections {
    /// The render layer the quads are drawn on, once there is one.
    layer: Option<usize>,
    /// Each slice drawn, by the source that cuts it.
    slices: Vec<Slice>,
}

struct Slice {
    source: Entity,
    camera: Entity,
    quad: Entity,
    image: Handle<Image>,
    mesh: Handle<Mesh>,
    corners: [Vec3; 4],
}

/// Belongs to the frame drawing cross-sections, and goes with it.
#[derive(Component, Clone, Copy)]
pub struct SectionOf(pub Entity);

/// Where a display coordinate lies for an axis of the specimen: x right, y
/// up and z toward the viewer, which puts y and z the other way round from
/// the data's, as a flat frame and a volume do.
fn display(axis: char, value: f32) -> Option<(usize, f32)> {
    match axis {
        'x' => Some((0, value)),
        'y' => Some((1, -value)),
        'z' => Some((2, -value)),
        _ => None,
    }
}

/// The point with `across` at `a`, `down` at `d` and `through` at `t`.
fn point(axes: &SourceAxes, through: char, (a, d, t): (f32, f32, f32)) -> Option<Vec3> {
    let mut point = Vec3::ZERO;
    for (axis, value) in [(axes.across, a), (axes.down, d), (through, t)] {
        let (at, value) = display(axis, value)?;
        point[at] = value;
    }
    Some(point)
}

/// What one source cut through a stack spans, and where its slice is.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cut {
    /// Across and down, in the data's own coordinates.
    across: (f32, f32),
    down: (f32, f32),
    /// Through: the first and last slice, and the one showing.
    through: (f32, f32),
    at: f32,
}

impl Cut {
    fn of(extent: &SourceExtent, axes: &SourceAxes, stack: &SliceStack) -> Option<Cut> {
        let through = axes.through?;
        let half = extent.size * 0.5;
        Some(Cut {
            across: (extent.center.x - half.x, extent.center.x + half.x),
            // A flat frame draws y negated, so its extent is too.
            down: (-(extent.center.y + half.y), -(extent.center.y - half.y)),
            through: (through.at(0), through.at(stack.last())),
            at: through.at(stack.current),
        })
    }

    /// The quad's corners: top left, top right, bottom left, bottom right,
    /// as the image is drawn.
    fn corners(&self, axes: &SourceAxes) -> Option<[Vec3; 4]> {
        let through = axes.through?.axis;
        let corner = |a, d| point(axes, through, (a, d, self.at));
        Some([
            corner(self.across.0, self.down.0)?,
            corner(self.across.1, self.down.0)?,
            corner(self.across.0, self.down.1)?,
            corner(self.across.1, self.down.1)?,
        ])
    }

    /// Every corner of the slab the stack fills.
    fn bounds(&self, axes: &SourceAxes) -> Option<(Vec3, Vec3)> {
        let through = axes.through?.axis;
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for a in [self.across.0, self.across.1] {
            for d in [self.down.0, self.down.1] {
                for t in [self.through.0, self.through.1] {
                    let p = point(axes, through, (a, d, t))?;
                    min = min.min(p);
                    max = max.max(p);
                }
            }
        }
        Some((min, max))
    }

    /// The image the slice is drawn into, keeping its shape, and the scale
    /// that fits the slice to it.
    fn image(&self) -> (UVec2, f32) {
        let (width, height) = (
            (self.across.1 - self.across.0).abs().max(f32::MIN_POSITIVE),
            (self.down.1 - self.down.0).abs().max(f32::MIN_POSITIVE),
        );
        let long = SLICE_PX as f32;
        let size = if width >= height {
            UVec2::new(SLICE_PX, ((long * height / width).round() as u32).max(1))
        } else {
            UVec2::new(((long * width / height).round() as u32).max(1), SLICE_PX)
        };
        (size, width.max(height) / long)
    }

    /// Where a flat camera looks to see the whole slice.
    fn center(&self) -> Vec2 {
        Vec2::new(
            f32::midpoint(self.across.0, self.across.1),
            -f32::midpoint(self.down.0, self.down.1),
        )
    }
}

fn quad(corners: [Vec3; 4]) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, corners.to_vec())
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
    )
    .with_inserted_indices(Indices::U32(vec![0, 2, 1, 1, 2, 3]))
}

fn slice_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.data = None;
    // As a layer's image: redrawn every frame, so nothing to copy on resize.
    image.copy_on_resize = false;
    image
}

/// The sources that cut the frame's dataset, one for each axis a cut pages
/// through: the dataset's address with any plane taken off, and every open
/// source whose address is that with or without a plane.
fn cuts_of(
    shown: Entity,
    urls: &Query<(Entity, &SourceUrl, Option<&SourceAxes>)>,
) -> Vec<(Entity, SourceAxes)> {
    let Ok((_, url, _)) = urls.get(shown) else {
        return Vec::new();
    };
    let base = split_plane(&url.0).0;
    let mut cuts: Vec<(Entity, SourceAxes)> = Vec::new();
    for (entity, url, axes) in urls {
        let Some(axes) = axes.filter(|axes| axes.through.is_some()) else {
            continue;
        };
        if split_plane(&url.0).0 != base {
            continue;
        }
        let along = axes.through.map(|through| through.axis);
        if !cuts
            .iter()
            .any(|(_, other)| other.through.map(|t| t.axis) == along)
        {
            cuts.push((entity, *axes));
        }
    }
    cuts
}

/// Find each frame's cuts, draw a slice for any new one, and keep every
/// slice's camera, image and quad where its stack says.
#[expect(
    clippy::too_many_arguments,
    reason = "the frames, the sources they cut, and the assets drawn from them"
)]
pub fn sync_cross_sections(
    mut commands: Commands,
    mut registry: ResMut<SourceRegistry>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut frames: Query<
        (
            Entity,
            &ShowsSource,
            &mut CrossSections,
            &Transform,
            &Projection,
            Has<Orbit>,
        ),
        With<Panel>,
    >,
    urls: Query<(Entity, &SourceUrl, Option<&SourceAxes>)>,
    sources: Query<(&DataSource, &SourceExtent, &SliceStack)>,
    mut cameras: Query<(&mut Transform, &mut Projection), (With<SectionOf>, Without<Panel>)>,
) {
    for (panel, shows, mut sections, transform, projection, orbiting) in &mut frames {
        let layer = *sections
            .layer
            .get_or_insert_with(|| registry.allocate_layer());

        for (source, axes) in cuts_of(shows.0, &urls) {
            if sections.slices.iter().any(|slice| slice.source == source) {
                continue;
            }
            let Ok((data, extent, stack)) = sources.get(source) else {
                continue;
            };
            let Some(cut) = Cut::of(extent, &axes, stack) else {
                continue;
            };
            let Some(corners) = cut.corners(&axes) else {
                continue;
            };
            let (size, scale) = cut.image();
            let image = images.add(slice_image(size));
            let order = SLICE_CAMERA_ORDER - sections.slices.len() as isize;
            let camera = commands
                .spawn((
                    Camera2d,
                    Camera {
                        order,
                        clear_color: ClearColorConfig::Custom(Color::BLACK),
                        ..default()
                    },
                    RenderTarget::Image(ImageRenderTarget {
                        handle: image.clone(),
                        scale_factor: 1.0,
                    }),
                    Projection::Orthographic(OrthographicProjection {
                        scale,
                        ..OrthographicProjection::default_2d()
                    }),
                    Transform::from_translation(cut.center().extend(1000.0)),
                    RenderLayers::layer(data.layer),
                    // What has the source stream the tiles this slice needs.
                    ShowsSource(source),
                    SectionOf(panel),
                ))
                .id();
            let mesh = meshes.add(quad(corners));
            let quad = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(materials.add(ColorMaterial {
                        texture: Some(image.clone()),
                        alpha_mode: AlphaMode2d::Opaque,
                        ..default()
                    })),
                    Transform::IDENTITY,
                    RenderLayers::layer(layer),
                    SectionOf(panel),
                ))
                .id();
            info!("cross-sections: {} placed in 3D", data.name);
            sections.slices.push(Slice {
                source,
                camera,
                quad,
                image,
                mesh,
                corners,
            });
        }

        // Keep each slice where its stack is, and its camera on the slice.
        let mut bounds: Option<(Vec3, Vec3)> = None;
        for slice in &mut sections.slices {
            let (Ok((_, extent, stack)), Ok((_, _, Some(axes)))) =
                (sources.get(slice.source), urls.get(slice.source))
            else {
                continue;
            };
            let Some(cut) = Cut::of(extent, axes, stack) else {
                continue;
            };
            if let Some((min, max)) = cut.bounds(axes) {
                bounds = Some(bounds.map_or((min, max), |(a, b)| (a.min(min), b.max(max))));
            }
            if let Some(corners) = cut.corners(axes)
                && corners != slice.corners
                && let Some(mut mesh) = meshes.get_mut(&slice.mesh)
            {
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, corners.to_vec());
                slice.corners = corners;
            }
            let (size, scale) = cut.image();
            if images
                .get(&slice.image)
                .is_some_and(|image| image.size() != size)
                && let Some(mut image) = images.get_mut(&slice.image)
            {
                image.resize(Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                });
            }
            if let Ok((mut transform, mut projection)) = cameras.get_mut(slice.camera) {
                let center = cut.center().extend(1000.0);
                if transform.translation != center {
                    transform.translation = center;
                }
                if let Projection::Orthographic(ortho) = &*projection
                    && ortho.scale != scale
                    && let Projection::Orthographic(ortho) = projection.as_mut()
                {
                    ortho.scale = scale;
                }
            }
        }

        // Turned to 3D once there is something to turn around.
        if !orbiting
            && let Some((min, max)) = bounds
            && let Projection::Orthographic(ortho) = projection
        {
            let volume = SourceVolume {
                center: (min + max) * 0.5,
                size: max - min,
                layer,
            };
            let flat = View {
                center: transform.translation.truncate(),
                scale: ortho.scale,
            };
            commands.entity(panel).insert(Orbit::fit(&volume, flat));
        }
    }
}

/// Drop the slices of a frame that has gone, or no longer draws them.
pub fn clear_cross_sections(
    mut commands: Commands,
    parts: Query<(Entity, &SectionOf)>,
    frames: Query<&CrossSections>,
) {
    for (entity, of) in &parts {
        let kept = frames.get(of.0).is_ok_and(|sections| {
            sections
                .slices
                .iter()
                .any(|slice| slice.camera == entity || slice.quad == entity)
        });
        if !kept {
            commands.entity(entity).despawn();
        }
    }
}

/// A frame turned back to 2D stops drawing cross-sections.
pub fn leave_cross_sections(
    mut commands: Commands,
    mut removed: RemovedComponents<Orbit>,
    frames: Query<(), With<CrossSections>>,
) {
    for panel in removed.read() {
        if frames.contains(panel) {
            commands.entity(panel).remove::<CrossSections>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::stack::Through;

    fn axes(across: char, down: char, through: char) -> SourceAxes {
        SourceAxes {
            across,
            down,
            through: Some(Through {
                axis: through,
                origin: 0.0,
                step: 2.0,
            }),
        }
    }

    fn extent(width: f32, height: f32) -> SourceExtent {
        SourceExtent {
            center: Vec2::new(width * 0.5, -height * 0.5),
            size: Vec2::new(width, height),
            finest: 1.0,
        }
    }

    #[test]
    fn a_slice_lies_where_its_stack_is_along_the_axis_it_pages() {
        // Looking down z at slice 10 of 50, 2 apart: the plane z = 20.
        let stack = SliceStack {
            current: 10,
            count: 50,
        };
        let xy = axes('x', 'y', 'z');
        let cut = Cut::of(&extent(100.0, 60.0), &xy, &stack).unwrap();
        let [top_left, top_right, bottom_left, _] = cut.corners(&xy).unwrap();
        assert_eq!(top_left, Vec3::new(0.0, 0.0, -20.0));
        assert_eq!(top_right, Vec3::new(100.0, 0.0, -20.0));
        assert_eq!(bottom_left, Vec3::new(0.0, -60.0, -20.0));
        let (min, max) = cut.bounds(&xy).unwrap();
        assert_eq!((min.z, max.z), (-98.0, 0.0), "the whole stack's depth");
    }

    #[test]
    fn a_cut_along_x_stands_across_the_one_down_z() {
        // z across and y down, on the slice at x = 40.
        let stack = SliceStack {
            current: 20,
            count: 50,
        };
        let zy = axes('z', 'y', 'x');
        let cut = Cut::of(&extent(98.0, 60.0), &zy, &stack).unwrap();
        let corners = cut.corners(&zy).unwrap();
        assert!(corners.iter().all(|corner| corner.x == 40.0));
        assert_eq!(corners[1], Vec3::new(40.0, 0.0, -98.0));
    }

    #[test]
    fn a_slice_is_drawn_into_an_image_of_its_own_shape() {
        let stack = SliceStack {
            current: 0,
            count: 2,
        };
        let cut = Cut::of(&extent(2000.0, 1000.0), &axes('x', 'y', 'z'), &stack).unwrap();
        let (size, scale) = cut.image();
        assert_eq!(size, UVec2::new(SLICE_PX, SLICE_PX / 2));
        assert!((scale * SLICE_PX as f32 - 2000.0).abs() < 1e-3);
        assert_eq!(cut.center(), Vec2::new(1000.0, -500.0));
    }
}
