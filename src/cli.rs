//! The command line, and opening what it names.
//!
//! Everything is opened before the window is, so a bad URL fails on the command
//! line rather than behind a blank panel.
//!
//! Nothing is opened unless it is named. The window starts empty and offers the
//! examples in `formats::EXAMPLES` instead, because a first run that spends a
//! minute fetching three reference datasets nobody asked for is a first run
//! spent waiting.

use std::sync::Arc;

use clap::Parser;

use crate::formats::image::dataset::Dataset;
use crate::formats::scatterbrain::Scatterbrain;

#[derive(Parser, Debug)]
#[command(
    name = "bevy-data-explorer",
    about = "Stream and explore large scientific datasets"
)]
pub struct Args {
    /// OME-Zarr store (http(s) URL or local directory), or a manifest .json
    /// describing one. Left out, the window starts empty.
    pub source: Option<String>,

    /// Scatterbrain metadata JSON (http(s) URL or local file).
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

    /// Sectioned Scatterbrain metadata JSON, shown as its own panel.
    #[arg(long)]
    pub slices: Option<String>,

    /// A second Scatterbrain point cloud, shown as its own panel.
    #[arg(long)]
    pub cells: Option<String>,

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

/// Everything the command line named, opened and ready to be handed to plugins.
pub struct Datasets {
    pub image: Option<Arc<Dataset>>,
    pub points: Option<Arc<Scatterbrain>>,
    pub cells: Option<Arc<Scatterbrain>>,
    pub sections: Option<Arc<Scatterbrain>>,
}

impl Args {
    /// Open every source named, reporting each as it lands.
    pub fn open(&self) -> Result<Datasets, String> {
        Ok(Datasets {
            image: self.open_image()?,
            points: open_cloud("points", self.points.as_deref())?,
            cells: open_cloud("cells", self.cells.as_deref())?,
            sections: open_cloud("slices", self.slices.as_deref())?,
        })
    }

    fn open_image(&self) -> Result<Option<Arc<Dataset>>, String> {
        let Some(source) = self.source.as_deref() else {
            return Ok(None);
        };
        println!("opening {:<6} {source}", "image");
        let image = Arc::new(crate::formats::image::store::open(source)?);
        println!(
            "  {}: {} levels, {} channels, {} x {} px",
            image.name,
            image.levels.len(),
            image.channels.len(),
            image.levels[0].width,
            image.levels[0].height
        );
        Ok(Some(image))
    }
}

/// Open a point cloud, if one was named.
fn open_cloud(label: &str, source: Option<&str>) -> Result<Option<Arc<Scatterbrain>>, String> {
    let Some(source) = source else {
        return Ok(None);
    };
    println!("opening {label:<6} {source}");
    let cloud = Arc::new(load_points(source)?);
    describe(label, &cloud);
    Ok(Some(cloud))
}

fn load_points(source: &str) -> Result<Scatterbrain, String> {
    Scatterbrain::parse(&crate::formats::discover::fetch_text(source)?)
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
