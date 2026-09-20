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

pub mod cells;
pub mod projects;

use std::sync::Arc;

use futures::future::BoxFuture;
use serde::Deserialize;

use super::examples::Example;
use super::{Catalog, CellService, Entry};

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

/// Visualizations from the platform worth opening first, one or two of each
/// kind it has: a UMAP, a coronal and a sagittal grid, and grids of sections.
///
/// Written at their production addresses, which are the ones the catalog
/// lists, so each is matched to its entry and shows the platform's labels and
/// colors — and, for the imputed genes, its gene search — once the listing
/// lands.
pub const EXAMPLES: [Example; 7] = [
    Example {
        name: "Whole mouse brain, 10x scRNA-seq",
        kind: "UMAP",
        url: "https://bkp-2d-visualizations.s3.amazonaws.com/wmb_tenx_02082024-20240220165404/G4I4GFJXJB9ATZ3PTX1/ScatterBrain.json",
    },
    Example {
        name: "SEA-AD snRNA-seq, MTG and DLPFC",
        kind: "UMAP",
        url: "https://bkp-2d-visualizations.s3.amazonaws.com/bkppg-sfs-prod-sea-ad-tenx-updated-and-reingested-20250725-v2-20250728232258/98JE3Z1ILSDCIEMA6LQ/ScatterBrain.json",
    },
    Example {
        name: "Developing mouse visual cortex",
        kind: "UMAP",
        url: "https://bkp-2d-visualizations.s3.amazonaws.com/bkppg-abca-prod-dev-mouse-051826-20260518194401/592FE9657CFF611153/ScatterBrain.json",
    },
    Example {
        name: "Zhuang-ABCA-1 MERFISH",
        kind: "Coronal grid",
        url: "https://bkp-2d-visualizations.s3.amazonaws.com/zhuang1_01262024-20240216212536/MGA5LUTH4ETM859L5IM/ScatterBrain.json",
    },
    Example {
        name: "Zhuang-ABCA-3 MERFISH",
        kind: "Sagittal grid",
        url: "https://bkp-2d-visualizations.s3.amazonaws.com/zhuang3_01262024-20240216223831/040LTKC6FZ4NDT2ADYB/ScatterBrain.json",
    },
    Example {
        name: "MERFISH with imputed genes",
        kind: "Grid of sections",
        url: "https://bkp-2d-visualizations.s3.amazonaws.com/bkppg-sfs-prod-wmb-imputed-genes-20240926234907/6MT7UC6ETYECBWF50PK/ScatterBrain.json",
    },
    Example {
        name: "Human basal ganglia spatial atlas",
        kind: "Grid of sections",
        url: "https://bkp-2d-visualizations.s3.amazonaws.com/bkppg-sfs-prod-hmba_bg_spatial_human_10012025-20251011165634/HZEYXSQOEDND2Q6M97M/ScatterBrain.json",
    },
];

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

impl Catalog for Bkp {
    fn name(&self) -> &str {
        "Brain Knowledge Platform"
    }

    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
        Box::pin(list(self.endpoint.clone()))
    }
}

async fn list(endpoint: String) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    let mut after = None;
    for _ in 0..MAX_PAGES {
        let body = serde_json::json!({
            "query": QUERY,
            "variables": { "first": PAGE, "after": after },
        });
        let (page, next) = parse_page(
            &endpoint,
            &crate::app::net::post_json(&endpoint, body.to_string()).await?,
        )?;
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
    let response: Response =
        serde_json::from_str(text).map_err(|e| format!("parsing BKP datasets: {e}"))?;
    if let Some(error) = response.errors.first() {
        return Err(format!("BKP datasets: {}", error.message));
    }
    let connection = response
        .data
        .and_then(|data| data.bkp_datasets)
        .ok_or("BKP datasets: the response held no datasets")?;

    let mut entries = Vec::new();
    for dataset in connection.nodes {
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
                url,
                keywords: format!("{} {}", dataset.title, visualization.title),
                cells: Some(cells.clone()),
            });
        }
    }

    let next = connection
        .page_info
        .has_next_page
        .then_some(connection.page_info.end_cursor)
        .flatten();
    Ok((entries, next))
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
    fn the_examples_show_several_kinds_of_visualization() {
        let mut kinds: Vec<&str> = EXAMPLES.iter().map(|example| example.kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        assert!(kinds.len() >= 3, "{kinds:?}");
    }

    #[test]
    fn no_example_is_offered_twice() {
        let mut urls: Vec<&str> = EXAMPLES
            .iter()
            .chain(&super::super::examples::EXAMPLES)
            .map(|example| example.url)
            .collect();
        let count = urls.len();
        urls.sort_unstable();
        urls.dedup();
        assert_eq!(urls.len(), count);
    }

    #[test]
    #[ignore = "reads every example from the live store"]
    fn every_example_is_recognised() {
        for example in &EXAMPLES {
            let found = crate::app::net::block_on(crate::formats::discover::discover(example.url));
            assert!(found.is_ok(), "{}: {}", example.name, found.err().unwrap());
        }
    }

    #[test]
    #[ignore = "reads the live BKP API"]
    fn every_example_is_still_listed() {
        // An example is a BKP dataset only while the catalog lists its exact
        // address: that is what names it and describes its cells and genes. A
        // re-ingest moves a visualization to a new address, and this is what
        // notices.
        let entries = crate::app::net::block_on(list(PRODUCTION.to_string())).unwrap();
        for example in &EXAMPLES {
            let entry = entries.iter().find(|entry| entry.url == example.url);
            assert!(entry.is_some(), "{} is no longer listed", example.name);
            assert!(entry.unwrap().cells.is_some());
        }
    }

    #[test]
    #[ignore = "reads the live BKP API and every dataset it lists"]
    fn every_live_entry_is_recognised() {
        let entries = crate::app::net::block_on(list(PRODUCTION.to_string())).unwrap();
        assert!(!entries.is_empty());
        for entry in &entries {
            let found = crate::app::net::block_on(crate::formats::discover::discover(&entry.url));
            assert!(found.is_ok(), "{}: {}", entry.name, found.err().unwrap());
        }
    }
}
