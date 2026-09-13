//! Adding a dataset by typing its URL.
//!
//! Sits at the bottom of View configuration's **Edit layout** menu, under the
//! datasets already loaded, because that menu is where what is on screen is
//! chosen and this is one more thing to choose.
//!
//! What the URL points at is worked out by reading it rather than by asking:
//! `formats::discover` recognises the format, and whatever it finds is
//! registered as a source like any other, so a dataset opened here is
//! indistinguishable afterwards from one named on the command line. A source
//! that matches nothing leaves the message it failed with on screen.
//!
//! The read is blocking — an HTTP fetch and a parse — so it runs on a task and
//! the window keeps drawing while it is in flight.

use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy_feathers::controls::{FeathersButton, FeathersTextInput, FeathersTextInputContainer};
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::Activate;

use crate::app::schedule::Stage;
use crate::formats::discover::{self, Discovered};
use crate::formats::{LoadSettings, spawn_discovered};
use crate::view::{BlocksFrameInput, PanelRequest};

/// The field a URL is typed into. On the inner text entity, which is the one
/// holding the [`EditableText`], rather than on its container.
#[derive(Component, Clone, Default)]
pub struct CustomUrlInput;

/// The button that opens whatever the field names.
#[derive(Component, Clone, Default)]
pub struct LoadCustomButton;

/// The line under the field reporting how the last attempt went.
#[derive(Component, Clone, Default)]
pub struct CustomStatus;

/// Colour of a status line that reports progress rather than a problem.
const PROGRESS: Color = Color::srgb(0.70, 0.76, 0.85);
/// Colour of a status line naming something that went wrong.
const PROBLEM: Color = Color::srgb(0.95, 0.48, 0.45);

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
    pub fn message(&self) -> (String, Color) {
        match self {
            LoadStatus::Idle => (String::new(), PROGRESS),
            LoadStatus::Loading(source) => (format!("reading {source}\u{2026}"), PROGRESS),
            LoadStatus::Loaded(name) => (format!("loaded {name}"), PROGRESS),
            LoadStatus::Failed(message) => (message.clone(), PROBLEM),
        }
    }
}

/// The load in flight, if any, and what to say about it.
#[derive(Resource, Default)]
pub struct CustomLoad {
    task: Option<Task<Result<Discovered, String>>>,
    pub status: LoadStatus,
}

impl CustomLoad {
    pub fn is_loading(&self) -> bool {
        self.task.is_some()
    }

    /// Start reading `url`, unless something is already being read.
    ///
    /// One at a time: two sources arriving together would each want a frame,
    /// and a second press while the first is still fetching reads as the first
    /// press not having worked.
    pub fn start(&mut self, url: String) {
        if self.is_loading() {
            return;
        }
        let url = url.trim().to_string();
        if url.is_empty() {
            self.status = LoadStatus::Failed("type the URL of a dataset to load".into());
            return;
        }
        self.status = LoadStatus::Loading(url.clone());
        self.task =
            Some(AsyncComputeTaskPool::get().spawn(async move { discover::discover(&url) }));
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
                    label_dim("An OME-Zarr store or Scatterbrain metadata URL. Which it is \
                               is worked out from the data.")
                    InheritableFont { font_size: { 11.0f32 } }
                ),
            ]
        })
        .id();

    let row = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
            }
            Children [
                (
                    @FeathersTextInputContainer
                    BlocksFrameInput
                    Children [(
                        @FeathersTextInput
                        CustomUrlInput
                    )]
                ),
                (
                    @FeathersButton {
                        @caption: { bsn_list![label("Load")] }
                    }
                    BlocksFrameInput
                    LoadCustomButton
                    // The field gives way instead, so a long URL never squeezes
                    // the button out of the row.
                    Node { flex_shrink: { 0.0_f32 } }
                ),
            ]
        })
        .id();

    let status = commands
        .spawn_scene(bsn! {
            CustomStatus
            Text({ String::new() })
            TextFont { font_size: { bevy::text::FontSize::Px(11.0) } }
            TextColor({ PROGRESS })
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
    buttons: Query<(), With<LoadCustomButton>>,
    inputs: Query<&EditableText, With<CustomUrlInput>>,
    mut load: ResMut<CustomLoad>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    load.start(typed(&inputs));
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
    if inputs.get(key.focused_entity).is_err() || !key.input.state.is_pressed() {
        return;
    }
    if matches!(key.input.key_code, KeyCode::Enter | KeyCode::NumpadEnter) {
        load.start(typed(&inputs));
    }
}

fn typed(inputs: &Query<&EditableText, With<CustomUrlInput>>) -> String {
    inputs
        .iter()
        .next()
        .map(|text| text.value().to_string())
        .unwrap_or_default()
}

/// Register whatever the task came back with, and open a frame onto it.
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
    let Some(outcome) = block_on(poll_once(task)) else {
        return;
    };
    load.task = None;

    match outcome {
        Ok(discovered) => {
            load.status = LoadStatus::Loaded(discovered.name().to_string());
            let settings = *settings;
            commands.queue(move |world: &mut World| {
                let source = spawn_discovered(world, discovered, settings);
                world.write_message(PanelRequest::Open(source));
            });
            // The URL has been opened, so leave the field ready for the next
            // one rather than holding a value that would load a duplicate.
            for mut text in &mut inputs {
                text.clear();
            }
        }
        Err(message) => {
            bevy::log::warn!("{message}");
            load.status = LoadStatus::Failed(message);
        }
    }
}

/// Show how the last attempt went, and hold the button while one is in flight.
pub fn sync_custom_status(
    mut commands: Commands,
    load: Res<CustomLoad>,
    mut labels: Query<(&mut Text, &mut TextColor, &mut Node), With<CustomStatus>>,
    buttons: Query<Entity, With<LoadCustomButton>>,
) {
    if !load.is_changed() {
        return;
    }

    let (message, colour) = load.status.message();
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
                (poll_custom_load, sync_custom_status)
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
        let (message, _) = LoadStatus::Idle.message();
        assert!(message.is_empty(), "an empty line would leave a gap");
    }

    #[test]
    fn a_failure_is_shown_in_its_own_colour() {
        let (message, colour) = LoadStatus::Failed("could not recognise it".into()).message();
        assert_eq!(message, "could not recognise it");
        assert_eq!(colour, PROBLEM);
        assert_ne!(PROBLEM, PROGRESS);
    }

    #[test]
    fn progress_and_success_name_what_they_are_about() {
        let (loading, colour) = LoadStatus::Loading("https://store/x.json".into()).message();
        assert!(loading.contains("https://store/x.json"));
        assert_eq!(colour, PROGRESS);

        let (loaded, colour) = LoadStatus::Loaded("Sections".into()).message();
        assert!(loaded.contains("Sections"));
        assert_eq!(colour, PROGRESS);
    }

    #[test]
    fn an_empty_field_is_refused_rather_than_fetched() {
        let mut load = CustomLoad::default();
        load.start("   ".into());
        assert!(!load.is_loading());
        assert!(matches!(load.status, LoadStatus::Failed(_)));
    }
}
