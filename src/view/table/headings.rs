//! The headings over a table's columns, which sort it when pressed and say
//! how it is sorted.

use super::*;

/// Room a heading keeps beside its name for the arrow saying which way the
/// column is sorted, and the number saying where it falls among several.
pub(super) const SORT_MARK_PX: f32 = 24.0;

/// A heading that sorts its table when pressed.
#[derive(Component)]
pub struct TableHeading {
    pub(super) panel: Entity,
    pub(super) column: String,
}

/// Beside a heading, what says how its column is sorted: the arrow, or with
/// `rank`, where it falls among several columns sorted at once.
#[derive(Component)]
pub struct SortMark {
    pub(super) panel: Entity,
    pub(super) column: String,
    pub(super) rank: bool,
}

/// A heading: the column's name, the grip at its right edge that resizes it,
/// and beside the name the mark saying how the column is sorted when `sorts`.
/// Returns the heading and the name in it.
pub(super) fn spawn_heading(
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
