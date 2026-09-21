//! The settings screen: what the app remembers between sessions, and the
//! way to make it forget.
//!
//! A modal, like the help screen: opened from the sidebar, and closed by its
//! own X, Escape, or a click outside it.

use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::window::WindowTheme;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersCheckbox, FeathersTextInput, FeathersTextInputContainer,
};
use bevy_feathers::display::label_dim;
use bevy_feathers::rounded_corners::RoundedCorners;
use bevy_ui_widgets::{Activate, ValueChange};

use crate::app::net::{Fetching, fetching};
use crate::app::prefs::{Preferences, PreferencesFile};
use crate::app::schedule::{Boot, Stage};
use crate::app::theme::{Palette, ThemeMode};
use crate::catalog::registry::{self, AssetSample};
use crate::ui::filtered::{FilteredTarget, filtered_controls};
use crate::ui::log_panel::LogPanel;
use crate::widgets::space;
use crate::widgets::{
    AddModal, Icon, Modal, ResetDockSizes, button_icon, button_text, field_well, set_modal_open,
    size, spawn_modal, text,
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
        Node { column_gap: { Val::Px(space::ICON_LABEL) }, flex_grow: { 1.0_f32 } }
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
            Node { flex_direction: { FlexDirection::Column }, row_gap: { Val::Px(space::ROWS) } }
            Children [
                // A subtitle, so it is drawn closer to the title than the
                // panel's gap would put it.
                (
                    label_dim(saved_in)
                    Node { margin: { UiRect::top(Val::Px(space::STACKED - space::ROWS)) } }
                ),
                (
                    text("Layout", size::DOCK_TITLE)
                    Node { margin: { UiRect::top(Val::Px(space::HEADING)) } }
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
                        Node { column_gap: { Val::Px(space::ICON_LABEL) } }
                        ResetLayoutButton
                    )]
                ),
                (
                    text("Appearance", size::DOCK_TITLE)
                    Node { margin: { UiRect::top(Val::Px(space::HEADING)) } }
                ),
                (
                    Node { width: { Val::Percent(100.0) }, column_gap: { Val::Px(space::SEAM) } }
                    Children [
                        theme_option(ThemeOption::Light, Icon::Sun, "Light", RoundedCorners::Left),
                        theme_option(ThemeOption::Dark, Icon::Moon, "Dark", RoundedCorners::None),
                        theme_option(ThemeOption::System, Icon::Monitor, "System", RoundedCorners::Right),
                    ]
                ),
                label_dim("System follows your operating system's light or dark setting."),
                (
                    text("Point clouds", size::DOCK_TITLE)
                    Node { margin: { UiRect::top(Val::Px(space::HEADING)) } }
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
                        Node { column_gap: { Val::Px(space::ICON_LABEL) } }
                        ResetPointCloudButton
                    )]
                ),
            ]
        })
        .id();
    let registry = spawn_registry_section(&mut commands);
    let logs = commands
        .spawn_scene(bsn! {
            // Not a setting, so it sits apart from them, in the corner.
            Node {
                justify_content: { JustifyContent::End },
                margin: { UiRect::top(Val::Px(space::HEADING)) },
            }
            Children [(
                @FeathersButton {
                    @caption: { bsn_list![
                        button_icon(Icon::ScrollText),
                        button_text("Logs"),
                    ] }
                }
                Node { column_gap: { Val::Px(space::ICON_LABEL) } }
                ShowLogsButton
            )]
        })
        .id();
    commands.entity(body).add_children(&[registry, logs]);
    commands.entity(modal.panel).add_child(body);
}

/// Where the BKP Registry token is pasted, and the button that tries it.
///
/// The field has no masking, so it is only ever a place to paste into: saving
/// empties it, and from then on the token is shown by its last characters.
fn spawn_registry_section(commands: &mut Commands) -> Entity {
    commands
        .spawn_scene(bsn! {
            Node { flex_direction: { FlexDirection::Column }, row_gap: { Val::Px(space::ROWS) } }
            Children [
                (
                    text("BKP Registry", size::DOCK_TITLE)
                    Node { margin: { UiRect::top(Val::Px(space::HEADING)) } }
                ),
                label_dim("Pre-production. Paste a bearer token to use it."),
                (
                    field_well()
                    Children [
                        (
                            Node {
                                width: { Val::Percent(100.0) },
                                align_items: { AlignItems::Center },
                                column_gap: { Val::Px(space::CONTROLS) },
                            }
                            Children [
                                (
                                    @FeathersTextInputContainer
                                    Children [(
                                        @FeathersTextInput
                                        RegistryTokenInput
                                    )]
                                ),
                                (
                                    @FeathersButton {
                                        @caption: { bsn_list![button_text("Save")] }
                                    }
                                    Node { flex_shrink: { 0.0_f32 } }
                                    SaveTokenButton
                                ),
                            ]
                        ),
                        (
                            RegistryStatus
                            Text({ String::new() })
                            TextFont { font_size: { FontSize::Px(size::SMALL) } }
                        ),
                    ]
                ),
                (
                    Node { column_gap: { Val::Px(space::CONTROLS) } }
                    Children [
                        (
                            @FeathersButton {
                                @caption: { bsn_list![button_text("Test request")] }
                            }
                            TestRegistryButton
                        ),
                        (
                            @FeathersButton {
                                @caption: { bsn_list![button_text("Forget token")] }
                            }
                            ForgetTokenButton
                        ),
                    ]
                ),
            ]
        })
        .id()
}

/// The field a token is pasted into.
#[derive(Component, Clone, Default)]
pub struct RegistryTokenInput;

#[derive(Component, Clone, Default)]
pub struct SaveTokenButton;

#[derive(Component, Clone, Default)]
pub struct ForgetTokenButton;

#[derive(Component, Clone, Default)]
pub struct TestRegistryButton;

/// The line saying which token is saved and how the last request went.
#[derive(Component, Clone, Default)]
pub struct RegistryStatus;

/// The test request in flight, if any, and how the last one went.
#[derive(Resource, Default)]
pub struct RegistryProbe {
    task: Option<Fetching<Result<AssetSample, String>>>,
    outcome: Option<Result<String, String>>,
}

impl RegistryProbe {
    /// What the status line says, and whether it is a problem.
    fn message(&self, token: Option<&str>) -> (String, bool) {
        let saved = match token {
            Some(token) => format!("Token {} saved.", registry::token_hint(token)),
            None => "No token saved.".to_string(),
        };
        match &self.outcome {
            _ if self.task.is_some() => (format!("{saved} Asking\u{2026}"), false),
            Some(Ok(answer)) => (format!("{saved} {answer}"), false),
            Some(Err(problem)) => (format!("{saved} {problem}"), true),
            None => (saved, false),
        }
    }
}

fn save_token(
    prefs: &mut Preferences,
    probe: &mut RegistryProbe,
    inputs: &mut Query<&mut EditableText, With<RegistryTokenInput>>,
) {
    let Ok(mut field) = inputs.single_mut() else {
        return;
    };
    let token = registry::clean_token(&field.value().to_string());
    if token.is_empty() {
        return;
    }
    field.clear();
    info!(
        "saved a BKP Registry token ({})",
        registry::token_hint(&token)
    );
    prefs.registry_token = Some(token);
    probe.outcome = None;
}

pub fn on_save_token(
    activate: On<Activate>,
    buttons: Query<(), With<SaveTokenButton>>,
    mut inputs: Query<&mut EditableText, With<RegistryTokenInput>>,
    mut prefs: ResMut<Preferences>,
    mut probe: ResMut<RegistryProbe>,
) {
    if buttons.contains(activate.entity) {
        save_token(&mut prefs, &mut probe, &mut inputs);
    }
}

/// Return in the field saves it, as it reads an address typed into a search.
pub fn on_token_submitted(
    key: On<FocusedInput<KeyboardInput>>,
    mut inputs: Query<&mut EditableText, With<RegistryTokenInput>>,
    mut prefs: ResMut<Preferences>,
    mut probe: ResMut<RegistryProbe>,
) {
    if inputs.contains(key.focused_entity)
        && key.input.state.is_pressed()
        && matches!(key.input.key_code, KeyCode::Enter | KeyCode::NumpadEnter)
    {
        save_token(&mut prefs, &mut probe, &mut inputs);
    }
}

pub fn on_forget_token(
    activate: On<Activate>,
    buttons: Query<(), With<ForgetTokenButton>>,
    mut prefs: ResMut<Preferences>,
    mut probe: ResMut<RegistryProbe>,
) {
    if buttons.contains(activate.entity) && prefs.registry_token.is_some() {
        prefs.registry_token = None;
        probe.outcome = None;
        info!("forgot the BKP Registry token");
    }
}

pub fn on_test_registry(
    activate: On<Activate>,
    buttons: Query<(), With<TestRegistryButton>>,
    prefs: Res<Preferences>,
    mut probe: ResMut<RegistryProbe>,
) {
    if !buttons.contains(activate.entity) || probe.task.is_some() {
        return;
    }
    let Some(token) = prefs.registry_token.clone() else {
        return;
    };
    info!(
        "BKP Registry: asking {} for {} data assets",
        registry::STAGE,
        registry::SAMPLE
    );
    probe.task = Some(fetching(registry::sample_assets(
        registry::STAGE.to_string(),
        token,
    )));
}

/// Log what the registry answered, asset by asset, and sum it up on screen.
pub fn poll_registry_probe(mut probe: ResMut<RegistryProbe>) {
    let Some(outcome) = probe.task.as_mut().and_then(Fetching::take) else {
        return;
    };
    probe.task = None;
    probe.outcome = Some(match outcome {
        Ok(sample) => {
            info!(
                "BKP Registry: {} data assets in all, {} on this page{}",
                sample.total,
                sample.assets.len(),
                if sample.more {
                    ", more to page through"
                } else {
                    ""
                }
            );
            for (kind, count) in sample.kinds() {
                info!("BKP Registry: type {kind}: {count}");
            }
            for asset in &sample.assets {
                let tags: Vec<&str> = asset
                    .tags
                    .iter()
                    .flatten()
                    .flatten()
                    .map(String::as_str)
                    .collect();
                info!(
                    "BKP Registry: {} [{}] {} id={} tags={tags:?}",
                    asset.name,
                    asset.kind.as_deref().unwrap_or("no type"),
                    asset.status,
                    asset.id,
                );
                for instance in &asset.instances {
                    info!("BKP Registry:   {}", instance.download_url);
                }
            }
            Ok(format!(
                "{} data assets; the first {} are in the log.",
                sample.total,
                sample.assets.len()
            ))
        }
        Err(problem) => {
            warn!("{problem}");
            Err(problem)
        }
    });
}

pub fn sync_registry(
    mut commands: Commands,
    prefs: Res<Preferences>,
    probe: Res<RegistryProbe>,
    palette: Res<Palette>,
    mut labels: Query<(&mut Text, &mut TextColor), With<RegistryStatus>>,
    buttons: Query<
        (Entity, Has<InteractionDisabled>, Has<TestRegistryButton>),
        Or<(With<TestRegistryButton>, With<ForgetTokenButton>)>,
    >,
) {
    let token = prefs.registry_token.as_deref();
    let can_test = token.is_some() && probe.task.is_none();
    for (entity, disabled, test) in &buttons {
        let enabled = if test { can_test } else { token.is_some() };
        if enabled && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !enabled && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    let (message, problem) = probe.message(token);
    let color = if problem {
        palette.problem
    } else {
        palette.progress
    };
    for (mut text, mut text_color) in &mut labels {
        if text.0 != message {
            text.0 = message.clone();
        }
        if text_color.0 != color {
            text_color.0 = color;
        }
    }
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
            .init_resource::<RegistryProbe>()
            .add_observer(on_save_token)
            .add_observer(on_token_submitted)
            .add_observer(on_forget_token)
            .add_observer(on_test_registry)
            .add_systems(Update, poll_registry_probe.in_set(Stage::ControlsApply))
            .add_systems(
                Update,
                (sync_settings, sync_registry).in_set(Stage::ControlsPlace),
            )
            .add_systems(Startup, spawn_settings.in_set(Boot::Shell));
    }
}
