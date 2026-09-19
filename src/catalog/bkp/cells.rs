//! What the Brain Knowledge Platform says about a dataset's cells.
//!
//! A Scatterbrain file stores each categorical column as codes, with nothing
//! naming them. The platform holds the rest, keyed by the same column ids:
//!
//! - `getDisplayProperty` — which properties the portal shows, in what order,
//!   which are levels of one taxonomy, and which one it colours by first;
//! - `cellProperties` — each code's label, colour and place in its list;
//! - `numericProperties` — the extent of each numeric column;
//! - `cellRangeCounts` and `cellCounts` — how many cells fall in each bucket of
//!   a numeric column, and hold each value of a categorical one.
//!
//! The first four answer in well under a second, so they are asked together
//! and make up [`BkpCells::describe`]. Counting every categorical value takes
//! seconds, so it is [`BkpCells::count`], asked once the labels are showing.

use std::collections::HashMap;

use bevy::prelude::*;
use futures::future::{BoxFuture, join_all, try_join3};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::post;
use crate::catalog::{CellCounts, DescribeCells};
use crate::source::properties::{
    CellColumns, CellProperties, CellProperty, NumericRange, PropertyKind, PropertyValue,
};

/// The most value records asked for in one page. The API allows far more than
/// the 50 its dataset listing does, and a taxonomy with thousands of clusters
/// would otherwise take a hundred round trips.
const VALUE_PAGE: usize = 5000;

/// Enough pages for a quarter of a million values, so a cursor that never
/// ends cannot keep a lookup going forever.
const MAX_VALUE_PAGES: usize = 50;

/// Buckets in a numeric property's histogram.
const BUCKETS: usize = 32;

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
                    .map(|(id, extent)| histogram(&endpoint, &filter, id, *extent)),
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

    fn count(&self, properties: &CellProperties) -> BoxFuture<'static, Result<CellCounts, String>> {
        let endpoint = self.endpoint.clone();
        let filter = self.dataset_filter();
        // Counts come back keyed by label, so each property takes its labels
        // along to turn them back into codes.
        let wanted: Vec<(String, HashMap<String, u16>)> = properties
            .properties
            .iter()
            .filter(|property| property.is_categorical())
            .map(|property| {
                let codes = property
                    .values()
                    .iter()
                    .map(|value| (value.label.clone(), value.code))
                    .collect();
                (property.id.clone(), codes)
            })
            .collect();
        Box::pin(async move {
            let counted = join_all(
                wanted
                    .iter()
                    .map(|(id, codes)| counts(&endpoint, &filter, id, codes)),
            )
            .await;
            let mut found = Vec::new();
            let mut last_error = None;
            for ((id, _), result) in wanted.into_iter().zip(counted) {
                match result {
                    Ok(counts) => found.push((id, counts)),
                    Err(e) => last_error = Some(e),
                }
            }
            match last_error {
                Some(e) if found.is_empty() => Err(e),
                _ => Ok(found),
            }
        })
    }
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
    let text = post(endpoint, body.to_string()).await?;
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
      featureTypeValueIndex { value index priorityOrder }
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

const RANGE_COUNTS: &str = "query($filter: DatasetFilter!, $field: String!, $range: [String]!) {
  cellRangeCounts(datasetFilter: $filter, groupBy: { field: $field, range: $range }) {
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
    value: Option<String>,
}

/// Counts in [`BUCKETS`] equal buckets across `extent`.
async fn histogram(
    endpoint: &str,
    filter: &Value,
    field: &str,
    extent: (f32, f32),
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
    let variables = json!({ "filter": filter, "field": field, "range": range });
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

const COUNTS: &str = "query($filter: DatasetFilter!, $field: String!) {
  cellCounts(datasetFilter: $filter, groupBy: [$field]) {
    count
    properties { value }
  }
}";

/// How many cells hold each value of one categorical column, by code.
async fn counts(
    endpoint: &str,
    filter: &Value,
    field: &str,
    codes: &HashMap<String, u16>,
) -> Result<Vec<(u16, u64)>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        cell_counts: Vec<Counted>,
    }

    let variables = json!({ "filter": filter, "field": field });
    let data: Data = ask(endpoint, COUNTS, variables).await?;
    Ok(data
        .cell_counts
        .iter()
        .filter_map(|counted| {
            let label = counted.properties.first()?.value.as_deref()?;
            Some((*codes.get(label)?, counted.count as u64))
        })
        .collect())
}

/// Assemble the properties the portal would show, for the columns the files
/// hold.
///
/// Listed in the portal's order: a taxonomy expands in place into its levels. Columns the portal does not show are kept
/// but hidden, so the section's menu can still offer them, and only if the
/// platform knows enough about them to be worth offering.
fn build(
    columns: &CellColumns,
    display: Display,
    values: Vec<ValueRecord>,
    extents: &HashMap<String, (f32, f32)>,
    histograms: &HashMap<String, Vec<u32>>,
) -> CellProperties {
    let mut by_column: HashMap<String, Vec<(i32, PropertyValue)>> = HashMap::new();
    for record in values {
        let index = record.feature_type_value_index;
        let Ok(code) = u16::try_from(index.index) else {
            continue;
        };
        by_column
            .entry(record.feature_type.reference_id)
            .or_default()
            .push((
                index.priority_order.unwrap_or(i32::MAX),
                PropertyValue {
                    code,
                    label: index.value,
                    colour: record.color.as_deref().and_then(parse_colour),
                    count: None,
                    selected: false,
                },
            ));
    }

    // One entry per column, in the order the portal lists them.
    struct Listed {
        id: String,
        title: Option<String>,
        shown: bool,
    }
    let mut features = display.display_features;
    features.sort_by_key(|feature| feature.priority_order.unwrap_or(i32::MAX));
    let mut listed: Vec<Listed> = Vec::new();
    for feature in features {
        match feature.feature_set {
            Some(mut levels) if !levels.is_empty() => {
                levels.sort_by_key(|level| level.priority_order.unwrap_or(i32::MAX));
                for level in levels {
                    listed.push(Listed {
                        id: level.feature_type.reference_id,
                        title: level.feature_type.title,
                        shown: feature.is_default,
                    });
                }
            }
            _ => listed.push(Listed {
                id: feature.feature_type.reference_id,
                title: feature.feature_type.title,
                shown: feature.is_default,
            }),
        }
    }
    for column in &columns.0 {
        listed.push(Listed {
            id: column.id.clone(),
            title: None,
            shown: false,
        });
    }

    let mut properties = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in listed {
        let Some(column) = columns.0.iter().find(|column| column.id == entry.id) else {
            continue;
        };
        if !seen.insert(entry.id.clone()) {
            continue;
        }
        let kind = if column.numeric {
            let Some(&(low, high)) = extents.get(&column.id) else {
                continue;
            };
            let histogram = histograms.get(&column.id).cloned().unwrap_or_default();
            PropertyKind::Numeric(NumericRange::full(low, high, histogram))
        } else {
            let Some(mut values) = by_column.remove(&column.id) else {
                continue;
            };
            values.sort_by_key(|(priority, value)| (*priority, value.code));
            PropertyKind::Categorical(values.into_iter().map(|(_, value)| value).collect())
        };
        properties.push(CellProperty {
            id: column.id.clone(),
            name: entry.title.unwrap_or_else(|| column.name.clone()),
            shown: entry.shown,
            kind,
        });
    }

    let mut described = CellProperties::ready(properties);
    if let Some(default) = display.default_color_by {
        described.colour_by_id(&default.reference_id);
    }
    described
}

/// A colour as the platform writes it: `#rrggbb`, in either case.
fn parse_colour(text: &str) -> Option<Color> {
    Srgba::hex(text.trim()).ok().map(Color::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::properties::CellColumn;

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
         "featureTypeValueIndex": {"value": "STR D1 MSN", "index": 15, "priorityOrder": 1}},
        {"color": null, "featureType": {"referenceId": "LEVEL_0"},
         "featureTypeValueIndex": {"value": "Neurons", "index": 0, "priorityOrder": 1}},
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
    fn properties_follow_the_portals_order_with_taxonomy_levels_in_place() {
        let properties = described();
        let ids: Vec<&str> = properties
            .properties
            .iter()
            .map(|property| property.id.as_str())
            .collect();
        // The taxonomy first, its levels in their own order; the genes, which
        // the files hold no column for, nowhere; the donor, which the portal
        // does not list, last and hidden; and the column the platform knows
        // nothing about, not at all.
        assert_eq!(ids, ["LEVEL_0", "LEVEL_1", "BRAAK", "CPS", "DONOR"]);
        let level = &properties.properties[1];
        assert_eq!(level.name, "Subclass");
        assert!(!properties.properties[4].shown);
    }

    #[test]
    fn values_carry_the_platforms_labels_colours_and_order() {
        let properties = described();
        let braak = &properties.properties[2];
        let labels: Vec<(&str, u16)> = braak
            .values()
            .iter()
            .map(|value| (value.label.as_str(), value.code))
            .collect();
        assert_eq!(labels, [("Braak 0", 5), ("Braak IV", 3)]);
        assert_eq!(
            braak.values()[1].colour,
            Some(Color::from(Srgba::hex("d52221").unwrap()))
        );
        // A value with no colour of its own falls back to the default palette.
        assert_eq!(properties.properties[0].values()[0].colour, None);
    }

    #[test]
    fn colouring_starts_where_the_portal_says() {
        let properties = described();
        assert_eq!(
            properties.selection().colour_by.as_deref(),
            Some("LEVEL_1"),
            "the portal's default, not the first listed"
        );
        // And draws in the platform's colours.
        let palette = properties.selection().palette;
        let d1 = Color::from(Srgba::hex("1655F2").unwrap()).to_linear();
        assert_eq!(palette[15], [d1.red, d1.green, d1.blue, 1.0]);
    }

    #[test]
    fn a_numeric_property_takes_the_platforms_extent_and_histogram() {
        let properties = described();
        let range = properties.properties[3].range().unwrap();
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
            properties.selection().colour_by.as_deref(),
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
            .iter()
            .find(|(id, _)| id == "E4E313ECEDC2194BBE")
            .map(|(_, counts)| counts.iter().map(|(_, count)| count).sum())
            .unwrap();
        assert_eq!(braak, 686_439);
    }
}
