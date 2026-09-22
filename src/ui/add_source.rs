//! Reading a dataset by its address, for whatever asked for one.
//!
//! A [`DatasetRequest`] arrives from a picker — an entry of a catalog, or an
//! address typed into its search — or from an example on the empty window.
//! One already open is shown where it was asked for; anything else is read.
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
use bevy::prelude::*;

use crate::app::schedule::Stage;
use crate::app::theme::Palette;
use crate::formats::discover::{self, Discovered};
use crate::formats::{LoadSettings, spawn_discovered};
use crate::source::SourceUrl;
use crate::view::{DatasetRequest, DatasetTarget, PendingShow, ShowFailed};
use crate::widgets::size;
use crate::widgets::{patch_node, set_text};

/// A line reporting how the last read went, for a place with no frame of its
/// own to say so: the empty window's examples.
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
    /// What the status line shows, and in what color. An empty message hides
    /// the line rather than leaving a gap under the field.
    ///
    /// The colors are handed in rather than named here: which red stands out
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
            self.target = target;
            self.begin(url);
        }
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

/// The status line, for the empty window to place.
pub fn status_line() -> impl Scene {
    bsn! {
        CustomStatus
        Text({ String::new() })
        TextFont { font_size: { FontSize::Px(size::SMALL) } }
        Node { display: { Display::None } }
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
        }
        Err(message) => {
            bevy::log::warn!("{message}");
            load.status = LoadStatus::Failed(message.clone());
            // A frame left waiting would say it is reading something forever,
            // and one browsing for it says why it did not open.
            if let DatasetTarget::Show(panel) = std::mem::take(&mut load.target) {
                commands
                    .entity(panel)
                    .try_remove::<PendingShow>()
                    .try_insert(ShowFailed(message));
            }
        }
    }
    load.start_queued();
}

/// Show how the last attempt went.
pub fn sync_custom_status(
    load: Res<CustomLoad>,
    palette: Res<Palette>,
    mut labels: Query<(&mut Text, &mut TextColor, &mut Node), With<CustomStatus>>,
) {
    // The theme repaints what it knows about; this line is colored by what it
    // has to say, so it is repainted here instead.
    if !load.is_changed() && !palette.is_changed() {
        return;
    }

    let (message, color) = load.status.message(&palette);
    for (text, mut text_color, node) in &mut labels {
        let wanted = if message.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        patch_node(node, |node| node.display = wanted);
        set_text(text, &message);
        text_color.set_if_neq(TextColor(color));
    }
}

/// Reading datasets by address.
pub struct AddSourcePlugin;

impl Plugin for AddSourcePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CustomLoad>()
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
    fn a_failure_is_shown_in_its_own_color() {
        let palette = Palette::dark();
        let (message, color) =
            LoadStatus::Failed("could not recognise it".into()).message(&palette);
        assert_eq!(message, "could not recognise it");
        assert_eq!(color, palette.problem);
        assert_ne!(palette.problem, palette.progress);
    }

    #[test]
    fn progress_and_success_name_what_they_are_about() {
        let palette = Palette::dark();
        let (loading, color) = LoadStatus::Loading("https://store/x.json".into()).message(&palette);
        assert!(loading.contains("https://store/x.json"));
        assert_eq!(color, palette.progress);

        let (loaded, color) = LoadStatus::Loaded("Sections".into()).message(&palette);
        assert!(loaded.contains("Sections"));
        assert_eq!(color, palette.progress);
    }

    #[test]
    fn an_empty_address_is_refused_rather_than_fetched() {
        let mut load = CustomLoad::default();
        load.request("   ".into(), DatasetTarget::NewFrame);
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
