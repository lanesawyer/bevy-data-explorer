//! What the table can be narrowed by: every column offered at once, and what
//! each holds read only when it is opened.

use super::*;

/// What each answer to [`ask_values`] holds for its column: every value and
/// how many specimens hold it, or nothing if the platform could not group by
/// it.
pub(super) type Groups = Vec<Option<Vec<(String, u64)>>>;

/// Ask what each of these columns holds, in one request, counted under
/// `terms`.
///
/// A root field per column, aliased, since the counts come back grouped by
/// whichever column was asked about and there is no way to ask for several
/// groupings at once.
///
/// A column the platform cannot group by comes back null beside the ones it
/// could, so the answer is read for what is in it rather than refused whole —
/// which is what keeps one column's failure from costing the rest. Numeric
/// measurements are the ones that fail: grouping by one answers "Unable to
/// cast object of type 'System.Double' to type 'System.String'".
///
/// Counted under every term, the column's own included, as a recount counts
/// them, so what each value says is what ticking it would leave on screen.
pub(super) async fn ask_values(
    endpoint: &str,
    scope: &Scope,
    ids: &[String],
    terms: &[TableFilterTerm],
) -> Result<Groups, String> {
    let fields: String = ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            format!(
                "c{index}: aio_specimenCounts(filter: $specimens, groupBy: [\"{id}\"]) \
                 {{ count properties {{ property value }} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let query = format!("query($specimens: [Filter]) {{ {fields} }}");

    let variables = json!({ "specimens": specimen_filters(scope, terms) });
    let response: Response<BTreeMap<String, Option<Vec<Grouped>>>> =
        graphql::answer(endpoint, &query, variables).await?;
    for error in &response.errors {
        debug!("a column cannot be grouped by: {}", error.message);
    }
    let answers = response.partial(endpoint)?;
    Ok((0..ids.len())
        .map(|index| match answers.get(&format!("c{index}")) {
            Some(Some(groups)) => Some(labeled(groups)),
            _ => None,
        })
        .collect())
}

/// The table's filters: a column per feature the platform can narrow by,
/// none of them read.
///
/// A feature the platform lays out as a span is offered as one. Any other is
/// offered as values to tick, and one that turns out to be numbers when it is
/// read — which only a project the platform does not lay out can hold — is
/// made a span then.
pub(super) fn filters_of(plan: &Plan) -> Vec<TableFilter> {
    plan.columns()
        .into_iter()
        .map(|feature| {
            if feature.spanned == Some(true) {
                TableFilter::range(feature.id, feature.title)
            } else {
                TableFilter::unread_values(feature.id, feature.title)
            }
        })
        .collect()
}

/// Offer the table's filters, once per source and per kind.
///
/// Nothing is asked: what a column holds is read by [`serve_values`] when it
/// is opened. Asking for every column the moment a table opened cost the V1
/// neurons thirteen seconds, most of it on columns nobody looks at.
pub(super) fn offer_filters(
    mut commands: Commands,
    mut sources: Query<(Entity, &mut SpecimenPages)>,
) {
    for (source, mut pages) in &mut sources {
        if pages.offered {
            continue;
        }
        pages.offered = true;
        let columns = filters_of(&pages.plan);
        if columns.is_empty() {
            // Nothing to narrow by is not a failure: the section simply
            // says so.
            commands.entity(source).remove::<TableFilters>();
        } else {
            commands.entity(source).insert(TableFilters::ready(columns));
        }
    }
}

/// Read what the open columns hold, and put it into them when it lands.
///
/// Every column opened while nothing is out goes in one request; one opened
/// while a read is out waits for the next.
pub(super) fn serve_values(mut sources: Query<(&mut SpecimenPages, &mut TableFilters)>) {
    for (mut pages, mut filters) in &mut sources {
        if let Some((_, _, fetch)) = pages.reading.as_mut()
            && let Some(answer) = fetch.take()
        {
            let (ids, asked, _) = pages.reading.take().expect("just matched");
            match answer {
                Ok(answers) => {
                    for (id, answer) in ids.iter().zip(answers) {
                        take_values(&mut pages, &mut filters, id, answer, &asked);
                    }
                }
                Err(e) => {
                    // Asked once and left alone, as a span is: the columns
                    // say they could not be read, and reopening asks again.
                    warn!("reading what specimens can be narrowed by: {e}");
                    for column in &mut filters.columns {
                        if ids.contains(&column.id) {
                            column.want(false);
                        }
                    }
                }
            }
        }
        if pages.reading.is_some() || filters.pending {
            continue;
        }
        let ids: Vec<String> = filters
            .columns
            .iter()
            .filter(|it| it.awaiting() && matches!(it.kind, TableFilterKind::Values(_)))
            .map(|it| it.id.clone())
            .collect();
        if ids.is_empty() {
            continue;
        }
        let terms = filters.chosen();
        let (endpoint, scope) = (pages.endpoint.clone(), pages.scope.clone());
        let (asked_ids, asked) = (ids.clone(), terms.clone());
        pages.reading = Some((
            asked_ids,
            asked,
            fetching(async move { ask_values(&endpoint, &scope, &ids, &terms).await }),
        ));
    }
}

/// Put what one column holds into it.
fn take_values(
    pages: &mut SpecimenPages,
    filters: &mut TableFilters,
    id: &str,
    answer: Option<Vec<(String, u64)>>,
    asked: &[TableFilterTerm],
) {
    // Found to be numbers only when the layout did not already say it was
    // values: a worded measurement such as sex is one the platform may still
    // fail to group.
    let may_be_span = pages
        .plan
        .columns()
        .iter()
        .any(|it| it.id == id && it.measured && it.spanned.is_none());
    let Some(column) = filters.columns.iter_mut().find(|it| it.id == id) else {
        return;
    };
    if column.read() {
        return;
    }
    match answer {
        Some(groups) => {
            let values = groups
                .into_iter()
                .map(|(label, count)| TableFilterValue {
                    label,
                    count,
                    chosen: false,
                })
                .collect();
            column.kind = TableFilterKind::Values(Some(values));
            pages.counted.insert(id.to_string(), asked.to_vec());
        }
        // The platform cannot group a column of numbers, so that is what
        // marks one out, and a number is narrowed by taking a span of it.
        // Still open, so its span is asked for next.
        None if may_be_span => column.kind = TableFilterKind::Range(None),
        None => column.want(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        let feature = |id: &str, measured: bool, spanned: Option<bool>| Feature {
            id: id.into(),
            title: id.into(),
            unit: None,
            measured,
            hidden: false,
            spanned,
            extent: None,
        };
        Plan::laid_out(vec![
            feature("diagnosis", false, Some(false)),
            feature("age", true, Some(true)),
            feature("sex", true, Some(false)),
            feature("ph", true, None),
            feature("", false, None),
        ])
    }

    #[test]
    fn every_column_is_offered_at_once_and_none_is_read() {
        let filters = filters_of(&plan());
        let kinds: Vec<(&str, bool)> = filters
            .iter()
            .map(|it| (it.id.as_str(), matches!(it.kind, TableFilterKind::Range(_))))
            .collect();
        // The layout says which are spans; a measurement it says nothing
        // about is taken for values until it is read. A feature with no id
        // cannot be narrowed by at all.
        assert_eq!(
            kinds,
            [
                ("diagnosis", false),
                ("age", true),
                ("sex", false),
                ("ph", false)
            ]
        );
        assert!(filters.iter().all(|it| !it.read() && !it.wanted));
    }

    #[test]
    fn a_column_is_read_as_values_or_found_to_be_numbers() {
        let mut pages = SpecimenPages::new(
            "E".into(),
            Scope::project("P"),
            plan(),
            Vec::new(),
            Vec::new(),
        );
        let mut filters = TableFilters::ready(filters_of(&pages.plan));
        for column in &mut filters.columns {
            column.want(true);
        }
        take_values(
            &mut pages,
            &mut filters,
            "diagnosis",
            Some(vec![("Dementia".into(), 3)]),
            &[],
        );
        take_values(&mut pages, &mut filters, "ph", None, &[]);
        take_values(&mut pages, &mut filters, "sex", None, &[]);

        assert_eq!(filters.columns[0].listed()[0].count, 3);
        assert_eq!(pages.counted.get("diagnosis"), Some(&Vec::new()));
        // Numbers where values were expected: a span, still open, so it is
        // asked for next.
        assert!(matches!(
            filters.columns[3].kind,
            TableFilterKind::Range(None)
        ));
        assert!(filters.columns[3].awaiting());
        // A worded column the platform could not group: shut, so the panel
        // says it could not be read rather than waiting on it.
        assert!(!filters.columns[2].read() && !filters.columns[2].awaiting());
    }
}
