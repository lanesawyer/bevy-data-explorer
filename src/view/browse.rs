//! The browser a frame shows while it is choosing what to show.
//!
//! An empty frame is where a dataset is found: a search over everything open
//! and everything the catalogs offer, narrowed by category, which also reads
//! any address pasted into it. A frame that already shows something can browse
//! too, from its dropdown, and keeps showing it underneath until something
//! else is chosen.
//!
//! The browser covers the frame's cell and stands in for its header, which
//! would otherwise name a dataset the frame may not have. Choosing from it is
//! choosing from the frame's dropdown: the frame is repointed, and repointing
//! it is what ends the browsing.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersToolButton};
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_ui_widgets::Activate;

use super::dataset_menu::{PickerTarget, spawn_dataset_browser};
use super::overlay::PanelHeader;
use super::{Browsing, FrameArea, MAX_PANELS, Panel, PanelRequest, PendingShow, ShowFailed};
use crate::app::schedule::Stage;
use crate::app::theme::token;
use crate::catalog::Catalogs;
use crate::source::{DataSource, ShowsSource};
use crate::widgets::space;
use crate::widgets::{
    BlocksFrameInput, Icon, button_icon, button_text, patch_node, set_text, size, text,
};
use crate::widgets::{Notice, Tone, notice};

/// Over a frame's own chrome, which it stands in for, and a table's rows, but
/// under the menus that open over everything.
const BROWSE_Z: i32 = 4;

/// Wide enough for a name and its caption, and no wider: a list stretched
/// across a wide frame is hard to read along.
const COLUMN_PX: f32 = 720.0;

/// Opens an empty frame to browse from.
#[derive(Component, Clone, Default)]
pub struct NewFrameButton;

/// The button that opens an empty frame, wherever one is offered.
pub fn new_frame_button(label: &'static str, variant: ButtonVariant) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: { bsn_list![button_text(label)] },
            @variant: { variant }
        }
        BlocksFrameInput
        NewFrameButton
    }
}

pub fn on_new_frame(
    activate: On<Activate>,
    buttons: Query<(), With<NewFrameButton>>,
    mut requests: MessageWriter<PanelRequest>,
) {
    if buttons.contains(activate.entity) {
        requests.write(PanelRequest::Browse(None));
    }
}

/// Hold the new frame buttons while the grid has no room for another.
pub fn sync_new_frame_buttons(
    mut commands: Commands,
    panels: Query<(), With<Panel>>,
    buttons: Query<(Entity, Has<InteractionDisabled>), With<NewFrameButton>>,
) {
    let full = panels.iter().count() >= MAX_PANELS;
    for (entity, disabled) in &buttons {
        if full && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else if !full && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}

/// The browser over one frame.
#[derive(Component, Clone)]
pub struct BrowsePane {
    panel: Entity,
}

impl Default for BrowsePane {
    fn default() -> Self {
        BrowsePane {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// What the browser is for, at its top.
#[derive(Component, Clone, Default)]
pub struct BrowseTitle;

/// How the last choice is going: being read, or why it could not be.
#[derive(Component, Clone, Default)]
pub struct BrowseStatus;

/// Closes the browser: the frame with it, if it showed nothing.
#[derive(Component, Clone)]
pub struct BrowseClose {
    panel: Entity,
}

impl Default for BrowseClose {
    fn default() -> Self {
        BrowseClose {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// Keep one browser per browsing frame, and give a new one the keyboard so
/// typing searches straight away.
pub fn sync_browse_panes(
    mut commands: Commands,
    catalogs: Res<Catalogs>,
    panels: Query<Entity, (With<Panel>, With<Browsing>)>,
    panes: Query<(Entity, &BrowsePane)>,
    mut focus: ResMut<InputFocus>,
) {
    for (entity, pane) in &panes {
        if !panels.contains(pane.panel) {
            commands.entity(entity).despawn();
        }
    }
    for panel in &panels {
        if panes.iter().any(|(_, pane)| pane.panel == panel) {
            continue;
        }
        let field = spawn_pane(&mut commands, &catalogs, panel);
        focus.set(field, FocusCause::Navigated);
    }
}

/// Build the browser over `panel`, returning its search field.
fn spawn_pane(commands: &mut Commands, catalogs: &Catalogs, panel: Entity) -> Entity {
    let pane = commands
        .spawn_scene(bsn! {
            BrowsePane { panel: { panel } }
            BlocksFrameInput
            Node {
                position_type: { PositionType::Absolute },
                flex_direction: { FlexDirection::Column },
                align_items: { AlignItems::Center },
                padding: { UiRect::all(Val::Px(space::PANEL_INSET)) },
            }
            ThemeBackgroundColor({ token::FRAME_BG })
            GlobalZIndex({ BROWSE_Z })
            InheritableFont { font_size: { 13.0f32 } }
        })
        .id();

    let column = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                max_width: { Val::Px(COLUMN_PX) },
                flex_grow: { 1.0_f32 },
                min_height: { Val::Px(0.0) },
                row_gap: { Val::Px(space::ROWS) },
            }
        })
        .id();

    let header = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(space::CONTROLS) },
            }
            Children [
                (
                    text("", size::FRAME_TITLE)
                    BrowseTitle
                    Node { flex_grow: { 1.0_f32 } }
                ),
                (
                    @FeathersToolButton {
                        @caption: { bsn_list![button_icon(Icon::X)] }
                    }
                    BrowseClose { panel: { panel } }
                ),
            ]
        })
        .id();

    let status = commands
        .spawn_scene(bsn! {
            notice()
            BrowseStatus
        })
        .id();

    let (browser, field) = spawn_dataset_browser(commands, catalogs, PickerTarget::Frame(panel));
    commands
        .entity(column)
        .add_children(&[header, status, browser]);
    commands.entity(pane).add_child(column);
    field
}

/// Cover each browsing frame's cell.
pub fn place_browse_panes(
    area: Res<FrameArea>,
    panels: Query<&Panel>,
    mut panes: Query<(&BrowsePane, &mut Node)>,
) {
    let count = panels.iter().count();
    for (pane, node) in &mut panes {
        let Ok(panel) = panels.get(pane.panel) else {
            continue;
        };
        let cell = area.cell(count, panel.index);
        patch_node(node, |node| {
            node.left = Val::Px(cell.min.x);
            node.top = Val::Px(cell.min.y);
            node.width = Val::Px(cell.width());
            node.height = Val::Px(cell.height());
        });
    }
}

/// Hide the header of a browsing frame, which the browser stands in for.
pub fn hide_browsing_headers(
    panels: Query<Has<Browsing>, With<Panel>>,
    mut headers: Query<(&PanelHeader, &mut Node)>,
) {
    for (header, node) in &mut headers {
        let browsing = panels.get(header.panel).unwrap_or(false);
        let wanted = if browsing {
            Display::None
        } else {
            Display::Flex
        };
        patch_node(node, |node| node.display = wanted);
    }
}

/// Say what each browser is for, and how its last choice is going.
pub fn sync_browse_text(
    panels: Query<(
        Option<&ShowsSource>,
        Option<&PendingShow>,
        Option<&ShowFailed>,
    )>,
    sources: Query<&DataSource>,
    panes: Query<(&BrowsePane, &Children)>,
    columns: Query<&Children>,
    mut titles: Query<&mut Text, With<BrowseTitle>>,
    mut statuses: Query<&mut Notice, With<BrowseStatus>>,
) {
    for (pane, children) in &panes {
        let Ok((shows, pending, failed)) = panels.get(pane.panel) else {
            continue;
        };
        let title = match shows.and_then(|shows| sources.get(shows.0).ok()) {
            Some(source) => format!("Replace {}", source.name),
            None => "Open a dataset".to_string(),
        };
        let status = match (pending, failed) {
            (Some(pending), _) => {
                Notice::new(Tone::Info, format!("Reading {}\u{2026}", pending.name))
            }
            (None, Some(failed)) => Notice::new(Tone::Error, failed.0.clone()),
            (None, None) => Notice::default(),
        };
        let descendants = children
            .iter()
            .flat_map(|column| columns.get(column).into_iter().flatten().copied())
            .flat_map(|child| {
                std::iter::once(child).chain(columns.get(child).into_iter().flatten().copied())
            });
        for entity in descendants {
            if let Ok(text) = titles.get_mut(entity) {
                set_text(text, &title);
            }
            if let Ok(mut shown) = statuses.get_mut(entity) {
                shown.set_if_neq(status.clone());
            }
        }
    }
}

/// Close a browser: an empty frame goes with it, and one showing something
/// goes back to showing it.
pub fn on_browse_close(
    activate: On<Activate>,
    mut commands: Commands,
    buttons: Query<&BrowseClose>,
    showing: Query<(), With<ShowsSource>>,
    mut requests: MessageWriter<PanelRequest>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    if showing.contains(button.panel) {
        commands
            .entity(button.panel)
            .remove::<(Browsing, ShowFailed)>();
    } else {
        requests.write(PanelRequest::Close(button.panel));
    }
}

/// The browser over each frame choosing what to show.
pub struct BrowsePlugin;

impl Plugin for BrowsePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_browse_close)
            .add_observer(on_new_frame)
            .add_systems(
                Update,
                (
                    sync_browse_panes,
                    place_browse_panes,
                    hide_browsing_headers,
                    sync_browse_text,
                    sync_new_frame_buttons,
                )
                    .chain()
                    .in_set(Stage::Chrome),
            );
    }
}
