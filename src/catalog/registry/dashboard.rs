//! The BKP Registry's front page: how much it holds, what of that the viewer
//! can open, and the newest of it.
//!
//! Asked in two requests. The first counts, and lists the species, specimen
//! types and newest assets; the second counts subjects and specimens by the
//! species and types the first named, since the registry has no grouped
//! count to ask for them in one. Every count is a `totalCount` of a filter
//! over populated types, which the registry answers in well under a second —
//! both together took about 2 s against the stage service on 2026-09-26.
//!
//! Images are counted by the modality tag each carries (`tissuecyte`,
//! `brightfield`, `epifluorescence`), which between them covered 18,350 of
//! its 18,696 OME-Zarr stores on 2026-09-26. The light-sheet brains have no
//! such tag on their stores; they are the Neuroglancer states.

use std::collections::{BTreeMap, HashSet};

use serde::Deserialize;

use super::{Asset, entry};
use crate::app::graphql::{self, Response};
use crate::catalog::Entry;
use crate::catalog::dashboard::{Bar, Block, Dashboard, Figure, sentence_case};

/// How many of the newest images are offered.
const NEWEST: usize = 6;
/// How many are asked for to find them: a store is often registered more than
/// once, three times over for one image seen, so the newest few are asked for
/// several times over and told apart by address.
const NEWEST_ASKED: usize = 24;
/// How many of the light-sheet brains are offered.
const BRAINS: usize = 4;

/// The modality tags images carry, and what each is called here.
const MODALITIES: [(&str, &str); 3] = [
    ("tissuecyte", "TissueCyte"),
    ("brightfield", "Brightfield"),
    ("epifluorescence", "Epifluorescence"),
];

/// The statuses an asset may have, and what each is called here. `RETRACTED`
/// is left out, since nothing has been.
const STATUSES: [(&str, &str); 5] = [
    ("PUBLISHED", "Published"),
    ("PENDING_REVIEW", "Pending review"),
    ("ARCHIVED", "Archived"),
    ("QC_PASSED", "Passed QC"),
    ("QC_FAILED", "Failed QC"),
];

/// What people call the species the registry names only in Latin. Its own
/// `commonName` was empty for every one of them on 2026-09-26.
const COMMON_NAMES: [(&str, &str); 7] = [
    ("Mus musculus", "Mouse"),
    ("Homo sapiens", "Human"),
    ("Macaca mulatta", "Rhesus macaque"),
    ("Macaca nemestrina", "Pig-tailed macaque"),
    ("Macaca fascicularis", "Crab-eating macaque"),
    ("Callithrix jacchus", "Marmoset"),
    ("Saimiri sciureus", "Squirrel monkey"),
];

const IMAGES: &str = r#"{ type: { name: { in: ["zarr fileset", "MIP zarr fileset"] } } }"#;
const TABLES: &str = r#"{ type: { name: { in: ["CSV", "parquet"] } } }"#;
const STATES: &str = r#"{ and: [{ type: { name: { eq: "JSON" } } }, { name: { containsInsensitive: "neuroglancer" } }] }"#;

/// What is read of each asset offered.
const ASSET: &str = "nodes { id name type status createdAt tags instances { downloadUrl } }";

/// The first request: the headline counts, the lists the second counts by,
/// and the assets offered.
fn overview_query() -> String {
    let mut query = format!(
        "{{
  assets: dataAssets(first: 1) {{ totalCount }}
  images: dataAssets(first: 1, where: {IMAGES}) {{ totalCount }}
  tables: dataAssets(first: 1, where: {TABLES}) {{ totalCount }}
  subjects(first: 1) {{ totalCount }}
  specimens(first: 1) {{ totalCount }}
  cells(first: 1) {{ totalCount }}
  collections: dataAssetCollections(first: 1) {{ totalCount }}
  species(first: 50) {{ nodes {{ name commonName }} }}
  specimenTypes(first: 50) {{ nodes {{ name label }} }}
  newest: dataAssets(first: {NEWEST_ASKED}, order: [{{ createdAt: DESC }}], where: {{ and: [{IMAGES}, {{ status: {{ eq: PUBLISHED }} }}] }}) {{ {ASSET} }}
  brains: dataAssets(first: {BRAINS}, order: [{{ createdAt: DESC }}], where: {STATES}) {{ totalCount {ASSET} }}
"
    );
    for (tag, _) in MODALITIES {
        query.push_str(&format!(
            "  {tag}: dataAssets(first: 1, order: [{{ createdAt: DESC }}], where: {{ and: [{IMAGES}, {{ status: {{ eq: PUBLISHED }} }}, {{ tags: {{ some: {{ value: {{ eq: \"{tag}\" }} }} }} }}] }}) {{ totalCount {ASSET} }}\n"
        ));
    }
    for (status, _) in STATUSES {
        query.push_str(&format!(
            "  status_{status}: dataAssets(first: 1, where: {{ status: {{ eq: {status} }} }}) {{ totalCount }}\n"
        ));
    }
    query.push('}');
    query
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Overview {
    assets: Count,
    images: Count,
    tables: Count,
    subjects: Count,
    specimens: Count,
    cells: Count,
    collections: Count,
    species: Nodes<Species>,
    specimen_types: Nodes<SpecimenType>,
    newest: Nodes<Asset>,
    brains: Page,
    /// The modalities and the statuses, by alias.
    #[serde(flatten)]
    rest: BTreeMap<String, Page>,
}

#[derive(Deserialize)]
struct Count {
    #[serde(rename = "totalCount")]
    total: u64,
}

#[derive(Deserialize)]
struct Nodes<T> {
    nodes: Vec<T>,
}

/// A count, and whatever nodes were asked for beside it.
#[derive(Deserialize)]
struct Page {
    #[serde(rename = "totalCount")]
    total: u64,
    #[serde(default)]
    nodes: Vec<Asset>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Species {
    name: String,
    common_name: Option<String>,
}

#[derive(Deserialize)]
struct SpecimenType {
    name: String,
    label: String,
}

/// The second request: subjects by species and specimens by type, an alias
/// each, numbered in the order the first request listed them.
fn breakdown_query(overview: &Overview) -> String {
    let mut query = String::from("{\n");
    for (at, species) in overview.species.nodes.iter().enumerate() {
        let name = serde_json::Value::from(species.name.as_str());
        query.push_str(&format!(
            "  species{at}: subjects(first: 1, where: {{ species: {{ name: {{ eq: {name} }} }} }}) {{ totalCount }}\n"
        ));
    }
    for (at, kind) in overview.specimen_types.nodes.iter().enumerate() {
        let name = serde_json::Value::from(kind.name.as_str());
        query.push_str(&format!(
            "  type{at}: specimens(first: 1, where: {{ specimenType: {{ name: {{ eq: {name} }} }} }}) {{ totalCount }}\n"
        ));
    }
    query.push('}');
    query
}

/// Read one answer, saying in words what went wrong if anything did.
///
/// Read loosely before it is read as `T`: a refused request still answers
/// with data, every alias in it null, which `T` cannot hold.
fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, String> {
    let unreadable = |e: serde_json::Error| {
        let start: String = text.chars().take(300).collect();
        format!("reading the BKP Registry's counts: {e}: {start}")
    };
    let response = Response::<serde_json::Value>::parse(text).map_err(unreadable)?;
    let data = response
        .strict()
        .map_err(|error| match error.code() {
            Some(code) if code.starts_with("AUTH_") => {
                "Sign in to the BKP Registry in Settings to see what it holds.".to_string()
            }
            _ => format!("BKP Registry: {}", error.message),
        })?
        .ok_or_else(|| "BKP Registry: the answer held no counts".to_string())?;
    serde_json::from_value(data).map_err(unreadable)
}

/// Count what the registry holds, and pick what to offer from it.
pub async fn dashboard(endpoint: &str, token: &str) -> Result<Dashboard, String> {
    let text = graphql::post(
        endpoint,
        &overview_query(),
        serde_json::json!({}),
        Some(token),
    )
    .await?;
    let overview: Overview = parse(&text)?;
    let text = graphql::post(
        endpoint,
        &breakdown_query(&overview),
        serde_json::json!({}),
        Some(token),
    )
    .await?;
    let breakdown: BTreeMap<String, Count> = parse(&text)?;
    Ok(build(overview, &breakdown))
}

fn build(mut overview: Overview, breakdown: &BTreeMap<String, Count>) -> Dashboard {
    let counted = |alias: String| breakdown.get(&alias).map_or(0, |count| count.total);
    let page =
        |rest: &BTreeMap<String, Page>, alias: &str| rest.get(alias).map_or(0, |page| page.total);
    let published = page(&overview.rest, "status_PUBLISHED");
    let species = overview.species.nodes.len();

    let figures = Block::Figures(vec![
        Figure::new("Data assets", overview.assets.total).note(format!(
            "{} published",
            crate::source::compact_count(published)
        )),
        Figure::new("OME-Zarr images", overview.images.total).note("Open in the viewer"),
        Figure::new("Light-sheet brains", overview.brains.total).note("Neuroglancer states"),
        Figure::new("Tables", overview.tables.total).note("CSV and Parquet"),
        Figure::new("Subjects", overview.subjects.total).note(format!("Across {species} species")),
        Figure::new("Specimens", overview.specimens.total),
        Figure::new("Cells", overview.cells.total),
        Figure::new("Collections", overview.collections.total),
    ]);

    let modalities = Block::breakdown(
        "Images by modality",
        MODALITIES.map(|(tag, label)| Bar {
            label: label.to_string(),
            value: page(&overview.rest, tag),
        }),
    );

    let mut seen = HashSet::new();
    let newest: Vec<Entry> = std::mem::take(&mut overview.newest.nodes)
        .into_iter()
        .filter_map(highlight)
        .filter(|entry| seen.insert(entry.url.clone()))
        .take(NEWEST)
        .collect();

    let each: Vec<Entry> = MODALITIES
        .iter()
        .filter_map(|(tag, _)| overview.rest.get_mut(*tag)?.nodes.pop())
        .filter_map(highlight)
        .collect();

    let brains: Vec<Entry> = std::mem::take(&mut overview.brains.nodes)
        .into_iter()
        .filter_map(highlight)
        .collect();

    let by_species = Block::breakdown(
        "Subjects by species",
        overview
            .species
            .nodes
            .iter()
            .enumerate()
            .map(|(at, species)| Bar {
                label: common_name(species),
                value: counted(format!("species{at}")),
            }),
    );
    let by_type = Block::breakdown(
        "Specimens by type",
        overview
            .specimen_types
            .nodes
            .iter()
            .enumerate()
            .map(|(at, kind)| Bar {
                label: sentence_case(&kind.label),
                value: counted(format!("type{at}")),
            }),
    );
    let by_status = Block::breakdown(
        "Data assets by status",
        STATUSES.map(|(status, label)| Bar {
            label: label.to_string(),
            value: page(&overview.rest, &format!("status_{status}")),
        }),
    );

    let blocks = vec![
        figures,
        Block::Datasets {
            title: "Newest images".into(),
            note: Some("The latest published OME-Zarr stores.".into()),
            entries: newest,
        },
        Block::Datasets {
            title: "One of each modality".into(),
            note: Some("The newest published image of each.".into()),
            entries: each,
        },
        Block::Datasets {
            title: "Light-sheet brains".into(),
            note: Some("Whole SmartSPIM brains, in three channels.".into()),
            entries: brains,
        },
        modalities,
        by_species,
        by_type,
        by_status,
    ];
    Dashboard {
        blocks: blocks
            .into_iter()
            .filter(|block| !block.is_empty())
            .collect(),
    }
}

/// An asset as a dataset to offer, its kind saying which modality it is and
/// when it was registered, since its name says neither.
fn highlight(asset: Asset) -> Option<Entry> {
    let modality = asset.tags.iter().flatten().flatten().find_map(|tag| {
        MODALITIES
            .iter()
            .find(|(of, _)| tag.eq_ignore_ascii_case(of))
            .map(|(_, label)| *label)
    });
    let day = asset
        .created_at
        .as_deref()
        .and_then(|at| at.get(..10))
        .map(str::to_string);
    let mut entry = entry(asset)?;
    let kind = modality.unwrap_or(entry.kind.as_str()).to_string();
    entry.kind = match day {
        Some(day) => format!("{kind} \u{00b7} {day}"),
        None => kind,
    };
    Some(entry)
}

fn common_name(species: &Species) -> String {
    let common = species
        .common_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .or_else(|| {
            COMMON_NAMES
                .iter()
                .find(|(latin, _)| *latin == species.name)
                .map(|(_, common)| *common)
        });
    match common {
        Some(common) => format!("{common} ({})", species.name),
        None => species.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::registry::STAGE;

    /// Cut down from what the stage service answered on 2026-09-26.
    const OVERVIEW: &str = r#"{"data":{
        "assets": {"totalCount": 17815792},
        "images": {"totalCount": 18696},
        "tables": {"totalCount": 80660},
        "subjects": {"totalCount": 5602},
        "specimens": {"totalCount": 30145},
        "cells": {"totalCount": 35152528},
        "collections": {"totalCount": 1511729},
        "species": {"nodes": [{"name": "Homo sapiens", "commonName": null},
                              {"name": "Mus musculus", "commonName": null}]},
        "specimenTypes": {"nodes": [{"name": "brain_specimen_block", "label": "brain specimen block"},
                                    {"name": "library_aliquot", "label": "multiplexed sequencing library"}]},
        "newest": {"nodes": [
            {"id": "1", "name": "ome_zarr_image_series_1343147512-MIP", "type": "zarr fileset",
             "status": "PUBLISHED", "createdAt": "2026-09-24T18:02:11Z",
             "tags": ["epifluorescence", "ome-zarr"],
             "instances": [{"downloadUrl": "s3://public-development-802451596237-us-west-2/1343147512-MIP/ome_zarr_conversion/1343147512-MIP.zarr"}]},
            {"id": "2", "name": "ome_zarr_image_series_1343147512-MIP", "type": "zarr fileset",
             "status": "PUBLISHED", "createdAt": "2026-09-24T18:01:09Z",
             "tags": ["epifluorescence", "ome-zarr"],
             "instances": [{"downloadUrl": "s3://public-development-802451596237-us-west-2/1343147512-MIP/ome_zarr_conversion/1343147512-MIP.zarr"}]},
            {"id": "3", "name": "ome_zarr_image_series_1191617882", "type": "zarr fileset",
             "status": "PUBLISHED", "createdAt": "2026-09-24T09:40:00Z",
             "tags": ["{'scanner_name': 'TISSUECYTE'}", "tissuecyte", null],
             "instances": [{"downloadUrl": "s3://public-development-802451596237-us-west-2/tissuecyte/1191617882/ome_zarr_conversion/1191617882.zarr"}]}
        ]},
        "brains": {"totalCount": 86, "nodes": [
            {"id": "4", "name": "SmartSPIM_789900_raw_neuroglancer_config", "type": "JSON",
             "status": "PUBLISHED", "createdAt": "2025-08-22T00:00:00Z", "tags": ["open-data"],
             "instances": [{"downloadUrl": "s3://aind-open-data/SmartSPIM_789900/neuroglancer_config.json"}]}
        ]},
        "tissuecyte": {"totalCount": 6308, "nodes": []},
        "brightfield": {"totalCount": 5869, "nodes": []},
        "epifluorescence": {"totalCount": 6173, "nodes": []},
        "status_PUBLISHED": {"totalCount": 38562},
        "status_PENDING_REVIEW": {"totalCount": 15767076},
        "status_ARCHIVED": {"totalCount": 2009740},
        "status_QC_PASSED": {"totalCount": 368},
        "status_QC_FAILED": {"totalCount": 46}
    }}"#;

    const BREAKDOWN: &str = r#"{"data":{
        "species0": {"totalCount": 297}, "species1": {"totalCount": 5204},
        "type0": {"totalCount": 6330}, "type1": {"totalCount": 0}
    }}"#;

    fn built() -> Dashboard {
        let overview: Overview = parse(OVERVIEW).unwrap();
        let breakdown: BTreeMap<String, Count> = parse(BREAKDOWN).unwrap();
        build(overview, &breakdown)
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
    fn a_store_registered_twice_is_offered_once() {
        let dashboard = built();
        let newest = datasets(&dashboard, "Newest images");
        assert_eq!(newest.len(), 2);
        assert_eq!(newest[0].kind, "Epifluorescence \u{00b7} 2026-09-24");
        assert_eq!(newest[1].kind, "TissueCyte \u{00b7} 2026-09-24");
    }

    #[test]
    fn subjects_are_counted_by_species_under_the_names_people_use() {
        let dashboard = built();
        let species = bars(&dashboard, "Subjects by species");
        assert_eq!(species[0].label, "Mouse (Mus musculus)");
        assert_eq!(species[0].value, 5204);
        assert_eq!(species[1].label, "Human (Homo sapiens)");
        let types = bars(&dashboard, "Specimens by type");
        assert_eq!(types.len(), 1, "a type holding nothing is left out");
        assert_eq!(types[0].label, "Brain specimen block");
    }

    #[test]
    fn nothing_to_offer_leaves_a_block_out_rather_than_empty() {
        let dashboard = built();
        assert!(dashboard.blocks.iter().all(|block| !block.is_empty()));
        assert!(
            !dashboard.blocks.iter().any(
                |block| matches!(block, Block::Datasets { title, .. } if title == "One of each modality")
            )
        );
        assert_eq!(
            datasets(&dashboard, "Light-sheet brains")[0].kind,
            "Neuroglancer \u{00b7} 2025-08-22"
        );
    }

    #[test]
    fn the_headline_counts_what_is_published() {
        let dashboard = built();
        let Block::Figures(figures) = &dashboard.blocks[0] else {
            panic!("figures lead");
        };
        assert_eq!(figures[0].value, 17_815_792);
        assert_eq!(figures[0].note.as_deref(), Some("38.6K published"));
        assert_eq!(figures[4].note.as_deref(), Some("Across 2 species"));
    }

    #[test]
    fn a_missing_token_points_to_the_settings() {
        let text = r#"{"errors":[{"message":"The current user is not authorized to access this resource.","extensions":{"code":"AUTH_NOT_AUTHENTICATED"}}],"data":{"assets":null}}"#;
        let error = parse::<Overview>(text).err().unwrap();
        assert!(error.contains("Settings"), "{error}");
    }

    #[test]
    #[ignore = "reads the live BKP Registry, with BKPR_TOKEN set"]
    fn a_live_dashboard_counts_and_offers_stores_that_open() {
        let token = std::env::var("BKPR_TOKEN").expect("BKPR_TOKEN");
        let started = std::time::Instant::now();
        let dashboard = crate::app::net::block_on(dashboard(STAGE, token.trim())).unwrap();
        println!("counted in {:?}", started.elapsed());
        for block in &dashboard.blocks {
            println!("{block:?}");
        }
        let newest = datasets(&dashboard, "Newest images");
        assert_eq!(newest.len(), NEWEST);
        crate::app::net::block_on(crate::formats::discover::discover(&newest[0].url))
            .expect("the newest image should open");
        let brains = datasets(&dashboard, "Light-sheet brains");
        crate::app::net::block_on(crate::formats::discover::discover(&brains[0].url))
            .expect("the newest brain should open");
    }
}
