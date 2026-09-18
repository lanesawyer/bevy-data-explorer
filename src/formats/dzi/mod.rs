//! Deep Zoom images: the tile pyramid OpenSeadragon reads, and the shape
//! pathology slides are commonly published in.
//!
//! Streamed the way an OME-Zarr image is — the overview tile and the level
//! matching the zoom, a margin and the neighbouring levels only once those have
//! landed, a least-recently-wanted cache under the same memory budget — but
//! each tile is a JPEG or PNG of its own
//! rather than a region cut from an array, so there is no store to open and
//! nothing to composite.

pub mod pyramid;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::app::net::{Fetching, fetching};
use crate::app::schedule::Stage;
use crate::formats::image::dataset::TilePixels;
use crate::formats::image::{Candidate, Tiers, View, plan_eviction, request_order, tile_texture};
use crate::source::hover::{HoverInfo, HoverProbe};
use crate::source::{self, SourceBusy, SourceExtent, SourceStatus};
use crate::view::ShowsSource;
use pyramid::DeepZoom;

/// Tile requests in flight at once, per image.
///
/// The same allowance an OME-Zarr image gets. A tile here is one small object
/// rather than a range out of a shard, so what this bounds is connections to
/// one host, and a request abandoned before it starts costs nothing.
const MAX_IN_FLIGHT: usize = crate::formats::image::TILE_FETCH_THREADS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TileKey {
    /// Deep Zoom's own numbering: higher is finer.
    level: u32,
    column: u64,
    row: u64,
}

/// Marks a spawned tile sprite.
#[derive(Component)]
pub struct DziTile;

struct Slot {
    state: SlotState,
    last_wanted: u64,
}

enum SlotState {
    Loading(Fetching<Result<TilePixels, String>>),
    Ready { entity: Entity, bytes: usize },
    Failed,
}

#[derive(Component)]
pub struct DziStreamer {
    pub source: Entity,
    dzi: Arc<DeepZoom>,
    slots: HashMap<TileKey, Slot>,
    /// Tiles to request, in order: visible ones, then prefetch once idle.
    wanted: Vec<TileKey>,
    /// Tiles worth keeping resident whether or not they are being requested.
    retained: HashSet<TileKey>,
    /// The finest level any frame showing this image is drawing.
    active_level: u32,
    in_flight: usize,
    frame: u64,
    resident_bytes: usize,
    budget_bytes: usize,
    cancelled: usize,
}

impl DziStreamer {
    fn new(dzi: Arc<DeepZoom>, source: Entity, budget_bytes: usize) -> Self {
        DziStreamer {
            source,
            active_level: dzi.min_level(),
            dzi,
            slots: HashMap::new(),
            wanted: Vec::new(),
            retained: HashSet::new(),
            in_flight: 0,
            frame: 0,
            resident_bytes: 0,
            budget_bytes,
            cancelled: 0,
        }
    }

    fn count(&self, matches: impl Fn(&SlotState) -> bool) -> usize {
        self.slots
            .values()
            .filter(|slot| matches(&slot.state))
            .count()
    }
}

/// Work out what every frame showing an image can see, and want the tiles
/// that cover it: visible ones first, and nothing else until they have landed.
fn select_tiles(
    mut streamers: Query<&mut DziStreamer>,
    panels: Query<(&Camera, &GlobalTransform, &Projection, &ShowsSource)>,
) {
    for mut streamer in &mut streamers {
        let dzi = streamer.dzi.clone();
        let source = streamer.source;
        let (coarsest, finest) = (dzi.min_level(), dzi.max_level());
        let mut active = coarsest;

        // The union over every frame showing this image, so a duplicate zoomed
        // somewhere else streams its own detail.
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
            let level = dzi.level_for(ortho.area.width() / viewport.x.max(1.0));
            active = active.max(level);
            views.push((View::new(transform, ortho), level));
        }

        let mut visible = Tiers::default();
        // The single overview tile, so a cold open shows the slide at once.
        for (view, _) in &views {
            visible.extend(tiles_over(&dzi, coarsest, view, view.half));
        }
        for (view, level) in &views {
            visible.extend(tiles_over(&dzi, *level, view, view.half));
        }
        let mut prefetch = visible.followed_by();
        for (view, level) in &views {
            prefetch.extend(tiles_over(&dzi, *level, view, view.half + view.margin));
            if *level > coarsest {
                prefetch.extend(tiles_over(&dzi, level - 1, view, view.half));
            }
            if *level < finest {
                prefetch.extend(tiles_over(&dzi, level + 1, view, view.half));
            }
        }

        streamer.active_level = active;
        streamer.frame = streamer.frame.wrapping_add(1);
        let frame = streamer.frame;
        for key in &prefetch.seen {
            if let Some(slot) = streamer.slots.get_mut(key) {
                slot.last_wanted = frame;
            }
        }

        streamer.wanted = request_order(&visible.order, &prefetch.order, |key| {
            streamer
                .slots
                .get(key)
                .is_some_and(|slot| !matches!(slot.state, SlotState::Loading(_)))
        });
        streamer.retained = prefetch.seen;
    }
}

/// The tiles of one level within `half` of a view's centre, nearest the middle
/// first.
fn tiles_over(dzi: &DeepZoom, level: u32, view: &View, half: Vec2) -> Vec<TileKey> {
    // Display y is negated; the pyramid is laid out top-down.
    let (x0, x1) = (view.centre.x - half.x, view.centre.x + half.x);
    let (y0, y1) = (-(view.centre.y + half.y), -(view.centre.y - half.y));
    let span = (dzi.tile_size * dzi.scale(level)) as f32;
    let (columns, rows) = dzi.tile_count(level);
    let first = |at: f32| (at / span).floor().max(0.0) as u64;
    let last = |at: f32, count: u64| ((at / span).ceil().max(0.0) as u64).min(count);

    let mut tiles = Vec::new();
    for row in first(y0)..last(y1, rows) {
        for column in first(x0)..last(x1, columns) {
            let middle = Vec2::new((column as f32 + 0.5) * span, -(row as f32 + 0.5) * span);
            let key = TileKey { level, column, row };
            tiles.push((middle.distance_squared(view.centre) as u64, key));
        }
    }
    tiles.sort_unstable_by_key(|(distance, _)| *distance);
    tiles.into_iter().map(|(_, key)| key).collect()
}

fn spawn_tile_tasks(mut streamers: Query<&mut DziStreamer>) {
    for mut streamer in &mut streamers {
        let wanted = std::mem::take(&mut streamer.wanted);
        for key in &wanted {
            if streamer.in_flight >= MAX_IN_FLIGHT {
                break;
            }
            if streamer.slots.contains_key(key) {
                continue;
            }
            let url = streamer.dzi.tile_url(key.level, key.column, key.row);
            let task = fetching(async move { pyramid::read_tile(&url).await });
            let frame = streamer.frame;
            streamer.slots.insert(
                *key,
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

fn collect_tiles(
    mut commands: Commands,
    mut streamers: Query<&mut DziStreamer>,
    mut images: ResMut<Assets<Image>>,
    sources: Query<&source::DataSource>,
) {
    for mut streamer in &mut streamers {
        let Ok(layer) = sources.get(streamer.source).map(|s| s.layer) else {
            continue;
        };
        let dzi = streamer.dzi.clone();

        let mut finished = Vec::new();
        for (key, slot) in &mut streamer.slots {
            if let SlotState::Loading(task) = &mut slot.state
                && let Some(outcome) = task.take()
            {
                finished.push((*key, outcome));
            }
        }

        for (key, outcome) in finished {
            streamer.in_flight = streamer.in_flight.saturating_sub(1);
            let rect = dzi.tile_rect(key.level, key.column, key.row);
            let state = match (outcome, rect) {
                (Ok(pixels), Some((x0, y0, x1, y1))) => {
                    let bytes = pixels.rgba.len();
                    let entity = commands
                        .spawn((
                            Sprite {
                                image: images.add(tile_texture(pixels)),
                                custom_size: Some(Vec2::new(x1 - x0, y1 - y0)),
                                ..default()
                            },
                            Anchor::TOP_LEFT,
                            // Finer levels sit on top of coarser ones.
                            Transform::from_xyz(x0, -y0, key.level as f32),
                            RenderLayers::layer(layer),
                            DziTile,
                        ))
                        .id();
                    streamer.resident_bytes += bytes;
                    SlotState::Ready { entity, bytes }
                }
                (Ok(_), None) => SlotState::Failed,
                (Err(e), _) => {
                    warn!("deep zoom tile {key:?}: {e}");
                    SlotState::Failed
                }
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

/// Abandon reads the view has moved off, and keep resident tiles inside the
/// budget, least recently wanted first.
fn evict_tiles(mut commands: Commands, mut streamers: Query<&mut DziStreamer>) {
    for mut streamer in &mut streamers {
        let wanted: HashSet<TileKey> = streamer.wanted.iter().copied().collect();

        let mut cancelled = 0usize;
        streamer.slots.retain(|key, slot| {
            let abandon = matches!(slot.state, SlotState::Loading(_)) && !wanted.contains(key);
            cancelled += usize::from(abandon);
            !abandon
        });
        streamer.in_flight = streamer.in_flight.saturating_sub(cancelled);
        streamer.cancelled += cancelled;

        // Nothing wanted means the view has left the image: keep what is drawn
        // for when it comes back.
        if wanted.is_empty() {
            continue;
        }

        let candidates = streamer
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
    }
}

/// Hide resident tiles finer than the level in use, which would otherwise
/// paint over it and shimmer as they minify.
fn update_tile_visibility(
    streamers: Query<&DziStreamer>,
    mut tiles: Query<&mut Visibility, With<DziTile>>,
) {
    for streamer in &streamers {
        for (key, slot) in &streamer.slots {
            let SlotState::Ready { entity, .. } = slot.state else {
                continue;
            };
            let Ok(mut visibility) = tiles.get_mut(entity) else {
                continue;
            };
            let wanted = if key.level <= streamer.active_level {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            visibility.set_if_neq(wanted);
        }
    }
}

fn report_status(
    streamers: Query<&DziStreamer>,
    mut sources: Query<&mut SourceStatus>,
    mut busy: Query<&mut SourceBusy>,
) {
    for streamer in &streamers {
        if let Ok(mut busy) = busy.get_mut(streamer.source) {
            busy.set_if_neq(SourceBusy(streamer.in_flight > 0));
        }
        let Ok(mut status) = sources.get_mut(streamer.source) else {
            continue;
        };
        let dzi = &streamer.dzi;
        let level = streamer.active_level;
        let (width, height) = dzi.level_size(level);

        let mut notes = String::new();
        if streamer.cancelled > 0 {
            notes += &format!(", {} cancelled", streamer.cancelled);
        }
        let failed = streamer.count(|state| matches!(state, SlotState::Failed));
        if failed > 0 {
            notes += &format!(", {failed} failed");
        }

        status.0 = format!(
            "level {level}/{}  ({width} x {height} px, {} px/px)\n\
             tiles {} cached ({} MB / {} MB), {} loading{notes}",
            dzi.max_level(),
            dzi.scale(level),
            streamer.count(|state| matches!(state, SlotState::Ready { .. })),
            streamer.resident_bytes / (1024 * 1024),
            streamer.budget_bytes / (1024 * 1024),
            streamer.in_flight,
        );
    }
}

/// Name the full-resolution pixel under the pointer, and the tile covering it
/// at the level that frame is drawing.
fn resolve_hover(
    streamers: Query<&DziStreamer>,
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
            .and_then(|probe| describe(&streamer.dzi, probe))
            .unwrap_or_default();
        info.set_if_neq(next);
    }
}

fn describe(dzi: &DeepZoom, probe: &HoverProbe) -> Option<HoverInfo> {
    let (x, y) = (probe.world.x, -probe.world.y);
    if x < 0.0 || y < 0.0 || x >= dzi.width as f32 || y >= dzi.height as f32 {
        return None;
    }
    let level = dzi.level_for(probe.units_per_px);
    let span = (dzi.tile_size * dzi.scale(level)) as f32;
    Some(
        HoverInfo::titled(format!("px {}, {}", x as u64, y as u64))
            .row("level", format!("{level} of {}", dzi.max_level()))
            .row(
                "tile",
                format!("{}, {}", (x / span) as u64, (y / span) as u64),
            ),
    )
}

/// The systems every Deep Zoom image shares, registered once however many are
/// open.
pub struct DziSystems;

impl Plugin for DziSystems {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                select_tiles,
                spawn_tile_tasks,
                collect_tiles,
                evict_tiles,
                update_tile_visibility,
                report_status,
            )
                .chain()
                .in_set(Stage::Sources),
        )
        .add_systems(Update, resolve_hover.in_set(source::hover::HoverProbing));
    }
}

/// Register an open Deep Zoom image as a source, and bind a streamer to it.
pub fn spawn_source(world: &mut World, dzi: Arc<DeepZoom>, budget_bytes: usize) -> Entity {
    let (width, height) = (dzi.width as f32, dzi.height as f32);
    let source = source::register_in(
        world,
        source::SourceInfo {
            name: dzi.name.clone(),
            unit: "px".into(),
            detail: format!(
                "Deep Zoom image, {} levels, {} tiles",
                dzi.max_level() + 1,
                dzi.format.to_ascii_uppercase()
            ),
            stat: format!("{} x {} PX", dzi.width, dzi.height),
        },
        SourceExtent {
            centre: Vec2::new(width * 0.5, -height * 0.5),
            size: Vec2::new(width, height),
            finest: 1.0 / 8.0,
        },
    );
    world.entity_mut(source).insert((
        HoverInfo::default(),
        DziStreamer::new(dzi, source, budget_bytes),
    ));
    source
}
