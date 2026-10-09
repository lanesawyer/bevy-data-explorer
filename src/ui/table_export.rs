//! Exporting the page of the selected frame's table that is on screen, from a
//! download menu on the table filters' header.
//!
//! What is written is what the frame shows: the rows of the page it is on, as
//! they are narrowed, sorted and searched, under the columns it draws, in its
//! order. The menu says which rows those are before it is pressed. Nothing is
//! read for it, so it is offered for any table, whatever produced the rows.

use bevy::prelude::*;
use bevy_ui_widgets::Activate;

use crate::app::export::{Exports, Format, Table};
use crate::app::schedule::Stage;
use crate::bookmark::store::file_stem;
use crate::source::table::{HiddenColumns, SourceTable, TablePaging};
use crate::source::{DataSource, grouped};
use crate::view::SelectedSource;
use crate::widgets::{close_menu_holding, set_text, spawn_export_menu};

/// The text saying what the menu will export.
#[derive(Component)]
struct TableExportAbout;

/// A button exporting the selected frame's page in `format`.
#[derive(Component)]
struct ExportTable {
    format: Format,
}

/// The download menu, in the table filters' `header`.
pub fn spawn_table_export_menu(commands: &mut Commands, header: Entity) {
    let menu = spawn_export_menu(commands, header, "Export this page", "");
    commands.entity(menu.description).insert(TableExportAbout);
    for (format, button) in menu.formats {
        commands.entity(button).insert(ExportTable { format });
    }
}

/// The page's rows, counted from one, when the table runs to more than one
/// page; nothing when the page is the whole table.
fn span(table: &SourceTable, paging: Option<&TablePaging>) -> Option<(usize, usize)> {
    let whole = paging.is_none_or(|paging| paging.pages().is_some_and(|pages| pages <= 1));
    (!whole && !table.rows.is_empty()).then(|| (table.first + 1, table.first + table.rows.len()))
}

/// What the menu offers.
fn describe(table: &SourceTable, paging: Option<&TablePaging>, columns: usize) -> String {
    let rows = match (span(table, paging), paging.and_then(|paging| paging.total)) {
        _ if table.rows.is_empty() => return "There are no rows on screen to export.".into(),
        (Some((first, last)), Some(total)) => format!(
            "Rows {} \u{2013} {} of {}",
            grouped(first),
            grouped(last),
            grouped(total)
        ),
        (Some((first, last)), None) => {
            format!("Rows {} \u{2013} {}", grouped(first), grouped(last))
        }
        (None, _) if table.rows.len() == 1 => "The one row".to_string(),
        (None, _) => format!("All {} rows", grouped(table.rows.len())),
    };
    let columns = match columns {
        1 => "the one column shown".to_string(),
        n => format!("the {n} columns shown"),
    };
    format!("{rows}, as filtered, sorted and searched, in {columns}.")
}

/// Which of the table's columns the frame draws, by position.
fn shown_columns(table: &SourceTable, hidden: Option<&HiddenColumns>) -> Vec<usize> {
    (0..table.columns.len())
        .filter(|&at| hidden.is_none_or(|hidden| !hidden.0.contains(&table.columns[at].name)))
        .collect()
}

fn update_table_export_menu(
    selected: SelectedSource,
    sources: Query<(&SourceTable, Option<&TablePaging>, Option<&HiddenColumns>)>,
    about: Query<Entity, With<TableExportAbout>>,
    mut texts: Query<&mut Text>,
) {
    let Some((table, paging, hidden)) = selected.get(&sources) else {
        return;
    };
    let next = describe(table, paging, shown_columns(table, hidden).len());
    for entity in &about {
        if let Ok(text) = texts.get_mut(entity) {
            set_text(text, &next);
        }
    }
}

/// The page's rows under the columns the frame draws, an empty cell left
/// empty.
fn page_rows(table: &SourceTable, hidden: Option<&HiddenColumns>) -> Table {
    let kept = shown_columns(table, hidden);
    let rows = (0..table.rows.len())
        .map(|row| {
            kept.iter()
                .map(|&at| Some(table.cell(row, at).trim()).filter(|cell| !cell.is_empty()))
                .map(|cell| cell.map(str::to_string))
                .collect()
        })
        .collect();
    Table {
        title: "Export this page".into(),
        stem: String::new(),
        columns: kept
            .into_iter()
            .map(|at| table.columns[at].name.clone())
            .collect(),
        rows,
    }
}

fn on_export_table(
    activate: On<Activate>,
    buttons: Query<&ExportTable>,
    selected: SelectedSource,
    sources: Query<(
        &DataSource,
        &SourceTable,
        Option<&TablePaging>,
        Option<&HiddenColumns>,
    )>,
    mut commands: Commands,
    mut exports: ResMut<Exports>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    close_menu_holding(&mut commands, activate.entity);
    let Some((source, table, paging, hidden)) = selected.get(&sources) else {
        return;
    };
    if table.rows.is_empty() {
        info!("no rows on screen to export from {}", source.name);
        return;
    }
    let mut export = page_rows(table, hidden);
    export.stem = match span(table, paging) {
        Some((first, last)) => format!("{}-rows-{first}-{last}", file_stem(&source.name)),
        None => file_stem(&source.name),
    };
    exports.save(export, button.format);
}

pub struct TableExportPlugin;

impl Plugin for TableExportPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_export_table)
            .add_systems(Update, update_table_export_menu.in_set(Stage::Chrome));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::table::TableColumn;

    fn table(first: usize, rows: usize) -> SourceTable {
        let column = |name: &str| TableColumn {
            name: name.into(),
            chars: 1,
            numeric: false,
            hidden_by_default: false,
        };
        SourceTable {
            columns: vec![column("a"), column("b"), column("c")],
            rows: (0..rows)
                .map(|n| vec![n.to_string(), "x".into(), " ".into()])
                .collect(),
            first,
        }
    }

    #[test]
    fn the_offer_names_the_rows_on_screen() {
        let paging = TablePaging {
            page: 2,
            ..TablePaging::new(100, Some(10_901))
        };
        assert_eq!(
            describe(&table(200, 100), Some(&paging), 2),
            "Rows 201 \u{2013} 300 of 10,901, as filtered, sorted and searched, in the 2 \
             columns shown."
        );
        let one = TablePaging::new(100, Some(29));
        assert!(describe(&table(0, 29), Some(&one), 3).starts_with("All 29 rows,"));
        assert!(describe(&table(0, 0), Some(&one), 3).starts_with("There are no rows"));
    }

    #[test]
    fn hidden_columns_are_left_out_and_empty_cells_left_empty() {
        let hidden = HiddenColumns(["b".to_string()].into());
        let page = page_rows(&table(0, 1), Some(&hidden));
        assert_eq!(page.columns, ["a", "c"]);
        assert_eq!(page.rows, vec![vec![Some("0".into()), None]]);
    }
}
