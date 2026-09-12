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

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::FeathersCheckbox;
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::{Activate, ValueChange};

use crate::cellproperties::{CellProperties, PropertyState};
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
    mut shown: Local<Option<(Entity, usize, Vec<bool>)>>,
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
        .flat_map(|property| property.values.iter().map(|value| value.included))
        .collect();
    let fingerprint = (entity, properties.colour_by.unwrap_or(usize::MAX), ticks);
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

        let mut rows = Vec::with_capacity(property.values.len());
        for (position, value) in property.values.iter().enumerate() {
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
            rows.push(row);
        }
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
        .and_then(|property| property.values.get_mut(checkbox.value))
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
    sources: Query<(Entity, &CellProperties), Changed<CellProperties>>,
    mut points: Option<ResMut<crate::pointcloud::PointStreamer>>,
    mut slices: Option<ResMut<crate::slices::SliceStreamer>>,
) {
    for (entity, properties) in &sources {
        let selection = properties.selection();

        if let Some(streamer) = points.as_mut()
            && streamer.source == entity
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
