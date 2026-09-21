//! The help screen: how to get around, and where the project lives.
//!
//! Opened from the sidebar's footer and laid over the whole window, docks
//! included, since nothing under it is meant to be used while it is up. It
//! closes on its own button, on Escape, or on a click outside it.

use bevy::prelude::*;
use bevy_feathers::controls::ButtonVariant;
use bevy_feathers::display::{label, label_dim};

use crate::app::schedule::Boot;
use crate::widgets::{AddModal, Icon, Modal, link_button, size, spawn_modal, text};

/// Taken from the manifest, like the version, so the two cannot disagree.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
pub const AUTHOR: &str = env!("CARGO_PKG_AUTHORS");
pub const LICENSE: &str = "GPL-3.0";
pub const LICENSE_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/blob/main/LICENSE");
const NEW_ISSUE_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues/new");
const RELEASES_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases");
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

const PANEL_PX: f32 = 520.0;

/// Keys and gestures, and what each does. Kept to what works in every frame
/// or says which kind of dataset it acts on.
const SHORTCUTS: [(&str, &str); 11] = [
    ("Drag", "Pan a frame; in 3D, turn it"),
    ("Shift or right drag", "Slide a 3D frame"),
    ("Drag, selecting cells", "Draw a rectangle over the points"),
    ("Drag inside it", "Move the rectangle, keeping its size"),
    ("Scroll", "Zoom toward the pointer"),
    ("R", "Reset the selected frame's view"),
    ("\u{2190} \u{2192}  [ ]  PgUp PgDn", "Step through slices"),
    ("G", "Show every section, or one at a time"),
    ("1\u{2013}9", "Toggle an image's channels"),
    ("F12", "Show or hide the log"),
    ("Esc", "Close this screen"),
];

/// The dimmed backdrop, which is the whole screen.
#[derive(Component, Clone, Default)]
pub struct HelpScreen;

/// The sidebar button that opens it, and the one inside it that closes it.
#[derive(Component, Clone, Default)]
pub struct HelpToggle;

impl Modal for HelpScreen {
    type Toggle = HelpToggle;
}

pub fn spawn_help(mut commands: Commands) {
    let modal = spawn_modal::<HelpScreen>(&mut commands, "Bevy Data Explorer", PANEL_PX);
    let version = commands.spawn_scene(bsn! { label_dim(VERSION) }).id();
    commands.entity(modal.header).insert_children(1, &[version]);
    let panel = modal.panel;
    let intro = commands
        .spawn_scene(bsn! {
            Node { flex_direction: { FlexDirection::Column }, row_gap: { Val::Px(10.0) } }
            Children [
                label_dim(crate::ui::welcome::BLURB),
                (
                    text("Getting around", size::DOCK_TITLE)
                    Node { margin: { UiRect::top(Val::Px(6.0)) } }
                ),
            ]
        })
        .id();

    let mut rows = Vec::new();
    for (keys, action) in SHORTCUTS {
        rows.push(
            commands
                .spawn_scene(bsn! {
                    Node { column_gap: { Val::Px(12.0) } }
                    Children [
                        (
                            label(keys)
                            Node { width: { Val::Px(170.0) }, flex_shrink: { 0.0_f32 } }
                        ),
                        label_dim(action),
                    ]
                })
                .id(),
        );
    }

    let links = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(6.0) },
            }
            Children [
                (
                    text("Feedback and source", size::DOCK_TITLE)
                    Node { margin: { UiRect::top(Val::Px(6.0)) } }
                ),
                label_dim(
                    "Found a bug or want a format supported? Open an issue. For a \
                     bug, copy the log from the log panel (F12) into it."
                ),
                (
                    Node { column_gap: { Val::Px(8.0) }, flex_wrap: { FlexWrap::Wrap }, row_gap: { Val::Px(6.0) } }
                    Children [
                        link_button(Icon::Bug, "Report an issue", NEW_ISSUE_URL, ButtonVariant::Primary),
                        link_button(Icon::ExternalLink, "GitHub", REPOSITORY, ButtonVariant::Normal),
                        link_button(Icon::ExternalLink, "Releases", RELEASES_URL, ButtonVariant::Normal),
                    ]
                ),
                (
                    Node {
                        align_items: { AlignItems::Center },
                        column_gap: { Val::Px(4.0) },
                        margin: { UiRect::top(Val::Px(8.0)) },
                    }
                    Children [
                        label_dim(format!("Made by {AUTHOR} \u{00b7} Licensed under")),
                        link_button(Icon::ExternalLink, LICENSE, LICENSE_URL, ButtonVariant::Plain),
                    ]
                ),
            ]
        })
        .id();

    let mut children = vec![intro];
    children.extend(rows);
    children.push(links);
    commands.entity(panel).add_children(&children);
}

/// The help screen and the button that opens it.
pub struct HelpPlugin;

impl Plugin for HelpPlugin {
    fn build(&self, app: &mut App) {
        app.add_modal::<HelpScreen>()
            .add_systems(Startup, spawn_help.in_set(Boot::Shell));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_links_point_at_the_repository() {
        assert!(REPOSITORY.starts_with("https://github.com/"));
        for url in [LICENSE_URL, NEW_ISSUE_URL, RELEASES_URL] {
            assert!(url.starts_with(REPOSITORY), "{url}");
        }
    }
}
