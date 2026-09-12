//! Panels arranged in a grid.
//!
//! Each panel is a 2D camera with its own viewport covering one cell of the
//! window and its own pan/zoom state. Input is routed to whichever panel the
//! cursor is over, so views pan and zoom independently.
//!
//! A panel does not know what kind of data it is showing. It points at a source
//! entity and draws that source's render layer, which is what allows a frame to
//! be repointed at a different dataset later without this module changing.
//!
//! Panels can be duplicated at runtime. A duplicate is another camera on the
//! same layer, which is why duplicating costs no extra geometry: the two
//! cameras draw the same entities from different viewpoints.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Viewport};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::Interaction;

use crate::datasource::DataSource;

/// Width of the rule drawn between panels, in logical pixels.
const DIVIDER_PX: f32 = 2.0;

/// The grid never exceeds four columns by two rows.
pub const MAX_COLUMNS: usize = 4;
pub const MAX_ROWS: usize = 2;
pub const MAX_PANELS: usize = MAX_COLUMNS * MAX_ROWS;

/// Columns and rows for `count` panels.
///
/// Panels stay in one row until there are more than two, after which the grid
/// goes two deep and grows sideways. That keeps cells closer to square than a
/// single row of eight would, while never exceeding the 4x2 maximum.
pub fn grid_for(count: usize) -> (usize, usize) {
    let count = count.clamp(1, MAX_PANELS);
    let rows = if count <= 2 { 1 } else { MAX_ROWS };
    (count.div_ceil(rows), rows)
}

/// Marks a panel camera and records its cell in the grid.
#[derive(Component)]
pub struct Panel {
    /// Cell index, left to right then top to bottom.
    pub index: usize,
}

/// The source a panel is currently displaying.
///
/// Held as an entity rather than a format tag so that repointing a frame at
/// another dataset is a component write plus a layer change.
#[derive(Component, Clone, Copy)]
pub struct ShowsSource(pub Entity);

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

/// The view a panel is currently showing, used when duplicating it.
#[derive(Clone, Copy)]
pub struct View {
    pub centre: Vec2,
    pub scale: f32,
}

/// Spawn a panel camera in cell `index`, showing `source`.
pub fn spawn_panel(
    commands: &mut Commands,
    source: Entity,
    layer: usize,
    index: usize,
    limits: ViewLimits,
    view: Option<View>,
) -> Entity {
    let view = view.unwrap_or(View {
        centre: limits.centre,
        scale: limits.fit_scale,
    });
    commands
        .spawn((
            Camera2d,
            Camera {
                // The first camera clears the window; the rest draw over it,
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
                scale: view.scale,
                ..OrthographicProjection::default_2d()
            }),
            Transform::from_translation(view.centre.extend(1000.0)),
            RenderLayers::layer(layer),
            Panel { index },
            ShowsSource(source),
            limits,
        ))
        .id()
}

/// Keep each panel's viewport, and the UI drawn over it, matched to the grid.
pub fn update_viewports(
    windows: Query<&Window>,
    mut panels: Query<(&Panel, &mut Camera)>,
    mut dividers: Query<(&PanelDivider, &mut Node), Without<PanelButton>>,
    mut buttons: Query<(&PanelButton, &mut Node), Without<PanelDivider>>,
    indices: Query<&Panel>,
) {
    let Ok(window) = windows.single() else { return };
    let size = window.physical_size();
    if size.x == 0 || size.y == 0 {
        return;
    }

    let count = panels.iter().count();
    let (columns, rows) = grid_for(count);

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
            physical_position: UVec2::new(width * col as u32, height * row as u32),
            physical_size: UVec2::new(this_width.max(1), this_height.max(1)),
            ..default()
        });
    }

    // UI is laid out in logical pixels against the whole window.
    let logical = Vec2::new(window.width(), window.height());
    let cell = Vec2::new(logical.x / columns as f32, logical.y / rows as f32);

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
                node.left = Val::Px(cell.x * (divider.ordinal + 1) as f32);
                node.top = Val::Px(0.0);
                node.width = Val::Px(DIVIDER_PX);
                node.height = Val::Percent(100.0);
            }
            Axis::Horizontal => {
                node.left = Val::Px(0.0);
                node.top = Val::Px(cell.y * (divider.ordinal + 1) as f32);
                node.width = Val::Percent(100.0);
                node.height = Val::Px(DIVIDER_PX);
            }
        }
    }

    for (button, mut node) in &mut buttons {
        let Ok(panel) = indices.get(button.panel) else {
            continue;
        };
        let (col, row) = (panel.index % columns, panel.index / columns);
        node.left = Val::Px(cell.x * (col + 1) as f32 - BUTTON_PX - 8.0);
        node.top = Val::Px(cell.y * row as f32 + 8.0);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Vertical,
    Horizontal,
}

#[derive(Component)]
pub struct PanelDivider {
    axis: Axis,
    ordinal: usize,
}

/// Marks the camera that the UI is laid out against.
#[derive(Component)]
pub struct UiCamera;

/// Spawn a camera that exists purely to host the UI.
///
/// Bevy sizes the UI layout root from its target camera's *viewport*, not from
/// the window. With only panel cameras present it picks one of them, so every
/// UI position ends up measured against a single panel — a divider at the
/// halfway mark lands in the middle of that panel instead of between the
/// panels. A camera with no viewport keeps the UI measured against the window.
pub fn spawn_ui_camera(commands: &mut Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            // After every panel the grid can hold, so it never clears their
            // output no matter how many are added later.
            order: MAX_PANELS as isize,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        // Draws no world geometry, only UI.
        RenderLayers::none(),
        bevy::ui::IsDefaultUiCamera,
        UiCamera,
    ));
}

/// Spawn the full set of rules the grid can ever need, and let
/// [`update_viewports`] show only the ones the current layout uses.
pub fn spawn_dividers(commands: &mut Commands) {
    let mut rule = |axis: Axis, ordinal: usize| {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgb(0.25, 0.27, 0.32)),
            PanelDivider { axis, ordinal },
        ));
    };
    for ordinal in 0..MAX_COLUMNS - 1 {
        rule(Axis::Vertical, ordinal);
    }
    for ordinal in 0..MAX_ROWS - 1 {
        rule(Axis::Horizontal, ordinal);
    }
}

const BUTTON_PX: f32 = 22.0;

/// A button that duplicates the panel it belongs to.
#[derive(Component)]
pub struct PanelButton {
    pub panel: Entity,
}

/// Give every panel a duplicate button, including panels added at runtime.
pub fn sync_panel_buttons(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    buttons: Query<&PanelButton>,
) {
    for panel in &panels {
        if buttons.iter().any(|b| b.panel == panel) {
            continue;
        }
        commands
            .spawn((
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(BUTTON_PX),
                    height: Val::Px(BUTTON_PX),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.18, 0.20, 0.26, 0.85)),
                PanelButton { panel },
            ))
            .with_child((
                Text::new("+"),
                TextFont {
                    font_size: bevy::text::FontSize::Px(15.0),
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.9, 0.95)),
            ));
    }
}

/// Duplicate a panel when its button is pressed.
///
/// The copy inherits the source panel's current centre and zoom rather than its
/// fitted defaults, so a duplicate starts as the same view and can then be
/// driven somewhere else.
pub fn duplicate_panel(
    mut commands: Commands,
    pressed: Query<(&Interaction, &PanelButton), Changed<Interaction>>,
    panels: Query<(&ShowsSource, &Transform, &Projection, &ViewLimits)>,
    sources: Query<&DataSource>,
) {
    let count = panels.iter().count();
    for (interaction, button) in &pressed {
        if *interaction != Interaction::Pressed || count >= MAX_PANELS {
            continue;
        }
        let Ok((shows, transform, projection, limits)) = panels.get(button.panel) else {
            continue;
        };
        let Ok(source) = sources.get(shows.0) else {
            continue;
        };
        let Projection::Orthographic(ortho) = projection else {
            continue;
        };
        spawn_panel(
            &mut commands,
            shows.0,
            source.layer,
            count,
            *limits,
            Some(View {
                centre: transform.translation.truncate(),
                scale: ortho.scale,
            }),
        );
    }
}

/// Highlight the duplicate button under the pointer.
pub fn highlight_panel_buttons(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<PanelButton>),
    >,
) {
    for (interaction, mut colour) in &mut buttons {
        colour.0 = match interaction {
            Interaction::Pressed => Color::srgba(0.35, 0.55, 0.85, 0.95),
            Interaction::Hovered => Color::srgba(0.28, 0.31, 0.40, 0.95),
            Interaction::None => Color::srgba(0.18, 0.20, 0.26, 0.85),
        };
    }
}

/// A drag in progress, and the panel it began in.
#[derive(Clone, Copy)]
pub struct Drag {
    panel: usize,
    last: Vec2,
}

/// Which panel input applies to.
///
/// A drag keeps hold of the panel it started in even after the pointer crosses
/// into another, so dragging past a panel edge carries on panning the view the
/// gesture began in rather than grabbing its neighbour mid-stroke.
fn active_panel(drag: Option<Drag>, cursor: Vec2, window: Vec2, count: usize) -> usize {
    match drag {
        Some(drag) => drag.panel.min(count.saturating_sub(1)),
        None => panel_under_cursor(cursor, window, count),
    }
}

/// Which panel the cursor is over.
fn panel_under_cursor(cursor: Vec2, window: Vec2, count: usize) -> usize {
    let (columns, rows) = grid_for(count);
    let col = ((cursor.x / (window.x / columns as f32).max(1.0)) as usize).min(columns - 1);
    let row = ((cursor.y / (window.y / rows as f32).max(1.0)) as usize).min(rows - 1);
    (row * columns + col).min(count.saturating_sub(1))
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
    ui: Query<&Interaction, With<PanelButton>>,
    mut drag: Local<Option<Drag>>,
) {
    let Ok(window) = windows.single() else { return };
    let held = buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Middle);

    let Some(cursor) = window.cursor_position() else {
        // The pointer left the window. Hold the gesture so it resumes if the
        // pointer comes back with the button still down.
        if !held {
            *drag = None;
        }
        wheel.clear();
        return;
    };

    // A click on a panel's button must not also pan that panel. An existing
    // drag is left alone, so passing over a button mid-stroke does not end it.
    if drag.is_none() && ui.iter().any(|i| *i != Interaction::None) {
        wheel.clear();
        return;
    }

    let count = panels.iter().count();
    let window_size = Vec2::new(window.width(), window.height());

    if !held {
        *drag = None;
    } else if drag.is_none()
        && (buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Middle))
    {
        *drag = Some(Drag {
            panel: panel_under_cursor(cursor, window_size, count),
            last: cursor,
        });
    }

    let active = active_panel(*drag, cursor, window_size, count);

    let mut scroll = 0.0;
    for event in wheel.read() {
        scroll += match event.unit {
            MouseScrollUnit::Line => event.y,
            // Trackpads report pixels; scale them into comparable steps.
            MouseScrollUnit::Pixel => event.y / 50.0,
        };
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

        if let Some(state) = *drag {
            let delta = cursor - state.last;
            transform.translation.x -= delta.x * ortho.scale;
            transform.translation.y += delta.y * ortho.scale;
        }
    }

    if let Some(state) = drag.as_mut() {
        state.last = cursor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_stays_one_row_until_it_has_to_split() {
        assert_eq!(grid_for(1), (1, 1));
        assert_eq!(grid_for(2), (2, 1));
    }

    #[test]
    fn the_grid_grows_sideways_two_deep() {
        assert_eq!(grid_for(3), (2, 2));
        assert_eq!(grid_for(4), (2, 2));
        assert_eq!(grid_for(5), (3, 2));
        assert_eq!(grid_for(6), (3, 2));
        assert_eq!(grid_for(7), (4, 2));
        assert_eq!(grid_for(8), (4, 2));
    }

    #[test]
    fn the_grid_never_exceeds_four_by_two() {
        for count in 1..=MAX_PANELS {
            let (columns, rows) = grid_for(count);
            assert!(columns <= MAX_COLUMNS && rows <= MAX_ROWS);
            assert!(columns * rows >= count, "{count} panels would not fit");
        }
    }

    #[test]
    fn every_cell_maps_back_to_its_panel() {
        // A cursor in the middle of each cell must select that cell's panel.
        let window = Vec2::new(800.0, 600.0);
        for count in 1..=MAX_PANELS {
            let (columns, rows) = grid_for(count);
            let cell = Vec2::new(window.x / columns as f32, window.y / rows as f32);
            for index in 0..count {
                let (col, row) = (index % columns, index / columns);
                let centre = Vec2::new(cell.x * (col as f32 + 0.5), cell.y * (row as f32 + 0.5));
                assert_eq!(
                    panel_under_cursor(centre, window, count),
                    index,
                    "{count} panels, cell {index}"
                );
            }
        }
    }

    #[test]
    fn the_cursor_selects_the_panel_it_is_over() {
        let window = Vec2::new(1000.0, 600.0);
        assert_eq!(panel_under_cursor(Vec2::new(10.0, 5.0), window, 2), 0);
        assert_eq!(panel_under_cursor(Vec2::new(499.0, 5.0), window, 2), 0);
        assert_eq!(panel_under_cursor(Vec2::new(501.0, 5.0), window, 2), 1);
        // The far corner must not select a panel that does not exist.
        assert_eq!(panel_under_cursor(Vec2::new(1000.0, 600.0), window, 2), 1);
    }

    #[test]
    fn a_partly_filled_grid_never_selects_an_empty_cell() {
        // Three panels leave the bottom-right cell empty; a cursor there has to
        // fall back to a panel that exists.
        let window = Vec2::new(800.0, 600.0);
        assert_eq!(panel_under_cursor(Vec2::new(700.0, 500.0), window, 3), 2);
    }

    #[test]
    fn a_drag_keeps_the_panel_it_started_in() {
        let window = Vec2::new(800.0, 600.0);
        // Started in panel 0, pointer has since crossed into panel 1.
        let drag = Some(Drag {
            panel: 0,
            last: Vec2::new(100.0, 100.0),
        });
        let crossed = Vec2::new(700.0, 100.0);
        assert_eq!(panel_under_cursor(crossed, window, 2), 1);
        assert_eq!(active_panel(drag, crossed, window, 2), 0);
    }

    #[test]
    fn without_a_drag_input_follows_the_pointer() {
        let window = Vec2::new(800.0, 600.0);
        let cursor = Vec2::new(700.0, 100.0);
        assert_eq!(active_panel(None, cursor, window, 2), 1);
    }

    #[test]
    fn a_drag_survives_the_pointer_leaving_the_grid() {
        // Dragging well past the window edge still pans the original panel.
        let window = Vec2::new(800.0, 600.0);
        let drag = Some(Drag {
            panel: 1,
            last: Vec2::new(500.0, 100.0),
        });
        for cursor in [
            Vec2::new(-200.0, -50.0),
            Vec2::new(5000.0, 5000.0),
            Vec2::new(10.0, 590.0),
        ] {
            assert_eq!(active_panel(drag, cursor, window, 4), 1);
        }
    }

    #[test]
    fn a_single_panel_takes_every_cursor_position() {
        let window = Vec2::new(1000.0, 600.0);
        assert_eq!(panel_under_cursor(Vec2::new(999.0, 599.0), window, 1), 0);
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
}
