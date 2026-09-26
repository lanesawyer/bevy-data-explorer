//! The rows on screen, and only those: which ones a scroll shows, and
//! building them and their cells as it does.

use super::*;

/// Rows built past the ones on screen, at each end, so that a scroll shows the
/// next row rather than a gap while it is built.
pub(super) const OVERSCAN: usize = 3;

/// A ceiling on the rows built at once, however tall a frame is made.
pub(super) const MAX_LIVE_ROWS: usize = 400;

/// One cell, sized to its column and holding as much of `value` as fits.
pub(super) fn spawn_cell(
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
pub(super) fn visible_rows(
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
pub(super) fn rows_in_view(y: f32, height: f32, rows: usize) -> Range<usize> {
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

pub(super) fn spawn_row(
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
