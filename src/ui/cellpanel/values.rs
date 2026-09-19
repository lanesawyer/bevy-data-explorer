//! A categorical or tree property's values: a field to search them by label,
//! over a list that scrolls rather than running the length of the sidebar.
//!
//! Rows are spawned the first time they are wanted and hidden, not despawned,
//! when a search leaves them out, so typing never respawns a checkbox that is
//! already there. A search reaches every value, including the ones past
//! [`MAX_VALUE_ROWS`] that an untouched list leaves out.
//!
//! A tree is browsed nested while the field is empty, and searched flat: a
//! match can sit at any depth, and opening every branch on its way would bury
//! it. A flat row is the same checkbox as the nested one, so ticking either
//! ticks both.

use std::collections::{BTreeMap, HashSet};

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy_feathers::display::label_dim;

use super::tree::{TreeCheckbox, TreeCount, Unveil, spawn_tree_body};
use super::{MAX_VALUE_ROWS, SMALL_PX, ValueCheckbox, ValueCount, spawn_value_row};
use crate::source::properties::{CellProperties, CellProperty, PropertyKind};
use crate::view::{SelectedPanel, ShowsSource};
use crate::widgets::{matches_search, scroll_list, spawn_search_field};

/// Fewest values a property has before it offers a search. Below this the
/// whole list is in view anyway.
const SEARCH_FROM: usize = 12;

/// Tallest a property's list grows before it scrolls. About a dozen rows: the
/// sidebar has other properties to show.
const LIST_MAX_PX: f32 = 300.0;

/// The field a property's values are searched from. On the inner text entity,
/// which holds the [`EditableText`].
#[derive(Component, Clone, Default)]
pub struct ValueSearch;

/// The rows of one property's values, kept to match its search.
#[derive(Component)]
pub struct ValueList {
    property: usize,
    /// The search field, for a property with enough values to have one.
    field: Option<Entity>,
    /// A tree's nested body, hidden while a search lists matches flat.
    tree: Option<Entity>,
    /// The line under the rows: nothing matched, or more did than are listed.
    note: Entity,
    /// The search the rows were last matched to; nothing before the first.
    shown: Option<String>,
    /// Rows spawned so far, by value, or by node for a tree.
    rows: BTreeMap<usize, Entity>,
}

/// Build the body of a categorical or tree property: its search, if it has
/// enough values to need one, and the list under it.
pub fn spawn_values(commands: &mut Commands, index: usize, property: &CellProperty) -> Vec<Entity> {
    let (count, tree) = match &property.kind {
        PropertyKind::Categorical(values) => (values.len(), None),
        PropertyKind::Tree(tree) => (tree.nodes.len(), Some(spawn_tree_body(commands, index))),
        PropertyKind::Numeric(_) => return Vec::new(),
    };

    let search = (count >= SEARCH_FROM).then(|| {
        let search = spawn_search_field(commands, format!("Search {count} values"));
        commands.entity(search.field).insert(ValueSearch);
        search
    });

    let note = commands
        .spawn_scene(bsn! {
            label_dim("")
            TextFont { font_size: { FontSize::Px(SMALL_PX) } }
            Node { display: { Display::None } }
        })
        .id();
    let list = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
            ValueList {
                property: index,
                field: search.as_ref().map(|search| search.field),
                tree,
                note,
                shown: None,
                rows: BTreeMap::new(),
            },
        ))
        .id();
    let scroller = commands.spawn_scene(scroll_list(LIST_MAX_PX)).id();
    let children: Vec<Entity> = tree.into_iter().chain([list, note]).collect();
    commands.entity(scroller).add_children(&children);

    search
        .map(|search| search.entry)
        .into_iter()
        .chain([scroller])
        .collect()
}

/// Match each list to what is typed in its search: spawn the rows it newly
/// wants, and hide the ones it no longer does.
pub fn sync_value_lists(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    fields: Query<&EditableText, With<ValueSearch>>,
    mut lists: Query<(Entity, &mut ValueList)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let Some(properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok())
    else {
        return;
    };

    for (entity, mut list) in &mut lists {
        let query = list
            .field
            .and_then(|field| fields.get(field).ok())
            .map(|text| text.value().to_string().trim().to_string())
            .unwrap_or_default();
        if list.shown.as_ref() == Some(&query) {
            continue;
        }
        let Some(property) = properties.properties.get(list.property) else {
            continue;
        };
        list.shown = Some(query.clone());

        let browsing = list.tree.is_some() && query.is_empty();
        if let Some(tree) = list.tree {
            set_display(&mut nodes, tree, browsing);
        }
        set_display(&mut nodes, entity, !browsing);

        let matches: Vec<usize> = if browsing {
            Vec::new()
        } else {
            match &property.kind {
                PropertyKind::Categorical(values) => values
                    .iter()
                    .enumerate()
                    .filter(|(_, value)| matches_search(&query, &[&value.label]))
                    .map(|(index, _)| index)
                    .collect(),
                PropertyKind::Tree(tree) => tree
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, node)| matches_search(&query, &[&node.value.label]))
                    .map(|(index, _)| index)
                    .collect(),
                PropertyKind::Numeric(_) => Vec::new(),
            }
        };
        let listed: HashSet<usize> = matches.iter().take(MAX_VALUE_ROWS).copied().collect();

        for &index in matches.iter().take(MAX_VALUE_ROWS) {
            if list.rows.contains_key(&index) {
                continue;
            }
            let row = match &property.kind {
                PropertyKind::Categorical(values) => spawn_value_row(
                    &mut commands,
                    &values[index],
                    values[index].selected,
                    ValueCheckbox {
                        property: list.property,
                        value: index,
                    },
                    ValueCount {
                        property: list.property,
                        value: index,
                    },
                ),
                PropertyKind::Tree(tree) => spawn_value_row(
                    &mut commands,
                    &tree.nodes[index].value,
                    tree.checked(index),
                    TreeCheckbox {
                        property: list.property,
                        node: index,
                    },
                    TreeCount {
                        property: list.property,
                        node: index,
                    },
                ),
                PropertyKind::Numeric(_) => continue,
            };
            // Kept in the order the values are listed in, whatever order a
            // search asked for them in.
            let position = list.rows.range(..index).count();
            commands
                .entity(row)
                .insert((Visibility::Hidden, Unveil::new()));
            commands.entity(entity).insert_children(position, &[row]);
            list.rows.insert(index, row);
        }
        for (index, row) in &list.rows {
            set_display(&mut nodes, *row, listed.contains(index));
        }

        let note = note_for(&query, browsing, matches.len());
        set_display(&mut nodes, list.note, !note.is_empty());
        if let Ok(mut text) = texts.get_mut(list.note)
            && text.0 != note
        {
            text.0 = note;
        }
    }
}

fn set_display(nodes: &mut Query<&mut Node>, entity: Entity, shown: bool) {
    let wanted = if shown { Display::Flex } else { Display::None };
    if let Ok(mut node) = nodes.get_mut(entity)
        && node.display != wanted
    {
        node.display = wanted;
    }
}

/// The line under a list: that nothing matched, or that more did than are
/// listed.
fn note_for(query: &str, browsing: bool, matched: usize) -> String {
    if browsing {
        String::new()
    } else if matched == 0 && !query.is_empty() {
        format!("Nothing matches \"{query}\".")
    } else if matched > MAX_VALUE_ROWS {
        format!("and {} more; search to narrow", matched - MAX_VALUE_ROWS)
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_that_fits_says_nothing() {
        assert_eq!(note_for("", false, 40), "");
        assert_eq!(note_for("glut", false, 3), "");
    }

    #[test]
    fn a_search_that_finds_nothing_says_so() {
        assert_eq!(note_for("zzz", false, 0), "Nothing matches \"zzz\".");
    }

    #[test]
    fn a_list_cut_short_says_by_how_much() {
        assert!(note_for("", false, MAX_VALUE_ROWS + 5).starts_with("and 5 more"));
    }

    #[test]
    fn a_tree_being_browsed_says_nothing() {
        assert_eq!(note_for("", true, 0), "");
    }
}
