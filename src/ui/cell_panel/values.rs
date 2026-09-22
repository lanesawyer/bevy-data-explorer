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

use bevy::ecs::query::QueryFilter;
use bevy::prelude::*;
use bevy::text::EditableText;

use super::tree::{TreeCheckbox, TreeCount, Unveil, spawn_tree_body};
use super::{MAX_VALUE_ROWS, ValueCheckbox, ValueColumn, ValueCount, spawn_value_row};
use crate::source::properties::{CellProperties, CellProperty, PropertyKind};
use crate::view::SelectedSource;
use crate::widgets::space;
use crate::widgets::{
    matches_search, scroll_list, set_display, set_text, size, spawn_search_field, text_dim,
};

/// Fewest values a property has before it offers a search. Below this the
/// whole list is in view anyway. The table filters go by the same figure.
pub const SEARCH_FROM: usize = 12;

/// Tallest a property's list grows before it scrolls. About a dozen rows: the
/// sidebar has other properties to show.
pub const LIST_MAX_PX: f32 = 300.0;

/// The field a property's values are searched from. On the inner text entity,
/// which holds the [`EditableText`].
#[derive(Component, Clone, Default)]
pub struct ValueSearch;

/// The rows of one property's values, kept to match its search.
#[derive(Component)]
pub struct ValueList {
    property: usize,
    /// A tree's nested body, hidden while a search lists matches flat.
    tree: Option<Entity>,
    /// Rows by value, or by node for a tree.
    rows: SearchedRows,
}

/// A list's rows, kept to match what is typed in its search. The table
/// filters' lists are kept by the same rules.
pub struct SearchedRows {
    /// The search field, for a list long enough to have one.
    field: Option<Entity>,
    /// The line under the rows: nothing matched, or more did than are listed.
    note: Entity,
    /// What was typed when the rows were last matched; nothing before the
    /// first time.
    typed: Option<String>,
    /// Rows spawned so far, by index.
    rows: BTreeMap<usize, Entity>,
}

impl SearchedRows {
    pub fn new(field: Option<Entity>, note: Entity) -> Self {
        SearchedRows {
            field,
            note,
            typed: None,
            rows: BTreeMap::new(),
        }
    }

    /// What is typed in the search, if the rows have not been matched to it
    /// yet. Compared in place, so a list whose search has not changed costs
    /// nothing to look at.
    pub fn typed<F: QueryFilter>(&self, fields: &Query<&EditableText, F>) -> Option<String> {
        let text = self.field.and_then(|field| fields.get(field).ok());
        let unchanged = self.typed.as_deref().is_some_and(|typed| match text {
            Some(text) => text.value() == typed,
            None => typed.is_empty(),
        });
        (!unchanged).then(|| {
            text.map(|text| text.value().to_string())
                .unwrap_or_default()
        })
    }

    /// List `matches` for what was `typed`: spawn through `spawn` the rows
    /// newly wanted, and hide the ones no longer wanted rather than despawn
    /// them.
    #[expect(
        clippy::too_many_arguments,
        reason = "the list, what it matched, and the UI it writes into"
    )]
    pub fn show(
        &mut self,
        commands: &mut Commands,
        list: Entity,
        typed: String,
        browsing: bool,
        matches: &[usize],
        nodes: &mut Query<&mut Node>,
        texts: &mut Query<&mut Text>,
        mut spawn: impl FnMut(&mut Commands, usize) -> Option<Entity>,
    ) {
        let listed = &matches[..matches.len().min(MAX_VALUE_ROWS)];
        for &index in listed {
            if self.rows.contains_key(&index) {
                continue;
            }
            let Some(row) = spawn(commands, index) else {
                continue;
            };
            // Kept in the order the values are listed in, whatever order a
            // search asked for them in.
            let position = self.rows.range(..index).count();
            commands
                .entity(row)
                .insert((Visibility::Hidden, Unveil::new()));
            commands.entity(list).insert_children(position, &[row]);
            self.rows.insert(index, row);
        }
        let listed: HashSet<usize> = listed.iter().copied().collect();
        for (index, row) in &self.rows {
            set_display(nodes, *row, listed.contains(index));
        }

        let note = note_for(typed.trim(), browsing, matches.len());
        set_display(nodes, self.note, !note.is_empty());
        if let Ok(text) = texts.get_mut(self.note) {
            set_text(text, &note);
        }
        self.typed = Some(typed);
    }
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
            text_dim("", size::SMALL)
            Node { display: { Display::None } }
        })
        .id();
    let list = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(space::LIST_ITEMS),
                ..default()
            },
            ValueList {
                property: index,
                tree,
                rows: SearchedRows::new(search.as_ref().map(|search| search.field), note),
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
    selected: SelectedSource,
    sources: Query<&CellProperties>,
    fields: Query<&EditableText, With<ValueSearch>>,
    mut lists: Query<(Entity, &mut ValueList)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let Some(properties) = selected.get(&sources) else {
        return;
    };

    for (entity, mut list) in &mut lists {
        let Some(typed) = list.rows.typed(&fields) else {
            continue;
        };
        let Some(property) = properties.properties.get(list.property) else {
            continue;
        };
        let query = typed.trim();

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
                    .filter(|(_, value)| matches_search(query, &[&value.label]))
                    .map(|(index, _)| index)
                    .collect(),
                PropertyKind::Tree(tree) => tree
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, node)| matches_search(query, &[&node.value.label]))
                    .map(|(index, _)| index)
                    .collect(),
                PropertyKind::Numeric(_) => Vec::new(),
            }
        };

        let property_index = list.property;
        list.rows.show(
            &mut commands,
            entity,
            typed,
            browsing,
            &matches,
            &mut nodes,
            &mut texts,
            |commands, index| match &property.kind {
                PropertyKind::Categorical(values) => Some(spawn_value_row(
                    commands,
                    &values[index],
                    values[index].selected,
                    ValueCheckbox {
                        property: property_index,
                        value: index,
                    },
                    ValueCount {
                        property: property_index,
                        value: index,
                    },
                    ValueColumn {
                        column: property.id.clone(),
                        code: values[index].code,
                    },
                )),
                PropertyKind::Tree(tree) => Some(spawn_value_row(
                    commands,
                    &tree.nodes[index].value,
                    tree.checked(index),
                    TreeCheckbox {
                        property: property_index,
                        node: index,
                    },
                    TreeCount {
                        property: property_index,
                        node: index,
                    },
                    ValueColumn {
                        column: tree.column_of(index).unwrap_or_default().to_string(),
                        code: tree.nodes[index].value.code,
                    },
                )),
                PropertyKind::Numeric(_) => None,
            },
        );
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
