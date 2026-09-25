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

use std::cmp::Ordering;

use bevy::prelude::*;

use crate::app::schedule::Stage;
use crate::source::table::{
    ColumnWidths, HiddenColumns, SourceTable, TableColumn, TablePaging, TableSort,
};
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

/// Rows to a page.
///
/// Small enough that a source reading a page at a time fetches it quickly,
/// and that a frame is never holding much; large enough that paging is
/// occasional rather than constant.
pub const PAGE_ROWS: usize = 100;

/// A table, and what to call it.
pub struct Table {
    pub name: String,
    /// What kind of table it is, in the words a listing shows beside it.
    pub detail: String,
    /// What was left out, in the words of whatever read it. Shown in the
    /// frame's status under the shape of the table.
    pub note: Option<String>,
    /// How many rows there are altogether, when `rows` holds only a page of
    /// them. Absent when the whole table was read, which is the usual case
    /// and the one where the rows can be counted.
    pub total: Option<usize>,
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
        rows: Vec<Vec<String>>,
        note: Option<String>,
    ) -> Table {
        Table::paged(name, detail, headers, rows, note, None)
    }

    /// A table whose rows are a page of `total`, read from somewhere that will
    /// be asked again for the next one.
    pub fn paged(
        name: impl Into<String>,
        detail: impl Into<String>,
        headers: Vec<String>,
        mut rows: Vec<Vec<String>>,
        note: Option<String>,
        total: Option<usize>,
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
            total,
            rows: SourceTable {
                first: 0,
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

/// Every row of a table that was read whole, out of sight of the frame.
///
/// The frame draws a page at a time, so the rest waits here until it is asked
/// for. A format that reads a page at a time from somewhere else has no such
/// component; it answers the same question its own way.
#[derive(Component)]
pub struct WholeTable {
    /// In the order they were read, which is what a table goes back to when
    /// it is no longer sorted.
    pub rows: Vec<Vec<String>>,
    /// Where each row falls as the table is sorted now: `rows[order[0]]` is
    /// the first on the first page.
    pub order: Vec<usize>,
}

impl WholeTable {
    fn new(rows: Vec<Vec<String>>) -> Self {
        WholeTable {
            order: (0..rows.len()).collect(),
            rows,
        }
    }
}

/// Put the page a frame is asking for into the rows it draws, in the order
/// the table is sorted.
///
/// Slicing rather than fetching, because this serves the tables that were
/// read whole. The page is clamped here rather than where it is written, so a
/// frame asking past the end lands on the last page instead of an empty one.
fn serve_pages(
    mut tables: Query<
        (
            &mut WholeTable,
            &mut TablePaging,
            &mut SourceTable,
            Ref<TableSort>,
        ),
        Or<(Changed<TablePaging>, Changed<TableSort>)>,
    >,
) {
    for (mut whole, mut paging, mut rows, sort) in &mut tables {
        if sort.is_changed() {
            whole.order = order_of(&whole.rows, &rows.columns, &sort);
        }
        let page = paging.clamped(paging.page);
        if page != paging.page {
            paging.page = page;
        }
        let first = paging.first();
        let wanted: Vec<Vec<String>> = whole
            .order
            .iter()
            .skip(first)
            .take(paging.size)
            .map(|&row| whole.rows[row].clone())
            .collect();
        if rows.first != first || rows.rows != wanted {
            rows.first = first;
            rows.rows = wanted;
        }
    }
}

/// The order `rows` fall in when sorted by `sort`: each key in turn, the next
/// deciding only among rows the ones before leave tied. Rows tied on every
/// key stay in the order they were read.
fn order_of(rows: &[Vec<String>], columns: &[TableColumn], sort: &TableSort) -> Vec<usize> {
    let keys: Vec<(usize, bool, bool)> = sort
        .0
        .iter()
        .filter_map(|key| {
            let at = columns
                .iter()
                .position(|column| column.name == key.column)?;
            Some((at, columns[at].numeric, key.descending))
        })
        .collect();
    let mut order: Vec<usize> = (0..rows.len()).collect();
    let value = |row: usize, at: usize| rows[row].get(at).map_or("", String::as_str);
    order.sort_by(|&a, &b| {
        keys.iter()
            .map(|&(at, numeric, descending)| {
                compare(value(a, at), value(b, at), numeric, descending)
            })
            .find(|ordering| ordering.is_ne())
            .unwrap_or(Ordering::Equal)
    });
    order
}

/// Two values of one column. A gap goes after every value whichever way the
/// column runs, so turning a sort over does not bring the empty rows to the
/// top.
fn compare(a: &str, b: &str, numeric: bool, descending: bool) -> Ordering {
    let (a, b) = (a.trim(), b.trim());
    let ordering = match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ if numeric => {
            let number = |value: &str| value.parse::<f64>().unwrap_or(f64::NAN);
            number(a).total_cmp(&number(b))
        }
        _ => a
            .chars()
            .flat_map(char::to_lowercase)
            .cmp(b.chars().flat_map(char::to_lowercase)),
    };
    if descending {
        ordering.reverse()
    } else {
        ordering
    }
}

/// The systems every table read whole shares.
pub struct TableSystems;

impl Plugin for TableSystems {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, serve_pages.in_set(Stage::Sources));
    }
}

/// Register a table as a source.
///
/// A table read whole keeps the rest of itself in a [`WholeTable`] and draws a
/// page out of it; one that arrived as a page is left as it is, for whatever
/// read it to serve the next page its own way.
pub fn spawn_source(world: &mut World, table: Table) -> Entity {
    let held = table.rows.rows.len();
    let paged = table.total.is_some();
    let (rows, columns) = (table.total.unwrap_or(held), table.rows.columns.len());
    let source = source::register_in(
        world,
        source::SourceInfo {
            name: table.name,
            // A table is not measured in anything. Its rows are records, not
            // a place, which is also why its frame scrolls rather than pans.
            unit: String::new(),
            detail: table.detail,
            stat: format!("{} ROWS", source::compact_count(rows as u64)),
            category: source::Category::Table,
        },
        // Nominal: a table occupies no space, and its frame never looks at the
        // world. It is still registered with one because every source is
        // framed from an extent when its panel opens.
        SourceExtent {
            center: Vec2::ZERO,
            size: Vec2::ONE,
            finest: 1.0,
        },
    );

    // Written out rather than set with a multiplication sign: the overlay is
    // read, not calculated, and one column of one row is still "1 row".
    let mut status = format!(
        "{} {} in {columns} columns",
        source::grouped(rows),
        if rows == 1 { "row" } else { "rows" }
    );
    if let Some(note) = table.note {
        status += &format!(", {note}");
    }

    let mut page = table.rows;
    let paging = TablePaging::new(PAGE_ROWS, Some(rows));
    let mut entity = world.entity_mut(source);
    entity.insert((
        SourceStatus(status),
        paging,
        TableSort::default(),
        HiddenColumns::default(),
        ColumnWidths::default(),
    ));
    if !paged {
        // Everything is already in hand, so the whole table is put aside and
        // the first page of it is what the frame draws.
        let whole = std::mem::take(&mut page.rows);
        page.rows = whole.iter().take(PAGE_ROWS).cloned().collect();
        entity.insert(WholeTable::new(whole));
    }
    entity.insert(page);
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
    fn a_frame_opens_on_the_first_page_and_the_rest_waits() {
        let mut app = App::new();
        let rows: Vec<Vec<String>> = (0..PAGE_ROWS + 30)
            .map(|row| vec![row.to_string()])
            .collect();
        let table = Table::new("t", "test", headers(&["a"]), rows, None);
        let source = spawn_source(app.world_mut(), table);

        let world = app.world();
        let page = world.get::<SourceTable>(source).unwrap();
        assert_eq!(page.rows.len(), PAGE_ROWS);
        assert_eq!(page.first, 0);
        assert_eq!(page.rows[0][0], "0");
        // The rest is held rather than thrown away or drawn.
        assert_eq!(
            world.get::<WholeTable>(source).unwrap().rows.len(),
            PAGE_ROWS + 30
        );
        let paging = world.get::<TablePaging>(source).unwrap();
        assert_eq!(paging.pages(), Some(2));
        assert_eq!(paging.page, 0);
    }

    #[test]
    fn asking_for_a_page_serves_it_out_of_what_was_read() {
        let mut app = App::new();
        app.add_systems(Update, serve_pages);
        let rows: Vec<Vec<String>> = (0..250).map(|row| vec![row.to_string()]).collect();
        let table = Table::new("t", "test", headers(&["a"]), rows, None);
        let source = spawn_source(app.world_mut(), table);

        app.world_mut().get_mut::<TablePaging>(source).unwrap().page = 1;
        app.update();
        let page = app.world().get::<SourceTable>(source).unwrap();
        assert_eq!(page.first, 100);
        assert_eq!(page.rows[0][0], "100");
        assert_eq!(page.rows.len(), 100);

        // The last page is short, and a page past the end lands on it.
        app.world_mut().get_mut::<TablePaging>(source).unwrap().page = 99;
        app.update();
        assert_eq!(app.world().get::<TablePaging>(source).unwrap().page, 2);
        let page = app.world().get::<SourceTable>(source).unwrap();
        assert_eq!(page.first, 200);
        assert_eq!(page.rows.len(), 50);
    }

    #[test]
    fn a_sorted_table_is_served_in_order_from_its_first_page() {
        let mut app = App::new();
        app.add_systems(Update, serve_pages);
        let table = Table::new(
            "t",
            "test",
            headers(&["name", "age"]),
            rows(&[&["b", "9"], &["a", ""], &["C", "10"], &["a", "2"]]),
            None,
        );
        let source = spawn_source(app.world_mut(), table);
        let column = |app: &App, at: usize| -> Vec<String> {
            let page = app.world().get::<SourceTable>(source).unwrap();
            page.rows.iter().map(|row| row[at].clone()).collect()
        };

        // Numbers as numbers, so 10 comes after 9, and the gap last.
        app.world_mut()
            .get_mut::<TableSort>(source)
            .unwrap()
            .press("age", false);
        app.update();
        assert_eq!(column(&app, 1), ["2", "9", "10", ""]);
        // Turned over, the gap stays last.
        app.world_mut()
            .get_mut::<TableSort>(source)
            .unwrap()
            .press("age", false);
        app.update();
        assert_eq!(column(&app, 1), ["10", "9", "2", ""]);

        // Words without regard to case, then the second key among ties.
        let mut sort = app.world_mut().get_mut::<TableSort>(source).unwrap();
        sort.press("name", false);
        sort.press("age", true);
        sort.press("age", true);
        app.update();
        assert_eq!(column(&app, 0), ["a", "a", "b", "C"]);
        assert_eq!(column(&app, 1), ["2", "", "9", "10"]);

        // Unsorted, it is back in the order it was read.
        app.world_mut()
            .get_mut::<TableSort>(source)
            .unwrap()
            .0
            .clear();
        app.update();
        assert_eq!(column(&app, 0), ["b", "a", "C", "a"]);
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
            "1 row in 2 columns, read as far as 1 row"
        );
        let data = world.get::<source::DataSource>(source).unwrap();
        assert_eq!(data.stat, "1 ROWS");
        // Nothing to line a table up against, so it shares space with nothing.
        assert!(data.unit.is_empty());
        assert!(world.get::<SourceTable>(source).is_some());
    }
}
