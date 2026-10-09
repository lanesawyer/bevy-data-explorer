//! Frames that show a lineage: where a record came from and what came of it.
//!
//! A lineage is records rather than a place, but it is read the way a
//! picture is: dragged around to follow a line, and the wheel stepping back
//! to take in more of it. Each record
//! is a card in a column — what it was made from to the left, what was made
//! from it to the right — with lines between them. A card's disclosure button
//! asks for its own links, which the format fetches and adds; a crowd of them
//! is folded into one card until that is pressed too. A record stored
//! somewhere opens in a frame of its own, and the two point at each other
//! (`links`).
//!
//! It knows nothing about any particular format: it reads the
//! [`SourceLineage`] off whichever source a frame points at, and writes what
//! it wants into [`LineageAsked`].

use bevy::input::mouse::{MouseScrollPixelsPerLine, MouseScrollUnit};
use bevy::picking::cursor::{EntityCursor, OverrideCursor};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::SystemCursorIcon;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersToolButton};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use bevy_ui_widgets::Activate;

use crate::app::schedule::Stage;
use crate::app::theme::token;
use crate::formats::discover::datasets_in;
use crate::source::lineage::{LineageAsked, Links, SourceLineage};
use crate::source::{ShowsSource, SourceUrl, compact_count};
use crate::widgets::{
    BlocksFrameInput, Icon, button_icon, button_text, hold_drag_cursor, patch_node, size, space,
    text, text_dim, truncate_to_width,
};

use super::overlay::PanelHeader;
use super::table::{TABLE_Z, chrome_depth};
use super::{DatasetRequest, DatasetTarget, FrameArea, Panel, SelectedPanel};

mod layout;
mod links;

use layout::{CARD_H, CARD_W, Shows, lay_out};
use links::{
    CardRecord, HoveredRecord, frames_showing, hover_records, mark_shown_cards,
    outline_hovered_frames,
};

/// Room inside a card for its text, past its padding and border.
const TEXT_W: f32 = CARD_W - 2.0 * space::CONTROL_INSET - 2.0;

/// Farthest and nearest the wheel goes. UI text is drawn at its own size
/// and only stretched, so past the size the cards were laid out at it
/// softens; half as large again still reads.
const ZOOM: (f32, f32) = (0.3, 1.5);

/// The lineage filling one frame.
#[derive(Component)]
pub struct LineageView {
    panel: Entity,
    /// The source drawn, so a frame pointed at another is rebuilt.
    source: Entity,
    /// The node as large as the graph, moved and scaled inside the frame.
    content: Entity,
    /// Where the graph's own corner is, from the frame's, in logical pixels.
    offset: Vec2,
    scale: f32,
    /// How large the graph is laid out.
    size: Vec2,
    /// Where the record it is drawn from was laid out last, so the graph is
    /// held still around it as it grows in any direction.
    root: Option<Vec2>,
    /// The frame's corner in the window and its size, as last placed.
    origin: Vec2,
    room: Vec2,
    /// Whether it has been put in the middle of the frame yet.
    centered: bool,
    dragging: bool,
}

impl LineageView {
    /// The graph's transform: its corner at `offset`, scaled by `scale`,
    /// given that a node is scaled about its middle.
    fn transform(&self) -> UiTransform {
        let middle = self.size / 2.0;
        let shift = self.offset - middle * (1.0 - self.scale);
        UiTransform {
            translation: Val2::px(shift.x, shift.y),
            scale: Vec2::splat(self.scale),
            ..default()
        }
    }

    /// Zoom by `factor` about `at`, a point in the frame, keeping what is
    /// under it there.
    fn zoom(&mut self, factor: f32, at: Vec2) {
        let scale = (self.scale * factor).clamp(ZOOM.0, ZOOM.1);
        let under = (at - self.offset) / self.scale;
        self.offset = at - under * scale;
        self.scale = scale;
    }
}

/// The button showing or hiding a record's links.
#[derive(Component, Clone, Default)]
pub struct ToggleLinks {
    source: Option<Entity>,
    id: String,
}

/// The card standing for records folded away, which shows them all.
#[derive(Component, Clone, Default)]
pub struct Unfold {
    source: Option<Entity>,
    id: String,
}

/// The button opening a stored record in a frame of its own.
#[derive(Component, Clone, Default)]
pub struct OpenStored {
    url: String,
}

/// Give every frame showing a lineage one, and take it away from every frame
/// that has stopped showing one.
pub fn sync_lineages(
    mut commands: Commands,
    panels: Query<(Entity, &ShowsSource), With<Panel>>,
    lineages: Query<(), With<SourceLineage>>,
    views: Query<(Entity, &LineageView)>,
) {
    for (entity, view) in &views {
        let shown = panels
            .get(view.panel)
            .is_ok_and(|(_, shows)| shows.0 == view.source);
        if !shown {
            commands.entity(entity).despawn();
        }
    }
    for (panel, shows) in &panels {
        if !lineages.contains(shows.0)
            || views
                .iter()
                .any(|(_, view)| view.panel == panel && view.source == shows.0)
        {
            continue;
        }
        spawn_lineage(&mut commands, panel, shows.0);
    }
}

fn spawn_lineage(commands: &mut Commands, panel: Entity, source: Entity) {
    let content = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            UiTransform::default(),
        ))
        .id();
    commands
        .spawn((
            LineageView {
                panel,
                source,
                content,
                offset: Vec2::ZERO,
                scale: 1.0,
                size: Vec2::ZERO,
                root: None,
                origin: Vec2::ZERO,
                room: Vec2::ZERO,
                centered: false,
                dragging: false,
            },
            // It covers the whole cell, so it stops the pointer reaching the
            // frame behind it, as a table does.
            BlocksFrameInput,
            EntityCursor::System(SystemCursorIcon::Grab),
            Node {
                position_type: PositionType::Absolute,
                overflow: Overflow::clip(),
                ..default()
            },
            GlobalZIndex(TABLE_Z),
        ))
        .add_child(content)
        .observe(on_drag_start)
        .observe(on_drag)
        .observe(on_drag_end)
        .observe(on_wheel);
}

fn on_drag_start(start: On<PointerDragStart>, mut views: Query<&mut LineageView>) {
    if start.button == PointerButton::Primary
        && let Ok(mut view) = views.get_mut(start.entity)
    {
        view.dragging = true;
    }
}

/// Move the graph with the pointer, wherever on it the drag began.
fn on_drag(drag: On<PointerDrag>, mut views: Query<&mut LineageView>) {
    if let Ok(mut view) = views.get_mut(drag.entity)
        && view.dragging
    {
        view.offset += drag.delta;
    }
}

fn on_drag_end(end: On<PointerDragEnd>, mut views: Query<&mut LineageView>) {
    if let Ok(mut view) = views.get_mut(end.entity) {
        view.dragging = false;
    }
}

/// Step back from the graph or in again, about the pointer.
fn on_wheel(
    mut wheel: On<PointerScroll>,
    per_line: Res<MouseScrollPixelsPerLine>,
    mut views: Query<&mut LineageView>,
) {
    let Ok(mut view) = views.get_mut(wheel.entity) else {
        return;
    };
    let lines = match wheel.unit {
        MouseScrollUnit::Line => wheel.y,
        MouseScrollUnit::Pixel => wheel.y / *per_line,
    };
    let at = wheel.pointer.position - view.origin;
    view.zoom(1.1f32.powf(lines), at);
    wheel.propagate(false);
}

/// Hold the grabbing hand for as long as a graph is dragged.
pub fn hold_pan_cursor(
    views: Query<&LineageView>,
    mut held: Local<bool>,
    cursor: Option<ResMut<OverrideCursor>>,
) {
    let dragging = views.iter().any(|view| view.dragging);
    hold_drag_cursor(dragging, &mut held, cursor, SystemCursorIcon::Grabbing);
}

/// Lay each lineage out again whenever it grows or something is asked of
/// it, holding the record it is drawn from where it was.
pub fn fill_lineages(
    mut commands: Commands,
    mut views: Query<&mut LineageView>,
    lineages: Query<(Ref<SourceLineage>, Ref<LineageAsked>)>,
    mut nodes: Query<&mut Node>,
) {
    for mut view in &mut views {
        let Ok((lineage, asked)) = lineages.get(view.source) else {
            continue;
        };
        if !view.is_added() && !lineage.is_changed() && !asked.is_changed() {
            continue;
        }
        let laid = lay_out(&lineage, &asked);
        let root = laid
            .cards
            .iter()
            .find(|card| card.shows == Shows::Node(0))
            .map(|card| card.rect.min);
        if let (Some(before), Some(now)) = (view.root, root) {
            let scale = view.scale;
            view.offset += (before - now) * scale;
        }
        view.root = root;
        view.size = laid.size;
        commands.entity(view.content).despawn_children();
        if let Ok(node) = nodes.get_mut(view.content) {
            patch_node(node, |node| {
                node.width = Val::Px(laid.size.x);
                node.height = Val::Px(laid.size.y);
            });
        }
        // Lines first, so the cards are drawn over where they meet.
        for line in &laid.lines {
            let line = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(line.min.x),
                        top: Val::Px(line.min.y),
                        width: Val::Px(line.width()),
                        height: Val::Px(line.height()),
                        ..default()
                    },
                    ThemeBackgroundColor(token::OVERLAY_DIM),
                    Pickable::IGNORE,
                ))
                .id();
            commands.entity(view.content).add_child(line);
        }
        for card in &laid.cards {
            let at = card.rect.min;
            let entity = match card.shows {
                Shows::Node(node) => {
                    spawn_card(&mut commands, view.source, &lineage, &asked, node, at)
                }
                Shows::Fold {
                    parent,
                    hidden,
                    more,
                } => spawn_fold(
                    &mut commands,
                    view.source,
                    &lineage.nodes()[parent].id,
                    (hidden, more),
                    at,
                ),
            };
            commands.entity(view.content).add_child(entity);
        }
    }
}

/// A record's card: what it is, its name, and what can be done with it.
fn spawn_card(
    commands: &mut Commands,
    source: Entity,
    lineage: &SourceLineage,
    asked: &LineageAsked,
    at: usize,
    place: Vec2,
) -> Entity {
    let node = &lineage.nodes()[at];
    let detail = match &node.links {
        Links::Fetching => "Reading its links\u{2026}".to_string(),
        Links::Failed(e) => e.clone(),
        _ => node.detail.clone(),
    };
    let detail = truncate_to_width(&detail, TEXT_W, size::SMALL);
    let name = truncate_to_width(&node.name, TEXT_W, size::BODY);
    let mut buttons = Vec::new();
    if node.links != Links::None {
        let open = asked.expanded.contains(&node.id);
        let icon = if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        let caption = if open { "Hide links" } else { "Show links" };
        let id = node.id.clone();
        buttons.push(
            commands
                .spawn_scene(bsn! {
                    @FeathersButton {
                        @variant: { ButtonVariant::Normal },
                        @caption: { bsn_list! {@button_icon(icon) -- @button_text(caption)} }
                    }
                    Node { column_gap: { Val::Px(space::ICON_LABEL) } }
                    BlocksFrameInput
                    ToggleLinks { source: { Some(source) }, id: { id } }
                })
                .id(),
        );
    }
    let opens = node
        .address
        .as_deref()
        .and_then(|address| datasets_in(address).into_iter().next());
    if let Some(url) = opens.clone() {
        buttons.push(
            commands
                .spawn_scene(bsn! {
                    @FeathersToolButton {
                        @caption: { bsn_list! {@button_icon(Icon::ExternalLink)} }
                    }
                    BlocksFrameInput
                    OpenStored { url: { url } }
                })
                .id(),
        );
    }
    let background = if node.step {
        token::OVERLAY_BG
    } else {
        token::FRAME_BG
    };
    // Set again by `mark_shown_cards`, the moment the card exists.
    let border = token::DIVIDER;
    let row = commands
        .spawn(Node {
            column_gap: Val::Px(space::CONTROLS),
            margin: UiRect::top(Val::Auto),
            ..default()
        })
        .add_children(&buttons)
        .id();
    let heading = commands.spawn_scene(text_dim(detail, size::SMALL)).id();
    let title = commands.spawn_scene(text(name, size::BODY)).id();
    commands
        .spawn((
            card_node(place),
            ThemeBackgroundColor(background),
            ThemeBorderColor(border),
            CardRecord {
                opens,
                root: at == 0,
            },
        ))
        .add_children(&[heading, title, row])
        .id()
}

/// The card standing for the records reached from one that are folded away.
fn spawn_fold(
    commands: &mut Commands,
    source: Entity,
    parent: &str,
    (hidden, more): (usize, bool),
    place: Vec2,
) -> Entity {
    let id = parent.to_string();
    let caption = format!("{} more", compact_count(hidden as u64));
    let button = if !more {
        commands
            .spawn_scene(text_dim(format!("and {caption}"), size::SECONDARY))
            .id()
    } else {
        commands
            .spawn_scene(bsn! {
                @FeathersButton {
                    @variant: { ButtonVariant::Normal },
                    @caption: { bsn_list! {@button_icon(Icon::Ellipsis) -- @button_text(caption)} }
                }
                Node { column_gap: { Val::Px(space::ICON_LABEL) } }
                BlocksFrameInput
                Unfold { source: { Some(source) }, id: { id } }
            })
            .id()
    };
    let mut node = card_node(place);
    node.justify_content = JustifyContent::Center;
    node.align_items = AlignItems::Center;
    commands
        .spawn((node, ThemeBorderColor(token::DIVIDER)))
        .add_child(button)
        .id()
}

fn card_node(place: Vec2) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(place.x),
        top: Val::Px(place.y),
        width: Val::Px(CARD_W),
        height: Val::Px(CARD_H),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(space::STACKED),
        padding: UiRect::all(Val::Px(space::CONTROL_INSET)),
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(6.0)),
        // Not clipped: a clip is worked out from the card's unscaled layout,
        // so a zoomed card cut its own text off. Its text is cut to fit
        // instead.
        ..default()
    }
}

/// Ask for a record's links, or stop showing them.
pub fn on_toggle_links(
    activate: On<Activate>,
    buttons: Query<&ToggleLinks>,
    mut asked: Query<&mut LineageAsked>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(mut asked) = button.source.and_then(|it| asked.get_mut(it).ok()) else {
        return;
    };
    if !asked.expanded.remove(&button.id) {
        asked.expanded.insert(button.id.clone());
    } else {
        // Folded again the next time it is opened.
        asked.unfolded.remove(&button.id);
    }
}

/// Show every record reached from one, rather than the first few.
pub fn on_unfold(
    activate: On<Activate>,
    buttons: Query<&Unfold>,
    mut asked: Query<&mut LineageAsked>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    if let Some(mut asked) = button.source.and_then(|it| asked.get_mut(it).ok()) {
        asked.unfolded.insert(button.id.clone());
    }
}

/// Open a stored record in a frame of its own, or select the frame already
/// showing it.
pub fn on_open_stored(
    activate: On<Activate>,
    buttons: Query<&OpenStored>,
    panels: Query<(Entity, &Panel, &ShowsSource)>,
    urls: Query<&SourceUrl>,
    mut selected: ResMut<SelectedPanel>,
    mut requests: MessageWriter<DatasetRequest>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    if let Some((frame, _)) = frames_showing(&button.url, &panels, &urls).next() {
        selected.0 = Some(frame);
        return;
    }
    requests.write(DatasetRequest {
        url: button.url.clone(),
        target: DatasetTarget::NewFrame,
    });
}

/// Fit each lineage to the frame it fills, below the frame's own chrome, and
/// put the graph where it has been dragged and zoomed to.
pub fn place_lineages(
    area: Res<FrameArea>,
    panels: Query<&Panel>,
    mut views: Query<(Entity, &mut LineageView)>,
    headers: Query<(&PanelHeader, &ComputedNode)>,
    mut nodes: Query<&mut Node>,
    mut transforms: Query<&mut UiTransform>,
) {
    let count = panels.iter().count();
    for (root, mut view) in &mut views {
        let Ok(panel) = panels.get(view.panel) else {
            continue;
        };
        let cell = area.cell(count, panel.index);
        let inset = chrome_depth(&headers, view.panel);
        if let Ok(node) = nodes.get_mut(root) {
            patch_node(node, |node| {
                node.left = Val::Px(cell.min.x);
                node.top = Val::Px(cell.min.y + inset);
                node.width = Val::Px(cell.width());
                node.height = Val::Px((cell.height() - inset).max(0.0));
            });
        }
        view.origin = cell.min + Vec2::new(0.0, inset);
        view.room = Vec2::new(cell.width(), (cell.height() - inset).max(0.0));
        // Opened with the record it is drawn from in the middle, once there
        // is a frame to be in the middle of and a graph to put there.
        if !view.centered
            && let Some(at) = view.root
            && view.room.min_element() > 0.0
        {
            let middle = at + Vec2::new(CARD_W, CARD_H) / 2.0;
            view.offset = view.room / 2.0 - middle * view.scale;
            view.centered = true;
        }
        let transform = view.transform();
        if let Ok(mut current) = transforms.get_mut(view.content)
            && *current != transform
        {
            *current = transform;
        }
    }
}

/// Select a lineage's frame when it is pressed anywhere, as a table's is.
pub fn select_pressed_lineages(
    buttons: Res<ButtonInput<MouseButton>>,
    hover: Res<HoverMap>,
    views: Query<&LineageView>,
    parents: Query<&ChildOf>,
    mut selected: ResMut<SelectedPanel>,
) {
    if !buttons.any_just_pressed([MouseButton::Left, MouseButton::Middle, MouseButton::Right]) {
        return;
    }
    let pressed = hover
        .values()
        .flat_map(|hits| hits.keys())
        .find_map(|hovered| {
            std::iter::once(*hovered)
                .chain(parents.iter_ancestors(*hovered))
                .find_map(|entity| views.get(entity).ok())
        });
    if let Some(view) = pressed
        && selected.0 != Some(view.panel)
    {
        selected.0 = Some(view.panel);
    }
}

/// The frames that show a lineage rather than a view onto space.
pub struct LineagePlugin;

impl Plugin for LineagePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_toggle_links)
            .add_observer(on_unfold)
            .add_observer(on_open_stored)
            .add_systems(Update, select_pressed_lineages.in_set(Stage::ControlsRead))
            .add_systems(Update, sync_lineages.in_set(Stage::FrameChrome))
            .init_resource::<HoveredRecord>()
            .add_systems(
                Update,
                (
                    fill_lineages,
                    place_lineages,
                    hold_pan_cursor,
                    hover_records,
                    outline_hovered_frames,
                    mark_shown_cards,
                )
                    .chain()
                    .in_set(Stage::Chrome),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> LineageView {
        LineageView {
            panel: Entity::PLACEHOLDER,
            source: Entity::PLACEHOLDER,
            content: Entity::PLACEHOLDER,
            offset: Vec2::new(40.0, -10.0),
            scale: 1.0,
            size: Vec2::new(900.0, 400.0),
            root: None,
            origin: Vec2::ZERO,
            room: Vec2::new(800.0, 600.0),
            centered: true,
            dragging: false,
        }
    }

    /// Where a point of the graph ends up in the frame, as the UI draws it:
    /// scaled about the middle of the node, then moved.
    fn drawn(view: &LineageView, point: Vec2) -> Vec2 {
        let transform = view.transform();
        let Val::Px(x) = transform.translation.x else {
            unreachable!()
        };
        let Val::Px(y) = transform.translation.y else {
            unreachable!()
        };
        let middle = view.size / 2.0;
        middle + (point - middle) * transform.scale + Vec2::new(x, y)
    }

    #[test]
    fn the_graph_is_drawn_from_its_corner_however_it_is_zoomed() {
        let mut view = view();
        view.scale = 0.5;
        let point = Vec2::new(300.0, 120.0);
        assert!(drawn(&view, point).abs_diff_eq(view.offset + point * 0.5, 1e-3));
    }

    #[test]
    fn the_wheel_zooms_about_the_pointer_and_no_nearer_than_laid_out() {
        let mut view = view();
        let pointer = Vec2::new(250.0, 180.0);
        let under = (pointer - view.offset) / view.scale;
        view.zoom(0.5, pointer);
        assert_eq!(view.scale, 0.5);
        assert!(drawn(&view, under).abs_diff_eq(pointer, 1e-3));
        view.zoom(100.0, pointer);
        assert_eq!(view.scale, ZOOM.1);
        view.zoom(0.0001, pointer);
        assert_eq!(view.scale, ZOOM.0);
    }
}
