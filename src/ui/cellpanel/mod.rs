//! The cell properties section of the sidebar.
//!
//! Offered only by sources that advertise [`CellProperties`], so an image never
//! shows it. Each property becomes a sub-section: the button in its header
//! colours points by that property, and the checkboxes inside filter points
//! down to the values still ticked.
//!
//! The section is rebuilt whenever the selected source's properties change, so
//! a lookup that fills them in later — over HTTP, from whatever service knows
//! the labels — needs no cooperation from this module.

use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::FeathersCheckbox;
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::{Activate, ValueChange};

use crate::app::schedule::{Boot, Stage};
use crate::source::DataSource;
use crate::source::properties::{
    CellProperties, CellProperty, NumericRange, PropertyKind, PropertyState, RangeEnd,
};
use crate::ui::sidebar::{SectionOrder, SidebarContent};
use crate::ui::widgets::{Accordion, spawn_accordion, spawn_header_button, spawn_menu};
use crate::view::{BlocksFrameInput, SelectedPanel, ShowsSource};

/// The section itself, hidden for sources with no properties to show.
#[derive(Component, Clone, Default)]
pub struct CellPanel;

/// The section's own menu, for controls that act on all of its properties.
#[derive(Component, Clone, Default)]
pub struct CellPanelMenu;

/// Marks what a rebuild replaces.
///
/// Only the roots of the rebuilt subtrees carry this. Despawning is recursive,
/// so marking something nested inside another marked entity means trying to
/// despawn it twice, which Bevy reports as touching a dead entity.
#[derive(Component, Clone, Default)]
pub struct CellPanelContent;

/// The button that colours points by a property.
#[derive(Component, Clone, Default)]
pub struct ColourByButton {
    pub property: usize,
}

/// The button that drops one property's filters.
#[derive(Component, Clone, Default)]
pub struct ClearPropertyButton {
    pub property: usize,
}

/// The button that drops every filter, on the section's own header.
#[derive(Component, Clone, Default)]
pub struct ClearAllButton;

/// A checkbox admitting one value of one property.
#[derive(Component, Clone, Default)]
pub struct ValueCheckbox {
    pub property: usize,
    pub value: usize,
}

/// Below the view configuration, which applies to every source.
const SECTION_ORDER: u32 = 20;

pub fn spawn_cell_panel(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let accordion = spawn_accordion(&mut commands, "Cell properties", true);
    commands
        .entity(accordion.section)
        .insert(CellPanel)
        .insert(SectionOrder(SECTION_ORDER))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: Val::Percent(100.0),
            display: Display::None,
            ..default()
        });
    commands.entity(parent).add_child(accordion.section);
    commands.entity(accordion.body).insert(CellPanelBody);

    // Added before the menu so it sits to its left, and hides itself when
    // there is nothing to clear.
    let clear = spawn_header_button(&mut commands, accordion.header, "Clear filters");
    commands.entity(clear).insert(ClearAllButton);

    let menu = spawn_menu(&mut commands, accordion.header);
    commands.entity(menu).insert(CellPanelMenu);
}

/// The body the sub-sections are built into.
#[derive(Component, Clone, Default)]
pub struct CellPanelBody;

/// Rebuild the sub-sections when the selected source's properties change.
pub fn rebuild_cell_panel(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<(&DataSource, &CellProperties)>,
    body: Query<Entity, With<CellPanelBody>>,
    mut section: Query<&mut Node, With<CellPanel>>,
    existing: Query<Entity, With<CellPanelContent>>,
    open: Res<OpenSections>,
    mut shown: Local<Option<(Entity, Vec<String>)>>,
) {
    let Ok(body) = body.single() else { return };

    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok().map(|found| (shows.0, found)));

    for mut node in &mut section {
        let wanted = if source.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let Some((entity, (_, properties))) = source else {
        *shown = None;
        return;
    };

    // Rebuilt only when something visible changed: which source, what it is
    // coloured by, and which values are ticked.
    // Only what the sections are built from. Everything a property's controls
    // display — ticks, the colour choice, range ends — is written onto the
    // existing entities instead, because rebuilding respawns every checkbox and
    // Feathers draws a checkbox's mark before its styling system has had a
    // frame to hide it, which reads as every box flashing ticked.
    let fingerprint: (Entity, Vec<String>) = (
        entity,
        properties
            .properties
            .iter()
            .map(|property| property.id.clone())
            .collect(),
    );
    if shown.as_ref() == Some(&fingerprint) {
        return;
    }
    *shown = Some(fingerprint);

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    if let PropertyState::Failed(error) = &properties.state {
        let message = commands
            .spawn_scene(bsn! {
                CellPanelContent
                label_dim(format!("Could not load properties: {error}"))
                InheritableFont { font_size: { 11.0f32 } }
            })
            .id();
        commands.entity(body).add_child(message);
        return;
    }
    if properties.properties.is_empty() {
        let message = commands
            .spawn_scene(bsn! {
                CellPanelContent
                label_dim(match properties.state {
                    PropertyState::Pending => "Loading properties...",
                    _ => "No properties for this dataset.",
                })
                InheritableFont { font_size: { 11.0f32 } }
            })
            .id();
        commands.entity(body).add_child(message);
        return;
    }

    let mut sections = Vec::with_capacity(properties.properties.len());
    for (index, property) in properties.properties.iter().enumerate() {
        let colouring = properties.colour_by == Some(index);
        // Sub-sections start closed, since a property can have many values and
        // all of them open at once would bury the rest of the sidebar. One the
        // user opened stays open across a rebuild.
        let was_open = open.0.get(&index).copied().unwrap_or(false);
        let sub = spawn_accordion(&mut commands, &property.name, was_open);
        commands
            .entity(sub.section)
            .insert((CellPanelContent, PropertySection { property: index }));

        // Clearing sits to the left of the colour control, as it does on the
        // section's own header. It hides itself when there is nothing to clear.
        let clear = spawn_header_button(&mut commands, sub.header, "clear");
        commands
            .entity(clear)
            .insert(ClearPropertyButton { property: index });

        let button =
            spawn_header_button(&mut commands, sub.header, if colouring { "*" } else { "o" });
        commands
            .entity(button)
            .insert(ColourByButton { property: index });

        let rows = match &property.kind {
            PropertyKind::Categorical(values) => values
                .iter()
                .enumerate()
                .map(|(position, value)| {
                    let caption = value.label.clone();
                    let row = commands
                        .spawn_scene(bsn! {
                            @FeathersCheckbox {
                                @caption: { bsn_list![label(caption)] }
                            }
                            BlocksFrameInput
                            ValueCheckbox { property: { index }, value: { position } }
                        })
                        .id();
                    if value.selected {
                        commands.entity(row).insert(Checked);
                    }
                    row
                })
                .collect(),
            PropertyKind::Numeric(range) => {
                vec![spawn_range_control(&mut commands, index, range)]
            }
        };
        commands.entity(sub.body).add_children(&rows);
        sections.push(sub.section);
    }
    commands.entity(body).add_children(&sections);
}

/// Colour points by the property whose header button was pressed.
pub fn on_colour_by(
    activate: On<Activate>,
    buttons: Query<&ColourByButton>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    properties.colour_by = Some(button.property);
}

/// Admit or exclude one value of one property.
pub fn on_value_toggled(
    change: On<ValueChange<bool>>,
    checkboxes: Query<&ValueCheckbox>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(checkbox) = checkboxes.get(change.source) else {
        return;
    };

    // The box's own `Checked` is deliberately not touched here. Changing the
    // property rebuilds the section, which respawns the box with its state
    // taken from the property — and writing to the old entity would land on
    // one that had just been despawned.
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    if let Some(value) = properties
        .properties
        .get_mut(checkbox.property)
        .and_then(|property| match &mut property.kind {
            PropertyKind::Categorical(values) => values.get_mut(checkbox.value),
            PropertyKind::Numeric(_) => None,
        })
    {
        value.selected = change.value;
    }
}

/// Which sub-sections are open, by property.
///
/// Rebuilding despawns the sub-sections, so ticking a checkbox would otherwise
/// close the very section being used. The flag outlives the accordion here.
#[derive(Resource, Default)]
pub struct OpenSections(pub std::collections::HashMap<usize, bool>);

/// Marks a sub-section with the property it stands for.
#[derive(Component, Clone, Default)]
pub struct PropertySection {
    pub property: usize,
}

pub fn record_open_sections(
    mut open: ResMut<OpenSections>,
    sections: Query<(&PropertySection, &Accordion)>,
) {
    for (section, accordion) in &sections {
        open.0.insert(section.property, accordion.open);
    }
}

/// Push each source's selection into the streamer drawing it, rebuilding the
/// resident points when it changes.
///
/// Colouring and filtering both decide what the vertices are, and the raw
/// columns are not kept after a node is built, so a change means loading those
/// nodes again. The same trade the image panel makes for its channels.
pub fn apply_selection(
    mut commands: Commands,
    changed: Query<(Entity, &CellProperties), Changed<CellProperties>>,
    mut points: Query<&mut crate::formats::pointcloud::PointStreamer>,
    mut slices: Option<ResMut<crate::formats::slices::SliceStreamer>>,
) {
    for (entity, properties) in &changed {
        let selection = properties.selection();

        // The streamer lives on the source entity, so the cloud whose
        // properties changed is the one to rebuild.
        if let Ok(mut streamer) = points.get_mut(entity)
            && streamer.selection != selection
        {
            streamer.selection = selection.clone();
            streamer.reset(&mut commands);
        }
        if let Some(streamer) = slices.as_mut()
            && streamer.source == entity
            && streamer.selection != selection
        {
            streamer.selection = selection.clone();
            streamer.reset(&mut commands);
        }
    }
}

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
fn bar_colour(inside: bool) -> Color {
    if inside {
        Color::srgb(0.38, 0.60, 0.90)
    } else {
        Color::srgb(0.22, 0.25, 0.31)
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
fn spawn_range_control(commands: &mut Commands, property: usize, range: &NumericRange) -> Entity {
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
                    BackgroundColor({ bar_colour(inside) })
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
            BackgroundColor({ Color::srgb(0.20, 0.22, 0.28) })
            Children [(
                RangeFill { property: { property } }
                Node {
                    position_type: { PositionType::Absolute },
                    left: { Val::Percent(from * 100.0) },
                    width: { Val::Percent((to - from) * 100.0) },
                    height: { Val::Percent(100.0) },
                }
                BackgroundColor({ Color::srgb(0.38, 0.60, 0.90) })
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
                BackgroundColor({ Color::srgb(0.85, 0.89, 0.95) })
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

    for (bar, mut colour) in &mut bars {
        let Some(range) = range_of(bar.property) else {
            continue;
        };
        let centre = (bar.bucket as f32 + 0.5) / range.histogram.len().max(1) as f32;
        let wanted = bar_colour(range.admits(range.value_at(centre)));
        if colour.0 != wanted {
            colour.0 = wanted;
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

/// Drop one property's filters.
pub fn on_clear_property(
    activate: On<Activate>,
    buttons: Query<&ClearPropertyButton>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    if let Some(property) = properties.properties.get_mut(button.property) {
        property.clear();
    }
}

/// Drop every filter on the selected source.
pub fn on_clear_all(
    activate: On<Activate>,
    buttons: Query<&ClearAllButton>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    properties.clear_all();
}

/// Show the clear controls only when they have something to clear, and keep
/// the count on the section's own button.
pub fn update_clear_buttons(
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    mut per_property: Query<(&ClearPropertyButton, &mut Node), Without<ClearAllButton>>,
    mut clear_all: Query<(Entity, &mut Node), (With<ClearAllButton>, Without<ClearPropertyButton>)>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    let properties = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok());

    for (button, mut node) in &mut per_property {
        let applied = properties
            .and_then(|properties| properties.properties.get(button.property))
            .map(CellProperty::applied)
            .unwrap_or(0);
        let wanted = if applied > 0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let total = properties.map(CellProperties::applied).unwrap_or(0);
    for (entity, mut node) in &mut clear_all {
        let wanted = if total > 0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
        if total == 0 {
            continue;
        }
        let label = format!(
            "Clear {total} {}",
            if total == 1 { "filter" } else { "filters" }
        );
        for child in children.iter_descendants(entity) {
            if let Ok(mut text) = texts.get_mut(child)
                && text.0 != label
            {
                text.0 = label.clone();
            }
        }
    }
}

/// Keep each checkbox and colour control matching its property, without
/// respawning them.
///
/// Ticking a box used to rebuild the whole section. Beyond being wasteful, a
/// fresh Feathers checkbox draws its mark until the styling system runs a frame
/// later, so a rebuild made every box flash ticked before settling.
pub fn update_property_controls(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    boxes: Query<(Entity, &ValueCheckbox, Has<Checked>)>,
    colours: Query<(Entity, &ColourByButton)>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    let Some(properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok())
    else {
        return;
    };

    for (entity, checkbox, checked) in &boxes {
        let selected = properties
            .properties
            .get(checkbox.property)
            .and_then(|property| property.values().get(checkbox.value))
            .is_some_and(|value| value.selected);
        if selected == checked {
            continue;
        }
        if selected {
            commands.entity(entity).insert(Checked);
        } else {
            commands.entity(entity).remove::<Checked>();
        }
    }

    for (entity, button) in &colours {
        let wanted = if properties.colour_by == Some(button.property) {
            "*"
        } else {
            "o"
        };
        for child in children.iter_descendants(entity) {
            if let Ok(mut text) = texts.get_mut(child)
                && text.0 != wanted
            {
                text.0 = wanted.to_string();
            }
        }
    }
}
/// The sidebar section that filters and colours a point cloud by its cell
/// properties.
pub struct CellPanelPlugin;

impl Plugin for CellPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenSections>()
            .add_observer(on_colour_by)
            .add_observer(on_value_toggled)
            .add_observer(on_clear_property)
            .add_observer(on_clear_all)
            .add_systems(
                Update,
                (record_open_sections, drag_range_handles)
                    .chain()
                    .in_set(Stage::ControlsRead),
            )
            .add_systems(Update, rebuild_cell_panel.in_set(Stage::ControlsBuild))
            .add_systems(
                Update,
                (
                    update_property_controls,
                    update_range_controls,
                    update_clear_buttons,
                )
                    .chain()
                    .in_set(Stage::ControlsPlace),
            )
            .add_systems(Update, apply_selection.in_set(Stage::ControlsApply))
            .add_systems(Startup, spawn_cell_panel.in_set(Boot::DockContent));
    }
}
