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
//! rather than holding them back.

use bevy::prelude::*;

use super::{Catalogs, CellCounts, CellService, Entry};
use crate::app::net::{Fetching, fetching};
use crate::source::SourceUrl;
use crate::source::properties::{CellColumns, CellProperties, Provenance};

/// A question in flight to the service describing a source's cells.
#[derive(Component)]
pub enum Describing {
    Labels {
        service: CellService,
        catalog: String,
        reading: Fetching<Result<CellProperties, String>>,
    },
    Counts {
        catalog: String,
        reading: Fetching<Result<CellCounts, String>>,
    },
}

/// Marks a source whose properties wait on no service any more, whether one
/// answered, failed, or there was none to ask.
#[derive(Component)]
pub struct Described;

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
                commands.entity(entity).insert(Describing::Labels {
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

/// Take in whatever the services have answered.
pub fn take_answers(
    mut commands: Commands,
    mut sources: Query<(Entity, &mut Describing, &mut CellProperties)>,
) {
    for (entity, mut describing, mut properties) in &mut sources {
        let next = match &mut *describing {
            Describing::Labels {
                service,
                catalog,
                reading,
            } => {
                let Some(result) = reading.take() else {
                    continue;
                };
                match result {
                    Ok(mut described) => {
                        info!(
                            "{catalog} described {} cell properties",
                            described.properties.len()
                        );
                        described.provenance = Provenance::Service(catalog.clone());
                        let counting = fetching(service.0.count(&described));
                        *properties = described;
                        Some(Describing::Counts {
                            catalog: catalog.clone(),
                            reading: counting,
                        })
                    }
                    Err(error) => {
                        warn!("{catalog}: could not describe cells: {error}");
                        properties.provenance = Provenance::Unavailable {
                            service: catalog.clone(),
                            error,
                        };
                        None
                    }
                }
            }
            Describing::Counts { catalog, reading } => {
                let Some(result) = reading.take() else {
                    continue;
                };
                match result {
                    Ok(counts) => {
                        info!("{catalog} counted cells for {} properties", counts.len());
                        apply_counts(&mut properties, counts);
                    }
                    Err(error) => warn!("{catalog}: could not count cells: {error}"),
                }
                None
            }
        };
        match next {
            Some(next) => *describing = next,
            None => {
                commands
                    .entity(entity)
                    .remove::<Describing>()
                    .insert(Described);
            }
        }
    }
}

fn apply_counts(properties: &mut CellProperties, counts: CellCounts) {
    for (column, counted) in counts {
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
    use crate::source::properties::{CellProperty, PropertyKind, PropertyValue};

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
            vec![
                ("braak".into(), vec![(1, 20)]),
                ("missing".into(), vec![(0, 5)]),
            ],
        );
        let counts: Vec<Option<u64>> = properties.properties[0]
            .values()
            .iter()
            .map(|value| value.count)
            .collect();
        assert_eq!(counts, [Some(0), Some(20)]);
    }
}
