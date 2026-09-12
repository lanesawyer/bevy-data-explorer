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

/// The frame the sidebar's controls act on.
///
/// Selection follows the last frame the pointer acted in, so the controls
/// always describe the view just touched.
#[derive(Resource, Default)]
pub struct SelectedPanel(pub Option<Entity>);

/// The outline drawn around the selected frame.
#[derive(Component, Clone, Default)]
pub struct SelectionBorder;

/// A change to the set of frames.
///
/// Both a frame's own corner buttons and the sidebar's layout menu raise these,
/// so the two routes cannot drift apart: the rules about what may be opened or
/// closed live in one place.
#[derive(Message, Clone, Copy)]
pub enum PanelRequest {
    Duplicate(Entity),
    Close(Entity),
    /// Open a new frame onto a source.
    Open(Entity),
    /// Show what is known about a frame.
    Inspect(Entity),
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

    /// Take `amount` off the right edge, for chrome docked there.
    pub fn reserve_right(&mut self, amount: f32) {
        let amount = amount.clamp(0.0, self.size.x);
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
/// Spawn the outline that marks the selected frame.
pub fn spawn_selection_border(commands: &mut Commands) {
    commands.spawn_scene(bsn! {
        SelectionBorder
        Node {
            position_type: { PositionType::Absolute },
            border: { UiRect::all(Val::Px(SELECTION_PX)) },
            display: { Display::None },
        }
        GlobalZIndex({ SELECTION_Z })
        template_value(BorderColor::all(SELECTION_COLOUR))
    });
}

/// Keep the outline over the selected frame, and pick one if none is selected.
pub fn update_selection_border(
    mut selected: ResMut<SelectedPanel>,
    area: Res<FrameArea>,
    panels: Query<(Entity, &Panel)>,
    mut border: Query<&mut Node, With<SelectionBorder>>,
) {
    // A closed frame leaves the selection dangling, and there is always a frame
    // to fall back to because the last one cannot be closed.
    let still_there = selected.0.is_some_and(|entity| panels.get(entity).is_ok());
    if !still_there {
        selected.0 = panels
            .iter()
            .min_by_key(|(_, panel)| panel.index)
            .map(|(entity, _)| entity);
    }

    let (columns, rows) = grid_for(panels.iter().count());
    let cell = Vec2::new(area.size.x / columns as f32, area.size.y / rows as f32);

    for mut node in &mut border {
        let Some(panel) = selected.0.and_then(|e| panels.get(e).ok()) else {
            node.display = Display::None;
            continue;
        };
        let (col, row) = (panel.1.index % columns, panel.1.index / columns);
        node.display = Display::Flex;
        node.left = Val::Px(area.origin.x + cell.x * col as f32);
        node.top = Val::Px(area.origin.y + cell.y * row as f32);
        node.width = Val::Px(cell.x);
        node.height = Val::Px(cell.y);
    }
}

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

const SELECTION_PX: f32 = 2.0;
const SELECTION_COLOUR: Color = Color::srgb(0.38, 0.60, 0.90);

/// Draw order for the selection outline.
///
/// A cell's left and top borders fall exactly on the rules between cells, since
/// a border is drawn inside the node while the rule sits just outside it. The
/// outline and the rules are separate UI roots, so nothing orders them
/// implicitly and the rule would cover the shared edges — leaving every frame
/// except the top-left one outlined on two sides only.
const SELECTION_Z: i32 = 1;

const BUTTON_PX: f32 = 22.0;
const BUTTON_GAP: f32 = 4.0;
const IDLE_BUTTON: Color = Color::srgba(0.18, 0.20, 0.26, 0.85);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelAction {
    Duplicate,
    Close,
    Info,
}

impl PanelAction {
    const ALL: [PanelAction; 3] = [
        PanelAction::Info,
        PanelAction::Duplicate,
        PanelAction::Close,
    ];

    /// Buttons are laid out right to left from the panel's top corner.
    fn slot(self) -> f32 {
        match self {
            PanelAction::Close => 0.0,
            PanelAction::Duplicate => 1.0,
            PanelAction::Info => 2.0,
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            PanelAction::Duplicate => "+",
            PanelAction::Close => "x",
            PanelAction::Info => "i",
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
        for action in PanelAction::ALL {
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
pub fn panel_buttons(
    mut requests: MessageWriter<PanelRequest>,
    pressed: Query<(&Interaction, &PanelButton), Changed<Interaction>>,
) {
    for (interaction, button) in &pressed {
        if *interaction != Interaction::Pressed {
            continue;
        }
        requests.write(match button.action {
            PanelAction::Duplicate => PanelRequest::Duplicate(button.panel),
            PanelAction::Close => PanelRequest::Close(button.panel),
            PanelAction::Info => PanelRequest::Inspect(button.panel),
        });
    }
}

/// Apply requested changes to the set of frames.
pub fn apply_panel_requests(
    mut commands: Commands,
    mut requests: MessageReader<PanelRequest>,
    mut selected: ResMut<SelectedPanel>,
    area: Res<FrameArea>,
    panels: Query<(
        Entity,
        &Panel,
        &ShowsSource,
        &Transform,
        &Projection,
        &ViewLimits,
    )>,
    sources: Query<(&DataSource, &crate::datasource::SourceExtent)>,
) {
    let requests: Vec<PanelRequest> = requests.read().copied().collect();
    if requests.is_empty() {
        return;
    }

    let mut open: Vec<(usize, Entity)> = panels
        .iter()
        .map(|(entity, panel, ..)| (panel.index, entity))
        .collect();
    open.sort_unstable();

    let (columns, rows) = grid_for(open.len().max(1));
    let viewport = Vec2::new(area.size.x / columns as f32, area.size.y / rows as f32);

    let mut closing: Vec<Entity> = Vec::new();
    let mut spawned = 0usize;

    for request in requests {
        match request {
            PanelRequest::Duplicate(panel) => {
                if open.len() + spawned >= MAX_PANELS {
                    continue;
                }
                let Ok((_, _, shows, transform, projection, limits)) = panels.get(panel) else {
                    continue;
                };
                let Ok((source, _)) = sources.get(shows.0) else {
                    continue;
                };
                let Projection::Orthographic(ortho) = projection else {
                    continue;
                };
                spawn_panel(
                    &mut commands,
                    shows.0,
                    source.layer,
                    open.len() + spawned,
                    *limits,
                    Some(View {
                        centre: transform.translation.truncate(),
                        scale: ortho.scale,
                    }),
                );
                spawned += 1;
            }
            PanelRequest::Open(source_entity) => {
                if open.len() + spawned >= MAX_PANELS {
                    continue;
                }
                let Ok((source, extent)) = sources.get(source_entity) else {
                    continue;
                };
                spawn_panel(
                    &mut commands,
                    source_entity,
                    source.layer,
                    open.len() + spawned,
                    extent.limits(viewport),
                    None,
                );
                spawned += 1;
            }
            PanelRequest::Close(panel) => {
                if panels.get(panel).is_ok() && !closing.contains(&panel) {
                    closing.push(panel);
                }
            }
            // Selecting is handled here so the inspector can simply follow the
            // selection rather than tracking a frame of its own.
            PanelRequest::Inspect(panel) => {
                if panels.get(panel).is_ok() {
                    selected.0 = Some(panel);
                }
            }
        }
    }

    if closing.is_empty() {
        return;
    }
    let existing: Vec<(Entity, usize)> = panels
        .iter()
        .map(|(entity, panel, ..)| (entity, panel.index))
        .collect();
    // Never close the last frame: an empty window offers no way back.
    if renumber(&existing, &closing).is_empty() && spawned == 0 {
        return;
    }
    for entity in closing {
        commands.entity(entity).despawn();
    }
    // Cells, draw order and which camera clears are settled by
    // `normalize_panels` once the despawns have taken effect.
}

/// Keep cells contiguous, and keep draw order and clearing in step with them.
///
/// Every path that adds or removes a frame funnels through here rather than
/// fixing up indices itself. Opening and closing in the same breath can hand
/// out a duplicate index, and a frame spawned this tick is not yet visible to
/// the code that renumbers the survivors, so ownership of the invariant sits in
/// one place that runs after the dust settles.
pub fn normalize_panels(mut panels: Query<(Entity, &mut Panel, &mut Camera)>) {
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
        let wanted_order = position as isize;
        if panel.index == position && camera.order == wanted_order {
            continue;
        }
        panel.index = position;
        camera.order = wanted_order;
        // Only the first camera clears. Losing the frame that held that job
        // leaves nothing clearing the window, and every frame then paints over
        // the last one instead of replacing it.
        camera.clear_color = clear_color_for(position);
    }
}

/// Cells for the given frames, keeping their relative order and closing any
/// gaps or duplicates.
fn assign_cells(mut frames: Vec<(usize, Entity)>) -> Vec<(usize, Entity)> {
    // Sorting by index then entity keeps the result stable when two frames
    // claim the same cell, which happens when one is opened in the same breath
    // as another is closed.
    frames.sort_unstable();
    frames
        .into_iter()
        .enumerate()
        .map(|(position, (_, entity))| (position, entity))
        .collect()
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
            PanelAction::Duplicate | PanelAction::Info => Color::srgba(0.35, 0.55, 0.85, 0.95),
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
    panel_entities: Query<(Entity, &Panel)>,
    mut selected: ResMut<SelectedPanel>,
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
        let index = panel_under_cursor(local, window_size, count);
        *drag = Some(Drag {
            panel: index,
            last: cursor,
        });
        selected.0 = panel_entities
            .iter()
            .find(|(_, panel)| panel.index == index)
            .map(|(entity, _)| entity);
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

    /// Span a cell's left border occupies, and the rule drawn at that column.
    fn left_border_span(col: usize, cell_x: f32) -> (f32, f32) {
        let left = cell_x * col as f32;
        (left, left + SELECTION_PX)
    }

    fn rule_span(ordinal: usize, cell_x: f32) -> (f32, f32) {
        let left = cell_x * (ordinal + 1) as f32;
        (left, left + DIVIDER_PX)
    }

    fn overlaps(a: (f32, f32), b: (f32, f32)) -> bool {
        a.0 < b.1 && b.0 < a.1
    }

    #[test]
    fn a_cells_left_border_lands_on_the_rule_beside_it() {
        // This is why the outline needs an explicit draw order: for every
        // column but the first, the border and the rule occupy the same pixels.
        let cell = 400.0;
        assert!(overlaps(left_border_span(1, cell), rule_span(0, cell)));
        assert!(overlaps(left_border_span(2, cell), rule_span(1, cell)));
        // The first column has no rule to its left, which is why that frame
        // looked correctly outlined while the others did not.
        assert!(!overlaps(left_border_span(0, cell), rule_span(0, cell)));
    }

    #[test]
    fn the_outline_draws_above_the_rules() {
        // Rules carry no explicit index, so they sit at zero.
        assert!(SELECTION_Z > 0);
    }

    #[test]
    fn cells_end_up_unique_and_contiguous() {
        let e = |n: u32| Entity::from_raw_u32(n).unwrap();
        // Gaps from closing, and a duplicate from opening while closing.
        let frames = vec![(0, e(1)), (2, e(2)), (2, e(3)), (7, e(4))];
        let cells = assign_cells(frames);

        let positions: Vec<usize> = cells.iter().map(|(p, _)| *p).collect();
        assert_eq!(positions, vec![0, 1, 2, 3]);

        let entities: std::collections::HashSet<Entity> = cells.iter().map(|(_, e)| *e).collect();
        assert_eq!(entities.len(), 4, "a frame was dropped or duplicated");
    }

    #[test]
    fn exactly_one_frame_clears_the_window() {
        // With none clearing, each frame paints over the last instead of
        // replacing it; with several, later ones wipe what came before.
        let e = |n: u32| Entity::from_raw_u32(n).unwrap();
        let cells = assign_cells(vec![(3, e(1)), (5, e(2)), (9, e(3))]);
        let clearing = cells
            .iter()
            .filter(|(position, _)| {
                matches!(clear_color_for(*position), ClearColorConfig::Custom(_))
            })
            .count();
        assert_eq!(clearing, 1);
    }

    #[test]
    fn closing_the_first_frame_hands_clearing_to_another() {
        let e = |n: u32| Entity::from_raw_u32(n).unwrap();
        // Frame 0 is gone; whatever is left must take over clearing.
        let cells = assign_cells(vec![(1, e(2)), (2, e(3))]);
        assert_eq!(cells[0].0, 0);
        assert!(matches!(
            clear_color_for(cells[0].0),
            ClearColorConfig::Custom(_)
        ));
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
        let mut slots: Vec<f32> = PanelAction::ALL.iter().map(|a| a.slot()).collect();
        slots.sort_by(f32::total_cmp);
        for pair in slots.windows(2) {
            // Slots are measured in button widths from the right edge, so
            // adjacent slots must be at least one button plus its gap apart.
            let spacing = (pair[1] - pair[0]) * (BUTTON_PX + BUTTON_GAP);
            assert!(spacing >= BUTTON_PX, "buttons at {pair:?} would overlap");
        }
    }

    #[test]
    fn every_action_has_its_own_slot_and_glyph() {
        let slots: std::collections::HashSet<u32> = PanelAction::ALL
            .iter()
            .map(|a| a.slot().to_bits())
            .collect();
        let glyphs: std::collections::HashSet<&str> =
            PanelAction::ALL.iter().map(|a| a.glyph()).collect();
        assert_eq!(slots.len(), PanelAction::ALL.len());
        assert_eq!(glyphs.len(), PanelAction::ALL.len());
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
