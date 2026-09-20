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
use serde_json::json;

use crate::app::net::{Fetching, fetching};
use crate::app::schedule::Stage;
use crate::source::SourceBusy;
use crate::source::table::{SourceTable, TableColumn, TablePaging};

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
      featureType { title }
      taxons { symbol }
    }
    measurements {
      featureType { title }
      value
      unit
    }
  }
}";

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
struct Titled {
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
    let answer = ask(endpoint, project, 0).await?;
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

async fn ask(endpoint: &str, project: &str, offset: usize) -> Result<Data, String> {
    #[derive(Deserialize)]
    struct Response {
        data: Option<Data>,
        #[serde(default)]
        errors: Vec<GraphQlError>,
    }
    #[derive(Deserialize)]
    struct GraphQlError {
        message: String,
    }

    let body = json!({
        "query": QUERY,
        "variables": {
            "project": [{ "field": "referenceId", "operator": "EQ", "value": project }],
            "specimens": [{
                "field": "projectReferenceIds", "operator": "EQ", "value": project
            }],
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
    annotations: Vec<String>,
    /// Each measurement's feature and the unit it is recorded in, which names
    /// the column rather than every cell in it.
    measurements: Vec<(String, Option<String>)>,
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
        let mut annotations: BTreeMap<&str, ()> = BTreeMap::new();
        let mut measurements: BTreeMap<&str, Option<&str>> = BTreeMap::new();
        for specimen in specimens {
            for annotation in &specimen.annotations {
                if let Some(title) = annotation.feature_type.title.as_deref() {
                    annotations.insert(title, ());
                }
            }
            for measurement in &specimen.measurements {
                if let Some(title) = measurement.feature_type.title.as_deref() {
                    // The unit belongs to the feature rather than to the
                    // reading, so the first one seen names the column.
                    measurements
                        .entry(title)
                        .or_insert_with(|| measurement.unit.as_deref().filter(|it| !it.is_empty()));
                }
            }
        }

        let before = (self.annotations.len(), self.measurements.len());
        for title in annotations.keys() {
            if !self.annotations.iter().any(|known| known == title) {
                self.annotations.push((*title).to_string());
            }
        }
        for (title, unit) in &measurements {
            if !self.measurements.iter().any(|(known, _)| known == title) {
                self.measurements
                    .push(((*title).to_string(), unit.map(str::to_string)));
            }
        }
        self.annotations.sort();
        self.measurements.sort();
        before != (self.annotations.len(), self.measurements.len())
    }

    fn headers(&self) -> Vec<String> {
        let mut headers = vec![SPECIMEN.to_string(), KIND.to_string()];
        headers.extend(self.annotations.iter().cloned());
        headers.extend(self.measurements.iter().map(|(title, unit)| match unit {
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
                        .map(|title| annotated(specimen, title)),
                );
                row.extend(
                    self.measurements
                        .iter()
                        .map(|(title, _)| measured(specimen, title)),
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
}

impl SpecimenPages {
    pub fn new(endpoint: String, project: String, plan: Plan) -> Self {
        SpecimenPages {
            endpoint,
            project,
            plan,
            fetching: None,
        }
    }
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
    )>,
) {
    for (mut pages, mut paging, mut rows, mut busy) in &mut sources {
        if let Some((page, fetch)) = pages.fetching.as_mut()
            && let Some(answer) = fetch.take()
        {
            let page = *page;
            pages.fetching = None;
            match answer {
                Ok(data) => take_page(&mut pages, &mut rows, page * paging.size, &data),
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

        let wanted = paging.first();
        let asking = pages.fetching.as_ref().map(|(page, _)| page * paging.size);
        if rows.first != wanted && asking != Some(wanted) {
            let (endpoint, project) = (pages.endpoint.clone(), pages.project.clone());
            pages.fetching = Some((
                paging.page,
                fetching(async move { ask(&endpoint, &project, wanted).await }),
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
        app.add_systems(Update, serve_pages.in_set(Stage::Sources));
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
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
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
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
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
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
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
