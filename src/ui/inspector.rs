//! The docked inspector.
//!
//! Shows what is known about the selected frame. Like the sidebar it takes a
//! slice off [`FrameArea`] rather than talking to the grid, and it can be
//! dragged to any width between a readable minimum and half the window.
//!
//! It starts closed, because it is opened on demand from a frame's info button
//! rather than being somewhere to put things permanently.

use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, Interaction};
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::display::label;
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::Activate;

use crate::source::{DataSource, SourceStatus};
use crate::view::{BlocksFrameInput, FrameArea, PanelRequest, SelectedPanel, ShowsSource};

const MIN_PX: f32 = 200.0;
const MAX_FRACTION: f32 = 0.5;
const HANDLE_PX: f32 = 6.0;

#[derive(Resource)]
pub struct Inspector {
    pub width: f32,
    pub open: bool,
    resizing: bool,
}

impl Default for Inspector {
    fn default() -> Self {
        Inspector {
            width: 300.0,
            open: false,
            resizing: false,
        }
    }
}

impl Inspector {
    pub fn current_width(&self) -> f32 {
        if self.open { self.width } else { 0.0 }
    }

    pub fn resizing(&self) -> bool {
        self.resizing
    }

    /// Width a drag to `cursor_x` should produce, measured from the right edge
    /// since the inspector is docked there.
    ///
    /// Always a usable width. Dragging does not close the inspector: squeezing
    /// it to nothing leaves a dock that is still open but invisible, with no
    /// edge left to grab. Closing is the X button's job.
    fn width_for_drag(cursor_x: f32, window_width: f32) -> f32 {
        let from_right = window_width - cursor_x;
        from_right.clamp(MIN_PX, (window_width * MAX_FRACTION).max(MIN_PX))
    }
}

#[derive(Component, Clone, Default)]
pub struct InspectorRoot;

#[derive(Component, Clone, Default)]
pub struct InspectorHandle;

#[derive(Component, Clone, Default)]
pub struct InspectorTitle;

#[derive(Component, Clone, Default)]
pub struct InspectorBody;

#[derive(Component, Clone, Default)]
pub struct InspectorClose;

pub fn spawn_inspector(commands: &mut Commands) {
    commands.spawn_scene(bsn! {
        InspectorRoot
        BlocksFrameInput
        Node {
            position_type: { PositionType::Absolute },
            top: { Val::Px(0.0) },
            height: { Val::Percent(100.0) },
            display: { Display::None },
            flex_direction: { FlexDirection::Column },
            row_gap: { Val::Px(8.0) },
            padding: { UiRect::all(Val::Px(10.0)) },
        }
        BackgroundColor({ Color::srgb(0.09, 0.10, 0.13) })
        InheritableFont { font_size: { 13.0f32 } }
        Children [
            (
                Node {
                    width: { Val::Percent(100.0) },
                    align_items: { AlignItems::Center },
                    justify_content: { JustifyContent::SpaceBetween },
                }
                Children [
                    (
                        InspectorTitle
                        label("Details")
                        InheritableFont { font_size: { 15.0f32 } }
                    ),
                    (
                        @FeathersToolButton {
                            @caption: { bsn_list![label("x")] }
                        }
                        InspectorClose
                        BlocksFrameInput
                    ),
                ]
            ),
            (
                InspectorBody
                Text({ String::new() })
                TextFont { font_size: { bevy::text::FontSize::Px(12.0) } }
                TextColor({ Color::srgb(0.74, 0.79, 0.87) })
            ),
        ]
    });

    commands.spawn_scene(bsn! {
        InspectorHandle
        // Not a button, as in the sidebar: a press without the chrome.
        Interaction
        template_value(FocusPolicy::Block)
        BlocksFrameInput
        Node {
            position_type: { PositionType::Absolute },
            top: { Val::Px(0.0) },
            width: { Val::Px(HANDLE_PX) },
            height: { Val::Percent(100.0) },
            display: { Display::None },
        }
        BackgroundColor({ Color::srgb(0.20, 0.22, 0.28) })
    });
}

/// Open the inspector when a frame's info button asks for it.
pub fn open_on_request(
    mut requests: MessageReader<PanelRequest>,
    mut inspector: ResMut<Inspector>,
) {
    for request in requests.read() {
        if matches!(request, PanelRequest::Inspect(_)) {
            inspector.open = true;
        }
    }
}

pub fn close_inspector(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorClose>>,
    mut inspector: ResMut<Inspector>,
) {
    if buttons.get(activate.entity).is_ok() {
        inspector.open = false;
    }
}

/// Take the inspector's width off the frame grid.
pub fn reserve_space(inspector: Res<Inspector>, mut area: ResMut<FrameArea>) {
    area.reserve_right(inspector.current_width());
}

/// Drag the edge to resize, or far enough right to close.
pub fn resize_inspector(
    mut inspector: ResMut<Inspector>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    handle: Query<&Interaction, With<InspectorHandle>>,
) {
    let Ok(window) = windows.single() else { return };

    if handle.iter().any(|i| *i == Interaction::Pressed) {
        inspector.resizing = true;
    }
    if !mouse.pressed(MouseButton::Left) {
        inspector.resizing = false;
        return;
    }
    if !inspector.resizing {
        return;
    }

    let Some(cursor) = window.cursor_position() else {
        return;
    };
    inspector.width = Inspector::width_for_drag(cursor.x, window.width());
}

/// Show a resize cursor over the drag handle, and for as long as a drag lasts.
pub fn inspector_cursor(
    mut commands: Commands,
    inspector: Res<Inspector>,
    hover: Res<HoverMap>,
    handles: Query<Entity, With<InspectorHandle>>,
    windows: Query<(Entity, Option<&CursorIcon>), With<PrimaryWindow>>,
) {
    let over = inspector.resizing()
        || hover
            .values()
            .flat_map(|hits| hits.keys())
            .any(|hovered| handles.get(*hovered).is_ok());
    if !over && !inspector.is_changed() {
        return;
    }
    let wanted = if over {
        CursorIcon::System(SystemCursorIcon::ColResize)
    } else {
        return;
    };
    for (window, current) in &windows {
        if current != Some(&wanted) {
            commands.entity(window).insert(wanted.clone());
        }
    }
}

/// Match the inspector's chrome to its width, and fill it from the selection.
pub fn update_inspector(
    inspector: Res<Inspector>,
    windows: Query<&Window>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<(&DataSource, &SourceStatus)>,
    mut roots: Query<&mut Node, (With<InspectorRoot>, Without<InspectorHandle>)>,
    mut handles: Query<&mut Node, (With<InspectorHandle>, Without<InspectorRoot>)>,
    titles: Query<Entity, With<InspectorTitle>>,
    mut texts: Query<&mut Text>,
    bodies: Query<Entity, With<InspectorBody>>,
) {
    let Ok(window) = windows.single() else { return };
    let width = inspector.current_width();
    let shown = if inspector.open {
        Display::Flex
    } else {
        Display::None
    };

    for mut node in &mut roots {
        node.display = shown;
        node.width = Val::Px(width);
        node.left = Val::Px(window.width() - width);
    }
    for mut node in &mut handles {
        node.display = shown;
        // Straddles the edge so it can be grabbed from either side.
        node.left = Val::Px(window.width() - width - HANDLE_PX * 0.5);
    }
    if !inspector.open {
        return;
    }

    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok());

    let (title, body) = match source {
        Some((data, status)) => (
            data.name.clone(),
            format!("{}\n{}\n\n{}", data.detail, data.stat, status.0),
        ),
        None => ("Details".to_string(), "No frame selected.".to_string()),
    };

    for entity in &titles {
        if let Ok(mut text) = texts.get_mut(entity)
            && text.0 != title
        {
            text.0 = title.clone();
        }
    }
    for entity in &bodies {
        if let Ok(mut text) = texts.get_mut(entity)
            && text.0 != body
        {
            text.0 = body.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_inspector_takes_no_space_from_the_grid() {
        let mut inspector = Inspector::default();
        assert!(!inspector.open);
        assert_eq!(inspector.current_width(), 0.0);
        inspector.open = true;
        assert_eq!(inspector.current_width(), 300.0);
    }

    #[test]
    fn dragging_measures_from_the_right_edge() {
        // Docked right, so a cursor far from that edge means a wide panel.
        assert_eq!(Inspector::width_for_drag(1200.0, 1600.0), 400.0);
    }

    #[test]
    fn dragging_never_squeezes_the_inspector_away() {
        // Collapsed to nothing it would still be open, with no edge left to
        // grab and no way back. Closing belongs to the X button.
        for cursor in [1400.0, 1590.0, 1600.0, 2000.0] {
            assert_eq!(Inspector::width_for_drag(cursor, 1600.0), MIN_PX);
        }
    }

    #[test]
    fn dragging_is_clamped_to_half_the_window() {
        assert_eq!(Inspector::width_for_drag(100.0, 1600.0), 800.0);
    }

    #[test]
    fn a_narrow_window_does_not_invert_the_clamp() {
        // Half a small window is under the minimum; the result must still be a
        // width the grid can survive.
        assert_eq!(Inspector::width_for_drag(0.0, 300.0), MIN_PX);
    }

    #[test]
    fn the_grid_keeps_room_when_both_docks_are_open() {
        let mut area = FrameArea {
            origin: Vec2::ZERO,
            size: Vec2::new(1600.0, 900.0),
        };
        area.reserve_left(260.0);
        area.reserve_right(300.0);
        assert_eq!(area.origin.x, 260.0);
        assert_eq!(area.size.x, 1040.0);
    }
}
