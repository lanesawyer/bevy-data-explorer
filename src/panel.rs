//! Side-by-side panels.
//!
//! Each panel is a 2D camera with its own viewport covering a slice of the
//! window, its own pan/zoom state, and its own render layer so that one
//! panel's contents cannot leak into another. Input is routed to whichever
//! panel the cursor is over, so the two views pan and zoom independently.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Viewport};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

/// Render layer for the OME-Zarr image tiles.
pub const IMAGE_LAYER: usize = 1;
/// Render layer for the Scatterbrain points.
pub const POINTS_LAYER: usize = 2;

/// Width of the rule drawn between panels, in logical pixels.
const DIVIDER_PX: f32 = 2.0;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelKind {
    Image,
    Points,
}

impl PanelKind {
    pub fn layer(self) -> usize {
        match self {
            PanelKind::Image => IMAGE_LAYER,
            PanelKind::Points => POINTS_LAYER,
        }
    }
}

/// Marks a panel camera and records where it sits in the window.
#[derive(Component)]
pub struct Panel {
    pub kind: PanelKind,
    /// Column index, left to right.
    pub index: usize,
    pub columns: usize,
}

/// Pan and zoom bounds for a panel, derived from the extent of its data.
#[derive(Component, Clone, Copy)]
pub struct ViewLimits {
    pub min_scale: f32,
    pub max_scale: f32,
    pub fit_scale: f32,
    pub centre: Vec2,
}

impl ViewLimits {
    /// Frame `width` x `height` of world in a viewport, allowing zoom in to
    /// `finest_detail` world units per pixel and out to a few screens' worth.
    pub fn fit(centre: Vec2, width: f32, height: f32, viewport: Vec2, finest_detail: f32) -> Self {
        let fit_scale = (width / viewport.x.max(1.0))
            .max(height / viewport.y.max(1.0))
            .max(f32::MIN_POSITIVE);
        ViewLimits {
            min_scale: finest_detail.min(fit_scale),
            max_scale: fit_scale * 4.0,
            fit_scale,
            centre,
        }
    }
}

/// Spawn a panel camera. `index` counts from the left.
pub fn spawn_panel(
    commands: &mut Commands,
    kind: PanelKind,
    index: usize,
    columns: usize,
    limits: ViewLimits,
) -> Entity {
    commands
        .spawn((
            Camera2d,
            Camera {
                // The leftmost camera clears the window; the rest draw over it,
                // since a later clear would wipe the panels already drawn.
                clear_color: if index == 0 {
                    ClearColorConfig::Custom(Color::srgb(0.04, 0.04, 0.06))
                } else {
                    ClearColorConfig::None
                },
                order: index as isize,
                ..default()
            },
            Projection::Orthographic(OrthographicProjection {
                scale: limits.fit_scale,
                ..OrthographicProjection::default_2d()
            }),
            Transform::from_translation(limits.centre.extend(1000.0)),
            RenderLayers::layer(kind.layer()),
            Panel {
                kind,
                index,
                columns,
            },
            limits,
        ))
        .id()
}

/// Keep each panel's viewport matched to the window as it resizes.
pub fn update_viewports(
    windows: Query<&Window>,
    mut panels: Query<(&Panel, &mut Camera)>,
    mut dividers: Query<&mut Node, With<PanelDivider>>,
) {
    let Ok(window) = windows.single() else { return };
    let size = window.physical_size();
    if size.x == 0 || size.y == 0 {
        return;
    }

    for (panel, mut camera) in &mut panels {
        let columns = panel.columns.max(1) as u32;
        let width = size.x / columns;
        // Give the rightmost panel any remainder so no column goes unpainted.
        let this_width = if panel.index + 1 == columns as usize {
            size.x - width * (columns - 1)
        } else {
            width
        };
        camera.viewport = Some(Viewport {
            physical_position: UVec2::new(width * panel.index as u32, 0),
            physical_size: UVec2::new(this_width.max(1), size.y),
            ..default()
        });
    }

    let columns = panels.iter().next().map(|(p, _)| p.columns).unwrap_or(1);
    for (i, mut node) in dividers.iter_mut().enumerate() {
        node.left = Val::Percent(100.0 * (i + 1) as f32 / columns.max(1) as f32);
    }
}

#[derive(Component)]
pub struct PanelDivider;

/// Marks the camera that the UI is laid out against.
#[derive(Component)]
pub struct UiCamera;

/// Spawn a camera that exists purely to host the UI.
///
/// Bevy sizes the UI layout root from its target camera's *viewport*, not from
/// the window. With only panel cameras present it picks one of them, so every
/// UI position ends up measured against a single panel — a divider at 50%
/// lands in the middle of that panel instead of between the panels. A camera
/// with no viewport keeps the UI measured against the whole window.
pub fn spawn_ui_camera(commands: &mut Commands, columns: usize) {
    commands.spawn((
        Camera2d,
        Camera {
            // After every panel, so it never clears their output.
            order: columns as isize,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        // Draws no world geometry, only UI.
        RenderLayers::none(),
        bevy::ui::IsDefaultUiCamera,
        UiCamera,
    ));
}

pub fn spawn_dividers(commands: &mut Commands, columns: usize) {
    for _ in 1..columns {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                width: Val::Px(DIVIDER_PX),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.25, 0.27, 0.32)),
            PanelDivider,
        ));
    }
}

/// Which panel the cursor is over, by column index.
fn panel_under_cursor(cursor: Vec2, window_width: f32, columns: usize) -> usize {
    let column_width = window_width / columns.max(1) as f32;
    ((cursor.x / column_width.max(1.0)) as usize).min(columns.saturating_sub(1))
}

/// Scroll to zoom about the cursor and drag to pan, in whichever panel the
/// pointer is over.
pub fn panel_controls(
    mut wheel: MessageReader<MouseWheel>,
    mut panels: Query<(
        &Camera,
        &GlobalTransform,
        &mut Transform,
        &mut Projection,
        &Panel,
        &ViewLimits,
    )>,
    windows: Query<&Window>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut drag: Local<Option<Vec2>>,
) {
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        *drag = None;
        wheel.clear();
        return;
    };

    let columns = panels.iter().next().map(|p| p.4.columns).unwrap_or(1);
    let active = panel_under_cursor(cursor, window.width(), columns);

    let mut scroll = 0.0;
    for event in wheel.read() {
        scroll += match event.unit {
            MouseScrollUnit::Line => event.y,
            // Trackpads report pixels; scale them into comparable steps.
            MouseScrollUnit::Pixel => event.y / 50.0,
        };
    }

    let dragging = buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Middle);
    if buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Middle) {
        *drag = Some(cursor);
    }
    if !dragging {
        *drag = None;
    }

    for (camera, global, mut transform, mut projection, panel, limits) in &mut panels {
        if panel.index != active {
            continue;
        }
        let Projection::Orthographic(ortho) = projection.as_mut() else {
            continue;
        };

        if keys.just_pressed(KeyCode::KeyR) {
            transform.translation = limits.centre.extend(transform.translation.z);
            ortho.scale = limits.fit_scale;
            continue;
        }

        if scroll != 0.0 {
            let before = camera.viewport_to_world_2d(global, cursor).ok();
            let factor = 1.12_f32.powf(-scroll);
            ortho.scale = (ortho.scale * factor).clamp(limits.min_scale, limits.max_scale);

            // Pin the world point under the cursor. Bevy refreshes the
            // projection's `area` after this system, so the post-zoom mapping
            // is recomputed by hand from the new scale.
            if let (Some(before), Some(viewport)) = (before, camera.logical_viewport_size()) {
                let local = cursor - camera.logical_viewport_rect().map_or(Vec2::ZERO, |r| r.min);
                let ndc = (local - viewport * 0.5) * Vec2::new(1.0, -1.0);
                let after = transform.translation.truncate() + ndc * ortho.scale;
                let correction = before - after;
                transform.translation.x += correction.x;
                transform.translation.y += correction.y;
            }
        }

        if dragging {
            if let Some(previous) = *drag {
                let delta = cursor - previous;
                transform.translation.x -= delta.x * ortho.scale;
                transform.translation.y += delta.y * ortho.scale;
            }
        }
    }

    if dragging {
        *drag = Some(cursor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_selects_the_panel_it_is_over() {
        assert_eq!(panel_under_cursor(Vec2::new(10.0, 5.0), 1000.0, 2), 0);
        assert_eq!(panel_under_cursor(Vec2::new(499.0, 5.0), 1000.0, 2), 0);
        assert_eq!(panel_under_cursor(Vec2::new(501.0, 5.0), 1000.0, 2), 1);
        // The right edge must not select a panel that does not exist.
        assert_eq!(panel_under_cursor(Vec2::new(1000.0, 5.0), 1000.0, 2), 1);
    }

    #[test]
    fn a_single_panel_takes_every_cursor_position() {
        assert_eq!(panel_under_cursor(Vec2::new(999.0, 5.0), 1000.0, 1), 0);
    }

    #[test]
    fn fit_limits_frame_the_data_and_allow_zooming_in() {
        // A 40 x 40 world in an 800 x 400 viewport is limited by height.
        let limits = ViewLimits::fit(Vec2::ZERO, 40.0, 40.0, Vec2::new(800.0, 400.0), 0.001);
        assert!((limits.fit_scale - 0.1).abs() < 1e-6);
        assert!(limits.min_scale < limits.fit_scale);
        assert!(limits.max_scale > limits.fit_scale);
    }

    #[test]
    fn zoom_in_is_never_limited_to_less_than_the_fitted_view() {
        // Data coarser than one unit per pixel must still be fully visible.
        let limits = ViewLimits::fit(Vec2::ZERO, 1.0, 1.0, Vec2::new(800.0, 400.0), 5.0);
        assert!(limits.min_scale <= limits.fit_scale);
    }

    #[test]
    fn panels_render_on_separate_layers() {
        assert_ne!(PanelKind::Image.layer(), PanelKind::Points.layer());
    }
}
