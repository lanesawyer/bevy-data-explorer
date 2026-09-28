//! The Allen Institute's Brain Knowledge Platform, listed through its GraphQL
//! API.
//!
//! Every BKP dataset carries its visualizations, and each of those names a
//! Scatterbrain metadata file — a single cloud for a UMAP or a coronal grid, a
//! sectioned one for a dynamic grid. That is decided by reading the file, as
//! with anything else, so nothing here depends on the visualization's type
//! beyond the words shown beside it.
//!
//! Every entry also carries the platform's own description of the dataset's
//! cells, from [`cells`], so a BKP dataset shows the labels, colors and counts
//! the portal does rather than the codes its files hold.
//!
//! Its [`dashboard`] counts the platform's projects, cells and specimens, and
//! offers its newest datasets and every specimen table.

pub mod cells;
pub mod dashboard;
pub mod projects;

use std::sync::Arc;

use futures::future::BoxFuture;
use serde::Deserialize;

use super::dashboard::Dashboard;
use super::{Catalog, CellService, Entry, Provider};
use crate::app::graphql::{self, Response};
use crate::source::Category;

pub const PRODUCTION: &str = "https://idf-api-prod.aibs-idk-prod.net/";

/// The most the API returns in one page; asking for more is an error rather
/// than a shorter page.
const PAGE: usize = 50;

/// Enough pages for 5000 datasets, so a cursor that never ends cannot keep a
/// listing going forever.
const MAX_PAGES: usize = 100;

/// The visualization types are an interface, and only the implementations
/// carry a URL, so each has to be asked for by name.
const QUERY: &str = "query($first: Int, $after: String) {
  bkpDatasets(first: $first, after: $after) {
    pageInfo { hasNextPage endCursor }
    nodes {
      referenceId
      projectReferenceId
      dataCollectionReferenceId
      version
      title
      shortTitle
      visualizations {
        __typename
        title
        ... on Umap { url }
        ... on CoronalGrid { url }
        ... on DynamicGrid { url }
      }
    }
  }
}";

pub struct Bkp {
    endpoint: String,
}

impl Bkp {
    pub fn production() -> Self {
        Bkp {
            endpoint: PRODUCTION.to_string(),
        }
    }
}

/// Both of the platform's catalogs, turned off together.
pub const PROVIDER: Provider = Provider {
    key: "bkp",
    name: "Brain Knowledge Platform",
    about: "The Allen Institute's public visualizations and specimen tables.",
};

impl Catalog for Bkp {
    fn name(&self) -> &str {
        "Brain Knowledge Platform"
    }

    fn provider(&self) -> Option<Provider> {
        Some(PROVIDER)
    }

    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
        Box::pin(list(self.endpoint.clone()))
    }

    fn has_dashboard(&self) -> bool {
        true
    }

    fn dashboard(&self) -> BoxFuture<'static, Result<Dashboard, String>> {
        let endpoint = self.endpoint.clone();
        Box::pin(async move { dashboard::dashboard(&endpoint).await })
    }
}

async fn list(endpoint: String) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    let mut after = None;
    for _ in 0..MAX_PAGES {
        let variables = serde_json::json!({ "first": PAGE, "after": after });
        let text = graphql::post(&endpoint, QUERY, variables, None).await?;
        let (page, next) = parse_page(&endpoint, &text)?;
        entries.extend(page);
        after = next;
        if after.is_none() {
            break;
        }
    }
    // The API's own order changes from one request to the next.
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    Ok(entries)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    bkp_datasets: Option<Connection>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    page_info: PageInfo,
    nodes: Vec<Dataset>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dataset {
    reference_id: String,
    project_reference_id: String,
    data_collection_reference_id: String,
    version: String,
    title: String,
    short_title: String,
    visualizations: Vec<Visualization>,
    /// When it was added. Only the dashboard asks for it.
    #[serde(default)]
    created_at: Option<String>,
}

#[derive(Deserialize)]
struct Visualization {
    #[serde(rename = "__typename")]
    typename: String,
    title: String,
    url: Option<String>,
}

/// One page's entries, and the cursor for the next page if there is one.
fn parse_page(endpoint: &str, text: &str) -> Result<(Vec<Entry>, Option<String>), String> {
    let connection = Response::<Data>::parse(text)
        .map_err(|e| format!("parsing BKP datasets: {e}"))?
        .strict()
        .map_err(|error| format!("BKP datasets: {}", error.message))?
        .and_then(|data| data.bkp_datasets)
        .ok_or("BKP datasets: the response held no datasets")?;

    let entries = connection
        .nodes
        .into_iter()
        .flat_map(|dataset| entries_of(endpoint, dataset))
        .collect();

    let next = connection
        .page_info
        .has_next_page
        .then_some(connection.page_info.end_cursor)
        .flatten();
    Ok((entries, next))
}

/// An entry for each of a dataset's visualizations, each carrying the
/// platform's description of its cells.
fn entries_of(endpoint: &str, dataset: Dataset) -> Vec<Entry> {
    let mut entries = Vec::new();
    {
        let several = dataset.visualizations.len() > 1;
        let cells = CellService(Arc::new(cells::BkpCells {
            endpoint: endpoint.to_string(),
            dataset: dataset.reference_id,
            project: dataset.project_reference_id,
            collection: dataset.data_collection_reference_id,
            version: dataset.version,
        }));
        for visualization in dataset.visualizations {
            let Some(url) = visualization.url else {
                continue;
            };
            let name = if several {
                format!("{} · {}", dataset.short_title, visualization.title)
            } else {
                dataset.short_title.clone()
            };
            entries.push(Entry {
                name,
                kind: describe(&visualization.typename).to_string(),
                // Every visualization the platform lists is of cells.
                category: Category::Cells,
                url,
                keywords: format!("{} {}", dataset.title, visualization.title),
                cells: Some(cells.clone()),
            });
        }
    }
    entries
}

fn describe(typename: &str) -> &str {
    match typename {
        "Umap" => "UMAP",
        "CoronalGrid" => "Coronal grid",
        "DynamicGrid" => "Grid of sections",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE_TEXT: &str = r#"{"data":{"bkpDatasets":{
        "pageInfo":{"hasNextPage":true,"endCursor":"NDk="},
        "nodes":[
          {"referenceId":"DATA","projectReferenceId":"PROJ","dataCollectionReferenceId":"COLL",
           "version":"v0","title":"SEA-AD Caudate Head snRNA-seq Atlas","shortTitle":"SEA-AD CaH",
           "visualizations":[{"__typename":"Umap","title":"SEA-AD CaH UMAP",
             "url":"https://store/A/ScatterBrain.json"}]},
          {"referenceId":"DATB","projectReferenceId":"PROJ","dataCollectionReferenceId":"COLL",
           "version":"v1","title":"Two views","shortTitle":"Both",
           "visualizations":[
             {"__typename":"DynamicGrid","title":"Grid","url":"https://store/B/ScatterBrain.json"},
             {"__typename":"Umap","title":"UMAP","url":"https://store/C/ScatterBrain.json"}]}
        ]}}}"#;

    #[test]
    fn every_visualization_is_an_entry() {
        let (entries, next) = parse_page(PRODUCTION, PAGE_TEXT).unwrap();
        assert_eq!(next.as_deref(), Some("NDk="));
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["SEA-AD CaH", "Both · Grid", "Both · UMAP"]);
        assert_eq!(entries[0].kind, "UMAP");
        assert!(entries[0].keywords.contains("Caudate Head"));
        assert_eq!(entries[1].url, "https://store/B/ScatterBrain.json");
        assert!(entries.iter().all(|entry| entry.cells.is_some()));
    }

    #[test]
    fn the_last_page_has_no_cursor_to_follow() {
        let text = PAGE_TEXT.replace("\"hasNextPage\":true", "\"hasNextPage\":false");
        assert_eq!(parse_page(PRODUCTION, &text).unwrap().1, None);
    }

    #[test]
    fn an_error_from_the_api_is_reported_rather_than_listed_as_nothing() {
        let text = r#"{"errors":[{"message":"The maximum allowed items per page were exceeded."}],
                       "data":{"bkpDatasets":null}}"#;
        let error = parse_page(PRODUCTION, text).err().unwrap();
        assert!(error.contains("maximum allowed items"));
    }

    #[test]
    #[ignore = "reads the live BKP API"]
    fn every_example_is_still_listed() {
        // An example is a BKP dataset only while the catalog lists its exact
        // address: that is what names it and describes its cells and genes, and
        // the examples catalog leaves it out on that account. A re-ingest moves
        // a visualization to a new address, and this is what notices.
        let entries = crate::app::net::block_on(list(PRODUCTION.to_string())).unwrap();
        // A specimen table is a project rather than a visualization, and is
        // listed by the query that finds those.
        let (tables, visualizations): (Vec<_>, Vec<_>) = super::super::examples::EXAMPLES
            .iter()
            .filter(|example| example.cataloged)
            .partition(|example| crate::formats::specimens::query_of(example.url).is_some());
        let projects = crate::app::net::block_on(projects::list(PRODUCTION.to_string())).unwrap();
        for example in tables {
            assert!(
                projects.iter().any(|entry| entry.url == example.url),
                "{} is no longer listed",
                example.name
            );
        }
        for example in visualizations {
            let entry = entries.iter().find(|entry| entry.url == example.url);
            assert!(entry.is_some(), "{} is no longer listed", example.name);
            assert!(entry.unwrap().cells.is_some());
        }
    }

    #[test]
    #[ignore = "reads the live BKP API and every dataset it lists"]
    fn every_live_entry_is_recognized() {
        let entries = crate::app::net::block_on(list(PRODUCTION.to_string())).unwrap();
        assert!(!entries.is_empty());
        for entry in &entries {
            let found = crate::app::net::block_on(crate::formats::discover::discover(&entry.url));
            assert!(found.is_ok(), "{}: {}", entry.name, found.err().unwrap());
        }
    }
}
