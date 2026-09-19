//! Bookmarks kept on disk, and handed to and taken from other people as files.
//!
//! Each saved bookmark is one JSON file in the user's data folder, so the list
//! survives restarts and a bookmark can be found, copied or deleted outside the
//! app. Exporting writes the same JSON wherever the user chooses; importing
//! reads either form `codec` accepts.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

use super::codec::{from_text, to_json};
use super::snapshot::Bookmark;
use crate::app::net::{Fetching, fetching};

/// Where bookmarks are kept: the XDG data folder, as a desktop app's data
/// belongs, or beside the app when there is no home to find it in.
pub fn folder() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    match data {
        Some(data) => data.join("bevy-data-explorer").join("bookmarks"),
        None => PathBuf::from("bookmarks"),
    }
}

/// A bookmark read from disk, and the file it came from.
#[derive(Clone, Debug)]
pub struct Saved {
    pub path: PathBuf,
    pub bookmark: Bookmark,
}

/// The saved bookmarks, newest first, read again whenever one is added or
/// removed.
#[derive(Resource)]
pub struct SavedBookmarks {
    folder: PathBuf,
    pub list: Vec<Saved>,
}

impl Default for SavedBookmarks {
    fn default() -> Self {
        let mut saved = SavedBookmarks {
            folder: folder(),
            list: Vec::new(),
        };
        saved.refresh();
        saved
    }
}

impl SavedBookmarks {
    pub fn refresh(&mut self) {
        self.list = list(&self.folder);
    }

    pub fn save(&mut self, bookmark: &Bookmark) -> Result<PathBuf, String> {
        let path = save(&self.folder, bookmark)?;
        self.refresh();
        Ok(path)
    }

    pub fn delete(&mut self, path: &Path) -> Result<(), String> {
        std::fs::remove_file(path).map_err(|e| format!("deleting {}: {e}", path.display()))?;
        self.refresh();
        Ok(())
    }
}

/// Every readable bookmark in `folder`, newest first. One that cannot be read
/// is reported and left where it is rather than hiding the rest.
fn list(folder: &Path) -> Vec<Saved> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut saved: Vec<Saved> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| {
            let read = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| from_text(&text));
            match read {
                Ok(bookmark) => Some(Saved { path, bookmark }),
                Err(error) => {
                    warn!("skipping bookmark {}: {error}", path.display());
                    None
                }
            }
        })
        .collect();
    saved.sort_by(|a, b| {
        b.bookmark
            .created
            .cmp(&a.bookmark.created)
            .then_with(|| a.bookmark.name.cmp(&b.bookmark.name))
    });
    saved
}

fn save(folder: &Path, bookmark: &Bookmark) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("creating {}: {e}", folder.display()))?;
    let path = folder.join(format!(
        "{}-{}.json",
        file_stem(&bookmark.name),
        bookmark.created
    ));
    std::fs::write(&path, to_json(bookmark))
        .map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

/// A name made safe to be a file's, keeping it recognisable.
pub fn file_stem(name: &str) -> String {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if stem.is_empty() {
        "bookmark".into()
    } else {
        stem
    }
}

/// What a file dialog came back with.
pub enum DialogOutcome {
    /// Written to this path; nothing if the dialog was cancelled.
    Exported(Result<Option<PathBuf>, String>),
    Imported(Result<Option<Bookmark>, String>),
}

/// The file dialog open, if any. One at a time: a second would open over the
/// first with no way to tell which answer was which.
#[derive(Resource, Default)]
pub struct FileDialog {
    task: Option<Fetching<DialogOutcome>>,
}

impl FileDialog {
    pub fn is_open(&self) -> bool {
        self.task.is_some()
    }

    /// Ask where to write `bookmark`, and write it there.
    pub fn export(&mut self, bookmark: Bookmark) {
        if self.is_open() {
            return;
        }
        self.task = Some(fetching(async move {
            // The dialog blocks until answered, so it gets a thread of its own
            // rather than one of the runtime's.
            let outcome = tokio::task::spawn_blocking(move || {
                let Some(path) = rfd::FileDialog::new()
                    .set_title("Export bookmark")
                    .add_filter("Bookmark", &["json"])
                    .set_file_name(format!("{}.json", file_stem(&bookmark.name)))
                    .save_file()
                else {
                    return Ok(None);
                };
                std::fs::write(&path, to_json(&bookmark))
                    .map(|()| Some(path.clone()))
                    .map_err(|e| format!("writing {}: {e}", path.display()))
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            DialogOutcome::Exported(outcome)
        }));
    }

    /// Ask for a bookmark file, and read it.
    pub fn import(&mut self) {
        if self.is_open() {
            return;
        }
        self.task = Some(fetching(async {
            let outcome = tokio::task::spawn_blocking(|| {
                let Some(path) = rfd::FileDialog::new()
                    .set_title("Import bookmark")
                    .add_filter("Bookmark", &["json"])
                    .pick_file()
                else {
                    return Ok(None);
                };
                std::fs::read_to_string(&path)
                    .map_err(|e| format!("reading {}: {e}", path.display()))
                    .and_then(|text| from_text(&text))
                    .map(Some)
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            DialogOutcome::Imported(outcome)
        }));
    }

    pub fn take(&mut self) -> Option<DialogOutcome> {
        let outcome = self.task.as_mut()?.take()?;
        self.task = None;
        Some(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookmark::snapshot::VERSION;

    fn bookmark(name: &str, created: u64) -> Bookmark {
        Bookmark {
            version: VERSION,
            name: name.into(),
            created,
            sources: Vec::new(),
            frames: Vec::new(),
            selected: None,
        }
    }

    #[test]
    fn a_name_becomes_a_file_name_that_still_reads_as_it() {
        assert_eq!(file_stem("Cortex, layer 5/6"), "cortex-layer-5-6");
        assert_eq!(file_stem("  ??  "), "bookmark");
        assert_eq!(file_stem("MERFISH_v2"), "merfish_v2");
    }

    #[test]
    fn saved_bookmarks_list_newest_first_and_can_be_deleted() {
        let folder = std::env::temp_dir().join(format!(
            "bde-bookmarks-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut saved = SavedBookmarks {
            folder: folder.clone(),
            list: Vec::new(),
        };
        saved.save(&bookmark("older", 10)).unwrap();
        let newer = saved.save(&bookmark("newer", 20)).unwrap();
        std::fs::write(folder.join("broken.json"), "{ not a bookmark").unwrap();
        saved.refresh();

        let names: Vec<&str> = saved
            .list
            .iter()
            .map(|s| s.bookmark.name.as_str())
            .collect();
        assert_eq!(names, ["newer", "older"]);

        saved.delete(&newer).unwrap();
        assert_eq!(saved.list.len(), 1);
        std::fs::remove_dir_all(folder).unwrap();
    }
}
