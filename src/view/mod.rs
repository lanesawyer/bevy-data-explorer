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

pub mod camera;
pub mod capture;
pub mod chrome;
pub mod grid;
pub mod input;
pub mod overlay;
pub mod requests;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::app::schedule::{Boot, Stage};
use crate::source::{DataSource, SourceExtent, ViewLimits};

use camera::{clear_when_empty, follow_theme, normalize_panels, spawn_ui_camera, update_viewports};
use chrome::{
    panel_buttons, spawn_dividers, spawn_selection_border, sync_panel_buttons,
    update_selection_border,
};
use grid::clear_color_for;
use input::{page_slice_stack, panel_controls, probe_hover, track_text_focus};
use requests::apply_panel_requests;

// The grid's vocabulary, kept importable from `view` itself so that what a
// caller needs to know does not depend on how this module is cut up.
pub use grid::{MAX_PANELS, grid_for};
pub use input::{BlocksFrameInput, TextEntryFocused};
pub use requests::PanelRequest;

/// The frame the sidebar's controls act on.
///
/// Selection follows the last frame the pointer acted in, so the controls
/// always describe the view just touched.
#[derive(Resource, Default)]
pub struct SelectedPanel(pub Option<Entity>);

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

    /// Take `amount` off the bottom edge, for chrome docked there.
    pub fn reserve_bottom(&mut self, amount: f32) {
        let amount = amount.clamp(0.0, self.size.y);
        self.size.y -= amount;
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

/// The view a panel is currently showing, used when duplicating it.
#[derive(Clone, Copy)]
pub struct View {
    pub centre: Vec2,
    pub scale: f32,
}

/// Spawn a panel camera in cell `index`, showing `source`.
#[expect(
    clippy::too_many_arguments,
    reason = "a frame is a camera, a cell, a source, a view and the colour it clears to"
)]
pub fn spawn_panel(
    commands: &mut Commands,
    source: Entity,
    layer: usize,
    index: usize,
    limits: ViewLimits,
    view: Option<View>,
    background: Color,
) -> Entity {
    let view = view.unwrap_or(View {
        centre: limits.centre,
        scale: limits.fit_scale,
    });
    commands
        .spawn_scene(bsn! {
            Camera2d
            Camera {
                clear_color: { clear_color_for(index, background) },
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

/// The frame grid: the cameras, their chrome, and the pointer input that drives
/// them.
pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((overlay::OverlayPlugin, capture::CapturePlugin))
            .add_message::<PanelRequest>()
            .init_resource::<FrameArea>()
            .init_resource::<SelectedPanel>()
            .init_resource::<TextEntryFocused>()
            .add_observer(panel_buttons)
            .add_systems(Update, track_text_focus.in_set(Stage::Focus))
            .add_systems(Update, reset_frame_area.in_set(Stage::FrameArea))
            .add_systems(
                Update,
                (apply_panel_requests, normalize_panels)
                    .chain()
                    .in_set(Stage::Frames),
            )
            .add_systems(Update, sync_panel_buttons.in_set(Stage::FrameChrome))
            .add_systems(
                Update,
                (
                    panel_controls,
                    update_viewports,
                    clear_when_empty,
                    follow_theme,
                )
                    .chain()
                    .in_set(Stage::Viewports),
            )
            .add_systems(Update, update_selection_border.in_set(Stage::ControlsPlace))
            // Paging writes through to the source it pages, which is what every
            // other control does in this stage — and being here is what has the
            // new slice streaming the same frame it was asked for.
            .add_systems(Update, page_slice_stack.in_set(Stage::ControlsApply))
            .add_systems(Update, probe_hover.in_set(Stage::HoverProbe))
            .add_systems(
                Startup,
                (spawn_ui_camera, spawn_dividers, spawn_selection_border).in_set(Boot::Shell),
            )
            .add_systems(Startup, open_frames.in_set(Boot::Frames));
    }
}

/// Open one frame per registered source, in registration order.
///
/// Sources are discovered from the world rather than listed here, so adding a
/// format plugin is enough to get it a frame.
fn open_frames(
    mut commands: Commands,
    windows: Query<&Window>,
    palette: Res<crate::app::theme::Palette>,
    sources: Query<(Entity, &DataSource, &SourceExtent)>,
) {
    let window = windows
        .iter()
        .next()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));

    let mut sources: Vec<(Entity, &DataSource, &SourceExtent)> = sources.iter().collect();
    // Layers are handed out in registration order, which is the order the
    // plugins were added.
    sources.sort_by_key(|(_, source, _)| source.layer);

    let (columns, rows) = grid_for(sources.len());
    let viewport = Vec2::new(window.x / columns as f32, window.y / rows as f32);

    for (index, (entity, source, extent)) in sources.into_iter().enumerate() {
        spawn_panel(
            &mut commands,
            entity,
            source.layer,
            index,
            extent.limits(viewport),
            None,
            palette.frame_bg,
        );
    }
}
