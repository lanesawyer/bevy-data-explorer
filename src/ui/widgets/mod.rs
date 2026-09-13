//! Reusable sidebar widgets.
//!
//! Accordions are built as generic containers rather than one-offs, because the
//! sidebar's contents will vary with whatever dataset is selected: a section is
//! a title, an open flag, and whatever children a caller hangs off it.

use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersSlider, FeathersToolButton};
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_feathers::tokens;
use bevy_ui_widgets::Activate;
use bevy_ui_widgets::ScrollArea;
use bevy_ui_widgets::SliderPrecision;

use crate::view::BlocksFrameInput;

pub const ACCORDION_INDENT: f32 = 8.0;
/// Size the accordion titles are drawn at, which sets how many characters fit.
const TITLE_FONT: f32 = 13.0;
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
    /// Attach a menu button here with [`spawn_menu`].
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
                // Keeps the controls at the end of a header apart.
                column_gap: { Val::Px(4.0) },
                padding: { UiRect::right(Val::Px(4.0)) },
                border_radius: { BorderRadius::all(Val::Px(3.0)) },
                overflow: { Overflow::clip() },
            }
            ThemeBackgroundColor({ tokens::WINDOW_BG })
            InheritableFont { font_size: { 13.0f32 } }
        })
        .id();

    let toggle = commands
        .spawn_scene(bsn! {
            // The plain variant, so a full-width section header picks up hover
            // and press feedback without taking on button chrome.
            @FeathersButton {
                @variant: { ButtonVariant::Plain }
            }
            BlocksFrameInput
            AccordionHeader { accordion: { accordion } }
            Node {
                flex_grow: { 1.0_f32 },
                // A long property name must not push the header's buttons out
                // of a header that clips.
                flex_shrink: { 1.0_f32 },
                min_width: { Val::Px(0.0) },
                overflow: { Overflow::clip() },
                height: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                justify_content: { JustifyContent::Start },
                column_gap: { Val::Px(6.0) },
                padding: { UiRect::horizontal(Val::Px(6.0)) },
            }
            Children [
                (
                    AccordionCaret
                    label(caret(open))
                ),
                (
                    AccordionTitle { full: { title.to_string() } }
                    Node {
                        flex_grow: { 1.0_f32 },
                        flex_shrink: { 1.0_f32 },
                        min_width: { Val::Px(0.0) },
                    }
                    label(title.to_string())
                    // Long names are cut to fit rather than wrapped onto a
                    // second line, which would break the header's height.
                    TextLayout { linebreak: { LineBreak::NoWrap } }
                ),
            ]
        })
        .id();
    commands.entity(header).add_child(toggle);

    let body = commands
        .spawn_scene(bsn! {
            AccordionBody { accordion: { accordion } }
            Node {
                // Set here rather than left for `update_accordions` to correct:
                // that runs a frame later, and a section spawned closed would
                // draw its contents once before being hidden.
                display: { body_display(open) },
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

/// Add a button to the right of an accordion's header, returning it so the
/// caller can attach its own marker and act on it.
pub fn spawn_header_button(commands: &mut Commands, header: Entity, caption: &str) -> Entity {
    let caption = caption.to_string();
    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![label(caption)] }
            }
            BlocksFrameInput
            // Header buttons hold their size; the title beside them gives way
            // instead. Without this the title's growth squeezed them out of a
            // header that clips, and they simply were not there to click.
            Node { flex_shrink: { 0.0_f32 } }
        })
        .id();
    commands.entity(header).add_child(button);
    button
}

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
const MENU_MARGIN: f32 = 12.0;
/// A menu never shrinks below this, even when opened near the bottom edge.
const MENU_MIN_HEIGHT: f32 = 120.0;

/// Add a menu button under `parent`, returning the popup for the caller to
/// fill.
///
/// The popup is a root node rather than a child of the button, because both
/// the sidebar and a frame's header clip their contents and a menu is meant to
/// overhang them.
pub fn spawn_menu(commands: &mut Commands, parent: Entity) -> Entity {
    let menu = commands
        .spawn_scene(bsn! {
            Menu
            // A menu long enough to run off the screen scrolls instead.
            ScrollArea
            Node {
                position_type: { PositionType::Absolute },
                display: { Display::None },
                width: { Val::Px(MENU_WIDTH) },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(4.0) },
                padding: { UiRect::all(Val::Px(10.0)) },
                border_radius: { BorderRadius::all(Val::Px(6.0)) },
                overflow: { Overflow::scroll_y() },
            }
            ThemeBackgroundColor({ tokens::MENU_BG })
            InheritableFont { font_size: { 13.0f32 } }
            GlobalZIndex({ MENU_Z })
            BlocksFrameInput
        })
        .id();

    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![label("...")] }
            }
            BlocksFrameInput
            MenuButton { menu: { menu } }
        })
        .id();

    commands.entity(parent).add_child(button);
    commands.entity(menu).insert(MenuAnchor { button });
    menu
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
        let top = centre.y + size.y * 0.5 + 4.0;
        node.left = Val::Px(left.max(8.0));
        node.top = Val::Px(top);
        // Stop at the bottom of the window rather than running past it; the
        // contents scroll once they no longer fit.
        node.max_height = Val::Px((window.height() - top - MENU_MARGIN).max(MENU_MIN_HEIGHT));
    }
}

/// An accordion's untruncated title, kept because the text it is shown through
/// is rewritten to fit.
#[derive(Component, Clone, Default)]
pub struct AccordionTitle {
    pub full: String,
}

/// Rough width of a glyph as a fraction of the font size.
///
/// Bevy has no text truncation, and measuring would mean laying the string out
/// and reacting to the result a frame later, which oscillates as the measured
/// average shifts with the characters left. A fixed estimate is stable, and
/// erring narrow truncates a little early rather than overflowing.
const GLYPH_WIDTH: f32 = 0.58;

/// Fit `text` into `width`, ending with an ellipsis if it has to be cut.
fn truncate_to_width(text: &str, width: f32, font_size: f32) -> String {
    let glyph = (font_size * GLYPH_WIDTH).max(1.0);
    let fits = (width / glyph).floor().max(0.0) as usize;
    if text.chars().count() <= fits {
        return text.to_string();
    }
    // One character is given back to the ellipsis itself.
    let keep = fits.saturating_sub(1);
    if keep == 0 {
        return String::new();
    }
    text.chars().take(keep).collect::<String>() + "\u{2026}"
}

/// Cut each accordion title to whatever room its header leaves it.
pub fn truncate_accordion_titles(
    titles: Query<(Entity, &AccordionTitle, &ComputedNode)>,
    mut texts: Query<&mut Text>,
) {
    for (entity, title, node) in &titles {
        let width = node.size().x * node.inverse_scale_factor();
        if width <= 0.0 {
            continue;
        }
        let wanted = truncate_to_width(&title.full, width, TITLE_FONT);
        if let Ok(mut text) = texts.get_mut(entity)
            && text.0 != wanted
        {
            text.0 = wanted;
        }
    }
}

fn caret(open: bool) -> &'static str {
    if open { "v" } else { ">" }
}

/// Whether a section's body is laid out. Shared by the spawn and the update so
/// the two cannot disagree about what a closed section looks like.
fn body_display(open: bool) -> Display {
    if open { Display::Flex } else { Display::None }
}

/// Toggle a section when its header is clicked.
pub fn toggle_accordions(
    activate: On<Activate>,
    headers: Query<&AccordionHeader>,
    mut accordions: Query<&mut Accordion>,
) {
    let Ok(header) = headers.get(activate.entity) else {
        return;
    };
    if let Ok(mut accordion) = accordions.get_mut(header.accordion) {
        accordion.open = !accordion.open;
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
        let wanted = body_display(open);
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

/// Spawn a slider over `range`, showing its value to `decimals` places.
///
/// Feathers styles and drives it; the app only writes the value back, via
/// `bevy_ui_widgets::slider_self_update`.
///
/// `SliderPrecision` is added explicitly because Feathers' own scene omits it
/// while the system that moves the fill and rewrites the value text requires
/// it. Without it the slider drags but never redraws.
pub fn spawn_slider(
    commands: &mut Commands,
    value: f32,
    range: (f32, f32),
    decimals: i32,
) -> Entity {
    commands
        .spawn_scene(bsn! {
            BlocksFrameInput
            @FeathersSlider {
                @value: { value },
                @min: { range.0 },
                @max: { range.1 }
            }
            SliderPrecision({ decimals })
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
    fn a_title_that_fits_is_left_alone() {
        assert_eq!(truncate_to_width("Class", 200.0, 13.0), "Class");
    }

    #[test]
    fn a_long_title_is_cut_and_marked() {
        let cut = truncate_to_width("Subclass Bootstrapping Probability", 80.0, 13.0);
        assert!(cut.ends_with('\u{2026}'));
        assert!(cut.chars().count() < "Subclass Bootstrapping Probability".chars().count());
        assert!(cut.starts_with("Subcl"));
    }

    #[test]
    fn a_narrower_header_cuts_more() {
        let wide = truncate_to_width("Neurotransmitter Type", 120.0, 13.0);
        let narrow = truncate_to_width("Neurotransmitter Type", 60.0, 13.0);
        assert!(narrow.chars().count() < wide.chars().count());
    }

    #[test]
    fn no_room_at_all_yields_nothing_rather_than_a_bare_ellipsis() {
        assert_eq!(truncate_to_width("Class", 0.0, 13.0), "");
        assert_eq!(truncate_to_width("Class", 4.0, 13.0), "");
    }

    #[test]
    fn truncation_never_splits_a_character() {
        // Cutting by bytes would panic on a multi-byte name.
        let cut = truncate_to_width("Größe über alles", 40.0, 13.0);
        assert!(cut.is_char_boundary(cut.len()));
    }

    #[test]
    fn the_caret_shows_whether_a_section_is_open() {
        assert_ne!(caret(true), caret(false));
    }

    #[test]
    fn a_closed_section_is_not_laid_out() {
        // A section spawned closed must be hidden from the start. Leaving it to
        // the update showed its contents for a frame first, which read as a
        // flash of checkboxes whenever the panel was rebuilt.
        assert_eq!(body_display(false), Display::None);
        assert_eq!(body_display(true), Display::Flex);
    }

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
        assert!(MENU_Z > 1);
    }
}
