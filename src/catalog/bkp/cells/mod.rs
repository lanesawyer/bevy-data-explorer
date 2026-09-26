//! What the Brain Knowledge Platform says about a dataset's cells.
//!
//! A Scatterbrain file stores each categorical column as codes, with nothing
//! naming them. The platform holds the rest, keyed by the same column ids:
//!
//! - `getDisplayProperty` — which properties the portal shows, in what order,
//!   which are levels of one taxonomy, and which one it colors by first;
//! - `cellProperties` — each code's label, color and place in its list;
//! - `numericProperties` — the extent of each numeric column;
//! - `cellRangeCounts` and `cellCounts` — how many cells fall in each bucket of
//!   a numeric column, and hold each value of a categorical one.
//!
//! The first four answer in well under a second, so they are asked together
//! and make up [`BkpCells::describe`]. Counting every categorical value takes
//! seconds, so it is [`BkpCells::count`], asked once the labels are showing.
//!
//! Genes are asked about one at a time: `cellGenes` finds them by symbol
//! within the dataset's collection and version, and `cellRangeCounts` counts
//! one's expression when it is taken on, grouped by the gene's index.

use std::collections::HashMap;

use bevy::prelude::*;
use futures::future::{BoxFuture, join_all, try_join3};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::graphql::ask;
use crate::catalog::{CellCounts, CellRecord, DescribeCells, RegionFocus};
use crate::source::genes::Gene;
use crate::source::properties::{
    CellColumns, CellProperties, CellProperty, NumericRange, PropertyKind, PropertyValue, Tree,
    TreeLevel, TreeNode,
};
use crate::source::region::SelectedRegion;

mod build;
mod counts;
mod filters;
#[cfg(test)]
mod fixtures;
mod histogram;
mod queries;

use build::*;
use counts::*;
use filters::*;
use histogram::*;
use queries::*;
// By name, over the prelude's `Display`, which the glob above would
// otherwise leave ambiguous.
use queries::Display;

/// Genes offered for one search. Enough to find one by its first letters;
/// a whole genome would be tens of thousands of rows.
const GENE_RESULTS: usize = 30;

/// The most values a coloring can have before the counts stop being crossed
/// against it. Crossing costs a row per pairing that occurs — measured at
/// 13,000 rows and 1.9MB for a 700-value column against a 338-value one — and
/// a bar divided into hundreds of colors says nothing anyway.
const MIX_COLORS: usize = 128;

/// One BKP dataset, as the queries about its cells name it.
pub struct BkpCells {
    pub endpoint: String,
    pub dataset: String,
    pub project: String,
    pub collection: String,
    pub version: String,
}

impl BkpCells {
    /// The filter the counting queries take, which names the dataset three ways.
    fn dataset_filter(&self) -> Value {
        json!({
            "dataCollectionReferenceId": self.collection,
            "datasetReferenceId": self.dataset,
            "version": self.version,
        })
    }
}

impl DescribeCells for BkpCells {
    fn describe(&self, columns: CellColumns) -> BoxFuture<'static, Result<CellProperties, String>> {
        let endpoint = self.endpoint.clone();
        let dataset = self.dataset.clone();
        let project = self.project.clone();
        let filter = self.dataset_filter();
        Box::pin(async move {
            let (display, values, extents) = try_join3(
                display(&endpoint, &dataset, &project),
                values(&endpoint, &dataset),
                extents(&endpoint, &dataset),
            )
            .await?;

            // Only columns the files hold can be read, so only theirs are
            // counted into histograms.
            let numeric: Vec<(&str, (f32, f32))> = columns
                .0
                .iter()
                .filter(|column| column.numeric)
                .filter_map(|column| {
                    extents
                        .get(&column.id)
                        .map(|extent| (column.id.as_str(), *extent))
                })
                .collect();
            let histograms = join_all(
                numeric
                    .iter()
                    .map(|(id, extent)| histogram(&endpoint, &filter, id, *extent, &Value::Null)),
            )
            .await;
            let histograms: HashMap<String, Vec<u32>> = numeric
                .iter()
                .zip(histograms)
                .filter_map(|((id, _), histogram)| match histogram {
                    Ok(histogram) => Some((id.to_string(), histogram)),
                    Err(e) => {
                        warn!("BKP: no histogram for {id}: {e}");
                        None
                    }
                })
                .collect();

            Ok(build(&columns, display, values, &extents, &histograms))
        })
    }

    fn has_genes(&self) -> bool {
        true
    }

    fn search_genes(&self, text: String) -> BoxFuture<'static, Result<Vec<Gene>, String>> {
        let endpoint = self.endpoint.clone();
        let variables = json!({
            "collection": self.collection,
            "version": self.version,
            "prefixes": prefixes(&text)
                .into_iter()
                .map(|prefix| json!({ "symbol": { "startsWith": prefix } }))
                .collect::<Vec<_>>(),
            "first": GENE_RESULTS,
        });
        Box::pin(async move {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Data {
                cell_genes: Nodes,
            }
            #[derive(Deserialize)]
            struct Nodes {
                nodes: Vec<Found>,
            }
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Found {
                reference_id: String,
                symbol: String,
                index: u32,
                max: f32,
            }
            let data: Data = ask(&endpoint, GENES, variables).await?;
            Ok(data
                .cell_genes
                .nodes
                .into_iter()
                .map(|found| Gene {
                    id: found.reference_id,
                    symbol: found.symbol,
                    index: found.index,
                    high: found.max.max(0.0),
                })
                .collect())
        })
    }

    fn describe_gene(&self, gene: Gene) -> BoxFuture<'static, Result<CellProperty, String>> {
        let endpoint = self.endpoint.clone();
        let filter = self.dataset_filter();
        Box::pin(async move {
            let extent = (0.0, gene.high);
            // Cells not expressing the gene hold no value and are not counted,
            // so the histogram is of the cells that do. Counting them in would
            // flatten every other bar under one at zero.
            let histogram = match histogram(
                &endpoint,
                &filter,
                &gene.index.to_string(),
                extent,
                &Value::Null,
            )
            .await
            {
                Ok(histogram) => histogram,
                Err(e) => {
                    warn!("BKP: no histogram for {}: {e}", gene.symbol);
                    Vec::new()
                }
            };
            Ok(CellProperty {
                id: gene.id,
                name: gene.symbol,
                shown: true,
                kind: PropertyKind::Numeric(NumericRange::full(extent.0, extent.1, histogram)),
                gene: Some(gene.index),
            })
        })
    }

    fn count(&self, properties: &CellProperties) -> BoxFuture<'static, Result<CellCounts, String>> {
        self.count_within(properties, Vec::new())
    }

    fn counts_regions(&self) -> bool {
        true
    }

    fn count_region(
        &self,
        properties: &CellProperties,
        region: &SelectedRegion,
    ) -> BoxFuture<'static, Result<CellCounts, String>> {
        self.count_within(properties, vec![point_filter(region)])
    }

    fn cells_in(
        &self,
        properties: &CellProperties,
        region: &SelectedRegion,
        within: Option<&RegionFocus>,
        limit: usize,
    ) -> BoxFuture<'static, Result<Vec<CellRecord>, String>> {
        let endpoint = self.endpoint.clone();
        // Every categorical column the dataset has, so a record reads as the
        // whole of what is known about a cell rather than as the columns the
        // sidebar happens to be listing.
        let columns: Vec<String> = properties
            .properties
            .iter()
            .filter(|property| property.gene.is_none())
            .flat_map(|property| property.columns())
            .map(|(column, _)| column.to_string())
            .collect();
        // The cells listed are the cells on screen, so the sidebar's filters
        // narrow them as they narrow everything else: a class unticked in the
        // cell panel is not drawn, and must not be listed either.
        let mut filters: Vec<Value> = properties
            .properties
            .iter()
            .filter_map(cell_filter)
            .collect();
        // Each is a further group, so a cell must satisfy all of them — be
        // inside the rectangle, and of the category, and admitted by every
        // filter — while the conditions within one group are alternatives.
        filters.push(point_filter(region));
        filters.extend(within.map(facet_filter));
        let variables = json!({
            "filter": self.dataset_filter(),
            "properties": columns,
            "filters": filters,
            "limit": limit,
        });
        Box::pin(async move {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Data {
                cell_info: Vec<Found>,
            }
            #[derive(Deserialize)]
            struct Found {
                id: Option<String>,
                index: Option<u64>,
                #[serde(default)]
                properties: Vec<Held>,
            }
            #[derive(Deserialize)]
            struct Held {
                property: Option<String>,
                value: Option<String>,
            }

            let data: Data = ask(&endpoint, CELL_INFO, variables).await?;
            Ok(data
                .cell_info
                .into_iter()
                .map(|found| CellRecord {
                    id: found.id.unwrap_or_default(),
                    index: found.index.unwrap_or_default(),
                    values: found
                        .properties
                        .into_iter()
                        .filter_map(|held| Some((held.property?, held.value?)))
                        // A column a cell holds nothing in is left out rather
                        // than shown empty, which would read as a value.
                        .filter(|(_, value)| !value.is_empty() && value != "NA")
                        .collect(),
                })
                .collect())
        })
    }
}

impl BkpCells {
    /// The counts, optionally narrowed to one more group of conditions every
    /// counted cell must also satisfy.
    ///
    /// The whole-dataset counts and a region's are the same round of queries
    /// asked either side of one extra filter, so they are written once. Only
    /// categorical values are counted for a region: the histograms are what
    /// the range controls are drawn from, and redrawing those under a
    /// rectangle would have a filter's shape change as it was dragged.
    fn count_within(
        &self,
        properties: &CellProperties,
        within: Vec<Value>,
    ) -> BoxFuture<'static, Result<CellCounts, String>> {
        let endpoint = self.endpoint.clone();
        let filter = self.dataset_filter();
        let region_only = !within.is_empty();
        let groups: Vec<Option<Value>> = properties.properties.iter().map(cell_filter).collect();
        // Values are counted under every filter, their own property's too, so
        // a value left unticked counts none and a partly ticked tree node
        // counts what is ticked under it. A range is counted under every
        // filter but its own, so its histogram keeps the shape outside it.
        let filters_for = |index: usize, own: bool| {
            let mut applied: Vec<&Value> = groups
                .iter()
                .enumerate()
                .filter(|(other, _)| own || *other != index)
                .filter_map(|(_, group)| group.as_ref())
                .collect();
            // These narrow every count, their own property's included: a
            // value outside the rectangle holds none of its cells, which is
            // the whole point of drawing one.
            applied.extend(within.iter());
            json!(applied)
        };
        // Every count but the coloring's own is crossed against the column
        // the points are colored by, which is what the panel's bars are drawn
        // from. A region's counts are not: the summary already breaks a
        // rectangle down by the coloring, and a drag re-asks these.
        let mix_column = properties
            .mix_column()
            .filter(|_| !region_only)
            .filter(|column| {
                let values = properties
                    .properties
                    .iter()
                    .flat_map(CellProperty::columns)
                    .find(|(id, _)| id == column)
                    .map_or(0, |(_, values)| values.len());
                if values > MIX_COLORS {
                    info!("BKP: not crossing counts against {values} colors");
                }
                values <= MIX_COLORS
            })
            .map(str::to_string);
        let mix_codes: HashMap<String, u16> = mix_column
            .as_deref()
            .and_then(|column| {
                properties
                    .properties
                    .iter()
                    .flat_map(CellProperty::columns)
                    .find(|(id, _)| *id == column)
            })
            .map(|(_, values)| {
                values
                    .iter()
                    .map(|value| (value.label.clone(), value.code))
                    .collect()
            })
            .unwrap_or_default();

        // Counts come back keyed by label, so each column takes its labels
        // along to turn them back into codes.
        let mut columns: Vec<(String, HashMap<String, u16>, Value)> = Vec::new();
        let mut ranges: Vec<(String, String, (f32, f32), Value)> = Vec::new();
        for (index, property) in properties.properties.iter().enumerate() {
            if let Some(range) = property.range().filter(|_| !region_only) {
                // Genes are counted by their index, cell columns by their id.
                let field = property
                    .gene
                    .map_or_else(|| property.id.clone(), |gene| gene.to_string());
                ranges.push((
                    property.id.clone(),
                    field,
                    (range.low, range.high),
                    filters_for(index, false),
                ));
            }
            for (column, values) in property.columns() {
                let codes = values
                    .iter()
                    .map(|value| (value.label.clone(), value.code))
                    .collect();
                columns.push((column.to_string(), codes, filters_for(index, true)));
            }
        }
        Box::pin(async move {
            let (counted, histograms) = futures::future::join(
                join_all(columns.iter().map(|(id, codes, filters)| {
                    // A column crossed against itself would count each value
                    // against itself, so the coloring's own counts are asked
                    // for plainly.
                    let against = mix_column
                        .as_deref()
                        .filter(|column| column != id)
                        .map(|column| (column, &mix_codes));
                    counts(&endpoint, &filter, id, codes, against, filters)
                })),
                join_all(ranges.iter().map(|(_, field, extent, filters)| {
                    histogram(&endpoint, &filter, field, *extent, filters)
                })),
            )
            .await;
            let mut found = CellCounts {
                mix_column: mix_column.clone(),
                ..CellCounts::default()
            };
            let mut last_error = None;
            for ((id, _, _), result) in columns.into_iter().zip(counted) {
                match result {
                    Ok((counts, mixes)) => {
                        found.values.push((id.clone(), counts));
                        if !mixes.is_empty() {
                            found.mixes.push((id, mixes));
                        }
                    }
                    Err(e) => last_error = Some(e),
                }
            }
            for ((id, _, _, _), result) in ranges.into_iter().zip(histograms) {
                match result {
                    Ok(histogram) => found.histograms.push((id, histogram)),
                    Err(e) => last_error = Some(e),
                }
            }
            match last_error {
                Some(e) if found.values.is_empty() && found.histograms.is_empty() => Err(e),
                _ => Ok(found),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::source::properties::{CellColumn, Column};

    #[test]
    #[ignore = "reads the live BKP API"]
    fn a_region_counts_some_of_the_dataset_and_not_all_of_it() {
        // The box filter was found by asking the API, not from a schema that
        // documents it, so this is what notices if its field naming or its
        // corner order ever changes. Zhuang-ABCA-1 spans roughly x 0..79 and
        // y 2..95, and the rectangle takes a bite out of the middle.
        let cells = BkpCells {
            endpoint: super::super::PRODUCTION.to_string(),
            dataset: "ZGCBY62J0LFKZ58JIUE".into(),
            project: "5C0201JSVE04WY6DMVC".into(),
            collection: "U5V94ES4J76MYSL7QL7".into(),
            version: "v0".into(),
        };
        // Several columns, so the records that come back are the shape a
        // sidebar shows rather than one field wide.
        let columns = CellColumns(
            [
                ("FS00DXV0T9R1X9FJ4QE", "Class"),
                ("QY5S8KMO5HLJUF0P00K", "Subclass"),
                ("4MV7HA5DG2XJZ3UD8G9", "Neurotransmitter Type"),
                ("73GVTDXDEGE27M2XJMT", "Anatomical Division"),
            ]
            .into_iter()
            .map(|(id, name)| CellColumn {
                id: id.into(),
                name: name.into(),
                numeric: false,
            })
            .collect(),
        );
        let properties = crate::app::net::block_on(cells.describe(columns)).unwrap();
        let everything = crate::app::net::block_on(cells.count(&properties)).unwrap();
        let region = SelectedRegion {
            key: "MGA5LUTH4ETM859L5IM".into(),
            min: Vec2::new(20.0, 30.0),
            max: Vec2::new(40.0, 50.0),
        };
        let inside = crate::app::net::block_on(cells.count_region(&properties, &region)).unwrap();
        assert!(counted(&inside) > 0, "the rectangle counted nothing");
        assert!(
            counted(&inside) < counted(&everything),
            "the rectangle counted the whole dataset, so the filter did nothing"
        );

        // Drilling into one category narrows it again, and the two filters
        // have to compose: the box and the category are separate groups, and
        // a service that read them as alternatives would answer with more
        // cells rather than fewer.
        const CLASS: &str = "FS00DXV0T9R1X9FJ4QE";
        let (biggest, _) = inside
            .values
            .iter()
            .find(|(column, _)| column == CLASS)
            .expect("the class column was counted")
            .1
            .iter()
            .max_by_key(|(_, count)| *count)
            .expect("the rectangle held some cells");
        // Through `columns`, not `values`: the platform folds the taxonomy's
        // columns into a tree, whose codes live in its levels rather than in a
        // flat list.
        let label = properties
            .properties
            .iter()
            .flat_map(|property| property.columns())
            .find(|(column, _)| *column == CLASS)
            .expect("the class column is one of the described properties")
            .1
            .iter()
            .find(|value| value.code == *biggest)
            .expect("the counted code is one of the described values")
            .label
            .clone();
        let focus = RegionFocus {
            column: CLASS.into(),
            label,
        };
        let listed =
            crate::app::net::block_on(cells.cells_in(&properties, &region, Some(&focus), 5))
                .unwrap();
        assert!(!listed.is_empty(), "the category listed no cells");
        assert!(listed.len() <= 5, "the page limit was not honored");
        for cell in &listed {
            assert!(!cell.id.is_empty(), "a cell came back with no identifier");
            // Every one of them is of the category asked for, which is what
            // says the box and the category composed rather than being read as
            // alternatives.
            assert_eq!(cell.value(CLASS), Some(focus.label.as_str()));
            // Values arrive as labels rather than as the codes the files
            // store, so a record needs no translation to be shown.
            assert!(cell.values.len() > 1, "a cell came back nearly empty");
        }

        // The cells listed are the cells on screen, so a value unticked in the
        // cell panel must not appear among them. Ticking one value of a column
        // admits only that one.
        const NEUROTRANSMITTER: &str = "4MV7HA5DG2XJZ3UD8G9";
        let mut filtered = properties;
        let admitted = filtered
            .properties
            .iter_mut()
            .flat_map(|property| property.column_values_mut(NEUROTRANSMITTER))
            .find(|value| value.label == "Glut")
            .map(|value| {
                value.selected = true;
                value.label.clone()
            })
            .expect("the dataset describes a neurotransmitter type of Glut");
        let narrowed =
            crate::app::net::block_on(cells.cells_in(&filtered, &region, None, 10)).unwrap();
        assert!(!narrowed.is_empty(), "the filter admitted no cells at all");
        for cell in &narrowed {
            assert_eq!(
                cell.value(NEUROTRANSMITTER),
                Some(admitted.as_str()),
                "a cell the filters exclude was listed anyway"
            );
        }
    }

    #[test]
    #[ignore = "reads the live BKP API"]
    fn finds_and_describes_a_live_gene() {
        let cells = BkpCells {
            endpoint: super::super::PRODUCTION.into(),
            dataset: "Q1NCWWPG6FZ0DNIXJBQ".into(),
            project: String::new(),
            collection: "AP8JNN5LYABGVMGKY1B".into(),
            version: "v0".into(),
        };
        let found = crate::app::net::block_on(cells.search_genes("gad".into())).unwrap();
        let gad1 = found
            .iter()
            .find(|gene| gene.symbol == "Gad1")
            .expect("a mouse dataset names it Gad1")
            .clone();
        assert!(gad1.high > 0.0);

        let property = crate::app::net::block_on(cells.describe_gene(gad1.clone())).unwrap();
        assert_eq!(property.gene, Some(gad1.index));
        let range = property.range().unwrap();
        assert_eq!((range.low, range.high), (0.0, gad1.high));
        assert!(
            range.histogram.iter().sum::<u32>() > 0,
            "someone expresses it"
        );
    }

    #[test]
    #[ignore = "reads the live BKP API"]
    fn describes_a_live_dataset() {
        let cells = BkpCells {
            endpoint: super::super::PRODUCTION.into(),
            dataset: "DATSEAADCAHTEST12F3670E".into(),
            project: "UMSVXTDIAZTAFKGE43T".into(),
            collection: "69Q58MKNTCZKVFQ00V4".into(),
            version: "v0".into(),
        };
        let columns = CellColumns(vec![
            column("CCN20260701_LEVEL_1", false),
            column("E4E313ECEDC2194BBE", false),
            column("2E8980E6E33AECF44C", true),
        ]);
        let properties = crate::app::net::block_on(cells.describe(columns)).unwrap();
        assert_eq!(properties.properties.len(), 3);
        assert_eq!(
            properties
                .selection()
                .color_by
                .as_ref()
                .and_then(Column::cell),
            Some("CCN20260701_LEVEL_1")
        );
        let age = properties
            .properties
            .iter()
            .find_map(CellProperty::range)
            .unwrap();
        assert_eq!((age.low, age.high), (68.0, 99.0));
        assert_eq!(age.histogram.iter().sum::<u32>(), 686_439);

        let counts = crate::app::net::block_on(cells.count(&properties)).unwrap();
        let braak: u64 = counts
            .values
            .iter()
            .find(|(id, _)| id == "E4E313ECEDC2194BBE")
            .map(|(_, counts)| counts.iter().map(|(_, count)| count).sum())
            .unwrap();
        assert_eq!(braak, 686_439);
        let aged = |counts: &CellCounts| -> u32 {
            counts
                .histograms
                .iter()
                .find(|(id, _)| id == "2E8980E6E33AECF44C")
                .map(|(_, histogram)| histogram.iter().sum())
                .unwrap()
        };
        assert_eq!(aged(&counts), 686_439);

        // Ticking one Braak stage narrows every count and histogram to that
        // stage's cells, Braak's own counts included.
        let mut properties = properties;
        let braak = properties
            .properties
            .iter_mut()
            .find(|property| property.id == "E4E313ECEDC2194BBE")
            .unwrap();
        let PropertyKind::Categorical(values) = &mut braak.kind else {
            panic!("Braak is categorical");
        };
        values[0].selected = true;
        let (label, code) = (values[0].label.clone(), values[0].code);
        let counts = crate::app::net::block_on(cells.count(&properties)).unwrap();
        let total = |column: &str| -> u64 {
            counts
                .values
                .iter()
                .find(|(id, _)| id == column)
                .map(|(_, counts)| counts.iter().map(|(_, count)| count).sum())
                .unwrap()
        };
        let stage = values_counted(&counts, "E4E313ECEDC2194BBE", code);
        assert!(stage > 0 && stage < 686_439, "{label} holds {stage}");
        assert_eq!(total("E4E313ECEDC2194BBE"), stage);
        assert_eq!(total("CCN20260701_LEVEL_1"), stage);
        assert_eq!(u64::from(aged(&counts)), stage);
    }
}
