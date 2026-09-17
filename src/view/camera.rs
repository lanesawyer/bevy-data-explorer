//! The cameras: one per frame, each with its own viewport and its own place
//! in the grid, plus the single camera the UI is drawn through.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Viewport};
use bevy::prelude::*;
use bevy::ui::IsDefaultUiCamera;

use super::chrome::{Axis, BUTTON_GAP, BUTTON_PX, DIVIDER_PX, PanelButton, PanelDivider};
use super::grid::{UI_CAMERA_ORDER, assign_cells, camera_order, clear_color_for, grid_for};
use super::{FrameArea, Panel};
use crate::app::theme::Palette;

/// Marks the camera that the UI is laid out against.
#[derive(Component, Clone, Default)]
pub struct UiCamera;

/// Spawn a camera that exists purely to host the UI.
///
/// Bevy sizes the UI layout root from its target camera's *viewport*, not from
/// the window. With only panel cameras present it picks one of them, so every
/// UI position ends up measured against a single panel — a divider at the
/// halfway mark lands in the middle of that panel instead of between the
/// panels. A camera with no viewport keeps the UI measured against the window.
pub(super) fn spawn_ui_camera(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        Camera2d
        Camera {
            // After every panel the grid can hold, so it never clears their
            // output no matter how many are added later.
            order: { UI_CAMERA_ORDER },
            clear_color: { ClearColorConfig::None },
        }
        // Draws no world geometry, only UI. RenderLayers keeps its field
        // private, so it is supplied whole rather than patched field by field.
        template_value(RenderLayers::none())
        IsDefaultUiCamera
        UiCamera
    });
}

/// Keep each panel's viewport, and the UI drawn over it, matched to the grid.
pub fn update_viewports(
    windows: Query<&Window>,
    area: Res<FrameArea>,
    mut panels: Query<(&Panel, &mut Camera)>,
    mut dividers: Query<(&PanelDivider, &mut Node), Without<PanelButton>>,
    mut buttons: Query<(&PanelButton, &mut Node), Without<PanelDivider>>,
    indices: Query<&Panel>,
) {
    let Ok(window) = windows.single() else { return };
    if window.physical_size().x == 0 || window.physical_size().y == 0 {
        return;
    }

    let count = panels.iter().count();
    let (columns, rows) = grid_for(count);

    // Viewports are physical; the area is tracked in logical pixels.
    let scale = window.scale_factor();
    let origin = (area.origin * scale).as_uvec2();
    let size = (area.size * scale).as_uvec2().max(UVec2::ONE);

    for (panel, mut camera) in &mut panels {
        let (col, row) = (panel.index % columns, panel.index / columns);
        let width = size.x / columns as u32;
        let height = size.y / rows as u32;
        // Give the last column and row any remainder so no cell goes unpainted.
        let this_width = if col + 1 == columns {
            size.x - width * (columns as u32 - 1)
        } else {
            width
        };
        let this_height = if row + 1 == rows {
            size.y - height * (rows as u32 - 1)
        } else {
            height
        };
        camera.viewport = Some(Viewport {
            physical_position: origin + UVec2::new(width * col as u32, height * row as u32),
            physical_size: UVec2::new(this_width.max(1), this_height.max(1)),
            ..default()
        });
    }

    let cell = Vec2::new(area.size.x / columns as f32, area.size.y / rows as f32);
    let base = area.origin;

    for (divider, mut node) in &mut dividers {
        let used = match divider.axis {
            Axis::Vertical => divider.ordinal + 1 < columns,
            Axis::Horizontal => divider.ordinal + 1 < rows,
        };
        node.display = if used { Display::Flex } else { Display::None };
        if !used {
            continue;
        }
        match divider.axis {
            Axis::Vertical => {
                node.left = Val::Px(base.x + cell.x * (divider.ordinal + 1) as f32);
                node.top = Val::Px(base.y);
                node.width = Val::Px(DIVIDER_PX);
                node.height = Val::Px(area.size.y);
            }
            Axis::Horizontal => {
                node.left = Val::Px(base.x);
                node.top = Val::Px(base.y + cell.y * (divider.ordinal + 1) as f32);
                node.width = Val::Px(area.size.x);
                node.height = Val::Px(DIVIDER_PX);
            }
        }
    }

    for (button, mut node) in &mut buttons {
        let Ok(panel) = indices.get(button.panel) else {
            continue;
        };
        let cell = area.cell(count, panel.index);
        let from_right = (BUTTON_PX + BUTTON_GAP) * button.action.slot() + BUTTON_PX + 8.0;
        node.left = Val::Px(cell.max.x - from_right);
        node.top = Val::Px(cell.min.y + 8.0);
    }
}

/// Clear the window from the UI camera while there is no frame to do it.
///
/// Cell 0 clears whenever a frame exists, and nothing else may — but with no
/// frames at all nothing clears, and the window keeps whatever was last drawn
/// into it behind the empty-window panel. The UI camera draws after every
/// frame the grid can hold, so it takes the job only while the grid is empty.
pub fn clear_when_empty(
    palette: Res<Palette>,
    panels: Query<&Panel>,
    mut cameras: Query<&mut Camera, With<UiCamera>>,
) {
    let empty = panels.iter().next().is_none();
    for mut camera in &mut cameras {
        let clearing = !matches!(camera.clear_color, ClearColorConfig::None);
        // Repainted when the theme changes as well as when the grid empties:
        // the colour it clears to is not the same in both themes.
        if clearing == empty && !palette.is_changed() {
            continue;
        }
        camera.clear_color = if empty {
            clear_color_for(0, palette.frame_bg)
        } else {
            ClearColorConfig::None
        };
    }
}

/// Repaint what the frames clear to when the theme changes.
///
/// Only cell 0 clears, and which cell that is can change, so this writes every
/// camera's colour rather than tracking the one that happens to hold the job.
pub fn follow_theme(palette: Res<Palette>, mut panels: Query<(&Panel, &mut Camera)>) {
    if !palette.is_changed() {
        return;
    }
    for (panel, mut camera) in &mut panels {
        camera.clear_color = clear_color_for(panel.index, palette.frame_bg);
    }
}

/// Keep cells contiguous, and keep draw order and clearing in step with them.
///
/// Every path that adds or removes a frame funnels through here rather than
/// fixing up indices itself. Opening and closing in the same breath can hand
/// out a duplicate index, and a frame spawned this tick is not yet visible to
/// the code that renumbers the survivors, so ownership of the invariant sits in
/// one place that runs after the dust settles.
pub fn normalize_panels(
    background: Res<Palette>,
    mut panels: Query<(Entity, &mut Panel, &mut Camera)>,
) {
    let cells = assign_cells(
        panels
            .iter()
            .map(|(entity, panel, _)| (panel.index, entity))
            .collect(),
    );

    for (position, entity) in cells {
        let Ok((_, mut panel, mut camera)) = panels.get_mut(entity) else {
            continue;
        };
        let wanted_order = camera_order(position, 0);
        if panel.index == position && camera.order == wanted_order {
            continue;
        }
        panel.index = position;
        camera.order = wanted_order;
        // Only the first camera clears. Losing the frame that held that job
        // leaves nothing clearing the window, and every frame then paints over
        // the last one instead of replacing it.
        camera.clear_color = clear_color_for(position, background.frame_bg);
    }
}
