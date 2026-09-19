//! A two-ended control for a numeric property, drawn over a histogram of its
//! distribution.
//!
//! Not a generic slider: it reads and writes a [`NumericRange`] on the source's
//! [`CellProperties`] directly, and the histogram underneath is what makes a
//! span meaningful to choose. Keeping it beside its only caller is what stops
//! the generic widgets in `ui::widgets` having to know about properties at all.
//!
//! Buckets outside the chosen span are dimmed rather than hidden, so the shape
//! of the whole distribution stays visible while a slice of it is picked.

use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy_feathers::display::label_dim;
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::ThemeBackgroundColor;

use crate::app::theme::{Palette, token};
use crate::source::properties::{CellProperties, NumericRange, RangeEnd};
use crate::view::{BlocksFrameInput, SelectedPanel, ShowsSource};

/// Height of the histogram drawn above a numeric range.
const HISTOGRAM_PX: f32 = 44.0;
const RANGE_TRACK_PX: f32 = 6.0;
const RANGE_THUMB_PX: f32 = 12.0;

/// One end of a numeric range's control.
#[derive(Component, Clone)]
pub struct RangeHandle {
    pub property: usize,
    pub end: RangeEnd,
}

impl Default for RangeHandle {
    fn default() -> Self {
        RangeHandle {
            property: 0,
            end: RangeEnd::From,
        }
    }
}

/// The track a numeric range is dragged along, which is what a drag is
/// measured against.
#[derive(Component, Clone, Default)]
pub struct RangeTrack {
    pub property: usize,
}

/// The readout under a numeric range.
#[derive(Component, Clone, Default)]
pub struct RangeReadout {
    pub property: usize,
}

/// The filled span of a numeric range's rail.
#[derive(Component, Clone, Default)]
pub struct RangeFill {
    pub property: usize,
}

/// One bar of a numeric range's histogram.
#[derive(Component, Clone, Default)]
pub struct RangeBar {
    pub property: usize,
    pub bucket: usize,
}

/// Buckets inside the chosen span are drawn lit, the rest dimmed, so the whole
/// distribution stays visible while part of it is picked.
fn bar_color(inside: bool, palette: &Palette) -> Color {
    if inside {
        palette.fill
    } else {
        palette.bar_dim
    }
}

/// Where a handle sits on its rail: a percentage along, and a pixel nudge back
/// so both ends stay on the rail without depending on its measured width.
fn handle_placement(fraction: f32) -> (f32, f32) {
    (fraction * 100.0, -RANGE_THUMB_PX * fraction)
}

/// A histogram of the data's distribution, with a two-ended control under it.
///
/// The bars are drawn behind the control rather than beside it so the span
/// being chosen reads against the shape of the data.
pub(super) fn spawn_range_control(
    commands: &mut Commands,
    property: usize,
    range: &NumericRange,
    palette: &Palette,
) -> Entity {
    let peak = range.histogram.iter().copied().max().unwrap_or(1).max(1);
    let bars: Vec<Entity> = range
        .histogram
        .iter()
        .enumerate()
        .map(|(bucket, count)| {
            // Buckets outside the chosen span are dimmed rather than hidden, so
            // the whole distribution stays visible while a part of it is picked.
            let centre = (bucket as f32 + 0.5) / range.histogram.len() as f32;
            let inside = range.admits(range.value_at(centre));
            let height = (*count as f32 / peak as f32).max(0.02) * HISTOGRAM_PX;
            commands
                .spawn_scene(bsn! {
                    RangeBar { property: { property }, bucket: { bucket } }
                    Node {
                        flex_grow: { 1.0_f32 },
                        height: { Val::Px(height) },
                        margin: { UiRect::horizontal(Val::Px(0.5)) },
                    }
                    BackgroundColor({ bar_color(inside, palette) })
                })
                .id()
        })
        .collect();

    let histogram = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                height: { Val::Px(HISTOGRAM_PX) },
                align_items: { AlignItems::End },
            }
        })
        .id();
    commands.entity(histogram).add_children(&bars);

    let track = commands
        .spawn_scene(bsn! {
            RangeTrack { property: { property } }
            BlocksFrameInput
            Node {
                width: { Val::Percent(100.0) },
                height: { Val::Px(RANGE_THUMB_PX) },
                margin: { UiRect::top(Val::Px(4.0)) },
                justify_content: { JustifyContent::Center },
                flex_direction: { FlexDirection::Column },
            }
        })
        .id();

    let from = range.fraction_of(range.from);
    let to = range.fraction_of(range.to);
    let rail = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                height: { Val::Px(RANGE_TRACK_PX) },
                border_radius: { BorderRadius::all(Val::Px(RANGE_TRACK_PX * 0.5)) },
            }
            ThemeBackgroundColor({ token::TRACK })
            Children [(
                RangeFill { property: { property } }
                Node {
                    position_type: { PositionType::Absolute },
                    left: { Val::Percent(from * 100.0) },
                    width: { Val::Percent((to - from) * 100.0) },
                    height: { Val::Percent(100.0) },
                }
                ThemeBackgroundColor({ token::SELECTION })
            )]
        })
        .id();
    commands.entity(track).add_child(rail);

    for (end, fraction) in [(RangeEnd::From, from), (RangeEnd::To, to)] {
        let handle = commands
            .spawn_scene(bsn! {
                // A thumb, not a button. It is grabbed from the picking
                // hover state, so it only needs to be pickable.
                BlocksFrameInput
                RangeHandle { property: { property }, end: { end } }
                Node {
                    position_type: { PositionType::Absolute },
                    width: { Val::Px(RANGE_THUMB_PX) },
                    height: { Val::Px(RANGE_THUMB_PX) },
                    // Placed by percentage with a nudge back, so both ends stay
                    // on the rail without depending on its measured width.
                    left: { Val::Percent(handle_placement(fraction).0) },
                    margin: { UiRect::left(Val::Px(handle_placement(fraction).1)) },
                    border_radius: { BorderRadius::all(Val::Px(RANGE_THUMB_PX * 0.5)) },
                }
                ThemeBackgroundColor({ token::THUMB })
            })
            .id();
        commands.entity(track).add_child(handle);
    }

    let readout = commands
        .spawn_scene(bsn! {
            // Deliberately not marked for rebuilding: it hangs inside a
            // sub-section that is, and despawning is recursive.
            RangeReadout { property: { property } }
            label_dim(format!("{:.2} - {:.2}", range.from, range.to))
            InheritableFont { font_size: { 11.0f32 } }
        })
        .id();

    let column = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                row_gap: { Val::Px(2.0) },
            }
        })
        .id();
    commands
        .entity(column)
        .add_children(&[histogram, track, readout]);
    column
}

/// Drag either end of a numeric range.
///
/// The value comes from where the pointer sits along the track rather than
/// from accumulated deltas, so a fast drag cannot fall behind the cursor.
pub fn drag_range_handles(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    hover: Res<HoverMap>,
    handles: Query<&RangeHandle>,
    tracks: Query<(&RangeTrack, &ComputedNode, &UiGlobalTransform)>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
    mut dragging: Local<Option<(usize, RangeEnd)>>,
) {
    if !mouse.pressed(MouseButton::Left) {
        *dragging = None;
        return;
    }

    if dragging.is_none() {
        // Grabbed on the way down and held until release, so the pointer may
        // leave the handle without dropping the drag. Remembered by which end
        // of which property it is rather than by entity: the panel may rebuild
        // mid-drag, and an entity would be left pointing at something despawned.
        let grabbed = hover
            .values()
            .flat_map(|hits| hits.keys())
            .find_map(|entity| handles.get(*entity).ok());
        let Some(handle) = grabbed else {
            return;
        };
        *dragging = Some((handle.property, handle.end));
    }
    let Some((property, end)) = *dragging else {
        return;
    };

    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    let Some((_, node, transform)) = tracks
        .iter()
        .find(|(track, _, _)| track.property == property)
    else {
        return;
    };
    let scale = node.inverse_scale_factor();
    let width = node.size().x * scale;
    if width <= 0.0 {
        return;
    }
    let left = transform.translation.x * scale - width * 0.5;
    let fraction = ((cursor.x - left) / width).clamp(0.0, 1.0);

    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    if let Some(range) = properties
        .properties
        .get_mut(property)
        .and_then(|property| property.range_mut())
    {
        let value = range.value_at(fraction);
        range.set_end(end, value);
    }
}

/// Keep the range controls matching their property, without respawning them.
///
/// A drag moves an end continuously, and rebuilding the panel would despawn
/// the very handle under the pointer — which stopped a drag dead after the
/// first step. Everything a range draws is updated in place instead.
pub fn update_range_controls(
    palette: Res<Palette>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    mut fills: Query<(&RangeFill, &mut Node), (Without<RangeHandle>, Without<RangeBar>)>,
    mut handles: Query<(&RangeHandle, &mut Node), (Without<RangeFill>, Without<RangeBar>)>,
    mut bars: Query<(&RangeBar, &mut BackgroundColor), (Without<RangeFill>, Without<RangeHandle>)>,
    readouts: Query<(Entity, &RangeReadout)>,
    mut texts: Query<&mut Text>,
) {
    let Some(properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok())
    else {
        return;
    };
    let range_of = |index: usize| {
        properties
            .properties
            .get(index)
            .and_then(|property| property.range())
    };

    for (fill, mut node) in &mut fills {
        let Some(range) = range_of(fill.property) else {
            continue;
        };
        let from = range.fraction_of(range.from);
        let to = range.fraction_of(range.to);
        node.left = Val::Percent(from * 100.0);
        node.width = Val::Percent((to - from) * 100.0);
    }

    for (handle, mut node) in &mut handles {
        let Some(range) = range_of(handle.property) else {
            continue;
        };
        let value = match handle.end {
            RangeEnd::From => range.from,
            RangeEnd::To => range.to,
        };
        let (percent, nudge) = handle_placement(range.fraction_of(value));
        node.left = Val::Percent(percent);
        node.margin.left = Val::Px(nudge);
    }

    for (bar, mut color) in &mut bars {
        let Some(range) = range_of(bar.property) else {
            continue;
        };
        let centre = (bar.bucket as f32 + 0.5) / range.histogram.len().max(1) as f32;
        let wanted = bar_color(range.admits(range.value_at(centre)), &palette);
        if color.0 != wanted {
            color.0 = wanted;
        }
    }

    for (entity, readout) in &readouts {
        let Some(range) = range_of(readout.property) else {
            continue;
        };
        if let Ok(mut text) = texts.get_mut(entity) {
            let wanted = format!("{:.2} - {:.2}", range.from, range.to);
            if text.0 != wanted {
                text.0 = wanted;
            }
        }
    }
}
