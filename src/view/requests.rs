//! Opening, closing and duplicating frames.
//!
//! Requests are messages rather than direct edits, so a button in any dock
//! can ask for a frame without holding the world.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::grid::{MAX_PANELS, grid_for};
use super::layers::{FrameLayers, LayerOf, can_add_layer, spawn_layer, stacked_sources};
use super::{FrameArea, Panel, SelectedPanel, ShowsSource, View, spawn_panel};
use crate::source::{DataSource, ViewLimits};

/// A change to the set of frames.
///
/// Both a frame's own corner buttons and the sidebar's layout menu raise these,
/// so the two routes cannot drift apart: the rules about what may be opened or
/// closed live in one place.
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
}

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
    )>,
    layer_cameras: Query<&ShowsSource, With<LayerOf>>,
    sources: Query<(&DataSource, &crate::source::SourceExtent)>,
    palette: Res<crate::app::theme::Palette>,
) {
    let requests: Vec<PanelRequest> = requests.read().copied().collect();
    if requests.is_empty() {
        return;
    }

    let mut open: Vec<(usize, Entity)> = panels
        .iter()
        .map(|(entity, panel, ..)| (panel.index, entity))
        .collect();
    open.sort_unstable();

    let (columns, rows) = grid_for(open.len().max(1));
    let viewport = Vec2::new(area.size.x / columns as f32, area.size.y / rows as f32);

    let mut closing: Vec<Entity> = Vec::new();
    let mut spawned = 0usize;
    let lookup = |entity: Entity| sources.get(entity).ok().map(|(data, _)| data);
    // Layers added by an earlier request this frame, which the query cannot
    // see until the commands have run.
    let mut added: Vec<(Entity, Entity)> = Vec::new();

    for request in requests {
        match request {
            PanelRequest::Duplicate(panel) => {
                if open.len() + spawned >= MAX_PANELS {
                    continue;
                }
                let Ok((_, _, shows, transform, projection, limits, layers)) = panels.get(panel)
                else {
                    continue;
                };
                let Ok((source, _)) = sources.get(shows.0) else {
                    continue;
                };
                let Projection::Orthographic(ortho) = projection else {
                    continue;
                };
                let copy = spawn_panel(
                    &mut commands,
                    shows.0,
                    source.layer,
                    open.len() + spawned,
                    *limits,
                    Some(View {
                        centre: transform.translation.truncate(),
                        scale: ortho.scale,
                    }),
                    palette.frame_bg,
                );
                // The same stack, so a duplicate is the same picture.
                for layer in stacked_sources(shows, layers, &layer_cameras)
                    .into_iter()
                    .skip(1)
                {
                    if let Some(data) = lookup(layer) {
                        spawn_layer(&mut commands, copy, layer, data.layer);
                    }
                }
                info!("duplicated the frame showing {}", source.name);
                spawned += 1;
            }
            PanelRequest::Open(source_entity) => {
                if open.len() + spawned >= MAX_PANELS {
                    continue;
                }
                let Ok((source, extent)) = sources.get(source_entity) else {
                    continue;
                };
                spawn_panel(
                    &mut commands,
                    source_entity,
                    source.layer,
                    open.len() + spawned,
                    extent.limits(viewport),
                    None,
                    palette.frame_bg,
                );
                info!("opened a frame onto {}", source.name);
                spawned += 1;
            }
            PanelRequest::Close(panel) => {
                if let Ok((_, _, shows, ..)) = panels.get(panel)
                    && !closing.contains(&panel)
                {
                    let name = sources
                        .get(shows.0)
                        .map_or("a dataset", |(source, _)| source.name.as_str());
                    info!("closed the frame showing {name}");
                    closing.push(panel);
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
                let Ok((_, _, shows, _, _, _, layers)) = panels.get(panel) else {
                    continue;
                };
                if shows.0 == source {
                    continue;
                }
                let Ok((data, extent)) = sources.get(source) else {
                    continue;
                };
                // Repointing is the whole reason a frame holds a source entity
                // rather than naming a format: the camera moves to that
                // source's layer and is reframed to its extent.
                let limits = extent.limits(viewport);
                info!("frame now showing {}", data.name);
                commands.entity(panel).insert((
                    ShowsSource(source),
                    RenderLayers::layer(data.layer),
                    limits,
                    Transform::from_translation(limits.centre.extend(1000.0)),
                    Projection::Orthographic(OrthographicProjection {
                        scale: limits.fit_scale,
                        ..OrthographicProjection::default_2d()
                    }),
                ));
                // The layers stay, over whatever is now underneath them —
                // except one of the source now at the bottom, which would draw
                // it twice.
                for camera in layers.map(FrameLayers::cameras).unwrap_or_default() {
                    if layer_cameras
                        .get(*camera)
                        .is_ok_and(|layer| layer.0 == source)
                    {
                        commands.entity(*camera).despawn();
                    }
                }
                selected.0 = Some(panel);
            }
            PanelRequest::AddLayer { panel, source } => {
                let Ok((_, _, shows, _, _, _, layers)) = panels.get(panel) else {
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
                spawn_layer(&mut commands, panel, source, data.layer);
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
                let Ok((.., Some(layers))) = panels.get(panel) else {
                    continue;
                };
                for camera in layers.cameras() {
                    if layer_cameras
                        .get(*camera)
                        .is_ok_and(|shows| shows.0 == source)
                    {
                        commands.entity(*camera).despawn();
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{SourceExtent, SourceInfo, register_in};

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
            },
            SourceExtent {
                centre: Vec2::ZERO,
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
}
