//! Neuroglancer viewer states, opened as the images they describe.
//!
//! A state is not a format of its own: it is a JSON document saying which
//! stores to show and how — each layer's address, whether it is visible, and
//! a shader whose controls give each channel its color and display range. The
//! BKP Registry holds them beside the light-sheet stacks they were written
//! for, as `JSON` assets named `…_neuroglancer_config`.
//!
//! Only image layers whose source is a Zarr store are kept, since that is what
//! the app reads; a state listing none of those says so rather than opening
//! nothing. Every store is opened, and each channel takes the color and range
//! the state gave it. Stores that line up pixel for pixel — a specimen written
//! a channel to a store — become one image whose channels are mixed as one
//! store's are, which is what Neuroglancer draws; its address is the state's,
//! so a bookmark of it names the state. Stores that do not line up are opened
//! as a [`Scene`] instead: a frame each, linked.
//!
//! A state arrives three ways: as a file, as a `neuroglancer…/#!<address>`
//! link naming one, or as a link carrying the whole state after the `#!`.

use bevy::color::Srgba;
use serde_json::Value;

use crate::formats::discover::Discovered;
use crate::formats::image::dataset::{Channel, Dataset};

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

/// How a state shows one channel of a store. Anything it does not say is
/// left as the store publishes it.
#[derive(Debug, Clone, Default, PartialEq)]
struct Look {
    color: Option<[f32; 3]>,
    range: Option<(f32, f32)>,
    shown: Option<bool>,
}

/// One image layer of a state, before its store has been opened.
#[derive(Debug, PartialEq)]
struct Layer {
    url: String,
    shown: bool,
    /// By channel of the store.
    looks: Vec<Look>,
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
                looks: looks_of(
                    layer["shader"].as_str().unwrap_or_default(),
                    &layer["shaderControls"],
                ),
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

/// One `#uicontrol` a shader declares.
#[derive(Debug, PartialEq)]
enum Control {
    Color([f32; 3]),
    /// An `invlerp`: the range intensity is mapped through, and the channel
    /// it reads.
    Range((f32, f32), Option<usize>),
    Checkbox(bool),
}

/// The controls a shader declares, by name, at their defaults.
fn controls_of(shader: &str) -> Vec<(String, Control)> {
    shader
        .lines()
        .filter_map(|line| line.trim().strip_prefix("#uicontrol"))
        .filter_map(|rest| {
            let mut words = rest.split_whitespace();
            let kind = words.next()?;
            let declared = words.collect::<Vec<_>>().join(" ");
            let name: String = declared
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let arguments = declared[name.len()..].trim_start();
            let control = match kind {
                "vec3" => Control::Color(hex(&quoted(arguments)?)?),
                "invlerp" => Control::Range(
                    bracketed(arguments, "range")
                        .and_then(|it| Some((*it.first()?, *it.get(1)?)))
                        .unwrap_or((0.0, 0.0)),
                    bracketed(arguments, "channel").and_then(|it| it.first().map(|c| *c as usize)),
                ),
                "bool" => Control::Checkbox(!arguments.replace(' ', "").contains("default=false")),
                _ => return None,
            };
            Some((name, control))
        })
        .collect()
}

/// The first string in double quotes.
fn quoted(text: &str) -> Option<String> {
    let (_, rest) = text.split_once('"')?;
    Some(rest.split_once('"')?.0.to_string())
}

/// The numbers in `key=[…]`.
fn bracketed(text: &str, key: &str) -> Option<Vec<f32>> {
    let at = text.find(&format!("{key}="))?;
    let rest = text[at..].split_once('[')?.1.split_once(']')?.0;
    rest.split(',')
        .map(|it| it.trim().parse::<f32>().ok())
        .collect()
}

fn hex(text: &str) -> Option<[f32; 3]> {
    let color = Srgba::hex(text.trim()).ok()?;
    Some([color.red, color.green, color.blue])
}

/// The channel a control belongs to by its name, as `channel2_color` does.
fn channel_named(name: &str) -> Option<usize> {
    let digits: String = name
        .strip_prefix("channel")?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// How each channel of a layer is shown, from its shader and whatever the
/// state saved of its controls over their defaults.
///
/// A control is placed on a channel by the channel its `invlerp` reads, or by
/// its name; one naming no channel is the first's, which is a single-channel
/// layer's only one.
fn looks_of(shader: &str, saved: &Value) -> Vec<Look> {
    let mut looks: Vec<Look> = Vec::new();
    for (name, mut control) in controls_of(shader) {
        match (&mut control, saved.get(&name)) {
            (Control::Color(color), Some(Value::String(text))) => {
                if let Some(saved) = hex(text) {
                    *color = saved;
                }
            }
            (Control::Range(range, _), Some(saved)) => {
                let saved = saved.get("range").unwrap_or(saved);
                if let Some([low, high]) = saved.as_array().and_then(|it| {
                    Some([it.first()?.as_f64()? as f32, it.get(1)?.as_f64()? as f32])
                }) {
                    *range = (low, high);
                }
            }
            (Control::Checkbox(shown), Some(Value::Bool(saved))) => *shown = *saved,
            _ => {}
        }
        let channel = match &control {
            Control::Range(_, Some(channel)) => *channel,
            _ => channel_named(&name).unwrap_or(0),
        };
        if looks.len() <= channel {
            looks.resize(channel + 1, Look::default());
        }
        let look = &mut looks[channel];
        match control {
            Control::Color(color) => look.color = Some(color),
            Control::Range(range, _) if range.1 > range.0 => look.range = Some(range),
            Control::Range(..) => {}
            Control::Checkbox(shown) => look.shown = Some(shown),
        }
    }
    looks
}

/// Show `channels` as `looks` and the layer's visibility say.
fn apply(channels: &mut [Channel], looks: &[Look], layer_shown: bool) {
    for (index, channel) in channels.iter_mut().enumerate() {
        let look = looks.get(index).cloned().unwrap_or_default();
        if let Some(color) = look.color {
            channel.color = color;
        }
        if let Some((start, end)) = look.range {
            channel.start = start;
            channel.end = end;
        }
        channel.active = layer_shown && look.shown.unwrap_or(channel.active);
    }
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

/// Read a state and open every store it names.
///
/// Every store is opened together; one that cannot be fails the state, since
/// a frame for it would only fail again.
pub async fn read(source: &str, text: &str) -> Result<Discovered, String> {
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

    let opened = futures::future::join_all(
        layers
            .iter()
            .map(|layer| crate::formats::image::store::open(&layer.url)),
    )
    .await
    .into_iter()
    .collect::<Result<Vec<_>, String>>()?;

    let name = state["title"]
        .as_str()
        .filter(|title| !title.trim().is_empty())
        .map_or_else(
            || crate::formats::discover::label_for(source),
            str::to_string,
        );

    // As the stores publish them, for the scene should they not line up.
    let published: Vec<Vec<Channel>> = opened.iter().map(|it| it.channels.clone()).collect();
    let mut shown = opened;
    for (dataset, layer) in shown.iter_mut().zip(&layers) {
        apply(&mut dataset.channels, &layer.looks, layer.shown);
    }
    let mut stores = shown.into_iter();
    let first = stores
        .next()
        .ok_or_else(|| format!("{source} lists no layers"))?;
    match Dataset::overlay(first, stores.collect()) {
        Ok(mut dataset) => {
            dataset.name = name;
            Ok(Discovered::Image(Box::new(dataset)))
        }
        Err(why) => {
            bevy::log::info!("{name}: {why}, so each store opens in a frame of its own");
            Ok(Discovered::Scene(scene_of(name, &layers, &published)))
        }
    }
}

/// A frame each for stores that do not line up, each channel as bright as the
/// state showed it.
fn scene_of(name: String, layers: &[Layer], published: &[Vec<Channel>]) -> Scene {
    let layers = layers
        .iter()
        .zip(published)
        .map(|(layer, channels)| SceneLayer {
            url: layer.url.clone(),
            channels: channels
                .iter()
                .enumerate()
                .map(|(index, channel)| {
                    let look = layer.looks.get(index).cloned().unwrap_or_default();
                    SceneChannel {
                        label: channel.label.clone(),
                        shown: layer.shown && look.shown.unwrap_or(channel.active),
                        gain: look.range.map_or(1.0, |(_, high)| {
                            gain_for((channel.start, channel.end), high)
                        }),
                    }
                })
                .collect(),
        })
        .collect();
    Scene { name, layers }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> Value {
        serde_json::from_str(include_str!("../../testdata/neuroglancer_smartspim.json")).unwrap()
    }

    fn mip() -> Value {
        serde_json::from_str(include_str!(
            "../../testdata/neuroglancer_smartspim_mip.json"
        ))
        .unwrap()
    }

    #[test]
    fn a_registry_config_is_a_state_and_scatterbrain_is_not() {
        assert!(is_state(&raw()));
        let points: Value =
            serde_json::from_str(include_str!("../../testdata/scatterbrain.json")).unwrap();
        assert!(!is_state(&points));
    }

    #[test]
    fn each_channel_store_is_a_layer_in_its_color_and_range() {
        let (layers, skipped) = layers_of(&raw());
        assert_eq!(skipped, 0);
        assert_eq!(layers.len(), 3);
        assert_eq!(
            layers[0].url,
            "https://aind-open-data.s3.us-west-2.amazonaws.com/SmartSPIM_719692_2024-03-13_16-03-36_stitched_2024-04-02_12-50-17/image_tile_fusing/OMEZarr/Ex_639_Em_680.zarr"
        );
        assert!(layers[0].shown);
        let look = &layers[0].looks[0];
        assert_eq!(look.range, Some((0.0, 200.0)));
        let [r, g, b] = look.color.unwrap();
        assert!((r - 240.0 / 255.0).abs() < 1e-3 && g < 1e-3 && (b - 80.0 / 255.0).abs() < 1e-3);
    }

    #[test]
    fn a_projection_gives_each_of_its_channels_a_color_and_range() {
        let (layers, _) = layers_of(&mip());
        assert_eq!(layers.len(), 1);
        assert!(layers[0].url.ends_with("OMEZarr/sagittal_MIP.zarr"));
        let looks = &layers[0].looks;
        assert_eq!(looks.len(), 3);
        assert_eq!(looks[0].color, Some([1.0, 0.0, 0.0]));
        assert_eq!(looks[1].range, Some((0.0, 721.0)));
        assert_eq!(looks[2].color, Some([0.0, 0.0, 1.0]));
        assert_eq!(looks[2].range, Some((0.0, 4095.0)));
        assert_eq!(looks[2].shown, Some(true));
    }

    #[test]
    fn a_saved_control_overrides_its_default() {
        let looks = looks_of(
            "#uicontrol vec3 color color(default=\"#ff0000\")\n\
             #uicontrol invlerp normalized",
            &serde_json::json!({ "color": "#00ff00", "normalized": { "range": [5, 50] } }),
        );
        assert_eq!(looks[0].color, Some([0.0, 1.0, 0.0]));
        assert_eq!(looks[0].range, Some((5.0, 50.0)));
    }

    #[test]
    fn a_state_shows_its_channels_as_it_says() {
        let mut channels = vec![Channel {
            label: "a".into(),
            color: [1.0; 3],
            start: 0.0,
            end: 350.0,
            active: true,
        }];
        let look = Look {
            color: Some([0.0, 1.0, 0.0]),
            range: Some((10.0, 200.0)),
            shown: None,
        };
        apply(&mut channels, &[look], false);
        assert_eq!(channels[0].color, [0.0, 1.0, 0.0]);
        assert_eq!((channels[0].start, channels[0].end), (10.0, 200.0));
        assert!(!channels[0].active, "a hidden layer's channels are hidden");
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
        assert!(layers[0].looks.is_empty());
    }

    #[test]
    #[ignore = "reads a live state from the BKP Registry's light-sheet stacks"]
    fn a_registry_state_opens_as_one_image_of_its_channels() {
        const STATE: &str = "s3://aind-open-data/SmartSPIM_719692_2024-03-13_16-03-36_stitched_2024-04-02_12-50-17/neuroglancer_config.json";
        let found = crate::app::net::block_on(crate::formats::discover::discover(STATE)).unwrap();
        let Discovered::Image(dataset) = found else {
            panic!("three stores on one grid are one image");
        };
        assert_eq!(dataset.channels.len(), 3);
        assert_eq!(dataset.members.len(), 2);
        assert_eq!(dataset.channels[0].label, "Ex_639_Em_680.zarr");
        assert_eq!(dataset.channels[0].end, 200.0);

        // And a tile of it holds all three.
        let level = dataset.levels.last().unwrap();
        let tile = crate::app::net::block_on(crate::formats::image::dataset::read_tile(
            &dataset,
            level,
            crate::formats::image::dataset::TileSource::Array,
            0,
            0,
            dataset.depth() / 2,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(tile.layers, 1, "three channels fit one layer of four");
        let lit = |channel: usize| {
            tile.data
                .chunks(8)
                .any(|texel| texel[channel * 2..channel * 2 + 2] != [0, 0])
        };
        assert!(
            lit(0) && lit(1) && lit(2),
            "every store's channel has tissue"
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
