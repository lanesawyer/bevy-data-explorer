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
//! and clicking a bar narrows the span to that bucket. Under the track, each
//! end is also a number field, typed into or scrubbed sideways, for a bound
//! too exact to drag to.

use bevy::picking::cursor::{EntityCursor, OverrideCursor};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::scene::Ready;
use bevy::text::{EditableText, EditableTextFilter};
use bevy::ui::UiGlobalTransform;
use bevy::window::SystemCursorIcon;
use bevy_feathers::controls::{
    FeathersNumberInput, HardLimit, NumberInputPrecision, NumberInputValue,
};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};
use bevy_feathers::tokens;
use bevy_ui_widgets::ValueChange;

use crate::app::theme::{Palette, token};
use crate::source::compact_count;
use crate::source::properties::{CellProperties, ColorScale, NumericRange, Ramp, RangeEnd};
use crate::source::table::{TableFilters, TablePaging, to_first_page};
use crate::view::SelectedSource;
use crate::widgets::space;
use crate::widgets::{BlocksFrameInput, hold_drag_cursor, patch_node, set_text, size};

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

/// One end of a numeric range as a number field, under its track.
#[derive(Component, Clone)]
pub struct RangeInput {
    pub owner: RangeOwner,
    pub property: usize,
    pub end: RangeEnd,
}

impl Default for RangeInput {
    fn default() -> Self {
        RangeInput {
            owner: RangeOwner::CellProperty,
            property: 0,
            end: RangeEnd::From,
        }
    }
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
    let value = range.bucket_center(bucket);
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

/// Decimal places a range's fields show and scrub in: two across an extent
/// of about one, fewer across a wider one and more across a narrower one, so
/// a step of the last digit is always a small part of the whole.
fn decimals(range: &NumericRange) -> i32 {
    let extent = (range.high - range.low).abs();
    if !extent.is_normal() {
        return 2;
    }
    (2 - extent.log10().floor() as i32).clamp(0, 6)
}

/// An end's value as its field shows it, rounded to the field's places.
fn shown_value(range: &NumericRange, end: RangeEnd) -> NumberInputValue {
    let value = match end {
        RangeEnd::From => range.from,
        RangeEnd::To => range.to,
    };
    let scale = 10f32.powi(decimals(range));
    NumberInputValue::F32((value * scale).round() / scale)
}

/// What a range's field lets be typed: enough for a decimal number, and
/// nothing that could not be part of one.
fn is_number_char(c: char) -> bool {
    c.is_ascii_digit() || c == '.' || c == '-'
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

/// How far along a track the pointer is, from 0 at its left end to 1 at its
/// right, however far past either end it has gone.
///
/// The pointer is in logical pixels and the track's layout in physical ones,
/// its center and its width both, so the layout is scaled down first: read
/// unscaled, a track on a display at twice the density put its ends in the
/// wrong place. `None` for a track not laid out yet.
fn fraction_along(
    pointer_x: f32,
    physical_center_x: f32,
    physical_width: f32,
    inverse_scale: f32,
) -> Option<f32> {
    let width = physical_width * inverse_scale;
    if width <= 0.0 {
        return None;
    }
    let left = physical_center_x * inverse_scale - width * 0.5;
    Some(((pointer_x - left) / width).clamp(0.0, 1.0))
}

/// Where a span grabbed at `grabbed` along its track, starting at `from`,
/// starts once the pointer is at `fraction`: moved by as much of the extent
/// `low..high` as the pointer moved of the track.
fn slid_from(from: f32, grabbed: f32, fraction: f32, low: f32, high: f32) -> f32 {
    from + (fraction - grabbed) * (high - low)
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
                    Children [RangeBar { owner: { owner }, property: { property }, bucket: { bucket } }
// Its column takes the click, however short the bar.
Pickable::IGNORE
Node {
    width: { Val::Percent(100.0) },
    height: { height },
}
BackgroundColor({ bar_color(range, bucket, ramp, palette) })]
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
                    Pickable::IGNORE
                    Children [RangeFill { owner: { owner }, property: { property } }
        Pickable::IGNORE
        Node {
            position_type: { PositionType::Absolute },
            left: { Val::Percent(from * 100.0) },
            width: { Val::Percent((to - from) * 100.0) },
            height: { Val::Percent(100.0) },
        }
        ThemeBackgroundColor({ token::SELECTION })]
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
    let places = decimals(range);
    let (low, high) = (range.low, range.high);
    // No `SoftLimit`: Feathers draws one as a bar filling the field, which
    // reads as progress and does not move with the pointer. Without it a
    // scrub moves the value relative to where it started, at a pace taken
    // from the places shown.
    let field = |end: RangeEnd| {
        let value = shown_value(range, end);
        bsn! {
            @FeathersNumberInput
            RangeInput { owner: { owner }, property: { property }, end: { end } }
            ~{ value }
            NumberInputPrecision({ places })
            ~{ HardLimit::f32(low..=high) }
            BlocksFrameInput
            Node { flex_grow: { 1.0_f32 }, flex_basis: { Val::ZERO }, min_width: { Val::ZERO } }
            on(filter_range_input)
        }
    };
    let count = count_text(range);
    let readout = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(space::CONTROLS) },
            }
            Children [
                {field(RangeEnd::From)}
                --
                {field(RangeEnd::To)}
                --
                RangeCount { owner: { owner }, property: { property } }
                Text({ count })
                TextFont { font_size: { FontSize::Px(size::SMALL) } }
                ThemeTextColor({ tokens::TEXT_DIM })
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
    ui_scale: Res<UiScale>,
    hover: Res<HoverMap>,
    handles: Query<&RangeHandle>,
    buckets: Query<&RangeBucket>,
    tracks: Query<(&RangeTrack, &ComputedNode, &UiGlobalTransform)>,
    selected: SelectedSource,
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
    let Some(pointer) = crate::widgets::ui_cursor(window, &ui_scale) else {
        return;
    };
    // How far along a control's track the pointer is, as a fraction.
    let along = |owner: RangeOwner, property: usize| {
        let (_, node, transform) = tracks
            .iter()
            .find(|(track, _, _)| track.owner == owner && track.property == property)?;
        fraction_along(
            pointer.x,
            transform.translation.x,
            node.size().x,
            node.inverse_scale_factor(),
        )
    };
    // A source holds one or the other, never both, so both are taken and
    // whichever the control names is the one written to.
    let Some(source) = selected.entity() else {
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
            range.slide_to(slid_from(from, grabbed, fraction, range.low, range.high));
            to_first_page(paging);
        }
    }
}

/// Keep letters out of a range's field once its text exists.
///
/// Feathers' number input takes any text and only rejects it on Enter, so the
/// filter goes on the text inside it, which a scene's `Ready` waits for.
fn filter_range_input(
    ready: On<Ready>,
    children: Query<&Children>,
    texts: Query<(), With<EditableText>>,
    mut commands: Commands,
) {
    for text in children
        .iter_descendants(ready.entity)
        .filter(|entity| texts.contains(*entity))
    {
        commands
            .entity(text)
            .insert(EditableTextFilter::new(is_number_char));
    }
}

/// Move an end to what was typed into its field, or scrubbed to.
///
/// Kept on its own side of the other end rather than pivoting past it as a
/// dragged thumb does: a field names the end it sets, and a typed lower bound
/// above the upper one is a slip, not a request to swap them.
pub fn on_range_input(
    change: On<ValueChange<f32>>,
    inputs: Query<&RangeInput>,
    selected: SelectedSource,
    mut sources: Query<&mut CellProperties>,
    mut tables: Query<(&mut TableFilters, Option<&mut TablePaging>)>,
) {
    let Ok(input) = inputs.get(change.source) else {
        return;
    };
    let Some(source) = selected.entity() else {
        return;
    };
    let properties = sources.get_mut(source).ok();
    let (filters, paging) = tables.get_mut(source).ok().unzip();
    let Some(range) = range_mut(
        input.owner,
        input.property,
        properties.map(Mut::into_inner),
        filters.map(Mut::into_inner),
    ) else {
        return;
    };
    range.set_end(input.end, change.value);
    to_first_page(paging.flatten());
}

/// Keep the range controls matching their property, without respawning them.
///
/// A drag moves an end continuously, and rebuilding the panel would despawn
/// the very handle under the pointer — which stopped a drag dead after the
/// first step. Everything a range draws is updated in place instead.
pub fn update_range_controls(
    palette: Res<Palette>,
    selected: SelectedSource,
    sources: Query<(&CellProperties, Option<&ColorScale>)>,
    tables: Query<&TableFilters>,
    mut fills: Query<(&RangeFill, &mut Node), (Without<RangeHandle>, Without<RangeBar>)>,
    mut handles: Query<(&RangeHandle, &mut Node), (Without<RangeFill>, Without<RangeBar>)>,
    mut bars: Query<
        (&RangeBar, &mut BackgroundColor, &mut Node),
        (Without<RangeFill>, Without<RangeHandle>),
    >,
    inputs: Query<(Entity, &RangeInput, &NumberInputValue)>,
    counts: Query<(Entity, &RangeCount)>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    // A source holds cell properties or a table, never both, so a control is
    // read from whichever it names.
    let Some(source) = selected.entity() else {
        return;
    };
    let (properties, scale) = sources.get(source).ok().unzip();
    let scale = scale.flatten();
    let filters = tables.get(source).ok();
    let range_of = |owner: RangeOwner, index: usize| range_of(owner, index, properties, filters);

    for (fill, node) in &mut fills {
        let Some(range) = range_of(fill.owner, fill.property) else {
            continue;
        };
        let from = range.fraction_of(range.from);
        let to = range.fraction_of(range.to);
        patch_node(node, |node| {
            node.left = Val::Percent(from * 100.0);
            node.width = Val::Percent((to - from) * 100.0);
        });
    }

    for (handle, node) in &mut handles {
        let Some(range) = range_of(handle.owner, handle.property) else {
            continue;
        };
        let value = match handle.end {
            RangeEnd::From => range.from,
            RangeEnd::To => range.to,
        };
        let (percent, nudge) = handle_placement(range.fraction_of(value));
        patch_node(node, |node| {
            node.left = Val::Percent(percent);
            node.margin.left = Val::Px(nudge);
        });
    }

    let ramp = properties.and_then(|properties| properties.ramp(scale));
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

    // Only when it differs: the value is immutable, and inserting it is what
    // rewrites the field's text.
    for (entity, input, shown) in &inputs {
        let Some(range) = range_of(input.owner, input.property) else {
            continue;
        };
        let wanted = shown_value(range, input.end);
        if *shown != wanted {
            commands.entity(entity).insert(wanted);
        }
    }

    for (entity, count) in &counts {
        let Some(range) = range_of(count.owner, count.property) else {
            continue;
        };
        if let Ok(text) = texts.get_mut(entity) {
            let wanted = count_text(range);
            set_text(text, &wanted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_of_a_track_is_halfway_along_at_any_display_density() {
        // A track 200 logical pixels wide centered at 300, as laid out at one
        // pixel to the point and at two.
        assert_eq!(fraction_along(300.0, 300.0, 200.0, 1.0), Some(0.5));
        assert_eq!(fraction_along(300.0, 600.0, 400.0, 0.5), Some(0.5));
        assert_eq!(fraction_along(250.0, 600.0, 400.0, 0.5), Some(0.25));
    }

    #[test]
    fn a_pointer_past_either_end_holds_that_end() {
        assert_eq!(fraction_along(0.0, 300.0, 200.0, 1.0), Some(0.0));
        assert_eq!(fraction_along(900.0, 300.0, 200.0, 1.0), Some(1.0));
    }

    #[test]
    fn a_track_not_laid_out_yet_says_nothing() {
        assert_eq!(fraction_along(300.0, 300.0, 0.0, 1.0), None);
    }

    #[test]
    fn a_span_moves_as_much_of_the_extent_as_the_pointer_moves_of_the_track() {
        // Grabbed halfway along a track over 0..100 and moved a quarter of it.
        assert_eq!(slid_from(10.0, 0.5, 0.75, 0.0, 100.0), 35.0);
        assert_eq!(
            slid_from(10.0, 0.5, 0.25, 0.0, 100.0),
            -15.0,
            "slide_to clamps"
        );
    }

    #[test]
    fn the_tallest_bucket_fills_the_histogram_and_an_empty_one_is_a_sliver() {
        let range = NumericRange::full(0.0, 3.0, vec![10, 5, 0]);
        assert_eq!(bar_height(&range, 0), Val::Px(HISTOGRAM_PX));
        assert_eq!(bar_height(&range, 1), Val::Px(HISTOGRAM_PX * 0.5));
        // Still drawn, so the width of the distribution reads even where it
        // is empty.
        assert_eq!(bar_height(&range, 2), Val::Px(HISTOGRAM_PX * 0.02));
    }

    #[test]
    fn a_histogram_with_nothing_counted_yet_draws_no_bar_and_no_count() {
        let range = NumericRange::full(0.0, 1.0, vec![0, 0]);
        assert_eq!(bar_height(&range, 0), Val::Px(HISTOGRAM_PX * 0.02));
        assert_eq!(count_text(&range), "");
    }

    #[test]
    fn the_count_says_of_how_many_only_when_the_span_leaves_some_out() {
        let mut range = NumericRange::full(0.0, 4.0, vec![10, 20, 30, 40]);
        assert_eq!(count_text(&range), "100");
        range.from = 2.0;
        assert_eq!(count_text(&range), "70 of 100");
    }

    #[test]
    fn a_field_shows_fewer_places_the_wider_the_extent() {
        assert_eq!(decimals(&NumericRange::full(0.0, 1.0, vec![])), 2);
        assert_eq!(decimals(&NumericRange::full(0.0, 500.0, vec![])), 0);
        assert_eq!(decimals(&NumericRange::full(0.0, 0.05, vec![])), 4);
        assert_eq!(decimals(&NumericRange::full(3.0, 3.0, vec![])), 2);
    }

    #[test]
    fn a_field_shows_its_end_rounded_to_its_places() {
        let mut range = NumericRange::full(0.0, 10.0, vec![]);
        range.from = 1.2345;
        assert_eq!(
            shown_value(&range, RangeEnd::From),
            NumberInputValue::F32(1.2)
        );
        assert_eq!(
            shown_value(&range, RangeEnd::To),
            NumberInputValue::F32(10.0)
        );
    }

    #[test]
    fn a_field_takes_only_what_a_number_is_written_with() {
        assert!("-12.5".chars().all(is_number_char));
        assert!(!"1e3".chars().all(is_number_char));
        assert!(!"abc".chars().all(is_number_char));
    }

    #[test]
    fn a_handle_at_either_end_stays_on_its_rail() {
        assert_eq!(handle_placement(0.0), (0.0, 0.0));
        // All the way along, pulled back by its own width so it does not
        // hang off the end.
        assert_eq!(handle_placement(1.0), (100.0, -RANGE_THUMB_PX));
    }

    #[test]
    fn a_bucket_outside_the_span_is_dimmed_and_one_inside_is_lit() {
        let palette = Palette::dark();
        let mut range = NumericRange::full(0.0, 4.0, vec![1, 1, 1, 1]);
        range.from = 2.0;
        assert_eq!(bar_color(&range, 0, None, &palette), palette.bar_dim);
        assert_eq!(bar_color(&range, 3, None, &palette), palette.fill);
    }
}
