//! Each data source's front page, on the home page.
//!
//! A card per catalog with a [`Dashboard`], shown while its source is on and
//! filled from whatever blocks the catalog composed: its headline numbers, a
//! bar for each part of a breakdown, and a button for each dataset it
//! offers. Nothing here knows which source it is drawing, so a new one needs
//! nothing but a catalog that has a dashboard.
//!
//! One is shown at a time, picked from a list down the left the way the
//! settings screen's pages are, so the home page stays one dashboard tall
//! however many sources have one. Only the sources turned on are listed, and
//! the list itself only once there are two to choose between.
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
use crate::view::{DatasetRequest, DatasetTarget};
use crate::widgets::{
    BlocksFrameInput, Notice, Tone, button_text, display, field_well, notice, patch_node, size,
    space, spawn_skeleton, text, text_dim, truncate_to_width, width_of,
};

/// Width a card is held to, which fits two blocks side by side.
pub const CARD_PX: f32 = 2.0 * BLOCK_PX + space::SCREEN_WIDE + 2.0 * space::CONTROL_INSET + 2.0;

/// Width of each block after the headline numbers. Two sit side by side in a
/// wide frame area and wrap one under the other in a narrow one.
const BLOCK_PX: f32 = 440.0;

/// The narrowest a headline number is given, so four share a row of a card.
const FIGURE_PX: f32 = 180.0;

/// The list of dashboards down the left, wide enough for a source's name on
/// one line: "Brain Knowledge Platform" wrapped at 180.
const NAV_PX: f32 = 220.0;

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

/// The button in the list that shows a dashboard.
#[derive(Component)]
struct DashboardTab(DashboardId);

/// The list of dashboards, hidden while there is only one to show.
#[derive(Component)]
struct DashboardNav;

/// The dashboard shown. Nothing until the first is picked for it, and moved
/// on to another if its source is turned off.
#[derive(Resource, Default)]
struct ShownDashboard(Option<DashboardId>);

/// The dashboards: a button for each down the left, and a card for each
/// beside them, one card shown at a time.
pub fn spawn_dashboards(commands: &mut Commands, catalogs: &Catalogs) -> Entity {
    let tabs: Vec<Entity> = catalogs
        .dashboards()
        .map(|view| {
            let name = view.name.to_string();
            commands
                .spawn_scene(bsn! {
                    @FeathersButton {
                        @variant: { ButtonVariant::Plain },
                        @caption: { bsn_list![(
                            button_text(name)
                            TextLayout { linebreak: { LineBreak::NoWrap } }
                        )] }
                    }
                    Node { justify_content: { JustifyContent::Start } }
                    BlocksFrameInput
                })
                .insert(DashboardTab(view.id))
                .id()
        })
        .collect();
    let nav = commands
        .spawn((
            DashboardNav,
            Node {
                width: Val::Px(NAV_PX),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(space::LIST_ITEMS),
                ..default()
            },
        ))
        .add_children(&tabs)
        .id();
    let cards = spawn_cards(commands, catalogs);
    commands
        .spawn(Node {
            max_width: Val::Percent(100.0),
            align_items: AlignItems::FlexStart,
            column_gap: Val::Px(space::SCREEN_INSET),
            ..default()
        })
        .add_child(nav)
        .add_children(&cards)
        .id()
}

/// A card for each catalog with a dashboard, empty until [`fill_dashboards`]
/// fills it and hidden until [`show_dashboard`] picks it.
fn spawn_cards(commands: &mut Commands, catalogs: &Catalogs) -> Vec<Entity> {
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
            commands
                .spawn_scene(bsn! {
                    field_well()
                    Node {
                        display: { Display::None },
                        width: { Val::Px(CARD_PX) },
                        min_width: { Val::Px(0.0) },
                        flex_shrink: { 1.0_f32 },
                        row_gap: { Val::Px(space::STACKED) },
                    }
                })
                .insert(DashboardCard { id: view.id, body })
                .add_children(&parts)
                .id()
        })
        .collect()
}

/// Show the page picked, among those whose source is on, and mark its
/// button; list only the dashboards that are on, and only when there is a
/// choice.
fn show_dashboard(
    catalogs: Res<Catalogs>,
    mut shown: ResMut<ShownDashboard>,
    mut tabs: Query<(&DashboardTab, &mut ButtonVariant, &mut Node), Without<DashboardCard>>,
    mut cards: Query<(&DashboardCard, &mut Node), Without<DashboardTab>>,
    mut navs: Query<
        &mut Node,
        (
            With<DashboardNav>,
            Without<DashboardTab>,
            Without<DashboardCard>,
        ),
    >,
) {
    let on: Vec<DashboardId> = catalogs
        .dashboards()
        .filter(|view| {
            view.provider
                .is_none_or(|provider| catalogs.source_on(provider.key))
        })
        .map(|view| view.id)
        .collect();
    let picked = shown_of(shown.0, &on);
    if shown.0 != picked {
        shown.0 = picked;
    }
    for (DashboardTab(id), mut variant, node) in &mut tabs {
        variant.set_if_neq(if picked == Some(*id) {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Plain
        });
        patch_node(node, |node| node.display = display(on.contains(id)));
    }
    for (card, node) in &mut cards {
        patch_node(node, |node| node.display = display(picked == Some(card.id)));
    }
    for node in &mut navs {
        patch_node(node, |node| node.display = display(on.len() > 1));
    }
}

/// The dashboard to show among those `on`: the one picked while it is still
/// on, and otherwise the first that is.
fn shown_of<T: Copy + PartialEq>(picked: Option<T>, on: &[T]) -> Option<T> {
    picked
        .filter(|picked| on.contains(picked))
        .or_else(|| on.first().copied())
}

fn on_tab_pressed(
    activate: On<Activate>,
    tabs: Query<&DashboardTab>,
    mut shown: ResMut<ShownDashboard>,
) {
    if let Ok(DashboardTab(id)) = tabs.get(activate.entity)
        && shown.0 != Some(*id)
    {
        shown.0 = Some(*id);
    }
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
        app.init_resource::<ShownDashboard>()
            .add_observer(on_dataset_pressed)
            .add_observer(on_tab_pressed)
            .add_systems(Update, fill_dashboards.in_set(Stage::ControlsBuild))
            .add_systems(Update, show_dashboard.in_set(Stage::ControlsPlace));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_dashboard_is_shown_until_another_is_picked() {
        assert_eq!(shown_of(None, &[2, 5]), Some(2));
        assert_eq!(shown_of(Some(5), &[2, 5]), Some(5));
    }

    #[test]
    fn turning_off_the_source_shown_moves_on_to_one_still_on() {
        assert_eq!(shown_of(Some(5), &[2]), Some(2));
        assert_eq!(shown_of(Some(5), &[]), None);
    }
}
