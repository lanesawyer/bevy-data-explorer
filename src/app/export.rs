//! Writing what is on screen out to a file: a table of rows, as CSV or JSON.
//!
//! Whatever asks for an export builds a [`Table`] from what it knows and hands
//! it to [`Exports::save`] in the [`Format`] that was asked for; it never deals
//! with dialogs or files.
//!
//! JSON is an array of objects, one per row, keyed by column name in the
//! table's order. A missing cell is `null` there and empty in CSV.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

use crate::app::net::{Fetching, fetching};
use crate::app::schedule::Stage;

/// Rows to write, under column names that become the CSV header and the JSON
/// keys.
pub struct Table {
    /// What the dialog is titled after, as in "Export colors".
    pub title: String,
    /// The file name the dialog suggests, without an extension.
    pub stem: String,
    pub columns: Vec<&'static str>,
    pub rows: Vec<Vec<Option<String>>>,
}

impl Table {
    pub fn to_csv(&self) -> String {
        let mut out = String::new();
        let header: Vec<_> = self.columns.iter().map(|name| csv_field(name)).collect();
        out.push_str(&header.join(","));
        out.push('\n');
        for row in &self.rows {
            let cells: Vec<_> = row
                .iter()
                .map(|cell| cell.as_deref().map(csv_field).unwrap_or_default())
                .collect();
            out.push_str(&cells.join(","));
            out.push('\n');
        }
        out
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    fn to_format(&self, format: Format) -> String {
        match format {
            Format::Csv => self.to_csv(),
            Format::Json => self.to_json(),
        }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Format {
    Csv,
    #[default]
    Json,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Csv => "CSV",
            Format::Json => "JSON",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Json => "json",
        }
    }
}

impl Serialize for Table {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Row<'a>(&'a [&'static str], &'a [Option<String>]);
        impl Serialize for Row<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut map = serializer.serialize_map(Some(self.0.len()))?;
                for (name, cell) in self.0.iter().zip(self.1) {
                    map.serialize_entry(name, cell)?;
                }
                map.end()
            }
        }
        let mut seq = serializer.serialize_seq(Some(self.rows.len()))?;
        for row in &self.rows {
            seq.serialize_element(&Row(&self.columns, row))?;
        }
        seq.end()
    }
}

/// Quoted only when it has to be, so a plain file stays plain.
fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// The export whose dialog is open, if one is. One at a time: a second
/// request while a dialog is up is dropped rather than stacking dialogs.
#[derive(Resource, Default)]
pub struct Exports {
    task: Option<Fetching<Result<Option<PathBuf>, String>>>,
}

impl Exports {
    /// Ask where to write `table`, and write it there as `format`.
    pub fn save(&mut self, table: Table, format: Format) {
        if self.task.is_some() {
            return;
        }
        self.task = Some(fetching(async move {
            // The dialog blocks until answered, so it gets a thread of its own
            // rather than one of the runtime's.
            tokio::task::spawn_blocking(move || {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(&table.title)
                    .add_filter(format.label(), &[format.extension()])
                    .set_file_name(format!("{}.{}", table.stem, format.extension()))
                    .save_file()
                else {
                    return Ok(None);
                };
                std::fs::write(&path, table.to_format(format))
                    .map(|()| Some(path.clone()))
                    .map_err(|e| format!("writing {}: {e}", path.display()))
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
        }));
    }
}

/// Say where an export went, or why it did not.
fn poll_exports(mut exports: ResMut<Exports>) {
    let Some(outcome) = exports.task.as_mut().and_then(Fetching::take) else {
        return;
    };
    exports.task = None;
    match outcome {
        Ok(Some(path)) => info!("exported to {}", path.display()),
        Ok(None) => {}
        Err(error) => warn!("export failed: {error}"),
    }
}

pub struct ExportPlugin;

impl Plugin for ExportPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Exports>()
            .add_systems(Update, poll_exports.in_set(Stage::ControlsApply));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        Table {
            title: "Export colors".into(),
            stem: "colors".into(),
            columns: vec!["category", "displayName", "refId", "color"],
            rows: vec![
                vec![
                    Some("Class".into()),
                    Some("07 CTX-MGE GABA".into()),
                    Some("L1398FBXBBMPRNUW417".into()),
                    Some("#4c1549".into()),
                ],
                vec![
                    Some("Region".into()),
                    Some("Layer 2, \"upper\"".into()),
                    None,
                    Some("#ffffff".into()),
                ],
            ],
        }
    }

    #[test]
    fn json_keeps_column_order_and_nulls() {
        let json = table().to_json();
        let first = json.find("category").unwrap();
        assert!(first < json.find("displayName").unwrap());
        assert!(json.find("refId").unwrap() < json.find("color").unwrap());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed[0]["refId"], "L1398FBXBBMPRNUW417");
        assert!(parsed[1]["refId"].is_null());
    }

    #[test]
    fn csv_quotes_only_what_needs_it() {
        assert_eq!(
            table().to_csv(),
            "category,displayName,refId,color\n\
             Class,07 CTX-MGE GABA,L1398FBXBBMPRNUW417,#4c1549\n\
             Region,\"Layer 2, \"\"upper\"\"\",,#ffffff\n"
        );
    }
}
