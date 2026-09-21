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
//!
//! Either end is dragged by its thumb, and may be dragged past the other to
//! pivot around it. Dragging anywhere else on the track slides the whole span,
//! and clicking a bar narrows the span to that bucket.

use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::window::SystemCursorIcon;
use bevy_feathers::cursor::{EntityCursor, OverrideCursor};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};
use bevy_feathers::tokens;

use crate::app::theme::{Palette, token};
use crate::source::properties::{CellProperties, NumericRange, Ramp, RangeEnd};
use crate::source::table::{TableFilters, TablePaging, to_first_page};
use crate::source::{ShowsSource, compact_count};
use crate::view::SelectedPanel;
use crate::widgets::space;
use crate::widgets::{BlocksFrameInput, hold_drag_cursor, size};

/// Height of the histogram drawn above a numeric range.
const HISTOGRAM_PX: f32 = 44.0;
const RANGE_TRACK_PX: f32 = 6.0;
const RANGE_THUMB_PX: f32 = 12.0;

/// Where a range control's numbers live.
///
/// The control draws and drags a [`NumericRange`], and two things hold one: a
/// numeric cell property, and a column of a table narrowed by a span. Naming
/// which keeps one widget serving both rather than a second one drifting away
/// from this.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum RangeOwner {
    /// A numeric property of the source's cells, by its place in them.
    #[default]
    CellProperty,
    /// A column of the source's table, by its place in its filters.
    TableColumn,
}

/// One end of a numeric range's control.
#[derive(Component, Clone)]
pub struct RangeHandle {
    pub owner: RangeOwner,
    pub property: usize,
    pub end: RangeEnd,
}

impl Default for RangeHandle {
    fn default() -> Self {
        RangeHandle {
            owner: RangeOwner::CellProperty,
            property: 0,
            end: RangeEnd::From,
        }
    }
}

/// The track a numeric range is dragged along, which is what a drag is
/// measured against.
#[derive(Component, Clone, Default)]
pub struct RangeTrack {
    pub owner: RangeOwner,
    pub property: usize,
}

/// The readout under a numeric range.
#[derive(Component, Clone, Default)]
pub struct RangeReadout {
    pub owner: RangeOwner,
    pub property: usize,
}

/// The filled span of a numeric range's rail.
#[derive(Component, Clone, Default)]
pub struct RangeFill {
    pub owner: RangeOwner,
    pub property: usize,
}

/// How many cells the span admits, beside the readout.
#[derive(Component, Clone, Default)]
pub struct RangeCount {
    pub owner: RangeOwner,
    pub property: usize,
}

/// One bar of a numeric range's histogram.
#[derive(Component, Clone, Default)]
pub struct RangeBar {
    pub owner: RangeOwner,
    pub property: usize,
    pub bucket: usize,
}

/// The full-height column a bar stands in, clicked to pick its bucket.
///
/// Clicked rather than the bar itself, which may be only a pixel or two high.
#[derive(Component, Clone, Default)]
pub struct RangeBucket {
    pub owner: RangeOwner,
    pub property: usize,
    pub bucket: usize,
}

/// What a press on a numeric range is dragging, until the button is released.
///
/// Held by property index rather than by entity: the panel may rebuild
/// mid-drag, and an entity would be left pointing at something despawned.
#[derive(Clone, Copy)]
pub enum RangeDrag {
    /// One end, which may change as it is dragged past the other.
    End {
        owner: RangeOwner,
        property: usize,
        end: RangeEnd,
    },
    /// The whole span, from where it started and where along the track it was
    /// grabbed.
    Span {
        owner: RangeOwner,
        property: usize,
        from: f32,
        grabbed: f32,
    },
}

/// Buckets inside the chosen span are drawn lit, the rest dimmed, so the whole
/// distribution stays visible while part of it is picked.
///
/// While points are colored by this property the lit buckets take the colors
/// those points are drawn in, which makes the histogram its legend.
fn bar_color(range: &NumericRange, bucket: usize, ramp: Option<&Ramp>, palette: &Palette) -> Color {
    let value = range.bucket_centre(bucket);
    match ramp {
        _ if !range.admits(value) => palette.bar_dim,
        Some(ramp) => ramp.gradient.sample(ramp.fraction_of(value)),
        None => palette.fill,
    }
}

/// A bar's height, against the tallest in its histogram.
fn bar_height(range: &NumericRange, bucket: usize) -> Val {
    let peak = range.histogram.iter().copied().max().unwrap_or(1).max(1);
    let count = range.histogram.get(bucket).copied().unwrap_or(0);
    Val::Px((count as f32 / peak as f32).max(0.02) * HISTOGRAM_PX)
}

/// The readout's text: the span's ends.
fn span_text(range: &NumericRange) -> String {
    format!("{:.2} - {:.2}", range.from, range.to)
}

/// The count's text: the cells inside the span, and of how many when the span
/// leaves some out. Blank until the histogram has anything in it.
fn count_text(range: &NumericRange) -> String {
    match range.counts() {
        (_, 0) => String::new(),
        (inside, total) if inside == total => compact_count(total),
        (inside, total) => format!("{} of {}", compact_count(inside), compact_count(total)),
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
pub fn spawn_range_control(
    commands: &mut Commands,
    owner: RangeOwner,
    property: usize,
    range: &NumericRange,
    ramp: Option<&Ramp>,
    palette: &Palette,
) -> Entity {
    let bars: Vec<Entity> = (0..range.histogram.len())
        .map(|bucket| {
            let height = bar_height(range, bucket);
            commands
                .spawn_scene(bsn! {
                    RangeBucket { owner: { owner }, property: { property }, bucket: { bucket } }
                    BlocksFrameInput
                    EntityCursor::System({ SystemCursorIcon::Pointer })
                    Node {
                        flex_grow: { 1.0_f32 },
                        flex_basis: { Val::ZERO },
                        height: { Val::Percent(100.0) },
                        align_items: { AlignItems::End },
                        margin: { UiRect::horizontal(Val::Px(space::SEAM / 2.0)) },
                    }
                    Children [(
                        RangeBar { owner: { owner }, property: { property }, bucket: { bucket } }
                        // Its column takes the click, however short the bar.
                        template_value(Pickable::IGNORE)
                        Node {
                            width: { Val::Percent(100.0) },
                            height: { height },
                        }
                        BackgroundColor({ bar_color(range, bucket, ramp, palette) })
                    )]
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
            RangeTrack { owner: { owner }, property: { property } }
            BlocksFrameInput
            EntityCursor::System({ SystemCursorIcon::Grab })
            Node {
                width: { Val::Percent(100.0) },
                height: { Val::Px(RANGE_THUMB_PX) },
                margin: { UiRect::top(Val::Px(space::CONTROLS)) },
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
            // The track takes the press, wherever along it, to slide the span.
            template_value(Pickable::IGNORE)
            Children [(
                RangeFill { owner: { owner }, property: { property } }
                template_value(Pickable::IGNORE)
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
                EntityCursor::System({ SystemCursorIcon::EwResize })
                RangeHandle { owner: { owner }, property: { property }, end: { end } }
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

    // Deliberately not marked for rebuilding: these hang inside a
    // sub-section that is, and despawning is recursive.
    let span = span_text(range);
    let count = count_text(range);
    let readout = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                justify_content: { JustifyContent::SpaceBetween },
                column_gap: { Val::Px(space::CONTROLS) },
            }
            Children [
                (
                    RangeReadout { owner: { owner }, property: { property } }
                    Text({ span })
                    TextFont { font_size: { FontSize::Px(size::SMALL) } }
                    ThemeTextColor({ tokens::TEXT_DIM })
                ),
                (
                    RangeCount { owner: { owner }, property: { property } }
                    Text({ count })
                    TextFont { font_size: { FontSize::Px(size::SMALL) } }
                    ThemeTextColor({ tokens::TEXT_DIM })
                ),
            ]
        })
        .id();

    let column = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                row_gap: { Val::Px(space::STACKED) },
            }
        })
        .id();
    commands
        .entity(column)
        .add_children(&[histogram, track, readout]);
    column
}

/// The range a control acts on, wherever it lives.
fn range_mut<'a>(
    owner: RangeOwner,
    property: usize,
    properties: Option<&'a mut CellProperties>,
    filters: Option<&'a mut TableFilters>,
) -> Option<&'a mut NumericRange> {
    match owner {
        RangeOwner::CellProperty => properties?
            .properties
            .get_mut(property)
            .and_then(|property| property.range_mut()),
        RangeOwner::TableColumn => filters?.columns.get_mut(property)?.span_mut(),
    }
}

/// The same, to read.
fn range_of<'a>(
    owner: RangeOwner,
    property: usize,
    properties: Option<&'a CellProperties>,
    filters: Option<&'a TableFilters>,
) -> Option<&'a NumericRange> {
    match owner {
        RangeOwner::CellProperty => properties?.properties.get(property)?.range(),
        RangeOwner::TableColumn => filters?.columns.get(property)?.span(),
    }
}

/// Drag either end of a numeric range, slide its span, or pick a bucket.
///
/// Values come from where the pointer sits along the track rather than from
/// accumulated deltas, so a fast drag cannot fall behind the cursor.
pub fn drag_range_handles(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    hover: Res<HoverMap>,
    handles: Query<&RangeHandle>,
    buckets: Query<&RangeBucket>,
    tracks: Query<(&RangeTrack, &ComputedNode, &UiGlobalTransform)>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
    mut tables: Query<(&mut TableFilters, Option<&mut TablePaging>)>,
    mut dragging: Local<Option<RangeDrag>>,
    mut held: Local<bool>,
    cursor: Option<ResMut<OverrideCursor>>,
) {
    if !mouse.pressed(MouseButton::Left) {
        *dragging = None;
        hold_drag_cursor(false, &mut held, cursor, SystemCursorIcon::Default);
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(pointer) = window.cursor_position() else {
        return;
    };
    // How far along a control's track the pointer is, as a fraction.
    let along = |owner: RangeOwner, property: usize| {
        let (_, node, transform) = tracks
            .iter()
            .find(|(track, _, _)| track.owner == owner && track.property == property)?;
        let scale = node.inverse_scale_factor();
        let width = node.size().x * scale;
        if width <= 0.0 {
            return None;
        }
        let left = transform.translation.x * scale - width * 0.5;
        Some(((pointer.x - left) / width).clamp(0.0, 1.0))
    };
    // A source holds one or the other, never both, so both are taken and
    // whichever the control names is the one written to.
    let Some(source) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0)
    else {
        return;
    };
    let mut properties = sources.get_mut(source).ok();
    let (mut filters, mut paging) = tables.get_mut(source).ok().unzip();
    let paging = paging.take().flatten();

    if mouse.just_pressed(MouseButton::Left) {
        // Grabbed on the way down and held until release, so the pointer may
        // leave what it grabbed without dropping the drag.
        let hovered: Vec<Entity> = hover
            .values()
            .flat_map(|hits| hits.keys().copied())
            .collect();
        if let Some(handle) = hovered.iter().find_map(|entity| handles.get(*entity).ok()) {
            *dragging = Some(RangeDrag::End {
                owner: handle.owner,
                property: handle.property,
                end: handle.end,
            });
        } else if let Some(bucket) = hovered.iter().find_map(|entity| buckets.get(*entity).ok()) {
            if let Some(range) = range_mut(
                bucket.owner,
                bucket.property,
                properties.as_deref_mut(),
                filters.as_deref_mut(),
            ) {
                let (from, to) = range.bucket_span(bucket.bucket);
                range.from = from;
                range.to = to;
                to_first_page(paging);
            }
            return;
        } else if let Some((track, _, _)) =
            hovered.iter().find_map(|entity| tracks.get(*entity).ok())
            && let Some(grabbed) = along(track.owner, track.property)
            && let Some(range) = range_mut(
                track.owner,
                track.property,
                properties.as_deref_mut(),
                filters.as_deref_mut(),
            )
        {
            *dragging = Some(RangeDrag::Span {
                owner: track.owner,
                property: track.property,
                from: range.from,
                grabbed,
            });
        }
    }
    let Some(drag) = *dragging else {
        return;
    };
    let icon = match drag {
        RangeDrag::End { .. } => SystemCursorIcon::EwResize,
        RangeDrag::Span { .. } => SystemCursorIcon::Grabbing,
    };
    hold_drag_cursor(true, &mut held, cursor, icon);

    match drag {
        RangeDrag::End {
            owner,
            property,
            end,
        } => {
            let (Some(fraction), Some(range)) = (
                along(owner, property),
                range_mut(
                    owner,
                    property,
                    properties.as_deref_mut(),
                    filters.as_deref_mut(),
                ),
            ) else {
                return;
            };
            let value = range.value_at(fraction);
            let end = range.drag_end(end, value);
            *dragging = Some(RangeDrag::End {
                owner,
                property,
                end,
            });
            to_first_page(paging);
        }
        RangeDrag::Span {
            owner,
            property,
            from,
            grabbed,
        } => {
            let (Some(fraction), Some(range)) = (
                along(owner, property),
                range_mut(
                    owner,
                    property,
                    properties.as_deref_mut(),
                    filters.as_deref_mut(),
                ),
            ) else {
                return;
            };
            let moved = (fraction - grabbed) * (range.high - range.low);
            range.slide_to(from + moved);
            to_first_page(paging);
        }
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
    tables: Query<&TableFilters>,
    mut fills: Query<(&RangeFill, &mut Node), (Without<RangeHandle>, Without<RangeBar>)>,
    mut handles: Query<(&RangeHandle, &mut Node), (Without<RangeFill>, Without<RangeBar>)>,
    mut bars: Query<
        (&RangeBar, &mut BackgroundColor, &mut Node),
        (Without<RangeFill>, Without<RangeHandle>),
    >,
    readouts: Query<(Entity, &RangeReadout)>,
    counts: Query<(Entity, &RangeCount)>,
    mut texts: Query<&mut Text>,
) {
    // A source holds cell properties or a table, never both, so a control is
    // read from whichever it names.
    let Some(source) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0)
    else {
        return;
    };
    let properties = sources.get(source).ok();
    let filters = tables.get(source).ok();
    let range_of = |owner: RangeOwner, index: usize| range_of(owner, index, properties, filters);

    for (fill, mut node) in &mut fills {
        let Some(range) = range_of(fill.owner, fill.property) else {
            continue;
        };
        let from = range.fraction_of(range.from);
        let to = range.fraction_of(range.to);
        node.left = Val::Percent(from * 100.0);
        node.width = Val::Percent((to - from) * 100.0);
    }

    for (handle, mut node) in &mut handles {
        let Some(range) = range_of(handle.owner, handle.property) else {
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

    let ramp = properties.and_then(CellProperties::ramp);
    // Bars are redrawn as well as recolored: a histogram is counted again
    // among the cells the other filters admit whenever they change.
    for (bar, mut color, mut node) in &mut bars {
        let Some(range) = range_of(bar.owner, bar.property) else {
            continue;
        };
        let ramp = ramp.as_ref().filter(|_| {
            bar.owner == RangeOwner::CellProperty
                && properties.is_some_and(|it| it.color_by == Some(bar.property))
        });
        let wanted = bar_color(range, bar.bucket, ramp, &palette);
        if color.0 != wanted {
            color.0 = wanted;
        }
        let height = bar_height(range, bar.bucket);
        if node.height != height {
            node.height = height;
        }
    }

    for (entity, readout) in &readouts {
        let Some(range) = range_of(readout.owner, readout.property) else {
            continue;
        };
        if let Ok(mut text) = texts.get_mut(entity) {
            let wanted = span_text(range);
            if text.0 != wanted {
                text.0 = wanted;
            }
        }
    }

    for (entity, count) in &counts {
        let Some(range) = range_of(count.owner, count.property) else {
            continue;
        };
        if let Ok(mut text) = texts.get_mut(entity) {
            let wanted = count_text(range);
            if text.0 != wanted {
                text.0 = wanted;
            }
        }
    }
}
