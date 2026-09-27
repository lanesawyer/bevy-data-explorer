//! The docked inspector.
//!
//! Shows what is known about the selected frame. Like the sidebar it takes a
//! slice off [`FrameArea`] rather than talking to the grid, and it can be
//! dragged to any width between a readable minimum and half the window.
//!
//! It starts closed, because it is opened on demand from a frame's info button
//! rather than being somewhere to put things permanently.
//!
//! A table's frame opens it too, when a row is clicked: under the frame's
//! details it lists everything that row holds, columns the frame hides
//! included. A value naming a dataset — a Neuroglancer link, a Zarr store —
//! gets a button opening it in a frame of its own, through `discover` like
//! any address typed in.

use bevy::clipboard::Clipboard;
use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersToolButton};
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};
use bevy_feathers::tokens;
use bevy_ui_widgets::Activate;

use crate::app::schedule::{Boot, Stage};
use crate::formats::discover::datasets_in;
use crate::source::table::{Record, SelectedRecord};
use crate::source::{DataSource, SourceStatus};
use crate::view::{DatasetRequest, DatasetTarget, PanelRequest, SelectedSource};
use crate::widgets::space;
use crate::widgets::{
    AddDock, BlocksFrameInput, Dock, DockEdge, DockWidth, Icon, button_icon, button_text,
    dock_handle, place_right_dock, scroll_list, set_text, size, text, text_dim,
};

const WIDTH_PX: f32 = 300.0;
const MIN_PX: f32 = 200.0;

/// Taller than any window, so the record scrolls against the room the dock
/// leaves it rather than at some arbitrary point.
const RECORD_MAX_PX: f32 = 4000.0;

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

    fn taken(&self, window: Vec2) -> f32 {
        self.current_width(window.x)
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

/// The list the picked record's fields are built into.
#[derive(Component, Clone, Default)]
pub struct InspectorRecord;

/// A button copying a field's value.
#[derive(Component, Clone, Default)]
pub struct CopyField {
    value: String,
}

/// A button opening the dataset a field names in a frame of its own.
#[derive(Component, Clone, Default)]
pub struct OpenFieldDataset {
    url: String,
}

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
            (
                InspectorRecord
                scroll_list(RECORD_MAX_PX)
                Node {
                    display: { Display::None },
                    flex_grow: { 1.0_f32 },
                    min_height: { Val::ZERO },
                    row_gap: { Val::Px(space::ROWS) },
                }
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

/// Build the picked record of the selected frame's table into the inspector,
/// whenever it or the selection changes.
pub fn rebuild_record(
    mut commands: Commands,
    inspector: Res<Inspector>,
    selected: SelectedSource,
    records: Query<&SelectedRecord>,
    mut lists: Query<(Entity, &mut Node), With<InspectorRecord>>,
    mut shown: Local<Option<Record>>,
) {
    let Ok((list, mut node)) = lists.single_mut() else {
        return;
    };
    let record = selected
        .get(&records)
        .and_then(|selected| selected.0.as_ref())
        .filter(|record| record.fields.is_some() && inspector.open);
    if record == shown.as_ref() {
        return;
    }
    *shown = record.cloned();
    commands.entity(list).despawn_children();
    let display = if record.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
    let Some(record) = record else { return };

    let heading = commands
        .spawn_scene(text(format!("Row {}", record.row + 1), size::BODY))
        .id();
    commands.entity(list).add_child(heading);
    for (name, value) in record.fields.iter().flatten() {
        let field = spawn_field(&mut commands, name, value);
        commands.entity(list).add_child(field);
    }
}

/// One column of a record: its heading, what the row holds under it, and a
/// button for each dataset that names.
fn spawn_field(commands: &mut Commands, name: &str, value: &str) -> Entity {
    let name = name.to_string();
    // The value is plain text, not selectable, so it is copied with a button
    // beside its heading instead.
    let heading = if value.is_empty() {
        commands.spawn_scene(text_dim(name, size::SMALL)).id()
    } else {
        let value = value.to_string();
        commands
            .spawn_scene(bsn! {
                Node {
                    width: { Val::Percent(100.0) },
                    align_items: { AlignItems::Center },
                    column_gap: { Val::Px(space::CONTROLS) },
                }
                Children [
                    (
                        text_dim(name, size::SMALL)
                        Node { flex_grow: { 1.0_f32 }, min_width: { Val::ZERO } }
                    ),
                    (
                        @FeathersToolButton {
                            @caption: { bsn_list![button_icon(Icon::Copy)] }
                        }
                        Node { flex_shrink: { 0.0_f32 } }
                        BlocksFrameInput
                        CopyField { value: { value } }
                    ),
                ]
            })
            .id()
    };
    let shown = if value.is_empty() { "\u{2014}" } else { value };
    // An address is one long word, so it is broken wherever it has to be
    // rather than running out past the dock.
    let held = commands
        .spawn_scene(bsn! {
            Text({ shown.to_string() })
            TextFont { font_size: { FontSize::Px(size::SECONDARY) } }
            TextLayout { linebreak: { LineBreak::AnyCharacter } }
            ThemeTextColor({ if value.is_empty() { tokens::TEXT_DIM } else { tokens::TEXT_MAIN } })
            Node { max_width: { Val::Percent(100.0) } }
        })
        .id();
    let field = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(space::STACKED) },
                flex_shrink: { 0.0_f32 },
            }
        })
        .add_children(&[heading, held])
        .id();
    for url in datasets_in(value) {
        let button = commands
            .spawn_scene(bsn! {
                @FeathersButton {
                    @variant: { ButtonVariant::Normal },
                    @caption: { bsn_list![button_icon(Icon::Plus), button_text("Open in a new frame")] }
                }
                Node {
                    align_self: { AlignSelf::Start },
                    column_gap: { Val::Px(space::ICON_LABEL) },
                    margin: { UiRect::top(Val::Px(space::STACKED)) },
                }
                BlocksFrameInput
                OpenFieldDataset { url: { url.clone() } }
            })
            .id();
        commands.entity(field).add_child(button);
    }
    field
}

/// Copy a field's whole value, and mark its button with a tick until another
/// is copied.
pub fn on_copy_field(
    activate: On<Activate>,
    buttons: Query<(Entity, &CopyField)>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
    mut clipboard: ResMut<Clipboard>,
) {
    let Ok((_, field)) = buttons.get(activate.entity) else {
        return;
    };
    if let Err(e) = clipboard.set_text(field.value.as_str()) {
        warn!("could not copy the field: {e}");
        return;
    }
    for (button, _) in &buttons {
        let icon = if button == activate.entity {
            Icon::Check
        } else {
            Icon::Copy
        };
        for entity in children.iter_descendants(button) {
            if let Ok(text) = texts.get_mut(entity) {
                set_text(text, icon.glyph());
            }
        }
    }
}

/// Open the dataset a field's button names, in a frame of its own.
pub fn on_open_field_dataset(
    activate: On<Activate>,
    buttons: Query<&OpenFieldDataset>,
    mut requests: MessageWriter<DatasetRequest>,
) {
    if let Ok(button) = buttons.get(activate.entity) {
        requests.write(DatasetRequest {
            url: button.url.clone(),
            target: DatasetTarget::NewFrame,
        });
    }
}

/// The dock on the right, and the space it claims from the grid.
pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.add_dock::<Inspector>()
            .add_observer(close_inspector)
            .add_observer(on_open_field_dataset)
            .add_observer(on_copy_field)
            .add_systems(Update, open_on_request.in_set(Stage::DockInput))
            .add_systems(
                Update,
                super::reserve_space::<Inspector>.in_set(Stage::DockReserve),
            )
            .add_systems(
                Update,
                (update_inspector, rebuild_record).in_set(Stage::Chrome),
            )
            .add_systems(Startup, spawn_inspector.in_set(Boot::Shell));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::FrameArea;

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
