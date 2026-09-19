//! What the user has told the app to remember between sessions: how big they
//! dragged each dock, the theme they picked, and how point clouds open.
//!
//! One JSON file in the XDG config folder, read once at startup and written a
//! moment after the last change, so a drag across the window is one write
//! rather than one a frame. A file that cannot be read is reported and set
//! aside for the defaults, never allowed to stop the app starting.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app::schedule::Stage;
use crate::source::properties::{FilteredPoints, SavedFiltered};

/// How long the preferences must sit still before they are written.
const SAVE_AFTER_SECS: f32 = 0.5;

/// Where preferences are kept: the XDG config folder, or beside the app when
/// there is no home to find it in.
pub fn path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    match config {
        Some(config) => config.join("bevy-data-explorer").join("preferences.json"),
        None => PathBuf::from("preferences.json"),
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    Dark,
    Light,
}

/// Every field defaults on its own, so a file from an older version, or one
/// edited by hand, keeps whatever it does say.
#[derive(Resource, Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Preferences {
    /// Whether dock sizes are kept at all. Off, the docks open at their
    /// defaults every time.
    pub remember_layout: bool,
    /// Each dock's size, keyed by `Dock::KEY`.
    pub docks: BTreeMap<String, f32>,
    /// The theme picked by hand; none while it follows the desktop.
    pub theme: Option<ThemeChoice>,
    /// How a point cloud opens drawing the points its filters leave out;
    /// none while it follows the built-in default, so a change to that
    /// reaches anyone who never changed theirs.
    pub filtered_points: Option<SavedFiltered>,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            remember_layout: true,
            docks: BTreeMap::new(),
            theme: None,
            filtered_points: None,
        }
    }
}

impl Preferences {
    pub fn filtered_points(&self) -> FilteredPoints {
        self.filtered_points
            .map_or_else(FilteredPoints::default, |saved| saved.restored())
    }

    pub fn load(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                warn!("reading preferences {}: {e}", path.display());
                return Self::default();
            }
        };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            warn!("ignoring preferences {}: {e}", path.display());
            Self::default()
        })
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(folder) = path
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty())
        {
            std::fs::create_dir_all(folder)
                .map_err(|e| format!("creating {}: {e}", folder.display()))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("writing {}: {e}", path.display()))
    }
}

/// Where the preferences were read from, and are written back to.
#[derive(Resource)]
pub struct PreferencesFile(pub PathBuf);

/// Write the preferences once they have stopped changing.
fn save_preferences(
    prefs: Res<Preferences>,
    file: Res<PreferencesFile>,
    time: Res<Time>,
    mut changed_at: Local<Option<f32>>,
) {
    let now = time.elapsed_secs();
    if prefs.is_changed() && !prefs.is_added() {
        *changed_at = Some(now);
    }
    let Some(at) = *changed_at else { return };
    if now - at < SAVE_AFTER_SECS {
        return;
    }
    *changed_at = None;
    if let Err(error) = prefs.save(&file.0) {
        warn!("could not save preferences: {error}");
    }
}

/// Read the preferences. Added before anything that starts from them, the
/// theme among them.
pub struct PreferencesPlugin;

impl Plugin for PreferencesPlugin {
    fn build(&self, app: &mut App) {
        let file = path();
        app.insert_resource(Preferences::load(&file))
            .insert_resource(PreferencesFile(file))
            .add_systems(Update, save_preferences.in_set(Stage::Overlay));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_survive_a_round_trip_and_a_missing_file() {
        let path = std::env::temp_dir().join(format!(
            "bde-prefs-test-{}/preferences.json",
            std::process::id()
        ));
        assert_eq!(Preferences::load(&path), Preferences::default());

        let mut prefs = Preferences::default();
        prefs.docks.insert("sidebar".into(), 412.0);
        prefs.theme = Some(ThemeChoice::Light);
        prefs.save(&path).unwrap();
        assert_eq!(Preferences::load(&path), prefs);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_partial_or_broken_file_falls_back_field_by_field() {
        let partial: Preferences = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert!(partial.remember_layout);
        assert_eq!(partial.theme, Some(ThemeChoice::Dark));
        assert!(serde_json::from_str::<Preferences>("{ nope").is_err());
    }

    #[test]
    fn point_clouds_follow_the_built_in_default_until_changed() {
        let fresh: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(fresh.filtered_points, None);
        assert_eq!(fresh.filtered_points(), FilteredPoints::default());

        let changed: Preferences =
            serde_json::from_str(r#"{"filtered_points":{"shown":false,"color":[0.5,0.5,0.5]}}"#)
                .unwrap();
        assert!(!changed.filtered_points().shown);
    }
}
