//! A screen laid over the whole window: a dimmed backdrop and a panel in the
//! middle of it, with a title and a button to close it.
//!
//! Help and settings are both one of these, and open and close the same ways:
//! the button that opens it, its own X, Escape, or a click outside the panel.
//!
//! The panel is centered in whatever the backdrop's padding leaves. That is
//! set from above, by `ui::center_modals`, since what the frames occupy is the
//! grid's business rather than a widget's.

use bevy::prelude::*;
use bevy::ui::{FocusPolicy, Interaction};
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_feathers::tokens;
use bevy_ui_widgets::{Activate, ScrollArea};

use super::space;
use super::{BlocksFrameInput, Icon, button_icon, size, text};
use crate::app::schedule::Stage;

/// Above the menus, since it covers everything they could open over.
const MODAL_Z: i32 = 20;
/// Space between the panel's edge and what is in it, on every side.
const PADDING_PX: f32 = space::SCREEN_INSET;

/// Marks every modal's backdrop, whichever modal it is, for what places them
/// all alike.
#[derive(Component, Clone, Default)]
pub struct ModalScreen;

/// Marks a modal's backdrop. `Toggle` marks every button that opens or closes
/// it, its own X among them.
pub trait Modal: Component + Default {
    type Toggle: Component + Default;
}

/// What [`spawn_modal`] built, for the caller to fill.
pub struct ModalParts {
    /// The panel's column, below the header.
    pub panel: Entity,
    /// The title row: the title, a spacer, then the X. Anything inserted at
    /// index 1 sits beside the title.
    pub header: Entity,
}

/// Spawn a closed modal `width` wide, titled `title`.
pub fn spawn_modal<M: Modal>(
    commands: &mut Commands,
    title: &'static str,
    width: f32,
) -> ModalParts {
    let screen = commands
        .spawn_scene(bsn! {
            BlocksFrameInput
            // It holds buttons, but the backdrop around them does not, and a
            // click there has to stop at it rather than reach a frame.
            Interaction
            template_value(FocusPolicy::Block)
            Node {
                position_type: { PositionType::Absolute },
                display: { Display::None },
                width: { Val::Percent(100.0) },
                height: { Val::Percent(100.0) },
                justify_content: { JustifyContent::Center },
                align_items: { AlignItems::Center },
            }
            BackgroundColor({ Color::srgba(0.0, 0.0, 0.0, 0.7) })
            GlobalZIndex({ MODAL_Z })
            InheritableFont { font_size: { 13.0f32 } }
        })
        .insert((ModalScreen, M::default()))
        .observe(close_on_backdrop::<M>)
        .id();

    let panel = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Px(width) },
                max_width: { Val::Percent(90.0) },
                max_height: { Val::Percent(90.0) },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(space::ROWS) },
                // The scrollbar takes its lane out of the right padding.
                padding: { UiRect::all(Val::Px(PADDING_PX)) },
                border_radius: { BorderRadius::all(Val::Px(8.0)) },
                overflow: { Overflow::scroll_y() },
            }
            ScrollArea
            ThemeBackgroundColor({ tokens::WINDOW_BG })
        })
        // A click on the panel is not a click on the backdrop behind it.
        .observe(|mut click: On<Pointer<Click>>| click.propagate(false))
        .id();

    let close = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_icon(Icon::X)] }
            }
        })
        .insert(M::Toggle::default())
        .id();

    // Children added one by one rather than from a scene, so that where a
    // caller inserts beside the title is certain.
    let title = commands
        .spawn_scene(bsn! { text(title, size::SCREEN_HEADING) })
        .id();
    let spacer = commands
        .spawn(Node {
            flex_grow: 1.0,
            ..default()
        })
        .id();
    let header = commands
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(space::CONTROLS),
            ..default()
        })
        .add_children(&[title, spacer, close])
        .id();

    commands.entity(panel).add_child(header);
    commands.entity(screen).add_child(panel);
    ModalParts { panel, header }
}

/// Open `M`, close it, or with `None` switch it.
pub fn set_modal_open<M: Modal>(screens: &mut Query<&mut Node, With<M>>, open: Option<bool>) {
    for mut node in screens {
        let now_open = open.unwrap_or(node.display == Display::None);
        node.display = if now_open {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn on_toggle<M: Modal>(
    activate: On<Activate>,
    toggles: Query<(), With<M::Toggle>>,
    mut screens: Query<&mut Node, With<M>>,
) {
    if toggles.contains(activate.entity) {
        set_modal_open::<M>(&mut screens, None);
    }
}

fn close_on_backdrop<M: Modal>(_click: On<Pointer<Click>>, mut screens: Query<&mut Node, With<M>>) {
    set_modal_open::<M>(&mut screens, Some(false));
}

fn close_on_escape<M: Modal>(
    keys: Res<ButtonInput<KeyCode>>,
    mut screens: Query<&mut Node, With<M>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        set_modal_open::<M>(&mut screens, Some(false));
    }
}

pub trait AddModal {
    /// Register a modal's toggling and closing. Spawning it is the caller's,
    /// with [`spawn_modal`].
    fn add_modal<M: Modal>(&mut self) -> &mut Self;
}

impl AddModal for App {
    fn add_modal<M: Modal>(&mut self) -> &mut Self {
        self.add_observer(on_toggle::<M>)
            .add_systems(Update, close_on_escape::<M>.in_set(Stage::ControlsRead))
    }
}
