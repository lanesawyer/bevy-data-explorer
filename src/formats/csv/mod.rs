//! Delimited text — CSV and TSV — read into rows and columns.
//!
//! The thinnest format here. A delimited file is read whole and has nothing to
//! stream, nothing to draw on a render layer and no detail to pull in as a
//! view moves, so there are no systems at all: [`spawn_source`] parses it into
//! a [`SourceTable`] and the grid fills the frame with it. What that looks
//! like is [`crate::view::table`]'s business, which is what keeps a table
//! drawn the same way whichever format the rows came out of.

pub mod parse;

use std::sync::Arc;

use bevy::prelude::*;

use crate::source::table::{SourceTable, TableColumn};
use crate::source::{self, SourceExtent, SourceStatus};
use parse::Table;

/// Register a parsed table as a source.
pub fn spawn_source(world: &mut World, table: Arc<Table>) -> Entity {
    let source = source::register_in(
        world,
        source::SourceInfo {
            name: table.name.clone(),
            // A table is not measured in anything. Its rows are records, not
            // a place, which is also why its frame scrolls rather than pans.
            unit: String::new(),
            detail: format!(
                "{}, {} columns",
                if table.delimiter == '\t' {
                    "Tab-separated table"
                } else {
                    "Comma-separated table"
                },
                table.columns.len()
            ),
            stat: format!("{} ROWS", source::compact_count(table.rows.len() as u64)),
        },
        // Nominal: a table occupies no space, and its frame never looks at the
        // world. It is still registered with one because every source is
        // framed from an extent when its panel opens.
        SourceExtent {
            centre: Vec2::ZERO,
            size: Vec2::ONE,
            finest: 1.0,
        },
    );

    let mut status = format!(
        "{} rows \u{00d7} {} columns",
        table.rows.len(),
        table.columns.len()
    );
    if table.skipped > 0 {
        status += &format!(", {} rows past the limit not read", table.skipped);
    }

    world.entity_mut(source).insert((
        SourceStatus(status),
        SourceTable {
            columns: table
                .columns
                .iter()
                .map(|column| TableColumn {
                    name: column.name.clone(),
                    chars: column.chars,
                    numeric: column.numeric,
                })
                .collect(),
            rows: table.rows.clone(),
        },
    ));
    source
}

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

    #[test]
    fn registering_a_table_hands_the_grid_its_rows() {
        let table = parse::parse("genes", include_str!("../../../testdata/genes.csv")).unwrap();
        let mut app = App::new();
        let source = spawn_source(app.world_mut(), Arc::new(table));

        let world = app.world();
        let rows = world.get::<SourceTable>(source).unwrap();
        assert_eq!(rows.columns.len(), 5);
        assert_eq!(rows.rows.len(), 6);
        assert_eq!(rows.columns[1].name, "gene_symbol");
        // The comma inside a quoted field is part of the value, not a column.
        assert_eq!(rows.cell(0, 3), "protein kinase C, theta");

        let source = world.get::<source::DataSource>(source).unwrap();
        assert_eq!(source.stat, "6 ROWS");
        // Nothing to line a table up against, so it shares space with nothing.
        assert!(source.unit.is_empty());
    }

    #[test]
    fn a_status_says_the_shape_of_the_table_before_anything_is_drawn() {
        let table = parse::parse("genes", include_str!("../../../testdata/genes.csv")).unwrap();
        let mut app = App::new();
        let source = spawn_source(app.world_mut(), Arc::new(table));
        let status = &app.world().get::<SourceStatus>(source).unwrap().0;
        assert_eq!(status, "6 rows \u{00d7} 5 columns");
    }
}
