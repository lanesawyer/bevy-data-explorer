//! Opening, closing and duplicating frames, and emptying one to browse for
//! what it should show.
//!
//! Requests are messages rather than direct edits, so a button in any dock
//! can ask for a frame without holding the world.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::grid::{MAX_PANELS, grid_for};
use super::layers::{
    FrameLayers, LayerOf, LayerOpacity, can_add_layer, spawn_layer, stacked_sources,
};
use super::{Browsing, FrameArea, Panel, SelectedPanel, View, spawn_browse_panel, spawn_panel};
use crate::source::table::SourceTable;
use crate::source::{DataSource, ShowsSource, SourceExtent, SourceUrl, ViewLimits};

/// A change to the set of frames.
///
/// A frame's own corner buttons, its browser and the sidebar's New frame all
/// raise these, so the routes cannot drift apart: the rules about what may be
/// opened or closed live in one place.
#[derive(Message, Clone, Copy)]
pub enum PanelRequest {
    Duplicate(Entity),
    Close(Entity),
    /// Open a new frame onto a source.
    Open(Entity),
    /// Show what is known about a frame.
    Inspect(Entity),
    /// Point a frame at a different source.
    Show {
        panel: Entity,
        source: Entity,
    },
    /// Draw a source over whatever a frame already shows.
    AddLayer {
        panel: Entity,
        source: Entity,
    },
    /// Take a source's layer off a frame.
    RemoveLayer {
        panel: Entity,
        source: Entity,
    },
    /// Browse for a dataset: in a new, empty frame, or over what this frame
    /// shows now, which stays until something else is chosen.
    Browse(Option<Entity>),
    /// Move a source's layer one place up the stack, towards the top, or down.
    /// It never moves below the source the frame opened onto.
    MoveLayer {
        panel: Entity,
        source: Entity,
        up: bool,
    },
}

/// Show a dataset by its address, fetching it first if it is not open yet.
///
/// Raised by anything that offers a known dataset, and answered by whatever
/// can read one — which lives above the grid, so the grid asks rather than
/// reads. A dataset already open is not fetched again: the answer is the
/// [`PanelRequest`] it would have ended in.
#[derive(Message, Clone, Debug)]
pub struct DatasetRequest {
    pub url: String,
    pub target: DatasetTarget,
}

/// Where a dataset asked for by address ends up once it is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DatasetTarget {
    /// A frame of its own.
    #[default]
    NewFrame,
    /// In place of what this frame shows now.
    Show(Entity),
    /// Drawn over what this frame shows.
    Layer(Entity),
}

impl DatasetTarget {
    /// The request that puts an open source where it was asked for.
    pub fn request_for(self, source: Entity) -> PanelRequest {
        match self {
            DatasetTarget::NewFrame => PanelRequest::Open(source),
            DatasetTarget::Show(panel) => PanelRequest::Show { panel, source },
            DatasetTarget::Layer(panel) => PanelRequest::AddLayer { panel, source },
        }
    }
}

/// A frame waiting on a dataset still being read, to show in place of its own.
///
/// Carries the address and a name to report, so the frame can say what it is
/// about to become. Taken off once that dataset is shown, or its read fails.
#[derive(Component, Clone, Debug)]
pub struct PendingShow {
    pub url: String,
    pub name: String,
}

/// Why the dataset a frame last asked for could not be opened. Taken off once
/// it shows one, or asks for another.
#[derive(Component, Clone, Debug)]
pub struct ShowFailed(pub String);

/// Apply requested changes to the set of frames.
pub fn apply_panel_requests(
    mut commands: Commands,
    mut requests: MessageReader<PanelRequest>,
    mut selected: ResMut<SelectedPanel>,
    area: Res<FrameArea>,
    panels: Query<(
        Entity,
        &Panel,
        &ShowsSource,
        &Transform,
        &Projection,
        &ViewLimits,
        Option<&FrameLayers>,
        Option<&super::Orbit>,
    )>,
    layer_cameras: Query<&ShowsSource, With<LayerOf>>,
    layer_opacities: Query<(&ShowsSource, &LayerOpacity), With<LayerOf>>,
    sources: Query<(&DataSource, &SourceExtent, Option<&crate::source::HomeView>)>,
    tables: Query<(), With<SourceTable>>,
    pending: Query<&PendingShow>,
    urls: Query<&SourceUrl>,
    palette: Res<crate::app::theme::Palette>,
    // Every frame, including an empty one, which `panels` cannot see: it
    // shows no source.
    frames: Query<(Entity, &Panel)>,
) {
    let requests: Vec<PanelRequest> = requests.read().copied().collect();
    if requests.is_empty() {
        return;
    }

    let mut open: Vec<(usize, Entity)> = frames
        .iter()
        .map(|(entity, panel)| (panel.index, entity))
        .collect();
    open.sort_unstable();

    let (columns, rows) = grid_for(open.len().max(1));
    let viewport = Vec2::new(area.size.x / columns as f32, area.size.y / rows as f32);

    let mut closing: Vec<Entity> = Vec::new();
    let mut spawned = 0usize;
    let lookup = |entity: Entity| sources.get(entity).ok().map(|(data, ..)| data);
    // Layers added by an earlier request this frame, which the query cannot
    // see until the commands have run.
    let mut added: Vec<(Entity, Entity)> = Vec::new();

    for request in requests {
        match request {
            PanelRequest::Duplicate(panel) => {
                if open.len() + spawned >= MAX_PANELS {
                    continue;
                }
                let Ok((_, _, shows, transform, projection, limits, layers, orbit)) =
                    panels.get(panel)
                else {
                    continue;
                };
                let Ok((source, ..)) = sources.get(shows.0) else {
                    continue;
                };
                // A 3D frame is duplicated in 3D, turned the same way, and
                // keeps the 2D view it would go back to.
                let flat = match (orbit, projection) {
                    (Some(orbit), _) => orbit.flat,
                    (None, Projection::Orthographic(ortho)) => View {
                        center: transform.translation.truncate(),
                        scale: ortho.scale,
                    },
                    (None, _) => continue,
                };
                let copy = spawn_panel(
                    &mut commands,
                    shows.0,
                    source.layer,
                    open.len() + spawned,
                    *limits,
                    Some(flat),
                    palette.frame_bg,
                );
                if let Some(orbit) = orbit {
                    commands.entity(copy).insert(*orbit);
                }
                // The same stack at the same opacities, so a duplicate is the
                // same picture.
                for camera in layers.map(FrameLayers::cameras).unwrap_or_default() {
                    let Ok((layer, opacity)) = layer_opacities.get(*camera) else {
                        continue;
                    };
                    if let Some(data) = lookup(layer.0) {
                        spawn_layer(&mut commands, copy, layer.0, data.layer, *opacity);
                    }
                }
                info!("duplicated the frame showing {}", source.name);
                spawned += 1;
            }
            PanelRequest::Open(source_entity) => {
                if open.len() + spawned >= MAX_PANELS {
                    continue;
                }
                let Ok((source, extent, home)) = sources.get(source_entity) else {
                    continue;
                };
                spawn_panel(
                    &mut commands,
                    source_entity,
                    source.layer,
                    open.len() + spawned,
                    extent.limits_from(home, viewport),
                    None,
                    palette.frame_bg,
                );
                info!("opened a frame onto {}", source.name);
                spawned += 1;
            }
            PanelRequest::Close(panel) => {
                if frames.contains(panel) && !closing.contains(&panel) {
                    let name = panels
                        .get(panel)
                        .ok()
                        .and_then(|(_, _, shows, ..)| sources.get(shows.0).ok())
                        .map_or("nothing", |(source, ..)| source.name.as_str());
                    info!("closed the frame showing {name}");
                    closing.push(panel);
                }
            }
            PanelRequest::Browse(None) => {
                if open.len() + spawned >= MAX_PANELS {
                    continue;
                }
                let panel =
                    spawn_browse_panel(&mut commands, open.len() + spawned, palette.frame_bg);
                selected.0 = Some(panel);
                info!("opened an empty frame to browse from");
                spawned += 1;
            }
            PanelRequest::Browse(Some(panel)) => {
                if frames.contains(panel) {
                    commands
                        .entity(panel)
                        .insert(Browsing)
                        .remove::<ShowFailed>();
                    selected.0 = Some(panel);
                }
            }
            // Selecting is handled here so the inspector can simply follow the
            // selection rather than tracking a frame of its own.
            PanelRequest::Inspect(panel) => {
                if panels.get(panel).is_ok() {
                    selected.0 = Some(panel);
                }
            }
            PanelRequest::Show { panel, source } => {
                // An empty frame shows nothing yet, so has nothing stacked.
                let (shows, layers) = match panels.get(panel) {
                    Ok((_, _, shows, _, _, _, layers, _)) => (Some(shows.0), layers),
                    Err(_) if frames.contains(panel) => (None, None),
                    Err(_) => continue,
                };
                // Only the dataset the frame is waiting on settles the wait: a
                // later choice may already have replaced it.
                if let (Ok(waiting), Ok(url)) = (pending.get(panel), urls.get(source))
                    && waiting.url == url.0
                {
                    commands.entity(panel).remove::<PendingShow>();
                }
                if shows == Some(source) {
                    commands.entity(panel).remove::<(Browsing, ShowFailed)>();
                    continue;
                }
                let Ok((data, extent, home)) = sources.get(source) else {
                    continue;
                };
                // Repointing is the whole reason a frame holds a source entity
                // rather than naming a format: the camera moves to that
                // source's layer and is reframed to its extent.
                let limits = extent.limits_from(home, viewport);
                info!("frame now showing {}", data.name);
                // In 2D, whatever it was: the new dataset may have no depth, and
                // one that does has its own to be framed to.
                commands
                    .entity(panel)
                    .remove::<(super::Orbit, Browsing, ShowFailed)>()
                    .insert((
                        ShowsSource(source),
                        RenderLayers::layer(data.layer),
                        limits,
                        Transform::from_translation(limits.center.extend(1000.0)),
                        Projection::Orthographic(OrthographicProjection {
                            scale: limits.fit_scale,
                            ..OrthographicProjection::default_2d()
                        }),
                    ));
                // The layers stay, over whatever is now underneath them —
                // except one of the source now at the bottom, which would draw
                // it twice, and all of them under a table, which has nothing
                // for them to lie over.
                if tables.contains(source) {
                    for camera in layers.map(FrameLayers::cameras).unwrap_or_default() {
                        commands.entity(*camera).despawn();
                    }
                } else {
                    remove_layers_of(&mut commands, layers, &layer_cameras, source);
                }
                selected.0 = Some(panel);
            }
            PanelRequest::AddLayer { panel, source } => {
                let Ok((_, _, shows, _, _, _, layers, _)) = panels.get(panel) else {
                    continue;
                };
                let Some(data) = lookup(source) else {
                    continue;
                };
                let mut stack = stacked_sources(shows, layers, &layer_cameras);
                stack.extend(
                    added
                        .iter()
                        .filter(|(onto, _)| *onto == panel)
                        .map(|(_, layer)| *layer),
                );
                if !can_add_layer(&stack, source) {
                    continue;
                }
                // A table's rows are records rather than a place, so it shares
                // no coordinates with anything it could be stacked with.
                if tables.contains(source) {
                    warn!("{} is a table, which cannot be a layer", data.name);
                    continue;
                }
                if stack.first().is_some_and(|base| tables.contains(*base)) {
                    warn!("a table cannot have {} layered over it", data.name);
                    continue;
                }
                spawn_layer(
                    &mut commands,
                    panel,
                    source,
                    data.layer,
                    LayerOpacity::default(),
                );
                added.push((panel, source));
                selected.0 = Some(panel);
                match stack.first().and_then(|base| lookup(*base)) {
                    Some(base) => match super::layers::unit_mismatch(base, data) {
                        Some(mismatch) => {
                            info!("layered {} onto {} ({mismatch})", data.name, base.name);
                        }
                        None => info!("layered {} onto {}", data.name, base.name),
                    },
                    None => info!("layered {}", data.name),
                }
            }
            PanelRequest::RemoveLayer { panel, source } => {
                if let Ok((.., layers, _)) = panels.get(panel) {
                    remove_layers_of(&mut commands, layers, &layer_cameras, source);
                }
            }
            PanelRequest::MoveLayer { panel, source, up } => {
                let Ok((.., Some(layers), _)) = panels.get(panel) else {
                    continue;
                };
                let Some(camera) = layers.cameras().iter().copied().find(|camera| {
                    layer_cameras
                        .get(*camera)
                        .is_ok_and(|shows| shows.0 == source)
                }) else {
                    continue;
                };
                // Found again when the command runs rather than now, so two
                // moves asked for in one frame both land.
                commands
                    .entity(panel)
                    .queue(move |mut frame: EntityWorldMut| {
                        let Some(mut layers) = frame.get_mut::<FrameLayers>() else {
                            return;
                        };
                        let Some(at) = layers.cameras().iter().position(|c| *c == camera) else {
                            return;
                        };
                        let to = if up { at + 1 } else { at.wrapping_sub(1) };
                        if to < layers.cameras().len() {
                            layers.swap(at, to);
                        }
                    });
            }
        }
    }

    // Closing them all is allowed: the window falls back to the empty state it
    // starts in, which is a way back rather than a dead end.
    for entity in closing {
        commands.entity(entity).despawn();
    }
    // Cells, draw order and which camera clears are settled by
    // `normalize_panels` once the despawns have taken effect.
}

/// Despawn every layer camera in `layers` drawing `source`.
fn remove_layers_of(
    commands: &mut Commands,
    layers: Option<&FrameLayers>,
    cameras: &Query<&ShowsSource, With<LayerOf>>,
    source: Entity,
) {
    for camera in layers.map(FrameLayers::cameras).unwrap_or_default() {
        if cameras.get(*camera).is_ok_and(|shows| shows.0 == source) {
            commands.entity(*camera).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{SourceInfo, register_in};

    fn app() -> App {
        let mut app = App::new();
        // Frames are spawned as scenes, which resolve through the asset server.
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .add_message::<PanelRequest>()
        .init_resource::<FrameArea>()
        .init_resource::<SelectedPanel>()
        .insert_resource(crate::app::theme::Palette::dark())
        .add_systems(Update, apply_panel_requests);
        app
    }

    fn source(app: &mut App, name: &str, unit: &str) -> Entity {
        register_in(
            app.world_mut(),
            SourceInfo {
                name: name.into(),
                unit: unit.into(),
                detail: String::new(),
                stat: String::new(),
                category: crate::source::Category::Image,
            },
            SourceExtent {
                center: Vec2::ZERO,
                size: Vec2::splat(100.0),
                finest: 0.1,
            },
        )
    }

    fn request(app: &mut App, request: PanelRequest) {
        app.world_mut().write_message(request);
        app.update();
    }

    fn only_panel(app: &mut App) -> Entity {
        let mut panels = app.world_mut().query_filtered::<Entity, With<Panel>>();
        let found: Vec<Entity> = panels.iter(app.world()).collect();
        assert_eq!(found.len(), 1);
        found[0]
    }

    fn layers_of(app: &App, panel: Entity) -> Vec<Entity> {
        app.world()
            .get::<FrameLayers>(panel)
            .map(|layers| {
                layers
                    .cameras()
                    .iter()
                    .map(|camera| app.world().get::<ShowsSource>(*camera).unwrap().0)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn an_empty_frame_takes_a_cell_and_is_filled_by_what_is_chosen() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        request(&mut app, PanelRequest::Browse(None));
        let panel = only_panel(&mut app);
        assert!(app.world().get::<Browsing>(panel).is_some());
        assert!(app.world().get::<ShowsSource>(panel).is_none());

        request(
            &mut app,
            PanelRequest::Show {
                panel,
                source: slide,
            },
        );
        assert_eq!(only_panel(&mut app), panel, "filled, not replaced");
        assert_eq!(app.world().get::<ShowsSource>(panel).unwrap().0, slide);
        assert!(app.world().get::<Browsing>(panel).is_none());
    }

    #[test]
    fn a_frame_can_browse_and_an_empty_one_can_close() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);
        request(&mut app, PanelRequest::Browse(Some(panel)));
        assert!(app.world().get::<Browsing>(panel).is_some());
        assert_eq!(
            app.world().get::<ShowsSource>(panel).unwrap().0,
            slide,
            "what it shows stays until something else is chosen"
        );

        request(&mut app, PanelRequest::Browse(None));
        let mut empty = app
            .world_mut()
            .query_filtered::<(Entity, &Panel), Without<ShowsSource>>();
        let (empty, cell) = empty.single(app.world()).unwrap();
        assert_eq!(cell.index, 1, "after the frame already open");
        request(&mut app, PanelRequest::Close(empty));
        assert_eq!(only_panel(&mut app), panel);
    }

    #[test]
    fn a_layer_is_drawn_on_its_own_source_render_layer() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let cells = source(&mut app, "Cells", "um");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);

        // Measured differently, and layered anyway.
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: cells,
            },
        );
        assert_eq!(layers_of(&app, panel), vec![cells]);

        let camera = app.world().get::<FrameLayers>(panel).unwrap().cameras()[0];
        let expected = app.world().get::<DataSource>(cells).unwrap().layer;
        assert_eq!(
            app.world().get::<RenderLayers>(camera),
            Some(&RenderLayers::layer(expected))
        );
        // A layer is not a frame of its own.
        assert!(app.world().get::<Panel>(camera).is_none());
    }

    fn table(app: &mut App, name: &str) -> Entity {
        let entity = source(app, name, "");
        app.world_mut()
            .entity_mut(entity)
            .insert(SourceTable::default());
        entity
    }

    #[test]
    fn a_table_is_never_layered_and_never_layered_over() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let specimens = table(&mut app, "Specimens");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: specimens,
            },
        );
        assert!(layers_of(&app, panel).is_empty());

        request(&mut app, PanelRequest::Close(panel));
        request(&mut app, PanelRequest::Open(specimens));
        let panel = only_panel(&mut app);
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: slide,
            },
        );
        assert!(layers_of(&app, panel).is_empty());
    }

    #[test]
    fn repointing_a_frame_at_a_table_drops_its_layers() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let outlines = source(&mut app, "Outlines", "px");
        let specimens = table(&mut app, "Specimens");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: outlines,
            },
        );
        assert_eq!(layers_of(&app, panel), vec![outlines]);

        request(
            &mut app,
            PanelRequest::Show {
                panel,
                source: specimens,
            },
        );
        assert!(layers_of(&app, panel).is_empty());
    }

    #[test]
    fn asking_twice_stacks_a_source_once() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let outlines = source(&mut app, "Outlines", "px");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);

        app.world_mut().write_message(PanelRequest::AddLayer {
            panel,
            source: outlines,
        });
        app.world_mut().write_message(PanelRequest::AddLayer {
            panel,
            source: outlines,
        });
        app.update();
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: outlines,
            },
        );
        assert_eq!(layers_of(&app, panel), vec![outlines]);
    }

    #[test]
    fn closing_a_frame_takes_its_layers_with_it() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let outlines = source(&mut app, "Outlines", "px");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: outlines,
            },
        );

        request(&mut app, PanelRequest::Close(panel));
        let mut cameras = app.world_mut().query_filtered::<(), With<LayerOf>>();
        assert_eq!(
            cameras.iter(app.world()).count(),
            0,
            "a layer outlived its frame"
        );
    }

    #[test]
    fn removing_a_layer_leaves_the_rest_of_the_stack() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let outlines = source(&mut app, "Outlines", "px");
        let cells = source(&mut app, "Cells", "um");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: outlines,
            },
        );
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: cells,
            },
        );

        request(
            &mut app,
            PanelRequest::RemoveLayer {
                panel,
                source: outlines,
            },
        );
        assert_eq!(layers_of(&app, panel), vec![cells]);
    }

    #[test]
    fn a_layer_moves_within_the_stack_and_no_further() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let outlines = source(&mut app, "Outlines", "px");
        let cells = source(&mut app, "Cells", "um");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);
        for layer in [outlines, cells] {
            request(
                &mut app,
                PanelRequest::AddLayer {
                    panel,
                    source: layer,
                },
            );
        }

        let move_layer = |source, up| PanelRequest::MoveLayer { panel, source, up };
        request(&mut app, move_layer(outlines, true));
        assert_eq!(layers_of(&app, panel), vec![cells, outlines]);
        // Already on top, and nothing goes under the base.
        request(&mut app, move_layer(outlines, true));
        request(&mut app, move_layer(cells, false));
        assert_eq!(layers_of(&app, panel), vec![cells, outlines]);

        // Two moves in one frame both land.
        app.world_mut().write_message(move_layer(outlines, false));
        app.world_mut().write_message(move_layer(outlines, true));
        app.update();
        assert_eq!(layers_of(&app, panel), vec![cells, outlines]);
    }

    #[test]
    fn a_duplicate_is_the_same_stack() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let outlines = source(&mut app, "Outlines", "px");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: outlines,
            },
        );

        request(&mut app, PanelRequest::Duplicate(panel));
        let mut panels = app.world_mut().query_filtered::<Entity, With<Panel>>();
        let copy = panels
            .iter(app.world())
            .find(|entity| *entity != panel)
            .expect("no duplicate");
        assert_eq!(layers_of(&app, copy), vec![outlines]);
    }

    #[test]
    fn a_duplicate_keeps_each_layers_opacity() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let outlines = source(&mut app, "Outlines", "px");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: outlines,
            },
        );
        let layer = app.world().get::<FrameLayers>(panel).unwrap().cameras()[0];
        app.world_mut().entity_mut(layer).insert(LayerOpacity(0.3));

        request(&mut app, PanelRequest::Duplicate(panel));
        let mut panels = app.world_mut().query_filtered::<Entity, With<Panel>>();
        let copy = panels
            .iter(app.world())
            .find(|entity| *entity != panel)
            .expect("no duplicate");
        let copied = app.world().get::<FrameLayers>(copy).unwrap().cameras()[0];
        assert_eq!(
            app.world().get::<LayerOpacity>(copied),
            Some(&LayerOpacity(0.3))
        );
    }

    #[test]
    fn showing_a_layers_own_source_at_the_bottom_drops_that_layer() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let outlines = source(&mut app, "Outlines", "px");
        let cells = source(&mut app, "Cells", "um");
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: outlines,
            },
        );
        request(
            &mut app,
            PanelRequest::AddLayer {
                panel,
                source: cells,
            },
        );

        request(
            &mut app,
            PanelRequest::Show {
                panel,
                source: outlines,
            },
        );
        assert_eq!(layers_of(&app, panel), vec![cells]);
    }

    #[test]
    fn a_frame_stops_waiting_only_for_the_dataset_it_asked_for() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "px");
        let first = source(&mut app, "First", "px");
        let second = source(&mut app, "Second", "px");
        app.world_mut()
            .entity_mut(first)
            .insert(SourceUrl("https://store/first".into()));
        app.world_mut()
            .entity_mut(second)
            .insert(SourceUrl("https://store/second".into()));
        request(&mut app, PanelRequest::Open(slide));
        let panel = only_panel(&mut app);
        app.world_mut().entity_mut(panel).insert(PendingShow {
            url: "https://store/second".into(),
            name: "Second".into(),
        });

        // An earlier choice landing late is shown, but the frame is still
        // waiting on the one chosen after it.
        request(&mut app, DatasetTarget::Show(panel).request_for(first));
        assert_eq!(app.world().get::<ShowsSource>(panel).unwrap().0, first);
        assert!(app.world().get::<PendingShow>(panel).is_some());

        request(&mut app, DatasetTarget::Show(panel).request_for(second));
        assert_eq!(app.world().get::<ShowsSource>(panel).unwrap().0, second);
        assert!(app.world().get::<PendingShow>(panel).is_none());
    }
}
