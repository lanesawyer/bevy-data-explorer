//! An image stack drawn in depth.
//!
//! Only a stack whose metadata measures z in the space its pixels are measured
//! in is offered this way; see [`Dataset::volume_extent`]. Such a stack is read
//! whole, at the finest level that fits a 3D texture, the first time a frame
//! turns to look at it in 3D — not before, since most stacks are only ever
//! paged through — and drawn as one ray-marched box on the source's volume
//! layer.
//!
//! Zooming in reads finer detail. Whatever the 3D frames can see of the
//! volume is read again at the finest level where it fits the same budget,
//! once the views have held still for a moment, and drawn inside the whole in
//! place of the coarse voxels there. The whole stays, so turning away from the
//! detail shows the rest of the specimen at once rather than a hole.
//!
//! [`Dataset::volume_extent`]: super::dataset::Dataset::volume_extent

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::TileStreamer;
use super::dataset::{VolumePixels, VolumeRegion, read_volume};
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

/// How long the views must hold still before the detail they see is read.
///
/// A turn or a zoom passes over many regions on its way to the one it stops
/// at, and each read is seconds of transfer; starting one per region crossed
/// would spend the bandwidth on views nobody stopped to look at.
const DETAIL_SETTLE_SECS: f32 = 0.3;

/// Rays cast across each side of a frame to find what it can see. The region
/// they bound is widened to whole tiles, so a sparse grid is enough.
const RAYS_PER_SIDE: usize = 9;

type Read = Fetching<Result<VolumePixels, String>>;

enum State {
    /// Not read, because no frame has looked at it in 3D yet.
    Idle,
    Loading {
        task: Read,
        /// Slices read so far, written by the task.
        progress: Arc<AtomicU64>,
    },
    Ready {
        entity: Entity,
        material: Handle<VolumeMaterial>,
    },
    Failed(String),
}

/// The finer read of what the frames can see.
#[derive(Default)]
struct Detail {
    /// The region the views want, and when they started wanting it.
    wanted: Option<(VolumeRegion, f32)>,
    loading: Option<(VolumeRegion, Read, Arc<AtomicU64>)>,
    shown: Option<VolumeRegion>,
}

/// The 3D form of an image stack, on the source entity beside its tiles.
#[derive(Component)]
pub struct ImageVolume {
    /// The whole stack, at the level it is drawn coarsely from.
    whole: VolumeRegion,
    state: State,
    detail: Detail,
    /// Which channels the volume was composited with. The composite is baked
    /// into the texture, as it is into tiles, so toggling one means reading
    /// the stack again.
    channels: Vec<bool>,
}

impl ImageVolume {
    pub fn new(whole: VolumeRegion) -> Self {
        ImageVolume {
            whole,
            state: State::Idle,
            detail: Detail::default(),
            channels: Vec::new(),
        }
    }

    /// A line for the status overlay, once there is anything to say.
    pub fn status(&self, depth: u64) -> Option<String> {
        let whole = match &self.state {
            State::Idle => return None,
            State::Loading { progress, .. } => {
                return Some(format!(
                    "3D: reading slices, {} of {depth}",
                    progress.load(Ordering::Relaxed)
                ));
            }
            State::Failed(e) => return Some(format!("3D: {e}")),
            State::Ready { .. } => format!("3D: level {}", self.whole.level),
        };
        let detail = match (&self.detail.loading, &self.detail.shown) {
            (Some((region, _, progress)), _) => format!(
                ", reading level {} for detail, {} of {depth}",
                region.level,
                progress.load(Ordering::Relaxed)
            ),
            (None, Some(region)) => format!(
                ", detail at level {} ({} x {} px)",
                region.level,
                region.width(),
                region.height()
            ),
            (None, None) => String::new(),
        };
        Some(whole + &detail)
    }

    fn drop_drawn(&mut self, commands: &mut Commands) {
        if let State::Ready { entity, .. } = self.state {
            commands.entity(entity).despawn();
        }
        self.state = State::Idle;
        self.detail = Detail::default();
    }

    /// Whether a read is needed for the views to see `wanted` as finely as it
    /// fits: nothing shown or on its way already holds it.
    fn needs(&self, wanted: &VolumeRegion) -> bool {
        let held = |region: &VolumeRegion| region.covers(wanted);
        !self.detail.shown.as_ref().is_some_and(held)
            && !self
                .detail
                .loading
                .as_ref()
                .is_some_and(|(region, ..)| held(region))
    }
}

fn start_read(streamer: &TileStreamer, region: VolumeRegion) -> (Read, Arc<AtomicU64>) {
    let dataset = streamer.dataset().clone();
    let channels = streamer.channels.clone();
    let progress = Arc::new(AtomicU64::new(0));
    let counter = progress.clone();
    let task = fetching(async move { read_volume(&dataset, region, &channels, &counter).await });
    (task, progress)
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
        let whole = volume.whole;
        let (task, progress) = start_read(streamer, whole);
        info!(
            "3D: reading level {} of {}",
            whole.level,
            streamer.dataset().name
        );
        volume.channels = channels;
        volume.state = State::Loading { task, progress };
    }
}

/// Read what the 3D frames can see at the finest level it fits, once they
/// have held still on it.
pub fn request_detail(
    time: Res<Time>,
    mut volumes: Query<(Entity, &SourceVolume, &TileStreamer, &mut ImageVolume)>,
    frames: Query<(&Camera, &GlobalTransform, &ShowsSource), With<Orbit>>,
) {
    let now = time.elapsed_secs();
    for (source, placed, streamer, mut volume) in &mut volumes {
        if !matches!(volume.state, State::Ready { .. }) {
            continue;
        }
        let rays = frames
            .iter()
            .filter(|(.., shows)| shows.0 == source)
            .flat_map(|(camera, global, _)| {
                let rect = camera.logical_viewport_rect().unwrap_or(Rect::EMPTY);
                (0..RAYS_PER_SIDE * RAYS_PER_SIDE).filter_map(move |i| {
                    let at = Vec2::new((i % RAYS_PER_SIDE) as f32, (i / RAYS_PER_SIDE) as f32)
                        / (RAYS_PER_SIDE - 1) as f32;
                    camera
                        .viewport_to_world(global, rect.min + rect.size() * at)
                        .ok()
                })
            });
        let dataset = streamer.dataset();
        // Only worth reading when it is finer than what is already drawn.
        let wanted = placed
            .seen_by(rays)
            .and_then(|(min, max)| {
                dataset.region_within(min, max, VOLUME_VOXEL_BUDGET, MAX_TEXTURE_EDGE)
            })
            .filter(|region| region.level < volume.whole.level);

        let Some(wanted) = wanted else {
            volume.detail.wanted = None;
            continue;
        };
        let since = match volume.detail.wanted {
            Some((region, since)) if region == wanted => since,
            _ => {
                volume.detail.wanted = Some((wanted, now));
                now
            }
        };
        if now - since < DETAIL_SETTLE_SECS || !volume.needs(&wanted) {
            continue;
        }
        // Replacing a read under way abandons it: the views have moved on.
        let (task, progress) = start_read(streamer, wanted);
        volume.detail.loading = Some((wanted, task, progress));
    }
}

/// Turn finished reads into what is drawn: the box for the whole, and the
/// detail inside it.
pub fn collect_volumes(
    mut commands: Commands,
    mut volumes: Query<(&SourceVolume, &TileStreamer, &mut ImageVolume)>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<VolumeMaterial>>,
) {
    for (placed, streamer, mut volume) in &mut volumes {
        if let State::Loading { task, .. } = &mut volume.state
            && let Some(outcome) = task.take()
        {
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
                    let material =
                        materials.add(VolumeMaterial::new(texture, min, max, dimensions));
                    let entity = commands
                        .spawn((
                            Mesh2d(meshes.add(Cuboid::from_size(placed.size))),
                            MeshMaterial2d(material.clone()),
                            Transform::from_translation(placed.centre),
                            RenderLayers::layer(placed.layer),
                        ))
                        .id();
                    info!(
                        "3D: volume ready, {} x {} x {}",
                        dimensions.x, dimensions.y, dimensions.z
                    );
                    State::Ready { entity, material }
                }
                Err(e) => {
                    warn!("3D: {e}");
                    State::Failed(e)
                }
            };
        }

        let Some((region, task, _)) = &mut volume.detail.loading else {
            continue;
        };
        let Some(outcome) = task.take() else {
            continue;
        };
        let region = *region;
        volume.detail.loading = None;
        let pixels = match outcome {
            Ok(pixels) => pixels,
            Err(e) => {
                warn!("3D detail: {e}");
                continue;
            }
        };
        let State::Ready { material, .. } = &volume.state else {
            continue;
        };
        let (Some((min, max)), Some(mut material)) = (
            streamer.dataset().region_box(&region),
            materials.get_mut(material),
        ) else {
            continue;
        };
        let dimensions = UVec3::new(pixels.width, pixels.height, pixels.depth);
        let texture = images.add(volume_texture(
            pixels.width,
            pixels.height,
            pixels.depth,
            pixels.rgba,
        ));
        // The texture it replaces is dropped with its last handle, here.
        material.set_detail(texture, min, max, dimensions);
        volume.detail.shown = Some(region);
        info!(
            "3D: detail at level {}, {} x {} px",
            region.level,
            region.width(),
            region.height()
        );
    }
}
