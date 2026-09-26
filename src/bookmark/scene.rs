//! A scene from another viewer, opened as a bookmark.
//!
//! A Neuroglancer state is what a bookmark is — datasets by address and how
//! each is shown — written by a different program. So it becomes one and is
//! restored like any other, replacing what is on screen: a frame for each
//! store, fitted to its data, with each channel as bright as the state showed
//! it. The frames are linked, because they are one specimen seen channel by
//! channel and are meant to be looked at in the same place.

use bevy::prelude::*;

use super::snapshot::{Bookmark, ChannelState, FrameState, SourceState, VERSION};
use crate::formats::neuroglancer::Scene;
use crate::view::grid::MAX_PANELS;

/// The bookmark that shows `scene`.
pub fn bookmark_of(scene: &Scene) -> Bookmark {
    if scene.layers.len() > MAX_PANELS {
        warn!(
            "{} has {} layers; the grid shows the first {MAX_PANELS}",
            scene.name,
            scene.layers.len()
        );
    }
    let layers = &scene.layers[..scene.layers.len().min(MAX_PANELS)];
    let linked = layers.len() > 1;
    Bookmark {
        version: VERSION,
        name: scene.name.clone(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
        sources: layers
            .iter()
            .map(|layer| SourceState {
                url: layer.url.clone(),
                channels: layer
                    .channels
                    .iter()
                    .map(|channel| ChannelState {
                        label: channel.label.clone(),
                        shown: channel.shown,
                        gain: channel.gain,
                        color: None,
                    })
                    .collect(),
                ..Default::default()
            })
            .collect(),
        frames: (0..layers.len())
            .map(|source| FrameState {
                source,
                view: None,
                orbit: None,
                layers: Vec::new(),
                selection: None,
                linked,
            })
            .collect(),
        selected: (!layers.is_empty()).then_some(0),
        empty_frames: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::neuroglancer::{SceneChannel, SceneLayer};

    fn layer(n: usize) -> SceneLayer {
        SceneLayer {
            url: format!("https://host/{n}.zarr"),
            channels: vec![SceneChannel {
                label: format!("{n}.zarr"),
                shown: true,
                gain: 1.75,
            }],
        }
    }

    #[test]
    fn each_store_is_a_linked_frame_fitted_to_its_data() {
        let scene = Scene {
            name: "SmartSPIM".into(),
            layers: (0..3).map(layer).collect(),
        };
        let bookmark = bookmark_of(&scene);
        assert_eq!(bookmark.sources.len(), 3);
        assert_eq!(bookmark.sources[1].url, "https://host/1.zarr");
        assert_eq!(bookmark.sources[1].channels[0].gain, 1.75);
        assert!(bookmark.frames.iter().all(|frame| frame.linked));
        assert!(bookmark.frames.iter().all(|frame| frame.view.is_none()));
    }

    #[test]
    fn a_scene_of_one_store_is_not_linked_to_anything() {
        let scene = Scene {
            name: "one".into(),
            layers: vec![layer(0)],
        };
        assert!(!bookmark_of(&scene).frames[0].linked);
    }

    #[test]
    fn a_scene_larger_than_the_grid_keeps_what_fits() {
        let scene = Scene {
            name: "many".into(),
            layers: (0..MAX_PANELS + 3).map(layer).collect(),
        };
        assert_eq!(bookmark_of(&scene).frames.len(), MAX_PANELS);
    }
}
