//! How a numeric column's values are spread: counted below each bucket edge
//! at the platform, and turned into the histogram a span is drawn over.

use super::*;

/// A floor below anything the platform holds, written out in full.
///
/// Its range parser takes plain decimals only: `-1e12` comes back "Invalid
/// range format", and so does an edge that Rust would have printed in
/// scientific notation, which is why the edges below are written to a fixed
/// number of places.
pub(super) const FLOOR: &str = "-1000000000000";

/// Buckets a column's distribution is drawn in.
///
/// Each one is a separate index query on the platform — about 0.14s apiece —
/// so this is the width of a histogram traded against how long opening a
/// column takes.
pub(super) const BUCKETS: usize = 20;

/// How far past the values already in hand a distribution is asked for, as a
/// fraction of their span.
///
/// The page on screen is the only sample of a column there is without asking,
/// and a hundred rows of ten thousand will not hold either end. Asking wide
/// and keeping the buckets that answered is cheaper than finding the ends
/// first: the platform has no query that gives a column's extent —
/// `measurementStats` sits on `aio_specimenFacetedSearchProperties`, which
/// answers with an empty list for every project.
pub(super) const WIDEN: f64 = 0.5;

/// Ask how a column's numbers are distributed, in one request.
///
/// A cumulative count at each bucket edge — how many rows hold less than this
/// — since that is the one shape `aio_specimenRangeCounts` answers. Taking
/// the differences gives the histogram, and the outermost buckets that hold
/// anything give the extent.
///
/// `seen` is whatever values are already on screen, which is what the span
/// asked about is built from.
pub(super) async fn ask_span(
    endpoint: &str,
    project: &str,
    field: &str,
    seen: &[f64],
    terms: &[TableFilterTerm],
) -> Result<NumericRange, String> {
    let edges = bucket_edges(seen);
    let cumulative =
        ask_cumulative(endpoint, specimen_filters(project, terms), field, &edges).await?;
    histogram_of(&edges, &cumulative).ok_or_else(|| format!("{field} holds no numbers"))
}

/// How many rows hold less than each edge, among those `filters` admit.
pub(super) async fn ask_cumulative(
    endpoint: &str,
    filters: Value,
    field: &str,
    edges: &[f64],
) -> Result<Vec<u32>, String> {
    let fields: String = edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            format!(
                "e{index}: aio_specimenRangeCounts(filter: $specimens, \
                 groupBy: {{ field: \"{field}\", range: \"[{FLOOR},{edge:.6}]\" }}) {{ count }}"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let query = format!("query($specimens: [Filter]) {{ {fields} }}");

    let response: Response<BTreeMap<String, Option<Vec<Aggregate>>>> =
        graphql::answer(endpoint, &query, json!({ "specimens": filters })).await?;
    let answers = response.partial(endpoint)?;

    Ok((0..edges.len())
        .map(|index| {
            answers
                .get(&format!("e{index}"))
                .and_then(|counted| counted.as_ref()?.first()?.count)
                .unwrap_or_default() as u32
        })
        .collect())
}

/// The edges a distribution is asked about: [`BUCKETS`] buckets across what is
/// on screen, widened at both ends for what is not.
pub(super) fn bucket_edges(seen: &[f64]) -> Vec<f64> {
    let low = seen.iter().copied().fold(f64::INFINITY, f64::min);
    let high = seen.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (low, high) = if low.is_finite() && high.is_finite() {
        (low, high)
    } else {
        (0.0, 1.0)
    };
    // A column of one value throughout still needs a span to widen, or its
    // value would sit on the lowest edge and be counted in no bucket.
    let pad = (high - low).max(high.abs().max(1.0) * 1e-6) * WIDEN;
    NumericRange::bucket_edges(low - pad, high + pad, BUCKETS)
}

/// How many rows fall between each pair of edges, from how many fall below
/// each.
pub(super) fn buckets_of(cumulative: &[u32]) -> Vec<u32> {
    cumulative
        .windows(2)
        .map(|pair| pair[1].saturating_sub(pair[0]))
        .collect()
}

/// Turn cumulative counts at each edge into a histogram and the extent that
/// holds it.
///
/// The outermost buckets holding anything are the extent, so a column asked
/// about far wider than it runs is still drawn across what it has.
pub(super) fn histogram_of(edges: &[f64], cumulative: &[u32]) -> Option<NumericRange> {
    let buckets = buckets_of(cumulative);
    let first = buckets.iter().position(|count| *count > 0)?;
    let last = buckets.iter().rposition(|count| *count > 0)?;
    Some(NumericRange::full(
        edges[first] as f32,
        edges[last + 1] as f32,
        buckets[first..=last].to_vec(),
    ))
}

/// Ask how a column's numbers are spread, once someone opens it.
///
/// Opening is the signal because a distribution is twenty-odd index queries:
/// asking for every column of a table the moment it opened would be a minute
/// of waiting for histograms nobody looked at.
pub(super) fn serve_spans(
    mut sources: Query<(&mut SpecimenPages, &mut TableFilters, &SourceTable)>,
) {
    for (mut pages, mut filters, rows) in &mut sources {
        if let Some((_, _, fetch)) = pages.spanning.as_mut()
            && let Some(answer) = fetch.take()
        {
            let (column, asked, _) = pages.spanning.take().expect("just matched");
            match answer {
                Ok(span) => {
                    if let Some(filter) = filters.columns.get_mut(column) {
                        filter.kind = TableFilterKind::Range {
                            span: Some(span),
                            wanted: true,
                        };
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

        let Some(column) = filters.columns.iter().position(TableFilter::awaiting_span) else {
            continue;
        };
        let field = filters.columns[column].id.clone();
        // What is on screen is the only sample of the column there is without
        // asking, and it is what the span asked about is built around.
        let seen = column_values(rows, &filters.columns[column].name);
        let (endpoint, project) = (pages.endpoint.clone(), pages.project.clone());
        // A column with no span yet has no term of its own among these.
        let terms = filters.chosen();
        let asked = terms.clone();
        pages.spanning = Some((
            column,
            asked,
            fetching(async move { ask_span(&endpoint, &project, &field, &seen, &terms).await }),
        ));
    }
}

/// The numbers a column holds on the page in hand.
pub(super) fn column_values(rows: &SourceTable, name: &str) -> Vec<f64> {
    let Some(index) = rows
        .columns
        .iter()
        .position(|column| column.name == name || column.name.starts_with(&format!("{name} (")))
    else {
        return Vec::new();
    };
    rows.rows
        .iter()
        .filter_map(|row| row.get(index)?.trim().parse::<f64>().ok())
        .collect()
}
