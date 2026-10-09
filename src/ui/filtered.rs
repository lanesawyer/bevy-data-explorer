//! Whether points the filters leave out are drawn, and in what color: for the
//! selected source in the view configuration, and in the settings for what
//! every point cloud opens with.

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::{ColorInputValue, FeathersCheckbox, FeathersColorInput};
use bevy_ui_widgets::ValueChange;

use crate::app::prefs::Preferences;
use crate::app::schedule::Stage;
use crate::source::properties::{CellProperties, FILTERED_GRAY, FilteredPoints};
use crate::view::SelectedSource;
use crate::widgets::space;
use crate::widgets::{BlocksFrameInput, button_text, patch_node, size, text};

/// Two colors this close are the same one held in two color spaces.
const SAME_COLOR: f32 = 0.5 / 255.0;

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

/// The row under the box holding the color, hidden while filtered-out points
/// are not drawn.
#[derive(Component, Clone, Default)]
pub struct FilteredPicker(pub FilteredTarget);

/// The color input the points are drawn in.
#[derive(Component, Clone, Default)]
pub struct FilteredColor(pub FilteredTarget);

/// The box, and beneath it the color they are drawn in: Feathers' color
/// input, as every other color square in the app is.
pub fn filtered_controls(target: FilteredTarget) -> impl Scene {
    bsn! {
        Node {
            flex_direction: { FlexDirection::Column },
            row_gap: { Val::Px(space::ROWS) },
        }
        Children [
            @FeathersCheckbox {
                @caption: { bsn_list! {@button_text("Draw filtered-out points")} }
            }
            BlocksFrameInput
            FilteredBox({ target })
            --
            FilteredPicker({ target })
            Node {
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(space::CONTROLS) },
            }
            Children [
                @FeathersColorInput
                ColorInputValue({ FILTERED_GRAY })
                BlocksFrameInput
                FilteredColor({ target })
                --
                @text("Drawn in", size::BODY)
            ]
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
    selected: SelectedSource,
    mut sources: Query<&mut FilteredPoints>,
    mut prefs: ResMut<Preferences>,
) {
    let Ok(FilteredBox(target)) = boxes.get(change.source) else {
        return;
    };
    match target {
        FilteredTarget::Selected => {
            if let Some(mut filtered) = selected.get_mut(&mut sources) {
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
    selected: Option<Entity>,
    sources: &Query<&mut FilteredPoints>,
) -> Option<FilteredPoints> {
    match target {
        FilteredTarget::Selected => selected
            .and_then(|source| sources.get(source).ok())
            .copied(),
        FilteredTarget::Default => Some(prefs.filtered_points()),
    }
}

/// Change the color a control edits.
fn recolor(
    target: FilteredTarget,
    prefs: &mut Preferences,
    selected: Option<Entity>,
    sources: &mut Query<&mut FilteredPoints>,
    color: Color,
) {
    let Some(mut filtered) = current(target, prefs, selected, sources) else {
        return;
    };
    filtered.color = color;
    match target {
        FilteredTarget::Selected => {
            if let Some(source) = selected
                && let Ok(mut current) = sources.get_mut(source)
            {
                current.set_if_neq(filtered);
            }
        }
        FilteredTarget::Default => prefs.filtered_points = Some(filtered.saved()),
    }
}

/// Draw filtered-out points in the color picked, telling the input its own
/// value back as Feathers' `color_input_self_update` would.
fn on_color_picked(
    change: On<ValueChange<Color>>,
    inputs: Query<&FilteredColor>,
    selected: SelectedSource,
    mut sources: Query<&mut FilteredPoints>,
    mut prefs: ResMut<Preferences>,
    mut commands: Commands,
) {
    let Ok(FilteredColor(target)) = inputs.get(change.source) else {
        return;
    };
    commands
        .entity(change.source)
        .insert(ColorInputValue(change.value));
    recolor(
        *target,
        &mut prefs,
        selected.entity(),
        &mut sources,
        change.value,
    );
}

/// Whether `a` and `b` are the same color, whatever space each is held in.
fn same_color(a: Color, b: Color) -> bool {
    let (a, b) = (a.to_srgba(), b.to_srgba());
    [a.red - b.red, a.green - b.green, a.blue - b.blue]
        .iter()
        .all(|gap| gap.abs() < SAME_COLOR)
}

/// Show each control as the setting it edits stands.
fn sync_filtered_controls(
    mut commands: Commands,
    prefs: Res<Preferences>,
    selected: SelectedSource,
    sources: Query<&mut FilteredPoints>,
    boxes: Query<(Entity, &FilteredBox, Has<Checked>)>,
    mut pickers: Query<(&FilteredPicker, &mut Node)>,
    inputs: Query<(Entity, &FilteredColor, &ColorInputValue)>,
) {
    let current = |target| current(target, &prefs, selected.entity(), &sources);
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
    for (entity, FilteredColor(target), value) in &inputs {
        if let Some(filtered) = current(*target)
            && !same_color(value.0, filtered.color)
        {
            commands
                .entity(entity)
                .insert(ColorInputValue(filtered.color));
        }
    }
}

pub struct FilteredControlsPlugin;

impl Plugin for FilteredControlsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_box_toggled)
            .add_observer(on_color_picked)
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
