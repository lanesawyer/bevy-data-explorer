//! A streaming OME-Zarr viewer.
//!
//! Reading is done by `zarrs`, which handles Zarr v3 metadata, the codec
//! pipeline and — importantly for images this size — partial reads of sharded
//! arrays over HTTP range requests. This crate is the viewer on top: a
//! resolution pyramid mapped into world space, tiles streamed in on demand, and
//! a 2D camera.

mod dataset;
mod source;
mod tiles;
mod viewer;

use std::sync::Arc;

use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::prelude::*;
use bevy::window::PresentMode;
use clap::Parser;

use viewer::DatasetHandle;

#[derive(Parser, Debug)]
#[command(
    name = "bevy-data-explorer",
    about = "Stream and explore an OME-Zarr image"
)]
struct Args {
    /// OME-Zarr store (http(s) URL or local directory), or a manifest .json
    /// describing one. Defaults to the reference image.
    #[arg(default_value = source::DEFAULT_SOURCE)]
    source: String,

    /// Z slice to display for volumetric images.
    #[arg(long, default_value_t = 0)]
    z: u64,

    /// Texture memory budget for cached tiles, in MB. Larger values make
    /// zooming back out and revisiting areas redraw without refetching.
    #[arg(long, default_value_t = tiles::DEFAULT_CACHE_BUDGET_MB)]
    cache_mb: usize,
}

/// Give tile fetching a pool large enough to keep many requests in flight.
///
/// Bevy's default async-compute pool tops out at four threads, which caps tile
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Open the store before starting the window: a bad URL should fail on the
    // command line rather than behind a blank viewport.
    println!("opening {}", args.source);
    let dataset = Arc::new(source::open(&args.source)?);

    println!(
        "{}: {} levels, {} channels, {} x {} px at full resolution",
        dataset.name,
        dataset.levels.len(),
        dataset.channels.len(),
        dataset.levels[0].width,
        dataset.levels[0].height
    );
    for level in &dataset.levels {
        println!(
            "  level {}  {:>6} x {:<6} px  {:.5} {}/px  tiles {}x{} of {}px",
            level.index,
            level.width,
            level.height,
            level.scale_x,
            dataset.unit,
            level.tiles_x,
            level.tiles_y,
            level.tile_px,
        );
    }

    let mut streamer = tiles::TileStreamer::new(dataset.clone());
    streamer.z_slice = args.z;
    streamer.budget_bytes = args.cache_mb * 1024 * 1024;

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: format!("OME-Zarr — {}", dataset.name),
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
        .insert_resource(ClearColor(Color::srgb(0.04, 0.04, 0.06)))
        .insert_resource(DatasetHandle(dataset))
        .insert_resource(streamer)
        .add_systems(Startup, (viewer::setup_camera, viewer::setup_hud))
        .add_systems(
            Update,
            (
                viewer::camera_controls,
                viewer::toggle_channels,
                tiles::select_tiles,
                tiles::spawn_tile_tasks,
                tiles::collect_tile_tasks,
                tiles::evict_tiles,
                tiles::update_tile_visibility,
                viewer::update_hud,
            )
                .chain(),
        )
        .run();

    Ok(())
}
