//! Exporting the row picked out in the inspector, and what is known about it.
//!
//! Each section of the record that holds rows of its own gets a download menu
//! in its header, offering CSV or JSON: Fields writes the row itself, every
//! column the frame hides included; Files the files held about it; Related
//! every record it links to, not only the ones the section has room to list.
//! What is written is read when the button is pressed, so a section whose
//! contents are still arriving exports whatever has arrived.

use bevy::prelude::*;
use bevy_ui_widgets::Activate;

use crate::app::export::{Exports, Format, Table};
use crate::bookmark::store::file_stem;
use crate::source::DataSource;
use crate::source::table::{
    FollowedRecord, Record, RecordFile, RecordFiles, RecordTrail, RelatedRecords, SelectedRecord,
};
use crate::ui::inspector::RecordPart;
use crate::view::SelectedSource;
use crate::widgets::{close_menu_holding, spawn_export_menu};

/// A button exporting `part` of the selected frame's picked record.
#[derive(Component)]
pub struct ExportRecord {
    part: RecordPart,
    format: Format,
}

/// The popup of a record's export menu. Popups are roots rather than children
/// of their button, so these are despawned by hand when the record is rebuilt.
#[derive(Component)]
pub struct RecordExportMenu;

/// A download menu in `header` exporting `part`, for the parts that have rows
/// to write.
pub fn spawn_record_export_menu(commands: &mut Commands, header: Entity, part: RecordPart) {
    let (title, description) = match part {
        RecordPart::Fields => (
            "Export this row",
            "Every column of the row, hidden ones included.",
        ),
        RecordPart::Files => (
            "Export files",
            "The name, kind and address of every file held about this row.",
        ),
        RecordPart::Related => (
            "Export linked records",
            "Every record this row links to, under what links them, including \
             any past those listed here.",
        ),
        RecordPart::Images => return,
    };
    let menu = spawn_export_menu(commands, header, title, description);
    commands.entity(menu.menu).insert(RecordExportMenu);
    for (format, button) in menu.formats {
        commands
            .entity(button)
            .insert(ExportRecord { part, format });
    }
}

fn present(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn fields_table(record: &Record) -> Option<(Vec<String>, Vec<Vec<Option<String>>>)> {
    let fields = record.fields.as_ref()?;
    let columns = fields.iter().map(|(name, _)| name.clone()).collect();
    let row = fields.iter().map(|(_, value)| present(value)).collect();
    Some((columns, vec![row]))
}

fn files_rows(files: &[RecordFile]) -> Vec<Vec<Option<String>>> {
    files
        .iter()
        .map(|file| {
            vec![
                present(&file.name),
                present(&file.kind),
                present(&file.address),
            ]
        })
        .collect()
}

fn related_rows(related: &RelatedRecords) -> Vec<Vec<Option<String>>> {
    let RelatedRecords::Ready(groups) = related else {
        return Vec::new();
    };
    groups
        .iter()
        .flat_map(|group| {
            group.records.iter().map(|record| {
                vec![
                    present(&group.title),
                    present(&record.name),
                    present(&record.detail),
                    record.address.clone(),
                ]
            })
        })
        .collect()
}

fn to_strings<const N: usize>(names: [&str; N]) -> Vec<String> {
    names.map(String::from).to_vec()
}

fn on_export_record(
    activate: On<Activate>,
    buttons: Query<&ExportRecord>,
    selected: SelectedSource,
    sources: Query<(
        &DataSource,
        &SelectedRecord,
        Option<&RecordFiles>,
        Option<&RelatedRecords>,
        Option<(&RecordTrail, &FollowedRecord)>,
    )>,
    mut commands: Commands,
    mut exports: ResMut<Exports>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    close_menu_holding(&mut commands, activate.entity);
    let Some((source, SelectedRecord(Some(record)), files, related, followed)) =
        selected.get(&sources)
    else {
        return;
    };
    // A record followed from the row is exported in the row's place.
    let followed = followed.and_then(|(trail, followed)| Some((trail.0.last()?, followed)));
    let shown = match followed {
        Some((_, FollowedRecord::Ready(fields))) => Record {
            row: record.row,
            fields: Some(fields.clone()),
        },
        Some(_) => return,
        None => record.clone(),
    };
    let record = &shown;
    let named = match followed {
        Some((step, _)) => step.name.clone(),
        None => format!("row-{}", record.row + 1),
    };
    let (columns, rows, what) = match button.part {
        RecordPart::Fields => {
            let Some((columns, rows)) = fields_table(record) else {
                return;
            };
            (columns, rows, "row")
        }
        RecordPart::Files => (
            to_strings(["name", "kind", "address"]),
            files_rows(files.map_or(&[][..], |files| &files.0)),
            "files",
        ),
        RecordPart::Related => (
            to_strings(["link", "name", "detail", "address"]),
            related_rows(related.unwrap_or(&RelatedRecords::None)),
            "linked",
        ),
        RecordPart::Images => return,
    };
    if rows.is_empty() {
        info!("nothing to export for row {}", record.row + 1);
        return;
    }
    exports.save(
        Table {
            title: "Export row".into(),
            stem: format!("{}-{}-{what}", file_stem(&source.name), file_stem(&named)),
            columns,
            rows,
        },
        button.format,
    );
}

pub struct RecordExportPlugin;

impl Plugin for RecordExportPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_export_record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::table::{RelatedGroup, RelatedRecord};

    #[test]
    fn a_row_keeps_its_columns_and_leaves_empty_cells_empty() {
        let record = Record {
            row: 4,
            fields: Some(vec![
                ("id".into(), "S1".into()),
                ("notes".into(), String::new()),
            ]),
        };
        let (columns, rows) = fields_table(&record).unwrap();
        assert_eq!(columns, ["id", "notes"]);
        assert_eq!(rows, vec![vec![Some("S1".into()), None]]);
        assert!(
            fields_table(&Record {
                row: 4,
                fields: None
            })
            .is_none()
        );
    }

    #[test]
    fn linked_records_are_all_written_under_their_link() {
        let related = RelatedRecords::Ready(vec![RelatedGroup {
            title: "Input to".into(),
            records: (0..60)
                .map(|n| RelatedRecord {
                    name: format!("P{n}"),
                    detail: "process".into(),
                    address: None,
                    link: None,
                })
                .collect(),
        }]);
        let rows = related_rows(&related);
        assert_eq!(rows.len(), 60);
        assert_eq!(
            rows[0],
            vec![
                Some("Input to".into()),
                Some("P0".into()),
                Some("process".into()),
                None
            ]
        );
        assert!(related_rows(&RelatedRecords::Fetching).is_empty());
    }
}
