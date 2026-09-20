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

use serde::Deserialize;
use serde_json::json;

use super::table::{MAX_ROWS, Table};

/// Specimens asked for in one request.
///
/// Measured against the SEA-AD donor project: 84 specimens of 30 features came
/// to 199KB in 0.7s, so a page of this many is a request of a megabyte or two
/// for the widest records the platform holds.
const PAGE: usize = 500;

/// The query. Both root fields in one request, so naming the table costs no
/// extra round trip.
///
/// A specimen's own identifier is its `cRID` symbol — the accession the portal
/// shows — rather than its `referenceId`, which is an internal handle nobody
/// reading the table would recognise.
const QUERY: &str = "query($project: [Filter], $specimens: [Filter], $limit: Int, $offset: Int) {
  project: dataCollectionProjectInventory(filter: $project, limit: 1) {
    referenceId
    title
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

#[derive(Deserialize)]
struct Data {
    #[serde(default)]
    project: Vec<Project>,
    #[serde(default)]
    aio_specimen: Vec<Specimen>,
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
    #[serde(default)]
    annotations: Vec<Annotation>,
    #[serde(default)]
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
    #[serde(default)]
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

/// Read every specimen in a project, a page at a time.
pub async fn read(endpoint: &str, project: &str) -> Result<Table, String> {
    let mut specimens = Vec::new();
    let mut title = None;
    let mut more = false;

    while specimens.len() < MAX_ROWS {
        let page = ask(endpoint, project, specimens.len()).await?;
        title = title.or_else(|| page.project.first().and_then(|it| it.title.clone()));
        let read = page.aio_specimen.len();
        specimens.extend(page.aio_specimen);
        // A short page is the last one. A full one says nothing either way, so
        // the next is asked for and may come back empty.
        if read < PAGE {
            break;
        }
        if specimens.len() >= MAX_ROWS {
            more = true;
        }
    }

    if specimens.is_empty() {
        return Err(format!(
            "{endpoint} knows no specimens in project {project}"
        ));
    }

    Ok(tabulate(
        label_for(project, title.as_deref()),
        &specimens,
        more.then(|| format!("read as far as {MAX_ROWS} rows, and there are more")),
    ))
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

/// Turn specimens into rows, with a column per feature any of them carries.
///
/// Columns are ordered so a table reads the same way twice: what identifies a
/// specimen first, then its annotations and its measurements, each by name.
/// The platform's own order is not stable between requests.
fn tabulate(name: String, specimens: &[Specimen], note: Option<String>) -> Table {
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
                // The unit belongs to the feature rather than to the reading,
                // so the first one seen names the column for all of them.
                measurements
                    .entry(title)
                    .or_insert_with(|| measurement.unit.as_deref().filter(|it| !it.is_empty()));
            }
        }
    }

    let mut headers = vec![SPECIMEN.to_string(), KIND.to_string()];
    headers.extend(annotations.keys().map(|title| (*title).to_string()));
    headers.extend(measurements.iter().map(|(title, unit)| match unit {
        Some(unit) => format!("{title} ({unit})"),
        None => (*title).to_string(),
    }));

    let rows = specimens
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
            row.extend(annotations.keys().map(|title| annotated(specimen, title)));
            row.extend(measurements.keys().map(|title| measured(specimen, title)));
            row
        })
        .collect();

    Table::new(
        name,
        format!(
            "Brain Knowledge Platform specimens, {} columns",
            headers.len()
        ),
        headers,
        rows,
        note,
    )
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

/// Read a saved answer, for tests and for anything that has the JSON already.
#[cfg(test)]
fn from_answer(name: &str, text: &str) -> Result<Table, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let data: Data = serde_json::from_value(value["data"].clone()).map_err(|e| e.to_string())?;
    Ok(tabulate(name.to_string(), &data.aio_specimen, None))
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

    #[test]
    fn a_project_with_no_title_is_named_after_what_was_asked_for() {
        assert_eq!(label_for("ABC", Some("SEA-AD donors")), "SEA-AD donors");
        assert_eq!(label_for("ABC", None), "Specimens ABC");
        assert_eq!(label_for("ABC", Some("  ")), "Specimens ABC");
    }
}
