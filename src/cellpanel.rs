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

use crate::cellproperties::{CellProperties, NumericRange, PropertyKind, PropertyState, RangeEnd};
use crate::datasource::DataSource;
use crate::panel::{BlocksFrameInput, SelectedPanel, ShowsSource};
use crate::sidebar::SidebarContent;
use crate::widgets::{Accordion, spawn_accordion, spawn_header_button};

/// The section itself, hidden for sources with no properties to show.
#[derive(Component, Clone, Default)]
pub struct CellPanel;

/// Everything the section builds, so a rebuild can clear what it made.
#[derive(Component, Clone, Default)]
pub struct CellPanelContent;

/// The button that colours points by a property.
#[derive(Component, Clone, Default)]
pub struct ColourByButton {
    pub property: usize,
}

/// A checkbox admitting one value of one property.
#[derive(Component, Clone, Default)]
pub struct ValueCheckbox {
    pub property: usize,
    pub value: usize,
}

pub fn spawn_cell_panel(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let accordion = spawn_accordion(&mut commands, "Cell properties", true);
    commands
        .entity(accordion.section)
        .insert(CellPanel)
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: Val::Percent(100.0),
            display: Display::None,
            ..default()
        });
    commands.entity(parent).add_child(accordion.section);
    commands.entity(accordion.body).insert(CellPanelBody);
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
    mut shown: Local<Option<(Entity, usize, Vec<bool>, Vec<(u16, u16)>)>>,
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
    let ticks: Vec<bool> = properties
        .properties
        .iter()
        .flat_map(|property| property.values().iter().map(|value| value.included))
        .collect();
    // Range ends are part of what is drawn, so a drag has to rebuild too. They
    // are quantised: a rebuild per pixel of drag would be wasted work, and the
    // histogram cannot show a finer distinction than its buckets anyway.
    let spans: Vec<(u16, u16)> = properties
        .properties
        .iter()
        .filter_map(|property| property.range())
        .map(|range| {
            (
                (range.fraction_of(range.from) * 200.0) as u16,
                (range.fraction_of(range.to) * 200.0) as u16,
            )
        })
        .collect();
    let fingerprint = (
        entity,
        properties.colour_by.unwrap_or(usize::MAX),
        ticks,
        spans,
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
                    if value.included {
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
    mut commands: Commands,
    checkboxes: Query<&ValueCheckbox>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(checkbox) = checkboxes.get(change.source) else {
        return;
    };
    // Feathers leaves the widget's own state to the app, as it does for
    // sliders.
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }

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
        value.included = change.value;
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
    mut points: Query<&mut crate::pointcloud::PointStreamer>,
    mut slices: Option<ResMut<crate::slices::SliceStreamer>>,
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
pub struct RangeReadout;

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
                    Node {
                        flex_grow: { 1.0_f32 },
                        height: { Val::Px(height) },
                        margin: { UiRect::horizontal(Val::Px(0.5)) },
                    }
                    BackgroundColor({ if inside {
                        Color::srgb(0.38, 0.60, 0.90)
                    } else {
                        Color::srgb(0.22, 0.25, 0.31)
                    } })
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
                Button
                BlocksFrameInput
                RangeHandle { property: { property }, end: { end } }
                Node {
                    position_type: { PositionType::Absolute },
                    width: { Val::Px(RANGE_THUMB_PX) },
                    height: { Val::Px(RANGE_THUMB_PX) },
                    // Placed by percentage with a nudge back, so both ends stay
                    // on the rail without depending on its measured width.
                    left: { Val::Percent(fraction * 100.0) },
                    margin: { UiRect::left(Val::Px(-RANGE_THUMB_PX * fraction)) },
                    border_radius: { BorderRadius::all(Val::Px(RANGE_THUMB_PX * 0.5)) },
                }
                BackgroundColor({ Color::srgb(0.85, 0.89, 0.95) })
            })
            .id();
        commands.entity(track).add_child(handle);
    }

    let readout = commands
        .spawn_scene(bsn! {
            RangeReadout
            CellPanelContent
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
    tracks: Query<(&ComputedNode, &UiGlobalTransform), With<RangeTrack>>,
    parents: Query<&ChildOf>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
    mut dragging: Local<Option<(Entity, usize, RangeEnd)>>,
) {
    if !mouse.pressed(MouseButton::Left) {
        *dragging = None;
        return;
    }

    if dragging.is_none() {
        // Grabbed on the way down, and held until release, so the pointer may
        // leave the handle without dropping the drag.
        let grabbed = hover
            .values()
            .flat_map(|hits| hits.keys())
            .find_map(|entity| handles.get(*entity).ok().map(|handle| (*entity, handle)));
        let Some((entity, handle)) = grabbed else {
            return;
        };
        *dragging = Some((entity, handle.property, handle.end));
    }
    let Some((entity, property, end)) = *dragging else {
        return;
    };

    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    // The handle's track is the one it hangs from.
    let Some((node, transform)) = parents
        .iter_ancestors(entity)
        .find_map(|ancestor| tracks.get(ancestor).ok())
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
