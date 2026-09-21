//! The settings screen: what the app remembers between sessions, and the
//! way to make it forget.
//!
//! A modal, like the help screen: opened from the sidebar, and closed by its
//! own X, Escape, or a click outside it.

use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::window::WindowTheme;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersCheckbox};
use bevy_feathers::display::label_dim;
use bevy_feathers::rounded_corners::RoundedCorners;
use bevy_ui_widgets::{Activate, ValueChange};

use crate::app::prefs::{Preferences, PreferencesFile};
use crate::app::schedule::{Boot, Stage};
use crate::app::theme::ThemeMode;
use crate::ui::filtered::{FilteredTarget, filtered_controls};
use crate::ui::log_panel::LogPanel;
use crate::widgets::{
    AddModal, Icon, Modal, ResetDockSizes, button_icon, button_text, set_modal_open, size,
    spawn_modal, text,
};

const PANEL_PX: f32 = 440.0;

/// The dimmed backdrop, which is the whole screen.
#[derive(Component, Clone, Default)]
pub struct SettingsScreen;

/// The sidebar button that opens it, and the one inside it that closes it.
#[derive(Component, Clone, Default)]
pub struct SettingsToggle;

impl Modal for SettingsScreen {
    type Toggle = SettingsToggle;
}

/// Opens the log panel, closing this screen so the log can be seen.
#[derive(Component, Clone, Default)]
pub struct ShowLogsButton;

#[derive(Component, Clone, Default)]
pub struct RememberLayoutBox;

/// One button of the theme group.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ThemeOption {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemeOption {
    fn of(mode: &ThemeMode) -> Self {
        match (mode.chosen(), mode.is_dark()) {
            (false, _) => ThemeOption::System,
            (true, true) => ThemeOption::Dark,
            (true, false) => ThemeOption::Light,
        }
    }
}

/// A button of the theme group. Only the ends are rounded, so the three read
/// as one control.
fn theme_option(
    option: ThemeOption,
    icon: Icon,
    text: &'static str,
    corners: RoundedCorners,
) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: { bsn_list![button_icon(icon), button_text(text)] },
            @corners: { corners }
        }
        Node { column_gap: { Val::Px(6.0) }, flex_grow: { 1.0_f32 } }
        template_value(option)
    }
}

#[derive(Component, Clone, Default)]
pub struct ResetLayoutButton;

#[derive(Component, Clone, Default)]
pub struct ResetPointCloudButton;

pub fn spawn_settings(mut commands: Commands, file: Res<PreferencesFile>) {
    let saved_in = format!("Saved in {}", file.0.display());
    let modal = spawn_modal::<SettingsScreen>(&mut commands, "Settings", PANEL_PX);
    let body = commands
        .spawn_scene(bsn! {
            Node { flex_direction: { FlexDirection::Column }, row_gap: { Val::Px(10.0) } }
            Children [
                // A subtitle, so it is drawn closer to the title than the
                // panel's gap would put it.
                (
                    label_dim(saved_in)
                    Node { margin: { UiRect::top(Val::Px(-6.0)) } }
                ),
                (
                    text("Layout", size::DOCK_TITLE)
                    Node { margin: { UiRect::top(Val::Px(6.0)) } }
                ),
                (
                    @FeathersCheckbox {
                        @caption: { bsn_list![button_text("Remember panel sizes")] }
                    }
                    RememberLayoutBox
                ),
                label_dim(
                    "The sidebar, inspector and log open at the size you last \
                     dragged them to."
                ),
                (
                    Node { align_items: { AlignItems::Start } }
                    Children [(
                        @FeathersButton {
                            @caption: { bsn_list![
                                button_icon(Icon::RotateCcw),
                                button_text("Reset panel sizes"),
                            ] }
                        }
                        Node { column_gap: { Val::Px(6.0) } }
                        ResetLayoutButton
                    )]
                ),
                (
                    text("Appearance", size::DOCK_TITLE)
                    Node { margin: { UiRect::top(Val::Px(6.0)) } }
                ),
                (
                    Node { width: { Val::Percent(100.0) }, column_gap: { Val::Px(1.0) } }
                    Children [
                        theme_option(ThemeOption::Light, Icon::Sun, "Light", RoundedCorners::Left),
                        theme_option(ThemeOption::Dark, Icon::Moon, "Dark", RoundedCorners::None),
                        theme_option(ThemeOption::System, Icon::Monitor, "System", RoundedCorners::Right),
                    ]
                ),
                label_dim("System follows your operating system's light or dark setting."),
                (
                    text("Point clouds", size::DOCK_TITLE)
                    Node { margin: { UiRect::top(Val::Px(6.0)) } }
                ),
                filtered_controls(FilteredTarget::Default),
                label_dim(
                    "How a point cloud opens. Change one already open in its view \
                     configuration."
                ),
                (
                    Node { align_items: { AlignItems::Start } }
                    Children [(
                        @FeathersButton {
                            @caption: { bsn_list![
                                button_icon(Icon::RotateCcw),
                                button_text("Reset point cloud defaults"),
                            ] }
                        }
                        Node { column_gap: { Val::Px(6.0) } }
                        ResetPointCloudButton
                    )]
                ),
                (
                    // Not a setting, so it sits apart from them, in the corner.
                    Node {
                        justify_content: { JustifyContent::End },
                        margin: { UiRect::top(Val::Px(6.0)) },
                    }
                    Children [(
                        @FeathersButton {
                            @caption: { bsn_list![
                                button_icon(Icon::ScrollText),
                                button_text("Logs"),
                            ] }
                        }
                        Node { column_gap: { Val::Px(6.0) } }
                        ShowLogsButton
                    )]
                ),
            ]
        })
        .id();
    commands.entity(modal.panel).add_child(body);
}

pub fn on_show_logs(
    activate: On<Activate>,
    buttons: Query<(), With<ShowLogsButton>>,
    mut screens: Query<&mut Node, With<SettingsScreen>>,
    mut logs: ResMut<LogPanel>,
) {
    if buttons.contains(activate.entity) {
        set_modal_open::<SettingsScreen>(&mut screens, Some(false));
        logs.open = true;
    }
}

pub fn on_reset_layout(
    activate: On<Activate>,
    buttons: Query<(), With<ResetLayoutButton>>,
    mut commands: Commands,
) {
    if buttons.contains(activate.entity) {
        commands.trigger(ResetDockSizes);
        info!("reset the panels to their default sizes");
    }
}

/// Back to following the built-in default. Point clouds already open keep
/// what they have, as they do for any change here.
pub fn on_reset_point_cloud(
    activate: On<Activate>,
    buttons: Query<(), With<ResetPointCloudButton>>,
    mut prefs: ResMut<Preferences>,
) {
    if buttons.contains(activate.entity) && prefs.filtered_points.is_some() {
        prefs.filtered_points = None;
    }
}

/// Forgetting the sizes as well as no longer noting them, so turning this off
/// is enough for the next launch to open at the defaults.
pub fn on_remember_layout(
    change: On<ValueChange<bool>>,
    boxes: Query<(), With<RememberLayoutBox>>,
    mut prefs: ResMut<Preferences>,
) {
    if !boxes.contains(change.source) || prefs.remember_layout == change.value {
        return;
    }
    prefs.remember_layout = change.value;
    if !change.value {
        prefs.docks.clear();
    }
}

pub fn on_theme_option(
    activate: On<Activate>,
    options: Query<&ThemeOption>,
    windows: Query<&Window>,
    mut mode: ResMut<ThemeMode>,
) {
    let Ok(&option) = options.get(activate.entity) else {
        return;
    };
    if option == ThemeOption::of(&mode) {
        return;
    }
    match option {
        ThemeOption::Light => mode.choose(WindowTheme::Light),
        ThemeOption::Dark => mode.choose(WindowTheme::Dark),
        ThemeOption::System => {
            let desktop = windows.iter().next().and_then(|window| window.window_theme);
            mode.follow_desktop(desktop);
        }
    }
}

/// Show each control as what it stands for, which can change from outside
/// this screen: the desktop's theme, while it is followed.
pub fn sync_settings(
    mut commands: Commands,
    prefs: Res<Preferences>,
    mode: Res<ThemeMode>,
    remember: Query<(Entity, Has<Checked>), With<RememberLayoutBox>>,
    mut options: Query<(&ThemeOption, &mut ButtonVariant)>,
    resets: Query<(Entity, Has<InteractionDisabled>), With<ResetPointCloudButton>>,
) {
    // Nothing to reset while the default is already followed.
    let customized = prefs.filtered_points.is_some();
    for (entity, disabled) in &resets {
        if customized && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !customized && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    for (entity, checked) in &remember {
        if prefs.remember_layout && !checked {
            commands.entity(entity).insert(Checked);
        } else if !prefs.remember_layout && checked {
            commands.entity(entity).remove::<Checked>();
        }
    }
    let current = ThemeOption::of(&mode);
    for (option, mut variant) in &mut options {
        variant.set_if_neq(if *option == current {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        });
    }
}

/// The settings screen and the button that opens it.
pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_modal::<SettingsScreen>()
            .add_observer(on_show_logs)
            .add_observer(on_reset_layout)
            .add_observer(on_reset_point_cloud)
            .add_observer(on_remember_layout)
            .add_observer(on_theme_option)
            .add_systems(Update, sync_settings.in_set(Stage::ControlsPlace))
            .add_systems(Startup, spawn_settings.in_set(Boot::Shell));
    }
}
