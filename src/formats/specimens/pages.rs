//! Turning the page: asking the platform for the one a frame has moved to,
//! and putting it into the rows the frame draws.

use super::*;

/// The project a frame's specimen table came from, so a page it has not got
/// can be asked for.
///
/// A component of the source entity rather than a resource, so two frames on
/// two projects each turn their own pages.
#[derive(Component)]
pub struct SpecimenPages {
    pub(super) endpoint: String,
    pub(super) project: String,
    /// The columns the first page settled. Every page after fills these, so
    /// the table does not relay itself out under the pointer.
    pub(super) plan: Plan,
    /// The page being fetched, and the fetch, while one is in flight.
    pub(super) fetching: Option<(usize, Fetching<Result<Data, String>>)>,
    /// What the columns hold, asked for once.
    pub(super) offering: Option<Fetching<Result<Vec<TableFilter>, String>>>,
    /// Whether the columns have been asked about, so they are asked once
    /// whether or not anything came back.
    pub(super) offered: bool,
    /// The values in force on the rows now on screen, so a tick that changes
    /// nothing does not refetch and one that does is noticed.
    pub(super) applied: Vec<TableFilterTerm>,
    /// The order the rows now on screen were asked for in, for the same
    /// reason.
    pub(super) sorted: Vec<SortKey>,
    /// The column whose distribution is being asked about, and the ask.
    /// What it was asked under is kept, since that is what its histogram
    /// has been counted under.
    pub(super) spanning: Option<(
        usize,
        Vec<TableFilterTerm>,
        Fetching<Result<NumericRange, String>>,
    )>,
    /// What each column's counts were last counted under, by id. A column
    /// not here was counted under nothing, as every column first is.
    pub(super) counted: HashMap<String, Vec<TableFilterTerm>>,
    /// A recount in flight, and what each column in it was asked under.
    pub(super) counting: Option<(
        Vec<(String, Vec<TableFilterTerm>)>,
        Fetching<Result<Vec<Recounted>, String>>,
    )>,
}

impl SpecimenPages {
    pub fn new(endpoint: String, project: String, plan: Plan) -> Self {
        SpecimenPages {
            endpoint,
            project,
            plan,
            fetching: None,
            offering: None,
            offered: false,
            applied: Vec::new(),
            sorted: Vec::new(),
            spanning: None,
            counted: HashMap::new(),
            counting: None,
        }
    }
}

/// Ask for the page a frame has turned to, and take it when it lands.
///
/// One system rather than two because the two halves share the plan: what
/// comes back is only rows once it is read against the columns already on
/// screen.
pub(super) fn serve_pages(
    mut sources: Query<(
        &mut SpecimenPages,
        &mut TablePaging,
        &mut SourceTable,
        &mut SourceBusy,
        Option<&TableFilters>,
        Option<&TableSort>,
    )>,
) {
    for (mut pages, mut paging, mut rows, mut busy, filters, sort) in &mut sources {
        let wanted_values = filters.map(TableFilters::chosen).unwrap_or_default();
        let wanted_sort = sort.map(|sort| sort.0.clone()).unwrap_or_default();
        if let Some((page, fetch)) = pages.fetching.as_mut()
            && let Some(answer) = fetch.take()
        {
            let page = *page;
            pages.fetching = None;
            match answer {
                Ok(data) => {
                    // A narrowed table is a different table: however many rows
                    // it now has is what the paging counts to.
                    if let Some(total) = data.counted() {
                        paging.total = Some(total);
                    } else if data.aio_specimen.is_empty() {
                        paging.total = Some(0);
                    }
                    take_page(&mut pages, &mut rows, page * paging.size, &data);
                    // A page restored from a bookmark can be past the end of
                    // the table its filters narrow to.
                    let last = paging.clamped(paging.page);
                    if paging.total.is_some() && paging.page != last {
                        paging.page = last;
                    }
                }
                Err(e) => {
                    // The rows on screen are left alone — a page that could
                    // not be read is better than an empty table — and the
                    // frame is put back on the page it is actually showing.
                    // Without that it would ask for the missing one again
                    // every frame, forever.
                    warn!("reading specimens: {e}");
                    let showing = rows.first / paging.size.max(1);
                    if paging.page != showing {
                        paging.page = showing;
                    }
                }
            }
        }

        // Whatever narrows or sorts the table puts it back on its first page,
        // not this: a bookmark restores its filters and its page together.
        let narrowed = pages.applied != wanted_values || pages.sorted != wanted_sort;

        let wanted = paging.first();
        let asking = pages.fetching.as_ref().map(|(page, _)| page * paging.size);
        if (narrowed || rows.first != wanted) && asking != Some(wanted) {
            let (endpoint, project) = (pages.endpoint.clone(), pages.project.clone());
            let values = wanted_values.clone();
            let order = pages.plan.sort(&wanted_sort);
            pages.applied = wanted_values;
            pages.sorted = wanted_sort;
            pages.fetching = Some((
                paging.page,
                fetching(async move { ask(&endpoint, &project, wanted, &values, order).await }),
            ));
        }
        busy.set_if_neq(SourceBusy(pages.fetching.is_some()));
    }
}

/// Put a landed page into the rows a frame draws.
pub(super) fn take_page(
    pages: &mut SpecimenPages,
    rows: &mut SourceTable,
    first: usize,
    data: &Data,
) {
    // A page carrying a feature no earlier page had grows the columns rather
    // than dropping the value. They only ever gain, so the frame notices and
    // lays itself out again.
    if pages.plan.extend(&data.aio_specimen) {
        let headers = pages.plan.headers();
        rows.columns = headers
            .iter()
            .map(|name| TableColumn {
                name: name.clone(),
                chars: name.chars().count(),
                numeric: false,
            })
            .collect();
    }

    rows.rows = pages.plan.rows(&data.aio_specimen);
    rows.first = first;
    // Only the page in hand can be measured, so a column is as wide as the
    // widest value seen on any page so far. It grows and never shrinks: a
    // column that narrowed on every page would shuffle the table sideways
    // each time one was turned.
    for (index, column) in rows.columns.iter_mut().enumerate() {
        let widest = rows
            .rows
            .iter()
            .filter_map(|row| row.get(index))
            .map(|value| value.trim().chars().count())
            .max()
            .unwrap_or_default();
        column.chars = column.chars.max(widest).min(super::super::table::MAX_CHARS);
    }
}
