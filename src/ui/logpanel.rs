//! The debug panel: the log, where a user can see it.
//!
//! Docked along the bottom, closed until asked for with `F12` or the button in
//! the sidebar's footer. It takes its height off the frame grid like the
//! sidebar takes its width, so the frames shrink rather than being covered:
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
use crate::widgets::{Icon, button_icon};

/// Height the panel takes off the grid when it is open.
const HEIGHT_PX: f32 = 240.0;

/// Lines kept on screen. The log holds more; this is what a panel this tall can
/// show without spending a thousand text entities on what nobody scrolls to.
const SHOWN_LINES: usize = 200;

/// Whether the panel is showing, and what it last had in it.
#[derive(Resource, Default)]
pub struct LogPanel {
    pub open: bool,
    /// Records written when the lines were last built, so the panel rebuilds
    /// when the log moves rather than every frame.
    shown: u64,
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
pub fn reserve_space(panel: Res<LogPanel>, mut area: ResMut<FrameArea>) {
    if panel.open {
        area.reserve_bottom(HEIGHT_PX);
    }
}

/// Keep the panel across the bottom of whatever the grid was left.
pub fn place_log_panel(
    panel: Res<LogPanel>,
    area: Res<FrameArea>,
    mut roots: Query<&mut Node, With<LogPanelRoot>>,
) {
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
        node.top = Val::Px(area.origin.y + area.size.y);
        node.width = Val::Px(area.size.x);
        node.height = Val::Px(HEIGHT_PX);
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
        app.init_resource::<LogPanel>()
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
    fn reserving_more_than_there_is_leaves_nothing_rather_than_going_negative() {
        let mut area = FrameArea {
            origin: Vec2::ZERO,
            size: Vec2::new(1000.0, 100.0),
        };
        area.reserve_bottom(HEIGHT_PX);
        assert_eq!(area.size.y, 0.0);
    }
}
