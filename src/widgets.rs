//! Reusable sidebar widgets.
//!
//! Accordions are built as generic containers rather than one-offs, because the
//! sidebar's contents will vary with whatever dataset is selected: a section is
//! a title, an open flag, and whatever children a caller hangs off it.

use bevy::prelude::*;
use bevy::ui::Interaction;
use bevy_feathers::controls::FeathersSlider;
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_feathers::tokens;

use crate::panel::BlocksFrameInput;

pub const ACCORDION_INDENT: f32 = 8.0;
const HEADER_HEIGHT: f32 = 26.0;

/// A collapsible section of the sidebar.
#[derive(Component, Clone, Default)]
pub struct Accordion {
    pub open: bool,
}

/// The clickable header of an accordion, pointing back at its section.
#[derive(Component, Clone)]
pub struct AccordionHeader {
    pub accordion: Entity,
}

impl Default for AccordionHeader {
    fn default() -> Self {
        AccordionHeader {
            accordion: Entity::PLACEHOLDER,
        }
    }
}

/// The part of an accordion that is hidden when it is closed.
#[derive(Component, Clone)]
pub struct AccordionBody {
    pub accordion: Entity,
}

impl Default for AccordionBody {
    fn default() -> Self {
        AccordionBody {
            accordion: Entity::PLACEHOLDER,
        }
    }
}

/// The caret drawn in a header, which turns as the section opens.
#[derive(Component, Clone, Default)]
pub struct AccordionCaret;

/// The pieces of a spawned accordion that callers need.
pub struct AccordionParts {
    pub section: Entity,
    /// Fill this with the section's contents.
    pub body: Entity,
    /// Attach a menu button here with [`spawn_accordion_menu`].
    pub header: Entity,
}

/// Spawn an accordion.
///
/// The body is returned rather than populated here so that callers compose
/// their own contents into it; a section knows nothing about what it holds.
pub fn spawn_accordion(commands: &mut Commands, title: &str, open: bool) -> AccordionParts {
    let accordion = commands
        .spawn_scene(bsn! {
            Accordion { open: { open } }
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
            }
        })
        .id();

    let header = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                height: { Val::Px(HEADER_HEIGHT) },
                align_items: { AlignItems::Center },
                border_radius: { BorderRadius::all(Val::Px(3.0)) },
                overflow: { Overflow::clip() },
            }
            ThemeBackgroundColor({ tokens::WINDOW_BG })
            InheritableFont { font_size: { 13.0f32 } }
        })
        .id();

    let toggle = commands
        .spawn_scene(bsn! {
            Button
            BlocksFrameInput
            AccordionHeader { accordion: { accordion } }
            Node {
                flex_grow: { 1.0_f32 },
                height: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
                padding: { UiRect::horizontal(Val::Px(6.0)) },
            }
            Children [
                (
                    AccordionCaret
                    label(caret(open))
                ),
                label(title.to_string()),
            ]
        })
        .id();
    commands.entity(header).add_child(toggle);

    let body = commands
        .spawn_scene(bsn! {
            AccordionBody { accordion: { accordion } }
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                row_gap: { Val::Px(6.0) },
                padding: { UiRect::new(
                    Val::Px(ACCORDION_INDENT),
                    Val::Px(2.0),
                    Val::Px(6.0),
                    Val::Px(6.0),
                ) },
            }
        })
        .id();

    commands.entity(accordion).add_children(&[header, body]);
    AccordionParts {
        section: accordion,
        body,
        header,
    }
}

/// A popup anchored under an accordion's menu button.
#[derive(Component, Clone, Default)]
pub struct AccordionMenu {
    pub open: bool,
}

/// The button that opens a menu, on the right of an accordion header.
#[derive(Component, Clone)]
pub struct AccordionMenuButton {
    pub menu: Entity,
}

impl Default for AccordionMenuButton {
    fn default() -> Self {
        AccordionMenuButton {
            menu: Entity::PLACEHOLDER,
        }
    }
}

/// Anchors a menu to the button that opens it.
#[derive(Component, Clone)]
pub struct AnchoredTo {
    pub button: Entity,
}

impl Default for AnchoredTo {
    fn default() -> Self {
        AnchoredTo {
            button: Entity::PLACEHOLDER,
        }
    }
}

/// Width of a menu popup.
pub const MENU_WIDTH: f32 = 320.0;
/// Menus draw over the frames and everything docked beside them.
const MENU_Z: i32 = 10;

/// Add a menu button to an accordion header, returning the popup's content
/// node for the caller to fill.
///
/// The popup is a root node rather than a child of the header, because the
/// sidebar clips its contents and a menu is meant to overhang it.
pub fn spawn_accordion_menu(commands: &mut Commands, header: Entity) -> Entity {
    let menu = commands
        .spawn_scene(bsn! {
            AccordionMenu
            Node {
                position_type: { PositionType::Absolute },
                display: { Display::None },
                width: { Val::Px(MENU_WIDTH) },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(4.0) },
                padding: { UiRect::all(Val::Px(10.0)) },
                border_radius: { BorderRadius::all(Val::Px(6.0)) },
            }
            ThemeBackgroundColor({ tokens::MENU_BG })
            InheritableFont { font_size: { 13.0f32 } }
            GlobalZIndex({ MENU_Z })
            BlocksFrameInput
        })
        .id();

    let button = commands
        .spawn_scene(bsn! {
            Button
            BlocksFrameInput
            AccordionMenuButton { menu: { menu } }
            Node {
                width: { Val::Px(HEADER_HEIGHT) },
                height: { Val::Percent(100.0) },
                justify_content: { JustifyContent::Center },
                align_items: { AlignItems::Center },
            }
            Children [label("...")]
        })
        .id();

    commands.entity(header).add_child(button);
    commands.entity(menu).insert(AnchoredTo { button });
    menu
}

/// Open and close menus, and dismiss them when something else is clicked.
pub fn toggle_accordion_menus(
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &AccordionMenuButton)>,
    interactions: Query<&Interaction>,
    children: Query<&Children>,
    mut menus: Query<(Entity, &mut AccordionMenu)>,
) {
    let pressed = buttons
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, button)| button.menu);

    if let Some(target) = pressed {
        for (entity, mut menu) in &mut menus {
            menu.open = entity == target && !menu.open;
        }
        return;
    }

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // A press anywhere that is not inside an open menu dismisses it.
    for (entity, mut menu) in &mut menus {
        if !menu.open {
            continue;
        }
        let inside = std::iter::once(entity)
            .chain(descendants(entity, &children))
            .any(|e| interactions.get(e).is_ok_and(|i| *i != Interaction::None));
        if !inside {
            menu.open = false;
        }
    }
}

fn descendants(root: Entity, children: &Query<&Children>) -> Vec<Entity> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if let Ok(kids) = children.get(entity) {
            for child in kids.iter() {
                found.push(child);
                stack.push(child);
            }
        }
    }
    found
}

/// Show open menus, positioned under the button that opens them.
pub fn position_accordion_menus(
    windows: Query<&Window>,
    anchors: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut menus: Query<(&AccordionMenu, &AnchoredTo, &mut Node)>,
) {
    let Ok(window) = windows.single() else { return };

    for (menu, anchor, mut node) in &mut menus {
        node.display = if menu.open {
            Display::Flex
        } else {
            Display::None
        };
        if !menu.open {
            continue;
        }
        let Ok((computed, transform)) = anchors.get(anchor.button) else {
            continue;
        };
        // Layout reports physical pixels; `left` and `top` are logical.
        let scale = computed.inverse_scale_factor();
        let size = computed.size() * scale;
        let centre = Vec2::new(transform.translation.x, transform.translation.y) * scale;
        let left = (centre.x - size.x * 0.5).min(window.width() - MENU_WIDTH - 8.0);
        node.left = Val::Px(left.max(8.0));
        node.top = Val::Px(centre.y + size.y * 0.5 + 4.0);
    }
}

fn caret(open: bool) -> &'static str {
    if open { "v" } else { ">" }
}

/// Toggle a section when its header is clicked.
pub fn toggle_accordions(
    headers: Query<(&Interaction, &AccordionHeader), Changed<Interaction>>,
    mut accordions: Query<&mut Accordion>,
) {
    for (interaction, header) in &headers {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Ok(mut accordion) = accordions.get_mut(header.accordion) {
            accordion.open = !accordion.open;
        }
    }
}

/// Show or hide bodies, and turn the carets, to match each section's state.
pub fn update_accordions(
    accordions: Query<&Accordion>,
    mut bodies: Query<(&AccordionBody, &mut Node)>,
    headers: Query<(&AccordionHeader, &Children)>,
    carets: Query<Entity, With<AccordionCaret>>,
    mut texts: Query<&mut Text>,
) {
    for (body, mut node) in &mut bodies {
        let open = accordions.get(body.accordion).is_ok_and(|a| a.open);
        let wanted = if open { Display::Flex } else { Display::None };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    for (header, children) in &headers {
        let open = accordions.get(header.accordion).is_ok_and(|a| a.open);
        for child in children.iter() {
            if carets.get(child).is_err() {
                continue;
            }
            if let Ok(mut text) = texts.get_mut(child) {
                let wanted = caret(open);
                if text.0 != wanted {
                    text.0 = wanted.to_string();
                }
            }
        }
    }
}

/// Spawn a labelled slider over `range`.
///
/// Feathers styles and drives it; the app only writes the value back, via
/// `bevy_ui_widgets::slider_self_update`.
pub fn spawn_slider(commands: &mut Commands, value: f32, range: (f32, f32)) -> Entity {
    commands
        .spawn_scene(bsn! {
            BlocksFrameInput
            @FeathersSlider {
                @value: { value },
                @min: { range.0 },
                @max: { range.1 }
            }
        })
        .id()
}

/// A dim caption, for the secondary lines of a listing.
pub fn caption(commands: &mut Commands, text: impl Into<String>) -> Entity {
    let text = text.into();
    commands.spawn_scene(bsn! { label_dim(text) }).id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caret_shows_whether_a_section_is_open() {
        assert_ne!(caret(true), caret(false));
    }

    #[test]
    fn hit_tests_scale_layout_pixels_down_to_cursor_pixels() {
        // The layout reports physical pixels while the cursor is logical, so a
        // menu on a scaled display would be hit-tested at the wrong place.
        let rect = scaled_rect(Vec2::new(320.0, 200.0), Vec2::new(400.0, 300.0), 0.5);
        assert_eq!(rect.min, Vec2::new(120.0, 100.0));
        assert_eq!(rect.max, Vec2::new(280.0, 200.0));
        assert!(rect.contains(Vec2::new(200.0, 150.0)));
        assert!(!rect.contains(Vec2::new(400.0, 300.0)));
    }

    #[test]
    fn an_unscaled_display_is_left_alone() {
        let rect = scaled_rect(Vec2::new(100.0, 50.0), Vec2::new(200.0, 100.0), 1.0);
        assert_eq!(rect.min, Vec2::new(150.0, 75.0));
        assert_eq!(rect.max, Vec2::new(250.0, 125.0));
    }

    #[test]
    fn menus_draw_above_the_frames_and_the_dock() {
        // A menu overhangs the sidebar it opens from, so it has to outrank
        // both the sidebar's chrome and the frame outline beneath it.
        assert!(MENU_Z > 1);
    }
}
