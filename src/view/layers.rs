//! Several sources drawn in one frame, one over another.
//!
//! A frame's own camera draws the source it was opened onto. Every further
//! layer is a camera of its own, sharing the frame's view and projection and
//! drawing its source's render layer — but into an image the size of the
//! frame rather than into the window. That image is then laid over the frame
//! as a UI image, in stack order, at the layer's opacity. Nothing a format
//! spawns has to know which layer it ended up in, and one source can be the
//! base of one frame and a layer of another at the same time.
//!
//! The image is what makes a layer see-through. Everything a source draws
//! overdraws itself — an image keeps coarser tiles under finer ones, a point
//! cloud stacks dozens of points on a pixel — so fading its geometry directly
//! compounds wherever it overlaps, and why fading a source everywhere else
//! dims it instead. Drawn whole into its own image first, a layer's overlaps
//! are settled before any transparency is applied, and the frame beneath shows
//! through evenly.
//!
//! A layer camera also carries [`ShowsSource`], which is the whole of what a
//! streamer asks of a view. So a source streams for a frame it is layered into
//! exactly as it does for a frame of its own, without its plugin changing.
//!
//! Nothing is rescaled: a layer is drawn in its own coordinates, so two
//! sources line up when they were measured alike. Any source may still be
//! layered over any other; one measured differently is flagged, not refused.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};

use super::grid::{MAX_LAYERS, camera_order};
use super::{Panel, ShowsSource};
use crate::source::DataSource;

/// A camera drawing one extra source over a frame.
#[derive(Component, Clone, Copy, Debug)]
#[relationship(relationship_target = FrameLayers)]
pub struct LayerOf(pub Entity);

/// The layers stacked over a frame, bottom first.
///
/// Despawning the frame despawns its layers, so closing a frame cannot leave
/// a camera behind drawing into the cell it used to occupy.
#[derive(Component, Debug, Default)]
#[relationship_target(relationship = LayerOf, linked_spawn)]
pub struct FrameLayers(Vec<Entity>);

impl FrameLayers {
    /// The layer cameras, bottom first.
    pub fn cameras(&self) -> &[Entity] {
        &self.0
    }
}

/// How opaque a layer is laid over what is beneath it.
///
/// On the layer rather than on its source: how strongly a dataset is overlaid
/// is a property of the stack it is in, and the same dataset may be a faint
/// layer in one frame and fully shown in another.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct LayerOpacity(pub f32);

impl Default for LayerOpacity {
    fn default() -> Self {
        LayerOpacity(1.0)
    }
}

/// The image a layer camera draws into, once it has a frame to be sized to.
#[derive(Component, Clone, Debug)]
pub struct LayerImage {
    pub handle: Handle<Image>,
    /// The window scale factor the target was made for, so a move to a
    /// display with another one retargets it.
    scale_factor: f32,
}

/// The UI image laying a layer over its frame.
#[derive(Component, Clone, Copy, Debug)]
#[relationship(relationship_target = LayerComposite)]
pub struct CompositeOf(pub Entity);

/// The UI image laying this layer over its frame. Despawned with the layer.
#[derive(Component, Debug, Default)]
#[relationship_target(relationship = CompositeOf, linked_spawn)]
pub struct LayerComposite(Vec<Entity>);

/// Where layer images sit among the UI roots: below everything, since every
/// other root is chrome drawn over the frames. Each layer adds its depth, so
/// higher layers draw over lower ones.
const COMPOSITE_Z: i32 = -(MAX_LAYERS as i32) - 1;

/// Marks a source named on the command line as a layer rather than a frame.
///
/// It is stacked onto the first frame when the frames open, and gets a frame of
/// its own only when nothing else named one.
#[derive(Component, Clone, Copy, Default)]
pub struct OpensAsLayer;

/// Spawn a camera drawing `source` over `panel`.
///
/// Its image, view and order are left to [`sync_layers`], which writes them
/// every frame from the frame it belongs to.
pub fn spawn_layer(
    commands: &mut Commands,
    panel: Entity,
    source: Entity,
    layer: usize,
    opacity: LayerOpacity,
) -> Entity {
    commands
        .spawn((
            Camera2d,
            Camera {
                // Clears its own image, to nothing, so that where the layer
                // draws nothing the frame beneath shows through. It never
                // draws into the window, so cell 0 is still the only camera
                // that clears that.
                clear_color: ClearColorConfig::Custom(Color::NONE),
                // Inactive until it has an image, or it would draw once over
                // the whole window.
                is_active: false,
                ..default()
            },
            RenderLayers::layer(layer),
            ShowsSource(source),
            LayerOf(panel),
            opacity,
        ))
        .id()
}

/// The sources a frame is showing, bottom first: the one it opened onto, then
/// each layer over it.
pub fn stacked_sources(
    shows: &ShowsSource,
    layers: Option<&FrameLayers>,
    cameras: &Query<&ShowsSource, With<LayerOf>>,
) -> Vec<Entity> {
    let mut stack = vec![shows.0];
    if let Some(layers) = layers {
        stack.extend(
            layers
                .cameras()
                .iter()
                .filter_map(|camera| cameras.get(*camera).ok())
                .map(|shows| shows.0),
        );
    }
    stack
}

/// Whether `source` can be layered onto a frame currently stacking `stack`.
///
/// Any source can be drawn over any other. Whether that means anything is the
/// judgement of whoever is looking, and a viewer that refuses to overlay two
/// datasets is a viewer somebody exports screenshots from to overlay by hand.
/// What is refused is only what cannot be drawn: a source already in the stack,
/// and more layers than a cell has camera orders for.
pub fn can_add_layer(stack: &[Entity], source: Entity) -> bool {
    !stack.is_empty() && stack.len() < MAX_LAYERS && !stack.contains(&source)
}

/// A warning for a layer measured differently from the frame it is drawn
/// over, such as `um over px`. Drawn anyway — nothing rescales it — so the
/// menu says so rather than letting the two look aligned by coincidence.
pub fn unit_mismatch(base: &DataSource, layer: &DataSource) -> Option<String> {
    (!base.shares_space_with(layer)).then(|| {
        let unit = |source: &DataSource| {
            if source.unit.is_empty() {
                "no unit".to_string()
            } else {
                source.unit.clone()
            }
        };
        format!("{} over {}", unit(layer), unit(base))
    })
}

/// Keep every layer looking through its frame's camera, drawing into an
/// image the size of that frame, and laid over it.
///
/// Copied rather than parented, because neither the projection nor the size
/// of the frame is inherited.
pub fn sync_layers(
    mut commands: Commands,
    windows: Query<&Window>,
    mut images: ResMut<Assets<Image>>,
    frames: Query<(&Panel, &Camera, &Transform, &Projection, &FrameLayers)>,
    mut layers: Query<
        (
            &mut Camera,
            &mut Transform,
            &mut Projection,
            &LayerOpacity,
            Option<&LayerImage>,
            Option<&LayerComposite>,
        ),
        (With<LayerOf>, Without<Panel>),
    >,
    mut composites: Query<(&mut Node, &mut ImageNode, &mut GlobalZIndex), Without<Camera>>,
) {
    let scale_factor = windows.single().map_or(1.0, Window::scale_factor);

    for (panel, frame_camera, frame_transform, frame_projection, stack) in &frames {
        let viewport = frame_camera
            .viewport
            .as_ref()
            .filter(|viewport| viewport.physical_size.min_element() > 0);

        for (depth, entity) in stack.cameras().iter().enumerate() {
            let Ok((mut camera, mut transform, mut projection, opacity, image, composite)) =
                layers.get_mut(*entity)
            else {
                continue;
            };
            let depth = depth + 1;
            let order = camera_order(panel.index, depth);
            if camera.order != order {
                camera.order = order;
            }

            let Some(viewport) = viewport else {
                if camera.is_active {
                    camera.is_active = false;
                }
                continue;
            };
            let size = viewport.physical_size;

            // The image, made on first sight of a frame to size it to and
            // resized with the frame after that.
            let handle = match image {
                Some(image) => {
                    if images
                        .get(&image.handle)
                        .is_some_and(|existing| existing.size() != size)
                        && let Some(mut existing) = images.get_mut(&image.handle)
                    {
                        existing.resize(Extent3d {
                            width: size.x,
                            height: size.y,
                            depth_or_array_layers: 1,
                        });
                    }
                    if image.scale_factor != scale_factor {
                        commands.entity(*entity).insert((
                            target(&image.handle, scale_factor),
                            LayerImage {
                                handle: image.handle.clone(),
                                scale_factor,
                            },
                        ));
                    }
                    image.handle.clone()
                }
                None => {
                    let handle = images.add(layer_image(size));
                    commands.entity(*entity).insert((
                        target(&handle, scale_factor),
                        LayerImage {
                            handle: handle.clone(),
                            scale_factor,
                        },
                    ));
                    handle
                }
            };

            // Nothing to draw at no opacity, so nothing is drawn.
            let active = frame_camera.is_active && image.is_some() && opacity.0 > 0.0;
            if camera.is_active != active {
                camera.is_active = active;
            }
            transform.set_if_neq(*frame_transform);
            // Read before writing: taking the projection mutably marks it
            // changed, and the camera is recomputed for every change.
            if let Projection::Orthographic(theirs) = frame_projection
                && matches!(&*projection, Projection::Orthographic(mine) if mine.scale != theirs.scale)
                && let Projection::Orthographic(mine) = projection.as_mut()
            {
                mine.scale = theirs.scale;
            }

            // Laid over the frame in logical pixels, which is what UI layout
            // measures in; the viewport is physical.
            let at = viewport.physical_position.as_vec2() / scale_factor;
            let extent = size.as_vec2() / scale_factor;
            let tint = Color::srgba(1.0, 1.0, 1.0, opacity.0.clamp(0.0, 1.0));
            let z = GlobalZIndex(COMPOSITE_Z + depth as i32);
            let existing = composite
                .and_then(|composite| composite.iter().next())
                .and_then(|node| composites.get_mut(node).ok());
            match existing {
                Some((mut node, mut image_node, mut global_z)) => {
                    let wanted = composite_node(at, extent, active);
                    if *node != wanted {
                        *node = wanted;
                    }
                    if image_node.image != handle {
                        image_node.image = handle;
                    }
                    if image_node.color != tint {
                        image_node.color = tint;
                    }
                    global_z.set_if_neq(z);
                }
                None => {
                    commands.spawn((
                        composite_node(at, extent, active),
                        ImageNode {
                            image: handle,
                            color: tint,
                            ..default()
                        },
                        z,
                        // Only a picture of the frame, so the pointer passes
                        // through it to the frame it lies over.
                        Pickable::IGNORE,
                        CompositeOf(*entity),
                    ));
                }
            }
        }
    }
}

fn target(handle: &Handle<Image>, scale_factor: f32) -> RenderTarget {
    RenderTarget::Image(ImageRenderTarget {
        handle: handle.clone(),
        // The window's, so the layer's logical size is the frame's and its
        // projection can be copied from the frame's unchanged.
        scale_factor,
    })
}

/// An empty image to render a layer into.
fn layer_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    // Only ever drawn into on the GPU, so keeping a CPU copy of every pixel
    // would be megabytes held for nothing.
    image.data = None;
    image
}

fn composite_node(at: Vec2, extent: Vec2, shown: bool) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(at.x),
        top: Val::Px(at.y),
        width: Val::Px(extent.x),
        height: Val::Px(extent.y),
        display: if shown { Display::Flex } else { Display::None },
        ..default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(unit: &str) -> DataSource {
        DataSource {
            name: "test".into(),
            unit: unit.into(),
            detail: String::new(),
            stat: String::new(),
            layer: 1,
        }
    }

    fn entities(count: u32) -> Vec<Entity> {
        (1..=count)
            .map(|n| Entity::from_raw_u32(n).unwrap())
            .collect()
    }

    #[test]
    fn anything_can_be_layered_whatever_it_is_measured_in() {
        let [base, other] = entities(2)[..] else {
            unreachable!()
        };
        assert!(can_add_layer(&[base], other));
    }

    #[test]
    fn a_source_is_never_stacked_twice_in_one_frame() {
        let [base, layer] = entities(2)[..] else {
            unreachable!()
        };
        assert!(!can_add_layer(&[base], base));
        assert!(!can_add_layer(&[base, layer], layer));
    }

    #[test]
    fn a_stack_stops_where_the_camera_orders_run_out() {
        let stack = entities(MAX_LAYERS as u32);
        let extra = Entity::from_raw_u32(1000).unwrap();
        assert!(!can_add_layer(&stack, extra));
        assert!(can_add_layer(&stack[1..], extra));
    }

    #[test]
    fn there_is_nothing_to_layer_onto_without_a_frame() {
        assert!(!can_add_layer(&[], Entity::from_raw_u32(1).unwrap()));
    }

    #[test]
    fn a_layer_measured_differently_says_so() {
        assert_eq!(unit_mismatch(&source("px"), &source("px")), None);
        assert_eq!(
            unit_mismatch(&source("px"), &source("um")).as_deref(),
            Some("um over px")
        );
        assert_eq!(
            unit_mismatch(&source("px"), &source("")).as_deref(),
            Some("no unit over px")
        );
    }
}
