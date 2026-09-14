//! One module per supported data format, each a Bevy plugin that registers
//! a source entity.
//!
//! A format's *systems* and its *datasets* are registered separately. The
//! systems go in once, whether or not the command line named a dataset of that
//! format, which is what lets a URL typed in later open into the same
//! machinery — including a format that was switched off at startup.

pub mod discover;
pub mod image;
pub mod pointcloud;
pub mod scatterbrain;
pub mod slices;

use bevy::prelude::*;

use discover::Discovered;

/// Every format's systems, registered whether or not a dataset of that format
/// is open yet.
pub struct FormatsPlugin;

impl Plugin for FormatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadSettings>().add_plugins((
            image::ImageSystems,
            pointcloud::PointCloudSystems,
            slices::SlicesSystems,
        ));
    }
}

/// A dataset the app knows the address of.
///
/// Listed here rather than in the command line or in the UI because it is the
/// formats that know what there is to open. Nothing loads on its own any more,
/// so this is what an empty window has to offer — and what the layout menu
/// offers once something is already open.
pub struct Example {
    pub name: &'static str,
    /// What kind of dataset it is, in the words shown beside it.
    pub kind: &'static str,
    pub url: &'static str,
    /// Whether the empty window offers it.
    ///
    /// That screen offers one of each kind, because its job is to show what the
    /// viewer can draw rather than everything it knows an address for. The
    /// layout menu offers all of them.
    pub featured: bool,
}

pub const EXAMPLES: [Example; 5] = [
    Example {
        name: "Epifluorescence whole slide",
        kind: "OME-Zarr image",
        url: "https://h301-scanning-802451596237-us-west-2.s3.us-west-2.amazonaws.com/2402091625/ome_zarr_conversion/1458501514.zarr/",
        featured: true,
    },
    Example {
        name: "Whole mouse brain cells",
        kind: "Scatterbrain point cloud",
        url: "https://d2o7sc91n904vd.cloudfront.net/wmb_tenx_01172024_stage-20240128193624/G4I4GFJXJB9ATZ3PTX1/ScatterBrain.json",
        featured: true,
    },
    Example {
        name: "Imputed genes, 53 sections",
        kind: "Scatterbrain sections",
        url: "https://d2o7sc91n904vd.cloudfront.net/bkppg-sfs-stage-wmb-imputed-genes-20240918212918/VFOFYPFQGRKUDQUZ3FF/ScatterBrain.json",
        featured: true,
    },
    Example {
        name: "SEA-AD mapped cells",
        kind: "Scatterbrain point cloud",
        url: "https://d2o7sc91n904vd.cloudfront.net/bkppg-sfs-stage-mjff-updates-03262025-20250403032833/839TIB6YQVFHZSGX401/ScatterBrain.json",
        featured: false,
    },
    Example {
        name: "Epifluorescence, Zarr v2",
        kind: "OME-Zarr image",
        url: "https://allen-genetic-tools.s3.us-west-2.amazonaws.com/epifluorescence/1401210938/ome_zarr_conversion/1401210938.zarr/",
        featured: false,
    },
];

/// The budgets a dataset is opened with, kept so that one opened at runtime is
/// given the same allowances as one named on the command line.
#[derive(Resource, Clone, Copy)]
pub struct LoadSettings {
    pub z_slice: u64,
    pub cache_bytes: usize,
    pub point_budget: usize,
    pub slice_budget: usize,
}

impl Default for LoadSettings {
    fn default() -> Self {
        LoadSettings {
            z_slice: 0,
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
        Discovered::Points { name, cloud } => pointcloud::spawn_source(
            world,
            name,
            std::sync::Arc::new(cloud),
            settings.point_budget,
        ),
        Discovered::Slices(cloud) => {
            slices::spawn_source(world, std::sync::Arc::new(cloud), settings.slice_budget)
        }
    }
}
