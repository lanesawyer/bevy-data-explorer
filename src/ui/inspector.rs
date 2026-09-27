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
//! any address typed in. Pictures of the record, when whatever produced
//! the rows has some, are shown above the fields, grouped by what they show,
//! and open larger when clicked; files held about it, such as an OME-Zarr
//! store, are listed with the same copy and open buttons as a field. So are
//! the records it links to — the processes a BKP Registry specimen went
//! into, the data assets a process wrote — under what links them. Fields,
//! Files and Related each export from their header (`record_export`).

use bevy::clipboard::Clipboard;
use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersToolButton};
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};
use bevy_feathers::tokens;
use bevy_ui_widgets::Activate;

use crate::app::schedule::{Boot, Stage};
use crate::formats::discover::datasets_in;
use crate::source::table::{
    Record, RecordFiles, RecordImage, RecordImages, RecordImagesState, RelatedRecords,
    SelectedRecord,
};
use crate::source::{DataSource, SourceStatus};
use crate::ui::record_export::{RecordExportMenu, spawn_record_export_menu};
use crate::view::{DatasetRequest, DatasetTarget, PanelRequest, SelectedSource};
use crate::widgets::space;
use crate::widgets::{
    Accordion, AddDock, BlocksFrameInput, Dock, DockEdge, DockWidth, Enlargeable, Icon,
    SectionLevel, button_icon, button_text, dock_handle, place_right_dock, scroll_list,
    set_display, set_text, size, spawn_accordion, text, text_dim,
};

const WIDTH_PX: f32 = 300.0;
const MIN_PX: f32 = 200.0;

/// Taller than any window, so the record scrolls against the room the dock
/// leaves it rather than at some arbitrary point.
const RECORD_MAX_PX: f32 = 4000.0;

/// Linked records listed under one heading before the rest are only counted.
/// A specimen can be input to hundreds of processes, and every one listed is
/// a handful of entities.
const RELATED_SHOWN: usize = 50;

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

/// A section of a picked record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecordPart {
    Images,
    Files,
    Related,
    Fields,
}

/// A section of a picked record, whose open state carries over to the next.
/// Images, Files and Related are hidden while there is nothing in them.
#[derive(Component)]
pub struct RecordSection(RecordPart);

/// Where a section's contents are built, as they arrive.
#[derive(Component)]
pub struct RecordBody(RecordPart);

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
    sections: Query<(&Accordion, &RecordSection)>,
    export_menus: Query<Entity, With<RecordExportMenu>>,
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
    // A section closed on one row stays closed on the next.
    let open = |part: RecordPart| {
        sections
            .iter()
            .find(|(_, section)| section.0 == part)
            .is_none_or(|(accordion, _)| accordion.open)
    };
    commands.entity(list).despawn_children();
    for menu in &export_menus {
        commands.entity(menu).despawn();
    }
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

    // Pictures first: a record with any is usually worth opening for them.
    // Images, Files and Related are shown by `fill_record_images`,
    // `fill_record_files` and `fill_record_related` once whatever produced
    // the rows says it has some.
    for (part, title) in [
        (RecordPart::Images, "Images"),
        (RecordPart::Files, "Files"),
        (RecordPart::Related, "Related"),
        (RecordPart::Fields, "Fields"),
    ] {
        let section = spawn_accordion(&mut commands, title, open(part), SectionLevel::Pane);
        commands.entity(section.section).insert(RecordSection(part));
        commands.entity(section.body).insert(RecordBody(part));
        spawn_record_export_menu(&mut commands, section.header, part);
        if part == RecordPart::Fields {
            for (name, value) in record.fields.iter().flatten() {
                let field = spawn_field(&mut commands, name, value);
                commands.entity(section.body).add_child(field);
            }
        }
        commands.entity(list).add_child(section.section);
    }
}

/// The body of `part` of the record on screen, and whether it was just built.
fn body_of(
    bodies: &Query<(Entity, &RecordBody, Ref<RecordBody>)>,
    part: RecordPart,
) -> Option<(Entity, bool)> {
    bodies
        .iter()
        .find(|(_, body, _)| body.0 == part)
        .map(|(entity, _, fresh)| (entity, fresh.is_added()))
}

/// Show or hide the section holding `part`.
fn show_section(
    sections: &Query<(Entity, &RecordSection)>,
    nodes: &mut Query<&mut Node>,
    part: RecordPart,
    shown: bool,
) {
    for (entity, section) in sections {
        if section.0 == part {
            set_display(nodes, entity, shown);
        }
    }
}

/// Fill the Files section with what is held about the picked record, as it
/// arrives, and hide it while there is nothing. Each file is copied and, when
/// it names something that opens, opened, the way a field is.
pub fn fill_record_files(
    mut commands: Commands,
    selected: SelectedSource,
    files: Query<Ref<RecordFiles>>,
    bodies: Query<(Entity, &RecordBody, Ref<RecordBody>)>,
    sections: Query<(Entity, &RecordSection)>,
    mut nodes: Query<&mut Node>,
) {
    let Some((body, fresh)) = body_of(&bodies, RecordPart::Files) else {
        return;
    };
    let files = selected.get(&files);
    if !fresh && !files.as_ref().is_some_and(Ref::is_changed) {
        return;
    }
    let files = files.map(|it| it.0.clone()).unwrap_or_default();
    show_section(&sections, &mut nodes, RecordPart::Files, !files.is_empty());
    commands.entity(body).despawn_children();
    for file in files {
        let heading = if file.kind.is_empty() {
            file.name
        } else {
            format!("{} ({})", file.name, file.kind)
        };
        let field = spawn_field(&mut commands, &heading, &file.address);
        commands.entity(body).add_child(field);
    }
}

/// Fill the Related section with the records the picked one links to, as
/// they arrive, and hide it while there is nothing to say. A linked record
/// that is stored somewhere shows where, with the buttons a field has; any
/// other shows its name.
pub fn fill_record_related(
    mut commands: Commands,
    selected: SelectedSource,
    related: Query<Ref<RelatedRecords>>,
    bodies: Query<(Entity, &RecordBody, Ref<RecordBody>)>,
    sections: Query<(Entity, &RecordSection)>,
    mut nodes: Query<&mut Node>,
) {
    let Some((body, fresh)) = body_of(&bodies, RecordPart::Related) else {
        return;
    };
    let related = selected.get(&related);
    if !fresh && !related.as_ref().is_some_and(Ref::is_changed) {
        return;
    }
    let related: RelatedRecords = related.map(|it| (*it).clone()).unwrap_or_default();
    show_section(
        &sections,
        &mut nodes,
        RecordPart::Related,
        related != RelatedRecords::None,
    );
    commands.entity(body).despawn_children();
    let groups = match related {
        RelatedRecords::None => return,
        RelatedRecords::Fetching => Err("Fetching linked records\u{2026}".to_string()),
        RelatedRecords::Failed(e) => Err(e),
        RelatedRecords::Ready(groups) if groups.is_empty() => Err("Linked to nothing.".to_string()),
        RelatedRecords::Ready(groups) => Ok(groups),
    };
    let groups = match groups {
        Ok(groups) => groups,
        Err(note) => {
            let note = commands.spawn_scene(text_dim(note, size::SMALL)).id();
            commands.entity(body).add_child(note);
            return;
        }
    };
    for group in groups {
        let heading = commands
            .spawn_scene(bsn! {
                text(format!("{} \u{b7} {}", group.title, group.records.len()), size::SECONDARY)
                Node { margin: { UiRect::top(Val::Px(space::ROWS)) } }
            })
            .id();
        commands.entity(body).add_child(heading);
        let more = group.records.len().saturating_sub(RELATED_SHOWN);
        for record in group.records.into_iter().take(RELATED_SHOWN) {
            let field = match record.address {
                Some(address) => spawn_field(
                    &mut commands,
                    &format!("{} \u{b7} {}", record.name, record.detail),
                    &address,
                ),
                None => spawn_field(&mut commands, &record.detail, &record.name),
            };
            commands.entity(body).add_child(field);
        }
        if more > 0 {
            let note = commands
                .spawn_scene(text_dim(format!("and {more} more"), size::SMALL))
                .id();
            commands.entity(body).add_child(note);
        }
    }
}

/// Fill the Images section with the pictures of the picked record, as they
/// arrive, and hide it while there are none.
pub fn fill_record_images(
    mut commands: Commands,
    selected: SelectedSource,
    pictures: Query<Ref<RecordImages>>,
    bodies: Query<(Entity, &RecordBody, Ref<RecordBody>)>,
    sections: Query<(Entity, &RecordSection)>,
    mut nodes: Query<&mut Node>,
) {
    let Some((body, fresh)) = body_of(&bodies, RecordPart::Images) else {
        return;
    };
    let pictures = selected.get(&pictures);
    let changed = fresh || pictures.as_ref().is_some_and(Ref::is_changed);
    if !changed {
        return;
    }
    let state = pictures.map_or(RecordImagesState::None, |it| it.0.clone());
    show_section(
        &sections,
        &mut nodes,
        RecordPart::Images,
        !matches!(state, RecordImagesState::None),
    );
    commands.entity(body).despawn_children();
    let note = match &state {
        RecordImagesState::None => return,
        RecordImagesState::Fetching => "Fetching images\u{2026}".to_string(),
        RecordImagesState::Failed(e) => e.clone(),
        RecordImagesState::Ready(_) => String::new(),
    };
    let RecordImagesState::Ready(images) = state else {
        let note = commands.spawn_scene(text_dim(note, size::SMALL)).id();
        commands.entity(body).add_child(note);
        return;
    };
    let mut group = None;
    for image in images {
        if group.as_ref() != Some(&image.group) {
            let heading = commands
                .spawn_scene(bsn! {
                    text(image.group.clone(), size::SECONDARY)
                    Node { margin: { UiRect::top(Val::Px(space::ROWS)) } }
                })
                .id();
            commands.entity(body).add_child(heading);
            group = Some(image.group.clone());
        }
        let picture = spawn_picture(&mut commands, image);
        commands.entity(body).add_child(picture);
    }
}

/// One picture under its title, as wide as the dock allows and never wider
/// than itself, opening larger when clicked.
fn spawn_picture(commands: &mut Commands, image: RecordImage) -> Entity {
    let title = commands
        .spawn_scene(text_dim(image.title.clone(), size::SMALL))
        .id();
    let size = image.size.as_vec2().max(Vec2::ONE);
    let picture = commands
        .spawn((
            ImageNode::new(image.image),
            Node {
                width: Val::Percent(100.0),
                max_width: Val::Px(size.x),
                aspect_ratio: Some(size.x / size.y),
                ..default()
            },
            Enlargeable { title: image.title },
        ))
        .id();
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(space::STACKED),
            flex_shrink: 0.0,
            ..default()
        })
        .add_children(&[title, picture])
        .id()
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
                (
                    update_inspector,
                    rebuild_record,
                    fill_record_images,
                    fill_record_files,
                    fill_record_related,
                )
                    .chain()
                    .in_set(Stage::Chrome),
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
