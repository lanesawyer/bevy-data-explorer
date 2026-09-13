//! The command line, and opening what it names.
//!
//! Everything is opened before the window is, so a bad URL fails on the command
//! line rather than behind a blank panel.

use std::sync::Arc;

use clap::Parser;

use crate::formats::image::dataset::Dataset;
use crate::formats::scatterbrain::Scatterbrain;

/// Scatterbrain metadata for the reference point cloud.
const DEFAULT_POINTS: &str = "https://d2o7sc91n904vd.cloudfront.net/wmb_tenx_01172024_stage-20240128193624/G4I4GFJXJB9ATZ3PTX1/ScatterBrain.json";

/// Scatterbrain metadata for the SEA-AD mapped dataset, which carries numeric
/// properties alongside categorical ones.
const DEFAULT_CELLS: &str = "https://d2o7sc91n904vd.cloudfront.net/bkppg-sfs-stage-mjff-updates-03262025-20250403032833/839TIB6YQVFHZSGX401/ScatterBrain.json";

/// Scatterbrain metadata for the reference sectioned dataset.
const DEFAULT_SLICES: &str = "https://d2o7sc91n904vd.cloudfront.net/bkppg-sfs-stage-wmb-imputed-genes-20240918212918/VFOFYPFQGRKUDQUZ3FF/ScatterBrain.json";

/// What a source has to be set to for it to be left out.
const NONE: &str = "none";

#[derive(Parser, Debug)]
#[command(
    name = "bevy-data-explorer",
    about = "Stream and explore large scientific datasets"
)]
pub struct Args {
    /// OME-Zarr store (http(s) URL or local directory), or a manifest .json
    /// describing one. Defaults to the reference image.
    #[arg(default_value = crate::formats::image::store::DEFAULT_SOURCE)]
    pub source: String,

    /// Scatterbrain metadata JSON (http(s) URL or local file). Pass `none` to
    /// show the image on its own.
    #[arg(long, default_value = DEFAULT_POINTS)]
    pub points: String,

    /// Z slice to display for volumetric images.
    #[arg(long, default_value_t = 0)]
    pub z: u64,

    /// Texture memory budget for cached tiles, in MB. Larger values make
    /// zooming back out and revisiting areas redraw without refetching.
    #[arg(long, default_value_t = crate::formats::image::DEFAULT_CACHE_BUDGET_MB)]
    pub cache_mb: usize,

    /// Sectioned Scatterbrain metadata JSON, shown as a third panel. Pass
    /// `none` to leave it out.
    #[arg(long, default_value = DEFAULT_SLICES)]
    pub slices: String,

    /// A second Scatterbrain point cloud, shown as a fourth panel. Pass `none`
    /// to leave it out.
    #[arg(long, default_value = DEFAULT_CELLS)]
    pub cells: String,

    /// Maximum points held on the GPU for the point cloud.
    #[arg(long, default_value_t = crate::formats::pointcloud::DEFAULT_POINT_BUDGET)]
    pub point_budget: usize,

    /// Maximum points held on the GPU for the sectioned panel.
    #[arg(long, default_value_t = crate::formats::slices::DEFAULT_SLICE_BUDGET)]
    pub slice_budget: usize,
}

/// Everything the command line named, opened and ready to be handed to plugins.
pub struct Datasets {
    pub image: Arc<Dataset>,
    pub points: Option<Arc<Scatterbrain>>,
    pub cells: Option<Arc<Scatterbrain>>,
    pub sections: Option<Arc<Scatterbrain>>,
}

impl Args {
    /// Open every source named, reporting each as it lands.
    pub fn open(&self) -> Result<Datasets, String> {
        println!("opening {:<6} {}", "image", self.source);
        let image = Arc::new(crate::formats::image::store::open(&self.source)?);
        println!(
            "  {}: {} levels, {} channels, {} x {} px",
            image.name,
            image.levels.len(),
            image.channels.len(),
            image.levels[0].width,
            image.levels[0].height
        );

        Ok(Datasets {
            image,
            points: open_cloud("points", &self.points)?,
            cells: open_cloud("cells", &self.cells)?,
            sections: open_cloud("slices", &self.slices)?,
        })
    }
}

/// Open a point cloud, unless it was turned off.
fn open_cloud(label: &str, source: &str) -> Result<Option<Arc<Scatterbrain>>, String> {
    if source.eq_ignore_ascii_case(NONE) {
        return Ok(None);
    }
    println!("opening {label:<6} {source}");
    let cloud = Arc::new(load_points(source)?);
    describe(label, &cloud);
    Ok(Some(cloud))
}

fn load_points(source: &str) -> Result<Scatterbrain, String> {
    let text = if source.starts_with("http://") || source.starts_with("https://") {
        reqwest::blocking::get(source)
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
            .map_err(|e| format!("fetching {source}: {e}"))?
    } else {
        std::fs::read_to_string(source).map_err(|e| format!("reading {source}: {e}"))?
    };
    Scatterbrain::parse(&text)
}

fn describe(label: &str, cloud: &Scatterbrain) {
    println!(
        "  {} points across {} slide(s), {} octree nodes, depth {} [{label}]",
        cloud.total_points(),
        cloud.slides.len(),
        cloud.node_count(),
        cloud.max_depth(),
    );
    if let Some(first) = cloud.slides.first() {
        // The root is a subsample; children add the rest. Showing both makes
        // the additive structure visible at a glance.
        println!(
            "  slide {} root holds {} of its {} points",
            first.index,
            first.root().count,
            first.total_points,
        );
    }
}
