//! A tree property's body — its nodes nested under one another with a
//! checkbox each — and the menu its header's color button opens, of the
//! levels it can color by.
//!
//! Only the roots are built with the section. A node's children are spawned
//! when it is expanded and despawned when it is collapsed, because a
//! whole-brain taxonomy runs to thousands of clusters and a checkbox apiece
//! would make the sidebar crawl for rows nobody has opened.
//!
//! Ticks, counts and the partial markers are written onto the rows that exist
//! rather than rebuilt for, as the flat properties' are.

use std::collections::HashSet;

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersToolButton};
use bevy_feathers::display::label;
use bevy_ui_widgets::{Activate, ValueChange};

use super::{MAX_VALUE_ROWS, spawn_more_note, spawn_value_row};
use crate::app::theme::Palette;
use crate::source::compact_count;
use crate::source::properties::{CellProperties, Tree};
use crate::view::{BlocksFrameInput, SelectedPanel, ShowsSource};
use crate::widgets::Menu;
use crate::widgets::{Icon, button_icon, button_text};

/// Indent per level of the tree.
const INDENT_PX: f32 = 14.0;
/// The expand control's width, kept by leaves as a gap so labels line up.
const TOGGLE_PX: f32 = 22.0;
/// Space between one node's row and the next, so neighbouring checkboxes do
/// not touch.
const ROW_GAP_PX: f32 = 4.0;

/// Which nodes are expanded, by property and node.
///
/// Outlives the rows, so a rebuild of the section keeps what was open.
#[derive(Resource, Default)]
pub struct OpenBranches(pub HashSet<(usize, usize)>);

/// The container a node's children are spawned into, or the roots' for
/// `None`.
#[derive(Component, Clone, Default)]
pub struct TreeChildren {
    pub property: usize,
    pub parent: Option<usize>,
}

/// The control that expands or collapses a node.
#[derive(Component, Clone, Default)]
pub struct TreeToggle {
    pub property: usize,
    pub node: usize,
}

/// A node's checkbox.
#[derive(Component, Clone, Default)]
pub struct TreeCheckbox {
    pub property: usize,
    pub node: usize,
}

/// A node's count of cells.
#[derive(Component, Clone, Default)]
pub struct TreeCount {
    pub property: usize,
    pub node: usize,
}

/// A dot shown beside a node some but not all of whose cells are admitted, so
/// a tick deep inside a collapsed branch is not invisible.
#[derive(Component, Clone, Default)]
pub struct TreePartial {
    pub property: usize,
    pub node: usize,
}

/// The button that colors points by one level of a tree.
#[derive(Component, Clone, Default)]
pub struct ColorLevelButton {
    pub property: usize,
    pub level: usize,
}

/// Held hidden for a couple of frames after spawning. A fresh Feathers
/// checkbox draws its mark before its styling system has run, so a branch
/// shown at once flashes every box ticked.
#[derive(Component)]
pub struct Unveil(u8);

/// Fill a tree's color menu with one choice per level.
pub fn fill_color_menu(
    commands: &mut Commands,
    menu: Entity,
    property: usize,
    tree: &Tree,
    coloring: bool,
) {
    let heading = commands
        .spawn_scene(bsn! {
            label("Color by")
            Node { margin: { UiRect::bottom(Val::Px(4.0)) } }
        })
        .id();
    commands.entity(menu).add_child(heading);
    for (level, found) in tree.levels.iter().enumerate() {
        let name = found.name.clone();
        let button = commands
            .spawn_scene(bsn! {
                @FeathersButton {
                    @caption: { bsn_list![button_text(name)] }
                }
                BlocksFrameInput
                ColorLevelButton { property: { property }, level: { level } }
            })
            .id();
        if coloring && tree.color_level == level {
            commands.entity(button).insert(ButtonVariant::Primary);
        }
        commands.entity(menu).add_child(button);
    }
}

/// Build the body of a tree property: the container its roots go in.
pub fn spawn_tree_body(commands: &mut Commands, property: usize) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(ROW_GAP_PX),
                ..default()
            },
            TreeChildren {
                property,
                parent: None,
            },
        ))
        .id()
}

/// One node: its row, and the container its children go in when expanded.
fn spawn_branch(
    commands: &mut Commands,
    property: usize,
    tree: &Tree,
    node: usize,
    palette: &Palette,
) -> Entity {
    let lead = if tree.has_children(node) {
        commands
            .spawn_scene(bsn! {
                @FeathersToolButton {
                    @caption: { bsn_list![button_icon(Icon::ChevronRight)] }
                }
                BlocksFrameInput
                Node { flex_shrink: { 0.0_f32 }, width: { Val::Px(TOGGLE_PX) } }
                TreeToggle { property: { property }, node: { node } }
            })
            .id()
    } else {
        commands
            .spawn(Node {
                width: Val::Px(TOGGLE_PX),
                flex_shrink: 0.0,
                ..default()
            })
            .id()
    };
    let row = spawn_value_row(
        commands,
        &tree.nodes[node].value,
        tree.checked(node),
        TreeCheckbox { property, node },
        TreeCount { property, node },
    );
    commands.entity(row).insert(Node {
        flex_grow: 1.0,
        min_width: Val::ZERO,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        column_gap: Val::Px(6.0),
        ..default()
    });
    let partial = commands
        .spawn((
            Node {
                width: Val::Px(6.0),
                height: Val::Px(6.0),
                flex_shrink: 0.0,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                display: display(tree.partly_checked(node)),
                ..default()
            },
            BackgroundColor(palette.fill),
            TreePartial { property, node },
        ))
        .id();
    let line = commands
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(2.0),
            ..default()
        })
        .add_children(&[lead, partial, row])
        .id();
    let children = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(ROW_GAP_PX),
                padding: UiRect::left(Val::Px(INDENT_PX)),
                // Laid out only once it holds rows, or the gap above it would
                // space every collapsed node twice as far from the next.
                display: Display::None,
                ..default()
            },
            TreeChildren {
                property,
                parent: Some(node),
            },
        ))
        .id();
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(ROW_GAP_PX),
            ..default()
        })
        .add_children(&[line, children])
        .id()
}

fn display(shown: bool) -> Display {
    if shown { Display::Flex } else { Display::None }
}

/// The selected source's properties, if it has any.
fn selected_properties<'a>(
    selected: &SelectedPanel,
    panels: &Query<&ShowsSource>,
    sources: &'a Query<&CellProperties>,
) -> Option<&'a CellProperties> {
    selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok())
}

/// Spawn the children of every expanded node that has none yet, and despawn
/// those of every collapsed one.
pub fn sync_branches(
    mut commands: Commands,
    open: Res<OpenBranches>,
    palette: Res<Palette>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    mut containers: Query<(Entity, &TreeChildren, Option<&Children>, &mut Node)>,
) {
    let Some(properties) = selected_properties(&selected, &panels, &sources) else {
        return;
    };
    for (entity, container, children, mut node) in &mut containers {
        let Some(tree) = properties
            .properties
            .get(container.property)
            .and_then(|property| property.tree())
        else {
            continue;
        };
        let wanted = container
            .parent
            .is_none_or(|node| open.0.contains(&(container.property, node)));
        let built = children.is_some_and(|children| !children.is_empty());
        if wanted == built {
            continue;
        }
        node.display = display(wanted);
        if !wanted {
            commands.entity(entity).despawn_related::<Children>();
            continue;
        }
        let nodes: Vec<usize> = tree.children(container.parent).collect();
        let mut rows: Vec<Entity> = nodes
            .iter()
            .take(MAX_VALUE_ROWS)
            .map(|&node| spawn_branch(&mut commands, container.property, tree, node, &palette))
            .collect();
        if nodes.len() > MAX_VALUE_ROWS {
            rows.push(spawn_more_note(&mut commands, nodes.len() - MAX_VALUE_ROWS));
        }
        commands
            .entity(entity)
            .add_children(&rows)
            .insert((Visibility::Hidden, Unveil(2)));
    }
}

/// Show a branch once its checkboxes have been styled.
pub fn unveil(mut commands: Commands, mut held: Query<(Entity, &mut Unveil, &mut Visibility)>) {
    for (entity, mut unveil, mut visibility) in &mut held {
        if unveil.0 > 0 {
            unveil.0 -= 1;
            continue;
        }
        *visibility = Visibility::Inherited;
        commands.entity(entity).remove::<Unveil>();
    }
}

/// Keep ticks, counts, partial markers, chevrons and level buttons matching
/// the tree, without respawning anything.
#[expect(
    clippy::too_many_arguments,
    reason = "a system's parameters are its queries"
)]
pub fn update_tree_controls(
    mut commands: Commands,
    open: Res<OpenBranches>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    boxes: Query<(Entity, &TreeCheckbox, Has<Checked>)>,
    mut counts: Query<(&TreeCount, &mut Text), Without<TreeToggle>>,
    mut partials: Query<(&TreePartial, &mut Node)>,
    toggles: Query<(&TreeToggle, &Children)>,
    mut glyphs: Query<&mut Text, Without<TreeCount>>,
    mut levels: Query<(&ColorLevelButton, &mut ButtonVariant)>,
) {
    let Some(properties) = selected_properties(&selected, &panels, &sources) else {
        return;
    };
    let tree_of = |property: usize| {
        properties
            .properties
            .get(property)
            .and_then(|property| property.tree())
    };

    for (entity, checkbox, checked) in &boxes {
        let Some(tree) = tree_of(checkbox.property) else {
            continue;
        };
        let wanted = tree.checked(checkbox.node);
        if wanted != checked {
            if wanted {
                commands.entity(entity).insert(Checked);
            } else {
                commands.entity(entity).remove::<Checked>();
            }
        }
    }

    for (count, mut text) in &mut counts {
        let wanted = tree_of(count.property)
            .and_then(|tree| tree.nodes.get(count.node))
            .and_then(|node| node.value.count)
            .map(compact_count)
            .unwrap_or_default();
        if text.0 != wanted {
            text.0 = wanted;
        }
    }

    for (partial, mut node) in &mut partials {
        let wanted = display(
            tree_of(partial.property).is_some_and(|tree| tree.partly_checked(partial.node)),
        );
        if node.display != wanted {
            node.display = wanted;
        }
    }

    for (toggle, children) in &toggles {
        let icon = if open.0.contains(&(toggle.property, toggle.node)) {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        for child in children.iter() {
            if let Ok(mut glyph) = glyphs.get_mut(child)
                && glyph.0 != icon.glyph()
            {
                glyph.0 = icon.glyph().to_string();
            }
        }
    }

    for (button, mut variant) in &mut levels {
        let coloring = properties.color_by == Some(button.property)
            && tree_of(button.property).is_some_and(|tree| tree.color_level == button.level);
        variant.set_if_neq(if coloring {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        });
    }
}

/// Expand or collapse the node whose chevron was pressed.
pub fn on_toggle(
    activate: On<Activate>,
    toggles: Query<&TreeToggle>,
    mut open: ResMut<OpenBranches>,
) {
    let Ok(toggle) = toggles.get(activate.entity) else {
        return;
    };
    let key = (toggle.property, toggle.node);
    if !open.0.remove(&key) {
        open.0.insert(key);
    }
}

/// Tick or untick one node, and everything under it with it.
pub fn on_node_toggled(
    change: On<ValueChange<bool>>,
    checkboxes: Query<&TreeCheckbox>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(checkbox) = checkboxes.get(change.source) else {
        return;
    };
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    if let Some(tree) = properties
        .properties
        .get_mut(checkbox.property)
        .and_then(|property| property.tree_mut())
    {
        tree.set(checkbox.node, change.value);
        if let Some(node) = tree.nodes.get(checkbox.node) {
            info!(
                "{} {}",
                if change.value {
                    "filtering to"
                } else {
                    "no longer filtering to"
                },
                node.value.label
            );
        }
    }
}

/// Color points by the level whose button was pressed.
pub fn on_color_level(
    activate: On<Activate>,
    buttons: Query<&ColorLevelButton>,
    mut menus: Query<&mut Menu>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    // A choice made is the menu done with.
    for mut menu in &mut menus {
        menu.open = false;
    }
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    let Some(tree) = properties
        .properties
        .get_mut(button.property)
        .and_then(|property| property.tree_mut())
    else {
        return;
    };
    tree.color_level = button.level;
    let name = tree
        .levels
        .get(button.level)
        .map(|level| level.name.clone());
    properties.color_by = Some(button.property);
    if let Some(name) = name {
        info!("coloring by {name}");
    }
}
