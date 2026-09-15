//! Several sources drawn in one frame, one over another.
//!
//! A frame's own camera draws the source it was opened onto. Every further
//! layer is a camera of its own, sharing the frame's viewport, view and
//! projection, drawing its source's render layer and ordered straight after
//! the one beneath it. Stacking by camera order rather than by depth is what
//! lets any source sit on any other: nothing a format spawns has to know which
//! layer it ended up in, and one source can be the base of one frame and a
//! layer of another at the same time.
//!
//! A layer camera also carries [`ShowsSource`], which is the whole of what a
//! streamer asks of a view. So a source streams for a frame it is layered into
//! exactly as it does for a frame of its own, without its plugin changing.
//!
//! Nothing is rescaled: a layer is drawn in its own coordinates, so two
//! sources line up when they were measured alike. Any source may still be
//! layered over any other; one measured differently is flagged, not refused.

use bevy::camera::ClearColorConfig;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::grid::camera_order;
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

/// Marks a source named on the command line as a layer rather than a frame.
///
/// It is stacked onto the first frame when the frames open, provided the two
/// share a space, and gets a frame of its own otherwise.
#[derive(Component, Clone, Copy, Default)]
pub struct OpensAsLayer;

/// Spawn a camera drawing `source` over `panel`.
///
/// Its view, viewport and order are left to [`sync_layers`], which writes
/// them every frame from the frame it belongs to.
pub fn spawn_layer(commands: &mut Commands, panel: Entity, source: Entity, layer: usize) -> Entity {
    commands
        .spawn((
            Camera2d,
            Camera {
                // Only cell 0 clears. A layer that did would wipe the frame it
                // was meant to be drawn over, and every frame before it.
                clear_color: ClearColorConfig::None,
                // Inactive until it has a viewport, or it would draw once over
                // the whole window.
                is_active: false,
                ..default()
            },
            RenderLayers::layer(layer),
            ShowsSource(source),
            LayerOf(panel),
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
    !stack.is_empty() && stack.len() < super::grid::MAX_LAYERS && !stack.contains(&source)
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

/// Keep every layer looking through its frame's camera.
///
/// Copied rather than parented, because the projection and the viewport are
/// not inherited, and a layer that took its transform from a parent would
/// still need the other two written here.
pub fn sync_layers(
    frames: Query<(&Panel, &Camera, &Transform, &Projection, &FrameLayers)>,
    mut layers: Query<
        (&mut Camera, &mut Transform, &mut Projection),
        (With<LayerOf>, Without<Panel>),
    >,
) {
    for (panel, frame_camera, frame_transform, frame_projection, stack) in &frames {
        for (depth, entity) in stack.cameras().iter().enumerate() {
            let Ok((mut camera, mut transform, mut projection)) = layers.get_mut(*entity) else {
                continue;
            };
            let order = camera_order(panel.index, depth + 1);
            if camera.order != order {
                camera.order = order;
            }
            if !same_viewport(camera.viewport.as_ref(), frame_camera.viewport.as_ref()) {
                camera.viewport.clone_from(&frame_camera.viewport);
            }
            let active = frame_camera.is_active && frame_camera.viewport.is_some();
            if camera.is_active != active {
                camera.is_active = active;
            }
            transform.set_if_neq(*frame_transform);
            // Read before writing: taking the projection mutably marks it
            // changed, and the camera is recomputed for every change.
            let wanted = match frame_projection {
                Projection::Orthographic(theirs) => theirs.scale,
                _ => continue,
            };
            if matches!(&*projection, Projection::Orthographic(mine) if mine.scale != wanted)
                && let Projection::Orthographic(mine) = projection.as_mut()
            {
                mine.scale = wanted;
            }
        }
    }
}

fn same_viewport(a: Option<&bevy::camera::Viewport>, b: Option<&bevy::camera::Viewport>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.physical_position == b.physical_position
                && a.physical_size == b.physical_size
                && a.depth == b.depth
        }
        _ => false,
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
        let stack = entities(crate::view::grid::MAX_LAYERS as u32);
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
