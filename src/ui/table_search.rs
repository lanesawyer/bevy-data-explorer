//! Searching a table, at the top of its filters.
//!
//! Offered only for a table whose rows can all be searched — a source
//! carrying [`TableSearch`] — and never for one it could only search a page
//! of. What is typed is handed over once typing settles, since
//! each search is a request, and the table goes back to its first page as a
//! tick does. Under the field, the columns it looks in are named, since one
//! source searches names alone and another every column, and then how it
//! matches.
//!
//! Like the filters it names no format: this writes the text, and whatever
//! produced the rows searches for it.

use bevy::prelude::*;
use bevy::text::EditableText;

use crate::app::schedule::Stage;
use crate::source::table::{SearchedColumns, SourceTable, TablePaging, TableSearch, to_first_page};
use crate::view::SelectedSource;
use crate::widgets::{field_well, patch_node, set_text, size, space, spawn_search_field, text_dim};

/// How long typing must stop before what is typed is searched for.
const SETTLE_SECS: f32 = 0.4;

/// The row the field is built into, hidden unless the table can be searched.
#[derive(Component, Clone, Default)]
pub struct SearchRow;

/// The field's inner text entity.
#[derive(Component, Clone, Default)]
struct TableSearchInput;

/// Which columns the search looks in and how it matches, under the field.
#[derive(Component, Clone, Default)]
struct SearchAbout;

/// The row a table's filters put under the choice of kind, for their section
/// to place.
pub fn spawn_search_row(commands: &mut Commands) -> Entity {
    let search = spawn_search_field(commands, "Search the table");
    commands.entity(search.field).insert(TableSearchInput);
    // The field would not show against the pane's body on its own.
    let well = commands.spawn_scene(field_well()).id();
    commands.entity(well).add_child(search.entry);
    let about = commands
        .spawn_scene(text_dim("", size::SMALL))
        .insert(SearchAbout)
        .id();
    commands
        .spawn((
            SearchRow,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                width: Val::Percent(100.0),
                row_gap: Val::Px(space::STACKED),
                margin: UiRect::bottom(Val::Px(space::ROWS)),
                ..default()
            },
        ))
        .add_children(&[well, about])
        .id()
}

/// Where the field stands against the selected table's search.
#[derive(Default)]
struct Typing {
    source: Option<Entity>,
    /// The text last put into the field, or taken from it.
    synced: String,
    /// What the field held last frame, and since when.
    typed: String,
    since: f32,
}

/// Hand what is typed to the selected table once typing settles, and put a
/// table's own search into the field when another is selected or a bookmark
/// changes it, so one table's search never carries over to the next.
fn read_table_search(
    selected: SelectedSource,
    mut sources: Query<(&mut TableSearch, Option<&mut TablePaging>)>,
    mut fields: Query<&mut EditableText, With<TableSearchInput>>,
    time: Res<Time>,
    mut typing: Local<Typing>,
) {
    let Ok(mut field) = fields.single_mut() else {
        return;
    };
    let now = time.elapsed_secs();
    let source = selected.entity().filter(|it| sources.contains(*it));
    let Some((mut search, paging)) = source.and_then(|it| sources.get_mut(it).ok()) else {
        typing.source = None;
        return;
    };
    if typing.source != source || search.text != typing.synced {
        if field.value().to_string() != search.text {
            field.editor_mut().set_text(&search.text);
        }
        *typing = Typing {
            source,
            synced: search.text.clone(),
            typed: search.text.clone(),
            since: now,
        };
        return;
    }
    let typed = field.value().to_string();
    if typed != typing.typed {
        typing.typed = typed;
        typing.since = now;
        return;
    }
    if typed != search.text && now - typing.since >= SETTLE_SECS {
        search.text.clone_from(&typed);
        typing.synced = typed;
        to_first_page(paging);
    }
}

/// What is written under the field: the columns searched, by heading, then
/// how they are matched.
fn about(search: &TableSearch, table: Option<&SourceTable>) -> String {
    let headings: Vec<&str> = match &search.columns {
        SearchedColumns::Only(columns) => columns.iter().map(String::as_str).collect(),
        SearchedColumns::All => table
            .map(|table| table.columns.iter().map(|it| it.name.as_str()).collect())
            .unwrap_or_default(),
    };
    let searches = match (&search.columns, headings.len()) {
        (SearchedColumns::All, 0) => "Searches every column.".to_string(),
        (SearchedColumns::All, 1) => format!("Searches its one column: {}.", headings[0]),
        (SearchedColumns::All, count) => {
            format!("Searches all {count} columns: {}.", headings.join(", "))
        }
        (SearchedColumns::Only(_), _) => format!("Searches {}.", headings.join(", ")),
    };
    if search.how.is_empty() {
        searches
    } else {
        format!("{searches} {}", search.how)
    }
}

/// Show the field only for a table that can be searched, saying what it
/// looks in.
fn sync_search_row(
    selected: SelectedSource,
    sources: Query<(&TableSearch, Option<&SourceTable>)>,
    mut rows: Query<&mut Node, With<SearchRow>>,
    mut abouts: Query<&mut Text, With<SearchAbout>>,
) {
    let search = selected.get(&sources);
    for node in &mut rows {
        let wanted = if search.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        patch_node(node, |node| node.display = wanted);
    }
    if let Some((search, table)) = search {
        let wanted = about(search, table);
        for text in &mut abouts {
            set_text(text, &wanted);
        }
    }
}

pub struct TableSearchPlugin;

impl Plugin for TableSearchPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, read_table_search.in_set(Stage::ControlsRead))
            .add_systems(Update, sync_search_row.in_set(Stage::ControlsPlace));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::table::TableColumn;

    fn table(headings: &[&str]) -> SourceTable {
        SourceTable {
            columns: headings
                .iter()
                .map(|name| TableColumn {
                    name: name.to_string(),
                    chars: 1,
                    numeric: false,
                    hidden_by_default: false,
                })
                .collect(),
            ..default()
        }
    }

    #[test]
    fn the_columns_searched_are_named_before_how() {
        let names = TableSearch::new(
            SearchedColumns::Only(vec!["Name".into()]),
            "Anywhere in it.",
        );
        assert_eq!(
            about(&names, Some(&table(&["Name", "Type"]))),
            "Searches Name. Anywhere in it."
        );
        let every = TableSearch::new(SearchedColumns::All, "");
        assert_eq!(
            about(&every, Some(&table(&["species", "region"]))),
            "Searches all 2 columns: species, region."
        );
        assert_eq!(about(&every, None), "Searches every column.");
    }
}
