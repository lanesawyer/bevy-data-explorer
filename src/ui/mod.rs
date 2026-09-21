//! Chrome that belongs to no single frame: the docks and the controls inside
//! them.

use bevy::prelude::*;

pub mod add_source;
pub mod bookmarks;
pub mod cell_panel;
pub mod channels;
pub mod filtered;
pub mod genes;
pub mod help;
pub mod inspector;
pub mod layers;
pub mod log_panel;
pub mod selection;
pub mod settings;
pub mod sidebar;
pub mod table_filters;
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
            selection::SelectionPanelPlugin,
            genes::GenePanelPlugin,
        ))
        // Split because a plugin tuple caps at sixteen, the way a system
        // tuple caps at twenty.
        .add_plugins((
            table_filters::TableFilterPlugin,
            welcome::WelcomePlugin,
            log_panel::LogPanelPlugin,
            help::HelpPlugin,
            settings::SettingsPlugin,
        ));
    }
}
