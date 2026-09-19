//! Putting a [`Bookmark`] back on screen.
//!
//! Restoring goes through the same doors opening a dataset by hand does:
//! each address is recognised by `discover` and registered with
//! `spawn_discovered`, and a dataset already open is reused rather than read
//! again. The reads run at once rather than one after another, and the frames
//! are laid out only when every one has landed or failed, so a slow dataset
//! cannot leave the grid half built and then reshuffle it.
//!
//! What a dataset was showing is put back as it becomes possible, which is not
//! all at once: a slice can be set as soon as the source exists, but cell
//! filters have to wait for the catalog's service to replace the format's
//! placeholder properties, or the replacement would throw them away.

use std::sync::atomic::{AtomicU64, Ordering};

use bevy::prelude::*;

use super::BookmarkNotice;
use super::capture::saved_address;
use super::snapshot::{
    Bookmark, SourceState, apply_cells, apply_channels, apply_slice, clamp_point_size,
};
use crate::app::net::{Fetching, fetching};
use crate::app::theme::Palette;
use crate::catalog::cells::Described;
use crate::formats::discover::{self, Discovered};
use crate::formats::{LoadSettings, spawn_discovered};
use crate::render::points::SourcePointSize;
use crate::render::settings::SourceOpacity;
use crate::source::channels::SourceChannels;
use crate::source::properties::{CellColumns, CellProperties, PropertyState};
use crate::source::stack::{SliceGrid, SliceStack};
use crate::source::volume::SourceVolume;
use crate::source::{DataSource, SourceExtent, SourceUrl};
use crate::view::grid::{MAX_LAYERS, MAX_PANELS};
use crate::view::layers::spawn_layer;
use crate::view::{FrameArea, LayerOpacity, Orbit, Panel, SelectedPanel, View, spawn_panel};

/// How long a dataset's cell settings wait for its properties before being
/// given up on. A service that has not answered by then is not going to.
const CELLS_PATIENCE_SECS: f32 = 120.0;

/// A bookmark being restored.
#[derive(Resource)]
pub struct Restoring {
    /// Tells a registration finishing late which restore it belonged to, so
    /// one replaced by a newer restore writes into nothing.
    id: u64,
    bookmark: Bookmark,
    slots: Vec<Slot>,
    begun: bool,
}

enum Slot {
    Reading(Fetching<Result<Discovered, String>>),
    /// Read, and queued to be registered as a source.
    Registering,
    Open(Entity),
    Failed(String),
}

impl Restoring {
    pub fn new(bookmark: Bookmark) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Restoring {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            bookmark,
            slots: Vec::new(),
            begun: false,
        }
    }

    fn open(&self, index: usize) -> Option<Entity> {
        match self.slots.get(index) {
            Some(Slot::Open(entity)) => Some(*entity),
            _ => None,
        }
    }
}

/// What a source still has to have put back.
#[derive(Component)]
pub struct PendingSettings {
    state: SourceState,
    /// When it was asked for, in seconds since startup.
    since: f32,
}

/// Open the bookmark's datasets, and lay out its frames once they are open.
pub fn drive_restore(
    mut commands: Commands,
    restoring: Option<ResMut<Restoring>>,
    time: Res<Time>,
    opened: Query<(Entity, &SourceUrl)>,
    panels: Query<Entity, With<Panel>>,
    sources: Query<(&DataSource, &SourceExtent, Option<&SourceVolume>)>,
    area: Res<FrameArea>,
    palette: Res<Palette>,
    mut selected: ResMut<SelectedPanel>,
    mut notice: ResMut<BookmarkNotice>,
) {
    let Some(mut restoring) = restoring else {
        return;
    };
    let now = time.elapsed_secs();

    if !restoring.begun {
        restoring.begun = true;
        info!("restoring bookmark {}", restoring.bookmark.name);
        *notice = BookmarkNotice::Working(format!("opening {}\u{2026}", restoring.bookmark.name));
        let open: Vec<(Entity, String)> = opened
            .iter()
            .map(|(entity, url)| (entity, saved_address(&url.0)))
            .collect();
        let slots = restoring
            .bookmark
            .sources
            .iter()
            .map(
                |state| match open.iter().find(|(_, url)| *url == state.url) {
                    Some((entity, _)) => {
                        commands.entity(*entity).insert(PendingSettings {
                            state: state.clone(),
                            since: now,
                        });
                        Slot::Open(*entity)
                    }
                    None => {
                        let url = state.url.clone();
                        Slot::Reading(fetching(async move { discover::discover(&url).await }))
                    }
                },
            )
            .collect();
        restoring.slots = slots;
    }

    let id = restoring.id;
    for index in 0..restoring.slots.len() {
        let Slot::Reading(reading) = &mut restoring.slots[index] else {
            continue;
        };
        let Some(outcome) = reading.take() else {
            continue;
        };
        let state = restoring.bookmark.sources[index].clone();
        restoring.slots[index] = match outcome {
            Ok(discovered) => {
                commands.queue(move |world: &mut World| {
                    let settings = *world.resource::<LoadSettings>();
                    let source = spawn_discovered(world, discovered, settings);
                    world.entity_mut(source).insert((
                        SourceUrl(state.url.clone()),
                        PendingSettings { state, since: now },
                    ));
                    match world.get_resource_mut::<Restoring>() {
                        Some(mut restoring) if restoring.id == id => {
                            restoring.slots[index] = Slot::Open(source);
                        }
                        // Replaced by another restore: the dataset stays open,
                        // offered in the menus like any other.
                        _ => {}
                    }
                });
                Slot::Registering
            }
            Err(error) => {
                warn!("bookmark: could not open {}: {error}", state.url);
                Slot::Failed(format!("{}: {error}", state.url))
            }
        };
    }

    if restoring
        .slots
        .iter()
        .any(|slot| matches!(slot, Slot::Reading(_) | Slot::Registering))
    {
        return;
    }

    let bookmark = &restoring.bookmark;
    let frames: Vec<_> = bookmark
        .frames
        .iter()
        .enumerate()
        .filter_map(|(at, frame)| {
            let base = restoring.open(frame.source)?;
            sources.get(base).ok().map(|found| (at, frame, base, found))
        })
        .take(MAX_PANELS)
        .collect();

    // With nothing to show, what is on screen stays rather than being swapped
    // for an empty window.
    if !frames.is_empty() {
        for panel in &panels {
            commands.entity(panel).despawn();
        }
    }

    let count = frames.len();
    let mut spawned = Vec::new();
    for (position, (at, frame, base, (data, extent, volume))) in frames.into_iter().enumerate() {
        let cell = area.cell(count, position).size();
        let limits = extent.limits(cell);
        let flat = View {
            centre: Vec2::from_array(frame.view.centre),
            scale: frame
                .view
                .scale_in(cell.to_array())
                .clamp(limits.min_scale, limits.max_scale),
        };
        let panel = spawn_panel(
            &mut commands,
            base,
            data.layer,
            position,
            limits,
            Some(flat),
            palette.frame_bg,
        );
        match (frame.orbit, volume) {
            (Some(saved), Some(volume)) => {
                let mut orbit = Orbit::fit(volume, flat);
                orbit.target = Vec3::from_array(saved.target);
                orbit.yaw = saved.yaw;
                orbit.pitch = saved.pitch;
                orbit.distance = saved.distance;
                commands.entity(panel).insert(orbit);
            }
            (Some(_), None) => warn!(
                "bookmark: {} is no longer a volume, so shown flat",
                data.name
            ),
            _ => {}
        }
        let mut stacked = vec![base];
        for layer in &frame.layers {
            if stacked.len() >= MAX_LAYERS {
                break;
            }
            let Some(source) = restoring.open(layer.source) else {
                continue;
            };
            let Ok((layered, ..)) = sources.get(source) else {
                continue;
            };
            if stacked.contains(&source) {
                continue;
            }
            stacked.push(source);
            spawn_layer(
                &mut commands,
                panel,
                source,
                layered.layer,
                LayerOpacity(layer.opacity.clamp(0.0, 1.0)),
            );
        }
        spawned.push((at, panel));
    }

    selected.0 = bookmark
        .selected
        .and_then(|wanted| spawned.iter().find(|(at, _)| *at == wanted))
        .or(spawned.first())
        .map(|(_, panel)| *panel);

    let failed: Vec<&str> = restoring
        .slots
        .iter()
        .filter_map(|slot| match slot {
            Slot::Failed(error) => Some(error.as_str()),
            _ => None,
        })
        .collect();
    *notice = match (spawned.len(), failed.is_empty()) {
        (0, _) => BookmarkNotice::Failed(format!(
            "could not open {}: {}",
            bookmark.name,
            failed.join("; ")
        )),
        (_, true) => BookmarkNotice::Done(format!("restored {}", bookmark.name)),
        (_, false) => BookmarkNotice::Failed(format!(
            "restored {} without {}",
            bookmark.name,
            failed.join("; ")
        )),
    };
    info!(
        "restored bookmark {}: {} frames, {} datasets could not be opened",
        bookmark.name,
        spawned.len(),
        failed.len()
    );
    commands.remove_resource::<Restoring>();
}

/// Put back each restored dataset's own settings once it can take them.
pub fn apply_pending_settings(
    mut commands: Commands,
    time: Res<Time>,
    mut sources: Query<(
        Entity,
        &DataSource,
        &mut PendingSettings,
        Option<&mut SliceStack>,
        Option<&mut SliceGrid>,
        Option<&mut SourcePointSize>,
        Option<&mut SourceChannels>,
        Option<&mut CellProperties>,
        Has<CellColumns>,
        Has<Described>,
    )>,
) {
    for (
        entity,
        data,
        mut pending,
        stack,
        grid,
        point_size,
        channels,
        properties,
        has_columns,
        described,
    ) in &mut sources
    {
        let since = pending.since;
        let state = &mut pending.state;
        if let (Some(slice), Some(mut stack)) = (state.slice.take(), stack) {
            apply_slice(&mut stack, slice);
        }
        if let (Some(on), Some(mut grid)) = (state.slice_grid.take(), grid) {
            grid.set_if_neq(SliceGrid(on));
        }
        if let (Some(size), Some(mut current)) = (state.point_size.take(), point_size) {
            current.0 = clamp_point_size(size);
        }
        if let Some(opacity) = state.opacity.take() {
            commands
                .entity(entity)
                .insert(SourceOpacity(opacity.clamp(0.0, 1.0)));
        }
        let saved_channels = std::mem::take(&mut state.channels);
        if let (false, Some(mut channels)) = (saved_channels.is_empty(), channels) {
            let missing = apply_channels(&mut channels, &saved_channels);
            if !missing.is_empty() {
                warn!(
                    "bookmark: {} has no channel {}",
                    data.name,
                    missing.join(", ")
                );
            }
        }

        let waiting = match (&state.cells, properties) {
            (None, _) => false,
            (Some(_), None) => {
                warn!("bookmark: {} has no cell properties to filter", data.name);
                false
            }
            // A service will replace these, so filters set now would be lost.
            (Some(_), Some(properties))
                if properties.state != PropertyState::Ready || (has_columns && !described) =>
            {
                if time.elapsed_secs() - since > CELLS_PATIENCE_SECS {
                    warn!(
                        "bookmark: gave up waiting for {}'s cell properties",
                        data.name
                    );
                    false
                } else {
                    true
                }
            }
            (Some(saved), Some(mut properties)) => {
                let missing = apply_cells(&mut properties, saved);
                if !missing.is_empty() {
                    warn!(
                        "bookmark: {} has no cell property {}",
                        data.name,
                        missing.join(", ")
                    );
                }
                false
            }
        };
        if !waiting {
            commands.entity(entity).remove::<PendingSettings>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookmark::capture::capture;
    use crate::source::{SourceInfo, register_in};
    use crate::view::requests::apply_panel_requests;
    use crate::view::{FrameLayers, PanelRequest};

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .add_message::<PanelRequest>()
        .init_resource::<FrameArea>()
        .init_resource::<SelectedPanel>()
        .init_resource::<BookmarkNotice>()
        .insert_resource(LoadSettings::default())
        .insert_resource(Palette::dark())
        .add_systems(
            Update,
            (apply_panel_requests, drive_restore, apply_pending_settings).chain(),
        );
        app
    }

    fn source(app: &mut App, name: &str, url: &str) -> Entity {
        let source = register_in(
            app.world_mut(),
            SourceInfo {
                name: name.into(),
                unit: "px".into(),
                detail: String::new(),
                stat: String::new(),
            },
            SourceExtent {
                centre: Vec2::ZERO,
                size: Vec2::splat(100.0),
                finest: 0.1,
            },
        );
        app.world_mut()
            .entity_mut(source)
            .insert(SourceUrl(url.into()));
        source
    }

    fn panels(app: &mut App) -> Vec<Entity> {
        let mut query = app.world_mut().query_filtered::<Entity, With<Panel>>();
        query.iter(app.world()).collect()
    }

    #[test]
    fn a_frame_comes_back_as_it_was_saved() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "https://store/slide.dzi");
        let outlines = source(&mut app, "Outlines", "https://store/outlines.svg");
        app.world_mut()
            .entity_mut(slide)
            .insert(SliceStack::new(10));

        app.world_mut().write_message(PanelRequest::Open(slide));
        app.update();
        let panel = panels(&mut app)[0];
        app.world_mut().write_message(PanelRequest::AddLayer {
            panel,
            source: outlines,
        });
        app.update();
        let layer = app.world().get::<FrameLayers>(panel).unwrap().cameras()[0];
        app.world_mut().entity_mut(layer).insert(LayerOpacity(0.4));
        app.world_mut().entity_mut(panel).insert((
            Transform::from_xyz(5.0, 6.0, 1000.0),
            Projection::Orthographic(OrthographicProjection {
                scale: 0.5,
                ..OrthographicProjection::default_2d()
            }),
        ));
        app.world_mut()
            .get_mut::<SliceStack>(slide)
            .unwrap()
            .current = 3;

        let bookmark = capture(app.world_mut(), "test".into()).unwrap();
        assert_eq!(bookmark.sources.len(), 2);

        app.world_mut()
            .get_mut::<SliceStack>(slide)
            .unwrap()
            .current = 7;
        app.world_mut().write_message(PanelRequest::Close(panel));
        app.update();
        assert!(panels(&mut app).is_empty());

        app.insert_resource(Restoring::new(bookmark));
        app.update();
        app.update();

        let restored = panels(&mut app);
        assert_eq!(restored.len(), 1);
        let panel = restored[0];
        let world = app.world();
        assert_eq!(
            world.get::<crate::view::ShowsSource>(panel).unwrap().0,
            slide
        );
        let translation = world.get::<Transform>(panel).unwrap().translation;
        assert_eq!(translation.truncate(), Vec2::new(5.0, 6.0));
        let Projection::Orthographic(ortho) = world.get::<Projection>(panel).unwrap() else {
            panic!("a flat frame came back in 3D");
        };
        assert!((ortho.scale - 0.5).abs() < 1e-5);
        let layer = world.get::<FrameLayers>(panel).unwrap().cameras()[0];
        assert_eq!(world.get::<LayerOpacity>(layer), Some(&LayerOpacity(0.4)));
        assert_eq!(world.get::<SliceStack>(slide).unwrap().current, 3);
        assert_eq!(world.resource::<SelectedPanel>().0, Some(panel));
        assert!(world.get_resource::<Restoring>().is_none());
        assert!(world.get::<PendingSettings>(slide).is_none());
        assert_eq!(
            *world.resource::<BookmarkNotice>(),
            BookmarkNotice::Done("restored test".into())
        );
    }
}
