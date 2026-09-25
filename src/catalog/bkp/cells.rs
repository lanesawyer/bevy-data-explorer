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
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::catalog::{CellCounts, CellRecord, DescribeCells, RegionFocus};
use crate::source::genes::Gene;
use crate::source::properties::{
    CellColumns, CellProperties, CellProperty, NumericRange, PropertyKind, PropertyValue, Tree,
    TreeLevel, TreeNode,
};
use crate::source::region::SelectedRegion;

/// The most value records asked for in one page. The API allows far more than
/// the 50 its dataset listing does, and a taxonomy with thousands of clusters
/// would otherwise take a hundred round trips.
const VALUE_PAGE: usize = 5000;

/// Enough pages for a quarter of a million values, so a cursor that never
/// ends cannot keep a lookup going forever.
const MAX_VALUE_PAGES: usize = 50;

/// Buckets in a numeric property's histogram.
const BUCKETS: usize = 32;

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

/// A selected rectangle as the counting queries take it: one condition every
/// counted cell must satisfy.
///
/// The API's only spatial filter is this box. Its field names the
/// visualization whose coordinates the rectangle is in — the same id the
/// Scatterbrain metadata carries, which is what [`SelectedRegion::key`] holds
/// — and its value is the two corners, the far one first. Corner order does
/// not actually matter to the service, but it is written the way the platform
/// writes it so the two can be compared when one of them changes.
fn point_filter(region: &SelectedRegion) -> Value {
    let (min, max) = region.corners();
    json!([{
        "type": "POINT",
        "field": format!("{}_point", region.key),
        "operator": "CONTAINED_IN",
        "value": format!("[{},{},{},{}]", max[0], max[1], min[0], min[1]),
    }])
}

/// One category as the counting queries take it, for drilling into a region.
///
/// Named by label, as every metadata condition here is: the API matches the
/// value the platform shows rather than the code the files store.
fn facet_filter(within: &RegionFocus) -> Value {
    json!([{
        "type": "METADATA",
        "field": within.column,
        "operator": "EQ",
        "value": within.label,
    }])
}

/// A property's filter as the counting queries take it: the conditions any
/// one of which admits a cell, or nothing if it admits every cell.
///
/// Values are named by label rather than code. A tree names each ticked node
/// at its own level, which is far fewer conditions than the finest codes
/// those ticks admit.
fn cell_filter(property: &CellProperty) -> Option<Value> {
    if !property.restricts() {
        return None;
    }
    let condition = |field: &str, operator: &str, value: String| {
        let kind = if property.gene.is_some() {
            "GENE"
        } else {
            "METADATA"
        };
        json!({ "type": kind, "field": field, "operator": operator, "value": value })
    };
    let conditions: Vec<Value> = match &property.kind {
        PropertyKind::Categorical(values) => values
            .iter()
            .filter(|value| value.selected)
            .map(|value| condition(&property.id, "EQ", value.label.clone()))
            .collect(),
        PropertyKind::Tree(tree) => tree
            .nodes
            .iter()
            .filter(|node| node.value.selected)
            .filter_map(|node| {
                let level = tree.levels.get(node.level)?;
                Some(condition(&level.id, "EQ", node.value.label.clone()))
            })
            .collect(),
        PropertyKind::Numeric(range) => {
            // Ranges here leave out their high end, and the one on screen
            // keeps it.
            let to = f64::from(range.to);
            let past = to + (to.abs() * 1e-6).max(1e-9);
            let field = property
                .gene
                .map_or_else(|| property.id.clone(), |index| index.to_string());
            vec![condition(
                &field,
                "BETWEEN",
                format!("[{},{past}]", range.from),
            )]
        }
    };
    Some(json!(conditions))
}

/// Ask one GraphQL question and take its `data`.
async fn ask<T: DeserializeOwned>(
    endpoint: &str,
    query: &str,
    variables: Value,
) -> Result<T, String> {
    #[derive(Deserialize)]
    struct Response<T> {
        data: Option<T>,
        #[serde(default)]
        errors: Vec<Error>,
    }
    #[derive(Deserialize)]
    struct Error {
        message: String,
    }

    let body = json!({ "query": query, "variables": variables });
    let text = crate::app::net::post_json(endpoint, body.to_string()).await?;
    let response: Response<T> =
        serde_json::from_str(&text).map_err(|e| format!("parsing BKP answer: {e}"))?;
    if let Some(error) = response.errors.first() {
        return Err(error.message.clone());
    }
    response
        .data
        .ok_or_else(|| "BKP answered with no data".into())
}

const DISPLAY: &str = "query($filter: DisplayPropertyFilter!) {
  getDisplayProperty(displayPropertyFilter: $filter) {
    ... on DatasetDisplayProperty {
      defaultColorBy { referenceId }
      displayFeatures {
        isDefault
        priorityOrder
        featureType { referenceId title }
        ... on HierarchicalDisplayProperty {
          featureSet { priorityOrder featureType { referenceId title } }
        }
        ... on TreeDisplayProperty {
          featureSet { priorityOrder featureType { referenceId title } }
        }
      }
    }
  }
}";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Display {
    default_color_by: Option<FeatureType>,
    #[serde(default)]
    display_features: Vec<Feature>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Feature {
    is_default: bool,
    priority_order: Option<i32>,
    feature_type: FeatureType,
    /// The levels of a taxonomy or other hierarchy; absent for a plain column.
    feature_set: Option<Vec<Level>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Level {
    priority_order: Option<i32>,
    feature_type: FeatureType,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeatureType {
    reference_id: String,
    title: Option<String>,
}

async fn display(endpoint: &str, dataset: &str, project: &str) -> Result<Display, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        get_display_property: Option<Display>,
    }
    let filter = json!({
        "filter": { "type": "DATASET", "typeReferenceId": dataset, "projectReferenceId": project }
    });
    let data: Data = ask(endpoint, DISPLAY, filter).await?;
    // A dataset with no display settings still has labels worth showing.
    Ok(data.get_display_property.unwrap_or_default())
}

const VALUES: &str = "query($dataset: String!, $first: Int, $after: String) {
  cellProperties(first: $first, after: $after,
                 where: { dataset: { referenceId: { eq: $dataset } } }) {
    pageInfo { hasNextPage endCursor }
    nodes {
      color
      featureType { referenceId }
      featureTypeValueIndex { value index priorityOrder referenceId parentReferenceId }
    }
  }
}";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValueRecord {
    color: Option<String>,
    feature_type: FeatureType,
    feature_type_value_index: ValueIndex,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValueIndex {
    value: String,
    /// The code the files store for this value.
    index: i64,
    priority_order: Option<i32>,
    reference_id: Option<String>,
    /// The value a level up in a hierarchy, by its `reference_id`.
    parent_reference_id: Option<String>,
}

async fn values(endpoint: &str, dataset: &str) -> Result<Vec<ValueRecord>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        cell_properties: Page,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Page {
        page_info: PageInfo,
        nodes: Vec<ValueRecord>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PageInfo {
        has_next_page: bool,
        end_cursor: Option<String>,
    }

    let mut records = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..MAX_VALUE_PAGES {
        let variables = json!({ "dataset": dataset, "first": VALUE_PAGE, "after": after });
        let data: Data = ask(endpoint, VALUES, variables).await?;
        records.extend(data.cell_properties.nodes);
        let info = data.cell_properties.page_info;
        after = info.has_next_page.then_some(info.end_cursor).flatten();
        if after.is_none() {
            break;
        }
    }
    Ok(records)
}

const EXTENTS: &str = "query($dataset: String!) {
  numericProperties(first: 1000, where: { dataset: { referenceId: { eq: $dataset } } }) {
    nodes { min max featureType { referenceId } }
  }
}";

/// Each numeric column's extent across the whole dataset, by column id.
async fn extents(endpoint: &str, dataset: &str) -> Result<HashMap<String, (f32, f32)>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        numeric_properties: Nodes,
    }
    #[derive(Deserialize)]
    struct Nodes {
        nodes: Vec<Extent>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Extent {
        min: f32,
        max: f32,
        feature_type: FeatureType,
    }

    let data: Data = ask(endpoint, EXTENTS, json!({ "dataset": dataset })).await?;
    Ok(data
        .numeric_properties
        .nodes
        .into_iter()
        .map(|extent| {
            (
                extent.feature_type.reference_id,
                (extent.min, extent.max.max(extent.min)),
            )
        })
        .collect())
}

const GENES: &str = "query($collection: String!, $version: String!,
                           $prefixes: [CellGeneFilterInput!], $first: Int) {
  cellGenes(first: $first, order: [{ symbol: ASC }],
            where: { dataCollectionId: { eq: $collection }, version: { eq: $version },
                     or: $prefixes }) {
    nodes { referenceId symbol index max }
  }
}";

/// The ways a typed prefix may be cased in a symbol.
///
/// Matching is case-sensitive, and conventions differ by species: human genes
/// are upper case (`GAD1`), mouse ones capitalized (`Gad1`). Asking for each
/// finds either however it was typed.
fn prefixes(text: &str) -> Vec<String> {
    let text = text.trim();
    let mut capitalized: String = text.chars().take(1).flat_map(char::to_uppercase).collect();
    capitalized.extend(text.chars().skip(1).flat_map(char::to_lowercase));
    let mut prefixes = vec![text.to_string(), text.to_uppercase(), capitalized];
    prefixes.sort();
    prefixes.dedup();
    prefixes
}

const RANGE_COUNTS: &str = "query($filter: DatasetFilter!, $field: String!, $range: [String]!,
                                $filters: [[CellFilterInput!]]) {
  cellRangeCounts(datasetFilter: $filter, groupBy: { field: $field, range: $range },
                  filters: $filters) {
    count
    properties { value }
  }
}";

#[derive(Deserialize)]
struct Counted {
    count: f64,
    #[serde(default)]
    properties: Vec<Tuple>,
}

#[derive(Deserialize)]
struct Tuple {
    /// Which column this value came from, for a group crossing two.
    #[serde(default)]
    property: Option<String>,
    value: Option<String>,
}

/// Counts in [`BUCKETS`] equal buckets across `extent`, of the cells
/// `filters` admit, as [`counts`] takes them.
async fn histogram(
    endpoint: &str,
    filter: &Value,
    field: &str,
    extent: (f32, f32),
    filters: &Value,
) -> Result<Vec<u32>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        cell_range_counts: Vec<Counted>,
    }

    let edges = bucket_edges(extent);
    let range: Vec<String> = edges
        .windows(2)
        .map(|pair| format!("[{},{}]", pair[0], pair[1]))
        .collect();
    let variables = json!({ "filter": filter, "field": field, "range": range, "filters": filters });
    let data: Data = ask(endpoint, RANGE_COUNTS, variables).await?;
    Ok(bin(&edges, &data.cell_range_counts))
}

/// Edges of [`BUCKETS`] buckets across `extent`.
///
/// The API's ranges are half-open, including their low end and not their high
/// one, so the last edge sits just past the maximum to keep the cells holding
/// it. A column with one value throughout gets a unit's width to count in.
fn bucket_edges((low, high): (f32, f32)) -> Vec<f64> {
    let low = f64::from(low);
    let span = (f64::from(high) - low).max(f64::EPSILON);
    let mut edges: Vec<f64> = (0..=BUCKETS)
        .map(|i| low + span * i as f64 / BUCKETS as f64)
        .collect();
    let last = edges.len() - 1;
    edges[last] += span.max(1.0) * 1e-6;
    edges
}

/// Put each counted range back in its bucket.
///
/// The API echoes each range back reformatted — `[68.0,...]` returns as
/// `[68,...]` — and not necessarily in the order asked, so a range is placed by
/// the low end it parses to rather than by its text or its position.
fn bin(edges: &[f64], counted: &[Counted]) -> Vec<u32> {
    let mut buckets = vec![0u32; edges.len() - 1];
    for counted in counted {
        let Some(low) = counted
            .properties
            .first()
            .and_then(|tuple| tuple.value.as_deref())
            .and_then(|value| value.trim_start_matches('[').split(',').next())
            .and_then(|low| low.trim().parse::<f64>().ok())
        else {
            continue;
        };
        let nearest = edges[..edges.len() - 1]
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - low).abs().total_cmp(&(b.1 - low).abs()))
            .map(|(bucket, _)| bucket);
        if let Some(bucket) = nearest {
            buckets[bucket] = buckets[bucket].saturating_add(counted.count as u32);
        }
    }
    buckets
}

/// The cells themselves, rather than a count of them.
///
/// Each value comes back named by its column and already as the label the
/// platform shows, so a record needs no translation from the codes the files
/// store.
const CELL_INFO: &str = "query($filter: DatasetFilter!, $properties: [String!],
                              $filters: [[CellFilterInput!]], $limit: Int) {
  cellInfo(datasetFilter: $filter, properties: $properties,
           filters: $filters, limit: $limit) {
    id
    index
    properties { property value }
  }
}";

const COUNTS: &str = "query($filter: DatasetFilter!, $fields: [String!],
                          $filters: [[CellFilterInput!]]) {
  cellCounts(datasetFilter: $filter, groupBy: $fields, filters: $filters) {
    count
    properties { property value }
  }
}";

/// How many cells hold each value of one categorical column, and — when
/// `against` names a second column — how many hold each pairing of the two,
/// among the cells `filters` admit: every one of its groups, and any
/// condition in a group.
///
/// Crossing two columns is one query rather than one per value: the API
/// groups by as many fields as it is given and returns only the pairings that
/// occur, so a taxonomy crossed with its own coloring costs a row per cluster
/// rather than a row per cluster per color.
async fn counts(
    endpoint: &str,
    filter: &Value,
    field: &str,
    codes: &HashMap<String, u16>,
    against: Option<(&str, &HashMap<String, u16>)>,
    filters: &Value,
) -> Result<(Vec<(u16, u64)>, Vec<(u16, u16, u64)>), String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        cell_counts: Vec<Counted>,
    }

    let fields: Vec<&str> = [field]
        .into_iter()
        .chain(against.map(|(id, _)| id))
        .collect();
    let variables = json!({ "filter": filter, "fields": fields, "filters": filters });
    let data: Data = ask(endpoint, COUNTS, variables).await?;
    Ok(tally(&data.cell_counts, field, codes, against))
}

/// Put each counted group back on its codes: the column's own count, summed
/// across whatever it was crossed with, and the crossing itself.
///
/// Each group names the column every one of its values came from, so a
/// crossed pair is read by name rather than by trusting the order it comes
/// back in. A group holding a label neither column knows is dropped.
fn tally<'a>(
    groups: &'a [Counted],
    field: &str,
    codes: &HashMap<String, u16>,
    against: Option<(&str, &HashMap<String, u16>)>,
) -> (Vec<(u16, u64)>, Vec<(u16, u16, u64)>) {
    let held = |counted: &'a Counted, column: &str| -> Option<&'a str> {
        counted
            .properties
            .iter()
            .find(|tuple| tuple.property.as_deref() == Some(column))
            // One group has nothing to tell apart, and the range counts name
            // no column at all.
            .or_else(|| counted.properties.first().filter(|_| against.is_none()))
            .and_then(|tuple| tuple.value.as_deref())
    };

    let mut totals: HashMap<u16, u64> = HashMap::new();
    let mut mixes = Vec::new();
    for counted in groups {
        let Some(code) = held(counted, field).and_then(|label| codes.get(label).copied()) else {
            continue;
        };
        let count = counted.count as u64;
        *totals.entry(code).or_default() += count;
        if let Some((column, against)) = against
            && let Some(color) = held(counted, column).and_then(|label| against.get(label).copied())
        {
            mixes.push((code, color, count));
        }
    }
    let mut values: Vec<(u16, u64)> = totals.into_iter().collect();
    values.sort_unstable();
    (values, mixes)
}

/// One value as the platform lists it, before it is placed.
struct Listed {
    priority: i32,
    reference: Option<String>,
    parent: Option<String>,
    value: PropertyValue,
}

/// What the portal lists, in its order.
enum Entry {
    Column {
        id: String,
        title: Option<String>,
        shown: bool,
    },
    Tree {
        id: String,
        title: Option<String>,
        shown: bool,
        /// Column id and title of each level, coarsest first.
        levels: Vec<(String, Option<String>)>,
    },
}

/// Assemble the properties the portal would show, for the columns the files
/// hold.
///
/// Listed in the portal's order. A taxonomy becomes one tree over whichever of
/// its levels the files hold, or a plain property if they hold only one.
/// Columns the portal does not show are kept but hidden, so the section's menu
/// can still offer them, and only if the platform knows enough about them to be
/// worth offering.
fn build(
    columns: &CellColumns,
    display: Display,
    values: Vec<ValueRecord>,
    extents: &HashMap<String, (f32, f32)>,
    histograms: &HashMap<String, Vec<u32>>,
) -> CellProperties {
    let mut by_column: HashMap<String, Vec<Listed>> = HashMap::new();
    // Every value's parent, whatever its column, so a tree can reach past a
    // level the files do not hold.
    let mut parents: HashMap<String, String> = HashMap::new();
    for record in values {
        let index = record.feature_type_value_index;
        let Ok(code) = u16::try_from(index.index) else {
            continue;
        };
        let listed = by_column
            .entry(record.feature_type.reference_id)
            .or_default();
        // Some datasets list every value twice, identically.
        if listed.iter().any(|found| found.value.code == code) {
            continue;
        }
        if let (Some(reference), Some(parent)) = (&index.reference_id, &index.parent_reference_id) {
            parents.insert(reference.clone(), parent.clone());
        }
        listed.push(Listed {
            priority: index.priority_order.unwrap_or(i32::MAX),
            reference: index.reference_id.clone(),
            parent: index.parent_reference_id,
            value: PropertyValue {
                code,
                label: index.value,
                reference: index.reference_id,
                color: record.color.as_deref().and_then(parse_color),
                count: None,
                selected: false,
            },
        });
    }
    for listed in by_column.values_mut() {
        listed.sort_by_key(|found| (found.priority, found.value.code));
    }

    let mut features = display.display_features;
    features.sort_by_key(|feature| feature.priority_order.unwrap_or(i32::MAX));
    let mut entries: Vec<Entry> = Vec::new();
    for feature in features {
        match feature.feature_set {
            Some(mut levels) if !levels.is_empty() => {
                levels.sort_by_key(|level| level.priority_order.unwrap_or(i32::MAX));
                entries.push(Entry::Tree {
                    id: feature.feature_type.reference_id,
                    title: feature.feature_type.title,
                    shown: feature.is_default,
                    levels: levels
                        .into_iter()
                        .map(|level| (level.feature_type.reference_id, level.feature_type.title))
                        .collect(),
                });
            }
            _ => entries.push(Entry::Column {
                id: feature.feature_type.reference_id,
                title: feature.feature_type.title,
                shown: feature.is_default,
            }),
        }
    }
    for column in &columns.0 {
        entries.push(Entry::Column {
            id: column.id.clone(),
            title: None,
            shown: false,
        });
    }

    let readable = |id: &str, by_column: &HashMap<String, Vec<Listed>>| {
        columns
            .0
            .iter()
            .any(|column| column.id == id && !column.numeric)
            && by_column.contains_key(id)
    };

    let mut properties = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        match entry {
            Entry::Tree {
                id,
                title,
                shown,
                levels,
            } => {
                let present: Vec<(String, Option<String>)> = levels
                    .into_iter()
                    .filter(|(level, _)| !seen.contains(level) && readable(level, &by_column))
                    .collect();
                if present.len() == 1 {
                    let (level, level_title) = present.into_iter().next().unwrap();
                    entries_column(
                        &mut properties,
                        &mut seen,
                        &mut by_column,
                        columns,
                        extents,
                        histograms,
                        &level,
                        level_title,
                        shown,
                    );
                    continue;
                }
                if present.is_empty() || !seen.insert(id.clone()) {
                    continue;
                }
                let tree = tree_of(&present, &mut by_column, &parents, columns);
                for (level, _) in &present {
                    seen.insert(level.clone());
                }
                properties.push(CellProperty {
                    id,
                    name: title.unwrap_or_else(|| "Taxonomy".into()),
                    shown,
                    kind: PropertyKind::Tree(tree),
                    gene: None,
                });
            }
            Entry::Column { id, title, shown } => entries_column(
                &mut properties,
                &mut seen,
                &mut by_column,
                columns,
                extents,
                histograms,
                &id,
                title,
                shown,
            ),
        }
    }

    let mut described = CellProperties::ready(properties);
    if let Some(default) = display.default_color_by {
        described.color_by_id(&default.reference_id);
    }
    described
}

/// Add one column as a property of its own, if the files hold it and the
/// platform says enough about it.
#[expect(
    clippy::too_many_arguments,
    reason = "the state one pass of the build threads through"
)]
fn entries_column(
    properties: &mut Vec<CellProperty>,
    seen: &mut std::collections::HashSet<String>,
    by_column: &mut HashMap<String, Vec<Listed>>,
    columns: &CellColumns,
    extents: &HashMap<String, (f32, f32)>,
    histograms: &HashMap<String, Vec<u32>>,
    id: &str,
    title: Option<String>,
    shown: bool,
) {
    let Some(column) = columns.0.iter().find(|column| column.id == id) else {
        return;
    };
    if seen.contains(id) {
        return;
    }
    let kind = if column.numeric {
        let Some(&(low, high)) = extents.get(id) else {
            return;
        };
        let histogram = histograms.get(id).cloned().unwrap_or_default();
        PropertyKind::Numeric(NumericRange::full(low, high, histogram))
    } else {
        let Some(values) = by_column.remove(id) else {
            return;
        };
        PropertyKind::Categorical(values.into_iter().map(|found| found.value).collect())
    };
    seen.insert(id.to_string());
    properties.push(CellProperty {
        id: id.to_string(),
        name: title.unwrap_or_else(|| column.name.clone()),
        shown,
        kind,
        gene: None,
    });
}

/// Nest the values of `levels` under one another by their parents.
///
/// A parent is found by walking up the platform's links until one lands on a
/// value in a level held here, so a level missing from the files is stepped
/// over rather than cutting the tree apart. A value whose line reaches no such
/// parent becomes a root of its own rather than being lost.
fn tree_of(
    levels: &[(String, Option<String>)],
    by_column: &mut HashMap<String, Vec<Listed>>,
    parents: &HashMap<String, String>,
    columns: &CellColumns,
) -> Tree {
    let mut nodes: Vec<TreeNode> = Vec::new();
    let mut placed: HashMap<String, usize> = HashMap::new();
    for (level, (id, _)) in levels.iter().enumerate() {
        for listed in by_column.remove(id).unwrap_or_default() {
            let mut above = listed.parent.clone();
            let parent = loop {
                match above {
                    Some(reference) => match placed.get(&reference) {
                        Some(&node) => break Some(node),
                        None => above = parents.get(&reference).cloned(),
                    },
                    None => break None,
                }
            };
            if let Some(reference) = listed.reference {
                placed.insert(reference, nodes.len());
            }
            nodes.push(TreeNode {
                level,
                parent,
                value: listed.value,
            });
        }
    }
    Tree {
        levels: levels
            .iter()
            .map(|(id, title)| TreeLevel {
                id: id.clone(),
                name: title.clone().unwrap_or_else(|| {
                    columns
                        .0
                        .iter()
                        .find(|column| &column.id == id)
                        .map_or_else(|| id.clone(), |column| column.name.clone())
                }),
            })
            .collect(),
        nodes,
        color_level: 0,
    }
}

/// A color as the platform writes it: `#rrggbb`, in either case.
fn parse_color(text: &str) -> Option<Color> {
    Srgba::hex(text.trim()).ok().map(Color::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two groups of a real crossed answer: the API returns one row per
    /// pairing that occurs, each value named by the column it came from.
    const CROSSED_TEXT: &str = r#"[
      { "count": 2144, "properties": [
          { "property": "NEUROTRANSMITTER", "value": "Chol" },
          { "property": "CLASS", "value": "17 MH-LH Glut" } ] },
      { "count": 1778, "properties": [
          { "property": "NEUROTRANSMITTER", "value": "Chol" },
          { "property": "CLASS", "value": "08 CNU-MGE GABA" } ] },
      { "count": 6121, "properties": [
          { "property": "NEUROTRANSMITTER", "value": "Dopa" },
          { "property": "CLASS", "value": "05 OB-IMN GABA" } ] },
      { "count": 12, "properties": [
          { "property": "NEUROTRANSMITTER", "value": "Chol" },
          { "property": "CLASS", "value": "a class nobody described" } ] }
    ]"#;

    fn crossed() -> (Vec<(u16, u64)>, Vec<(u16, u16, u64)>) {
        let groups: Vec<Counted> = serde_json::from_str(CROSSED_TEXT).unwrap();
        let codes = HashMap::from([("Chol".to_string(), 7), ("Dopa".to_string(), 9)]);
        let colors = HashMap::from([
            ("17 MH-LH Glut".to_string(), 17),
            ("08 CNU-MGE GABA".to_string(), 8),
            ("05 OB-IMN GABA".to_string(), 5),
        ]);
        let (values, mixes) = tally(
            &groups,
            "NEUROTRANSMITTER",
            &codes,
            Some(("CLASS", &colors)),
        );
        // Owned, so the test's maps do not have to outlive the answer.
        (values, mixes)
    }

    #[test]
    fn a_crossed_count_adds_up_to_the_plain_one() {
        let (values, _) = crossed();
        // Chol's three groups, the undescribed class among them, and Dopa's one.
        assert_eq!(values, [(7, 2144 + 1778 + 12), (9, 6121)]);
    }

    #[test]
    fn a_crossed_count_keeps_the_pairings_it_can_name() {
        let (_, mixes) = crossed();
        assert_eq!(mixes, [(7, 17, 2144), (7, 8, 1778), (9, 5, 6121)]);
    }

    #[test]
    fn an_uncrossed_count_reads_the_one_value_a_group_holds() {
        let groups: Vec<Counted> =
            serde_json::from_str(r#"[{ "count": 40, "properties": [{ "value": "Braak 0" }] }]"#)
                .unwrap();
        let codes = HashMap::from([("Braak 0".to_string(), 5)]);
        let (values, mixes) = tally(&groups, "BRAAK", &codes, None);
        assert_eq!(values, [(5, 40)]);
        assert!(mixes.is_empty());
    }

    #[test]
    fn a_region_is_asked_for_as_the_box_the_api_takes() {
        // The API rejects anything but `[x,y,x,y]`, and names the field after
        // the visualization rather than the dataset; both were found by asking
        // it, so a change here has to be checked against it again.
        let region = SelectedRegion {
            key: "MGA5LUTH4ETM859L5IM".into(),
            min: Vec2::new(20.0, 30.0),
            max: Vec2::new(40.0, 50.0),
        };
        let filter = point_filter(&region);
        let condition = &filter[0];
        assert_eq!(condition["type"], "POINT");
        assert_eq!(condition["operator"], "CONTAINED_IN");
        assert_eq!(condition["field"], "MGA5LUTH4ETM859L5IM_point");
        assert_eq!(condition["value"], "[40,50,20,30]");
    }

    /// How many cells a round of counts came to, however they were grouped.
    #[cfg(test)]
    fn counted(counts: &CellCounts) -> u64 {
        counts
            .values
            .iter()
            .flat_map(|(_, values)| values.iter().map(|(_, count)| *count))
            .sum()
    }

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
    fn a_region_dragged_backwards_asks_the_same_question() {
        let key = "V".to_string();
        let forward = point_filter(&SelectedRegion {
            key: key.clone(),
            min: Vec2::new(20.0, 30.0),
            max: Vec2::new(40.0, 50.0),
        });
        let backward = point_filter(&SelectedRegion {
            key,
            min: Vec2::new(40.0, 50.0),
            max: Vec2::new(20.0, 30.0),
        });
        assert_eq!(forward, backward);
    }
    use crate::source::properties::{CellColumn, Column, RangeEnd};

    fn column(id: &str, numeric: bool) -> CellColumn {
        CellColumn {
            id: id.into(),
            name: format!("file {id}"),
            numeric,
        }
    }

    /// Trimmed from the live answers for SEA-AD CaH.
    const DISPLAY_TEXT: &str = r#"{
        "defaultColorBy": {"referenceId": "LEVEL_1"},
        "displayFeatures": [
          {"isDefault": true, "priorityOrder": 7,
           "featureType": {"referenceId": "BRAAK", "title": "Braak"}},
          {"isDefault": true, "priorityOrder": 9,
           "featureType": {"referenceId": "CPS", "title": "CPS"}},
          {"isDefault": true, "priorityOrder": 0,
           "featureType": {"referenceId": "TAX", "title": "SEA-AD CaH Taxonomy"},
           "featureSet": [
             {"priorityOrder": 2, "featureType": {"referenceId": "LEVEL_1", "title": "Subclass"}},
             {"priorityOrder": 1, "featureType": {"referenceId": "LEVEL_0", "title": "Neighborhood"}}
           ]},
          {"isDefault": true, "priorityOrder": 3,
           "featureType": {"referenceId": "GENES", "title": "Genes"}}
        ]
    }"#;

    const VALUES_TEXT: &str = r##"[
        {"color": "#d52221", "featureType": {"referenceId": "BRAAK"},
         "featureTypeValueIndex": {"value": "Braak IV", "index": 3, "priorityOrder": 5}},
        {"color": "#fedbcb", "featureType": {"referenceId": "BRAAK"},
         "featureTypeValueIndex": {"value": "Braak 0", "index": 5, "priorityOrder": 1}},
        {"color": "#1655F2", "featureType": {"referenceId": "LEVEL_1"},
         "featureTypeValueIndex": {"value": "STR D1 MSN", "index": 15, "priorityOrder": 1,
                                   "referenceId": "SCLA_01", "parentReferenceId": "NEIG_01"}},
        {"color": "#1655F2", "featureType": {"referenceId": "LEVEL_1"},
         "featureTypeValueIndex": {"value": "STR D1 MSN", "index": 15, "priorityOrder": 1,
                                   "referenceId": "SCLA_01", "parentReferenceId": "NEIG_01"}},
        {"color": "#8D6C62", "featureType": {"referenceId": "LEVEL_1"},
         "featureTypeValueIndex": {"value": "Endothelial", "index": 1, "priorityOrder": 14,
                                   "referenceId": "SCLA_14", "parentReferenceId": "NEIG_07"}},
        {"color": null, "featureType": {"referenceId": "LEVEL_0"},
         "featureTypeValueIndex": {"value": "Neurons", "index": 0, "priorityOrder": 1,
                                   "referenceId": "NEIG_01"}},
        {"color": null, "featureType": {"referenceId": "LEVEL_0"},
         "featureTypeValueIndex": {"value": "Vascular", "index": 3, "priorityOrder": 7,
                                   "referenceId": "NEIG_07"}},
        {"color": "#000000", "featureType": {"referenceId": "DONOR"},
         "featureTypeValueIndex": {"value": "H20.33.046", "index": 37, "priorityOrder": 24}}
    ]"##;

    fn described() -> CellProperties {
        let columns = CellColumns(vec![
            column("DONOR", false),
            column("BRAAK", false),
            column("LEVEL_0", false),
            column("LEVEL_1", false),
            column("CPS", true),
            column("UNKNOWN", false),
        ]);
        build(
            &columns,
            serde_json::from_str(DISPLAY_TEXT).unwrap(),
            serde_json::from_str(VALUES_TEXT).unwrap(),
            &HashMap::from([("CPS".to_string(), (0.0, 1.0))]),
            &HashMap::from([("CPS".to_string(), vec![1, 2, 3])]),
        )
    }

    #[test]
    fn properties_follow_the_portals_order_with_a_taxonomy_as_one_tree() {
        let properties = described();
        let ids: Vec<&str> = properties
            .properties
            .iter()
            .map(|property| property.id.as_str())
            .collect();
        // The taxonomy first, as one property; the genes, which the files hold
        // no column for, nowhere; the donor, which the portal does not list,
        // last and hidden; and the column the platform knows nothing about,
        // not at all.
        assert_eq!(ids, ["TAX", "BRAAK", "CPS", "DONOR"]);
        assert_eq!(properties.properties[0].name, "SEA-AD CaH Taxonomy");
        assert!(!properties.properties[3].shown);
    }

    #[test]
    fn a_taxonomy_nests_its_levels_by_their_parents() {
        let properties = described();
        let tree = properties.properties[0].tree().unwrap();
        let names: Vec<&str> = tree
            .levels
            .iter()
            .map(|level| level.name.as_str())
            .collect();
        assert_eq!(names, ["Neighborhood", "Subclass"]);

        let label = |node: usize| tree.nodes[node].value.label.as_str();
        let roots: Vec<&str> = tree.children(None).map(label).collect();
        assert_eq!(roots, ["Neurons", "Vascular"]);
        let neurons = tree.children(None).next().unwrap();
        let under: Vec<&str> = tree.children(Some(neurons)).map(label).collect();
        assert_eq!(under, ["STR D1 MSN"], "listed once, though sent twice");

        // Filtering happens in the finest column.
        let mut properties = properties;
        properties.properties[0]
            .tree_mut()
            .unwrap()
            .set(neurons, true);
        let selection = properties.selection();
        assert_eq!(selection.filters.len(), 1);
        assert_eq!(selection.filters[0].0, Column::Cell("LEVEL_1".into()));
    }

    #[test]
    fn values_carry_the_platforms_labels_colors_and_order() {
        let properties = described();
        let braak = &properties.properties[1];
        let labels: Vec<(&str, u16)> = braak
            .values()
            .iter()
            .map(|value| (value.label.as_str(), value.code))
            .collect();
        assert_eq!(labels, [("Braak 0", 5), ("Braak IV", 3)]);
        assert_eq!(
            braak.values()[1].color,
            Some(Color::from(Srgba::hex("d52221").unwrap()))
        );
        // A value with no color of its own falls back to the default palette.
        let tree = properties.properties[0].tree().unwrap();
        assert_eq!(tree.nodes[0].value.color, None);
    }

    #[test]
    fn coloring_starts_where_the_portal_says() {
        let properties = described();
        assert_eq!(
            properties
                .selection()
                .color_by
                .as_ref()
                .and_then(Column::cell),
            Some("LEVEL_1"),
            "the portal's default, not the first listed"
        );
        // And draws in the platform's colors.
        let palette = properties.selection().palette;
        let d1 = Color::from(Srgba::hex("1655F2").unwrap()).to_linear();
        assert_eq!(palette[15], [d1.red, d1.green, d1.blue, 1.0]);
    }

    #[test]
    fn a_numeric_property_takes_the_platforms_extent_and_histogram() {
        let properties = described();
        let range = properties.properties[2].range().unwrap();
        assert_eq!((range.low, range.high), (0.0, 1.0));
        assert_eq!(range.histogram, [1, 2, 3]);
        assert!(!range.restricts());
    }

    #[test]
    fn counted_ranges_are_placed_by_their_low_end_whatever_their_text() {
        let edges = bucket_edges((68.0, 99.0));
        assert_eq!(edges.len(), BUCKETS + 1);
        assert!(edges[BUCKETS] > 99.0, "the maximum must fall in a bucket");

        let counted: Vec<Counted> = serde_json::from_str(
            r#"[
              {"count": 7, "properties": [{"value": "[68,68.96875]"}]},
              {"count": 3, "properties": [{"value": "[98.03125,99.000031]"}]},
              {"count": 1, "properties": []}
            ]"#,
        )
        .unwrap();
        let buckets = bin(&edges, &counted);
        assert_eq!(buckets[0], 7);
        assert_eq!(buckets[BUCKETS - 1], 3);
        assert_eq!(buckets.iter().sum::<u32>(), 10);
    }

    #[test]
    fn a_gene_is_searched_for_however_its_species_cases_it() {
        assert_eq!(prefixes(" gad "), ["GAD", "Gad", "gad"]);
        // Nothing asked twice when the typing already matches a convention.
        assert_eq!(prefixes("GAD"), ["GAD", "Gad"]);
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
    fn a_column_of_one_value_still_has_a_bucket_to_count_in() {
        let edges = bucket_edges((5.0, 5.0));
        assert!(edges[BUCKETS] > edges[0]);
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

    fn values_counted(counts: &CellCounts, column: &str, code: u16) -> u64 {
        counts
            .values
            .iter()
            .find(|(id, _)| id == column)
            .and_then(|(_, counts)| counts.iter().find(|(found, _)| *found == code))
            .map_or(0, |(_, count)| *count)
    }

    #[test]
    fn a_filter_names_values_by_label_and_ticked_nodes_at_their_level() {
        let mut properties = described();
        assert!(
            properties
                .properties
                .iter()
                .all(|p| cell_filter(p).is_none())
        );

        let tree = properties.properties[0].tree_mut().unwrap();
        let neurons = tree.children(None).next().unwrap();
        tree.set(neurons, true);
        assert_eq!(
            cell_filter(&properties.properties[0]),
            Some(json!([
                { "type": "METADATA", "field": "LEVEL_0", "operator": "EQ", "value": "Neurons" }
            ]))
        );

        let cps = properties.properties[2].range_mut().unwrap();
        cps.set_end(RangeEnd::To, 0.5);
        let filter = cell_filter(&properties.properties[2]).unwrap();
        let range = filter[0]["value"].as_str().unwrap();
        assert!(range.starts_with("[0,0.5000"), "{range} keeps its high end");
        assert_eq!(filter[0]["operator"], "BETWEEN");
    }

    #[test]
    fn a_gene_filters_by_its_index() {
        let property = CellProperty {
            id: "ENSG00000128683".into(),
            name: "GAD1".into(),
            shown: true,
            kind: PropertyKind::Numeric(NumericRange {
                from: 1.0,
                ..NumericRange::full(0.0, 4.0, Vec::new())
            }),
            gene: Some(11618),
        };
        let filter = cell_filter(&property).unwrap();
        assert_eq!(filter[0]["type"], "GENE");
        assert_eq!(filter[0]["field"], "11618");
    }
}
