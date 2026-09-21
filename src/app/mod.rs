//! The shell: the window, the schedule, the theme, and the chrome every
//! dataset is shown in.
//!
//! Everything here is true whatever is being explored. What is being explored
//! is decided by the format plugins `main` adds on top.

use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::prelude::*;
use bevy::window::PresentMode;

pub mod accent;
pub mod logs;
pub mod net;
pub mod prefs;
pub mod schedule;
pub mod theme;

/// Everything but the data: a window, a schedule, a grid of frames and the
/// docks around it.
pub struct ExplorerPlugin;

impl Plugin for ExplorerPlugin {
    fn build(&self, app: &mut App) {
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
                })
                // The log keeps going to the terminal; this adds a copy in
                // memory, which is the one a user can read and send back.
                .set(bevy::log::LogPlugin {
                    custom_layer: logs::capture,
                    ..default()
                }),
        );

        // After `DefaultPlugins`, which is what creates the schedules being
        // configured, and before any plugin below declares membership of a
        // stage.
        schedule::configure(app);

        app.add_plugins(bevy_feathers::FeathersPlugins)
            .add_plugins(prefs::PreferencesPlugin)
            .add_plugins(theme::ThemePlugin)
            .add_plugins(crate::render::points::PointRenderPlugin)
            .add_plugins(crate::render::lines::LineRenderPlugin)
            .add_plugins(crate::render::channels::ChannelRenderPlugin)
            .add_plugins(crate::render::volume::VolumeRenderPlugin)
            .add_plugins(crate::render::settings::SourceSettingsPlugin)
            .add_plugins(crate::view::ViewPlugin)
            .add_plugins(crate::widgets::WidgetsPlugin)
            .add_plugins(crate::bookmark::BookmarkPlugin)
            .add_plugins(crate::ui::UiPlugin)
            .add_systems(Startup, maximize_window.in_set(schedule::Boot::Window));
    }
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
            + crate::formats::tiles::TILE_FETCH_THREADS,
        async_compute: TaskPoolThreadAssignmentPolicy {
            min_threads: crate::formats::tiles::TILE_FETCH_THREADS,
            max_threads: crate::formats::tiles::TILE_FETCH_THREADS,
            percent: 1.0,
            on_thread_spawn: None,
            on_thread_destroy: None,
        },
        ..default()
    }
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
