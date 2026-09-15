//! A streaming explorer for large scientific datasets.
//!
//! Three formats are supported so far, shown side by side in independent
//! panels: OME-Zarr images, read through `zarrs`, Deep Zoom images, and the
//! Allen Institute's Scatterbrain point clouds. All are far too large to load
//! whole, so each panel streams only what its own view needs and pulls in more
//! detail as you zoom.

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
    let layers = cli::open_layers(&args.layer)?;

    let mut app = App::new();
    app.add_plugins(ExplorerPlugin);

    // Every format's systems go in whether or not the command line named a
    // dataset for it, so a URL typed into the sidebar later opens into the
    // same machinery. The budgets go in beside them, for the same reason.
    app.add_plugins(formats::FormatsPlugin)
        .insert_resource(args.load_settings());

    // Each dataset named on the command line is a plugin. Registration order
    // decides which cell a source's frame opens in, and nothing else here knows
    // what the formats are. Naming none of them is the ordinary case: the
    // window opens empty and offers the examples instead.
    if let Some(dataset) = data.image {
        app.add_plugins(formats::image::ImagePlugin {
            dataset,
            z_slice: args.z,
            budget_bytes: args.cache_mb * 1024 * 1024,
        });
        record_url(&mut app, args.source.as_deref());
    }
    if let Some(dzi) = data.deep_zoom {
        app.add_plugins(formats::dzi::DziPlugin {
            dzi,
            budget_bytes: args.cache_mb * 1024 * 1024,
        });
        record_url(&mut app, args.source.as_deref());
    }
    if let Some(cloud) = data.points {
        app.add_plugins(formats::pointcloud::PointCloudPlugin {
            name: "Point cloud".into(),
            cloud,
            budget: args.point_budget,
        });
        record_url(&mut app, args.points.as_deref());
    }
    if let Some(cloud) = data.cells {
        app.add_plugins(formats::pointcloud::PointCloudPlugin {
            name: "SEA-AD mapped cells".into(),
            cloud,
            budget: args.point_budget,
        });
        record_url(&mut app, args.cells.as_deref());
    }
    if let Some(cloud) = data.sections {
        app.add_plugins(formats::slices::SlicesPlugin {
            cloud,
            budget: args.slice_budget,
        });
        record_url(&mut app, args.slices.as_deref());
    }

    // After every frame's dataset, so a layer is never mistaken for the frame
    // it is meant to be drawn over.
    for (url, layer) in args.layer.iter().zip(layers) {
        let source = formats::spawn_discovered(app.world_mut(), layer, args.load_settings());
        app.world_mut().entity_mut(source).insert((
            view::OpensAsLayer,
            source::SourceUrl(url.trim().to_string()),
        ));
    }

    app.run();
    Ok(())
}

/// Record the address the source a plugin just registered was read from, so a
/// dataset named here is not offered again in the menus as one to download.
///
/// Plugins register as they are added, and every one hands out the next render
/// layer, so the newest source is the one with the highest.
fn record_url(app: &mut App, url: Option<&str>) {
    let Some(url) = url else { return };
    let world = app.world_mut();
    let newest = world
        .query::<(Entity, &source::DataSource)>()
        .iter(world)
        .max_by_key(|(_, source)| source.layer)
        .map(|(entity, _)| entity);
    if let Some(entity) = newest {
        world
            .entity_mut(entity)
            .insert(source::SourceUrl(url.trim().to_string()));
    }
}
