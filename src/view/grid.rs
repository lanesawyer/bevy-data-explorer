//! The geometry of the grid: how many cells, which cell is where, and
//! which one the pointer is in.
//!
//! Everything here is a pure function of a count and a rectangle, which is
//! what lets the layout be tested without a window.

use bevy::camera::ClearColorConfig;
use bevy::prelude::*;

/// The grid never exceeds four columns by two rows.
pub const MAX_COLUMNS: usize = 4;
pub const MAX_ROWS: usize = 2;
pub const MAX_PANELS: usize = MAX_COLUMNS * MAX_ROWS;

/// Sources one frame can stack, counting the one it opened onto.
pub const MAX_LAYERS: usize = 8;

/// The camera order of layer `depth` in cell `index`, where depth 0 is the
/// frame's own camera.
///
/// Layers are drawn by their order rather than by depth within one camera, so
/// a source never has to cooperate to be drawn over another. Each cell gets a
/// band of orders wide enough for every layer it can hold, which keeps a cell's
/// layers together and every cell after the one before it.
pub fn camera_order(index: usize, depth: usize) -> isize {
    (index * MAX_LAYERS + depth.min(MAX_LAYERS - 1)) as isize
}

/// After every layer of every frame the grid can hold, so the UI is never
/// cleared or drawn over.
pub const UI_CAMERA_ORDER: isize = (MAX_PANELS * MAX_LAYERS) as isize;

/// Columns and rows for `count` panels.
///
/// Panels stay in one row until there are more than two, after which the grid
/// goes two deep and grows sideways. That keeps cells closer to square than a
/// single row of eight would, while never exceeding the 4x2 maximum.
pub fn grid_for(count: usize) -> (usize, usize) {
    let count = count.clamp(1, MAX_PANELS);
    let rows = if count <= 2 { 1 } else { MAX_ROWS };
    (count.div_ceil(rows), rows)
}

/// Cells for the given frames, keeping their relative order and closing any
/// gaps or duplicates.
pub(super) fn assign_cells(mut frames: Vec<(usize, Entity)>) -> Vec<(usize, Entity)> {
    // Sorting by index then entity keeps the result stable when two frames
    // claim the same cell, which happens when one is opened in the same breath
    // as another is closed.
    frames.sort_unstable();
    frames
        .into_iter()
        .enumerate()
        .map(|(position, (_, entity))| (position, entity))
        .collect()
}

/// Only the first camera clears the window; a later clear would wipe the panels
/// already drawn.
pub(super) fn clear_color_for(index: usize, background: Color) -> ClearColorConfig {
    if index == 0 {
        ClearColorConfig::Custom(background)
    } else {
        ClearColorConfig::None
    }
}

/// Whether a pointer position, measured from the grid's origin, is over the
/// grid at all.
///
/// Both edges matter. Testing only for negatives caught the sidebar, which
/// docks to the left, but let a click in the right-hand inspector through: it
/// lands past the grid's right edge, where the cell lookup clamps it to the
/// last frame and drags that instead.
pub(super) fn within_frames(local: Vec2, size: Vec2) -> bool {
    local.x >= 0.0 && local.y >= 0.0 && local.x < size.x && local.y < size.y
}

/// A drag in progress, and the panel it began in.
#[derive(Clone, Copy)]
pub struct Drag {
    pub(super) panel: usize,
    pub(super) last: Vec2,
}

/// Which panel input applies to.
///
/// A drag keeps hold of the panel it started in even after the pointer crosses
/// into another, so dragging past a panel edge carries on panning the view the
/// gesture began in rather than grabbing its neighbour mid-stroke.
pub(super) fn active_panel(drag: Option<Drag>, cursor: Vec2, window: Vec2, count: usize) -> usize {
    match drag {
        Some(drag) => drag.panel.min(count.saturating_sub(1)),
        None => panel_under_cursor(cursor, window, count),
    }
}

/// Which panel the cursor is over.
pub(super) fn panel_under_cursor(cursor: Vec2, window: Vec2, count: usize) -> usize {
    let (columns, rows) = grid_for(count);
    let col = ((cursor.x / (window.x / columns as f32).max(1.0)) as usize).min(columns - 1);
    let row = ((cursor.y / (window.y / rows as f32).max(1.0)) as usize).min(rows - 1);
    (row * columns + col).min(count.saturating_sub(1))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(in crate::view) fn panels(count: usize) -> Vec<(Entity, usize)> {
        (0..count)
            .map(|i| (Entity::from_raw_u32(i as u32 + 1).unwrap(), i))
            .collect()
    }

    #[test]
    fn the_grid_stays_one_row_until_it_has_to_split() {
        assert_eq!(grid_for(1), (1, 1));
        assert_eq!(grid_for(2), (2, 1));
    }

    #[test]
    fn the_grid_grows_sideways_two_deep() {
        assert_eq!(grid_for(3), (2, 2));
        assert_eq!(grid_for(4), (2, 2));
        assert_eq!(grid_for(5), (3, 2));
        assert_eq!(grid_for(6), (3, 2));
        assert_eq!(grid_for(7), (4, 2));
        assert_eq!(grid_for(8), (4, 2));
    }

    #[test]
    fn the_grid_never_exceeds_four_by_two() {
        for count in 1..=MAX_PANELS {
            let (columns, rows) = grid_for(count);
            assert!(columns <= MAX_COLUMNS && rows <= MAX_ROWS);
            assert!(columns * rows >= count, "{count} panels would not fit");
        }
    }

    #[test]
    fn every_cell_maps_back_to_its_panel() {
        // A cursor in the middle of each cell must select that cell's panel.
        let window = Vec2::new(800.0, 600.0);
        for count in 1..=MAX_PANELS {
            let (columns, rows) = grid_for(count);
            let cell = Vec2::new(window.x / columns as f32, window.y / rows as f32);
            for index in 0..count {
                let (col, row) = (index % columns, index / columns);
                let center = Vec2::new(cell.x * (col as f32 + 0.5), cell.y * (row as f32 + 0.5));
                assert_eq!(
                    panel_under_cursor(center, window, count),
                    index,
                    "{count} panels, cell {index}"
                );
            }
        }
    }

    #[test]
    fn the_cursor_selects_the_panel_it_is_over() {
        let window = Vec2::new(1000.0, 600.0);
        assert_eq!(panel_under_cursor(Vec2::new(10.0, 5.0), window, 2), 0);
        assert_eq!(panel_under_cursor(Vec2::new(499.0, 5.0), window, 2), 0);
        assert_eq!(panel_under_cursor(Vec2::new(501.0, 5.0), window, 2), 1);
        // The far corner must not select a panel that does not exist.
        assert_eq!(panel_under_cursor(Vec2::new(1000.0, 600.0), window, 2), 1);
    }

    #[test]
    fn a_partly_filled_grid_never_selects_an_empty_cell() {
        // Three panels leave the bottom-right cell empty; a cursor there has to
        // fall back to a panel that exists.
        let window = Vec2::new(800.0, 600.0);
        assert_eq!(panel_under_cursor(Vec2::new(700.0, 500.0), window, 3), 2);
    }

    /// The cells the survivors are given after some panels are despawned.
    fn after_closing(count: usize, closed: &[usize]) -> Vec<Entity> {
        let all = panels(count);
        let left: Vec<(usize, Entity)> = all
            .iter()
            .filter(|(_, index)| !closed.contains(index))
            .map(|(entity, index)| (*index, *entity))
            .collect();
        assign_cells(left)
            .into_iter()
            .map(|(_, entity)| entity)
            .collect()
    }

    #[test]
    fn closing_a_panel_closes_the_gap_it_leaves() {
        let all = panels(4);
        // Close the second of four.
        assert_eq!(
            after_closing(4, &[1]),
            vec![all[0].0, all[2].0, all[3].0],
            "the survivors should take cells 0, 1 and 2"
        );
    }

    #[test]
    fn the_surviving_panels_keep_their_relative_order() {
        let all = panels(5);
        assert_eq!(
            after_closing(5, &[0]),
            vec![all[1].0, all[2].0, all[3].0, all[4].0]
        );
    }

    #[test]
    fn closing_everything_leaves_no_cells() {
        // Allowed since the window has an empty state to fall back to: the
        // welcome screen shows itself exactly when no frame is left.
        assert!(assign_cells(Vec::new()).is_empty());
    }

    #[test]
    fn renumbering_is_independent_of_query_order() {
        // ECS iteration order is not the cell order, so the result has to come
        // from the recorded indices rather than from how panels are visited.
        let all = panels(4);
        let ordered: Vec<(usize, Entity)> = all
            .iter()
            .map(|(entity, index)| (*index, *entity))
            .collect();
        let shuffled = vec![ordered[2], ordered[0], ordered[3], ordered[1]];
        assert_eq!(assign_cells(shuffled), assign_cells(ordered));
    }

    #[test]
    fn cells_end_up_unique_and_contiguous() {
        let e = |n: u32| Entity::from_raw_u32(n).unwrap();
        // Gaps from closing, and a duplicate from opening while closing.
        let frames = vec![(0, e(1)), (2, e(2)), (2, e(3)), (7, e(4))];
        let cells = assign_cells(frames);

        let positions: Vec<usize> = cells.iter().map(|(p, _)| *p).collect();
        assert_eq!(positions, vec![0, 1, 2, 3]);

        let entities: std::collections::HashSet<Entity> = cells.iter().map(|(_, e)| *e).collect();
        assert_eq!(entities.len(), 4, "a frame was dropped or duplicated");
    }

    #[test]
    fn exactly_one_frame_clears_the_window() {
        // With none clearing, each frame paints over the last instead of
        // replacing it; with several, later ones wipe what came before.
        let e = |n: u32| Entity::from_raw_u32(n).unwrap();
        let cells = assign_cells(vec![(3, e(1)), (5, e(2)), (9, e(3))]);
        let clearing = cells
            .iter()
            .filter(|(position, _)| {
                matches!(
                    clear_color_for(*position, Color::WHITE),
                    ClearColorConfig::Custom(_)
                )
            })
            .count();
        assert_eq!(clearing, 1);
    }

    #[test]
    fn closing_the_first_frame_hands_clearing_to_another() {
        let e = |n: u32| Entity::from_raw_u32(n).unwrap();
        // Frame 0 is gone; whatever is left must take over clearing.
        let cells = assign_cells(vec![(1, e(2)), (2, e(3))]);
        assert_eq!(cells[0].0, 0);
        assert!(matches!(
            clear_color_for(cells[0].0, Color::WHITE),
            ClearColorConfig::Custom(_)
        ));
    }

    #[test]
    fn only_the_first_cell_clears_the_window() {
        // A second clear would wipe the panels already drawn beneath it.
        assert!(matches!(
            clear_color_for(0, Color::WHITE),
            ClearColorConfig::Custom(_)
        ));
        for index in 1..MAX_PANELS {
            assert!(matches!(
                clear_color_for(index, Color::WHITE),
                ClearColorConfig::None
            ));
        }
    }

    #[test]
    fn input_outside_the_grid_is_ignored_on_every_side() {
        let size = Vec2::new(1000.0, 800.0);
        assert!(within_frames(Vec2::new(500.0, 400.0), size));
        assert!(within_frames(Vec2::ZERO, size));
        // Left of the grid: the sidebar.
        assert!(!within_frames(Vec2::new(-1.0, 400.0), size));
        // Right of it: the inspector, which used to steal the last frame.
        assert!(!within_frames(Vec2::new(1000.0, 400.0), size));
        assert!(!within_frames(Vec2::new(1200.0, 400.0), size));
        assert!(!within_frames(Vec2::new(500.0, 800.0), size));
    }

    #[test]
    fn a_drag_keeps_the_panel_it_started_in() {
        let window = Vec2::new(800.0, 600.0);
        // Started in panel 0, pointer has since crossed into panel 1.
        let drag = Some(Drag {
            panel: 0,
            last: Vec2::new(100.0, 100.0),
        });
        let crossed = Vec2::new(700.0, 100.0);
        assert_eq!(panel_under_cursor(crossed, window, 2), 1);
        assert_eq!(active_panel(drag, crossed, window, 2), 0);
    }

    #[test]
    fn without_a_drag_input_follows_the_pointer() {
        let window = Vec2::new(800.0, 600.0);
        let cursor = Vec2::new(700.0, 100.0);
        assert_eq!(active_panel(None, cursor, window, 2), 1);
    }

    #[test]
    fn a_drag_survives_the_pointer_leaving_the_grid() {
        // Dragging well past the window edge still pans the original panel.
        let window = Vec2::new(800.0, 600.0);
        let drag = Some(Drag {
            panel: 1,
            last: Vec2::new(500.0, 100.0),
        });
        for cursor in [
            Vec2::new(-200.0, -50.0),
            Vec2::new(5000.0, 5000.0),
            Vec2::new(10.0, 590.0),
        ] {
            assert_eq!(active_panel(drag, cursor, window, 4), 1);
        }
    }

    #[test]
    fn a_cells_layers_draw_after_it_and_before_the_next_cell() {
        for index in 0..MAX_PANELS {
            let orders: Vec<isize> = (0..MAX_LAYERS).map(|d| camera_order(index, d)).collect();
            assert!(orders.windows(2).all(|pair| pair[0] < pair[1]));
            if index + 1 < MAX_PANELS {
                assert!(orders[MAX_LAYERS - 1] < camera_order(index + 1, 0));
            }
        }
        assert!(camera_order(MAX_PANELS - 1, MAX_LAYERS - 1) < UI_CAMERA_ORDER);
    }

    #[test]
    fn a_single_panel_takes_every_cursor_position() {
        let window = Vec2::new(1000.0, 600.0);
        assert_eq!(panel_under_cursor(Vec2::new(999.0, 599.0), window, 1), 0);
    }
}
