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
