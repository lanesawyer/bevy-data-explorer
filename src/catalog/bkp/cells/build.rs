//! The platform's answers made into [`CellProperties`]: each column's values
//! with their labels, colors and order, and the taxonomy's levels as one tree.

use super::*;

/// One value as the platform lists it, before it is placed.
pub(super) struct Listed {
    pub(super) priority: i32,
    pub(super) reference: Option<String>,
    pub(super) parent: Option<String>,
    pub(super) value: PropertyValue,
}

/// What the portal lists, in its order.
pub(super) enum Entry {
    Column {
        id: String,
        title: Option<String>,
        shown: bool,
    },
    Tree {
        id: String,
        title: Option<String>,
        shown: bool,
        /// Column id and title of each level, coarsest first.
        levels: Vec<(String, Option<String>)>,
    },
}

/// Assemble the properties the portal would show, for the columns the files
/// hold.
///
/// Listed in the portal's order. A taxonomy becomes one tree over whichever of
/// its levels the files hold, or a plain property if they hold only one.
/// Columns the portal does not show are kept but hidden, so the section's menu
/// can still offer them, and only if the platform knows enough about them to be
/// worth offering.
pub(super) fn build(
    columns: &CellColumns,
    display: Display,
    values: Vec<ValueRecord>,
    extents: &HashMap<String, (f32, f32)>,
    histograms: &HashMap<String, Vec<u32>>,
) -> CellProperties {
    let mut by_column: HashMap<String, Vec<Listed>> = HashMap::new();
    // Every value's parent, whatever its column, so a tree can reach past a
    // level the files do not hold.
    let mut parents: HashMap<String, String> = HashMap::new();
    for record in values {
        let index = record.feature_type_value_index;
        let Ok(code) = u16::try_from(index.index) else {
            continue;
        };
        let listed = by_column
            .entry(record.feature_type.reference_id)
            .or_default();
        // Some datasets list every value twice, identically.
        if listed.iter().any(|found| found.value.code == code) {
            continue;
        }
        if let (Some(reference), Some(parent)) = (&index.reference_id, &index.parent_reference_id) {
            parents.insert(reference.clone(), parent.clone());
        }
        listed.push(Listed {
            priority: index.priority_order.unwrap_or(i32::MAX),
            reference: index.reference_id.clone(),
            parent: index.parent_reference_id,
            value: PropertyValue {
                code,
                label: index.value,
                reference: index.reference_id,
                color: record.color.as_deref().and_then(parse_color),
                count: None,
                selected: false,
            },
        });
    }
    for listed in by_column.values_mut() {
        listed.sort_by_key(|found| (found.priority, found.value.code));
    }

    let mut features = display.display_features;
    features.sort_by_key(|feature| feature.priority_order.unwrap_or(i32::MAX));
    let mut entries: Vec<Entry> = Vec::new();
    for feature in features {
        match feature.feature_set {
            Some(mut levels) if !levels.is_empty() => {
                levels.sort_by_key(|level| level.priority_order.unwrap_or(i32::MAX));
                entries.push(Entry::Tree {
                    id: feature.feature_type.reference_id,
                    title: feature.feature_type.title,
                    shown: feature.is_default,
                    levels: levels
                        .into_iter()
                        .map(|level| (level.feature_type.reference_id, level.feature_type.title))
                        .collect(),
                });
            }
            _ => entries.push(Entry::Column {
                id: feature.feature_type.reference_id,
                title: feature.feature_type.title,
                shown: feature.is_default,
            }),
        }
    }
    for column in &columns.0 {
        entries.push(Entry::Column {
            id: column.id.clone(),
            title: None,
            shown: false,
        });
    }

    let readable = |id: &str, by_column: &HashMap<String, Vec<Listed>>| {
        columns
            .0
            .iter()
            .any(|column| column.id == id && !column.numeric)
            && by_column.contains_key(id)
    };

    let mut properties = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        match entry {
            Entry::Tree {
                id,
                title,
                shown,
                levels,
            } => {
                let present: Vec<(String, Option<String>)> = levels
                    .into_iter()
                    .filter(|(level, _)| !seen.contains(level) && readable(level, &by_column))
                    .collect();
                if present.len() == 1 {
                    let (level, level_title) = present.into_iter().next().unwrap();
                    entries_column(
                        &mut properties,
                        &mut seen,
                        &mut by_column,
                        columns,
                        extents,
                        histograms,
                        &level,
                        level_title,
                        shown,
                    );
                    continue;
                }
                if present.is_empty() || !seen.insert(id.clone()) {
                    continue;
                }
                let tree = tree_of(&present, &mut by_column, &parents, columns);
                for (level, _) in &present {
                    seen.insert(level.clone());
                }
                properties.push(CellProperty {
                    id,
                    name: title.unwrap_or_else(|| "Taxonomy".into()),
                    shown,
                    kind: PropertyKind::Tree(tree),
                    gene: None,
                });
            }
            Entry::Column { id, title, shown } => entries_column(
                &mut properties,
                &mut seen,
                &mut by_column,
                columns,
                extents,
                histograms,
                &id,
                title,
                shown,
            ),
        }
    }

    let mut described = CellProperties::ready(properties);
    if let Some(default) = display.default_color_by {
        described.color_by_id(&default.reference_id);
    }
    described
}

/// Add one column as a property of its own, if the files hold it and the
/// platform says enough about it.
#[expect(
    clippy::too_many_arguments,
    reason = "the state one pass of the build threads through"
)]
pub(super) fn entries_column(
    properties: &mut Vec<CellProperty>,
    seen: &mut std::collections::HashSet<String>,
    by_column: &mut HashMap<String, Vec<Listed>>,
    columns: &CellColumns,
    extents: &HashMap<String, (f32, f32)>,
    histograms: &HashMap<String, Vec<u32>>,
    id: &str,
    title: Option<String>,
    shown: bool,
) {
    let Some(column) = columns.0.iter().find(|column| column.id == id) else {
        return;
    };
    if seen.contains(id) {
        return;
    }
    let kind = if column.numeric {
        let Some(&(low, high)) = extents.get(id) else {
            return;
        };
        let histogram = histograms.get(id).cloned().unwrap_or_default();
        PropertyKind::Numeric(NumericRange::full(low, high, histogram))
    } else {
        let Some(values) = by_column.remove(id) else {
            return;
        };
        PropertyKind::Categorical(values.into_iter().map(|found| found.value).collect())
    };
    seen.insert(id.to_string());
    properties.push(CellProperty {
        id: id.to_string(),
        name: title.unwrap_or_else(|| column.name.clone()),
        shown,
        kind,
        gene: None,
    });
}

/// Nest the values of `levels` under one another by their parents.
///
/// A parent is found by walking up the platform's links until one lands on a
/// value in a level held here, so a level missing from the files is stepped
/// over rather than cutting the tree apart. A value whose line reaches no such
/// parent becomes a root of its own rather than being lost.
pub(super) fn tree_of(
    levels: &[(String, Option<String>)],
    by_column: &mut HashMap<String, Vec<Listed>>,
    parents: &HashMap<String, String>,
    columns: &CellColumns,
) -> Tree {
    let mut nodes: Vec<TreeNode> = Vec::new();
    let mut placed: HashMap<String, usize> = HashMap::new();
    for (level, (id, _)) in levels.iter().enumerate() {
        for listed in by_column.remove(id).unwrap_or_default() {
            let mut above = listed.parent.clone();
            let parent = loop {
                match above {
                    Some(reference) => match placed.get(&reference) {
                        Some(&node) => break Some(node),
                        None => above = parents.get(&reference).cloned(),
                    },
                    None => break None,
                }
            };
            if let Some(reference) = listed.reference {
                placed.insert(reference, nodes.len());
            }
            nodes.push(TreeNode {
                level,
                parent,
                value: listed.value,
            });
        }
    }
    Tree {
        levels: levels
            .iter()
            .map(|(id, title)| TreeLevel {
                id: id.clone(),
                name: title.clone().unwrap_or_else(|| {
                    columns
                        .0
                        .iter()
                        .find(|column| &column.id == id)
                        .map_or_else(|| id.clone(), |column| column.name.clone())
                }),
            })
            .collect(),
        nodes,
        color_level: 0,
    }
}

/// A color as the platform writes it: `#rrggbb`, in either case.
pub(super) fn parse_color(text: &str) -> Option<Color> {
    Srgba::hex(text.trim()).ok().map(Color::from)
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;
    use crate::source::properties::Column;

    #[test]
    fn properties_follow_the_portals_order_with_a_taxonomy_as_one_tree() {
        let properties = described();
        let ids: Vec<&str> = properties
            .properties
            .iter()
            .map(|property| property.id.as_str())
            .collect();
        // The taxonomy first, as one property; the genes, which the files hold
        // no column for, nowhere; the donor, which the portal does not list,
        // last and hidden; and the column the platform knows nothing about,
        // not at all.
        assert_eq!(ids, ["TAX", "BRAAK", "CPS", "DONOR"]);
        assert_eq!(properties.properties[0].name, "SEA-AD CaH Taxonomy");
        assert!(!properties.properties[3].shown);
    }

    #[test]
    fn a_taxonomy_nests_its_levels_by_their_parents() {
        let properties = described();
        let tree = properties.properties[0].tree().unwrap();
        let names: Vec<&str> = tree
            .levels
            .iter()
            .map(|level| level.name.as_str())
            .collect();
        assert_eq!(names, ["Neighborhood", "Subclass"]);

        let label = |node: usize| tree.nodes[node].value.label.as_str();
        let roots: Vec<&str> = tree.children(None).map(label).collect();
        assert_eq!(roots, ["Neurons", "Vascular"]);
        let neurons = tree.children(None).next().unwrap();
        let under: Vec<&str> = tree.children(Some(neurons)).map(label).collect();
        assert_eq!(under, ["STR D1 MSN"], "listed once, though sent twice");

        // Filtering happens in the finest column.
        let mut properties = properties;
        properties.properties[0]
            .tree_mut()
            .unwrap()
            .set(neurons, true);
        let selection = properties.selection();
        assert_eq!(selection.filters.len(), 1);
        assert_eq!(selection.filters[0].0, Column::Cell("LEVEL_1".into()));
    }

    #[test]
    fn values_carry_the_platforms_labels_colors_and_order() {
        let properties = described();
        let braak = &properties.properties[1];
        let labels: Vec<(&str, u16)> = braak
            .values()
            .iter()
            .map(|value| (value.label.as_str(), value.code))
            .collect();
        assert_eq!(labels, [("Braak 0", 5), ("Braak IV", 3)]);
        assert_eq!(
            braak.values()[1].color,
            Some(Color::from(Srgba::hex("d52221").unwrap()))
        );
        // A value with no color of its own falls back to the default palette.
        let tree = properties.properties[0].tree().unwrap();
        assert_eq!(tree.nodes[0].value.color, None);
    }

    #[test]
    fn coloring_starts_where_the_portal_says() {
        let properties = described();
        assert_eq!(
            properties
                .selection()
                .color_by
                .as_ref()
                .and_then(Column::cell),
            Some("LEVEL_1"),
            "the portal's default, not the first listed"
        );
        // And draws in the platform's colors.
        let palette = properties.selection().palette;
        let d1 = Color::from(Srgba::hex("1655F2").unwrap()).to_linear();
        assert_eq!(palette[15], [d1.red, d1.green, d1.blue, 1.0]);
    }

    #[test]
    fn a_numeric_property_takes_the_platforms_extent_and_histogram() {
        let properties = described();
        let range = properties.properties[2].range().unwrap();
        assert_eq!((range.low, range.high), (0.0, 1.0));
        assert_eq!(range.histogram, [1, 2, 3]);
        assert!(!range.restricts());
    }
}
