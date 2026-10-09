//! A screen laid over the whole window: Feathers' modal dialog, with a title
//! and a button to close it.
//!
//! Help, settings and the lightbox are each one of these, and close the same
//! ways: their own X, Escape, or a click outside the dialog. One exists only
//! while it is open. Opening spawns it and closing despawns it, which is what
//! Feathers' dialog expects: it takes the keyboard as it appears and keeps Tab
//! inside it until it goes.
//!
//! What goes in it is the modal's own business. [`ModalParts`] lands on the
//! dialog once its scene has spawned, and each modal fills its body from an
//! observer of that, with whatever it needs to read to do so.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::scene::Ready;
use bevy_feathers::controls::{
    FeathersDialog, FeathersDialogBody, FeathersDialogClose, FeathersDialogHeader,
};
use bevy_feathers::theme::ThemedText;
use bevy_ui_widgets::{Activate, RequestClose, ScrollArea};

use super::BlocksFrameInput;
use super::space;
use crate::app::schedule::Stage;

/// The tallest a dialog's body grows, as a share of the window, before it
/// scrolls.
const BODY_MAX_HEIGHT_VH: f32 = 80.0;

/// Marks every modal, whichever modal it is: the backdrop that covers the
/// window, holding the dialog.
#[derive(Component, Clone, Default)]
pub struct ModalScreen;

/// Marks a modal, on its backdrop. `Toggle` marks every button that opens or
/// closes it.
pub trait Modal: Component + Clone + Default + Unpin {
    type Toggle: Component + Default;
    const TITLE: &'static str;
    const WIDTH: Val;
}

/// Where a modal's contents go, put on its backdrop once its dialog has
/// spawned.
#[derive(Component, Clone, Copy)]
pub struct ModalParts {
    /// The dialog's column, below the header.
    pub body: Entity,
    /// The title row: the title, a spacer, then the X. Anything inserted at
    /// index 1 sits beside the title.
    pub header: Entity,
}

#[derive(Component, Clone, Default)]
struct ModalBody;

#[derive(Component, Clone, Default)]
struct ModalHeader;

/// The title, which holds the keyboard as the modal opens. Feathers otherwise
/// hands it to the last control in the dialog and rings it, which drew the
/// eye to the help screen's license link. A title draws no ring, and with the
/// keyboard inside the dialog Escape and Tab work as before.
#[derive(Component, Clone, Default)]
struct ModalTitle;

/// Open `modal`, titled `title` and `width` wide.
pub fn spawn_modal<M: Modal>(
    commands: &mut Commands,
    modal: M,
    title: impl Into<String>,
    width: Val,
) -> Entity {
    let title = title.into();
    commands
        .spawn_scene(bsn! {
            @FeathersDialog {
                @width: { width },
                @contents: { bsn_list! {
                    @FeathersDialogHeader
                    ModalHeader
                    Children [
                        Text({ title }) ThemedText ModalTitle TabIndex(-1)
                        --
                        Node { flex_grow: { 1.0_f32 } }
                        --
                        @FeathersDialogClose
                    ]
                    --
                    @FeathersDialogBody
                    ModalBody
                    ScrollArea
                    Node {
                        max_height: { Val::Vh(BODY_MAX_HEIGHT_VH) },
                        row_gap: { Val::Px(space::ROWS) },
                        overflow: { Overflow::scroll_y() },
                    }
                } },
            }
            ~{ modal }
            ModalScreen
            BlocksFrameInput
            on(find_parts)
        })
        .id()
}

/// Hand a modal the entities to fill, once its dialog has spawned.
fn find_parts(
    ready: On<Ready>,
    screens: Query<(), (With<ModalScreen>, Without<ModalParts>)>,
    children: Query<&Children>,
    bodies: Query<(), With<ModalBody>>,
    headers: Query<(), With<ModalHeader>>,
    titles: Query<(), With<ModalTitle>>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if !screens.contains(ready.entity) {
        return;
    }
    let body = children
        .iter_descendants(ready.entity)
        .find(|entity| bodies.contains(*entity));
    let header = children
        .iter_descendants(ready.entity)
        .find(|entity| headers.contains(*entity));
    if let Some(title) = children
        .iter_descendants(ready.entity)
        .find(|entity| titles.contains(*entity))
    {
        focus.set(title, FocusCause::Navigated);
    }
    if let (Some(body), Some(header)) = (body, header) {
        commands
            .entity(ready.entity)
            .insert(ModalParts { body, header });
    }
}

/// Close `M`, if it is open.
pub fn close_modal<M: Modal>(commands: &mut Commands, open: &Query<Entity, With<M>>) {
    for screen in open {
        commands.entity(screen).try_despawn();
    }
}

fn on_toggle<M: Modal>(
    activate: On<Activate>,
    toggles: Query<(), With<M::Toggle>>,
    open: Query<Entity, With<M>>,
    mut commands: Commands,
) {
    if !toggles.contains(activate.entity) {
        return;
    }
    if open.is_empty() {
        spawn_modal(&mut commands, M::default(), M::TITLE, M::WIDTH);
    } else {
        close_modal(&mut commands, &open);
    }
}

/// Close a modal its X or its backdrop asked to close.
fn on_request_close(
    close: On<RequestClose>,
    screens: Query<(), With<ModalScreen>>,
    mut commands: Commands,
) {
    if screens.contains(close.event_target()) {
        commands.entity(close.event_target()).try_despawn();
    }
}

/// Close on Escape wherever the keyboard is. Feathers' own Escape reaches the
/// dialog only while something in it has the keyboard, and a click on its
/// text takes the keyboard away.
fn close_on_escape<M: Modal>(
    keys: Res<ButtonInput<KeyCode>>,
    open: Query<Entity, With<M>>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) {
        close_modal(&mut commands, &open);
    }
}

pub trait AddModal {
    /// Register a modal's opening and closing. Filling it is the caller's,
    /// from an observer of [`ModalParts`] landing on it.
    fn add_modal<M: Modal>(&mut self) -> &mut Self;
}

impl AddModal for App {
    fn add_modal<M: Modal>(&mut self) -> &mut Self {
        self.add_observer(on_toggle::<M>)
            .add_systems(Update, close_on_escape::<M>.in_set(Stage::ControlsRead))
    }
}

/// What every modal shares, registered once.
pub struct ModalPlugin;

impl Plugin for ModalPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_request_close);
    }
}
