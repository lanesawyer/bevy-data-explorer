//! Per-frame status overlays.
//!
//! The overlay knows nothing about any particular format. It reads the name and
//! status text off whichever source entity a panel points at, and adds the
//! lines that only a panel can know — its own zoom, which differs between two
//! frames showing the same source.

use bevy::prelude::*;

use crate::datasource::{DataSource, SourceStatus};
use crate::panel::{Panel, ShowsSource};

/// A status overlay bound to one panel. Bound by entity rather than by source
/// so that duplicated panels each get their own and report their own zoom.
#[derive(Component, Clone)]
pub struct PanelText {
    panel: Entity,
}

impl Default for PanelText {
    fn default() -> Self {
        // Scenes patch over defaults; the real panel is written on top.
        PanelText {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// Keep one overlay per panel, and drop the overlays of panels that have gone
/// away.
pub fn sync_hud(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    texts: Query<(Entity, &PanelText)>,
) {
    for (entity, text) in &texts {
        if panels.get(text.panel).is_err() {
            commands.entity(entity).despawn();
        }
    }

    for panel in &panels {
        if texts.iter().any(|(_, t)| t.panel == panel) {
            continue;
        }
        commands.spawn_scene(bsn! {
            Text
            TextFont { font_size: { bevy::text::FontSize::Px(13.0) } }
            TextColor({ Color::srgb(0.85, 0.9, 0.95) })
            Node { position_type: { PositionType::Absolute } }
            PanelText { panel: { panel } }
        });
    }
}

/// Keep each overlay over its panel's cell.
pub fn position_hud(
    windows: Query<&Window>,
    panels: Query<&Panel>,
    mut texts: Query<(&PanelText, &mut Node)>,
) {
    let Ok(window) = windows.single() else { return };
    let (columns, rows) = crate::panel::grid_for(panels.iter().count());
    let cell = Vec2::new(
        window.width() / columns as f32,
        window.height() / rows as f32,
    );

    for (text, mut node) in &mut texts {
        let Ok(panel) = panels.get(text.panel) else {
            continue;
        };
        let (col, row) = (panel.index % columns, panel.index / columns);
        node.left = Val::Px(cell.x * col as f32 + 10.0);
        node.top = Val::Px(cell.y * row as f32 + 8.0);
        // Keep the text clear of the duplicate button in the corner.
        node.max_width = Val::Px((cell.x - 46.0).max(80.0));
    }
}

pub fn update_hud(
    panels: Query<(&Camera, &Projection, &ShowsSource)>,
    sources: Query<(&DataSource, &SourceStatus)>,
    mut texts: Query<(&mut Text, &PanelText)>,
) {
    for (mut text, panel_text) in &mut texts {
        let Ok((camera, projection, shows)) = panels.get(panel_text.panel) else {
            continue;
        };
        let Ok((source, status)) = sources.get(shows.0) else {
            continue;
        };
        let Projection::Orthographic(ortho) = projection else {
            continue;
        };
        let viewport = camera.logical_viewport_size().unwrap_or(Vec2::ONE);
        let units_per_px = ortho.area.width() / viewport.x.max(1.0);

        text.0 = format!(
            "{}\n{}\nzoom {:.5} {}/screen px\ndrag pan · scroll zoom · R reset",
            source.name, status.0, units_per_px, source.unit,
        );
    }
}
