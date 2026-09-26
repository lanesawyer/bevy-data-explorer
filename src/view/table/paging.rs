//! The strip along a table's foot: the buttons that turn its pages, and the
//! readout saying which rows are on screen.

use super::*;

/// How tall the strip of paging buttons along the bottom is.
pub(super) const FOOTER_PX: f32 = 32.0;

/// Which way a button moves through the pages.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageStep {
    First,
    Previous,
    Next,
    Last,
}

impl PageStep {
    pub(super) const ALL: [PageStep; 4] = [
        PageStep::First,
        PageStep::Previous,
        PageStep::Next,
        PageStep::Last,
    ];

    pub(super) fn icon(self) -> Icon {
        match self {
            PageStep::First => Icon::ChevronFirst,
            PageStep::Previous => Icon::ChevronLeft,
            PageStep::Next => Icon::ChevronRight,
            PageStep::Last => Icon::ChevronLast,
        }
    }

    /// Where this step lands from `paging`, or nothing if it would not move.
    pub(super) fn from(self, paging: &TablePaging) -> Option<usize> {
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

pub(super) fn page_button(commands: &mut Commands, panel: Entity, step: PageStep) -> Entity {
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
pub(super) fn readout(table: &SourceTable, paging: &TablePaging) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
