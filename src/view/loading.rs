//! A bar across the top of a frame while anything it shows is being fetched.
//!
//! Indeterminate on purpose: streaming has no end to measure against, since
//! what is wanted changes with every pan and zoom. The bar says only that more
//! is on its way, and keeps sweeping while what has already arrived is drawn
//! under it.

use bevy::prelude::*;
use bevy_feathers::theme::ThemeBackgroundColor;

use crate::app::theme::token;
use crate::source::{ShowsSource, SourceBusy};
use crate::view::chrome::SELECTION_PX;
use crate::view::layers::stacked_sources;
use crate::view::{FrameArea, FrameLayers, LayerOf, Panel, PendingShow, SelectedPanel};
use crate::widgets::patch_node;

const HEIGHT_PX: f32 = 3.0;
/// How much of the frame's width the moving segment covers, in percent.
const SWEEP_PERCENT: f32 = 30.0;
/// Seconds for the segment to cross the frame once.
const SWEEP_SECS: f32 = 1.4;
/// Seconds the bar stays up after the last fetch lands. Tiles arrive in bursts
/// a frame or two apart, and a bar that went out in every gap flickered.
const LINGER_SECS: f32 = 0.3;

/// One frame's loading bar.
#[derive(Component, Clone)]
pub struct LoadingBar {
    pub panel: Entity,
    /// Seconds since anything in the frame was last fetching.
    idle_for: f32,
}

impl Default for LoadingBar {
    fn default() -> Self {
        LoadingBar {
            panel: Entity::PLACEHOLDER,
            // Starts out as long idle, so a frame over a source with nothing
            // to fetch never shows the bar at all.
            idle_for: LINGER_SECS,
        }
    }
}

/// The segment that moves along a loading bar.
#[derive(Component, Clone, Default)]
pub struct LoadingSweep;

/// Keep one loading bar per frame, and drop those of frames that have gone.
pub fn sync_loading_bars(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    bars: Query<(Entity, &LoadingBar)>,
) {
    for (entity, bar) in &bars {
        if panels.get(bar.panel).is_err() {
            commands.entity(entity).despawn();
        }
    }
    for panel in &panels {
        if bars.iter().any(|(_, bar)| bar.panel == panel) {
            continue;
        }
        commands.spawn_scene(bsn! {
            LoadingBar { panel: { panel } }
            Node {
                position_type: { PositionType::Absolute },
                display: { Display::None },
                height: { Val::Px(HEIGHT_PX) },
                overflow: { Overflow::clip() },
            }
            // Decoration along the frame's edge, like the outline under it.
            template_value(Pickable::IGNORE)
            Children [(
                LoadingSweep
                Node {
                    position_type: { PositionType::Absolute },
                    width: { Val::Percent(SWEEP_PERCENT) },
                    height: { Val::Percent(100.0) },
                }
                ThemeBackgroundColor({ token::LOADING })
                template_value(Pickable::IGNORE)
            )]
        });
    }
}

/// Show each frame's bar while any source in its stack is fetching, or while
/// the dataset it is about to show is still being read, and sweep it.
pub fn update_loading_bars(
    time: Res<Time>,
    area: Res<FrameArea>,
    selected: Res<SelectedPanel>,
    panels: Query<(
        &Panel,
        Option<&ShowsSource>,
        Option<&FrameLayers>,
        Has<PendingShow>,
    )>,
    layer_cameras: Query<&ShowsSource, With<LayerOf>>,
    busy: Query<&SourceBusy>,
    mut bars: Query<(&mut LoadingBar, &mut Node)>,
    mut sweeps: Query<&mut Node, (With<LoadingSweep>, Without<LoadingBar>)>,
) {
    let count = panels.iter().count();
    let mut any_shown = false;
    for (mut bar, node) in &mut bars {
        let Ok((panel, shows, layers, pending)) = panels.get(bar.panel) else {
            continue;
        };
        // An empty frame has nothing streaming, only what it is waiting on.
        let fetching = pending
            || shows.is_some_and(|shows| {
                stacked_sources(shows, layers, &layer_cameras)
                    .iter()
                    .any(|source| busy.get(*source).is_ok_and(|busy| busy.0))
            });
        bar.idle_for = if fetching {
            0.0
        } else {
            bar.idle_for + time.delta_secs()
        };

        let shown = bar.idle_for < LINGER_SECS;
        if !shown {
            patch_node(node, |node| node.display = Display::None);
            continue;
        }
        any_shown = true;
        // Inside the selection outline where there is one, rather than over
        // it; flush with the edge everywhere else, where an inset would leave
        // a sliver of the frame showing above the bar.
        let inset = if outlined(count, selected.0 == Some(bar.panel)) {
            SELECTION_PX
        } else {
            0.0
        };
        let cell = area.cell(count, panel.index);
        patch_node(node, |node| {
            node.display = Display::Flex;
            node.left = Val::Px(cell.min.x + inset);
            node.top = Val::Px(cell.min.y + inset);
            node.width = Val::Px((cell.width() - 2.0 * inset).max(0.0));
        });
    }

    // A hidden bar's sweep moving would still send the UI back through layout.
    if !any_shown {
        return;
    }
    let left = sweep_left(time.elapsed_secs());
    for mut node in &mut sweeps {
        node.left = Val::Percent(left);
    }
}

/// Whether a frame carries the selection outline, by the rule
/// `update_selection_border` draws it by: only the selected frame, and only
/// when there is more than one to choose between.
fn outlined(frames: usize, selected: bool) -> bool {
    selected && frames > 1
}

/// Where the segment starts, in percent of the bar: entering from beyond the
/// left edge and leaving past the right, so it never sits still at either end.
fn sweep_left(elapsed: f32) -> f32 {
    let phase = (elapsed / SWEEP_SECS).fract();
    phase * (100.0 + SWEEP_PERCENT) - SWEEP_PERCENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_outlined_frame_is_inset() {
        assert!(!outlined(1, true));
        assert!(!outlined(2, false));
        assert!(outlined(2, true));
    }

    #[test]
    fn the_sweep_crosses_the_whole_bar_and_back_off_it() {
        assert_eq!(sweep_left(0.0), -SWEEP_PERCENT);
        assert!((sweep_left(SWEEP_SECS * 0.999) - 100.0).abs() < 0.5);
        assert_eq!(sweep_left(SWEEP_SECS * 1.5), sweep_left(SWEEP_SECS * 0.5));
    }
}
