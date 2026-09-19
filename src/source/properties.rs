//! Categorical properties of a point cloud's cells.
//!
//! A source advertises properties by carrying [`CellProperties`]. Nothing here
//! knows where they came from: a format fills them in from the dataset's own
//! metadata, and a service that knows the dataset — the catalog it was listed
//! in — can replace them with real labels, colours and counts without the
//! sidebar or the streamers changing, because both read the component rather
//! than the source of it. [`Provenance`] records which of those it was.
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
    /// The colour the dataset's publisher gives this value, if it gives one.
    /// Values without one are coloured by [`default_colour`].
    pub colour: Option<Color>,
    /// How many cells in the whole dataset hold this value, once known.
    pub count: Option<u64>,
    /// Whether this value has been picked out as a filter.
    ///
    /// Nothing picked means the property filters nothing and every point is
    /// drawn, so an untouched panel starts with all of these clear rather than
    /// all set.
    pub selected: bool,
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
    /// Whether the panel lists this property.
    ///
    /// A dataset can advertise more properties than the sidebar can usefully
    /// hold, so the section's menu picks which of them appear. Hiding one drops
    /// its filters: a property that went on filtering with no control on screen
    /// would remove points with nothing left to explain why.
    pub shown: bool,
    pub kind: PropertyKind,
}

impl CellProperty {
    /// Whether this property currently excludes anything.
    ///
    /// A property that admits everything costs a column fetch per node and
    /// removes nothing, so it is worth knowing not to ask.
    pub fn restricts(&self) -> bool {
        match &self.kind {
            PropertyKind::Categorical(values) => values.iter().any(|value| value.selected),
            PropertyKind::Numeric(range) => range.restricts(),
        }
    }

    /// How many filters this property contributes: one per value picked out,
    /// or one for a narrowed range.
    pub fn applied(&self) -> usize {
        match &self.kind {
            PropertyKind::Categorical(values) => {
                values.iter().filter(|value| value.selected).count()
            }
            PropertyKind::Numeric(range) => usize::from(range.restricts()),
        }
    }

    /// Drop every filter on this property, leaving it drawing everything.
    pub fn clear(&mut self) {
        match &mut self.kind {
            PropertyKind::Categorical(values) => {
                for value in values {
                    value.selected = false;
                }
            }
            PropertyKind::Numeric(range) => {
                range.from = range.low;
                range.to = range.high;
            }
        }
    }

    pub fn is_categorical(&self) -> bool {
        matches!(self.kind, PropertyKind::Categorical(_))
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
                    .filter(|value| value.selected)
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

/// Where a source's properties came from, so the panel can say whether its
/// labels are the publisher's or stand-ins.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Provenance {
    /// Read from the dataset's own files, which hold codes but not their names.
    #[default]
    Files,
    /// A service is being asked; what is shown meanwhile is from the files.
    Fetching(String),
    /// Supplied by the named service.
    Service(String),
    /// The named service was asked and failed, so the files' version stands.
    Unavailable { service: String, error: String },
}

/// The columns a dataset holds for its cells, as its own files describe them.
///
/// What a service is asked to describe. It knows more about the dataset than
/// the files do, but only the files say which columns can actually be read.
#[derive(Component, Debug, Clone, Default)]
pub struct CellColumns(pub Vec<CellColumn>);

#[derive(Debug, Clone, PartialEq)]
pub struct CellColumn {
    pub id: String,
    pub name: String,
    /// One float per cell rather than one categorical code.
    pub numeric: bool,
}

/// The properties a source offers, and what is currently being done with them.
#[derive(Component, Debug, Clone, Default)]
pub struct CellProperties {
    pub properties: Vec<CellProperty>,
    /// Index into `properties` of the one points are coloured by.
    pub colour_by: Option<usize>,
    pub state: PropertyState,
    pub provenance: Provenance,
}

impl CellProperties {
    /// Colouring starts on the first listed categorical property: points are
    /// coloured by code, and a numeric column holds none.
    pub fn ready(properties: Vec<CellProperty>) -> Self {
        let colour_by = properties
            .iter()
            .position(|property| property.shown && property.is_categorical());
        CellProperties {
            properties,
            colour_by,
            state: PropertyState::Ready,
            provenance: Provenance::Files,
        }
    }

    /// Colour by the property with this id, if it is one that can colour.
    pub fn colour_by_id(&mut self, id: &str) {
        if let Some(index) = self
            .properties
            .iter()
            .position(|property| property.id == id && property.is_categorical())
        {
            self.colour_by = Some(index);
            self.properties[index].shown = true;
        }
    }

    /// Total filters applied across every property, for the control that
    /// clears them.
    pub fn applied(&self) -> usize {
        self.properties.iter().map(CellProperty::applied).sum()
    }

    /// How to name a code of the column points are currently coloured by: the
    /// property's name, and the label for that value.
    ///
    /// A code with no label — the files hold codes, and not every dataset has a
    /// service naming them — names itself rather than showing nothing.
    pub fn colour_label(&self, code: u16) -> (String, String) {
        let Some(property) = self.colour_by.and_then(|index| self.properties.get(index)) else {
            return ("value".into(), code.to_string());
        };
        let label = match &property.kind {
            PropertyKind::Categorical(values) => values
                .iter()
                .find(|value| value.code == code)
                .map(|value| value.label.clone()),
            PropertyKind::Numeric(_) => None,
        };
        (
            property.name.clone(),
            label.unwrap_or_else(|| format!("code {code}")),
        )
    }

    pub fn clear_all(&mut self) {
        for property in &mut self.properties {
            property.clear();
        }
    }

    /// List or hide one property in the panel.
    ///
    /// Hiding drops that property's filters, so nothing goes on excluding points
    /// with no control on screen to say why. The property points are coloured by
    /// cannot be hidden at all: it is what the colours on screen mean, and
    /// hiding it would take away the only control that says which property they
    /// came from.
    pub fn set_shown(&mut self, index: usize, shown: bool) {
        if !shown && self.colour_by == Some(index) {
            return;
        }
        let Some(property) = self.properties.get_mut(index) else {
            return;
        };
        property.shown = shown;
        if !shown {
            property.clear();
        }
    }

    /// What the streamers need in order to draw: the column to colour by and
    /// the colours of its codes, and the columns that restrict which points
    /// are drawn at all.
    ///
    /// Properties that exclude nothing are left out, so an untouched panel
    /// costs no extra fetching.
    pub fn selection(&self) -> CellSelection {
        let colouring = self.colour_by.and_then(|index| self.properties.get(index));
        CellSelection {
            colour_by: colouring.map(|property| property.id.clone()),
            palette: colouring.map(palette_of).unwrap_or_default(),
            filters: self
                .properties
                .iter()
                .filter(|property| property.restricts())
                .map(|property| (property.id.clone(), property.restriction()))
                .collect(),
        }
    }
}

/// A repeating categorical palette, for values nobody has chosen a colour for.
/// Codes are label indices with no inherent order, so hues are spread by a
/// golden-ratio step to keep neighbouring codes visually distinct.
pub fn default_colour(code: u16) -> Color {
    let hue = (f32::from(code) * 137.507_76) % 360.0;
    Color::hsl(hue, 0.72, 0.62)
}

impl PropertyValue {
    /// The colour points holding this value are drawn in.
    pub fn swatch(&self) -> Color {
        self.colour.unwrap_or_else(|| default_colour(self.code))
    }
}

/// Linear colours indexed by code, or nothing when no value carries a colour of
/// its own and the default palette says it all.
fn palette_of(property: &CellProperty) -> Vec<[f32; 4]> {
    let values = property.values();
    if values.iter().all(|value| value.colour.is_none()) {
        return Vec::new();
    }
    let len = values.iter().map(|value| usize::from(value.code) + 1).max();
    let mut palette: Vec<[f32; 4]> = (0..len.unwrap_or(0))
        .map(|code| linear(default_colour(code as u16)))
        .collect();
    for value in values {
        palette[usize::from(value.code)] = linear(value.swatch());
    }
    palette
}

fn linear(colour: Color) -> [f32; 4] {
    let colour = colour.to_linear();
    [colour.red, colour.green, colour.blue, 1.0]
}

/// The part of [`CellProperties`] that affects what is drawn.
///
/// Compared between frames to decide whether resident points have to be built
/// again, so it holds only what changes the result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellSelection {
    pub colour_by: Option<String>,
    /// Linear colour per code of `colour_by`. Empty, or too short for a code,
    /// means that code takes [`default_colour`].
    pub palette: Vec<[f32; 4]>,
    pub filters: Vec<(String, Restriction)>,
}

impl CellSelection {
    /// The linear colour a point with this code is drawn in.
    pub fn colour(&self, code: u16) -> [f32; 4] {
        self.palette
            .get(usize::from(code))
            .copied()
            .unwrap_or_else(|| linear(default_colour(code)))
    }

    /// Whether a point survives the filters, given its value in each filtered
    /// column in the same order as `filters`.
    pub fn admits(&self, values: &[f32]) -> bool {
        self.filters
            .iter()
            .zip(values)
            .all(|((_, restriction), value)| restriction.admits_value(*value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn categorical(id: &str, codes: &[u16]) -> CellProperty {
        CellProperty {
            id: id.into(),
            name: id.into(),
            shown: true,
            kind: PropertyKind::Categorical(
                codes
                    .iter()
                    .map(|code| PropertyValue {
                        code: *code,
                        label: format!("value {code}"),
                        colour: None,
                        count: None,
                        selected: false,
                    })
                    .collect(),
            ),
        }
    }

    fn numeric(id: &str) -> CellProperty {
        CellProperty {
            id: id.into(),
            name: id.into(),
            shown: true,
            kind: PropertyKind::Numeric(NumericRange::full(0.0, 1.0, vec![1, 2, 3, 4])),
        }
    }

    fn pick(property: &mut CellProperty, position: usize) {
        if let PropertyKind::Categorical(values) = &mut property.kind {
            values[position].selected = true;
        }
    }

    #[test]
    fn a_property_with_nothing_picked_restricts_nothing() {
        // Nothing picked means the property is not filtering, so every point is
        // drawn and no column needs fetching. Only a picked value narrows it.
        let mut property = categorical("class", &[0, 1, 2]);
        assert!(!property.restricts());
        assert_eq!(property.applied(), 0);

        pick(&mut property, 1);
        assert!(property.restricts());
        assert_eq!(property.applied(), 1);
    }

    #[test]
    fn clearing_a_property_returns_it_to_drawing_everything() {
        let mut property = categorical("class", &[0, 1, 2]);
        pick(&mut property, 0);
        pick(&mut property, 2);
        assert_eq!(property.applied(), 2);

        property.clear();
        assert!(!property.restricts());
        assert_eq!(property.applied(), 0);
    }

    #[test]
    fn clearing_a_range_returns_it_to_its_full_extent() {
        let mut property = numeric("score");
        property.range_mut().unwrap().set_end(RangeEnd::From, 0.4);
        assert_eq!(property.applied(), 1);

        property.clear();
        assert!(!property.restricts());
        let range = property.range().unwrap();
        assert_eq!((range.from, range.to), (range.low, range.high));
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

        pick(&mut properties.properties[1], 1);
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
    fn an_end_can_be_dragged_continuously_across_the_range() {
        // A drag sets the end from where the pointer is, so successive
        // positions must keep moving it rather than sticking after the first.
        let mut range = NumericRange::full(0.0, 1.0, vec![1; 8]);
        let mut last = range.to;
        for step in (0..=20).rev() {
            let fraction = step as f32 / 20.0;
            range.set_end(RangeEnd::To, range.value_at(fraction));
            assert!(range.to <= last, "the end stopped following the pointer");
            last = range.to;
        }
        assert_eq!(range.to, range.from);
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
    fn points_are_admitted_only_when_every_filter_agrees() {
        let mut properties =
            CellProperties::ready(vec![categorical("class", &[0, 1]), numeric("score")]);
        pick(&mut properties.properties[0], 0);
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
    fn an_untouched_panel_has_nothing_ticked_and_filters_nothing() {
        // The ticks mark what has been picked out, so a fresh panel shows all
        // of them clear and draws every point.
        let properties = CellProperties::ready(vec![categorical("class", &[0, 1, 2])]);
        assert!(
            properties.properties[0]
                .values()
                .iter()
                .all(|value| !value.selected)
        );
        assert_eq!(properties.applied(), 0);
        assert!(properties.selection().filters.is_empty());
    }

    #[test]
    fn hiding_a_property_drops_its_filters() {
        // A hidden property with a filter still on it would remove points with
        // no control on screen to say why.
        let mut properties = CellProperties::ready(vec![
            categorical("class", &[0, 1]),
            categorical("region", &[7, 8]),
        ]);
        pick(&mut properties.properties[1], 0);
        assert_eq!(properties.applied(), 1);

        properties.set_shown(1, false);
        assert_eq!(properties.applied(), 0);
        assert!(properties.selection().filters.is_empty());
    }

    #[test]
    fn the_coloured_property_cannot_be_hidden() {
        // The colours on screen mean whatever this property says they mean, so
        // it stays listed until something else is coloured by.
        let mut properties = CellProperties::ready(vec![
            categorical("class", &[0, 1]),
            categorical("region", &[7, 8]),
        ]);
        assert_eq!(properties.colour_by, Some(0));

        properties.set_shown(0, false);
        assert!(properties.properties[0].shown);
        assert_eq!(properties.selection().colour_by.as_deref(), Some("class"));

        // Colouring by something else releases it.
        properties.colour_by = Some(1);
        properties.set_shown(0, false);
        assert!(!properties.properties[0].shown);
    }

    #[test]
    fn hiding_every_other_property_leaves_the_coloured_one_listed() {
        let mut properties = CellProperties::ready(vec![
            categorical("class", &[0, 1]),
            categorical("region", &[7, 8]),
        ]);
        for index in 0..properties.properties.len() {
            properties.set_shown(index, false);
        }
        let listed: Vec<&str> = properties
            .properties
            .iter()
            .filter(|property| property.shown)
            .map(|property| property.id.as_str())
            .collect();
        assert_eq!(listed, ["class"]);
    }

    #[test]
    fn listing_a_property_again_leaves_the_rest_of_the_panel_alone() {
        // The menu edits the ticks and nothing else: showing one back does not
        // take colouring from the property that has it.
        let mut properties = CellProperties::ready(vec![
            categorical("class", &[0, 1]),
            categorical("region", &[7, 8]),
        ]);
        properties.set_shown(1, false);
        properties.set_shown(1, true);
        assert!(properties.properties[1].shown);
        assert_eq!(properties.colour_by, Some(0));
        assert_eq!(properties.applied(), 0);
    }

    #[test]
    fn colouring_starts_on_the_first_listed_property() {
        let mut hidden = categorical("class", &[0, 1]);
        hidden.shown = false;
        let properties = CellProperties::ready(vec![hidden, categorical("region", &[7, 8])]);
        assert_eq!(properties.selection().colour_by.as_deref(), Some("region"));
    }

    #[test]
    fn filters_are_counted_across_every_property() {
        let mut properties =
            CellProperties::ready(vec![categorical("class", &[0, 1, 2]), numeric("score")]);
        assert_eq!(properties.applied(), 0);

        pick(&mut properties.properties[0], 0);
        pick(&mut properties.properties[0], 2);
        properties.properties[1]
            .range_mut()
            .unwrap()
            .set_end(RangeEnd::To, 0.5);
        // Two values picked, and a narrowed range counting as one.
        assert_eq!(properties.applied(), 3);

        properties.clear_all();
        assert_eq!(properties.applied(), 0);
        assert!(properties.selection().filters.is_empty());
    }
}
