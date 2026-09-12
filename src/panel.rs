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
use bevy::ui::{Interaction, IsDefaultUiCamera};

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

/// Marks interactive chrome that swallows pointer input before a frame sees it.
///
/// Needed because chrome can overlap the grid — the sidebar's drag handle
/// straddles its own edge — so position alone cannot decide who gets the drag.
#[derive(Component, Clone, Default)]
pub struct BlocksFrameInput;

/// The region of the window the frame grid occupies, in logical pixels.
///
/// Everything that places a frame or its chrome measures from here rather than
/// from the window, so surrounding UI can take space off the grid without any
/// of that code knowing it exists.
#[derive(Resource, Clone, Copy)]
pub struct FrameArea {
    pub origin: Vec2,
    pub size: Vec2,
}

impl Default for FrameArea {
    fn default() -> Self {
        FrameArea {
            origin: Vec2::ZERO,
            size: Vec2::new(1280.0, 720.0),
        }
    }
}

impl FrameArea {
    /// Take `amount` off the left edge, for chrome docked there.
    pub fn reserve_left(&mut self, amount: f32) {
        let amount = amount.clamp(0.0, self.size.x);
        self.origin.x += amount;
        self.size.x -= amount;
    }
}

/// Reset the grid to the whole window before anything reserves part of it.
pub fn reset_frame_area(windows: Query<&Window>, mut area: ResMut<FrameArea>) {
    let Ok(window) = windows.single() else { return };
    area.origin = Vec2::ZERO;
    area.size = Vec2::new(window.width(), window.height());
}

/// Marks a panel camera and records its cell in the grid.
#[derive(Component, Clone, Default)]
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

impl Default for ShowsSource {
    fn default() -> Self {
        ShowsSource(Entity::PLACEHOLDER)
    }
}

/// Pan and zoom bounds for a panel, derived from the extent of its data.
#[derive(Component, Clone, Copy, Default)]
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
        .spawn_scene(bsn! {
            Camera2d
            Camera {
                clear_color: { clear_color_for(index) },
                order: { index as isize },
            }
            // Both keep private state, so they are supplied whole rather than
            // patched field by field.
            template_value(Projection::Orthographic(OrthographicProjection {
                scale: view.scale,
                ..OrthographicProjection::default_2d()
            }))
            template_value(RenderLayers::layer(layer))
            Transform { translation: { view.centre.extend(1000.0) } }
            Panel { index: { index } }
            ShowsSource({ source })
            template_value(limits)
        })
        .id()
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
        let (col, row) = (panel.index % columns, panel.index / columns);
        let from_right = (BUTTON_PX + BUTTON_GAP) * button.action.slot() + BUTTON_PX + 8.0;
        node.left = Val::Px(base.x + cell.x * (col + 1) as f32 - from_right);
        node.top = Val::Px(base.y + cell.y * row as f32 + 8.0);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Axis {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Component, Clone, Default)]
pub struct PanelDivider {
    axis: Axis,
    ordinal: usize,
}

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
pub fn spawn_ui_camera(commands: &mut Commands) {
    commands.spawn_scene(bsn! {
        Camera2d
        Camera {
            // After every panel the grid can hold, so it never clears their
            // output no matter how many are added later.
            order: { MAX_PANELS as isize },
            clear_color: { ClearColorConfig::None },
        }
        // Draws no world geometry, only UI. RenderLayers keeps its field
        // private, so it is supplied whole rather than patched field by field.
        template_value(RenderLayers::none())
        IsDefaultUiCamera
        UiCamera
    });
}

/// Spawn the full set of rules the grid can ever need, and let
/// [`update_viewports`] show only the ones the current layout uses.
pub fn spawn_dividers(commands: &mut Commands) {
    let mut rule = |axis: Axis, ordinal: usize| {
        commands.spawn_scene(bsn! {
            Node {
                position_type: { PositionType::Absolute },
                display: { Display::None },
            }
            BackgroundColor({ Color::srgb(0.25, 0.27, 0.32) })
            PanelDivider { axis: { axis }, ordinal: { ordinal } }
        });
    };
    for ordinal in 0..MAX_COLUMNS - 1 {
        rule(Axis::Vertical, ordinal);
    }
    for ordinal in 0..MAX_ROWS - 1 {
        rule(Axis::Horizontal, ordinal);
    }
}

const BUTTON_PX: f32 = 22.0;
const BUTTON_GAP: f32 = 4.0;
const IDLE_BUTTON: Color = Color::srgba(0.18, 0.20, 0.26, 0.85);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelAction {
    Duplicate,
    Close,
}

impl PanelAction {
    /// Buttons are laid out right to left from the panel's top corner.
    fn slot(self) -> f32 {
        match self {
            PanelAction::Close => 0.0,
            PanelAction::Duplicate => 1.0,
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            PanelAction::Duplicate => "+",
            PanelAction::Close => "x",
        }
    }
}

/// A button in a panel's corner.
#[derive(Component, Clone)]
pub struct PanelButton {
    pub panel: Entity,
    pub action: PanelAction,
}

impl Default for PanelButton {
    fn default() -> Self {
        // Scenes are patches over defaults, so this only has to be a value the
        // real one is written over.
        PanelButton {
            panel: Entity::PLACEHOLDER,
            action: PanelAction::Duplicate,
        }
    }
}

/// The chrome every corner button shares.
///
/// Split out as a scene so the two buttons differ only by the patch layered on
/// top of it, rather than by two near-identical spawn calls.
fn button_chrome() -> impl Scene {
    bsn! {
        Button
        Node {
            position_type: { PositionType::Absolute },
            width: { Val::Px(BUTTON_PX) },
            height: { Val::Px(BUTTON_PX) },
            justify_content: { JustifyContent::Center },
            align_items: { AlignItems::Center },
            border_radius: { BorderRadius::all(Val::Px(4.0)) },
        }
        BackgroundColor({ IDLE_BUTTON })
        BlocksFrameInput
    }
}

/// Keep one button per action on every panel, and drop the buttons of panels
/// that have gone away.
pub fn sync_panel_buttons(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    buttons: Query<(Entity, &PanelButton)>,
) {
    for (entity, button) in &buttons {
        if panels.get(button.panel).is_err() {
            commands.entity(entity).despawn();
        }
    }

    for panel in &panels {
        for action in [PanelAction::Duplicate, PanelAction::Close] {
            if buttons
                .iter()
                .any(|(_, b)| b.panel == panel && b.action == action)
            {
                continue;
            }
            commands.spawn_scene(bsn! {
                button_chrome()
                PanelButton { panel: { panel }, action: { action } }
                Children [(
                    Text({ action.glyph().to_string() })
                    TextFont { font_size: { bevy::text::FontSize::Px(15.0) } }
                    TextColor({ Color::srgb(0.85, 0.9, 0.95) })
                )]
            });
        }
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
        if *interaction != Interaction::Pressed
            || button.action != PanelAction::Duplicate
            || count >= MAX_PANELS
        {
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

/// Close a panel when its button is pressed, and close the gap it leaves.
///
/// Cell assignment is by index, so the remaining panels are renumbered to stay
/// contiguous — otherwise the grid would keep a hole and the cursor would map
/// to a panel that is no longer there. Camera order follows the index, and the
/// panel that ends up first takes over clearing the window.
pub fn close_panel(
    mut commands: Commands,
    pressed: Query<(&Interaction, &PanelButton), Changed<Interaction>>,
    mut panels: Query<(Entity, &mut Panel, &mut Camera)>,
) {
    let closing: Vec<Entity> = pressed
        .iter()
        .filter(|(interaction, button)| {
            **interaction == Interaction::Pressed && button.action == PanelAction::Close
        })
        .map(|(_, button)| button.panel)
        .collect();
    if closing.is_empty() {
        return;
    }

    let existing: Vec<(Entity, usize)> = panels
        .iter()
        .map(|(entity, panel, _)| (entity, panel.index))
        .collect();
    let remaining = renumber(&existing, &closing);
    // Never close the last frame: an empty window offers no way back.
    if remaining.is_empty() {
        return;
    }

    for entity in closing {
        commands.entity(entity).despawn();
    }

    for (position, entity) in remaining.into_iter().enumerate() {
        let Ok((_, mut panel, mut camera)) = panels.get_mut(entity) else {
            continue;
        };
        panel.index = position;
        camera.order = position as isize;
        camera.clear_color = clear_color_for(position);
    }
}

/// The panels left after closing, in the order they should occupy cells.
///
/// Returns empty when everything would be closed, which the caller treats as a
/// refusal rather than emptying the window.
fn renumber(existing: &[(Entity, usize)], closing: &[Entity]) -> Vec<Entity> {
    if existing.iter().all(|(entity, _)| closing.contains(entity)) {
        return Vec::new();
    }
    let mut remaining: Vec<(Entity, usize)> = existing
        .iter()
        .copied()
        .filter(|(entity, _)| !closing.contains(entity))
        .collect();
    // Keep the surviving panels in their existing order so closing one shuffles
    // the rest along rather than rearranging the whole grid.
    remaining.sort_by_key(|(_, index)| *index);
    remaining.into_iter().map(|(entity, _)| entity).collect()
}

/// Only the first camera clears the window; a later clear would wipe the panels
/// already drawn.
fn clear_color_for(index: usize) -> ClearColorConfig {
    if index == 0 {
        ClearColorConfig::Custom(Color::srgb(0.04, 0.04, 0.06))
    } else {
        ClearColorConfig::None
    }
}

/// Highlight the button under the pointer, warning on the destructive one.
pub fn highlight_panel_buttons(
    mut buttons: Query<(&Interaction, &PanelButton, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (interaction, button, mut colour) in &mut buttons {
        let active = match button.action {
            PanelAction::Duplicate => Color::srgba(0.35, 0.55, 0.85, 0.95),
            PanelAction::Close => Color::srgba(0.80, 0.30, 0.30, 0.95),
        };
        colour.0 = match interaction {
            Interaction::Pressed => active,
            Interaction::Hovered => active.with_alpha(0.7),
            Interaction::None => IDLE_BUTTON,
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
    area: Res<FrameArea>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    ui: Query<&Interaction, With<BlocksFrameInput>>,
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
    // Measured inside the grid, so chrome docked beside it neither receives
    // frame input nor shifts which frame the pointer is over.
    let local = cursor - area.origin;
    if drag.is_none() && (local.x < 0.0 || local.y < 0.0) {
        wheel.clear();
        return;
    }
    let window_size = area.size;

    if !held {
        *drag = None;
    } else if drag.is_none()
        && (buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Middle))
    {
        *drag = Some(Drag {
            panel: panel_under_cursor(local, window_size, count),
            last: cursor,
        });
    }

    let active = active_panel(*drag, local, window_size, count);

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

    fn panels(count: usize) -> Vec<(Entity, usize)> {
        (0..count)
            .map(|i| (Entity::from_raw_u32(i as u32 + 1).unwrap(), i))
            .collect()
    }

    #[test]
    fn closing_a_panel_closes_the_gap_it_leaves() {
        let all = panels(4);
        // Close the second of four.
        let left = renumber(&all, &[all[1].0]);
        assert_eq!(left, vec![all[0].0, all[2].0, all[3].0]);
    }

    #[test]
    fn the_surviving_panels_keep_their_relative_order() {
        let all = panels(5);
        let left = renumber(&all, &[all[0].0]);
        assert_eq!(left, vec![all[1].0, all[2].0, all[3].0, all[4].0]);
    }

    #[test]
    fn renumbering_is_independent_of_query_order() {
        // ECS iteration order is not the cell order, so the result has to come
        // from the recorded indices rather than from how panels are visited.
        let all = panels(4);
        let shuffled = vec![all[2], all[0], all[3], all[1]];
        assert_eq!(renumber(&shuffled, &[]), renumber(&all, &[]));
    }

    #[test]
    fn the_last_panel_cannot_be_closed() {
        let all = panels(1);
        assert!(renumber(&all, &[all[0].0]).is_empty());

        // Nor can every panel be closed at once.
        let all = panels(3);
        let everything: Vec<Entity> = all.iter().map(|(e, _)| *e).collect();
        assert!(renumber(&all, &everything).is_empty());
    }

    #[test]
    fn only_the_first_cell_clears_the_window() {
        // A second clear would wipe the panels already drawn beneath it.
        assert!(matches!(clear_color_for(0), ClearColorConfig::Custom(_)));
        for index in 1..MAX_PANELS {
            assert!(matches!(clear_color_for(index), ClearColorConfig::None));
        }
    }

    #[test]
    fn the_buttons_do_not_overlap() {
        let slots = [PanelAction::Close.slot(), PanelAction::Duplicate.slot()];
        assert_ne!(slots[0], slots[1]);
        // Slots are measured in button widths from the right edge, so adjacent
        // slots must be at least one button plus its gap apart.
        let spacing = (slots[1] - slots[0]).abs() * (BUTTON_PX + BUTTON_GAP);
        assert!(spacing >= BUTTON_PX);
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
