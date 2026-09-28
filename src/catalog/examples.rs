//! The datasets the app ships knowing the address of, as a catalog like any
//! other.

use futures::future::BoxFuture;

use super::{Catalog, Entry};
use crate::source::Category;

pub struct Examples;

impl Catalog for Examples {
    fn name(&self) -> &str {
        "Examples"
    }

    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
        let entries = EXAMPLES
            .iter()
            // Offered once, by the catalog that describes its cells.
            .filter(|example| !example.cataloged)
            .map(|example| Entry {
                name: example.name.to_string(),
                kind: example.kind.to_string(),
                category: example.category,
                url: example.url.to_string(),
                keywords: String::new(),
                // The examples are read for what their files hold, whoever
                // else may know them.
                cells: None,
            })
            .collect();
        Box::pin(async move { Ok(entries) })
    }
}

/// A dataset the app knows the address of.
///
/// Nothing loads on its own any more, so this is what an empty window has to
/// offer, as buttons on the welcome screen — and, as a catalog, what every
/// dataset picker offers once something is already open.
///
/// There is one of each kind of thing the viewer reads, so the list doubles
/// as a showing of what can be pasted into it: a new format adds its example
/// here. The BKP Registry's records are the exception, since none of them
/// opens without signing in to a pre-production service.
pub struct Example {
    pub name: &'static str,
    /// What kind of dataset it is, in the words shown beside it.
    pub kind: &'static str,
    pub category: Category,
    pub url: &'static str,
    /// Listed by a catalog of its own, which names it and describes its
    /// cells, so the examples catalog leaves offering it to that one.
    pub cataloged: bool,
}

pub const EXAMPLES: [Example; 9] = [
    Example {
        name: "Epifluorescence whole slide",
        kind: "OME-Zarr image",
        category: Category::Image,
        url: "https://h301-scanning-802451596237-us-west-2.s3.us-west-2.amazonaws.com/2402091625/ome_zarr_conversion/1458501514.zarr/",
        cataloged: false,
    },
    Example {
        // Three stores on one grid, opened as one image of their channels
        // where the state looks.
        name: "SmartSPIM, 3 channels",
        kind: "Neuroglancer state",
        category: Category::Image,
        url: "s3://aind-open-data/SmartSPIM_719692_2024-03-13_16-03-36_stitched_2024-04-02_12-50-17/neuroglancer_config.json",
        cataloged: false,
    },
    Example {
        name: "SEA-AD pathology slide",
        kind: "Deep Zoom image",
        category: Category::Image,
        url: "https://idk-etl-prod-download-bucket.s3.amazonaws.com/idf-23-10-pathology-images/pat_images_JGCXWER774NLNWX2NNR/H20.33.040-A12-I6-primary/H20.33.040-A12-I6-primary.dzi",
        cataloged: false,
    },
    Example {
        name: "SEA-AD pathology annotations",
        kind: "SVG annotations",
        category: Category::Annotations,
        url: "https://idk-etl-prod-download-bucket.s3.amazonaws.com/idf-23-10-pathology-images/pat_images_JGCXWER774NLNWX2NNR/H20.33.040-A12-I6-primary/annotation.svg",
        cataloged: false,
    },
    Example {
        name: "Whole mouse brain, 10x scRNA-seq",
        kind: "Scatterbrain UMAP",
        category: Category::Cells,
        url: "https://bkp-2d-visualizations.s3.amazonaws.com/wmb_tenx_02082024-20240220165404/G4I4GFJXJB9ATZ3PTX1/ScatterBrain.json",
        cataloged: true,
    },
    Example {
        name: "MERFISH with imputed genes",
        kind: "Scatterbrain sections",
        category: Category::Cells,
        url: "https://bkp-2d-visualizations.s3.amazonaws.com/bkppg-sfs-prod-wmb-imputed-genes-20240926234907/6MT7UC6ETYECBWF50PK/ScatterBrain.json",
        cataloged: true,
    },
    Example {
        // Short and narrow, so the whole of it is on screen at once rather
        // than behind a scroll, which is what an example of a new kind is for.
        name: "Brain regions of interest",
        kind: "CSV table",
        category: Category::Table,
        url: "https://allen-brain-cell-atlas.s3.us-west-2.amazonaws.com/metadata/WMB-10X/20230830/region_of_interest_metadata.csv",
        cataloged: false,
    },
    Example {
        name: "Allen adult mouse terminology",
        kind: "Parquet table",
        category: Category::Table,
        url: "s3://allen-atlas-assets/terminologies/allen-adult-mouse-terminology/2026-03/terminology.parquet",
        cataloged: false,
    },
    Example {
        // Not a file: the platform's API, asked for one project's specimens.
        // 84 donors of 30 features apiece, which is a table worth scrolling
        // rather than one that fits on screen.
        name: "SEA-AD donors and neuropathology",
        kind: "BKP specimen table",
        category: Category::Table,
        url: "https://idf-api-prod.aibs-idk-prod.net/?specimens=JGN327NUXRZSHEV88TN",
        cataloged: true,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_one_example_of_each_kind() {
        let mut kinds: Vec<&str> = EXAMPLES.iter().map(|example| example.kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), EXAMPLES.len(), "two examples share a kind");
    }

    #[test]
    fn every_example_names_an_address_that_can_be_fetched() {
        // Written the way they were copied — one of them straight out of a
        // neuroglancer config — so what matters is that each one comes out of
        // the translation as something fetchable.
        for example in &EXAMPLES {
            let url = crate::formats::plain_url(example.url);
            assert!(url.starts_with("https://"), "{}: {url}", example.name);
            assert!(!example.name.is_empty());
        }
    }

    #[test]
    #[ignore = "reads every example from the live store"]
    fn every_example_is_recognized() {
        for example in &EXAMPLES {
            let found = crate::app::net::block_on(crate::formats::discover::discover(example.url));
            assert!(found.is_ok(), "{}: {}", example.name, found.err().unwrap());
        }
    }
}
