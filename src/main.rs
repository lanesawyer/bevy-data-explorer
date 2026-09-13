//! A streaming explorer for large scientific datasets.
//!
//! Two formats are supported so far, shown side by side in independent panels:
//! OME-Zarr images, read through `zarrs`, and the Allen Institute's
//! Scatterbrain point clouds. Both are far too large to load whole, so each
//! panel streams only what its own view needs and pulls in more detail as you
//! zoom.

mod app;
mod cli;
mod formats;
mod render;
mod source;
mod ui;
mod view;
mod widgets;

use bevy::prelude::*;
use clap::Parser;

use app::ExplorerPlugin;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Args::parse();
    let data = args.open()?;

    let mut app = App::new();
    app.add_plugins(ExplorerPlugin);

    // Each format is a plugin. Registration order decides which cell a source's
    // frame opens in, and nothing else here knows what the formats are.
    app.add_plugins(formats::image::ImagePlugin {
        dataset: data.image,
        z_slice: args.z,
        budget_bytes: args.cache_mb * 1024 * 1024,
    });
    if let Some(cloud) = data.points {
        app.add_plugins(formats::pointcloud::PointCloudPlugin {
            name: "Point cloud".into(),
            cloud,
            budget: args.point_budget,
        });
    }
    if let Some(cloud) = data.cells {
        app.add_plugins(formats::pointcloud::PointCloudPlugin {
            name: "SEA-AD mapped cells".into(),
            cloud,
            budget: args.point_budget,
        });
    }
    if let Some(cloud) = data.sections {
        app.add_plugins(formats::slices::SlicesPlugin {
            cloud,
            budget: args.slice_budget,
        });
    }

    app.run();
    Ok(())
}
