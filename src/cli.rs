//! The command line, and opening what it names.
//!
//! Everything is opened before the window is, so a bad URL fails on the command
//! line rather than behind a blank panel.
//!
//! Nothing is opened unless it is named. The window starts empty and offers the
//! examples in `catalog::examples::EXAMPLES` instead, because a first run that spends a
//! minute fetching three reference datasets nobody asked for is a first run
//! spent waiting.

use clap::Parser;

use crate::formats::discover;
use crate::view::grid::MAX_PANELS;

#[derive(Parser, Debug)]
#[command(
    name = "bevy-data-explorer",
    about = "Stream and explore large scientific datasets"
)]
pub struct Args {
    /// Datasets to open, each in a frame of its own, in the order given: an
    /// OME-Zarr store (http(s) URL or local directory) or a manifest .json
    /// describing one, a Deep Zoom .dzi, Scatterbrain metadata, an .svg, a
    /// .csv, .tsv or .parquet table, or a Brain Knowledge Platform endpoint
    /// with `?specimens=<project>` on it. Left out, the window starts empty.
    pub sources: Vec<String>,

    /// Another dataset in a frame of its own, usually Scatterbrain metadata
    /// JSON (http(s) URL or local file). Recognized by reading it, like any
    /// other.
    #[arg(long)]
    pub points: Option<String>,

    /// Z slice to display for volumetric images. Left out, a stack opens in
    /// the middle, where the specimen is.
    #[arg(long)]
    pub z: Option<u64>,

    /// Texture memory budget for cached tiles, in MB. Larger values make
    /// zooming back out and revisiting areas redraw without refetching.
    #[arg(long, default_value_t = crate::formats::image::DEFAULT_CACHE_BUDGET_MB)]
    pub cache_mb: usize,

    /// Memory for image chunks as downloaded, in MB, shared by every stack.
    /// Paging through a stack reads the same chunks slice after slice, and
    /// what is held here is decoded again rather than downloaded again.
    #[arg(long, default_value_t = crate::formats::image::chunk_cache::DEFAULT_CHUNK_CACHE_MB)]
    pub chunk_cache_mb: usize,

    /// Another dataset in a frame of its own, usually sectioned Scatterbrain
    /// metadata. A cloud listing more than one slide gets the sections panel
    /// wherever it is named.
    #[arg(long)]
    pub slices: Option<String>,

    /// Another dataset in a frame of its own, usually a second point cloud.
    #[arg(long)]
    pub cells: Option<String>,

    /// A dataset to draw over the first frame, such as an SVG of annotations
    /// over a slide. Repeat for more than one. Anything can be layered; one
    /// measured differently from that frame is drawn anyway, not rescaled.
    #[arg(long)]
    pub layer: Vec<String>,

    /// A bookmark to open: a file exported from the sidebar, or a shared line
    /// starting `bde1:`. Its frames replace any the other flags open.
    #[arg(long)]
    pub bookmark: Option<String>,

    /// Maximum points held on the GPU for the point cloud.
    #[arg(long, default_value_t = crate::formats::pointcloud::DEFAULT_POINT_BUDGET)]
    pub point_budget: usize,

    /// Maximum points held on the GPU for the sectioned panel.
    #[arg(long, default_value_t = crate::formats::slices::DEFAULT_SLICE_BUDGET)]
    pub slice_budget: usize,
}

impl Args {
    /// The budgets every dataset is opened with, including one opened later
    /// from the sidebar rather than from here.
    pub fn load_settings(&self) -> crate::formats::LoadSettings {
        crate::formats::LoadSettings {
            z_slice: self.z,
            cache_bytes: self.cache_mb * 1024 * 1024,
            point_budget: self.point_budget,
            slice_budget: self.slice_budget,
        }
    }
}

impl Args {
    /// The bookmark named, read now so a bad one fails here rather than in a
    /// window that opens empty.
    pub fn bookmark(&self) -> Result<Option<crate::bookmark::snapshot::Bookmark>, String> {
        let Some(named) = self.bookmark.as_deref() else {
            return Ok(None);
        };
        let text = if std::path::Path::new(named).is_file() {
            std::fs::read_to_string(named).map_err(|e| format!("reading {named}: {e}"))?
        } else {
            named.to_string()
        };
        let bookmark = crate::bookmark::codec::from_text(&text)?;
        println!("opening bookmark {}", bookmark.name);
        Ok(Some(bookmark))
    }
}

/// A dataset the command line named, read and recognized.
pub struct Opened {
    /// The address as it was given, recorded on the source it becomes.
    pub url: String,
    pub dataset: discover::Discovered,
    /// Named with `--layer`, so drawn over the first frame.
    pub as_layer: bool,
}

impl Args {
    /// Read every dataset named, in the order their frames open, reporting
    /// each as it lands.
    ///
    /// Every one is recognized by reading it, exactly as a URL typed into the
    /// sidebar is, so the flags say only where a dataset goes and not what it
    /// is. Frames first, then layers, so a layer is never mistaken for the
    /// frame it is meant to be drawn over.
    pub fn open(&self) -> Result<Vec<Opened>, String> {
        let frames: Vec<(&str, &str, bool)> = self
            .sources
            .iter()
            .map(|url| ("source", url.as_str(), false))
            .chain(
                [
                    ("points", &self.points),
                    ("cells", &self.cells),
                    ("slices", &self.slices),
                ]
                .into_iter()
                .filter_map(|(label, url)| Some((label, url.as_deref()?, false))),
            )
            .collect();
        // Said before anything is read, rather than after a minute of fetching
        // datasets there is no room to show.
        if frames.len() > MAX_PANELS {
            return Err(format!(
                "{} datasets named, but the grid holds {MAX_PANELS} frames; \
                 draw some over another with --layer",
                frames.len()
            ));
        }
        let layers = self.layer.iter().map(|url| ("layer", url.as_str(), true));

        frames
            .into_iter()
            .chain(layers)
            .map(|(label, url, as_layer)| {
                println!("opening {label:<6} {url}");
                // Nothing else is happening yet: the window is not up, and a
                // bad URL should fail here rather than behind a blank panel.
                let dataset = crate::app::net::block_on(discover::discover(url))?;
                println!("  {}", dataset.name());
                Ok(Opened {
                    url: url.trim().to_string(),
                    dataset,
                    as_layer,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_datasets_open_in_the_order_named() {
        let args =
            Args::try_parse_from(["bde", "a.zarr", "b.csv", "--layer", "c.svg", "d.dzi"]).unwrap();
        assert_eq!(args.sources, ["a.zarr", "b.csv", "d.dzi"]);
        assert_eq!(args.layer, ["c.svg"]);
    }

    #[test]
    fn more_datasets_than_frames_is_refused_before_anything_is_read() {
        let mut named = vec!["bde".to_string()];
        named.extend((0..=MAX_PANELS).map(|n| format!("https://example.invalid/{n}.zarr")));
        let error = Args::try_parse_from(named).unwrap().open().err().unwrap();
        assert!(error.contains("--layer"), "{error}");
    }
}
