//! The settings screen: what the app remembers between sessions, and the
//! way to make it forget.
//!
//! A modal, like the help screen: opened from the sidebar, and closed by its
//! own X, Escape, or a click outside it. Its pages are picked from a list down
//! its left side: general settings, and the data sources offered to search.

use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::window::WindowTheme;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersCheckbox, FeathersToggleSwitch,
};
use bevy_feathers::display::label_dim;
use bevy_feathers::rounded_corners::RoundedCorners;
use bevy_ui_widgets::{Activate, ValueChange};

use crate::app::net::{Fetching, fetching};
use crate::app::prefs::{Preferences, PreferencesFile, RegistryLogin};
use crate::app::schedule::{Boot, Stage};
use crate::app::theme::ThemeMode;
use crate::catalog::Catalogs;
use crate::catalog::registry::login::{self, SignIn};
use crate::catalog::registry::{self, AssetSample};
use crate::source::properties::{CellProperties, ColorScale, Gradient};
use crate::ui::filtered::{FilteredTarget, filtered_controls};
use crate::ui::log_panel::LogPanel;
use crate::widgets::space;
use crate::widgets::{
    AddModal, Icon, Modal, ResetDockSizes, button_icon, button_text, field_well, set_modal_open,
    size, spawn_modal, text,
};
use crate::widgets::{Notice, Tone, notice};

const PANEL_PX: f32 = 620.0;
/// The list of pages down the left.
const NAV_PX: f32 = 140.0;

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

#[derive(Component, Clone, Default)]
pub struct SystemAccentBox;

#[derive(Component, Clone, Default)]
pub struct ReverseGradientBox;

#[derive(Component, Clone, Default)]
pub struct WholeExtentBox;

/// One button of the gradient choice.
#[derive(Component, Clone, Copy, Default)]
pub struct GradientOption(pub Gradient);

/// The swatch of a gradient on its button, drawn the way round it is used.
#[derive(Component, Clone, Copy, Default)]
pub struct GradientStrip(pub Gradient);

/// Gradients offered in a row.
const GRADIENT_COLUMNS: u16 = 3;
const STRIP_PX: f32 = 40.0;
const STRIP_HEIGHT_PX: f32 = 10.0;

/// `gradient` drawn left to right, or right to left when `reversed`.
fn strip(gradient: Gradient, reversed: bool) -> BackgroundGradient {
    const STOPS: usize = 16;
    let stops = (0..STOPS)
        .map(|step| {
            let fraction = step as f32 / (STOPS - 1) as f32;
            let along = if reversed { 1.0 - fraction } else { fraction };
            ColorStop::new(gradient.sample(along), Val::Percent(fraction * 100.0))
        })
        .collect();
    LinearGradient::to_right(stops).into()
}

/// A button of the gradient choice: its swatch, then its name.
fn gradient_option(gradient: Gradient) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: { bsn_list![
                (
                    Node {
                        width: { Val::Px(STRIP_PX) },
                        height: { Val::Px(STRIP_HEIGHT_PX) },
                        flex_shrink: { 0.0_f32 },
                    }
                    template_value(strip(gradient, false))
                    template_value(GradientStrip(gradient))
                ),
                button_text(gradient.name()),
            ] }
        }
        Node {
            column_gap: { Val::Px(space::CONTROLS * 2.0) },
            justify_content: { JustifyContent::Start },
        }
        template_value(GradientOption(gradient))
    }
}

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

/// One page of the screen: the row that picks it, and the column it shows.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum SettingsPage {
    #[default]
    General,
    DataSources,
}

impl SettingsPage {
    fn title(self) -> &'static str {
        match self {
            SettingsPage::General => "General",
            SettingsPage::DataSources => "Data sources",
        }
    }
}

/// The page shown, which stays put while the screen is closed.
#[derive(Resource, Default)]
pub struct ShownPage(SettingsPage);

/// The button that shows `page`, drawn as primary while it is shown.
fn page_button(page: SettingsPage) -> impl Scene {
    bsn! {
        @FeathersButton {
            @variant: { ButtonVariant::Plain },
            @caption: { bsn_list![button_text(page.title())] }
        }
        Node { justify_content: { JustifyContent::Start } }
        template_value(page)
    }
}

/// The switch turning a data source on or off, by `Provider::key`.
#[derive(Component, Clone, Default)]
pub struct SourceSwitch(pub &'static str);

/// Shown only while the data source keyed is on, such as the registry's token
/// or a provider's examples on the welcome screen.
///
/// Carries the display it is shown with, since not everything so marked is a
/// flex box: the welcome screen's example columns are grids.
#[derive(Component, Clone)]
pub struct WhileSourceOn {
    pub key: &'static str,
    pub shown: Display,
}

#[derive(Component, Clone, Default)]
pub struct ResetLayoutButton;

#[derive(Component, Clone, Default)]
pub struct ResetPointCloudButton;

pub fn spawn_settings(mut commands: Commands, file: Res<PreferencesFile>, catalogs: Res<Catalogs>) {
    let saved_in = format!("Saved in {}", file.0.display());
    let modal = spawn_modal::<SettingsScreen>(&mut commands, "Settings", PANEL_PX);
    // A subtitle, so it is drawn closer to the title than the panel's gap
    // would put it.
    let subtitle = commands
        .spawn_scene(bsn! {
            label_dim(saved_in)
            Node { margin: { UiRect::top(Val::Px(space::STACKED - space::ROWS)) } }
        })
        .id();
    let nav = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Px(NAV_PX) },
                flex_shrink: { 0.0_f32 },
                flex_direction: { FlexDirection::Column },
                justify_content: { JustifyContent::SpaceBetween },
                row_gap: { Val::Px(space::GROUPS) },
            }
            Children [
                (
                    Node {
                        flex_direction: { FlexDirection::Column },
                        row_gap: { Val::Px(space::LIST_ITEMS) },
                    }
                    Children [
                        page_button(SettingsPage::General),
                        page_button(SettingsPage::DataSources),
                    ]
                ),
                // Not a setting, so it sits apart from them, in the corner.
                (
                    Node { align_items: { AlignItems::Start } }
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
                ),
            ]
        })
        .id();
    let general = commands
        .spawn_scene(bsn! {
            template_value(SettingsPage::General)
            // Every page in the one cell, so the stack is as tall as the
            // tallest and the screen keeps its size from page to page.
            Node {
                grid_row: { GridPlacement::start(1) },
                grid_column: { GridPlacement::start(1) },
                flex_direction: { FlexDirection::Column },
                min_width: { Val::Px(0.0) },
                row_gap: { Val::Px(space::ROWS) },
            }
            Children [
                text("Layout", size::DOCK_TITLE),
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
                    @FeathersCheckbox {
                        @caption: { bsn_list![button_text("Use the system accent color")] }
                    }
                    SystemAccentBox
                ),
                label_dim(
                    "Buttons, switches and the selection take your operating system's \
                     accent, where it has one."
                ),
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
    let gradients = spawn_gradient_section(&mut commands);
    commands.entity(general).add_child(gradients);
    let sources = commands
        .spawn_scene(bsn! {
            template_value(SettingsPage::DataSources)
            // Every page in the one cell, so the stack is as tall as the
            // tallest and the screen keeps its size from page to page.
            Node {
                grid_row: { GridPlacement::start(1) },
                grid_column: { GridPlacement::start(1) },
                flex_direction: { FlexDirection::Column },
                min_width: { Val::Px(0.0) },
                row_gap: { Val::Px(space::ROWS) },
            }
            Children [
                text("Data sources", size::DOCK_TITLE),
                label_dim(
                    "What search offers. A source turned off is neither listed nor \
                     searched; anything already open from it stays open."
                ),
            ]
        })
        .id();
    for provider in catalogs.providers() {
        let row = spawn_source_switch(&mut commands, provider.key, provider.name, provider.about);
        commands.entity(sources).add_child(row);
        if provider.key == registry::PROVIDER.key {
            let details = spawn_registry_section(&mut commands);
            commands.entity(details).insert(WhileSourceOn {
                key: provider.key,
                shown: Display::Flex,
            });
            commands.entity(sources).add_child(details);
        }
    }
    let stack = commands
        .spawn(Node {
            display: Display::Grid,
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            grid_template_columns: vec![GridTrack::flex(1.0)],
            ..default()
        })
        .add_children(&[general, sources])
        .id();
    let pages = commands
        .spawn(Node {
            column_gap: Val::Px(space::SCREEN_INSET),
            ..default()
        })
        .add_children(&[nav, stack])
        .id();
    commands
        .entity(modal.panel)
        .add_children(&[subtitle, pages]);
}

/// The gradient numeric coloring is drawn along, and how.
fn spawn_gradient_section(commands: &mut Commands) -> Entity {
    let options: Vec<Entity> = Gradient::ALL
        .into_iter()
        .map(|gradient| commands.spawn_scene(gradient_option(gradient)).id())
        .collect();
    let grid = commands
        .spawn(Node {
            display: Display::Grid,
            grid_template_columns: vec![RepeatedGridTrack::flex(GRADIENT_COLUMNS, 1.0)],
            column_gap: Val::Px(space::CONTROLS),
            row_gap: Val::Px(space::CONTROLS),
            ..default()
        })
        .add_children(&options)
        .id();
    let heading = commands
        .spawn_scene(bsn! {
            text("Gradient", size::DOCK_TITLE)
            Node { margin: { UiRect::top(Val::Px(space::HEADING)) } }
        })
        .id();
    let rest = commands
        .spawn_scene(bsn! {
            Node { flex_direction: { FlexDirection::Column }, row_gap: { Val::Px(space::ROWS) } }
            Children [
                label_dim(
                    "What points colored by a number are drawn along, in every \
                     dataset open and every one opened after."
                ),
                (
                    @FeathersCheckbox {
                        @caption: { bsn_list![button_text("Reverse the gradient")] }
                    }
                    ReverseGradientBox
                ),
                (
                    @FeathersCheckbox {
                        @caption: { bsn_list![button_text("Span the whole range, ignoring filters")] }
                    }
                    WholeExtentBox
                ),
                label_dim(
                    "Narrowing a range then leaves every point the color it was, \
                     rather than spreading the gradient over what is left."
                ),
            ]
        })
        .id();
    commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(space::ROWS),
            ..default()
        })
        .add_children(&[heading, grid, rest])
        .id()
}

/// A data source's name and what it offers, with the switch that turns it on
/// or off.
fn spawn_source_switch(
    commands: &mut Commands,
    key: &'static str,
    name: &'static str,
    about: &'static str,
) -> Entity {
    commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(space::CONTROLS) },
                margin: { UiRect::top(Val::Px(space::HEADING)) },
            }
            Children [
                (
                    Node {
                        flex_direction: { FlexDirection::Column },
                        flex_grow: { 1.0_f32 },
                        min_width: { Val::Px(0.0) },
                        row_gap: { Val::Px(space::STACKED) },
                    }
                    Children [
                        text(name, size::BODY),
                        label_dim(about),
                    ]
                ),
                (
                    @FeathersToggleSwitch
                    Node { flex_shrink: { 0.0_f32 } }
                    template_value(SourceSwitch(key))
                ),
            ]
        })
        .id()
}

/// Signing in to the BKP Registry, and the button that tries it.
fn spawn_registry_section(commands: &mut Commands) -> Entity {
    commands
        .spawn_scene(bsn! {
            Node { flex_direction: { FlexDirection::Column }, row_gap: { Val::Px(space::ROWS) } }
            Children [
                label_dim("Sign in with your Institute account in the browser."),
                (
                    field_well()
                    Children [
                        (
                            @FeathersButton {
                                @variant: { ButtonVariant::Primary },
                                @caption: { bsn_list![button_text("Sign in with browser")] }
                            }
                            Node { align_self: { AlignSelf::Start } }
                            SignInButton
                        ),
                        (
                            notice()
                            RegistryStatus
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
                                @caption: { bsn_list![button_text("Sign out")] }
                            }
                            ForgetTokenButton
                        ),
                    ]
                ),
            ]
        })
        .id()
}

/// Opens the login page, or opens it again while a sign-in is waiting.
#[derive(Component, Clone, Default)]
pub struct SignInButton;

#[derive(Component, Clone, Default)]
pub struct ForgetTokenButton;

#[derive(Component, Clone, Default)]
pub struct TestRegistryButton;

/// The line saying who is signed in and how the last request went.
#[derive(Component, Clone, Default)]
pub struct RegistryStatus;

/// The test request or sign-in in flight, if any, and how the last one went.
#[derive(Resource, Default)]
pub struct RegistryProbe {
    task: Option<Fetching<Result<AssetSample, String>>>,
    signing_in: Option<SignIn>,
    outcome: Option<Result<String, String>>,
}

impl RegistryProbe {
    /// What the status line says.
    fn notice(&self, token: Option<&str>, login: Option<&RegistryLogin>) -> Notice {
        let saved = match (token, login) {
            (
                Some(_),
                Some(RegistryLogin {
                    email: Some(email), ..
                }),
            ) => {
                format!("Signed in as {email}.")
            }
            (Some(_), Some(_)) => "Signed in.".to_string(),
            (Some(token), None) => format!("Token {} saved.", registry::token_hint(token)),
            (None, _) => "Not signed in.".to_string(),
        };
        match &self.outcome {
            _ if self.signing_in.is_some() => Notice::new(
                Tone::Info,
                format!("{saved} Waiting for the browser\u{2026}"),
            ),
            _ if self.task.is_some() => Notice::new(Tone::Info, format!("{saved} Asking\u{2026}")),
            Some(Ok(answer)) => Notice::new(Tone::Success, format!("{saved} {answer}")),
            Some(Err(problem)) => Notice::new(Tone::Error, format!("{saved} {problem}")),
            None => Notice::new(Tone::Info, saved),
        }
    }
}

/// Open the login page, starting a sign-in unless one is already waiting,
/// in which case its page is opened again in case its tab was lost.
pub fn on_sign_in(
    activate: On<Activate>,
    buttons: Query<(), With<SignInButton>>,
    mut probe: ResMut<RegistryProbe>,
) {
    if !buttons.contains(activate.entity) {
        return;
    }
    if probe.signing_in.is_none() {
        match login::sign_in() {
            Ok(sign_in) => probe.signing_in = Some(sign_in),
            Err(problem) => {
                warn!("{problem}");
                probe.outcome = Some(Err(problem));
                return;
            }
        }
    }
    let Some(url) = probe.signing_in.as_ref().map(|sign_in| sign_in.url.clone()) else {
        return;
    };
    // Detached, as a link is: a browser that is not already running would
    // otherwise hold the frame until it finished starting.
    match open::that_detached(&url) {
        Ok(()) => info!("BKP Registry: opened the login page in the browser"),
        Err(error) => warn!("BKP Registry: could not open a browser ({error}); sign in at {url}"),
    }
}

/// Keep what the browser came back with.
pub fn poll_sign_in(mut probe: ResMut<RegistryProbe>, mut prefs: ResMut<Preferences>) {
    let Some(outcome) = probe
        .signing_in
        .as_mut()
        .and_then(|sign_in| sign_in.waiting.take())
    else {
        return;
    };
    probe.signing_in = None;
    match outcome {
        Ok(tokens) => {
            info!(
                "signed in to the BKP Registry as {}",
                tokens.email.as_deref().unwrap_or("an unnamed account")
            );
            if tokens.refresh.is_none() {
                warn!("BKP Registry: no refresh token came back, so this sign-in will not renew");
            }
            prefs.registry_token = Some(tokens.access);
            prefs.registry_login = Some(RegistryLogin {
                refresh_token: tokens.refresh,
                email: tokens.email,
            });
            probe.outcome = None;
        }
        Err(problem) => {
            warn!("{problem}");
            probe.outcome = Some(Err(problem));
        }
    }
}

pub fn on_forget_token(
    activate: On<Activate>,
    buttons: Query<(), With<ForgetTokenButton>>,
    mut prefs: ResMut<Preferences>,
    mut probe: ResMut<RegistryProbe>,
) {
    if !buttons.contains(activate.entity) {
        return;
    }
    probe.signing_in = None;
    probe.outcome = None;
    if prefs.registry_token.is_some() || prefs.registry_login.is_some() {
        prefs.registry_token = None;
        prefs.registry_login = None;
        info!("signed out of the BKP Registry");
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
    mut notices: Query<&mut Notice, With<RegistryStatus>>,
    buttons: Query<
        (Entity, Has<InteractionDisabled>, Has<TestRegistryButton>),
        Or<(With<TestRegistryButton>, With<ForgetTokenButton>)>,
    >,
) {
    let token = prefs.registry_token.as_deref();
    let can_test = token.is_some() && probe.task.is_none();
    let can_sign_out = token.is_some() || probe.signing_in.is_some();
    for (entity, disabled, test) in &buttons {
        let enabled = if test { can_test } else { can_sign_out };
        if enabled && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !enabled && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    let wanted = probe.notice(token, prefs.registry_login.as_ref());
    for mut shown in &mut notices {
        shown.set_if_neq(wanted.clone());
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

pub fn on_page_picked(
    activate: On<Activate>,
    buttons: Query<&SettingsPage, With<FeathersButton>>,
    mut shown: ResMut<ShownPage>,
) {
    if let Ok(&page) = buttons.get(activate.entity)
        && shown.0 != page
    {
        shown.0 = page;
    }
}

/// Show the page picked, and mark its button.
///
/// The others are hidden rather than taken out of the layout, so they still
/// hold the screen at the size of the tallest.
pub fn sync_page(
    shown: Res<ShownPage>,
    mut buttons: Query<(&SettingsPage, &mut ButtonVariant)>,
    mut pages: Query<(&SettingsPage, &mut Visibility), Without<ButtonVariant>>,
) {
    let shown = shown.0;
    for (page, mut variant) in &mut buttons {
        variant.set_if_neq(if *page == shown {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Plain
        });
    }
    for (page, mut visibility) in &mut pages {
        visibility.set_if_neq(if *page == shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

pub fn on_source_switch(
    change: On<ValueChange<bool>>,
    switches: Query<&SourceSwitch>,
    mut prefs: ResMut<Preferences>,
) {
    let Ok(SourceSwitch(key)) = switches.get(change.source) else {
        return;
    };
    let off = prefs.sources_off.contains(*key);
    if change.value == off {
        if change.value {
            prefs.sources_off.remove(*key);
        } else {
            prefs.sources_off.insert(key.to_string());
        }
        info!(
            "turned the {key} data source {}",
            if change.value { "on" } else { "off" }
        );
    }
}

/// Each switch as its source stands, and what belongs to a source shown only
/// while it is on.
pub fn sync_sources(
    mut commands: Commands,
    prefs: Res<Preferences>,
    switches: Query<(Entity, &SourceSwitch, Has<Checked>)>,
    mut details: Query<(&WhileSourceOn, &mut Node)>,
) {
    if !prefs.is_changed() {
        return;
    }
    for (entity, SourceSwitch(key), checked) in &switches {
        let on = !prefs.sources_off.contains(*key);
        if on && !checked {
            commands.entity(entity).insert(Checked);
        } else if !on && checked {
            commands.entity(entity).remove::<Checked>();
        }
    }
    for (details, mut node) in &mut details {
        node.display = if prefs.sources_off.contains(details.key) {
            Display::None
        } else {
            details.shown
        };
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

pub fn on_system_accent(
    change: On<ValueChange<bool>>,
    boxes: Query<(), With<SystemAccentBox>>,
    mut prefs: ResMut<Preferences>,
) {
    if boxes.contains(change.source) && prefs.system_accent != change.value {
        prefs.system_accent = change.value;
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
    accent: Query<(Entity, Has<Checked>), With<SystemAccentBox>>,
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
    for (entity, checked) in &accent {
        if prefs.system_accent && !checked {
            commands.entity(entity).insert(Checked);
        } else if !prefs.system_accent && checked {
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

pub fn on_gradient_option(
    activate: On<Activate>,
    options: Query<&GradientOption>,
    mut prefs: ResMut<Preferences>,
) {
    if let Ok(&GradientOption(gradient)) = options.get(activate.entity)
        && prefs.color_scale.gradient != gradient
    {
        prefs.color_scale.gradient = gradient;
    }
}

pub fn on_reverse_gradient(
    change: On<ValueChange<bool>>,
    boxes: Query<(), With<ReverseGradientBox>>,
    mut prefs: ResMut<Preferences>,
) {
    if boxes.contains(change.source) && prefs.color_scale.reversed != change.value {
        prefs.color_scale.reversed = change.value;
    }
}

pub fn on_whole_extent(
    change: On<ValueChange<bool>>,
    boxes: Query<(), With<WholeExtentBox>>,
    mut prefs: ResMut<Preferences>,
) {
    if boxes.contains(change.source) && prefs.color_scale.whole_extent != change.value {
        prefs.color_scale.whole_extent = change.value;
    }
}

/// Mark the gradient chosen, draw each the way round it is used, and tick the
/// boxes as the preferences stand.
pub fn sync_gradient_controls(
    mut commands: Commands,
    prefs: Res<Preferences>,
    mut options: Query<(&GradientOption, &mut ButtonVariant)>,
    mut strips: Query<(&GradientStrip, &mut BackgroundGradient)>,
    boxes: Query<
        (Entity, Has<Checked>, Has<ReverseGradientBox>),
        Or<(With<ReverseGradientBox>, With<WholeExtentBox>)>,
    >,
    mut drawn_reversed: Local<Option<bool>>,
) {
    let scale = prefs.color_scale;
    for (option, mut variant) in &mut options {
        variant.set_if_neq(if option.0 == scale.gradient {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        });
    }
    if *drawn_reversed != Some(scale.reversed) && !strips.is_empty() {
        *drawn_reversed = Some(scale.reversed);
        for (GradientStrip(gradient), mut drawn) in &mut strips {
            *drawn = strip(*gradient, scale.reversed);
        }
    }
    for (entity, checked, reverse) in &boxes {
        let on = if reverse {
            scale.reversed
        } else {
            scale.whole_extent
        };
        if on && !checked {
            commands.entity(entity).insert(Checked);
        } else if !on && checked {
            commands.entity(entity).remove::<Checked>();
        }
    }
}

/// Every source with cells to color starts drawing them as the preferences
/// say. A bookmark being restored may already have said otherwise, and wins.
fn start_scale_from_preference(
    mut commands: Commands,
    prefs: Res<Preferences>,
    sources: Query<Entity, (With<CellProperties>, Without<ColorScale>)>,
) {
    for source in &sources {
        commands.entity(source).insert_if_new(prefs.color_scale);
    }
}

/// Carry a change of preference to every source already open, unlike the
/// point cloud defaults: this is a way of looking rather than how a dataset
/// opens.
///
/// Watches the scale rather than the preferences, which change with every
/// dock dragged, and would put back a scale a bookmark had just restored.
fn follow_scale_preference(
    prefs: Res<Preferences>,
    mut sources: Query<&mut ColorScale>,
    mut followed: Local<Option<ColorScale>>,
) {
    let scale = prefs.color_scale;
    if followed.replace(scale).is_none_or(|was| was == scale) {
        return;
    }
    for mut current in &mut sources {
        current.set_if_neq(scale);
    }
}

/// The settings screen and the button that opens it.
pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_modal::<SettingsScreen>()
            .init_resource::<ShownPage>()
            .add_observer(on_page_picked)
            .add_observer(on_source_switch)
            .add_observer(on_show_logs)
            .add_observer(on_reset_layout)
            .add_observer(on_reset_point_cloud)
            .add_observer(on_remember_layout)
            .add_observer(on_theme_option)
            .add_observer(on_system_accent)
            .add_observer(on_gradient_option)
            .add_observer(on_reverse_gradient)
            .add_observer(on_whole_extent)
            .init_resource::<RegistryProbe>()
            .add_observer(on_forget_token)
            .add_observer(on_test_registry)
            .add_observer(on_sign_in)
            .add_systems(
                Update,
                (
                    poll_registry_probe,
                    poll_sign_in,
                    start_scale_from_preference,
                    follow_scale_preference,
                )
                    .in_set(Stage::ControlsApply),
            )
            .add_systems(
                Update,
                (
                    sync_settings,
                    sync_gradient_controls,
                    sync_registry,
                    sync_page,
                    sync_sources,
                )
                    .in_set(Stage::ControlsPlace),
            )
            .add_systems(Startup, spawn_settings.in_set(Boot::Shell));
    }
}
