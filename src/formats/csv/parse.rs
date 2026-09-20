//! Reading delimited text into rows and columns.
//!
//! No Bevy types: this is the half that turns bytes into a table, and the
//! layout and drawing sit on top of it.
//!
//! The dialect is the one every exporter agrees on — RFC 4180 with the tab
//! allowed in place of the comma — because a table is read from whatever a
//! portal happened to hand out. Quoted fields may hold the delimiter, a
//! doubled quote, or a newline; a ragged row is padded or trimmed to the
//! header rather than rejected, since one bad line should not lose the file.

use crate::formats::table::{MAX_ROWS, Table};

/// Read `text` as delimited text, or say why it is not.
pub fn parse(name: &str, text: &str) -> Result<Table, String> {
    let delimiter = delimiter_of(text);
    let mut records = records(text, delimiter).into_iter().filter(|record| {
        // A blank line is nothing, not a row of empty cells.
        record.iter().any(|field| !field.trim().is_empty())
    });

    let header = records.next().ok_or("the file holds no rows")?;
    if header.len() < 2 {
        return Err(format!(
            "the first line holds one field, so nothing separates its columns: {}",
            header.first().map_or("", String::as_str)
        ));
    }

    let headers: Vec<String> = header
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            if name.trim().is_empty() {
                format!("column {}", index + 1)
            } else {
                name
            }
        })
        .collect();

    let mut rows = Vec::new();
    let mut skipped = 0;
    for record in records {
        if rows.len() == MAX_ROWS {
            skipped += 1;
            continue;
        }
        rows.push(record);
    }

    let kind = if delimiter == '\t' {
        "Tab-separated table"
    } else {
        "Comma-separated table"
    };
    let note = (skipped > 0).then(|| format!("{skipped} rows past the limit not read"));
    Ok(Table::new(
        name,
        format!("{kind}, {} columns", headers.len()),
        headers,
        rows,
        note,
    ))
}

/// Which character separates the fields, decided from the first line.
///
/// Whichever of the two appears more often in the header, since a header is
/// the one line that holds every column and the least likely to hold prose. A
/// file with neither is one column wide, which [`parse`] refuses.
fn delimiter_of(text: &str) -> char {
    let header = text.lines().next().unwrap_or_default();
    if header.matches('\t').count() > header.matches(',').count() {
        '\t'
    } else {
        ','
    }
}

/// Split `text` into records of fields, honoring quotes.
///
/// A quote opens only at the start of a field, which is what lets a stray
/// quote inside an unquoted value — an inch mark, most often — stay a
/// character rather than swallowing the rest of the file.
fn records(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            '\r' => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            c if c == delimiter => record.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    // A file that does not end in a newline still ends in a row.
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn genes() -> Table {
        parse("genes", include_str!("../../../testdata/genes.csv")).unwrap()
    }

    /// What is at `row` and `column`. Only the tests read a table this way:
    /// what is parsed goes straight to the grid.
    fn cell(table: &Table, row: usize, column: usize) -> &str {
        table.rows.cell(row, column)
    }

    fn names(table: &Table) -> Vec<&str> {
        table
            .rows
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect()
    }

    #[test]
    fn a_header_names_the_columns_and_the_rest_are_rows() {
        let table = genes();
        assert_eq!(
            names(&table),
            [
                "gene_identifier",
                "gene_symbol",
                "transcript_identifier",
                "name",
                "mapped_ncbi_identifier"
            ]
        );
        assert_eq!(table.rows.rows.len(), 6);
        assert_eq!(cell(&table, 0, 1), "Prkcq");
    }

    #[test]
    fn a_quoted_field_keeps_the_delimiter_inside_it() {
        // The one thing a naive split gets wrong, and the reason this fixture
        // is real data rather than invented.
        assert_eq!(cell(&genes(), 0, 3), "protein kinase C, theta");
    }

    #[test]
    fn a_doubled_quote_inside_a_quoted_field_is_one_quote() {
        let table = parse("t", "a,b\n\"he said \"\"no\"\"\",2\n").unwrap();
        assert_eq!(cell(&table, 0, 0), "he said \"no\"");
    }

    #[test]
    fn a_newline_inside_a_quoted_field_does_not_end_the_row() {
        let table = parse("t", "a,b\n\"one\ntwo\",2\n").unwrap();
        assert_eq!(table.rows.rows.len(), 1);
        assert_eq!(cell(&table, 0, 0), "one\ntwo");
    }

    #[test]
    fn a_quote_partway_through_a_field_is_just_a_character() {
        let table = parse("t", "a,b\n6\" pipe,2\n").unwrap();
        assert_eq!(cell(&table, 0, 0), "6\" pipe");
        assert_eq!(cell(&table, 0, 1), "2");
    }

    #[test]
    fn tabs_separate_a_file_written_with_tabs() {
        let table = parse("t", "a\tb\tc\n1\t2\t3\n").unwrap();
        assert_eq!(table.rows.columns.len(), 3);
        assert!(table.detail.starts_with("Tab-separated"));
        assert_eq!(cell(&table, 0, 2), "3");
    }

    #[test]
    fn a_short_row_is_padded_and_a_long_one_trimmed() {
        // One bad line should cost that line's missing values, not the file.
        let table = parse("t", "a,b,c\n1\n1,2,3,4\n").unwrap();
        assert_eq!(table.rows.rows[0], ["1", "", ""]);
        assert_eq!(table.rows.rows[1], ["1", "2", "3"]);
    }

    #[test]
    fn windows_line_endings_leave_no_carriage_returns_behind() {
        let table = parse("t", "a,b\r\n1,2\r\n").unwrap();
        assert_eq!(cell(&table, 0, 1), "2");
        assert_eq!(table.rows.columns[1].name, "b");
    }

    #[test]
    fn a_file_that_does_not_end_in_a_newline_keeps_its_last_row() {
        assert_eq!(parse("t", "a,b\n1,2").unwrap().rows.rows.len(), 1);
    }

    #[test]
    fn blank_lines_are_not_rows() {
        assert_eq!(
            parse("t", "a,b\n1,2\n\n\n3,4\n").unwrap().rows.rows.len(),
            2
        );
    }

    #[test]
    fn an_unnamed_column_is_named_after_its_position() {
        let table = parse("t", "a,,c\n1,2,3\n").unwrap();
        assert_eq!(table.rows.columns[1].name, "column 2");
    }

    #[test]
    fn nothing_and_a_single_column_are_both_refused() {
        // One field on the first line means the delimiter guessed wrong or the
        // file is not a table at all; either way it is not worth drawing.
        assert!(parse("t", "").is_err());
        assert!(parse("t", "\n\n").is_err());
        assert!(parse("t", "just a sentence\n").is_err());
    }

    #[test]
    fn a_file_longer_than_the_cap_is_read_as_far_as_the_cap_and_says_so() {
        let mut text = String::from("a,b\n");
        for row in 0..MAX_ROWS + 5 {
            text.push_str(&format!("{row},2\n"));
        }
        let table = parse("t", &text).unwrap();
        assert_eq!(table.rows.rows.len(), MAX_ROWS);
        assert_eq!(
            table.note.as_deref(),
            Some("5 rows past the limit not read")
        );
    }
}
