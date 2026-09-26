//! Bookmarks: what is on screen, saved to come back to or to send to someone.
//!
//! A bookmark is not a dump of the world. Frames point at source entities,
//! sources hold streamers, render layers are handed out at registration, and
//! none of that means anything in another process. So a bookmark records what
//! the user set up, named the way it would be named to someone else — datasets
//! by address, channels by label, cell filters by column and code — and
//! restoring replays it through the same paths opening a dataset by hand
//! takes. See [`snapshot`] for what is kept and [`restore`] for the replay.
//!
//! App preferences are left out on purpose: a bookmark someone sends should
//! not change the recipient's theme or the width of their sidebar.

pub mod capture;
pub mod codec;
pub mod restore;
pub mod scene;
pub mod snapshot;
pub mod store;

use bevy::prelude::*;

use crate::app::schedule::Stage;
use restore::{Restoring, apply_pending_settings, drive_restore};
use snapshot::Bookmark;
use store::{DialogOutcome, FileDialog, SavedBookmarks};

/// How the last thing done with a bookmark went, for the sidebar to say.
#[derive(Resource, Default, Clone, PartialEq, Eq, Debug)]
pub enum BookmarkNotice {
    #[default]
    Idle,
    Working(String),
    Done(String),
    Failed(String),
}

/// Put `bookmark` on screen in place of what is there now.
pub fn restore(commands: &mut Commands, bookmark: Bookmark) {
    commands.insert_resource(Restoring::new(bookmark));
}

/// Open a bookmark someone sent, keeping it in the saved list unless it is
/// already there.
pub fn open_shared(commands: &mut Commands, bookmark: Bookmark) {
    let kept = bookmark.clone();
    commands.queue(move |world: &mut World| {
        let mut saved = world.resource_mut::<SavedBookmarks>();
        let known = saved.list.iter().any(|entry| {
            entry.bookmark.name == kept.name && entry.bookmark.created == kept.created
        });
        if !known && let Err(error) = saved.save(&kept) {
            warn!("could not keep bookmark {}: {error}", kept.name);
        }
    });
    restore(commands, bookmark);
}

/// Save what is on screen as `name`, into the saved list.
pub fn save_current(commands: &mut Commands, name: String) {
    commands.queue(move |world: &mut World| {
        let notice = match capture::capture(world, name) {
            Ok(bookmark) => {
                let local = local_addresses(&bookmark);
                match world.resource_mut::<SavedBookmarks>().save(&bookmark) {
                    Ok(path) => {
                        info!("saved bookmark {} to {}", bookmark.name, path.display());
                        if local.is_empty() {
                            BookmarkNotice::Done(format!("saved {}", bookmark.name))
                        } else {
                            BookmarkNotice::Done(format!(
                                "saved {}; it names files on this machine, so will not open elsewhere",
                                bookmark.name
                            ))
                        }
                    }
                    Err(error) => BookmarkNotice::Failed(error),
                }
            }
            Err(error) => BookmarkNotice::Failed(error),
        };
        if let BookmarkNotice::Failed(error) = &notice {
            warn!("could not save a bookmark: {error}");
        }
        world.insert_resource(notice);
    });
}

/// The addresses in `bookmark` that only open on this machine.
pub fn local_addresses(bookmark: &Bookmark) -> Vec<&str> {
    bookmark
        .sources
        .iter()
        .map(|source| source.url.as_str())
        .filter(|url| !capture::is_remote(url))
        .collect()
}

/// Take in what a file dialog answered.
fn poll_file_dialog(
    mut commands: Commands,
    mut dialog: ResMut<FileDialog>,
    mut notice: ResMut<BookmarkNotice>,
) {
    let Some(outcome) = dialog.take() else {
        return;
    };
    *notice = match outcome {
        DialogOutcome::Exported(Ok(Some(path))) => {
            info!("exported a bookmark to {}", path.display());
            BookmarkNotice::Done(format!("exported to {}", path.display()))
        }
        DialogOutcome::Imported(Ok(Some(bookmark))) => {
            open_shared(&mut commands, bookmark);
            return;
        }
        DialogOutcome::Exported(Ok(None)) | DialogOutcome::Imported(Ok(None)) => {
            BookmarkNotice::Idle
        }
        DialogOutcome::Exported(Err(error)) | DialogOutcome::Imported(Err(error)) => {
            warn!("{error}");
            BookmarkNotice::Failed(error)
        }
    };
}

/// Saving, restoring and sharing bookmarks.
pub struct BookmarkPlugin;

impl Plugin for BookmarkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BookmarkNotice>()
            .init_resource::<SavedBookmarks>()
            .init_resource::<FileDialog>()
            .add_systems(Update, drive_restore.in_set(Stage::Restore))
            .add_systems(
                Update,
                (poll_file_dialog, apply_pending_settings).in_set(Stage::ControlsApply),
            );
    }
}
