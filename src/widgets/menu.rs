//! Popups anchored under the button that opens them.

use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_feathers::tokens;
use bevy_ui_widgets::{Activate, ScrollArea};

use super::space;
use super::{BlocksFrameInput, Icon, button_icon, display, patch_node};

/// A popup anchored under the button that opens it.
///
/// Not specific to accordions: a frame's own header uses the same machinery.
#[derive(Component, Clone, Default)]
pub struct Menu {
    pub open: bool,
}

/// The button that opens a menu.
#[derive(Component, Clone)]
pub struct MenuButton {
    pub menu: Entity,
}

impl Default for MenuButton {
    fn default() -> Self {
        MenuButton {
            menu: Entity::PLACEHOLDER,
        }
    }
}

/// Anchors a menu to the button that opens it.
#[derive(Component, Clone)]
pub struct MenuAnchor {
    pub button: Entity,
}

impl Default for MenuAnchor {
    fn default() -> Self {
        MenuAnchor {
            button: Entity::PLACEHOLDER,
        }
    }
}

/// Width of a menu popup.
pub const MENU_WIDTH: f32 = 320.0;
/// Menus draw over the frames and everything docked beside them.
const MENU_Z: i32 = 10;
/// Gap left between a menu and the bottom of the window.
const MENU_MARGIN: f32 = space::PANEL_INSET;
/// A menu never shrinks below this, even when opened near the bottom edge.
const MENU_MIN_HEIGHT: f32 = 120.0;

/// Add a menu button under `parent`, returning the popup for the caller to
/// fill.
///
/// The popup is a root node rather than a child of the button, because both
/// the sidebar and a frame's header clip their contents and a menu is meant to
/// overhang them.
pub fn spawn_menu(commands: &mut Commands, parent: Entity) -> Entity {
    let menu = spawn_popup(commands);
    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_icon(Icon::Ellipsis)] }
            }
            BlocksFrameInput
            MenuButton { menu: { menu } }
        })
        .id();
    anchor(commands, parent, button, menu);
    menu
}

/// [`spawn_menu`] behind a button showing `icon` and a small chevron, returning
/// the button and the popup.
///
/// Only the ellipsis says "menu" by itself. Any other icon reads as a button
/// that acts when pressed, so the chevron is what warns that this one opens
/// something instead — and it is not optional, so no menu goes without it.
pub fn spawn_icon_menu(commands: &mut Commands, parent: Entity, icon: Icon) -> (Entity, Entity) {
    let menu = spawn_popup(commands);
    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![
                    button_icon(icon),
                    (
                        button_icon(Icon::ChevronDown)
                        TextFont { font_size: { FontSize::Px(10.0) } }
                    ),
                ] }
            }
            BlocksFrameInput
            MenuButton { menu: { menu } }
            Node { column_gap: { Val::Px(space::SEAM) } }
        })
        .id();
    anchor(commands, parent, button, menu);
    (button, menu)
}

fn anchor(commands: &mut Commands, parent: Entity, button: Entity, menu: Entity) {
    commands.entity(parent).add_child(button);
    commands.entity(menu).insert(MenuAnchor { button });
}

/// A closed popup of its own, for a caller that anchors it with a
/// [`MenuAnchor`] and opens it itself — one popup that moves between several
/// buttons, rather than a button apiece. That popup opens from something that
/// already looks like it opens one, such as a color swatch; an icon button
/// opening a menu goes through [`spawn_icon_menu`] for its chevron.
pub fn spawn_popup(commands: &mut Commands) -> Entity {
    commands
        .spawn_scene(bsn! {
            Menu
            // A menu long enough to run off the screen scrolls instead.
            ScrollArea
            Node {
                position_type: { PositionType::Absolute },
                display: { Display::None },
                width: { Val::Px(MENU_WIDTH) },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(space::ROWS) },
                padding: { UiRect::all(Val::Px(space::PANEL_INSET)) },
                border_radius: { BorderRadius::all(Val::Px(6.0)) },
                overflow: { Overflow::scroll_y() },
            }
            ThemeBackgroundColor({ tokens::MENU_BG })
            InheritableFont { font_size: { 13.0f32 } }
            GlobalZIndex({ MENU_Z })
            BlocksFrameInput
        })
        .id()
}

/// Open and close menus, and dismiss them when something else is clicked.
/// Open the menu whose button was pressed, and close any other.
pub fn on_menu_button(
    activate: On<Activate>,
    buttons: Query<&MenuButton>,
    mut menus: Query<(Entity, &mut Menu)>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    for (entity, mut menu) in &mut menus {
        menu.open = entity == button.menu && !menu.open;
    }
}

/// Dismiss an open menu when something outside it is pressed.
pub fn dismiss_menus(
    mouse: Res<ButtonInput<MouseButton>>,
    hover: Res<HoverMap>,
    parents: Query<&ChildOf>,
    mut menus: Query<(Entity, &MenuAnchor, &mut Menu)>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    for (entity, anchor, mut menu) in &mut menus {
        if !menu.open {
            continue;
        }
        // Whatever the pointer is over decides this, read from the picking
        // hover state rather than from `Interaction`: the Feathers controls in
        // the menu report through `Activate` events and carry no `Interaction`
        // for a hit test to find. Dismissing here on mouse-down also removed
        // the buttons before the release that would have activated them, which
        // is why none of them appeared to work.
        //
        // The button that opened the menu counts as inside, or pressing it
        // would dismiss here and immediately reopen on release.
        let keeps_open = |hovered: Entity| {
            hovered == entity
                || hovered == anchor.button
                || parents
                    .iter_ancestors(hovered)
                    .any(|ancestor| ancestor == entity || ancestor == anchor.button)
        };
        let inside = hover
            .values()
            .flat_map(|hits| hits.keys())
            .any(|hovered| keeps_open(*hovered));

        if !inside {
            menu.open = false;
        }
    }
}

/// Show open menus, positioned under the button that opens them.
pub fn position_menus(
    windows: Query<&Window>,
    anchors: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut menus: Query<(&Menu, &MenuAnchor, &mut Node)>,
) {
    let Ok(window) = windows.single() else { return };

    for (menu, anchor, node) in &mut menus {
        let placed = anchors.get(anchor.button).ok().filter(|_| menu.open);
        let Some((computed, transform)) = placed else {
            patch_node(node, |node| node.display = display(menu.open));
            continue;
        };
        // Layout reports physical pixels; `left` and `top` are logical.
        let scale = computed.inverse_scale_factor();
        let size = computed.size() * scale;
        let centre = Vec2::new(transform.translation.x, transform.translation.y) * scale;
        let left = (centre.x - size.x * 0.5).min(window.width() - MENU_WIDTH - 8.0);
        let top = centre.y + size.y * 0.5 + 4.0;
        patch_node(node, |node| {
            node.display = Display::Flex;
            node.left = Val::Px(left.max(8.0));
            node.top = Val::Px(top);
            // Stop at the bottom of the window rather than running past it;
            // the contents scroll once they no longer fit.
            node.max_height = Val::Px((window.height() - top - MENU_MARGIN).max(MENU_MIN_HEIGHT));
        });
    }
}

/// Let go of the keyboard when the menu holding it closes.
///
/// A closed menu is only hidden, so a text field inside one keeps focus and
/// goes on swallowing every keystroke — with the field itself no longer on
/// screen to show where they are going.
pub fn release_focus_from_closed_menus(
    mut focus: ResMut<InputFocus>,
    menus: Query<(Entity, &Menu)>,
    parents: Query<&ChildOf>,
) {
    let Some(focused) = focus.get() else { return };
    let inside_a_closed_menu = menus.iter().any(|(entity, menu)| {
        !menu.open
            && (focused == entity
                || parents
                    .iter_ancestors(focused)
                    .any(|ancestor| ancestor == entity))
    });
    if inside_a_closed_menu {
        focus.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A menu is placed from its button downward, so how much room is left
    /// depends on where that button sits.
    fn menu_height(window_height: f32, top: f32) -> f32 {
        (window_height - top - MENU_MARGIN).max(MENU_MIN_HEIGHT)
    }

    #[test]
    fn a_menu_stops_short_of_the_bottom_of_the_window() {
        assert_eq!(menu_height(1000.0, 200.0), 1000.0 - 200.0 - MENU_MARGIN);
    }

    #[test]
    fn a_menu_opened_near_the_bottom_still_has_usable_height() {
        // Opened low, the arithmetic would otherwise give a height of nothing
        // at all, or a negative one.
        assert_eq!(menu_height(1000.0, 995.0), MENU_MIN_HEIGHT);
        assert!(menu_height(400.0, 600.0) > 0.0);
    }

    #[test]
    fn menus_draw_above_the_frames_and_the_dock() {
        // A menu overhangs the sidebar it opens from, so it has to outrank
        // both the sidebar's chrome and the frame outline beneath it.
        const { assert!(MENU_Z > 1) };
    }
}
