//! Opening, closing and duplicating frames.
//!
//! Requests are messages rather than direct edits, so a button in any dock
//! can ask for a frame without holding the world.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::grid::{MAX_PANELS, grid_for, renumber};
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
    )>,
    sources: Query<(&DataSource, &crate::source::SourceExtent)>,
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

    for request in requests {
        match request {
            PanelRequest::Duplicate(panel) => {
                if open.len() + spawned >= MAX_PANELS {
                    continue;
                }
                let Ok((_, _, shows, transform, projection, limits)) = panels.get(panel) else {
                    continue;
                };
                let Ok((source, _)) = sources.get(shows.0) else {
                    continue;
                };
                let Projection::Orthographic(ortho) = projection else {
                    continue;
                };
                spawn_panel(
                    &mut commands,
                    shows.0,
                    source.layer,
                    open.len() + spawned,
                    *limits,
                    Some(View {
                        centre: transform.translation.truncate(),
                        scale: ortho.scale,
                    }),
                );
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
                );
                spawned += 1;
            }
            PanelRequest::Close(panel) => {
                if panels.get(panel).is_ok() && !closing.contains(&panel) {
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
                let Ok((_, _, shows, ..)) = panels.get(panel) else {
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
                selected.0 = Some(panel);
            }
        }
    }

    if closing.is_empty() {
        return;
    }
    let existing: Vec<(Entity, usize)> = panels
        .iter()
        .map(|(entity, panel, ..)| (entity, panel.index))
        .collect();
    // Never close the last frame: an empty window offers no way back.
    if renumber(&existing, &closing).is_empty() && spawned == 0 {
        return;
    }
    for entity in closing {
        commands.entity(entity).despawn();
    }
    // Cells, draw order and which camera clears are settled by
    // `normalize_panels` once the despawns have taken effect.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::grid::tests::panels;

    #[test]
    fn the_last_panel_cannot_be_closed() {
        let all = panels(1);
        assert!(renumber(&all, &[all[0].0]).is_empty());

        // Nor can every panel be closed at once.
        let all = panels(3);
        let everything: Vec<Entity> = all.iter().map(|(e, _)| *e).collect();
        assert!(renumber(&all, &everything).is_empty());
    }
}
