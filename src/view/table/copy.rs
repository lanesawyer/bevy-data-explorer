//! The button that follows the pointer over a table's cells and copies the
//! whole of the one it is on.

use super::*;

/// How big the button that copies a cell is, square. Under a row's height, so
/// it sits inside the cell rather than over the rows either side.
pub(super) const COPY_PX: f32 = 20.0;

/// A value's cell, which the copy button comes to when it is hovered.
///
/// Only cells holding something are marked: there is nothing to copy out of
/// an empty one.
#[derive(Component)]
pub struct TableCell {
    pub(super) panel: Entity,
    /// The row on the page, and the column as a place in the source's own.
    pub(super) row: usize,
    pub(super) at: usize,
    /// The column as a place among those drawn.
    pub(super) drawn: usize,
}

/// The button that copies the hovered cell of a frame's table.
#[derive(Component)]
pub struct CopyCellButton {
    pub(super) panel: Entity,
    /// The cell it sits in, as a row and a column in the source's own.
    pub(super) target: Option<(usize, usize)>,
    /// The cell it last copied, which it shows a tick for until it moves on.
    pub(super) copied: Option<(usize, usize)>,
    pub(super) icon: Entity,
}

pub(super) fn spawn_copy_button(commands: &mut Commands, panel: Entity) -> Entity {
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
