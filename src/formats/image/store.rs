//! Resolving a user-supplied source into an open dataset.
//!
//! A source may be a Zarr store (HTTP URL or local directory) or a manifest
//! JSON of the kind produced alongside these conversions, which carries the
//! store URL plus a copy of the root attributes.

use std::sync::Arc;

use ome_zarr_metadata::v0_4::{Axis, MultiscaleImageDataset, Omero};
use serde::Deserialize;

use zarrs_object_store::AsyncObjectStore;

use crate::formats::image::dataset::{Dataset, ReadStore};

/// A multiscale image normalized across OME-Zarr versions. The 0.4 and 0.5
/// types are separate wrappers around identical axis and dataset types.
#[derive(Debug, Clone)]
pub struct MultiscaleSpec {
    pub name: Option<String>,
    pub axes: Vec<Axis>,
    pub datasets: Vec<MultiscaleImageDataset>,
}

/// A manifest describing where a store lives and what is in it.
#[derive(Debug, Deserialize)]
struct Manifest {
    url: String,
    #[serde(default)]
    attrs: Option<serde_json::Value>,
}

/// Open whatever `source` points at.
pub async fn open(source: &str) -> Result<Dataset, String> {
    // Addresses arrive as they were copied. The command line reaches here
    // without going through `discover`, so this is where every path that opens
    // a store meets the same translation.
    let source = &crate::formats::plain_url(source);
    let (store_url, fallback_attrs) = if is_manifest(source) {
        let manifest = load_manifest(source).await?;
        (manifest.url, manifest.attrs)
    } else {
        (source.clone(), None)
    };

    let store = open_store(&crate::formats::plain_url(&store_url))?;

    // The store is authoritative; the manifest is only a fallback for stores
    // whose root attributes cannot be read.
    let attrs = match read_root_attributes(&store).await {
        Ok(Some(attrs)) => Some(attrs),
        Ok(None) => fallback_attrs.clone(),
        Err(e) => {
            if fallback_attrs.is_some() {
                bevy::log::warn!("falling back to manifest attributes: {e}");
                fallback_attrs.clone()
            } else {
                return Err(e);
            }
        }
    };
    let attrs = attrs.ok_or_else(|| {
        format!("no OME-Zarr metadata found at {store_url}; is this a multiscale image root?")
    })?;

    let (multiscales, omero) = parse_ome(&attrs)?;
    let multiscale = multiscales
        .first()
        .ok_or("the OME metadata lists no multiscale images")?;

    let mut dataset = Dataset::open(store, multiscale, omero.as_ref()).await?;
    if !names_something(multiscale.name.as_deref()) {
        dataset.name = crate::formats::discover::label_for(&store_url);
    }
    Ok(dataset)
}

/// Each channel's label and published display window, read from the root
/// attributes alone: for something that has to name a store's channels, or
/// measure against their windows, before the store is opened.
///
/// Labeled as [`Dataset::open`] labels them, so a setting written against
/// these finds its channel once the store is open.
pub async fn published_channels(source: &str) -> Result<Vec<(String, f32, f32)>, String> {
    let store = open_store(&crate::formats::plain_url(source))?;
    let attrs = read_root_attributes(&store)
        .await?
        .ok_or_else(|| format!("no OME-Zarr metadata found at {source}"))?;
    let (_, omero) = parse_ome(&attrs)?;
    Ok(omero
        .map(|omero| {
            omero
                .channels
                .iter()
                .enumerate()
                .map(|(i, channel)| {
                    let label = channel
                        .other
                        .get("label")
                        .and_then(|value| value.as_str())
                        .map_or_else(|| format!("channel {i}"), str::to_string);
                    (
                        label,
                        channel.window.start as f32,
                        channel.window.end as f32,
                    )
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Whether a multiscale's own name is worth showing. Some writers put the
/// group path there, which for an image at the root of its store is `/` — a
/// name that tells nobody anything, where the store's own is its identifier.
fn names_something(name: Option<&str>) -> bool {
    name.is_some_and(|name| !name.trim().trim_matches('/').is_empty())
}

fn is_manifest(source: &str) -> bool {
    source.trim_end_matches('/').ends_with(".json")
}

async fn load_manifest(source: &str) -> Result<Manifest, String> {
    let text = crate::formats::discover::fetch_text(source).await?;
    serde_json::from_str(&text).map_err(|e| format!("parsing manifest {source}: {e}"))
}

/// Trim trailing slashes from an HTTP store root.
///
/// `zarrs_http` joins keys onto the base with an unconditional `/`, so a base
/// that already ends in one yields `.../image.zarr//zarr.json`. Object stores
/// treat that as a distinct key and return 404 for every read, which surfaces
/// as "group metadata is missing" rather than anything about the URL. Manifest
/// `url` fields conventionally carry the trailing slash, so normalize here.
fn http_base(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

fn open_store(url: &str) -> Result<ReadStore, String> {
    // Both backends come from `object_store`, which is the one zarrs can drive
    // asynchronously. A read here is a future that can be dropped, which is
    // what lets a tile nobody is waiting for any more be given up on.
    if crate::app::net::is_http(url) {
        let base = http_base(url);
        let parsed = url::Url::parse(&base).map_err(|e| format!("reading {base}: {e}"))?;
        let store = zarrs_object_store::object_store::http::HttpBuilder::new()
            .with_url(parsed.as_str())
            .build()
            .map_err(|e| format!("opening HTTP store {base}: {e}"))?;
        Ok(Arc::new(AsyncObjectStore::new(store)))
    } else {
        let path = std::path::Path::new(url)
            .canonicalize()
            .map_err(|e| format!("opening {url}: {e}"))?;
        let store =
            zarrs_object_store::object_store::local::LocalFileSystem::new_with_prefix(&path)
                .map_err(|e| format!("opening {url}: {e}"))?;
        Ok(Arc::new(AsyncObjectStore::new(store)))
    }
}

/// Read the root group's attributes, returning `None` when there is no root
/// group rather than treating that as a hard error.
async fn read_root_attributes(store: &ReadStore) -> Result<Option<serde_json::Value>, String> {
    match zarrs::group::Group::async_open(store.clone(), "/").await {
        Ok(group) => Ok(Some(serde_json::Value::Object(group.attributes().clone()))),
        Err(e) => Err(format!("reading root group: {e}")),
    }
}

/// Pull multiscales and omero out of root attributes, accepting both the 0.5
/// layout (nested under `ome`) and the 0.4 layout (at the top level).
pub(crate) fn parse_ome(
    attrs: &serde_json::Value,
) -> Result<(Vec<MultiscaleSpec>, Option<Omero>), String> {
    let mut attrs = attrs.clone();
    normalize_omero_colors(&mut attrs);
    strip_axis_extras(&mut attrs);
    let attrs = &attrs;

    if let Some(ome) = attrs.get("ome") {
        let fields: ome_zarr_metadata::v0_5::OmeFields = serde_json::from_value(ome.clone())
            .map_err(|e| format!("parsing OME-Zarr 0.5 metadata: {e}"))?;
        let multiscales = fields
            .multiscales
            .unwrap_or_default()
            .into_iter()
            .map(|m| MultiscaleSpec {
                name: m.name,
                axes: m.axes,
                datasets: m.datasets,
            })
            .collect();
        return Ok((multiscales, fields.omero));
    }

    let fields: ome_zarr_metadata::v0_4::OmeNgffGroupAttributes =
        serde_json::from_value(attrs.clone())
            .map_err(|e| format!("parsing OME-Zarr 0.4 metadata: {e}"))?;
    let multiscales = fields
        .multiscales
        .unwrap_or_default()
        .into_iter()
        .map(|m| MultiscaleSpec {
            name: m.name,
            axes: m.axes,
            datasets: m.datasets,
        })
        .collect();
    Ok((multiscales, fields.omero))
}

/// Rewrite omero channel colors into the form the spec mandates.
///
/// OME-NGFF defines the color as six bare hex digits, and the metadata crate
/// enforces that. Real converters are looser: both reference images here write
/// a leading `#`, and one uses the three-digit CSS shorthand (`#0df`). Rejecting
/// those would mean refusing to open working datasets over a cosmetic
/// difference, so normalize instead.
fn normalize_omero_colors(attrs: &mut serde_json::Value) {
    for root in ["ome", ""] {
        let node = if root.is_empty() {
            Some(&mut *attrs)
        } else {
            attrs.get_mut(root)
        };
        let Some(node) = node else { continue };
        let Some(channels) = node
            .get_mut("omero")
            .and_then(|o| o.get_mut("channels"))
            .and_then(|c| c.as_array_mut())
        else {
            continue;
        };
        for channel in channels {
            let Some(color) = channel.get_mut("color") else {
                continue;
            };
            let Some(text) = color.as_str() else { continue };
            if let Some(fixed) = canonical_hex(text) {
                *color = serde_json::Value::String(fixed);
            }
        }
    }
}

/// Drop from each axis anything the spec does not define.
///
/// An axis is a name, a type and a unit. The v2 reference image also writes a
/// `scale` on every axis — the same number the dataset's
/// `coordinateTransformations` already carries — and the metadata crate refuses
/// the whole document over it. The transforms are what the viewer reads, so the
/// duplicate is dropped rather than allowed to cost us the dataset.
fn strip_axis_extras(attrs: &mut serde_json::Value) {
    const DEFINED: [&str; 3] = ["name", "type", "unit"];
    let node = ome_root(attrs);
    if let Some(multiscales) = node.get_mut("multiscales").and_then(|m| m.as_array_mut()) {
        for multiscale in multiscales {
            let Some(axes) = multiscale.get_mut("axes").and_then(|a| a.as_array_mut()) else {
                continue;
            };
            for axis in axes {
                if let Some(fields) = axis.as_object_mut() {
                    fields.retain(|name, _| DEFINED.contains(&name.as_str()));
                }
            }
        }
    }
}

/// Where the OME metadata sits: nested under `ome` in 0.5, at the top level in
/// 0.4.
fn ome_root(attrs: &mut serde_json::Value) -> &mut serde_json::Value {
    if attrs.get("ome").is_some() {
        attrs.get_mut("ome").expect("just checked")
    } else {
        attrs
    }
}

/// Expand `#abc`, `#aabbcc` and `aabbcc` to `aabbcc`; leave anything else for
/// the parser to reject.
fn canonical_hex(text: &str) -> Option<String> {
    let body = text.strip_prefix('#').unwrap_or(text);
    if !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match body.len() {
        6 => Some(body.to_string()),
        3 => Some(body.chars().flat_map(|c| [c, c]).collect()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_path_is_not_taken_for_an_images_name() {
        assert!(!names_something(Some("/")));
        assert!(!names_something(Some(" ")));
        assert!(!names_something(None));
        assert!(names_something(Some("1458501514")));
        assert_eq!(
            crate::formats::discover::label_for(&crate::formats::plain_url(
                "zarr2://s3://allen-genetic-tools/tissuecyte/1219090168/ome_zarr_conversion/1219090168.zarr/"
            )),
            "1219090168"
        );
    }

    #[test]
    fn recognizes_manifests_by_extension() {
        assert!(is_manifest("image.json"));
        assert!(is_manifest("https://example.com/a/image.json"));
        assert!(!is_manifest("https://example.com/image.zarr/"));
        assert!(!is_manifest("/data/image.zarr"));
    }

    #[test]
    fn trailing_slashes_are_trimmed_from_http_roots() {
        // The manifests these images ship with all end in a slash.
        assert_eq!(
            http_base("https://example.com/a.zarr/"),
            "https://example.com/a.zarr"
        );
        assert_eq!(
            http_base("https://example.com/a.zarr"),
            "https://example.com/a.zarr"
        );
        assert_eq!(
            http_base("https://example.com/a.zarr///"),
            "https://example.com/a.zarr"
        );
    }

    #[test]
    fn the_example_store_survives_normalization() {
        assert!(!http_base(crate::catalog::examples::EXAMPLES[0].url).ends_with('/'));
    }

    #[test]
    fn parses_the_real_root_attributes() {
        let root: serde_json::Value =
            serde_json::from_str(include_str!("../../../testdata/root_zarr_v3.json")).unwrap();
        let (multiscales, omero) = parse_ome(&root["attributes"]).unwrap();

        assert_eq!(multiscales.len(), 1);
        assert_eq!(multiscales[0].datasets.len(), 6);
        assert_eq!(multiscales[0].axes.len(), 4);

        let omero = omero.expect("the reference image declares omero channels");
        assert_eq!(omero.channels.len(), 3);
        // Channel colors drive the composite, so check they survive parsing.
        assert_eq!(
            (
                omero.channels[0].color.r,
                omero.channels[0].color.g,
                omero.channels[0].color.b
            ),
            (255, 0, 0)
        );
        assert_eq!(omero.channels[0].window.end, 255.0);
    }

    #[test]
    fn parses_the_v2_root_attributes() {
        // A Zarr v2 store keeps its attributes in `.zattrs`, with the OME
        // fields at the top level rather than under `ome`. Saved from the
        // reference v2 image, which is what the axis and color quirks below
        // were found in.
        let attrs: serde_json::Value =
            serde_json::from_str(include_str!("../../../testdata/root_zarr_v2.json")).unwrap();
        let (multiscales, omero) = parse_ome(&attrs).unwrap();

        assert_eq!(multiscales.len(), 1);
        assert_eq!(multiscales[0].datasets.len(), 7);
        assert_eq!(multiscales[0].axes.len(), 4);
        assert_eq!(multiscales[0].axes[3].name, "x");

        let omero = omero.expect("the reference v2 image declares omero channels");
        assert_eq!(omero.channels.len(), 2);
        // `#0df`, expanded from CSS shorthand.
        let cyan = &omero.channels[0].color;
        assert_eq!((cyan.r, cyan.g, cyan.b), (0, 0xdd, 0xff));
        assert_eq!(omero.channels[1].window.end, 60828.0);
    }

    #[test]
    fn an_axis_keeps_only_the_fields_the_spec_defines() {
        // The v2 reference image writes a `scale` on every axis, which the
        // metadata crate rejects outright — it belongs in the dataset's
        // coordinate transformations, where the viewer reads it from.
        let mut attrs = serde_json::json!({
            "multiscales": [{
                "axes": [{"name": "x", "type": "space", "unit": "millimeter", "scale": 0.00065}],
                "datasets": [],
            }]
        });
        strip_axis_extras(&mut attrs);
        let axis = &attrs["multiscales"][0]["axes"][0];
        assert!(axis.get("scale").is_none());
        assert_eq!(axis["unit"], "millimeter");
    }

    #[test]
    fn the_0_5_layout_is_stripped_where_it_actually_sits() {
        // Nested under `ome`, so stripping the top level would miss it.
        let mut attrs = serde_json::json!({
            "ome": {"multiscales": [{"axes": [{"name": "y", "scale": 1.0}]}]}
        });
        strip_axis_extras(&mut attrs);
        assert!(
            attrs["ome"]["multiscales"][0]["axes"][0]
                .get("scale")
                .is_none()
        );
    }

    #[test]
    fn parses_the_root_attributes_of_a_stack() {
        // The tissuecyte reference: a specimen cut into sections, three
        // channels, ten levels. Saved from the store the neuroglancer config
        // points at.
        let attrs: serde_json::Value =
            serde_json::from_str(include_str!("../../../testdata/root_zarr_v2_stack.json"))
                .unwrap();
        let (multiscales, omero) = parse_ome(&attrs).unwrap();

        assert_eq!(multiscales[0].datasets.len(), 10);
        let axes: Vec<&str> = multiscales[0]
            .axes
            .iter()
            .map(|axis| axis.name.as_str())
            .collect();
        assert_eq!(axes, ["c", "z", "y", "x"], "z is an axis to page through");

        let omero = omero.expect("the stack declares omero channels");
        let labels: Vec<&str> = omero
            .channels
            .iter()
            .map(|channel| channel.other["label"].as_str().unwrap())
            .collect();
        assert_eq!(labels, ["red", "green", "blue"]);
    }

    #[test]
    fn a_stack_holds_many_slices_in_one_chunk() {
        // Why the levels carry a chunk cache. A chunk of this store is forty
        // slices deep, so reading one slice of a tile decodes the thirty-nine
        // around it — and the next slice paged to is already in memory.
        let meta: serde_json::Value =
            serde_json::from_str(include_str!("../../../testdata/array0_zarr_v2_stack.json"))
                .unwrap();
        let chunks: Vec<u64> = serde_json::from_value(meta["chunks"].clone()).unwrap();
        let shape: Vec<u64> = serde_json::from_value(meta["shape"].clone()).unwrap();
        assert_eq!(chunks, [3, 40, 128, 128]);
        assert_eq!(shape[1], 142, "142 slices to page through");
        assert!(
            chunks[1] > 1,
            "a chunk covering one slice would make paging a refetch"
        );
    }

    #[test]
    fn a_v2_array_declares_no_codec_chain() {
        // Which is how a level is known not to be sharded: v2 names a single
        // `compressor`, and has no `codecs` for a sharding codec to sit in.
        let meta: serde_json::Value =
            serde_json::from_str(include_str!("../../../testdata/array0_zarr_v2.json")).unwrap();
        assert_eq!(meta["zarr_format"], 2);
        assert!(meta.get("codecs").is_none());
        assert_eq!(meta["compressor"]["id"], "blosc");
        assert_eq!(meta["chunks"], serde_json::json!([2, 1, 128, 128]));
    }

    #[test]
    fn a_manifest_carrying_v2_attributes_parses() {
        // The manifests written alongside these conversions add fields of their
        // own — the Zarr version, the array shapes, a color listing — none of
        // which the viewer reads. They must not stop the OME fields being
        // found.
        let manifest: Manifest = serde_json::from_str(
            r#"{"url":"https://example.com/a.zarr/",
                "zarrVersion":2,
                "arrays":[{"path":"0","shape":[2,1,26669,53718],"attrs":{}}],
                "colorChannels":[{"label":"CFP"}],
                "attrs":{"zarrVersion":2,
                         "multiscales":[{"version":"0.4",
                            "axes":[{"name":"y","type":"space","scale":0.00065},
                                    {"name":"x","type":"space","scale":0.00065}],
                            "datasets":[{"path":"0","coordinateTransformations":[
                                {"type":"scale","scale":[0.00065,0.00065]}]}]}]}}"#,
        )
        .unwrap();
        let (multiscales, _) = parse_ome(&manifest.attrs.unwrap()).unwrap();
        assert_eq!(multiscales.len(), 1);
        assert_eq!(multiscales[0].datasets.len(), 1);
    }

    #[test]
    fn parses_a_manifest_of_the_documented_shape() {
        let manifest: Manifest = serde_json::from_str(
            r#"{"url":"https://example.com/a.zarr/","attrs":{"zarrVersion":3},"arrays":[]}"#,
        )
        .unwrap();
        assert_eq!(manifest.url, "https://example.com/a.zarr/");
        assert!(manifest.attrs.is_some());
    }

    #[test]
    fn normalizes_the_color_forms_real_converters_emit() {
        // Both reference images prefix with `#`, which the spec does not allow.
        assert_eq!(canonical_hex("#FF0000").unwrap(), "FF0000");
        assert_eq!(canonical_hex("FF0000").unwrap(), "FF0000");
        // CSS shorthand, as used by the v2 reference image.
        assert_eq!(canonical_hex("#0df").unwrap(), "00ddff");
        // Anything else is left alone so the parser can complain properly.
        assert!(canonical_hex("rebeccapurple").is_none());
        assert!(canonical_hex("#12345").is_none());
    }

    #[test]
    fn a_hash_prefixed_color_still_opens_the_image() {
        let mut attrs = serde_json::json!({
            "ome": {
                "version": "0.5",
                "omero": {"channels": [{"color": "#00FF00",
                    "window": {"min": 0.0, "max": 255.0, "start": 0.0, "end": 255.0}}]}
            }
        });
        normalize_omero_colors(&mut attrs);
        assert_eq!(attrs["ome"]["omero"]["channels"][0]["color"], "00FF00");
    }

    #[test]
    fn reports_a_useful_error_for_metadata_that_is_not_ome() {
        let attrs = serde_json::json!({"something": "else"});
        let (multiscales, _) = parse_ome(&attrs).unwrap_or_default();
        assert!(multiscales.is_empty());
    }
}
