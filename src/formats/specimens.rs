//! Specimen records from the Brain Knowledge Platform, as a table.
//!
//! Not a file format: a question asked of the platform's GraphQL API. It is
//! here rather than in `catalog` because a catalog only says what there is to
//! open, and this *is* a dataset — one a frame shows exactly the way it shows
//! a CSV, since both end at rows and columns.
//!
//! The address is the API endpoint itself with the project named on it:
//!
//! ```text
//! https://idf-api-prod.aibs-idk-prod.net/?specimens=JGN327NUXRZSHEV88TN
//! ```
//!
//! So the thing being talked to is the thing being named, and a different
//! deployment of the API is reached by writing its host instead.
//!
//! A specimen is not a row. It carries a list of annotations and a list of
//! measurements, each tagged with the feature it belongs to, and which
//! features a specimen has varies within one project. So the columns are the
//! union of the features seen across the page, and a specimen with nothing
//! under a feature leaves that cell empty — which is the honest answer, and
//! what a spreadsheet of the same records would show.

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::net::{Fetching, fetching};
use crate::app::schedule::Stage;
use crate::source::SourceBusy;
use crate::source::properties::NumericRange;
use crate::source::table::{
    SourceTable, TableColumn, TableFilter, TableFilterTerm, TableFilterValue, TableFilters,
    TablePaging,
};

use super::table::{PAGE_ROWS, Table};

/// Specimens asked for in one request: the page the frame is showing and no
/// more.
///
/// Measured against the SEA-AD donor project, where 84 specimens of 30
/// features came to 199KB in 0.7s, and the Genetic Tools Atlas, where 500 came
/// to 1.5MB in 1.1s. Reading all 10,901 of the latter took 34MB and some
/// twenty seconds before a frame appeared, which is the whole reason a page is
/// what gets fetched.
const PAGE: usize = PAGE_ROWS;

const QUERY: &str = "query($project: [Filter], $specimens: [Filter],
  $groupBy: [groupBy_List_String_pattern_id], $limit: Int, $offset: Int) {
  project: dataCollectionProjectInventory(filter: $project, limit: 1) {
    referenceId
    title
  }
  total: aio_specimenCounts(filter: $specimens, groupBy: $groupBy) {
    count
  }
  aio_specimen(filter: $specimens, limit: $limit, offset: $offset) {
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

/// A floor below anything the platform holds, written out in full.
///
/// Its range parser takes plain decimals only: `-1e12` comes back "Invalid
/// range format", and so does an edge that Rust would have printed in
/// scientific notation, which is why the edges below are written to a fixed
/// number of places.
const FLOOR: &str = "-1000000000000";

/// Buckets a column's distribution is drawn in.
///
/// Each one is a separate index query on the platform — about 0.14s apiece —
/// so this is the width of a histogram traded against how long opening a
/// column takes.
const BUCKETS: usize = 20;

/// How far past the values already in hand a distribution is asked for, as a
/// fraction of their span.
///
/// The page on screen is the only sample of a column there is without asking,
/// and a hundred rows of ten thousand will not hold either end. Asking wide
/// and keeping the buckets that answered is cheaper than finding the ends
/// first: the platform has no query that gives a column's extent —
/// `measurementStats` sits on `aio_specimenFacetedSearchProperties`, which
/// answers with an empty list for every project.
const WIDEN: f64 = 0.5;

/// What the API says went wrong, which arrives beside the data rather than as
/// a status.
#[derive(Deserialize)]
struct GraphQlError {
    message: String,
}

/// The query parameter that names a project's specimens.
const PARAMETER: &str = "specimens";

/// The column holding a specimen's own accession.
const SPECIMEN: &str = "Specimen";

/// The column holding what kind of specimen it is.
const KIND: &str = "Type";

/// Whether `url` asks for a project's specimens, and if so the endpoint to ask
/// and the project to ask about.
///
/// The endpoint is the address with the query dropped: what names the dataset
/// is also what answers for it.
pub fn query_of(url: &str) -> Option<(String, String)> {
    let (endpoint, query) = url.split_once('?')?;
    let project = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == PARAMETER)
        .map(|(_, value)| value)?;
    if project.is_empty() {
        return None;
    }
    Some((endpoint.to_string(), project.to_string()))
}

/// Name a table after the project, falling back to what it was asked for.
fn label_for(project: &str, title: Option<&str>) -> String {
    match title {
        Some(title) if !title.trim().is_empty() => title.trim().to_string(),
        _ => format!("Specimens {project}"),
    }
}

/// Read a list that may arrive as `null` rather than as an empty one.
///
/// The platform writes null where a record has nothing, and serde's `default`
/// covers a field that is *missing* rather than one that is present and null.
/// Found on the last page of the Genetic Tools Atlas, where a specimen has no
/// measurements at all.
fn maybe_list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Deserialize)]
struct Data {
    #[serde(default, deserialize_with = "maybe_list")]
    project: Vec<Project>,
    #[serde(default, deserialize_with = "maybe_list")]
    total: Vec<Aggregate>,
    #[serde(default, deserialize_with = "maybe_list")]
    aio_specimen: Vec<Specimen>,
}

#[derive(Deserialize)]
struct Aggregate {
    count: Option<f64>,
}

impl Data {
    /// How many specimens the project holds, as the platform counted them.
    ///
    /// Grouped by the project itself, so there is one figure to take; summed
    /// rather than indexed in case the grouping ever splits.
    fn counted(&self) -> Option<usize> {
        let total: f64 = self.total.iter().filter_map(|it| it.count).sum();
        (total > 0.0).then_some(total as usize)
    }
}

#[derive(Deserialize)]
struct Project {
    title: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Specimen {
    #[serde(rename = "cRID")]
    crid: Option<Crid>,
    specimen_type: Option<Named>,
    #[serde(default, deserialize_with = "maybe_list")]
    annotations: Vec<Annotation>,
    #[serde(default, deserialize_with = "maybe_list")]
    measurements: Vec<Measurement>,
}

#[derive(Deserialize)]
struct Crid {
    symbol: Option<String>,
}

#[derive(Deserialize)]
struct Named {
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Annotation {
    feature_type: Titled,
    #[serde(default, deserialize_with = "maybe_list")]
    taxons: Vec<Taxon>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Measurement {
    feature_type: Titled,
    value: Option<String>,
    unit: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Titled {
    /// Only annotations carry one through: it is what a filter narrows by.
    #[serde(default)]
    reference_id: Option<String>,
    title: Option<String>,
}

#[derive(Deserialize)]
struct Taxon {
    symbol: Option<String>,
}

/// Read the first page of a project's specimens, and find out how many there
/// are altogether.
///
/// One request: the project's title, the count, and the page. A frame opens on
/// what comes back, and [`SpecimenSystems`] fetches any other page the frame
/// is turned to.
pub async fn read(endpoint: &str, project: &str) -> Result<Specimens, String> {
    let answer = ask(endpoint, project, 0, &[]).await?;
    if answer.aio_specimen.is_empty() {
        return Err(format!(
            "{endpoint} knows no specimens in project {project}"
        ));
    }

    let title = answer.project.first().and_then(|it| it.title.clone());
    let total = answer.counted().unwrap_or(answer.aio_specimen.len());
    let plan = Plan::of(&answer.aio_specimen);
    let table = Table::paged(
        label_for(project, title.as_deref()),
        format!(
            "Brain Knowledge Platform specimens, {} columns",
            plan.headers().len()
        ),
        plan.headers(),
        plan.rows(&answer.aio_specimen),
        None,
        Some(total),
    );
    Ok(Specimens {
        endpoint: endpoint.to_string(),
        project: project.to_string(),
        table,
        plan,
    })
}

/// A project's specimens: the page that was read, and what it takes to read
/// another.
pub struct Specimens {
    endpoint: String,
    project: String,
    pub table: Table,
    plan: Plan,
}

/// One request: the project's title, how many specimens match, and a page of
/// them.
///
/// `terms` are what has been asked of the table. Two terms on one column
/// widen — the platform treats a field named twice as either — and terms on
/// different columns narrow each other, which is what a row of checkboxes and
/// a span beside it are read to mean.
async fn ask(
    endpoint: &str,
    project: &str,
    offset: usize,
    terms: &[TableFilterTerm],
) -> Result<Data, String> {
    #[derive(Deserialize)]
    struct Response {
        data: Option<Data>,
        #[serde(default)]
        errors: Vec<GraphQlError>,
    }
    let body = json!({
        "query": QUERY,
        "variables": {
            "project": [{ "field": "referenceId", "operator": "EQ", "value": project }],
            "specimens": specimen_filters(project, terms),
            "groupBy": ["projectReferenceIds"],
            "limit": PAGE,
            "offset": offset,
        }
    });
    let text = crate::app::net::post_json(endpoint, body.to_string()).await?;
    let response: Response =
        serde_json::from_str(&text).map_err(|e| format!("parsing {endpoint}: {e}"))?;
    if let Some(error) = response.errors.first() {
        return Err(error.message.clone());
    }
    response
        .data
        .ok_or_else(|| format!("{endpoint} answered with no data"))
}

/// Which columns a specimen table has, and in what order.
///
/// Settled from the first page and kept, so turning the page does not relay
/// out the table under the pointer. A later page carrying a feature no earlier
/// one had grows the plan rather than losing the value — columns only ever
/// gain, which the frame notices and rebuilds for.
///
/// Ordered so a table reads the same way twice: what identifies a specimen
/// first, then annotations by name, then measurements by name. The platform's
/// own order is not stable between requests.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Each annotation's feature — what the platform calls it, which is what
    /// a filter narrows by, and what the column is headed.
    annotations: Vec<(String, String)>,
    /// Each measurement's feature, its title, and the unit it is recorded in
    /// — which names the column rather than every cell in it.
    measurements: Vec<(String, String, Option<String>)>,
}

impl Plan {
    fn of(specimens: &[Specimen]) -> Plan {
        let mut plan = Plan::default();
        plan.extend(specimens);
        plan
    }

    /// Add any feature these specimens carry that the plan has not got.
    ///
    /// Returns whether anything was added, since that is what makes the
    /// columns on screen wrong until they are rebuilt.
    fn extend(&mut self, specimens: &[Specimen]) -> bool {
        let mut annotations: BTreeMap<&str, &str> = BTreeMap::new();
        let mut measurements: BTreeMap<&str, (&str, Option<&str>)> = BTreeMap::new();
        for specimen in specimens {
            for annotation in &specimen.annotations {
                if let Some(title) = annotation.feature_type.title.as_deref() {
                    annotations.insert(
                        title,
                        annotation
                            .feature_type
                            .reference_id
                            .as_deref()
                            .unwrap_or(""),
                    );
                }
            }
            for measurement in &specimen.measurements {
                if let Some(title) = measurement.feature_type.title.as_deref() {
                    // The unit belongs to the feature rather than to the
                    // reading, so the first one seen names the column.
                    measurements.entry(title).or_insert_with(|| {
                        (
                            measurement
                                .feature_type
                                .reference_id
                                .as_deref()
                                .unwrap_or(""),
                            measurement.unit.as_deref().filter(|it| !it.is_empty()),
                        )
                    });
                }
            }
        }

        let before = (self.annotations.len(), self.measurements.len());
        for (title, id) in &annotations {
            if !self.annotations.iter().any(|(_, known)| known == title) {
                self.annotations
                    .push(((*id).to_string(), (*title).to_string()));
            }
        }
        for (title, (id, unit)) in &measurements {
            if !self.measurements.iter().any(|(_, known, _)| known == title) {
                self.measurements.push((
                    (*id).to_string(),
                    (*title).to_string(),
                    unit.map(str::to_string),
                ));
            }
        }
        self.annotations.sort_by(|a, b| a.1.cmp(&b.1));
        self.measurements.sort_by(|a, b| a.1.cmp(&b.1));
        before != (self.annotations.len(), self.measurements.len())
    }

    /// Every column that could be narrowed by, as the platform names it, as
    /// the table heads it, and whether it is a measurement — which is to say
    /// whether a span is worth offering when the platform cannot list it.
    fn columns(&self) -> Vec<(String, String, bool)> {
        self.annotations
            .iter()
            .map(|(id, title)| (id.clone(), title.clone(), false))
            .chain(
                self.measurements
                    .iter()
                    .map(|(id, title, _)| (id.clone(), title.clone(), true)),
            )
            .filter(|(id, ..)| !id.is_empty())
            .collect()
    }

    fn headers(&self) -> Vec<String> {
        let mut headers = vec![SPECIMEN.to_string(), KIND.to_string()];
        headers.extend(self.annotations.iter().map(|(_, title)| title.clone()));
        headers.extend(self.measurements.iter().map(|(_, title, unit)| match unit {
            Some(unit) => format!("{title} ({unit})"),
            None => title.clone(),
        }));
        headers
    }

    fn rows(&self, specimens: &[Specimen]) -> Vec<Vec<String>> {
        specimens
            .iter()
            .map(|specimen| {
                let mut row = vec![
                    specimen
                        .crid
                        .as_ref()
                        .and_then(|crid| crid.symbol.clone())
                        .unwrap_or_default(),
                    specimen
                        .specimen_type
                        .as_ref()
                        .and_then(|kind| kind.name.clone())
                        .unwrap_or_default(),
                ];
                row.extend(
                    self.annotations
                        .iter()
                        .map(|(_, title)| annotated(specimen, title)),
                );
                row.extend(
                    self.measurements
                        .iter()
                        .map(|(_, title, _)| measured(specimen, title)),
                );
                row
            })
            .collect()
    }
}

/// What a specimen is annotated with under `title`.
///
/// An annotation can name more than one taxon — a donor with two clinical
/// diagnoses has both — so they are listed rather than one being picked.
fn annotated(specimen: &Specimen, title: &str) -> String {
    let mut values: Vec<&str> = specimen
        .annotations
        .iter()
        .filter(|annotation| annotation.feature_type.title.as_deref() == Some(title))
        .flat_map(|annotation| annotation.taxons.iter())
        .filter_map(|taxon| taxon.symbol.as_deref())
        .filter(|symbol| !symbol.is_empty())
        .collect();
    values.dedup();
    values.join(", ")
}

/// What a specimen measured for `title`.
///
/// The first reading, because the platform repeats some of them: the SEA-AD
/// donors each carry sex and age at death twice, with the same value both
/// times. Listing a value beside itself would say something the data does not.
fn measured(specimen: &Specimen, title: &str) -> String {
    specimen
        .measurements
        .iter()
        .find(|measurement| measurement.feature_type.title.as_deref() == Some(title))
        .and_then(|measurement| measurement.value.clone())
        .unwrap_or_default()
}

/// Ask what every column holds, in one request.
///
/// A root field per column, aliased, since the counts come back grouped by
/// whichever column was asked about and there is no way to ask for several
/// groupings at once.
///
/// Every column is asked about and the platform decides which it can answer.
/// A column it cannot comes back null beside the ones it could, so the answer
/// is read for what is in it rather than refused whole — which is what keeps
/// one column's failure from costing the rest. Numeric measurements are the
/// ones that fail today: grouping by one answers "Unable to cast object of
/// type 'System.Double' to type 'System.String'", so the platform's own
/// filters offer Age where these cannot.
///
/// The counts are of the whole project rather than of what is on screen. A
/// count then says what ticking a value would bring back, which is the
/// question a reader is asking of it, and it does not shift under them as
/// they tick.
async fn ask_values(
    endpoint: &str,
    project: &str,
    columns: &[(String, String, bool)],
) -> Result<Vec<TableFilter>, String> {
    let fields: String = columns
        .iter()
        .enumerate()
        .map(|(index, (id, ..))| {
            format!(
                "c{index}: aio_specimenCounts(filter: $specimens, groupBy: [\"{id}\"]) \
                 {{ count properties {{ property value }} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let query = format!("query($specimens: [Filter]) {{ {fields} }}");

    #[derive(Deserialize)]
    struct Response {
        data: Option<BTreeMap<String, Option<Vec<Grouped>>>>,
        #[serde(default)]
        errors: Vec<GraphQlError>,
    }
    #[derive(Deserialize)]
    struct Grouped {
        count: Option<f64>,
        #[serde(default, deserialize_with = "maybe_list")]
        properties: Vec<Property>,
    }
    #[derive(Deserialize)]
    struct Property {
        value: Option<String>,
    }

    let body = json!({
        "query": query,
        "variables": {
            "specimens": [{
                "field": "projectReferenceIds", "operator": "EQ", "value": project
            }],
        }
    });
    let text = crate::app::net::post_json(endpoint, body.to_string()).await?;
    let response: Response =
        serde_json::from_str(&text).map_err(|e| format!("parsing {endpoint}: {e}"))?;
    // Only when nothing at all came back: a column the platform could not
    // group by is reported beside the ones it could, and costs only itself.
    let Some(answers) = response.data else {
        return Err(response.errors.first().map_or_else(
            || format!("{endpoint} answered with no data"),
            |error| error.message.clone(),
        ));
    };
    for error in &response.errors {
        debug!("a column cannot be grouped by: {}", error.message);
    }

    let mut filters = Vec::new();
    for (index, (id, name, numeric)) in columns.iter().enumerate() {
        let Some(Some(groups)) = answers.get(&format!("c{index}")) else {
            // The platform cannot group a column of numbers — it answers
            // "Unable to cast object of type 'System.Double' to type
            // 'System.String'" — so that is what marks one out, and a number
            // is narrowed by taking a span of it rather than by ticking every
            // reading anyone took.
            if *numeric {
                filters.push(TableFilter::range(id, name));
            }
            continue;
        };
        let values: Vec<TableFilterValue> = groups
            .iter()
            .filter_map(|group| {
                let label = group.properties.first()?.value.clone()?;
                (!label.trim().is_empty()).then(|| TableFilterValue {
                    label,
                    count: group.count.unwrap_or_default() as u64,
                    chosen: false,
                })
            })
            .collect();
        if values.is_empty() {
            continue;
        }
        filters.push(TableFilter::values(id, name, values));
    }
    Ok(filters)
}

/// Ask how a column's numbers are distributed, in one request.
///
/// A cumulative count at each bucket edge — how many rows hold less than this
/// — since that is the one shape `aio_specimenRangeCounts` answers. Taking
/// the differences gives the histogram, and the outermost buckets that hold
/// anything give the extent.
///
/// `seen` is whatever values are already on screen, which is what the span
/// asked about is built from.
async fn ask_span(
    endpoint: &str,
    project: &str,
    field: &str,
    seen: &[f64],
    terms: &[TableFilterTerm],
) -> Result<NumericRange, String> {
    let edges = bucket_edges(seen);
    let fields: String = edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            format!(
                "e{index}: aio_specimenRangeCounts(filter: $specimens, \
                 groupBy: {{ field: \"{field}\", range: \"[{FLOOR},{edge:.6}]\" }}) {{ count }}"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let query = format!("query($specimens: [Filter]) {{ {fields} }}");

    #[derive(Deserialize)]
    struct Response {
        data: Option<BTreeMap<String, Option<Vec<Counted>>>>,
        #[serde(default)]
        errors: Vec<GraphQlError>,
    }
    #[derive(Deserialize)]
    struct Counted {
        count: Option<f64>,
    }

    let body = json!({
        "query": query,
        "variables": { "specimens": specimen_filters(project, terms) },
    });
    let text = crate::app::net::post_json(endpoint, body.to_string()).await?;
    let response: Response =
        serde_json::from_str(&text).map_err(|e| format!("parsing {endpoint}: {e}"))?;
    let Some(answers) = response.data else {
        return Err(response.errors.first().map_or_else(
            || format!("{endpoint} answered with no distribution"),
            |error| error.message.clone(),
        ));
    };

    let cumulative: Vec<u32> = (0..edges.len())
        .map(|index| {
            answers
                .get(&format!("e{index}"))
                .and_then(|counted| counted.as_ref()?.first()?.count)
                .unwrap_or_default() as u32
        })
        .collect();
    histogram_of(&edges, &cumulative).ok_or_else(|| format!("{field} holds no numbers"))
}

/// The project, and whatever has been asked of it.
fn specimen_filters(project: &str, terms: &[TableFilterTerm]) -> Value {
    let mut filters = vec![json!({
        "field": "projectReferenceIds", "operator": "EQ", "value": project
    })];
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

/// The edges a distribution is asked about: [`BUCKETS`] buckets across what is
/// on screen, widened at both ends for what is not.
fn bucket_edges(seen: &[f64]) -> Vec<f64> {
    let low = seen.iter().copied().fold(f64::INFINITY, f64::min);
    let high = seen.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (low, high) = if low.is_finite() && high.is_finite() {
        (low, high)
    } else {
        (0.0, 1.0)
    };
    // A column of one value throughout still needs a span to draw across.
    let span = (high - low)
        .max(f64::EPSILON)
        .max(high.abs().max(1.0) * 1e-6);
    let (low, high) = (low - span * WIDEN, high + span * WIDEN);
    let step = (high - low) / BUCKETS as f64;
    (0..=BUCKETS).map(|i| low + step * i as f64).collect()
}

/// Turn cumulative counts at each edge into a histogram and the extent that
/// holds it.
///
/// The outermost buckets holding anything are the extent, so a column asked
/// about far wider than it runs is still drawn across what it has.
fn histogram_of(edges: &[f64], cumulative: &[u32]) -> Option<NumericRange> {
    let buckets: Vec<u32> = cumulative
        .windows(2)
        .map(|pair| pair[1].saturating_sub(pair[0]))
        .collect();
    let first = buckets.iter().position(|count| *count > 0)?;
    let last = buckets.iter().rposition(|count| *count > 0)?;
    Some(NumericRange::full(
        edges[first] as f32,
        edges[last + 1] as f32,
        buckets[first..=last].to_vec(),
    ))
}

/// The project a frame's specimen table came from, so a page it has not got
/// can be asked for.
///
/// A component of the source entity rather than a resource, so two frames on
/// two projects each turn their own pages.
#[derive(Component)]
pub struct SpecimenPages {
    endpoint: String,
    project: String,
    /// The columns the first page settled. Every page after fills these, so
    /// the table does not relay itself out under the pointer.
    plan: Plan,
    /// The page being fetched, and the fetch, while one is in flight.
    fetching: Option<(usize, Fetching<Result<Data, String>>)>,
    /// What the columns hold, asked for once.
    offering: Option<Fetching<Result<Vec<TableFilter>, String>>>,
    /// Whether the columns have been asked about, so they are asked once
    /// whether or not anything came back.
    offered: bool,
    /// The values in force on the rows now on screen, so a tick that changes
    /// nothing does not refetch and one that does is noticed.
    applied: Vec<TableFilterTerm>,
    /// The column whose distribution is being asked about, and the ask.
    spanning: Option<(usize, Fetching<Result<NumericRange, String>>)>,
}

impl SpecimenPages {
    pub fn new(endpoint: String, project: String, plan: Plan) -> Self {
        SpecimenPages {
            endpoint,
            project,
            plan,
            fetching: None,
            offering: None,
            offered: false,
            applied: Vec::new(),
            spanning: None,
        }
    }
}

/// Ask the platform what each column holds, once per source, and offer the
/// answer as filters.
///
/// Only the annotations: a measurement is a number, and a number wants a range
/// rather than a list of every reading anyone took.
fn offer_filters(mut commands: Commands, mut sources: Query<(Entity, &mut SpecimenPages)>) {
    for (source, mut pages) in &mut sources {
        if let Some(fetch) = pages.offering.as_mut()
            && let Some(answer) = fetch.take()
        {
            pages.offering = None;
            match answer {
                Ok(columns) if !columns.is_empty() => {
                    info!("{} columns to narrow specimens by", columns.len());
                    commands.entity(source).insert(TableFilters::ready(columns));
                }
                // Nothing to narrow by is not a failure: the section simply
                // does not appear.
                Ok(_) => {
                    commands.entity(source).remove::<TableFilters>();
                }
                Err(e) => {
                    warn!("asking what specimens can be narrowed by: {e}");
                    commands.entity(source).remove::<TableFilters>();
                }
            }
        }
        if pages.offered {
            continue;
        }
        pages.offered = true;
        let (endpoint, project) = (pages.endpoint.clone(), pages.project.clone());
        // Every column, not only the annotations: the platform answers for
        // whichever of them it can group by.
        let columns = pages.plan.columns();
        commands.entity(source).insert(TableFilters::pending());
        pages.offering = Some(fetching(async move {
            ask_values(&endpoint, &project, &columns).await
        }));
    }
}

/// Ask how a column's numbers are spread, once someone opens it.
///
/// Opening is the signal because a distribution is twenty-odd index queries:
/// asking for every column of a table the moment it opened would be a minute
/// of waiting for histograms nobody looked at.
fn serve_spans(
    mut sources: Query<(
        &mut SpecimenPages,
        &mut TableFilters,
        &SourceTable,
        &TablePaging,
    )>,
) {
    for (mut pages, mut filters, rows, paging) in &mut sources {
        if let Some((column, fetch)) = pages.spanning.as_mut()
            && let Some(answer) = fetch.take()
        {
            let column = *column;
            pages.spanning = None;
            match answer {
                Ok(span) => {
                    if let Some(filter) = filters.columns.get_mut(column) {
                        filter.kind = crate::source::table::TableFilterKind::Range {
                            span: Some(span),
                            wanted: true,
                        };
                    }
                }
                Err(e) => {
                    // Asked once and left alone. Without this the column is
                    // still open, still has no span, and is asked about again
                    // every frame for as long as it is looked at.
                    warn!("reading how a column is spread: {e}");
                    if let Some(filter) = filters.columns.get_mut(column) {
                        filter.want(false);
                    }
                }
            }
        }
        if pages.spanning.is_some() {
            continue;
        }

        let Some(column) = filters.columns.iter().position(TableFilter::awaiting_span) else {
            continue;
        };
        let field = filters.columns[column].id.clone();
        // What is on screen is the only sample of the column there is without
        // asking, and it is what the span asked about is built around.
        let seen = column_values(rows, &filters.columns[column].name);
        let (endpoint, project) = (pages.endpoint.clone(), pages.project.clone());
        let terms = filters.chosen();
        let _ = paging;
        pages.spanning = Some((
            column,
            fetching(async move { ask_span(&endpoint, &project, &field, &seen, &terms).await }),
        ));
    }
}

/// The numbers a column holds on the page in hand.
fn column_values(rows: &SourceTable, name: &str) -> Vec<f64> {
    let Some(index) = rows
        .columns
        .iter()
        .position(|column| column.name == name || column.name.starts_with(&format!("{name} (")))
    else {
        return Vec::new();
    };
    rows.rows
        .iter()
        .filter_map(|row| row.get(index)?.trim().parse::<f64>().ok())
        .collect()
}

/// Ask for the page a frame has turned to, and take it when it lands.
///
/// One system rather than two because the two halves share the plan: what
/// comes back is only rows once it is read against the columns already on
/// screen.
fn serve_pages(
    mut sources: Query<(
        &mut SpecimenPages,
        &mut TablePaging,
        &mut SourceTable,
        &mut SourceBusy,
        Option<&TableFilters>,
    )>,
) {
    for (mut pages, mut paging, mut rows, mut busy, filters) in &mut sources {
        let wanted_values = filters.map(TableFilters::chosen).unwrap_or_default();
        if let Some((page, fetch)) = pages.fetching.as_mut()
            && let Some(answer) = fetch.take()
        {
            let page = *page;
            pages.fetching = None;
            match answer {
                Ok(data) => {
                    // A narrowed table is a different table: however many rows
                    // it now has is what the paging counts to.
                    if let Some(total) = data.counted() {
                        paging.total = Some(total);
                    } else if data.aio_specimen.is_empty() {
                        paging.total = Some(0);
                    }
                    take_page(&mut pages, &mut rows, page * paging.size, &data);
                }
                Err(e) => {
                    // The rows on screen are left alone — a page that could
                    // not be read is better than an empty table — and the
                    // frame is put back on the page it is actually showing.
                    // Without that it would ask for the missing one again
                    // every frame, forever.
                    warn!("reading specimens: {e}");
                    let showing = rows.first / paging.size.max(1);
                    if paging.page != showing {
                        paging.page = showing;
                    }
                }
            }
        }

        // A tick is a new table, so it starts at the top rather than on
        // whichever page the old one happened to be showing.
        let narrowed = pages.applied != wanted_values;
        if narrowed && paging.page != 0 {
            paging.page = 0;
        }

        let wanted = paging.first();
        let asking = pages.fetching.as_ref().map(|(page, _)| page * paging.size);
        if (narrowed || rows.first != wanted) && asking != Some(wanted) {
            let (endpoint, project) = (pages.endpoint.clone(), pages.project.clone());
            let values = wanted_values.clone();
            pages.applied = wanted_values;
            pages.fetching = Some((
                paging.page,
                fetching(async move { ask(&endpoint, &project, wanted, &values).await }),
            ));
        }
        busy.set_if_neq(SourceBusy(pages.fetching.is_some()));
    }
}

/// Put a landed page into the rows a frame draws.
fn take_page(pages: &mut SpecimenPages, rows: &mut SourceTable, first: usize, data: &Data) {
    // A page carrying a feature no earlier page had grows the columns rather
    // than dropping the value. They only ever gain, so the frame notices and
    // lays itself out again.
    if pages.plan.extend(&data.aio_specimen) {
        let headers = pages.plan.headers();
        rows.columns = headers
            .iter()
            .map(|name| TableColumn {
                name: name.clone(),
                chars: name.chars().count(),
                numeric: false,
            })
            .collect();
    }

    rows.rows = pages.plan.rows(&data.aio_specimen);
    rows.first = first;
    // Only the page in hand can be measured, so a column is as wide as the
    // widest value seen on any page so far. It grows and never shrinks: a
    // column that narrowed on every page would shuffle the table sideways
    // each time one was turned.
    for (index, column) in rows.columns.iter_mut().enumerate() {
        let widest = rows
            .rows
            .iter()
            .filter_map(|row| row.get(index))
            .map(|value| value.trim().chars().count())
            .max()
            .unwrap_or_default();
        column.chars = column.chars.max(widest).min(super::table::MAX_CHARS);
    }
}

/// The systems every specimen table shares, registered once however many are
/// open.
pub struct SpecimenSystems;

impl Plugin for SpecimenSystems {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (offer_filters, serve_spans, serve_pages)
                .chain()
                .in_set(Stage::Sources),
        );
    }
}

/// Register a project's specimens as a source that can be paged.
pub fn spawn_source(world: &mut World, specimens: Specimens) -> Entity {
    let Specimens {
        endpoint,
        project,
        table,
        plan,
    } = specimens;
    let source = super::table::spawn_source(world, table);
    world
        .entity_mut(source)
        .insert(SpecimenPages::new(endpoint, project, plan));
    source
}

/// Read a saved answer, for tests and for anything that has the JSON already.
#[cfg(test)]
fn from_answer(name: &str, text: &str) -> Result<Table, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let data: Data = serde_json::from_value(value["data"].clone()).map_err(|e| e.to_string())?;
    let plan = Plan::of(&data.aio_specimen);
    Ok(Table::paged(
        name,
        "test",
        plan.headers(),
        plan.rows(&data.aio_specimen),
        None,
        data.counted(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Table {
        from_answer("SEA-AD", include_str!("../../testdata/specimens.json")).unwrap()
    }

    fn names(table: &Table) -> Vec<&str> {
        table
            .rows
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect()
    }

    fn column(table: &Table, name: &str) -> usize {
        names(table).iter().position(|it| *it == name).unwrap()
    }

    #[test]
    fn an_address_names_the_endpoint_that_answers_for_it() {
        let (endpoint, project) =
            query_of("https://idf-api-prod.aibs-idk-prod.net/?specimens=JGN327NUXRZSHEV88TN")
                .unwrap();
        assert_eq!(endpoint, "https://idf-api-prod.aibs-idk-prod.net/");
        assert_eq!(project, "JGN327NUXRZSHEV88TN");
        // Whatever else is on the query string, and whatever the host.
        assert_eq!(
            query_of("https://dev.example.net/?v=2&specimens=ABC").unwrap(),
            ("https://dev.example.net/".into(), "ABC".into())
        );
    }

    #[test]
    fn anything_else_is_not_a_specimen_query() {
        assert!(query_of("https://example.com/image.zarr/").is_none());
        assert!(query_of("https://example.com/a.csv?x=1").is_none());
        // Named with nothing to name: not a project.
        assert!(query_of("https://example.com/?specimens=").is_none());
        // A parameter that merely ends the same way.
        assert!(query_of("https://example.com/?nospecimens=ABC").is_none());
    }

    #[test]
    fn a_specimen_becomes_a_row_led_by_its_accession() {
        let table = fixture();
        assert_eq!(names(&table)[0], SPECIMEN);
        assert_eq!(table.rows.rows.len(), 3);
        assert_eq!(table.rows.cell(0, 0), "H21.33.012");
        assert_eq!(table.rows.cell(0, column(&table, KIND)), "sample");
    }

    #[test]
    fn every_feature_any_specimen_carries_gets_a_column() {
        // MoCA is recorded for one of the three, and is a column all the same;
        // the two without it are empty there rather than absent.
        let table = fixture();
        let moca = column(&table, "MoCA score");
        let recorded: Vec<&str> = (0..3).map(|row| table.rows.cell(row, moca)).collect();
        assert_eq!(recorded.iter().filter(|it| !it.is_empty()).count(), 1);
    }

    #[test]
    fn a_measurement_names_its_unit_in_the_column_rather_than_every_cell() {
        let table = fixture();
        let age = column(&table, "Age at death (years)");
        assert_eq!(table.rows.cell(0, age), "93");
        // And one with no unit is named plainly.
        assert_eq!(table.rows.cell(0, column(&table, "Sex")), "Female");
    }

    #[test]
    fn a_measurement_the_platform_repeats_is_reported_once() {
        // The SEA-AD donors carry sex and age at death twice over, with the
        // same value both times.
        let table = fixture();
        assert_eq!(table.rows.cell(0, column(&table, "Sex")), "Female");
    }

    #[test]
    fn an_annotation_with_more_than_one_taxon_lists_them() {
        let table = fixture();
        let diagnosis = column(&table, "Consensus clinical diagnosis");
        assert_eq!(
            table.rows.cell(0, diagnosis),
            "Multiple System Atrophy, Other"
        );
    }

    #[test]
    fn the_columns_come_out_in_the_same_order_twice() {
        // The platform's own order changes between requests, so the table
        // fixes one: what identifies a specimen, then its annotations by
        // name, then its measurements by name.
        assert_eq!(
            names(&fixture()),
            [
                SPECIMEN,
                KIND,
                "Cognitive status",
                "Consensus clinical diagnosis",
                "Donor ID",
                "Race/ ethnicity",
                "Age at death (years)",
                "Brain pH",
                "MoCA score",
                "Sex",
            ]
        );
    }

    #[test]
    fn a_numeric_column_is_told_from_a_worded_one() {
        let table = fixture();
        assert!(table.rows.columns[column(&table, "Age at death (years)")].numeric);
        assert!(!table.rows.columns[column(&table, "Sex")].numeric);
    }

    fn specimens(text: &str) -> Vec<Specimen> {
        let value: Value = serde_json::from_str(text).unwrap();
        let data: Data = serde_json::from_value(value["data"].clone()).unwrap();
        data.aio_specimen
    }

    #[test]
    fn a_page_read_later_fills_the_columns_the_first_one_settled() {
        // What stops the table relaying itself out every time a page turns.
        let all = specimens(include_str!("../../testdata/specimens.json"));
        let mut plan = Plan::of(&all[..1]);
        let settled = plan.headers();
        assert!(!plan.extend(&all[..1]), "the same page adds nothing");
        assert_eq!(plan.headers(), settled);

        // And every row of a later page comes out the same width.
        for row in plan.rows(&all[1..]) {
            assert_eq!(row.len(), settled.len());
        }
    }

    #[test]
    fn a_feature_first_seen_on_a_later_page_gains_a_column() {
        // Losing the value would be the alternative, and columns only ever
        // gain, so the frame notices and lays itself out again.
        let all = specimens(include_str!("../../testdata/specimens.json"));
        let with_moca = all
            .iter()
            .position(|it| {
                it.measurements
                    .iter()
                    .any(|m| m.feature_type.title.as_deref() == Some("MoCA score"))
            })
            .expect("one of the fixture donors has a MoCA score");
        let without: Vec<&Specimen> = all
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != with_moca)
            .map(|(_, it)| it)
            .collect();

        // A plan built without that donor has no column for it.
        let mut plan = Plan::default();
        for specimen in &without {
            plan.extend(std::slice::from_ref(*specimen));
        }
        assert!(!plan.headers().iter().any(|name| name == "MoCA score"));

        // Reading the page that has one adds it.
        assert!(plan.extend(&all[with_moca..=with_moca]));
        assert!(plan.headers().iter().any(|name| name == "MoCA score"));
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
    fn a_project_with_no_title_is_named_after_what_was_asked_for() {
        assert_eq!(label_for("ABC", Some("SEA-AD donors")), "SEA-AD donors");
        assert_eq!(label_for("ABC", None), "Specimens ABC");
        assert_eq!(label_for("ABC", Some("  ")), "Specimens ABC");
    }
}
