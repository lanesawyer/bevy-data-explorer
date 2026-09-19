//! The settings screen: what the app remembers between sessions, and the
//! way to make it forget.
//!
//! Laid over the whole window like the help screen, and opened and closed the
//! same ways: its sidebar button, its own X, Escape, or a click outside it.

use bevy::prelude::*;
use bevy::ui::{Checked, FocusPolicy, Interaction};
use bevy::window::WindowTheme;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersCheckbox, FeathersToolButton,
};
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::rounded_corners::RoundedCorners;
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_feathers::tokens;
use bevy_ui_widgets::{Activate, ValueChange};

use crate::app::prefs::{Preferences, PreferencesFile};
use crate::app::schedule::{Boot, Stage};
use crate::app::theme::ThemeMode;
use crate::view::BlocksFrameInput;
use crate::widgets::{Icon, ResetDockSizes, button_icon, button_text};

/// With the help screen: both cover everything a menu could open over.
const SETTINGS_Z: i32 = 20;
const PANEL_PX: f32 = 440.0;

/// The dimmed backdrop, which is the whole screen.
#[derive(Component, Clone, Default)]
pub struct SettingsScreen;

/// The sidebar button that opens it, and the one inside it that closes it.
#[derive(Component, Clone, Default)]
pub struct SettingsToggle;

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

pub fn spawn_settings(mut commands: Commands, file: Res<PreferencesFile>) {
    let saved_in = format!("Saved in {}", file.0.display());
    let screen = commands
        .spawn_scene(bsn! {
            SettingsScreen
            BlocksFrameInput
            // The backdrop holds no buttons of its own, and a click on it has
            // to stop there rather than reach a frame.
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
            GlobalZIndex({ SETTINGS_Z })
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
                    }
                    Children [
                        (
                            label("Settings")
                            TextFont { font_size: { FontSize::Px(20.0) } }
                            Node { flex_grow: { 1.0_f32 } }
                        ),
                        (
                            @FeathersToolButton {
                                @caption: { bsn_list![button_icon(Icon::X)] }
                            }
                            SettingsToggle
                        ),
                    ]
                ),
                (
                    label("Layout")
                    TextFont { font_size: { FontSize::Px(15.0) } }
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
                    label("Appearance")
                    TextFont { font_size: { FontSize::Px(15.0) } }
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
                    label_dim(saved_in)
                    Node { margin: { UiRect::top(Val::Px(8.0)) } }
                ),
            ]
        })
        // A click on the panel is not a click on the backdrop behind it.
        .observe(|mut click: On<Pointer<Click>>| click.propagate(false))
        .id();
    commands.entity(screen).add_child(panel);
}

fn set_open(screens: &mut Query<&mut Node, With<SettingsScreen>>, open: Option<bool>) {
    for mut node in screens {
        let now_open = open.unwrap_or(node.display == Display::None);
        node.display = if now_open {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub fn on_settings_toggle(
    activate: On<Activate>,
    toggles: Query<(), With<SettingsToggle>>,
    mut screens: Query<&mut Node, With<SettingsScreen>>,
) {
    if toggles.contains(activate.entity) {
        set_open(&mut screens, None);
    }
}

fn close_on_backdrop(
    _click: On<Pointer<Click>>,
    mut screens: Query<&mut Node, With<SettingsScreen>>,
) {
    set_open(&mut screens, Some(false));
}

pub fn close_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut screens: Query<&mut Node, With<SettingsScreen>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        set_open(&mut screens, Some(false));
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
/// this screen: the sidebar's theme button picks light or dark by hand.
pub fn sync_settings(
    mut commands: Commands,
    prefs: Res<Preferences>,
    mode: Res<ThemeMode>,
    remember: Query<(Entity, Has<Checked>), With<RememberLayoutBox>>,
    mut options: Query<(&ThemeOption, &mut ButtonVariant)>,
) {
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
        app.add_observer(on_settings_toggle)
            .add_observer(on_reset_layout)
            .add_observer(on_remember_layout)
            .add_observer(on_theme_option)
            .add_systems(Update, close_on_escape.in_set(Stage::ControlsRead))
            .add_systems(Update, sync_settings.in_set(Stage::ControlsPlace))
            .add_systems(Startup, spawn_settings.in_set(Boot::Shell));
    }
}
