//! A streaming explorer for large scientific datasets.
//!
//! Two formats are supported so far, shown side by side in independent panels:
//! OME-Zarr images, read through `zarrs`, and the Allen Institute's
//! Scatterbrain point clouds. Both are far too large to load whole, so each
//! panel streams only what its own view needs and pulls in more detail as you
//! zoom.

mod dataset;
mod hud;
mod panel;
mod pointcloud;
mod scatterbrain;
mod source;
mod tiles;

use std::sync::Arc;

use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::prelude::*;
use bevy::window::PresentMode;
use clap::Parser;

use panel::{PanelKind, ViewLimits};

/// Scatterbrain metadata for the reference point cloud.
const DEFAULT_POINTS: &str = "https://d2o7sc91n904vd.cloudfront.net/wmb_tenx_01172024_stage-20240128193624/G4I4GFJXJB9ATZ3PTX1/ScatterBrain.json";

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

    /// Maximum points held on the GPU for the point cloud.
    #[arg(long, default_value_t = pointcloud::DEFAULT_POINT_BUDGET)]
    point_budget: usize,
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

    let cloud = if args.points.eq_ignore_ascii_case("none") {
        None
    } else {
        println!("opening points {}", args.points);
        let cloud = Arc::new(load_points(&args.points)?);
        let b = cloud.tight_bounds;
        println!(
            "  {} points across {} octree nodes, depth {}, root holds {}",
            cloud.total_points,
            cloud.nodes.len(),
            cloud.nodes.iter().map(|n| n.depth).max().unwrap_or(0),
            cloud.root().count,
        );
        println!(
            "  extent x [{:.3}, {:.3}]  y [{:.3}, {:.3}]  (octree cube {:.3} wide)",
            b.min_x,
            b.max_x,
            b.min_y,
            b.max_y,
            cloud.bounds.width(),
        );
        Some(cloud)
    };

    let mut streamer = tiles::TileStreamer::new(dataset.clone());
    streamer.z_slice = args.z;
    streamer.budget_bytes = args.cache_mb * 1024 * 1024;

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
    .insert_resource(streamer)
    .add_systems(
        Update,
        (
            panel::panel_controls,
            panel::update_viewports,
            tiles::select_tiles,
            tiles::spawn_tile_tasks,
            tiles::collect_tile_tasks,
            tiles::evict_tiles,
            tiles::update_tile_visibility,
            toggle_channels,
            hud::update_hud,
        )
            .chain(),
    );

    if let Some(cloud) = cloud.clone() {
        let mut points = pointcloud::PointStreamer::new(cloud);
        points.budget = args.point_budget;
        app.insert_resource(points).add_systems(
            Update,
            (
                pointcloud::select_nodes,
                pointcloud::spawn_node_tasks,
                pointcloud::collect_node_tasks,
                pointcloud::evict_nodes,
            )
                .chain()
                .after(panel::update_viewports),
        );
    }

    let image_world = dataset.world;
    let finest = dataset.levels[0].scale_x as f32;
    let cloud_for_setup = cloud.clone();

    app.add_systems(
        Startup,
        move |mut commands: Commands, windows: Query<&Window>| {
            let window = windows
                .iter()
                .next()
                .map(|w| Vec2::new(w.width(), w.height()))
                .unwrap_or(Vec2::new(1280.0, 720.0));
            let columns = if cloud_for_setup.is_some() { 2 } else { 1 };
            let viewport = Vec2::new(window.x / columns as f32, window.y);

            let (x0, y0, x1, y1) = image_world;
            panel::spawn_panel(
                &mut commands,
                PanelKind::Image,
                0,
                columns,
                // World y is negated so the image reads top-down.
                ViewLimits::fit(
                    Vec2::new((x0 + x1) * 0.5, -(y0 + y1) * 0.5),
                    (x1 - x0).abs(),
                    (y1 - y0).abs(),
                    viewport,
                    finest / 8.0,
                ),
            );

            let mut panels = vec![(PanelKind::Image, 0)];
            if let Some(cloud) = &cloud_for_setup {
                let b = cloud.tight_bounds;
                let (cx, cy) = b.centre();
                panel::spawn_panel(
                    &mut commands,
                    PanelKind::Points,
                    1,
                    columns,
                    ViewLimits::fit(
                        Vec2::new(cx, -cy),
                        b.width(),
                        b.height(),
                        viewport,
                        b.width() / 100_000.0,
                    ),
                );
                panels.push((PanelKind::Points, 1));
            }

            panel::spawn_ui_camera(&mut commands, columns);
            panel::spawn_dividers(&mut commands, columns);
            hud::spawn_hud(&mut commands, &panels, columns);
        },
    );

    app.run();
    Ok(())
}

/// Number keys toggle image channels. Tiles bake the composite into RGBA, so
/// the visible ones are rebuilt; the shard decoders survive, which keeps the
/// refetch cheap.
fn toggle_channels(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut streamer: ResMut<tiles::TileStreamer>,
) {
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];

    let Some(index) = DIGITS
        .iter()
        .position(|key| keys.just_pressed(*key))
        .filter(|i| *i < streamer.channels.len())
    else {
        return;
    };

    streamer.channels[index].active = !streamer.channels[index].active;
    streamer.reset(&mut commands);
}
