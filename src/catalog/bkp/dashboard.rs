//! The Brain Knowledge Platform's front page: its projects counted by what
//! they study and how, the cells in its datasets, its specimen tables, and
//! its newest datasets to open.
//!
//! Asked in three requests. The datasets are listed first, since the cells
//! are counted a dataset at a time and only the list says which there are;
//! then the cells and everything else are asked for together. Counting the
//! cells is what takes the time — about 4 s for 29 datasets on 2026-09-26,
//! the same whether asked in one request or split across ten, so it is one —
//! and the rest, about 1 s, is done in its shadow.
//!
//! Projects are counted by `dataCollectionProjectInventoryCounts`, which
//! groups by one field per call. A project can have several modalities,
//! species and techniques, so the bars of one breakdown do not add up to the
//! number of projects.

use std::collections::{BTreeMap, HashMap};

use futures::future::join;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::projects::{Project, entry as table_entry, tabulated};
use super::{Dataset, entries_of};
use crate::app::graphql::{self, Response};
use crate::catalog::Entry;
use crate::catalog::dashboard::{Bar, Block, Dashboard, Figure, sentence_case};
use crate::source::compact_count;

/// How many of the newest visualizations are offered.
const NEWEST: usize = 6;

/// How many bars a breakdown of something long, like species, keeps.
const LARGEST: usize = 10;

/// Every dataset, newest first. The platform held 29 on 2026-09-26, so one
/// page of the most it sends is all of them for a while yet.
const DATASETS: &str = "{
  bkpDatasets(first: 50, order: [{ createdAt: DESC }]) {
    totalCount
    nodes {
      referenceId
      projectReferenceId
      dataCollectionReferenceId
      version
      title
      shortTitle
      createdAt
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

/// The project fields counted by, the alias each is asked under, and what
/// the breakdown is called.
const GROUPINGS: [(&str, &str, &str); 4] = [
    ("modality", "modality.name", "Projects by modality"),
    ("species", "species.name", "Projects by species"),
    ("technique", "technique.name", "Projects by technique"),
    ("program", "program.shortTitle", "Projects by program"),
];

/// The projects counted every way at once, their specimens counted by
/// project, and the projects themselves to name those by.
fn inventory_query() -> String {
    let mut query = String::from("{\n");
    for (alias, field, _) in GROUPINGS {
        query.push_str(&format!(
            "  {alias}: dataCollectionProjectInventoryCounts(groupBy: [\"{field}\"]) {{ count properties {{ property value }} }}\n"
        ));
    }
    query.push_str(
        "  specimens: aio_specimenCounts(groupBy: [\"projectReferenceIds\"]) { count properties { property value } }
  projects: dataCollectionProjectInventory(limit: 500) { referenceId title shortTitle capabilities }
}",
    );
    query
}

/// Every dataset's cells, an alias each. Grouped by nothing, a dataset's
/// cells come back as one count.
fn cells_query(datasets: &[Dataset]) -> String {
    let mut query = String::from("{\n");
    for (at, dataset) in datasets.iter().enumerate() {
        let [collection, reference, version] = [
            &dataset.data_collection_reference_id,
            &dataset.reference_id,
            &dataset.version,
        ]
        .map(|id| serde_json::Value::from(id.as_str()));
        query.push_str(&format!(
            "  d{at}: cellCounts(datasetFilter: {{ dataCollectionReferenceId: {collection}, datasetReferenceId: {reference}, version: {version} }}, groupBy: []) {{ count }}\n"
        ));
    }
    query.push('}');
    query
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    bkp_datasets: Connection,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    total_count: u64,
    nodes: Vec<Dataset>,
}

#[derive(Deserialize)]
struct Inventory {
    projects: Vec<Project>,
    specimens: Vec<Group>,
    /// The groupings, by alias.
    #[serde(flatten)]
    counted: BTreeMap<String, Vec<Group>>,
}

/// One row of a grouped count: the value grouped by, and how many hold it.
#[derive(Deserialize)]
struct Group {
    count: f64,
    #[serde(default)]
    properties: Option<Vec<Property>>,
}

#[derive(Deserialize)]
struct Property {
    value: Option<String>,
}

impl Group {
    fn value(&self) -> Option<&str> {
        self.properties.as_ref()?.first()?.value.as_deref()
    }
}

#[derive(Deserialize)]
struct Counted {
    count: f64,
}

/// Read one answer, saying in words what went wrong if anything did.
fn parse<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    let unreadable = |e: serde_json::Error| {
        let start: String = text.chars().take(300).collect();
        format!("reading the platform's counts: {e}: {start}")
    };
    Response::<serde_json::Value>::parse(text)
        .map_err(unreadable)?
        .strict()
        .map_err(|error| format!("Brain Knowledge Platform: {}", error.message))?
        .ok_or_else(|| "Brain Knowledge Platform: the answer held no counts".to_string())
        .and_then(|data| serde_json::from_value(data).map_err(unreadable))
}

/// Count what the platform holds, and pick what to offer from it.
pub async fn dashboard(endpoint: &str) -> Result<Dashboard, String> {
    let text = graphql::post(endpoint, DATASETS, serde_json::json!({}), None).await?;
    let listed: Listed = parse(&text)?;
    let datasets = listed.bkp_datasets;
    let (cells, inventory) = join(
        graphql::post(
            endpoint,
            &cells_query(&datasets.nodes),
            serde_json::json!({}),
            None,
        ),
        graphql::post(endpoint, &inventory_query(), serde_json::json!({}), None),
    )
    .await;
    let cells: BTreeMap<String, Vec<Counted>> = parse(&cells?)?;
    let inventory: Inventory = parse(&inventory?)?;
    Ok(build(endpoint, datasets, &cells, inventory))
}

fn build(
    endpoint: &str,
    datasets: Connection,
    cells: &BTreeMap<String, Vec<Counted>>,
    mut inventory: Inventory,
) -> Dashboard {
    let cells_of: Vec<u64> = (0..datasets.nodes.len())
        .map(|at| {
            cells
                .get(&format!("d{at}"))
                .map_or(0, |rows| rows.iter().map(|row| row.count as u64).sum())
        })
        .collect();
    let all_cells: u64 = cells_of.iter().sum();
    let by_dataset = Block::largest(
        "Cells by dataset",
        datasets
            .nodes
            .iter()
            .zip(&cells_of)
            .map(|(dataset, &value)| Bar {
                label: dataset.short_title.clone(),
                value,
            }),
        LARGEST,
    );

    let specimens_of: HashMap<&str, u64> = inventory
        .specimens
        .iter()
        .filter_map(|group| Some((group.value()?, group.count as u64)))
        .collect();
    let all_specimens: u64 = specimens_of.values().sum();
    let projects = inventory.projects.len() as u64;
    let mut tables: Vec<(u64, Entry)> = std::mem::take(&mut inventory.projects)
        .into_iter()
        .filter(|project| tabulated(&project.capabilities))
        .map(|project| {
            let specimens = specimens_of
                .get(project.reference_id.as_str())
                .copied()
                .unwrap_or(0);
            let mut entry = table_entry(endpoint, project);
            entry.kind = format!("{} specimens", compact_count(specimens));
            (specimens, entry)
        })
        .collect();
    tables.sort_by_key(|(specimens, _)| std::cmp::Reverse(*specimens));
    let groupings = GROUPINGS.map(|(alias, _, title)| {
        let bars: Vec<Bar> = inventory
            .counted
            .remove(alias)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|group| {
                Some(Bar {
                    label: sentence_case(group.value()?),
                    value: group.count as u64,
                })
            })
            .collect();
        (bars.len(), Block::largest(title, bars, LARGEST))
    });
    let [
        (_, modalities),
        (species, by_species),
        (techniques, by_technique),
        (programs, by_program),
    ] = groupings;

    let newest: Vec<Entry> = datasets
        .nodes
        .into_iter()
        .flat_map(|dataset| {
            let day = dataset
                .created_at
                .as_deref()
                .and_then(|at| at.get(..10))
                .map(str::to_string);
            entries_of(endpoint, dataset)
                .into_iter()
                .map(move |mut entry| {
                    if let Some(day) = &day {
                        entry.kind = format!("{} \u{00b7} {day}", entry.kind);
                    }
                    entry
                })
        })
        .take(NEWEST)
        .collect();

    let figures = Block::Figures(vec![
        Figure::new("Projects", projects).note(format!("In {programs} programs")),
        Figure::new("Datasets", datasets.total_count).note("Open in the viewer"),
        Figure::new("Cells", all_cells).note("Some datasets share cells"),
        Figure::new("Specimen records", all_specimens)
            .note(format!("In {} projects", tables.len())),
        Figure::new("Species", species as u64).note("Across its projects"),
        Figure::new("Techniques", techniques as u64),
    ]);

    let blocks = vec![
        figures,
        Block::Datasets {
            title: "Newest datasets".into(),
            note: Some("The latest visualizations added to the platform.".into()),
            entries: newest,
        },
        Block::Datasets {
            title: "Specimen tables".into(),
            note: Some("Each project's specimens, as a table to sort and filter.".into()),
            entries: tables.into_iter().map(|(_, entry)| entry).collect(),
        },
        by_dataset,
        modalities,
        by_species,
        by_technique,
        by_program,
    ];
    Dashboard {
        blocks: blocks
            .into_iter()
            .filter(|block| !block.is_empty())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::bkp::PRODUCTION;

    /// Cut down from what production answered on 2026-09-26.
    const DATASETS_TEXT: &str = r#"{"data":{"bkpDatasets":{"totalCount": 29, "nodes": [
        {"referenceId": "A", "projectReferenceId": "P", "dataCollectionReferenceId": "C",
         "version": "v1", "title": "SEA-AD Caudate Head", "shortTitle": "SEA-AD CaH",
         "createdAt": "2026-09-02T17:00:00Z",
         "visualizations": [{"__typename": "Umap", "title": "UMAP",
                             "url": "https://store/A/ScatterBrain.json"}]},
        {"referenceId": "B", "projectReferenceId": "Q", "dataCollectionReferenceId": "C",
         "version": "v1", "title": "Zhuang ABCA 1", "shortTitle": "Zhuang-ABCA-1",
         "createdAt": "2024-02-16T00:00:00Z",
         "visualizations": [{"__typename": "CoronalGrid", "title": "Grid",
                             "url": "https://store/B/ScatterBrain.json"}]}
    ]}}}"#;

    const CELLS_TEXT: &str = r#"{"data":{
        "d0": [{"count": 686439.0}],
        "d1": [{"count": 2846908.0}]
    }}"#;

    const INVENTORY_TEXT: &str = r#"{"data":{
        "modality": [{"count": 80.0, "properties": [{"property": "modality.name", "value": "transcriptomics"}]},
                     {"count": 20.0, "properties": [{"property": "modality.name", "value": "spatial transcriptomics"}]}],
        "species": [{"count": 101.0, "properties": [{"property": "species.name", "value": "mouse"}]},
                    {"count": 69.0, "properties": [{"property": "species.name", "value": "human"}]},
                    {"count": 1.0, "properties": null}],
        "technique": [{"count": 11.0, "properties": [{"property": "technique.name", "value": "MERFISH"}]}],
        "program": [{"count": 104.0, "properties": [{"property": "program.shortTitle", "value": "BICCN"}]},
                    {"count": 64.0, "properties": [{"property": "program.shortTitle", "value": "Allen Brain Map"}]}],
        "specimens": [{"count": 84.0, "properties": [{"property": "projectReferenceIds", "value": "JGN327NUXRZSHEV88TN"}]},
                      {"count": 10901.0, "properties": [{"property": "projectReferenceIds", "value": "7CVKSF7QGAKIQ8LM5LC"}]}],
        "projects": [
            {"referenceId": "JGN327NUXRZSHEV88TN", "title": "SEA-AD Metadata and Neuropathology",
             "shortTitle": "SEA-AD Metadata and Neuropath", "capabilities": ["DONOR"]},
            {"referenceId": "7CVKSF7QGAKIQ8LM5LC", "title": "Genetic Tools Atlas",
             "shortTitle": "Genetic Tools Atlas", "capabilities": ["SPECIMEN", "SPECIMEN_FILES"]},
            {"referenceId": "LVDBJAW8BI5YSS1QUBG", "title": "Whole mouse brain",
             "shortTitle": "WMB", "capabilities": ["BKP_DATASET"]}
        ]
    }}"#;

    fn built() -> Dashboard {
        let listed: Listed = parse(DATASETS_TEXT).unwrap();
        let cells: BTreeMap<String, Vec<Counted>> = parse(CELLS_TEXT).unwrap();
        let inventory: Inventory = parse(INVENTORY_TEXT).unwrap();
        build(PRODUCTION, listed.bkp_datasets, &cells, inventory)
    }

    fn datasets<'a>(dashboard: &'a Dashboard, title: &str) -> &'a [Entry] {
        dashboard
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Datasets {
                    title: of, entries, ..
                } if of == title => Some(entries.as_slice()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {title}"))
    }

    fn bars<'a>(dashboard: &'a Dashboard, title: &str) -> &'a [Bar] {
        dashboard
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Breakdown {
                    title: of, bars, ..
                } if of == title => Some(bars.as_slice()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {title}"))
    }

    #[test]
    fn the_newest_datasets_open_as_the_catalog_lists_them() {
        let dashboard = built();
        let newest = datasets(&dashboard, "Newest datasets");
        assert_eq!(newest[0].name, "SEA-AD CaH");
        assert_eq!(newest[0].kind, "UMAP \u{00b7} 2026-09-02");
        assert!(
            newest[0].cells.is_some(),
            "the platform describes its cells"
        );
        assert_eq!(newest[1].kind, "Coronal grid \u{00b7} 2024-02-16");
    }

    #[test]
    fn specimen_tables_lead_with_the_most_specimens() {
        let dashboard = built();
        let tables = datasets(&dashboard, "Specimen tables");
        let found: Vec<(&str, &str)> = tables
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("Genetic Tools Atlas", "10.9K specimens"),
                ("SEA-AD Metadata and Neuropathology", "84 specimens"),
            ]
        );
        assert!(tables[0].url.ends_with("?specimens=7CVKSF7QGAKIQ8LM5LC"));
    }

    #[test]
    fn cells_are_counted_a_dataset_at_a_time() {
        let dashboard = built();
        let cells = bars(&dashboard, "Cells by dataset");
        assert_eq!(cells[0].label, "Zhuang-ABCA-1");
        assert_eq!(cells[0].value, 2_846_908);
        let Block::Figures(figures) = &dashboard.blocks[0] else {
            panic!("figures lead");
        };
        let value = |label: &str| {
            figures
                .iter()
                .find(|figure| figure.label == label)
                .map(|figure| figure.value)
        };
        assert_eq!(value("Cells"), Some(686_439 + 2_846_908));
        assert_eq!(value("Projects"), Some(3));
        assert_eq!(value("Datasets"), Some(29));
        assert_eq!(value("Specimen records"), Some(10_985));
        assert_eq!(
            value("Species"),
            Some(2),
            "a group with no value is no species"
        );
    }

    #[test]
    fn a_group_with_no_value_is_left_out_of_its_bars() {
        let dashboard = built();
        let species = bars(&dashboard, "Projects by species");
        assert_eq!(species.len(), 2);
        assert_eq!(species[0].label, "Mouse");
    }

    #[test]
    #[ignore = "reads the live Brain Knowledge Platform"]
    fn a_live_dashboard_counts_and_offers_datasets_that_open() {
        let started = std::time::Instant::now();
        let dashboard = crate::app::net::block_on(dashboard(PRODUCTION)).unwrap();
        println!("counted in {:?}", started.elapsed());
        for block in &dashboard.blocks {
            println!("{block:?}\n");
        }
        let newest = datasets(&dashboard, "Newest datasets");
        assert_eq!(newest.len(), NEWEST);
        crate::app::net::block_on(crate::formats::discover::discover(&newest[0].url))
            .expect("the newest dataset should open");
        let tables = datasets(&dashboard, "Specimen tables");
        crate::app::net::block_on(crate::formats::discover::discover(&tables[0].url))
            .expect("the largest specimen table should open");
    }
}
