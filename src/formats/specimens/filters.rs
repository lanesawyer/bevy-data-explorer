//! What every column holds, asked once and offered as the table's filters.

use super::*;

/// Ask what every column holds, in one request.
///
/// A root field per column, aliased, since the counts come back grouped by
/// whichever column was asked about and there is no way to ask for several
/// groupings at once.
///
/// Every column is asked about and the platform decides which it can answer.
/// A column it cannot comes back null beside the ones it could, so the answer
/// is read for what is in it rather than refused whole — which is what keeps
/// one column's failure from costing the rest. Numeric measurements are the
/// ones that fail today: grouping by one answers "Unable to cast object of
/// type 'System.Double' to type 'System.String'", so the platform's own
/// filters offer Age where these cannot.
///
/// The counts are of the whole project, since nothing narrows it yet;
/// [`ask_recount`] counts them again once something does.
pub(super) async fn ask_values(
    endpoint: &str,
    project: &str,
    columns: &[(String, String, bool)],
) -> Result<Vec<TableFilter>, String> {
    let fields: String = columns
        .iter()
        .enumerate()
        .map(|(index, (id, ..))| {
            format!(
                "c{index}: aio_specimenCounts(filter: $specimens, groupBy: [\"{id}\"]) \
                 {{ count properties {{ property value }} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let query = format!("query($specimens: [Filter]) {{ {fields} }}");

    let variables = json!({ "specimens": specimen_filters(project, &[]) });
    let response: Response<BTreeMap<String, Option<Vec<Grouped>>>> =
        graphql::answer(endpoint, &query, variables).await?;
    // A column the platform could not group by is reported beside the ones it
    // could, and costs only itself.
    for error in &response.errors {
        debug!("a column cannot be grouped by: {}", error.message);
    }
    let answers = response.partial(endpoint)?;

    let mut filters = Vec::new();
    for (index, (id, name, numeric)) in columns.iter().enumerate() {
        let Some(Some(groups)) = answers.get(&format!("c{index}")) else {
            // The platform cannot group a column of numbers — it answers
            // "Unable to cast object of type 'System.Double' to type
            // 'System.String'" — so that is what marks one out, and a number
            // is narrowed by taking a span of it rather than by ticking every
            // reading anyone took.
            if *numeric {
                filters.push(TableFilter::range(id, name));
            }
            continue;
        };
        let values: Vec<TableFilterValue> = labeled(groups)
            .into_iter()
            .map(|(label, count)| TableFilterValue {
                label,
                count,
                chosen: false,
            })
            .collect();
        if values.is_empty() {
            continue;
        }
        filters.push(TableFilter::values(id, name, values));
    }
    Ok(filters)
}

/// Ask the platform what each column holds, once per source, and offer the
/// answer as filters.
pub(super) fn offer_filters(
    mut commands: Commands,
    mut sources: Query<(Entity, &mut SpecimenPages)>,
) {
    for (source, mut pages) in &mut sources {
        if let Some(fetch) = pages.offering.as_mut()
            && let Some(answer) = fetch.take()
        {
            pages.offering = None;
            match answer {
                Ok(columns) if !columns.is_empty() => {
                    info!("{} columns to narrow specimens by", columns.len());
                    commands.entity(source).insert(TableFilters::ready(columns));
                }
                // Nothing to narrow by is not a failure: the section simply
                // does not appear.
                Ok(_) => {
                    commands.entity(source).remove::<TableFilters>();
                }
                Err(e) => {
                    warn!("asking what specimens can be narrowed by: {e}");
                    commands.entity(source).remove::<TableFilters>();
                }
            }
        }
        if pages.offered {
            continue;
        }
        pages.offered = true;
        let (endpoint, project) = (pages.endpoint.clone(), pages.project.clone());
        // Every column, not only the annotations: the platform answers for
        // whichever of them it can group by.
        let columns = pages.plan.columns();
        commands.entity(source).insert(TableFilters::pending());
        pages.offering = Some(fetching(async move {
            ask_values(&endpoint, &project, &columns).await
        }));
    }
}
