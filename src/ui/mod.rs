//! Chrome that belongs to no single frame: the docks and the controls inside
//! them.

use bevy::prelude::*;

use crate::view::FrameArea;
use crate::widgets::{Dock, DockEdge};

pub mod add_source;
pub mod bookmarks;
pub mod cell_panel;
pub mod channels;
pub mod color_export;
pub mod color_overrides;
pub mod dashboards;
pub mod filtered;
pub mod genes;
pub mod help;
pub mod inspector;
pub mod layers;
pub mod log_panel;
pub mod record_export;
pub mod selection;
pub mod settings;
pub mod sidebar;
pub mod table_export;
pub mod table_filters;
pub mod table_partitions;
pub mod table_search;
pub mod view_config;
pub mod welcome;

/// The docks and everything in them.
///
/// The sections are listed here rather than in `main` because which sections a
/// dock offers is a property of the dock, not of the application.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            sidebar::SidebarPlugin,
            inspector::InspectorPlugin,
            view_config::ViewConfigPlugin,
            channels::ChannelControlsPlugin,
            filtered::FilteredControlsPlugin,
            layers::LayersPlugin,
            add_source::AddSourcePlugin,
            bookmarks::BookmarksPlugin,
            cell_panel::CellPanelPlugin,
            color_overrides::ColorOverridesPlugin,
            color_export::ColorExportPlugin,
            selection::SelectionPanelPlugin,
            genes::GenePanelPlugin,
        ))
        // Split because a plugin tuple caps at sixteen, the way a system
        // tuple caps at twenty.
        .add_plugins((
            table_filters::TableFilterPlugin,
            table_export::TableExportPlugin,
            table_partitions::TablePartitionPlugin,
            table_search::TableSearchPlugin,
            welcome::WelcomePlugin,
            dashboards::DashboardsPlugin,
            log_panel::LogPanelPlugin,
            help::HelpPlugin,
            settings::SettingsPlugin,
            record_export::RecordExportPlugin,
        ));
    }
}

/// Take a dock's share of the window off the frame grid, on its own edge.
pub fn reserve_space<D: Dock>(
    dock: Res<D>,
    windows: Query<&Window>,
    ui_scale: Res<UiScale>,
    mut area: ResMut<FrameArea>,
) {
    let Ok(window) = windows.single() else { return };
    let taken = dock.taken(crate::widgets::ui_size(window, &ui_scale));
    match D::EDGE {
        DockEdge::Left => area.reserve_left(taken),
        DockEdge::Right => area.reserve_right(taken),
        DockEdge::Bottom => area.reserve_bottom(taken),
    }
}
