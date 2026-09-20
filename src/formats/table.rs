//! A table of records, whatever read it.
//!
//! Two formats end at rows and columns — a delimited file and a query against
//! the Brain Knowledge Platform — and neither draws anything. So the shape
//! they both produce lives here, along with the one thing they both do with
//! it: register it as a source for [`crate::view::table`] to fill a frame
//! with.
//!
//! Measuring a column is here too, because it is the same question either
//! way: how wide its widest value is, and whether every value in it is a
//! number.

use bevy::prelude::*;

use crate::source::table::{SourceTable, TableColumn};
use crate::source::{self, SourceExtent, SourceStatus};

/// Rows kept, past which a table is read as far as this and says so.
///
/// A ceiling on the strings a table holds, which is what a multi-million row
/// export would otherwise spend a gigabyte on.
pub const MAX_ROWS: usize = 100_000;

/// The widest a column is laid out, in characters. A value longer than this is
/// drawn elided, so one prose column cannot push every column after it off the
/// side of a frame.
pub const MAX_CHARS: usize = 44;

/// A table, and what to call it.
pub struct Table {
    pub name: String,
    /// What kind of table it is, in the words a listing shows beside it.
    pub detail: String,
    /// What was left out, in the words of whatever read it. Shown in the
    /// frame's status under the shape of the table.
    pub note: Option<String>,
    pub rows: SourceTable,
}

impl Table {
    /// Measure `rows` against `headers` and make a table of them.
    ///
    /// A row shorter or longer than the header is padded or trimmed rather
    /// than rejected: one bad record should cost that record's missing values,
    /// not the whole table.
    pub fn new(
        name: impl Into<String>,
        detail: impl Into<String>,
        headers: Vec<String>,
        mut rows: Vec<Vec<String>>,
        note: Option<String>,
    ) -> Table {
        let width = headers.len();
        let mut chars: Vec<usize> = headers
            .iter()
            .map(|name| name.chars().count().min(MAX_CHARS))
            .collect();
        let mut numeric = vec![true; width];

        for row in &mut rows {
            row.resize(width, String::new());
            for (column, value) in row.iter().enumerate() {
                let value = value.trim();
                chars[column] = chars[column].max(value.chars().count()).min(MAX_CHARS);
                if !value.is_empty() && value.parse::<f64>().is_err() {
                    numeric[column] = false;
                }
            }
        }

        let empty = rows.is_empty();
        Table {
            name: name.into(),
            detail: detail.into(),
            note,
            rows: SourceTable {
                columns: headers
                    .into_iter()
                    .zip(chars)
                    .zip(numeric)
                    .map(|((name, chars), numeric)| TableColumn {
                        name,
                        chars,
                        // With no rows to judge by, every column would
                        // otherwise qualify as numbers.
                        numeric: numeric && !empty,
                    })
                    .collect(),
                rows,
            },
        }
    }
}

/// Register a table as a source.
pub fn spawn_source(world: &mut World, table: Table) -> Entity {
    let (rows, columns) = (table.rows.rows.len(), table.rows.columns.len());
    let source = source::register_in(
        world,
        source::SourceInfo {
            name: table.name,
            // A table is not measured in anything. Its rows are records, not
            // a place, which is also why its frame scrolls rather than pans.
            unit: String::new(),
            detail: table.detail,
            stat: format!("{} ROWS", source::compact_count(rows as u64)),
        },
        // Nominal: a table occupies no space, and its frame never looks at the
        // world. It is still registered with one because every source is
        // framed from an extent when its panel opens.
        SourceExtent {
            centre: Vec2::ZERO,
            size: Vec2::ONE,
            finest: 1.0,
        },
    );

    let mut status = format!("{rows} rows \u{00d7} {columns} columns");
    if let Some(note) = table.note {
        status += &format!(", {note}");
    }
    world
        .entity_mut(source)
        .insert((SourceStatus(status), table.rows));
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| row.iter().map(|value| (*value).to_string()).collect())
            .collect()
    }

    fn headers(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn a_column_of_numbers_is_set_flush_right_and_one_of_words_is_not() {
        let table = Table::new(
            "t",
            "test",
            headers(&["label", "count"]),
            rows(&[&["first", "10"], &["second", ""], &["third", "-3.5"]]),
            None,
        );
        assert!(!table.rows.columns[0].numeric);
        // A gap says nothing about the column, so it does not disqualify it.
        assert!(table.rows.columns[1].numeric);
    }

    #[test]
    fn a_header_with_nothing_under_it_has_no_numeric_columns() {
        let table = Table::new("t", "test", headers(&["a", "b"]), Vec::new(), None);
        assert!(!table.rows.columns[0].numeric);
    }

    #[test]
    fn a_column_is_as_wide_as_its_widest_value_up_to_the_cap() {
        let table = Table::new(
            "t",
            "test",
            headers(&["ab", "count"]),
            rows(&[&["longer than the header", "2"]]),
            None,
        );
        assert_eq!(table.rows.columns[0].chars, "longer than the header".len());
        // A header wider than anything under it still fits.
        assert_eq!(table.rows.columns[1].chars, "count".len());

        let wide = Table::new(
            "t",
            "test",
            headers(&["a"]),
            rows(&[&["x".repeat(500).as_str()]]),
            None,
        );
        assert_eq!(wide.rows.columns[0].chars, MAX_CHARS);
    }

    #[test]
    fn a_short_row_is_padded_and_a_long_one_trimmed() {
        let table = Table::new(
            "t",
            "test",
            headers(&["a", "b", "c"]),
            rows(&[&["1"], &["1", "2", "3", "4"]]),
            None,
        );
        assert_eq!(table.rows.rows[0], ["1", "", ""]);
        assert_eq!(table.rows.rows[1], ["1", "2", "3"]);
    }

    #[test]
    fn registering_a_table_says_its_shape_and_what_was_left_out() {
        let mut app = App::new();
        let table = Table::new(
            "Specimens",
            "test table",
            headers(&["a", "b"]),
            rows(&[&["1", "2"]]),
            Some("read as far as 1 row".into()),
        );
        let source = spawn_source(app.world_mut(), table);

        let world = app.world();
        assert_eq!(
            world.get::<SourceStatus>(source).unwrap().0,
            "1 rows \u{00d7} 2 columns, read as far as 1 row"
        );
        let data = world.get::<source::DataSource>(source).unwrap();
        assert_eq!(data.stat, "1 ROWS");
        // Nothing to line a table up against, so it shares space with nothing.
        assert!(data.unit.is_empty());
        assert!(world.get::<SourceTable>(source).is_some());
    }
}
