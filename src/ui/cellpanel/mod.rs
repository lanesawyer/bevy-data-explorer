//! The cell properties section of the sidebar.
//!
//! Offered only by sources that advertise [`CellProperties`], so an image never
//! shows it. Each property becomes a sub-section: the button in its header
//! colors points by that property, and the checkboxes inside filter points
//! down to the values still ticked.
//!
//! Which properties are listed is chosen from the section's own menu, in
//! `visibility`; a dataset offers more of them than are worth reading at once.
//!
//! The section is rebuilt whenever the selected source's properties change, so
//! a lookup that fills them in later — over HTTP, from whatever service knows
//! the labels — needs no cooperation from this module.
//!
//! Each value shows the color its points are drawn in and, once a service has
//! counted them, how many cells hold it.
//!
//! Genes are properties too, but are listed in their own section
//! (`ui::genes`), which builds its controls from the same parts.

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersCheckbox};
use bevy_feathers::theme::ThemeTextColor;
use bevy_feathers::tokens;
use bevy_ui_widgets::{Activate, ValueChange};

pub mod range;
pub mod tree;
pub mod values;
pub mod visibility;

use crate::app::schedule::{Boot, Stage};
use crate::catalog::cells::{self, Described};
use crate::source::properties::{
    CellColumns, CellProperties, CellProperty, PropertyKind, PropertyState, PropertyValue,
    Provenance,
};
use crate::source::{DataSource, ShowsSource, compact_count};
use crate::ui::sidebar::{SectionOrder, SidebarContent};
use crate::view::SelectedPanel;
use crate::widgets::{
    Accordion, BlocksFrameInput, Icon, SectionLevel, button_icon, button_text, size,
    spawn_accordion, spawn_header_button, spawn_icon_menu, spawn_menu, spawn_skeleton, text_dim,
};

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

/// The button that colors points by a property.
#[derive(Component, Clone, Default)]
pub struct ColorByButton {
    pub property: usize,
}

/// The button that drops one property's filters.
#[derive(Component, Clone, Default)]
pub struct ClearPropertyButton {
    pub property: usize,
}

/// The button that asks a failed service again.
#[derive(Component, Clone, Default)]
pub struct RetryButton;

/// The button that drops every filter, on the section's own header.
#[derive(Component, Clone, Default)]
pub struct ClearAllButton;

/// A checkbox admitting one value of one property.
#[derive(Component, Clone, Default)]
pub struct ValueCheckbox {
    pub property: usize,
    pub value: usize,
}

/// The count beside one value, filled in once a service has counted it.
#[derive(Component, Clone, Default)]
pub struct ValueCount {
    pub property: usize,
    pub value: usize,
}

/// Below the view configuration, which applies to every source.
const SECTION_ORDER: u32 = 20;

/// The most values listed under one property at once. A whole-brain taxonomy
/// has thousands of clusters, and a checkbox apiece makes the sidebar crawl;
/// its coarser levels, or a search, are the way in.
pub const MAX_VALUE_ROWS: usize = 300;

/// Placeholder rows shown while the properties are on their way, about as
/// tall as the sub-sections that replace them.
const SKELETON_ROWS: usize = 4;
const SKELETON_ROW_PX: f32 = 26.0;

pub fn spawn_cell_panel(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let accordion = spawn_accordion(&mut commands, "Cell properties", true, SectionLevel::Pane);
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
    let clear = spawn_header_button(&mut commands, accordion.header, Icon::FilterX);
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
    sources: Query<(
        &DataSource,
        &CellProperties,
        Has<CellColumns>,
        Has<Described>,
    )>,
    body: Query<Entity, With<CellPanelBody>>,
    mut section: Query<&mut Node, With<CellPanel>>,
    existing: Query<Entity, With<CellPanelContent>>,
    open: Res<OpenSections>,
    mut shown: Local<Option<(Entity, Vec<String>, Provenance, PropertyState, bool)>>,
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

    let Some((entity, (_, properties, has_columns, described))) = source else {
        *shown = None;
        return;
    };

    // Still to be read, or read from the files while a service is asked for
    // the real labels. What the files say would be shown for a moment and then
    // swapped out, so placeholders stand in until the answer is final.
    let loading = properties.state == PropertyState::Pending || (has_columns && !described);

    // Rebuilt only when something visible changed: which source, what it is
    // colored by, and which values are ticked.
    // Only what the sections are built from. Everything a property's controls
    // display — ticks, the color choice, range ends — is written onto the
    // existing entities instead, because rebuilding respawns every checkbox and
    // Feathers draws a checkbox's mark before its styling system has had a
    // frame to hide it, which reads as every box flashing ticked.
    //
    // Where the labels came from is part of it: a service's answer can name
    // the same columns as the placeholders it replaces, with other labels.
    let fingerprint = (
        entity,
        properties
            .cell_properties()
            .map(|(_, property)| property.id.clone())
            .collect(),
        properties.provenance.clone(),
        properties.state.clone(),
        loading,
    );
    if shown.as_ref() == Some(&fingerprint) {
        return;
    }
    *shown = Some(fingerprint);

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    if let PropertyState::Failed(error) = &properties.state {
        let failed = spawn_failed(&mut commands, error);
        commands.entity(body).add_child(failed);
        return;
    }
    if loading {
        let skeleton = spawn_skeleton(&mut commands, SKELETON_ROWS, SKELETON_ROW_PX);
        commands.entity(skeleton).insert(CellPanelContent);
        commands.entity(body).add_child(skeleton);
        return;
    }
    if properties.cell_properties().next().is_none() {
        let message = commands
            .spawn_scene(bsn! {
                CellPanelContent
                text_dim("No properties for this dataset.", size::SMALL)
            })
            .id();
        commands.entity(body).add_child(message);
        return;
    }

    let mut sections = Vec::with_capacity(properties.properties.len());
    for (index, property) in properties.cell_properties() {
        let coloring = properties.color_by == Some(index);
        // Sub-sections start closed, since a property can have many values and
        // all of them open at once would bury the rest of the sidebar. One the
        // user opened stays open across a rebuild.
        let was_open = open.0.get(&index).copied().unwrap_or(false);
        let sub = spawn_accordion(
            &mut commands,
            &property.name,
            was_open,
            // A group rather than a pane: these sit inside the section above
            // them, and a second pane header would not say so.
            SectionLevel::Group,
        );
        commands
            .entity(sub.section)
            .insert((CellPanelContent, PropertySection { property: index }));

        // Clearing sits to the left of the color control, as it does on the
        // section's own header. It hides itself when there is nothing to clear.
        let clear = spawn_header_button(&mut commands, sub.header, Icon::FilterX);
        commands
            .entity(clear)
            .insert(ClearPropertyButton { property: index });

        // A tree colors by one of its levels, so its button opens a menu of
        // them.
        let button = if let Some(tree) = property.tree() {
            let (button, menu) = spawn_icon_menu(&mut commands, sub.header, Icon::Palette, true);
            // The popup is a root of its own, not under the section, so a
            // rebuild has to be told to take it too.
            commands.entity(menu).insert(CellPanelContent);
            tree::fill_color_menu(&mut commands, menu, index, tree, coloring);
            button
        } else {
            spawn_header_button(&mut commands, sub.header, Icon::Palette)
        };
        commands
            .entity(button)
            .insert(ColorByButton { property: index });
        if coloring {
            commands.entity(button).insert(ButtonVariant::Primary);
        }

        let rows = match &property.kind {
            PropertyKind::Categorical(_) | PropertyKind::Tree(_) => {
                values::spawn_values(&mut commands, index, property)
            }
            PropertyKind::Numeric(range) => {
                vec![range::spawn_range_control(
                    &mut commands,
                    index,
                    range,
                    properties.ramp().as_ref().filter(|_| coloring),
                    &palette,
                )]
            }
        };
        commands.entity(sub.body).add_children(&rows);
        sections.push(sub.section);
    }
    commands.entity(body).add_children(&sections);
}

/// One value's row: its checkbox, captioned with the color its points are
/// drawn in and its label, and the count of cells holding it.
///
/// `checkbox` and `count` mark the two for whatever keeps them in sync.
pub fn spawn_value_row(
    commands: &mut Commands,
    value: &PropertyValue,
    checked: bool,
    checkbox: impl Bundle,
    count: impl Bundle,
) -> Entity {
    let caption = value.label.clone();
    let swatch = value.swatch();
    let counted = value.count.map(compact_count).unwrap_or_default();
    let boxed = commands
        .spawn_scene(bsn! {
            @FeathersCheckbox {
                @caption: { bsn_list![
                    (
                        Node {
                            width: { Val::Px(10.0) },
                            height: { Val::Px(10.0) },
                            flex_shrink: { 0.0_f32 },
                            border_radius: { BorderRadius::all(Val::Px(2.0)) },
                        }
                        // The value's own color, not a theme's: it is what
                        // its points are painted in.
                        BackgroundColor({ swatch })
                    ),
                    button_text(caption),
                ] }
            }
            Node { flex_shrink: { 1.0_f32 }, min_width: { Val::ZERO } }
            BlocksFrameInput
        })
        .insert(checkbox)
        .id();
    if checked {
        commands.entity(boxed).insert(Checked);
    }
    let counted = commands
        .spawn_scene(bsn! {
            Text({ counted })
            TextFont { font_size: { FontSize::Px(size::SMALL) } }
            ThemeTextColor({ tokens::TEXT_DIM })
            Node { flex_shrink: { 0.0_f32 } }
        })
        .insert(count)
        .id();
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .add_children(&[boxed, counted])
        .id()
}

/// The note under a branch cut short at [`MAX_VALUE_ROWS`].
pub fn spawn_more_note(commands: &mut Commands, more: usize) -> Entity {
    let note = format!("and {more} more; search to find them");
    commands
        .spawn_scene(bsn! {
            text_dim(note, size::SMALL)
        })
        .id()
}

/// Color points by the property whose header button was pressed. A tree's
/// button only opens its menu of levels; choosing one colors.
pub fn on_color_by(
    activate: On<Activate>,
    buttons: Query<&ColorByButton>,
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
    if !properties
        .properties
        .get(button.property)
        .is_some_and(|property| property.tree().is_none())
    {
        return;
    }
    properties.color_by = Some(button.property);
    if let Some(property) = properties.properties.get(button.property) {
        info!("coloring by {}", property.name);
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
            _ => None,
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

/// Why the properties could not be loaded, and a way to ask again.
fn spawn_failed(commands: &mut Commands, error: &str) -> Entity {
    let message = format!("Could not load properties from {error}");
    commands
        .spawn_scene(bsn! {
            CellPanelContent
            Node {
                flex_direction: { FlexDirection::Column },
                align_items: { AlignItems::Start },
                row_gap: { Val::Px(6.0) },
            }
            Children [
                (
                    text_dim(message, size::SMALL)
                ),
                (
                    @FeathersButton {
                        @caption: { bsn_list![button_icon(Icon::RotateCcw), button_text("Retry")] }
                    }
                    Node { column_gap: { Val::Px(6.0) } }
                    BlocksFrameInput
                    RetryButton
                ),
            ]
        })
        .id()
}

/// Ask the selected source's service about its cells again.
pub fn on_retry(
    activate: On<Activate>,
    buttons: Query<&RetryButton>,
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some((source, mut properties)) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok().map(|found| (shows.0, found)))
    else {
        return;
    };
    info!("retrying the cell properties");
    cells::retry(&mut commands, source, &mut properties);
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

/// Show the clear controls only when they have something to clear.
///
/// Both are the icon alone. A count used to be written into the section's
/// button, but into every `Text` under it — the icon's glyph among them, which
/// then drew the words in the icon font as a row of unrelated icons.
pub fn update_clear_buttons(
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    mut per_property: Query<(&ClearPropertyButton, &mut Node), Without<ClearAllButton>>,
    mut clear_all: Query<&mut Node, (With<ClearAllButton>, Without<ClearPropertyButton>)>,
) {
    let properties = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok());

    for (button, mut node) in &mut per_property {
        let applied = properties
            .and_then(|properties| properties.properties.get(button.property))
            .map_or(0, CellProperty::applied);
        let wanted = if applied > 0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let total = properties.map_or(0, CellProperties::applied);
    for mut node in &mut clear_all {
        let wanted = if total > 0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// Keep each checkbox and color control matching its property, without
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
    mut colors: Query<(&ColorByButton, &mut ButtonVariant)>,
    mut counts: Query<(&ValueCount, &mut Text)>,
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

    // Counts arrive seconds after the labels, and are written in rather than
    // rebuilt for, for the same reason the ticks are.
    for (count, mut text) in &mut counts {
        let wanted = properties
            .properties
            .get(count.property)
            .and_then(|property| property.values().get(count.value))
            .and_then(|value| value.count)
            .map(compact_count)
            .unwrap_or_default();
        if text.0 != wanted {
            text.0 = wanted;
        }
    }

    // The one property coloring the points is the one whose palette is lit.
    for (button, mut variant) in &mut colors {
        let wanted = if properties.color_by == Some(button.property) {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        };
        variant.set_if_neq(wanted);
    }
}
/// The sidebar section that filters and colors a point cloud by its cell
/// properties.
pub struct CellPanelPlugin;

impl Plugin for CellPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenSections>()
            .init_resource::<tree::OpenBranches>()
            .add_observer(on_color_by)
            .add_observer(tree::on_toggle)
            .add_observer(tree::on_node_toggled)
            .add_observer(tree::on_color_level)
            .add_observer(on_value_toggled)
            .add_observer(on_clear_property)
            .add_observer(on_clear_all)
            .add_observer(on_retry)
            .add_observer(visibility::on_show_toggled)
            .add_systems(
                Update,
                (record_open_sections, range::drag_range_handles)
                    .chain()
                    .in_set(Stage::ControlsRead),
            )
            .add_systems(
                Update,
                (
                    rebuild_cell_panel,
                    values::sync_value_lists,
                    visibility::rebuild_visibility_menu,
                    tree::sync_branches,
                )
                    .chain()
                    .in_set(Stage::ControlsBuild),
            )
            .add_systems(
                Update,
                (
                    update_property_controls,
                    visibility::update_property_visibility,
                    range::update_range_controls,
                    tree::update_tree_controls,
                    tree::unveil,
                    update_clear_buttons,
                )
                    .chain()
                    .in_set(Stage::ControlsPlace),
            )
            .add_systems(Startup, spawn_cell_panel.in_set(Boot::DockContent));
    }
}
