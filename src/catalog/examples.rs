//! The datasets the app ships knowing the address of, as a catalog like any
//! other.

use futures::future::BoxFuture;

use super::{Catalog, Entry};

pub struct Examples;

impl Catalog for Examples {
    fn name(&self) -> &str {
        "Examples"
    }

    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
        let entries = EXAMPLES
            .iter()
            .map(|example| Entry {
                name: example.name.to_string(),
                kind: example.kind.to_string(),
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
/// Every one of them is offered in both places. They differ in more than kind —
/// a Zarr v2 store against a v3 one, a flat image against a stack of sections
/// — so picking one per kind to show would hide exactly the differences worth
/// opening them for. Scatterbrain files are not among them: every one worth
/// showing is a Brain Knowledge Platform visualization, and those are offered
/// from [`super::bkp::EXAMPLES`] where the platform describes their cells.
pub struct Example {
    pub name: &'static str,
    /// What kind of dataset it is, in the words shown beside it.
    pub kind: &'static str,
    pub url: &'static str,
}

pub const EXAMPLES: [Example; 5] = [
    Example {
        name: "Epifluorescence whole slide",
        kind: "OME-Zarr image",
        url: "https://h301-scanning-802451596237-us-west-2.s3.us-west-2.amazonaws.com/2402091625/ome_zarr_conversion/1458501514.zarr/",
    },
    Example {
        name: "Epifluorescence, Zarr v2",
        kind: "OME-Zarr image",
        url: "https://allen-genetic-tools.s3.us-west-2.amazonaws.com/epifluorescence/1401210938/ome_zarr_conversion/1401210938.zarr/",
    },
    Example {
        name: "Tissuecyte, 142 sections",
        kind: "OME-Zarr image stack",
        url: "zarr2://s3://allen-genetic-tools/tissuecyte/1219090168/ome_zarr_conversion/1219090168.zarr/",
    },
    Example {
        name: "SEA-AD pathology slide",
        kind: "Deep Zoom image",
        url: "https://idk-etl-prod-download-bucket.s3.amazonaws.com/idf-23-10-pathology-images/pat_images_JGCXWER774NLNWX2NNR/H20.33.040-A12-I6-primary/H20.33.040-A12-I6-primary.dzi",
    },
    Example {
        name: "SEA-AD pathology annotations",
        kind: "SVG annotations",
        url: "https://idk-etl-prod-download-bucket.s3.amazonaws.com/idf-23-10-pathology-images/pat_images_JGCXWER774NLNWX2NNR/H20.33.040-A12-I6-primary/annotation.svg",
    },
];
