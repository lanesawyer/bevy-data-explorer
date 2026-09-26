//! Which of a table's columns a frame draws, and where each one starts.

use super::*;

/// The columns of `table` a frame draws: all but the hidden ones.
pub(super) fn visible_of(table: &SourceTable, hidden: Option<&HiddenColumns>) -> Vec<usize> {
    (0..table.columns.len())
        .filter(|&at| hidden.is_none_or(|hidden| !hidden.0.contains(&table.columns[at].name)))
        .collect()
}

/// Where each drawn column starts, the numbering gutter first, with the full
/// width last.
///
/// Sized from the characters a format already counted rather than from laying
/// the text out, which would mean reacting to the measurement a frame later.
/// The gutter is sized for the *last* row number rather than the page's, since
/// the numbers carry on across pages. A table that sorts keeps room in every
/// heading for the mark saying how. A column dragged to a width keeps it.
pub(super) fn edges_of(
    table: &SourceTable,
    visible: &[usize],
    total: usize,
    sorts: bool,
    widths: Option<&ColumnWidths>,
) -> Vec<f32> {
    let gutter = width_of(total.max(1).to_string().len(), size::SMALL) + PAD_PX * 2.0;
    let mark = if sorts { SORT_MARK_PX } else { 0.0 };
    let mut edges = vec![0.0, gutter];
    for &at in visible {
        let last = edges.last().copied().unwrap_or_default();
        let column = &table.columns[at];
        let width = widths
            .and_then(|widths| widths.0.get(&column.name))
            .map_or_else(
                || width_of(column.chars, size::SECONDARY) + mark + PAD_PX * 2.0,
                |&width| width.max(MIN_COLUMN_PX),
            );
        edges.push(last + width);
    }
    edges
}

/// What a frame's table is laid out from, asked of its source.
pub(super) type LayoutQuery = (
    &'static SourceTable,
    &'static TablePaging,
    Option<&'static HiddenColumns>,
    Has<TableSort>,
    Option<&'static ColumnWidths>,
);

/// The answer: the rows, the paging, what is hidden, whether it sorts, and
/// what has been dragged wider or narrower.
pub(super) type Layout<'a> = (
    &'a SourceTable,
    &'a TablePaging,
    Option<&'a HiddenColumns>,
    bool,
    Option<&'a ColumnWidths>,
);

/// The columns drawn and where each starts.
pub(super) fn layout_of((table, paging, hidden, sorts, widths): Layout) -> (Vec<usize>, Vec<f32>) {
    let visible = visible_of(table, hidden);
    let edges = edges_of(
        table,
        &visible,
        paging.total.unwrap_or_default(),
        sorts,
        widths,
    );
    (visible, edges)
}

/// Where the `drawn`th column starts and how wide it is.
pub(super) fn column_span(edges: &[f32], drawn: usize) -> (f32, f32) {
    (edges[drawn + 1], edges[drawn + 2] - edges[drawn + 1])
}

/// As much of a heading's name as fits a column `width` wide.
pub(super) fn heading_text(name: &str, width: f32, sorts: bool) -> String {
    let mark = if sorts { SORT_MARK_PX } else { 0.0 };
    truncate_to_width(name.trim(), width - mark - PAD_PX * 2.0, size::SECONDARY)
}

/// A table of `rows` rows under `columns`, each named, as wide in characters
/// as given, and numeric or not.
#[cfg(test)]
pub(super) fn table(rows: usize, columns: &[(&str, usize, bool)]) -> SourceTable {
    SourceTable {
        columns: columns
            .iter()
            .map(|(name, chars, numeric)| crate::source::table::TableColumn {
                name: (*name).to_string(),
                chars: *chars,
                numeric: *numeric,
            })
            .collect(),
        rows: (0..rows)
            .map(|row| columns.iter().map(|_| row.to_string()).collect())
            .collect(),
        first: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_is_wide_enough_for_its_widest_value() {
        let table = table(3, &[("a", 10, false), ("b", 4, true)]);
        let edges = edges_of(&table, &[0, 1], 3, false, None);
        // The numbering gutter, then a column apiece, then the whole width.
        assert_eq!(edges.len(), 3 + 1);
        assert!(edges[0] < edges[1] && edges[1] < edges[2] && edges[2] < edges[3]);
        let first = edges[2] - edges[1];
        assert!(truncate_to_width("0123456789", first - PAD_PX * 2.0, size::SECONDARY).len() == 10);
    }

    #[test]
    fn a_hidden_column_is_left_out_and_the_rest_close_up() {
        let table = table(3, &[("a", 4, false), ("b", 10, false), ("c", 4, false)]);
        let hidden = HiddenColumns(["b".to_string()].into());
        let visible = visible_of(&table, Some(&hidden));
        assert_eq!(visible, [0, 2]);
        let edges = edges_of(&table, &visible, 3, false, None);
        assert_eq!(edges, edges_of(&table, &[0, 0], 3, false, None));
    }

    #[test]
    fn a_dragged_column_keeps_its_width_and_the_rest_move_over() {
        let table = table(3, &[("a", 4, false), ("b", 10, false)]);
        let fitted = edges_of(&table, &[0, 1], 3, false, None);
        let widths = ColumnWidths([("a".to_string(), 200.0)].into());
        let dragged = edges_of(&table, &[0, 1], 3, false, Some(&widths));
        assert!((dragged[2] - dragged[1] - 200.0).abs() < 1e-3);
        assert!(((dragged[3] - dragged[2]) - (fitted[3] - fitted[2])).abs() < 1e-3);
    }

    #[test]
    fn a_table_that_sorts_keeps_room_in_its_headings_for_the_mark() {
        let table = table(3, &[("a", 4, false)]);
        let plain = edges_of(&table, &[0], 3, false, None);
        let sorting = edges_of(&table, &[0], 3, true, None);
        let grown = (sorting[2] - sorting[1]) - (plain[2] - plain[1]);
        assert!((grown - SORT_MARK_PX).abs() < 1e-3);
    }

    #[test]
    fn the_gutter_is_wide_enough_for_the_last_row_number() {
        // Sized for the last row number in the whole table, not the page's:
        // a page of 100 rows can still be numbering row 10,901.
        let table = table(9, &[("a", 4, false)]);
        let few = edges_of(&table, &[0], 9, false, None);
        let many = edges_of(&table, &[0], 10_901, false, None);
        assert!(many[1] > few[1], "a five-digit number needs more room");
    }

    #[test]
    fn a_table_with_no_rows_still_lays_out_its_columns() {
        let edges = edges_of(&table(0, &[("a", 4, false)]), &[0], 0, false, None);
        assert_eq!(edges.len(), 3);
        assert!(edges[2] > edges[1]);
        assert_eq!(rows_in_view(0.0, 400.0, 0), 0..0);
    }
}
