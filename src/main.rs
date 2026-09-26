//! A streaming explorer for large scientific datasets.
//!
//! Four formats are supported so far, shown side by side in independent
//! panels or stacked as layers in one: OME-Zarr images, read through `zarrs`,
//! Deep Zoom images, the Allen Institute's Scatterbrain point clouds, and SVG
//! annotations drawn over a slide. All but the annotations are far too large
//! to load whole, so each panel streams only what its own view needs and pulls
//! in more detail as you zoom.

mod app;
mod bookmark;
mod catalog;
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
use catalog::AppCatalogs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Args::parse();
    let opened = args.open()?;
    let bookmark = args.bookmark()?;
    let settings = args.load_settings();

    let mut app = App::new();
    app.add_plugins(ExplorerPlugin);

    // Every format's systems go in whether or not the command line named a
    // dataset for it, so a URL typed into the sidebar later opens into the
    // same machinery. The budgets go in beside them, for the same reason.
    app.add_plugins(formats::FormatsPlugin)
        .insert_resource(settings);

    // What the dataset dropdown offers beyond what is already open, in the
    // order listed here.
    app.add_plugins(catalog::CatalogPlugin)
        .add_catalog(catalog::bkp::Bkp::production())
        .add_catalog(catalog::bkp::projects::SpecimenTables::production())
        .add_catalog(catalog::examples::Examples)
        .add_catalog(catalog::registry::Registry::stage());

    // Each dataset named is registered exactly as one opened from the sidebar
    // is. Registration order decides which cell a source's frame opens in, and
    // nothing here knows what the formats are. Naming none of them is the
    // ordinary case: the window opens empty and offers the examples instead.
    let mut scene = None;
    for cli::Opened {
        url,
        dataset,
        as_layer,
    } in opened
    {
        // Another viewer's state is several datasets, opened as a bookmark
        // is; the first named is the one shown.
        if let formats::discover::Discovered::Scene(named) = &dataset {
            scene.get_or_insert_with(|| bookmark::scene::bookmark_of(named));
            continue;
        }
        let Some(source) = formats::spawn_discovered(app.world_mut(), dataset, settings) else {
            continue;
        };
        let mut source = app.world_mut().entity_mut(source);
        // What a dataset named here is recognized by, so the menus do not offer
        // it again as one to download.
        source.insert(source::SourceUrl(url));
        if as_layer {
            source.insert(view::OpensAsLayer);
        }
    }

    // Restored once the window is up, through the same reads the sidebar's
    // bookmarks take, so it replaces whatever the flags above opened.
    if let Some(bookmark) = bookmark.or(scene) {
        app.insert_resource(bookmark::restore::Restoring::new(bookmark));
    }

    app.run();
    Ok(())
}
