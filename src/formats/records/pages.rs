//! Turning the page: asking the registry for the one a frame has moved to,
//! in the order and under the filters it asks for.

use super::*;

/// Ask for the page a frame has turned to, and take it when it lands.
///
/// A search or a tick that changes what is asked puts the table back on its
/// first page, which the sidebar does as it writes them.
pub(super) fn serve_pages(
    mut sources: Query<(
        &mut RecordPages,
        &mut TablePaging,
        &mut SourceTable,
        &mut SourceBusy,
        &TableFilters,
        &TableSearch,
        &TableSort,
    )>,
) {
    for (mut pages, mut paging, mut rows, mut busy, filters, search, sort) in &mut sources {
        let wanted_values = asked_of(filters, search);
        let wanted_sort = sort.0.clone();
        if let Some((page, fetch)) = pages.fetching.as_mut()
            && let Some(answer) = fetch.take()
        {
            let page = *page;
            pages.fetching = None;
            match answer {
                Ok(landed) => {
                    // A narrowed table is a different table: however many
                    // rows it now has is what the paging counts to.
                    paging.total = Some(landed.total);
                    take_page(pages.kind, &mut rows, page * paging.size, &landed);
                    // A page restored from a bookmark can be past the end of
                    // the table its filters narrow to.
                    let last = paging.clamped(paging.page);
                    if paging.page != last {
                        paging.page = last;
                    }
                }
                Err(e) => {
                    // The rows on screen are left alone, and the frame put
                    // back on the page it is showing, or it would ask for the
                    // missing one again every frame.
                    warn!("reading BKP Registry {}: {e}", pages.kind.root());
                    let showing = rows.first / paging.size.max(1);
                    if paging.page != showing {
                        paging.page = showing;
                    }
                }
            }
        }

        let narrowed = pages.applied != wanted_values || pages.sorted != wanted_sort;
        let wanted = paging.first();
        let asking = pages.fetching.as_ref().map(|(page, _)| page * paging.size);
        if (narrowed || rows.first != wanted) && asking != Some(wanted) {
            let (endpoint, kind, size) = (pages.endpoint.clone(), pages.kind, paging.size);
            let (values, order) = (wanted_values.clone(), wanted_sort.clone());
            pages.applied = wanted_values;
            pages.sorted = wanted_sort;
            pages.fetching = Some((
                paging.page,
                fetching(
                    async move { ask_page(&endpoint, kind, wanted, size, &values, &order).await },
                ),
            ));
        }
        busy.set_if_neq(SourceBusy(pages.fetching.is_some()));
    }
}

/// Put a landed page into the rows a frame draws.
fn take_page(kind: Kind, rows: &mut SourceTable, first: usize, landed: &Landed) {
    rows.rows = kind.rows(&landed.nodes);
    rows.first = first;
    // A column is as wide as the widest value seen on any page so far, so
    // turning a page never shuffles the table sideways.
    for (index, column) in rows.columns.iter_mut().enumerate() {
        let widest = rows
            .rows
            .iter()
            .filter_map(|row| row.get(index))
            .map(|value| value.trim().chars().count())
            .max()
            .unwrap_or_default();
        column.chars = column.chars.max(widest).min(MAX_CHARS);
    }
}
