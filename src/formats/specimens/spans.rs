//! How a numeric column's values are spread: its exact extent, and a count
//! below each bucket edge at the platform, turned into the histogram a span
//! is drawn over.

use super::*;

/// A floor below anything the platform holds, written out in full.
///
/// Its range parser takes plain decimals only: `-1e12` comes back "Invalid
/// range format", and so does an edge that Rust would have printed in
/// scientific notation, which is why the edges below are written to a fixed
/// number of places.
pub(super) const FLOOR: &str = "-1000000000000";

/// A ceiling above anything the platform holds, for the same reason.
pub(super) const CEILING: &str = "1000000000000";

/// Buckets a column's distribution is drawn in.
///
/// Each one is a separate index query on the platform — about 0.14s apiece —
/// so this is the width of a histogram traded against how long opening a
/// column takes.
pub(super) const BUCKETS: usize = 20;

/// Ask how a column's numbers are spread, in one request once its extent is
/// known.
///
/// The extent is the platform's own: `measurementStats` from its layout where
/// there is one, which matched the column's true smallest and largest values
/// on every column checked, or otherwise the first value each way of the
/// column sorted. It is never guessed from the page on screen, which holds a
/// hundred rows of perhaps ten thousand. The span stays on that extent however
/// the other filters narrow the histogram inside it, so its ends do not move
/// under the pointer.
pub(super) async fn ask_span(
    endpoint: &str,
    scope: &Scope,
    field: &str,
    extent: Option<(f64, f64)>,
    terms: &[TableFilterTerm],
) -> Result<NumericRange, String> {
    let (low, high) = match extent {
        Some(extent) => extent,
        None => ask_extent(endpoint, scope, field)
            .await?
            .ok_or_else(|| format!("{field} holds no numbers"))?,
    };
    let edges = NumericRange::bucket_edges(low, high, BUCKETS);
    let histogram = ask_histogram(endpoint, specimen_filters(scope, terms), field, &edges).await?;
    Ok(NumericRange::full(low as f32, high as f32, histogram))
}

/// The smallest and largest value a column holds, or nothing if it holds
/// none: the first specimen each way with the column sorted, among those
/// that have it at all.
pub(super) async fn ask_extent(
    endpoint: &str,
    scope: &Scope,
    field: &str,
) -> Result<Option<(f64, f64)>, String> {
    const QUERY: &str = "query($specimens: [Filter], $up: [Sort], $down: [Sort]) {
  low: aio_specimen(filter: $specimens, sort: $up, limit: 1) {
    measurements { featureType { referenceId title } value }
  }
  high: aio_specimen(filter: $specimens, sort: $down, limit: 1) {
    measurements { featureType { referenceId title } value }
  }
}";
    #[derive(Deserialize)]
    struct Ends {
        #[serde(default, deserialize_with = "maybe_list")]
        low: Vec<Specimen>,
        #[serde(default, deserialize_with = "maybe_list")]
        high: Vec<Specimen>,
    }

    // Only specimens holding the column: sorted, one without it could come
    // first either way.
    let held = TableFilterTerm::Between {
        field: field.to_string(),
        low: f32::NEG_INFINITY,
        high: f32::INFINITY,
    };
    let variables = json!({
        "specimens": specimen_filters(scope, &[held]),
        "up": [{ "field": field, "order": "ASC" }],
        "down": [{ "field": field, "order": "DESC" }],
    });
    let ends: Ends = graphql::ask(endpoint, QUERY, variables).await?;
    let value = |specimens: &[Specimen]| -> Option<f64> {
        specimens
            .first()?
            .measurements
            .iter()
            .find(|it| it.feature_type.reference_id.as_deref() == Some(field))?
            .value
            .as_deref()?
            .trim()
            .parse()
            .ok()
    };
    Ok(value(&ends.low).zip(value(&ends.high)))
}

/// How many rows fall in each bucket between `edges`, among those `filters`
/// admit.
///
/// The platform counts a range with its low end in and its high end out:
/// from the smallest age at death to the largest leaves out the donor who
/// died oldest. So nothing is asked below the first edge, which is the
/// column's smallest value and has nothing under it, and the last bucket is
/// counted up to a ceiling above everything rather than to the last edge,
/// which is the largest value and would be left out.
pub(super) async fn ask_histogram(
    endpoint: &str,
    filters: Value,
    field: &str,
    edges: &[f64],
) -> Result<Vec<u32>, String> {
    if edges.len() < 2 {
        return Ok(Vec::new());
    }
    let mut ends: Vec<String> = edges[1..edges.len() - 1]
        .iter()
        .map(|edge| format!("{edge:.6}"))
        .collect();
    ends.push(CEILING.to_string());
    let below = ask_below(endpoint, filters, field, &ends).await?;
    let cumulative: Vec<u32> = std::iter::once(0).chain(below).collect();
    Ok(buckets_of(&cumulative))
}

/// How many rows hold less than each of `ends`, among those `filters` admit.
async fn ask_below(
    endpoint: &str,
    filters: Value,
    field: &str,
    ends: &[String],
) -> Result<Vec<u32>, String> {
    let fields: String = ends
        .iter()
        .enumerate()
        .map(|(index, end)| {
            format!(
                "e{index}: aio_specimenRangeCounts(filter: $specimens, \
                 groupBy: {{ field: \"{field}\", range: \"[{FLOOR},{end}]\" }}) {{ count }}"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let query = format!("query($specimens: [Filter]) {{ {fields} }}");

    let response: Response<BTreeMap<String, Option<Vec<Aggregate>>>> =
        graphql::answer(endpoint, &query, json!({ "specimens": filters })).await?;
    let answers = response.partial(endpoint)?;

    Ok((0..ends.len())
        .map(|index| {
            answers
                .get(&format!("e{index}"))
                .and_then(|counted| counted.as_ref()?.first()?.count)
                .unwrap_or_default() as u32
        })
        .collect())
}

/// How many rows fall between each pair of edges, from how many fall below
/// each.
pub(super) fn buckets_of(cumulative: &[u32]) -> Vec<u32> {
    cumulative
        .windows(2)
        .map(|pair| pair[1].saturating_sub(pair[0]))
        .collect()
}

/// Ask how a column's numbers are spread, once someone opens it.
///
/// Opening is the signal because a distribution is twenty-odd index queries:
/// asking for every column of a table the moment it opened would be a minute
/// of waiting for histograms nobody looked at.
pub(super) fn serve_spans(mut sources: Query<(&mut SpecimenPages, &mut TableFilters)>) {
    for (mut pages, mut filters) in &mut sources {
        if let Some((_, _, fetch)) = pages.spanning.as_mut()
            && let Some(answer) = fetch.take()
        {
            let (column, asked, _) = pages.spanning.take().expect("just matched");
            match answer {
                Ok(span) => {
                    if let Some(filter) = filters.columns.get_mut(column) {
                        filter.kind = TableFilterKind::Range(Some(span));
                        pages.counted.insert(filter.id.clone(), asked);
                    }
                }
                Err(e) => {
                    // Asked once and left alone. Without this the column is
                    // still open, still has no span, and is asked about again
                    // every frame for as long as it is looked at.
                    warn!("reading how a column is spread: {e}");
                    if let Some(filter) = filters.columns.get_mut(column) {
                        filter.want(false);
                    }
                }
            }
        }
        if pages.spanning.is_some() {
            continue;
        }

        let Some(column) = filters
            .columns
            .iter()
            .position(|it| it.awaiting() && matches!(it.kind, TableFilterKind::Range(_)))
        else {
            continue;
        };
        let field = filters.columns[column].id.clone();
        let extent = pages.plan.extent(&field);
        let (endpoint, scope) = (pages.endpoint.clone(), pages.scope.clone());
        // A column with no span yet has no term of its own among these.
        let terms = filters.chosen();
        let asked = terms.clone();
        pages.spanning = Some((
            column,
            asked,
            fetching(async move { ask_span(&endpoint, &scope, &field, extent, &terms).await }),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_histogram_is_the_difference_between_counts_below_each_edge() {
        assert_eq!(buckets_of(&[0, 3, 3, 10]), [3, 0, 7]);
        // A count that went down, which a changing index could produce, is
        // nothing rather than an underflow.
        assert_eq!(buckets_of(&[5, 2]), [0]);
    }

    #[test]
    #[ignore = "reads the live SEA-AD donor specimens"]
    fn a_histogram_counts_every_row_its_extent_holds_both_ends_included() {
        // Age at death: 84 donors, 65 to 102, one of them at 102. Counted as
        // the platform counts a range, low end in and high end out, the
        // donor who died oldest would be left out.
        const PROJECT: &str = "JGN327NUXRZSHEV88TN";
        let endpoint = "https://idf-api-prod.aibs-idk-prod.net/";
        crate::app::net::block_on(async {
            let specimens = read(endpoint, PROJECT).await.unwrap();
            let age = specimens
                .plan
                .columns()
                .into_iter()
                .find(|it| it.title == "Age at death")
                .unwrap();
            assert_eq!(age.extent, Some((65.0, 102.0)), "the platform's own extent");
            let scope = Scope::project(PROJECT);
            let span = ask_span(endpoint, &scope, &age.id, age.extent, &[])
                .await
                .unwrap();
            assert_eq!((span.low, span.high), (65.0, 102.0));
            assert_eq!(span.histogram.len(), BUCKETS);
            assert_eq!(span.histogram.iter().sum::<u32>(), 84);
            assert!(*span.histogram.last().unwrap() >= 1, "the oldest donor");

            // Without the platform's extent, the column's own ends are asked
            // for, and they are the same.
            assert_eq!(
                ask_extent(endpoint, &scope, &age.id).await.unwrap(),
                Some((65.0, 102.0))
            );
        });
    }
}
