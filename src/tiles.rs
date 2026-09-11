//! Tile streaming.
//!
//! Each frame the viewer works out which level of the pyramid matches the
//! current zoom, then requests the tiles covering the viewport at that level
//! *and* at every coarser level. Coarse tiles are few, arrive first, and are
//! drawn underneath, so panning into new territory shows a blurry version
//! immediately that sharpens as finer tiles land.
//!
//! Tiles that scroll out of view are not dropped straight away. They are kept
//! in a least-recently-wanted cache under a memory budget, so zooming in and
//! back out, or panning away and returning, redraws from what is already
//! resident instead of refetching it.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::Anchor;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use zarrs_codec::ArrayPartialDecoderTraits;

use crate::dataset::{Channel, Dataset, TilePixels, read_tile};
use crate::panel::{Panel, PanelKind};

/// Threads reserved for fetching and decoding tiles.
///
/// A tile read is blocking from end to end — a blocking HTTP request followed
/// by a synchronous decode — so one tile occupies one thread for its whole
/// duration and concurrency is capped by the pool, not by any limit here.
/// Bevy's default async-compute pool is at most four threads, which on a
/// typical machine leaves only three tiles in flight; these threads sit blocked
/// on a socket rather than burning CPU, so a much larger pool is appropriate.
pub const TILE_FETCH_THREADS: usize = 24;

/// Tile requests in flight at once, matched to the pool so that queued work
/// stays short enough to remain cancellable.
const MAX_IN_FLIGHT: usize = TILE_FETCH_THREADS;

/// Cached shard decoders. Each holds a shard index, so reusing one saves both a
/// round trip and ~16 KB per tile.
const MAX_CACHED_SHARDS: usize = 96;

/// How much texture memory resident tiles may occupy before the least recently
/// wanted ones are dropped.
pub const DEFAULT_CACHE_BUDGET_MB: usize = 256;

/// Upper bound on remembered "nothing here" and failed tiles. These hold no
/// texture, and keeping them avoids re-requesting known-empty regions.
const MAX_EMPTY_SLOTS: usize = 32_768;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub level: usize,
    pub ty: u64,
    pub tx: u64,
}

/// Marks a spawned tile sprite, carrying which tile it draws so that
/// visibility can follow the active level.
#[derive(Component)]
pub struct Tile(pub TileKey);

struct Slot {
    state: SlotState,
    /// Frame on which this tile was last part of the wanted set. Drives
    /// eviction order.
    last_wanted: u64,
}

enum SlotState {
    Loading(Task<TileOutcome>),
    Ready {
        entity: Entity,
        /// Texture footprint, used to keep the cache inside its budget.
        bytes: usize,
    },
    /// Nothing to draw: the tile is outside the image or entirely fill value.
    Blank,
    Failed,
}

impl SlotState {
    fn holds_texture(&self) -> bool {
        matches!(self, SlotState::Ready { .. })
    }
}

enum TileOutcome {
    Ready(TilePixels),
    Blank,
    Failed(String),
    /// The view moved on before this tile started, so its work was skipped.
    Cancelled,
}

type ShardKey = (usize, Vec<u64>);
type DecoderCell = Arc<OnceLock<Result<Arc<dyn ArrayPartialDecoderTraits>, String>>>;

#[derive(Default)]
struct DecoderCache {
    entries: Mutex<HashMap<ShardKey, DecoderCell>>,
}

impl DecoderCache {
    /// Get or build the decoder for a shard. Construction happens outside the
    /// map lock so that a slow index fetch never blocks other shards, while
    /// `OnceLock` still collapses a race on the *same* shard into one fetch.
    fn get(
        &self,
        dataset: &Dataset,
        key: ShardKey,
    ) -> Result<Arc<dyn ArrayPartialDecoderTraits>, String> {
        let cell = {
            let mut entries = self.entries.lock().unwrap();
            entries.entry(key.clone()).or_default().clone()
        };
        cell.get_or_init(|| {
            dataset.levels[key.0]
                .array
                .partial_decoder(&key.1)
                .map(|d| d as Arc<dyn ArrayPartialDecoderTraits>)
                .map_err(|e| e.to_string())
        })
        .clone()
    }

    fn retain(&self, keep: &HashSet<ShardKey>) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() <= MAX_CACHED_SHARDS {
            return;
        }
        entries.retain(|k, _| keep.contains(k));
    }
}

#[derive(Resource)]
pub struct TileStreamer {
    dataset: Arc<Dataset>,
    /// Live display settings, edited by the UI and copied into each task.
    pub channels: Vec<Channel>,
    decoders: Arc<DecoderCache>,
    /// The wanted set, shared with workers so that queued tasks can bail out
    /// when the view moves before they start.
    wanted_shared: Arc<RwLock<HashSet<TileKey>>>,
    slots: HashMap<TileKey, Slot>,
    /// Tiles that should exist right now, coarsest first.
    wanted: Vec<TileKey>,
    pub z_slice: u64,
    pub active_level: usize,
    pub in_flight: usize,
    /// Frame counter driving least-recently-wanted eviction.
    frame: u64,
    /// Texture memory currently held by resident tiles.
    resident_bytes: usize,
    /// Budget for `resident_bytes`.
    pub budget_bytes: usize,
    /// Tiles abandoned before starting, for the status display.
    pub cancelled: usize,
}

impl TileStreamer {
    pub fn new(dataset: Arc<Dataset>) -> Self {
        TileStreamer {
            channels: dataset.channels.clone(),
            dataset,
            decoders: Arc::new(DecoderCache::default()),
            wanted_shared: Arc::new(RwLock::new(HashSet::new())),
            slots: HashMap::new(),
            wanted: Vec::new(),
            z_slice: 0,
            active_level: 0,
            in_flight: 0,
            frame: 0,
            resident_bytes: 0,
            budget_bytes: DEFAULT_CACHE_BUDGET_MB * 1024 * 1024,
            cancelled: 0,
        }
    }

    pub fn dataset(&self) -> &Arc<Dataset> {
        &self.dataset
    }

    pub fn loaded(&self) -> usize {
        self.slots
            .values()
            .filter(|s| s.state.holds_texture())
            .count()
    }

    pub fn failed(&self) -> usize {
        self.slots
            .values()
            .filter(|s| matches!(s.state, SlotState::Failed))
            .count()
    }

    /// Texture memory held by resident tiles, in bytes.
    pub fn resident_bytes(&self) -> usize {
        self.resident_bytes
    }

    /// Drop every tile and start over, e.g. after changing channels.
    pub fn reset(&mut self, commands: &mut Commands) {
        for slot in self.slots.values() {
            if let SlotState::Ready { entity, .. } = slot.state {
                commands.entity(entity).despawn();
            }
        }
        self.slots.clear();
        self.in_flight = 0;
        self.resident_bytes = 0;
    }
}

/// Work out the visible world rectangle and queue the tiles that cover it.
pub fn select_tiles(
    mut streamer: ResMut<TileStreamer>,
    panels: Query<(&Camera, &GlobalTransform, &Projection, &Panel)>,
) {
    let Some((camera, transform, projection)) = panels
        .iter()
        .find(|(_, _, _, panel)| panel.kind == PanelKind::Image)
        .map(|(c, t, p, _)| (c, t, p))
    else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let Some(viewport) = camera.logical_viewport_size() else {
        return;
    };

    let centre = transform.translation().truncate();
    let half = Vec2::new(ortho.area.width(), ortho.area.height()) * 0.5;
    // A margin keeps tiles just off screen ready before they are panned into.
    let margin = half * 0.15;
    let min = centre - half - margin;
    let max = centre + half + margin;

    let units_per_px = ortho.area.width() / viewport.x.max(1.0);
    let dataset = streamer.dataset.clone();
    let active = dataset.level_for(units_per_px);
    streamer.active_level = active;

    let mut wanted = Vec::new();
    // Coarsest first so the cheap, fast tiles are requested ahead of fine ones.
    for level_index in (active..dataset.levels.len()).rev() {
        let level = &dataset.levels[level_index];
        let scale_x = level.scale_x as f32;
        let scale_y = level.scale_y as f32;
        if scale_x <= 0.0 || scale_y <= 0.0 {
            continue;
        }

        // World rect -> level pixels -> tile indices. World y runs downward in
        // image space but upward in Bevy, hence the negation.
        let px_x0 = (min.x - level.origin_x as f32) / scale_x;
        let px_x1 = (max.x - level.origin_x as f32) / scale_x;
        let px_y0 = (-max.y - level.origin_y as f32) / scale_y;
        let px_y1 = (-min.y - level.origin_y as f32) / scale_y;

        let tx0 = (px_x0 / level.tile_px as f32).floor().max(0.0) as u64;
        let ty0 = (px_y0 / level.tile_px as f32).floor().max(0.0) as u64;
        let tx1 = (px_x1 / level.tile_px as f32).ceil().max(0.0) as u64;
        let ty1 = (px_y1 / level.tile_px as f32).ceil().max(0.0) as u64;

        // Within a level, ask for the tiles nearest the middle of the view
        // first: on a fast pan or zoom those are what the eye lands on, and
        // the outer ones are the likeliest to be abandoned.
        let mut level_tiles: Vec<(u64, TileKey)> = Vec::new();
        for ty in ty0..ty1.min(level.tiles_y) {
            for tx in tx0..tx1.min(level.tiles_x) {
                let key = TileKey {
                    level: level_index,
                    ty,
                    tx,
                };
                let Some((wx0, wy0, wx1, wy1)) = level.tile_world_rect(ty, tx) else {
                    continue;
                };
                let mid = Vec2::new((wx0 + wx1) * 0.5, -(wy0 + wy1) * 0.5);
                level_tiles.push((mid.distance_squared(centre) as u64, key));
            }
        }
        level_tiles.sort_unstable_by_key(|(distance, _)| *distance);
        wanted.extend(level_tiles.into_iter().map(|(_, key)| key));
    }

    // Touch everything wanted so eviction can tell live tiles from stale ones.
    streamer.frame = streamer.frame.wrapping_add(1);
    let frame = streamer.frame;
    for key in &wanted {
        if let Some(slot) = streamer.slots.get_mut(key) {
            slot.last_wanted = frame;
        }
    }

    // Publish for the workers, so queued tasks can check whether they still
    // matter before doing any network work.
    if let Ok(mut shared) = streamer.wanted_shared.write() {
        shared.clear();
        shared.extend(wanted.iter().copied());
    }

    streamer.wanted = wanted;
}

/// Start tasks for wanted tiles that are not loaded yet.
pub fn spawn_tile_tasks(mut streamer: ResMut<TileStreamer>) {
    let pool = AsyncComputeTaskPool::get();
    let dataset = streamer.dataset.clone();
    let decoders = streamer.decoders.clone();
    let z = streamer.z_slice;
    let channels = streamer.channels.clone();

    let shared = streamer.wanted_shared.clone();
    let wanted = std::mem::take(&mut streamer.wanted);
    for key in &wanted {
        if streamer.in_flight >= MAX_IN_FLIGHT {
            break;
        }
        if streamer.slots.contains_key(key) {
            continue;
        }

        let dataset = dataset.clone();
        let decoders = decoders.clone();
        let channels = channels.clone();
        let shared = shared.clone();
        let key = *key;
        let task = pool.spawn(async move {
            // A blocking read cannot be interrupted once it is under way, so
            // the useful moment to give up is before starting. Tasks queued
            // behind a busy pool reach here long after being spawned, by which
            // point a pan or zoom may have made them irrelevant.
            let still_wanted = |shared: &RwLock<HashSet<TileKey>>| {
                shared.read().map(|w| w.contains(&key)).unwrap_or(true)
            };
            if !still_wanted(&shared) {
                return TileOutcome::Cancelled;
            }

            let level = &dataset.levels[key.level];
            let shard = level.shard_of(&dataset.layout, key.ty, key.tx, z);
            let decoder = match decoders.get(&dataset, (key.level, shard)) {
                Ok(d) => d,
                Err(e) => return TileOutcome::Failed(e),
            };

            // Fetching a shard index is itself a round trip, so check again
            // before paying for the tile body.
            if !still_wanted(&shared) {
                return TileOutcome::Cancelled;
            }
            match read_tile(
                &dataset,
                level,
                &channels,
                decoder.as_ref(),
                key.ty,
                key.tx,
                z,
            ) {
                Ok(Some(pixels)) => TileOutcome::Ready(pixels),
                Ok(None) => TileOutcome::Blank,
                Err(e) => TileOutcome::Failed(e),
            }
        });

        let frame = streamer.frame;
        streamer.slots.insert(
            key,
            Slot {
                state: SlotState::Loading(task),
                last_wanted: frame,
            },
        );
        streamer.in_flight += 1;
    }
    streamer.wanted = wanted;
}

/// Turn finished tasks into sprites.
pub fn collect_tile_tasks(
    mut commands: Commands,
    mut streamer: ResMut<TileStreamer>,
    mut images: ResMut<Assets<Image>>,
) {
    let dataset = streamer.dataset.clone();
    let level_count = dataset.levels.len();
    let mut finished = Vec::new();

    for (key, slot) in streamer.slots.iter_mut() {
        let SlotState::Loading(task) = &mut slot.state else {
            continue;
        };
        if let Some(outcome) = block_on(poll_once(task)) {
            finished.push((*key, outcome));
        }
    }

    for (key, outcome) in finished {
        streamer.in_flight = streamer.in_flight.saturating_sub(1);

        if matches!(outcome, TileOutcome::Cancelled) {
            // Forget it entirely: it did no work, and dropping the slot lets it
            // be requested again if the view comes back.
            streamer.slots.remove(&key);
            streamer.cancelled += 1;
            continue;
        }

        let level = &dataset.levels[key.level];

        let state = match outcome {
            TileOutcome::Ready(pixels) => {
                let Some((x0, y0, x1, y1)) = level.tile_world_rect(key.ty, key.tx) else {
                    let frame = streamer.frame;
                    streamer.slots.insert(
                        key,
                        Slot {
                            state: SlotState::Blank,
                            last_wanted: frame,
                        },
                    );
                    continue;
                };
                let bytes = pixels.rgba.len();

                let mut image = Image::new(
                    Extent3d {
                        width: pixels.width,
                        height: pixels.height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    pixels.rgba,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::RENDER_WORLD,
                );
                // Nearest magnification keeps individual pixels crisp past 1:1;
                // linear minification avoids shimmer when zoomed out. Clamping
                // stops neighbouring tiles bleeding across their seams.
                image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    mag_filter: ImageFilterMode::Nearest,
                    min_filter: ImageFilterMode::Linear,
                    mipmap_filter: ImageFilterMode::Linear,
                    address_mode_u: ImageAddressMode::ClampToEdge,
                    address_mode_v: ImageAddressMode::ClampToEdge,
                    ..default()
                });

                // Finer levels sit on top of coarser ones.
                let z = (level_count - key.level) as f32;
                let entity = commands
                    .spawn((
                        Sprite {
                            image: images.add(image),
                            custom_size: Some(Vec2::new(x1 - x0, y1 - y0)),
                            ..default()
                        },
                        Anchor::TOP_LEFT,
                        Transform::from_xyz(x0, -y0, z),
                        RenderLayers::layer(PanelKind::Image.layer()),
                        Tile(key),
                    ))
                    .id();
                streamer.resident_bytes += bytes;
                SlotState::Ready { entity, bytes }
            }
            TileOutcome::Blank => SlotState::Blank,
            TileOutcome::Failed(e) => {
                warn!("tile {:?}: {e}", key);
                SlotState::Failed
            }
            TileOutcome::Cancelled => unreachable!("handled above"),
        };
        let frame = streamer.frame;
        streamer.slots.insert(
            key,
            Slot {
                state,
                last_wanted: frame,
            },
        );
    }
}

/// Keep the tile cache inside its memory budget by dropping the least recently
/// wanted tiles.
///
/// Tiles are deliberately *not* dropped as soon as they leave the viewport.
/// Zooming in narrows the wanted set to a handful of fine tiles, and the coarse
/// ones covering the surrounding area are exactly what is needed again a moment
/// later when zooming back out. Holding them until memory runs short makes that
/// round trip free.
pub fn evict_tiles(mut commands: Commands, mut streamer: ResMut<TileStreamer>) {
    let wanted: HashSet<TileKey> = streamer.wanted.iter().copied().collect();
    if wanted.is_empty() {
        return;
    }

    let candidates: Vec<Candidate> = streamer
        .slots
        .iter()
        .filter(|(key, _)| !wanted.contains(*key))
        .filter_map(|(key, slot)| match slot.state {
            SlotState::Ready { bytes, .. } => Some(Candidate {
                key: *key,
                last_wanted: slot.last_wanted,
                bytes,
            }),
            _ => None,
        })
        .collect();

    for key in plan_eviction(candidates, streamer.resident_bytes, streamer.budget_bytes) {
        if let Some(slot) = streamer.slots.remove(&key) {
            if let SlotState::Ready { entity, bytes } = slot.state {
                commands.entity(entity).despawn();
                streamer.resident_bytes = streamer.resident_bytes.saturating_sub(bytes);
            }
        }
    }

    // Blank and failed slots cost no texture memory, but should not grow without
    // bound on a long pan across a large image.
    let empty_slots = streamer
        .slots
        .values()
        .filter(|slot| !slot.state.holds_texture())
        .count();
    if empty_slots > MAX_EMPTY_SLOTS {
        let cutoff = streamer.frame.saturating_sub(600);
        streamer.slots.retain(|key, slot| {
            slot.state.holds_texture()
                || wanted.contains(key)
                || slot.last_wanted > cutoff
                || matches!(slot.state, SlotState::Loading(_))
        });
    }

    let dataset = streamer.dataset.clone();
    let keep: HashSet<ShardKey> = wanted
        .iter()
        .map(|key| {
            let level = &dataset.levels[key.level];
            (
                key.level,
                level.shard_of(&dataset.layout, key.ty, key.tx, streamer.z_slice),
            )
        })
        .collect();
    streamer.decoders.retain(&keep);
}

/// A resident tile considered for eviction.
struct Candidate {
    key: TileKey,
    last_wanted: u64,
    bytes: usize,
}

/// Choose which tiles to drop so that `resident` falls within `budget`,
/// least recently wanted first. Returns nothing while the cache fits.
fn plan_eviction(mut candidates: Vec<Candidate>, resident: usize, budget: usize) -> Vec<TileKey> {
    if resident <= budget {
        return Vec::new();
    }
    candidates.sort_unstable_by_key(|c| c.last_wanted);

    let mut freed = 0usize;
    let mut evict = Vec::new();
    for candidate in candidates {
        if resident - freed <= budget {
            break;
        }
        freed += candidate.bytes;
        evict.push(candidate.key);
    }
    evict
}

/// Whether a cached tile should paint at the level currently in use.
fn tile_visible(level: usize, active_level: usize) -> bool {
    level >= active_level
}

/// Hide cached tiles that are finer than the level currently in use.
///
/// A retained fine tile sits above the coarse level in z, so once the view
/// zooms out it would be drawn over the level actually chosen for this scale,
/// shimmering as it minifies. Hiding keeps it resident and instantly available
/// without letting it paint.
pub fn update_tile_visibility(
    streamer: Res<TileStreamer>,
    mut tiles: Query<(&Tile, &mut Visibility)>,
) {
    let active = streamer.active_level;
    for (tile, mut visibility) in &mut tiles {
        let wanted = if tile_visible(tile.0.level, active) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(level: usize, ty: u64) -> TileKey {
        TileKey { level, ty, tx: 0 }
    }

    fn candidate(level: usize, ty: u64, last_wanted: u64, bytes: usize) -> Candidate {
        Candidate {
            key: key(level, ty),
            last_wanted,
            bytes,
        }
    }

    #[test]
    fn nothing_is_evicted_while_the_cache_fits() {
        // This is what keeps a zoom out instant: tiles left the viewport but
        // stay resident because there is still room for them.
        let candidates = vec![candidate(0, 0, 1, 100), candidate(0, 1, 2, 100)];
        assert!(plan_eviction(candidates, 200, 1000).is_empty());
    }

    #[test]
    fn the_least_recently_wanted_tile_goes_first() {
        let candidates = vec![
            candidate(0, 0, 30, 100),
            candidate(0, 1, 10, 100),
            candidate(0, 2, 20, 100),
        ];
        assert_eq!(plan_eviction(candidates, 300, 250), vec![key(0, 1)]);
    }

    #[test]
    fn eviction_stops_as_soon_as_the_budget_is_met() {
        let candidates = vec![
            candidate(0, 0, 1, 100),
            candidate(0, 1, 2, 100),
            candidate(0, 2, 3, 100),
            candidate(0, 3, 4, 100),
        ];
        // Needs to free 150, so two tiles suffice and the rest stay cached.
        let evicted = plan_eviction(candidates, 400, 250);
        assert_eq!(evicted, vec![key(0, 0), key(0, 1)]);
    }

    #[test]
    fn an_empty_candidate_list_cannot_loop_forever() {
        // Every resident tile is in use, so nothing can be freed.
        assert!(plan_eviction(Vec::new(), 500, 100).is_empty());
    }

    #[test]
    fn the_fetch_pool_is_large_enough_to_matter() {
        // Bevy's default async-compute pool caps at 4 threads, and one blocking
        // tile read occupies a thread for its whole duration.
        assert!(
            TILE_FETCH_THREADS > 4,
            "a pool this small would serialise tile loading"
        );
        assert_eq!(MAX_IN_FLIGHT, TILE_FETCH_THREADS);
    }

    #[test]
    fn tiles_finer_than_the_active_level_are_hidden() {
        // Zoomed out to level 3: the coarse tiles paint, and level 0 tiles
        // retained from an earlier zoom stay resident but invisible.
        assert!(tile_visible(3, 3));
        assert!(tile_visible(5, 3));
        assert!(!tile_visible(0, 3));
    }
}
