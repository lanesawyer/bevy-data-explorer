//! Delimited text — CSV and TSV — read into rows and columns.
//!
//! The thinnest format here. A delimited file is read whole and has nothing to
//! stream, nothing to draw on a render layer and no detail to pull in as a
//! view moves, so there are no systems at all: [`parse`] turns the text into a
//! [`super::table::Table`] and the grid fills a frame with it.

pub mod parse;

/// Name a table after the file it came from.
pub fn label_for(url: &str) -> String {
    super::table::label_for(url, &["csv", "tsv"])
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
