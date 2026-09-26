//! Every column counted again under the filters now in force, so what each
//! offers says what ticking it would leave.

use super::*;

/// What one column is counted again under.
pub(super) struct Recounting {
    pub(super) id: String,
    /// What it is counted under, as the cell properties are counted: a
    /// column of values under every term, its own too, so a value left
    /// unticked counts none — the count is what is on screen. A span under
    /// every term but its own, so its histogram keeps the shape outside it.
    pub(super) terms: Vec<TableFilterTerm>,
    /// A span's bucket edges, kept so the histogram is recounted in the same
    /// buckets it was drawn in; nothing for a column of values.
    pub(super) edges: Option<Vec<f64>>,
}

/// A column counted again: each value's count, or a span's histogram.
pub(super) enum Recounted {
    Values(String, Vec<(String, u64)>),
    Span(String, Vec<u32>),
}

/// What to count again for the filters now in force.
pub(super) fn recounting(filters: &TableFilters) -> Vec<Recounting> {
    let terms = filters.chosen();
    filters
        .columns
        .iter()
        .filter_map(|column| {
            let edges = match &column.kind {
                TableFilterKind::Values(_) => None,
                TableFilterKind::Range { span, .. } => {
                    let span = span.as_ref()?;
                    let buckets = span.histogram.len().max(1);
                    let step = f64::from(span.high - span.low) / buckets as f64;
                    Some(
                        (0..=buckets)
                            .map(|i| f64::from(span.low) + step * i as f64)
                            .collect(),
                    )
                }
            };
            Some(Recounting {
                id: column.id.clone(),
                terms: terms
                    .iter()
                    .filter(|term| edges.is_none() || term.field() != column.id)
                    .cloned()
                    .collect(),
                edges,
            })
        })
        .collect()
}

/// Count every column again under the filters now in force.
///
/// The values in one request, a variable per column since each is counted
/// under different terms. Each span in a request of its own, as it was first
/// counted: its edges already come close to the ceiling on aliases one
/// request can carry.
pub(super) async fn ask_recount(
    endpoint: &str,
    project: &str,
    columns: Vec<Recounting>,
) -> Result<Vec<Recounted>, String> {
    let (spans, values): (Vec<_>, Vec<_>) = columns.into_iter().partition(|it| it.edges.is_some());
    let mut recounted = Vec::new();

    if !values.is_empty() {
        let declared: String = (0..values.len())
            .map(|index| format!("$f{index}: [Filter]"))
            .collect::<Vec<_>>()
            .join(", ");
        let fields: String = values
            .iter()
            .enumerate()
            .map(|(index, column)| {
                format!(
                    "c{index}: aio_specimenCounts(filter: $f{index}, groupBy: [\"{}\"]) \
                     {{ count properties {{ property value }} }}",
                    column.id
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let query = format!("query({declared}) {{ {fields} }}");
        let variables: serde_json::Map<String, Value> = values
            .iter()
            .enumerate()
            .map(|(index, column)| {
                (
                    format!("f{index}"),
                    specimen_filters(project, &column.terms),
                )
            })
            .collect();

        let response: Response<BTreeMap<String, Option<Vec<Grouped>>>> =
            graphql::answer(endpoint, &query, Value::Object(variables)).await?;
        let answers = response.partial(endpoint)?;
        for (index, column) in values.into_iter().enumerate() {
            // A column that could not be counted keeps the counts it had.
            if let Some(Some(groups)) = answers.get(&format!("c{index}")) {
                recounted.push(Recounted::Values(column.id, labeled(groups)));
            }
        }
    }

    for column in spans {
        let edges = column.edges.unwrap_or_default();
        let cumulative = ask_cumulative(
            endpoint,
            specimen_filters(project, &column.terms),
            &column.id,
            &edges,
        )
        .await?;
        recounted.push(Recounted::Span(column.id, buckets_of(&cumulative)));
    }
    Ok(recounted)
}

/// Write counts that came back into the filters they were counted for.
///
/// A value the answer does not name is held by no row the other columns
/// admit, which is a count of nothing rather than one left as it was.
pub(super) fn take_recount(filters: &mut TableFilters, recounted: Vec<Recounted>) {
    for answer in recounted {
        match answer {
            Recounted::Values(id, counts) => {
                let Some(column) = filters.columns.iter_mut().find(|it| it.id == id) else {
                    continue;
                };
                let counts: HashMap<String, u64> = counts.into_iter().collect();
                for value in column.listed_mut() {
                    let count = counts.get(&value.label).copied().unwrap_or(0);
                    if value.count != count {
                        value.count = count;
                    }
                }
            }
            Recounted::Span(id, histogram) => {
                if let Some(span) = filters
                    .columns
                    .iter_mut()
                    .find(|it| it.id == id)
                    .and_then(TableFilter::span_mut)
                    && span.histogram.len() == histogram.len()
                    && span.histogram != histogram
                {
                    span.histogram = histogram;
                }
            }
        }
    }
}

/// Count the filters again whenever what narrows the table changes, as the
/// cell properties are counted again when theirs do.
///
/// Only the columns whose terms moved: dragging a span recounts everything
/// but that span's own histogram, which it would not change.
///
/// One ask at a time. A change made while one is out is caught when it lands,
/// since what it was asked under no longer matches — so a span dragged across
/// a dozen frames costs a couple of asks rather than a dozen.
pub(super) fn serve_counts(mut sources: Query<(&mut SpecimenPages, &mut TableFilters)>) {
    for (mut pages, mut filters) in &mut sources {
        if filters.pending {
            continue;
        }
        if let Some((_, fetch)) = pages.counting.as_mut()
            && let Some(answer) = fetch.take()
        {
            let (asked, _) = pages.counting.take().expect("just matched");
            match answer {
                Ok(recounted) => take_recount(&mut filters, recounted),
                // Left as they were, and not asked again until the filters
                // change: asking again at once would fail again at once.
                Err(e) => warn!("counting specimens again: {e}"),
            }
            pages.counted.extend(asked);
        }
        if pages.counting.is_some() {
            continue;
        }
        let columns: Vec<Recounting> = recounting(&filters)
            .into_iter()
            .filter(|column| {
                pages
                    .counted
                    .get(&column.id)
                    .map_or(!column.terms.is_empty(), |counted| *counted != column.terms)
            })
            .collect();
        if columns.is_empty() {
            continue;
        }
        let asked = columns
            .iter()
            .map(|column| (column.id.clone(), column.terms.clone()))
            .collect();
        let (endpoint, project) = (pages.endpoint.clone(), pages.project.clone());
        pages.counting = Some((
            asked,
            fetching(async move { ask_recount(&endpoint, &project, columns).await }),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn narrowed() -> TableFilters {
        let value = |label: &str, count: u64| TableFilterValue {
            label: label.into(),
            count,
            chosen: false,
        };
        let mut age = TableFilter::range("age", "Age");
        age.kind = TableFilterKind::Range {
            span: Some(NumericRange::full(60.0, 100.0, vec![5, 5, 5, 5])),
            wanted: true,
        };
        let mut filters = TableFilters::ready(vec![
            TableFilter::values("donor", "Donor", vec![value("D1", 1), value("D2", 1)]),
            age,
        ]);
        filters.columns[0].listed_mut()[0].chosen = true;
        filters.columns[1].span_mut().unwrap().from = 80.0;
        filters
    }

    #[test]
    fn values_are_counted_under_every_filter_and_a_span_under_all_but_its_own() {
        let columns = recounting(&narrowed());
        let donor = columns.iter().find(|it| it.id == "donor").unwrap();
        assert_eq!(donor.terms, narrowed().chosen());
        let age = columns.iter().find(|it| it.id == "age").unwrap();
        assert!(age.terms.iter().all(|term| term.field() == "donor"));
    }

    #[test]
    fn a_span_is_counted_again_in_the_buckets_it_was_drawn_in() {
        let columns = recounting(&narrowed());
        let edges = columns
            .iter()
            .find(|it| it.id == "age")
            .and_then(|it| it.edges.clone())
            .unwrap();
        assert_eq!(edges, [60.0, 70.0, 80.0, 90.0, 100.0]);
    }

    #[test]
    fn a_value_the_recount_does_not_name_counts_nothing() {
        let mut filters = narrowed();
        take_recount(
            &mut filters,
            vec![
                Recounted::Values("donor".into(), vec![("D2".into(), 3)]),
                Recounted::Span("age".into(), vec![0, 1, 2, 3]),
            ],
        );
        let counts: Vec<u64> = filters.columns[0]
            .listed()
            .iter()
            .map(|v| v.count)
            .collect();
        assert_eq!(counts, [0, 3]);
        assert_eq!(filters.columns[1].span().unwrap().histogram, [0, 1, 2, 3]);
        // The span the user chose is left where it was.
        assert_eq!(filters.columns[1].span().unwrap().from, 80.0);
    }

    #[test]
    fn a_histogram_of_another_shape_is_not_taken() {
        let mut filters = narrowed();
        take_recount(
            &mut filters,
            vec![Recounted::Span("age".into(), vec![1, 2])],
        );
        assert_eq!(filters.columns[1].span().unwrap().histogram, [5, 5, 5, 5]);
    }
}
