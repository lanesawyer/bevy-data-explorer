//! The view configuration section of the sidebar.
//!
//! Shows the settings that apply to whichever frame is selected, starting with
//! transparency. The section is built once; its contents re-read the selection
//! every frame, so it follows the frame you last touched rather than being
//! rebuilt when the selection moves.

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::FeathersCheckbox;
use bevy_feathers::theme::ThemeTextColor;
use bevy_ui_widgets::{SliderRange, SliderValue, ValueChange};

use crate::app::schedule::{Boot, Stage};
use crate::render::points::{DEFAULT_POINT_PX, MAX_POINT_PX, MIN_POINT_PX, SourcePointSize};
use crate::render::settings::SourceOpacity;
use crate::source::stack::{SliceGrid, SliceStack};
use crate::source::table::SourceTable;
use crate::source::{DataSource, ShowsSource};
use crate::ui::filtered::{FilteredTarget, filtered_controls};
use crate::ui::sidebar::{SectionFor, SectionOrder, SidebarContent};
use crate::view::SelectedPanel;
use crate::widgets::{
    BlocksFrameInput, SectionLevel, button_text, size, spawn_accordion, spawn_slider, text,
};

/// The opacity slider runs 0..100, so its built-in readout is a percentage.
const PERCENT: f32 = 100.0;

/// The label naming the selected dataset.
#[derive(Component, Clone, Default)]
pub struct SelectedName;

/// The slider driving the selected source's opacity.
#[derive(Component, Clone, Default)]
pub struct OpacitySlider;

/// The slider driving the selected source's point size.
#[derive(Component, Clone, Default)]
pub struct PointSizeSlider;

/// The rows holding the point controls, hidden for sources without points.
#[derive(Component, Clone, Default)]
pub struct PointSizeRow;

/// Slices are whole numbers, so anything less than half of one is the slider
/// and the stack saying the same thing.
const HALF_SLICE: f32 = 0.5;

/// The slider that pages through a stack of slices.
#[derive(Component, Clone, Default)]
pub struct SliceSlider;

/// The row holding the paging control, hidden for sources with no stack.
#[derive(Component, Clone, Default)]
pub struct SliceRow;

/// The line naming which slice is showing.
#[derive(Component, Clone, Default)]
pub struct SliceReadout;

/// The box that shows every slice of a stack at once, for a stack that can be
/// laid out that way.
#[derive(Component, Clone, Default)]
pub struct SliceGridBox;

/// Above the per-dataset sections: it acts on the selected frame whatever
/// that frame is showing.
const SECTION_ORDER: u32 = 10;

/// Everything but a table, whose rows are chrome drawn over the frame rather
/// than into it, so there is nothing to fade, size or page.
fn applies(source: EntityRef) -> bool {
    !source.contains::<SourceTable>()
}

pub fn spawn_view_config(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let accordion = spawn_accordion(
        &mut commands,
        "View configuration",
        true,
        SectionLevel::Pane,
    );
    commands
        .entity(accordion.section)
        .insert((SectionOrder(SECTION_ORDER), SectionFor(applies)));
    commands.entity(parent).add_child(accordion.section);
    let body = accordion.body;

    let name = commands
        .spawn_scene(bsn! {
            SelectedName
            Text({ String::new() })
            TextFont { font_size: { FontSize::Px(size::SECONDARY) } }
            ThemeTextColor({ bevy_feathers::tokens::TEXT_DIM })
        })
        .id();

    let transparency = commands
        .spawn_scene(text("Transparency", size::SECONDARY))
        .id();

    // Percent rather than a fraction, so the slider's own readout is a whole
    // number that means something without a separate caption beside it.
    let slider = spawn_slider(&mut commands, 100.0, (0.0, PERCENT), 0);
    commands.entity(slider).insert(OpacitySlider);

    let size_label = commands
        .spawn_scene(bsn! {
            PointSizeRow
            text("Point size", size::SECONDARY)
        })
        .id();
    let size_slider = spawn_slider(
        &mut commands,
        DEFAULT_POINT_PX,
        (MIN_POINT_PX, MAX_POINT_PX),
        1,
    );
    commands
        .entity(size_slider)
        .insert((PointSizeSlider, PointSizeRow));
    let filtered = commands
        .spawn_scene(bsn! {
            PointSizeRow
            filtered_controls(FilteredTarget::Selected)
        })
        .id();

    // Paging sits with the rest of what a frame shows. The control knows
    // nothing about images: it reads the stack off the source, which is a
    // source-layer component any format can advertise.
    let slice_label = commands
        .spawn_scene(bsn! {
            SliceRow
            SliceReadout
            Text({ String::new() })
            TextFont { font_size: { FontSize::Px(size::SECONDARY) } }
            ThemeTextColor({ bevy_feathers::tokens::TEXT_MAIN })
        })
        .id();
    // The range is rewritten for whichever source is selected; a stack's depth
    // is its own.
    let slice_slider = spawn_slider(&mut commands, 1.0, (1.0, 2.0), 0);
    commands
        .entity(slice_slider)
        .insert((SliceSlider, SliceRow));
    // Beside the paging it undoes: a step from the grid shows one slice, and
    // this is the way back.
    let grid_box = commands
        .spawn_scene(bsn! {
            @FeathersCheckbox {
                @caption: { bsn_list![button_text("Show every slice")] }
            }
            BlocksFrameInput
            SliceGridBox
        })
        .id();

    // Last, since how many rows it holds depends on the dataset.
    let channels = crate::ui::channels::spawn_channel_section(&mut commands);

    commands.entity(body).add_children(&[
        name,
        transparency,
        slider,
        size_label,
        size_slider,
        filtered,
        slice_label,
        slice_slider,
        grid_box,
        channels,
    ]);
}

/// Show the grid box for a stack that can be laid out every slice at once,
/// ticked as the selected source is.
pub fn sync_slice_grid(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    grids: Query<&SliceGrid>,
    mut boxes: Query<(Entity, &mut Node, Has<Checked>), With<SliceGridBox>>,
) {
    let grid =
        crate::view::selected_source(&selected, &panels).and_then(|source| grids.get(source).ok());
    for (entity, mut node, checked) in &mut boxes {
        let display = if grid.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let Some(grid) = grid else { continue };
        if grid.0 && !checked {
            commands.entity(entity).insert(Checked);
        } else if !grid.0 && checked {
            commands.entity(entity).remove::<Checked>();
        }
    }
}

/// Write a tick of the grid box through to the selected source.
pub fn on_slice_grid_toggled(
    change: On<ValueChange<bool>>,
    boxes: Query<(), With<SliceGridBox>>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut grids: Query<&mut SliceGrid>,
) {
    if !boxes.contains(change.source) {
        return;
    }
    if let Some(source) = crate::view::selected_source(&selected, &panels)
        && let Ok(mut grid) = grids.get_mut(source)
    {
        grid.set_if_neq(SliceGrid(change.value));
    }
}

/// Point the paging control at the selected source's stack, and write it back.
///
/// The same two-way shape as the other controls here: a change of selection
/// loads that source's slice into the slider rather than leaving the previous
/// source's on it, and a drag writes through to the stack — which is the only
/// thing the format watches.
pub fn sync_slice_slider(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut stacks: Query<&mut SliceStack>,
    slider: Query<(Entity, &SliderValue), With<SliceSlider>>,
    mut rows: Query<&mut Node, With<SliceRow>>,
    mut readouts: Query<&mut Text, With<SliceReadout>>,
    mut shown: Local<Option<(Entity, f32)>>,
) {
    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0)
        .filter(|source| stacks.get(*source).is_ok());

    for mut node in &mut rows {
        let wanted = if source.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let Ok((slider_entity, value)) = slider.single() else {
        return;
    };
    let Some(source) = source else {
        *shown = None;
        return;
    };
    let Ok(stack) = stacks.get(source) else {
        return;
    };
    let on_stack = stack.current as f32 + 1.0;

    // Which way the value is travelling has to be decided, because this is not
    // the only thing that pages: the frame's keys write to the same stack. A
    // slider that always wrote its own value back undid every keypress the
    // frame after it landed, which is exactly what it did.
    match *shown {
        Some((bound, last)) if bound == source => {
            if (value.0 - last).abs() > HALF_SLICE {
                // The slider was dragged, so the stack follows it. Counted from
                // one on the control, from zero in the stack.
                let wanted = (value.0.round() as i64 - 1).max(0) as u64;
                if let Ok(mut stack) = stacks.get_mut(source) {
                    stack.go_to(wanted);
                }
                *shown = Some((source, value.0));
            } else if (on_stack - value.0).abs() > HALF_SLICE {
                // Something else paged, so the slider follows the stack.
                commands.entity(slider_entity).insert(SliderValue(on_stack));
                *shown = Some((source, on_stack));
            }
        }
        _ => {
            // Selection moved: this stack's depth and place, not the last one's.
            commands.entity(slider_entity).insert((
                SliderRange::new(1.0, stack.count as f32),
                SliderValue(on_stack),
            ));
            *shown = Some((source, on_stack));
        }
    }

    if let Ok(stack) = stacks.get(source) {
        let label = stack.label();
        for mut text in &mut readouts {
            if text.0 != label {
                text.0 = label.clone();
            }
        }
    }
}

/// Point the slider at the selected source, and write its value back.
///
/// Runs both ways in one system so a change of selection loads that source's
/// opacity into the slider instead of the previous source's value leaking
/// across.
pub fn sync_opacity_slider(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<(&DataSource, Option<&mut SourceOpacity>)>,
    slider: Query<(Entity, &SliderValue), With<OpacitySlider>>,
    mut names: Query<&mut Text, With<SelectedName>>,
    mut shown: Local<Option<Entity>>,
) {
    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0);

    let Ok((slider_entity, value)) = slider.single() else {
        return;
    };
    let Some(source) = source else { return };
    let Ok((data, opacity)) = sources.get_mut(source) else {
        return;
    };

    for mut text in &mut names {
        if text.0 != data.name {
            text.0 = data.name.clone();
        }
    }

    match opacity {
        // Selection moved: load the newly selected source's own value into the
        // widget rather than letting the previous source's value leak across.
        Some(mut opacity) if *shown != Some(source) => {
            commands
                .entity(slider_entity)
                .insert(SliderValue(opacity.0 * PERCENT));
            opacity.set_changed();
        }
        Some(mut opacity) => {
            let wanted = value.0 / PERCENT;
            if (opacity.0 - wanted).abs() > f32::EPSILON {
                opacity.0 = wanted;
            }
        }
        None => {
            commands
                .entity(source)
                .insert(SourceOpacity(value.0 / PERCENT));
        }
    }
    *shown = Some(source);
}

/// The sidebar section that acts on the selected frame.
pub struct ViewConfigPlugin;

impl Plugin for ViewConfigPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_slice_grid_toggled)
            .add_systems(
                Update,
                (
                    sync_opacity_slider,
                    sync_point_size,
                    sync_slice_slider,
                    sync_slice_grid,
                )
                    .chain()
                    .in_set(Stage::ControlsPlace),
            )
            .add_systems(Startup, spawn_view_config.in_set(Boot::DockContent));
    }
}

/// Point the size slider at the selected source, and write its value back.
///
/// The control is hidden for sources that do not draw points, rather than
/// shown doing nothing.
pub fn sync_point_size(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut SourcePointSize>,
    slider: Query<(Entity, &SliderValue), With<PointSizeSlider>>,
    mut rows: Query<&mut Node, With<PointSizeRow>>,
    mut shown: Local<Option<Entity>>,
) {
    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0);

    let Ok((slider_entity, value)) = slider.single() else {
        return;
    };

    let sized = source.filter(|source| sources.get(*source).is_ok());
    for mut node in &mut rows {
        let wanted = if sized.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let Some(source) = sized else {
        *shown = None;
        return;
    };
    let Ok(mut size) = sources.get_mut(source) else {
        return;
    };

    if *shown != Some(source) {
        // Selection moved: load this source's own size rather than carrying
        // the previous source's across.
        commands.entity(slider_entity).insert(SliderValue(size.0));
        size.set_changed();
    } else if (size.0 - value.0).abs() > f32::EPSILON {
        size.0 = value.0;
    }
    *shown = Some(source);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slider works in percent while opacity is a fraction, so the two
    /// conversions have to agree or the value drifts every time the selection
    /// changes.
    #[test]
    fn percent_and_opacity_round_trip() {
        for opacity in [0.0f32, 0.25, 0.46, 0.5, 1.0] {
            let shown = opacity * PERCENT;
            assert!((shown / PERCENT - opacity).abs() < 1e-6);
        }
        assert_eq!(100.0 / PERCENT, 1.0);
        assert_eq!(0.0 / PERCENT, 0.0);
    }
}
