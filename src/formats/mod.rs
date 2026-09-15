//! One module per supported data format, each a Bevy plugin that registers
//! a source entity.
//!
//! A format's *systems* and its *datasets* are registered separately. The
//! systems go in once, whether or not the command line named a dataset of that
//! format, which is what lets a URL typed in later open into the same
//! machinery — including a format that was switched off at startup.

pub mod discover;
pub mod dzi;
pub mod image;
pub mod pointcloud;
pub mod scatterbrain;
pub mod slices;
pub mod svg;

use bevy::prelude::*;

use discover::Discovered;

/// Every format's systems, registered whether or not a dataset of that format
/// is open yet.
pub struct FormatsPlugin;

impl Plugin for FormatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadSettings>().add_plugins((
            image::ImageSystems,
            dzi::DziSystems,
            pointcloud::PointCloudSystems,
            slices::SlicesSystems,
            svg::SvgSystems,
        ));
    }
}

/// The URL a store is actually fetched from.
///
/// Addresses arrive as they were copied from wherever the user found them, and
/// the viewers around these datasets write them their own way. Neuroglancer
/// puts the format in front — `zarr2://`, `zarr://`, `n5://` — and names
/// buckets as `s3://bucket/key`, neither of which is something to fetch. The
/// format prefix is dropped because the bytes decide what a source is anyway,
/// and the bucket becomes the virtual-hosted URL it is served from.
///
/// The region is the one these datasets live in. A bucket elsewhere would
/// answer with a redirect naming its own, which is a better error than
/// refusing to try.
pub fn plain_url(source: &str) -> String {
    const REGION: &str = "us-west-2";

    let mut rest = source;
    for prefix in ["zarr2://", "zarr3://", "zarr://", "n5://", "precomputed://"] {
        if let Some(stripped) = rest.strip_prefix(prefix) {
            rest = stripped;
            break;
        }
    }

    let Some(bucket_and_key) = rest.strip_prefix("s3://") else {
        return rest.to_string();
    };
    match bucket_and_key.split_once('/') {
        Some((bucket, key)) => format!("https://{bucket}.s3.{REGION}.amazonaws.com/{key}"),
        None => format!("https://{bucket_and_key}.s3.{REGION}.amazonaws.com/"),
    }
}

/// A dataset the app knows the address of.
///
/// Listed here rather than in the command line or in the UI because it is the
/// formats that know what there is to open. Nothing loads on its own any more,
/// so this is what an empty window has to offer — and what the layout menu
/// offers once something is already open.
///
/// Every one of them is offered in both places. They differ in more than kind —
/// a Zarr v2 store against a v3 one, a flat image against a stack of sections,
/// a single cloud against a sectioned one — so picking one per kind to show
/// would hide exactly the differences worth opening them for.
pub struct Example {
    pub name: &'static str,
    /// What kind of dataset it is, in the words shown beside it.
    pub kind: &'static str,
    pub url: &'static str,
}

pub const EXAMPLES: [Example; 8] = [
    Example {
        name: "Epifluorescence whole slide",
        kind: "OME-Zarr image",
        url: "https://h301-scanning-802451596237-us-west-2.s3.us-west-2.amazonaws.com/2402091625/ome_zarr_conversion/1458501514.zarr/",
    },
    Example {
        name: "Whole mouse brain cells",
        kind: "Scatterbrain point cloud",
        url: "https://d2o7sc91n904vd.cloudfront.net/wmb_tenx_01172024_stage-20240128193624/G4I4GFJXJB9ATZ3PTX1/ScatterBrain.json",
    },
    Example {
        name: "Imputed genes, 53 sections",
        kind: "Scatterbrain sections",
        url: "https://d2o7sc91n904vd.cloudfront.net/bkppg-sfs-stage-wmb-imputed-genes-20240918212918/VFOFYPFQGRKUDQUZ3FF/ScatterBrain.json",
    },
    Example {
        name: "SEA-AD mapped cells",
        kind: "Scatterbrain point cloud",
        url: "https://d2o7sc91n904vd.cloudfront.net/bkppg-sfs-stage-mjff-updates-03262025-20250403032833/839TIB6YQVFHZSGX401/ScatterBrain.json",
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

/// The budgets a dataset is opened with, kept so that one opened at runtime is
/// given the same allowances as one named on the command line.
#[derive(Resource, Clone, Copy)]
pub struct LoadSettings {
    /// The slice a volumetric image opens on, when one was named. Left alone,
    /// a stack opens in its middle.
    pub z_slice: Option<u64>,
    pub cache_bytes: usize,
    pub point_budget: usize,
    pub slice_budget: usize,
}

impl Default for LoadSettings {
    fn default() -> Self {
        LoadSettings {
            z_slice: None,
            cache_bytes: image::DEFAULT_CACHE_BUDGET_MB * 1024 * 1024,
            point_budget: pointcloud::DEFAULT_POINT_BUDGET,
            slice_budget: slices::DEFAULT_SLICE_BUDGET,
        }
    }
}

/// Register a recognised dataset as a source, whichever format it turned out
/// to be.
///
/// The one place that maps a [`Discovered`] onto a format, so nothing above
/// `formats` has to name one.
pub fn spawn_discovered(
    world: &mut World,
    discovered: Discovered,
    settings: LoadSettings,
) -> Entity {
    match discovered {
        Discovered::Image(dataset) => image::spawn_source(
            world,
            std::sync::Arc::new(*dataset),
            settings.z_slice,
            settings.cache_bytes,
        ),
        Discovered::DeepZoom(dzi) => {
            dzi::spawn_source(world, std::sync::Arc::new(dzi), settings.cache_bytes)
        }
        Discovered::Points { name, cloud } => pointcloud::spawn_source(
            world,
            name,
            std::sync::Arc::new(cloud),
            settings.point_budget,
        ),
        Discovered::Slices(cloud) => {
            slices::spawn_source(world, std::sync::Arc::new(cloud), settings.slice_budget)
        }
        Discovered::Annotations(svg) => svg::spawn_source(world, std::sync::Arc::new(svg)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_neuroglancer_address_becomes_one_that_can_be_fetched() {
        // Copied straight out of a neuroglancer config, which is where these
        // datasets are usually found.
        assert_eq!(
            plain_url(
                "zarr2://s3://allen-genetic-tools/tissuecyte/1219090168/ome_zarr_conversion/1219090168.zarr/"
            ),
            "https://allen-genetic-tools.s3.us-west-2.amazonaws.com/tissuecyte/1219090168/ome_zarr_conversion/1219090168.zarr/"
        );
    }

    #[test]
    fn the_format_in_front_is_dropped_whatever_it_claims() {
        // What a source is gets decided by reading it, so a prefix saying what
        // it is cannot be worth refusing over — or believing.
        for prefix in ["zarr://", "zarr2://", "zarr3://", "n5://", "precomputed://"] {
            let url = plain_url(&format!("{prefix}https://example.com/a.zarr/"));
            assert_eq!(url, "https://example.com/a.zarr/");
        }
    }

    #[test]
    fn a_bucket_on_its_own_still_names_a_host() {
        assert_eq!(
            plain_url("s3://allen-genetic-tools"),
            "https://allen-genetic-tools.s3.us-west-2.amazonaws.com/"
        );
    }

    #[test]
    fn an_ordinary_url_is_left_exactly_as_it_was() {
        for url in [
            "https://example.com/a.zarr/",
            "http://example.com/ScatterBrain.json",
            "/data/local.zarr",
            "metadata.json",
        ] {
            assert_eq!(plain_url(url), url);
        }
    }
}
