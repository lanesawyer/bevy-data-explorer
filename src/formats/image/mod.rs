//! Tile streaming.
//!
//! Each frame the viewer works out which level of the pyramid matches the
//! current zoom, and requests the tiles covering the viewport at that level,
//! behind the overview tile at the coarsest level. Nothing off screen is asked
//! for until every visible tile has landed; only then does it fetch a margin
//! around the view and the levels either side, and those are abandoned the
//! moment the view moves. Coarser tiles already resident are drawn underneath,
//! so a zoom shows a blurry version immediately that sharpens as finer tiles
//! land.
//!
//! Tiles that scroll out of view are not dropped straight away. They are kept
//! in a least-recently-wanted cache under a memory budget, so zooming in and
//! back out, or panning away and returning, redraws from what is already
//! resident instead of refetching it.

pub mod dataset;
pub mod store;
pub mod volume;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::Anchor;
use zarrs_codec::AsyncArrayPartialDecoderTraits;

use crate::app::net::{Fetching, fetching};
use crate::app::schedule::Stage;
use crate::formats::image::dataset::{Channel, Dataset, TilePixels, TileSource, read_tile};
use crate::source::hover::{HoverInfo, HoverProbe};
use crate::source::stack::SliceStack;
use crate::source::{self, SourceExtent, SourceStatus};
use crate::view::ShowsSource;

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

/// Marks a spawned tile sprite.
///
/// Which tile it draws is not recorded here: the streamer's own slots say that,
/// and reading it from there is what keeps one image's visibility from reaching
/// another image's tiles.
#[derive(Component)]
pub struct Tile;

struct Slot {
    state: SlotState,
    /// Frame on which this tile was last part of the wanted set. Drives
    /// eviction order.
    last_wanted: u64,
}

enum SlotState {
    Loading(Fetching<TileOutcome>),
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
type Decoder = Arc<dyn AsyncArrayPartialDecoderTraits>;
/// An async cell rather than a `OnceLock`: building a decoder fetches the
/// shard's index, which is now a read to await rather than a call to block on.
type DecoderCell = Arc<tokio::sync::OnceCell<Result<Decoder, String>>>;

#[derive(Default)]
struct DecoderCache {
    entries: Mutex<HashMap<ShardKey, DecoderCell>>,
}

impl DecoderCache {
    /// Get or build the decoder for a shard. Construction happens outside the
    /// map lock so that a slow index fetch never blocks other shards, while
    /// `OnceLock` still collapses a race on the *same* shard into one fetch.
    async fn get(&self, dataset: &Dataset, key: ShardKey) -> Result<Decoder, String> {
        let cell = {
            let mut entries = self.entries.lock().unwrap();
            entries.entry(key.clone()).or_default().clone()
        };
        cell.get_or_init(|| async {
            dataset.levels[key.0]
                .array
                .async_partial_decoder(&key.1)
                .await
                .map(|decoder| decoder as Decoder)
                .map_err(|e| e.to_string())
        })
        .await
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

#[derive(Component)]
pub struct TileStreamer {
    /// The source entity this streamer serves. Panels showing it are the ones
    /// whose views drive loading.
    pub source: Entity,
    dataset: Arc<Dataset>,
    /// Live display settings, edited by the UI and copied into each task.
    pub channels: Vec<Channel>,
    decoders: Arc<DecoderCache>,
    /// The wanted set, shared with workers so that queued tasks can bail out
    /// when the view moves before they start.
    wanted_shared: Arc<RwLock<HashSet<TileKey>>>,
    slots: HashMap<TileKey, Slot>,
    /// Tiles to request, in order: visible ones, then prefetch once idle.
    wanted: Vec<TileKey>,
    /// Tiles worth keeping resident whether or not they are being requested:
    /// the visible set and the prefetch set.
    retained: HashSet<TileKey>,
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
    pub fn new(dataset: Arc<Dataset>, source: Entity) -> Self {
        TileStreamer {
            source,
            channels: dataset.channels.clone(),
            dataset,
            decoders: Arc::new(DecoderCache::default()),
            wanted_shared: Arc::new(RwLock::new(HashSet::new())),
            slots: HashMap::new(),
            wanted: Vec::new(),
            retained: HashSet::new(),
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

/// Work out what each frame shows and queue the tiles that cover it.
///
/// Visible tiles first, and nothing else until they have all landed: see
/// [`request_order`].
pub fn select_tiles(
    mut streamers: Query<&mut TileStreamer>,
    panels: Query<(&Camera, &GlobalTransform, &Projection, &ShowsSource)>,
) {
    for mut streamer in &mut streamers {
        let dataset = streamer.dataset.clone();
        let coarsest = dataset.levels.len().saturating_sub(1);
        // The finest level any panel is asking for. Visibility is driven from this
        // so that a tile one panel needs is never hidden on behalf of another.
        let mut active = coarsest;

        // Every panel of this kind draws the same entities, so the resident set is
        // the union of what each of them needs. A duplicated panel zoomed somewhere
        // else therefore pulls in its own tiles.
        let source = streamer.source;
        let mut views = Vec::new();
        for (camera, transform, projection, _) in
            panels.iter().filter(|(_, _, _, shows)| shows.0 == source)
        {
            let Projection::Orthographic(ortho) = projection else {
                continue;
            };
            let Some(viewport) = camera.logical_viewport_size() else {
                continue;
            };
            let view = View::new(transform, ortho);
            let level = dataset.level_for(ortho.area.width() / viewport.x.max(1.0));
            active = active.min(level);
            views.push((view, level));
        }

        let mut visible = Tiers::default();
        // The one overview a frame can show at once while its own level loads.
        for (view, _) in &views {
            visible.extend(tiles_over(&dataset, coarsest, view, view.half));
        }
        for (view, level) in &views {
            visible.extend(tiles_over(&dataset, *level, view, view.half));
        }
        let mut prefetch = visible.followed_by();
        for (view, level) in &views {
            prefetch.extend(tiles_over(&dataset, *level, view, view.half + view.margin));
            if *level < coarsest {
                prefetch.extend(tiles_over(&dataset, level + 1, view, view.half));
            }
            if *level > 0 {
                prefetch.extend(tiles_over(&dataset, level - 1, view, view.half));
            }
        }

        streamer.active_level = active;

        // Touch everything worth keeping, prefetch included, so eviction can
        // tell live tiles from stale ones whether or not they are being asked for.
        streamer.frame = streamer.frame.wrapping_add(1);
        let frame = streamer.frame;
        for key in &prefetch.seen {
            if let Some(slot) = streamer.slots.get_mut(key) {
                slot.last_wanted = frame;
            }
        }

        let wanted = request_order(&visible.order, &prefetch.order, |key| {
            streamer
                .slots
                .get(key)
                .is_some_and(|slot| !matches!(slot.state, SlotState::Loading(_)))
        });

        // Publish for the workers, so queued tasks can check whether they still
        // matter before doing any network work.
        if let Ok(mut shared) = streamer.wanted_shared.write() {
            shared.clear();
            shared.extend(wanted.iter().copied());
        }

        streamer.retained = prefetch.seen;
        streamer.wanted = wanted;
    }
}

/// What one frame is looking at, in display coordinates.
pub(crate) struct View {
    pub centre: Vec2,
    pub half: Vec2,
    /// How far past the edges a pan is likely to go next.
    pub margin: Vec2,
}

impl View {
    pub fn new(transform: &GlobalTransform, ortho: &OrthographicProjection) -> Self {
        let half = Vec2::new(ortho.area.width(), ortho.area.height()) * 0.5;
        View {
            centre: transform.translation().truncate(),
            half,
            margin: half * 0.15,
        }
    }
}

/// Tiles in the order they were first named, each named once.
pub(crate) struct Tiers<K> {
    pub order: Vec<K>,
    pub seen: HashSet<K>,
}

impl<K> Default for Tiers<K> {
    fn default() -> Self {
        Tiers {
            order: Vec::new(),
            seen: HashSet::new(),
        }
    }
}

impl<K: Copy + Eq + std::hash::Hash> Tiers<K> {
    /// A tier to follow this one, skipping anything already named in it. Its
    /// `seen` ends up holding both.
    pub fn followed_by(&self) -> Self {
        Tiers {
            order: Vec::new(),
            seen: self.seen.clone(),
        }
    }

    pub fn extend(&mut self, keys: impl IntoIterator<Item = K>) {
        for key in keys {
            if self.seen.insert(key) {
                self.order.push(key);
            }
        }
    }
}

/// The tiles of one level within `half` of a view's centre, nearest the middle
/// first: on a fast pan or zoom those are what the eye lands on, and the outer
/// ones are the likeliest to be abandoned.
fn tiles_over(dataset: &Dataset, level_index: usize, view: &View, half: Vec2) -> Vec<TileKey> {
    let level = &dataset.levels[level_index];
    let scale_x = level.scale_x as f32;
    let scale_y = level.scale_y as f32;
    if scale_x <= 0.0 || scale_y <= 0.0 {
        return Vec::new();
    }
    let (min, max) = (view.centre - half, view.centre + half);

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

    let mut tiles: Vec<(u64, TileKey)> = Vec::new();
    for ty in ty0..ty1.min(level.tiles_y) {
        for tx in tx0..tx1.min(level.tiles_x) {
            let Some((wx0, wy0, wx1, wy1)) = level.tile_world_rect(ty, tx) else {
                continue;
            };
            let mid = Vec2::new(f32::midpoint(wx0, wx1), -f32::midpoint(wy0, wy1));
            let key = TileKey {
                level: level_index,
                ty,
                tx,
            };
            tiles.push((mid.distance_squared(view.centre) as u64, key));
        }
    }
    tiles.sort_unstable_by_key(|(distance, _)| *distance);
    tiles.into_iter().map(|(_, key)| key).collect()
}

/// The tiles to ask for this frame: what the frames show, and — only once all
/// of that has landed — what they might show next.
///
/// A tile off screen shares the connection with the ones on it. Even the last
/// request to be cut off has been taking bandwidth from a visible tile until
/// then, so the margin and the levels either side are not started while
/// anything visible is still loading. Leaving them out of the wanted set is
/// also what abandons one already under way the moment the view moves.
pub(crate) fn request_order<K: Copy>(
    visible: &[K],
    prefetch: &[K],
    resolved: impl Fn(&K) -> bool,
) -> Vec<K> {
    let mut wanted = visible.to_vec();
    if visible.iter().all(resolved) {
        wanted.extend_from_slice(prefetch);
    }
    wanted
}

/// Start tasks for wanted tiles that are not loaded yet.
pub fn spawn_tile_tasks(mut streamers: Query<&mut TileStreamer>) {
    for mut streamer in &mut streamers {
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
            let task = fetching(async move {
                // A read is abandoned when its slot is dropped, but a task
                // queued behind a busy runtime can reach here long after being
                // spawned — after a pan has already made it irrelevant — so it
                // costs nothing to check before opening a connection.
                let still_wanted = |shared: &RwLock<HashSet<TileKey>>| {
                    shared.read().map_or(true, |w| w.contains(&key))
                };
                if !still_wanted(&shared) {
                    return TileOutcome::Cancelled;
                }

                let level = &dataset.levels[key.level];
                // Without shards there is no index to fetch and nothing for a
                // decoder to hold, so the tile is read from the array and its
                // chunks are fetched together.
                let decoder = if level.sharded {
                    let shard = level.shard_of(&dataset.layout, key.ty, key.tx, z);
                    match decoders.get(&dataset, (key.level, shard)).await {
                        Ok(d) => Some(d),
                        Err(e) => return TileOutcome::Failed(e),
                    }
                } else {
                    None
                };

                // Fetching a shard index is itself a round trip, so check again
                // before paying for the tile body.
                if !still_wanted(&shared) {
                    return TileOutcome::Cancelled;
                }
                let source = match decoder.as_ref() {
                    Some(decoder) => TileSource::Shard(decoder.as_ref()),
                    None => TileSource::Array,
                };
                match read_tile(&dataset, level, &channels, source, key.ty, key.tx, z).await {
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
}

/// Turn finished tasks into sprites.
pub fn collect_tile_tasks(
    mut commands: Commands,
    mut streamers: Query<&mut TileStreamer>,
    mut images: ResMut<Assets<Image>>,
    sources: Query<&source::DataSource>,
) {
    for mut streamer in &mut streamers {
        let Ok(layer) = sources.get(streamer.source).map(|s| s.layer) else {
            continue;
        };
        let dataset = streamer.dataset.clone();
        let level_count = dataset.levels.len();
        let mut finished = Vec::new();

        for (key, slot) in &mut streamer.slots {
            let SlotState::Loading(task) = &mut slot.state else {
                continue;
            };
            if let Some(outcome) = task.take() {
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
                    let image = tile_texture(pixels);

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
                            RenderLayers::layer(layer),
                            Tile,
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
}

/// Keep the tile cache inside its memory budget by dropping the least recently
/// wanted tiles.
///
/// Tiles are deliberately *not* dropped as soon as they leave the viewport.
/// Zooming in narrows the wanted set to a handful of fine tiles, and the coarse
/// ones covering the surrounding area are exactly what is needed again a moment
/// later when zooming back out. Holding them until memory runs short makes that
/// round trip free.
pub fn evict_tiles(mut commands: Commands, mut streamers: Query<&mut TileStreamer>) {
    for mut streamer in &mut streamers {
        let wanted: HashSet<TileKey> = streamer.wanted.iter().copied().collect();

        // Give up on tiles the view has moved off. A read used to be kept
        // whatever happened, because a blocking one could not be stopped and
        // throwing away the slot would only have lost the answer while still
        // paying for it. Now dropping the slot aborts the read, which for a
        // tile is megabytes of chunk fetched and decoded for a view nobody is
        // looking at any more.
        let mut cancelled = 0usize;
        streamer.slots.retain(|key, slot| {
            if matches!(slot.state, SlotState::Loading(_)) && !wanted.contains(key) {
                cancelled += 1;
                return false;
            }
            true
        });
        streamer.in_flight = streamer.in_flight.saturating_sub(cancelled);
        streamer.cancelled += cancelled;

        // Nothing wanted means the view has left the image. Its reads are
        // abandoned above, but what is already drawn is kept: panning back
        // should not have to fetch it again.
        if wanted.is_empty() {
            continue;
        }

        let candidates: Vec<Candidate> = streamer
            .slots
            .iter()
            .filter(|(key, _)| !streamer.retained.contains(*key))
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
            if let Some(slot) = streamer.slots.remove(&key)
                && let SlotState::Ready { entity, bytes } = slot.state
            {
                commands.entity(entity).despawn();
                streamer.resident_bytes = streamer.resident_bytes.saturating_sub(bytes);
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
            let TileStreamer {
                slots, retained, ..
            } = &mut *streamer;
            slots.retain(|key, slot| {
                slot.state.holds_texture()
                    || retained.contains(key)
                    || slot.last_wanted > cutoff
                    || matches!(slot.state, SlotState::Loading(_))
            });
        }

        let dataset = streamer.dataset.clone();
        let keep: HashSet<ShardKey> = wanted
            .iter()
            .filter(|key| dataset.levels[key.level].sharded)
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
}

/// A tile's pixels as a texture, sampled the way every tile pyramid is drawn.
pub(crate) fn tile_texture(pixels: TilePixels) -> Image {
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
    // Nearest magnification keeps individual pixels crisp past 1:1; linear
    // minification avoids shimmer when zoomed out. Clamping stops neighbouring
    // tiles bleeding across their seams.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..default()
    });
    image
}

/// A resident tile considered for eviction.
pub(crate) struct Candidate<K = TileKey> {
    pub key: K,
    pub last_wanted: u64,
    pub bytes: usize,
}

/// Choose which tiles to drop so that `resident` falls within `budget`,
/// least recently wanted first. Returns nothing while the cache fits.
pub(crate) fn plan_eviction<K>(
    mut candidates: Vec<Candidate<K>>,
    resident: usize,
    budget: usize,
) -> Vec<K> {
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
    streamers: Query<&TileStreamer>,
    mut tiles: Query<&mut Visibility, With<Tile>>,
) {
    // Walked per streamer rather than over every tile in the world, so one
    // image's active level cannot hide another image's tiles.
    for streamer in &streamers {
        let active = streamer.active_level;
        for (key, slot) in &streamer.slots {
            let SlotState::Ready { entity, .. } = slot.state else {
                continue;
            };
            let Ok(mut visibility) = tiles.get_mut(entity) else {
                continue;
            };
            let wanted = if tile_visible(key.level, active) {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *visibility != wanted {
                *visibility = wanted;
            }
        }
    }
}

/// The systems every image shares, registered once however many are open.
///
/// Separate from any one image so that a store opened after the window is up
/// streams through the same systems as one named on the command line.
pub struct ImageSystems;

impl Plugin for ImageSystems {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                follow_slice_stack,
                select_tiles,
                spawn_tile_tasks,
                collect_tile_tasks,
                evict_tiles,
                update_tile_visibility,
                toggle_channels,
                volume::request_volumes,
                volume::collect_volumes,
                report_status,
            )
                .chain()
                .in_set(Stage::Sources),
        )
        .add_systems(Update, resolve_hover.in_set(source::hover::HoverProbing));
    }
}

/// Register an open image as a source, and bind a streamer to it.
pub fn spawn_source(
    world: &mut World,
    dataset: Arc<Dataset>,
    z_slice: Option<u64>,
    budget_bytes: usize,
) -> Entity {
    let (x0, y0, x1, y1) = dataset.world;
    let level = &dataset.levels[0];
    let source = source::register_in(
        world,
        source::SourceInfo {
            name: dataset.name.clone(),
            unit: dataset.unit.clone(),
            detail: format!(
                "OME-Zarr image, {} levels, {} channels",
                dataset.levels.len(),
                dataset.channels.len()
            ),
            stat: format!("{} x {} PX", level.width, level.height),
        },
        SourceExtent {
            // World y is negated so the image reads top-down.
            centre: Vec2::new(f32::midpoint(x0, x1), -(y0 + y1) * 0.5),
            size: Vec2::new((x1 - x0).abs(), (y1 - y0).abs()),
            finest: dataset.levels[0].scale_x as f32 / 8.0,
        },
    );

    // Written from registration so the hover system can go through a query
    // rather than through commands.
    world.entity_mut(source).insert(HoverInfo::default());

    // Advertising the stack is what puts the paging control in the sidebar and
    // gives the frame's keys something to step; a flat image offers neither.
    let depth = dataset.depth();
    let mut stack = SliceStack::new(depth);
    if let Some(named) = z_slice {
        stack.go_to(named);
    }
    if depth > 1 {
        world.entity_mut(source).insert(stack);
    }

    // Offered in 3D only when the metadata puts every slice somewhere, and only
    // when some level of it fits a texture; a stack that is merely paged
    // through never shows the control.
    if let Some((centre, size)) = dataset.volume_extent()
        && let Some(level) =
            dataset.volume_level(volume::VOLUME_VOXEL_BUDGET, volume::MAX_TEXTURE_EDGE)
    {
        source::volume::advertise(world, source, centre, size);
        world
            .entity_mut(source)
            .insert(volume::ImageVolume::new(level));
    }

    let mut streamer = TileStreamer::new(dataset, source);
    // Wherever the stack opened, which is the middle unless something named a
    // slice. Starting the tiles anywhere else would load a slice nobody asked
    // for and then load the right one over it.
    streamer.z_slice = stack.current;
    streamer.budget_bytes = budget_bytes;
    world.entity_mut(source).insert(streamer);
    source
}

/// Report where in the image the pointer is.
///
/// An image has no cells to name, so what it identifies is the place itself:
/// the full-resolution pixel under the pointer, and the tile that covers it at
/// the level this frame is drawing.
fn resolve_hover(
    streamers: Query<&TileStreamer>,
    probes: Query<&HoverProbe>,
    mut infos: Query<&mut HoverInfo>,
) {
    for streamer in &streamers {
        let Ok(mut info) = infos.get_mut(streamer.source) else {
            continue;
        };
        let next = probes
            .get(streamer.source)
            .ok()
            .and_then(|probe| describe(streamer, probe))
            .unwrap_or_default();
        if *info != next {
            *info = next;
        }
    }
}

fn describe(streamer: &TileStreamer, probe: &HoverProbe) -> Option<HoverInfo> {
    let dataset = streamer.dataset();
    let full = dataset.levels.first()?;
    // World y is negated for display; levels are laid out in image order.
    let (x, y) = pixel_in(full, probe.world)?;

    let index = dataset.level_for(probe.units_per_px);
    let level = dataset.levels.get(index)?;
    let (lx, ly) = pixel_in(level, probe.world)?;

    Some(
        HoverInfo::titled(format!("px {x}, {y}"))
            .row("level", format!("{index} of {}", dataset.levels.len() - 1))
            .row(
                "tile",
                format!("{}, {}", lx / level.tile_px, ly / level.tile_px),
            ),
    )
}

/// A display-space point as pixel coordinates in a level, or `None` when it
/// falls outside the image.
fn pixel_in(level: &dataset::Level, world: Vec2) -> Option<(u64, u64)> {
    if level.scale_x <= 0.0 || level.scale_y <= 0.0 {
        return None;
    }
    let x = (f64::from(world.x) - level.origin_x) / level.scale_x;
    let y = (f64::from(-world.y) - level.origin_y) / level.scale_y;
    if x < 0.0 || y < 0.0 || x >= level.width as f64 || y >= level.height as f64 {
        return None;
    }
    Some((x as u64, y as u64))
}

/// Number keys toggle channels. Tiles bake the composite into RGBA, so the
/// visible ones are rebuilt; the shard decoders survive, which keeps the
/// refetch cheap.
fn toggle_channels(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<crate::view::TextEntryFocused>,
    selected: Res<crate::view::SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut streamers: Query<&mut TileStreamer>,
) {
    // A digit typed into a URL is a digit, not a channel.
    if typing.0 {
        return;
    }
    // The selected frame's image and no other. With two images open, a digit
    // that reached both would toggle a channel on the one nobody was looking
    // at, and there would be nothing on screen to say it had happened.
    let Some(source) = crate::view::selected_source(&selected, &panels) else {
        return;
    };
    if let Ok(mut streamer) = streamers.get_mut(source) {
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
        let channel = &streamer.channels[index];
        info!(
            "{} the {} channel",
            if channel.active { "showing" } else { "hiding" },
            channel.label
        );
        streamer.reset(&mut commands);
    }
}

/// Draw whichever slice the source's stack is on.
///
/// The stack is the source's, so the sidebar's control and the frame's keys
/// both write to it without knowing an image is what they are paging through;
/// this is the half that turns that into tiles. Tiles hold one slice each, so
/// moving means loading them again — the chunks behind them usually hold
/// dozens of slices, and the chunk cache is what makes the next one instant.
fn follow_slice_stack(
    mut commands: Commands,
    mut streamers: Query<(&SliceStack, &mut TileStreamer), Changed<SliceStack>>,
) {
    for (stack, mut streamer) in &mut streamers {
        if streamer.z_slice == stack.current {
            continue;
        }
        streamer.z_slice = stack.current;
        streamer.reset(&mut commands);
        info!("image: showing {}", stack.label());
    }
}

fn report_status(
    streamers: Query<&TileStreamer>,
    stacks: Query<&SliceStack>,
    volumes: Query<&volume::ImageVolume>,
    mut sources: Query<&mut SourceStatus>,
) {
    for streamer in &streamers {
        let mut stacked = stacks
            .get(streamer.source)
            .map(|stack| format!("{}, PgUp/PgDn to page\n", stack.label()))
            .unwrap_or_default();
        if let Some(line) = volumes
            .get(streamer.source)
            .ok()
            .and_then(|volume| volume.status(streamer.dataset().depth()))
        {
            stacked.push_str(&line);
            stacked.push('\n');
        }
        let Ok(mut status) = sources.get_mut(streamer.source) else {
            continue;
        };
        let dataset = streamer.dataset();
        let level = &dataset.levels[streamer.active_level];

        let channels = streamer
            .channels
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mark = if c.active { '*' } else { ' ' };
                format!("{}{}:{}", mark, i + 1, c.label)
            })
            .collect::<Vec<_>>()
            .join("  ");

        let mut notes = String::new();
        if streamer.cancelled > 0 {
            notes.push_str(&format!(", {} cancelled", streamer.cancelled));
        }
        let failed = streamer.failed();
        if failed > 0 {
            notes.push_str(&format!(", {failed} failed"));
        }

        status.0 = format!(
            "{stacked}\
         level {}/{}  ({} x {} px, {:.4} {}/px)\n\
         tiles {} cached ({} MB / {} MB), {} loading{}\n\
         channels  {}\n\
         1-9 toggle channel",
            streamer.active_level,
            dataset.levels.len() - 1,
            level.width,
            level.height,
            level.scale_x,
            dataset.unit,
            streamer.loaded(),
            streamer.resident_bytes() / (1024 * 1024),
            streamer.budget_bytes / (1024 * 1024),
            streamer.in_flight,
            notes,
            channels,
        );
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
        assert!(plan_eviction::<TileKey>(Vec::new(), 500, 100).is_empty());
    }

    #[test]
    fn the_fetch_pool_is_large_enough_to_matter() {
        // Bevy's default async-compute pool caps at 4 threads, and one blocking
        // tile read occupies a thread for its whole duration.
        const {
            assert!(
                TILE_FETCH_THREADS > 4,
                "a pool this small would serialise tile loading"
            );
        };
        assert_eq!(MAX_IN_FLIGHT, TILE_FETCH_THREADS);
    }

    #[test]
    fn nothing_off_screen_is_requested_while_a_visible_tile_is_loading() {
        // One visible tile outstanding is enough to hold back every prefetch:
        // even the last to be cut off would have shared its bandwidth.
        let visible = [key(3, 0), key(0, 0), key(0, 1)];
        let prefetch = [key(0, 2), key(1, 0)];
        let wanted = request_order(&visible, &prefetch, |k| *k != key(0, 1));
        assert_eq!(wanted, visible);
    }

    #[test]
    fn prefetch_follows_once_every_visible_tile_has_landed() {
        let visible = [key(3, 0), key(0, 0)];
        let prefetch = [key(0, 2), key(1, 0)];
        let wanted = request_order(&visible, &prefetch, |_| true);
        assert_eq!(wanted, [key(3, 0), key(0, 0), key(0, 2), key(1, 0)]);
    }

    #[test]
    fn a_tile_named_twice_is_requested_once_where_first_named() {
        let mut tiers = Tiers::default();
        tiers.extend([key(3, 0), key(0, 0)]);
        tiers.extend([key(0, 0), key(0, 1)]);
        assert_eq!(tiers.order, [key(3, 0), key(0, 0), key(0, 1)]);

        // Nor is a visible tile named again as a prefetch.
        let mut prefetch = tiers.followed_by();
        prefetch.extend([key(0, 1), key(1, 0)]);
        assert_eq!(prefetch.order, [key(1, 0)]);
        assert_eq!(prefetch.seen.len(), 4);
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
