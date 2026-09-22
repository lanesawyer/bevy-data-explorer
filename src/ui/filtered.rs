//! Whether points the filters leave out are drawn, and in what color: for the
//! selected source in the view configuration, and in the settings for what
//! every point cloud opens with.

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::{
    ColorChannel, ColorPlaneValue, ColorSwatchValue, FeathersCheckbox, FeathersColorPlane,
    FeathersColorSlider, FeathersColorSwatch, SliderBaseColor,
};
use bevy_ui_widgets::{SliderValue, ValueChange};

use crate::app::prefs::Preferences;
use crate::app::schedule::Stage;
use crate::source::ShowsSource;
use crate::source::properties::{CellProperties, FILTERED_GRAY, FilteredPoints};
use crate::view::SelectedPanel;
use crate::widgets::space;
use crate::widgets::{BlocksFrameInput, button_text, patch_node};

/// Height of the hue and saturation plane. Feathers' own minimum, which is
/// room enough to aim at in a sidebar.
const PLANE_PX: f32 = 100.0;

/// Below this saturation a color has no hue to speak of, and reading one back
/// from it would throw the plane's thumb to the left edge.
const GRAY: f32 = 1e-3;

/// Which setting a control edits.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum FilteredTarget {
    /// The selected frame's source.
    #[default]
    Selected,
    /// The preference new sources start from.
    Default,
}

#[derive(Component, Clone, Default)]
pub struct FilteredBox(pub FilteredTarget);

/// The picker under the box, hidden while filtered-out points are not drawn.
#[derive(Component, Clone, Default)]
pub struct FilteredPicker(pub FilteredTarget);

/// The swatch showing the color picked.
#[derive(Component, Clone, Default)]
pub struct FilteredSwatch(pub FilteredTarget);

/// Hue across, saturation down.
#[derive(Component, Clone, Default)]
pub struct FilteredPlane(pub FilteredTarget);

#[derive(Component, Clone, Default)]
pub struct FilteredLightness(pub FilteredTarget);

/// The box, and beneath it a picker: a swatch, a hue and saturation plane, and
/// a lightness slider.
pub fn filtered_controls(target: FilteredTarget) -> impl Scene {
    bsn! {
        Node {
            flex_direction: { FlexDirection::Column },
            row_gap: { Val::Px(space::ROWS) },
        }
        Children [
            (
                @FeathersCheckbox {
                    @caption: { bsn_list![button_text("Draw filtered-out points")] }
                }
                BlocksFrameInput
                FilteredBox({ target })
            ),
            (
                FilteredPicker({ target })
                Node {
                    flex_direction: { FlexDirection::Column },
                    row_gap: { Val::Px(space::ROWS) },
                }
                Children [
                    (
                        @FeathersColorSwatch
                        FilteredSwatch({ target })
                    ),
                    (
                        @FeathersColorPlane
                        FeathersColorPlane::HueSaturation
                        Node { height: { Val::Px(PLANE_PX) } }
                        BlocksFrameInput
                        FilteredPlane({ target })
                    ),
                    (
                        @FeathersColorSlider {
                            @channel: { ColorChannel::HslLightness },
                            @value: { Hsla::from(FILTERED_GRAY).lightness }
                        }
                        BlocksFrameInput
                        FilteredLightness({ target })
                    ),
                ]
            ),
        ]
    }
}

/// Every source with cells to filter starts as the preferences say. A
/// bookmark being restored may already have said otherwise, and wins.
fn start_from_preference(
    mut commands: Commands,
    prefs: Res<Preferences>,
    sources: Query<Entity, (With<CellProperties>, Without<FilteredPoints>)>,
) {
    for source in &sources {
        commands
            .entity(source)
            .insert_if_new(prefs.filtered_points());
    }
}

fn on_box_toggled(
    change: On<ValueChange<bool>>,
    boxes: Query<&FilteredBox>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut FilteredPoints>,
    mut prefs: ResMut<Preferences>,
) {
    let Ok(FilteredBox(target)) = boxes.get(change.source) else {
        return;
    };
    match target {
        FilteredTarget::Selected => {
            if let Some(source) = crate::view::selected_source(&selected, &panels)
                && let Ok(mut filtered) = sources.get_mut(source)
            {
                filtered.shown = change.value;
            }
        }
        FilteredTarget::Default => {
            let mut filtered = prefs.filtered_points();
            filtered.shown = change.value;
            prefs.filtered_points = Some(filtered.saved());
        }
    }
}

/// The setting a control edits, as it stands.
fn current(
    target: FilteredTarget,
    prefs: &Preferences,
    selected: &SelectedPanel,
    panels: &Query<&ShowsSource>,
    sources: &Query<&mut FilteredPoints>,
) -> Option<FilteredPoints> {
    match target {
        FilteredTarget::Selected => crate::view::selected_source(selected, panels)
            .and_then(|source| sources.get(source).ok())
            .copied(),
        FilteredTarget::Default => Some(prefs.filtered_points()),
    }
}

/// Change the color a control edits. Held as HSL so a gray keeps the hue it
/// was dragged to, rather than the plane's thumb jumping as saturation reaches
/// nothing.
fn recolor(
    target: FilteredTarget,
    prefs: &mut Preferences,
    selected: &SelectedPanel,
    panels: &Query<&ShowsSource>,
    sources: &mut Query<&mut FilteredPoints>,
    change: impl FnOnce(&mut Hsla),
) {
    let Some(mut filtered) = current(target, prefs, selected, panels, sources) else {
        return;
    };
    let mut color = Hsla::from(filtered.color);
    change(&mut color);
    filtered.color = Color::Hsla(color);
    match target {
        FilteredTarget::Selected => {
            if let Some(source) = crate::view::selected_source(selected, panels)
                && let Ok(mut current) = sources.get_mut(source)
            {
                current.set_if_neq(filtered);
            }
        }
        FilteredTarget::Default => prefs.filtered_points = Some(filtered.saved()),
    }
}

fn on_plane(
    change: On<ValueChange<Vec2>>,
    planes: Query<&FilteredPlane>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut FilteredPoints>,
    mut prefs: ResMut<Preferences>,
) {
    let Ok(FilteredPlane(target)) = planes.get(change.source) else {
        return;
    };
    let value = change.value;
    recolor(
        *target,
        &mut prefs,
        &selected,
        &panels,
        &mut sources,
        |color| {
            color.hue = value.x * 360.0;
            color.saturation = 1.0 - value.y;
        },
    );
}

fn on_lightness(
    change: On<ValueChange<f32>>,
    sliders: Query<&FilteredLightness>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut FilteredPoints>,
    mut prefs: ResMut<Preferences>,
) {
    let Ok(FilteredLightness(target)) = sliders.get(change.source) else {
        return;
    };
    let value = change.value;
    recolor(
        *target,
        &mut prefs,
        &selected,
        &panels,
        &mut sources,
        |color| {
            color.lightness = value;
        },
    );
}

/// Show each control as the setting it edits stands.
fn sync_filtered_controls(
    mut commands: Commands,
    prefs: Res<Preferences>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&mut FilteredPoints>,
    boxes: Query<(Entity, &FilteredBox, Has<Checked>)>,
    mut pickers: Query<(&FilteredPicker, &mut Node)>,
    mut swatches: Query<(&FilteredSwatch, &mut ColorSwatchValue)>,
    mut planes: Query<(&FilteredPlane, &mut ColorPlaneValue)>,
    mut sliders: Query<(
        &FilteredLightness,
        &SliderValue,
        &mut SliderBaseColor,
        Entity,
    )>,
) {
    let current = |target| current(target, &prefs, &selected, &panels, &sources);
    for (entity, FilteredBox(target), checked) in &boxes {
        let Some(filtered) = current(*target) else {
            continue;
        };
        if filtered.shown && !checked {
            commands.entity(entity).insert(Checked);
        } else if !filtered.shown && checked {
            commands.entity(entity).remove::<Checked>();
        }
    }
    for (FilteredPicker(target), node) in &mut pickers {
        let display = match current(*target) {
            Some(filtered) if filtered.shown => Display::Flex,
            _ => Display::None,
        };
        patch_node(node, |node| node.display = display);
    }
    for (FilteredSwatch(target), mut swatch) in &mut swatches {
        if let Some(filtered) = current(*target)
            && swatch.0 != filtered.color
        {
            swatch.0 = filtered.color;
        }
    }
    for (FilteredPlane(target), mut plane) in &mut planes {
        let Some(filtered) = current(*target) else {
            continue;
        };
        let color = Hsla::from(filtered.color);
        let across = if color.saturation < GRAY {
            plane.0.x
        } else {
            color.hue / 360.0
        };
        let wanted = Vec3::new(across, 1.0 - color.saturation, color.lightness);
        if plane.0 != wanted {
            plane.0 = wanted;
        }
    }
    for (FilteredLightness(target), value, mut base, entity) in &mut sliders {
        let Some(filtered) = current(*target) else {
            continue;
        };
        if base.0 != filtered.color {
            base.0 = filtered.color;
        }
        let lightness = Hsla::from(filtered.color).lightness;
        if (value.0 - lightness).abs() > 1e-4 {
            commands.entity(entity).insert(SliderValue(lightness));
        }
    }
}

pub struct FilteredControlsPlugin;

impl Plugin for FilteredControlsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_box_toggled)
            .add_observer(on_plane)
            .add_observer(on_lightness)
            .add_systems(Update, sync_filtered_controls.in_set(Stage::ControlsPlace))
            .add_systems(Update, start_from_preference.in_set(Stage::ControlsApply));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_survives_the_preferences() {
        let restored = FilteredPoints::default().saved().restored();
        assert!(restored.shown);
        let (a, b) = (restored.color.to_srgba(), FILTERED_GRAY.to_srgba());
        assert!((a.red - b.red).abs() < 1e-6 && (a.blue - b.blue).abs() < 1e-6);
    }
}
