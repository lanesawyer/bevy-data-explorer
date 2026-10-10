//! Popups anchored under the button that opens them.
//!
//! Every one is Feathers' menu: the button toggles the popup, `Popover` puts
//! it under the button — or above it when there is no room below — and
//! `OverrideClip` lets it overhang the sidebar or frame header that would
//! otherwise clip it. It closes on Escape, and when the keyboard leaves it,
//! which a click anywhere outside it does.
//!
//! What goes in one is whatever the caller adds, not only menu items: fields,
//! checkboxes, a paragraph saying what the buttons do. A click on something in
//! it that cannot take the keyboard would move the keyboard to the window and
//! so close the menu under the pointer, which is why the popup takes the
//! keyboard itself, with a `TabIndex` that keeps it out of tabbing.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy_feathers::controls::{FeathersMenu, FeathersMenuPopup, FeathersMenuToolButton};
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::{MenuAction, MenuEvent, ScrollArea};

use super::space;
use super::{BlocksFrameInput, Icon, button_icon};

/// What [`spawn_icon_menu`] built.
pub struct IconMenu {
    /// Holds the button and the popup: hide this to take the menu away.
    pub root: Entity,
    pub button: Entity,
    pub popup: Entity,
}

/// Width of a menu popup.
pub const MENU_WIDTH: f32 = 320.0;
/// The tallest a menu grows, as a share of the window, before it scrolls.
const MENU_MAX_HEIGHT_VH: f32 = 70.0;

/// Add a `...` menu button under `parent`, returning the popup for the caller
/// to fill.
pub fn spawn_menu(commands: &mut Commands, parent: Entity) -> Entity {
    let popup = spawn_popup(commands);
    let button = commands
        .spawn_scene(bsn! {
            @FeathersMenuToolButton {
                @caption: { bsn_list! {@button_icon(Icon::Ellipsis)} },
                @arrow: false,
            }
            BlocksFrameInput
        })
        .id();
    spawn_root(commands, parent, button, popup);
    popup
}

/// A menu behind a button showing `icon` and a chevron.
///
/// Only the ellipsis says "menu" by itself. Any other icon reads as a button
/// that acts when pressed, so the chevron is what warns that this one opens
/// something instead — and it is not optional, so no menu goes without it.
pub fn spawn_icon_menu(commands: &mut Commands, parent: Entity, icon: Icon) -> IconMenu {
    let popup = spawn_popup(commands);
    let button = commands
        .spawn_scene(bsn! {
            @FeathersMenuToolButton {
                @caption: { bsn_list! {@button_icon(icon)} }
            }
            BlocksFrameInput
        })
        .id();
    let root = spawn_root(commands, parent, button, popup);
    IconMenu {
        root,
        button,
        popup,
    }
}

/// A menu behind a `FeathersMenuToolButton` the caller built, for a button
/// that says more than an icon. Left for the caller to place.
pub fn menu_behind(commands: &mut Commands, button: Entity) -> IconMenu {
    let popup = spawn_popup(commands);
    let root = commands
        .spawn_scene(bsn! { @FeathersMenu })
        .add_children(&[button, popup])
        .id();
    IconMenu {
        root,
        button,
        popup,
    }
}

/// The menu that holds `button` and `popup`, which `Popover` places the popup
/// against.
fn spawn_root(commands: &mut Commands, parent: Entity, button: Entity, popup: Entity) -> Entity {
    let root = commands
        .spawn_scene(bsn! { @FeathersMenu })
        .add_children(&[button, popup])
        .id();
    commands.entity(parent).add_child(root);
    root
}

/// A closed popup, for the caller to fill.
fn spawn_popup(commands: &mut Commands) -> Entity {
    commands
        .spawn_scene(bsn! {
            @FeathersMenuPopup
            // A menu long enough to run off the screen scrolls instead.
            ScrollArea
            Node {
                width: { Val::Px(MENU_WIDTH) },
                max_height: { Val::Vh(MENU_MAX_HEIGHT_VH) },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(space::ROWS) },
                padding: { UiRect::all(Val::Px(space::PANEL_INSET)) },
                overflow: { Overflow::scroll_y() },
            }
            InheritableFont { font_size: { 13.0f32 } }
            TabIndex(-1)
            BlocksFrameInput
        })
        .id()
}

/// Close the menu holding `inside`, once what was pressed in it is done, and
/// hand the keyboard back to its button.
pub fn close_menu_holding(commands: &mut Commands, inside: Entity) {
    for action in [MenuAction::FocusRoot, MenuAction::CloseAll] {
        commands.trigger(MenuEvent {
            source: inside,
            action,
        });
    }
}
