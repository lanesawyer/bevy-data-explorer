//! A streaming explorer for large scientific datasets.
//!
//! Two formats are supported so far, shown side by side in independent panels:
//! OME-Zarr images, read through `zarrs`, and the Allen Institute's
//! Scatterbrain point clouds. Both are far too large to load whole, so each
//! panel streams only what its own view needs and pulls in more detail as you
//! zoom.

mod dataset;
mod datasource;
mod hud;
mod panel;
mod pointcloud;
mod scatterbrain;
mod sidebar;
mod slices;
mod source;
mod tiles;

use std::sync::Arc;

use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::prelude::*;
use bevy::window::PresentMode;
use clap::Parser;

use datasource::{DataSource, SourceExtent};

/// Scatterbrain metadata for the reference point cloud.
const DEFAULT_POINTS: &str = "https://d2o7sc91n904vd.cloudfront.net/wmb_tenx_01172024_stage-20240128193624/G4I4GFJXJB9ATZ3PTX1/ScatterBrain.json";

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
    #[arg(default_value = source::DEFAULT_SOURCE)]
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
    #[arg(long, default_value_t = tiles::DEFAULT_CACHE_BUDGET_MB)]
    cache_mb: usize,

    /// Sectioned Scatterbrain metadata JSON, shown as a third panel. Pass
    /// `none` to leave it out.
    #[arg(long, default_value = DEFAULT_SLICES)]
    slices: String,

    /// Maximum points held on the GPU for the point cloud.
    #[arg(long, default_value_t = pointcloud::DEFAULT_POINT_BUDGET)]
    point_budget: usize,

    /// Maximum points held on the GPU for the sectioned panel.
    #[arg(long, default_value_t = slices::DEFAULT_SLICE_BUDGET)]
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
        min_total_threads: bevy::tasks::available_parallelism() + tiles::TILE_FETCH_THREADS,
        async_compute: TaskPoolThreadAssignmentPolicy {
            min_threads: tiles::TILE_FETCH_THREADS,
            max_threads: tiles::TILE_FETCH_THREADS,
            percent: 1.0,
            on_thread_spawn: None,
            on_thread_destroy: None,
        },
        ..default()
    }
}

fn load_points(source: &str) -> Result<scatterbrain::Scatterbrain, String> {
    let text = if source.starts_with("http://") || source.starts_with("https://") {
        reqwest::blocking::get(source)
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
            .map_err(|e| format!("fetching {source}: {e}"))?
    } else {
        std::fs::read_to_string(source).map_err(|e| format!("reading {source}: {e}"))?
    };
    scatterbrain::Scatterbrain::parse(&text)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Open both sources before opening a window, so a bad URL fails on the
    // command line rather than behind a blank panel.
    println!("opening image  {}", args.source);
    let dataset = Arc::new(source::open(&args.source)?);
    println!(
        "  {}: {} levels, {} channels, {} x {} px",
        dataset.name,
        dataset.levels.len(),
        dataset.channels.len(),
        dataset.levels[0].width,
        dataset.levels[0].height
    );

    let describe = |label: &str, cloud: &scatterbrain::Scatterbrain| {
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

    let sections = if args.slices.eq_ignore_ascii_case("none") {
        None
    } else {
        println!("opening slices {}", args.slices);
        let cloud = Arc::new(load_points(&args.slices)?);
        describe("slices", &cloud);
        Some(cloud)
    };

    let mut app = App::new();
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
    .init_resource::<panel::FrameArea>()
    .init_resource::<sidebar::Sidebar>()
    .add_systems(
        Update,
        (
            sidebar::toggle_sidebar,
            sidebar::resize_sidebar,
            sidebar::update_sidebar,
            panel::reset_frame_area,
            sidebar::reserve_space,
            panel::duplicate_panel,
            panel::close_panel,
            panel::sync_panel_buttons,
            panel::highlight_panel_buttons,
            hud::sync_hud,
            panel::panel_controls,
            panel::update_viewports,
            hud::position_hud,
        )
            .chain(),
    )
    // The overlay reads whatever each source reported this frame, so it runs
    // after every source plugin has had its turn.
    .add_systems(Update, hud::update_hud.after(panel::update_viewports));

    // Each format is a plugin. Registration order decides which cell a source's
    // frame opens in, and nothing else here knows what the formats are.
    app.add_plugins(tiles::ImagePlugin {
        dataset,
        z_slice: args.z,
        budget_bytes: args.cache_mb * 1024 * 1024,
    });
    if let Some(cloud) = cloud {
        app.add_plugins(pointcloud::PointCloudPlugin {
            cloud,
            budget: args.point_budget,
        });
    }
    if let Some(cloud) = sections {
        app.add_plugins(slices::SlicesPlugin {
            cloud,
            budget: args.slice_budget,
        });
    }

    app.add_systems(Startup, open_frames);

    app.run();
    Ok(())
}

/// Open one frame per registered source, in registration order.
///
/// Sources are discovered from the world rather than listed here, so adding a
/// format plugin is enough to get it a frame.
fn open_frames(
    mut commands: Commands,
    windows: Query<&Window>,
    sources: Query<(Entity, &DataSource, &SourceExtent)>,
) {
    let window = windows
        .iter()
        .next()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));

    let mut sources: Vec<(Entity, &DataSource, &SourceExtent)> = sources.iter().collect();
    // Layers are handed out in registration order, which is the order the
    // plugins were added.
    sources.sort_by_key(|(_, source, _)| source.layer);

    let (columns, rows) = panel::grid_for(sources.len());
    let viewport = Vec2::new(window.x / columns as f32, window.y / rows as f32);

    for (index, (entity, source, extent)) in sources.into_iter().enumerate() {
        panel::spawn_panel(
            &mut commands,
            entity,
            source.layer,
            index,
            extent.limits(viewport),
            None,
        );
    }

    panel::spawn_ui_camera(&mut commands);
    panel::spawn_dividers(&mut commands);
    sidebar::spawn_sidebar(&mut commands);
}
