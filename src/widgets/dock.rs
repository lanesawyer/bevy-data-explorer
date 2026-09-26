//! Docks: panels hung from an edge of the window and sized by dragging their
//! inner edge.
//!
//! The docks size themselves more than one way — the sidebar collapses to a
//! ribbon, the others never shrink past a minimum, and the two on the right
//! share that minimum-and-half-the-window rule as [`DockWidth`] — but grabbing
//! the edge, following the pointer and holding the cursor through the drag are
//! the same for all of them, and live here. So does remembering the size each
//! was dragged to, in the user's preferences.

use bevy::ecs::component::Mutable;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::window::SystemCursorIcon;
use bevy_feathers::cursor::{EntityCursor, OverrideCursor};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use bevy_feathers::tokens;

use super::{BlocksFrameInput, display, patch_node};
use crate::app::prefs::Preferences;
use crate::app::schedule::{Boot, Stage};

/// How thick a dock's drag handle is.
pub const HANDLE_PX: f32 = 6.0;

/// Draw order for the edges docks are dragged by.
///
/// Each straddles its dock's edge, so half of it lies over whatever is beside
/// the dock: the welcome screen, or another dock — the log panel runs along
/// the bottom beside the sidebar. Anything drawn over an edge hides it from
/// the pointer as well as the eye, so its cursor never shows there and it
/// cannot be grabbed. Above those; below the menus.
const DOCK_HANDLE_Z: i32 = 6;

/// The window edge a dock hangs from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockEdge {
    Left,
    Right,
    Bottom,
}

impl DockEdge {
    fn cursor(self) -> SystemCursorIcon {
        match self {
            DockEdge::Left | DockEdge::Right => SystemCursorIcon::ColResize,
            DockEdge::Bottom => SystemCursorIcon::RowResize,
        }
    }

    /// How far `cursor` is from this edge, and how far the window runs the
    /// same way, both in logical pixels.
    fn reach(self, cursor: Vec2, window: Vec2) -> (f32, f32) {
        match self {
            DockEdge::Left => (cursor.x, window.x),
            DockEdge::Right => (window.x - cursor.x, window.x),
            DockEdge::Bottom => (window.y - cursor.y, window.y),
        }
    }
}

/// A panel docked to an edge of the window.
pub trait Dock: Resource + Component<Mutability = Mutable> + FromWorld {
    /// Marks this dock's handle among every dock's.
    type Handle: Component;
    const EDGE: DockEdge;
    /// Names the dock's size in the preferences file. Changing it forgets
    /// every size saved under the old one.
    const KEY: &'static str;
    const DEFAULT_SIZE: f32;

    /// The size it was dragged to, across its edge.
    fn size(&self) -> f32;
    /// Take a size from somewhere other than a drag: a saved preference, which
    /// may have been edited by hand, or a reset.
    fn set_size(&mut self, size: f32);

    /// Whether it is at its default, which is not saved: a later change to the
    /// default then still reaches anyone who never dragged it.
    fn at_default(&self) -> bool {
        self.size() == Self::DEFAULT_SIZE
    }

    /// Back to its default size.
    fn reset(&mut self) {
        self.set_size(Self::DEFAULT_SIZE);
    }

    /// Size the dock for a drag reaching `reach` in from its edge, in a window
    /// `span` across that way.
    fn drag_to(&mut self, reach: f32, span: f32);
}

/// The most of the window a dock sized by [`DockWidth`] may take.
const MAX_FRACTION: f32 = 0.5;

/// How wide a dock is across its edge: dragged anywhere between a readable
/// minimum and half the window, and held to that half as the window narrows,
/// so that narrowing it cannot leave the docks covering every frame.
///
/// Always a usable width. Dragging does not close a dock: squeezed to nothing
/// it would still be open but invisible, with no edge left to grab. Closing is
/// its X button's job.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockWidth {
    pub px: f32,
    min: f32,
}

impl DockWidth {
    pub const fn new(px: f32, min: f32) -> Self {
        DockWidth { px, min }
    }

    /// The width it takes in a window this wide.
    pub fn within(self, window_width: f32) -> f32 {
        self.px.min(self.max(window_width))
    }

    /// Take a width from somewhere other than a drag.
    pub fn set(&mut self, px: f32) {
        self.px = px.max(self.min);
    }

    /// Follow a drag reaching `reach` in from the dock's edge.
    pub fn drag_to(&mut self, reach: f32, window_width: f32) {
        self.px = reach.clamp(self.min, self.max(window_width));
    }

    fn max(self, window_width: f32) -> f32 {
        (window_width * MAX_FRACTION).max(self.min)
    }
}

/// Place a dock hung from the right, its outer edge at `right` from the left of
/// the window, with its handle straddling its inner edge so it can be grabbed
/// from either side.
pub fn place_right_dock<'a>(
    roots: impl IntoIterator<Item = Mut<'a, Node>>,
    handles: impl IntoIterator<Item = Mut<'a, Node>>,
    open: bool,
    width: f32,
    right: f32,
) {
    for node in roots {
        patch_node(node, |node| {
            node.display = display(open);
            node.width = Val::Px(width);
            node.left = Val::Px(right - width);
        });
    }
    for node in handles {
        patch_node(node, |node| {
            node.display = display(open);
            node.left = Val::Px(right - width - HANDLE_PX * 0.5);
        });
    }
}

/// The strip along a dock's inner edge that resizes it.
#[derive(Component, Clone, Default)]
pub struct DockHandle {
    dragging: bool,
}

/// A dock's drag handle, to be placed along its inner edge by the dock.
///
/// Not a button: `ui_focus_system` drives `Interaction` on any node carrying
/// it, and a drag target wants the press without the chrome. It names its own
/// cursor for hovering; see [`hold_drag_cursor`] for the drag itself.
pub fn dock_handle(edge: DockEdge) -> impl Scene {
    let (width, height) = match edge {
        DockEdge::Left | DockEdge::Right => (Val::Px(HANDLE_PX), Val::Percent(100.0)),
        DockEdge::Bottom => (Val::Auto, Val::Px(HANDLE_PX)),
    };
    let cursor = edge.cursor();
    bsn! {
        DockHandle
        Interaction
        template_value(FocusPolicy::Block)
        BlocksFrameInput
        EntityCursor::System({ cursor })
        GlobalZIndex({ DOCK_HANDLE_Z })
        Node {
            position_type: { PositionType::Absolute },
            top: { Val::Px(0.0) },
            width: { width },
            height: { height },
        }
        ThemeBackgroundColor({ tokens::BUTTON_BG })
    }
}

/// The shading of a dock's fixed bands — a title above what scrolls, controls
/// below it — a step off the dock's own, with a rule where each meets the
/// scrolling part. Without them rows scrolled out of sight against a hard
/// edge nothing marked. The caller says which side the rule is on, by giving
/// the band a border there.
pub fn dock_band() -> impl Scene {
    bsn! {
        ThemeBackgroundColor({ tokens::PANE_BODY_BG })
        ThemeBorderColor({ tokens::PANE_HEADER_BORDER })
    }
}

/// Follow a drag of the dock's handle, for as long as the button is held.
fn drag_dock<D: Dock>(
    mut dock: ResMut<D>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut handles: Query<(&Interaction, &mut DockHandle), With<D::Handle>>,
) {
    let held = mouse.pressed(MouseButton::Left);
    let mut dragging = false;
    for (interaction, mut handle) in &mut handles {
        let now = held && (handle.dragging || *interaction == Interaction::Pressed);
        if handle.dragging != now {
            handle.dragging = now;
        }
        dragging |= now;
    }
    if !dragging {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (reach, span) = D::EDGE.reach(cursor, window.size());
    dock.drag_to(reach, span);
}

/// Hold the resize cursor for as long as the dock is being dragged.
fn hold_dock_cursor<D: Dock>(
    handles: Query<&DockHandle, With<D::Handle>>,
    mut held: Local<bool>,
    cursor: Option<ResMut<OverrideCursor>>,
) {
    let dragging = handles.iter().any(|handle| handle.dragging);
    hold_drag_cursor(dragging, &mut held, cursor, D::EDGE.cursor());
}

/// Hold `icon` as the window's cursor for as long as `dragging` is set.
///
/// Hovering a drag handle is the handle's own `EntityCursor`, which Feathers
/// applies. Feathers also sets the window's cursor every frame from whatever is
/// hovered, so a system writing the cursor directly is either overwritten or,
/// once it writes a default back, overwrites every other control's. A drag
/// outlives the hover — the edge moves out from under the pointer as soon as
/// it starts — so for its length the cursor is held through Feathers' own
/// override. Written only as a drag starts and ends, so docks do not clear
/// each other's.
pub fn hold_drag_cursor(
    dragging: bool,
    held: &mut bool,
    cursor: Option<ResMut<OverrideCursor>>,
    icon: SystemCursorIcon,
) {
    if dragging == *held {
        return;
    }
    *held = dragging;
    if let Some(mut cursor) = cursor {
        cursor.0 = dragging.then_some(EntityCursor::System(icon));
    }
}

/// Put every dock back at its default size.
#[derive(Event)]
pub struct ResetDockSizes;

/// Open the dock at the size it was left at last time.
fn restore_dock_size<D: Dock>(mut dock: ResMut<D>, prefs: Res<Preferences>) {
    if !prefs.remember_layout {
        return;
    }
    if let Some(&size) = prefs.docks.get(D::KEY)
        && size.is_finite()
    {
        dock.set_size(size);
    }
}

/// Note the dock's size whenever it changes, or once remembering is turned on.
///
/// A dock at its default is left out, so a later change to the default still
/// reaches anyone who never dragged it.
fn remember_dock_size<D: Dock>(dock: Res<D>, mut prefs: ResMut<Preferences>) {
    if !prefs.remember_layout || !(dock.is_changed() || prefs.is_changed()) {
        return;
    }
    let size = dock.size();
    if dock.at_default() {
        if prefs.docks.contains_key(D::KEY) {
            prefs.docks.remove(D::KEY);
        }
    } else if prefs.docks.get(D::KEY) != Some(&size) {
        prefs.docks.insert(D::KEY.to_string(), size);
    }
}

fn reset_dock_size<D: Dock>(
    _reset: On<ResetDockSizes>,
    mut dock: ResMut<D>,
    mut prefs: ResMut<Preferences>,
) {
    dock.reset();
    if prefs.docks.contains_key(D::KEY) {
        prefs.docks.remove(D::KEY);
    }
}

pub trait AddDock {
    /// Register a dock: its resource, the systems that drag it, and the ones
    /// that remember how big it was left.
    fn add_dock<D: Dock>(&mut self) -> &mut Self;
}

impl AddDock for App {
    fn add_dock<D: Dock>(&mut self) -> &mut Self {
        self.init_resource::<D>()
            .add_observer(reset_dock_size::<D>)
            .add_systems(Startup, restore_dock_size::<D>.in_set(Boot::Window))
            .add_systems(
                Update,
                (
                    drag_dock::<D>,
                    hold_dock_cursor::<D>,
                    remember_dock_size::<D>,
                )
                    .chain()
                    .in_set(Stage::DockInput),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dragged(reach: f32, window: f32) -> f32 {
        let mut width = DockWidth::new(300.0, 200.0);
        width.drag_to(reach, window);
        width.px
    }

    #[test]
    fn dragging_never_squeezes_a_dock_away() {
        for reach in [200.0, 10.0, 0.0, -400.0] {
            assert_eq!(dragged(reach, 1600.0), 200.0);
        }
    }

    #[test]
    fn dragging_is_clamped_to_half_the_window() {
        assert_eq!(dragged(400.0, 1600.0), 400.0);
        assert_eq!(dragged(1500.0, 1600.0), 800.0);
    }

    #[test]
    fn a_narrow_window_does_not_invert_the_clamp() {
        // Half a small window is under the minimum; the result must still be a
        // width the grid can survive.
        assert_eq!(dragged(300.0, 300.0), 200.0);
    }

    #[test]
    fn narrowing_the_window_narrows_a_wide_dock() {
        assert_eq!(DockWidth::new(800.0, 200.0).within(800.0), 400.0);
    }

    #[test]
    fn each_edge_measures_from_itself() {
        let window = Vec2::new(1600.0, 1000.0);
        let cursor = Vec2::new(1200.0, 600.0);
        assert_eq!(DockEdge::Left.reach(cursor, window), (1200.0, 1600.0));
        // Docked right, so a cursor far from that edge means a wide panel.
        assert_eq!(DockEdge::Right.reach(cursor, window), (400.0, 1600.0));
        assert_eq!(DockEdge::Bottom.reach(cursor, window), (400.0, 1000.0));
    }
}
