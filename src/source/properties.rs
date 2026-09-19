//! Categorical properties of a point cloud's cells.
//!
//! A source advertises properties by carrying [`CellProperties`]. Nothing here
//! knows where they came from: a format fills them in from the dataset's own
//! metadata, and a service that knows the dataset — the catalog it was listed
//! in — can replace them with real labels, colors and counts without the
//! sidebar or the streamers changing, because both read the component rather
//! than the source of it. [`Provenance`] records which of those it was.
//!
//! Each property offers two things: coloring points by it, and filtering
//! points down to a chosen set of its values. Categorical values are colored
//! one color each; numeric ones along a [`Gradient`].

use std::collections::HashSet;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub use super::gradient::Gradient;
pub use super::tree::{Tree, TreeLevel, TreeNode};

/// One value a property can take.
#[derive(Debug, Clone)]
pub struct PropertyValue {
    /// The code stored in the dataset's column for this value.
    pub code: u16,
    pub label: String,
    /// The color the dataset's publisher gives this value, if it gives one.
    /// Values without one are colored by [`default_color`].
    pub color: Option<Color>,
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
    #[cfg(test)]
    pub fn set_end(&mut self, end: RangeEnd, value: f32) {
        let value = value.clamp(self.low, self.high);
        match end {
            RangeEnd::From => self.from = value.min(self.to),
            RangeEnd::To => self.to = value.max(self.from),
        }
    }

    /// Move one end as a drag does, letting it pass the other.
    ///
    /// Crossing swaps the ends: the one left behind stays put as the new
    /// other end, and the returned end is the one now under the pointer, so a
    /// drag can pivot around a value and carry on past it.
    pub fn drag_end(&mut self, end: RangeEnd, value: f32) -> RangeEnd {
        let value = value.clamp(self.low, self.high);
        match end {
            RangeEnd::From if value > self.to => {
                self.from = self.to;
                self.to = value;
                RangeEnd::To
            }
            RangeEnd::To if value < self.from => {
                self.to = self.from;
                self.from = value;
                RangeEnd::From
            }
            RangeEnd::From => {
                self.from = value;
                end
            }
            RangeEnd::To => {
                self.to = value;
                end
            }
        }
    }

    /// Move the whole span so it starts at `from`, keeping its width and
    /// stopping at either edge of the data.
    pub fn slide_to(&mut self, from: f32) {
        let width = self.to - self.from;
        self.from = from.clamp(self.low, self.high - width);
        self.to = self.from + width;
    }

    /// The values one histogram bucket covers.
    pub fn bucket_span(&self, bucket: usize) -> (f32, f32) {
        let buckets = self.histogram.len().max(1) as f32;
        (
            self.value_at(bucket as f32 / buckets),
            self.value_at((bucket + 1) as f32 / buckets),
        )
    }

    /// The value at the middle of a histogram bucket, which is what decides
    /// whether the bucket counts as inside the span.
    pub fn bucket_centre(&self, bucket: usize) -> f32 {
        let (start, end) = self.bucket_span(bucket);
        f32::midpoint(start, end)
    }

    /// Cells in the buckets the span admits, and in the whole histogram.
    ///
    /// Counted by bucket, so it is as fine as the histogram and no finer.
    pub fn counts(&self) -> (u64, u64) {
        self.histogram
            .iter()
            .enumerate()
            .fold((0, 0), |(inside, total), (bucket, &count)| {
                let count = u64::from(count);
                let admitted = self.admits(self.bucket_centre(bucket));
                (inside + if admitted { count } else { 0 }, total + count)
            })
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
    /// Values nesting across several columns, ticked anywhere in the tree.
    Tree(Tree),
}

/// A property of a dataset's cells, such as a class, a region, or a
/// bootstrapping probability.
#[derive(Debug, Clone)]
pub struct CellProperty {
    /// Column identifier, used to fetch the per-point values. A tree spans
    /// several columns, named by its levels, and this names the tree.
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
    /// A gene's index in the dataset's expression files, for a property that
    /// is a gene's expression rather than a column of cell metadata.
    ///
    /// Genes are added one at a time from the genes panel and always follow
    /// the cell properties, so adding or removing one never moves those.
    pub gene: Option<u32>,
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
            PropertyKind::Tree(tree) => tree.applied() > 0,
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
            PropertyKind::Tree(tree) => tree.applied(),
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
            PropertyKind::Tree(tree) => tree.clear(),
        }
    }

    /// The column points are colored by when this property colors them.
    pub fn color_column_id(&self) -> Option<&str> {
        match &self.kind {
            PropertyKind::Numeric(_) => Some(&self.id),
            _ => self.color_column().map(|(column, _)| column),
        }
    }

    /// The values of a flat categorical property; nothing for any other kind.
    pub fn values(&self) -> &[PropertyValue] {
        match &self.kind {
            PropertyKind::Categorical(values) => values,
            _ => &[],
        }
    }

    pub fn tree(&self) -> Option<&Tree> {
        match &self.kind {
            PropertyKind::Tree(tree) => Some(tree),
            _ => None,
        }
    }

    pub fn tree_mut(&mut self) -> Option<&mut Tree> {
        match &mut self.kind {
            PropertyKind::Tree(tree) => Some(tree),
            _ => None,
        }
    }

    /// The categorical column points are colored by when this property
    /// colors them, and the values in it.
    pub fn color_column(&self) -> Option<(&str, Vec<&PropertyValue>)> {
        match &self.kind {
            PropertyKind::Categorical(values) => Some((self.id.as_str(), values.iter().collect())),
            PropertyKind::Tree(tree) => tree.levels.get(tree.color_level).map(|level| {
                (
                    level.id.as_str(),
                    tree.level_values(tree.color_level).collect(),
                )
            }),
            PropertyKind::Numeric(_) => None,
        }
    }

    /// Every categorical column this property reads, and its values.
    pub fn columns(&self) -> Vec<(&str, Vec<&PropertyValue>)> {
        match &self.kind {
            PropertyKind::Categorical(values) => vec![(self.id.as_str(), values.iter().collect())],
            PropertyKind::Tree(tree) => tree
                .levels
                .iter()
                .enumerate()
                .map(|(index, level)| (level.id.as_str(), tree.level_values(index).collect()))
                .collect(),
            PropertyKind::Numeric(_) => Vec::new(),
        }
    }

    /// The values held in `column`, wherever in this property it is.
    pub fn column_values_mut(&mut self, column: &str) -> Vec<&mut PropertyValue> {
        match &mut self.kind {
            PropertyKind::Categorical(values) if self.id == column => values.iter_mut().collect(),
            PropertyKind::Tree(tree) => {
                match tree.levels.iter().position(|level| level.id == column) {
                    Some(level) => tree.level_values_mut(level).collect(),
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    pub fn range(&self) -> Option<&NumericRange> {
        match &self.kind {
            PropertyKind::Numeric(range) => Some(range),
            _ => None,
        }
    }

    pub fn range_mut(&mut self) -> Option<&mut NumericRange> {
        match &mut self.kind {
            PropertyKind::Numeric(range) => Some(range),
            _ => None,
        }
    }

    /// Where the values of the column called `id` in this property are read.
    fn column(&self, id: &str) -> Column {
        match self.gene {
            Some(index) => Column::Gene(index),
            None => Column::Cell(id.to_string()),
        }
    }

    /// The column this property filters in, and how.
    fn restriction(&self) -> (Column, Restriction) {
        match &self.kind {
            PropertyKind::Categorical(values) => (
                self.column(&self.id),
                Restriction::Codes(
                    values
                        .iter()
                        .filter(|value| value.selected)
                        .map(|value| value.code)
                        .collect(),
                ),
            ),
            PropertyKind::Numeric(range) => (
                self.column(&self.id),
                Restriction::Span(range.from, range.to),
            ),
            PropertyKind::Tree(tree) => (
                self.column(tree.filter_column()),
                Restriction::Codes(tree.admitted()),
            ),
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
    /// The service that knows the labels was asked and failed. What the files
    /// say is kept, so points still draw, but the panel says why the labels
    /// are missing rather than passing stand-ins off as real.
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
    /// Index into `properties` of the one points are colored by.
    pub color_by: Option<usize>,
    /// The scale a numeric `color_by` is drawn along.
    pub gradient: Gradient,
    pub state: PropertyState,
    pub provenance: Provenance,
}

impl CellProperties {
    /// Coloring starts on the first listed property with values of its own
    /// to tell apart, or failing that the first listed numeric one.
    pub fn ready(properties: Vec<CellProperty>) -> Self {
        let color_by = first_to_color(&properties);
        CellProperties {
            properties,
            color_by,
            gradient: Gradient::default(),
            state: PropertyState::Ready,
            provenance: Provenance::Files,
        }
    }

    /// Color by the column with this id, if a property holds it: a
    /// categorical or numeric property, or one level of a tree.
    pub fn color_by_id(&mut self, id: &str) {
        for (index, property) in self.properties.iter_mut().enumerate() {
            let found = match &mut property.kind {
                PropertyKind::Categorical(_) | PropertyKind::Numeric(_) => property.id == id,
                PropertyKind::Tree(tree) => {
                    match tree.levels.iter().position(|level| level.id == id) {
                        Some(level) => {
                            tree.color_level = level;
                            true
                        }
                        None => false,
                    }
                }
            };
            if found {
                self.color_by = Some(index);
                property.shown = true;
                return;
            }
        }
    }

    /// Add a gene's property after the rest, unless it is already here.
    ///
    /// Coloring is left alone: a gene is as often added to filter by as to
    /// color by, and its own button colors by it.
    pub fn add_gene(&mut self, gene: CellProperty) {
        if !self
            .properties
            .iter()
            .any(|property| property.id == gene.id)
        {
            self.properties.push(gene);
        }
    }

    /// Drop a gene, and its filter with it.
    ///
    /// Coloring moves back to where it would start if the gene was what
    /// colored, and follows its property down a place if that came after it.
    pub fn remove_gene(&mut self, index: usize) {
        if self
            .properties
            .get(index)
            .is_none_or(|property| property.gene.is_none())
        {
            return;
        }
        self.properties.remove(index);
        self.color_by = match self.color_by {
            Some(colored) if colored == index => first_to_color(&self.properties),
            Some(colored) if colored > index => Some(colored - 1),
            other => other,
        };
    }

    /// The properties that are not genes, with their places.
    pub fn cell_properties(&self) -> impl Iterator<Item = (usize, &CellProperty)> {
        self.properties
            .iter()
            .enumerate()
            .filter(|(_, property)| property.gene.is_none())
    }

    /// The genes added, with their places among the properties.
    pub fn genes(&self) -> impl Iterator<Item = (usize, &CellProperty)> {
        self.properties
            .iter()
            .enumerate()
            .filter(|(_, property)| property.gene.is_some())
    }

    /// Total filters applied across every property, for the control that
    /// clears them.
    pub fn applied(&self) -> usize {
        self.properties.iter().map(CellProperty::applied).sum()
    }

    /// How to name a point's value in the column points are currently colored
    /// by: the property's name, and the label for that value.
    ///
    /// A code with no label — the files hold codes, and not every dataset has a
    /// service naming them — names itself rather than showing nothing.
    pub fn color_label(&self, shade: Shade) -> (String, String) {
        let code = match shade {
            Shade::Code(code) => code,
            Shade::Value(value) => {
                let name = self
                    .color_by
                    .and_then(|index| self.properties.get(index))
                    .map_or_else(|| "value".into(), |property| property.name.clone());
                return (name, format!("{value:.3}"));
            }
        };
        let Some(property) = self.color_by.and_then(|index| self.properties.get(index)) else {
            return ("value".into(), code.to_string());
        };
        let label = property.color_column().and_then(|(_, values)| {
            values
                .iter()
                .find(|value| value.code == code)
                .map(|value| value.label.clone())
        });
        // A tree is named by the level coloring, which is what the color says.
        let name = match property.tree() {
            Some(tree) => tree
                .levels
                .get(tree.color_level)
                .map_or_else(|| property.name.clone(), |level| level.name.clone()),
            None => property.name.clone(),
        };
        (name, label.unwrap_or_else(|| format!("code {code}")))
    }

    pub fn clear_all(&mut self) {
        for property in &mut self.properties {
            property.clear();
        }
    }

    /// List or hide one property in the panel.
    ///
    /// Hiding drops that property's filters, so nothing goes on excluding points
    /// with no control on screen to say why. The property points are colored by
    /// cannot be hidden at all: it is what the colors on screen mean, and
    /// hiding it would take away the only control that says which property they
    /// came from.
    pub fn set_shown(&mut self, index: usize, shown: bool) {
        if !shown && self.color_by == Some(index) {
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

    /// How points are colored, when they are colored by a numeric property.
    pub fn ramp(&self) -> Option<Ramp> {
        let range = self.properties.get(self.color_by?)?.range()?;
        Some(Ramp {
            gradient: self.gradient,
            from: range.from,
            to: range.to,
        })
    }

    /// What the streamers need in order to draw: the column to color by and
    /// the colors of its codes, and the columns that restrict which points
    /// are drawn at all.
    ///
    /// Properties that exclude nothing are left out, so an untouched panel
    /// costs no extra fetching.
    pub fn selection(&self) -> CellSelection {
        let property = self.color_by.and_then(|index| self.properties.get(index));
        let coloring = property.and_then(CellProperty::color_column);
        CellSelection {
            color_by: property.and_then(|property| {
                property
                    .color_column_id()
                    .map(|column| property.column(column))
            }),
            palette: coloring
                .map(|(_, values)| palette_of(&values))
                .unwrap_or_default(),
            ramp: self.ramp(),
            filters: self.filters(),
            draw_filtered: false,
        }
    }

    /// The columns that restrict which points are drawn, and how.
    pub fn filters(&self) -> Vec<(Column, Restriction)> {
        self.properties
            .iter()
            .filter(|property| property.restricts())
            .map(CellProperty::restriction)
            .collect()
    }
}

/// The property a dataset is first colored by: the first listed cell property
/// with values of its own to tell apart, or failing that the first numeric one.
fn first_to_color(properties: &[CellProperty]) -> Option<usize> {
    let listed = |numeric: bool| {
        properties.iter().position(|property| {
            property.shown
                && property.gene.is_none()
                && matches!(property.kind, PropertyKind::Numeric(_)) == numeric
        })
    };
    listed(false).or_else(|| listed(true))
}

/// A repeating categorical palette, for values nobody has chosen a color for.
/// Codes are label indices with no inherent order, so hues are spread by a
/// golden-ratio step to keep neighbouring codes visually distinct.
pub fn default_color(code: u16) -> Color {
    let hue = (f32::from(code) * 137.507_76) % 360.0;
    Color::hsl(hue, 0.72, 0.62)
}

impl PropertyValue {
    /// The color points holding this value are drawn in.
    pub fn swatch(&self) -> Color {
        self.color.unwrap_or_else(|| default_color(self.code))
    }
}

/// Linear colors indexed by code, or nothing when no value carries a color of
/// its own and the default palette says it all.
fn palette_of(values: &[&PropertyValue]) -> Vec<[f32; 4]> {
    if values.iter().all(|value| value.color.is_none()) {
        return Vec::new();
    }
    let len = values.iter().map(|value| usize::from(value.code) + 1).max();
    let mut palette: Vec<[f32; 4]> = (0..len.unwrap_or(0))
        .map(|code| linear(default_color(code as u16)))
        .collect();
    for value in values {
        palette[usize::from(value.code)] = linear(value.swatch());
    }
    palette
}

fn linear(color: Color) -> [f32; 4] {
    let color = color.to_linear();
    [color.red, color.green, color.blue, 1.0]
}

/// What a point is colored by: its code in a categorical column, or its value
/// in a numeric one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shade {
    Code(u16),
    Value(f32),
}

impl Shade {
    /// The categorical code, which is what hovering a point highlights the
    /// rest of. Values along a gradient form no groups to highlight.
    pub fn code(self) -> Option<u16> {
        match self {
            Shade::Code(code) => Some(code),
            Shade::Value(_) => None,
        }
    }
}

/// How a numeric column is colored: along `gradient`, from its start at
/// `from` to its end at `to`.
///
/// The span is the one the property admits, so narrowing a range spreads the
/// whole gradient over what is left rather than drawing it in a sliver of one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ramp {
    pub gradient: Gradient,
    pub from: f32,
    pub to: f32,
}

impl Ramp {
    /// Steps the gradient is sampled at, which is as fine as the eight bits a
    /// point's color is packed into can show.
    const STEPS: usize = 256;

    pub fn fraction_of(&self, value: f32) -> f32 {
        let span = self.to - self.from;
        if span.abs() < f32::EPSILON {
            return 0.5;
        }
        (value - self.from) / span
    }

    /// The linear color of each value, sampling the gradient once per step
    /// rather than once per point.
    pub fn colors(&self, values: &[f32]) -> Vec<[f32; 4]> {
        let table: Vec<[f32; 4]> = (0..Self::STEPS)
            .map(|step| linear(self.gradient.sample(step as f32 / (Self::STEPS - 1) as f32)))
            .collect();
        let last = (Self::STEPS - 1) as f32;
        values
            .iter()
            .map(|value| {
                let fraction = self.fraction_of(*value);
                if fraction.is_nan() {
                    return MISSING;
                }
                table[(fraction.clamp(0.0, 1.0) * last).round() as usize]
            })
            .collect()
    }
}

/// The light gray filtered-out points are drawn in unless the user picks
/// another.
pub const FILTERED_GRAY: Color = Color::srgb(0.88, 0.88, 0.88);

/// What becomes of the points a source's filters leave out: drawn beneath the
/// rest in `color`, or not drawn at all.
///
/// On the source rather than in [`CellProperties`], which a service replaces
/// wholesale when it describes the cells.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct FilteredPoints {
    pub shown: bool,
    pub color: Color,
}

impl Default for FilteredPoints {
    fn default() -> Self {
        FilteredPoints {
            shown: true,
            color: FILTERED_GRAY,
        }
    }
}

impl FilteredPoints {
    pub fn saved(&self) -> SavedFiltered {
        let color = self.color.to_srgba();
        SavedFiltered {
            shown: self.shown,
            color: [color.red, color.green, color.blue],
        }
    }
}

/// [`FilteredPoints`] as the preferences and bookmarks write it, its color in
/// sRGB.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct SavedFiltered {
    pub shown: bool,
    pub color: [f32; 3],
}

impl Default for SavedFiltered {
    fn default() -> Self {
        FilteredPoints::default().saved()
    }
}

impl SavedFiltered {
    /// Clamped, since a file edited by hand can say anything.
    pub fn restored(&self) -> FilteredPoints {
        let [r, g, b] = self.color.map(|channel| channel.clamp(0.0, 1.0));
        FilteredPoints {
            shown: self.shown,
            color: Color::srgb(r, g, b),
        }
    }
}

/// The linear color of a point with no value to color it by.
pub const MISSING: [f32; 4] = [0.8, 0.85, 0.9, 1.0];

/// The part of [`CellProperties`] that affects what is drawn.
///
/// Compared between frames to decide whether resident points have to be built
/// again, so it holds only what changes the result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellSelection {
    pub color_by: Option<Column>,
    /// Linear color per code of `color_by`. Empty, or too short for a code,
    /// means that code takes [`default_color`].
    pub palette: Vec<[f32; 4]>,
    /// Set when `color_by` is numeric, and how its values are colored.
    pub ramp: Option<Ramp>,
    pub filters: Vec<(Column, Restriction)>,
    /// Whether points the filters leave out are drawn beneath the rest rather
    /// than dropped. Only set while something is filtered, so turning it on
    /// with no filters rebuilds nothing. Their color is not here: it is a
    /// uniform, so picking one rebuilds nothing either.
    pub draw_filtered: bool,
}

/// Where a column of per-cell values is read from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Column {
    /// A column of the cell metadata, by its id.
    Cell(String),
    /// A gene's expression, by its index in the dataset's expression files.
    Gene(u32),
}

impl Column {
    /// The id of a cell metadata column; nothing for a gene.
    #[cfg(test)]
    pub fn cell(&self) -> Option<&str> {
        match self {
            Column::Cell(id) => Some(id),
            Column::Gene(_) => None,
        }
    }
}

impl CellSelection {
    /// Draw filtered-out points as `filtered` says, if the source says at all.
    pub fn with_filtered(mut self, filtered: Option<&FilteredPoints>) -> Self {
        self.draw_filtered =
            filtered.is_some_and(|filtered| filtered.shown) && !self.filters.is_empty();
        self
    }

    /// The linear color a point with this code is drawn in.
    pub fn color(&self, code: u16) -> [f32; 4] {
        self.palette
            .get(usize::from(code))
            .copied()
            .unwrap_or_else(|| linear(default_color(code)))
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
            gene: None,
            kind: PropertyKind::Categorical(
                codes
                    .iter()
                    .map(|code| PropertyValue {
                        code: *code,
                        label: format!("value {code}"),
                        color: None,
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
            gene: None,
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
        assert_eq!(selection.filters[0].0, Column::Cell("region".into()));
        assert_eq!(
            selection.filters[0].1,
            Restriction::Codes(HashSet::from([8]))
        );
    }

    #[test]
    fn coloring_defaults_to_the_first_property() {
        let properties = CellProperties::ready(vec![categorical("class", &[0])]);
        assert_eq!(
            properties
                .selection()
                .color_by
                .as_ref()
                .and_then(Column::cell),
            Some("class")
        );
    }

    #[test]
    fn a_numeric_property_colors_along_its_admitted_span() {
        let mut properties =
            CellProperties::ready(vec![categorical("class", &[0, 1]), numeric("score")]);
        assert_eq!(properties.selection().ramp, None, "a class colors by code");

        properties.color_by_id("score");
        properties.properties[1]
            .range_mut()
            .unwrap()
            .set_end(RangeEnd::From, 0.5);
        let selection = properties.selection();
        assert_eq!(
            selection.color_by.as_ref().and_then(Column::cell),
            Some("score")
        );
        assert!(selection.palette.is_empty());
        let ramp = selection
            .ramp
            .expect("a numeric property colors along a gradient");
        assert_eq!((ramp.from, ramp.to), (0.5, 1.0));
        assert_eq!(ramp.gradient, Gradient::Viridis);
    }

    #[test]
    fn coloring_prefers_a_property_with_values_to_tell_apart() {
        let properties =
            CellProperties::ready(vec![numeric("score"), categorical("class", &[0, 1])]);
        assert_eq!(
            properties
                .selection()
                .color_by
                .as_ref()
                .and_then(Column::cell),
            Some("class")
        );
        // With nothing else to go on, a numeric one still colors.
        let properties = CellProperties::ready(vec![numeric("score")]);
        assert_eq!(
            properties
                .selection()
                .color_by
                .as_ref()
                .and_then(Column::cell),
            Some("score")
        );
    }

    #[test]
    fn a_ramp_spans_its_gradient_and_marks_what_has_no_value() {
        let ramp = Ramp {
            gradient: Gradient::Viridis,
            from: 10.0,
            to: 20.0,
        };
        let colors = ramp.colors(&[10.0, 20.0, 5.0, 25.0, f32::NAN]);
        assert_eq!(colors[0], linear(Gradient::Viridis.sample(0.0)));
        assert_eq!(colors[1], linear(Gradient::Viridis.sample(1.0)));
        // Outside the span clamps to its ends.
        assert_eq!(colors[2], colors[0]);
        assert_eq!(colors[3], colors[1]);
        assert_eq!(colors[4], MISSING);
    }

    #[test]
    fn a_numeric_value_is_labelled_by_its_property() {
        let mut properties = CellProperties::ready(vec![numeric("score")]);
        properties.properties[0].name = "Score".into();
        assert_eq!(
            properties.color_label(Shade::Value(0.25)),
            ("Score".to_string(), "0.250".to_string())
        );
    }

    fn gene(id: &str, index: u32) -> CellProperty {
        CellProperty {
            gene: Some(index),
            ..numeric(id)
        }
    }

    #[test]
    fn a_gene_is_read_from_the_expression_files() {
        let mut properties = CellProperties::ready(vec![categorical("class", &[0, 1])]);
        properties.add_gene(gene("ENSG1", 12));
        assert_eq!(
            properties.color_by,
            Some(0),
            "adding a gene leaves coloring"
        );
        properties.color_by = Some(1);
        properties.properties[1]
            .range_mut()
            .unwrap()
            .set_end(RangeEnd::From, 0.5);
        let selection = properties.selection();
        assert_eq!(selection.color_by, Some(Column::Gene(12)));
        assert_eq!(selection.filters[0].0, Column::Gene(12));
    }

    #[test]
    fn adding_a_gene_twice_keeps_one() {
        let mut properties = CellProperties::ready(vec![categorical("class", &[0, 1])]);
        properties.add_gene(gene("ENSG1", 12));
        properties.add_gene(gene("ENSG1", 12));
        assert_eq!(properties.genes().count(), 1);
    }

    #[test]
    fn removing_the_colored_gene_colors_as_the_dataset_began() {
        let mut properties = CellProperties::ready(vec![
            categorical("class", &[0, 1]),
            categorical("region", &[7, 8]),
        ]);
        properties.add_gene(gene("ENSG1", 1));
        properties.add_gene(gene("ENSG2", 2));
        properties.color_by = Some(3);

        // Coloring follows its gene down a place when one before it goes.
        properties.remove_gene(2);
        assert_eq!(properties.color_by, Some(2));
        assert_eq!(properties.properties[2].id, "ENSG2");

        properties.remove_gene(2);
        assert_eq!(properties.color_by, Some(0));
        assert_eq!(properties.genes().count(), 0);
    }

    #[test]
    fn only_genes_can_be_removed() {
        let mut properties = CellProperties::ready(vec![categorical("class", &[0, 1])]);
        properties.remove_gene(0);
        assert_eq!(properties.properties.len(), 1);
    }

    #[test]
    fn a_gene_is_never_what_a_dataset_starts_colored_by() {
        let properties = CellProperties::ready(vec![gene("ENSG1", 1), numeric("score")]);
        assert_eq!(properties.color_by, Some(1));
        assert_eq!(properties.cell_properties().count(), 1);
    }

    #[test]
    fn a_source_with_no_properties_colors_by_nothing() {
        let properties = CellProperties::ready(Vec::new());
        assert_eq!(properties.color_by, None);
        assert_eq!(properties.selection().color_by, None);
    }

    #[test]
    fn an_end_can_be_dragged_continuously_across_the_range() {
        // A drag sets the end from where the pointer is, so successive
        // positions must keep moving it rather than sticking after the first.
        let mut range = NumericRange::full(0.0, 1.0, vec![1; 8]);
        let mut last = range.to;
        for step in (0..=20).rev() {
            let fraction = step as f32 / 20.0;
            range.drag_end(RangeEnd::To, range.value_at(fraction));
            assert!(range.to <= last, "the end stopped following the pointer");
            last = range.to;
        }
        assert_eq!(range.to, range.from);
    }

    #[test]
    fn a_dragged_end_swaps_with_the_other_when_it_crosses() {
        let mut range = NumericRange::full(0.0, 10.0, vec![1]);
        range.set_end(RangeEnd::From, 4.0);
        range.set_end(RangeEnd::To, 6.0);

        let now = range.drag_end(RangeEnd::From, 8.0);
        assert_eq!(now, RangeEnd::To, "the pointer now holds the upper end");
        assert_eq!((range.from, range.to), (6.0, 8.0));

        let now = range.drag_end(now, 2.0);
        assert_eq!(now, RangeEnd::From);
        assert_eq!((range.from, range.to), (2.0, 6.0));

        assert_eq!(range.drag_end(RangeEnd::From, -3.0), RangeEnd::From);
        assert_eq!(range.from, 0.0, "an end may not leave the data's extent");
    }

    #[test]
    fn a_span_slides_whole_and_stops_at_the_edges() {
        let mut range = NumericRange::full(0.0, 10.0, vec![1]);
        range.set_end(RangeEnd::From, 2.0);
        range.set_end(RangeEnd::To, 5.0);

        range.slide_to(4.0);
        assert_eq!((range.from, range.to), (4.0, 7.0));
        range.slide_to(9.0);
        assert_eq!((range.from, range.to), (7.0, 10.0));
        range.slide_to(-2.0);
        assert_eq!((range.from, range.to), (0.0, 3.0));
    }

    #[test]
    fn a_bucket_spans_its_share_of_the_extent_and_counts_when_picked() {
        let mut range = NumericRange::full(0.0, 8.0, vec![5, 3, 2, 7]);
        assert_eq!(range.bucket_span(1), (2.0, 4.0));
        assert_eq!(range.counts(), (17, 17));

        let (from, to) = range.bucket_span(1);
        range.from = from;
        range.to = to;
        assert_eq!(range.counts(), (3, 17), "only the picked bucket is inside");
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
    fn filtered_out_points_are_muted_only_while_something_is_filtered() {
        let mut properties = CellProperties::ready(vec![categorical("class", &[0, 1])]);
        let shown = FilteredPoints::default();
        // Nothing filtered: turning it on must not rebuild.
        assert!(
            !properties
                .selection()
                .with_filtered(Some(&shown))
                .draw_filtered
        );

        pick(&mut properties.properties[0], 0);
        let selection = properties.selection();
        assert!(selection.clone().with_filtered(Some(&shown)).draw_filtered);
        let hidden = FilteredPoints {
            shown: false,
            ..shown
        };
        assert!(!selection.clone().with_filtered(Some(&hidden)).draw_filtered);
        assert!(!selection.with_filtered(None).draw_filtered);
    }

    #[test]
    fn a_saved_filtered_color_is_clamped() {
        let saved = SavedFiltered {
            shown: true,
            color: [2.0, -1.0, 0.5],
        };
        let color = saved.restored().color.to_srgba();
        assert_eq!((color.red, color.green, color.blue), (1.0, 0.0, 0.5));
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
    fn the_colored_property_cannot_be_hidden() {
        // The colors on screen mean whatever this property says they mean, so
        // it stays listed until something else is colored by.
        let mut properties = CellProperties::ready(vec![
            categorical("class", &[0, 1]),
            categorical("region", &[7, 8]),
        ]);
        assert_eq!(properties.color_by, Some(0));

        properties.set_shown(0, false);
        assert!(properties.properties[0].shown);
        assert_eq!(
            properties
                .selection()
                .color_by
                .as_ref()
                .and_then(Column::cell),
            Some("class")
        );

        // Coloring by something else releases it.
        properties.color_by = Some(1);
        properties.set_shown(0, false);
        assert!(!properties.properties[0].shown);
    }

    #[test]
    fn hiding_every_other_property_leaves_the_colored_one_listed() {
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
        // take coloring from the property that has it.
        let mut properties = CellProperties::ready(vec![
            categorical("class", &[0, 1]),
            categorical("region", &[7, 8]),
        ]);
        properties.set_shown(1, false);
        properties.set_shown(1, true);
        assert!(properties.properties[1].shown);
        assert_eq!(properties.color_by, Some(0));
        assert_eq!(properties.applied(), 0);
    }

    #[test]
    fn coloring_starts_on_the_first_listed_property() {
        let mut hidden = categorical("class", &[0, 1]);
        hidden.shown = false;
        let properties = CellProperties::ready(vec![hidden, categorical("region", &[7, 8])]);
        assert_eq!(
            properties
                .selection()
                .color_by
                .as_ref()
                .and_then(Column::cell),
            Some("region")
        );
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
