//! Adding a dataset by typing its URL.
//!
//! Sits at the bottom of View configuration's **Edit layout** menu, under the
//! datasets already loaded, because that menu is where what is on screen is
//! chosen and this is one more thing to choose. The empty window spawns a
//! second one, so the section is built by a function rather than being a place:
//! each carries its own field, button and status line, and a button loads from
//! the field it was spawned beside rather than from whichever field is found
//! first.
//!
//! What the URL points at is worked out by reading it rather than by asking:
//! `formats::discover` recognises the format, and whatever it finds is
//! registered as a source like any other, so a dataset opened here is
//! indistinguishable afterwards from one named on the command line. A source
//! that matches nothing leaves the message it failed with on screen.
//!
//! The read — an HTTP fetch and a parse — runs on a task, so the window keeps
//! drawing while it is in flight.

use crate::app::net::{Fetching, fetching};
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy_feathers::controls::{FeathersButton, FeathersTextInput, FeathersTextInputContainer};
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::Activate;

use crate::app::schedule::Stage;
use crate::app::theme::Palette;
use crate::formats::discover::{self, Discovered};
use crate::formats::{LoadSettings, spawn_discovered};
use crate::source::SourceUrl;
use crate::view::{BlocksFrameInput, DatasetRequest, DatasetTarget, PendingShow};
use crate::widgets::button_text;

/// The field a URL is typed into. On the inner text entity, which is the one
/// holding the [`EditableText`], rather than on its container.
#[derive(Component, Clone, Default)]
pub struct CustomUrlInput;

/// The button that opens whatever its own field names.
#[derive(Component, Clone)]
pub struct LoadCustomButton {
    pub field: Entity,
}

impl Default for LoadCustomButton {
    fn default() -> Self {
        LoadCustomButton {
            field: Entity::PLACEHOLDER,
        }
    }
}

/// The line under the field reporting how the last attempt went.
#[derive(Component, Clone, Default)]
pub struct CustomStatus;

/// How the last attempt to open a URL went.
#[derive(Default, PartialEq, Eq, Clone, Debug)]
pub enum LoadStatus {
    /// Nothing asked for yet, so the line says nothing.
    #[default]
    Idle,
    Loading(String),
    Loaded(String),
    Failed(String),
}

impl LoadStatus {
    /// What the status line shows, and in what colour. An empty message hides
    /// the line rather than leaving a gap under the field.
    ///
    /// The colours are handed in rather than named here: which red stands out
    /// depends on what it is standing out against, and that is the theme's
    /// business.
    pub fn message(&self, palette: &Palette) -> (String, Color) {
        match self {
            LoadStatus::Idle => (String::new(), palette.progress),
            LoadStatus::Loading(source) => (format!("reading {source}\u{2026}"), palette.progress),
            LoadStatus::Loaded(name) => (format!("loaded {name}"), palette.progress),
            LoadStatus::Failed(message) => (message.clone(), palette.problem),
        }
    }
}

/// The load in flight, if any, and what to say about it.
#[derive(Resource, Default)]
pub struct CustomLoad {
    task: Option<Fetching<Result<Discovered, String>>>,
    /// The field the URL was typed into, so that opening it clears that field
    /// and not one the user is still typing in elsewhere. Absent for a load
    /// that came from a button rather than a field.
    field: Option<Entity>,
    /// Where the dataset being read goes once it is open.
    target: DatasetTarget,
    /// Known datasets asked for while another was being read, in order.
    ///
    /// Queued rather than refused: a layer is chosen from a frame's menu, far
    /// from the status line that would say a second choice had been dropped.
    queued: std::collections::VecDeque<(String, DatasetTarget)>,
    /// The URL being read, recorded on the source it becomes.
    loading: String,
    pub status: LoadStatus,
}

impl CustomLoad {
    pub fn is_loading(&self) -> bool {
        self.task.is_some()
    }

    /// Read `url`, now or once what is being read has landed, and open it
    /// where `target` says.
    ///
    /// One read at a time, so two datasets never race for the same cell.
    pub fn request(&mut self, url: String, target: DatasetTarget) {
        let url = url.trim().to_string();
        if self.already_asked(&url, target) {
            return;
        }
        self.queued.push_back((url, target));
        self.start_queued();
    }

    /// The frame the dataset being read will be layered onto, if any.
    pub fn loading_onto(&self) -> Option<Entity> {
        match self.target {
            DatasetTarget::Layer(panel) if self.is_loading() => Some(panel),
            _ => None,
        }
    }

    /// Whether `url` is already being read, or waiting to be, for `target`.
    fn already_asked(&self, url: &str, target: DatasetTarget) -> bool {
        (self.is_loading() && self.loading == url && self.target == target)
            || self
                .queued
                .iter()
                .any(|(queued, queued_for)| queued == url && *queued_for == target)
    }

    fn start_queued(&mut self) {
        if self.is_loading() {
            return;
        }
        if let Some((url, target)) = self.queued.pop_front() {
            self.field = None;
            self.target = target;
            self.begin(url);
        }
    }

    /// Start reading what was typed into `field`, which is emptied once the
    /// dataset is open.
    ///
    /// Refused rather than queued while something is being read: the status
    /// line under the field says so, and a URL still in the field can simply
    /// be loaded again.
    pub fn start_from(&mut self, field: Entity, url: String) {
        if self.is_loading() {
            return;
        }
        self.field = Some(field);
        self.target = DatasetTarget::NewFrame;
        self.begin(url);
    }

    fn begin(&mut self, url: String) {
        if self.is_loading() {
            return;
        }
        let url = url.trim().to_string();
        if url.is_empty() {
            self.status = LoadStatus::Failed("type the URL of a dataset to load".into());
            return;
        }
        self.status = LoadStatus::Loading(url.clone());
        self.loading = url.clone();
        self.task = Some(fetching(async move { discover::discover(&url).await }));
    }
}

/// Build the section, for the menu to hang off its end.
pub fn spawn_custom_section(commands: &mut Commands) -> Entity {
    let section = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                row_gap: { Val::Px(4.0) },
                margin: { UiRect::top(Val::Px(10.0)) },
            }
            Children [
                (
                    label("Custom visualization")
                    InheritableFont { font_size: { 12.0f32 } }
                ),
                (
                    label_dim("An OME-Zarr store, Deep Zoom .dzi, Scatterbrain .json or .svg URL")
                    InheritableFont { font_size: { 11.0f32 } }
                ),
            ]
        })
        .id();

    // Spawned before the row so the button can be told which field it loads.
    let field = commands
        .spawn_scene(bsn! {
            @FeathersTextInput
            CustomUrlInput
        })
        .id();
    let entry = commands
        .spawn_scene(bsn! {
            @FeathersTextInputContainer
            BlocksFrameInput
        })
        .id();
    commands.entity(entry).add_child(field);

    let button = commands
        .spawn_scene(bsn! {
            @FeathersButton {
                @caption: { bsn_list![button_text("Load")] }
            }
            BlocksFrameInput
            LoadCustomButton { field: { field } }
            // The field gives way instead, so a long URL never squeezes the
            // button out of the row.
            Node { flex_shrink: { 0.0_f32 } }
        })
        .id();

    let row = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
            }
        })
        .id();
    commands.entity(row).add_children(&[entry, button]);

    let status = commands
        .spawn_scene(bsn! {
            CustomStatus
            Text({ String::new() })
            TextFont { font_size: { FontSize::Px(11.0) } }
            Node { display: { Display::None } }
        })
        .id();

    commands.entity(section).add_children(&[row, status]);
    section
}

/// Open what the field names when the button is pressed.
///
/// Feathers controls report a press by triggering [`Activate`] on themselves
/// and carry no `Interaction`, so this is an observer rather than a system
/// polling for a changed interaction.
pub fn on_load_pressed(
    activate: On<Activate>,
    buttons: Query<&LoadCustomButton>,
    inputs: Query<&EditableText, With<CustomUrlInput>>,
    mut load: ResMut<CustomLoad>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Ok(typed) = inputs.get(button.field) else {
        return;
    };
    load.start_from(button.field, typed.value().to_string());
}

/// Open what the field names when return is pressed in it.
///
/// The field allows no newlines, so return is left unhandled by the widget and
/// is free to mean "load this".
pub fn on_url_submitted(
    key: On<FocusedInput<KeyboardInput>>,
    inputs: Query<&EditableText, With<CustomUrlInput>>,
    mut load: ResMut<CustomLoad>,
) {
    let Ok(typed) = inputs.get(key.focused_entity) else {
        return;
    };
    if !key.input.state.is_pressed() {
        return;
    }
    if matches!(key.input.key_code, KeyCode::Enter | KeyCode::NumpadEnter) {
        load.start_from(key.focused_entity, typed.value().to_string());
    }
}

/// Answer requests for known datasets: one already open goes straight to the
/// frame it was asked for, and anything else is read.
///
/// Recognised by the address it was read from rather than by a record kept
/// here, so a dataset named on the command line counts as open too.
pub fn answer_dataset_requests(
    mut requests: MessageReader<DatasetRequest>,
    sources: Query<(Entity, &SourceUrl)>,
    mut load: ResMut<CustomLoad>,
    mut panels: MessageWriter<crate::view::PanelRequest>,
) {
    for request in requests.read() {
        let url = request.url.trim();
        match sources.iter().find(|(_, opened)| opened.0 == url) {
            Some((source, _)) => {
                panels.write(request.target.request_for(source));
            }
            None => load.request(url.to_string(), request.target),
        }
    }
    load.start_queued();
}

/// Register whatever the task came back with, and put it where it was asked
/// for: a frame of its own, in place of a frame's dataset, or over it.
///
/// Registration wants the world rather than a query, so it is queued as a
/// command; the request to open a frame goes with it, from inside the same
/// command, so the frame cannot be asked for before the source it points at
/// exists.
pub fn poll_custom_load(
    mut commands: Commands,
    mut load: ResMut<CustomLoad>,
    settings: Res<LoadSettings>,
    mut inputs: Query<&mut EditableText, With<CustomUrlInput>>,
) {
    let Some(task) = load.task.as_mut() else {
        return;
    };
    let Some(outcome) = task.take() else {
        return;
    };
    load.task = None;

    match outcome {
        Ok(discovered) => {
            load.status = LoadStatus::Loaded(discovered.name().to_string());
            let url = std::mem::take(&mut load.loading);
            let target = std::mem::take(&mut load.target);
            let settings = *settings;
            commands.queue(move |world: &mut World| {
                let source = spawn_discovered(world, discovered, settings);
                // Recorded where the source entity first exists: what a URL
                // opened as is the difference between offering it again and
                // fetching it again.
                world.entity_mut(source).insert(SourceUrl(url));
                world.write_message(target.request_for(source));
            });
            // The URL has been opened, so leave the field ready for the next
            // one rather than holding a value that would load a duplicate.
            if let Some(mut text) = load.field.and_then(|field| inputs.get_mut(field).ok()) {
                text.clear();
            }
        }
        Err(message) => {
            bevy::log::warn!("{message}");
            load.status = LoadStatus::Failed(message);
            // A frame left waiting would say it is reading something forever.
            if let DatasetTarget::Show(panel) = std::mem::take(&mut load.target) {
                commands.entity(panel).try_remove::<PendingShow>();
            }
        }
    }
    load.start_queued();
}

/// Show how the last attempt went, and hold the button while one is in flight.
pub fn sync_custom_status(
    mut commands: Commands,
    load: Res<CustomLoad>,
    palette: Res<Palette>,
    mut labels: Query<(&mut Text, &mut TextColor, &mut Node), With<CustomStatus>>,
    buttons: Query<Entity, With<LoadCustomButton>>,
) {
    // The theme repaints what it knows about; this line is coloured by what it
    // has to say, so it is repainted here instead.
    if !load.is_changed() && !palette.is_changed() {
        return;
    }

    let (message, colour) = load.status.message(&palette);
    for (mut text, mut text_colour, mut node) in &mut labels {
        let wanted = if message.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != wanted {
            node.display = wanted;
        }
        if text.0 != message {
            text.0 = message.clone();
        }
        if text_colour.0 != colour {
            text_colour.0 = colour;
        }
    }

    for button in &buttons {
        if load.is_loading() {
            commands.entity(button).insert(InteractionDisabled);
        } else {
            commands.entity(button).remove::<InteractionDisabled>();
        }
    }
}

/// The custom dataset field, and what it opens.
pub struct AddSourcePlugin;

impl Plugin for AddSourcePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CustomLoad>()
            .add_observer(on_load_pressed)
            .add_observer(on_url_submitted)
            // Registering a source is the furthest-reaching thing a control
            // does, so it happens in the same stage as every other write
            // through to a source — and before `Stage::Sources`, so a dataset
            // opened this frame starts streaming this frame.
            .add_systems(
                Update,
                (
                    answer_dataset_requests,
                    poll_custom_load,
                    sync_custom_status,
                )
                    .chain()
                    .in_set(Stage::ControlsApply),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_typed_yet_shows_no_status_line() {
        let (message, _) = LoadStatus::Idle.message(&Palette::dark());
        assert!(message.is_empty(), "an empty line would leave a gap");
    }

    #[test]
    fn a_failure_is_shown_in_its_own_colour() {
        let palette = Palette::dark();
        let (message, colour) =
            LoadStatus::Failed("could not recognise it".into()).message(&palette);
        assert_eq!(message, "could not recognise it");
        assert_eq!(colour, palette.problem);
        assert_ne!(palette.problem, palette.progress);
    }

    #[test]
    fn progress_and_success_name_what_they_are_about() {
        let palette = Palette::dark();
        let (loading, colour) =
            LoadStatus::Loading("https://store/x.json".into()).message(&palette);
        assert!(loading.contains("https://store/x.json"));
        assert_eq!(colour, palette.progress);

        let (loaded, colour) = LoadStatus::Loaded("Sections".into()).message(&palette);
        assert!(loaded.contains("Sections"));
        assert_eq!(colour, palette.progress);
    }

    #[test]
    fn an_empty_field_is_refused_rather_than_fetched() {
        let mut load = CustomLoad::default();
        load.start_from(Entity::PLACEHOLDER, "   ".into());
        assert!(!load.is_loading());
        assert!(matches!(load.status, LoadStatus::Failed(_)));
    }

    #[test]
    fn asking_again_for_what_is_already_queued_is_recognised() {
        let panel = Entity::from_raw_u32(7).unwrap();
        let mut load = CustomLoad::default();
        load.queued
            .push_back(("https://store/a.svg".into(), DatasetTarget::Layer(panel)));
        assert!(load.already_asked("https://store/a.svg", DatasetTarget::Layer(panel)));
        // The same dataset for somewhere else is a different request.
        assert!(!load.already_asked("https://store/a.svg", DatasetTarget::NewFrame));
        assert!(!load.already_asked("https://store/a.svg", DatasetTarget::Show(panel)));
        assert!(!load.already_asked("https://store/b.svg", DatasetTarget::Layer(panel)));
    }
}
