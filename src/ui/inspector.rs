//! The docked inspector.
//!
//! Shows what is known about the selected frame. Like the sidebar it takes a
//! slice off [`FrameArea`] rather than talking to the grid, and it can be
//! dragged to any width between a readable minimum and half the window.
//!
//! It starts closed, because it is opened on demand from a frame's info button
//! rather than being somewhere to put things permanently.

use bevy::prelude::*;
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::display::label;
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};
use bevy_feathers::tokens;
use bevy_ui_widgets::Activate;

use crate::app::schedule::{Boot, Stage};
use crate::source::{DataSource, ShowsSource, SourceStatus};
use crate::view::{FrameArea, PanelRequest, SelectedPanel};
use crate::widgets::{
    AddDock, BlocksFrameInput, Dock, DockEdge, HANDLE_PX, Icon, button_icon, dock_handle,
};

const WIDTH_PX: f32 = 300.0;
const MIN_PX: f32 = 200.0;
const MAX_FRACTION: f32 = 0.5;

#[derive(Resource)]
pub struct Inspector {
    pub width: f32,
    pub open: bool,
}

impl Default for Inspector {
    fn default() -> Self {
        Inspector {
            width: WIDTH_PX,
            open: false,
        }
    }
}

impl Inspector {
    /// Width the inspector occupies in a window this wide: held to the most it
    /// may take, as the sidebar's is, so narrowing the window cannot leave the
    /// two covering every frame.
    pub fn current_width(&self, window_width: f32) -> f32 {
        if self.open {
            self.width.min(max_width(window_width))
        } else {
            0.0
        }
    }

    /// Width a drag reaching `from_right` in from the right edge should
    /// produce.
    ///
    /// Always a usable width. Dragging does not close the inspector: squeezing
    /// it to nothing leaves a dock that is still open but invisible, with no
    /// edge left to grab. Closing is the X button's job.
    fn width_for_drag(from_right: f32, window_width: f32) -> f32 {
        from_right.clamp(MIN_PX, max_width(window_width))
    }
}

impl Dock for Inspector {
    type Handle = InspectorHandle;
    const EDGE: DockEdge = DockEdge::Right;
    const KEY: &'static str = "inspector";
    const DEFAULT_SIZE: f32 = WIDTH_PX;

    fn size(&self) -> f32 {
        self.width
    }

    fn set_size(&mut self, size: f32) {
        self.width = size.max(MIN_PX);
    }

    fn drag_to(&mut self, reach: f32, span: f32) {
        self.width = Inspector::width_for_drag(reach, span);
    }
}

/// The widest the inspector may be in a window this wide.
fn max_width(window_width: f32) -> f32 {
    (window_width * MAX_FRACTION).max(MIN_PX)
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

fn spawn_inspector(mut commands: Commands) {
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
        ThemeBackgroundColor({ tokens::WINDOW_BG })
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
                        TextFont { font_size: { FontSize::Px(15.0f32) } }
                    ),
                    (
                        @FeathersToolButton {
                            @caption: { bsn_list![button_icon(Icon::X)] }
                        }
                        InspectorClose
                        BlocksFrameInput
                    ),
                ]
            ),
            (
                InspectorBody
                Text({ String::new() })
                TextFont { font_size: { FontSize::Px(12.0) } }
                ThemeTextColor({ tokens::TEXT_MAIN })
            ),
        ]
    });

    commands.spawn_scene(bsn! {
        InspectorHandle
        dock_handle(DockEdge::Right)
        Node {
            display: { Display::None },
        }
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
pub fn reserve_space(
    inspector: Res<Inspector>,
    windows: Query<&Window>,
    mut area: ResMut<FrameArea>,
) {
    let Ok(window) = windows.single() else { return };
    area.reserve_right(inspector.current_width(window.width()));
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
    let width = inspector.current_width(window.width());
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

/// The dock on the right, and the space it claims from the grid.
pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.add_dock::<Inspector>()
            .add_observer(close_inspector)
            .add_systems(Update, open_on_request.in_set(Stage::DockInput))
            .add_systems(Update, reserve_space.in_set(Stage::DockReserve))
            .add_systems(Update, update_inspector.in_set(Stage::Chrome))
            .add_systems(Startup, spawn_inspector.in_set(Boot::Shell));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_inspector_takes_no_space_from_the_grid() {
        let mut inspector = Inspector::default();
        assert!(!inspector.open);
        assert_eq!(inspector.current_width(1600.0), 0.0);
        inspector.open = true;
        assert_eq!(inspector.current_width(1600.0), 300.0);
    }

    #[test]
    fn dragging_never_squeezes_the_inspector_away() {
        // Collapsed to nothing it would still be open, with no edge left to
        // grab and no way back. Closing belongs to the X button.
        for reach in [200.0, 10.0, 0.0, -400.0] {
            assert_eq!(Inspector::width_for_drag(reach, 1600.0), MIN_PX);
        }
    }

    #[test]
    fn dragging_is_clamped_to_half_the_window() {
        assert_eq!(Inspector::width_for_drag(400.0, 1600.0), 400.0);
        assert_eq!(Inspector::width_for_drag(1500.0, 1600.0), 800.0);
    }

    #[test]
    fn a_narrow_window_does_not_invert_the_clamp() {
        // Half a small window is under the minimum; the result must still be a
        // width the grid can survive.
        assert_eq!(Inspector::width_for_drag(300.0, 300.0), MIN_PX);
    }

    #[test]
    fn narrowing_the_window_narrows_a_wide_inspector() {
        let inspector = Inspector {
            width: 800.0,
            open: true,
        };
        assert_eq!(inspector.current_width(800.0), 400.0);
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
