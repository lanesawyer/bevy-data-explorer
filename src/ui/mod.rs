//! Chrome that belongs to no single frame: the docks and the controls inside
//! them.

use bevy::prelude::*;

pub mod addsource;
pub mod cellpanel;
pub mod channels;
pub mod help;
pub mod inspector;
pub mod layers;
pub mod logpanel;
pub mod sidebar;
pub mod viewconfig;
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
            viewconfig::ViewConfigPlugin,
            channels::ChannelControlsPlugin,
            layers::LayersPlugin,
            addsource::AddSourcePlugin,
            cellpanel::CellPanelPlugin,
            welcome::WelcomePlugin,
            logpanel::LogPanelPlugin,
            help::HelpPlugin,
        ));
    }
}
