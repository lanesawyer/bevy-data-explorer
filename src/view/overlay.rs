//! Per-frame overlays.
//!
//! Each frame carries a row of buttons along its top: the dataset's name, which
//! opens a menu for pointing the frame at a different dataset, a button that
//! opens the inspector on it, a menu of what is layered over it, and at the far
//! end the buttons that duplicate and close the frame. The status its plugin
//! reports sits underneath.
//!
//! The overlay knows nothing about any particular format. It reads the name and
//! status off whichever source entity a panel points at, and adds the lines
//! only a panel can know — its own zoom, which differs between two frames
//! showing the same source.
//!
//! The status sits on a translucent panel, because it is drawn over imagery
//! that is bright in places and black in others. The buttons carry their own
//! backgrounds and sit outside it, so that every one of them lines up along
//! the same edge whichever end of the row it is at.

use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use bevy::ui::InteractionDisabled;
use bevy_feathers::constants::fonts;
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::display::label_dim;
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};

use crate::app::theme::token;
use crate::widgets::space;
use bevy_ui_widgets::Activate;

use crate::app::schedule::Stage;
use crate::catalog::{Catalogs, EntryId};
use crate::source::hover::{HoverInfo, HoverProbe};
use crate::source::table::SourceTable;
use crate::source::{DataSource, ShowsSource, SourceStatus};
use crate::view::layers::stacked_sources;
use crate::view::{
    DatasetRequest, DatasetTarget, FrameLayers, LayerOf, Panel, PanelRequest, PendingShow,
    ShowFailed,
};
use crate::widgets::{
    BlocksFrameInput, Icon, button_icon, patch_node, set_text, size, spawn_menu, text, text_dim,
};

use super::chrome::CHROME_GAP;

/// A frame's chrome: its row of buttons, and the status under them.
#[derive(Component, Clone)]
pub struct PanelHeader {
    pub panel: Entity,
}

impl Default for PanelHeader {
    fn default() -> Self {
        PanelHeader {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// The translucent panel a frame's status sits on, hidden while there is
/// nothing to say.
#[derive(Component, Clone)]
pub struct PanelStatusBox {
    pub panel: Entity,
}

impl Default for PanelStatusBox {
    fn default() -> Self {
        PanelStatusBox {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// How far a frame's chrome sits in from the edges of its cell.
pub(super) const CHROME_INSET: f32 = space::CONTROL_INSET;

/// The dataset's name, at the start of a frame's header.
#[derive(Component, Clone, Default)]
pub struct PanelTitle;

/// Opens the inspector on the frame it belongs to.
#[derive(Component, Clone)]
pub struct PanelInfoButton {
    panel: Entity,
}

impl Default for PanelInfoButton {
    fn default() -> Self {
        PanelInfoButton {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// The `...` menu of what is layered over a frame. What the frame shows is
/// chosen from its title instead; see [`super::dataset_menu`].
#[derive(Component, Clone)]
pub struct SourceMenu {
    panel: Entity,
}

impl SourceMenu {
    /// The frame this menu belongs to.
    pub fn panel(&self) -> Entity {
        self.panel
    }
}

impl Default for SourceMenu {
    fn default() -> Self {
        SourceMenu {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// One dataset offered by a frame's menus, and what choosing it does.
#[derive(Component, Clone)]
pub struct SourceChoice {
    pub(super) panel: Entity,
    pub(super) source: Entity,
    pub(super) action: ChoiceAction,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChoiceAction {
    /// Show this dataset instead of what the frame shows now.
    Show,
    /// Read a dataset from a catalog, and show it instead of what the frame
    /// shows once it lands.
    ShowCatalog(EntryId),
    /// Open this dataset in a frame of its own.
    Open,
    /// Read a dataset from a catalog, and open it in a frame of its own once
    /// it lands.
    OpenCatalog(EntryId),
    /// Draw it over what the frame shows.
    AddLayer,
    /// Take its layer off the frame.
    RemoveLayer,
    /// Move its layer one place towards the top of the stack, or the bottom.
    MoveLayer { up: bool },
    /// Read a dataset from a catalog, and draw it over the frame once it
    /// lands.
    LayerCatalog(EntryId),
    /// Read what is at the address on the item's [`ChoiceUrl`], and put it
    /// where the target says.
    Address(DatasetTarget),
}

/// The address an [`ChoiceAction::Address`] item reads, which is typed rather
/// than listed anywhere, so the item carries it.
#[derive(Component, Clone, Default)]
pub struct ChoiceUrl(pub String);

impl Default for SourceChoice {
    fn default() -> Self {
        SourceChoice {
            panel: Entity::PLACEHOLDER,
            source: Entity::PLACEHOLDER,
            action: ChoiceAction::Show,
        }
    }
}

/// What the pointer is over, in a frame's bottom corner.
///
/// Kept away from the header so that reading the tooltip never means reading
/// over the dataset's name, and one entity rather than a box around a label
/// because a `Text` is already a node and can carry its own background.
#[derive(Component, Clone)]
pub struct PanelTooltip {
    pub panel: Entity,
}

impl Default for PanelTooltip {
    fn default() -> Self {
        PanelTooltip {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// A status overlay bound to one panel. Bound by entity rather than by source
/// so that duplicated panels each get their own and report their own zoom.
#[derive(Component, Clone)]
pub struct PanelText {
    panel: Entity,
}

impl Default for PanelText {
    fn default() -> Self {
        // Scenes patch over defaults; the real panel is written on top.
        PanelText {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// Keep one overlay per panel, and drop the overlays of panels that have gone
/// away.
pub fn sync_hud(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    headers: Query<(Entity, &PanelHeader)>,
    tooltips: Query<(Entity, &PanelTooltip)>,
) {
    // The box owns the header row and the status, so despawning it takes the
    // whole overlay with it. The tooltip sits in the opposite corner and is its
    // own root, so it is cleaned up alongside.
    for (entity, header) in &headers {
        if panels.get(header.panel).is_err() {
            commands.entity(entity).despawn();
        }
    }
    for (entity, tooltip) in &tooltips {
        if panels.get(tooltip.panel).is_err() {
            commands.entity(entity).despawn();
        }
    }

    for panel in &panels {
        if headers.iter().any(|(_, header)| header.panel == panel) {
            continue;
        }
        spawn_overlay(&mut commands, panel);
        spawn_tooltip(&mut commands, panel);
    }
}

/// Build one frame's tooltip, hidden until its source finds something.
fn spawn_tooltip(commands: &mut Commands, panel: Entity) {
    commands.spawn_scene(bsn! {
        PanelTooltip { panel: { panel } }
        Text
        // Rewritten every frame, so it names the font itself rather than
        // coming through `widgets::text`. See that module.
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: { FontSize::Px(size::SECONDARY) },
        }
        ThemeTextColor({ token::OVERLAY_TEXT })
        Node {
            position_type: { PositionType::Absolute },
            display: { Display::None },
            padding: { UiRect::axes(Val::Px(space::CONTROL_INSET), Val::Px(space::CONTROL_INSET)) },
            border_radius: { BorderRadius::all(Val::Px(5.0)) },
        }
        ThemeBackgroundColor({ token::OVERLAY_BG })
        // Deliberately not `BlocksFrameInput`: a tooltip that swallowed the
        // pointer would suppress the very probe that produced it, and the
        // tooltip would flicker on and off as it appeared under the cursor.
        template_value(Pickable::IGNORE)
    });
}

/// Build one frame's overlay: a row of buttons over the status it reports.
fn spawn_overlay(commands: &mut Commands, panel: Entity) {
    let root = commands
        .spawn_scene(bsn! {
            PanelHeader { panel: { panel } }
            // Spans the cell's width, so it must let the pointer through to
            // the frame everywhere but on the buttons it holds.
            template_value(Pickable::IGNORE)
            Node {
                position_type: { PositionType::Absolute },
                flex_direction: { FlexDirection::Column },
                align_items: { AlignItems::Start },
                row_gap: { Val::Px(CHROME_GAP) },
            }
            InheritableFont { font_size: { 13.0f32 } }
        })
        .id();

    let header = commands
        .spawn_scene(bsn! {
            template_value(Pickable::IGNORE)
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(CHROME_GAP) },
            }
        })
        .id();

    // The name is the handle for changing it: switching datasets is about the
    // one named, so the control sits where the name already is.
    let title = super::dataset_menu::spawn_dataset_menu(commands, panel);

    let info = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_icon(Icon::Info)] }
            }
            BlocksFrameInput
            PanelInfoButton { panel: { panel } }
        })
        .id();

    commands.entity(header).add_children(&[title, info]);
    // Beside the button that opens the inspector: both are about this frame
    // rather than about the grid, which is what the corner buttons are for.
    super::capture::spawn_capture_button(commands, header, panel);
    super::orbit::spawn_view_button(commands, header, panel);
    super::select::spawn_select_button(commands, header, panel);
    super::link::spawn_link_button(commands, header, panel);
    super::plane::spawn_plane_menu(commands, header, panel);
    let menu = spawn_menu(commands, header);
    // Built once, under the rows `rebuild_source_menus` puts above it, so
    // whatever is typed into its search survives the stack changing.
    let picker = super::dataset_menu::spawn_dataset_picker(
        commands,
        super::dataset_menu::PickerTarget::Layer(panel),
    );
    commands
        .entity(menu)
        .insert(SourceMenu { panel })
        .add_child(picker);
    // The frame's own buttons end the row, pushed there by what is left of it.
    let spacer = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();
    let corner = super::chrome::spawn_corner_buttons(commands, panel);
    commands
        .entity(header)
        .add_child(spacer)
        .add_children(&corner);

    let box_ = commands
        .spawn_scene(bsn! {
            PanelStatusBox { panel: { panel } }
            Node {
                flex_direction: { FlexDirection::Column },
                max_width: { Val::Percent(100.0) },
                row_gap: { Val::Px(space::STACKED) },
                padding: { UiRect::axes(Val::Px(space::CONTROL_INSET), Val::Px(space::CONTROL_INSET)) },
                border_radius: { BorderRadius::all(Val::Px(5.0)) },
            }
            // Translucent, and on the same side as the theme: over imagery
            // rather than over the window, so a dark panel in a light theme
            // would read as a hole punched in the picture.
            ThemeBackgroundColor({ token::OVERLAY_BG })
        })
        .id();

    let status = commands
        .spawn_scene(bsn! {
            PanelText { panel: { panel } }
            Text
            TextFont {
                font: FontSourceTemplate::Handle(fonts::REGULAR),
                font_size: { FontSize::Px(size::SECONDARY) },
            }
            ThemeTextColor({ token::OVERLAY_DIM })
        })
        .id();

    commands.entity(box_).add_child(status);
    super::capture::spawn_capture_notice(commands, box_, panel);
    commands.entity(root).add_children(&[header, box_]);
}

/// Keep each overlay over its panel's cell.
pub fn position_hud(
    area: Res<crate::view::FrameArea>,
    panels: Query<&Panel>,
    mut texts: Query<(&PanelHeader, &mut Node)>,
) {
    let count = panels.iter().count();
    for (text, node) in &mut texts {
        let Ok(panel) = panels.get(text.panel) else {
            continue;
        };
        let cell = area.cell(count, panel.index);
        patch_node(node, |node| {
            node.left = Val::Px(cell.min.x + CHROME_INSET);
            node.top = Val::Px(cell.min.y + CHROME_INSET);
            node.width = Val::Px((cell.width() - 2.0 * CHROME_INSET).max(0.0));
        });
    }
}

/// Place each tooltip in the bottom corner of its frame's cell.
///
/// Anchored from the bottom so it grows upward as a source reports more, rather
/// than sliding off the frame.
pub fn position_tooltips(
    windows: Query<&Window>,
    area: Res<crate::view::FrameArea>,
    panels: Query<&Panel>,
    mut tooltips: Query<(&PanelTooltip, &mut Node)>,
) {
    let Ok(window) = windows.single() else { return };
    let count = panels.iter().count();
    for (tooltip, node) in &mut tooltips {
        let Ok(panel) = panels.get(tooltip.panel) else {
            continue;
        };
        let cell = area.cell(count, panel.index);
        patch_node(node, |node| {
            node.left = Val::Px(cell.min.x + 10.0);
            // `bottom` is measured from the bottom of the window, not of the
            // cell.
            node.bottom = Val::Px(window.height() - cell.max.y + 10.0);
            node.max_width = Val::Px((cell.width() - 20.0).max(120.0));
        });
    }
}

/// Show what the frame's sources found under the pointer.
///
/// Driven by the probe rather than by the answer alone, so a source that has
/// not cleared a stale `HoverInfo` still shows nothing once the pointer has
/// moved to another frame. A frame stacking several sources lists what each
/// found, topmost first, since the top is what the pointer is visibly on.
pub fn update_tooltips(
    panels: Query<(&ShowsSource, Option<&FrameLayers>)>,
    layer_cameras: Query<&ShowsSource, With<LayerOf>>,
    sources: Query<(&HoverInfo, &HoverProbe)>,
    mut tooltips: Query<(&PanelTooltip, &mut Text, &mut Node)>,
) {
    for (tooltip, text, node) in &mut tooltips {
        let found: Vec<String> = panels
            .get(tooltip.panel)
            .map(|(shows, layers)| stacked_sources(shows, layers, &layer_cameras))
            .unwrap_or_default()
            .into_iter()
            .rev()
            .filter_map(|source| sources.get(source).ok())
            .filter(|(info, probe)| probe.panel == tooltip.panel && !info.is_empty())
            .map(|(info, _)| info.lines())
            .collect();

        let display = if found.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        patch_node(node, |node| node.display = display);
        let lines = found.join("\n\n");
        set_text(text, &lines);
    }
}

pub fn update_hud(
    panels: Query<(&Camera, &Projection, &ShowsSource, Option<&PendingShow>)>,
    sources: Query<(&DataSource, &SourceStatus)>,
    tables: Query<(), With<SourceTable>>,
    mut texts: Query<(&mut Text, &PanelText)>,
    titles: Query<(Entity, &ChildOf), With<PanelTitle>>,
    headers: Query<&PanelHeader>,
    parents: Query<&ChildOf>,
    mut title_texts: Query<&mut Text, Without<PanelText>>,
) {
    for (text, panel_text) in &mut texts {
        let Ok((camera, projection, shows, pending)) = panels.get(panel_text.panel) else {
            continue;
        };
        let Ok((source, status)) = sources.get(shows.0) else {
            continue;
        };
        // The name has moved up into the header, so it is no longer repeated
        // here. A dataset on its way in is said first: the choice was made in
        // a menu that has since closed, and nothing else would show it landed.
        let waiting = pending.map_or_else(String::new, |pending| {
            format!("reading {}\u{2026}\n", pending.name)
        });
        // A zoom in units per pixel means nothing in perspective, where it
        // differs with depth, so a 3D frame says how it is driven instead —
        // and nothing at all in a frame filled with rows, which has no view
        // onto anything to report.
        let view = match projection {
            _ if tables.contains(shows.0) => String::new(),
            Projection::Orthographic(ortho) => {
                let viewport = camera.logical_viewport_size().unwrap_or(Vec2::ONE);
                let units_per_px = ortho.area.width() / viewport.x.max(1.0);
                format!("zoom {:.5} {}/screen px", units_per_px, source.unit)
            }
            _ => "3D: drag to turn, right- or shift-drag to move, scroll to zoom to the pointer, R to reset".to_string(),
        };
        let next = if view.is_empty() {
            format!("{waiting}{}", status.0)
        } else {
            format!("{waiting}{}\n{view}", status.0)
        };
        set_text(text, &next);
    }

    for (entity, _) in &titles {
        let Some(panel) = parents
            .iter_ancestors(entity)
            .find_map(|ancestor| headers.get(ancestor).ok())
            .map(|header| header.panel)
        else {
            continue;
        };
        let Ok((_, _, shows, _)) = panels.get(panel) else {
            continue;
        };
        let Ok((source, _)) = sources.get(shows.0) else {
            continue;
        };
        if let Ok(text) = title_texts.get_mut(entity) {
            set_text(text, &source.name);
        }
    }
}

/// Hide a status panel with nothing in it, which would otherwise be an empty
/// pill under the buttons.
pub fn show_status_boxes(
    mut boxes: Query<(&Children, &mut Node), With<PanelStatusBox>>,
    lines: Query<(&Text, &Node), Without<PanelStatusBox>>,
) {
    for (children, node) in &mut boxes {
        let said = children.iter().any(|child| {
            lines
                .get(child)
                .is_ok_and(|(text, line)| line.display != Display::None && !text.0.is_empty())
        });
        let wanted = if said { Display::Flex } else { Display::None };
        patch_node(node, |node| node.display = wanted);
    }
}

/// Open the inspector on a frame.
pub fn on_info_pressed(
    activate: On<Activate>,
    buttons: Query<&PanelInfoButton>,
    mut requests: MessageWriter<PanelRequest>,
) {
    if let Ok(button) = buttons.get(activate.entity) {
        requests.write(PanelRequest::Inspect(button.panel));
    }
}

/// Point a frame at the dataset chosen from its menu.
pub fn on_source_chosen(
    activate: On<Activate>,
    mut commands: Commands,
    choices: Query<(&SourceChoice, Option<&ChoiceUrl>)>,
    catalogs: Res<Catalogs>,
    mut requests: MessageWriter<PanelRequest>,
    mut datasets: MessageWriter<DatasetRequest>,
) {
    let Ok((choice, address)) = choices.get(activate.entity) else {
        return;
    };
    let (panel, source) = (choice.panel, choice.source);
    requests.write(match choice.action {
        ChoiceAction::Show => PanelRequest::Show { panel, source },
        ChoiceAction::Open => PanelRequest::Open(source),
        ChoiceAction::AddLayer => PanelRequest::AddLayer { panel, source },
        ChoiceAction::RemoveLayer => PanelRequest::RemoveLayer { panel, source },
        ChoiceAction::MoveLayer { up } => PanelRequest::MoveLayer { panel, source, up },
        ChoiceAction::ShowCatalog(id) => {
            if let Some(entry) = catalogs.get(id) {
                commands
                    .entity(panel)
                    .remove::<ShowFailed>()
                    .insert(PendingShow {
                        url: entry.url.clone(),
                        name: entry.name.clone(),
                    });
                datasets.write(DatasetRequest {
                    url: entry.url.clone(),
                    target: DatasetTarget::Show(panel),
                });
            }
            return;
        }
        ChoiceAction::OpenCatalog(id) => {
            if let Some(entry) = catalogs.get(id) {
                datasets.write(DatasetRequest {
                    url: entry.url.clone(),
                    target: DatasetTarget::NewFrame,
                });
            }
            return;
        }
        ChoiceAction::Address(target) => {
            let Some(ChoiceUrl(url)) = address else {
                return;
            };
            if let DatasetTarget::Show(panel) = target {
                commands
                    .entity(panel)
                    .remove::<ShowFailed>()
                    .insert(PendingShow {
                        url: url.clone(),
                        name: url.clone(),
                    });
            }
            datasets.write(DatasetRequest {
                url: url.clone(),
                target,
            });
            return;
        }
        ChoiceAction::LayerCatalog(id) => {
            if let Some(entry) = catalogs.get(id) {
                datasets.write(DatasetRequest {
                    url: entry.url.clone(),
                    target: DatasetTarget::Layer(panel),
                });
            }
            return;
        }
    });
}

/// Fill each frame's `...` menu with what it draws on top, over the picker
/// offering what else could go there.
///
/// Rebuilt when what a frame stacks changes, so the current state stays
/// marked.
pub fn rebuild_source_menus(
    mut commands: Commands,
    menus: Query<(Entity, &SourceMenu)>,
    panels: Query<(&ShowsSource, Option<&FrameLayers>), With<Panel>>,
    layer_cameras: Query<&ShowsSource, With<LayerOf>>,
    sources: Query<(Entity, &DataSource)>,
    existing: Query<Entity, With<SourceMenuContent>>,
    mut shown: Local<Option<(Vec<(Entity, Vec<Entity>)>, usize)>>,
) {
    // What each frame is stacking, plus how many datasets there are, since a
    // layer's row names its dataset.
    let mut current: Vec<(Entity, Vec<Entity>)> = menus
        .iter()
        .filter_map(|(_, menu)| {
            panels
                .get(menu.panel)
                .ok()
                .map(|(shows, layers)| (menu.panel, stacked_sources(shows, layers, &layer_cameras)))
        })
        .collect();
    current.sort_unstable();
    let fingerprint = (current, sources.iter().count());

    if shown.as_ref() == Some(&fingerprint) {
        return;
    }

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let lookup = |entity: Entity| sources.get(entity).ok().map(|(_, data)| data);

    for (menu_entity, menu) in &menus {
        let Some((_, stack)) = fingerprint.0.iter().find(|(panel, _)| *panel == menu.panel) else {
            continue;
        };
        let panel = menu.panel;
        let rows = layer_rows(&mut commands, panel, stack, lookup);
        commands.entity(menu_entity).insert_children(0, &rows);
    }
    *shown = Some(fingerprint);
}

/// What a frame draws over its dataset, and the heading over the picker for
/// what else could go on top.
fn layer_rows<'a>(
    commands: &mut Commands,
    panel: Entity,
    stack: &[Entity],
    lookup: impl Fn(Entity) -> Option<&'a DataSource>,
) -> Vec<Entity> {
    let base = stack.first().and_then(|base| lookup(*base));
    let mut rows = Vec::new();

    if stack.len() > 1 {
        rows.push(menu_heading(commands, "Layers, top first", 0.0));
        let layers = &stack[1..];
        for (depth, entity) in layers.iter().enumerate().rev() {
            let Some(source) = lookup(*entity) else {
                continue;
            };
            let choice = |action| SourceChoice {
                panel,
                source: *entity,
                action,
            };
            rows.push(menu_row(
                commands,
                &source.name,
                &layer_note(base, source),
                vec![
                    (
                        choice(ChoiceAction::MoveLayer { up: true }),
                        Icon::ChevronUp,
                        depth + 1 < layers.len(),
                    ),
                    (
                        choice(ChoiceAction::MoveLayer { up: false }),
                        Icon::ChevronDown,
                        depth > 0,
                    ),
                    (choice(ChoiceAction::RemoveLayer), Icon::Trash, true),
                ],
            ));
        }
    }

    let gap = if rows.is_empty() { 0.0 } else { 10.0 };
    rows.push(menu_heading(commands, "Add a layer", gap));
    if stack.len() >= super::grid::MAX_LAYERS {
        rows.push(menu_caption(commands, "This frame holds all it can."));
    }
    rows
}

/// Everything a frame's menu is rebuilt from, despawned wholesale.
#[derive(Component, Clone, Default)]
pub struct SourceMenuContent;

/// Room the name and caption get beside a row's buttons, in logical pixels.
///
/// Fixed because the menu is: a menu's width never changes, so text can be
/// cut to it once when the rows are built rather than measured every frame.
fn menu_text_px(buttons: usize) -> f32 {
    crate::widgets::MENU_WIDTH - 20.0 - buttons as f32 * (6.0 + 24.0)
}

/// What a layer is, and whether it is measured the way the frame under it is.
fn layer_note(base: Option<&DataSource>, source: &DataSource) -> String {
    match base.and_then(|base| super::layers::unit_mismatch(base, source)) {
        Some(mismatch) => format!("{mismatch}, not rescaled"),
        None => source.detail.clone(),
    }
}

fn menu_heading(commands: &mut Commands, content: &str, gap: f32) -> Entity {
    let content = content.to_string();
    commands
        .spawn_scene(bsn! {
            SourceMenuContent
            text(content, size::SECONDARY)
            Node { margin: { UiRect::new(Val::Px(space::STACKED), Val::Px(0.0), Val::Px(gap), Val::Px(space::STACKED)) } }
        })
        .id()
}

fn menu_caption(commands: &mut Commands, content: &str) -> Entity {
    let content = content.to_string();
    commands
        .spawn_scene(bsn! {
            SourceMenuContent
            label_dim(content)
            Node { margin: { UiRect::new(Val::Px(space::STACKED), Val::Px(0.0), Val::Px(0.0), Val::Px(space::HEADING)) } }
        })
        .id()
}

/// A dataset on one line, what it is on a dimmer one under it, and what this
/// row does: each button is a choice, its icon, and whether it is enabled.
///
/// Both lines are cut to fit rather than wrapped. A wrapped name breaks the
/// row's height and runs into its neighbour, and the full name is already in
/// the frame's header once it is shown.
fn menu_row(
    commands: &mut Commands,
    name: &str,
    note: &str,
    buttons: Vec<(SourceChoice, Icon, bool)>,
) -> Entity {
    let room = menu_text_px(buttons.len());
    let name = crate::widgets::truncate_to_width(name, room, 13.0);
    let note = crate::widgets::truncate_to_width(note, room, 11.0);
    let row = commands
        .spawn_scene(bsn! {
            SourceMenuContent
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(space::CONTROLS) },
                padding: { UiRect::vertical(Val::Px(space::ITEM_INSET)) },
            }
            Children [
                (
                    Node {
                        flex_direction: { FlexDirection::Column },
                        flex_grow: { 1.0_f32 },
                        flex_shrink: { 1.0_f32 },
                        min_width: { Val::Px(0.0) },
                        overflow: { Overflow::clip() },
                    }
                    Children [
                        (
                            text(name, size::BODY)
                            TextLayout { linebreak: { LineBreak::NoWrap } }
                        ),
                        (
                            text_dim(note, size::SMALL)
                            TextLayout { linebreak: { LineBreak::NoWrap } }
                        ),
                    ]
                ),
            ]
        })
        .id();
    for (choice, icon, enabled) in buttons {
        let button = commands
            .spawn_scene(bsn! {
                @FeathersToolButton {
                    @caption: { bsn_list![button_icon(icon)] }
                }
                BlocksFrameInput
                template_value(choice)
                Node { flex_shrink: { 0.0_f32 } }
            })
            .id();
        if !enabled {
            commands.entity(button).insert(InteractionDisabled);
        }
        commands.entity(row).add_child(button);
    }
    row
}

/// The overlay drawn over each frame: its title, status lines and tooltip.
pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_info_pressed)
            .add_observer(on_source_chosen)
            .add_observer(super::dataset_menu::on_search_key)
            .add_observer(super::dataset_menu::on_filter_chip)
            .add_observer(super::dataset_menu::on_source_chip)
            .add_observer(super::dataset_menu::on_browse_item)
            .add_systems(Update, sync_hud.in_set(Stage::FrameChrome))
            .add_systems(
                Update,
                (
                    position_hud,
                    rebuild_source_menus,
                    super::dataset_menu::clear_closed_searches,
                    super::dataset_menu::search_catalogs,
                    super::dataset_menu::rebuild_dataset_lists,
                    super::dataset_menu::sync_filter_chips,
                    super::dataset_menu::sync_source_chips,
                )
                    .chain()
                    .in_set(Stage::Chrome),
            )
            .add_systems(
                Update,
                (
                    update_hud,
                    show_status_boxes,
                    position_tooltips,
                    update_tooltips,
                )
                    .chain()
                    .in_set(Stage::Overlay),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Category;

    fn source(unit: &str, detail: &str) -> DataSource {
        DataSource {
            name: "a dataset".into(),
            unit: unit.into(),
            detail: detail.into(),
            stat: String::new(),
            category: Category::Image,
            layer: 1,
        }
    }

    #[test]
    fn a_layer_measured_like_its_frame_is_described_as_itself() {
        let base = source("µm", "OME-Zarr image");
        let layer = source("µm", "SVG annotations");
        assert_eq!(layer_note(Some(&base), &layer), "SVG annotations");
    }

    #[test]
    fn a_layer_measured_differently_says_it_is_not_rescaled() {
        let base = source("µm", "OME-Zarr image");
        let layer = source("px", "SVG annotations");
        assert_eq!(layer_note(Some(&base), &layer), "px over µm, not rescaled");
    }

    #[test]
    fn every_button_in_a_row_takes_room_from_its_text() {
        assert!(menu_text_px(3) < menu_text_px(1));
        // The most any row holds still leaves room for a name.
        assert!(menu_text_px(3) > 100.0);
    }
}
