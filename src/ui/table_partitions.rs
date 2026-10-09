//! The choice of which kind of record a table shows, at the top of its
//! filters.
//!
//! A button per kind, the one on screen marked, each with how many rows it
//! holds. It sits over the filters because it decides what they are: each
//! kind has its own columns, and so its own filters below.
//!
//! Like the filters it names no format. A source writes
//! [`TablePartitions`], this writes the choice back, and whatever produced
//! the rows serves the kind chosen.

use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_ui_widgets::Activate;

use crate::app::schedule::Stage;
use crate::source::grouped;
use crate::source::table::{TablePartition, TablePartitions};
use crate::view::SelectedSource;
use crate::widgets::{BlocksFrameInput, button_text, display, patch_node, size, space, text_dim};

/// The row the choice is built into: the first thing in the filters' body,
/// spawned with it, and hidden unless there is a choice to make.
#[derive(Component, Clone, Default)]
pub struct PartitionRow;

/// Anything built into the row, despawned wholesale on a rebuild.
#[derive(Component, Clone, Default)]
struct PartitionContent;

/// Shows one kind.
#[derive(Component, Clone)]
struct PartitionButton(String);

/// A kind's button caption: its name, and how many rows it holds once
/// counted.
fn caption(partition: &TablePartition) -> String {
    match partition.count {
        Some(count) => format!("{} ({})", partition.name, grouped(count as usize)),
        None => partition.name.clone(),
    }
}

/// Build a button per kind when the selection moves to another table, or its
/// kinds change.
///
/// Not when the choice does: that is written onto the buttons already there
/// by [`sync_partitions`], so the one pressed is not respawned under the
/// pointer.
fn rebuild_partitions(
    mut commands: Commands,
    selection: SelectedSource,
    tables: Query<&TablePartitions>,
    mut rows: Query<(Entity, &mut Node), With<PartitionRow>>,
    existing: Query<Entity, With<PartitionContent>>,
    mut built: Local<Option<(Entity, Vec<TablePartition>)>>,
) {
    let Ok((row, node)) = rows.single_mut() else {
        return;
    };
    let partitions = selection
        .entity()
        .and_then(|source| Some((source, tables.get(source).ok()?)));
    let fingerprint = partitions.map(|(source, it)| (source, it.partitions.clone()));
    if *built == fingerprint {
        return;
    }
    *built = fingerprint;

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    // One kind is no choice.
    let shown = partitions.filter(|(_, it)| it.partitions.len() > 1);
    patch_node(node, |node| node.display = display(shown.is_some()));
    let Some((_, partitions)) = shown else {
        return;
    };

    let heading = commands
        .spawn_scene(text_dim(partitions.label.clone(), size::SMALL))
        .insert(PartitionContent)
        .id();
    let buttons: Vec<Entity> = partitions
        .partitions
        .iter()
        .map(|partition| {
            let caption = caption(partition);
            commands
                .spawn_scene(bsn! {
                    @FeathersButton {
                        @variant: { ButtonVariant::Normal },
                        @caption: { bsn_list! {@button_text(caption)} }
                    }
                    BlocksFrameInput
                })
                .insert(PartitionButton(partition.id.clone()))
                .id()
        })
        .collect();
    let choices = commands
        .spawn((
            PartitionContent,
            Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(space::CONTROLS),
                row_gap: Val::Px(space::CONTROLS),
                ..default()
            },
        ))
        .add_children(&buttons)
        .id();
    commands.entity(row).add_children(&[heading, choices]);
}

/// Mark the kind on screen.
fn sync_partitions(
    selection: SelectedSource,
    tables: Query<&TablePartitions>,
    mut buttons: Query<(&PartitionButton, &mut ButtonVariant)>,
) {
    let Some(partitions) = selection.get(&tables) else {
        return;
    };
    for (PartitionButton(id), mut variant) in &mut buttons {
        variant.set_if_neq(if *id == partitions.chosen {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        });
    }
}

/// Show the kind whose button was pressed.
fn on_partition_pressed(
    activate: On<Activate>,
    buttons: Query<&PartitionButton>,
    selection: SelectedSource,
    mut tables: Query<&mut TablePartitions>,
) {
    let Ok(PartitionButton(id)) = buttons.get(activate.entity) else {
        return;
    };
    if let Some(mut partitions) = selection.get_mut(&mut tables) {
        partitions.choose(id);
    }
}

/// The row a table's filters start with, for their section to put first.
pub fn spawn_partition_row(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            PartitionRow,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                width: Val::Percent(100.0),
                row_gap: Val::Px(space::STACKED),
                margin: UiRect::bottom(Val::Px(space::ROWS)),
                ..default()
            },
        ))
        .id()
}

pub struct TablePartitionPlugin;

impl Plugin for TablePartitionPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_partition_pressed)
            .add_systems(Update, rebuild_partitions.in_set(Stage::ControlsBuild))
            .add_systems(Update, sync_partitions.in_set(Stage::ControlsPlace));
    }
}
