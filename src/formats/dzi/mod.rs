//! Deep Zoom images: the tile pyramid OpenSeadragon reads, and the shape
//! pathology slides are commonly published in.
//!
//! Streamed through [`crate::formats::tiles`], as an OME-Zarr image is, but
//! each tile is a JPEG or PNG of its own rather than a region cut from an
//! array, so there is no store to open and nothing to composite.

pub mod pyramid;

use std::sync::Arc;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::app::net::fetching;
use crate::app::schedule::Stage;
use crate::formats::tiles::{self, SlotState, TileCache, View};
use crate::source::hover::{HoverInfo, HoverProbe};
use crate::source::{self, ShowsSource, SourceBusy, SourceExtent, SourceStatus};
use pyramid::{DeepZoom, TilePixels};

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

#[derive(Component)]
pub struct DziStreamer {
    pub source: Entity,
    dzi: Arc<DeepZoom>,
    tiles: TileCache<TileKey, Result<TilePixels, String>>,
    /// The finest level any frame showing this image is drawing.
    active_level: u32,
}

/// Work out what every frame showing an image can see, and want the tiles
/// that cover it: visible ones first, and nothing else until they have landed.
fn select_tiles(
    mut streamers: Query<&mut DziStreamer>,
    panels: Query<(&Camera, &GlobalTransform, &Projection, &ShowsSource)>,
) {
    for mut streamer in &mut streamers {
        let dzi = streamer.dzi.clone();
        let (coarsest, finest) = (dzi.min_level(), dzi.max_level());
        let views: Vec<(View, u32)> = tiles::frame_views(&panels, streamer.source)
            .into_iter()
            .map(|(view, units_per_px)| (view, dzi.level_for(units_per_px)))
            .collect();
        streamer.active_level = views
            .iter()
            .map(|(_, level)| *level)
            .fold(coarsest, u32::max);
        let beside = |level: u32| {
            [
                (level > coarsest).then(|| level - 1),
                (level < finest).then(|| level + 1),
            ]
        };
        let tiers = tiles::tiers(&views, coarsest, beside, |level, view, half| {
            tiles_over(&dzi, level, view, half)
        });
        streamer.tiles.want(tiers);
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
        let DziStreamer { dzi, tiles, .. } = &mut *streamer;
        tiles.start(|key| {
            let url = dzi.tile_url(key.level, key.column, key.row);
            fetching(async move { pyramid::read_tile(&url).await })
        });
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
        for (key, outcome) in streamer.tiles.finished() {
            let rect = dzi.tile_rect(key.level, key.column, key.row);
            let state = match (outcome, rect) {
                (Ok(pixels), Some((x0, y0, x1, y1))) => {
                    let bytes = pixels.rgba.len();
                    let entity = commands
                        .spawn((
                            Sprite {
                                image: images.add(pyramid::tile_texture(pixels)),
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
                    SlotState::Ready {
                        entity,
                        bytes,
                        material: (),
                    }
                }
                (Ok(_), None) => SlotState::Blank,
                (Err(e), _) => {
                    warn!("deep zoom tile {key:?}: {e}");
                    SlotState::Failed
                }
            };
            streamer.tiles.settle(key, state);
        }
    }
}

fn evict_tiles(mut commands: Commands, mut streamers: Query<&mut DziStreamer>) {
    for mut streamer in &mut streamers {
        streamer.tiles.evict(&mut commands);
    }
}

/// Hide resident tiles finer than the level in use, which would otherwise
/// paint over it and shimmer as they minify.
fn update_tile_visibility(
    streamers: Query<&DziStreamer>,
    mut tiles: Query<&mut Visibility, With<DziTile>>,
) {
    for streamer in &streamers {
        let active = streamer.active_level;
        streamer.tiles.show(&mut tiles, |key| key.level <= active);
    }
}

fn report_status(
    streamers: Query<&DziStreamer>,
    mut sources: Query<&mut SourceStatus>,
    mut busy: Query<&mut SourceBusy>,
) {
    for streamer in &streamers {
        if let Ok(mut busy) = busy.get_mut(streamer.source) {
            busy.set_if_neq(SourceBusy(streamer.tiles.busy()));
        }
        let Ok(mut status) = sources.get_mut(streamer.source) else {
            continue;
        };
        let dzi = &streamer.dzi;
        let level = streamer.active_level;
        let (width, height) = dzi.level_size(level);
        status.0 = format!(
            "level {level}/{}  ({width} x {height} px, {} px/px)\n{}",
            dzi.max_level(),
            dzi.scale(level),
            streamer.tiles.status(),
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
        DziStreamer {
            source,
            active_level: dzi.min_level(),
            dzi,
            tiles: TileCache::new(budget_bytes),
        },
    ));
    source
}
