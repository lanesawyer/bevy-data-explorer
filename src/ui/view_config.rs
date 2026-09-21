//! The view configuration section of the sidebar.
//!
//! Shows the settings that apply to whichever frame is selected, starting with
//! transparency. The section is built once; its contents re-read the selection
//! every frame, so it follows the frame you last touched rather than being
//! rebuilt when the selection moves.

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::{FeathersCheckbox, FeathersToolButton};
use bevy_feathers::theme::ThemeTextColor;
use bevy_ui_widgets::{Activate, SliderRange, SliderValue, ValueChange};

use crate::app::schedule::{Boot, Stage};
use crate::render::points::{DEFAULT_POINT_PX, MAX_POINT_PX, MIN_POINT_PX, SourcePointSize};
use crate::render::settings::SourceOpacity;
use crate::source::stack::{SliceGrid, SliceStack};
use crate::source::{DataSource, ShowsSource};
use crate::ui::filtered::{FilteredTarget, filtered_controls};
use crate::ui::sidebar::{SectionOrder, SidebarContent};
use crate::view::SelectedPanel;
use crate::widgets::{
    BlocksFrameInput, Icon, SectionLevel, button_icon, button_text, caption, size, spawn_accordion,
    spawn_menu, spawn_slider, text,
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

/// A control that acts on the selected frame, and so has nothing to act on
/// while none is selected.
///
/// The point size row carries its own rule — a source may have no point size to
/// set even when it is selected — and that rule already covers there being no
/// selection at all, so it is not marked with this.
#[derive(Component, Clone, Default)]
pub struct FrameControl;

/// Above the per-dataset sections: it acts on the selected frame whatever
/// that frame is showing.
const SECTION_ORDER: u32 = 10;

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
        .insert(SectionOrder(SECTION_ORDER));
    commands.entity(parent).add_child(accordion.section);
    let menu = spawn_menu(&mut commands, accordion.header);
    commands.entity(menu).insert(LayoutMenu);

    // The frame rows are rebuilt wholesale, so they get a container of their
    // own: the custom dataset field below them holds what has been typed into
    // it, and would lose it every time a frame was opened or closed.
    let rows = commands
        .spawn_scene(bsn! {
            LayoutRows
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                row_gap: { Val::Px(2.0) },
            }
        })
        .id();
    // The picker is outside the rows too, for the same reason: it holds its
    // search. It is the one on every frame's title, opening into a new frame
    // instead of repointing one.
    let open = commands
        .spawn_scene(bsn! {
            text("Open a dataset", size::SECONDARY)
            Node { margin: { UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(8.0), Val::Px(2.0)) } }
        })
        .id();
    let picker = crate::view::dataset_menu::spawn_dataset_picker(
        &mut commands,
        crate::view::dataset_menu::PickerTarget::NewFrame,
    );
    let custom = crate::ui::add_source::spawn_custom_section(&mut commands);
    commands
        .entity(menu)
        .add_children(&[rows, open, picker, custom]);

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
        .spawn_scene(bsn! {
            FrameControl
            text("Transparency", size::SECONDARY)
        })
        .id();

    // Percent rather than a fraction, so the slider's own readout is a whole
    // number that means something without a separate caption beside it.
    let slider = spawn_slider(&mut commands, 100.0, (0.0, PERCENT), 0);
    commands
        .entity(slider)
        .insert((OpacitySlider, FrameControl));

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
    tables: Query<(), With<crate::source::table::SourceTable>>,
    slider: Query<(Entity, &SliderValue), With<OpacitySlider>>,
    mut names: Query<&mut Text, With<SelectedName>>,
    mut controls: Query<&mut Node, With<FrameControl>>,
    mut shown: Local<Option<Entity>>,
) {
    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0);

    // A control with nothing to act on is worse than no control: it invites a
    // drag that changes nothing. With no frame selected the section says so and
    // shows nothing else — and a frame full of rows has no geometry to fade,
    // since its table is chrome drawn over the frame rather than into it.
    let wanted = if source.is_some_and(|source| !tables.contains(source)) {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut controls {
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let Ok((slider_entity, value)) = slider.single() else {
        return;
    };
    let Some(source) = source else {
        for mut text in &mut names {
            text.0 = "no frame selected".into();
        }
        return;
    };
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
        app.add_observer(on_layout_button)
            .add_observer(on_slice_grid_toggled)
            .add_systems(Update, rebuild_layout_menu.in_set(Stage::ControlsBuild))
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

/// The menu that edits which visualizations are on screen.
#[derive(Component, Clone, Default)]
pub struct LayoutMenu;

/// Anything the menu rebuilds, so a rebuild can clear what it made.
#[derive(Component, Clone, Default)]
pub struct LayoutContent;

/// The part of the menu that is rebuilt, which is everything above the custom
/// dataset field.
#[derive(Component, Clone, Default)]
pub struct LayoutRows;

/// What a button in the layout menu does.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum LayoutAction {
    #[default]
    Clone,
    Remove,
}

#[derive(Component, Clone)]
pub struct LayoutButton {
    pub panel: Entity,
    pub action: LayoutAction,
}

impl Default for LayoutButton {
    fn default() -> Self {
        LayoutButton {
            panel: Entity::PLACEHOLDER,
            action: LayoutAction::Clone,
        }
    }
}

/// Rebuild the menu when the set of frames changes.
///
/// Rebuilding wholesale rather than reconciling row by row is fine at this
/// size: the grid holds at most eight frames, and the list only changes when
/// one is added or removed.
pub fn rebuild_layout_menu(
    mut commands: Commands,
    menus: Query<Entity, With<LayoutRows>>,
    panels: Query<(Entity, &crate::view::Panel, &ShowsSource)>,
    sources: Query<(Entity, &DataSource)>,
    content: Query<Entity, With<LayoutContent>>,
    mut previous: Local<Option<Vec<(Entity, Entity)>>>,
) {
    let Ok(menu) = menus.single() else { return };

    let mut frames: Vec<(usize, Entity, Entity)> = panels
        .iter()
        .map(|(entity, panel, shows)| (panel.index, entity, shows.0))
        .collect();
    frames.sort_by_key(|(index, _, _)| *index);
    let current: Vec<(Entity, Entity)> = frames
        .iter()
        .map(|(_, panel, source)| (*panel, *source))
        .collect();

    if previous.as_ref() == Some(&current) {
        return;
    }
    *previous = Some(current.clone());

    for entity in &content {
        commands.entity(entity).despawn();
    }

    let mut children = vec![heading(&mut commands, "Edit layout", 15.0, 0.0)];

    for (panel, source) in &current {
        let Ok((_, data)) = sources.get(*source) else {
            continue;
        };
        children.push(frame_row(&mut commands, *panel, data));
    }

    commands.entity(menu).add_children(&children);
}

fn heading(commands: &mut Commands, content: &str, size: f32, gap: f32) -> Entity {
    let content = content.to_string();
    commands
        .spawn_scene(bsn! {
            LayoutContent
            text(content, size)
            Node { margin: { UiRect::top(Val::Px(gap)) } }
        })
        .id()
}

/// Name, description and headline figure for one dataset.
fn summary(commands: &mut Commands, data: &DataSource) -> Entity {
    let name = data.name.clone();
    let row = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                flex_grow: { 1.0_f32 },
                row_gap: { Val::Px(1.0) },
            }
            Children [(
                text(name, size::BODY)
            )]
        })
        .id();

    // Provenance and headline figure, dimmed the way Feathers dims captions.
    let detail = caption(commands, data.detail.clone());
    let stat = caption(commands, data.stat.clone());
    commands.entity(row).add_children(&[detail, stat]);
    row
}

fn frame_row(commands: &mut Commands, panel: Entity, data: &DataSource) -> Entity {
    let row = commands
        .spawn_scene(bsn! {
            LayoutContent
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
                padding: { UiRect::vertical(Val::Px(6.0)) },
            }
        })
        .id();

    let details = summary(commands, data);
    let clone = action_button(commands, panel, LayoutAction::Clone, Icon::CopyPlus);
    // Every frame offers to close, the last one included: the window it leaves
    // behind offers the examples again.
    let remove = action_button(commands, panel, LayoutAction::Remove, Icon::X);
    commands.entity(row).add_children(&[details, clone, remove]);
    row
}

fn action_button(
    commands: &mut Commands,
    panel: Entity,
    action: LayoutAction,
    icon: Icon,
) -> Entity {
    commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_icon(icon)] }
            }
            BlocksFrameInput
            LayoutButton { panel: { panel }, action: { action } }
        })
        .id()
}

/// Clone or close a frame from the menu.
///
/// Feathers controls report a press by triggering [`Activate`] on themselves
/// rather than by carrying an `Interaction`, so these are observers rather than
/// systems polling for a changed interaction.
///
/// Both raise the same requests as a frame's own corner buttons, so the two
/// routes cannot drift apart.
pub fn on_layout_button(
    activate: On<Activate>,
    buttons: Query<&LayoutButton>,
    mut requests: MessageWriter<crate::view::PanelRequest>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    requests.write(match button.action {
        LayoutAction::Clone => crate::view::PanelRequest::Duplicate(button.panel),
        LayoutAction::Remove => crate::view::PanelRequest::Close(button.panel),
    });
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
