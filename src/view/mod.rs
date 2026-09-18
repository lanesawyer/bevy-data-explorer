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
//! A frame can stack further sources over the one it opened onto; see
//! [`layers`].
//!
//! Panels can be duplicated at runtime. A duplicate is another camera on the
//! same layer, which is why duplicating costs no extra geometry: the two
//! cameras draw the same entities from different viewpoints.

pub mod camera;
pub mod capture;
pub mod chrome;
pub mod dataset_menu;
pub mod grid;
pub mod input;
pub mod layers;
pub mod loading;
pub mod orbit;
pub mod overlay;
pub mod preset;
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
use input::{page_slice_stack, panel_controls, probe_hover, reset_selected_view, track_text_focus};
use requests::apply_panel_requests;

// The grid's vocabulary, kept importable from `view` itself so that what a
// caller needs to know does not depend on how this module is cut up.
pub use grid::{MAX_PANELS, grid_for};
pub use input::{BlocksFrameInput, TextEntryFocused};
pub use layers::{FrameLayers, LayerOf, LayerOpacity, OpensAsLayer};
pub use orbit::Orbit;
pub use preset::{OpenPreset, Preset, PresetDataset};
pub use requests::{DatasetRequest, DatasetTarget, PanelRequest, PendingShow};

/// The frame the sidebar's controls act on.
///
/// Selection follows the last frame the pointer acted in, so the controls
/// always describe the view just touched.
#[derive(Resource, Default)]
pub struct SelectedPanel(pub Option<Entity>);

/// The source the selected frame is showing, if a frame is selected at all.
///
/// Every keyboard shortcut goes through this. A key acts on the frame that is
/// selected — the one outlined in blue — and on nothing else: with two datasets
/// open, a key that reached both would page or toggle the one nobody was
/// looking at, and with the pointer somewhere over the sidebar it would be
/// unclear which frame it had been talking to.
pub fn selected_source(selected: &SelectedPanel, panels: &Query<&ShowsSource>) -> Option<Entity> {
    selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0)
}

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

    /// Where cell `index` lies while the grid holds `count` frames.
    pub fn cell(&self, count: usize, index: usize) -> Rect {
        let (columns, rows) = grid_for(count);
        let size = self.size / Vec2::new(columns as f32, rows as f32);
        let at = Vec2::new((index % columns) as f32, (index / columns) as f32);
        let min = self.origin + size * at;
        Rect::from_corners(min, min + size)
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
#[derive(Clone, Copy, Debug)]
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
                order: { grid::camera_order(index, 0) },
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
            .add_message::<DatasetRequest>()
            .add_message::<OpenPreset>()
            .init_resource::<preset::PendingPresets>()
            .init_resource::<FrameArea>()
            .init_resource::<SelectedPanel>()
            .init_resource::<TextEntryFocused>()
            .add_observer(panel_buttons)
            .add_observer(orbit::on_view_toggled)
            .add_systems(Update, track_text_focus.in_set(Stage::Focus))
            .add_systems(Update, reset_frame_area.in_set(Stage::FrameArea))
            .add_systems(
                Update,
                (
                    preset::open_presets,
                    apply_panel_requests,
                    normalize_panels,
                    // Once the frames just asked for exist, so a preset's
                    // layers go onto the frame opened for it.
                    preset::layer_presets,
                    preset::attach_follows,
                    preset::fit_presets,
                )
                    .chain()
                    .in_set(Stage::Frames),
            )
            .add_systems(
                Update,
                (sync_panel_buttons, loading::sync_loading_bars).in_set(Stage::FrameChrome),
            )
            .add_systems(
                Update,
                (
                    panel_controls,
                    reset_selected_view,
                    // After the pointer has turned it, so the camera never lags
                    // a gesture, and before the layers copy the frame's view.
                    orbit::apply_orbits,
                    update_viewports,
                    // After the viewports, and after the pan and zoom, so a
                    // layer never lags a frame behind what it is drawn over.
                    layers::sync_layers,
                    clear_when_empty,
                    follow_theme,
                )
                    .chain()
                    .in_set(Stage::Viewports),
            )
            .add_systems(Update, orbit::sync_view_buttons.in_set(Stage::Chrome))
            .add_systems(Update, update_selection_border.in_set(Stage::ControlsPlace))
            // Paging writes through to the source it pages, which is what every
            // other control does in this stage — and being here is what has the
            // new slice streaming the same frame it was asked for.
            .add_systems(Update, page_slice_stack.in_set(Stage::ControlsApply))
            // Writes through to the sources that follow another's sections,
            // which the formats read as they stream.
            .add_systems(
                Update,
                crate::source::sections::follow_sections.in_set(Stage::ControlsApply),
            )
            .add_systems(Update, probe_hover.in_set(Stage::HoverProbe))
            // After the sources, which is when each says whether it is fetching.
            .add_systems(Update, loading::update_loading_bars.in_set(Stage::Overlay))
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
/// format plugin is enough to get it a frame. One named as a layer is stacked
/// onto the first frame instead.
fn open_frames(
    mut commands: Commands,
    windows: Query<&Window>,
    palette: Res<crate::app::theme::Palette>,
    sources: Query<(Entity, &DataSource, &SourceExtent, Has<OpensAsLayer>)>,
) {
    let window = windows.iter().next().map_or(Vec2::new(1280.0, 720.0), |w| {
        Vec2::new(w.width(), w.height())
    });

    let mut sources: Vec<(Entity, &DataSource, &SourceExtent, bool)> = sources.iter().collect();
    // Layers are handed out in registration order, which is the order the
    // plugins were added.
    sources.sort_by_key(|(_, source, _, _)| source.layer);

    // Layers go onto the first frame, so there has to be one: with nothing
    // else named, they take frames of their own.
    let any_frame = sources.iter().any(|(.., as_layer)| !as_layer);
    let (layered, framed): (Vec<_>, Vec<_>) = sources
        .iter()
        .partition(|(.., as_layer)| *as_layer && any_frame);

    let (columns, rows) = grid_for(framed.len());
    let viewport = Vec2::new(window.x / columns as f32, window.y / rows as f32);

    let mut first_panel = None;
    for (index, (entity, source, extent, _)) in framed.into_iter().enumerate() {
        let panel = spawn_panel(
            &mut commands,
            entity,
            source.layer,
            index,
            extent.limits(viewport),
            None,
            palette.frame_bg,
        );
        first_panel.get_or_insert(panel);
    }
    if let Some(panel) = first_panel {
        for (entity, source, ..) in layered.into_iter().take(grid::MAX_LAYERS - 1) {
            layers::spawn_layer(
                &mut commands,
                panel,
                entity,
                source.layer,
                LayerOpacity::default(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_tile_the_frame_area_from_its_origin() {
        let area = FrameArea {
            origin: Vec2::new(300.0, 0.0),
            size: Vec2::new(900.0, 600.0),
        };
        // Five frames make a three by two grid.
        let first = area.cell(5, 0);
        assert_eq!(first.min, Vec2::new(300.0, 0.0));
        assert_eq!(first.size(), Vec2::new(300.0, 300.0));
        let last = area.cell(5, 4);
        assert_eq!(last.min, Vec2::new(600.0, 300.0));
        assert_eq!(area.cell(5, 5).max, area.origin + area.size);
    }
}
