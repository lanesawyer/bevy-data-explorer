//! What a bookmark holds, and reading it off and writing it back onto the
//! components it describes.
//!
//! Everything is named the way it would be named to someone else: a dataset by
//! its address, a channel by its label, a cell property by its column. Entity
//! ids, render layers and indices into a list the dataset may reorder mean
//! nothing in another process, so none of them is saved.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::render::points::{MAX_POINT_PX, MIN_POINT_PX};
use crate::source::channels::{MAX_GAIN, SourceChannels};
use crate::source::genes::Gene;
use crate::source::properties::{CellProperties, PropertyKind};
use crate::source::stack::SliceStack;

/// The format a bookmark is written in. Raised whenever a field changes
/// meaning; a field merely added is read as its default by older files.
pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Bookmark {
    pub version: u32,
    pub name: String,
    /// Seconds since the Unix epoch, for ordering a list of them.
    #[serde(default)]
    pub created: u64,
    /// Every dataset a frame or layer shows, each once.
    pub sources: Vec<SourceState>,
    /// The frames, in grid order.
    pub frames: Vec<FrameState>,
    /// Index into `frames` of the frame the sidebar was acting on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<usize>,
}

/// One dataset, and how it was being shown. Settings the dataset does not
/// offer are absent rather than defaulted, so restoring leaves them alone.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SourceState {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice_grid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cells: Option<CellsState>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ChannelState {
    pub label: String,
    pub shown: bool,
    pub gain: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct CellsState {
    /// The column points are colored by: a categorical property's id, or the
    /// id of the tree level that colors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_by: Option<String>,
    pub properties: Vec<PropertySetting>,
    /// Genes added to the properties. Their settings, like any property's,
    /// are in `properties` under the gene's id; these say how to add them
    /// back first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub genes: Vec<SavedGene>,
}

/// A gene as it was added, whole, so it can be added again without being
/// searched for. Its histogram is not saved: it is the service's to count.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SavedGene {
    pub id: String,
    pub symbol: String,
    /// Where its values sit in the dataset's expression files.
    pub index: u32,
    pub high: f32,
}

impl From<&SavedGene> for Gene {
    fn from(saved: &SavedGene) -> Self {
        Gene {
            id: saved.id.clone(),
            symbol: saved.symbol.clone(),
            index: saved.index,
            high: saved.high,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PropertySetting {
    pub id: String,
    pub shown: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
}

/// What a property admits, saved only when it restricts anything.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Filter {
    /// Codes picked out of a categorical property.
    Values { codes: Vec<u16> },
    /// An inclusive span of a numeric one.
    Span { from: f32, to: f32 },
    /// Nodes ticked in a tree, each by its level's column and its code there.
    Tree { ticked: Vec<TreeTick> },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TreeTick {
    pub level: String,
    pub code: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FrameState {
    /// Index into [`Bookmark::sources`] of the dataset the frame opened onto.
    pub source: usize,
    pub view: ViewState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orbit: Option<OrbitState>,
    /// Drawn over it, bottom first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<LayerState>,
}

/// A flat view, as the world it showed rather than a zoom factor.
///
/// A scale is world units per pixel, so the same scale in a smaller window
/// shows less. The extent is what was on screen, and is fitted into whatever
/// cell the frame is restored into.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct ViewState {
    pub centre: [f32; 2],
    pub extent: [f32; 2],
}

impl ViewState {
    pub fn new(centre: [f32; 2], scale: f32, cell: [f32; 2]) -> Self {
        ViewState {
            centre,
            extent: [scale * cell[0], scale * cell[1]],
        }
    }

    /// The scale that shows at least the whole saved extent in a cell this
    /// size.
    pub fn scale_in(&self, cell: [f32; 2]) -> f32 {
        (self.extent[0] / cell[0].max(1.0)).max(self.extent[1] / cell[1].max(1.0))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct OrbitState {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct LayerState {
    pub source: usize,
    pub opacity: f32,
}

pub fn channels_of(channels: &SourceChannels) -> Vec<ChannelState> {
    channels
        .channels
        .iter()
        .map(|channel| ChannelState {
            label: channel.label.clone(),
            shown: channel.shown,
            gain: channel.gain,
        })
        .collect()
}

/// Set each channel named in `saved`, matched by label. A channel the dataset
/// no longer has is skipped, and returned so it can be reported.
pub fn apply_channels(channels: &mut SourceChannels, saved: &[ChannelState]) -> Vec<String> {
    let mut missing = Vec::new();
    for state in saved {
        match channels
            .channels
            .iter_mut()
            .find(|channel| channel.label == state.label)
        {
            Some(channel) => {
                channel.shown = state.shown;
                channel.gain = state.gain.clamp(0.0, MAX_GAIN);
            }
            None => missing.push(state.label.clone()),
        }
    }
    missing
}

pub fn apply_slice(stack: &mut SliceStack, slice: u64) {
    stack.go_to(slice);
}

pub fn clamp_point_size(size: f32) -> f32 {
    size.clamp(MIN_POINT_PX, MAX_POINT_PX)
}

pub fn cells_of(properties: &CellProperties) -> CellsState {
    CellsState {
        color_by: properties
            .color_by
            .and_then(|index| properties.properties.get(index))
            .and_then(|property| property.color_column_id())
            .map(str::to_string),
        properties: properties
            .properties
            .iter()
            .map(|property| PropertySetting {
                id: property.id.clone(),
                shown: property.shown,
                filter: property.restricts().then(|| match &property.kind {
                    PropertyKind::Categorical(values) => Filter::Values {
                        codes: values
                            .iter()
                            .filter(|value| value.selected)
                            .map(|value| value.code)
                            .collect(),
                    },
                    PropertyKind::Numeric(range) => Filter::Span {
                        from: range.from,
                        to: range.to,
                    },
                    PropertyKind::Tree(tree) => Filter::Tree {
                        ticked: tree
                            .nodes
                            .iter()
                            .filter(|node| node.value.selected)
                            .filter_map(|node| {
                                Some(TreeTick {
                                    level: tree.levels.get(node.level)?.id.clone(),
                                    code: node.value.code,
                                })
                            })
                            .collect(),
                    },
                }),
            })
            .collect(),
        genes: properties
            .genes()
            .filter_map(|(_, gene)| {
                Some(SavedGene {
                    id: gene.id.clone(),
                    symbol: gene.name.clone(),
                    index: gene.gene?,
                    high: gene.range().map_or(0.0, |range| range.high),
                })
            })
            .collect(),
    }
}

/// The genes a bookmark adds that the dataset does not have yet, to be added
/// before [`apply_cells`] can set anything on them.
pub fn genes_missing(properties: &CellProperties, saved: &CellsState) -> Vec<Gene> {
    saved
        .genes
        .iter()
        .filter(|gene| !properties.properties.iter().any(|p| p.id == gene.id))
        .map(Gene::from)
        .collect()
}

/// Put saved cell settings onto a dataset's properties.
///
/// Every property the dataset has is reset first, so what is drawn is what was
/// saved and not that plus whatever was already ticked. Properties are matched
/// by id and values by code; one that no longer exists is skipped, and its id
/// returned so it can be reported.
pub fn apply_cells(properties: &mut CellProperties, saved: &CellsState) -> Vec<String> {
    let mut missing = Vec::new();
    properties.clear_all();
    // Genes the bookmark does not have are dropped, so what is listed is
    // what was saved. Last first, so the places of the rest hold.
    let genes: Vec<usize> = properties
        .genes()
        .filter(|(_, gene)| !saved.genes.iter().any(|kept| kept.id == gene.id))
        .map(|(index, _)| index)
        .collect();
    for index in genes.into_iter().rev() {
        properties.remove_gene(index);
    }
    for setting in &saved.properties {
        let Some(property) = properties
            .properties
            .iter_mut()
            .find(|property| property.id == setting.id)
        else {
            missing.push(setting.id.clone());
            continue;
        };
        property.shown = setting.shown;
        match (&mut property.kind, &setting.filter) {
            (_, None) => {}
            (PropertyKind::Categorical(values), Some(Filter::Values { codes })) => {
                let codes: HashSet<u16> = codes.iter().copied().collect();
                for value in values {
                    value.selected = codes.contains(&value.code);
                }
            }
            (PropertyKind::Numeric(range), Some(Filter::Span { from, to })) => {
                range.from = from.clamp(range.low, range.high);
                range.to = to.clamp(range.from, range.high);
            }
            (PropertyKind::Tree(tree), Some(Filter::Tree { ticked })) => {
                let levels: Vec<&str> = tree.levels.iter().map(|level| level.id.as_str()).collect();
                for node in &mut tree.nodes {
                    let level = levels.get(node.level).copied().unwrap_or_default();
                    node.value.selected = ticked
                        .iter()
                        .any(|tick| tick.level == level && tick.code == node.value.code);
                }
            }
            // The property changed kind since it was saved.
            _ => missing.push(setting.id.clone()),
        }
    }
    match &saved.color_by {
        Some(column) => properties.color_by_id(column),
        None => properties.color_by = None,
    }
    // Hidden properties filter nothing; a file edited by hand could say
    // otherwise.
    for property in &mut properties.properties {
        if !property.shown {
            property.clear();
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::channels::ChannelSetting;
    use crate::source::properties::{CellProperty, NumericRange, PropertyValue};
    use crate::source::tree::tests::tree;

    fn value(code: u16) -> PropertyValue {
        PropertyValue {
            code,
            label: format!("v{code}"),
            color: None,
            count: None,
            selected: false,
        }
    }

    fn properties() -> CellProperties {
        CellProperties::ready(vec![
            CellProperty {
                id: "class".into(),
                name: "Class".into(),
                shown: true,
                gene: None,
                kind: PropertyKind::Categorical((0..4).map(value).collect()),
            },
            CellProperty {
                id: "depth".into(),
                name: "Depth".into(),
                shown: true,
                gene: None,
                kind: PropertyKind::Numeric(NumericRange::full(0.0, 10.0, vec![1, 2, 3])),
            },
            CellProperty {
                id: "taxonomy".into(),
                name: "Taxonomy".into(),
                shown: false,
                gene: None,
                kind: PropertyKind::Tree(tree()),
            },
        ])
    }

    #[test]
    fn cell_settings_come_back_as_they_were_saved() {
        let mut edited = properties();
        edited.properties[0].column_values_mut("class")[2].selected = true;
        edited.properties[1].range_mut().unwrap().from = 2.5;
        edited.set_shown(2, true);
        edited.properties[2].tree_mut().unwrap().set(2, true);
        edited.color_by_id("subclass");
        let saved = cells_of(&edited);

        let mut restored = properties();
        let missing = apply_cells(&mut restored, &saved);
        assert!(missing.is_empty());
        assert_eq!(restored.selection(), edited.selection());
        assert_eq!(cells_of(&restored), saved);
        assert_eq!(restored.properties[2].tree().unwrap().color_level, 1);
    }

    fn gene(id: &str, index: u32) -> CellProperty {
        CellProperty {
            id: id.into(),
            name: id.to_uppercase(),
            shown: true,
            gene: Some(index),
            kind: PropertyKind::Numeric(NumericRange::full(0.0, 8.0, vec![4, 2, 1])),
        }
    }

    /// What the service would add for a gene a bookmark asks for.
    fn added(gene: &Gene) -> CellProperty {
        CellProperty {
            name: gene.symbol.clone(),
            ..self::gene(&gene.id, gene.index)
        }
    }

    #[test]
    fn genes_and_their_settings_come_back_as_they_were_saved() {
        let mut edited = properties();
        edited.add_gene(gene("gad1", 25351));
        edited.add_gene(gene("sst", 402));
        edited.properties[4].range_mut().unwrap().from = 3.0;
        edited.color_by_id("gad1");
        let saved = cells_of(&edited);
        assert_eq!(saved.genes.len(), 2);
        assert_eq!(saved.genes[0].index, 25351);
        assert_eq!(saved.genes[0].high, 8.0);
        assert_eq!(saved.color_by.as_deref(), Some("gad1"));

        // Restoring adds the genes first, then applies what was set on them.
        let mut restored = properties();
        let missing = genes_missing(&restored, &saved);
        let ids: Vec<&str> = missing.iter().map(|gene| gene.id.as_str()).collect();
        assert_eq!(ids, ["gad1", "sst"]);
        for gene in &missing {
            restored.add_gene(added(gene));
        }
        assert!(genes_missing(&restored, &saved).is_empty());
        assert!(apply_cells(&mut restored, &saved).is_empty());
        assert_eq!(restored.selection(), edited.selection());
        assert_eq!(cells_of(&restored), saved);
    }

    #[test]
    fn restoring_drops_genes_the_bookmark_does_not_have() {
        let mut open = properties();
        open.add_gene(gene("gad1", 1));
        open.color_by = Some(3);
        apply_cells(&mut open, &cells_of(&properties()));
        assert_eq!(open.genes().count(), 0);
        assert_eq!(open.color_by, Some(0));
    }

    #[test]
    fn a_gene_that_could_not_be_added_is_reported() {
        let mut edited = properties();
        edited.add_gene(gene("gad1", 1));
        edited.properties[3].range_mut().unwrap().to = 4.0;
        let saved = cells_of(&edited);
        assert_eq!(apply_cells(&mut properties(), &saved), ["gad1"]);
    }

    #[test]
    fn a_bookmark_from_before_genes_still_reads() {
        let saved: CellsState =
            serde_json::from_str(r#"{"color_by":"class","properties":[]}"#).unwrap();
        assert!(saved.genes.is_empty());
    }

    #[test]
    fn restoring_clears_filters_the_bookmark_does_not_have() {
        let mut open = properties();
        open.properties[0].column_values_mut("class")[1].selected = true;
        apply_cells(&mut open, &cells_of(&properties()));
        assert_eq!(open.applied(), 0);
    }

    #[test]
    fn a_property_the_dataset_lost_is_reported_rather_than_failing() {
        let saved = CellsState {
            color_by: Some("class".into()),
            properties: vec![PropertySetting {
                id: "gone".into(),
                shown: true,
                filter: Some(Filter::Values { codes: vec![1] }),
            }],
            genes: Vec::new(),
        };
        let mut restored = properties();
        assert_eq!(apply_cells(&mut restored, &saved), ["gone"]);
        assert_eq!(restored.color_by, Some(0));
    }

    #[test]
    fn a_span_is_held_to_the_data() {
        let saved = CellsState {
            color_by: None,
            properties: vec![PropertySetting {
                id: "depth".into(),
                shown: true,
                filter: Some(Filter::Span {
                    from: -5.0,
                    to: 50.0,
                }),
            }],
            genes: Vec::new(),
        };
        let mut restored = properties();
        apply_cells(&mut restored, &saved);
        let range = restored.properties[1].range().unwrap();
        assert_eq!((range.from, range.to), (0.0, 10.0));
        assert_eq!(restored.color_by, None);
    }

    #[test]
    fn channels_are_matched_by_label_not_position() {
        let mut channels = SourceChannels::new(vec![
            ChannelSetting::new("DAPI", [0.0, 0.0, 1.0], true),
            ChannelSetting::new("GFP", [0.0, 1.0, 0.0], true),
        ]);
        let saved = vec![
            ChannelState {
                label: "GFP".into(),
                shown: false,
                gain: 2.0,
            },
            ChannelState {
                label: "RFP".into(),
                shown: true,
                gain: 1.0,
            },
        ];
        assert_eq!(apply_channels(&mut channels, &saved), ["RFP"]);
        assert!(channels.channels[0].shown);
        assert!(!channels.channels[1].shown);
        assert_eq!(channels.channels[1].gain, 2.0);
    }

    #[test]
    fn a_view_shows_at_least_what_was_saved_in_any_cell() {
        let view = ViewState::new([0.0, 0.0], 2.0, [400.0, 300.0]);
        assert_eq!(view.extent, [800.0, 600.0]);
        // The same cell gives back the same scale.
        assert_eq!(view.scale_in([400.0, 300.0]), 2.0);
        // A narrower one zooms out to keep the width.
        assert_eq!(view.scale_in([200.0, 300.0]), 4.0);
    }
}
