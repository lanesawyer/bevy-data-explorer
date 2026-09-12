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

/// A continuous property, filtered by a range rather than a set of values.
#[derive(Debug, Clone, PartialEq)]
pub struct NumericRange {
    /// Full extent of the data, the bounds the control can reach.
    pub low: f32,
    pub high: f32,
    /// The span currently admitted, within `low..=high`.
    pub from: f32,
    pub to: f32,
    /// Counts per equal-width bucket across `low..=high`, drawn as a histogram
    /// so the range can be chosen against the shape of the data.
    pub histogram: Vec<u32>,
}

impl NumericRange {
    pub fn full(low: f32, high: f32, histogram: Vec<u32>) -> Self {
        NumericRange {
            low,
            high,
            from: low,
            to: high,
            histogram,
        }
    }

    /// Whether the chosen span is narrower than the data's full extent.
    pub fn restricts(&self) -> bool {
        self.from > self.low || self.to < self.high
    }

    pub fn admits(&self, value: f32) -> bool {
        value >= self.from && value <= self.to
    }

    /// Where a bound sits across the full extent, as a fraction.
    pub fn fraction_of(&self, value: f32) -> f32 {
        let span = self.high - self.low;
        if span.abs() < f32::EPSILON {
            return 0.0;
        }
        ((value - self.low) / span).clamp(0.0, 1.0)
    }

    /// The value a fraction of the way across the full extent.
    pub fn value_at(&self, fraction: f32) -> f32 {
        self.low + (self.high - self.low) * fraction.clamp(0.0, 1.0)
    }

    /// Move one end, keeping it on its own side of the other.
    pub fn set_end(&mut self, end: RangeEnd, value: f32) {
        let value = value.clamp(self.low, self.high);
        match end {
            RangeEnd::From => self.from = value.min(self.to),
            RangeEnd::To => self.to = value.max(self.from),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeEnd {
    From,
    To,
}

/// What a property is filtered by.
#[derive(Debug, Clone)]
pub enum PropertyKind {
    /// A fixed set of labelled values, ticked individually.
    Categorical(Vec<PropertyValue>),
    /// A continuous span, chosen between two ends.
    Numeric(NumericRange),
}

/// A property of a dataset's cells, such as a class, a region, or a
/// bootstrapping probability.
#[derive(Debug, Clone)]
pub struct CellProperty {
    /// Column identifier, used to fetch the per-point values.
    pub id: String,
    pub name: String,
    pub kind: PropertyKind,
}

impl CellProperty {
    /// Whether this property currently excludes anything.
    ///
    /// A property that admits everything costs a column fetch per node and
    /// removes nothing, so it is worth knowing not to ask.
    pub fn restricts(&self) -> bool {
        match &self.kind {
            PropertyKind::Categorical(values) => values.iter().any(|value| !value.included),
            PropertyKind::Numeric(range) => range.restricts(),
        }
    }

    pub fn values(&self) -> &[PropertyValue] {
        match &self.kind {
            PropertyKind::Categorical(values) => values,
            PropertyKind::Numeric(_) => &[],
        }
    }

    pub fn range(&self) -> Option<&NumericRange> {
        match &self.kind {
            PropertyKind::Numeric(range) => Some(range),
            PropertyKind::Categorical(_) => None,
        }
    }

    pub fn range_mut(&mut self) -> Option<&mut NumericRange> {
        match &mut self.kind {
            PropertyKind::Numeric(range) => Some(range),
            PropertyKind::Categorical(_) => None,
        }
    }

    fn restriction(&self) -> Restriction {
        match &self.kind {
            PropertyKind::Categorical(values) => Restriction::Codes(
                values
                    .iter()
                    .filter(|value| value.included)
                    .map(|value| value.code)
                    .collect(),
            ),
            PropertyKind::Numeric(range) => Restriction::Span(range.from, range.to),
        }
    }
}

/// How a filtered column restricts the points drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Restriction {
    /// Categorical codes that are admitted.
    Codes(HashSet<u16>),
    /// An inclusive numeric span.
    Span(f32, f32),
}

impl Restriction {
    /// Whether a point's value in this column is admitted.
    ///
    /// Categorical columns hold codes and numeric ones hold floats; the reader
    /// decodes each to a float, so codes arrive here exactly representable.
    pub fn admits_value(&self, value: f32) -> bool {
        match self {
            Restriction::Codes(codes) => codes.contains(&(value as u16)),
            Restriction::Span(from, to) => value >= *from && value <= *to,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Restriction::Span(..))
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
                .map(|property| (property.id.clone(), property.restriction()))
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
    pub filters: Vec<(String, Restriction)>,
}

impl CellSelection {
    /// Whether a point survives the filters, given its value in each filtered
    /// column in the same order as `filters`.
    pub fn admits(&self, values: &[f32]) -> bool {
        self.filters
            .iter()
            .zip(values)
            .all(|((_, restriction), value)| restriction.admits_value(*value))
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
pub fn placeholder_properties(
    categorical: &[&crate::scatterbrain::PointAttribute],
    numeric: &[&crate::scatterbrain::PointAttribute],
) -> CellProperties {
    const SAMPLE_VALUES: usize = 6;
    const SHOWN: usize = 4;

    let mut properties: Vec<CellProperty> = categorical
        .iter()
        .take(SHOWN)
        .map(|column| CellProperty {
            id: column.name.clone(),
            name: column.description.clone(),
            kind: PropertyKind::Categorical(
                (0..SAMPLE_VALUES)
                    .map(|code| PropertyValue {
                        code: code as u16,
                        label: format!("{} {code}", column.description),
                        included: true,
                    })
                    .collect(),
            ),
        })
        .collect();

    properties.extend(numeric.iter().take(SHOWN).map(|column| CellProperty {
        id: column.name.clone(),
        name: column.description.clone(),
        kind: PropertyKind::Numeric(NumericRange::full(0.0, 1.0, placeholder_histogram())),
    }));

    CellProperties::ready(properties)
}

/// A stand-in distribution, skewed high the way a confidence score tends to be.
///
/// Real counts have to be binned over the whole dataset, which is a question
/// for the service that will supply the labels rather than something to compute
/// from the nodes that happen to be resident.
fn placeholder_histogram() -> Vec<u32> {
    const BUCKETS: usize = 24;
    (0..BUCKETS)
        .map(|bucket| {
            let t = bucket as f32 / (BUCKETS - 1) as f32;
            let weight = 0.05 + t.powi(3);
            (weight * 900.0) as u32 + 12
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn categorical(id: &str, codes: &[u16]) -> CellProperty {
        CellProperty {
            id: id.into(),
            name: id.into(),
            kind: PropertyKind::Categorical(
                codes
                    .iter()
                    .map(|code| PropertyValue {
                        code: *code,
                        label: format!("value {code}"),
                        included: true,
                    })
                    .collect(),
            ),
        }
    }

    fn numeric(id: &str) -> CellProperty {
        CellProperty {
            id: id.into(),
            name: id.into(),
            kind: PropertyKind::Numeric(NumericRange::full(0.0, 1.0, vec![1, 2, 3, 4])),
        }
    }

    fn exclude(property: &mut CellProperty, position: usize) {
        if let PropertyKind::Categorical(values) = &mut property.kind {
            values[position].included = false;
        }
    }

    #[test]
    fn a_property_including_everything_restricts_nothing() {
        // Otherwise an untouched panel would fetch a column per property for
        // every node and throw all of it away.
        let mut property = categorical("class", &[0, 1, 2]);
        assert!(!property.restricts());
        exclude(&mut property, 1);
        assert!(property.restricts());
    }

    #[test]
    fn a_full_numeric_span_restricts_nothing() {
        let mut property = numeric("score");
        assert!(!property.restricts());
        property.range_mut().unwrap().set_end(RangeEnd::From, 0.2);
        assert!(property.restricts());
    }

    #[test]
    fn the_selection_leaves_out_properties_that_exclude_nothing() {
        let mut properties = CellProperties::ready(vec![
            categorical("class", &[0, 1]),
            categorical("region", &[7, 8]),
        ]);
        assert!(properties.selection().filters.is_empty());

        exclude(&mut properties.properties[1], 0);
        let selection = properties.selection();
        assert_eq!(selection.filters.len(), 1);
        assert_eq!(selection.filters[0].0, "region");
        assert_eq!(
            selection.filters[0].1,
            Restriction::Codes(HashSet::from([8]))
        );
    }

    #[test]
    fn colouring_defaults_to_the_first_property() {
        let properties = CellProperties::ready(vec![categorical("class", &[0])]);
        assert_eq!(properties.selection().colour_by.as_deref(), Some("class"));
    }

    #[test]
    fn a_source_with_no_properties_colours_by_nothing() {
        let properties = CellProperties::ready(Vec::new());
        assert_eq!(properties.colour_by, None);
        assert_eq!(properties.selection().colour_by, None);
    }

    #[test]
    fn the_ends_of_a_range_cannot_cross() {
        // Dragging one past the other would otherwise admit nothing at all,
        // with no way to tell from the control why.
        let mut range = NumericRange::full(0.0, 10.0, vec![1]);
        range.set_end(RangeEnd::From, 8.0);
        range.set_end(RangeEnd::To, 3.0);
        assert!(range.from <= range.to);

        range.set_end(RangeEnd::To, 20.0);
        assert_eq!(range.to, 10.0, "an end may not leave the data's extent");
        range.set_end(RangeEnd::From, -5.0);
        assert_eq!(range.from, 0.0);
    }

    #[test]
    fn a_range_maps_between_values_and_fractions() {
        let range = NumericRange::full(10.0, 20.0, vec![1]);
        assert_eq!(range.fraction_of(15.0), 0.5);
        assert_eq!(range.value_at(0.5), 15.0);
        // Out of range on either side clamps rather than extrapolating.
        assert_eq!(range.fraction_of(0.0), 0.0);
        assert_eq!(range.value_at(2.0), 20.0);
    }

    #[test]
    fn a_degenerate_range_does_not_divide_by_zero() {
        let range = NumericRange::full(5.0, 5.0, vec![1]);
        assert_eq!(range.fraction_of(5.0), 0.0);
    }

    #[test]
    fn the_placeholder_uses_the_datasets_own_columns() {
        // Colouring has to work against live data, so the ids must be real
        // column identifiers even while the labels and counts are invented.
        let cloud = crate::scatterbrain::Scatterbrain::parse(include_str!(
            "../testdata/scatterbrain_cells.json"
        ))
        .unwrap();
        let categorical = cloud.category_columns();
        let numeric = cloud.numeric_columns();
        assert!(!numeric.is_empty(), "this dataset has float columns");

        let properties = placeholder_properties(&categorical, &numeric);
        assert_eq!(properties.state, PropertyState::Ready);

        let known: Vec<&str> = categorical
            .iter()
            .chain(numeric.iter())
            .map(|column| column.name.as_str())
            .collect();
        for property in &properties.properties {
            assert!(known.contains(&property.id.as_str()));
        }
        assert!(
            properties
                .properties
                .iter()
                .any(|property| property.range().is_some()),
            "numeric columns should surface as ranges"
        );
    }

    #[test]
    fn placeholder_properties_start_unfiltered() {
        // Opening the panel must not silently hide anything.
        let cloud = crate::scatterbrain::Scatterbrain::parse(include_str!(
            "../testdata/scatterbrain_cells.json"
        ))
        .unwrap();
        let properties =
            placeholder_properties(&cloud.category_columns(), &cloud.numeric_columns());
        assert!(properties.selection().filters.is_empty());
    }

    #[test]
    fn points_are_admitted_only_when_every_filter_agrees() {
        let mut properties =
            CellProperties::ready(vec![categorical("class", &[0, 1]), numeric("score")]);
        exclude(&mut properties.properties[0], 1);
        properties.properties[1]
            .range_mut()
            .unwrap()
            .set_end(RangeEnd::From, 0.5);
        let selection = properties.selection();

        assert!(selection.admits(&[0.0, 0.8]));
        assert!(!selection.admits(&[1.0, 0.8]), "excluded class");
        assert!(!selection.admits(&[0.0, 0.2]), "below the range");
    }

    #[test]
    fn no_filters_admits_everything() {
        let selection = CellProperties::ready(vec![categorical("class", &[0, 1])]).selection();
        assert!(selection.admits(&[]));
        assert!(selection.admits(&[9.0]));
    }

    #[test]
    fn excluding_every_value_draws_nothing_from_that_property() {
        let mut property = categorical("class", &[0, 1, 2]);
        if let PropertyKind::Categorical(values) = &mut property.kind {
            for value in values {
                value.included = false;
            }
        }
        assert!(property.restricts());
        assert_eq!(property.restriction(), Restriction::Codes(HashSet::new()));
    }
}
