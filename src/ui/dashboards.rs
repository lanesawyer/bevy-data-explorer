//! Each data source's front page, on the home page.
//!
//! A card per catalog with a [`Dashboard`], shown while its source is on and
//! filled from whatever blocks the catalog composed: its headline numbers, a
//! bar for each part of a breakdown, and a button for each dataset it
//! offers. Nothing here knows which source it is drawing, so a new one needs
//! nothing but a catalog that has a dashboard.
//!
//! The cards are spawned once, with the welcome screen; what is inside one is
//! thrown away and built again whenever its dashboard changes, which is
//! rarely — when it arrives, and when signing in or out has it counted again.

use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_ui_widgets::Activate;

use crate::app::schedule::Stage;
use crate::app::theme::token;
use crate::catalog::dashboard::{Bar, Block, Dashboard, DashboardState, Figure};
use crate::catalog::{Catalogs, DashboardId, Entry};
use crate::source::compact_count;
use crate::ui::settings::WhileSourceOn;
use crate::view::{DatasetRequest, DatasetTarget};
use crate::widgets::{
    BlocksFrameInput, Notice, Tone, button_text, field_well, notice, size, space, spawn_skeleton,
    text, text_dim, truncate_to_width, width_of,
};

/// Width a card is held to, which fits two blocks side by side.
pub const CARD_PX: f32 = 2.0 * BLOCK_PX + space::SCREEN_WIDE + 2.0 * space::CONTROL_INSET + 2.0;

/// Width of each block after the headline numbers. Two sit side by side in a
/// wide frame area and wrap one under the other in a narrow one.
const BLOCK_PX: f32 = 440.0;

/// The narrowest a headline number is given, so four share a row of a card.
const FIGURE_PX: f32 = 180.0;

/// What a button takes around its caption, its padding and border together.
const BUTTON_INSET_PX: f32 = (space::CONTROL_INSET + space::SEAM) * 2.0;

/// Width of the bars in a breakdown, and their height.
const BAR_WIDTH_PX: f32 = 140.0;
const BAR_PX: f32 = 8.0;

/// A card, and the part of it rebuilt when its dashboard changes.
#[derive(Component)]
pub struct DashboardCard {
    id: DashboardId,
    body: Entity,
}

/// Opens a dataset a dashboard offers.
#[derive(Component)]
struct DashboardDataset(String);

/// A card for each catalog with a dashboard, empty until [`fill_dashboards`]
/// fills it.
pub fn spawn_dashboards(commands: &mut Commands, catalogs: &Catalogs) -> Vec<Entity> {
    catalogs
        .dashboards()
        .map(|view| {
            let name = view.name.to_string();
            let heading = commands.spawn_scene(text(name, size::DOCK_TITLE)).id();
            let mut parts = vec![heading];
            if let Some(provider) = view.provider {
                parts.push(
                    commands
                        .spawn_scene(text_dim(provider.about, size::SECONDARY))
                        .id(),
                );
            }
            let body = commands
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    width: Val::Percent(100.0),
                    row_gap: Val::Px(space::SCREEN_GAP),
                    margin: UiRect::top(Val::Px(space::HEADING)),
                    ..default()
                })
                .id();
            parts.push(body);
            let card = commands
                .spawn_scene(bsn! {
                    field_well()
                    Node {
                        width: { Val::Px(CARD_PX) },
                        max_width: { Val::Percent(100.0) },
                        row_gap: { Val::Px(space::STACKED) },
                    }
                })
                .insert(DashboardCard { id: view.id, body })
                .add_children(&parts)
                .id();
            if let Some(provider) = view.provider {
                commands.entity(card).insert(WhileSourceOn {
                    key: provider.key,
                    shown: Display::Flex,
                });
            }
            card
        })
        .collect()
}

/// Build each card's inside again whenever the dashboards change.
pub fn fill_dashboards(
    mut commands: Commands,
    catalogs: Res<Catalogs>,
    cards: Query<&DashboardCard>,
    mut drawn: Local<Option<usize>>,
) {
    let generation = catalogs.dashboards_generation();
    if *drawn == Some(generation) || cards.is_empty() {
        return;
    }
    *drawn = Some(generation);
    for view in catalogs.dashboards() {
        for card in cards.iter().filter(|card| card.id == view.id) {
            commands.entity(card.body).despawn_children();
            let parts = match view.state {
                DashboardState::Waiting | DashboardState::Loading => {
                    vec![spawn_skeleton(&mut commands, 4, 18.0)]
                }
                DashboardState::Failed(problem) => {
                    let problem = Notice::new(Tone::Error, problem.clone());
                    vec![commands.spawn_scene(notice()).insert(problem).id()]
                }
                DashboardState::Ready(dashboard) => spawn_blocks(&mut commands, dashboard),
            };
            commands.entity(card.body).add_children(&parts);
        }
    }
}

/// The headline numbers across the top, and every other block under them.
fn spawn_blocks(commands: &mut Commands, dashboard: &Dashboard) -> Vec<Entity> {
    let mut parts = Vec::new();
    let mut rest = Vec::new();
    for block in &dashboard.blocks {
        match block {
            Block::Figures(figures) => parts.push(spawn_figures(commands, figures)),
            Block::Breakdown { title, note, bars } => {
                rest.push(spawn_breakdown(commands, title, note.as_deref(), bars));
            }
            Block::Datasets {
                title,
                note,
                entries,
            } => rest.push(spawn_datasets(commands, title, note.as_deref(), entries)),
        }
    }
    let grid = commands
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(space::SCREEN_WIDE),
            row_gap: Val::Px(space::SCREEN_GAP),
            ..default()
        })
        .add_children(&rest)
        .id();
    parts.push(grid);
    parts
}

/// A row of numbers, each over what it counts.
fn spawn_figures(commands: &mut Commands, figures: &[Figure]) -> Entity {
    let tiles: Vec<Entity> = figures
        .iter()
        .map(|figure| {
            let value = compact_count(figure.value);
            let label = figure.label.clone();
            let tile = commands
                .spawn_scene(bsn! {
                    Node {
                        flex_direction: { FlexDirection::Column },
                        flex_grow: { 1.0_f32 },
                        flex_basis: { Val::Px(FIGURE_PX) },
                        row_gap: { Val::Px(space::STACKED) },
                    }
                    Children [
                        text(value, size::SCREEN_HEADING),
                        text(label, size::SECONDARY),
                    ]
                })
                .id();
            if let Some(note) = &figure.note {
                let note = commands
                    .spawn_scene(text_dim(note.clone(), size::SMALL))
                    .id();
                commands.entity(tile).add_child(note);
            }
            tile
        })
        .collect();
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(space::GROUPS),
            row_gap: Val::Px(space::GROUPS),
            ..default()
        })
        .add_children(&tiles)
        .id()
}

/// A block's title over what is in it, at the width every block is.
fn spawn_block(commands: &mut Commands, title: &str, note: Option<&str>) -> Entity {
    let title = title.to_string();
    let block = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Px(BLOCK_PX) },
                max_width: { Val::Percent(100.0) },
                row_gap: { Val::Px(space::LIST_ITEMS) },
            }
            Children [
                (
                    text(title, size::BODY)
                    Node { margin: { UiRect::bottom(Val::Px(space::STACKED)) } }
                ),
            ]
        })
        .id();
    if let Some(note) = note {
        let note = commands
            .spawn_scene(bsn! {
                text_dim(note.to_string(), size::SECONDARY)
                Node { margin: { UiRect::bottom(Val::Px(space::STACKED)) } }
            })
            .id();
        commands.entity(block).add_child(note);
    }
    block
}

/// A row for each bar: what it is, the bar against the largest, and the
/// count.
fn spawn_breakdown(
    commands: &mut Commands,
    title: &str,
    note: Option<&str>,
    bars: &[Bar],
) -> Entity {
    let block = spawn_block(commands, title, note);
    let largest = bars.iter().map(|bar| bar.value).max().unwrap_or(1).max(1);
    let mut cells = Vec::new();
    for bar in bars {
        let label = bar.label.clone();
        let count = compact_count(bar.value);
        let share = bar.value as f32 / largest as f32 * 100.0;
        let fill = commands
            .spawn((
                Node {
                    width: Val::Percent(share),
                    // A part too small to see at this scale is still there.
                    min_width: Val::Px(BAR_PX / 2.0),
                    height: Val::Percent(100.0),
                    border_radius: BorderRadius::all(Val::Px(BAR_PX / 2.0)),
                    ..default()
                },
                ThemeBackgroundColor(token::SELECTION),
            ))
            .id();
        let track = commands
            .spawn((
                Node {
                    width: Val::Px(BAR_WIDTH_PX),
                    height: Val::Px(BAR_PX),
                    border_radius: BorderRadius::all(Val::Px(BAR_PX / 2.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                ThemeBackgroundColor(token::TRACK),
            ))
            .add_child(fill)
            .id();
        let label = commands
            .spawn_scene(bsn! {
                text_dim(label, size::SECONDARY)
                TextLayout { linebreak: { LineBreak::NoWrap } }
                Node { min_width: { Val::Px(0.0) }, overflow: { Overflow::clip() } }
            })
            .id();
        let count = commands
            .spawn_scene(bsn! {
                text(count, size::SECONDARY)
                TextLayout { justify: { Justify::Right } }
            })
            .id();
        cells.extend([label, track, count]);
    }
    let rows = commands
        .spawn(Node {
            display: Display::Grid,
            grid_template_columns: vec![
                GridTrack::minmax(
                    MinTrackSizingFunction::Px(0.0),
                    MaxTrackSizingFunction::Fraction(1.0),
                ),
                GridTrack::auto(),
                GridTrack::auto(),
            ],
            align_items: AlignItems::Center,
            column_gap: Val::Px(space::GROUPS),
            row_gap: Val::Px(space::ROWS),
            ..default()
        })
        .add_children(&cells)
        .id();
    commands.entity(block).add_child(rows);
    block
}

/// A button for each dataset, with its kind beside it.
///
/// A name too long for its button is cut to what the kinds leave room for,
/// ending in an ellipsis, rather than clipped at both ends by the centered
/// caption. The clip stays for a card narrowed past its width.
fn spawn_datasets(
    commands: &mut Commands,
    title: &str,
    note: Option<&str>,
    entries: &[Entry],
) -> Entity {
    let block = spawn_block(commands, title, note);
    let widest_kind = entries
        .iter()
        .map(|entry| entry.kind.chars().count())
        .max()
        .unwrap_or(0);
    let room = BLOCK_PX - width_of(widest_kind, size::SECONDARY) - space::GROUPS - BUTTON_INSET_PX;
    let mut cells = Vec::new();
    for entry in entries {
        let name = truncate_to_width(&entry.name, room, size::BODY);
        let kind = entry.kind.clone();
        let button = commands
            .spawn_scene(bsn! {
                @FeathersButton {
                    @variant: { ButtonVariant::Normal },
                    @caption: { bsn_list![(
                        button_text(name)
                        TextLayout { linebreak: { LineBreak::NoWrap } }
                    )] }
                }
                BlocksFrameInput
                Node {
                    min_width: { Val::Px(0.0) },
                    overflow: { Overflow::clip() },
                }
            })
            .insert(DashboardDataset(entry.url.clone()))
            .id();
        let kind = commands.spawn_scene(text_dim(kind, size::SECONDARY)).id();
        cells.extend([button, kind]);
    }
    let grid = commands
        .spawn(Node {
            display: Display::Grid,
            grid_template_columns: vec![
                GridTrack::minmax(
                    MinTrackSizingFunction::Px(0.0),
                    MaxTrackSizingFunction::Fraction(1.0),
                ),
                GridTrack::auto(),
            ],
            align_items: AlignItems::Center,
            column_gap: Val::Px(space::GROUPS),
            row_gap: Val::Px(space::LIST_ITEMS),
            ..default()
        })
        .add_children(&cells)
        .id();
    commands.entity(block).add_child(grid);
    block
}

/// Open the dataset whose button was pressed, the way an example is.
fn on_dataset_pressed(
    activate: On<Activate>,
    buttons: Query<&DashboardDataset>,
    mut requests: MessageWriter<DatasetRequest>,
) {
    let Ok(DashboardDataset(url)) = buttons.get(activate.entity) else {
        return;
    };
    requests.write(DatasetRequest {
        url: url.clone(),
        target: DatasetTarget::NewFrame,
    });
}

pub struct DashboardsPlugin;

impl Plugin for DashboardsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_dataset_pressed)
            .add_systems(Update, fill_dashboards.in_set(Stage::ControlsBuild));
    }
}
