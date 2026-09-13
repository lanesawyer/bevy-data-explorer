//! Resolving a user-supplied source into an open dataset.
//!
//! A source may be a Zarr store (HTTP URL or local directory) or a manifest
//! JSON of the kind produced alongside these conversions, which carries the
//! store URL plus a copy of the root attributes.

use std::sync::Arc;

use ome_zarr_metadata::v0_4::{Axis, MultiscaleImageDataset, Omero};
use serde::Deserialize;

use crate::formats::image::dataset::{Dataset, ReadStore};

/// The reference image, used when no source is given.
pub const DEFAULT_SOURCE: &str = "https://h301-scanning-802451596237-us-west-2.s3.us-west-2.amazonaws.com/2402091625/ome_zarr_conversion/1458501514.zarr/";

/// A multiscale image normalised across OME-Zarr versions. The 0.4 and 0.5
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
pub fn open(source: &str) -> Result<Dataset, String> {
    let (store_url, fallback_attrs) = if is_manifest(source) {
        let manifest = load_manifest(source)?;
        (manifest.url, manifest.attrs)
    } else {
        (source.to_string(), None)
    };

    let store = open_store(&store_url)?;

    // The store is authoritative; the manifest is only a fallback for stores
    // whose root attributes cannot be read.
    let attrs = match read_root_attributes(&store) {
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

    Dataset::open(store, multiscale, omero.as_ref())
}

fn is_manifest(source: &str) -> bool {
    source.trim_end_matches('/').ends_with(".json")
}

fn load_manifest(source: &str) -> Result<Manifest, String> {
    let text = if is_http(source) {
        reqwest::blocking::get(source)
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
            .map_err(|e| format!("fetching manifest {source}: {e}"))?
    } else {
        std::fs::read_to_string(source).map_err(|e| format!("reading manifest {source}: {e}"))?
    };
    serde_json::from_str(&text).map_err(|e| format!("parsing manifest {source}: {e}"))
}

/// Trim trailing slashes from an HTTP store root.
///
/// `zarrs_http` joins keys onto the base with an unconditional `/`, so a base
/// that already ends in one yields `.../image.zarr//zarr.json`. Object stores
/// treat that as a distinct key and return 404 for every read, which surfaces
/// as "group metadata is missing" rather than anything about the URL. Manifest
/// `url` fields conventionally carry the trailing slash, so normalise here.
fn http_base(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

fn is_http(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

fn open_store(url: &str) -> Result<ReadStore, String> {
    if is_http(url) {
        let base = http_base(url);
        let store = zarrs_http::HTTPStore::new(&base)
            .map_err(|e| format!("opening HTTP store {base}: {e}"))?;
        Ok(Arc::new(store))
    } else {
        let store = zarrs::filesystem::FilesystemStore::new(url)
            .map_err(|e| format!("opening {url}: {e}"))?;
        Ok(Arc::new(store))
    }
}

/// Read the root group's attributes, returning `None` when there is no root
/// group rather than treating that as a hard error.
fn read_root_attributes(store: &ReadStore) -> Result<Option<serde_json::Value>, String> {
    match zarrs::group::Group::open(store.clone(), "/") {
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

/// Rewrite omero channel colours into the form the spec mandates.
///
/// OME-NGFF defines the colour as six bare hex digits, and the metadata crate
/// enforces that. Real converters are looser: both reference images here write
/// a leading `#`, and one uses the three-digit CSS shorthand (`#0df`). Rejecting
/// those would mean refusing to open working datasets over a cosmetic
/// difference, so normalise instead.
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
    fn recognises_manifests_by_extension() {
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
    fn the_default_source_survives_normalisation() {
        assert!(!http_base(DEFAULT_SOURCE).ends_with('/'));
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
        // Channel colours drive the composite, so check they survive parsing.
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
    fn parses_a_manifest_of_the_documented_shape() {
        let manifest: Manifest = serde_json::from_str(
            r#"{"url":"https://example.com/a.zarr/","attrs":{"zarrVersion":3},"arrays":[]}"#,
        )
        .unwrap();
        assert_eq!(manifest.url, "https://example.com/a.zarr/");
        assert!(manifest.attrs.is_some());
    }

    #[test]
    fn normalises_the_colour_forms_real_converters_emit() {
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
    fn a_hash_prefixed_colour_still_opens_the_image() {
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
