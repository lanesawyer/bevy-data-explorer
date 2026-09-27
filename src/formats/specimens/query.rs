//! The page query: what is asked of the platform, the shape its answer comes
//! back in, and the one request that reads a page.

use super::*;

/// A page of specimens: the project's title, how many match, and the page.
///
/// Read with the platform's search in place of its listing when there is
/// text to search for. The search is asked for twice, since it has no count
/// of its own: once for the page, and once for as many of its matches as it
/// will send — which is also as far into them as it will page — named and no
/// more, to count them. That second read is left out when only the page has
/// turned, since the count cannot have changed.
pub(super) const QUERY: &str = "query($project: [Filter], $specimens: [Filter], $sort: [Sort],
  $groupBy: [groupBy_List_String_pattern_id], $limit: Int, $offset: Int,
  $search: String!, $searching: Boolean!, $countHits: Boolean!, $window: Int) {
  project: dataCollectionProjectInventory(filter: $project, limit: 1) {
    referenceId
    title
  }
  total: aio_specimenCounts(filter: $specimens, groupBy: $groupBy) @skip(if: $searching) {
    count
  }
  hits: aio_specimenSearch(query: $search, filter: $specimens, limit: $window)
    @include(if: $countHits) {
    cRID { symbol }
  }
  aio_specimen(filter: $specimens, sort: $sort, limit: $limit, offset: $offset)
    @skip(if: $searching) {
    ...Row
  }
  found: aio_specimenSearch(query: $search, filter: $specimens, sort: $sort, limit: $limit,
    offset: $offset) @include(if: $searching) {
    ...Row
  }
}
fragment Row on AIO_Specimen {
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
}";

/// The most matches the platform's search sends, and so the furthest into
/// them it pages: past this it refuses the request outright ("Result window
/// is too large"), measured 2026-09-27.
pub(super) const SEARCH_WINDOW: usize = 10_000;

/// Text to search specimens for, and whether to count what it matches.
#[derive(Clone, Debug)]
pub(super) struct Searching {
    pub(super) text: String,
    pub(super) count: bool,
}

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
    /// A search's page, which becomes `aio_specimen` once read.
    #[serde(default, deserialize_with = "maybe_list")]
    found: Vec<Specimen>,
    /// A search's matches, named and no more, when they were counted.
    #[serde(default)]
    hits: Option<Vec<Value>>,
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
        if let Some(hits) = &self.hits {
            return Some(hits.len());
        }
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
///
/// `search` reads the page through the platform's search instead, which
/// matches whole words in any of a specimen's values, and takes `*`, quotes
/// and `AND` as a search box on the platform does.
pub(super) async fn ask(
    endpoint: &str,
    scope: &Scope,
    offset: usize,
    terms: &[TableFilterTerm],
    sort: Value,
    search: Option<Searching>,
) -> Result<Data, String> {
    let variables = json!({
        "project": [{ "field": "referenceId", "operator": "EQ", "value": scope.project }],
        "specimens": specimen_filters(scope, terms),
        "sort": sort,
        "groupBy": ["projectReferenceIds"],
        "limit": PAGE,
        "offset": offset,
        "search": search.as_ref().map_or("", |it| it.text.as_str()),
        "searching": search.is_some(),
        "countHits": search.as_ref().is_some_and(|it| it.count),
        "window": SEARCH_WINDOW,
    });
    let mut data: Data = graphql::ask(endpoint, QUERY, variables).await?;
    if search.is_some() {
        data.aio_specimen = std::mem::take(&mut data.found);
    }
    Ok(data)
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
            let end = |end: f32, open: &str| {
                if end.is_finite() {
                    end.to_string()
                } else {
                    open.to_string()
                }
            };
            let (low, high) = (end(*low, FLOOR), end(*high, CEILING));
            json!({ "field": field, "operator": "BETWEEN", "value": format!("[{low},{high}]") })
        }
    }));
    Value::Array(filters)
}

/// How the platform lays a project's specimens out, and how many of each kind
/// it holds.
///
/// Two answers, since a project is laid out one of two ways and the other
/// comes back empty. `getSpecimenTypeDisplayPropertiesByProject` lists the
/// kinds of specimen a project shows apart, each with the features it is
/// shown with; every project that does not show kinds apart answers with an
/// empty list. `getDisplayProperty` of the project lays out one table of
/// every kind, and answers null for a project that shows its kinds apart.
/// Either is what the platform's own specimens page lays the table out from:
/// which features, in what order, which of them shown by default, and what
/// the rows are sorted by.
pub(super) const LAYOUT: &str = "query($project: String!, $specimens: [Filter]) {
  kinds: getSpecimenTypeDisplayPropertiesByProject(projectReferenceId: $project) {
    priorityOrder
    referenceId
    title
    defaultSort
    displayFeatures { ...Shown }
  }
  whole: getDisplayProperty(displayPropertyFilter: { type: PROJECT, typeReferenceId: $project }) {
    ... on ProjectDisplayProperty {
      defaultSort
      displayFeatures { ...Shown }
    }
  }
  counts: aio_specimenCounts(filter: $specimens, groupBy: [\"specimenType.referenceId\"]) {
    count
    properties { value }
  }
}
fragment Shown on FeatureDisplayProperty {
  type
  priorityOrder
  isDefault
  filterOperator
  featureType { referenceId title }
  ... on MeasurementDisplayProperty { unit measurementStats { min max } }
}";

#[derive(Deserialize)]
pub(super) struct LayoutData {
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) kinds: Vec<KindLayout>,
    #[serde(default)]
    pub(super) whole: Option<WholeLayout>,
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
    /// A JSON list of fields and orders, written as a string.
    #[serde(default)]
    pub(super) default_sort: Option<String>,
    #[serde(default, deserialize_with = "maybe_list")]
    pub(super) display_features: Vec<DisplayFeature>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WholeLayout {
    #[serde(default)]
    pub(super) default_sort: Option<String>,
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
    /// Shown when the table opens; the rest are offered but left out.
    #[serde(default)]
    pub(super) is_default: Option<bool>,
    /// How the portal narrows by it: `BETWEEN` for a span of numbers, `EQ`
    /// or `CONTAINS` for values.
    #[serde(default)]
    pub(super) filter_operator: Option<String>,
    /// A measurement's smallest and largest value across the project.
    #[serde(default)]
    pub(super) measurement_stats: Option<Stats>,
    pub(super) feature_type: Titled,
    #[serde(default)]
    pub(super) unit: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Stats {
    pub(super) min: Option<f64>,
    pub(super) max: Option<f64>,
}

/// Ask how the platform lays a project's specimens out.
pub(super) async fn ask_layout(endpoint: &str, project: &str) -> Result<LayoutData, String> {
    let variables = json!({
        "project": project,
        "specimens": specimen_filters(&Scope::project(project), &[]),
    });
    graphql::ask(endpoint, LAYOUT, variables).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_left_at_the_datas_end_is_written_as_no_end() {
        let term = |low: f32, high: f32| TableFilterTerm::Between {
            field: "age".into(),
            low,
            high,
        };
        let written = |term: TableFilterTerm| {
            specimen_filters(&Scope::project("P"), &[term])[1]["value"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(written(term(70.0, 90.5)), "[70,90.5]");
        // Never an f32 standing in for the data's own end, which could fall
        // just short of it and lose the rows there.
        assert_eq!(
            written(term(70.0, f32::INFINITY)),
            format!("[70,{CEILING}]")
        );
        assert_eq!(
            written(term(f32::NEG_INFINITY, 90.5)),
            format!("[{FLOOR},90.5]")
        );
    }

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

    #[test]
    #[ignore = "reads the live SEA-AD donors"]
    fn a_live_search_matches_words_and_is_counted() {
        const PROJECT: &str = "JGN327NUXRZSHEV88TN";
        let endpoint = "https://idf-api-prod.aibs-idk-prod.net/";
        let scope = Scope::project(PROJECT);
        crate::app::net::block_on(async {
            let search = |text: &str, count: bool| Searching {
                text: text.into(),
                count,
            };
            let both = ask(
                endpoint,
                &scope,
                0,
                &[],
                Value::Null,
                Some(search("Female AND Dementia", true)),
            )
            .await
            .unwrap();
            let counted = both.counted().unwrap();
            println!("{counted} female donors with dementia");
            assert!(counted > 0 && counted < 84);
            assert_eq!(both.aio_specimen.len(), counted.min(PAGE));
            // Turning the page does not count again.
            let paged = ask(
                endpoint,
                &scope,
                0,
                &[],
                Value::Null,
                Some(search("Female", false)),
            )
            .await
            .unwrap();
            assert_eq!(paged.counted(), None);
            assert!(!paged.aio_specimen.is_empty());
        });
    }
}
