//! Which plane a frame cuts a stack in.
//!
//! A menu in the frame's header offers the three ways through a volume, and
//! the way the dataset opens by default. Choosing one asks for the same
//! address with the plane named after it (`#plane=zy`), in this frame — so
//! the stack cut another way is a source of its own, read, framed, saved and
//! linked like any other, and the reader is the only thing that knows what a
//! plane is.

use bevy::prelude::*;
use bevy_feathers::controls::FeathersButton;
use bevy_ui_widgets::Activate;

use crate::formats::image::store::split_plane;
use crate::source::stack::SliceStack;
use crate::source::{ShowsSource, SourceUrl};
use crate::widgets::space;
use crate::widgets::{
    BlocksFrameInput, Icon, Menu, button_text, set_display, size, spawn_icon_menu, text, text_dim,
};

use super::{DatasetRequest, DatasetTarget};

/// The three planes through a volume, across then down, and how each is
/// described: by the axis it looks along, which is what it pages through.
const PLANES: [(&str, &str); 3] = [
    ("xy", "Looking along z"),
    ("zy", "Looking along x"),
    ("xz", "Looking along y"),
];

/// A frame's plane menu, shown only over a stack.
#[derive(Component, Clone)]
pub struct PanelPlaneMenu {
    panel: Entity,
    button: Entity,
}

/// One choice in the menu: a plane, or none for the dataset's own.
#[derive(Component, Clone)]
pub struct PlaneChoice {
    panel: Entity,
    plane: Option<&'static str>,
}

impl Default for PlaneChoice {
    fn default() -> Self {
        PlaneChoice {
            panel: Entity::PLACEHOLDER,
            plane: None,
        }
    }
}

/// Add a frame's plane menu to its header.
pub(super) fn spawn_plane_menu(commands: &mut Commands, header: Entity, panel: Entity) {
    // Hidden and shown by `sync_plane_menus`, which patches the display on
    // the button's own node rather than replacing it: a new `Node` would
    // drop the size Feathers gives a tool button.
    let (button, menu) = spawn_icon_menu(commands, header, Icon::Axis3d);
    commands
        .entity(menu)
        .insert(PanelPlaneMenu { panel, button });
    let choice = |plane: Option<&'static str>, label: &'static str| {
        bsn! {
            @FeathersButton {
                @caption: { bsn_list![button_text(label)] }
            }
            BlocksFrameInput
            PlaneChoice { panel: { panel }, plane: { plane } }
        }
    };
    let content = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(space::ROWS) },
            }
            Children [
                text("Plane", size::BODY),
                text_dim("Which way the stack is cut. Each opens as a dataset of its own.", size::SMALL),
                {choice(None, "As the dataset opens")},
                {choice(Some(PLANES[0].0), PLANES[0].1)},
                {choice(Some(PLANES[1].0), PLANES[1].1)},
                {choice(Some(PLANES[2].0), PLANES[2].1)},
            ]
        })
        .id();
    commands.entity(menu).add_child(content);
}

/// Offer the menu only over a stack read from an address, which is what can
/// be cut another way.
pub fn sync_plane_menus(
    menus: Query<&PanelPlaneMenu>,
    frames: Query<&ShowsSource>,
    stacks: Query<(), (With<SliceStack>, With<SourceUrl>)>,
    mut nodes: Query<&mut Node>,
) {
    for menu in &menus {
        let shown = frames
            .get(menu.panel)
            .is_ok_and(|shows| stacks.contains(shows.0));
        set_display(&mut nodes, menu.button, shown);
    }
}

/// Show the frame's dataset cut in the plane chosen.
pub fn on_plane_chosen(
    activate: On<Activate>,
    choices: Query<&PlaneChoice>,
    frames: Query<&ShowsSource>,
    urls: Query<&SourceUrl>,
    mut menus: Query<(&PanelPlaneMenu, &mut Menu)>,
    mut requests: MessageWriter<DatasetRequest>,
) {
    let Ok(choice) = choices.get(activate.entity) else {
        return;
    };
    for (menu, mut open) in &mut menus {
        if menu.panel == choice.panel {
            open.open = false;
        }
    }
    let Some(url) = frames
        .get(choice.panel)
        .ok()
        .and_then(|shows| urls.get(shows.0).ok())
    else {
        return;
    };
    let (address, _) = split_plane(&url.0);
    let url = match choice.plane {
        Some(plane) => format!("{address}#plane={plane}"),
        None => address.to_string(),
    };
    requests.write(DatasetRequest {
        url,
        target: DatasetTarget::Show(choice.panel),
    });
}
