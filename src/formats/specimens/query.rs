//! The page query: what is asked of the platform, the shape its answer comes
//! back in, and the one request that reads a page.

use super::*;

pub(super) const QUERY: &str = "query($project: [Filter], $specimens: [Filter], $sort: [Sort],
  $groupBy: [groupBy_List_String_pattern_id], $limit: Int, $offset: Int) {
  project: dataCollectionProjectInventory(filter: $project, limit: 1) {
    referenceId
    title
  }
  total: aio_specimenCounts(filter: $specimens, groupBy: $groupBy) {
    count
  }
  aio_specimen(filter: $specimens, sort: $sort, limit: $limit, offset: $offset) {
    cRID { symbol }
    specimenType { name }
    annotations {
      featureType { referenceId title }
      taxons { symbol }
    }
    measurements {
      featureType { referenceId title }
      value
      unit
    }
  }
}";

/// Read a list that may arrive as `null` rather than as an empty one.
///
/// The platform writes null where a record has nothing, and serde's `default`
/// covers a field that is *missing* rather than one that is present and null.
/// Found on the last page of the Genetic Tools Atlas, where a specimen has no
/// measurements at all.
pub(super) fn maybe_list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Deserialize)]
pub(super) struct Data {
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) project: Vec<Project>,
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) total: Vec<Aggregate>,
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) aio_specimen: Vec<Specimen>,
}

#[derive(Deserialize)]
pub(super) struct Aggregate {
    pub(super) count: Option<f64>,
}

impl Data {
    /// How many specimens the project holds, as the platform counted them.
    ///
    /// Grouped by the project itself, so there is one figure to take; summed
    /// rather than indexed in case the grouping ever splits.
    pub(super) fn counted(&self) -> Option<usize> {
        let total: f64 = self.total.iter().filter_map(|it| it.count).sum();
        (total > 0.0).then_some(total as usize)
    }
}

#[derive(Deserialize)]
pub(super) struct Project {
    pub(super) title: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Specimen {
    #[serde(rename = "cRID")]
    pub(super) crid: Option<Crid>,
    pub(super) specimen_type: Option<Named>,
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) annotations: Vec<Annotation>,
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) measurements: Vec<Measurement>,
}

#[derive(Deserialize)]
pub(super) struct Crid {
    pub(super) symbol: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Named {
    pub(super) name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Annotation {
    pub(super) feature_type: Titled,
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) taxons: Vec<Taxon>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Measurement {
    pub(super) feature_type: Titled,
    pub(super) value: Option<String>,
    pub(super) unit: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Titled {
    /// Only annotations carry one through: it is what a filter narrows by.
    #[serde(default)]
    pub(super) reference_id: Option<String>,
    pub(super) title: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Taxon {
    pub(super) symbol: Option<String>,
}

/// One request: the project's title, how many specimens match, and a page of
/// them.
///
/// `terms` are what has been asked of the table. Two terms on one column
/// widen — the platform treats a field named twice as either — and terms on
/// different columns narrow each other, which is what a row of checkboxes and
/// a span beside it are read to mean.
///
/// `sort` is the platform's own list of fields and orders, as [`Plan::sort`]
/// writes it.
pub(super) async fn ask(
    endpoint: &str,
    scope: &Scope,
    offset: usize,
    terms: &[TableFilterTerm],
    sort: Value,
) -> Result<Data, String> {
    let variables = json!({
        "project": [{ "field": "referenceId", "operator": "EQ", "value": scope.project }],
        "specimens": specimen_filters(scope, terms),
        "sort": sort,
        "groupBy": ["projectReferenceIds"],
        "limit": PAGE,
        "offset": offset,
    });
    graphql::ask(endpoint, QUERY, variables).await
}

/// One group of an `aio_specimenCounts` answer: a value and how many rows
/// hold it.
#[derive(Deserialize)]
pub(super) struct Grouped {
    pub(super) count: Option<f64>,
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) properties: Vec<Property>,
}

#[derive(Deserialize)]
pub(super) struct Property {
    pub(super) value: Option<String>,
}

/// Each value a column's groups name, with its count; blank values dropped.
pub(super) fn labeled(groups: &[Grouped]) -> Vec<(String, u64)> {
    groups
        .iter()
        .filter_map(|group| {
            let label = group.properties.first()?.value.clone()?;
            (!label.trim().is_empty()).then(|| (label, group.count.unwrap_or_default() as u64))
        })
        .collect()
}

/// Which specimens a table is of: a project's, and of those only one kind
/// when the project shows its kinds apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scope {
    pub(super) project: String,
    /// The kind of specimen, by the platform's id for it.
    pub(super) kind: Option<String>,
}

impl Scope {
    pub(super) fn project(project: impl Into<String>) -> Self {
        Scope {
            project: project.into(),
            kind: None,
        }
    }
}

/// The specimens in scope, and whatever has been asked of them.
pub(super) fn specimen_filters(scope: &Scope, terms: &[TableFilterTerm]) -> Value {
    let mut filters = vec![json!({
        "field": "projectReferenceIds", "operator": "EQ", "value": scope.project
    })];
    if let Some(kind) = &scope.kind {
        filters.push(json!({ "field": KIND_ID_FIELD, "operator": "EQ", "value": kind }));
    }
    filters.extend(terms.iter().map(|term| match term {
        TableFilterTerm::Is { field, value } => {
            json!({ "field": field, "operator": "EQ", "value": value })
        }
        // The platform takes a span as one string holding both ends.
        TableFilterTerm::Between { field, low, high } => {
            json!({ "field": field, "operator": "BETWEEN", "value": format!("[{low},{high}]") })
        }
    }));
    Value::Array(filters)
}

/// How a project shows its kinds of specimen apart, and how many of each it
/// holds.
///
/// `getSpecimenTypeDisplayPropertiesByProject` is what the platform's own
/// specimens page lays a project out from: for a project that shows its kinds
/// apart, a list of them, each with the features it is shown with. For every
/// other project it answers with an empty list.
pub(super) const KINDS: &str = "query($project: String!, $specimens: [Filter]) {
  kinds: getSpecimenTypeDisplayPropertiesByProject(projectReferenceId: $project) {
    priorityOrder
    referenceId
    title
    displayFeatures {
      type
      priorityOrder
      featureType { referenceId title }
      ... on MeasurementDisplayProperty { unit }
    }
  }
  counts: aio_specimenCounts(filter: $specimens, groupBy: [\"specimenType.referenceId\"]) {
    count
    properties { value }
  }
}";

#[derive(Deserialize)]
pub(super) struct KindsData {
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) kinds: Vec<KindLayout>,
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) counts: Vec<Grouped>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct KindLayout {
    #[serde(default)]
    pub(super) priority_order: Option<i64>,
    pub(super) reference_id: String,
    pub(super) title: Option<String>,
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) display_features: Vec<DisplayFeature>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DisplayFeature {
    #[serde(rename = "type")]
    pub(super) kind: Option<String>,
    #[serde(default)]
    pub(super) priority_order: Option<i64>,
    pub(super) feature_type: Titled,
    #[serde(default)]
    pub(super) unit: Option<String>,
}

/// Ask how a project shows its kinds of specimen apart.
pub(super) async fn ask_kinds(endpoint: &str, project: &str) -> Result<KindsData, String> {
    let variables = json!({
        "project": project,
        "specimens": specimen_filters(&Scope::project(project), &[]),
    });
    graphql::ask(endpoint, KINDS, variables).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_the_platform_writes_as_null_reads_as_an_empty_one() {
        // A specimen with no measurements at all, which is what the last page
        // of the Genetic Tools Atlas holds.
        let text = r#"{"data":{"project":null,"total":null,"aio_specimen":[
            {"cRID":{"symbol":"X"},"specimenType":null,
             "annotations":null,"measurements":null}]}}"#;
        let value: Value = serde_json::from_str(text).unwrap();
        let data: Data = serde_json::from_value(value["data"].clone()).unwrap();
        assert_eq!(data.aio_specimen.len(), 1);
        assert!(data.aio_specimen[0].annotations.is_empty());
        assert!(data.aio_specimen[0].measurements.is_empty());
        assert_eq!(data.counted(), None);
    }
}
