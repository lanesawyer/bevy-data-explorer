//! The docked side panel.
//!
//! Reserves space off the left of the frame grid for controls that do not
//! belong to any one frame — filters and dataset information, eventually. It
//! can be dragged to any width between a readable minimum and half the window,
//! or collapsed to a ribbon.
//!
//! It does not talk to the grid directly. It takes a slice off [`FrameArea`],
//! which is the only thing the panels and their chrome measure against, so
//! nothing over there has to know the sidebar exists.

use bevy::prelude::*;
use bevy::ui::Interaction;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};

use crate::panel::{BlocksFrameInput, FrameArea};

/// Width when collapsed. Enough for the short title and the toggle beneath it.
const RIBBON_PX: f32 = 52.0;
/// Narrowest useful expanded width.
const MIN_PX: f32 = 180.0;
/// The sidebar never takes more than this fraction of the window.
const MAX_FRACTION: f32 = 0.5;
/// Dragging the edge inside this collapses the sidebar rather than fighting the
/// minimum width.
const COLLAPSE_BELOW_PX: f32 = 120.0;

const HANDLE_PX: f32 = 6.0;
const TOGGLE_PX: f32 = 24.0;

const FULL_TITLE: &str = "Bevy Data Explorer";
const SHORT_TITLE: &str = "BDE";

#[derive(Resource)]
pub struct Sidebar {
    /// Width when expanded. Kept across a collapse so reopening restores it.
    pub width: f32,
    pub collapsed: bool,
    resizing: bool,
}

impl Default for Sidebar {
    fn default() -> Self {
        Sidebar {
            width: 260.0,
            collapsed: false,
            resizing: false,
        }
    }
}

impl Sidebar {
    pub fn resizing(&self) -> bool {
        self.resizing
    }

    /// Width the sidebar actually occupies right now.
    pub fn current_width(&self) -> f32 {
        if self.collapsed {
            RIBBON_PX
        } else {
            self.width
        }
    }

    pub fn title(&self) -> &'static str {
        if self.collapsed {
            SHORT_TITLE
        } else {
            FULL_TITLE
        }
    }

    /// Width a drag to `cursor_x` should produce, and whether that collapses it.
    ///
    /// Returned rather than applied so the rule can be checked directly: a drag
    /// well inside the minimum collapses instead of sticking at the minimum,
    /// which would otherwise leave no way to close it by dragging.
    fn width_for_drag(cursor_x: f32, window_width: f32) -> Option<f32> {
        if cursor_x < COLLAPSE_BELOW_PX {
            return None;
        }
        Some(cursor_x.clamp(MIN_PX, (window_width * MAX_FRACTION).max(MIN_PX)))
    }
}

#[derive(Component, Clone, Default)]
pub struct SidebarRoot;

#[derive(Component, Clone, Default)]
pub struct SidebarTitle;

#[derive(Component, Clone, Default)]
pub struct SidebarToggle;

/// The draggable edge.
#[derive(Component, Clone, Default)]
pub struct SidebarHandle;

/// The build's version, taken from the manifest so the two cannot disagree.
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

/// The version shown in the footer, hidden when the dock is a ribbon.
#[derive(Component, Clone, Default)]
pub struct SidebarVersion;

/// The column that accordions are added to.
///
/// Sections are attached here rather than to the root so that the sidebar owns
/// its own chrome and callers only ever append content.
#[derive(Component, Clone, Default)]
pub struct SidebarContent;

pub fn spawn_sidebar(commands: &mut Commands) {
    commands.spawn_scene(bsn! {
        SidebarRoot
        Node {
            position_type: { PositionType::Absolute },
            left: { Val::Px(0.0) },
            top: { Val::Px(0.0) },
            height: { Val::Percent(100.0) },
            flex_direction: { FlexDirection::Column },
            row_gap: { Val::Px(8.0) },
            padding: { UiRect::all(Val::Px(10.0)) },
        }
        BackgroundColor({ Color::srgb(0.09, 0.10, 0.13) })
        Children [
            (
                SidebarTitle
                Text({ FULL_TITLE.to_string() })
                TextFont { font_size: { bevy::text::FontSize::Px(15.0) } }
                TextColor({ Color::srgb(0.90, 0.93, 0.97) })
            ),
            (
                SidebarContent
                Node {
                    flex_direction: { FlexDirection::Column },
                    width: { Val::Percent(100.0) },
                    row_gap: { Val::Px(6.0) },
                    overflow: { Overflow::clip() },
                }
            ),
            (
                // The footer sits on the bottom edge: an automatic top margin
                // eats the free space above it, so it stays there whether the
                // sections are showing or the dock is collapsed to its ribbon.
                Node {
                    width: { Val::Percent(100.0) },
                    align_items: { AlignItems::Center },
                    justify_content: { JustifyContent::SpaceBetween },
                    margin: { UiRect::top(Val::Auto) },
                }
                Children [
                    (
                        SidebarToggle
                        Button
                        BlocksFrameInput
                        Node {
                            width: { Val::Px(TOGGLE_PX) },
                            height: { Val::Px(TOGGLE_PX) },
                            justify_content: { JustifyContent::Center },
                            align_items: { AlignItems::Center },
                            border_radius: { BorderRadius::all(Val::Px(4.0)) },
                        }
                        BackgroundColor({ Color::srgba(0.18, 0.20, 0.26, 0.85) })
                        Children [(
                            Text({ "<".to_string() })
                            TextFont { font_size: { bevy::text::FontSize::Px(14.0) } }
                            TextColor({ Color::srgb(0.85, 0.9, 0.95) })
                        )]
                    ),
                    (
                        SidebarVersion
                        Text({ VERSION.to_string() })
                        TextFont { font_size: { bevy::text::FontSize::Px(13.0) } }
                        TextColor({ Color::srgb(0.58, 0.64, 0.73) })
                    ),
                ]
            ),
        ]
    });

    commands.spawn_scene(bsn! {
        SidebarHandle
        Button
        BlocksFrameInput
        Node {
            position_type: { PositionType::Absolute },
            top: { Val::Px(0.0) },
            width: { Val::Px(HANDLE_PX) },
            height: { Val::Percent(100.0) },
        }
        BackgroundColor({ Color::srgb(0.20, 0.22, 0.28) })
    });
}

/// Take the sidebar's width off the frame grid.
pub fn reserve_space(sidebar: Res<Sidebar>, mut area: ResMut<FrameArea>) {
    area.reserve_left(sidebar.current_width());
}

/// Drag the edge to resize, or far enough left to collapse.
pub fn resize_sidebar(
    mut sidebar: ResMut<Sidebar>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    handle: Query<&Interaction, With<SidebarHandle>>,
) {
    let Ok(window) = windows.single() else { return };

    if handle.iter().any(|i| *i == Interaction::Pressed) {
        sidebar.resizing = true;
    }
    if !mouse.pressed(MouseButton::Left) {
        sidebar.resizing = false;
        return;
    }
    if !sidebar.resizing {
        return;
    }

    let Some(cursor) = window.cursor_position() else {
        return;
    };
    match Sidebar::width_for_drag(cursor.x, window.width()) {
        Some(width) => {
            sidebar.collapsed = false;
            sidebar.width = width;
        }
        None => sidebar.collapsed = true,
    }
}

/// Show a resize cursor over the drag handle, and for as long as a drag lasts.
///
/// The handle is a thin target, so without this it reads as decoration rather
/// than something to grab.
pub fn sidebar_cursor(
    mut commands: Commands,
    sidebar: Res<Sidebar>,
    windows: Query<(Entity, Option<&CursorIcon>), With<PrimaryWindow>>,
    handle: Query<&Interaction, With<SidebarHandle>>,
) {
    // Keep the cursor while dragging even once the pointer has left the
    // handle, which it does as soon as the edge starts moving.
    let over_handle = sidebar.resizing() || handle.iter().any(|i| *i != Interaction::None);
    let wanted = if over_handle {
        CursorIcon::System(SystemCursorIcon::ColResize)
    } else {
        CursorIcon::System(SystemCursorIcon::Default)
    };

    for (window, current) in &windows {
        if current != Some(&wanted) {
            commands.entity(window).insert(wanted.clone());
        }
    }
}

pub fn toggle_sidebar(
    mut sidebar: ResMut<Sidebar>,
    pressed: Query<&Interaction, (Changed<Interaction>, With<SidebarToggle>)>,
) {
    if pressed.iter().any(|i| *i == Interaction::Pressed) {
        sidebar.collapsed = !sidebar.collapsed;
    }
}

/// Match the sidebar's chrome to its current width and state.
pub fn update_sidebar(
    sidebar: Res<Sidebar>,
    mut roots: Query<&mut Node, (With<SidebarRoot>, Without<SidebarHandle>)>,
    mut handles: Query<&mut Node, (With<SidebarHandle>, Without<SidebarRoot>)>,
    mut content: Query<
        &mut Node,
        (
            With<SidebarContent>,
            Without<SidebarRoot>,
            Without<SidebarHandle>,
            Without<SidebarVersion>,
        ),
    >,
    mut version: Query<
        &mut Node,
        (
            With<SidebarVersion>,
            Without<SidebarRoot>,
            Without<SidebarHandle>,
            Without<SidebarContent>,
        ),
    >,
    titles: Query<Entity, With<SidebarTitle>>,
    toggles: Query<&Children, With<SidebarToggle>>,
    mut texts: Query<&mut Text>,
) {
    let width = sidebar.current_width();
    for mut node in &mut roots {
        node.width = Val::Px(width);
    }
    for mut node in &mut content {
        // Collapsed, the ribbon is too narrow to lay sections out in.
        node.display = if sidebar.collapsed {
            Display::None
        } else {
            Display::Flex
        };
    }
    for mut node in &mut version {
        // The ribbon has room for the toggle and nothing beside it.
        node.display = if sidebar.collapsed {
            Display::None
        } else {
            Display::Flex
        };
    }
    for mut node in &mut handles {
        // Straddles the edge so it can be grabbed from either side.
        node.left = Val::Px(width - HANDLE_PX * 0.5);
    }
    for entity in &titles {
        if let Ok(mut text) = texts.get_mut(entity) {
            let wanted = sidebar.title();
            if text.0 != wanted {
                text.0 = wanted.to_string();
            }
        }
    }
    for children in &toggles {
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child) {
                let wanted = if sidebar.collapsed { ">" } else { "<" };
                if text.0 != wanted {
                    text.0 = wanted.to_string();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_comes_from_the_manifest() {
        // Read at compile time from CARGO_PKG_VERSION, so a release cannot
        // leave the footer claiming an older build.
        assert_eq!(VERSION, format!("v{}", env!("CARGO_PKG_VERSION")));
        assert!(VERSION.starts_with('v'));
        assert_eq!(VERSION.matches('.').count(), 2, "expected v#.#.#");
    }

    #[test]
    fn collapsing_keeps_the_expanded_width_for_reopening() {
        let mut sidebar = Sidebar::default();
        sidebar.width = 320.0;
        sidebar.collapsed = true;
        assert_eq!(sidebar.current_width(), RIBBON_PX);
        sidebar.collapsed = false;
        assert_eq!(sidebar.current_width(), 320.0);
    }

    #[test]
    fn the_title_shortens_when_collapsed() {
        let mut sidebar = Sidebar::default();
        assert_eq!(sidebar.title(), "Bevy Data Explorer");
        sidebar.collapsed = true;
        assert_eq!(sidebar.title(), "BDE");
    }

    #[test]
    fn dragging_past_the_minimum_collapses_rather_than_sticking() {
        // Without this the edge would jam at the minimum width and there would
        // be no way to close the sidebar by dragging.
        assert_eq!(Sidebar::width_for_drag(40.0, 1600.0), None);
        assert_eq!(Sidebar::width_for_drag(0.0, 1600.0), None);
    }

    #[test]
    fn dragging_is_clamped_between_the_minimum_and_half_the_window() {
        assert_eq!(Sidebar::width_for_drag(150.0, 1600.0), Some(MIN_PX));
        assert_eq!(Sidebar::width_for_drag(400.0, 1600.0), Some(400.0));
        assert_eq!(Sidebar::width_for_drag(1500.0, 1600.0), Some(800.0));
    }

    #[test]
    fn a_narrow_window_still_leaves_room_for_the_grid() {
        // Half of a tiny window is below the minimum; the clamp must not invert
        // and hand back a width wider than the window.
        let width = Sidebar::width_for_drag(300.0, 200.0).unwrap();
        assert_eq!(width, MIN_PX);
    }

    #[test]
    fn reserving_space_never_takes_more_than_the_window() {
        let mut area = FrameArea {
            origin: Vec2::ZERO,
            size: Vec2::new(100.0, 600.0),
        };
        area.reserve_left(400.0);
        assert_eq!(area.origin.x, 100.0);
        assert_eq!(area.size.x, 0.0);
    }

    #[test]
    fn reserving_space_shifts_the_grid_right() {
        let mut area = FrameArea {
            origin: Vec2::ZERO,
            size: Vec2::new(1600.0, 900.0),
        };
        area.reserve_left(260.0);
        assert_eq!(area.origin, Vec2::new(260.0, 0.0));
        assert_eq!(area.size, Vec2::new(1340.0, 900.0));
    }
}
