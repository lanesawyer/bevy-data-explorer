//! Dragging a column wider or narrower by the grip at its heading's edge.

use super::*;

/// How wide the strip at a heading's right edge that resizes its column is.
pub(super) const GRIP_PX: f32 = 12.0;

/// The narrowest a column can be dragged: room for an ellipsis and the copy
/// button, not for nothing.
pub(super) const MIN_COLUMN_PX: f32 = 32.0;

/// The strip at a heading's right edge that drags its column wider or
/// narrower.
#[derive(Component)]
pub struct ColumnGrip {
    pub(super) panel: Entity,
    pub(super) column: String,
    /// The column as a place among those drawn.
    pub(super) drawn: usize,
    /// How wide the column was when the drag began, while one is going on.
    pub(super) from: Option<f32>,
}

/// How wide a column dragged `by` from `from` becomes: never narrower than
/// [`MIN_COLUMN_PX`], and in whole pixels, which is what a bookmark keeps.
pub(super) fn dragged_width(from: f32, by: f32) -> f32 {
    (from + by).round().max(MIN_COLUMN_PX)
}

/// Move a table's headings to new column widths, and have its rows rebuilt at
/// them.
pub(super) fn resize_table(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_is_never_dragged_to_nothing() {
        assert_eq!(dragged_width(80.0, -500.0), MIN_COLUMN_PX);
        assert_eq!(dragged_width(80.0, 20.4), 100.0);
        let table = table(3, &[("a", 4, false)]);
        let widths = ColumnWidths([("a".to_string(), 1.0)].into());
        let edges = edges_of(&table, &[0], 3, false, Some(&widths));
        assert!((edges[2] - edges[1] - MIN_COLUMN_PX).abs() < 1e-3);
    }
}
