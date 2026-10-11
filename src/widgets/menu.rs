//! Popups anchored under the button that opens them.
//!
//! Every one is Feathers' menu: the button toggles the popup, `Popover` puts
//! it under the button — or above it when there is no room below, and
//! leftward from the button's right edge when there is no room to the right —
//! and
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
use bevy_ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};
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
    spawn_wide_menu(commands, parent, MENU_WIDTH)
}

/// [`spawn_menu`] with a popup `width` wide, for one holding more than a
/// menu's width does.
pub fn spawn_wide_menu(commands: &mut Commands, parent: Entity, width: f32) -> Entity {
    let popup = spawn_popup(commands, width);
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
    let popup = spawn_popup(commands, MENU_WIDTH);
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
    let popup = spawn_popup(commands, MENU_WIDTH);
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

/// Where a popup may open, best first. `Popover` takes the first that fits in
/// the window.
///
/// Feathers offers only the two that start at the button's left edge, so a
/// menu behind a button near the window's right — the inspector's export, a
/// frame's header at the edge of the grid — ran off the screen. Ending at the
/// button's right edge keeps it in.
fn placements() -> Vec<PopoverPlacement> {
    [PopoverSide::Bottom, PopoverSide::Top]
        .into_iter()
        .flat_map(|side| {
            [PopoverAlign::Start, PopoverAlign::End].map(|align| PopoverPlacement {
                side,
                align,
                gap: 2.0,
            })
        })
        .collect()
}

/// A closed popup, for the caller to fill.
fn spawn_popup(commands: &mut Commands, width: f32) -> Entity {
    commands
        .spawn_scene(bsn! {
            @FeathersMenuPopup
            Popover { positions: { placements() } }
            // A menu long enough to run off the screen scrolls instead.
            ScrollArea
            Node {
                width: { Val::Px(width) },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_popup_tries_ending_at_its_button_before_flipping_above_it() {
        let order: Vec<_> = placements()
            .into_iter()
            .map(|placement| (placement.side, placement.align))
            .collect();
        assert_eq!(
            order,
            [
                (PopoverSide::Bottom, PopoverAlign::Start),
                (PopoverSide::Bottom, PopoverAlign::End),
                (PopoverSide::Top, PopoverAlign::Start),
                (PopoverSide::Top, PopoverAlign::End),
            ]
        );
    }
}
