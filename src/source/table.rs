//! Rows and columns a source holds, for the frames that show them.
//!
//! Part of the plugin surface like [`hover`] and [`region`]: a format writes
//! this onto its source entity, and the grid draws what is in it without
//! knowing which format produced it.
//!
//! A source carrying one has no space to look around. There is nothing to pan
//! over and nothing finer to zoom into, so its frame is filled with a table
//! that scrolls rather than a view that moves — which is why this is a
//! component the grid reads rather than geometry a format spawns.
//!
//! [`hover`]: super::hover
//! [`region`]: super::region

use bevy::prelude::*;

/// One column: what the header called it, and what the values under it look
/// like.
#[derive(Debug, Clone)]
pub struct TableColumn {
    pub name: String,
    /// The widest value in it, header included, in characters. What the column
    /// is sized from, so a format that has already measured its values does
    /// not make the grid measure them again.
    pub chars: usize,
    /// Every value in it is a number, so it is set flush right the way a
    /// spreadsheet sets one.
    pub numeric: bool,
}

/// A source's records, as rows under a header.
///
/// Held as strings because this is the last stop before they are drawn: a
/// format has already decided how its values read.
#[derive(Component, Debug, Default)]
pub struct SourceTable {
    pub columns: Vec<TableColumn>,
    /// One entry per row, each as wide as `columns`.
    pub rows: Vec<Vec<String>>,
}

impl SourceTable {
    /// What is at `row` and `column`, or nothing if either is past the end.
    ///
    /// A row short of the header is not an error here: a format pads what it
    /// can and the gap is drawn as the empty cell it is.
    pub fn cell(&self, row: usize, column: usize) -> &str {
        self.rows
            .get(row)
            .and_then(|row| row.get(column))
            .map_or("", String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> SourceTable {
        SourceTable {
            columns: ["a", "b"]
                .into_iter()
                .map(|name| TableColumn {
                    name: name.into(),
                    chars: 1,
                    numeric: false,
                })
                .collect(),
            rows: vec![vec!["1".into(), "2".into()], vec!["3".into()]],
        }
    }

    #[test]
    fn a_cell_past_the_end_reads_as_empty_rather_than_panicking() {
        let table = table();
        assert_eq!(table.cell(0, 1), "2");
        // A row shorter than the header, and a row that is not there at all.
        assert_eq!(table.cell(1, 1), "");
        assert_eq!(table.cell(9, 0), "");
    }
}
