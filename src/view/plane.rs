//! Which way a frame looks through a stack: one plane, or all of them.
//!
//! A menu in the frame's header offers every view at once, as Neuroglancer
//! opens: the stack cut the other two ways, each in a frame of its own, all
//! linked so one point is shared between them and the crosshairs meet on it,
//! and a fourth showing the three slices in 3D where they cross (see
//! [`super::sections`]). Beneath that it offers the three planes one at a
//! time, and the way the dataset opens by default.
//!
//! A plane is the same address with the plane named after it (`#plane=zy`),
//! so the stack cut another way is a source of its own, read, framed, saved
//! and linked like any other, and the reader is the only thing that knows
//! what a plane is.

use bevy::prelude::*;
use bevy_feathers::controls::FeathersButton;
use bevy_ui_widgets::Activate;

use crate::formats::image::store::split_plane;
use crate::source::stack::{SliceStack, SourceAxes};
use crate::source::{ShowsSource, SourceUrl};
use crate::widgets::space;
use crate::widgets::{
    BlocksFrameInput, Icon, Menu, button_text, set_display, size, spawn_icon_menu, text, text_dim,
};

use super::link::Linked;
use super::sections::CrossSections;
use super::{DatasetRequest, DatasetTarget, Panel, PanelRequest};

/// How long frames asked for by "All views" are waited on before being given
/// up on: the other cuts are read from the store like anything else, and a
/// store that has not answered by then is not going to.
const ARRANGE_PATIENCE_SECS: f32 = 60.0;

/// The three planes through a volume, across then down, how each is
/// described, and the axis it looks along, which is what it pages through.
const PLANES: [(&str, &str, char); 3] = [
    ("xy", "Looking along z", 'z'),
    ("zy", "Looking along x", 'x'),
    ("xz", "Looking along y", 'y'),
];

/// The choice that opens every view of the stack at once.
#[derive(Component, Clone)]
pub struct AllViews {
    panel: Entity,
}

impl Default for AllViews {
    fn default() -> Self {
        AllViews {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// Frames "All views" asked for and has not yet arranged: the other cuts by
/// address, to link, and the frame onto this stack to draw the slices in 3D.
#[derive(Resource)]
pub struct Arranging {
    addresses: Vec<String>,
    sections: Option<Entity>,
    origin: Entity,
    since: f32,
}

/// A frame's plane menu, shown only over a stack.
#[derive(Component, Clone)]
pub struct PanelPlaneMenu {
    panel: Entity,
    button: Entity,
}

/// The way back to the dataset's own plane, offered only once another has
/// been chosen: before that there is nothing to go back to.
#[derive(Component, Clone)]
pub struct BackToDefault {
    panel: Entity,
}

impl Default for BackToDefault {
    fn default() -> Self {
        BackToDefault {
            panel: Entity::PLACEHOLDER,
        }
    }
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
                (
                    @FeathersButton {
                        @caption: { bsn_list![button_text("All views")] }
                    }
                    BlocksFrameInput
                    AllViews { panel: { panel } }
                ),
                text_dim(
                    "The stack cut three ways and in 3D, a frame each, linked on one point. \
                     Double-click in any of them to move it.",
                    size::SMALL
                ),
                text("One plane", size::BODY),
                {choice(Some(PLANES[0].0), PLANES[0].1)},
                {choice(Some(PLANES[1].0), PLANES[1].1)},
                {choice(Some(PLANES[2].0), PLANES[2].1)},
                (
                    Node {
                        flex_direction: { FlexDirection::Column },
                        row_gap: { Val::Px(space::STACKED) },
                    }
                    BackToDefault { panel: { panel } }
                    Children [
                        {choice(None, "Back to the default view")},
                        text_dim(
                            "Undo the plane chosen, and cut the stack the way it opens on its own.",
                            size::SMALL
                        ),
                    ]
                ),
            ]
        })
        .id();
    commands.entity(menu).add_child(content);
}

/// Offer the menu only over a stack read from an address, which is what can
/// be cut another way, and the way back only over one cut in a chosen plane.
pub fn sync_plane_menus(
    menus: Query<&PanelPlaneMenu>,
    backs: Query<(Entity, &BackToDefault)>,
    frames: Query<&ShowsSource>,
    stacks: Query<(), (With<SliceStack>, With<SourceUrl>)>,
    urls: Query<&SourceUrl>,
    mut nodes: Query<&mut Node>,
) {
    for menu in &menus {
        let shown = frames
            .get(menu.panel)
            .is_ok_and(|shows| stacks.contains(shows.0));
        set_display(&mut nodes, menu.button, shown);
    }
    for (entity, back) in &backs {
        let chosen = frames
            .get(back.panel)
            .ok()
            .and_then(|shows| urls.get(shows.0).ok())
            .is_some_and(|url| split_plane(&url.0).1.is_some());
        set_display(&mut nodes, entity, chosen);
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

/// The planes a frame looking along `along` does not show.
fn other_planes(along: Option<char>) -> impl Iterator<Item = &'static str> {
    PLANES
        .iter()
        .filter(move |(.., looking)| Some(*looking) != along)
        .map(|(plane, ..)| *plane)
}

/// Open every view of the frame's stack: the two planes it is not cut in,
/// linked with it, and the three slices in 3D.
pub fn on_all_views(
    activate: On<Activate>,
    mut commands: Commands,
    time: Res<Time>,
    buttons: Query<&AllViews>,
    frames: Query<&ShowsSource>,
    sources: Query<(&SourceUrl, Option<&SourceAxes>)>,
    mut menus: Query<(&PanelPlaneMenu, &mut Menu)>,
    mut datasets: MessageWriter<DatasetRequest>,
    mut panels: MessageWriter<PanelRequest>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    for (menu, mut open) in &mut menus {
        if menu.panel == button.panel {
            open.open = false;
        }
    }
    let Some((source, (url, axes))) = frames
        .get(button.panel)
        .ok()
        .and_then(|shows| Some((shows.0, sources.get(shows.0).ok()?)))
    else {
        return;
    };
    // The plane this frame already shows is the one it looks along.
    let along = axes
        .and_then(|axes| axes.through)
        .map(|through| through.axis);
    let (address, _) = split_plane(&url.0);
    let addresses: Vec<String> = other_planes(along)
        .map(|plane| format!("{address}#plane={plane}"))
        .collect();
    for url in &addresses {
        datasets.write(DatasetRequest {
            url: url.clone(),
            target: DatasetTarget::NewFrame,
        });
    }
    panels.write(PanelRequest::Open(source));
    commands.entity(button.panel).insert(Linked::default());
    commands.insert_resource(Arranging {
        addresses,
        sections: Some(source),
        origin: button.panel,
        since: time.elapsed_secs(),
    });
}

/// Link each frame "All views" asked for as it opens, and have the new one
/// onto the same stack draw the slices in 3D.
pub fn arrange_views(
    mut commands: Commands,
    time: Res<Time>,
    arranging: Option<ResMut<Arranging>>,
    frames: Query<(Entity, &ShowsSource, Has<Linked>, Has<CrossSections>), With<Panel>>,
    urls: Query<&SourceUrl>,
) {
    let Some(mut arranging) = arranging else {
        return;
    };
    for (panel, shows, linked, sections) in &frames {
        if let Ok(url) = urls.get(shows.0)
            && let Some(at) = arranging.addresses.iter().position(|it| *it == url.0)
        {
            arranging.addresses.remove(at);
            if !linked {
                commands.entity(panel).insert(Linked::default());
            }
            continue;
        }
        if arranging.sections == Some(shows.0) && panel != arranging.origin && !linked && !sections
        {
            commands.entity(panel).insert(CrossSections::default());
            arranging.sections = None;
        }
    }
    let waited = time.elapsed_secs() - arranging.since > ARRANGE_PATIENCE_SECS;
    if (arranging.addresses.is_empty() && arranging.sections.is_none()) || waited {
        commands.remove_resource::<Arranging>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_views_opens_the_planes_a_frame_is_not_cut_in() {
        // A stack looking down z is joined by the cuts along x and along y.
        assert_eq!(other_planes(Some('z')).collect::<Vec<_>>(), ["zy", "xz"]);
        // The sagittal projection looks along x.
        assert_eq!(other_planes(Some('x')).collect::<Vec<_>>(), ["xy", "xz"]);
        // Without knowing, every plane is asked for.
        assert_eq!(other_planes(None).count(), 3);
    }
}
