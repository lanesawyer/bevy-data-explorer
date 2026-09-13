//! The cell properties section of the sidebar.
//!
//! Offered only by sources that advertise [`CellProperties`], so an image never
//! shows it. Each property becomes a sub-section: the button in its header
//! colours points by that property, and the checkboxes inside filter points
//! down to the values still ticked.
//!
//! Which properties are listed is chosen from the section's own menu, in
//! `visibility`; a dataset offers more of them than are worth reading at once.
//!
//! The section is rebuilt whenever the selected source's properties change, so
//! a lookup that fills them in later — over HTTP, from whatever service knows
//! the labels — needs no cooperation from this module.

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::FeathersCheckbox;
use bevy_feathers::display::label_dim;
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::{Activate, ValueChange};

pub mod range;
pub mod visibility;

use crate::app::schedule::{Boot, Stage};
use crate::source::DataSource;
use crate::source::properties::{CellProperties, CellProperty, PropertyKind, PropertyState};
use crate::ui::sidebar::{SectionOrder, SidebarContent};
use crate::view::{BlocksFrameInput, SelectedPanel, ShowsSource};
use crate::widgets::{Accordion, button_text, spawn_accordion, spawn_header_button, spawn_menu};

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
    palette: Res<crate::app::theme::Palette>,
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
                                @caption: { bsn_list![button_text(caption)] }
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
                vec![range::spawn_range_control(
                    &mut commands,
                    index,
                    range,
                    &palette,
                )]
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
    if let Some(property) = properties.properties.get(button.property) {
        info!("colouring by {}", property.name);
    }
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
        info!(
            "{} {}",
            if change.value {
                "filtering to"
            } else {
                "no longer filtering to"
            },
            value.label
        );
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
        info!("cleared the filters on {}", property.name);
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
    let cleared = properties.applied();
    properties.clear_all();
    info!(
        "cleared {cleared} {}",
        if cleared == 1 { "filter" } else { "filters" }
    );
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
            .add_observer(visibility::on_show_toggled)
            .add_systems(
                Update,
                (record_open_sections, range::drag_range_handles)
                    .chain()
                    .in_set(Stage::ControlsRead),
            )
            .add_systems(
                Update,
                (rebuild_cell_panel, visibility::rebuild_visibility_menu)
                    .chain()
                    .in_set(Stage::ControlsBuild),
            )
            .add_systems(
                Update,
                (
                    update_property_controls,
                    visibility::update_property_visibility,
                    range::update_range_controls,
                    update_clear_buttons,
                )
                    .chain()
                    .in_set(Stage::ControlsPlace),
            )
            .add_systems(Startup, spawn_cell_panel.in_set(Boot::DockContent));
    }
}
