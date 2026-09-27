//! The table filters section of the sidebar: which rows of the selected
//! frame's table are shown.
//!
//! Built from the same pieces as the cell properties above it — a sub-section
//! per column with a button on its header that clears it, the same value rows
//! and search over them, the same range over a histogram, and a button on the
//! section's header that clears the lot — because they are the same act:
//! narrowing what a frame shows without touching what it is showing.
//!
//! Its menu picks which columns the frame draws, as the cell properties' menu
//! picks which properties are listed. So the section is offered for any
//! table, even one with nothing to filter by.
//!
//! It knows nothing about the Brain Knowledge Platform, or about tables at
//! all beyond [`TableFilters`]. A format writes what its rows can be narrowed
//! by, this ticks values, and that format narrows them. So a second source of
//! rows would be filtered by this section without a line changing here.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy_feathers::controls::FeathersCheckbox;
use bevy_ui_widgets::{Activate, ValueChange};

use crate::app::schedule::{Boot, Stage};
use crate::app::theme::Palette;
use crate::source::compact_count;
use crate::source::properties::PropertyValue;
use crate::source::table::{
    HiddenColumns, SourceTable, TableFilter, TableFilterKind, TableFilters, TablePaging,
    to_first_page,
};
use crate::ui::cell_panel::range::{RangeOwner, spawn_range_control};
use crate::ui::cell_panel::values::{SearchedRows, spawn_searched_list};
use crate::ui::cell_panel::{ValueColumn, spawn_value_row};
use crate::ui::sidebar::{SectionFor, SectionOrder, SidebarContent};
use crate::ui::table_partitions::spawn_partition_row;
use crate::view::SelectedSource;
use crate::widgets::space;
use crate::widgets::{
    Accordion, BlocksFrameInput, Icon, SectionLevel, button_text, matches_search, patch_node,
    set_text, size, spawn_accordion, spawn_header_button, spawn_menu, spawn_skeleton, text,
    text_dim,
};

/// Under the cell properties, which is where the same act on a point cloud
/// sits, and above the genes.
const SECTION_ORDER: u32 = 22;

/// Placeholder rows shown while the columns are on their way, as the cell
/// properties show them.
const SKELETON_ROWS: usize = 4;
const SKELETON_ROW_PX: f32 = 26.0;

/// About as tall as the histogram, track and readout a span's placeholder
/// stands in for, so the column does not jump when its numbers land.
const SPAN_SKELETON_PX: f32 = 72.0;

/// Rows standing in for a column's values until they land.
const VALUES_SKELETON_ROWS: usize = 3;

/// The body the columns are built into.
#[derive(Component, Clone, Default)]
pub struct FilterBody;

/// Anything built into the body, despawned wholesale on a rebuild.
#[derive(Component, Clone, Default)]
pub struct FilterContent;

/// A column's own accordion, so opening it can say the column is being looked
/// at — which is what asks for what it holds.
#[derive(Component, Clone, Default)]
pub struct FilterColumn {
    pub column: usize,
}

/// The section's menu, which picks the columns the frame draws.
#[derive(Component, Clone, Default)]
pub struct ColumnMenu;

/// Marks what a rebuild of the menu replaces.
#[derive(Component, Clone, Default)]
pub struct ColumnMenuContent;

/// A checkbox in the menu, drawing or hiding one column by its heading.
#[derive(Component, Clone, Default)]
pub struct ShowColumnCheckbox {
    pub column: String,
}

/// Clears every tick on the selected frame's table.
#[derive(Component, Clone, Default)]
pub struct ClearFiltersButton;

/// Clears one column, on its own header.
#[derive(Component, Clone, Default)]
pub struct ClearColumnButton {
    pub column: usize,
}

/// One value's checkbox, naming where to write the tick back.
#[derive(Component, Clone, Default)]
pub struct FilterValueBox {
    pub column: usize,
    pub value: usize,
}

/// The count beside one value, written in as the table counts it again under
/// the other filters.
#[derive(Component, Clone, Default)]
pub struct FilterValueCount {
    pub column: usize,
    pub value: usize,
}

/// The field a column's values are searched from. On the inner text entity,
/// which holds the [`EditableText`].
#[derive(Component, Clone, Default)]
pub struct FilterSearch;

/// The rows of one column's values, kept to match its search, as the cell
/// properties keep theirs.
#[derive(Component)]
pub struct FilterValueList {
    column: usize,
    rows: SearchedRows,
}

/// What a column is showing in place of its control until what it holds
/// lands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ColumnShows {
    Waiting,
    Control,
    Failed,
}

/// The holder of a column's body, refilled when what it holds lands rather
/// than rebuilding the section — which would respawn every column shut, the
/// one just opened among them.
#[derive(Component)]
pub struct ColumnBody {
    column: usize,
    /// The column's accordion, for whether it is open.
    section: Entity,
    shows: Option<ColumnShows>,
}

pub fn spawn_filter_section(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let accordion = spawn_accordion(&mut commands, "Table filters", true, SectionLevel::Pane);
    commands.entity(accordion.section).insert((
        SectionOrder(SECTION_ORDER),
        SectionFor(|source| source.contains::<SourceTable>()),
    ));
    commands.entity(parent).add_child(accordion.section);
    commands.entity(accordion.body).insert(FilterBody);
    let partitions = spawn_partition_row(&mut commands);
    commands.entity(accordion.body).add_child(partitions);

    let clear = spawn_header_button(&mut commands, accordion.header, Icon::FilterX);
    commands.entity(clear).insert(ClearFiltersButton);

    let menu = spawn_menu(&mut commands, accordion.header);
    commands.entity(menu).insert(ColumnMenu);
}

/// Rebuild the columns when the selection moves to another table, or its
/// columns land.
///
/// Only then. A tick writes straight into the value it came from, and a span's
/// numbers are put into its own column, so rebuilding on every change would
/// respawn every checkbox under the pointer and shut every open column.
pub fn rebuild_filters(
    mut commands: Commands,
    selection: SelectedSource,
    tables: Query<Option<&TableFilters>, With<SourceTable>>,
    body: Query<Entity, With<FilterBody>>,
    existing: Query<Entity, With<FilterContent>>,
    mut built: Local<Option<(Entity, Option<(usize, bool)>)>>,
) {
    let Ok(body) = body.single() else { return };
    let source = selection.entity().filter(|source| tables.contains(*source));
    let filters = source.and_then(|source| tables.get(source).ok().flatten());

    let fingerprint = source.map(|source| {
        (
            source,
            filters.map(|filters| (filters.columns.len(), filters.pending)),
        )
    });
    if *built == fingerprint {
        return;
    }
    *built = fingerprint;

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    if source.is_none() {
        return;
    }
    let Some(table) = filters.filter(|filters| filters.pending || !filters.columns.is_empty())
    else {
        let note = commands
            .spawn_scene(text_dim("Nothing to filter by.", size::SMALL))
            .insert(FilterContent)
            .id();
        commands.entity(body).add_child(note);
        return;
    };

    if table.pending {
        let skeleton = spawn_skeleton(&mut commands, SKELETON_ROWS, SKELETON_ROW_PX);
        commands.entity(skeleton).insert(FilterContent);
        commands.entity(body).add_child(skeleton);
        return;
    }

    let mut rows = Vec::new();
    for (index, column) in table.columns.iter().enumerate() {
        // Shut to begin with: a table offers a dozen columns and some of them
        // hold a value per row, so opening one is how you say which you mean.
        let section = spawn_accordion(&mut commands, &heading(column), false, SectionLevel::Group);
        commands
            .entity(section.section)
            .insert((FilterContent, FilterColumn { column: index }));

        let clear = spawn_header_button(&mut commands, section.header, Icon::FilterX);
        commands
            .entity(clear)
            .insert(ClearColumnButton { column: index });

        // Filled by `sync_column_bodies` as the column is opened and what it
        // holds comes back.
        let body = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    width: Val::Percent(100.0),
                    ..default()
                },
                ColumnBody {
                    column: index,
                    section: section.section,
                    shows: None,
                },
            ))
            .id();
        commands.entity(section.body).add_child(body);
        rows.push(section.section);
    }
    commands.entity(body).add_children(&rows);
}

/// Fill the menu with one checkbox per column of the selected table.
///
/// Rebuilt only when the columns change; which are ticked is written onto the
/// existing boxes by [`sync_column_menu`], so a box is never respawned under
/// the pointer.
pub fn rebuild_column_menu(
    mut commands: Commands,
    selection: SelectedSource,
    tables: Query<(&SourceTable, Option<&HiddenColumns>)>,
    menus: Query<Entity, With<ColumnMenu>>,
    existing: Query<Entity, With<ColumnMenuContent>>,
    mut built: Local<Option<(Entity, Vec<String>)>>,
) {
    let Ok(menu) = menus.single() else { return };
    let table = selection
        .entity()
        .and_then(|source| tables.get(source).ok().map(|table| (source, table)));
    let fingerprint = table.map(|(source, (rows, _))| {
        (
            source,
            rows.columns
                .iter()
                .map(|column| column.name.clone())
                .collect(),
        )
    });
    if *built == fingerprint {
        return;
    }
    *built = fingerprint;

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let Some((_, (rows, hidden))) = table else {
        return;
    };

    let heading = commands
        .spawn_scene(bsn! {
            ColumnMenuContent
            text("Show columns", size::BODY)
            Node { margin: { UiRect::bottom(Val::Px(space::HEADING)) } }
        })
        .id();
    let mut entries = vec![heading];
    for column in &rows.columns {
        let caption = column.name.clone();
        let name = column.name.clone();
        let row = commands
            .spawn_scene(bsn! {
                ColumnMenuContent
                @FeathersCheckbox {
                    @caption: { bsn_list![button_text(caption)] }
                }
                BlocksFrameInput
                ShowColumnCheckbox { column: { name } }
            })
            .id();
        if hidden.is_none_or(|hidden| !hidden.0.contains(&column.name)) {
            commands.entity(row).insert(Checked);
        }
        entries.push(row);
    }
    commands.entity(menu).add_children(&entries);
}

/// Draw or hide the column whose box was ticked.
pub fn on_show_column_toggled(
    change: On<ValueChange<bool>>,
    boxes: Query<&ShowColumnCheckbox>,
    selection: SelectedSource,
    mut hidden: Query<&mut HiddenColumns>,
) {
    let Ok(checkbox) = boxes.get(change.source) else {
        return;
    };
    let Some(mut hidden) = selection
        .entity()
        .and_then(|source| hidden.get_mut(source).ok())
    else {
        return;
    };
    let changed = if change.value {
        hidden.0.remove(&checkbox.column)
    } else {
        hidden.0.insert(checkbox.column.clone())
    };
    if changed {
        info!(
            "{} {} in the table",
            if change.value { "showing" } else { "hiding" },
            checkbox.column
        );
    }
}

/// Keep the menu's ticks matching which columns are drawn.
pub fn sync_column_menu(
    mut commands: Commands,
    selection: SelectedSource,
    hidden: Query<&HiddenColumns>,
    boxes: Query<(Entity, &ShowColumnCheckbox, Has<Checked>)>,
) {
    let Some(hidden) = selection
        .entity()
        .and_then(|source| hidden.get(source).ok())
    else {
        return;
    };
    for (entity, checkbox, checked) in &boxes {
        let shown = !hidden.0.contains(&checkbox.column);
        if shown == checked {
            continue;
        }
        if shown {
            commands.entity(entity).insert(Checked);
        } else {
            commands.entity(entity).remove::<Checked>();
        }
    }
}

/// A column's heading: its name, and how many values it offers. A span has no
/// count to give.
fn heading(column: &TableFilter) -> String {
    match column.listed().len() {
        0 => column.name.clone(),
        listed => format!("{} ({listed})", column.name),
    }
}

/// A column of values: a search once there are enough to need one, over a
/// list that scrolls.
fn spawn_values(commands: &mut Commands, column: usize, count: usize) -> Vec<Entity> {
    spawn_searched_list(commands, count, FilterSearch, None, |rows| {
        FilterValueList { column, rows }
    })
}

/// Match each column's list to what is typed in its search: spawn the rows it
/// newly wants, and hide the ones it no longer does.
pub fn sync_value_lists(
    mut commands: Commands,
    selection: SelectedSource,
    filters: Query<&TableFilters>,
    fields: Query<&EditableText, With<FilterSearch>>,
    mut lists: Query<(Entity, &mut FilterValueList)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let Some(table) = selection.get(&filters) else {
        return;
    };

    for (entity, mut list) in &mut lists {
        let Some(typed) = list.rows.typed(&fields) else {
            continue;
        };
        let Some(column) = table.columns.get(list.column) else {
            continue;
        };

        let values = column.listed();
        let matches: Vec<usize> = values
            .iter()
            .enumerate()
            .filter(|(_, value)| matches_search(typed.trim(), &[&value.label]))
            .map(|(index, _)| index)
            .collect();

        let column = list.column;
        list.rows.show(
            &mut commands,
            entity,
            typed,
            false,
            &matches,
            &mut nodes,
            &mut texts,
            |commands, index| {
                let value = &values[index];
                // A cell property's row, without the parts that only mean
                // something on a point cloud: there is no coloring for a
                // swatch or a bar to follow, so both stay hidden.
                let shown = PropertyValue {
                    code: 0,
                    label: value.label.clone(),
                    reference: None,
                    color: None,
                    count: Some(value.count),
                    selected: value.chosen,
                };
                Some(spawn_value_row(
                    commands,
                    &shown,
                    value.chosen,
                    FilterValueBox {
                        column,
                        value: index,
                    },
                    FilterValueCount {
                        column,
                        value: index,
                    },
                    ValueColumn::default(),
                ))
            },
        );
    }
}

/// Put a column's control in it once what it holds lands — its values, or its
/// span — and a placeholder until it does.
pub fn sync_column_bodies(
    mut commands: Commands,
    palette: Res<Palette>,
    selection: SelectedSource,
    filters: Query<&TableFilters>,
    accordions: Query<&Accordion>,
    mut bodies: Query<(Entity, &mut ColumnBody)>,
) {
    let Some(table) = selection
        .entity()
        .and_then(|source| filters.get(source).ok())
    else {
        return;
    };
    for (entity, mut body) in &mut bodies {
        let Some(column) = table.columns.get(body.column) else {
            continue;
        };
        let open = accordions.get(body.section).is_ok_and(|it| it.open);
        // Opening a column asks for what it holds before this runs, so one
        // open, not waiting and not read is one whose ask failed.
        let wanted = if column.read() {
            ColumnShows::Control
        } else if column.awaiting() || !open {
            ColumnShows::Waiting
        } else {
            ColumnShows::Failed
        };
        if body.shows == Some(wanted) {
            continue;
        }
        body.shows = Some(wanted);
        commands.entity(entity).despawn_related::<Children>();

        let children = match (wanted, &column.kind) {
            (ColumnShows::Control, TableFilterKind::Values(_)) => {
                spawn_values(&mut commands, body.column, column.listed().len())
            }
            (ColumnShows::Control, TableFilterKind::Range(Some(span))) => {
                vec![spawn_range_control(
                    &mut commands,
                    RangeOwner::TableColumn,
                    body.column,
                    span,
                    None,
                    &palette,
                )]
            }
            (ColumnShows::Failed, _) => vec![
                commands
                    .spawn_scene(text_dim(
                        "Could not read this column. Close and reopen it to try again.",
                        size::SMALL,
                    ))
                    .id(),
            ],
            (_, TableFilterKind::Values(_)) => vec![spawn_skeleton(
                &mut commands,
                VALUES_SKELETON_ROWS,
                SKELETON_ROW_PX,
            )],
            _ => vec![spawn_skeleton(&mut commands, 1, SPAN_SKELETON_PX)],
        };
        commands.entity(entity).add_children(&children);
    }
}

/// Tick a value, and write it back to the table it narrows.
pub fn on_value_toggled(
    change: On<ValueChange<bool>>,
    boxes: Query<&FilterValueBox>,
    selection: SelectedSource,
    mut filters: Query<&mut TableFilters>,
    mut pagings: Query<&mut TablePaging>,
) {
    let Ok(box_) = boxes.get(change.source) else {
        return;
    };
    let Some(source) = selection.entity() else {
        return;
    };
    let Ok(mut filters) = filters.get_mut(source) else {
        return;
    };
    let Some(value) = filters
        .columns
        .get_mut(box_.column)
        .and_then(|column| column.listed_mut().get_mut(box_.value))
    else {
        return;
    };
    if value.chosen != change.value {
        value.chosen = change.value;
        to_first_page(pagings.get_mut(source).ok());
    }
}

/// Ask for what a column holds as it is opened.
///
/// A column's values or span cost a round trip or two to work out, so they
/// are asked for when someone opens the column rather than when the table
/// opens: most columns of most tables are never looked at. Only on the opening itself, so an ask that
/// failed is not repeated every frame the column stays open.
pub fn note_open_columns(
    selection: SelectedSource,
    mut filters: Query<&mut TableFilters>,
    sections: Query<(&FilterColumn, Ref<Accordion>)>,
) {
    let Some(source) = selection.entity() else {
        return;
    };
    let Ok(mut filters) = filters.get_mut(source) else {
        return;
    };
    for (section, accordion) in &sections {
        if !accordion.is_changed() || !accordion.open {
            continue;
        }
        let Some(column) = filters.columns.get(section.column) else {
            continue;
        };
        // Written only when it changes: a mutable look marks the whole table
        // changed, and the format takes that as a reason to refetch.
        if !column.read() && !column.awaiting() {
            filters.columns[section.column].want(true);
        }
    }
}

/// Show each checkbox as ticked or not with its count as last counted, and
/// offer each clear button only while there is something for it to clear.
pub fn sync_filter_controls(
    mut commands: Commands,
    selection: SelectedSource,
    filters: Query<&TableFilters>,
    boxes: Query<(Entity, &FilterValueBox, Has<Checked>)>,
    mut counts: Query<(&FilterValueCount, &mut Text)>,
    mut clear_all: Query<&mut Node, (With<ClearFiltersButton>, Without<ClearColumnButton>)>,
    mut clear_column: Query<(&ClearColumnButton, &mut Node), Without<ClearFiltersButton>>,
) {
    let table = selection
        .entity()
        .and_then(|source| filters.get(source).ok());

    let shown = |restricts: bool| {
        if restricts {
            Display::Flex
        } else {
            Display::None
        }
    };
    let wanted = shown(table.is_some_and(TableFilters::restricts));
    for node in &mut clear_all {
        patch_node(node, |node| node.display = wanted);
    }
    for (button, node) in &mut clear_column {
        let wanted = shown(
            table
                .and_then(|table| table.columns.get(button.column))
                .is_some_and(TableFilter::restricts),
        );
        patch_node(node, |node| node.display = wanted);
    }

    let Some(table) = table else { return };
    for (count, text) in &mut counts {
        let Some(value) = table
            .columns
            .get(count.column)
            .and_then(|column| column.listed().get(count.value))
        else {
            continue;
        };
        let wanted = compact_count(value.count);
        set_text(text, &wanted);
    }
    for (entity, box_, checked) in &boxes {
        let chosen = table
            .columns
            .get(box_.column)
            .and_then(|column| column.listed().get(box_.value))
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
    selection: SelectedSource,
    mut filters: Query<&mut TableFilters>,
    mut pagings: Query<&mut TablePaging>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(source) = selection.entity() else {
        return;
    };
    if let Ok(mut filters) = filters.get_mut(source)
        && filters.restricts()
    {
        filters.clear();
        to_first_page(pagings.get_mut(source).ok());
    }
}

/// Put one column back.
pub fn on_clear_column(
    activate: On<Activate>,
    buttons: Query<&ClearColumnButton>,
    selection: SelectedSource,
    mut filters: Query<&mut TableFilters>,
    mut pagings: Query<&mut TablePaging>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(source) = selection.entity() else {
        return;
    };
    let Ok(mut filters) = filters.get_mut(source) else {
        return;
    };
    if filters
        .columns
        .get(button.column)
        .is_some_and(TableFilter::restricts)
    {
        filters.columns[button.column].clear();
        to_first_page(pagings.get_mut(source).ok());
    }
}

/// The sidebar section that narrows a table.
pub struct TableFilterPlugin;

impl Plugin for TableFilterPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_value_toggled)
            .add_observer(on_show_column_toggled)
            .add_observer(on_clear_pressed)
            .add_observer(on_clear_column)
            .add_systems(Startup, spawn_filter_section.in_set(Boot::DockContent))
            .add_systems(Update, note_open_columns.in_set(Stage::ControlsRead))
            .add_systems(
                Update,
                (
                    rebuild_filters,
                    rebuild_column_menu,
                    sync_value_lists,
                    sync_column_bodies,
                )
                    .chain()
                    .in_set(Stage::ControlsBuild),
            )
            .add_systems(
                Update,
                (sync_filter_controls, sync_column_menu).in_set(Stage::ControlsPlace),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::table::TableFilterValue;

    fn value(label: &str) -> TableFilterValue {
        TableFilterValue {
            label: label.into(),
            count: 1,
            chosen: false,
        }
    }

    #[test]
    fn a_column_of_values_says_how_many_it_offers() {
        let column = TableFilter::values("sex", "Sex", vec![value("F"), value("M")]);
        assert_eq!(heading(&column), "Sex (2)");
    }

    #[test]
    fn a_span_or_an_empty_column_has_no_count_to_give() {
        assert_eq!(heading(&TableFilter::range("age", "Age")), "Age");
        assert_eq!(
            heading(&TableFilter::values("x", "Nothing", vec![])),
            "Nothing"
        );
    }
}
