//! A Deep Zoom image, reduced to what the viewer needs: the pyramid's
//! geometry, where each tile lives, and a way to read one.
//!
//! A `.dzi` is a few lines of XML naming the full size, the tile edge, the
//! overlap and the tile format. Everything else follows from the convention:
//! level `n` is the image scaled to fit `2^n` pixels on its longer side, so the
//! last level is full resolution and level 0 is a single pixel, and tiles live
//! beside the descriptor at `<name>_files/<level>/<column>_<row>.<format>`.
//!
//! Checked against the reference slide rather than the spec alone: an interior
//! tile at full resolution is 514px, 512 plus a pixel of overlap each side; the
//! first column is 513px, and the last one at 15936px wide is 65px — what is
//! left after 31 columns, plus the overlap on its left.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::{Image, default};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::Deserialize;

/// The descriptor as written. Attributes are `@`-prefixed for `quick-xml`;
/// the namespace and anything else a writer adds are ignored.
#[derive(Deserialize)]
struct Descriptor {
    #[serde(rename = "@Format")]
    format: String,
    #[serde(rename = "@Overlap")]
    overlap: u64,
    #[serde(rename = "@TileSize")]
    tile_size: u64,
    #[serde(rename = "Size")]
    size: DescriptorSize,
}

#[derive(Deserialize)]
struct DescriptorSize {
    #[serde(rename = "@Width")]
    width: u64,
    #[serde(rename = "@Height")]
    height: u64,
}

/// The descriptor's contents, and where its tiles are.
#[derive(Debug, Clone)]
pub struct DeepZoom {
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub tile_size: u64,
    pub overlap: u64,
    /// File extension of every tile, as the descriptor spells it.
    pub format: String,
    /// The descriptor's address with its extension replaced by `_files`.
    tiles_base: String,
}

impl DeepZoom {
    /// Parse a descriptor fetched from `source`.
    pub fn parse(source: &str, text: &str) -> Result<Self, String> {
        let Descriptor {
            format,
            overlap,
            tile_size,
            size: DescriptorSize { width, height },
        } = quick_xml::de::from_str(text)
            .map_err(|e| format!("reading the Deep Zoom descriptor at {source}: {e}"))?;
        let format = format.to_ascii_lowercase();

        if width == 0 || height == 0 || tile_size == 0 {
            return Err(format!(
                "the descriptor describes an empty image ({width} x {height}, tiles of {tile_size})"
            ));
        }
        if !matches!(format.as_str(), "jpeg" | "jpg" | "png") {
            return Err(format!("tiles in `{format}` cannot be decoded"));
        }

        let path = source.split(['?', '#']).next().unwrap_or(source);
        let stem = path
            .strip_suffix(".dzi")
            .or_else(|| path.strip_suffix(".xml"));
        let stem = stem.ok_or_else(|| format!("{source} does not end in .dzi"))?;

        Ok(DeepZoom {
            name: stem.rsplit('/').next().unwrap_or(stem).to_string(),
            width,
            height,
            tile_size,
            overlap,
            format,
            tiles_base: format!("{stem}_files"),
        })
    }

    /// The full-resolution level. Levels count up from a single pixel.
    pub fn max_level(&self) -> u32 {
        let longest = self.width.max(self.height);
        u64::BITS - (longest - 1).leading_zeros()
    }

    /// The coarsest level worth drawing: the finest whose image still fits in
    /// one tile. Below it every level is that one tile again, smaller — a
    /// handful of requests for pictures a few pixels across.
    pub fn min_level(&self) -> u32 {
        (0..=self.max_level())
            .rev()
            .find(|&level| {
                let (w, h) = self.level_size(level);
                w <= self.tile_size && h <= self.tile_size
            })
            .unwrap_or(0)
    }

    /// Full-resolution pixels per pixel of `level`.
    pub fn scale(&self, level: u32) -> u64 {
        1 << (self.max_level() - level)
    }

    /// A level's size in its own pixels.
    pub fn level_size(&self, level: u32) -> (u64, u64) {
        let scale = self.scale(level);
        (self.width.div_ceil(scale), self.height.div_ceil(scale))
    }

    /// Columns and rows of tiles in a level.
    pub fn tile_count(&self, level: u32) -> (u64, u64) {
        let (w, h) = self.level_size(level);
        (w.div_ceil(self.tile_size), h.div_ceil(self.tile_size))
    }

    /// The level to draw at a zoom: the coarsest one that still gives each
    /// screen pixel at least a pixel of its own.
    pub fn level_for(&self, pixels_per_screen_px: f32) -> u32 {
        let max = self.max_level();
        let mut level = max;
        while level > self.min_level() && self.scale(level - 1) as f32 <= pixels_per_screen_px {
            level -= 1;
        }
        level
    }

    /// The pixels a tile holds, in its level's coordinates, overlap included,
    /// as `(x0, y0, x1, y1)`. `None` when the tile is off the edge.
    pub fn tile_extent(&self, level: u32, column: u64, row: u64) -> Option<(u64, u64, u64, u64)> {
        let (w, h) = self.level_size(level);
        let span = |index: u64, length: u64| {
            let start = index * self.tile_size;
            (start < length).then(|| {
                (
                    start.saturating_sub(self.overlap),
                    (start + self.tile_size + self.overlap).min(length),
                )
            })
        };
        let (x0, x1) = span(column, w)?;
        let (y0, y1) = span(row, h)?;
        Some((x0, y0, x1, y1))
    }

    /// Where a tile sits in full-resolution pixels, as `(x0, y0, x1, y1)`.
    ///
    /// Overlap included: neighbours carry the same pixels along their shared
    /// edge, so drawing both over it costs nothing and closes the seam that
    /// filtering would otherwise open between them.
    pub fn tile_rect(&self, level: u32, column: u64, row: u64) -> Option<(f32, f32, f32, f32)> {
        let (x0, y0, x1, y1) = self.tile_extent(level, column, row)?;
        let scale = self.scale(level);
        Some((
            (x0 * scale) as f32,
            (y0 * scale) as f32,
            ((x1 * scale).min(self.width)) as f32,
            ((y1 * scale).min(self.height)) as f32,
        ))
    }

    pub fn tile_url(&self, level: u32, column: u64, row: u64) -> String {
        format!("{}/{level}/{column}_{row}.{}", self.tiles_base, self.format)
    }
}

/// A decoded tile ready to be uploaded as a texture.
pub struct TilePixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A tile's pixels as a texture.
pub fn tile_texture(pixels: TilePixels) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: pixels.width,
            height: pixels.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Nearest magnification keeps individual pixels crisp past 1:1; linear
    // minification avoids shimmer when zoomed out. Clamping stops neighbouring
    // tiles bleeding across their seams.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..default()
    });
    image
}

/// Fetch and decode one tile.
pub async fn read_tile(url: &str) -> Result<TilePixels, String> {
    let bytes = crate::app::net::read(url).await?;
    decode(&bytes).map_err(|e| format!("decoding {url}: {e}"))
}

fn decode(bytes: &[u8]) -> Result<TilePixels, String> {
    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let rgba = image.into_rgba8();
    Ok(TilePixels {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "https://bucket.s3.amazonaws.com/slides/H20.33.040-A12-I6-primary.dzi";

    fn reference() -> DeepZoom {
        DeepZoom::parse(SOURCE, include_str!("../../../testdata/deepzoom.dzi")).unwrap()
    }

    #[test]
    fn parses_the_real_descriptor() {
        let dzi = reference();
        assert_eq!((dzi.width, dzi.height), (15936, 11526));
        assert_eq!((dzi.tile_size, dzi.overlap), (512, 1));
        assert_eq!(dzi.format, "jpeg");
        assert_eq!(dzi.name, "H20.33.040-A12-I6-primary");
    }

    #[test]
    fn tiles_live_beside_the_descriptor() {
        assert_eq!(
            reference().tile_url(14, 31, 22),
            "https://bucket.s3.amazonaws.com/slides/H20.33.040-A12-I6-primary_files/14/31_22.jpeg"
        );
    }

    #[test]
    fn the_levels_run_from_one_pixel_to_full_resolution() {
        let dzi = reference();
        // 2^14 = 16384 is the first power of two to hold 15936.
        assert_eq!(dzi.max_level(), 14);
        assert_eq!(dzi.level_size(14), (15936, 11526));
        assert_eq!(dzi.level_size(13), (7968, 5763));
        assert_eq!(dzi.level_size(0), (1, 1));
        // Level 9 is 498 x 361, the first to fit a single tile.
        assert_eq!(dzi.min_level(), 9);
        assert_eq!(dzi.tile_count(14), (32, 23));
        assert_eq!(dzi.tile_count(13), (16, 12));
    }

    #[test]
    fn a_power_of_two_is_its_own_top_level() {
        let dzi = DeepZoom::parse(
            "a.dzi",
            r#"<Image TileSize="256" Overlap="0" Format="png"><Size Width="1024" Height="1"/></Image>"#,
        )
        .unwrap();
        assert_eq!(dzi.max_level(), 10);
        assert_eq!(dzi.level_size(10), (1024, 1));
    }

    #[test]
    fn tile_sizes_match_what_the_store_serves() {
        // Each measured by fetching the tile and reading its JPEG header.
        let dzi = reference();
        let size = |level, column, row| {
            let (x0, y0, x1, y1) = dzi.tile_extent(level, column, row).unwrap();
            (x1 - x0, y1 - y0)
        };
        assert_eq!(size(14, 0, 0), (513, 513));
        assert_eq!(size(14, 1, 1), (514, 514));
        assert_eq!(size(14, 31, 22), (65, 263));
        assert_eq!(size(13, 15, 11), (289, 132));
        assert_eq!(size(9, 0, 0), (498, 361));
        // The store answers 403 for these.
        assert!(dzi.tile_extent(14, 32, 0).is_none());
        assert!(dzi.tile_extent(14, 0, 23).is_none());
    }

    #[test]
    fn a_coarse_tile_covers_the_same_ground_as_the_fine_ones_under_it() {
        let dzi = reference();
        let (x0, y0, x1, y1) = dzi.tile_rect(13, 1, 0).unwrap();
        // Two full-resolution pixels a level-13 pixel, overlap included.
        assert_eq!((x0, y0), (1022.0, 0.0));
        assert_eq!((x1, y1), (2050.0, 1026.0));
        // The last tile stops at the image, not at a whole level-13 pixel.
        let (_, _, x1, y1) = dzi.tile_rect(13, 15, 11).unwrap();
        assert_eq!((x1, y1), (15936.0, 11526.0));
    }

    #[test]
    fn the_level_drawn_is_the_coarsest_that_still_resolves_the_view() {
        let dzi = reference();
        // Zoomed in past 1:1, only full resolution will do.
        assert_eq!(dzi.level_for(0.25), 14);
        assert_eq!(dzi.level_for(1.0), 14);
        assert_eq!(dzi.level_for(2.0), 13);
        assert_eq!(dzi.level_for(3.0), 13);
        assert_eq!(dzi.level_for(4.0), 12);
        // Zoomed far out, never below the level that fits in one tile.
        assert_eq!(dzi.level_for(1.0e6), 9);
    }

    #[test]
    fn a_descriptor_is_read_whatever_the_quoting_and_extra_attributes() {
        let dzi = DeepZoom::parse(
            "a.dzi",
            r#"<Image xmlns="x" Format = 'PNG' Url="elsewhere/" TileSize="256" Overlap="0">
                 <Size Width="300" Height="200"/>
               </Image>"#,
        )
        .unwrap();
        assert_eq!(dzi.format, "png");
        assert_eq!((dzi.tile_size, dzi.width, dzi.height), (256, 300, 200));
    }

    #[test]
    fn a_descriptor_that_is_not_one_says_so() {
        assert!(DeepZoom::parse("a.dzi", "<html></html>").is_err());
        assert!(
            DeepZoom::parse(
                "a.dzi",
                r#"<Image TileSize="256" Overlap="0" Format="webp"><Size Width="1" Height="1"/></Image>"#
            )
            .is_err()
        );
    }

    #[test]
    fn a_tile_decodes_to_rgba() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let pixels = decode(&png).unwrap();
        assert_eq!((pixels.width, pixels.height), (3, 2));
        assert_eq!(&pixels.rgba[..4], &[10, 20, 30, 255]);
    }
}
