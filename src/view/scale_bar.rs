//! A scale bar in each flat frame's bottom corner.
//!
//! Only where the source's unit is a length: a micron is the same micron in
//! every frame, so the bar can say how far across the tissue a stretch of
//! screen is. A slide measured in its own pixels has no length to report, and
//! a bar reading "500 px" would say nothing the zoom line does not.
//!
//! The bar's length is picked from 1, 2 and 5 times a power of ten, the
//! longest that fits in a fraction of the frame, and labeled in whichever of
//! nm, µm, mm and m keeps the number readable. It stays in a saved picture of
//! the frame, where it is what makes the picture a figure.

use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use bevy_feathers::constants::fonts;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};

use crate::app::schedule::Stage;
use crate::app::theme::token;
use crate::source::table::SourceTable;
use crate::source::{DataSource, ShowsSource, meters_per};
use crate::widgets::{patch_node, set_text, size, space};

use super::overlay::CHROME_INSET;
use super::{FrameArea, Panel};

/// The most of a frame's width the bar may take.
const MAX_FRACTION: f32 = 0.25;

/// Thickness of the bar itself, in logical pixels.
const BAR_PX: f32 = 3.0;

/// One frame's scale bar: a label over a bar, on the overlay's backing.
#[derive(Component, Clone)]
pub struct ScaleBar {
    panel: Entity,
}

impl Default for ScaleBar {
    fn default() -> Self {
        ScaleBar {
            panel: Entity::PLACEHOLDER,
        }
    }
}

#[derive(Component, Clone, Default)]
struct ScaleBarLabel;

#[derive(Component, Clone, Default)]
struct ScaleBarRule;

/// The bar for a frame zoomed to `units_per_px` of `unit`, at most `max_px`
/// long: its length in logical pixels, and what it reads.
fn scale_bar(units_per_px: f32, unit: &str, max_px: f32) -> Option<(f32, String)> {
    let meters_per_px = f64::from(units_per_px) * meters_per(unit)?;
    if !meters_per_px.is_finite() || meters_per_px <= 0.0 || max_px < 1.0 {
        return None;
    }
    let most = meters_per_px * f64::from(max_px);
    let power = 10f64.powf(most.log10().floor());
    let meters = [5.0, 2.0, 1.0]
        .into_iter()
        .map(|step| step * power)
        .find(|meters| *meters <= most)?;
    let px = (meters / meters_per_px) as f32;
    Some((px, label(meters)))
}

/// A length, in the largest of nm, µm, mm and m that keeps it at least one.
fn label(meters: f64) -> String {
    let (value, symbol) = [(1.0, "m"), (1e-3, "mm"), (1e-6, "µm"), (1e-9, "nm")]
        .into_iter()
        .find(|(scale, _)| meters >= scale * 0.999)
        .map_or((meters / 1e-9, "nm"), |(scale, symbol)| {
            (meters / scale, symbol)
        });
    format!("{} {symbol}", value.round())
}

/// Keep one scale bar per frame, and drop those whose frame has gone.
fn sync_scale_bars(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    bars: Query<(Entity, &ScaleBar)>,
) {
    for (entity, bar) in &bars {
        if panels.get(bar.panel).is_err() {
            commands.entity(entity).despawn();
        }
    }
    for panel in &panels {
        if !bars.iter().any(|(_, bar)| bar.panel == panel) {
            spawn_scale_bar(&mut commands, panel);
        }
    }
}

fn spawn_scale_bar(commands: &mut Commands, panel: Entity) {
    commands.spawn_scene(bsn! {
        ScaleBar { panel: { panel } }
        Node {
            position_type: { PositionType::Absolute },
            display: { Display::None },
            flex_direction: { FlexDirection::Column },
            align_items: { AlignItems::Center },
            row_gap: { Val::Px(space::STACKED) },
            padding: { UiRect::all(Val::Px(space::CONTROL_INSET)) },
            border_radius: { BorderRadius::all(Val::Px(5.0)) },
        }
        ThemeBackgroundColor({ token::OVERLAY_BG })
        // Drawn over the data but never in the way of it: the pointer passes
        // through to pan and hover the frame underneath.
        template_value(Pickable::IGNORE)
        Children [
            (
                ScaleBarLabel
                Text
                // Rewritten as the frame zooms, so it names the font itself
                // rather than coming through `widgets::text`.
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::REGULAR),
                    font_size: { FontSize::Px(size::SECONDARY) },
                }
                ThemeTextColor({ token::OVERLAY_TEXT })
                template_value(Pickable::IGNORE)
            ),
            (
                ScaleBarRule
                Node {
                    height: { Val::Px(BAR_PX) },
                }
                ThemeBackgroundColor({ token::OVERLAY_TEXT })
                template_value(Pickable::IGNORE)
            ),
        ]
    });
}

/// Size, label and place each bar for its frame's current zoom, or hide it
/// where there is no length to show.
fn update_scale_bars(
    windows: Query<&Window>,
    area: Res<FrameArea>,
    panels: Query<(&Panel, &Camera, &Projection, &ShowsSource)>,
    sources: Query<&DataSource>,
    tables: Query<(), With<SourceTable>>,
    mut bars: Query<(&ScaleBar, &Children, &mut Node)>,
    mut labels: Query<&mut Text, With<ScaleBarLabel>>,
    mut rules: Query<&mut Node, (With<ScaleBarRule>, Without<ScaleBar>)>,
) {
    let Ok(window) = windows.single() else { return };
    let count = panels.iter().count();
    for (bar, children, node) in &mut bars {
        let shown = panels
            .get(bar.panel)
            .ok()
            .filter(|(_, _, _, shows)| !tables.contains(shows.0))
            .and_then(|(panel, camera, projection, shows)| {
                let Projection::Orthographic(ortho) = projection else {
                    return None;
                };
                let source = sources.get(shows.0).ok()?;
                let viewport = camera.logical_viewport_size()?;
                let units_per_px = ortho.area.width() / viewport.x.max(1.0);
                let cell = area.cell(count, panel.index);
                scale_bar(units_per_px, &source.unit, cell.width() * MAX_FRACTION)
                    .map(|found| (cell, found))
            });

        let Some((cell, (px, text))) = shown else {
            patch_node(node, |node| node.display = Display::None);
            continue;
        };
        patch_node(node, |node| {
            node.display = Display::Flex;
            // Both measured from the window's far edges, not the cell's.
            node.right = Val::Px(window.width() - cell.max.x + CHROME_INSET);
            node.bottom = Val::Px(window.height() - cell.max.y + CHROME_INSET);
        });
        for child in children {
            if let Ok(label) = labels.get_mut(*child) {
                set_text(label, &text);
            }
            if let Ok(rule) = rules.get_mut(*child) {
                patch_node(rule, |rule| rule.width = Val::Px(px));
            }
        }
    }
}

/// A scale bar over every frame measured in a length.
pub struct ScaleBarPlugin;

impl Plugin for ScaleBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_scale_bars.in_set(Stage::FrameChrome))
            .add_systems(Update, update_scale_bars.in_set(Stage::Overlay));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_is_a_round_length_that_fits() {
        // 2 µm a pixel and 300 px to spare: 600 µm fits, so 500 µm is drawn.
        let (px, text) = scale_bar(2.0, "um", 300.0).unwrap();
        assert_eq!(text, "500 µm");
        assert!((px - 250.0).abs() < 1e-3);
    }

    #[test]
    fn scatterbrain_spells_its_unit_out() {
        // 0.01 mm a pixel, 250 px: 2.5 mm fits, so 2 mm.
        let (px, text) = scale_bar(0.01, "millimeter", 250.0).unwrap();
        assert_eq!(text, "2 mm");
        assert!((px - 200.0).abs() < 1e-3);
    }

    #[test]
    fn a_length_is_labeled_in_the_unit_that_reads_best() {
        assert_eq!(label(1e-3), "1 mm");
        assert_eq!(label(2e-4), "200 µm");
        assert_eq!(label(5e-8), "50 nm");
        assert_eq!(label(2.0), "2 m");
    }

    #[test]
    fn pixels_and_nothing_have_no_bar() {
        assert!(scale_bar(1.0, "px", 300.0).is_none());
        assert!(scale_bar(1.0, "", 300.0).is_none());
    }

    #[test]
    fn a_bar_never_overruns_its_room() {
        for units_per_px in [0.0003, 0.07, 1.3, 42.0, 900.0] {
            let (px, _) = scale_bar(units_per_px, "um", 180.0).unwrap();
            assert!(px <= 180.0 && px > 180.0 / 5.0, "{units_per_px}: {px}");
        }
    }
}
