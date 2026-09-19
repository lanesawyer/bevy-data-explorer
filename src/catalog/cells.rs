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

use bevy::prelude::*;

use super::{Catalogs, CellCounts, CellService, Entry};
use crate::app::net::{Fetching, fetching};
use crate::source::SourceUrl;
use crate::source::properties::{
    CellColumns, CellProperties, CellProperty, Column, PropertyState, Provenance, Restriction,
};

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

/// What a count depends on: which properties there are, and how they filter.
type Counted = (Vec<String>, Vec<(Column, Restriction)>);

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
                    catalog,
                    asked: None,
                    waited: 0.0,
                    reading: None,
                });
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
                    "{} counted cells for {} properties",
                    counting.catalog,
                    counts.values.len() + counts.histograms.len()
                );
                apply_counts(&mut properties, counts);
            }
            Err(error) => warn!("{}: could not count cells: {error}", counting.catalog),
        }
    }
}

fn apply_counts(properties: &mut CellProperties, counts: CellCounts) {
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
