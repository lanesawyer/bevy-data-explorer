//! Asking a catalog's service about the cells of a source it lists.
//!
//! A source is matched to its entry by address, so it makes no difference
//! whether it was opened from the dropdown, a typed URL or the command line.
//! One opened before its catalog has finished listing waits for it.
//!
//! Two questions are asked in turn. The first — labels, colors, which
//! properties to show — answers in a fraction of a second and replaces the
//! placeholders the format started with. The second, how many cells hold each
//! value, takes seconds, so it is written into the properties already on show
//! rather than holding them back. It is asked again whenever the filters
//! change, since each property is counted among the cells the others admit,
//! and whenever a gene is added, whose histogram has to be counted the same
//! way.

use std::collections::HashMap;

use bevy::prelude::*;

use super::{
    Catalogs, CellCounts, CellRecord, CellService, Entry, RegionFocus, RegionSummary, SummaryState,
};
use crate::app::net::{Fetching, fetching};
use crate::source::SourceUrl;
use crate::source::properties::{
    CellColumns, CellProperties, CellProperty, Column, Mixes, PropertyState, Provenance,
    Restriction,
};
use crate::source::region::SelectedRegion;

/// The most cells listed for one category. A rectangle can hold hundreds of
/// thousands, and the counts above the list are what say how many there really
/// are; this is a look at them, not all of them.
const CELL_PAGE: usize = 50;

/// How long the filters must hold still before they are counted under.
/// Dragging a range changes them every frame, and each change is a round of
/// queries taking seconds.
const SETTLE_SECS: f32 = 0.4;

/// The labels in flight from the service describing a source's cells.
#[derive(Component)]
pub struct Describing {
    service: CellService,
    catalog: String,
    reading: Fetching<Result<CellProperties, String>>,
}

/// Marks a source whose properties wait on no service any more, whether one
/// answered, failed, or there was none to ask.
#[derive(Component)]
pub struct Described;

/// Keeps a described source's counts in step with its filters.
#[derive(Component)]
pub struct Counting {
    service: CellService,
    catalog: String,
    /// The properties and filters last counted under, or nothing before the
    /// first count.
    asked: Option<Counted>,
    /// How long the properties or filters have differed from `asked`.
    waited: f32,
    /// The latest count. Replacing it drops, and so cancels, the one before.
    reading: Option<Fetching<Result<CellCounts, String>>>,
}

/// What a count depends on: which properties there are, how they filter, and
/// the column they are crossed against, since a change of coloring is a
/// different breakdown to ask for.
type Counted = (Vec<String>, Vec<(Column, Restriction)>, Option<String>);

/// Keeps a described source's selection summary in step with the rectangle
/// drawn over it and the filters in force.
///
/// Separate from [`Counting`] because the two answer different questions at
/// different rates: the whole dataset's counts change only when a filter does,
/// while a rectangle changes on every drag. Sharing one component would have
/// each drag re-ask for the histograms too.
#[derive(Component)]
pub struct CountingRegion {
    service: CellService,
    catalog: String,
    /// The region and filters last asked about, or nothing before the first.
    asked: Option<(SelectedRegion, Counted)>,
    /// How long they have differed from `asked`.
    waited: f32,
    reading: Option<Fetching<Result<CellCounts, String>>>,
    /// The same, for the category drilled into. Kept apart so that picking a
    /// category does not re-ask the count the categories were read from, and
    /// so that a slow drill-down never holds up the rectangle's own numbers.
    focus_asked: Option<(SelectedRegion, Counted, RegionFocus)>,
    focus_reading: Option<Fetching<Result<Vec<CellRecord>, String>>>,
}

/// Start describing each source a catalog has a service for.
pub fn ask(
    mut commands: Commands,
    catalogs: Res<Catalogs>,
    mut sources: Query<
        (Entity, &SourceUrl, &CellColumns, &mut CellProperties),
        (Without<Describing>, Without<Described>),
    >,
) {
    for (entity, url, columns, mut properties) in &mut sources {
        match catalogs.find(&url.0) {
            Some((
                catalog,
                Entry {
                    cells: Some(service),
                    ..
                },
            )) => {
                info!("asking {catalog} about the cells of {}", url.0);
                properties.provenance = Provenance::Fetching(catalog.to_string());
                commands.entity(entity).insert(Describing {
                    service: service.clone(),
                    catalog: catalog.to_string(),
                    reading: fetching(service.0.describe(columns.clone())),
                });
            }
            None if catalogs.listing() => {}
            _ => {
                commands.entity(entity).insert(Described);
            }
        }
    }
}

/// Ask a source's service about its cells again, after it failed.
pub fn retry(commands: &mut Commands, source: Entity, properties: &mut CellProperties) {
    properties.state = PropertyState::Ready;
    commands.entity(source).remove::<Described>();
}

/// Take in whatever labels the services have answered, and start counting.
pub fn take_answers(
    mut commands: Commands,
    mut sources: Query<(Entity, &mut Describing, &mut CellProperties)>,
) {
    for (entity, mut describing, mut properties) in &mut sources {
        let Some(result) = describing.reading.take() else {
            continue;
        };
        let catalog = describing.catalog.clone();
        let mut source = commands.entity(entity);
        match result {
            Ok(mut described) => {
                info!(
                    "{catalog} described {} cell properties",
                    described.properties.len()
                );
                described.provenance = Provenance::Service(catalog.clone());
                *properties = described;
                source.insert(Counting {
                    service: describing.service.clone(),
                    catalog: catalog.clone(),
                    asked: None,
                    waited: 0.0,
                    reading: None,
                });
                // Offered only where the service can answer it, so a dataset
                // whose cells are described but not indexed by position shows
                // no selection section rather than an empty one.
                if describing.service.0.counts_regions() {
                    source.insert((
                        CountingRegion {
                            service: describing.service.clone(),
                            catalog,
                            asked: None,
                            waited: 0.0,
                            reading: None,
                            focus_asked: None,
                            focus_reading: None,
                        },
                        RegionSummary::default(),
                    ));
                }
            }
            Err(error) => {
                warn!("{catalog}: could not describe cells: {error}");
                properties.provenance = Provenance::Files;
                properties.state = PropertyState::Failed(format!("{catalog}: {error}"));
            }
        }
        source.remove::<Describing>().insert(Described);
    }
}

/// Count each source's cells once described, and again once its filters
/// change and settle; take in whatever counts have landed.
pub fn recount(time: Res<Time>, mut sources: Query<(&mut Counting, &mut CellProperties)>) {
    for (mut counting, mut properties) in &mut sources {
        let wanted = (
            properties
                .properties
                .iter()
                .map(|property| property.id.clone())
                .collect(),
            properties.filters(),
            properties.mix_column().map(str::to_string),
        );
        if counting.asked.as_ref() == Some(&wanted) {
            counting.waited = 0.0;
        } else {
            counting.waited += time.delta_secs();
            if counting.asked.is_none() || counting.waited >= SETTLE_SECS {
                counting.reading = Some(fetching(counting.service.0.count(&properties)));
                counting.asked = Some(wanted);
                counting.waited = 0.0;
            }
        }

        let Some(result) = counting.reading.as_mut().and_then(Fetching::take) else {
            continue;
        };
        counting.reading = None;
        match result {
            Ok(counts) => {
                debug!(
                    "{} counted cells for {} properties, {} of them crossed against {}",
                    counting.catalog,
                    counts.values.len() + counts.histograms.len(),
                    counts.mixes.len(),
                    counts.mix_column.as_deref().unwrap_or("nothing"),
                );
                apply_counts(&mut properties, counts);
            }
            Err(error) => warn!("{}: could not count cells: {error}", counting.catalog),
        }
    }
}

/// Count the cells inside each source's selected rectangle, and again once it
/// or the filters change and settle; take in whatever counts have landed.
///
/// A source with no rectangle over it is put back to idle rather than left
/// showing the last one's numbers: a summary that outlived its selection reads
/// as the numbers for whatever is on screen now.
pub fn recount_region(
    time: Res<Time>,
    mut sources: Query<(
        &mut CountingRegion,
        &CellProperties,
        &mut RegionSummary,
        Option<&SelectedRegion>,
        Option<&RegionFocus>,
    )>,
) {
    for (mut counting, properties, mut summary, region, focus) in &mut sources {
        let region = region.filter(|region| !region.is_empty());
        let Some(region) = region else {
            counting.asked = None;
            counting.waited = 0.0;
            // Drops the fetch, which cancels it: a rectangle cleared while its
            // count was in flight should cost nothing more.
            counting.reading = None;
            counting.focus_asked = None;
            counting.focus_reading = None;
            if summary.state != SummaryState::Idle {
                summary.counted = None;
                summary.values.clear();
                summary.cells.clear();
                summary.state = SummaryState::Idle;
                summary.focus_state = SummaryState::Idle;
            }
            continue;
        };

        // Keyed on the filters as well as the rectangle: a region is counted
        // among the cells the filters admit, so unticking a class has to
        // re-ask even though the rectangle never moved.
        let wanted = (
            region.clone(),
            (
                properties
                    .properties
                    .iter()
                    .map(|property| property.id.clone())
                    .collect(),
                properties.filters(),
                // A region's counts are not crossed against the coloring, so
                // choosing another color-by does not re-ask them.
                None,
            ),
        );
        if counting.asked.as_ref() == Some(&wanted) {
            counting.waited = 0.0;
        } else {
            counting.waited += time.delta_secs();
            // A rectangle is dragged out over many frames, and each shape of
            // it would otherwise be a round of queries taking seconds.
            if counting.waited >= SETTLE_SECS {
                counting.reading = Some(fetching(
                    counting.service.0.count_region(properties, region),
                ));
                counting.asked = Some(wanted);
                counting.waited = 0.0;
                summary.state = SummaryState::Counting;
            }
        }

        // The category drilled into, asked for the same way and settled the
        // same way. Answered whenever it differs from what was last asked,
        // which covers the category changing and the rectangle moving under it.
        match focus {
            None => {
                counting.focus_asked = None;
                counting.focus_reading = None;
                if summary.focus_state != SummaryState::Idle {
                    summary.cells.clear();
                    summary.focus_state = SummaryState::Idle;
                }
            }
            Some(focus) => {
                let wanted = (
                    region.clone(),
                    (
                        properties
                            .properties
                            .iter()
                            .map(|property| property.id.clone())
                            .collect(),
                        properties.filters(),
                        None,
                    ),
                    focus.clone(),
                );
                if counting.focus_asked.as_ref() != Some(&wanted)
                    && counting.waited == 0.0
                    && counting.asked.is_some()
                {
                    counting.focus_reading = Some(fetching(counting.service.0.cells_in(
                        properties,
                        region,
                        Some(focus),
                        CELL_PAGE,
                    )));
                    counting.focus_asked = Some(wanted);
                    summary.focus_state = SummaryState::Counting;
                }
            }
        }

        if let Some(result) = counting.focus_reading.as_mut().and_then(Fetching::take) {
            counting.focus_reading = None;
            match result {
                Ok(cells) => {
                    debug!(
                        "{} listed {} cells of a category",
                        counting.catalog,
                        cells.len()
                    );
                    summary.cells = cells;
                    summary.focus_state = SummaryState::Ready;
                }
                Err(error) => {
                    warn!(
                        "{}: could not list a category's cells: {error}",
                        counting.catalog
                    );
                    summary.focus_state = SummaryState::Failed(error);
                }
            }
        }

        let Some(result) = counting.reading.as_mut().and_then(Fetching::take) else {
            continue;
        };
        counting.reading = None;
        match result {
            Ok(counts) => {
                debug!(
                    "{} counted a selection across {} columns",
                    counting.catalog,
                    counts.values.len()
                );
                summary.values = counts.values;
                summary.counted = counting.asked.as_ref().map(|(region, _)| region.clone());
                summary.state = SummaryState::Ready;
            }
            Err(error) => {
                warn!("{}: could not count a selection: {error}", counting.catalog);
                summary.state = SummaryState::Failed(error);
            }
        }
    }
}

fn apply_counts(properties: &mut CellProperties, counts: CellCounts) {
    // Crossed against a coloring that has since moved, the mixes describe
    // colors nothing is drawn in any more, so they are kept with the column
    // they were counted against and dropped rather than shown stale.
    properties.mixes = Mixes {
        column: counts.mix_column,
        by_column: counts
            .mixes
            .into_iter()
            .map(|(column, crossed)| {
                let mut codes: HashMap<u16, Vec<(u16, u64)>> = HashMap::new();
                for (code, color, count) in crossed {
                    codes.entry(code).or_default().push((color, count));
                }
                (column, codes)
            })
            .collect(),
    };
    for (id, histogram) in counts.histograms {
        let range = properties
            .properties
            .iter_mut()
            .find(|property| property.id == id)
            .and_then(CellProperty::range_mut);
        if let Some(range) = range {
            range.histogram = histogram;
        }
    }
    for (column, counted) in counts.values {
        for value in properties
            .properties
            .iter_mut()
            .flat_map(|property| property.column_values_mut(&column))
        {
            value.count = counted
                .iter()
                .find(|(code, _)| *code == value.code)
                .map(|(_, count)| *count)
                // A value the service counted no cells of has none.
                .or(Some(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::properties::{PropertyKind, PropertyValue};
    use futures::future::BoxFuture;
    use std::sync::Arc;

    struct Down;

    impl super::super::DescribeCells for Down {
        fn describe(&self, _: CellColumns) -> BoxFuture<'static, Result<CellProperties, String>> {
            Box::pin(async { Err("503".into()) })
        }

        fn count(&self, _: &CellProperties) -> BoxFuture<'static, Result<CellCounts, String>> {
            Box::pin(async { Err("503".into()) })
        }
    }

    #[test]
    fn a_failed_description_says_so_and_can_be_retried() {
        let mut app = App::new();
        app.add_systems(Update, take_answers);
        let service = CellService(Arc::new(Down));
        let source = app
            .world_mut()
            .spawn((
                CellProperties::ready(Vec::new()),
                Describing {
                    reading: fetching(service.0.describe(CellColumns::default())),
                    service,
                    catalog: "BKP".into(),
                },
            ))
            .id();
        while app.world().get::<Described>(source).is_none() {
            app.update();
        }
        let properties = app.world().get::<CellProperties>(source).unwrap();
        assert_eq!(properties.state, PropertyState::Failed("BKP: 503".into()));
        assert_eq!(properties.provenance, Provenance::Files);

        let mut properties = properties.clone();
        retry(&mut app.world_mut().commands(), source, &mut properties);
        app.world_mut().flush();
        assert_eq!(properties.state, PropertyState::Ready);
        assert!(app.world().get::<Described>(source).is_none());
    }

    #[test]
    fn counts_land_on_the_values_they_name() {
        let mut properties = CellProperties::ready(vec![CellProperty {
            id: "braak".into(),
            name: "Braak".into(),
            shown: true,
            gene: None,
            kind: PropertyKind::Categorical(
                [0u16, 1]
                    .into_iter()
                    .map(|code| PropertyValue {
                        code,
                        label: format!("Braak {code}"),
                        color: None,
                        count: None,
                        selected: false,
                    })
                    .collect(),
            ),
        }]);
        apply_counts(
            &mut properties,
            CellCounts {
                values: vec![
                    ("braak".into(), vec![(1, 20)]),
                    ("missing".into(), vec![(0, 5)]),
                ],
                histograms: Vec::new(),
                ..CellCounts::default()
            },
        );
        let counts: Vec<Option<u64>> = properties.properties[0]
            .values()
            .iter()
            .map(|value| value.count)
            .collect();
        assert_eq!(counts, [Some(0), Some(20)]);
    }
}
