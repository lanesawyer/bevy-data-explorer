//! Frames that show rows and columns.
//!
//! A source carrying a [`SourceTable`] has nowhere to look around: its rows
//! are records rather than a place. So its frame is not a view that pans and
//! zooms but a table that fills the cell and scrolls — a header that stays put
//! while the rows move under it, and the scrollbars every scrolling area in
//! the app is given.
//!
//! It knows nothing about any particular format, the same way the overlay does
//! not: it reads the rows off whichever source a frame points at, so a table
//! looks the same wherever the rows came from.
//!
//! Only the rows on screen exist. A hundred thousand of them would be a
//! hundred thousand nodes laid out every frame, so the content node is given
//! the full height and the rows inside it are placed absolutely within it.
//! That leaves the scrollbar measuring the whole table while only a screenful
//! is ever built — the same trade the tile and point streamers make, decided
//! here by what is on screen rather than by what has been fetched.
//!
//! A column is as wide as its widest value until its heading's right edge is
//! dragged. A value is copied with the button that appears at the end of the
//! cell under the pointer, and copied whole, however much of it the cell had
//! room to show. The text itself is not selectable: that would take an
//! editable text per cell, and a selection would stop at the ellipsis.

use std::collections::HashMap;
use std::ops::Range;

use bevy::clipboard::Clipboard;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, ScrollPosition};
use bevy::window::SystemCursorIcon;
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::cursor::{EntityCursor, OverrideCursor};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};
use bevy_feathers::tokens;
use bevy_ui_widgets::Activate;

use crate::app::schedule::Stage;
use crate::app::theme::{Palette, token};
use crate::source::table::{
    ColumnWidths, HiddenColumns, SourceTable, TablePaging, TableSort, to_first_page,
};
use crate::source::{ShowsSource, grouped};
use crate::widgets::space;
use crate::widgets::{
    BlocksFrameInput, Icon, Menu, MenuButton, ScrollBoth, button_icon, hold_drag_cursor, icon_text,
    patch_node, set_display, set_text, size, text, text_dim, truncate_to_width, width_of,
};

use super::chrome::{BUTTON_PX, CHROME_GAP};
use super::overlay::CHROME_INSET;
use super::overlay::PanelHeader;
use super::{FrameArea, Panel, SelectedPanel};

/// How tall one row is.
const ROW_PX: f32 = 24.0;

/// How tall the header strip is. Taller than a row, so the header reads as a
/// heading rather than as the first record.
const HEADER_PX: f32 = 30.0;

/// Clear space at each side of a cell's text.
const PAD_PX: f32 = space::CONTROL_INSET;

/// Rows built past the ones on screen, at each end, so that a scroll shows the
/// next row rather than a gap while it is built.
const OVERSCAN: usize = 3;

/// A ceiling on the rows built at once, however tall a frame is made.
const MAX_LIVE_ROWS: usize = 400;

/// Drawn over the frame's cleared cell, and under the header, the buttons and
/// the selection outline, which all sit at zero and must stay reachable.
const TABLE_Z: i32 = -1;

/// Clear space left between the frame's own chrome and the top of the table.
///
/// The chrome is drawn over its frame rather than beside it, so a table that
/// started at the top of the cell would have the name, the status and the
/// buttons sitting on its first rows. It starts under them, as far below
/// as the status is below the buttons.
const CHROME_GAP_PX: f32 = CHROME_GAP;

/// Where the frame's chrome starts, which is where [`super::overlay`] puts it.
const CHROME_TOP_PX: f32 = CHROME_INSET;

/// How tall the strip of paging buttons along the bottom is.
const FOOTER_PX: f32 = 32.0;

/// Room a heading keeps beside its name for the arrow saying which way the
/// column is sorted, and the number saying where it falls among several.
const SORT_MARK_PX: f32 = 24.0;

/// How wide the strip at a heading's right edge that resizes its column is.
const GRIP_PX: f32 = 12.0;

/// The narrowest a column can be dragged: room for an ellipsis and the copy
/// button, not for nothing.
const MIN_COLUMN_PX: f32 = 32.0;

/// How big the button that copies a cell is, square. Under a row's height, so
/// it sits inside the cell rather than over the rows either side.
const COPY_PX: f32 = 20.0;

/// The table filling one frame.
#[derive(Component)]
pub struct TableView {
    pub panel: Entity,
    /// The source these rows came from, so a frame repointed at another table
    /// is rebuilt rather than redrawn with the wrong columns.
    source: Entity,
    /// The row of headings, moved sideways against the horizontal scroll so it
    /// stays over the columns it names.
    headings: Entity,
    /// Each drawn column's heading and the name in it, in order, moved and
    /// cut to length as its column is resized.
    heading_cells: Vec<(Entity, Entity)>,
    /// The one button copying whichever cell is under the pointer.
    copy: Entity,
    /// The scrolling area, and the full-height node inside it the rows are
    /// placed in.
    body: Entity,
    content: Entity,
    /// The strip of paging buttons along the bottom, and the line in it that
    /// says which rows are on screen.
    footer: Entity,
    readout: Entity,
    /// The columns drawn, as places in the source's own, in order.
    visible: Vec<usize>,
    /// Where each drawn column starts, the numbering gutter first, with the
    /// whole width on the end.
    edges: Vec<f32>,
    /// The rows on screen, by their place on the page.
    live: HashMap<usize, Entity>,
}

impl TableView {
    fn width(&self) -> f32 {
        self.edges.last().copied().unwrap_or_default()
    }
}

/// How a cell is set: a value, or the number in the gutter.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Value,
    Number,
}

/// A value's cell, which the copy button comes to when it is hovered.
///
/// Only cells holding something are marked: there is nothing to copy out of
/// an empty one.
#[derive(Component)]
pub struct TableCell {
    panel: Entity,
    /// The row on the page, and the column as a place in the source's own.
    row: usize,
    at: usize,
    /// The column as a place among those drawn.
    drawn: usize,
}

/// The button that copies the hovered cell of a frame's table.
#[derive(Component)]
pub struct CopyCellButton {
    panel: Entity,
    /// The cell it sits in, as a row and a column in the source's own.
    target: Option<(usize, usize)>,
    /// The cell it last copied, which it shows a tick for until it moves on.
    copied: Option<(usize, usize)>,
    icon: Entity,
}

/// The strip at a heading's right edge that drags its column wider or
/// narrower.
#[derive(Component)]
pub struct ColumnGrip {
    panel: Entity,
    column: String,
    /// The column as a place among those drawn.
    drawn: usize,
    /// How wide the column was when the drag began, while one is going on.
    from: Option<f32>,
}

/// A heading that sorts its table when pressed.
#[derive(Component)]
pub struct TableHeading {
    panel: Entity,
    column: String,
}

/// Beside a heading, what says how its column is sorted: the arrow, or with
/// `rank`, where it falls among several columns sorted at once.
#[derive(Component)]
pub struct SortMark {
    panel: Entity,
    column: String,
    rank: bool,
}

/// Which way a button moves through the pages.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageStep {
    First,
    Previous,
    Next,
    Last,
}

impl PageStep {
    const ALL: [PageStep; 4] = [
        PageStep::First,
        PageStep::Previous,
        PageStep::Next,
        PageStep::Last,
    ];

    fn icon(self) -> Icon {
        match self {
            PageStep::First => Icon::ChevronFirst,
            PageStep::Previous => Icon::ChevronLeft,
            PageStep::Next => Icon::ChevronRight,
            PageStep::Last => Icon::ChevronLast,
        }
    }

    /// Where this step lands from `paging`, or nothing if it would not move.
    fn from(self, paging: &TablePaging) -> Option<usize> {
        let wanted = match self {
            PageStep::First => 0,
            PageStep::Previous if !paging.has_previous() => return None,
            PageStep::Previous => paging.page - 1,
            PageStep::Next if !paging.has_next() => return None,
            PageStep::Next => paging.page + 1,
            PageStep::Last => paging.last_page()?,
        };
        (wanted != paging.page).then_some(wanted)
    }
}

/// A button that moves a frame through its table's pages.
#[derive(Component, Clone, Copy)]
pub struct TablePageButton {
    pub panel: Entity,
    pub step: PageStep,
}

impl Default for TablePageButton {
    fn default() -> Self {
        TablePageButton {
            panel: Entity::PLACEHOLDER,
            step: PageStep::First,
        }
    }
}

/// The columns of `table` a frame draws: all but the hidden ones.
fn visible_of(table: &SourceTable, hidden: Option<&HiddenColumns>) -> Vec<usize> {
    (0..table.columns.len())
        .filter(|&at| hidden.is_none_or(|hidden| !hidden.0.contains(&table.columns[at].name)))
        .collect()
}

/// Where each drawn column starts, the numbering gutter first, with the full
/// width last.
///
/// Sized from the characters a format already counted rather than from laying
/// the text out, which would mean reacting to the measurement a frame later.
/// The gutter is sized for the *last* row number rather than the page's, since
/// the numbers carry on across pages. A table that sorts keeps room in every
/// heading for the mark saying how. A column dragged to a width keeps it.
fn edges_of(
    table: &SourceTable,
    visible: &[usize],
    total: usize,
    sorts: bool,
    widths: Option<&ColumnWidths>,
) -> Vec<f32> {
    let gutter = width_of(total.max(1).to_string().len(), size::SMALL) + PAD_PX * 2.0;
    let mark = if sorts { SORT_MARK_PX } else { 0.0 };
    let mut edges = vec![0.0, gutter];
    for &at in visible {
        let last = edges.last().copied().unwrap_or_default();
        let column = &table.columns[at];
        let width = widths
            .and_then(|widths| widths.0.get(&column.name))
            .map_or_else(
                || width_of(column.chars, size::SECONDARY) + mark + PAD_PX * 2.0,
                |&width| width.max(MIN_COLUMN_PX),
            );
        edges.push(last + width);
    }
    edges
}

/// How wide a column dragged `by` from `from` becomes: never narrower than
/// [`MIN_COLUMN_PX`], and in whole pixels, which is what a bookmark keeps.
fn dragged_width(from: f32, by: f32) -> f32 {
    (from + by).round().max(MIN_COLUMN_PX)
}

/// What a frame's table is laid out from, asked of its source.
type LayoutQuery = (
    &'static SourceTable,
    &'static TablePaging,
    Option<&'static HiddenColumns>,
    Has<TableSort>,
    Option<&'static ColumnWidths>,
);

/// The answer: the rows, the paging, what is hidden, whether it sorts, and
/// what has been dragged wider or narrower.
type Layout<'a> = (
    &'a SourceTable,
    &'a TablePaging,
    Option<&'a HiddenColumns>,
    bool,
    Option<&'a ColumnWidths>,
);

/// The columns drawn and where each starts.
fn layout_of((table, paging, hidden, sorts, widths): Layout) -> (Vec<usize>, Vec<f32>) {
    let visible = visible_of(table, hidden);
    let edges = edges_of(
        table,
        &visible,
        paging.total.unwrap_or_default(),
        sorts,
        widths,
    );
    (visible, edges)
}

/// Give every frame showing a table one, and take it away from every frame
/// that has stopped showing one.
pub fn sync_tables(
    mut commands: Commands,
    panels: Query<(Entity, &ShowsSource), With<Panel>>,
    tables: Query<LayoutQuery>,
    mut views: Query<(Entity, &mut TableView)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    for (entity, mut view) in &mut views {
        // Rebuilt rather than redrawn when the columns change: a frame pointed
        // at another table, one whose source grew a column it had not seen
        // on the first page, or one with a column hidden or shown, is laid
        // out afresh.
        let Ok((_, shows)) = panels.get(view.panel) else {
            commands.entity(entity).despawn();
            continue;
        };
        if shows.0 != view.source {
            commands.entity(entity).despawn();
            continue;
        }
        let Ok(layout) = tables.get(shows.0) else {
            continue;
        };
        let (visible, edges) = layout_of(layout);
        if visible != view.visible {
            commands.entity(entity).despawn();
        } else if edges != view.edges {
            // The same columns at other widths are moved rather than rebuilt:
            // a heading being dragged has to outlive the drag, and the table
            // has to stay scrolled where it was.
            resize_table(
                &mut commands,
                &mut view,
                layout,
                edges,
                &mut nodes,
                &mut texts,
            );
        }
    }

    for (panel, shows) in &panels {
        let Ok(layout) = tables.get(shows.0) else {
            continue;
        };
        if views
            .iter()
            .any(|(_, view)| view.panel == panel && view.source == shows.0)
        {
            continue;
        }
        spawn_table(&mut commands, panel, shows.0, layout);
    }
}

fn spawn_table(commands: &mut Commands, panel: Entity, source: Entity, layout: Layout) {
    let (table, _, _, sorts, _) = layout;
    let (visible, edges) = layout_of(layout);
    let width = edges.last().copied().unwrap_or_default();

    let headings = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            height: Val::Percent(100.0),
            ..default()
        })
        .id();
    let mut heading_cells = Vec::with_capacity(visible.len());
    for (drawn, &at) in visible.iter().enumerate() {
        let column = &table.columns[at];
        let (cell, label) = spawn_heading(
            commands,
            &edges,
            drawn,
            &column.name,
            column.numeric,
            panel,
            sorts,
        );
        commands.entity(headings).add_child(cell);
        heading_cells.push((cell, label));
    }
    // The headings are clipped by the strip around them rather than by
    // themselves, which is what lets them slide sideways under it.
    let strip = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(HEADER_PX),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                ..default()
            },
            ThemeBackgroundColor(token::OVERLAY_BG),
        ))
        .id();
    commands.entity(strip).add_child(headings);

    let content = commands
        .spawn(Node {
            width: Val::Px(width),
            height: Val::Px(table.rows.len() as f32 * ROW_PX),
            flex_shrink: 0.0,
            ..default()
        })
        .id();
    let copy = spawn_copy_button(commands, panel);
    commands.entity(content).add_child(copy);
    let body = commands
        .spawn((
            // Scrolls both ways: a table is as wide as its columns and as
            // tall as its rows, and neither is the frame's business. Shift
            // turns the wheel sideways, which is the only way across for a
            // wheel that has no sideways axis.
            ScrollBoth,
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                // What lets it shrink below its contents, which is the whole
                // point of its being the thing that scrolls.
                min_height: Val::ZERO,
                overflow: Overflow::scroll(),
                ..default()
            },
        ))
        .id();
    commands.entity(body).add_child(content);

    let readout = commands
        .spawn_scene(text_dim(String::new(), size::SMALL))
        .id();
    let footer = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(FOOTER_PX),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(space::CONTROLS),
                ..default()
            },
            ThemeBackgroundColor(token::OVERLAY_BG),
        ))
        .id();
    // Two buttons, the readout, two buttons: the row reads outward from where
    // it says where you are.
    let buttons: Vec<Entity> = PageStep::ALL
        .iter()
        .map(|step| page_button(commands, panel, *step))
        .collect();
    commands
        .entity(footer)
        .add_children(&[buttons[0], buttons[1], readout, buttons[2], buttons[3]]);

    commands
        .spawn((
            TableView {
                panel,
                source,
                headings,
                heading_cells,
                copy,
                footer,
                readout,
                body,
                content,
                visible,
                edges,
                live: HashMap::new(),
            },
            // It covers the whole cell, so it has to stop the pointer reaching
            // the frame behind it — there is nothing there to pan over. It
            // holds no button of its own, so it carries an `Interaction` to be
            // found by.
            BlocksFrameInput,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            GlobalZIndex(TABLE_Z),
        ))
        .add_children(&[strip, body, footer]);
}

fn page_button(commands: &mut Commands, panel: Entity, step: PageStep) -> Entity {
    commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_icon(step.icon())] }
            }
            BlocksFrameInput
            TablePageButton { panel: { panel }, step: { step } }
        })
        .id()
}

/// Where the `drawn`th column starts and how wide it is.
fn column_span(edges: &[f32], drawn: usize) -> (f32, f32) {
    (edges[drawn + 1], edges[drawn + 2] - edges[drawn + 1])
}

/// As much of a heading's name as fits a column `width` wide.
fn heading_text(name: &str, width: f32, sorts: bool) -> String {
    let mark = if sorts { SORT_MARK_PX } else { 0.0 };
    truncate_to_width(name.trim(), width - mark - PAD_PX * 2.0, size::SECONDARY)
}

/// A heading: the column's name, the grip at its right edge that resizes it,
/// and beside the name the mark saying how the column is sorted when `sorts`.
/// Returns the heading and the name in it.
fn spawn_heading(
    commands: &mut Commands,
    edges: &[f32],
    drawn: usize,
    name: &str,
    numeric: bool,
    panel: Entity,
    sorts: bool,
) -> (Entity, Entity) {
    let (left, width) = column_span(edges, drawn);
    let label = commands
        .spawn_scene(text_dim(heading_text(name, width, sorts), size::SECONDARY))
        .id();
    let line = commands
        .spawn((
            Node {
                width: Val::Px(1.0),
                height: Val::Percent(50.0),
                ..default()
            },
            ThemeBackgroundColor(token::DIVIDER),
        ))
        .id();
    let grip = commands
        .spawn((
            ColumnGrip {
                panel,
                column: name.to_string(),
                drawn,
                from: None,
            },
            Node {
                position_type: PositionType::Absolute,
                right: Val::ZERO,
                width: Val::Px(GRIP_PX),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::End,
                ..default()
            },
            EntityCursor::System(SystemCursorIcon::ColResize),
        ))
        .add_child(line)
        .id();
    let cell = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(left),
            width: Val::Px(width),
            height: Val::Percent(100.0),
            padding: UiRect::horizontal(Val::Px(PAD_PX)),
            column_gap: Val::Px(space::ICON_LABEL),
            align_items: AlignItems::Center,
            justify_content: if numeric {
                JustifyContent::End
            } else {
                JustifyContent::Start
            },
            overflow: Overflow::clip(),
            ..default()
        })
        .add_children(&[label, grip])
        .id();
    if !sorts {
        return (cell, label);
    }

    let arrow = commands
        .spawn_scene(bsn! {
            icon_text(Icon::ChevronUp)
            ThemeTextColor({ tokens::TEXT_DIM })
        })
        .insert(SortMark {
            panel,
            column: name.to_string(),
            rank: false,
        })
        .id();
    let rank = commands
        .spawn_scene(text_dim(String::new(), size::SMALL))
        .insert(SortMark {
            panel,
            column: name.to_string(),
            rank: true,
        })
        .id();
    commands
        .entity(cell)
        .insert((
            TableHeading {
                panel,
                column: name.to_string(),
            },
            EntityCursor::System(SystemCursorIcon::Pointer),
        ))
        .add_children(&[arrow, rank]);
    (cell, label)
}

/// Move a table's headings to new column widths, and have its rows rebuilt at
/// them.
fn resize_table(
    commands: &mut Commands,
    view: &mut TableView,
    (table, _, _, sorts, _): Layout,
    edges: Vec<f32>,
    nodes: &mut Query<&mut Node>,
    texts: &mut Query<&mut Text>,
) {
    view.edges = edges;
    for (drawn, (&at, &(cell, label))) in view.visible.iter().zip(&view.heading_cells).enumerate() {
        let (left, width) = column_span(&view.edges, drawn);
        if let Ok(node) = nodes.get_mut(cell) {
            patch_node(node, |node| {
                node.left = Val::Px(left);
                node.width = Val::Px(width);
            });
        }
        if let Ok(text) = texts.get_mut(label) {
            set_text(text, &heading_text(&table.columns[at].name, width, sorts));
        }
    }
    let width = view.width();
    if let Ok(node) = nodes.get_mut(view.content) {
        patch_node(node, |node| node.width = Val::Px(width));
    }
    for row in view.live.drain().map(|(_, row)| row) {
        commands.entity(row).despawn();
    }
}

/// Note how wide a column is as its grip starts to be dragged.
pub fn on_grip_drag_start(
    start: On<Pointer<DragStart>>,
    mut grips: Query<&mut ColumnGrip>,
    views: Query<&TableView>,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    let Ok(mut grip) = grips.get_mut(start.entity) else {
        return;
    };
    let Some(view) = views.iter().find(|view| view.panel == grip.panel) else {
        return;
    };
    grip.from = Some(column_span(&view.edges, grip.drawn).1);
}

/// Resize a column to follow its grip.
pub fn on_grip_drag(
    drag: On<Pointer<Drag>>,
    grips: Query<&ColumnGrip>,
    panels: Query<&ShowsSource>,
    mut widths: Query<&mut ColumnWidths>,
) {
    let Ok(grip) = grips.get(drag.entity) else {
        return;
    };
    let Some(from) = grip.from else {
        return;
    };
    let Ok(mut widths) = panels
        .get(grip.panel)
        .and_then(|shows| widths.get_mut(shows.0))
    else {
        return;
    };
    let width = dragged_width(from, drag.distance.x);
    if widths.0.get(&grip.column) != Some(&width) {
        widths.0.insert(grip.column.clone(), width);
    }
}

pub fn on_grip_drag_end(end: On<Pointer<DragEnd>>, mut grips: Query<&mut ColumnGrip>) {
    if let Ok(mut grip) = grips.get_mut(end.entity) {
        grip.from = None;
    }
}

/// Hold the resize cursor for as long as a column is being dragged, which
/// leaves its grip behind as soon as it starts.
pub fn hold_grip_cursor(
    grips: Query<&ColumnGrip>,
    mut held: Local<bool>,
    cursor: Option<ResMut<OverrideCursor>>,
) {
    let dragging = grips.iter().any(|grip| grip.from.is_some());
    hold_drag_cursor(dragging, &mut held, cursor, SystemCursorIcon::ColResize);
}

fn spawn_copy_button(commands: &mut Commands, panel: Entity) -> Entity {
    let icon = commands.spawn_scene(button_icon(Icon::Copy)).id();
    commands
        .spawn((
            CopyCellButton {
                panel,
                target: None,
                copied: None,
                icon,
            },
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(COPY_PX),
                height: Val::Px(COPY_PX),
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            ThemeBackgroundColor(tokens::BUTTON_BG),
            EntityCursor::System(SystemCursorIcon::Pointer),
            // Over the rows, which are added to the same parent after it.
            ZIndex(1),
        ))
        .add_child(icon)
        .id()
}

/// Put each table's copy button at the end of the cell under the pointer, and
/// take it away when the pointer is over no cell of that table.
///
/// Left where it is while the pointer is on the button itself, which covers
/// the end of the cell it copies.
pub fn place_copy_buttons(
    hover: Res<HoverMap>,
    cells: Query<&TableCell>,
    parents: Query<&ChildOf>,
    views: Query<&TableView>,
    mut buttons: Query<&mut CopyCellButton>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let hovered: Vec<Entity> = hover
        .values()
        .flat_map(|hits| hits.keys())
        .flat_map(|&hit| std::iter::once(hit).chain(parents.iter_ancestors(hit)))
        .collect();
    for view in &views {
        let Ok(mut button) = buttons.get_mut(view.copy) else {
            continue;
        };
        if !hovered.contains(&view.copy) {
            let cell = hovered
                .iter()
                .filter_map(|&entity| cells.get(entity).ok())
                .find(|cell| cell.panel == view.panel);
            let target = cell.map(|cell| (cell.row, cell.at));
            if button.target != target {
                button.target = target;
                button.copied = None;
            }
            if let Some(cell) = cell
                && let Ok(node) = nodes.get_mut(view.copy)
            {
                let (left, width) = column_span(&view.edges, cell.drawn);
                let inset = (ROW_PX - COPY_PX) / 2.0;
                patch_node(node, |node| {
                    node.left = Val::Px(left + width - COPY_PX - inset);
                    node.top = Val::Px(cell.row as f32 * ROW_PX + inset);
                });
            }
        }
        set_display(&mut nodes, view.copy, button.target.is_some());
        let icon = if button.copied.is_some() && button.copied == button.target {
            Icon::Check
        } else {
            Icon::Copy
        };
        if let Ok(text) = texts.get_mut(button.icon) {
            set_text(text, icon.glyph());
        }
    }
}

/// Copy the whole value of the cell a pressed copy button sits in.
pub fn on_copy_pressed(
    click: On<Pointer<Click>>,
    mut buttons: Query<&mut CopyCellButton>,
    panels: Query<&ShowsSource>,
    tables: Query<&SourceTable>,
    mut clipboard: ResMut<Clipboard>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(mut button) = buttons.get_mut(click.entity) else {
        return;
    };
    let Some((row, at)) = button.target else {
        return;
    };
    let Ok(table) = panels
        .get(button.panel)
        .and_then(|shows| tables.get(shows.0))
    else {
        return;
    };
    if row >= table.rows.len() {
        return;
    }
    match clipboard.set_text(table.cell(row, at).trim()) {
        Ok(()) => button.copied = Some((row, at)),
        Err(e) => warn!("could not copy the cell: {e}"),
    }
}

/// Sort a frame's table by the heading pressed: ascending, descending, then
/// not at all. With shift held, the column is sorted within the ones already
/// sorted rather than replacing them.
pub fn on_heading_pressed(
    click: On<Pointer<Click>>,
    headings: Query<&TableHeading>,
    keys: Res<ButtonInput<KeyCode>>,
    grips: Query<(), With<ColumnGrip>>,
    panels: Query<&ShowsSource>,
    mut sources: Query<(&mut TableSort, Option<&mut TablePaging>)>,
    mut positions: Query<&mut ScrollPosition>,
    views: Query<&TableView>,
) {
    // A grip sits inside its heading, so the release ending a resize would
    // otherwise sort the column too.
    if click.button != PointerButton::Primary || grips.contains(click.original_event_target()) {
        return;
    }
    let Ok(heading) = headings.get(click.entity) else {
        return;
    };
    let Ok((mut sort, paging)) = panels
        .get(heading.panel)
        .and_then(|shows| sources.get_mut(shows.0))
    else {
        return;
    };
    let add = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    sort.press(&heading.column, add);
    // A table in another order is a new table, read from its first row.
    to_first_page(paging);
    for view in &views {
        if view.panel == heading.panel
            && let Ok(mut at) = positions.get_mut(view.body)
        {
            at.y = 0.0;
        }
    }
}

/// Show beside each heading which way its column is sorted, and where it falls
/// among several.
pub fn update_sort_marks(
    panels: Query<&ShowsSource>,
    sorts: Query<&TableSort>,
    mut marks: Query<(&SortMark, &mut Text)>,
) {
    for (mark, text) in &mut marks {
        let sort = panels
            .get(mark.panel)
            .and_then(|shows| sorts.get(shows.0))
            .ok();
        let key = sort.and_then(|sort| sort.key(&mark.column));
        let wanted = match key {
            Some((at, _)) if mark.rank && sort.is_some_and(|sort| sort.0.len() > 1) => {
                (at + 1).to_string()
            }
            Some((_, key)) if !mark.rank => {
                let icon = if key.descending {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronUp
                };
                icon.glyph().to_string()
            }
            _ => String::new(),
        };
        set_text(text, &wanted);
    }
}

/// Turn the page of the frame whose button was pressed.
pub fn on_page_pressed(
    activate: On<Activate>,
    buttons: Query<&TablePageButton>,
    panels: Query<&ShowsSource>,
    mut paging: Query<&mut TablePaging>,
    mut positions: Query<&mut ScrollPosition>,
    views: Query<&TableView>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Ok(shows) = panels.get(button.panel) else {
        return;
    };
    let Ok(mut paging) = paging.get_mut(shows.0) else {
        return;
    };
    let Some(page) = button.step.from(&paging) else {
        return;
    };
    paging.page = page;

    // A new page is read from its first row, not from wherever the last one
    // was left. Sideways is left alone: the columns have not moved.
    for view in &views {
        if view.panel == button.panel
            && let Ok(mut at) = positions.get_mut(view.body)
        {
            at.y = 0.0;
        }
    }
}

/// Say which rows are on screen, and offer only the steps that lead anywhere.
///
/// A table that fits on one page has no footer at all: there is nowhere to go,
/// and the frame's own status already says how many rows there are.
pub fn update_footers(
    mut commands: Commands,
    views: Query<&TableView>,
    panels: Query<&ShowsSource>,
    tables: Query<(&SourceTable, &TablePaging)>,
    mut buttons: Query<(Entity, &TablePageButton, Has<InteractionDisabled>)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    for view in &views {
        let Ok((table, paging)) = panels.get(view.panel).and_then(|shows| tables.get(shows.0))
        else {
            continue;
        };
        let several = paging.pages().is_none_or(|pages| pages > 1);
        if let Ok(node) = nodes.get_mut(view.footer) {
            let wanted = if several {
                Display::Flex
            } else {
                Display::None
            };
            patch_node(node, |node| node.display = wanted);
        }
        if !several {
            continue;
        }
        if let Ok(text) = texts.get_mut(view.readout) {
            let next = readout(table, paging);
            set_text(text, &next);
        }
    }

    for (entity, button, disabled) in &mut buttons {
        let leads_somewhere = panels
            .get(button.panel)
            .and_then(|shows| tables.get(shows.0))
            .is_ok_and(|(_, paging)| button.step.from(paging).is_some());
        if leads_somewhere == disabled {
            if leads_somewhere {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
    }
}

/// Which rows are on screen, of how many.
fn readout(table: &SourceTable, paging: &TablePaging) -> String {
    if table.rows.is_empty() {
        return "no rows".to_string();
    }
    let first = table.first + 1;
    let last = table.first + table.rows.len();
    match paging.total {
        Some(total) => format!(
            "{} \u{2013} {} of {}",
            grouped(first),
            grouped(last),
            grouped(total)
        ),
        None => format!("{} \u{2013} {}", grouped(first), grouped(last)),
    }
}

/// One cell, sized to its column and holding as much of `value` as fits.
fn spawn_cell(
    commands: &mut Commands,
    edges: &[f32],
    column: usize,
    value: &str,
    numeric: bool,
    style: Cell,
) -> Entity {
    let width = edges[column + 1] - edges[column];
    let font = if style == Cell::Number {
        size::SMALL
    } else {
        size::SECONDARY
    };
    // A number reads from its last digit, so it is set flush right; so is the
    // gutter, which is nothing but numbers.
    let right = numeric || style == Cell::Number;
    let content = truncate_to_width(value.trim(), width - PAD_PX * 2.0, font);
    let label = match style {
        Cell::Value => commands.spawn_scene(text(content, font)).id(),
        Cell::Number => commands.spawn_scene(text_dim(content, font)).id(),
    };
    let cell = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(edges[column]),
            width: Val::Px(width),
            height: Val::Percent(100.0),
            padding: UiRect::horizontal(Val::Px(PAD_PX)),
            align_items: AlignItems::Center,
            justify_content: if right {
                JustifyContent::End
            } else {
                JustifyContent::Start
            },
            overflow: Overflow::clip(),
            ..default()
        })
        .id();
    commands.entity(cell).add_child(label);
    cell
}

/// Select a table's frame when it is pressed anywhere, as a view's frame is.
///
/// The table covers its frame and stops the pointer reaching it, so the frame
/// never hears the press that would have selected it.
pub fn select_pressed_tables(
    buttons: Res<ButtonInput<MouseButton>>,
    hover: Res<HoverMap>,
    views: Query<&TableView>,
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

/// Fit each table to the frame it fills, and hold its headings over the
/// columns they name.
pub fn place_tables(
    area: Res<FrameArea>,
    panels: Query<&Panel>,
    views: Query<(Entity, &TableView)>,
    positions: Query<&ScrollPosition>,
    headers: Query<(&PanelHeader, &ComputedNode)>,
    mut nodes: Query<&mut Node>,
) {
    let count = panels.iter().count();
    for (root, view) in &views {
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
        // The body scrolls; the headings are moved by hand against it, which
        // is what keeps them over their columns without scrolling away.
        let scrolled = positions.get(view.body).map_or(0.0, |at| at.x);
        if let Ok(node) = nodes.get_mut(view.headings) {
            patch_node(node, |node| {
                node.left = Val::Px(-scrolled);
                node.width = Val::Px(view.width());
            });
        }
    }
}

/// How far down a frame its own chrome reaches: the header as it measured
/// last frame, or the buttons in the opposite corner if they hang lower.
///
/// Measured rather than assumed, because the header is as tall as the status
/// it is reporting — two lines for one dataset and three for another.
fn chrome_depth(headers: &Query<(&PanelHeader, &ComputedNode)>, panel: Entity) -> f32 {
    let header = headers
        .iter()
        .find(|(header, _)| header.panel == panel)
        .map_or(0.0, |(_, computed)| {
            computed.size().y * computed.inverse_scale_factor
        });
    depth_below(header)
}

/// Where a table starts, given how tall its frame's header measured.
fn depth_below(header: f32) -> f32 {
    CHROME_TOP_PX + header.max(BUTTON_PX) + CHROME_GAP_PX
}

/// Hide what a frame filled with rows has no use for.
///
/// A table is not layered over anything and nothing is layered over it: it
/// shares no coordinates with an image, so stacking one on the other would
/// put two unrelated things in one cell. The `...` button that opens the menu
/// offering layers goes, and the menu is shut if it was open when the frame
/// turned into a table. The popup itself is left to the menu, which shows it
/// whenever it is open.
pub fn hide_layer_menus(
    panels: Query<&ShowsSource>,
    tables: Query<(), With<SourceTable>>,
    buttons: Query<(Entity, &MenuButton)>,
    mut menus: Query<(&super::overlay::SourceMenu, &mut Menu)>,
    mut nodes: Query<&mut Node>,
) {
    for (button, opens) in &buttons {
        let Ok((menu, mut state)) = menus.get_mut(opens.menu) else {
            continue;
        };
        let showing_rows = panels
            .get(menu.panel())
            .is_ok_and(|shows| tables.contains(shows.0));
        if showing_rows && state.open {
            state.open = false;
        }
        set_display(&mut nodes, button, !showing_rows);
    }
}

/// Build the rows each table's frame is actually showing, and drop the rest.
pub fn fill_tables(
    mut commands: Commands,
    mut views: Query<&mut TableView>,
    panels: Query<&ShowsSource>,
    tables: Query<Ref<SourceTable>>,
    bodies: Query<(&ComputedNode, &ScrollPosition)>,
    palette: Res<Palette>,
) {
    for mut view in &mut views {
        let Ok(table) = panels.get(view.panel).and_then(|shows| tables.get(shows.0)) else {
            continue;
        };
        // The stripes carry the theme, so a theme change is a rebuild of
        // whatever is on screen; so is a page turning, since the rows in hand
        // are then different ones. There is never much of it either way.
        if palette.is_changed() || table.is_changed() {
            for row in view.live.drain().map(|(_, row)| row) {
                commands.entity(row).despawn();
            }
        }

        let wanted = visible_rows(&bodies, view.body, table.rows.len());
        view.live.retain(|row, entity| {
            if wanted.contains(row) {
                return true;
            }
            commands.entity(*entity).despawn();
            false
        });

        let content = view.content;
        for row in wanted {
            if view.live.contains_key(&row) {
                continue;
            }
            let entity = spawn_row(&mut commands, &view, &table, row, &palette);
            commands.entity(content).add_child(entity);
            view.live.insert(row, entity);
        }
    }
}

/// Which rows a body of this height, scrolled this far, is showing.
fn visible_rows(
    bodies: &Query<(&ComputedNode, &ScrollPosition)>,
    body: Entity,
    rows: usize,
) -> Range<usize> {
    let Ok((computed, at)) = bodies.get(body) else {
        return 0..0;
    };
    // Layout is measured in physical pixels; the scroll and the row height are
    // logical.
    let height = computed.size().y * computed.inverse_scale_factor;
    rows_in_view(at.y, height, rows)
}

/// The rows a body `height` tall, scrolled `y` from the top, is showing, with
/// the overscan either side of them.
fn rows_in_view(y: f32, height: f32, rows: usize) -> Range<usize> {
    if rows == 0 || height <= 0.0 {
        return 0..0;
    }
    let first = (y / ROW_PX).floor().max(0.0) as usize;
    let on_screen = (height / ROW_PX).ceil().max(0.0) as usize;
    let first = first.saturating_sub(OVERSCAN).min(rows);
    let last = first
        .saturating_add(on_screen + OVERSCAN * 2)
        .min(rows)
        .min(first + MAX_LIVE_ROWS);
    first..last
}

fn spawn_row(
    commands: &mut Commands,
    view: &TableView,
    table: &SourceTable,
    row: usize,
    palette: &Palette,
) -> Entity {
    // Every other row is tinted, which is what carries the eye across a wide
    // table. The divider color turns over with the theme, so this does too.
    let stripe = if row % 2 == 1 {
        palette.divider.with_alpha(0.18)
    } else {
        Color::NONE
    };
    let entity = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(row as f32 * ROW_PX),
                width: Val::Px(view.width()),
                height: Val::Px(ROW_PX),
                ..default()
            },
            BackgroundColor(stripe),
        ))
        .id();

    // Numbered by where the row falls in the whole table rather than on the
    // page, which is the number the readout under it counts to.
    let number = spawn_cell(
        commands,
        &view.edges,
        0,
        &(table.first + row + 1).to_string(),
        false,
        Cell::Number,
    );
    commands.entity(entity).add_child(number);
    for (drawn, &at) in view.visible.iter().enumerate() {
        let value = table.cell(row, at);
        let cell = spawn_cell(
            commands,
            &view.edges,
            drawn + 1,
            value,
            table.columns[at].numeric,
            Cell::Value,
        );
        if !value.trim().is_empty() {
            commands.entity(cell).insert(TableCell {
                panel: view.panel,
                row,
                at,
                drawn,
            });
        }
        commands.entity(entity).add_child(cell);
    }
    entity
}

/// The frames that show rows rather than a view onto space.
pub struct TablePlugin;

impl Plugin for TablePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_page_pressed)
            .add_observer(on_heading_pressed)
            .add_observer(on_grip_drag_start)
            .add_observer(on_grip_drag)
            .add_observer(on_grip_drag_end)
            .add_observer(on_copy_pressed)
            .add_systems(Update, select_pressed_tables.in_set(Stage::ControlsRead))
            .add_systems(Update, sync_tables.in_set(Stage::FrameChrome))
            .add_systems(
                Update,
                (
                    place_tables,
                    fill_tables,
                    place_copy_buttons,
                    hold_grip_cursor,
                    update_footers,
                    update_sort_marks,
                    hide_layer_menus,
                )
                    .chain()
                    .in_set(Stage::Chrome),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::table::TableColumn;

    fn table(rows: usize, columns: &[(&str, usize, bool)]) -> SourceTable {
        SourceTable {
            columns: columns
                .iter()
                .map(|(name, chars, numeric)| TableColumn {
                    name: (*name).to_string(),
                    chars: *chars,
                    numeric: *numeric,
                })
                .collect(),
            rows: (0..rows)
                .map(|row| columns.iter().map(|_| row.to_string()).collect())
                .collect(),
            first: 0,
        }
    }

    #[test]
    fn a_column_is_wide_enough_for_its_widest_value() {
        let table = table(3, &[("a", 10, false), ("b", 4, true)]);
        let edges = edges_of(&table, &[0, 1], 3, false, None);
        // The numbering gutter, then a column apiece, then the whole width.
        assert_eq!(edges.len(), 3 + 1);
        assert!(edges[0] < edges[1] && edges[1] < edges[2] && edges[2] < edges[3]);
        let first = edges[2] - edges[1];
        assert!(truncate_to_width("0123456789", first - PAD_PX * 2.0, size::SECONDARY).len() == 10);
    }

    #[test]
    fn a_hidden_column_is_left_out_and_the_rest_close_up() {
        let table = table(3, &[("a", 4, false), ("b", 10, false), ("c", 4, false)]);
        let hidden = HiddenColumns(["b".to_string()].into());
        let visible = visible_of(&table, Some(&hidden));
        assert_eq!(visible, [0, 2]);
        let edges = edges_of(&table, &visible, 3, false, None);
        assert_eq!(edges, edges_of(&table, &[0, 0], 3, false, None));
    }

    #[test]
    fn a_dragged_column_keeps_its_width_and_the_rest_move_over() {
        let table = table(3, &[("a", 4, false), ("b", 10, false)]);
        let fitted = edges_of(&table, &[0, 1], 3, false, None);
        let widths = ColumnWidths([("a".to_string(), 200.0)].into());
        let dragged = edges_of(&table, &[0, 1], 3, false, Some(&widths));
        assert!((dragged[2] - dragged[1] - 200.0).abs() < 1e-3);
        assert!(((dragged[3] - dragged[2]) - (fitted[3] - fitted[2])).abs() < 1e-3);
    }

    #[test]
    fn a_column_is_never_dragged_to_nothing() {
        assert_eq!(dragged_width(80.0, -500.0), MIN_COLUMN_PX);
        assert_eq!(dragged_width(80.0, 20.4), 100.0);
        let table = table(3, &[("a", 4, false)]);
        let widths = ColumnWidths([("a".to_string(), 1.0)].into());
        let edges = edges_of(&table, &[0], 3, false, Some(&widths));
        assert!((edges[2] - edges[1] - MIN_COLUMN_PX).abs() < 1e-3);
    }

    #[test]
    fn a_table_that_sorts_keeps_room_in_its_headings_for_the_mark() {
        let table = table(3, &[("a", 4, false)]);
        let plain = edges_of(&table, &[0], 3, false, None);
        let sorting = edges_of(&table, &[0], 3, true, None);
        let grown = (sorting[2] - sorting[1]) - (plain[2] - plain[1]);
        assert!((grown - SORT_MARK_PX).abs() < 1e-3);
    }

    #[test]
    fn the_gutter_is_wide_enough_for_the_last_row_number() {
        // Sized for the last row number in the whole table, not the page's:
        // a page of 100 rows can still be numbering row 10,901.
        let table = table(9, &[("a", 4, false)]);
        let few = edges_of(&table, &[0], 9, false, None);
        let many = edges_of(&table, &[0], 10_901, false, None);
        assert!(many[1] > few[1], "a five-digit number needs more room");
    }

    #[test]
    fn a_table_with_no_rows_still_lays_out_its_columns() {
        let edges = edges_of(&table(0, &[("a", 4, false)]), &[0], 0, false, None);
        assert_eq!(edges.len(), 3);
        assert!(edges[2] > edges[1]);
        assert_eq!(rows_in_view(0.0, 400.0, 0), 0..0);
    }

    #[test]
    fn only_the_rows_on_screen_are_built() {
        // A body ten rows tall at the top of a hundred-row table.
        let rows = rows_in_view(0.0, ROW_PX * 10.0, 100);
        assert_eq!(rows.start, 0);
        assert_eq!(rows.end, 10 + OVERSCAN * 2);
    }

    #[test]
    fn scrolling_moves_which_rows_are_built() {
        let rows = rows_in_view(ROW_PX * 50.0, ROW_PX * 10.0, 100);
        assert_eq!(rows.start, 50 - OVERSCAN);
        assert!(rows.contains(&50) && rows.contains(&59));
        // And nothing far from the view is kept.
        assert!(!rows.contains(&0) && !rows.contains(&99));
    }

    #[test]
    fn nothing_past_the_last_row_is_ever_built() {
        // Scrolled to the very end, and a body taller than the whole table.
        assert_eq!(rows_in_view(ROW_PX * 95.0, ROW_PX * 10.0, 100).end, 100);
        assert_eq!(rows_in_view(0.0, ROW_PX * 1000.0, 20), 0..20);
    }

    #[test]
    fn a_frame_made_enormous_still_builds_a_bounded_number_of_rows() {
        let rows = rows_in_view(0.0, ROW_PX * 100_000.0, 1_000_000);
        assert_eq!(rows.len(), MAX_LIVE_ROWS);
    }

    #[test]
    fn a_table_starts_below_the_chrome_drawn_over_its_frame() {
        // A header taller than the buttons pushes the table further down.
        assert!(depth_below(60.0) > depth_below(40.0));
        assert!(depth_below(60.0) > CHROME_TOP_PX + 60.0);
        // The buttons in the opposite corner are the floor, so a frame whose
        // header has not been measured yet still clears them.
        assert_eq!(depth_below(0.0), depth_below(BUTTON_PX));
        assert!(depth_below(0.0) > CHROME_TOP_PX + BUTTON_PX);
    }

    #[test]
    fn a_step_that_would_not_move_leads_nowhere() {
        // Which is what grays its button out, so the ends of a table are not
        // buttons that look pressable and do nothing.
        let first = TablePaging::new(100, Some(250));
        assert_eq!(PageStep::First.from(&first), None);
        assert_eq!(PageStep::Previous.from(&first), None);
        assert_eq!(PageStep::Next.from(&first), Some(1));
        assert_eq!(PageStep::Last.from(&first), Some(2));

        let last = TablePaging {
            page: 2,
            ..TablePaging::new(100, Some(250))
        };
        assert_eq!(PageStep::First.from(&last), Some(0));
        assert_eq!(PageStep::Previous.from(&last), Some(1));
        assert_eq!(PageStep::Next.from(&last), None);
        assert_eq!(PageStep::Last.from(&last), None);
    }

    #[test]
    fn nowhere_to_go_in_a_table_that_fits_on_one_page() {
        let one = TablePaging::new(100, Some(29));
        for step in PageStep::ALL {
            assert_eq!(step.from(&one), None, "{step:?}");
        }
    }

    #[test]
    fn a_source_still_counting_offers_no_way_forward() {
        // Rather than a page it cannot fill. It corrects itself the moment a
        // total lands.
        let counting = TablePaging::new(100, None);
        assert_eq!(PageStep::Next.from(&counting), None);
        assert_eq!(PageStep::Last.from(&counting), None);
    }

    #[test]
    fn the_readout_counts_rows_rather_than_pages() {
        let mut table = table(100, &[("a", 4, false)]);
        table.first = 200;
        let paging = TablePaging {
            page: 2,
            ..TablePaging::new(100, Some(10_901))
        };
        assert_eq!(readout(&table, &paging), "201 \u{2013} 300 of 10,901");
    }

    #[test]
    fn a_short_last_page_says_where_it_really_ends() {
        let mut table = table(50, &[("a", 4, false)]);
        table.first = 200;
        let paging = TablePaging {
            page: 2,
            ..TablePaging::new(100, Some(250))
        };
        assert_eq!(readout(&table, &paging), "201 \u{2013} 250 of 250");
    }

    #[test]
    fn a_table_with_no_rows_says_so_rather_than_counting_to_zero() {
        let table = table(0, &[("a", 4, false)]);
        assert_eq!(readout(&table, &TablePaging::new(100, Some(0))), "no rows");
    }

    #[test]
    fn a_total_nobody_knows_yet_is_left_off() {
        let table = table(100, &[("a", 4, false)]);
        assert_eq!(
            readout(&table, &TablePaging::new(100, None)),
            "1 \u{2013} 100"
        );
    }

    #[test]
    fn the_table_draws_under_the_chrome_that_has_to_stay_reachable() {
        // The header, its buttons and the capture button all sit at zero.
        const { assert!(TABLE_Z < 0) };
        const { assert!(TABLE_Z > super::super::layers::COMPOSITE_Z) };
    }
}
