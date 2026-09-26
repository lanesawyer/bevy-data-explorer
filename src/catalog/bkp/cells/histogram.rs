//! A numeric column's histogram: the bucket edges asked for, and the counted
//! ranges placed back into them.

use super::*;

/// Buckets in a numeric property's histogram.
pub(super) const BUCKETS: usize = 32;

/// Counts in [`BUCKETS`] equal buckets across `extent`, of the cells
/// `filters` admit, as [`counts`] takes them.
pub(super) async fn histogram(
    endpoint: &str,
    filter: &Value,
    field: &str,
    extent: (f32, f32),
    filters: &Value,
) -> Result<Vec<u32>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        cell_range_counts: Vec<Counted>,
    }

    let edges = bucket_edges(extent);
    let range: Vec<String> = edges
        .windows(2)
        .map(|pair| format!("[{},{}]", pair[0], pair[1]))
        .collect();
    let variables = json!({ "filter": filter, "field": field, "range": range, "filters": filters });
    let data: Data = ask(endpoint, RANGE_COUNTS, variables).await?;
    Ok(bin(&edges, &data.cell_range_counts))
}

/// Edges of [`BUCKETS`] buckets across `extent`.
///
/// The API's ranges are half-open, including their low end and not their high
/// one, so the last edge sits just past the maximum to keep the cells holding
/// it.
pub(super) fn bucket_edges((low, high): (f32, f32)) -> Vec<f64> {
    let mut edges = NumericRange::bucket_edges(f64::from(low), f64::from(high), BUCKETS);
    let span = edges[BUCKETS] - edges[0];
    edges[BUCKETS] += span.max(1.0) * 1e-6;
    edges
}

/// Put each counted range back in its bucket.
///
/// The API echoes each range back reformatted — `[68.0,...]` returns as
/// `[68,...]` — and not necessarily in the order asked, so a range is placed by
/// the low end it parses to rather than by its text or its position.
pub(super) fn bin(edges: &[f64], counted: &[Counted]) -> Vec<u32> {
    let mut buckets = vec![0u32; edges.len() - 1];
    for counted in counted {
        let Some(low) = counted
            .properties
            .first()
            .and_then(|tuple| tuple.value.as_deref())
            .and_then(|value| value.trim_start_matches('[').split(',').next())
            .and_then(|low| low.trim().parse::<f64>().ok())
        else {
            continue;
        };
        let nearest = edges[..edges.len() - 1]
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - low).abs().total_cmp(&(b.1 - low).abs()))
            .map(|(bucket, _)| bucket);
        if let Some(bucket) = nearest {
            buckets[bucket] = buckets[bucket].saturating_add(counted.count as u32);
        }
    }
    buckets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counted_ranges_are_placed_by_their_low_end_whatever_their_text() {
        let edges = bucket_edges((68.0, 99.0));
        assert_eq!(edges.len(), BUCKETS + 1);
        assert!(edges[BUCKETS] > 99.0, "the maximum must fall in a bucket");

        let counted: Vec<Counted> = serde_json::from_str(
            r#"[
              {"count": 7, "properties": [{"value": "[68,68.96875]"}]},
              {"count": 3, "properties": [{"value": "[98.03125,99.000031]"}]},
              {"count": 1, "properties": []}
            ]"#,
        )
        .unwrap();
        let buckets = bin(&edges, &counted);
        assert_eq!(buckets[0], 7);
        assert_eq!(buckets[BUCKETS - 1], 3);
        assert_eq!(buckets.iter().sum::<u32>(), 10);
    }

    #[test]
    fn a_column_of_one_value_still_has_a_bucket_to_count_in() {
        let edges = bucket_edges((5.0, 5.0));
        assert!(edges[BUCKETS] > edges[0]);
    }
}
