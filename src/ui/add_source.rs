//! Reading a dataset by its address, for whatever asked for one.
//!
//! A [`DatasetRequest`] arrives from a picker — an entry of a catalog, or an
//! address typed into its search — or from an example on the empty window.
//! One already open is shown where it was asked for; anything else is read.
//!
//! What the URL points at is worked out by reading it rather than by asking:
//! `formats::discover` recognizes the format, and whatever it finds is
//! registered as a source like any other, so a dataset opened here is
//! indistinguishable afterwards from one named on the command line. A source
//! that matches nothing leaves the message it failed with on screen.
//!
//! The read — an HTTP fetch and a parse — runs on a task, so the window keeps
//! drawing while it is in flight.

use crate::app::net::{Fetching, fetching};
use bevy::prelude::*;

use crate::app::schedule::Stage;
use crate::formats::discover::{self, Discovered};
use crate::formats::{LoadSettings, spawn_discovered};
use crate::source::SourceUrl;
use crate::view::{DatasetRequest, DatasetTarget, PendingShow, ShowFailed};
use crate::widgets::{Notice, Tone, notice};

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
    /// What the status line shows. An empty message hides the line rather
    /// than leaving a gap under the field.
    pub fn notice(&self) -> Notice {
        match self {
            LoadStatus::Idle => Notice::default(),
            LoadStatus::Loading(source) => {
                Notice::new(Tone::Info, format!("Reading {source}\u{2026}"))
            }
            LoadStatus::Loaded(name) => Notice::new(Tone::Success, format!("Loaded {name}")),
            LoadStatus::Failed(message) => Notice::new(Tone::Error, message.clone()),
        }
    }
}

/// One dataset being read, and where it goes once it is open.
struct Reading {
    /// The address, recorded on the source it becomes.
    url: String,
    target: DatasetTarget,
    task: Fetching<Result<Discovered, String>>,
}

/// The reads in flight, and what to say about the last.
///
/// Several at once, so the planes "All views" asks for are read together
/// rather than each waiting on the one before. Two for the same frame cannot
/// both land: asking a frame for another dataset gives up the read it was
/// waiting on.
#[derive(Resource, Default)]
pub struct CustomLoad {
    reading: Vec<Reading>,
    pub status: LoadStatus,
}

impl CustomLoad {
    /// Read `url`, and open it where `target` says.
    pub fn request(&mut self, url: String, target: DatasetTarget) {
        let url = url.trim().to_string();
        if url.is_empty() {
            self.status = LoadStatus::Failed("type the URL of a dataset to load".into());
            return;
        }
        if self
            .reading
            .iter()
            .any(|reading| reading.url == url && reading.target == target)
        {
            return;
        }
        if let DatasetTarget::Show(panel) = target {
            self.reading
                .retain(|reading| reading.target != DatasetTarget::Show(panel));
        }
        self.status = LoadStatus::Loading(url.clone());
        let read = url.clone();
        self.reading.push(Reading {
            url,
            target,
            task: fetching(async move { discover::discover(&read).await }),
        });
    }

    /// Whether a dataset being read is to be drawn over `panel`.
    pub fn is_layering_onto(&self, panel: Entity) -> bool {
        self.reading
            .iter()
            .any(|reading| reading.target == DatasetTarget::Layer(panel))
    }
}

/// The status line, for the empty window to place.
pub fn status_line() -> impl Scene {
    bsn! {
        notice()
        CustomStatus
    }
}

/// Answer requests for known datasets: one already open goes straight to the
/// frame it was asked for, and anything else is read.
///
/// Recognized by the address it was read from rather than by a record kept
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
    // Polled without marking the load changed, which would repaint its
    // status line every frame whether or not anything landed.
    let mut finished = Vec::new();
    let reading = &mut load.bypass_change_detection().reading;
    reading.retain_mut(|reading| match reading.task.take() {
        Some(outcome) => {
            finished.push((std::mem::take(&mut reading.url), reading.target, outcome));
            false
        }
        None => true,
    });
    if finished.is_empty() {
        return;
    }
    for (url, target, outcome) in finished {
        land(&mut commands, &mut load, *settings, url, target, outcome);
    }
}

/// Open a dataset that has been read where it was asked for, or say why it
/// could not be.
fn land(
    commands: &mut Commands,
    load: &mut CustomLoad,
    settings: LoadSettings,
    url: String,
    target: DatasetTarget,
    outcome: Result<Discovered, String>,
) {
    match outcome {
        // Several datasets at once, which open as a bookmark does: in place of
        // every frame, so the one that asked is replaced along with the rest.
        Ok(Discovered::Scene(scene)) => {
            load.status = LoadStatus::Loaded(scene.name.clone());
            crate::bookmark::restore(commands, crate::bookmark::scene::bookmark_of(&scene));
        }
        Ok(discovered) => {
            load.status = LoadStatus::Loaded(discovered.name().to_string());
            commands.queue(move |world: &mut World| {
                let Some(source) = spawn_discovered(world, discovered, settings) else {
                    return;
                };
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
            if let DatasetTarget::Show(panel) = target {
                commands
                    .entity(panel)
                    .try_remove::<PendingShow>()
                    .try_insert(ShowFailed(message));
            }
        }
    }
}

/// Show how the last attempt went.
pub fn sync_custom_status(
    load: Res<CustomLoad>,
    mut notices: Query<&mut Notice, With<CustomStatus>>,
) {
    if !load.is_changed() {
        return;
    }
    let wanted = load.status.notice();
    for mut shown in &mut notices {
        shown.set_if_neq(wanted.clone());
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
        assert!(
            LoadStatus::Idle.notice().message.is_empty(),
            "an empty line would leave a gap"
        );
    }

    #[test]
    fn a_failure_is_shown_as_an_error() {
        let notice = LoadStatus::Failed("could not recognize it".into()).notice();
        assert_eq!(notice.message, "could not recognize it");
        assert_eq!(notice.tone, Tone::Error);
    }

    #[test]
    fn progress_and_success_name_what_they_are_about() {
        let loading = LoadStatus::Loading("https://store/x.json".into()).notice();
        assert!(loading.message.contains("https://store/x.json"));
        assert_eq!(loading.tone, Tone::Info);

        let loaded = LoadStatus::Loaded("Sections".into()).notice();
        assert!(loaded.message.contains("Sections"));
        assert_eq!(loaded.tone, Tone::Success);
    }

    #[test]
    fn an_empty_address_is_refused_rather_than_fetched() {
        let mut load = CustomLoad::default();
        load.request("   ".into(), DatasetTarget::NewFrame);
        assert!(load.reading.is_empty());
        assert!(matches!(load.status, LoadStatus::Failed(_)));
    }

    // Paths that do not exist, so the reads fail off disk rather than reaching
    // for the network.
    const A: &str = "/nonexistent/a.svg";
    const B: &str = "/nonexistent/b.svg";

    #[test]
    fn datasets_are_read_together_and_each_only_once() {
        let panel = Entity::from_raw_u32(7).unwrap();
        let mut load = CustomLoad::default();
        load.request(A.into(), DatasetTarget::NewFrame);
        load.request(B.into(), DatasetTarget::NewFrame);
        load.request(A.into(), DatasetTarget::NewFrame);
        // The same dataset for somewhere else is a different request.
        load.request(A.into(), DatasetTarget::Layer(panel));
        assert_eq!(load.reading.len(), 3);
    }

    #[test]
    fn a_frame_asked_for_another_dataset_gives_up_the_first() {
        let panel = Entity::from_raw_u32(7).unwrap();
        let mut load = CustomLoad::default();
        load.request(A.into(), DatasetTarget::Show(panel));
        load.request(B.into(), DatasetTarget::Show(panel));
        let urls: Vec<&str> = load.reading.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, [B]);
    }
}
