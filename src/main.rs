//! A streaming explorer for large scientific datasets.
//!
//! Two formats are supported so far, shown side by side in independent panels:
//! OME-Zarr images, read through `zarrs`, and the Allen Institute's
//! Scatterbrain point clouds. Both are far too large to load whole, so each
//! panel streams only what its own view needs and pulls in more detail as you
//! zoom.

mod app;
mod formats;
mod render;
mod source;
mod ui;
mod view;

use std::sync::Arc;

use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::prelude::*;
use bevy::window::PresentMode;
use clap::Parser;

/// Scatterbrain metadata for the reference point cloud.
const DEFAULT_POINTS: &str = "https://d2o7sc91n904vd.cloudfront.net/wmb_tenx_01172024_stage-20240128193624/G4I4GFJXJB9ATZ3PTX1/ScatterBrain.json";

/// Scatterbrain metadata for the SEA-AD mapped dataset, which carries numeric
/// properties alongside categorical ones.
const DEFAULT_CELLS: &str = "https://d2o7sc91n904vd.cloudfront.net/bkppg-sfs-stage-mjff-updates-03262025-20250403032833/839TIB6YQVFHZSGX401/ScatterBrain.json";

/// Scatterbrain metadata for the reference sectioned dataset.
const DEFAULT_SLICES: &str = "https://d2o7sc91n904vd.cloudfront.net/bkppg-sfs-stage-wmb-imputed-genes-20240918212918/VFOFYPFQGRKUDQUZ3FF/ScatterBrain.json";

#[derive(Parser, Debug)]
#[command(
    name = "bevy-data-explorer",
    about = "Stream and explore large scientific datasets"
)]
struct Args {
    /// OME-Zarr store (http(s) URL or local directory), or a manifest .json
    /// describing one. Defaults to the reference image.
    #[arg(default_value = formats::image::store::DEFAULT_SOURCE)]
    source: String,

    /// Scatterbrain metadata JSON (http(s) URL or local file). Pass `none` to
    /// show the image on its own.
    #[arg(long, default_value = DEFAULT_POINTS)]
    points: String,

    /// Z slice to display for volumetric images.
    #[arg(long, default_value_t = 0)]
    z: u64,

    /// Texture memory budget for cached tiles, in MB. Larger values make
    /// zooming back out and revisiting areas redraw without refetching.
    #[arg(long, default_value_t = formats::image::DEFAULT_CACHE_BUDGET_MB)]
    cache_mb: usize,

    /// Sectioned Scatterbrain metadata JSON, shown as a third panel. Pass
    /// `none` to leave it out.
    #[arg(long, default_value = DEFAULT_SLICES)]
    slices: String,

    /// A second Scatterbrain point cloud, shown as a fourth panel. Pass `none`
    /// to leave it out.
    #[arg(long, default_value = DEFAULT_CELLS)]
    cells: String,

    /// Maximum points held on the GPU for the point cloud.
    #[arg(long, default_value_t = formats::pointcloud::DEFAULT_POINT_BUDGET)]
    point_budget: usize,

    /// Maximum points held on the GPU for the sectioned panel.
    #[arg(long, default_value_t = formats::slices::DEFAULT_SLICE_BUDGET)]
    slice_budget: usize,
}

/// Give tile fetching a pool large enough to keep many requests in flight.
///
/// Bevy's default async-compute pool tops out at four threads, which caps
/// loading at roughly three concurrent requests. These threads are blocked on
/// sockets rather than using CPU, so they are added on top of the core count
/// instead of being carved out of it — carving them out would starve the
/// compute pool that runs the ECS schedule.
fn task_pool_options() -> TaskPoolOptions {
    TaskPoolOptions {
        min_total_threads: bevy::tasks::available_parallelism()
            + formats::image::TILE_FETCH_THREADS,
        async_compute: TaskPoolThreadAssignmentPolicy {
            min_threads: formats::image::TILE_FETCH_THREADS,
            max_threads: formats::image::TILE_FETCH_THREADS,
            percent: 1.0,
            on_thread_spawn: None,
            on_thread_destroy: None,
        },
        ..default()
    }
}

fn load_points(source: &str) -> Result<formats::scatterbrain::Scatterbrain, String> {
    let text = if source.starts_with("http://") || source.starts_with("https://") {
        reqwest::blocking::get(source)
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
            .map_err(|e| format!("fetching {source}: {e}"))?
    } else {
        std::fs::read_to_string(source).map_err(|e| format!("reading {source}: {e}"))?
    };
    formats::scatterbrain::Scatterbrain::parse(&text)
}

/// Feathers' dark theme, with the button states pushed further apart.
///
/// Its hover is a five percent lift in lightness, which is hard to see at all
/// over a frame's imagery and reads as a button that does not respond. The
/// tokens are widened rather than each button being styled by hand, so every
/// control in the app moves together.
fn app_theme() -> bevy_feathers::theme::ThemeProps {
    use bevy_feathers::{dark_theme::create_dark_theme, palette, tokens};

    let mut theme = create_dark_theme();
    theme
        .color
        .insert(tokens::BUTTON_BG_HOVER, palette::GRAY_3.lighter(0.14));
    theme
        .color
        .insert(tokens::BUTTON_BG_PRESSED, palette::ACCENT.darker(0.12));
    theme.color.insert(
        tokens::BUTTON_PRIMARY_BG_HOVER,
        palette::ACCENT.lighter(0.08),
    );
    theme
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Open both sources before opening a window, so a bad URL fails on the
    // command line rather than behind a blank panel.
    println!("opening image  {}", args.source);
    let dataset = Arc::new(formats::image::store::open(&args.source)?);
    println!(
        "  {}: {} levels, {} channels, {} x {} px",
        dataset.name,
        dataset.levels.len(),
        dataset.channels.len(),
        dataset.levels[0].width,
        dataset.levels[0].height
    );

    let describe = |label: &str, cloud: &formats::scatterbrain::Scatterbrain| {
        println!(
            "  {} points across {} slide(s), {} octree nodes, depth {} [{label}]",
            cloud.total_points(),
            cloud.slides.len(),
            cloud.node_count(),
            cloud.max_depth(),
        );
        if let Some(first) = cloud.slides.first() {
            // The root is a subsample; children add the rest. Showing both
            // makes the additive structure visible at a glance.
            println!(
                "  slide {} root holds {} of its {} points",
                first.index,
                first.root().count,
                first.total_points,
            );
        }
    };

    let cloud = if args.points.eq_ignore_ascii_case("none") {
        None
    } else {
        println!("opening points {}", args.points);
        let cloud = Arc::new(load_points(&args.points)?);
        describe("points", &cloud);
        Some(cloud)
    };

    let cells = if args.cells.eq_ignore_ascii_case("none") {
        None
    } else {
        println!("opening cells  {}", args.cells);
        let cloud = Arc::new(load_points(&args.cells)?);
        describe("cells", &cloud);
        Some(cloud)
    };

    let sections = if args.slices.eq_ignore_ascii_case("none") {
        None
    } else {
        println!("opening slices {}", args.slices);
        let cloud = Arc::new(load_points(&args.slices)?);
        describe("slices", &cloud);
        Some(cloud)
    };

    let mut app = App::new();
    app::schedule::configure(&mut app);
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Data Explorer".to_string(),
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                ..default()
            })
            .set(ImagePlugin::default_nearest())
            .set(TaskPoolPlugin {
                task_pool_options: task_pool_options(),
            }),
    )
    .add_plugins(render::points::PointRenderPlugin)
    .add_plugins(bevy_feathers::FeathersPlugins)
    .insert_resource(bevy_feathers::theme::UiTheme(app_theme()))
    .add_plugins(view::ViewPlugin)
    .add_plugins(ui::UiPlugin)
    .add_systems(Startup, maximize_window.in_set(app::schedule::Boot::Window));

    // Each format is a plugin. Registration order decides which cell a source's
    // frame opens in, and nothing else here knows what the formats are.
    app.add_plugins(formats::image::ImagePlugin {
        dataset,
        z_slice: args.z,
        budget_bytes: args.cache_mb * 1024 * 1024,
    });
    if let Some(cloud) = cloud {
        app.add_plugins(formats::pointcloud::PointCloudPlugin {
            name: "Point cloud".into(),
            cloud,
            budget: args.point_budget,
        });
    }
    if let Some(cloud) = cells {
        app.add_plugins(formats::pointcloud::PointCloudPlugin {
            name: "SEA-AD mapped cells".into(),
            cloud,
            budget: args.point_budget,
        });
    }
    if let Some(cloud) = sections {
        app.add_plugins(formats::slices::SlicesPlugin {
            cloud,
            budget: args.slice_budget,
        });
    }

    app.run();
    Ok(())
}

/// Start maximized.
///
/// Several frames side by side plus a sidebar need the room, and maximizing
/// takes whatever the display actually offers rather than guessing a size that
/// might not fit.
fn maximize_window(mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>) {
    for mut window in &mut windows {
        window.set_maximized(true);
    }
}
