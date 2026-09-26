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

mod copy;
mod headings;
mod layout;
mod paging;
mod resize;
mod rows;

use copy::*;
use headings::*;
use layout::*;
use paging::*;
use resize::*;
use rows::*;

/// How tall one row is.
const ROW_PX: f32 = 24.0;

/// How tall the header strip is. Taller than a row, so the header reads as a
/// heading rather than as the first record.
const HEADER_PX: f32 = 30.0;

/// Clear space at each side of a cell's text.
const PAD_PX: f32 = space::CONTROL_INSET;

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
    fn the_table_draws_under_the_chrome_that_has_to_stay_reachable() {
        // The header, its buttons and the capture button all sit at zero.
        const { assert!(TABLE_Z < 0) };
        const { assert!(TABLE_Z > super::super::layers::COMPOSITE_Z) };
    }
}
