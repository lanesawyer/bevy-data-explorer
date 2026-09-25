//! Chrome that belongs to no single frame: the docks and the controls inside
//! them.

use bevy::prelude::*;

use crate::app::schedule::Stage;
use crate::view::FrameArea;
use crate::widgets::ModalScreen;

pub mod add_source;
pub mod bookmarks;
pub mod cell_panel;
pub mod channels;
pub mod color_export;
pub mod color_overrides;
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
            color_overrides::ColorOverridesPlugin,
            color_export::ColorExportPlugin,
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
        ))
        .add_systems(Update, center_modals.in_set(Stage::Chrome));
    }
}

/// Center each modal's panel over the frames rather than the whole window.
///
/// The backdrop still covers everything, docks included, since none of it is
/// meant to be used while a modal is up. Only the panel moves: centered on the
/// window, it sat off to one side of the frames with the sidebar open, which
/// is where the eye already is. Padding the backdrop by what the docks take
/// leaves its centering to place the panel.
pub fn center_modals(
    area: Res<FrameArea>,
    windows: Query<&Window>,
    mut screens: Query<&mut Node, With<ModalScreen>>,
) {
    let Ok(window) = windows.single() else { return };
    let wanted = frames_inset(&area, window.size());
    for mut node in &mut screens {
        if node.padding != wanted {
            node.padding = wanted;
        }
    }
}

/// How far the frame area sits in from each edge of a window this size.
fn frames_inset(area: &FrameArea, window: Vec2) -> UiRect {
    let far = window - (area.origin + area.size);
    UiRect {
        left: Val::Px(area.origin.x),
        right: Val::Px(far.x.max(0.0)),
        top: Val::Px(area.origin.y),
        bottom: Val::Px(far.y.max(0.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modal_is_inset_by_what_the_docks_take() {
        // A sidebar of 400 on the left and a log of 240 along the bottom.
        let area = FrameArea {
            origin: Vec2::new(400.0, 0.0),
            size: Vec2::new(1200.0, 760.0),
        };
        let inset = frames_inset(&area, Vec2::new(1600.0, 1000.0));
        assert_eq!(inset.left, Val::Px(400.0));
        assert_eq!(inset.right, Val::Px(0.0));
        assert_eq!(inset.top, Val::Px(0.0));
        assert_eq!(inset.bottom, Val::Px(240.0));
    }
}
