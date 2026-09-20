//! Delimited text — CSV and TSV — read into rows and columns.
//!
//! The thinnest format here. A delimited file is read whole and has nothing to
//! stream, nothing to draw on a render layer and no detail to pull in as a
//! view moves, so there are no systems at all: [`parse`] turns the text into a
//! [`super::table::Table`] and the grid fills a frame with it.

pub mod parse;

/// Name a table after the file it came from.
pub fn label_for(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/')
        .find(|segment| !segment.is_empty())
        .map_or_else(
            || "Table".to_string(),
            |file| {
                let stem = file
                    .trim_end_matches(".csv")
                    .trim_end_matches(".CSV")
                    .trim_end_matches(".tsv")
                    .trim_end_matches(".TSV");
                if stem.is_empty() {
                    "Table".to_string()
                } else {
                    stem.to_string()
                }
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_is_named_after_its_file() {
        assert_eq!(label_for("https://store/metadata/gene.csv"), "gene");
        assert_eq!(label_for("/data/Regions.TSV?v=2"), "Regions");
        assert_eq!(label_for("cells.csv"), "cells");
        assert!(!label_for("").is_empty());
    }
}
