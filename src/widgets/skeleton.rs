//! Placeholder rows, shown where content is on its way.
//!
//! Something like the shape of what is coming, pulsing, so a section being
//! filled in says so rather than showing a stand-in that is swapped out a
//! moment later.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::space;
use crate::app::theme::Palette;

/// Seconds for one pulse.
const PULSE_SECS: f32 = 1.2;
/// The faintest the pulse gets, as a fraction of the bar's color.
const PULSE_LOW: f32 = 0.35;

/// One placeholder bar. Painted by [`pulse_skeletons`] rather than a theme
/// token, since the pulse is a color that changes every frame.
#[derive(Component, Clone, Default)]
pub struct SkeletonBar;

/// A column of `rows` bars, each `row_px` tall.
pub fn spawn_skeleton(commands: &mut Commands, rows: usize, row_px: f32) -> Entity {
    let bars: Vec<Entity> = (0..rows)
        .map(|_| {
            commands
                .spawn((
                    SkeletonBar,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(row_px),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    Pickable::IGNORE,
                ))
                .id()
        })
        .collect();
    commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Percent(100.0),
                row_gap: Val::Px(space::LIST_ITEMS),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .add_children(&bars)
        .id()
}

pub fn pulse_skeletons(
    time: Res<Time>,
    palette: Res<Palette>,
    mut bars: Query<&mut BackgroundColor, With<SkeletonBar>>,
) {
    let wave = 0.5 + 0.5 * (time.elapsed_secs() * TAU / PULSE_SECS).sin();
    let alpha = PULSE_LOW + (1.0 - PULSE_LOW) * wave;
    let color = palette.track.with_alpha(alpha);
    for mut background in &mut bars {
        background.set_if_neq(BackgroundColor(color));
    }
}
