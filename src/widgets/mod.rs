//! Generic controls, used by both the frames and the docks: one widget to a
//! file.

use bevy::prelude::*;

mod accordion;
mod button_text;
mod caption;
mod dock;
mod field_well;
mod frame_input;
mod icons;
mod link;
mod menu;
mod modal;
mod notice;
mod patch;
mod scroll;
mod scrollbar;
mod search;
mod selectable;
mod skeleton;
mod slider;
mod spacing;
mod text;
mod truncate;

pub use accordion::{Accordion, SectionLevel, spawn_accordion, spawn_header_button};
pub use button_text::button_text;
pub use caption::caption;
pub use dock::{
    AddDock, Dock, DockEdge, DockWidth, HANDLE_PX, ResetDockSizes, dock_band, dock_handle,
    hold_drag_cursor, place_right_dock,
};
pub use field_well::field_well;
pub use frame_input::BlocksFrameInput;
pub use icons::{Icon, button_icon, icon_text};
pub use link::link_button;
pub use menu::{
    MENU_WIDTH, Menu, MenuAnchor, MenuButton, spawn_icon_menu, spawn_menu, spawn_popup,
};
pub use modal::{AddModal, Modal, ModalScreen, set_modal_open, spawn_modal};
pub use notice::{Notice, Tone, notice};
pub use patch::{display, patch_node, set_display, set_text};
pub use scroll::{ScrollBoth, scroll_list};
pub use search::{matches_search, spawn_search_field};
pub use selectable::{SelectableText, has_selection};
pub use skeleton::spawn_skeleton;
pub use slider::spawn_slider;
pub use spacing::space;
pub use text::{size, text, text_dim, title};
pub use truncate::{truncate_to_width, width_of};

use crate::app::schedule::Stage;

/// Radius the Feathers containers round their outer corners to.
const CORNER_PX: f32 = 4.0;

/// The generic controls the docks are built from: accordions, menus, sliders.
pub struct WidgetsPlugin;

impl Plugin for WidgetsPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "assets/lucide.ttf");
        app.add_plugins(selectable::SelectableTextPlugin)
            // Feathers' slider reports a value change but leaves writing it
            // back to the app; this observer is what closes that loop.
            .add_observer(bevy_ui_widgets::slider_self_update)
            .add_observer(menu::on_menu_button)
            .add_observer(accordion::toggle_accordions)
            .add_observer(link::on_link_pressed)
            .add_observer(scroll::on_list_scroll)
            .add_observer(scroll::on_both_scroll)
            .add_observer(search::on_clear_search)
            .add_systems(
                Update,
                scrollbar::add_scrollbars.in_set(Stage::ControlsBuild),
            )
            .add_systems(
                Update,
                (
                    accordion::update_accordions,
                    accordion::truncate_accordion_titles,
                    menu::dismiss_menus,
                    menu::release_focus_from_closed_menus,
                    menu::position_menus,
                    scrollbar::show_scrollbars,
                    search::sync_search_hints,
                    skeleton::pulse_skeletons,
                    notice::sync_notices,
                )
                    .chain()
                    .in_set(Stage::ControlsPlace),
            );
    }
}
