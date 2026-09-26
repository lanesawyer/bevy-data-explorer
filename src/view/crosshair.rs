//! Crosshairs on linked frames, where the point they share is.
//!
//! Every linked frame is centered on the one point the link shares, so its
//! crosshair is the middle of the frame. What makes it worth drawing is the
//! others: a frame cut looking along x shows, on its crosshair, the line a
//! frame looking along z is paging through, and the two meet at the same
//! place in the specimen. Each line is drawn in the color of the axis it runs
//! along, as Neuroglancer draws them.

use bevy::prelude::*;
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_feathers::tokens;

use crate::app::schedule::Stage;
use crate::source::ShowsSource;
use crate::source::stack::SourceAxes;
use crate::source::table::SourceTable;
use crate::widgets::patch_node;

use super::link::Linked;
use super::{FrameArea, Panel};

/// How thick a line is, in logical pixels.
const LINE_PX: f32 = 1.0;

/// One line of a frame's crosshair.
#[derive(Component, Clone)]
pub struct CrosshairLine {
    panel: Entity,
    /// Runs across the frame, rather than down it.
    across: bool,
}

impl Default for CrosshairLine {
    fn default() -> Self {
        CrosshairLine {
            panel: Entity::PLACEHOLDER,
            across: false,
        }
    }
}

impl CrosshairLine {
    pub fn panel(&self) -> Entity {
        self.panel
    }
}

/// The color a line running along `axis` is drawn in.
fn axis_token(axis: char) -> bevy_feathers::theme::ThemeToken {
    match axis {
        'x' => tokens::TEXT_INPUT_X_AXIS,
        'y' => tokens::TEXT_INPUT_Y_AXIS,
        'z' => tokens::TEXT_INPUT_Z_AXIS,
        _ => crate::app::theme::token::OVERLAY_TEXT,
    }
}

/// Keep two lines per frame, and drop those whose frame has gone.
fn sync_crosshairs(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    lines: Query<(Entity, &CrosshairLine)>,
) {
    for (entity, line) in &lines {
        if panels.get(line.panel).is_err() {
            commands.entity(entity).despawn();
        }
    }
    for panel in &panels {
        if lines.iter().any(|(_, line)| line.panel == panel) {
            continue;
        }
        for across in [true, false] {
            commands.spawn_scene(bsn! {
                CrosshairLine { panel: { panel }, across: { across } }
                Node {
                    position_type: { PositionType::Absolute },
                    display: { Display::None },
                }
                ThemeBackgroundColor({ crate::app::theme::token::OVERLAY_TEXT })
                // Drawn over the data, never in the way of it.
                template_value(Pickable::IGNORE)
            });
        }
    }
}

/// Draw each linked flat frame's crosshair through its middle, in the colors
/// of the axes its lines run along; hide the rest.
fn place_crosshairs(
    mut commands: Commands,
    area: Res<FrameArea>,
    panels: Query<(&Panel, &ShowsSource, &Projection, Has<Linked>)>,
    axes: Query<&SourceAxes>,
    tables: Query<(), With<SourceTable>>,
    mut lines: Query<(Entity, &CrosshairLine, &mut Node, &ThemeBackgroundColor)>,
) {
    let count = panels.iter().count();
    for (entity, line, node, color) in &mut lines {
        let shown = panels
            .get(line.panel)
            .ok()
            .filter(|(_, shows, projection, linked)| {
                *linked
                    && !tables.contains(shows.0)
                    && matches!(projection, Projection::Orthographic(_))
            });
        let Some((panel, shows, ..)) = shown else {
            patch_node(node, |node| node.display = Display::None);
            continue;
        };
        let cell = area.cell(count, panel.index);
        let center = cell.center();
        let axes = axes.get(shows.0).copied().unwrap_or_default();
        let along = if line.across { axes.across } else { axes.down };
        // Immutable, so a line whose axis changed is given its color anew.
        let wanted = axis_token(along);
        if color.0 != wanted {
            commands.entity(entity).insert(ThemeBackgroundColor(wanted));
        }
        patch_node(node, |node| {
            node.display = Display::Flex;
            if line.across {
                node.left = Val::Px(cell.min.x);
                node.top = Val::Px(center.y - LINE_PX * 0.5);
                node.width = Val::Px(cell.width());
                node.height = Val::Px(LINE_PX);
            } else {
                node.left = Val::Px(center.x - LINE_PX * 0.5);
                node.top = Val::Px(cell.min.y);
                node.width = Val::Px(LINE_PX);
                node.height = Val::Px(cell.height());
            }
        });
    }
}

/// A crosshair over every linked frame.
pub struct CrosshairPlugin;

impl Plugin for CrosshairPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_crosshairs.in_set(Stage::FrameChrome))
            .add_systems(Update, place_crosshairs.in_set(Stage::Chrome));
    }
}
