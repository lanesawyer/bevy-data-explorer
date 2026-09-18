//! An image stack drawn in depth.
//!
//! Only a stack whose metadata measures z in the space its pixels are measured
//! in is offered this way; see [`Dataset::volume_extent`]. Such a stack is read
//! whole, at the finest level that fits a 3D texture, the first time a frame
//! turns to look at it in 3D — not before, since most stacks are only ever
//! paged through — and drawn as one ray-marched box on the source's volume
//! layer.
//!
//! [`Dataset::volume_extent`]: super::dataset::Dataset::volume_extent

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::TileStreamer;
use super::dataset::{VolumePixels, read_volume};
use crate::app::net::{Fetching, fetching};
use crate::render::volume::{VolumeMaterial, volume_texture};
use crate::source::volume::SourceVolume;
use crate::view::{Orbit, ShowsSource};

/// Voxels a volume may hold, which at four bytes each is 64 MB of texture.
///
/// The reference Tissuecyte stack is 142 slices deep: its coarsest level, 312 x
/// 234 a slice, comes to 10.4M voxels, and the next finer one to 41M — 166 MB
/// of texture, and four times the samples for every ray. One level of the
/// pyramid is a factor of four, so a budget between the two picks the level
/// that reads in seconds and still shows the anatomy.
pub const VOLUME_VOXEL_BUDGET: u64 = 16 * 1024 * 1024;

/// The longest edge a 3D texture is guaranteed to have under wgpu's default
/// limits.
pub const MAX_TEXTURE_EDGE: u64 = 2048;

enum State {
    /// Not read, because no frame has looked at it in 3D yet.
    Idle,
    Loading {
        task: Fetching<Result<VolumePixels, String>>,
        /// Slices read so far, written by the task.
        progress: Arc<AtomicU64>,
    },
    Ready(Entity),
    Failed(String),
}

/// The 3D form of an image stack, on the source entity beside its tiles.
#[derive(Component)]
pub struct ImageVolume {
    /// Level of the pyramid the volume is read from.
    level: usize,
    state: State,
    /// Which channels the volume was composited with. The composite is baked
    /// into the texture, as it is into tiles, so toggling one means reading
    /// the stack again.
    channels: Vec<bool>,
}

impl ImageVolume {
    pub fn new(level: usize) -> Self {
        ImageVolume {
            level,
            state: State::Idle,
            channels: Vec::new(),
        }
    }

    /// A line for the status overlay, once there is anything to say.
    pub fn status(&self, depth: u64) -> Option<String> {
        match &self.state {
            State::Idle => None,
            State::Loading { progress, .. } => Some(format!(
                "3D: reading slices, {} of {depth}",
                progress.load(Ordering::Relaxed)
            )),
            State::Ready(_) => Some(format!("3D: level {}, all {depth} slices", self.level)),
            State::Failed(e) => Some(format!("3D: {e}")),
        }
    }

    fn drop_drawn(&mut self, commands: &mut Commands) {
        if let State::Ready(entity) = self.state {
            commands.entity(entity).despawn();
        }
        self.state = State::Idle;
    }
}

/// Start reading a volume the first time a frame orbits it, and start again
/// when the channels it was composited with change.
pub fn request_volumes(
    mut commands: Commands,
    mut volumes: Query<(Entity, &TileStreamer, &mut ImageVolume)>,
    orbiting: Query<&ShowsSource, With<Orbit>>,
) {
    for (source, streamer, mut volume) in &mut volumes {
        let channels: Vec<bool> = streamer.channels.iter().map(|c| c.active).collect();
        if !matches!(volume.state, State::Idle) && volume.channels != channels {
            volume.drop_drawn(&mut commands);
        }
        if !matches!(volume.state, State::Idle) || !orbiting.iter().any(|shows| shows.0 == source) {
            continue;
        }

        let dataset = streamer.dataset().clone();
        let active = streamer.channels.clone();
        let level = volume.level;
        let progress = Arc::new(AtomicU64::new(0));
        let counter = progress.clone();
        let task = fetching(async move {
            read_volume(&dataset, &dataset.levels[level], &active, &counter).await
        });
        info!("3D: reading level {level} of {}", streamer.dataset().name);
        volume.channels = channels;
        volume.state = State::Loading { task, progress };
    }
}

/// Turn a finished read into the box that draws it.
pub fn collect_volumes(
    mut commands: Commands,
    mut volumes: Query<(&SourceVolume, &mut ImageVolume)>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<VolumeMaterial>>,
) {
    for (placed, mut volume) in &mut volumes {
        let State::Loading { task, .. } = &mut volume.state else {
            continue;
        };
        let Some(outcome) = task.take() else {
            continue;
        };
        volume.state = match outcome {
            Ok(pixels) => {
                let dimensions = UVec3::new(pixels.width, pixels.height, pixels.depth);
                let texture = images.add(volume_texture(
                    pixels.width,
                    pixels.height,
                    pixels.depth,
                    pixels.rgba,
                ));
                let (min, max) = placed.bounds();
                let entity = commands
                    .spawn((
                        Mesh2d(meshes.add(Cuboid::from_size(placed.size))),
                        MeshMaterial2d(
                            materials.add(VolumeMaterial::new(texture, min, max, dimensions)),
                        ),
                        Transform::from_translation(placed.centre),
                        RenderLayers::layer(placed.layer),
                    ))
                    .id();
                info!(
                    "3D: volume ready, {} x {} x {}",
                    dimensions.x, dimensions.y, dimensions.z
                );
                State::Ready(entity)
            }
            Err(e) => {
                warn!("3D: {e}");
                State::Failed(e)
            }
        };
    }
}
