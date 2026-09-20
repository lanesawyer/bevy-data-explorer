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
///
/// The rows are the page on screen rather than the whole table. A format that
/// read everything holds the rest out of sight; one that reads a page at a
/// time has only this much. Either way what is here is what is drawn, which is
/// what lets the frame draw both the same way.
#[derive(Component, Debug, Default)]
pub struct SourceTable {
    pub columns: Vec<TableColumn>,
    /// One entry per row, each as wide as `columns`.
    pub rows: Vec<Vec<String>>,
    /// Where `rows[0]` falls in the whole table, counted from zero, so a row
    /// is numbered by its place in the table rather than on the page.
    pub first: usize,
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

/// Where a frame is in a table too long to show at once.
///
/// Another of the questions the grid asks and a format answers. The frame's
/// paging buttons write the page; whatever produced the rows serves it — a
/// format holding the whole table slices it, and one reading a page at a time
/// fetches that page. Neither knows about the buttons, and the buttons know
/// about neither.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TablePaging {
    /// The page on screen, counted from zero.
    pub page: usize,
    /// Rows to a page.
    pub size: usize,
    /// How many rows there are altogether. Absent while a source that reads a
    /// page at a time has not been told, which is what makes the last page
    /// unreachable rather than wrong.
    pub total: Option<usize>,
}

impl TablePaging {
    pub fn new(size: usize, total: Option<usize>) -> Self {
        TablePaging {
            page: 0,
            size: size.max(1),
            total,
        }
    }

    /// Where this page starts in the whole table.
    pub fn first(&self) -> usize {
        self.page * self.size
    }

    /// How many pages there are, at least one, once the total is known.
    pub fn pages(&self) -> Option<usize> {
        self.total.map(|total| total.div_ceil(self.size).max(1))
    }

    /// The last page, once there is a total to count to.
    pub fn last_page(&self) -> Option<usize> {
        self.pages().map(|pages| pages - 1)
    }

    pub fn has_previous(&self) -> bool {
        self.page > 0
    }

    /// Whether there is a page after this one. A source still counting is
    /// taken at its word that there is not, rather than offering a page it
    /// cannot fill.
    pub fn has_next(&self) -> bool {
        self.last_page().is_some_and(|last| self.page < last)
    }

    /// `page`, held inside the table.
    pub fn clamped(&self, page: usize) -> usize {
        match self.last_page() {
            Some(last) => page.min(last),
            None => 0,
        }
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
            first: 0,
        }
    }

    #[test]
    fn a_page_covers_the_rows_it_says_it_does() {
        let paging = TablePaging {
            page: 2,
            size: 100,
            total: Some(1050),
        };
        assert_eq!(paging.first(), 200);
        assert_eq!(paging.pages(), Some(11));
        assert_eq!(paging.last_page(), Some(10));
        assert!(paging.has_previous() && paging.has_next());
    }

    #[test]
    fn a_table_that_fits_on_one_page_has_nowhere_to_go() {
        let paging = TablePaging::new(100, Some(29));
        assert_eq!(paging.pages(), Some(1));
        assert!(!paging.has_previous() && !paging.has_next());
        // An exact fit is one page, not two.
        assert_eq!(TablePaging::new(100, Some(100)).pages(), Some(1));
        assert_eq!(TablePaging::new(100, Some(101)).pages(), Some(2));
    }

    #[test]
    fn an_empty_table_still_has_a_page_to_be_on() {
        let paging = TablePaging::new(100, Some(0));
        assert_eq!(paging.pages(), Some(1));
        assert!(!paging.has_next());
    }

    #[test]
    fn a_source_that_has_not_counted_yet_offers_no_page_to_go_to() {
        // Better than offering one it cannot fill, and it corrects itself the
        // moment a total lands.
        let paging = TablePaging::new(100, None);
        assert_eq!(paging.pages(), None);
        assert!(!paging.has_next());
        assert_eq!(paging.clamped(7), 0);
    }

    #[test]
    fn a_page_past_the_end_lands_on_the_last_one() {
        let paging = TablePaging::new(100, Some(250));
        assert_eq!(paging.clamped(99), 2);
        assert_eq!(paging.clamped(1), 1);
    }

    #[test]
    fn a_page_size_is_never_zero() {
        // It divides, so a zero would be a crash rather than a small page.
        assert_eq!(TablePaging::new(0, Some(10)).size, 1);
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
