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

use std::collections::HashMap;
use std::ops::Range;

use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, ScrollPosition};
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_ui_widgets::Activate;

use crate::app::schedule::Stage;
use crate::app::theme::{Palette, token};
use crate::source::table::{SourceTable, TablePaging};
use crate::source::{ShowsSource, grouped};
use crate::widgets::{
    BlocksFrameInput, Icon, ScrollBoth, button_icon, size, text, text_dim, truncate_to_width,
    width_of,
};

use super::chrome::BUTTON_PX;
use super::overlay::PanelHeader;
use super::{FrameArea, Panel};

/// How tall one row is.
const ROW_PX: f32 = 24.0;

/// How tall the header strip is. Taller than a row, so the header reads as a
/// heading rather than as the first record.
const HEADER_PX: f32 = 30.0;

/// Clear space at each side of a cell's text.
const PAD_PX: f32 = 10.0;

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
/// buttons sitting on its first rows. It starts under them instead.
const CHROME_GAP_PX: f32 = 6.0;

/// Where the frame's chrome starts, which is where [`super::overlay`] puts the
/// header and [`super::camera`] puts the buttons.
const CHROME_TOP_PX: f32 = 8.0;

/// How tall the strip of paging buttons along the bottom is.
const FOOTER_PX: f32 = 32.0;

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
    /// The scrolling area, and the full-height node inside it the rows are
    /// placed in.
    body: Entity,
    content: Entity,
    /// The strip of paging buttons along the bottom, and the line in it that
    /// says which rows are on screen.
    footer: Entity,
    readout: Entity,
    /// Where each column starts, the numbering gutter first, with the whole
    /// width on the end.
    edges: Vec<f32>,
    /// The rows on screen, by their place on the page.
    live: HashMap<usize, Entity>,
}

impl TableView {
    fn width(&self) -> f32 {
        self.edges.last().copied().unwrap_or_default()
    }
}

/// How a cell is set: a heading, a value, or the number in the gutter.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Heading,
    Value,
    Number,
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

/// Where each column starts, the numbering gutter first, with the full width
/// last.
///
/// Sized from the characters a format already counted rather than from laying
/// the text out, which would mean reacting to the measurement a frame later.
/// The gutter is sized for the *last* row number rather than the page's, since
/// the numbers carry on across pages.
fn edges_of(table: &SourceTable, total: usize) -> Vec<f32> {
    let gutter = width_of(total.max(1).to_string().len(), size::SMALL) + PAD_PX * 2.0;
    let mut edges = vec![0.0, gutter];
    for column in &table.columns {
        let last = edges.last().copied().unwrap_or_default();
        edges.push(last + width_of(column.chars, size::SECONDARY) + PAD_PX * 2.0);
    }
    edges
}

/// Give every frame showing a table one, and take it away from every frame
/// that has stopped showing one.
pub fn sync_tables(
    mut commands: Commands,
    panels: Query<(Entity, &ShowsSource), With<Panel>>,
    tables: Query<(&SourceTable, &TablePaging)>,
    views: Query<(Entity, &TableView)>,
) {
    for (entity, view) in &views {
        // Rebuilt rather than redrawn when the columns move: a frame pointed
        // at another table, or one whose source grew a column it had not seen
        // on the first page, is laid out afresh.
        let stale = match panels.get(view.panel) {
            Ok((_, shows)) => {
                shows.0 != view.source
                    || tables.get(shows.0).is_ok_and(|(table, paging)| {
                        edges_of(table, paging.total.unwrap_or_default()) != view.edges
                    })
            }
            Err(_) => true,
        };
        if stale {
            commands.entity(entity).despawn();
        }
    }

    for (panel, shows) in &panels {
        let Ok((table, paging)) = tables.get(shows.0) else {
            continue;
        };
        if views
            .iter()
            .any(|(_, view)| view.panel == panel && view.source == shows.0)
        {
            continue;
        }
        spawn_table(&mut commands, panel, shows.0, table, paging);
    }
}

fn spawn_table(
    commands: &mut Commands,
    panel: Entity,
    source: Entity,
    table: &SourceTable,
    paging: &TablePaging,
) {
    let edges = edges_of(table, paging.total.unwrap_or_default());
    let width = edges.last().copied().unwrap_or_default();

    let headings = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            height: Val::Percent(100.0),
            ..default()
        })
        .id();
    for (column, heading) in table.columns.iter().enumerate() {
        let cell = spawn_cell(
            commands,
            &edges,
            column + 1,
            &heading.name,
            heading.numeric,
            Cell::Heading,
        );
        commands.entity(headings).add_child(cell);
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
                column_gap: Val::Px(6.0),
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
                footer,
                readout,
                body,
                content,
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
        if let Ok(mut node) = nodes.get_mut(view.footer) {
            let wanted = if several {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != wanted {
                node.display = wanted;
            }
        }
        if !several {
            continue;
        }
        if let Ok(mut text) = texts.get_mut(view.readout) {
            let next = readout(table, paging);
            if text.0 != next {
                text.0 = next;
            }
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
        _ => commands.spawn_scene(text_dim(content, font)).id(),
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
        if let Ok(mut node) = nodes.get_mut(root) {
            node.left = Val::Px(cell.min.x);
            node.top = Val::Px(cell.min.y + inset);
            node.width = Val::Px(cell.width());
            node.height = Val::Px((cell.height() - inset).max(0.0));
        }
        // The body scrolls; the headings are moved by hand against it, which
        // is what keeps them over their columns without scrolling away.
        let scrolled = positions.get(view.body).map_or(0.0, |at| at.x);
        if let Ok(mut node) = nodes.get_mut(view.headings) {
            node.left = Val::Px(-scrolled);
            node.width = Val::Px(view.width());
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
/// put two unrelated things in one cell. The menu that offers it goes.
pub fn hide_layer_menus(
    panels: Query<&ShowsSource>,
    tables: Query<(), With<SourceTable>>,
    mut menus: Query<(&super::overlay::SourceMenu, &mut Node)>,
) {
    for (menu, mut node) in &mut menus {
        let showing_rows = panels
            .get(menu.panel())
            .is_ok_and(|shows| tables.contains(shows.0));
        let wanted = if showing_rows {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != wanted {
            node.display = wanted;
        }
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
    for (column, heading) in table.columns.iter().enumerate() {
        let cell = spawn_cell(
            commands,
            &view.edges,
            column + 1,
            table.cell(row, column),
            heading.numeric,
            Cell::Value,
        );
        commands.entity(entity).add_child(cell);
    }
    entity
}

/// The frames that show rows rather than a view onto space.
pub struct TablePlugin;

impl Plugin for TablePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_page_pressed)
            .add_systems(Update, sync_tables.in_set(Stage::FrameChrome))
            .add_systems(
                Update,
                (place_tables, fill_tables, update_footers, hide_layer_menus)
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
        let edges = edges_of(&table(3, &[("a", 10, false), ("b", 4, true)]), 3);
        // The numbering gutter, then a column apiece, then the whole width.
        assert_eq!(edges.len(), 3 + 1);
        assert!(edges[0] < edges[1] && edges[1] < edges[2] && edges[2] < edges[3]);
        let first = edges[2] - edges[1];
        assert!(truncate_to_width("0123456789", first - PAD_PX * 2.0, size::SECONDARY).len() == 10);
    }

    #[test]
    fn the_gutter_is_wide_enough_for_the_last_row_number() {
        // Sized for the last row number in the whole table, not the page's:
        // a page of 100 rows can still be numbering row 10,901.
        let few = edges_of(&table(9, &[("a", 4, false)]), 9);
        let many = edges_of(&table(9, &[("a", 4, false)]), 10_901);
        assert!(many[1] > few[1], "a five-digit number needs more room");
    }

    #[test]
    fn a_table_with_no_rows_still_lays_out_its_columns() {
        let edges = edges_of(&table(0, &[("a", 4, false)]), 0);
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
        // Which is what greys its button out, so the ends of a table are not
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
