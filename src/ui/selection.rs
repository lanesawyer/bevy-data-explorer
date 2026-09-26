//! The cell selection summary, docked on the right.
//!
//! A dock rather than a sidebar section: it is opened by drawing a rectangle
//! over a frame and answers a question about that rectangle, so it belongs
//! beside the frame it was drawn on rather than under the controls that filter
//! every frame. It starts closed and takes no space from the grid until there
//! is a selection to report, like the inspector it sits beside.
//!
//! Offered only by sources whose catalog service can count a region — the files
//! themselves say nothing about where a cell is beyond drawing it — so a
//! dataset opened by address alone never opens it.
//!
//! What a rectangle is broken down by follows the cell panel's coloring: the
//! categories listed are the values of the column the points are painted by,
//! largest first, because those are the ones the eye is already reading the
//! rectangle by. Changing the breakdown is the palette buttons' job, not
//! another control here.
//!
//! Clicking a category drills into it and fetches the cells themselves: every
//! cell of that category the rectangle holds, up to a page of them, each with
//! everything the dataset knows about it. The counts say what a rectangle is
//! made of; the records say what is actually in it, which is what a category
//! is worth opening for.
//!
//! A rectangle can hold hundreds of thousands of cells, so only the first page
//! is ever asked for and the count above the list is what says how many there
//! really are.
//!
//! The counts are the service's, over every cell in the dataset rather than
//! over the points currently streamed in. They are also counted among the cells
//! the filters admit, so unticking a class in the cell panel narrows what a
//! selection reports as well as what is drawn.

use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersToolButton};
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};
use bevy_feathers::tokens;
use bevy_ui_widgets::Activate;

use crate::app::schedule::{Boot, Stage};
use crate::catalog::{CellRecord, RegionFocus, RegionSummary, SummaryState};
use crate::source::properties::{CellProperties, ColorOverrides, PropertyKind, PropertyValue};
use crate::source::region::SelectedRegion;
use crate::source::{ShowsSource, compact_count};
use crate::ui::cell_panel::spawn_more_note;
use crate::ui::inspector::Inspector;
use crate::view::{FrameArea, FrameRegion, SelectMode, SelectedPanel, SelectedSource};
use crate::widgets::space;
use crate::widgets::{
    AddDock, BlocksFrameInput, Dock, DockEdge, DockWidth, Icon, SelectableText, button_icon,
    button_text, dock_band, dock_handle, place_right_dock, scroll_list, set_text, size, text,
    text_dim,
};

/// Wide enough for a cluster's name beside its count without either wrapping.
const WIDTH_PX: f32 = 320.0;
const MIN_PX: f32 = 220.0;

/// The most values listed under one bucket. A whole-brain taxonomy has
/// thousands of clusters, and a selection rarely holds more of them than this
/// with any count worth reading.
const MAX_ROWS: usize = 100;

/// How tall the lists may grow before they scroll within the dock. Taller than
/// any window, so each scrolls against the room the dock leaves it rather than
/// at some arbitrary point up the panel.
const LIST_MAX_PX: f32 = 2000.0;

/// The most of the dock the categories take once one is drilled into, leaving
/// the rest to its cells. Until then they may have all of it.
const CATEGORIES_SHARE: f32 = 25.0;

#[derive(Resource)]
pub struct SelectionDock {
    pub width: DockWidth,
    pub open: bool,
    /// How much of the right edge the docks outboard of this one occupy.
    ///
    /// Kept here because a drag is measured from the window's edge, not from
    /// this dock's: with the inspector open, the handle sits that much further
    /// in, and a width taken straight from the reach would jump by the
    /// inspector's width the moment it was grabbed.
    outboard: f32,
}

impl Default for SelectionDock {
    fn default() -> Self {
        SelectionDock {
            width: DockWidth::new(WIDTH_PX, MIN_PX),
            open: false,
            outboard: 0.0,
        }
    }
}

impl SelectionDock {
    /// Width it occupies in a window this wide.
    pub fn current_width(&self, window_width: f32) -> f32 {
        if self.open {
            self.width.within(window_width)
        } else {
            0.0
        }
    }
}

impl Dock for SelectionDock {
    type Handle = SelectionHandle;
    const EDGE: DockEdge = DockEdge::Right;
    const KEY: &'static str = "selection";
    const DEFAULT_SIZE: f32 = WIDTH_PX;

    fn size(&self) -> f32 {
        self.width.px
    }

    fn set_size(&mut self, size: f32) {
        self.width.set(size);
    }

    fn drag_to(&mut self, reach: f32, span: f32) {
        self.width.drag_to(reach - self.outboard, span);
    }
}

#[derive(Component, Clone, Default)]
pub struct SelectionRoot;

#[derive(Component, Clone, Default)]
pub struct SelectionHandle;

#[derive(Component, Clone, Default)]
pub struct SelectionClose;

#[derive(Component, Clone, Default)]
pub struct SelectionTitle;

/// The list the categories are built into, which scrolls on its own so they
/// stay in reach while their cells are read below.
#[derive(Component, Clone, Default)]
pub struct SelectionBody;

/// The cells of the category drilled into, which take what the categories
/// leave and scroll on their own.
#[derive(Component, Clone, Default)]
pub struct SelectionDetail;

/// Marks what a rebuild replaces.
#[derive(Component, Clone, Default)]
pub struct SelectionContent;

/// The line above the buckets, saying what is selected or why nothing is.
#[derive(Component, Clone, Default)]
pub struct SelectionStatus;

/// A row naming one value of the column the points are colored by. Clicking it
/// drills into that category.
#[derive(Component, Clone, Default)]
pub struct CategoryRow {
    pub column: String,
    pub label: String,
}

/// The column the points are currently colored by, and what the cell panel
/// calls it.
///
/// The summary follows the coloring rather than offering every column at once:
/// the categories on screen are the ones the eye is already reading the
/// rectangle by, and picking a different breakdown is what the cell panel's
/// palette buttons are for.
fn color_column(properties: &CellProperties) -> Option<(String, String)> {
    let index = properties.color_by?;
    let property = properties.properties.get(index)?;
    let (column, _) = property.color_column()?;
    // A tree colors by one of its levels, and that level's name is what the
    // cell panel shows beside the swatches.
    let name = property.tree().map_or_else(
        || property.name.clone(),
        |tree| {
            tree.levels
                .get(tree.color_level)
                .map_or_else(|| property.name.clone(), |level| level.name.clone())
        },
    );
    Some((column.to_string(), name))
}

/// Every categorical column the dataset offers, each with the name the cell
/// panel gives it.
///
/// A tree contributes one per level, because "class" and "cluster" are
/// different things to say about the same cells and the platform lists both.
///
/// Includes the columns the cell panel is not currently listing. Hiding one
/// there says it is not worth a filter's worth of screen, which is a different
/// question from whether it is worth knowing about the cells just selected —
/// and the service counted it either way, so it costs nothing to say.
fn columns_of(properties: &CellProperties) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (_, property) in properties.cell_properties() {
        match &property.kind {
            PropertyKind::Categorical(_) => {
                found.push((property.id.clone(), property.name.clone()));
            }
            PropertyKind::Tree(tree) => {
                for level in &tree.levels {
                    found.push((level.id.clone(), level.name.clone()));
                }
            }
            // A range has nothing to say one value at a time; its histogram is
            // the cell panel's business.
            PropertyKind::Numeric(_) => {}
        }
    }
    found
}

/// The values stored in `column`, wherever among the properties it lives.
fn values_of<'a>(properties: &'a CellProperties, column: &str) -> Vec<&'a PropertyValue> {
    properties
        .properties
        .iter()
        .flat_map(|property| property.columns())
        .find(|(id, _)| *id == column)
        .map(|(_, values)| values)
        .unwrap_or_default()
}

fn spawn_selection_dock(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        SelectionRoot
        BlocksFrameInput
        Node {
            position_type: { PositionType::Absolute },
            top: { Val::Px(0.0) },
            height: { Val::Percent(100.0) },
            display: { Display::None },
            flex_direction: { FlexDirection::Column },
        }
        ThemeBackgroundColor({ tokens::WINDOW_BG })
        InheritableFont { font_size: { 13.0f32 } }
        Children [
            (
                // The title and the count of what is selected, on a band that
                // stays put above the lists that scroll.
                Node {
                    width: { Val::Percent(100.0) },
                    flex_direction: { FlexDirection::Column },
                    row_gap: { Val::Px(space::ROWS) },
                    padding: { UiRect::all(Val::Px(space::PANEL_INSET)) },
                    border: { UiRect::bottom(Val::Px(1.0)) },
                }
                dock_band()
                Children [
            (
                Node {
                    width: { Val::Percent(100.0) },
                    align_items: { AlignItems::Center },
                    justify_content: { JustifyContent::SpaceBetween },
                    column_gap: { Val::Px(space::CONTROLS) },
                }
                Children [
                    // No Node of its own: a `min_width` of zero on a label lets
                    // the row collapse it to nothing, which is a title that
                    // simply never appears. The inspector's is the shape that
                    // works.
                    (
                        SelectionTitle
                        text("Cell Selection", size::DOCK_TITLE)
                    ),
                    (
                        @FeathersToolButton {
                            @caption: { bsn_list![button_icon(Icon::X)] }
                        }
                        Node { flex_shrink: { 0.0_f32 } }
                        SelectionClose
                        BlocksFrameInput
                    ),
                ]
            ),
            (
                SelectionStatus
                text_dim("", size::SMALL)
            ),
                ]
            ),
            (
                // The padding the lists' scrollbars reach into.
                Node {
                    width: { Val::Percent(100.0) },
                    flex_direction: { FlexDirection::Column },
                    flex_grow: { 1.0_f32 },
                    min_height: { Val::ZERO },
                    row_gap: { Val::Px(space::ROWS) },
                    padding: { UiRect::all(Val::Px(space::PANEL_INSET)) },
                }
                Children [
            (
                SelectionBody
                scroll_list(LIST_MAX_PX)
                Node { flex_shrink: { 0.0_f32 } }
            ),
            (
                SelectionDetail
                scroll_list(LIST_MAX_PX)
                Node {
                    display: { Display::None },
                    flex_grow: { 1.0_f32 },
                    min_height: { Val::ZERO },
                }
            ),
                ]
            ),
        ]
    });

    commands.spawn_scene(bsn! {
        SelectionHandle
        dock_handle(DockEdge::Right)
        Node {
            display: { Display::None },
        }
    });
}

/// Open the dock when a rectangle is drawn, on a source that can count one.
///
/// Opened by the gesture rather than by a button of its own: drawing a
/// rectangle is the question, and having to then go and open a panel to read
/// the answer would make it two gestures.
///
/// Watched as an edge rather than for the rectangle appearing, because the two
/// halves do not arrive together: a bookmark restores its rectangle long
/// before the catalog's service has said it can count one, and an open keyed
/// on the rectangle alone would have missed it. An edge also leaves the dock
/// closed once it has been closed by hand, until a selection goes away and
/// another is made.
pub fn open_on_selection(
    frames: Query<&ShowsSource, With<FrameRegion>>,
    countable: Query<(), With<RegionSummary>>,
    mut dock: ResMut<SelectionDock>,
    mut had: Local<bool>,
) {
    let now = frames.iter().any(|shows| countable.contains(shows.0));
    if now && !*had {
        dock.open = true;
    }
    *had = now;
}

pub fn close_selection(
    activate: On<Activate>,
    buttons: Query<(), With<SelectionClose>>,
    mut dock: ResMut<SelectionDock>,
) {
    if buttons.get(activate.entity).is_ok() {
        dock.open = false;
    }
}

/// Take the dock's width off the frame grid.
pub fn reserve_space(
    dock: Res<SelectionDock>,
    windows: Query<&Window>,
    mut area: ResMut<FrameArea>,
) {
    let Ok(window) = windows.single() else { return };
    area.reserve_right(dock.current_width(window.width()));
}

/// Match the dock's chrome to its width, and keep its status line honest.
///
/// It sits inboard of the inspector when both are open, so the two stack along
/// the right edge rather than covering each other. Reserving from
/// [`FrameArea`] alone keeps the grid clear of both but says nothing about
/// where either is drawn, so the placement has to agree with it by hand.
pub fn update_selection_dock(
    mut dock: ResMut<SelectionDock>,
    inspector: Res<Inspector>,
    windows: Query<&Window>,
    selected: Res<SelectedPanel>,
    panels: Query<(&ShowsSource, Has<SelectMode>, Has<FrameRegion>)>,
    sources: Query<(&CellProperties, &RegionSummary, Option<&SelectedRegion>)>,
    mut roots: Query<&mut Node, (With<SelectionRoot>, Without<SelectionHandle>)>,
    mut handles: Query<&mut Node, (With<SelectionHandle>, Without<SelectionRoot>)>,
    mut status: Query<&mut Text, With<SelectionStatus>>,
) {
    let Ok(window) = windows.single() else { return };
    let width = dock.current_width(window.width());
    let outboard = inspector.current_width(window.width());
    // Assigned only when it moves: taking the dock mutably every frame would
    // mark it changed, and a changed dock is one the preferences re-examine.
    if dock.outboard != outboard {
        dock.outboard = outboard;
    }
    place_right_dock(
        &mut roots,
        &mut handles,
        dock.open,
        width,
        window.width() - outboard,
    );
    if !dock.open {
        return;
    }

    let frame = selected.0.and_then(|panel| panels.get(panel).ok());
    let found = frame.and_then(|(shows, ..)| sources.get(shows.0).ok());
    let Some((properties, summary, region)) = found else {
        let wanted = "This dataset's cells cannot be counted by where they are.";
        for text in &mut status {
            set_text(text, wanted);
        }
        return;
    };

    let selecting = frame.is_some_and(|(_, selecting, _)| selecting);
    let wanted = match &summary.state {
        SummaryState::Failed(error) => error.clone(),
        SummaryState::Counting => "Counting the cells in the rectangle\u{2026}".to_string(),
        SummaryState::Ready => match color_column(properties) {
            Some((column, name)) => {
                let total: u64 = summary
                    .buckets(&column)
                    .iter()
                    .map(|(_, count)| *count)
                    .sum();
                // Named by what it is broken down by, since that is what the
                // cell panel's coloring decides and what the rows below are.
                format!("{} cells selected, by {}", compact_count(total), name)
            }
            None => "Color the points by a property to break the selection down.".to_string(),
        },
        SummaryState::Idle if region.is_some() => "Rectangle too small to count.".to_string(),
        SummaryState::Idle if selecting => "Drag a rectangle over the frame.".to_string(),
        SummaryState::Idle => "Pick the selection tool in the frame's header.".to_string(),
    };
    for text in &mut status {
        set_text(text, &wanted);
    }
}

/// Rebuild the list of categories and the detail under the one drilled into.
///
/// Keyed on what the rows are built from, not on the numbers in them, so a
/// count landing for the same shape writes into the rows already there.
pub fn rebuild_selection_dock(
    mut commands: Commands,
    dock: Res<SelectionDock>,
    selected: SelectedSource,
    sources: Query<(
        &CellProperties,
        &ColorOverrides,
        &RegionSummary,
        Option<&RegionFocus>,
    )>,
    mut body: Query<(Entity, &mut Node), (With<SelectionBody>, Without<SelectionDetail>)>,
    mut detail: Query<(Entity, &mut Node), (With<SelectionDetail>, Without<SelectionBody>)>,
    existing: Query<Entity, With<SelectionContent>>,
    mut shown: Local<Option<Built>>,
) {
    let Ok((body, mut body_node)) = body.single_mut() else {
        return;
    };
    let Ok((detail, mut detail_node)) = detail.single_mut() else {
        return;
    };
    let source = selected
        .entity()
        .and_then(|source| sources.get(source).ok().map(|found| (source, found)));

    let Some((entity, (properties, overrides, summary, focus))) = source.filter(|_| dock.open)
    else {
        if shown.is_some() {
            *shown = None;
            for entity in &existing {
                commands.entity(entity).despawn();
            }
        }
        return;
    };

    let colored = color_column(properties);
    let categories = colored
        .as_ref()
        .map(|(column, _)| summary.buckets(column))
        .unwrap_or_default();
    let fingerprint = Built {
        source: entity,
        column: colored.as_ref().map(|(column, _)| column.clone()),
        state: summary.state.clone(),
        categories: categories.len(),
        focus: focus.cloned(),
        focus_state: summary.focus_state.clone(),
        detail: summary.cells.len(),
        colors: overrides.clone(),
    };
    if shown.as_ref() == Some(&fingerprint) {
        return;
    }
    *shown = Some(fingerprint);

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let drilled = focus.is_some() && summary.state == SummaryState::Ready && colored.is_some();
    body_node.max_height = if drilled {
        Val::Percent(CATEGORIES_SHARE)
    } else {
        Val::Px(LIST_MAX_PX)
    };
    body_node.flex_shrink = if drilled { 0.0 } else { 1.0 };
    body_node.min_height = Val::ZERO;
    detail_node.display = if drilled {
        Display::Flex
    } else {
        Display::None
    };
    if summary.state != SummaryState::Ready {
        return;
    }
    let Some((column, _)) = colored else { return };

    let mut rows = Vec::new();
    let values = values_of(properties, &column);
    if categories.is_empty() {
        rows.push(
            commands
                .spawn_scene(bsn! {
                    SelectionContent
                    text_dim("No cells in the rectangle.", size::SMALL)
                })
                .id(),
        );
    }
    for (code, count) in categories.iter().take(MAX_ROWS) {
        let value = values.iter().find(|value| value.code == *code);
        let label = value.map_or_else(
            // A code the service counted but the labels do not name. Showing
            // the code is more use than dropping the row, which would make the
            // listed counts fail to add up to the total above them.
            || format!("code {code}"),
            |value| value.label.clone(),
        );
        let swatch = value.map_or(Color::NONE, |value| overrides.swatch(&column, value));
        let picked = focus.is_some_and(|focus| focus.column == column && focus.label == label);
        rows.push(spawn_category_row(
            &mut commands,
            &column,
            &label,
            swatch,
            *count,
            picked,
        ));
    }
    if categories.len() > MAX_ROWS {
        let note = spawn_more_note(&mut commands, categories.len() - MAX_ROWS);
        commands.entity(note).insert(SelectionContent);
        rows.push(note);
    }

    if let Some(focus) = focus {
        // The records are only the first page, so how many there really are
        // comes from the counts rather than from counting the rows.
        let held = values
            .iter()
            .find(|value| value.label == focus.label)
            .and_then(|value| summary.counted_in(&column, value.code));
        let cells = spawn_detail(&mut commands, properties, summary, focus, held);
        commands.entity(detail).add_children(&cells);
    }
    commands.entity(body).add_children(&rows);
}

/// What the rows on screen were built from.
#[derive(PartialEq)]
pub struct Built {
    source: Entity,
    column: Option<String>,
    state: SummaryState,
    categories: usize,
    focus: Option<RegionFocus>,
    focus_state: SummaryState,
    detail: usize,
    /// The colors picked for values, which the rows' squares are drawn in.
    colors: ColorOverrides,
}

/// The cells themselves: every cell of the drilled-into category that the
/// rectangle holds, up to the page the service was asked for, each with
/// everything known about it.
///
/// Records rather than a further breakdown, because the counts above already
/// say what the rectangle is made of; what a category is worth opening for is
/// the cells in it.
fn spawn_detail(
    commands: &mut Commands,
    properties: &CellProperties,
    summary: &RegionSummary,
    focus: &RegionFocus,
    held: Option<u64>,
) -> Vec<Entity> {
    let mut rows = Vec::new();
    let caption = match held {
        Some(held) if held > summary.cells.len() as u64 => {
            format!("{} of {} cells", summary.cells.len(), compact_count(held))
        }
        _ => format!("{} cells", summary.cells.len()),
    };
    rows.push(
        commands
            .spawn_scene(bsn! {
                SelectionContent
                Node {
                    width: { Val::Percent(100.0) },
                    flex_direction: { FlexDirection::Column },
                    margin: { UiRect::top(Val::Px(space::GROUPS)) },
                    flex_shrink: { 0.0_f32 },
                }
                Children [
                    text(focus.label.clone(), size::BODY),
                    text_dim(caption, size::SMALL),
                ]
            })
            .id(),
    );

    if summary.focus_state != SummaryState::Ready {
        let note = match &summary.focus_state {
            SummaryState::Failed(error) => error.clone(),
            _ => "Fetching the cells\u{2026}".to_string(),
        };
        rows.push(
            commands
                .spawn_scene(bsn! {
                    SelectionContent
                    text_dim(note, size::SMALL)
                })
                .id(),
        );
        return rows;
    }

    // Named the way the cell panel names them, and in its order, so a record
    // reads the same way down the sidebar as the controls above it.
    let named = columns_of(properties);
    for cell in &summary.cells {
        rows.push(spawn_cell_record(commands, cell, &named));
    }
    if summary.cells.is_empty() {
        rows.push(
            commands
                .spawn_scene(bsn! {
                    SelectionContent
                    text_dim("No cells of this category in the rectangle.", size::SMALL)
                })
                .id(),
        );
    }
    rows
}

/// One cell: what it is called, and every column it holds something in.
fn spawn_cell_record(
    commands: &mut Commands,
    cell: &CellRecord,
    named: &[(String, String)],
) -> Entity {
    let mut lines = Vec::new();
    for (column, name) in named {
        let Some(value) = cell.value(column) else {
            continue;
        };
        let heading = commands
            .spawn_scene(bsn! {
                text_dim(name.clone(), size::SMALL)
            })
            .id();
        let held = commands
            .spawn_scene(bsn! {
                Text({ value.to_string() })
                TextFont { font_size: { FontSize::Px(size::SECONDARY) } }
                Node { flex_shrink: { 1.0_f32 }, min_width: { Val::ZERO } }
            })
            .id();
        lines.push(
            commands
                .spawn_scene(bsn! {
                    Node {
                        width: { Val::Percent(100.0) },
                        flex_direction: { FlexDirection::Column },
                        flex_shrink: { 0.0_f32 },
                    }
                })
                .add_children(&[heading, held])
                .id(),
        );
    }
    // The cell's own identifier, which is what names the record and what a
    // question about one cell would be asked with.
    let id = commands
        .spawn_scene(bsn! {
            SelectableText
            Text({ cell.id.clone() })
            TextFont { font_size: { FontSize::Px(size::SMALL) } }
            ThemeTextColor({ tokens::TEXT_DIM })
            Node { margin: { UiRect::bottom(Val::Px(space::STACKED)) } }
        })
        .id();
    let record = commands
        .spawn_scene(bsn! {
            SelectionContent
            Node {
                width: { Val::Percent(100.0) },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(space::LIST_ITEMS) },
                flex_shrink: { 0.0_f32 },
                margin: { UiRect::top(Val::Px(space::ROWS)) },
                padding: { UiRect::all(Val::Px(space::CONTROL_INSET)) },
                border_radius: { BorderRadius::all(Val::Px(4.0)) },
            }
            ThemeBackgroundColor({ tokens::BUTTON_BG })
        })
        .id();
    commands.entity(record).add_child(id);
    commands.entity(record).add_children(&lines);
    record
}

/// One category's row: the color its points are drawn in, its label, and how
/// many of the selection's cells hold it. Pressing it drills into that
/// category.
fn spawn_category_row(
    commands: &mut Commands,
    column: &str,
    label: &str,
    swatch: Color,
    count: u64,
    picked: bool,
) -> Entity {
    let caption = format!("{label}  {}", compact_count(count));
    let dot = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Px(10.0) },
                height: { Val::Px(10.0) },
                flex_shrink: { 0.0_f32 },
                border_radius: { BorderRadius::all(Val::Px(2.0)) },
            }
            // The value's own color, not a theme's: it is what its points are
            // painted in, and what the eye is matching it against.
            BackgroundColor({ swatch })
        })
        .id();
    let row = commands
        .spawn_scene(bsn! {
            @FeathersButton {
                @variant: { if picked { ButtonVariant::Primary } else { ButtonVariant::Normal } }
            }
            SelectionContent
            BlocksFrameInput
            Node {
                width: { Val::Percent(100.0) },
                justify_content: { JustifyContent::Start },
                column_gap: { Val::Px(space::ICON_LABEL) },
                flex_shrink: { 0.0_f32 },
            }
            CategoryRow { column: { column.to_string() }, label: { label.to_string() } }
        })
        .id();
    let text = commands
        .spawn_scene(bsn! {
            button_text(caption)
        })
        .id();
    commands.entity(row).add_children(&[dot, text]);
    row
}

/// Drill into the category whose row was pressed, or back out of it when the
/// one already drilled into is pressed again.
pub fn on_category_picked(
    activate: On<Activate>,
    mut commands: Commands,
    rows: Query<&CategoryRow>,
    selected: SelectedSource,
    focused: Query<&RegionFocus>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    let Some(source) = selected.entity() else {
        return;
    };
    let wanted = RegionFocus {
        column: row.column.clone(),
        label: row.label.clone(),
    };
    if focused.get(source).is_ok_and(|focus| *focus == wanted) {
        commands.entity(source).remove::<RegionFocus>();
    } else {
        commands.entity(source).insert(wanted);
    }
}

/// The dock on the right that summarizes the cells inside a dragged rectangle.
pub struct SelectionPanelPlugin;

impl Plugin for SelectionPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_dock::<SelectionDock>()
            .add_observer(on_category_picked)
            .add_observer(close_selection)
            .add_systems(Update, open_on_selection.in_set(Stage::DockInput))
            .add_systems(Update, reserve_space.in_set(Stage::DockReserve))
            .add_systems(Update, rebuild_selection_dock.in_set(Stage::ControlsBuild))
            .add_systems(Update, update_selection_dock.in_set(Stage::Chrome))
            .add_systems(Startup, spawn_selection_dock.in_set(Boot::Shell));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::properties::CellProperty;
    use crate::source::tree::{Tree, TreeLevel, TreeNode};

    fn value(code: u16, label: &str) -> PropertyValue {
        PropertyValue {
            code,
            label: label.into(),
            reference: None,
            color: None,
            count: None,
            selected: false,
        }
    }

    fn categorical(id: &str, name: &str, values: Vec<PropertyValue>) -> CellProperty {
        CellProperty {
            id: id.into(),
            name: name.into(),
            shown: true,
            gene: None,
            kind: PropertyKind::Categorical(values),
        }
    }

    #[test]
    fn a_closed_dock_takes_no_space_from_the_grid() {
        let mut dock = SelectionDock::default();
        assert!(!dock.open);
        assert_eq!(dock.current_width(1600.0), 0.0);
        dock.open = true;
        assert_eq!(dock.current_width(1600.0), WIDTH_PX);
    }

    #[test]
    fn both_right_docks_fit_beside_each_other() {
        // Reserving keeps the grid clear of them but says nothing about where
        // either is drawn; this is the sum the placement has to agree with, or
        // the two would cover each other in the same corner.
        let window = 1600.0;
        let inspector = Inspector {
            open: true,
            ..default()
        };
        let dock = SelectionDock {
            width: DockWidth::new(320.0, MIN_PX),
            open: true,
            outboard: 300.0,
        };
        let outboard = inspector.current_width(window);
        let mine = dock.current_width(window);
        let my_left = window - outboard - mine;
        assert_eq!(my_left + mine, window - outboard);

        let mut area = FrameArea {
            origin: Vec2::ZERO,
            size: Vec2::new(window, 900.0),
        };
        area.reserve_right(outboard);
        area.reserve_right(mine);
        assert_eq!(area.size.x, my_left);
    }

    #[test]
    fn dragging_the_inboard_dock_measures_from_its_own_edge() {
        // The handle's reach is from the window edge, so with the inspector
        // outboard of it a width taken straight from the reach would jump by
        // the inspector's width the moment the handle was grabbed.
        let mut dock = SelectionDock {
            width: DockWidth::new(320.0, MIN_PX),
            open: true,
            outboard: 300.0,
        };
        // The pointer 620px in from the right edge is 320px in from this
        // dock's own edge, so the width should not move.
        dock.drag_to(620.0, 1600.0);
        assert_eq!(dock.width.px, 320.0);
        dock.drag_to(700.0, 1600.0);
        assert_eq!(dock.width.px, 400.0);
    }

    #[test]
    fn a_taxonomy_offers_one_bucket_a_level() {
        // "Class" and "cluster" are different questions about the same cells,
        // and a selection is worth reading at either grain.
        let tree = CellProperty {
            id: "tax".into(),
            name: "Taxonomy".into(),
            shown: true,
            gene: None,
            kind: PropertyKind::Tree(Tree {
                levels: vec![
                    TreeLevel {
                        id: "class".into(),
                        name: "Class".into(),
                    },
                    TreeLevel {
                        id: "cluster".into(),
                        name: "Cluster".into(),
                    },
                ],
                nodes: vec![TreeNode {
                    level: 0,
                    parent: None,
                    value: value(1, "Glut"),
                }],
                color_level: 0,
            }),
        };
        let properties = CellProperties::ready(vec![
            tree,
            categorical("nt", "Neurotransmitter", vec![value(0, "GABA")]),
        ]);
        let names: Vec<String> = columns_of(&properties)
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        assert_eq!(names, ["Class", "Cluster", "Neurotransmitter"]);
    }

    #[test]
    fn a_property_the_cell_panel_hides_is_still_worth_knowing_here() {
        // Hiding a property there says it is not worth a filter's worth of
        // screen, not that it says nothing about the cells just selected.
        let mut hidden = categorical("nt", "Neurotransmitter", vec![value(0, "GABA")]);
        hidden.shown = false;
        let properties = CellProperties::ready(vec![hidden]);
        assert_eq!(
            columns_of(&properties),
            [("nt".to_string(), "Neurotransmitter".to_string())]
        );
    }

    #[test]
    fn a_numeric_property_is_not_a_bucket() {
        let properties = CellProperties::ready(vec![CellProperty {
            id: "qc".into(),
            name: "QC score".into(),
            shown: true,
            gene: None,
            kind: PropertyKind::Numeric(crate::source::properties::NumericRange::full(
                0.0,
                1.0,
                Vec::new(),
            )),
        }]);
        assert!(columns_of(&properties).is_empty());
    }

    #[test]
    fn a_selections_buckets_are_read_largest_first() {
        // A selection is read for what is mostly in it, and a taxonomy's own
        // order buries that under hundreds of values holding nothing.
        let summary = RegionSummary {
            state: SummaryState::Ready,
            values: vec![("class".into(), vec![(0, 5), (1, 900), (2, 0), (3, 40)])],
            ..default()
        };
        assert_eq!(summary.buckets("class"), [(1, 900), (3, 40), (0, 5)]);
    }

    #[test]
    fn a_record_answers_only_for_the_columns_its_cell_holds() {
        // A column a cell holds nothing in is left out by the service rather
        // than sent empty, so a record has to cope with gaps.
        let cell = CellRecord {
            id: "017a21b9".into(),
            index: 24,
            values: vec![
                ("class".into(), "30 Astro-Epen".into()),
                ("sex".into(), "M".into()),
            ],
        };
        assert_eq!(cell.value("class"), Some("30 Astro-Epen"));
        assert_eq!(cell.value("dissection"), None);
    }

    #[test]
    fn how_many_a_category_holds_comes_from_the_counts_not_the_records() {
        // The records are only ever the first page of them, so counting the
        // rows would under-report a category by however many were left behind.
        let summary = RegionSummary {
            state: SummaryState::Ready,
            values: vec![("class".into(), vec![(0, 5), (1, 900)])],
            cells: vec![CellRecord {
                id: "a".into(),
                index: 0,
                values: Vec::new(),
            }],
            ..default()
        };
        assert_eq!(summary.counted_in("class", 1), Some(900));
        assert_eq!(summary.counted_in("class", 7), None);
    }

    #[test]
    fn a_column_nobody_counted_says_nothing_rather_than_zero() {
        // "Still counting" has to read differently from "no cells here", or a
        // selection being counted looks like a selection holding nothing.
        let summary = RegionSummary::default();
        assert!(summary.buckets("class").is_empty());
        assert_eq!(summary.counted_in("class", 0), None);
    }

    #[test]
    fn the_summary_follows_whatever_the_points_are_colored_by() {
        // The categories on screen are the ones the eye is already reading the
        // rectangle by, so the list follows the cell panel's palette rather
        // than offering every column at once.
        let mut properties = CellProperties::ready(vec![
            categorical("nt", "Neurotransmitter", vec![value(0, "GABA")]),
            categorical("sex", "Sex", vec![value(0, "F"), value(1, "M")]),
        ]);
        properties.color_by = Some(1);
        assert_eq!(
            color_column(&properties),
            Some(("sex".to_string(), "Sex".to_string()))
        );
        properties.color_by = None;
        assert_eq!(color_column(&properties), None);
    }

    #[test]
    fn coloring_by_a_taxonomy_names_the_level_it_colors() {
        // A tree colors by one of its levels, and that level is both the
        // column counted and the name the cell panel shows beside it.
        let properties = CellProperties::ready(vec![CellProperty {
            id: "tax".into(),
            name: "Taxonomy".into(),
            shown: true,
            gene: None,
            kind: PropertyKind::Tree(Tree {
                levels: vec![
                    TreeLevel {
                        id: "class".into(),
                        name: "Class".into(),
                    },
                    TreeLevel {
                        id: "cluster".into(),
                        name: "Cluster".into(),
                    },
                ],
                nodes: vec![TreeNode {
                    level: 1,
                    parent: None,
                    value: value(1, "0505 DG Glut_2"),
                }],
                color_level: 1,
            }),
        }]);
        assert_eq!(
            color_column(&properties),
            Some(("cluster".to_string(), "Cluster".to_string()))
        );
    }

    #[test]
    fn values_are_found_in_whichever_property_holds_the_column() {
        let properties = CellProperties::ready(vec![
            categorical("nt", "Neurotransmitter", vec![value(0, "GABA")]),
            categorical("sex", "Sex", vec![value(0, "F"), value(1, "M")]),
        ]);
        let found: Vec<&str> = values_of(&properties, "sex")
            .into_iter()
            .map(|value| value.label.as_str())
            .collect();
        assert_eq!(found, ["F", "M"]);
        assert!(values_of(&properties, "nothing").is_empty());
    }
}
