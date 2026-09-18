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
use bevy::ui::{FocusPolicy, Interaction};
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor};
use bevy_feathers::tokens;
use bevy_ui_widgets::Activate;

use crate::app::schedule::{Boot, Stage};
use crate::app::theme::ThemeMode;
use crate::view::{BlocksFrameInput, FrameArea};
use crate::widgets::{Icon, button_icon};

/// Width when collapsed. Enough for the short title and the toggle beneath it.
const RIBBON_PX: f32 = 52.0;
/// Narrowest useful expanded width.
const MIN_PX: f32 = 220.0;
/// The sidebar never takes more than this fraction of the window.
const MAX_FRACTION: f32 = 0.5;
/// Dragging the edge inside this collapses the sidebar rather than fighting the
/// minimum width.
const COLLAPSE_BELOW_PX: f32 = 120.0;

const HANDLE_PX: f32 = 6.0;

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
            width: 320.0,
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

/// The button that switches between the light and dark themes.
#[derive(Component, Clone, Default)]
pub struct ThemeButton;

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

/// Where a section sits in the sidebar, ascending.
///
/// Sections are spawned by whichever plugin owns them, so without this their
/// order is the order those plugins happened to be registered in — which put
/// "View configuration" below "Cell properties" the moment each section became
/// its own plugin. Stating the position means adding a section cannot silently
/// reshuffle the ones already there.
#[derive(Component, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SectionOrder(pub u32);

/// Put the sections in their stated order, once they have all been spawned.
fn order_sections(
    mut commands: Commands,
    content: Query<(Entity, &Children), With<SidebarContent>>,
    order: Query<&SectionOrder>,
) {
    let Ok((parent, children)) = content.single() else {
        return;
    };
    let mut sections: Vec<Entity> = children.iter().collect();
    // A section that states no position keeps its spawn order, after those
    // that do.
    sections.sort_by_key(|entity| {
        order
            .get(*entity)
            .copied()
            .unwrap_or(SectionOrder(u32::MAX))
    });
    commands.entity(parent).replace_children(&sections);
}

fn spawn_sidebar(mut commands: Commands) {
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
        // Through a token rather than a literal: the theme repaints everything
        // that names one, and a dock painted by hand would stay dark while the
        // controls inside it went light.
        ThemeBackgroundColor({ tokens::WINDOW_BG })
        Children [
            (
                SidebarTitle
                Text({ FULL_TITLE.to_string() })
                TextFont { font_size: { FontSize::Px(15.0) } }
                ThemeTextColor({ tokens::TEXT_MAIN })
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
                // Stacked above the footer and pushed down with it: these act on
                // the app rather than on any section, so they sit with the
                // control that owns the dock itself. One per row, because the
                // ribbon is too narrow to hold two side by side.
                Node {
                    width: { Val::Percent(100.0) },
                    align_items: { AlignItems::Center },
                    margin: { UiRect::top(Val::Auto) },
                }
                Children [(
                    @FeathersToolButton {
                        @caption: { bsn_list![button_icon(Icon::ScrollText)] }
                    }
                    crate::ui::logpanel::LogPanelToggle
                    BlocksFrameInput
                )]
            ),
            (
                Node {
                    width: { Val::Percent(100.0) },
                    align_items: { AlignItems::Center },
                }
                Children [(
                    @FeathersToolButton {
                        @caption: { bsn_list![button_icon(Icon::Sun)] }
                    }
                    ThemeButton
                    BlocksFrameInput
                )]
            ),
            (
                // The footer sits on the bottom edge: the margin above belongs
                // to the theme row now, so both stay down there whether the
                // sections are showing or the dock is collapsed to its ribbon.
                Node {
                    width: { Val::Percent(100.0) },
                    align_items: { AlignItems::Center },
                    justify_content: { JustifyContent::SpaceBetween },
                }
                Children [
                    (
                        @FeathersToolButton {
                            @caption: { bsn_list![button_icon(Icon::PanelLeftClose)] }
                        }
                        SidebarToggle
                        BlocksFrameInput
                    ),
                    (
                        SidebarVersion
                        Text({ VERSION.to_string() })
                        TextFont { font_size: { FontSize::Px(13.0) } }
                        ThemeTextColor({ tokens::TEXT_DIM })
                    ),
                ]
            ),
        ]
    });

    commands.spawn_scene(bsn! {
        SidebarHandle
        // Not a button: `ui_focus_system` drives `Interaction` on any node
        // carrying it, and a drag target wants the press without the chrome.
        Interaction
        template_value(FocusPolicy::Block)
        BlocksFrameInput
        Node {
            position_type: { PositionType::Absolute },
            top: { Val::Px(0.0) },
            width: { Val::Px(HANDLE_PX) },
            height: { Val::Percent(100.0) },
        }
        ThemeBackgroundColor({ tokens::BUTTON_BG })
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
    activate: On<Activate>,
    toggles: Query<(), With<SidebarToggle>>,
    mut sidebar: ResMut<Sidebar>,
) {
    if toggles.get(activate.entity).is_ok() {
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
                let wanted = if sidebar.collapsed {
                    Icon::PanelLeftOpen
                } else {
                    Icon::PanelLeftClose
                }
                .glyph();
                if text.0 != wanted {
                    text.0 = wanted.to_string();
                }
            }
        }
    }
}

/// Switch the theme when the button is pressed.
pub fn on_theme_pressed(
    activate: On<Activate>,
    buttons: Query<(), With<ThemeButton>>,
    mut mode: ResMut<ThemeMode>,
) {
    if buttons.get(activate.entity).is_ok() {
        mode.toggle();
        info!(
            "switched to the {} theme",
            if mode.is_dark() { "dark" } else { "light" }
        );
    }
}

/// Keep the theme button showing the theme it would switch to.
pub fn sync_theme_button(
    mode: Res<ThemeMode>,
    buttons: Query<&Children, With<ThemeButton>>,
    mut texts: Query<&mut Text>,
) {
    let wanted = if mode.is_dark() {
        Icon::Sun
    } else {
        Icon::Moon
    }
    .glyph();
    for children in &buttons {
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child)
                && text.0 != wanted
            {
                text.0 = wanted.to_string();
            }
        }
    }
}

/// The dock on the left, and the space it claims from the grid.
pub struct SidebarPlugin;

impl Plugin for SidebarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sidebar>()
            .add_observer(toggle_sidebar)
            .add_observer(on_theme_pressed)
            .add_systems(
                Update,
                (resize_sidebar, sidebar_cursor)
                    .chain()
                    .in_set(Stage::DockInput),
            )
            .add_systems(Update, reserve_space.in_set(Stage::DockReserve))
            .add_systems(
                Update,
                (update_sidebar, sync_theme_button)
                    .chain()
                    .in_set(Stage::Chrome),
            )
            .add_systems(Startup, spawn_sidebar.in_set(Boot::Shell))
            .add_systems(Startup, order_sections.in_set(Boot::DockOrder));
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
        let mut sidebar = Sidebar {
            width: 320.0,
            collapsed: true,
            ..Default::default()
        };
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
    fn the_collapse_threshold_stays_below_the_minimum_width() {
        // Between the two the drag snaps out to the minimum. If the threshold
        // ever passed it, the sidebar would collapse the moment it was dragged
        // narrower rather than resisting at its minimum.
        const { assert!(COLLAPSE_BELOW_PX < MIN_PX) };
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
