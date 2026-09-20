//! The platform's projects, listed for the ones whose specimens are a table.
//!
//! A separate catalog from [`super::Bkp`] rather than more entries in it,
//! because they answer different questions: that one lists a dataset's
//! *visualizations*, and this lists *projects*. They are also two independent
//! queries, so listing them apart means a slow or failing one costs only its
//! own entries.
//!
//! Which projects qualify is the platform's own answer rather than a guess:
//! every project carries a list of capabilities, and three of them mean there
//! are specimens to tabulate. The URL an entry hands out is the one
//! [`crate::formats::specimens`] reads, so choosing one from the dropdown and
//! pasting its address do the same thing.

use futures::future::BoxFuture;
use serde::Deserialize;

use super::PRODUCTION;
use crate::catalog::{Catalog, Entry};

/// Projects asked for in one request. The platform held 172 of them when this
/// was written, so one page is normally the whole inventory.
const PAGE: usize = 500;

/// Enough pages for 50,000 projects, so a listing cannot run forever.
const MAX_PAGES: usize = 100;

/// The capabilities that mean a project's specimens can be tabulated.
///
/// Measured against the live API on 2026-09-20 rather than read off the names:
/// every project carrying one of these answered `aio_specimen` with rows — 38
/// for the marmoset atlas, 10,901 for the Genetic Tools Atlas — and every
/// project carrying only `BKP_DATASET`, `OME_ZARR`, `SPECIMEN_FILES` or
/// nothing at all answered with none. `SPECIMEN_FILES` is the trap: it reads
/// as though it should qualify and does not, and no project carries it alone.
const TABULATED: [&str; 3] = ["SPECIMEN", "DONOR", "PARTITIONED_SPECIMEN"];

const QUERY: &str = "query($limit: Int, $offset: Int) {
  dataCollectionProjectInventory(limit: $limit, offset: $offset) {
    referenceId
    title
    shortTitle
    capabilities
  }
}";

/// The projects whose specimens the viewer can show as a table.
pub struct SpecimenTables {
    endpoint: String,
}

impl SpecimenTables {
    pub fn production() -> Self {
        SpecimenTables {
            endpoint: PRODUCTION.to_string(),
        }
    }
}

impl Catalog for SpecimenTables {
    fn name(&self) -> &str {
        "BKP specimen tables"
    }

    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
        Box::pin(list(self.endpoint.clone()))
    }
}

async fn list(endpoint: String) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    for page in 0..MAX_PAGES {
        let body = serde_json::json!({
            "query": QUERY,
            "variables": { "limit": PAGE, "offset": page * PAGE },
        });
        let text = crate::app::net::post_json(&endpoint, body.to_string()).await?;
        let (found, read) = parse_page(&endpoint, &text)?;
        entries.extend(found);
        // A short page is the last one, whether or not any of it qualified.
        if read < PAGE {
            break;
        }
    }
    // The API's own order changes from one request to the next.
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    Ok(entries)
}

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    data_collection_project_inventory: Option<Vec<Project>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    reference_id: String,
    title: Option<String>,
    short_title: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
}

/// Whether the platform says this project's specimens are worth asking for.
fn tabulated(capabilities: &[String]) -> bool {
    capabilities
        .iter()
        .any(|capability| TABULATED.contains(&capability.as_str()))
}

/// The address a project's specimens are read from.
///
/// The endpoint with the project on its query string, which is exactly what
/// [`crate::formats::specimens`] takes apart again — so an entry chosen from
/// the dropdown and the same address pasted into the URL field reach the same
/// source rather than opening it twice.
fn address(endpoint: &str, project: &str) -> String {
    format!("{endpoint}?specimens={project}")
}

/// The qualifying entries on a page, and how many projects the page held.
fn parse_page(endpoint: &str, text: &str) -> Result<(Vec<Entry>, usize), String> {
    let response: Response =
        serde_json::from_str(text).map_err(|e| format!("parsing BKP projects: {e}"))?;
    if let Some(error) = response.errors.first() {
        return Err(format!("BKP projects: {}", error.message));
    }
    let projects = response
        .data
        .and_then(|data| data.data_collection_project_inventory)
        .ok_or("BKP projects: the response held no projects")?;

    let read = projects.len();
    let entries = projects
        .into_iter()
        .filter(|project| tabulated(&project.capabilities))
        .map(|project| {
            // The long title is what a project is known by; the short one is
            // worth searching on when they differ.
            let title = project
                .title
                .filter(|title| !title.trim().is_empty())
                .or_else(|| project.short_title.clone());
            let short = project.short_title.unwrap_or_default();
            Entry {
                name: title.unwrap_or_else(|| project.reference_id.clone()),
                kind: "Specimen table".into(),
                url: address(endpoint, &project.reference_id),
                keywords: short,
                // Specimens are records, not cells: nothing here describes
                // the cells of anything.
                cells: None,
            }
        })
        .collect();
    Ok((entries, read))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../testdata/bkp_projects.json");

    #[test]
    fn only_the_projects_that_have_specimens_are_offered() {
        let (entries, read) = parse_page(PRODUCTION, FIXTURE).unwrap();
        assert_eq!(read, 6, "every project on the page is counted");
        assert_eq!(entries.len(), 4, "only the ones with specimens are listed");
        assert!(
            entries
                .iter()
                .all(|entry| entry.kind == "Specimen table" && entry.cells.is_none())
        );
    }

    #[test]
    fn the_capability_is_what_decides_rather_than_the_name() {
        let caps = |names: &[&str]| -> Vec<String> {
            names.iter().map(|name| (*name).to_string()).collect()
        };
        assert!(tabulated(&caps(&["SPECIMEN"])));
        assert!(tabulated(&caps(&["DONOR"])));
        assert!(tabulated(&caps(&[
            "PARTITIONED_SPECIMEN",
            "SPECIMEN_FILES"
        ])));
        // Files of specimens are not specimens, which is the one that reads
        // as though it should qualify.
        assert!(!tabulated(&caps(&["SPECIMEN_FILES"])));
        assert!(!tabulated(&caps(&["BKP_DATASET", "OME_ZARR"])));
        assert!(!tabulated(&[]));
    }

    #[test]
    fn an_entry_is_addressed_the_way_the_reader_takes_it_apart() {
        let (entries, _) = parse_page(PRODUCTION, FIXTURE).unwrap();
        for entry in &entries {
            let (endpoint, project) =
                crate::formats::specimens::query_of(&entry.url).expect(&entry.url);
            assert_eq!(endpoint, PRODUCTION);
            assert!(!project.is_empty());
        }
    }

    #[test]
    fn a_project_is_listed_under_its_long_title_and_found_by_its_short_one() {
        let (entries, _) = parse_page(PRODUCTION, FIXTURE).unwrap();
        let sea_ad = entries
            .iter()
            .find(|entry| entry.url.ends_with("JGN327NUXRZSHEV88TN"))
            .unwrap();
        assert!(sea_ad.name.contains("Seattle Alzheimer"));
        assert!(sea_ad.name.len() > sea_ad.keywords.len());
    }

    #[test]
    fn an_api_error_is_reported_rather_than_listed_as_nothing() {
        let text = r#"{"errors":[{"message":"no such field"}]}"#;
        let error = parse_page(PRODUCTION, text).unwrap_err();
        assert!(error.contains("BKP projects"));
        assert!(error.contains("no such field"));
    }

    #[test]
    #[ignore = "reads the live BKP API"]
    fn every_live_entry_names_something_that_opens() {
        let entries = crate::app::net::block_on(list(PRODUCTION.to_string())).unwrap();
        println!("BKP specimen tables: {} projects", entries.len());
        assert!(!entries.is_empty());
        for entry in &entries {
            println!("  {} — {}", entry.name, entry.url);
            assert!(crate::formats::specimens::query_of(&entry.url).is_some());
        }
        // Opening every one would read tens of megabytes, so the smallest
        // stands for the rest: what this is checking is that the address a
        // listing hands out is one `discover` recognises.
        let smallest = entries
            .iter()
            .find(|entry| entry.url.ends_with("5O3GAQDWZK5ZM3IMWWB"))
            .expect("the marmoset atlas is the smallest specimen table");
        let opened = crate::app::net::block_on(crate::formats::discover::discover(&smallest.url))
            .expect("the marmoset specimens should open");
        assert_eq!(opened.name(), smallest.name);
    }
}
