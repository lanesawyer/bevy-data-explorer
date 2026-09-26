//! One module per supported data format, each a Bevy plugin that registers
//! a source entity.
//!
//! A format's *systems* and its *datasets* are registered separately. The
//! systems go in once, whether or not the command line named a dataset of that
//! format, which is what lets a URL typed in later open into the same
//! machinery — including a format that was switched off at startup.

pub mod csv;
pub mod discover;
pub mod dzi;
pub mod image;
pub mod neuroglancer;
pub mod parquet;
pub mod pointcloud;
pub mod scatterbrain;
pub mod slices;
pub mod specimens;
pub mod svg;
pub mod table;
pub mod table_filters;
pub mod tiles;

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
            table::TableSystems,
            specimens::SpecimenSystems,
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
///
/// A `file://` address is a path on this machine written as a URL, as a
/// browser's address bar shows a file opened in it, and becomes that path,
/// its escapes such as `%20` undone.
pub fn plain_url(source: &str) -> String {
    const REGION: &str = "us-west-2";

    let mut rest = source;
    for prefix in ["zarr2://", "zarr3://", "zarr://", "n5://", "precomputed://"] {
        if let Some(stripped) = rest.strip_prefix(prefix) {
            rest = stripped;
            break;
        }
    }

    if rest.starts_with("file://") {
        return url::Url::parse(rest)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .map_or_else(|| rest.to_string(), |path| path.display().to_string());
    }
    let Some(bucket_and_key) = rest.strip_prefix("s3://") else {
        return rest.to_string();
    };
    match bucket_and_key.split_once('/') {
        Some((bucket, key)) => format!("https://{bucket}.s3.{REGION}.amazonaws.com/{key}"),
        None => format!("https://{bucket_and_key}.s3.{REGION}.amazonaws.com/"),
    }
}

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

/// Register a recognized dataset as a source, whichever format it turned out
/// to be.
///
/// The one place that maps a [`Discovered`] onto a format, so nothing above
/// `formats` has to name one.
pub fn spawn_discovered(
    world: &mut World,
    discovered: Discovered,
    settings: LoadSettings,
) -> Option<Entity> {
    Some(match discovered {
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
        Discovered::Table(table) => table::spawn_source(world, *table),
        Discovered::Specimens(specimens) => specimens::spawn_source(world, *specimens),
        // Several datasets rather than one, which whoever asked opens as a
        // bookmark; there is no one source to register.
        Discovered::Scene(_) => return None,
    })
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
    fn a_file_address_is_the_path_it_names() {
        assert_eq!(plain_url("file:///data/cells.csv"), "/data/cells.csv");
        assert_eq!(
            plain_url("file:///data/my%20cells.csv"),
            "/data/my cells.csv"
        );
        assert_eq!(
            plain_url("zarr://file:///data/stack.zarr"),
            "/data/stack.zarr"
        );
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
