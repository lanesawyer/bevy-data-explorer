//! Categorical properties of a point cloud's cells.
//!
//! A source advertises properties by carrying [`CellProperties`]. Nothing here
//! knows where they came from: they are filled in from the dataset's own
//! metadata today, and an HTTP lookup against a separate service can replace
//! that without the sidebar or the streamers changing, because both read the
//! component rather than the source of it.
//!
//! Each property offers two things: colouring points by it, and filtering
//! points down to a chosen set of its values.

use std::collections::HashSet;

use bevy::prelude::*;

/// One value a property can take.
#[derive(Debug, Clone)]
pub struct PropertyValue {
    /// The code stored in the dataset's column for this value.
    pub code: u16,
    pub label: String,
    /// Whether points with this value are drawn.
    pub included: bool,
}

/// A categorical property, such as a cell class or dissection region.
#[derive(Debug, Clone)]
pub struct CellProperty {
    /// Column identifier, used to fetch the per-point codes.
    pub id: String,
    pub name: String,
    pub values: Vec<PropertyValue>,
}

impl CellProperty {
    /// Whether this property currently excludes anything.
    ///
    /// A property with everything included costs a column fetch per node and
    /// removes nothing, so it is worth knowing not to ask.
    pub fn restricts(&self) -> bool {
        self.values.iter().any(|value| !value.included)
    }

    pub fn included_codes(&self) -> HashSet<u16> {
        self.values
            .iter()
            .filter(|value| value.included)
            .map(|value| value.code)
            .collect()
    }
}

/// How a source's properties are coming along, so the panel can say so rather
/// than looking empty while a lookup is in flight.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PropertyState {
    #[default]
    Pending,
    Ready,
    /// Reported by the panel, but nothing produces it yet: it is here for the
    /// lookup that will fetch value labels over HTTP, which can fail in ways
    /// worth telling the user about rather than showing an empty section.
    #[expect(dead_code, reason = "part of the interface the loader will use")]
    Failed(String),
}

/// The properties a source offers, and what is currently being done with them.
#[derive(Component, Debug, Clone, Default)]
pub struct CellProperties {
    pub properties: Vec<CellProperty>,
    /// Index into `properties` of the one points are coloured by.
    pub colour_by: Option<usize>,
    pub state: PropertyState,
}

impl CellProperties {
    pub fn ready(properties: Vec<CellProperty>) -> Self {
        let colour_by = (!properties.is_empty()).then_some(0);
        CellProperties {
            properties,
            colour_by,
            state: PropertyState::Ready,
        }
    }

    /// What the streamers need in order to draw: the column to colour by, and
    /// the columns that restrict which points are drawn at all.
    ///
    /// Properties that exclude nothing are left out, so an untouched panel
    /// costs no extra fetching.
    pub fn selection(&self) -> CellSelection {
        CellSelection {
            colour_by: self
                .colour_by
                .and_then(|index| self.properties.get(index))
                .map(|property| property.id.clone()),
            filters: self
                .properties
                .iter()
                .filter(|property| property.restricts())
                .map(|property| (property.id.clone(), property.included_codes()))
                .collect(),
        }
    }
}

/// The part of [`CellProperties`] that affects what is drawn.
///
/// Compared between frames to decide whether resident points have to be built
/// again, so it holds only what changes the result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellSelection {
    pub colour_by: Option<String>,
    pub filters: Vec<(String, HashSet<u16>)>,
}

impl CellSelection {
    /// Whether a point survives the filters, given its code in each filtered
    /// column in the same order as `filters`.
    pub fn admits(&self, codes: &[u16]) -> bool {
        self.filters
            .iter()
            .zip(codes)
            .all(|((_, included), code)| included.contains(code))
    }
}

/// Stand-in properties built from a Scatterbrain's own categorical columns.
///
/// The column names and identifiers are real, so colouring by a property works
/// against the live data. The value labels are placeholders: the datasets store
/// codes, and the names behind them come from a separate service that is not
/// wired up yet. Replacing this with that lookup means writing
/// [`CellProperties`] from wherever the answer arrives — nothing that reads it
/// needs to change.
pub fn placeholder_properties(columns: &[&crate::scatterbrain::PointAttribute]) -> CellProperties {
    const SAMPLE_VALUES: usize = 6;

    let properties = columns
        .iter()
        .take(4)
        .map(|column| CellProperty {
            id: column.name.clone(),
            name: column.description.clone(),
            values: (0..SAMPLE_VALUES)
                .map(|code| PropertyValue {
                    code: code as u16,
                    label: format!("{} {code}", column.description),
                    included: true,
                })
                .collect(),
        })
        .collect();

    CellProperties::ready(properties)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn property(id: &str, codes: &[u16]) -> CellProperty {
        CellProperty {
            id: id.into(),
            name: id.into(),
            values: codes
                .iter()
                .map(|code| PropertyValue {
                    code: *code,
                    label: format!("value {code}"),
                    included: true,
                })
                .collect(),
        }
    }

    #[test]
    fn a_property_including_everything_restricts_nothing() {
        // Otherwise an untouched panel would fetch a column per property for
        // every node and throw all of it away.
        let mut property = property("class", &[0, 1, 2]);
        assert!(!property.restricts());
        property.values[1].included = false;
        assert!(property.restricts());
    }

    #[test]
    fn the_selection_leaves_out_properties_that_exclude_nothing() {
        let mut properties = CellProperties::ready(vec![
            property("class", &[0, 1]),
            property("region", &[7, 8]),
        ]);
        assert!(properties.selection().filters.is_empty());

        properties.properties[1].values[0].included = false;
        let selection = properties.selection();
        assert_eq!(selection.filters.len(), 1);
        assert_eq!(selection.filters[0].0, "region");
        assert_eq!(selection.filters[0].1, HashSet::from([8]));
    }

    #[test]
    fn colouring_defaults_to_the_first_property() {
        let properties = CellProperties::ready(vec![property("class", &[0])]);
        assert_eq!(properties.selection().colour_by.as_deref(), Some("class"));
    }

    #[test]
    fn a_source_with_no_properties_colours_by_nothing() {
        let properties = CellProperties::ready(Vec::new());
        assert_eq!(properties.colour_by, None);
        assert_eq!(properties.selection().colour_by, None);
    }

    #[test]
    fn the_placeholder_uses_the_datasets_own_column_names() {
        // Colouring has to work against the live data, so the ids must be the
        // real column identifiers even while the value labels are invented.
        let cloud = crate::scatterbrain::Scatterbrain::parse(include_str!(
            "../testdata/scatterbrain_slides.json"
        ))
        .unwrap();
        let columns = cloud.category_columns();
        let properties = placeholder_properties(&columns);

        assert!(!properties.properties.is_empty());
        assert_eq!(properties.state, PropertyState::Ready);
        for property in &properties.properties {
            assert!(
                columns.iter().any(|column| column.name == property.id),
                "{} is not a column of the dataset",
                property.id
            );
            assert!(!property.values.is_empty());
        }
    }

    #[test]
    fn placeholder_properties_start_unfiltered() {
        // Opening the panel must not silently hide anything.
        let cloud = crate::scatterbrain::Scatterbrain::parse(include_str!(
            "../testdata/scatterbrain_slides.json"
        ))
        .unwrap();
        let properties = placeholder_properties(&cloud.category_columns());
        assert!(properties.selection().filters.is_empty());
    }

    #[test]
    fn points_are_admitted_only_when_every_filter_agrees() {
        let mut properties = CellProperties::ready(vec![
            property("class", &[0, 1]),
            property("region", &[7, 8]),
        ]);
        properties.properties[0].values[1].included = false;
        properties.properties[1].values[1].included = false;
        let selection = properties.selection();

        assert!(selection.admits(&[0, 7]));
        assert!(!selection.admits(&[1, 7]));
        assert!(!selection.admits(&[0, 8]));
    }

    #[test]
    fn no_filters_admits_everything() {
        let selection = CellProperties::ready(vec![property("class", &[0, 1])]).selection();
        assert!(selection.admits(&[]));
        assert!(selection.admits(&[9]));
    }

    #[test]
    fn excluding_every_value_draws_nothing_from_that_property() {
        let mut property = property("class", &[0, 1, 2]);
        for value in &mut property.values {
            value.included = false;
        }
        assert!(property.included_codes().is_empty());
        assert!(property.restricts());
    }
}
