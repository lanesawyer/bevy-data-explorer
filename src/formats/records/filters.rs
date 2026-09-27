//! What a table of records can be narrowed by: every filter offered at
//! once, what each offers read only when it is opened, and every one read
//! counted again whenever what is ticked or searched for changes.

use super::query::{Reading, ask_values};
use super::*;

/// Read the filters that are open and unread, count again the ones read
/// under other ticks or another search than those now in force, and put what comes back into
/// them.
///
/// One request at a time, carrying every filter that wants reading when it
/// goes; one wanted while it is out waits for the next.
pub(super) fn serve_values(
    mut sources: Query<(&mut RecordPages, &mut TableFilters, &TableSearch)>,
) {
    for (mut pages, mut filters, search) in &mut sources {
        if let Some((_, _, fetch)) = pages.reading.as_mut()
            && let Some(answer) = fetch.take()
        {
            let (paths, asked, _) = pages.reading.take().expect("just matched");
            match answer {
                Ok(answers) => {
                    for (path, counts) in answers {
                        take_values(&mut filters, &path, counts, &asked);
                        pages.counted.insert(path, asked.clone());
                    }
                }
                Err(e) => {
                    // An unread filter is shut, so it says it could not be
                    // read and reopening asks again. A read one keeps the
                    // counts it had rather than being asked forever.
                    warn!("reading what BKP Registry records can be narrowed by: {e}");
                    for column in &mut filters.columns {
                        if paths.contains(&column.id) {
                            if column.read() {
                                pages.counted.insert(column.id.clone(), asked.clone());
                            } else {
                                column.want(false);
                            }
                        }
                    }
                }
            }
        }
        if pages.reading.is_some() || filters.pending {
            continue;
        }
        let now = asked_of(&filters, search);
        let readings: Vec<Reading> = filters
            .columns
            .iter()
            .filter_map(|column| {
                if column.awaiting() {
                    return Some(Reading {
                        path: column.id.clone(),
                        known: None,
                    });
                }
                let stale = column.read() && pages.counted.get(&column.id) != Some(&now);
                stale.then(|| Reading {
                    path: column.id.clone(),
                    known: Some(
                        column
                            .listed()
                            .iter()
                            .map(|value| value.label.clone())
                            .collect(),
                    ),
                })
            })
            .collect();
        if readings.is_empty() {
            continue;
        }
        let paths = readings.iter().map(|it| it.path.clone()).collect();
        let (endpoint, kind, asked) = (pages.endpoint.clone(), pages.kind, now.clone());
        pages.reading = Some((
            paths,
            now,
            fetching(async move { ask_values(&endpoint, kind, readings, &asked).await }),
        ));
    }
}

/// Put what one filter offers into it, or its counts into what it already
/// offers, keeping what is ticked.
///
/// A value nothing holds is not offered at all when it is first read under
/// no ticks and no search: the registry declares types it has no records
/// of, and a third of the data asset types are that.
fn take_values(filters: &mut TableFilters, path: &str, counts: Vec<(String, u64)>, asked: &Asked) {
    let Some(column) = filters.columns.iter_mut().find(|it| it.id == path) else {
        return;
    };
    match &mut column.kind {
        TableFilterKind::Values(Some(values)) => {
            for value in values.iter_mut() {
                if let Some((_, count)) = counts.iter().find(|(label, _)| *label == value.label) {
                    value.count = *count;
                }
            }
        }
        kind => {
            *kind = TableFilterKind::Values(Some(
                counts
                    .into_iter()
                    .filter(|(_, count)| *count > 0 || *asked != Asked::default())
                    .map(|(label, count)| TableFilterValue {
                        label,
                        count,
                        chosen: false,
                    })
                    .collect(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filters() -> TableFilters {
        TableFilters::ready(filters_of(Kind::DataAssets))
    }

    #[test]
    fn a_first_read_leaves_out_what_nothing_holds() {
        let mut filters = filters();
        take_values(
            &mut filters,
            "status",
            vec![("PUBLISHED".into(), 38_562), ("RETRACTED".into(), 0)],
            &Asked::default(),
        );
        let labels: Vec<&str> = filters.columns[1]
            .listed()
            .iter()
            .map(|it| it.label.as_str())
            .collect();
        assert_eq!(labels, ["PUBLISHED"]);
    }

    #[test]
    fn a_recount_keeps_what_is_ticked() {
        let mut filters = filters();
        take_values(
            &mut filters,
            "status",
            vec![("PUBLISHED".into(), 10), ("ARCHIVED".into(), 20)],
            &Asked::default(),
        );
        filters.columns[1].listed_mut()[0].chosen = true;
        let ticked = Asked {
            terms: filters.chosen(),
            search: None,
        };
        take_values(
            &mut filters,
            "status",
            vec![("PUBLISHED".into(), 10), ("ARCHIVED".into(), 0)],
            &ticked,
        );
        let values = filters.columns[1].listed();
        assert!(values[0].chosen);
        assert_eq!(values[1].count, 0);
        assert_eq!(values.len(), 2);
    }
}
