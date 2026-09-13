//! Saving a picture of one frame.
//!
//! The renderer can only screenshot a whole window, so this takes one and cuts
//! out the frame's viewport. What it cuts is exactly the pixels that frame
//! drew, at the size it is on screen.
//!
//! The frame's own chrome is hidden for the shot and put back afterwards — a
//! saved image of a dataset with buttons and a selection outline burnt into it
//! is a picture of the app rather than of the data. Hiding goes through
//! `Visibility` rather than `Node::display`, because several systems rewrite
//! `display` every frame and would put the chrome straight back.
//!
//! Where the frame drew nothing, the picture is transparent. The window's own
//! alpha channel cannot say that — with HDR on it carries brightness rather
//! than opacity, which is why Bevy's own saver discards it — so this keys out
//! the colour the frame clears to instead. Everything else is written fully
//! opaque. A colour key cuts hard edges: a point drawn half over the
//! background keeps a little of it, so an edge can carry a dark fringe, and
//! data that happens to be exactly the background colour goes with it.
//!
//! Capture is asynchronous: the shot is taken at the end of a frame and arrives
//! some frames later. The chrome therefore has to be hidden before the shot and
//! restored by whatever comes back — including nothing, which is why the wait
//! gives up rather than leaving a frame stripped of its controls for good.
//!
//! Writing the file is slower than the shot: several megapixels of PNG is work,
//! and doing it where the pixels arrive froze the window for as long as it took.
//! The encode goes to a task, and the chrome comes back the moment the pixels
//! are in hand rather than when the file is on disk.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::display::label;

use crate::app::schedule::Stage;
use crate::source::DataSource;
use crate::view::chrome::{PanelButton, SelectionBorder};
use crate::view::overlay::{PanelHeader, PanelTooltip};
use crate::view::{BlocksFrameInput, Panel, ShowsSource};

/// Where pictures are written, relative to where the app was started.
const FOLDER: &str = "screenshots";

/// How long the line naming the saved file stays up.
const NOTICE_SECONDS: f32 = 6.0;

/// Frames to wait for a shot before giving up and restoring the chrome.
///
/// Generous: a capture normally lands within two or three frames, and the cost
/// of waiting too long is a moment without a header, while the cost of giving
/// up too early is a picture with one in it.
const PATIENCE: u32 = 60;

/// The button that saves a picture of the frame it belongs to.
#[derive(Component, Clone)]
pub struct PanelCaptureButton {
    panel: Entity,
}

impl Default for PanelCaptureButton {
    fn default() -> Self {
        PanelCaptureButton {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// The line under a frame's status saying where its last picture went.
#[derive(Component, Clone)]
pub struct CaptureNotice {
    panel: Entity,
    /// Seconds left before it takes itself down.
    remaining: f32,
}

impl Default for CaptureNotice {
    fn default() -> Self {
        CaptureNotice {
            panel: Entity::PLACEHOLDER,
            remaining: 0.0,
        }
    }
}

/// Marks chrome hidden for a capture, so exactly what was hidden is restored.
#[derive(Component)]
pub struct HiddenForCapture;

/// How far along a capture is.
#[derive(Default)]
enum Progress {
    #[default]
    Idle,
    /// Asked for, with the chrome still up.
    Asked(Entity),
    /// Chrome hidden this frame; the shot is taken on the next one, so that
    /// what is hidden has been through a render before it is photographed.
    Hidden(Entity),
    /// Shot taken, waiting for the pixels to come back.
    Waiting { panel: Entity, frames: u32 },
}

/// The capture in progress, if any.
#[derive(Resource, Default)]
pub struct FrameCapture {
    stage: Progress,
    /// The file being written, and the frame it is a picture of. Held apart
    /// from the stages because the chrome is already back by then: the pixels
    /// are in hand and only the encode is left.
    writing: Option<(Entity, Task<Result<PathBuf, String>>)>,
    /// Where the last picture went, or why it did not go anywhere.
    outcome: Option<(Entity, String)>,
}

impl FrameCapture {
    fn busy(&self) -> bool {
        !matches!(self.stage, Progress::Idle) || self.writing.is_some()
    }
}

/// Add the button that saves a picture to a frame's header.
pub(super) fn spawn_capture_button(commands: &mut Commands, header: Entity, panel: Entity) {
    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                // Text, not a symbol: the default font carries no camera or
                // box glyph and draws a missing one as `?`. It names what it
                // writes, which is what the other header controls do.
                @caption: { bsn_list![label("png")] }
            }
            BlocksFrameInput
            PanelCaptureButton { panel: { panel } }
        })
        .id();
    commands.entity(header).add_child(button);
}

/// Add the line that says where a frame's last picture went. Spawned into the
/// overlay's own box, under the status, and hidden until there is something to
/// report.
pub(super) fn spawn_capture_notice(commands: &mut Commands, box_: Entity, panel: Entity) {
    let notice = commands
        .spawn_scene(bsn! {
            CaptureNotice { panel: { panel } }
            Text
            TextFont { font_size: { bevy::text::FontSize::Px(11.0) } }
            TextColor({ Color::srgb(0.70, 0.85, 0.72) })
            Node { display: { Display::None } }
        })
        .id();
    commands.entity(box_).add_child(notice);
}

/// Ask for a picture of the frame whose button was pressed.
pub fn on_capture_pressed(
    activate: On<bevy_ui_widgets::Activate>,
    buttons: Query<&PanelCaptureButton>,
    mut capture: ResMut<FrameCapture>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    // One at a time. Two captures at once would each hide the other's chrome
    // and restore it underneath the other's shot.
    if capture.busy() {
        return;
    }
    capture.stage = Progress::Asked(button.panel);
}

/// Drive a capture: hide the chrome, take the shot, and put the chrome back.
#[expect(
    clippy::too_many_arguments,
    reason = "one system owns the whole sequence so the chrome cannot be left hidden"
)]
pub fn drive_capture(
    mut commands: Commands,
    mut capture: ResMut<FrameCapture>,
    panels: Query<(&Camera, &ShowsSource), With<Panel>>,
    sources: Query<&DataSource>,
    headers: Query<(Entity, &PanelHeader)>,
    tooltips: Query<(Entity, &PanelTooltip)>,
    buttons: Query<(Entity, &PanelButton)>,
    outline: Query<Entity, With<SelectionBorder>>,
    hidden: Query<Entity, With<HiddenForCapture>>,
) {
    match capture.stage {
        Progress::Idle => {}
        Progress::Asked(panel) => {
            if panels.get(panel).is_err() {
                capture.stage = Progress::Idle;
                return;
            }
            let mut hide = |entity: Entity| {
                commands
                    .entity(entity)
                    .insert((Visibility::Hidden, HiddenForCapture));
            };
            for (entity, header) in &headers {
                if header.panel == panel {
                    hide(entity);
                }
            }
            for (entity, tooltip) in &tooltips {
                if tooltip.panel == panel {
                    hide(entity);
                }
            }
            for (entity, button) in &buttons {
                if button.panel == panel {
                    hide(entity);
                }
            }
            // The outline marks the selected frame and sits inside the cell, so
            // it lands in the picture whether or not this is the frame it is on.
            for entity in &outline {
                hide(entity);
            }
            capture.stage = Progress::Hidden(panel);
        }
        Progress::Hidden(panel) => {
            let Ok((camera, shows)) = panels.get(panel) else {
                capture.stage = Progress::Idle;
                restore(&mut commands, &hidden);
                return;
            };
            let Some(viewport) = camera.viewport.as_ref() else {
                capture.outcome = Some((panel, "the frame has no viewport to read".into()));
                capture.stage = Progress::Idle;
                restore(&mut commands, &hidden);
                return;
            };
            let name = sources
                .get(shows.0)
                .map(|source| source.name.clone())
                .unwrap_or_else(|_| "frame".into());
            let rect = (viewport.physical_position, viewport.physical_size);
            let path = picture_path(&name);

            commands.spawn(Screenshot::primary_window()).observe(
                move |captured: On<ScreenshotCaptured>,
                      mut commands: Commands,
                      mut capture: ResMut<FrameCapture>| {
                    commands.entity(captured.entity).despawn();
                    // The despawn lands a moment later, and the renderer can
                    // take a second shot of the same entity meanwhile. Only the
                    // one still being waited for is kept.
                    if !matches!(capture.stage, Progress::Waiting { .. }) {
                        return;
                    }
                    capture.stage = Progress::Idle;
                    let image = captured.image.clone();
                    let path = path.clone();
                    capture.writing = Some((
                        panel,
                        AsyncComputeTaskPool::get()
                            .spawn(async move { save_cropped(image, rect, path) }),
                    ));
                },
            );
            capture.stage = Progress::Waiting { panel, frames: 0 };
        }
        Progress::Waiting { panel, frames } => {
            if frames >= PATIENCE {
                capture.outcome = Some((panel, "the picture never arrived".into()));
                capture.stage = Progress::Idle;
            } else {
                capture.stage = Progress::Waiting {
                    panel,
                    frames: frames + 1,
                };
            }
        }
    }

    // Everything hidden goes back the moment the shot is in hand, however that
    // went: a frame left without its header has no way to get one back, and the
    // file is written from pixels that are already captured.
    if matches!(capture.stage, Progress::Idle) {
        restore(&mut commands, &hidden);
    }
}

/// Report a file once it has been written.
pub fn poll_capture_save(mut capture: ResMut<FrameCapture>) {
    let Some((panel, task)) = capture.writing.as_mut() else {
        return;
    };
    let Some(result) = block_on(poll_once(task)) else {
        return;
    };
    let panel = *panel;
    capture.writing = None;
    let outcome = match result {
        Ok(written) => format!("saved {}", written.display()),
        Err(e) => format!("could not save the picture: {e}"),
    };
    bevy::log::info!("{outcome}");
    capture.outcome = Some((panel, outcome));
}

fn restore(commands: &mut Commands, hidden: &Query<Entity, With<HiddenForCapture>>) {
    for entity in hidden {
        commands
            .entity(entity)
            .insert(Visibility::Inherited)
            .remove::<HiddenForCapture>();
    }
}

/// Show where a picture went, for a few seconds, on the frame it came from.
pub fn show_capture_notice(
    time: Res<Time>,
    mut capture: ResMut<FrameCapture>,
    mut notices: Query<(&mut CaptureNotice, &mut Text, &mut Node)>,
) {
    let fresh = capture.outcome.take();
    for (mut notice, mut text, mut node) in &mut notices {
        if let Some((panel, message)) = &fresh
            && notice.panel == *panel
        {
            text.0 = message.clone();
            notice.remaining = NOTICE_SECONDS;
        }
        if notice.remaining > 0.0 {
            notice.remaining -= time.delta_secs();
        }
        let wanted = if notice.remaining > 0.0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// Where a picture of `name` is written.
///
/// Named after the dataset and the moment, so that saving twice never silently
/// overwrites the first one. Seconds since the epoch rather than a date,
/// because a date needs a calendar crate to format and this only has to sort.
fn picture_path(name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    PathBuf::from(FOLDER).join(format!("{}-{stamp}.png", file_stem(name)))
}

/// A dataset's name, reduced to something a filesystem is happy with.
fn file_stem(name: &str) -> String {
    let mut stem = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            stem.extend(character.to_lowercase());
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let stem = stem.trim_matches('-').to_string();
    if stem.is_empty() {
        "frame".into()
    } else {
        stem
    }
}

/// Cut `rect` out of a captured window and write it as a PNG.
fn save_cropped(image: Image, rect: (UVec2, UVec2), path: PathBuf) -> Result<PathBuf, String> {
    let whole = image.try_into_dynamic().map_err(|e| e.to_string())?;
    let (origin, size) = crop(rect, UVec2::new(whole.width(), whole.height()));
    if size.x == 0 || size.y == 0 {
        return Err("the frame is not on screen".into());
    }

    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    let mut cut = whole
        .crop_imm(origin.x, origin.y, size.x, size.y)
        .to_rgba8();
    let background = background_bytes();
    for pixel in cut.pixels_mut() {
        // Written rather than kept: the alpha that comes back from the window
        // means brightness, not opacity, so every pixel has to be told what it
        // is.
        pixel[3] = if is_background(pixel.0, background) {
            0
        } else {
            255
        };
    }
    cut.save(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// The frame's clear colour as the bytes a screenshot of it comes back as.
fn background_bytes() -> [u8; 3] {
    let srgb = crate::view::grid::FRAME_BACKGROUND.to_srgba();
    [srgb.red, srgb.green, srgb.blue].map(|channel| (channel * 255.0).round() as u8)
}

/// Whether a pixel is background, within a channel of it.
///
/// A channel of slack rather than an exact match: the colour is written by the
/// GPU and read back through a surface format of its choosing, and a rounding
/// difference of one would otherwise leave the whole background opaque.
fn is_background(pixel: [u8; 4], background: [u8; 3]) -> bool {
    (0..3).all(|channel| pixel[channel].abs_diff(background[channel]) <= 1)
}

/// A viewport clamped to the picture it is being cut from.
///
/// The window can be resized between the shot being asked for and the pixels
/// coming back, so the rectangle is not assumed to fit.
fn crop(rect: (UVec2, UVec2), image: UVec2) -> (UVec2, UVec2) {
    let origin = rect.0.min(image);
    let size = rect.1.min(image - origin);
    (origin, size)
}

/// The button that saves a picture of a frame, and what it takes to take one.
pub struct CapturePlugin;

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameCapture>()
            .add_observer(on_capture_pressed)
            // With the chrome, which is what it hides and restores, and before
            // the overlay reads what happened.
            .add_systems(
                Update,
                (drive_capture, poll_capture_save, show_capture_notice)
                    .chain()
                    .in_set(Stage::Chrome),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_becomes_a_filename() {
        assert_eq!(
            file_stem("Epifluorescence whole slide"),
            "epifluorescence-whole-slide"
        );
        assert_eq!(file_stem("Sections"), "sections");
        // Punctuation collapses rather than repeating, and never trails.
        assert_eq!(file_stem("SEA-AD  mapped / cells!"), "sea-ad-mapped-cells");
    }

    #[test]
    fn a_nameless_dataset_still_gets_a_file() {
        assert_eq!(file_stem(""), "frame");
        assert_eq!(file_stem("///"), "frame");
    }

    #[test]
    fn two_pictures_of_one_dataset_do_not_collide() {
        // Only the stamp separates them, so it has to be in the name.
        let path = picture_path("Sections");
        assert!(path.starts_with(FOLDER));
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("sections-"), "{name}");
        assert!(name.ends_with(".png"));
        assert!(name.len() > "sections-.png".len());
    }

    #[test]
    fn the_colour_a_frame_clears_to_is_what_gets_keyed_out() {
        // Measured from a real capture: the clear colour comes back as these
        // bytes. If the background ever changes, this is what says so.
        assert_eq!(background_bytes(), [10, 10, 15]);
    }

    #[test]
    fn background_goes_transparent_and_everything_else_does_not() {
        let background = background_bytes();
        assert!(is_background([10, 10, 15, 255], background));
        // A channel of rounding slack, and no more.
        assert!(is_background([11, 9, 15, 255], background));
        assert!(!is_background([12, 10, 15, 255], background));
        // A dark point is still a point.
        assert!(!is_background([10, 10, 30, 255], background));
        assert!(!is_background([88, 186, 228, 255], background));
    }

    #[test]
    fn a_viewport_is_cut_to_the_picture_it_came_from() {
        // The window can shrink between asking for a shot and getting it.
        let (origin, size) = crop((UVec2::new(10, 10), UVec2::new(100, 100)), UVec2::splat(64));
        assert_eq!(origin, UVec2::new(10, 10));
        assert_eq!(size, UVec2::new(54, 54));
    }

    #[test]
    fn a_viewport_off_the_picture_cuts_nothing() {
        let (_, size) = crop((UVec2::new(80, 80), UVec2::new(20, 20)), UVec2::splat(64));
        assert_eq!(size, UVec2::ZERO, "nothing to save rather than a panic");
    }

    #[test]
    fn a_viewport_that_fits_is_left_alone() {
        let (origin, size) = crop((UVec2::new(0, 0), UVec2::new(64, 64)), UVec2::splat(64));
        assert_eq!((origin, size), (UVec2::ZERO, UVec2::splat(64)));
    }
}
