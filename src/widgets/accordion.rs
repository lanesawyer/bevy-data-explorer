//! Collapsible sections of the sidebar.
//!
//! Accordions are built as generic containers rather than one-offs, because the
//! sidebar's contents will vary with whatever dataset is selected: a section is
//! a title, an open flag, and whatever children a caller hangs off it.
//!
//! The look comes from Feathers' own containers — a pane for a section of the
//! dock, a group for a section inside one — and only the collapsing is ours,
//! since Feathers has no container that collapses. Anything drawn on a
//! section therefore names a Feathers token and not a color, so the light
//! theme turns it over along with everything else.

use bevy::prelude::*;
use bevy_feathers::containers::{group_body, group_header, pane_body, pane_header};
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersToolButton};
use bevy_feathers::display::label;
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::rounded_corners::RoundedCorners;
use bevy_feathers::theme::{ThemeBorderColor, ThemeTextColor};
use bevy_feathers::tokens;
use bevy_ui_widgets::Activate;

use super::{BlocksFrameInput, CORNER_PX, Icon, button_icon, icon_text, size, truncate_to_width};

pub const ACCORDION_INDENT: f32 = 8.0;
/// Size the accordion titles are drawn at, which sets how many characters fit.
const TITLE_FONT: f32 = size::BODY;

/// How deep in the sidebar a section sits, which decides what it is drawn as.
///
/// Both are Feathers containers, and the difference is only which set of
/// tokens they take: a pane reads as one of the dock's own divisions, a group
/// as something inside one. Nesting a pane in a pane gave two identical
/// headers one indent apart, with nothing to say which owned which.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SectionLevel {
    /// A division of the sidebar itself.
    Pane,
    /// A section within a pane, such as one property of a dataset.
    Group,
}

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

/// The border drawn over a header, rounded to match it as the section opens and
/// closes.
#[derive(Component, Clone, Default)]
pub struct AccordionOutline;

/// The caret drawn in a header, which turns as the section opens.
#[derive(Component, Clone, Default)]
pub struct AccordionCaret;

/// The pieces of a spawned accordion that callers need.
pub struct AccordionParts {
    pub section: Entity,
    /// Fill this with the section's contents.
    pub body: Entity,
    /// Attach a menu button here with [`spawn_menu`](super::spawn_menu).
    pub header: Entity,
}

/// Spawn an accordion, drawn as the Feathers container `level` names.
///
/// The container supplies the look — a headed box with its own background,
/// border and rounded corners — and this supplies the behaviour Feathers has
/// none of: a header that collapses the body under it. What the header holds
/// beyond its title is the caller's business, so the header is returned along
/// with the body.
pub fn spawn_accordion(
    commands: &mut Commands,
    title: &str,
    open: bool,
    level: SectionLevel,
) -> AccordionParts {
    let accordion = commands
        .spawn_scene(bsn! {
            Accordion { open: { open } }
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
            }
        })
        .id();

    // The two containers differ only in their tokens, but a scene is a type
    // rather than a value, so each level spawns its own. What is patched over
    // them is the same:
    //
    // - No border or padding, so the toggle fills the header and its hover
    //   reaches every edge. `update_accordions` makes room at the end once a
    //   control is added there. The border is drawn over the toggle instead,
    //   by an outline spawned after it: a toggle inside a border was placed
    //   in whole physical pixels while the border was not, and at a
    //   fractional scale factor the hover stopped a pixel short of it on the
    //   right and bottom.
    // - A clip, so a long title cannot push those controls out.
    let header = match level {
        SectionLevel::Pane => commands
            .spawn_scene(bsn! {
                pane_header()
                Node {
                    width: { Val::Percent(100.0) },
                    padding: { UiRect::ZERO },
                    column_gap: { Val::Px(4.0) },
                    border: { UiRect::ZERO },
                    border_radius: { header_corners(open).to_border_radius(CORNER_PX) },
                    overflow: { Overflow::clip() },
                }
                InheritableFont { font_size: { TITLE_FONT } }
            })
            .id(),
        SectionLevel::Group => commands
            .spawn_scene(bsn! {
                group_header()
                Node {
                    width: { Val::Percent(100.0) },
                    padding: { UiRect::ZERO },
                    column_gap: { Val::Px(4.0) },
                    border: { UiRect::ZERO },
                    border_radius: { header_corners(open).to_border_radius(CORNER_PX) },
                    overflow: { Overflow::clip() },
                }
                InheritableFont { font_size: { TITLE_FONT } }
            })
            .id(),
    };

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
                // Stretched to the header's height rather than Feathers' row
                // height, so the hover fills it top to bottom.
                height: { Val::Auto },
                align_self: { AlignSelf::Stretch },
                border_radius: { header_corners(open).to_border_radius(CORNER_PX) },
                align_items: { AlignItems::Center },
                justify_content: { JustifyContent::Start },
                column_gap: { Val::Px(6.0) },
                padding: { UiRect::horizontal(Val::Px(6.0)) },
            }
            Children [
                (
                    AccordionCaret
                    icon_text(caret(open))
                    ThemeTextColor({ tokens::TEXT_MAIN })
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
    // All round, where Feathers leaves the bottom open for the body to close:
    // a closed section has no body showing. It stays when the section opens,
    // as a rule under the header, since one that came and went changed the
    // header's height and shifted everything under it on every toggle.
    let border = match level {
        SectionLevel::Pane => tokens::PANE_HEADER_BORDER,
        SectionLevel::Group => tokens::GROUP_HEADER_BORDER,
    };
    let outline = commands
        .spawn_scene(bsn! {
            AccordionOutline
            Node {
                position_type: { PositionType::Absolute },
                left: { Val::Px(0.0) },
                right: { Val::Px(0.0) },
                top: { Val::Px(0.0) },
                bottom: { Val::Px(0.0) },
                border: { UiRect::all(Val::Px(1.0)) },
                border_radius: { header_corners(open).to_border_radius(CORNER_PX) },
            }
            ThemeBorderColor({ border })
            template_value(Pickable::IGNORE)
        })
        .id();
    commands.entity(header).add_children(&[toggle, outline]);

    let body = match level {
        SectionLevel::Pane => {
            commands.spawn_scene(bsn! {
                pane_body()
                {body_patch(accordion, open)}
                // Feathers' pane body has no border of its own, which left an
                // open pane's box without sides or a bottom under its header.
                ThemeBorderColor({ tokens::PANE_HEADER_BORDER })
            })
        }
        SectionLevel::Group => {
            commands.spawn_scene(bsn! { group_body() {body_patch(accordion, open)} })
        }
    }
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
pub fn spawn_header_button(commands: &mut Commands, header: Entity, icon: Icon) -> Entity {
    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_icon(icon)] }
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

/// An accordion's untruncated title, kept because the text it is shown through
/// is rewritten to fit.
#[derive(Component, Clone, Default)]
pub struct AccordionTitle {
    pub full: String,
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

/// What every accordion body patches over its container, whichever it is: the
/// indent the sidebar's contents are read against, and the state to start in.
fn body_patch(accordion: Entity, open: bool) -> impl Scene {
    bsn! {
        AccordionBody { accordion: { accordion } }
        Node {
            // Set here rather than left for `update_accordions` to correct:
            // that runs a frame later, and a section spawned closed would
            // draw its contents once before being hidden.
            display: { body_display(open) },
            width: { Val::Percent(100.0) },
            row_gap: { Val::Px(6.0) },
            border: { UiRect::new(Val::Px(1.0), Val::Px(1.0), Val::Px(0.0), Val::Px(1.0)) },
            padding: { UiRect::new(
                Val::Px(ACCORDION_INDENT),
                Val::Px(2.0),
                Val::Px(6.0),
                Val::Px(6.0),
            ) },
        }
    }
}

/// Which corners a header rounds. A closed section is a box on its own, so it
/// rounds all four; an open one is the top of the box its body finishes.
fn header_corners(open: bool) -> RoundedCorners {
    if open {
        RoundedCorners::Top
    } else {
        RoundedCorners::All
    }
}

/// Space kept at the end of a header, only once a control besides the toggle
/// is in it. With none there the toggle runs to the edge, so its hover is even
/// on both sides.
fn header_padding(controls: usize) -> UiRect {
    if controls > 0 {
        UiRect::right(Val::Px(4.0))
    } else {
        UiRect::ZERO
    }
}

fn caret(open: bool) -> Icon {
    if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    }
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

/// Show or hide bodies, round the headers, and turn the carets, to match each
/// section's state.
pub fn update_accordions(
    accordions: Query<&Accordion>,
    mut bodies: Query<(&AccordionBody, &mut Node)>,
    headers: Query<(Entity, &AccordionHeader, &ChildOf, &Children)>,
    siblings: Query<&Children, Without<AccordionHeader>>,
    outlines: Query<(), With<AccordionOutline>>,
    mut containers: Query<&mut Node, Without<AccordionBody>>,
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

    for (toggle, header, parent, children) in &headers {
        let open = accordions.get(header.accordion).is_ok_and(|a| a.open);
        // The container, the toggle filling it and the outline over both all
        // round the same corners; a closed section has no body under it to
        // finish the box.
        let corners = header_corners(open).to_border_radius(CORNER_PX);
        let beside = siblings.get(parent.parent()).ok();
        let outline = beside.and_then(|c| c.iter().find(|e| outlines.contains(*e)));
        for entity in [Some(parent.parent()), Some(toggle), outline]
            .into_iter()
            .flatten()
        {
            if let Ok(mut node) = containers.get_mut(entity)
                && node.border_radius != corners
            {
                node.border_radius = corners;
            }
        }
        // Everything in the header but the toggle and its outline is a control.
        let controls = beside.map_or(0, |c| c.len().saturating_sub(2));
        if let Ok(mut node) = containers.get_mut(parent.parent()) {
            let wanted = header_padding(controls);
            if node.padding != wanted {
                node.padding = wanted;
            }
        }
        for child in children.iter() {
            if carets.get(child).is_err() {
                continue;
            }
            if let Ok(mut text) = texts.get_mut(child) {
                let wanted = caret(open).glyph();
                if text.0 != wanted {
                    text.0 = wanted.to_string();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caret_shows_whether_a_section_is_open() {
        assert_ne!(caret(true), caret(false));
    }

    #[test]
    fn a_closed_header_rounds_the_corners_its_body_would_have_finished() {
        let open = header_corners(true).to_border_radius(CORNER_PX);
        let closed = header_corners(false).to_border_radius(CORNER_PX);
        // Open, the body under it carries the bottom of the box.
        assert_eq!(open.bottom_left, Val::Px(0.0));
        assert_eq!(open.top_left, Val::Px(CORNER_PX));
        assert_eq!(closed.bottom_left, Val::Px(CORNER_PX));
        assert_eq!(closed.top_left, Val::Px(CORNER_PX));
    }

    #[test]
    fn a_header_with_only_its_toggle_leaves_no_gap_at_the_end() {
        assert_eq!(header_padding(0), UiRect::ZERO);
        assert_eq!(header_padding(1).right, Val::Px(4.0));
    }

    #[test]
    fn a_closed_section_is_not_laid_out() {
        // A section spawned closed must be hidden from the start. Leaving it to
        // the update showed its contents for a frame first, which read as a
        // flash of checkboxes whenever the panel was rebuilt.
        assert_eq!(body_display(false), Display::None);
        assert_eq!(body_display(true), Display::Flex);
    }
}
