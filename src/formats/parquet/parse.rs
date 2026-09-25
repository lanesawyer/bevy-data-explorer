//! Reading a Parquet file into rows and columns.
//!
//! No Bevy types. Every value becomes the text a table cell shows: a list is
//! its elements joined by commas, and a null is an empty cell, so a column of
//! numbers with gaps in it still reads as numbers.

use bytes::Bytes;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::{Field, List};

use crate::formats::table::{MAX_ROWS, Table};

/// Read `bytes` as a Parquet file, or say why it is not one.
pub fn parse(name: &str, bytes: Vec<u8>) -> Result<Table, String> {
    let reader = SerializedFileReader::new(Bytes::from(bytes))
        .map_err(|e| format!("not a Parquet file: {e}"))?;
    let metadata = reader.metadata().file_metadata();
    let total = usize::try_from(metadata.num_rows()).unwrap_or(0);

    let headers: Vec<String> = metadata
        .schema_descr()
        .root_schema()
        .get_fields()
        .iter()
        .map(|field| field.name().to_string())
        .collect();
    if headers.is_empty() {
        return Err("the file holds no columns".into());
    }

    let mut rows = Vec::with_capacity(total.min(MAX_ROWS));
    for row in reader
        .get_row_iter(None)
        .map_err(|e| format!("reading rows: {e}"))?
        .take(MAX_ROWS)
    {
        let row = row.map_err(|e| format!("reading row {}: {e}", rows.len() + 1))?;
        rows.push(
            row.get_column_iter()
                .map(|(_, field)| text(field))
                .collect(),
        );
    }

    let skipped = total.saturating_sub(rows.len());
    let note = (skipped > 0).then(|| format!("{skipped} rows past the limit not read"));
    Ok(Table::new(
        name,
        format!("Parquet table, {} columns", headers.len()),
        headers,
        rows,
        note,
    ))
}

/// What a cell shows for `field`.
fn text(field: &Field) -> String {
    match field {
        Field::Null => String::new(),
        Field::Str(value) => value.clone(),
        Field::ListInternal(list) => joined(list),
        other => other.to_string(),
    }
}

fn joined(list: &List) -> String {
    list.elements()
        .iter()
        .map(text)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminology() -> Table {
        let bytes = include_bytes!("../../../testdata/terminology.parquet").to_vec();
        parse("terminology", bytes).unwrap()
    }

    fn column(table: &Table, name: &str) -> usize {
        table
            .rows
            .columns
            .iter()
            .position(|column| column.name == name)
            .unwrap()
    }

    #[test]
    fn every_top_level_field_is_a_column_and_every_record_a_row() {
        let table = terminology();
        assert_eq!(table.rows.columns.len(), 10);
        assert_eq!(table.rows.rows.len(), 1332);
        assert!(table.note.is_none());
        assert_eq!(table.rows.cell(0, column(&table, "identifier")), "MBA:997");
        assert_eq!(
            table.rows.cell(1, column(&table, "name")),
            "Basic cell groups and regions"
        );
    }

    #[test]
    fn a_string_is_shown_without_quotes() {
        let table = terminology();
        assert_eq!(
            table.rows.cell(0, column(&table, "color_hex_triplet")),
            "#FFFFFF"
        );
    }

    #[test]
    fn a_list_is_its_elements_joined_and_an_empty_one_is_empty() {
        let table = terminology();
        let sets = column(&table, "term_set_name");
        assert_eq!(table.rows.cell(1, sets), "category");
        assert_eq!(table.rows.cell(2, sets), "");
        assert!(
            table
                .rows
                .cell(0, sets)
                .starts_with("category, division, organ")
        );
    }

    #[test]
    fn an_integer_column_is_numeric() {
        let table = terminology();
        let values = column(&table, "annotation_value");
        assert_eq!(table.rows.cell(0, values), "987");
        assert!(table.rows.columns[values].numeric);
        assert!(!table.rows.columns[column(&table, "name")].numeric);
    }

    #[test]
    fn anything_else_is_refused() {
        assert!(parse("t", b"a,b\n1,2\n".to_vec()).is_err());
        assert!(parse("t", Vec::new()).is_err());
    }
}
