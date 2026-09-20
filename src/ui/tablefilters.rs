//! The table filters section of the sidebar: which rows of the selected
//! frame's table are shown.
//!
//! Built from the same pieces as the cell properties above it — a sub-section
//! per column, a checkbox per value with its count beside it, and a button on
//! the header that clears the lot — because they are the same act: narrowing
//! what a frame shows without touching what it is showing.
//!
//! It knows nothing about the Brain Knowledge Platform, or about tables at
//! all beyond [`TableFilters`]. A format writes what its rows can be narrowed
//! by, this ticks values, and that format narrows them. So a second source of
//! rows would be filtered by this section without a line changing here.

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::FeathersCheckbox;
use bevy_ui_widgets::{Activate, ValueChange};

use crate::app::schedule::{Boot, Stage};
use crate::source::ShowsSource;
use crate::source::compact_count;
use crate::source::table::TableFilters;
use crate::ui::sidebar::{SectionOrder, SidebarContent};
use crate::view::SelectedPanel;
use crate::widgets::{
    BlocksFrameInput, Icon, SectionLevel, button_text, size, spawn_accordion, spawn_header_button,
    text_dim,
};

/// Under the cell properties, which is where the same act on a point cloud
/// sits, and above the genes.
const SECTION_ORDER: u32 = 22;

/// The section itself, hidden for a frame with nothing to narrow.
#[derive(Component, Clone, Default)]
pub struct FilterSection;

/// The body the columns are built into.
#[derive(Component, Clone, Default)]
pub struct FilterBody;

/// Anything built into the body, despawned wholesale on a rebuild.
#[derive(Component, Clone, Default)]
pub struct FilterContent;

/// Clears every tick on the selected frame's table.
#[derive(Component, Clone, Default)]
pub struct ClearFiltersButton;

/// One value's checkbox, naming where to write the tick back.
#[derive(Component, Clone, Default)]
pub struct FilterValueBox {
    pub column: usize,
    pub value: usize,
}

pub fn spawn_filter_section(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let accordion = spawn_accordion(&mut commands, "Table filters", true, SectionLevel::Pane);
    commands
        .entity(accordion.section)
        .insert(FilterSection)
        .insert(SectionOrder(SECTION_ORDER))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: Val::Percent(100.0),
            display: Display::None,
            ..default()
        });
    commands.entity(parent).add_child(accordion.section);
    commands.entity(accordion.body).insert(FilterBody);

    let clear = spawn_header_button(&mut commands, accordion.header, Icon::FilterX);
    commands.entity(clear).insert(ClearFiltersButton);
}

/// Which table the sidebar is acting on: the selected frame's, if it has one
/// that can be narrowed.
fn selected(selected: &SelectedPanel, panels: &Query<&ShowsSource>) -> Option<Entity> {
    panels.get(selected.0?).ok().map(|shows| shows.0)
}

/// Rebuild the columns when the selection moves to another table.
///
/// Only then. A tick writes straight into the value it came from, so rebuilding
/// on every change would respawn every checkbox under the pointer — and a
/// Feathers checkbox draws its tick for a frame before it is styled, so they
/// would all flash.
pub fn rebuild_filters(
    mut commands: Commands,
    selection: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    filters: Query<&TableFilters>,
    mut sections: Query<&mut Node, With<FilterSection>>,
    body: Query<Entity, With<FilterBody>>,
    existing: Query<Entity, With<FilterContent>>,
    mut built: Local<Option<(Entity, usize)>>,
) {
    let Ok(body) = body.single() else { return };
    let source = selected(&selection, &panels);
    let table = source.and_then(|source| filters.get(source).ok());

    let wanted = if table.is_some_and(|table| !table.columns.is_empty()) {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut sections {
        if node.display != wanted {
            node.display = wanted;
        }
    }

    // The columns of one table never change, so the source and how many
    // columns it offers is enough to know whether these are the right rows.
    let fingerprint = source.zip(table.map(|table| table.columns.len()));
    if *built == fingerprint {
        return;
    }
    *built = fingerprint;

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let Some(table) = table else { return };

    let mut rows = Vec::new();
    for (index, column) in table.columns.iter().enumerate() {
        let section = spawn_accordion(&mut commands, &column.name, false, SectionLevel::Group);
        commands.entity(section.section).insert(FilterContent);
        for (place, value) in column.values.iter().enumerate() {
            let row = value_row(&mut commands, index, place, &value.label, value.count);
            commands.entity(section.body).add_child(row);
        }
        rows.push(section.section);
    }
    commands.entity(body).add_children(&rows);
}

/// One value: a checkbox naming it, with how many rows hold it beside.
fn value_row(
    commands: &mut Commands,
    column: usize,
    value: usize,
    label: &str,
    count: u64,
) -> Entity {
    let caption = label.to_string();
    let boxed = commands
        .spawn_scene(bsn! {
            @FeathersCheckbox {
                @caption: { bsn_list![button_text(caption)] }
            }
            BlocksFrameInput
            FilterValueBox { column: { column }, value: { value } }
            Node { flex_grow: { 1.0_f32 }, min_width: { Val::Px(0.0) } }
        })
        .id();
    let counted = commands
        .spawn_scene(text_dim(compact_count(count), size::SMALL))
        .id();
    let row = commands
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .id();
    commands.entity(row).add_children(&[boxed, counted]);
    row
}

/// Tick a value, and write it back to the table it narrows.
pub fn on_value_toggled(
    change: On<ValueChange<bool>>,
    boxes: Query<&FilterValueBox>,
    selection: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut filters: Query<&mut TableFilters>,
) {
    let Ok(box_) = boxes.get(change.source) else {
        return;
    };
    let Some(source) = selected(&selection, &panels) else {
        return;
    };
    let Ok(mut filters) = filters.get_mut(source) else {
        return;
    };
    let Some(value) = filters
        .columns
        .get_mut(box_.column)
        .and_then(|column| column.values.get_mut(box_.value))
    else {
        return;
    };
    if value.chosen != change.value {
        value.chosen = change.value;
    }
}

/// Show each checkbox as ticked or not, and offer the clear button only while
/// there is something to clear.
pub fn sync_filter_controls(
    mut commands: Commands,
    selection: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    filters: Query<&TableFilters>,
    boxes: Query<(Entity, &FilterValueBox, Has<Checked>)>,
    mut clear: Query<&mut Node, With<ClearFiltersButton>>,
) {
    let table = selected(&selection, &panels).and_then(|source| filters.get(source).ok());

    let wanted = if table.is_some_and(TableFilters::restricts) {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut clear {
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let Some(table) = table else { return };
    for (entity, box_, checked) in &boxes {
        let chosen = table
            .columns
            .get(box_.column)
            .and_then(|column| column.values.get(box_.value))
            .is_some_and(|value| value.chosen);
        if chosen == checked {
            continue;
        }
        if chosen {
            commands.entity(entity).insert(Checked);
        } else {
            commands.entity(entity).remove::<Checked>();
        }
    }
}

/// Put every column of the selected frame's table back.
pub fn on_clear_pressed(
    activate: On<Activate>,
    buttons: Query<&ClearFiltersButton>,
    selection: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut filters: Query<&mut TableFilters>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(source) = selected(&selection, &panels) else {
        return;
    };
    if let Ok(mut filters) = filters.get_mut(source)
        && filters.restricts()
    {
        filters.clear();
    }
}

/// The sidebar section that narrows a table.
pub struct TableFilterPlugin;

impl Plugin for TableFilterPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_value_toggled)
            .add_observer(on_clear_pressed)
            .add_systems(Startup, spawn_filter_section.in_set(Boot::DockContent))
            .add_systems(Update, rebuild_filters.in_set(Stage::ControlsBuild))
            .add_systems(Update, sync_filter_controls.in_set(Stage::ControlsPlace));
    }
}
