//! Parquet — a columnar table — read into rows and columns.
//!
//! Like delimited text, a Parquet file is read whole and ends at a
//! [`super::table::Table`], so there are no systems: [`parse`] turns the bytes
//! into rows and the grid fills a frame with them.

pub mod parse;

/// Name a table after the file it came from.
pub fn label_for(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/')
        .find(|segment| !segment.is_empty())
        .map(|file| {
            file.trim_end_matches(".parquet")
                .trim_end_matches(".PARQUET")
        })
        .filter(|stem| !stem.is_empty())
        .map_or_else(|| "Table".to_string(), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_is_named_after_its_file() {
        assert_eq!(
            label_for("s3://allen-atlas-assets/terminologies/2026-03/terminology.parquet"),
            "terminology"
        );
        assert_eq!(label_for("/data/cells.PARQUET?v=2"), "cells");
        assert!(!label_for("").is_empty());
    }
}
