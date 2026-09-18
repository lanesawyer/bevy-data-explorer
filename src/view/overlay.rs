//! Per-frame overlays.
//!
//! Each frame carries a header in its top corner: the dataset's name, which
//! opens a menu for pointing the frame at a different dataset, a button that
//! opens the inspector on it, and a menu of what is layered over it. The status
//! its plugin reports sits underneath.
//!
//! The overlay knows nothing about any particular format. It reads the name and
//! status off whichever source entity a panel points at, and adds the lines
//! only a panel can know — its own zoom, which differs between two frames
//! showing the same source.
//!
//! Everything sits on a translucent panel, because it is drawn over imagery
//! that is bright in places and black in others.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};

use crate::app::theme::token;
use bevy_ui_widgets::Activate;

use crate::app::schedule::Stage;
use crate::source::hover::{HoverInfo, HoverProbe};
use crate::source::{DataSource, SourceStatus, SourceUrl};
use crate::view::layers::stacked_sources;
use crate::view::{
    BlocksFrameInput, DatasetRequest, DatasetTarget, FrameLayers, LayerOf, Panel, PanelRequest,
    PendingShow, ShowsSource,
};
use crate::widgets::{button_text, spawn_menu};

/// The translucent panel a frame's header and status sit on.
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
    /// Read a known dataset that is not open yet, and show it instead of what
    /// the frame shows once it lands. Carries its place in
    /// [`crate::formats::EXAMPLES`].
    ShowExample(usize),
    /// Draw it over what the frame shows.
    AddLayer,
    /// Take its layer off the frame.
    RemoveLayer,
    /// Read a known dataset that is not open yet, and draw it over the frame
    /// once it lands. Carries its place in [`crate::formats::EXAMPLES`].
    LayerExample(usize),
}

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
        TextFont { font_size: { FontSize::Px(12.0) } }
        ThemeTextColor({ token::OVERLAY_TEXT })
        Node {
            position_type: { PositionType::Absolute },
            display: { Display::None },
            padding: { UiRect::axes(Val::Px(8.0), Val::Px(6.0)) },
            border_radius: { BorderRadius::all(Val::Px(5.0)) },
        }
        ThemeBackgroundColor({ token::OVERLAY_BG })
        // Deliberately not `BlocksFrameInput`: a tooltip that swallowed the
        // pointer would suppress the very probe that produced it, and the
        // tooltip would flicker on and off as it appeared under the cursor.
        template_value(Pickable::IGNORE)
    });
}

/// Build one frame's overlay: a header row over the status it reports.
fn spawn_overlay(commands: &mut Commands, panel: Entity) {
    let box_ = commands
        .spawn_scene(bsn! {
            PanelHeader { panel: { panel } }
            Node {
                position_type: { PositionType::Absolute },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(2.0) },
                padding: { UiRect::axes(Val::Px(8.0), Val::Px(6.0)) },
                border_radius: { BorderRadius::all(Val::Px(5.0)) },
            }
            // Translucent, and on the same side as the theme: over imagery
            // rather than over the window, so a dark panel in a light theme
            // would read as a hole punched in the picture.
            ThemeBackgroundColor({ token::OVERLAY_BG })
            InheritableFont { font_size: { 13.0f32 } }
        })
        .id();

    let header = commands
        .spawn_scene(bsn! {
            Node {
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(4.0) },
            }
        })
        .id();

    // The name is the handle for changing it: switching datasets is about the
    // one named, so the control sits where the name already is.
    let title = super::dataset_menu::spawn_dataset_menu(commands, panel);

    let info = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_text("i")] }
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
    let menu = spawn_menu(commands, header);
    commands.entity(menu).insert(SourceMenu { panel });

    let status = commands
        .spawn_scene(bsn! {
            PanelText { panel: { panel } }
            Text
            TextFont { font_size: { FontSize::Px(12.0) } }
            ThemeTextColor({ token::OVERLAY_DIM })
        })
        .id();

    commands.entity(box_).add_children(&[header, status]);
    super::capture::spawn_capture_notice(commands, box_, panel);
}

/// Keep each overlay over its panel's cell.
pub fn position_hud(
    area: Res<crate::view::FrameArea>,
    panels: Query<&Panel>,
    mut texts: Query<(&PanelHeader, &mut Node)>,
) {
    let count = panels.iter().count();
    for (text, mut node) in &mut texts {
        let Ok(panel) = panels.get(text.panel) else {
            continue;
        };
        let cell = area.cell(count, panel.index);
        node.left = Val::Px(cell.min.x + 10.0);
        node.top = Val::Px(cell.min.y + 8.0);
        // Clear of the frame's own buttons in the opposite corner.
        node.max_width = Val::Px((cell.width() - 90.0).max(120.0));
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
    for (tooltip, mut node) in &mut tooltips {
        let Ok(panel) = panels.get(tooltip.panel) else {
            continue;
        };
        let cell = area.cell(count, panel.index);
        node.left = Val::Px(cell.min.x + 10.0);
        // `bottom` is measured from the bottom of the window, not of the cell.
        node.bottom = Val::Px(window.height() - cell.max.y + 10.0);
        node.max_width = Val::Px((cell.width() - 20.0).max(120.0));
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
    for (tooltip, mut text, mut node) in &mut tooltips {
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
        if node.display != display {
            node.display = display;
        }
        let lines = found.join("\n\n");
        if text.0 != lines {
            text.0 = lines;
        }
    }
}

pub fn update_hud(
    panels: Query<(&Camera, &Projection, &ShowsSource, Option<&PendingShow>)>,
    sources: Query<(&DataSource, &SourceStatus)>,
    mut texts: Query<(&mut Text, &PanelText)>,
    titles: Query<(Entity, &ChildOf), With<PanelTitle>>,
    headers: Query<&PanelHeader>,
    parents: Query<&ChildOf>,
    mut title_texts: Query<&mut Text, Without<PanelText>>,
) {
    for (mut text, panel_text) in &mut texts {
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
        // differs with depth, so a 3D frame says how it is driven instead.
        let view = match projection {
            Projection::Orthographic(ortho) => {
                let viewport = camera.logical_viewport_size().unwrap_or(Vec2::ONE);
                let units_per_px = ortho.area.width() / viewport.x.max(1.0);
                format!("zoom {:.5} {}/screen px", units_per_px, source.unit)
            }
            _ => "3D: drag to turn, middle-drag to pan, scroll to zoom, R to reset".to_string(),
        };
        let next = format!("{waiting}{}\n{view}", status.0);
        if text.0 != next {
            text.0 = next;
        }
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
        if let Ok(mut text) = title_texts.get_mut(entity)
            && text.0 != source.name
        {
            text.0 = source.name.clone();
        }
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
    choices: Query<&SourceChoice>,
    mut requests: MessageWriter<PanelRequest>,
    mut datasets: MessageWriter<DatasetRequest>,
) {
    let Ok(choice) = choices.get(activate.entity) else {
        return;
    };
    let (panel, source) = (choice.panel, choice.source);
    requests.write(match choice.action {
        ChoiceAction::Show => PanelRequest::Show { panel, source },
        ChoiceAction::AddLayer => PanelRequest::AddLayer { panel, source },
        ChoiceAction::RemoveLayer => PanelRequest::RemoveLayer { panel, source },
        ChoiceAction::ShowExample(index) => {
            if let Some(example) = crate::formats::EXAMPLES.get(index) {
                commands.entity(panel).insert(PendingShow {
                    url: example.url.to_string(),
                    name: example.name.to_string(),
                });
                datasets.write(DatasetRequest {
                    url: example.url.to_string(),
                    target: DatasetTarget::Show(panel),
                });
            }
            return;
        }
        ChoiceAction::LayerExample(index) => {
            if let Some(example) = crate::formats::EXAMPLES.get(index) {
                datasets.write(DatasetRequest {
                    url: example.url.to_string(),
                    target: DatasetTarget::Layer(panel),
                });
            }
            return;
        }
    });
}

/// Fill each frame's `...` menu: what it draws on top, and what else could go
/// on top.
///
/// Rebuilt when the set of sources changes, or when what a frame stacks
/// changes, so the current state stays marked.
pub fn rebuild_source_menus(
    mut commands: Commands,
    menus: Query<(Entity, &SourceMenu)>,
    panels: Query<(&ShowsSource, Option<&FrameLayers>), With<Panel>>,
    layer_cameras: Query<&ShowsSource, With<LayerOf>>,
    sources: Query<(Entity, &DataSource)>,
    urls: Query<&SourceUrl>,
    existing: Query<Entity, With<SourceMenuContent>>,
    mut shown: Local<Option<(Vec<(Entity, Vec<Entity>)>, usize)>>,
) {
    // What each frame is stacking, plus how many datasets there are to offer.
    // Either changing is what the rows have to reflect.
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

    let mut listed: Vec<(Entity, &DataSource)> = sources.iter().collect();
    // Registration order, which is the order the frames were opened in.
    listed.sort_by_key(|(_, source)| source.layer);
    let lookup = |entity: Entity| sources.get(entity).ok().map(|(_, data)| data);
    let opened: Vec<&str> = urls.iter().map(|url| url.0.as_str()).collect();

    for (menu_entity, menu) in &menus {
        let Some((_, stack)) = fingerprint.0.iter().find(|(panel, _)| *panel == menu.panel) else {
            continue;
        };
        let panel = menu.panel;
        let rows = layer_rows(&mut commands, panel, stack, &listed, &opened, lookup);
        commands.entity(menu_entity).add_children(&rows);
    }
    *shown = Some(fingerprint);
}

/// What a frame draws over its dataset, and what else could go on top.
fn layer_rows<'a>(
    commands: &mut Commands,
    panel: Entity,
    stack: &[Entity],
    listed: &[(Entity, &DataSource)],
    opened: &[&str],
    lookup: impl Fn(Entity) -> Option<&'a DataSource>,
) -> Vec<Entity> {
    let base = stack.first().and_then(|base| lookup(*base));
    let mut rows = Vec::new();

    if stack.len() > 1 {
        rows.push(menu_heading(commands, "Layers, top first", 0.0));
        for entity in stack[1..].iter().rev() {
            let Some(source) = lookup(*entity) else {
                continue;
            };
            rows.push(menu_row(
                commands,
                &source.name,
                &layer_note(base, source),
                SourceChoice {
                    panel,
                    source: *entity,
                    action: ChoiceAction::RemoveLayer,
                },
                "Remove",
                true,
            ));
        }
    }

    let room = stack.len() < super::grid::MAX_LAYERS;
    let gap = if rows.is_empty() { 0.0 } else { 10.0 };
    rows.push(menu_heading(commands, "Add a layer", gap));
    for (entity, source) in listed {
        if !super::layers::can_add_layer(stack, *entity) {
            continue;
        }
        rows.push(menu_row(
            commands,
            &source.name,
            &layer_note(base, source),
            SourceChoice {
                panel,
                source: *entity,
                action: ChoiceAction::AddLayer,
            },
            "Add",
            true,
        ));
    }
    // Known datasets not read yet, so choosing a layer never means opening
    // it somewhere first. What they are measured in is not known until
    // they are read, so there is no mismatch to note.
    for (index, example) in crate::formats::unopened_examples(opened) {
        rows.push(menu_row(
            commands,
            example.name,
            &format!("{}, not loaded yet", example.kind),
            SourceChoice {
                panel,
                source: Entity::PLACEHOLDER,
                action: ChoiceAction::LayerExample(index),
            },
            "Add",
            room,
        ));
    }
    rows
}

/// Everything a frame's menu is rebuilt from, despawned wholesale.
#[derive(Component, Clone, Default)]
pub struct SourceMenuContent;

/// Room the name and caption get beside a row's button, in logical pixels.
///
/// Fixed because the menu is: a menu's width never changes, so text can be
/// cut to it once when the rows are built rather than measured every frame.
const MENU_TEXT_PX: f32 = crate::widgets::MENU_WIDTH - 20.0 - 6.0 - 64.0;

/// What a layer is, and whether it is measured the way the frame under it is.
fn layer_note(base: Option<&DataSource>, source: &DataSource) -> String {
    match base.and_then(|base| super::layers::unit_mismatch(base, source)) {
        Some(mismatch) => format!("{mismatch}, not rescaled"),
        None => source.detail.clone(),
    }
}

fn menu_heading(commands: &mut Commands, text: &str, gap: f32) -> Entity {
    let text = text.to_string();
    commands
        .spawn_scene(bsn! {
            SourceMenuContent
            label(text)
            InheritableFont { font_size: { 12.0f32 } }
            Node { margin: { UiRect::new(Val::Px(2.0), Val::Px(0.0), Val::Px(gap), Val::Px(2.0)) } }
        })
        .id()
}

/// A dataset on one line, what it is on a dimmer one under it, and the one
/// thing this row does.
///
/// Both lines are cut to fit rather than wrapped. A wrapped name breaks the
/// row's height and runs into its neighbour, and the full name is already in
/// the frame's header once it is shown.
fn menu_row(
    commands: &mut Commands,
    name: &str,
    note: &str,
    choice: SourceChoice,
    action: &str,
    enabled: bool,
) -> Entity {
    let name = crate::widgets::truncate_to_width(name, MENU_TEXT_PX, 13.0);
    let note = crate::widgets::truncate_to_width(note, MENU_TEXT_PX, 11.0);
    let action = action.to_string();
    let row = commands
        .spawn_scene(bsn! {
            SourceMenuContent
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
                padding: { UiRect::vertical(Val::Px(3.0)) },
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
                            label(name)
                            InheritableFont { font_size: { 13.0f32 } }
                            TextLayout { linebreak: { LineBreak::NoWrap } }
                        ),
                        (
                            label_dim(note)
                            InheritableFont { font_size: { 11.0f32 } }
                            TextLayout { linebreak: { LineBreak::NoWrap } }
                        ),
                    ]
                ),
            ]
        })
        .id();
    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_text(action)] }
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
    row
}

/// The overlay drawn over each frame: its title, status lines and tooltip.
pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_info_pressed)
            .add_observer(on_source_chosen)
            .add_observer(super::dataset_menu::on_search_key)
            .add_systems(Update, sync_hud.in_set(Stage::FrameChrome))
            .add_systems(
                Update,
                (
                    position_hud,
                    rebuild_source_menus,
                    super::dataset_menu::clear_closed_searches,
                    super::dataset_menu::rebuild_dataset_lists,
                    super::dataset_menu::sync_search_hints,
                )
                    .chain()
                    .in_set(Stage::Chrome),
            )
            .add_systems(
                Update,
                (update_hud, position_tooltips, update_tooltips)
                    .chain()
                    .in_set(Stage::Overlay),
            );
    }
}
