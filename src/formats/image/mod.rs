//! OME-Zarr images, streamed a tile at a time.
//!
//! Which tiles are wanted, in what order, and how long they are kept is
//! [`crate::formats::tiles`], shared with Deep Zoom. What is particular to
//! Zarr is here: reading a tile out of a shard or a run of chunks, mixing its
//! channels on the GPU, and paging through a stack of slices.

pub mod blocks;
pub mod dataset;
pub mod store;
pub mod volume;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::TextureDimension;
use zarrs_codec::AsyncArrayPartialDecoderTraits;

use crate::app::net::fetching;
use crate::app::schedule::Stage;
use crate::formats::image::dataset::{Channel, ChannelSamples, Dataset, TileSource, read_tile};
use crate::formats::tiles::{self, SlotState, TileCache, View};
use crate::render::channels::{ChannelTileMaterial, MixChannel, channel_texture};
use crate::source::channels::{ChannelSetting, SourceChannels};
use crate::source::hover::{HoverInfo, HoverProbe};
use crate::source::stack::SliceStack;
use crate::source::{self, SourceBusy, SourceExtent, SourceStatus};
use crate::view::ShowsSource;

/// Cached shard decoders. Each holds a shard index, so reusing one saves both a
/// round trip and ~16 KB per tile.
const MAX_CACHED_SHARDS: usize = 96;

/// How much texture memory resident tiles may occupy before the least recently
/// wanted ones are dropped.
pub const DEFAULT_CACHE_BUDGET_MB: usize = 256;

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

enum TileOutcome {
    Ready(ChannelSamples),
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
    /// Resident tiles keep the material their channels are mixed in, so a
    /// change of channels rewrites it rather than reading the tile again.
    tiles: TileCache<TileKey, TileOutcome, Handle<ChannelTileMaterial>>,
    pub z_slice: u64,
    pub active_level: usize,
}

impl TileStreamer {
    pub fn new(dataset: Arc<Dataset>, source: Entity) -> Self {
        TileStreamer {
            source,
            channels: dataset.channels.clone(),
            dataset,
            decoders: Arc::new(DecoderCache::default()),
            wanted_shared: Arc::new(RwLock::new(HashSet::new())),
            tiles: TileCache::new(DEFAULT_CACHE_BUDGET_MB * 1024 * 1024),
            z_slice: 0,
            active_level: 0,
        }
    }

    pub fn dataset(&self) -> &Arc<Dataset> {
        &self.dataset
    }

    /// The channels as the shaders mix them: windows over the intensities as
    /// they are stored, which is divided down by the dataset's sample scale.
    pub fn mix_channels(&self) -> Vec<MixChannel> {
        let scale = self.dataset.sample_scale;
        self.channels
            .iter()
            .map(|channel| MixChannel {
                color: channel.color,
                window: (channel.start / scale, channel.end / scale),
                shown: channel.active,
            })
            .collect()
    }

    /// Drop every tile and start over, e.g. after changing channels.
    pub fn reset(&mut self, commands: &mut Commands) {
        self.tiles.clear(commands);
    }
}

/// Work out what each frame shows and queue the tiles that cover it: visible
/// ones first, and nothing else until they have all landed.
pub fn select_tiles(
    mut streamers: Query<&mut TileStreamer>,
    panels: Query<(&Camera, &GlobalTransform, &Projection, &ShowsSource)>,
) {
    for mut streamer in &mut streamers {
        let dataset = streamer.dataset.clone();
        let coarsest = dataset.levels.len().saturating_sub(1);
        let views: Vec<(View, usize)> = tiles::frame_views(&panels, streamer.source)
            .into_iter()
            .map(|(view, units_per_px)| (view, dataset.level_for(units_per_px)))
            .collect();
        // The finest level any panel is asking for. Visibility is driven from this
        // so that a tile one panel needs is never hidden on behalf of another.
        streamer.active_level = views
            .iter()
            .map(|(_, level)| *level)
            .fold(coarsest, usize::min);
        let beside = |level: usize| [(level < coarsest).then(|| level + 1), level.checked_sub(1)];
        let tiers = tiles::tiers(&views, coarsest, beside, |level, view, half| {
            tiles_over(&dataset, level, view, half)
        });
        let wanted = streamer.tiles.want(tiers).to_vec();

        // Publish for the workers, so queued tasks can check whether they still
        // matter before doing any network work.
        if let Ok(mut shared) = streamer.wanted_shared.write() {
            shared.clear();
            shared.extend(wanted);
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

/// Start tasks for wanted tiles that are not loaded yet.
pub fn spawn_tile_tasks(mut streamers: Query<&mut TileStreamer>) {
    for mut streamer in &mut streamers {
        let dataset = streamer.dataset.clone();
        let decoders = streamer.decoders.clone();
        let z = streamer.z_slice;

        let shared = streamer.wanted_shared.clone();
        streamer.tiles.start(|key| {
            let dataset = dataset.clone();
            let decoders = decoders.clone();
            let shared = shared.clone();
            fetching(async move {
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
                match read_tile(&dataset, level, source, key.ty, key.tx, z).await {
                    Ok(Some(pixels)) => TileOutcome::Ready(pixels),
                    Ok(None) => TileOutcome::Blank,
                    Err(e) => TileOutcome::Failed(e),
                }
            })
        });
    }
}

/// Turn finished tasks into sprites.
pub fn collect_tile_tasks(
    mut commands: Commands,
    mut streamers: Query<&mut TileStreamer>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ChannelTileMaterial>>,
    sources: Query<&source::DataSource>,
) {
    for mut streamer in &mut streamers {
        let mix = crate::render::channels::ChannelMix::of(&streamer.mix_channels());
        let Ok(layer) = sources.get(streamer.source).map(|s| s.layer) else {
            continue;
        };
        let dataset = streamer.dataset.clone();
        let level_count = dataset.levels.len();

        for (key, outcome) in streamer.tiles.finished() {
            let level = &dataset.levels[key.level];
            let state = match outcome {
                // It did no work, and forgetting it lets it be requested again
                // if the view comes back.
                TileOutcome::Cancelled => {
                    streamer.tiles.forget(key);
                    continue;
                }
                TileOutcome::Ready(pixels) => {
                    let Some((x0, y0, x1, y1)) = level.tile_world_rect(key.ty, key.tx) else {
                        streamer.tiles.settle(key, SlotState::Blank);
                        continue;
                    };
                    let bytes = pixels.data.len();
                    let texture = images.add(channel_texture(
                        pixels.width,
                        pixels.height,
                        pixels.layers,
                        pixels.data,
                        TextureDimension::D2,
                    ));
                    let material = materials.add(ChannelTileMaterial {
                        mix,
                        samples: texture,
                    });

                    // Finer levels sit on top of coarser ones.
                    let z = (level_count - key.level) as f32;
                    let size = Vec2::new(x1 - x0, y1 - y0);
                    let entity = commands
                        .spawn((
                            Mesh2d(meshes.add(Rectangle::from_size(size))),
                            MeshMaterial2d(material.clone()),
                            // Placed by its middle; the image's y runs down.
                            Transform::from_xyz(x0 + size.x * 0.5, -(y0 + size.y * 0.5), z),
                            RenderLayers::layer(layer),
                            Tile,
                        ))
                        .id();
                    SlotState::Ready {
                        entity,
                        bytes,
                        material,
                    }
                }
                TileOutcome::Blank => SlotState::Blank,
                TileOutcome::Failed(e) => {
                    warn!("tile {:?}: {e}", key);
                    SlotState::Failed
                }
            };
            streamer.tiles.settle(key, state);
        }
    }
}

/// Keep the tile cache inside its memory budget, and the shard decoders to
/// those the wanted tiles are cut from.
pub fn evict_tiles(mut commands: Commands, mut streamers: Query<&mut TileStreamer>) {
    for mut streamer in &mut streamers {
        streamer.tiles.evict(&mut commands);

        let dataset = streamer.dataset.clone();
        let keep: HashSet<ShardKey> = streamer
            .tiles
            .wanted()
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
    for streamer in &streamers {
        let active = streamer.active_level;
        streamer
            .tiles
            .show(&mut tiles, |key| tile_visible(key.level, active));
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
                apply_channels,
                volume::request_volumes,
                volume::request_detail,
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
    // rather than through commands. The channels are what the sidebar offers
    // controls for, as the dataset publishes them.
    world.entity_mut(source).insert((
        HoverInfo::default(),
        SourceChannels::new(
            dataset
                .channels
                .iter()
                .map(|c| ChannelSetting::new(c.label.clone(), c.color, c.active))
                .collect(),
        ),
    ));

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
        && let Some(whole) =
            dataset.whole_volume(volume::VOLUME_VOXEL_BUDGET, volume::MAX_TEXTURE_EDGE)
    {
        source::volume::advertise(world, source, centre, size);
        world
            .entity_mut(source)
            .insert(volume::ImageVolume::new(whole));
    }

    let mut streamer = TileStreamer::new(dataset, source);
    // Wherever the stack opened, which is the middle unless something named a
    // slice. Starting the tiles anywhere else would load a slice nobody asked
    // for and then load the right one over it.
    streamer.z_slice = stack.current;
    streamer.tiles.budget_bytes = budget_bytes;
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

/// Number keys toggle channels.
///
/// Written to the source's channel settings, the same place the sidebar's
/// checkboxes write, so the two always agree; [`apply_channels`] turns either
/// into what is drawn.
fn toggle_channels(
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<crate::view::TextEntryFocused>,
    selected: Res<crate::view::SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut SourceChannels, With<TileStreamer>>,
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
    let Some(index) = DIGITS.iter().position(|key| keys.just_pressed(*key)) else {
        return;
    };
    if let Ok(mut channels) = sources.get_mut(source)
        && let Some(shown) = channels.toggle(index)
    {
        info!(
            "{} the {} channel",
            if shown { "showing" } else { "hiding" },
            channels.channels[index].label
        );
    }
}

/// The channels to composite with: the dataset's own, as `settings` show them.
fn composite_channels(published: &[Channel], settings: &[ChannelSetting]) -> Vec<Channel> {
    published
        .iter()
        .enumerate()
        .map(|(index, channel)| {
            let mut channel = channel.clone();
            if let Some(setting) = settings.get(index) {
                channel.active = setting.contributes();
                (channel.start, channel.end) = setting.window(channel.start, channel.end);
            }
            channel
        })
        .collect()
}

/// Mix tiles and the volume with the source's channel settings.
///
/// Instant: tiles keep every channel's intensity and the shader mixes them,
/// so a change of channels is a change of uniform on each resident tile and
/// nothing is read again. A slider can be dragged and watched.
fn apply_channels(
    mut streamers: Query<(
        Ref<SourceChannels>,
        &mut TileStreamer,
        Option<&volume::ImageVolume>,
    )>,
    mut tiles: ResMut<Assets<ChannelTileMaterial>>,
    mut volumes: ResMut<Assets<crate::render::volume::VolumeMaterial>>,
) {
    for (settings, mut streamer, volume) in &mut streamers {
        if !settings.is_changed() {
            continue;
        }
        let wanted = composite_channels(&streamer.dataset.channels, &settings.channels);
        if wanted == streamer.channels {
            continue;
        }
        streamer.channels = wanted;
        let mix = streamer.mix_channels();
        for (_, _, material) in streamer.tiles.ready() {
            if let Some(mut material) = tiles.get_mut(material) {
                material.mix.set_channels(&mix);
            }
        }
        if let Some(material) = volume
            .and_then(volume::ImageVolume::material)
            .and_then(|handle| volumes.get_mut(handle))
        {
            let mut material = material;
            material.mix.set_channels(&mix);
        }
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
    mut busy: Query<&mut SourceBusy>,
) {
    for streamer in &streamers {
        let reading = volumes
            .get(streamer.source)
            .is_ok_and(volume::ImageVolume::is_reading);
        if let Ok(mut busy) = busy.get_mut(streamer.source) {
            busy.set_if_neq(SourceBusy(streamer.tiles.busy() || reading));
        }
        let mut stacked = stacks
            .get(streamer.source)
            .map(|stack| format!("{}\n", stack.label()))
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

        status.0 = format!(
            "{stacked}\
         level {}/{}  ({} x {} px, {:.4} {}/px)\n\
         {}",
            streamer.active_level,
            dataset.levels.len() - 1,
            level.width,
            level.height,
            level.scale_x,
            dataset.unit,
            streamer.tiles.status(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn published() -> Vec<Channel> {
        ["red", "green"]
            .into_iter()
            .map(|label| Channel {
                label: label.into(),
                color: [1.0; 3],
                start: 0.0,
                end: 1000.0,
                active: true,
            })
            .collect()
    }

    #[test]
    fn settings_reach_the_composite_through_the_published_window() {
        let mut settings: Vec<ChannelSetting> = published()
            .iter()
            .map(|c| ChannelSetting::new(c.label.clone(), c.color, true))
            .collect();
        // As published, nothing changes, so nothing is read again.
        assert_eq!(composite_channels(&published(), &settings), published());

        settings[0].gain = 2.0;
        settings[1].shown = false;
        let channels = composite_channels(&published(), &settings);
        assert_eq!(channels[0].end, 500.0);
        assert!(!channels[1].active);
        // Brightness is always measured from the published window, so turning
        // one up and back down lands where it started.
        settings[0].gain = 1.0;
        assert_eq!(composite_channels(&published(), &settings)[0].end, 1000.0);
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
