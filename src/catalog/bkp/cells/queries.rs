//! The GraphQL documents asked of the platform, the shapes their answers
//! come back in, and the reads that page through them.

use super::*;

/// The most value records asked for in one page. The API allows far more than
/// the 50 its dataset listing does, and a taxonomy with thousands of clusters
/// would otherwise take a hundred round trips.
pub(super) const VALUE_PAGE: usize = 5000;

/// Enough pages for a quarter of a million values, so a cursor that never
/// ends cannot keep a lookup going forever.
pub(super) const MAX_VALUE_PAGES: usize = 50;

pub(super) const DISPLAY: &str = "query($filter: DisplayPropertyFilter!) {
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
pub(super) struct Display {
    pub(super) default_color_by: Option<FeatureType>,
    #[serde(default)]
    pub(super) display_features: Vec<Feature>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Feature {
    pub(super) is_default: bool,
    pub(super) priority_order: Option<i32>,
    pub(super) feature_type: FeatureType,
    /// The levels of a taxonomy or other hierarchy; absent for a plain column.
    pub(super) feature_set: Option<Vec<Level>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Level {
    pub(super) priority_order: Option<i32>,
    pub(super) feature_type: FeatureType,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FeatureType {
    pub(super) reference_id: String,
    pub(super) title: Option<String>,
}

pub(super) async fn display(
    endpoint: &str,
    dataset: &str,
    project: &str,
) -> Result<Display, String> {
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

pub(super) const VALUES: &str = "query($dataset: String!, $first: Int, $after: String) {
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
pub(super) struct ValueRecord {
    pub(super) color: Option<String>,
    pub(super) feature_type: FeatureType,
    pub(super) feature_type_value_index: ValueIndex,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ValueIndex {
    pub(super) value: String,
    /// The code the files store for this value.
    pub(super) index: i64,
    pub(super) priority_order: Option<i32>,
    pub(super) reference_id: Option<String>,
    /// The value a level up in a hierarchy, by its `reference_id`.
    pub(super) parent_reference_id: Option<String>,
}

pub(super) async fn values(endpoint: &str, dataset: &str) -> Result<Vec<ValueRecord>, String> {
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

pub(super) const EXTENTS: &str = "query($dataset: String!) {
  numericProperties(first: 1000, where: { dataset: { referenceId: { eq: $dataset } } }) {
    nodes { min max featureType { referenceId } }
  }
}";

/// Each numeric column's extent across the whole dataset, by column id.
pub(super) async fn extents(
    endpoint: &str,
    dataset: &str,
) -> Result<HashMap<String, (f32, f32)>, String> {
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

pub(super) const GENES: &str = "query($collection: String!, $version: String!,
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
pub(super) fn prefixes(text: &str) -> Vec<String> {
    let text = text.trim();
    let mut capitalized: String = text.chars().take(1).flat_map(char::to_uppercase).collect();
    capitalized.extend(text.chars().skip(1).flat_map(char::to_lowercase));
    let mut prefixes = vec![text.to_string(), text.to_uppercase(), capitalized];
    prefixes.sort();
    prefixes.dedup();
    prefixes
}

pub(super) const RANGE_COUNTS: &str =
    "query($filter: DatasetFilter!, $field: String!, $range: [String]!,
                                $filters: [[CellFilterInput!]]) {
  cellRangeCounts(datasetFilter: $filter, groupBy: { field: $field, range: $range },
                  filters: $filters) {
    count
    properties { value }
  }
}";

#[derive(Deserialize)]
pub(super) struct Counted {
    pub(super) count: f64,
    #[serde(default)]
    pub(super) properties: Vec<Tuple>,
}

#[derive(Deserialize)]
pub(super) struct Tuple {
    /// Which column this value came from, for a group crossing two.
    #[serde(default)]
    pub(super) property: Option<String>,
    pub(super) value: Option<String>,
}

/// The cells themselves, rather than a count of them.
///
/// Each value comes back named by its column and already as the label the
/// platform shows, so a record needs no translation from the codes the files
/// store.
pub(super) const CELL_INFO: &str = "query($filter: DatasetFilter!, $properties: [String!],
                              $filters: [[CellFilterInput!]], $limit: Int) {
  cellInfo(datasetFilter: $filter, properties: $properties,
           filters: $filters, limit: $limit) {
    id
    index
    properties { property value }
  }
}";

pub(super) const COUNTS: &str = "query($filter: DatasetFilter!, $fields: [String!],
                          $filters: [[CellFilterInput!]]) {
  cellCounts(datasetFilter: $filter, groupBy: $fields, filters: $filters) {
    count
    properties { property value }
  }
}";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gene_is_searched_for_however_its_species_cases_it() {
        assert_eq!(prefixes(" gad "), ["GAD", "Gad", "gad"]);
        // Nothing asked twice when the typing already matches a convention.
        assert_eq!(prefixes("GAD"), ["GAD", "Gad"]);
    }
}
