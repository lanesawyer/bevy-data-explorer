//! Narrowing a table that was read whole.
//!
//! What a table's rows can be narrowed by is worked out once, as it opens: a
//! column of numbers is narrowed by a span, drawn over its histogram, and a
//! column of words by ticking values — unless it holds so many different ones
//! that it is a column of names or identifiers, which a list of checkboxes
//! would not help anyone narrow.
//!
//! Each column is indexed then too: a word becomes its place in the column's
//! list and a number is parsed, so narrowing is a pass over integers and
//! floats rather than over strings. A table is capped at
//! [`super::table::MAX_ROWS`], so that pass is bounded however large the file
//! it came from.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::source::properties::NumericRange;
use crate::source::table::{
    TableColumn, TableFilter, TableFilterKind, TableFilterTerm, TableFilterValue, TableFilters,
};

/// The most different values a column of words may hold and still be offered
/// as a list to tick. Past this it is a column of names or identifiers.
const MAX_VALUES: usize = 1000;

/// Buckets in a numeric column's histogram.
const BUCKETS: usize = 32;

/// A row with nothing in a column of words.
const EMPTY: u32 = u32::MAX;

/// One filtered column, as its rows are read when narrowing.
enum Indexed {
    /// Each row's place in the column's list of values, or [`EMPTY`].
    Values(Vec<u32>),
    /// Each row's number, or NaN where it has none.
    Numbers(Vec<f32>),
}

/// The filtered columns of a table read whole, indexed in the order of its
/// [`TableFilters`], and the terms the rows were last narrowed by.
#[derive(Component)]
pub struct FilterIndex {
    columns: Vec<Indexed>,
    /// What the rows were last narrowed under, so counting again is skipped
    /// when a change to the filters was only to their counts.
    pub(super) applied: Option<Vec<TableFilterTerm>>,
}

/// What `rows` can be narrowed by, and the index to narrow them with.
pub fn offer<'a>(columns: &[TableColumn], rows: &'a [Vec<String>]) -> (TableFilters, FilterIndex) {
    let mut filters = Vec::new();
    let mut indexed = Vec::new();
    for (at, column) in columns.iter().enumerate() {
        let cell = |row: &'a [String]| row.get(at).map_or("", |value| value.trim());
        if column.numeric {
            let numbers: Vec<f32> = rows
                .iter()
                .map(|row| cell(row).parse::<f32>().unwrap_or(f32::NAN))
                .collect();
            let Some(span) = span_of(&numbers) else {
                continue;
            };
            filters.push(TableFilter {
                id: column.name.clone(),
                name: column.name.clone(),
                kind: TableFilterKind::Range {
                    span: Some(span),
                    wanted: false,
                },
            });
            indexed.push(Indexed::Numbers(numbers));
        } else {
            let mut places: HashMap<&str, u32> = HashMap::new();
            let mut labels: Vec<&str> = Vec::new();
            let mut codes = Vec::with_capacity(rows.len());
            for row in rows {
                let value = cell(row);
                if value.is_empty() {
                    codes.push(EMPTY);
                    continue;
                }
                let code = *places.entry(value).or_insert_with(|| {
                    labels.push(value);
                    (labels.len() - 1) as u32
                });
                codes.push(code);
                if labels.len() > MAX_VALUES {
                    break;
                }
            }
            if labels.is_empty() || labels.len() > MAX_VALUES {
                continue;
            }
            // Listed alphabetically, and the codes renumbered to match.
            let mut sorted: Vec<u32> = (0..labels.len() as u32).collect();
            sorted.sort_by_cached_key(|&code| labels[code as usize].to_lowercase());
            let mut renumber = vec![0u32; labels.len()];
            for (place, &code) in sorted.iter().enumerate() {
                renumber[code as usize] = place as u32;
            }
            for code in &mut codes {
                if *code != EMPTY {
                    *code = renumber[*code as usize];
                }
            }
            let mut counts = vec![0u64; labels.len()];
            for &code in &codes {
                if code != EMPTY {
                    counts[code as usize] += 1;
                }
            }
            let values = sorted
                .iter()
                .zip(counts)
                .map(|(&code, count)| TableFilterValue {
                    label: labels[code as usize].to_string(),
                    count,
                    chosen: false,
                })
                .collect();
            filters.push(TableFilter::values(&column.name, &column.name, values));
            indexed.push(Indexed::Values(codes));
        }
    }
    (
        TableFilters::ready(filters),
        FilterIndex {
            columns: indexed,
            applied: None,
        },
    )
}

/// A column's extent and histogram, or nothing if it holds no numbers.
fn span_of(numbers: &[f32]) -> Option<NumericRange> {
    let (low, high) = numbers.iter().filter(|value| !value.is_nan()).fold(
        None,
        |extent: Option<(f32, f32)>, &value| {
            Some(extent.map_or((value, value), |(low, high)| {
                (low.min(value), high.max(value))
            }))
        },
    )?;
    let mut span = NumericRange::full(low, high, vec![0; BUCKETS]);
    let everyone = vec![true; numbers.len()];
    span.histogram = histogram(numbers, &everyone, &span);
    Some(span)
}

/// Counts of `numbers` in `span`'s buckets, among the rows `counted` marks.
fn histogram(numbers: &[f32], counted: &[bool], span: &NumericRange) -> Vec<u32> {
    let buckets = span.histogram.len().max(1);
    let edges = NumericRange::bucket_edges(f64::from(span.low), f64::from(span.high), buckets);
    let (low, width) = (edges[0], edges[buckets] - edges[0]);
    let mut counts = vec![0u32; buckets];
    for (&value, &counted) in numbers.iter().zip(counted) {
        if !counted || value.is_nan() {
            continue;
        }
        let at = ((f64::from(value) - low) / width * buckets as f64) as usize;
        counts[at.min(buckets - 1)] += 1;
    }
    counts
}

/// The rows the filters admit, and each column counted again under them.
pub(super) struct Narrowed {
    pub admitted: Vec<bool>,
    /// For a column of words, how many admitted rows hold each value.
    /// For a column of numbers, its histogram over the rows every *other*
    /// column admits, since a span's own bounds would cut its histogram to
    /// itself and leave nothing to drag out into.
    pub counts: Vec<Recounted>,
}

pub(super) enum Recounted {
    Values(Vec<u64>),
    Histogram(Vec<u32>),
}

/// Narrow the rows by what is chosen in `filters`.
///
/// One pass over the rows, noting which columns turn each one away; a row
/// turned away by one span alone still counts toward that span's histogram.
pub(super) fn narrow(filters: &TableFilters, index: &FilterIndex, rows: usize) -> Narrowed {
    enum Test<'a> {
        None,
        Values(Vec<bool>, &'a [u32]),
        Span(f32, f32, &'a [f32]),
    }
    let tests: Vec<Test> = filters
        .columns
        .iter()
        .zip(&index.columns)
        .map(|(column, indexed)| match (&column.kind, indexed) {
            (TableFilterKind::Values(values), Indexed::Values(codes)) if column.restricts() => {
                Test::Values(values.iter().map(|value| value.chosen).collect(), codes)
            }
            (
                TableFilterKind::Range {
                    span: Some(span), ..
                },
                Indexed::Numbers(numbers),
            ) if span.restricts() => Test::Span(span.from, span.to, numbers),
            _ => Test::None,
        })
        .collect();

    // Which column turned each row away: none, the one, or several.
    const SEVERAL: usize = usize::MAX;
    const NONE: usize = usize::MAX - 1;
    let turned: Vec<usize> = (0..rows)
        .map(|row| {
            let mut by = NONE;
            for (at, test) in tests.iter().enumerate() {
                let admits = match test {
                    Test::None => true,
                    Test::Values(chosen, codes) => {
                        let code = codes[row];
                        code != EMPTY && chosen[code as usize]
                    }
                    Test::Span(from, to, numbers) => {
                        let value = numbers[row];
                        value >= *from && value <= *to
                    }
                };
                if !admits {
                    if by != NONE {
                        return SEVERAL;
                    }
                    by = at;
                }
            }
            by
        })
        .collect();
    let admitted: Vec<bool> = turned.iter().map(|&by| by == NONE).collect();

    let counts = filters
        .columns
        .iter()
        .zip(&index.columns)
        .enumerate()
        .map(|(at, (column, indexed))| match indexed {
            Indexed::Values(codes) => {
                let mut counts = vec![0u64; column.listed().len()];
                for (&code, &admitted) in codes.iter().zip(&admitted) {
                    if admitted && code != EMPTY {
                        counts[code as usize] += 1;
                    }
                }
                Recounted::Values(counts)
            }
            Indexed::Numbers(numbers) => {
                let others: Vec<bool> = turned.iter().map(|&by| by == NONE || by == at).collect();
                Recounted::Histogram(
                    column
                        .span()
                        .map(|span| histogram(numbers, &others, span))
                        .unwrap_or_default(),
                )
            }
        })
        .collect();

    Narrowed { admitted, counts }
}

/// Write counts that changed back into the filters, touching nothing when
/// none did, so the sidebar is not rebuilt for counts it already shows.
pub(super) fn take_counts(mut filters: Mut<TableFilters>, counts: Vec<Recounted>) {
    let differs = filters
        .columns
        .iter()
        .zip(&counts)
        .any(|(column, counted)| match counted {
            Recounted::Values(counts) => column
                .listed()
                .iter()
                .zip(counts)
                .any(|(value, count)| value.count != *count),
            Recounted::Histogram(histogram) => column
                .span()
                .is_some_and(|span| span.histogram != *histogram),
        });
    if !differs {
        return;
    }
    for (column, counted) in filters.columns.iter_mut().zip(counts) {
        match counted {
            Recounted::Values(counts) => {
                for (value, count) in column.listed_mut().iter_mut().zip(counts) {
                    value.count = count;
                }
            }
            Recounted::Histogram(histogram) => {
                if let Some(span) = column.span_mut()
                    && span.histogram.len() == histogram.len()
                {
                    span.histogram = histogram;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, numeric: bool) -> TableColumn {
        TableColumn {
            name: name.into(),
            chars: 1,
            numeric,
        }
    }

    fn rows(rows: &[[&str; 2]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| row.iter().map(|value| (*value).to_string()).collect())
            .collect()
    }

    fn table() -> (Vec<TableColumn>, Vec<Vec<String>>) {
        (
            vec![column("region", false), column("depth", true)],
            rows(&[
                ["cortex", "1"],
                ["striatum", "2"],
                ["Cortex", "3"],
                ["cortex", "4"],
                ["", "10"],
            ]),
        )
    }

    #[test]
    fn words_are_listed_alphabetically_and_numbers_get_a_span() {
        let (columns, rows) = table();
        let (filters, _) = offer(&columns, &rows);
        let labels: Vec<(&str, u64)> = filters.columns[0]
            .listed()
            .iter()
            .map(|value| (value.label.as_str(), value.count))
            .collect();
        assert_eq!(labels, [("cortex", 2), ("Cortex", 1), ("striatum", 1)]);
        let span = filters.columns[1].span().unwrap();
        assert_eq!((span.low, span.high), (1.0, 10.0));
        assert_eq!(span.histogram.iter().sum::<u32>(), 5);
    }

    #[test]
    fn a_column_of_identifiers_is_not_offered_as_a_list() {
        let columns = vec![column("id", false)];
        let rows: Vec<Vec<String>> = (0..=MAX_VALUES).map(|i| vec![format!("x{i}")]).collect();
        let (filters, index) = offer(&columns, &rows);
        assert!(filters.columns.is_empty());
        assert!(index.columns.is_empty());
    }

    #[test]
    fn ticked_values_and_a_span_narrow_the_rows_together() {
        let (columns, rows) = table();
        let (mut filters, index) = offer(&columns, &rows);
        filters.columns[0].listed_mut()[0].chosen = true; // cortex
        filters.columns[1].span_mut().unwrap().to = 3.5;

        let narrowed = narrow(&filters, &index, rows.len());
        assert_eq!(narrowed.admitted, [true, false, false, false, false]);
        let Recounted::Values(counts) = &narrowed.counts[0] else {
            panic!("words are counted by value")
        };
        assert_eq!(counts, &[1, 0, 0]);
        // The span's histogram is of every cortex row, the ones its own
        // bounds turn away included.
        let Recounted::Histogram(histogram) = &narrowed.counts[1] else {
            panic!("numbers are counted into a histogram")
        };
        assert_eq!(histogram.iter().sum::<u32>(), 2);
    }

    #[test]
    fn nothing_chosen_admits_every_row() {
        let (columns, rows) = table();
        let (filters, index) = offer(&columns, &rows);
        let narrowed = narrow(&filters, &index, rows.len());
        assert!(narrowed.admitted.iter().all(|admitted| *admitted));
    }

    #[test]
    fn a_hundred_thousand_rows_narrow_quickly() {
        // The most a table holds. Debug builds are slow, so this is a
        // ceiling on how bad it gets rather than a measure of it.
        let columns: Vec<TableColumn> = (0..20)
            .map(|at| column(&format!("c{at}"), at % 2 == 0))
            .collect();
        let rows: Vec<Vec<String>> = (0..crate::formats::table::MAX_ROWS)
            .map(|row| {
                (0..20)
                    .map(|at| format!("{}", (row * (at + 7)) % 97))
                    .collect()
            })
            .collect();
        let (mut filters, index) = offer(&columns, &rows);
        filters.columns[1].listed_mut()[3].chosen = true;
        filters.columns[0].span_mut().unwrap().to = 40.0;

        let started = std::time::Instant::now();
        let narrowed = narrow(&filters, &index, rows.len());
        assert_eq!(narrowed.admitted.len(), rows.len());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }
}
