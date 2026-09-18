//! The debug panel: the log, where a user can see it.
//!
//! Docked along the bottom, closed until asked for with `F12` or the button in
//! the sidebar's footer, and dragged taller or shorter by its top edge. It
//! takes its height off the frame grid like the sidebar takes its width, so the frames shrink rather than being covered:
//! whatever a report is about is usually still on screen while the log
//! explaining it is being read.
//!
//! The point of it is the **copy** button. A log a user can read is useful; a
//! log they can paste into a message is what actually comes back.
//!
//! The lines are rebuilt only when something has been logged, because the panel
//! holds a few hundred text entities and a viewer streaming tiles logs nothing
//! most frames.

use bevy::input::ButtonInput;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::display::label;
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::{ThemeBackgroundColor, UiTheme};
use bevy_feathers::tokens;
use bevy_ui_widgets::{Activate, ScrollArea};

use crate::app::logs::LogTail;
use crate::app::schedule::{Boot, Stage};
use crate::app::theme::Palette;
use crate::view::{BlocksFrameInput, FrameArea, TextEntryFocused};
use crate::widgets::{AddDock, Dock, DockEdge, HANDLE_PX, Icon, button_icon, dock_handle};

/// Height the panel opens at, before anyone has dragged it.
const HEIGHT_PX: f32 = 240.0;
/// The shortest it can be dragged: the header and a few lines under it.
const MIN_PX: f32 = 120.0;
/// The most of the window it can take, so the frames are never squeezed out.
const MAX_FRACTION: f32 = 0.75;

/// Lines kept on screen. The log holds more; this is what a panel this tall can
/// show without spending a thousand text entities on what nobody scrolls to.
const SHOWN_LINES: usize = 200;

/// Whether the panel is showing, how tall, and what it last had in it.
#[derive(Resource)]
pub struct LogPanel {
    pub open: bool,
    height: f32,
    /// Records written when the lines were last built, so the panel rebuilds
    /// when the log moves rather than every frame.
    shown: u64,
}

impl Default for LogPanel {
    fn default() -> Self {
        LogPanel {
            open: false,
            height: HEIGHT_PX,
            shown: 0,
        }
    }
}

impl LogPanel {
    /// Height a drag reaching `from_bottom` up from the bottom edge should
    /// produce.
    ///
    /// Always a usable height, as with the inspector: dragging does not close
    /// it, since a panel squeezed to nothing leaves no edge to grab.
    fn height_for_drag(from_bottom: f32, window_height: f32) -> f32 {
        from_bottom.clamp(MIN_PX, max_height(window_height))
    }
}

impl Dock for LogPanel {
    type Handle = LogPanelHandle;
    const EDGE: DockEdge = DockEdge::Bottom;

    fn drag_to(&mut self, reach: f32, span: f32) {
        self.height = LogPanel::height_for_drag(reach, span);
    }
}

/// The tallest the panel may be in a window this tall. Applied to the height
/// it was dragged to as well as to the drag, so shrinking the window later
/// cannot leave it covering the frames.
fn max_height(window_height: f32) -> f32 {
    (window_height * MAX_FRACTION).max(MIN_PX)
}

/// The panel itself.
#[derive(Component, Clone, Default)]
pub struct LogPanelRoot;

/// The scrolling body the lines are built into.
#[derive(Component, Clone, Default)]
pub struct LogPanelBody;

/// One line of the log.
#[derive(Component, Clone, Default)]
pub struct LogLine;

/// Opens and closes the panel.
#[derive(Component, Clone, Default)]
pub struct LogPanelToggle;

/// The top edge, dragged to resize.
#[derive(Component, Clone, Default)]
pub struct LogPanelHandle;

/// Puts the whole log on the clipboard.
#[derive(Component, Clone, Default)]
pub struct CopyLogButton;

/// The key that opens the panel, for when the sidebar is collapsed or in the
/// way.
const SHORTCUT: KeyCode = KeyCode::F12;

pub fn spawn_log_panel(mut commands: Commands) {
    let root = commands
        .spawn_scene(bsn! {
            LogPanelRoot
            BlocksFrameInput
            Node {
                position_type: { PositionType::Absolute },
                display: { Display::None },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(4.0) },
                padding: { UiRect::all(Val::Px(8.0)) },
            }
            ThemeBackgroundColor({ tokens::WINDOW_BG })
            InheritableFont { font_size: { 12.0f32 } }
        })
        .id();

    let header = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
            }
            Children [
                (
                    label("Log")
                    InheritableFont { font_size: { 13.0f32 } }
                    Node { flex_grow: { 1.0_f32 } }
                ),
                (
                    @FeathersToolButton {
                        @caption: { bsn_list![button_icon(Icon::Copy)] }
                    }
                    BlocksFrameInput
                    CopyLogButton
                ),
                (
                    @FeathersToolButton {
                        @caption: { bsn_list![button_icon(Icon::X)] }
                    }
                    BlocksFrameInput
                    LogPanelToggle
                ),
            ]
        })
        .id();

    let body = commands
        .spawn_scene(bsn! {
            LogPanelBody
            // Long enough to run off the panel, so it scrolls.
            ScrollArea
            Node {
                width: { Val::Percent(100.0) },
                flex_grow: { 1.0_f32 },
                flex_direction: { FlexDirection::Column },
                overflow: { Overflow::scroll_y() },
            }
        })
        .id();

    commands.entity(root).add_children(&[header, body]);

    commands.spawn_scene(bsn! {
        LogPanelHandle
        dock_handle(DockEdge::Bottom)
        Node {
            display: { Display::None },
        }
    });
}

/// Open and close the panel from the keyboard.
///
/// Through the same guard every frame shortcut uses: a URL being typed into
/// takes the keyboard, and `F12` in the middle of a URL should be a keystroke
/// rather than a panel.
pub fn log_panel_shortcut(
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<TextEntryFocused>,
    mut panel: ResMut<LogPanel>,
) {
    if typing.0 || !keys.just_pressed(SHORTCUT) {
        return;
    }
    panel.open = !panel.open;
}

/// Open and close it from a button.
pub fn on_log_panel_toggled(
    activate: On<Activate>,
    buttons: Query<(), With<LogPanelToggle>>,
    mut panel: ResMut<LogPanel>,
) {
    if buttons.get(activate.entity).is_ok() {
        panel.open = !panel.open;
    }
}

/// Put the log on the clipboard.
pub fn on_copy_pressed(
    activate: On<Activate>,
    buttons: Query<(), With<CopyLogButton>>,
    tail: Res<LogTail>,
    mut clipboard: ResMut<Clipboard>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    // Logged rather than shown: what went wrong with copying the log is itself
    // a log line, and the panel is open to read it.
    match clipboard.set_text(tail.report()) {
        Ok(()) => info!("log copied to the clipboard"),
        Err(e) => warn!("could not copy the log: {e}"),
    }
}

/// Take the panel's height off the grid while it is open.
pub fn reserve_space(panel: Res<LogPanel>, windows: Query<&Window>, mut area: ResMut<FrameArea>) {
    if !panel.open {
        return;
    }
    let Ok(window) = windows.single() else { return };
    area.reserve_bottom(panel.height.min(max_height(window.height())));
}

/// Keep the panel across the bottom of whatever the grid was left.
pub fn place_log_panel(
    panel: Res<LogPanel>,
    area: Res<FrameArea>,
    windows: Query<&Window>,
    mut roots: Query<&mut Node, (With<LogPanelRoot>, Without<LogPanelHandle>)>,
    mut handles: Query<&mut Node, (With<LogPanelHandle>, Without<LogPanelRoot>)>,
) {
    let Ok(window) = windows.single() else { return };
    // The panel runs from where the grid stops to the bottom of the window,
    // which is exactly the height it reserved.
    let top = area.origin.y + area.size.y;
    for mut node in &mut handles {
        let wanted = if panel.open {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
        // Straddles the edge so it can be grabbed from either side.
        node.left = Val::Px(area.origin.x);
        node.top = Val::Px(top - HANDLE_PX * 0.5);
        node.width = Val::Px(area.size.x);
    }
    for mut node in &mut roots {
        let wanted = if panel.open {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
        if !panel.open {
            continue;
        }
        // Under the grid, across what the docks beside it left over: the panel
        // reserved this strip itself, so the grid's own area stops where it
        // begins.
        node.left = Val::Px(area.origin.x);
        node.top = Val::Px(top);
        node.width = Val::Px(area.size.x);
        node.height = Val::Px((window.height() - top).max(0.0));
    }
}

/// Rebuild the lines when the log has moved.
pub fn rebuild_log_lines(
    mut commands: Commands,
    mut panel: ResMut<LogPanel>,
    tail: Res<LogTail>,
    palette: Res<Palette>,
    theme: Res<UiTheme>,
    body: Query<Entity, With<LogPanelBody>>,
    existing: Query<Entity, With<LogLine>>,
) {
    let Ok(body) = body.single() else { return };
    let written = tail.written();
    // Nothing is built while it is closed: the log goes on filling up and the
    // panel catches up the moment it is opened.
    if !panel.open || (panel.shown == written && !palette.is_changed()) {
        return;
    }
    panel.shown = written;

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let dim = theme.color(&tokens::TEXT_DIM);
    // Newest first. A log panel is opened to see what just happened, and the
    // newest line at the bottom of a scroll area is the one line nobody sees.
    // The copied report stays in the order things happened.
    let lines: Vec<Entity> = tail
        .recent(SHOWN_LINES)
        .into_iter()
        .rev()
        .map(|record| {
            let colour = match record.level {
                bevy::log::Level::ERROR => palette.problem,
                bevy::log::Level::WARN => palette.caution,
                _ => dim,
            };
            let line = record.line();
            commands
                .spawn_scene(bsn! {
                    LogLine
                    Text({ line })
                    TextFont { font_size: { FontSize::Px(11.0) } }
                    TextColor({ colour })
                })
                .id()
        })
        .collect();
    commands.entity(body).add_children(&lines);
}

/// Grey the copy button out when there is nothing to copy.
pub fn sync_copy_button(
    mut commands: Commands,
    panel: Res<LogPanel>,
    buttons: Query<Entity, With<CopyLogButton>>,
) {
    for entity in &buttons {
        if panel.shown == 0 {
            commands.entity(entity).insert(InteractionDisabled);
        } else {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}

/// The debug panel, and the log behind it.
pub struct LogPanelPlugin;

impl Plugin for LogPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_dock::<LogPanel>()
            .add_observer(on_log_panel_toggled)
            .add_observer(on_copy_pressed)
            // With the docks: it reads the keyboard before anything measures
            // against the space it takes, and gives that space back the frame
            // it closes.
            .add_systems(Update, log_panel_shortcut.in_set(Stage::DockInput))
            .add_systems(Update, reserve_space.in_set(Stage::DockReserve))
            .add_systems(Update, place_log_panel.in_set(Stage::Chrome))
            .add_systems(
                Update,
                (rebuild_log_lines, sync_copy_button)
                    .chain()
                    .in_set(Stage::ControlsBuild),
            )
            .add_systems(Startup, spawn_log_panel.in_set(Boot::Shell));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_takes_its_height_from_the_grid_only_while_it_is_open() {
        let mut area = FrameArea {
            origin: Vec2::ZERO,
            size: Vec2::new(1000.0, 800.0),
        };
        area.reserve_bottom(HEIGHT_PX);
        assert_eq!(area.size.y, 800.0 - HEIGHT_PX);
        assert_eq!(area.origin.y, 0.0, "it comes off the bottom, not the top");
    }

    #[test]
    fn dragging_never_squeezes_the_panel_away() {
        for reach in [50.0, 0.0, -200.0] {
            assert_eq!(LogPanel::height_for_drag(reach, 1000.0), MIN_PX);
        }
    }

    #[test]
    fn dragging_leaves_the_frames_a_quarter_of_the_window() {
        assert_eq!(LogPanel::height_for_drag(400.0, 1000.0), 400.0);
        assert_eq!(LogPanel::height_for_drag(1000.0, 1000.0), 750.0);
    }

    #[test]
    fn it_opens_at_its_old_fixed_height() {
        assert_eq!(LogPanel::default().height, HEIGHT_PX);
    }

    #[test]
    fn reserving_more_than_there_is_leaves_nothing_rather_than_going_negative() {
        let mut area = FrameArea {
            origin: Vec2::ZERO,
            size: Vec2::new(1000.0, 100.0),
        };
        area.reserve_bottom(HEIGHT_PX);
        assert_eq!(area.size.y, 0.0);
    }
}
