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
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};
use bevy_feathers::tokens;
use bevy_ui_widgets::Activate;

use crate::app::schedule::{Boot, Stage};
use crate::source::{DataSource, SourceStatus};
use crate::view::{FrameArea, PanelRequest, SelectedSource};
use crate::widgets::space;
use crate::widgets::{
    AddDock, BlocksFrameInput, Dock, DockEdge, DockWidth, Icon, button_icon, dock_handle,
    place_right_dock, set_text, size, text,
};

const WIDTH_PX: f32 = 300.0;
const MIN_PX: f32 = 200.0;

#[derive(Resource)]
pub struct Inspector {
    pub width: DockWidth,
    pub open: bool,
}

impl Default for Inspector {
    fn default() -> Self {
        Inspector {
            width: DockWidth::new(WIDTH_PX, MIN_PX),
            open: false,
        }
    }
}

impl Inspector {
    /// Width the inspector occupies in a window this wide.
    pub fn current_width(&self, window_width: f32) -> f32 {
        if self.open {
            self.width.within(window_width)
        } else {
            0.0
        }
    }
}

impl Dock for Inspector {
    type Handle = InspectorHandle;
    const EDGE: DockEdge = DockEdge::Right;
    const KEY: &'static str = "inspector";
    const DEFAULT_SIZE: f32 = WIDTH_PX;

    fn size(&self) -> f32 {
        self.width.px
    }

    fn set_size(&mut self, size: f32) {
        self.width.set(size);
    }

    fn drag_to(&mut self, reach: f32, span: f32) {
        self.width.drag_to(reach, span);
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
            row_gap: { Val::Px(space::ROWS) },
            padding: { UiRect::all(Val::Px(space::PANEL_INSET)) },
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
                        text("Details", size::DOCK_TITLE)
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
                TextFont { font_size: { FontSize::Px(size::SECONDARY) } }
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
    selected: SelectedSource,
    sources: Query<(&DataSource, &SourceStatus)>,
    mut roots: Query<&mut Node, (With<InspectorRoot>, Without<InspectorHandle>)>,
    mut handles: Query<&mut Node, (With<InspectorHandle>, Without<InspectorRoot>)>,
    titles: Query<Entity, With<InspectorTitle>>,
    mut texts: Query<&mut Text>,
    bodies: Query<Entity, With<InspectorBody>>,
) {
    let Ok(window) = windows.single() else { return };
    let width = inspector.current_width(window.width());
    place_right_dock(
        &mut roots,
        &mut handles,
        inspector.open,
        width,
        window.width(),
    );
    if !inspector.open {
        return;
    }

    let source = selected.get(&sources);

    let (title, body) = match source {
        Some((data, status)) => (
            data.name.clone(),
            format!("{}\n{}\n\n{}", data.detail, data.stat, status.0),
        ),
        None => ("Details".to_string(), "No frame selected.".to_string()),
    };

    for entity in &titles {
        if let Ok(text) = texts.get_mut(entity) {
            set_text(text, &title);
        }
    }
    for entity in &bodies {
        if let Ok(text) = texts.get_mut(entity) {
            set_text(text, &body);
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
