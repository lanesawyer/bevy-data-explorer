//! The help screen: how to get around, and where the project lives.
//!
//! Opened from the sidebar's footer and laid over the whole window, docks
//! included, since nothing under it is meant to be used while it is up. It
//! closes on its own button, on Escape, or on a click outside it.

use bevy::prelude::*;
use bevy::ui::{FocusPolicy, Interaction};
use bevy_feathers::controls::{ButtonVariant, FeathersToolButton};
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_feathers::tokens;
use bevy_ui_widgets::Activate;

use crate::app::schedule::{Boot, Stage};
use crate::widgets::{BlocksFrameInput, Icon, button_icon, link_button, size, text};

/// Taken from the manifest, like the version, so the two cannot disagree.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
pub const AUTHOR: &str = env!("CARGO_PKG_AUTHORS");
pub const LICENSE: &str = "GPL-3.0";
pub const LICENSE_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/blob/main/LICENSE");
const NEW_ISSUE_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues/new");
const RELEASES_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases");
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

/// Above the menus, since it covers everything they could open over.
const HELP_Z: i32 = 20;
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

pub fn spawn_help(mut commands: Commands) {
    let screen = commands
        .spawn_scene(bsn! {
            HelpScreen
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
            BackgroundColor({ Color::srgba(0.0, 0.0, 0.0, 0.5) })
            GlobalZIndex({ HELP_Z })
            InheritableFont { font_size: { 13.0f32 } }
        })
        .observe(close_on_backdrop)
        .id();

    let panel = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Px(PANEL_PX) },
                max_width: { Val::Percent(90.0) },
                max_height: { Val::Percent(90.0) },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(10.0) },
                padding: { UiRect::all(Val::Px(18.0)) },
                border_radius: { BorderRadius::all(Val::Px(8.0)) },
                overflow: { Overflow::scroll_y() },
            }
            bevy_ui_widgets::ScrollArea
            ThemeBackgroundColor({ tokens::WINDOW_BG })
            Children [
                (
                    Node {
                        width: { Val::Percent(100.0) },
                        align_items: { AlignItems::Center },
                        column_gap: { Val::Px(8.0) },
                    }
                    Children [
                        (
                            text("Bevy Data Explorer", size::SCREEN_HEADING)
                        ),
                        (
                            label_dim(VERSION)
                            Node { flex_grow: { 1.0_f32 } }
                        ),
                        (
                            @FeathersToolButton {
                                @caption: { bsn_list![button_icon(Icon::X)] }
                            }
                            HelpToggle
                        ),
                    ]
                ),
                label_dim(crate::ui::welcome::BLURB),
                (
                    text("Getting around", size::DOCK_TITLE)
                    Node { margin: { UiRect::top(Val::Px(6.0)) } }
                ),
            ]
        })
        // A click on the panel is not a click on the backdrop behind it.
        .observe(|mut click: On<Pointer<Click>>| click.propagate(false))
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

    let mut children = rows;
    children.push(links);
    commands.entity(panel).add_children(&children);
    commands.entity(screen).add_child(panel);
}

fn set_open(screens: &mut Query<&mut Node, With<HelpScreen>>, open: Option<bool>) {
    for mut node in screens {
        let now_open = open.unwrap_or(node.display == Display::None);
        node.display = if now_open {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub fn on_help_toggle(
    activate: On<Activate>,
    toggles: Query<(), With<HelpToggle>>,
    mut screens: Query<&mut Node, With<HelpScreen>>,
) {
    if toggles.get(activate.entity).is_ok() {
        set_open(&mut screens, None);
    }
}

fn close_on_backdrop(_click: On<Pointer<Click>>, mut screens: Query<&mut Node, With<HelpScreen>>) {
    set_open(&mut screens, Some(false));
}

pub fn close_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut screens: Query<&mut Node, With<HelpScreen>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        set_open(&mut screens, Some(false));
    }
}

/// The help screen and the button that opens it.
pub struct HelpPlugin;

impl Plugin for HelpPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_help_toggle)
            .add_systems(Update, close_on_escape.in_set(Stage::ControlsRead))
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
