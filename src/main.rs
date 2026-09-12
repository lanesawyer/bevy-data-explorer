//! A streaming explorer for large scientific datasets.
//!
//! Two formats are supported so far, shown side by side in independent panels:
//! OME-Zarr images, read through `zarrs`, and the Allen Institute's
//! Scatterbrain point clouds. Both are far too large to load whole, so each
//! panel streams only what its own view needs and pulls in more detail as you
//! zoom.

mod cellpanel;
mod cellproperties;
mod dataset;
mod datasource;
mod hud;
mod inspector;
mod panel;
mod pointcloud;
mod points_render;
mod scatterbrain;
mod sidebar;
mod slices;
mod source;
mod tiles;
mod viewconfig;
mod widgets;

use std::sync::Arc;

use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::prelude::*;
use bevy::window::PresentMode;
use clap::Parser;

use datasource::{DataSource, SourceExtent};

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

    /// A second Scatterbrain point cloud, shown as a fourth panel. Pass `none`
    /// to leave it out.
    #[arg(long, default_value = DEFAULT_CELLS)]
    cells: String,

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

/// The docks, which reserve their space before the frames are laid out.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct DockSystems;

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
    // Feathers styles the widgets; its slider reports value changes but leaves
    // writing them back to the app.
    .add_plugins(points_render::PointRenderPlugin)
    .add_plugins(bevy_feathers::FeathersPlugins)
    .insert_resource(bevy_feathers::theme::UiTheme(
        bevy_feathers::dark_theme::create_dark_theme(),
    ))
    .add_observer(bevy_ui_widgets::slider_self_update)
    .add_observer(viewconfig::on_layout_button)
    .add_observer(viewconfig::on_add_visualization)
    .add_observer(cellpanel::on_colour_by)
    .add_observer(cellpanel::on_value_toggled)
    .add_observer(cellpanel::on_clear_property)
    .add_observer(cellpanel::on_clear_all)
    .add_observer(hud::on_info_pressed)
    .add_observer(hud::on_source_chosen)
    .add_message::<panel::PanelRequest>()
    .init_resource::<panel::FrameArea>()
    .init_resource::<panel::SelectedPanel>()
    .init_resource::<sidebar::Sidebar>()
    .init_resource::<inspector::Inspector>()
    .init_resource::<cellpanel::OpenSections>()
    // The docks claim their space first; everything that places a frame or its
    // chrome measures against what is left.
    .add_systems(
        Update,
        (
            sidebar::toggle_sidebar,
            sidebar::resize_sidebar,
            sidebar::sidebar_cursor,
            inspector::open_on_request,
            inspector::close_inspector,
            inspector::resize_inspector,
            inspector::inspector_cursor,
            panel::reset_frame_area,
            sidebar::reserve_space,
            inspector::reserve_space,
        )
            .chain()
            .in_set(DockSystems),
    )
    .add_systems(
        Update,
        (
            panel::panel_buttons,
            panel::apply_panel_requests,
            panel::normalize_panels,
            panel::sync_panel_buttons,
            panel::highlight_panel_buttons,
            hud::sync_hud,
            panel::panel_controls,
            panel::update_viewports,
            hud::position_hud,
            hud::rebuild_source_menus,
            sidebar::update_sidebar,
            inspector::update_inspector,
        )
            .chain()
            .after(DockSystems),
    )
    // The sidebar's controls read the selection and write through to the
    // source, so they run after the frames have settled for the frame.
    .add_systems(
        Update,
        (
            panel::update_selection_border,
            widgets::toggle_accordions,
            widgets::update_accordions,
            viewconfig::rebuild_layout_menu,
            widgets::toggle_menus,
            widgets::position_menus,
            viewconfig::sync_opacity_slider,
            viewconfig::sync_point_size,
            cellpanel::record_open_sections,
            cellpanel::drag_range_handles,
            cellpanel::rebuild_cell_panel,
            cellpanel::update_range_controls,
            cellpanel::update_clear_buttons,
            cellpanel::apply_selection,
            viewconfig::apply_opacity,
            viewconfig::apply_opacity_to_new,
            viewconfig::apply_point_settings,
            viewconfig::apply_point_settings_to_new,
        )
            .chain()
            .after(panel::update_viewports),
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
            name: "Point cloud".into(),
            cloud,
            budget: args.point_budget,
        });
    }
    if let Some(cloud) = cells {
        app.add_plugins(pointcloud::PointCloudPlugin {
            name: "SEA-AD mapped cells".into(),
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

    app.add_systems(
        Startup,
        (
            maximize_window,
            open_frames,
            viewconfig::spawn_view_config,
            cellpanel::spawn_cell_panel,
        )
            .chain(),
    );

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
    inspector::spawn_inspector(&mut commands);
    panel::spawn_selection_border(&mut commands);
}
