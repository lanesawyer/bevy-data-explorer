//! Working out what a URL points at.
//!
//! The command line says which format it is opening; a URL typed into the
//! sidebar does not. So the bytes decide: whatever a source resolves to is
//! recognised by reading it rather than by asking the user to classify it
//! first, and a source that matches nothing reports what was tried rather than
//! failing silently.
//!
//! Recognition is deliberately cheap. Every format is described by a small
//! document at the root — Scatterbrain metadata, a Deep Zoom descriptor, or an
//! OME-Zarr group's attributes — so nothing beyond that document is fetched to
//! decide.

use crate::formats::dzi::pyramid::DeepZoom;
use crate::formats::image::dataset::Dataset;
use crate::formats::scatterbrain::Scatterbrain;
use crate::formats::svg::parse::Svg;

/// What a source turned out to be.
pub enum Discovered {
    Image(Box<Dataset>),
    DeepZoom(DeepZoom),
    /// A single octree.
    Points {
        name: String,
        cloud: Scatterbrain,
    },
    /// A specimen cut into slices, which gets the sectioned panel instead.
    Slices(Scatterbrain),
    /// Outlines drawn over an image, meant to be layered onto it.
    Annotations(Svg),
}

impl Discovered {
    /// What to call this in a status line, before it is registered as a source.
    pub fn name(&self) -> &str {
        match self {
            Discovered::Image(dataset) => &dataset.name,
            Discovered::DeepZoom(dzi) => &dzi.name,
            Discovered::Points { name, .. } => name,
            Discovered::Slices(_) => "Sections",
            Discovered::Annotations(svg) => &svg.name,
        }
    }
}

/// Recognise whatever `source` points at, or say why it could not be.
///
/// The same reading the command line does, which blocks on it before the
/// window is up; callers with a window open run it on a task instead.
pub async fn discover(source: &str) -> Result<Discovered, String> {
    let source = crate::formats::plain_url(source.trim());
    let source = source.as_str();
    if source.is_empty() {
        return Err("type the URL of a dataset to load".into());
    }

    // A Deep Zoom image is named by its descriptor, and nothing else ends in
    // `.dzi`, so there is nothing to try it against.
    if is_dzi(source) {
        let text = fetch_text(source).await?;
        return DeepZoom::parse(source, &text).map(Discovered::DeepZoom);
    }

    // Likewise an SVG, which is the one format here that is read whole.
    if is_svg(source) {
        let text = fetch_text(source).await?;
        return crate::formats::svg::parse::parse(&crate::formats::svg::label_for(source), &text)
            .map(Discovered::Annotations);
    }

    // A Zarr root is a directory, so only a `.json` can be Scatterbrain
    // metadata. Trying it first costs one fetch and settles the common case.
    if is_json(source) {
        let text = fetch_text(source).await?;
        let points = match Scatterbrain::parse(&text) {
            Ok(cloud) => return Ok(classify(source, cloud)),
            Err(e) => e,
        };
        // Not Scatterbrain, so the other thing a `.json` can be is an image
        // manifest naming the store that holds the pyramid.
        return crate::formats::image::store::open(source)
            .await
            .map(|dataset| Discovered::Image(Box::new(dataset)))
            .map_err(|image| unrecognised(source, &image, &points));
    }

    crate::formats::image::store::open(source)
        .await
        .map(|dataset| Discovered::Image(Box::new(dataset)))
        .map_err(|image| {
            format!(
                "{source} is not an OME-Zarr store: {image}\n\
                 Scatterbrain metadata is a .json, so it was not tried."
            )
        })
}

/// Which panel a parsed cloud belongs in.
///
/// Both shapes parse into a list of slides, so the count is what separates a
/// single cloud from a specimen cut into slices — the same distinction the
/// `--points` and `--slices` flags make by hand.
fn classify(source: &str, cloud: Scatterbrain) -> Discovered {
    if cloud.slides.len() > 1 {
        Discovered::Slices(cloud)
    } else {
        Discovered::Points {
            name: label_for(source),
            cloud,
        }
    }
}

fn unrecognised(source: &str, image: &str, points: &str) -> String {
    format!(
        "could not recognise {source}\n\
         as an OME-Zarr image: {image}\n\
         as Scatterbrain metadata: {points}"
    )
}

/// Read a source, over HTTP or off disk.
pub async fn fetch_text(source: &str) -> Result<String, String> {
    let source = &crate::formats::plain_url(source);
    if is_http(source) {
        crate::app::net::fetch_text(source).await
    } else {
        std::fs::read_to_string(source).map_err(|e| format!("reading {source}: {e}"))
    }
}

fn is_http(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

fn is_json(source: &str) -> bool {
    has_extension(source, ".json")
}

fn is_dzi(source: &str) -> bool {
    has_extension(source, ".dzi")
}

fn is_svg(source: &str) -> bool {
    has_extension(source, ".svg")
}

/// Whether the file a source names ends in `extension`, ignoring any query
/// string or fragment after it.
fn has_extension(source: &str, extension: &str) -> bool {
    let path = source.split(['?', '#']).next().unwrap_or(source);
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(extension))
}

/// Metadata file names that name the format rather than the dataset.
const GENERIC_NAMES: [&str; 2] = ["scatterbrain.json", "metadata.json"];

/// Name a point cloud after the place it came from.
///
/// Scatterbrain metadata carries no name of its own, and the command line gets
/// one passed in. A URL has only its path, so the last segment is used —
/// stepping up past a file name that names the format rather than the dataset,
/// since every cloud in a store would otherwise be called `ScatterBrain`.
pub fn label_for(source: &str) -> String {
    let path = source.split(['?', '#']).next().unwrap_or(source);
    let mut segments = path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .rev()
        .filter(|segment| !GENERIC_NAMES.contains(&segment.to_ascii_lowercase().as_str()));

    match segments.next() {
        Some(segment) => segment
            .trim_end_matches(".json")
            .trim_end_matches(".zarr")
            .to_string(),
        None => "Point cloud".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_json_can_be_scatterbrain_metadata() {
        assert!(is_json("https://example.com/a/ScatterBrain.json"));
        assert!(is_json("https://example.com/a/metadata.JSON"));
        // A store root is a directory, whatever it is called.
        assert!(!is_json("https://example.com/image.zarr/"));
        assert!(!is_json("/data/image.zarr"));
        // A directory named after a manifest must not be mistaken for one.
        assert!(!is_json("https://example.com/metadata.json/tiles"));
        // A signed URL is still the file it names.
        assert!(is_json(
            "https://example.com/a/ScatterBrain.json?X-Amz-Signature=abc"
        ));
    }

    #[test]
    fn a_deep_zoom_image_is_known_by_its_descriptor() {
        assert!(is_dzi(
            "https://example.com/slides/H20.33.040-A12-I6-primary.dzi"
        ));
        assert!(is_dzi("/data/slide.DZI?signature=abc"));
        assert!(!is_dzi("https://example.com/slide_files/14/0_0.jpeg"));
        assert!(!is_dzi("https://example.com/image.zarr/"));
    }

    #[test]
    fn annotations_are_known_by_their_extension() {
        assert!(is_svg("https://bucket/slide/annotation.svg"));
        assert!(is_svg("/data/Regions.SVG?v=2"));
        assert!(!is_svg("https://bucket/slide/slide.dzi"));
    }

    #[test]
    fn a_cloud_is_named_after_where_it_came_from() {
        assert_eq!(
            label_for("https://store/wmb_tenx/G4I4GFJXJB9ATZ3PTX1/ScatterBrain.json"),
            "G4I4GFJXJB9ATZ3PTX1"
        );
        // A file that does name the dataset keeps its name.
        assert_eq!(label_for("/data/visual_cortex.json"), "visual_cortex");
        assert_eq!(label_for("https://store/1458501514.zarr/"), "1458501514");
    }

    #[test]
    fn a_query_string_is_not_part_of_the_name() {
        assert_eq!(
            label_for("https://store/cortex.json?version=3#top"),
            "cortex"
        );
    }

    #[test]
    fn a_source_with_nothing_to_name_it_still_gets_a_name() {
        assert!(!label_for("ScatterBrain.json").is_empty());
        assert!(!label_for("").is_empty());
    }

    #[test]
    fn a_sectioned_dataset_goes_to_the_sections_panel() {
        let text = include_str!("../../testdata/scatterbrain_slides.json");
        let cloud = Scatterbrain::parse(text).unwrap();
        assert!(matches!(
            classify("https://store/x.json", cloud),
            Discovered::Slices(_)
        ));
    }

    #[test]
    fn a_single_cloud_goes_to_the_point_cloud_panel() {
        let text = include_str!("../../testdata/scatterbrain.json");
        let cloud = Scatterbrain::parse(text).unwrap();
        assert!(matches!(
            classify("https://store/REF01/ScatterBrain.json", cloud),
            Discovered::Points { .. }
        ));
    }

    #[test]
    fn nothing_at_all_is_rejected_before_anything_is_fetched() {
        // Run to completion here: it refuses before it reaches the network, so
        // there is nothing to wait for.
        assert!(crate::app::net::block_on(discover("   ")).is_err());
    }

    #[test]
    fn an_unrecognised_source_says_what_was_tried() {
        let message = unrecognised("https://store/x.json", "no multiscales", "no `root`");
        assert!(message.contains("https://store/x.json"));
        assert!(message.contains("no multiscales"));
        assert!(message.contains("no `root`"));
    }
}
