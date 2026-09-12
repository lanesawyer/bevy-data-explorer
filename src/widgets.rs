//! Reusable sidebar widgets.
//!
//! Accordions are built as generic containers rather than one-offs, because the
//! sidebar's contents will vary with whatever dataset is selected: a section is
//! a title, an open flag, and whatever children a caller hangs off it.

use bevy::prelude::*;
use bevy::ui::Interaction;
use bevy_ui_widgets::{Slider, SliderRange, SliderThumb, SliderValue};

use crate::panel::BlocksFrameInput;

pub const ACCORDION_INDENT: f32 = 8.0;
const HEADER_HEIGHT: f32 = 26.0;
const SLIDER_HEIGHT: f32 = 16.0;
const THUMB_PX: f32 = 12.0;

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

/// Spawn an accordion, returning the section and the body to fill.
///
/// The body is returned rather than populated here so that callers compose
/// their own contents into it; a section knows nothing about what it holds.
pub fn spawn_accordion(commands: &mut Commands, title: &str, open: bool) -> (Entity, Entity) {
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
            Button
            BlocksFrameInput
            AccordionHeader { accordion: { accordion } }
            Node {
                width: { Val::Percent(100.0) },
                height: { Val::Px(HEADER_HEIGHT) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
                padding: { UiRect::horizontal(Val::Px(6.0)) },
                border_radius: { BorderRadius::all(Val::Px(3.0)) },
            }
            BackgroundColor({ Color::srgba(0.16, 0.18, 0.23, 0.9) })
            Children [
                (
                    AccordionCaret
                    Text({ caret(open).to_string() })
                    TextFont { font_size: { bevy::text::FontSize::Px(11.0) } }
                    TextColor({ Color::srgb(0.65, 0.70, 0.78) })
                ),
                (
                    Text({ title.to_string() })
                    TextFont { font_size: { bevy::text::FontSize::Px(13.0) } }
                    TextColor({ Color::srgb(0.88, 0.91, 0.96) })
                ),
            ]
        })
        .id();

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
    (accordion, body)
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

/// The filled part of a slider track, sized from the slider's value.
#[derive(Component, Clone)]
pub struct SliderFill {
    pub slider: Entity,
}

impl Default for SliderFill {
    fn default() -> Self {
        SliderFill {
            slider: Entity::PLACEHOLDER,
        }
    }
}

/// Marks the thumb's owning slider, so it can be positioned from the value.
#[derive(Component, Clone)]
pub struct SliderHandle {
    pub slider: Entity,
}

impl Default for SliderHandle {
    fn default() -> Self {
        SliderHandle {
            slider: Entity::PLACEHOLDER,
        }
    }
}

/// Spawn a labelled horizontal slider over `range`, returning the slider.
///
/// Behaviour comes from `bevy_ui_widgets`' headless slider; everything here is
/// presentation and the thumb placement it leaves to the app.
pub fn spawn_slider(commands: &mut Commands, value: f32, range: (f32, f32)) -> Entity {
    let slider = commands
        .spawn_scene(bsn! {
            BlocksFrameInput
            template_value(Slider::default())
            template_value(SliderValue(value))
            template_value(SliderRange::new(range.0, range.1))
            Node {
                width: { Val::Percent(100.0) },
                height: { Val::Px(SLIDER_HEIGHT) },
                justify_content: { JustifyContent::Center },
                flex_direction: { FlexDirection::Column },
            }
        })
        .id();

    let track = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                height: { Val::Px(4.0) },
                border_radius: { BorderRadius::all(Val::Px(2.0)) },
            }
            BackgroundColor({ Color::srgb(0.22, 0.24, 0.30) })
            Children [(
                SliderFill { slider: { slider } }
                Node {
                    height: { Val::Percent(100.0) },
                    border_radius: { BorderRadius::all(Val::Px(2.0)) },
                }
                BackgroundColor({ Color::srgb(0.38, 0.60, 0.90) })
            )]
        })
        .id();

    let thumb = commands
        .spawn_scene(bsn! {
            SliderThumb
            SliderHandle { slider: { slider } }
            Node {
                position_type: { PositionType::Absolute },
                width: { Val::Px(THUMB_PX) },
                height: { Val::Px(THUMB_PX) },
                border_radius: { BorderRadius::all(Val::Px(THUMB_PX * 0.5)) },
            }
            BackgroundColor({ Color::srgb(0.85, 0.89, 0.95) })
        })
        .id();

    commands.entity(slider).add_children(&[track, thumb]);
    slider
}

/// Move the fill and thumb to match each slider's value.
pub fn update_sliders(
    sliders: Query<(&SliderValue, &SliderRange, &ComputedNode)>,
    mut fills: Query<(&SliderFill, &mut Node), Without<SliderHandle>>,
    mut handles: Query<(&SliderHandle, &mut Node), Without<SliderFill>>,
) {
    let fraction = |entity: Entity| {
        sliders.get(entity).ok().map(|(value, range, node)| {
            let span = range.end() - range.start();
            let t = if span.abs() < f32::EPSILON {
                0.0
            } else {
                ((value.0 - range.start()) / span).clamp(0.0, 1.0)
            };
            (t, node.size().x)
        })
    };

    for (fill, mut node) in &mut fills {
        if let Some((t, _)) = fraction(fill.slider) {
            node.width = Val::Percent(t * 100.0);
        }
    }
    for (handle, mut node) in &mut handles {
        if let Some((t, width)) = fraction(handle.slider) {
            // Inset by the thumb so it stays inside the track at both ends.
            node.left = Val::Px(t * (width - THUMB_PX).max(0.0));
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

    /// The thumb is positioned by hand, so the mapping from value to offset is
    /// worth pinning: both ends must stay inside the track.
    fn thumb_offset(value: f32, range: (f32, f32), width: f32) -> f32 {
        let span = range.1 - range.0;
        let t = ((value - range.0) / span).clamp(0.0, 1.0);
        t * (width - THUMB_PX).max(0.0)
    }

    #[test]
    fn the_thumb_stays_inside_the_track() {
        assert_eq!(thumb_offset(0.0, (0.0, 1.0), 100.0), 0.0);
        assert_eq!(thumb_offset(1.0, (0.0, 1.0), 100.0), 100.0 - THUMB_PX);
        assert_eq!(
            thumb_offset(0.5, (0.0, 1.0), 100.0),
            (100.0 - THUMB_PX) * 0.5
        );
    }

    #[test]
    fn values_outside_the_range_clamp() {
        assert_eq!(thumb_offset(-5.0, (0.0, 1.0), 100.0), 0.0);
        assert_eq!(thumb_offset(9.0, (0.0, 1.0), 100.0), 100.0 - THUMB_PX);
    }

    #[test]
    fn a_track_narrower_than_the_thumb_does_not_go_negative() {
        assert_eq!(thumb_offset(1.0, (0.0, 1.0), 4.0), 0.0);
    }
}
