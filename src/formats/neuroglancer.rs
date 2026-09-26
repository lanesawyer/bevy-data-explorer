//! Neuroglancer viewer states, opened as the datasets they point at.
//!
//! A state is not a format of its own: it is a JSON document saying which
//! stores to show and how — each layer's address, whether it is visible, and
//! a shader whose controls set its display range. The BKP Registry holds them
//! beside the light-sheet stacks they were written for, as `JSON` assets named
//! `…_neuroglancer_config`. So reading one means reading the layers it names,
//! and what it produces is a [`Scene`]: several datasets to open together, as
//! a bookmark from another viewer would be.
//!
//! Only image layers whose source is a Zarr store are kept, since that is what
//! the app reads; a state listing none of those says so rather than opening
//! nothing. Each layer is a store of its own, so each gets a frame of its own,
//! and the frames are linked so they pan and page together. Drawing them over
//! one another, as Neuroglancer adds them, is left for when one source can
//! take its channels from several stores.
//!
//! A state arrives three ways: as a file, as a `neuroglancer…/#!<address>`
//! link naming one, or as a link carrying the whole state after the `#!`.

use serde_json::Value;

use crate::formats::image::store::published_channels;

/// Several datasets a state asked to be shown together.
#[derive(Debug, Clone)]
pub struct Scene {
    /// The state's own title, or its file's name.
    pub name: String,
    pub layers: Vec<SceneLayer>,
}

/// One store a scene opens, and how its channels are shown.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneLayer {
    /// The store's address, as plainly as it can be fetched.
    pub url: String,
    pub channels: Vec<SceneChannel>,
}

/// A channel of a scene's store, by the label the store gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneChannel {
    pub label: String,
    pub shown: bool,
    /// The state's display range as a gain over the store's own window, so
    /// the channel is as bright here as it is in Neuroglancer.
    pub gain: f32,
}

/// Prefixes on a source naming a Zarr store, which is all that is read.
const ZARR: [&str; 3] = ["zarr://", "zarr2://", "zarr3://"];

/// The state inside a Neuroglancer link, if `source` is one: the address of
/// the state, or the state itself when the link carries it whole.
pub fn in_link(source: &str) -> Option<String> {
    let (_, state) = source.split_once("#!")?;
    let state = percent_decode(state);
    (!state.trim().is_empty()).then_some(state)
}

/// Undo a link's percent-encoding, which a state carried whole goes through.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(byte) = text
                .get(at + 1..at + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a JSON document is a Neuroglancer state: it lays out dimensions
/// and lists layers.
pub fn is_state(value: &Value) -> bool {
    value.get("layers").is_some_and(Value::is_array)
        && value.get("dimensions").is_some_and(Value::is_object)
}

/// One image layer of a state, before its store has been asked anything.
#[derive(Debug, PartialEq)]
struct Layer {
    url: String,
    shown: bool,
    /// The display range its shader was left at, if it has one.
    range: Option<(f32, f32)>,
}

/// The image layers of a state the app can read, and how many it could not.
fn layers_of(state: &Value) -> (Vec<Layer>, usize) {
    let mut kept = Vec::new();
    let mut skipped = 0;
    for layer in state["layers"].as_array().into_iter().flatten() {
        let url = source_of(&layer["source"]);
        let zarr = url.as_deref().and_then(|url| {
            ZARR.iter()
                .find_map(|prefix| url.strip_prefix(prefix))
                .or_else(|| (!url.contains("://") || url.starts_with("http")).then_some(url))
        });
        match (layer["type"].as_str(), zarr) {
            (Some("image"), Some(url)) => kept.push(Layer {
                url: crate::formats::plain_url(url),
                shown: layer["visible"].as_bool().unwrap_or(true),
                range: range_of(&layer["shaderControls"]),
            }),
            _ => skipped += 1,
        }
    }
    (kept, skipped)
}

/// A layer's source: a string, an object carrying a `url`, or a list of
/// either, of which the first is the data.
fn source_of(source: &Value) -> Option<String> {
    match source {
        Value::String(url) => Some(url.clone()),
        Value::Object(object) => object.get("url")?.as_str().map(str::to_string),
        Value::Array(sources) => sources.first().and_then(source_of),
        _ => None,
    }
}

/// The range an `invlerp` control was left at, the one that maps intensity
/// to brightness. Only a control naming no channel, which is a single-channel
/// layer's; one per channel belongs to a layout not read yet.
fn range_of(controls: &Value) -> Option<(f32, f32)> {
    controls.as_object()?.values().find_map(|control| {
        if control.get("channel").is_some() {
            return None;
        }
        let range = control.get("range")?.as_array()?;
        let low = range.first()?.as_f64()? as f32;
        let high = range.get(1)?.as_f64()? as f32;
        (high > low).then_some((low, high))
    })
}

/// The gain that brings a channel published over `start..end` to full
/// brightness at `high`, the top of the range a state showed it through.
fn gain_for((start, end): (f32, f32), high: f32) -> f32 {
    if high > start && end > start {
        (end - start) / (high - start)
    } else {
        1.0
    }
}

/// Read a state and the channels of every store it names.
///
/// Each store's root attributes are read for its channels' labels and
/// published windows, together; a store that cannot be read fails the scene,
/// since a frame for it would only fail again once opened.
pub async fn read(source: &str, text: &str) -> Result<Scene, String> {
    let state: Value =
        serde_json::from_str(text).map_err(|e| format!("parsing {source} as a state: {e}"))?;
    if !is_state(&state) {
        return Err(format!("{source} is not a Neuroglancer state"));
    }
    let (layers, skipped) = layers_of(&state);
    if layers.is_empty() {
        return Err(format!(
            "{source} lists no image layers read from a Zarr store, which is all \
             this viewer reads of a Neuroglancer state"
        ));
    }
    if skipped > 0 {
        bevy::log::warn!("{source}: {skipped} layer(s) are not Zarr images and are left out");
    }

    let published =
        futures::future::join_all(layers.iter().map(|layer| published_channels(&layer.url))).await;
    let layers = layers
        .into_iter()
        .zip(published)
        .map(|(layer, published)| {
            let channels = published?
                .into_iter()
                .map(|(label, start, end)| SceneChannel {
                    label,
                    shown: layer.shown,
                    gain: layer
                        .range
                        .map_or(1.0, |(_, high)| gain_for((start, end), high)),
                })
                .collect();
            Ok(SceneLayer {
                url: layer.url,
                channels,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let name = state["title"]
        .as_str()
        .filter(|title| !title.trim().is_empty())
        .map_or_else(
            || crate::formats::discover::label_for(source),
            str::to_string,
        );
    Ok(Scene { name, layers })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> Value {
        serde_json::from_str(include_str!("../../testdata/neuroglancer_smartspim.json")).unwrap()
    }

    #[test]
    fn a_registry_config_is_a_state_and_scatterbrain_is_not() {
        assert!(is_state(&raw()));
        let points: Value =
            serde_json::from_str(include_str!("../../testdata/scatterbrain.json")).unwrap();
        assert!(!is_state(&points));
    }

    #[test]
    fn each_channel_store_is_a_layer_with_its_range() {
        let (layers, skipped) = layers_of(&raw());
        assert_eq!(skipped, 0);
        assert_eq!(layers.len(), 3);
        assert_eq!(
            layers[0].url,
            "https://aind-open-data.s3.us-west-2.amazonaws.com/SmartSPIM_719692_2024-03-13_16-03-36_stitched_2024-04-02_12-50-17/image_tile_fusing/OMEZarr/Ex_639_Em_680.zarr"
        );
        assert!(layers[0].shown);
        assert_eq!(layers[0].range, Some((0.0, 200.0)));
    }

    #[test]
    fn a_projection_keeps_its_published_windows_for_now() {
        // One store of three channels, each with an invlerp control naming its
        // channel — a layout whose ranges are not read yet.
        let mip: Value = serde_json::from_str(include_str!(
            "../../testdata/neuroglancer_smartspim_mip.json"
        ))
        .unwrap();
        assert!(is_state(&mip));
        let (layers, skipped) = layers_of(&mip);
        assert_eq!((layers.len(), skipped), (1, 0));
        assert!(layers[0].url.ends_with("OMEZarr/sagittal_MIP.zarr"));
        assert_eq!(layers[0].range, None);
    }

    #[test]
    fn a_range_narrower_than_the_published_window_is_a_gain_above_one() {
        // The store publishes 0..350; the state shows it through 0..200.
        assert!((gain_for((0.0, 350.0), 200.0) - 1.75).abs() < 1e-6);
        assert_eq!(gain_for((0.0, 350.0), 0.0), 1.0);
    }

    #[test]
    fn layers_it_cannot_read_are_counted_and_left_out() {
        let state: Value = serde_json::json!({
            "dimensions": {},
            "layers": [
                {"type": "image", "source": "precomputed://gs://bucket/em"},
                {"type": "segmentation", "source": "zarr://s3://bucket/seg.zarr"},
                {"type": "image", "source": [{"url": "zarr2://https://host/a.zarr"}]},
            ]
        });
        let (layers, skipped) = layers_of(&state);
        assert_eq!(skipped, 2);
        assert_eq!(layers[0].url, "https://host/a.zarr");
        assert_eq!(layers[0].range, None);
    }

    #[test]
    #[ignore = "reads a live state from the BKP Registry's light-sheet stacks"]
    fn a_registry_state_opens_as_its_channels_at_their_brightness() {
        const STATE: &str = "s3://aind-open-data/SmartSPIM_719692_2024-03-13_16-03-36_stitched_2024-04-02_12-50-17/neuroglancer_config.json";
        let found = crate::app::net::block_on(crate::formats::discover::discover(STATE)).unwrap();
        let crate::formats::discover::Discovered::Scene(scene) = found else {
            panic!("a state is a scene");
        };
        assert_eq!(scene.layers.len(), 3);
        let first = &scene.layers[0].channels[0];
        assert_eq!(first.label, "Ex_639_Em_680.zarr");
        // Published 0..350, shown through 0..200.
        assert!((first.gain - 1.75).abs() < 1e-3, "{}", first.gain);

        // And what it names opens as the image it is.
        let store =
            crate::app::net::block_on(crate::formats::discover::discover(&scene.layers[0].url));
        assert!(
            matches!(store, Ok(crate::formats::discover::Discovered::Image(_))),
            "{:?}",
            store.err()
        );
    }

    #[test]
    fn a_state_named_only_as_a_state_is_named_for_its_folder() {
        assert_eq!(
            crate::formats::discover::label_for(
                "https://bucket.s3.amazonaws.com/SmartSPIM_719692/neuroglancer_config.json"
            ),
            "SmartSPIM_719692"
        );
    }

    #[test]
    fn a_link_names_its_state_or_carries_it() {
        assert_eq!(
            in_link("https://neuroglancer-demo.appspot.com/#!s3://bucket/state.json").as_deref(),
            Some("s3://bucket/state.json")
        );
        assert_eq!(
            in_link("https://ng.example/#!%7B%22layers%22%3A%5B%5D%7D").as_deref(),
            Some(r#"{"layers":[]}"#)
        );
        assert_eq!(in_link("https://host/a.zarr"), None);
    }
}
