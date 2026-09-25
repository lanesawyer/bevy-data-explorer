//! What the user has told the app to remember between sessions: how big they
//! dragged each dock, the theme they picked, and how point clouds open.
//!
//! One JSON file in the platform's config folder, read once at startup and
//! written a moment after the last change, so a drag across the window is one
//! write rather than one a frame. A file that cannot be read is reported and set
//! aside for the defaults, never allowed to stop the app starting.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app::schedule::Stage;
use crate::source::properties::{FilteredPoints, SavedFiltered};

/// How many picked colors are kept to pick again: one row of the picker.
pub const RECENT_COLORS: usize = 12;

/// How long the preferences must sit still before they are written.
const SAVE_AFTER_SECS: f32 = 0.5;

/// Where preferences are kept: the roaming app data folder on Windows, the
/// XDG config folder elsewhere, or beside the app when there is no home to
/// find it in.
pub fn path() -> PathBuf {
    path_in(cfg!(windows), |name| std::env::var_os(name))
}

/// [`path`], with the platform and environment handed in so each can be
/// tested from any of them.
///
/// Windows sets no `HOME`, so without `APPDATA` the file landed in whatever
/// folder the app happened to be started from.
fn path_in(windows: bool, var: impl Fn(&str) -> Option<std::ffi::OsString>) -> PathBuf {
    let set = |name| var(name).filter(|path| !path.is_empty()).map(PathBuf::from);
    let config = if windows {
        set("APPDATA")
    } else {
        set("XDG_CONFIG_HOME").or_else(|| set("HOME").map(|home| home.join(".config")))
    };
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
    /// The bearer token for the BKP Registry, got by signing in, or pasted
    /// by an older version. Kept in plain text, like anything else in this
    /// file.
    pub registry_token: Option<String>,
    /// What renews that token, and whose it is; none for a pasted one.
    pub registry_login: Option<RegistryLogin>,
    /// The data sources turned off, by `Provider::key`. Kept as what is off
    /// rather than what is on, so a source added later starts on.
    pub sources_off: BTreeSet<String>,
    /// Whether the accent is the desktop's rather than Feathers' blue.
    pub system_accent: bool,
    /// The colors last picked for values, newest first, in sRGB.
    pub recent_colors: Vec<[f32; 3]>,
}

/// A sign-in to the BKP Registry: what renews its token, and whose it is.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct RegistryLogin {
    pub refresh_token: Option<String>,
    pub email: Option<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            remember_layout: true,
            docks: BTreeMap::new(),
            theme: None,
            filtered_points: None,
            registry_token: None,
            registry_login: None,
            sources_off: BTreeSet::new(),
            system_accent: true,
            recent_colors: Vec::new(),
        }
    }
}

impl Preferences {
    pub fn filtered_points(&self) -> FilteredPoints {
        self.filtered_points
            .map_or_else(FilteredPoints::default, |saved| saved.restored())
    }

    /// Put `color` first among the recent colors, dropping it from further
    /// down and the oldest past [`RECENT_COLORS`].
    pub fn remember_color(&mut self, color: Color) {
        let srgba = color.to_srgba();
        let saved = [srgba.red, srgba.green, srgba.blue];
        let same = |other: &[f32; 3]| {
            other
                .iter()
                .zip(saved)
                .all(|(a, b)| (a - b).abs() < 0.5 / 255.0)
        };
        self.recent_colors.retain(|other| !same(other));
        self.recent_colors.insert(0, saved);
        self.recent_colors.truncate(RECENT_COLORS);
    }

    /// The recent colors, clamped, since a file edited by hand can say
    /// anything.
    pub fn recent_colors(&self) -> impl Iterator<Item = Color> + '_ {
        self.recent_colors.iter().map(|color| {
            let [r, g, b] = color.map(|channel| channel.clamp(0.0, 1.0));
            Color::srgb(r, g, b)
        })
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
    fn a_color_picked_again_moves_to_the_front_rather_than_repeating() {
        let mut prefs = Preferences::default();
        prefs.remember_color(Color::srgb(1.0, 0.0, 0.0));
        prefs.remember_color(Color::srgb(0.0, 1.0, 0.0));
        prefs.remember_color(Color::srgb(1.0, 0.0, 0.0));
        assert_eq!(prefs.recent_colors, vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    }

    #[test]
    fn only_the_newest_colors_are_kept() {
        let mut prefs = Preferences::default();
        for step in 0..RECENT_COLORS + 3 {
            prefs.remember_color(Color::srgb(step as f32 / 20.0, 0.0, 0.0));
        }
        assert_eq!(prefs.recent_colors.len(), RECENT_COLORS);
        assert!((prefs.recent_colors[0][0] - (RECENT_COLORS + 2) as f32 / 20.0).abs() < 1e-5);
    }

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
    fn windows_keeps_them_in_app_data_rather_than_where_it_was_started() {
        let env = |name: &str| match name {
            "APPDATA" => Some(r"C:\Users\lane\AppData\Roaming".into()),
            "HOME" => Some("/home/lane".into()),
            _ => None,
        };
        assert_eq!(
            path_in(true, env),
            PathBuf::from(r"C:\Users\lane\AppData\Roaming")
                .join("bevy-data-explorer")
                .join("preferences.json")
        );
        assert_eq!(
            path_in(false, env),
            PathBuf::from("/home/lane/.config/bevy-data-explorer/preferences.json")
        );
        assert_eq!(path_in(true, |_| None), PathBuf::from("preferences.json"));
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
