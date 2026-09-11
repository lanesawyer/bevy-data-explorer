//! An OME-Zarr multiscale image, opened and reduced to what the viewer needs:
//! a resolution pyramid in a shared world coordinate system, the channel
//! display settings, and a way to read one tile.

use std::sync::Arc;

use ome_zarr_metadata::v0_4::AxisType;

use crate::source::MultiscaleSpec;
use zarrs::array::{Array, ArraySubset, ChunkShapeTraits};
use zarrs::storage::ReadableStorageTraits;
use zarrs_codec::ArrayPartialDecoderTraits;

pub type ReadStore = Arc<dyn ReadableStorageTraits>;
pub type SharedArray = Array<dyn ReadableStorageTraits>;

/// Preferred tile edge in pixels.
///
/// Tiles are cut from a shard rather than matching its inner chunks, because
/// the HTTP store batches the inner-chunk byte ranges behind a single request.
/// Measured against the reference store, covering a 1024x1024 region costs the
/// same bytes at every tile size but ten times the wall clock at 128px versus
/// 1024px, purely in round trips. 512 keeps requests few without making the
/// first paint wait on a large decode.
pub const TARGET_TILE_PX: u64 = 512;

/// Which array axis carries which meaning.
#[derive(Debug, Clone, Copy)]
pub struct AxisLayout {
    pub ndim: usize,
    pub x: usize,
    pub y: usize,
    pub c: Option<usize>,
    pub z: Option<usize>,
}

impl AxisLayout {
    fn infer(axes: &[ome_zarr_metadata::v0_4::Axis]) -> Result<Self, String> {
        let named = |want: &str| axes.iter().position(|a| a.name.eq_ignore_ascii_case(want));
        let spatial: Vec<usize> = axes
            .iter()
            .enumerate()
            .filter(|(_, a)| matches!(a.r#type, Some(AxisType::Space)))
            .map(|(i, _)| i)
            .collect();

        // Prefer explicit names; fall back to the last two spatial axes, which
        // the spec requires to be ordered `.., y, x`.
        let x = named("x")
            .or_else(|| spatial.last().copied())
            .ok_or("could not identify an x axis")?;
        let y = named("y")
            .or_else(|| spatial.iter().rev().nth(1).copied())
            .ok_or("could not identify a y axis")?;
        if x == y {
            return Err("x and y resolved to the same axis".into());
        }

        let c = axes
            .iter()
            .position(|a| matches!(a.r#type, Some(AxisType::Channel)))
            .or_else(|| named("c"));
        let z = named("z").filter(|&i| i != x && i != y);

        Ok(AxisLayout {
            ndim: axes.len(),
            x,
            y,
            c,
            z,
        })
    }
}

/// A channel's display mapping: intensity window and tint.
#[derive(Debug, Clone)]
pub struct Channel {
    pub label: String,
    pub color: [f32; 3],
    pub start: f32,
    pub end: f32,
    pub active: bool,
}

impl Channel {
    /// Map a raw intensity to 0..1 across the channel's window.
    #[inline]
    fn normalize(&self, value: f32) -> f32 {
        let span = self.end - self.start;
        if span.abs() < f32::EPSILON {
            return 0.0;
        }
        ((value - self.start) / span).clamp(0.0, 1.0)
    }
}

/// One resolution level of the pyramid.
pub struct Level {
    pub index: usize,
    pub path: String,
    pub array: SharedArray,
    /// Level size in pixels.
    pub width: u64,
    pub height: u64,
    /// World units per pixel at this level, from the level's scale transform.
    pub scale_x: f64,
    pub scale_y: f64,
    /// World-space origin of the level, from its translation transform.
    pub origin_x: f64,
    pub origin_y: f64,
    /// Tile edge in level pixels.
    pub tile_px: u64,
    /// Top-level chunk (shard) extent in level pixels, along y and x.
    pub shard_y_px: u64,
    pub shard_x_px: u64,
    pub tiles_x: u64,
    pub tiles_y: u64,
}

impl Level {
    /// Tiles per shard along each axis. Tile size always divides the shard, so
    /// a tile never straddles two shards and can be served by one decoder.
    fn tiles_per_shard(&self) -> (u64, u64) {
        (
            self.shard_y_px / self.tile_px,
            self.shard_x_px / self.tile_px,
        )
    }

    /// The top-level chunk holding tile `(ty, tx)`, in full array coordinates.
    pub fn shard_of(&self, layout: &AxisLayout, ty: u64, tx: u64, z: u64) -> Vec<u64> {
        let (per_y, per_x) = self.tiles_per_shard();
        let mut indices = vec![0u64; layout.ndim];
        indices[layout.y] = ty / per_y;
        indices[layout.x] = tx / per_x;
        if let Some(zi) = layout.z {
            let chunk_z = self
                .array
                .chunk_shape(&vec![0; layout.ndim])
                .map_or(1, |s| s.to_array_shape().get(zi).copied().unwrap_or(1));
            indices[zi] = z / chunk_z.max(1);
        }
        indices
    }

    /// Pixel extent of a tile, clipped to the level. Returns `None` when the
    /// tile lies entirely outside the image.
    pub fn tile_extent(&self, ty: u64, tx: u64) -> Option<(u64, u64, u64, u64)> {
        let x0 = tx * self.tile_px;
        let y0 = ty * self.tile_px;
        if x0 >= self.width || y0 >= self.height {
            return None;
        }
        let x1 = (x0 + self.tile_px).min(self.width);
        let y1 = (y0 + self.tile_px).min(self.height);
        Some((x0, y0, x1, y1))
    }

    /// World-space rectangle covered by a tile, as `(min_x, min_y, max_x, max_y)`
    /// with y increasing downward in image space.
    pub fn tile_world_rect(&self, ty: u64, tx: u64) -> Option<(f32, f32, f32, f32)> {
        let (x0, y0, x1, y1) = self.tile_extent(ty, tx)?;
        Some((
            (self.origin_x + x0 as f64 * self.scale_x) as f32,
            (self.origin_y + y0 as f64 * self.scale_y) as f32,
            (self.origin_x + x1 as f64 * self.scale_x) as f32,
            (self.origin_y + y1 as f64 * self.scale_y) as f32,
        ))
    }
}

/// A decoded tile ready to be uploaded as a texture.
pub struct TilePixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct Dataset {
    pub name: String,
    /// Finest level first.
    pub levels: Vec<Level>,
    pub channels: Vec<Channel>,
    pub layout: AxisLayout,
    /// Physical unit of the spatial axes, for display.
    pub unit: String,
    /// Full image extent in world units.
    pub world: (f32, f32, f32, f32),
}

impl Dataset {
    pub fn open(
        store: ReadStore,
        multiscale: &MultiscaleSpec,
        omero: Option<&ome_zarr_metadata::v0_4::Omero>,
    ) -> Result<Self, String> {
        let layout = AxisLayout::infer(&multiscale.axes)?;

        let unit = multiscale
            .axes
            .get(layout.x)
            .and_then(|a| a.unit.as_ref())
            .map(unit_symbol)
            .unwrap_or_else(|| "px".to_string());

        let mut levels = Vec::new();
        for (index, dataset) in multiscale.datasets.iter().enumerate() {
            let path = format!("/{}", dataset.path.trim_matches('/'));
            let array = Array::open(store.clone(), &path)
                .map_err(|e| format!("opening array `{path}`: {e}"))?;

            let shape = array.shape().to_vec();
            if shape.len() != layout.ndim {
                return Err(format!(
                    "array `{path}` has {} dimensions but the axes list has {}",
                    shape.len(),
                    layout.ndim
                ));
            }

            let (scale, translation) = transforms(&dataset.coordinate_transformations, layout.ndim);
            let chunk = array
                .chunk_shape(&vec![0; layout.ndim])
                .map_err(|e| format!("chunk shape of `{path}`: {e}"))?
                .to_array_shape();
            let inner = inner_chunk_shape(&array).unwrap_or_else(|| chunk.clone());

            let tile_px = choose_tile_px(
                inner[layout.y].min(inner[layout.x]),
                chunk[layout.y].min(chunk[layout.x]),
                TARGET_TILE_PX,
            );

            let width = shape[layout.x];
            let height = shape[layout.y];
            levels.push(Level {
                index,
                path,
                array,
                width,
                height,
                scale_x: scale[layout.x],
                scale_y: scale[layout.y],
                origin_x: translation[layout.x],
                origin_y: translation[layout.y],
                tile_px,
                shard_y_px: chunk[layout.y],
                shard_x_px: chunk[layout.x],
                tiles_x: width.div_ceil(tile_px),
                tiles_y: height.div_ceil(tile_px),
            });
        }

        if levels.is_empty() {
            return Err("multiscale image lists no datasets".into());
        }
        // The spec orders datasets finest first, but do not rely on it.
        levels.sort_by(|a, b| b.width.cmp(&a.width));
        for (i, level) in levels.iter_mut().enumerate() {
            level.index = i;
        }

        let finest = &levels[0];
        let world = (
            finest.origin_x as f32,
            finest.origin_y as f32,
            (finest.origin_x + finest.width as f64 * finest.scale_x) as f32,
            (finest.origin_y + finest.height as f64 * finest.scale_y) as f32,
        );

        let channel_count = layout
            .c
            .map(|c| finest.array.shape()[c] as usize)
            .unwrap_or(1);
        let channels = build_channels(omero, channel_count, &finest.array);

        Ok(Dataset {
            name: multiscale.name.clone().unwrap_or_else(|| "image".into()),
            levels,
            channels,
            layout,
            unit,
            world,
        })
    }

    /// Pick the coarsest level that still resolves `world_units_per_screen_px`,
    /// so that one level pixel covers at most one screen pixel.
    pub fn level_for(&self, world_units_per_screen_px: f32) -> usize {
        let mut chosen = 0;
        for (i, level) in self.levels.iter().enumerate() {
            if level.scale_x as f32 <= world_units_per_screen_px {
                chosen = i;
            }
        }
        chosen
    }
}

/// Read a tile through a decoder already positioned on its shard, compositing
/// the active channels into RGBA.
pub fn read_tile(
    dataset: &Dataset,
    level: &Level,
    channels: &[Channel],
    decoder: &dyn ArrayPartialDecoderTraits,
    ty: u64,
    tx: u64,
    z: u64,
) -> Result<Option<TilePixels>, String> {
    let layout = &dataset.layout;
    let Some((x0, y0, x1, y1)) = level.tile_extent(ty, tx) else {
        return Ok(None);
    };
    let (per_y, per_x) = (
        level.shard_y_px / level.tile_px,
        level.shard_x_px / level.tile_px,
    );
    let shard_y = ty / per_y;
    let shard_x = tx / per_x;

    // Subsets handed to a partial decoder are relative to its chunk.
    let rel_y = y0 - shard_y * level.shard_y_px;
    let rel_x = x0 - shard_x * level.shard_x_px;
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);

    let mut accum = vec![0f32; w * h * 3];
    let channel_chunk = layout
        .c
        .map(|c| {
            level
                .array
                .chunk_shape(&vec![0; layout.ndim])
                .map(|s| s.to_array_shape()[c])
                .unwrap_or(1)
        })
        .unwrap_or(1)
        .max(1);

    // Channels sharing a chunk are fetched together; the reference store keeps
    // all of them in one chunk, so this is normally a single read.
    let mut groups: Vec<(u64, Vec<usize>)> = Vec::new();
    for (ci, _) in channels.iter().enumerate().filter(|(_, c)| c.active) {
        let group = ci as u64 / channel_chunk;
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some((_, list)) => list.push(ci),
            None => groups.push((group, vec![ci])),
        }
    }
    if groups.is_empty() {
        return Ok(None);
    }

    for (group, members) in groups {
        let mut ranges = vec![0..1u64; layout.ndim];
        ranges[layout.y] = rel_y..rel_y + h as u64;
        ranges[layout.x] = rel_x..rel_x + w as u64;
        let group_base = group * channel_chunk;
        if let Some(c) = layout.c {
            let count = channel_chunk.min(level.array.shape()[c] - group_base);
            ranges[c] = 0..count;
        }
        if let Some(zi) = layout.z {
            let chunk_z = level
                .array
                .chunk_shape(&vec![0; layout.ndim])
                .map(|s| s.to_array_shape()[zi])
                .unwrap_or(1)
                .max(1);
            let rel_z = z % chunk_z;
            ranges[zi] = rel_z..rel_z + 1;
        }

        let subset = ArraySubset::new_with_ranges(&ranges);
        let bytes = decoder
            .partial_decode(&subset, &Default::default())
            .map_err(|e| format!("decoding tile ({ty},{tx}) of {}: {e}", level.path))?;
        let raw = bytes
            .into_fixed()
            .map_err(|_| "variable-length data types are not supported".to_string())?;

        let element = element_size(&level.array)?;
        let plane = w * h;
        for &ci in &members {
            let within = ci as u64 - group_base;
            let offset = within as usize * plane * element;
            let channel = &channels[ci];
            composite(
                &raw[offset..offset + plane * element],
                &mut accum,
                channel,
                &level.array,
            )?;
        }
    }

    let mut rgba = vec![0u8; w * h * 4];
    for (pixel, out) in accum.chunks_exact(3).zip(rgba.chunks_exact_mut(4)) {
        out[0] = (pixel[0].min(1.0) * 255.0) as u8;
        out[1] = (pixel[1].min(1.0) * 255.0) as u8;
        out[2] = (pixel[2].min(1.0) * 255.0) as u8;
        out[3] = 255;
    }

    Ok(Some(TilePixels {
        width: w as u32,
        height: h as u32,
        rgba,
    }))
}

/// Add one channel's contribution to the running RGB accumulation.
fn composite(
    raw: &[u8],
    accum: &mut [f32],
    channel: &Channel,
    array: &SharedArray,
) -> Result<(), String> {
    let name = data_type_name(array);
    macro_rules! blend {
        ($ty:ty, $size:expr) => {{
            for (i, chunk) in raw.chunks_exact($size).enumerate() {
                let value = <$ty>::from_ne_bytes(chunk.try_into().unwrap()) as f32;
                let t = channel.normalize(value);
                if t > 0.0 {
                    let out = &mut accum[i * 3..i * 3 + 3];
                    out[0] += channel.color[0] * t;
                    out[1] += channel.color[1] * t;
                    out[2] += channel.color[2] * t;
                }
            }
        }};
    }
    match name.as_str() {
        "uint8" => blend!(u8, 1),
        "int8" => blend!(i8, 1),
        "uint16" => blend!(u16, 2),
        "int16" => blend!(i16, 2),
        "uint32" => blend!(u32, 4),
        "int32" => blend!(i32, 4),
        "float32" => blend!(f32, 4),
        "float64" => blend!(f64, 8),
        other => return Err(format!("unsupported data type `{other}`")),
    }
    Ok(())
}

fn data_type_name(array: &SharedArray) -> String {
    use zarrs_plugin::ExtensionName;
    array
        .data_type()
        .name(zarrs_plugin::ZarrVersion::V3)
        .map(|n| n.to_string())
        .unwrap_or_default()
}

fn element_size(array: &SharedArray) -> Result<usize, String> {
    array
        .data_type()
        .fixed_size()
        .ok_or_else(|| "variable-length data types are not supported".to_string())
}

/// Default display window when omero metadata is absent.
fn nominal_range(array: &SharedArray) -> (f32, f32) {
    match data_type_name(array).as_str() {
        "uint8" | "int8" => (0.0, 255.0),
        "uint16" | "int16" => (0.0, 65535.0),
        _ => (0.0, 1.0),
    }
}

fn build_channels(
    omero: Option<&ome_zarr_metadata::v0_4::Omero>,
    count: usize,
    array: &SharedArray,
) -> Vec<Channel> {
    // Fallbacks when a channel carries no colour: grey for a single channel,
    // then the usual RGB assignment.
    const FALLBACK: [[f32; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
    ];
    let (lo, hi) = nominal_range(array);

    (0..count)
        .map(|i| {
            let meta = omero.and_then(|o| o.channels.get(i));
            let color = meta
                .map(|c| {
                    [
                        c.color.r as f32 / 255.0,
                        c.color.g as f32 / 255.0,
                        c.color.b as f32 / 255.0,
                    ]
                })
                .filter(|c| c.iter().any(|v| *v > 0.0))
                .unwrap_or(if count == 1 {
                    [1.0; 3]
                } else {
                    FALLBACK[i % 6]
                });

            let (start, end) = meta
                .map(|c| (c.window.start as f32, c.window.end as f32))
                .filter(|(s, e)| e > s)
                .unwrap_or((lo, hi));

            let label = meta
                .and_then(|c| c.other.get("label"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("channel {i}"));

            let active = meta
                .and_then(|c| c.other.get("active"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            Channel {
                label,
                color,
                start,
                end,
                active,
            }
        })
        .collect()
}

/// Collapse a level's coordinate transformations into a scale and translation.
fn transforms(
    list: &[ome_zarr_metadata::v0_4::CoordinateTransform],
    ndim: usize,
) -> (Vec<f64>, Vec<f64>) {
    use ome_zarr_metadata::v0_4::{
        CoordinateTransform as T, CoordinateTransformScale as S,
        CoordinateTransformTranslation as Tr,
    };
    let mut scale = vec![1.0; ndim];
    let mut translation = vec![0.0; ndim];
    for transform in list {
        match transform {
            T::Scale(S::List { scale: values }) => {
                for (i, v) in values.iter().take(ndim).enumerate() {
                    scale[i] = *v as f64;
                }
            }
            T::Translation(Tr::List {
                translation: values,
            }) => {
                for (i, v) in values.iter().take(ndim).enumerate() {
                    translation[i] = *v as f64;
                }
            }
            // Transforms stored out-of-band are not read; the identity default
            // keeps the level positioned rather than failing the whole open.
            _ => {}
        }
    }
    (scale, translation)
}

/// A short display name for a physical axis unit.
///
/// The unit round-trips through serde to get its spec name rather than its
/// Rust debug form, which would read `Space(Micrometer)`.
fn unit_symbol(unit: &ome_zarr_metadata::v0_4::AxisUnit) -> String {
    let name = serde_json::to_value(unit)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "px".to_string());
    match name.as_str() {
        "nanometer" => "nm".into(),
        "micrometer" => "um".into(),
        "millimeter" => "mm".into(),
        "centimeter" => "cm".into(),
        "meter" => "m".into(),
        other => other.to_string(),
    }
}

/// Pull the inner chunk shape out of a `sharding_indexed` codec, if present.
fn inner_chunk_shape(array: &SharedArray) -> Option<Vec<u64>> {
    let metadata = serde_json::to_value(array.metadata()).ok()?;
    let codecs = metadata.get("codecs")?.as_array()?;
    let sharding = codecs
        .iter()
        .find(|c| c.get("name").and_then(|n| n.as_str()) == Some("sharding_indexed"))?;
    serde_json::from_value(sharding.get("configuration")?.get("chunk_shape")?.clone()).ok()
}

/// Choose a tile edge that divides the shard exactly, so a tile never straddles
/// two shards and can always be served by a single decoder.
///
/// Multiples of the inner chunk are preferred, since those avoid decoding
/// inner chunks only to throw part of them away. When no such multiple divides
/// the shard, exact tiling wins over alignment.
fn choose_tile_px(inner: u64, shard: u64, target: u64) -> u64 {
    let inner = inner.max(1).min(shard);
    let target = target.max(1).min(shard);

    let aligned = (1..=target / inner)
        .map(|k| k * inner)
        .filter(|size| shard % size == 0)
        .max();
    if let Some(size) = aligned {
        return size;
    }

    // Fall back to the largest divisor of the shard within the target.
    (1..=target)
        .rev()
        .find(|size| shard % size == 0)
        .unwrap_or(shard)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference image's root attributes, parsed the way the viewer does.
    fn reference_multiscale() -> Vec<crate::source::MultiscaleSpec> {
        let root: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/root_zarr_v3.json")).unwrap();
        crate::source::parse_ome(&root["attributes"]).unwrap().0
    }

    #[test]
    fn tile_size_divides_the_shard_and_respects_the_target() {
        // The reference store: 128px inner chunks inside 4096px shards.
        assert_eq!(choose_tile_px(128, 4096, 512), 512);
        assert_eq!(choose_tile_px(128, 4096, 1024), 1024);
        // Never exceed the shard, even when the target is larger.
        assert_eq!(choose_tile_px(128, 256, 512), 256);
        // Unsharded arrays tile at the chunk itself.
        assert_eq!(choose_tile_px(64, 64, 512), 64);
        // An inner chunk that does not divide the shard still yields exact
        // tiling, because a tile spanning two shards could not be decoded.
        assert_eq!(4096 % choose_tile_px(100, 4096, 512), 0);
        assert_eq!(4096 % choose_tile_px(384, 4096, 512), 0);
        assert_eq!(1000 % choose_tile_px(128, 1000, 512), 0);
    }

    #[test]
    fn axis_units_display_as_symbols() {
        let ms = &reference_multiscale()[0];
        let unit = ms.axes[3].unit.as_ref().map(unit_symbol).unwrap();
        assert_eq!(unit, "um");
    }

    #[test]
    fn channel_window_maps_onto_zero_to_one() {
        let c = Channel {
            label: "t".into(),
            color: [1.0; 3],
            start: 10.0,
            end: 20.0,
            active: true,
        };
        assert_eq!(c.normalize(10.0), 0.0);
        assert_eq!(c.normalize(15.0), 0.5);
        assert_eq!(c.normalize(20.0), 1.0);
        // Values outside the window clamp rather than wrapping or going negative.
        assert_eq!(c.normalize(0.0), 0.0);
        assert_eq!(c.normalize(1000.0), 1.0);
    }

    #[test]
    fn a_degenerate_window_does_not_divide_by_zero() {
        let c = Channel {
            label: "t".into(),
            color: [1.0; 3],
            start: 5.0,
            end: 5.0,
            active: true,
        };
        assert_eq!(c.normalize(5.0), 0.0);
    }

    #[test]
    fn axis_layout_comes_from_the_real_metadata() {
        let ms = &reference_multiscale()[0];
        let layout = AxisLayout::infer(&ms.axes).unwrap();
        assert_eq!(layout.ndim, 4);
        assert_eq!(layout.c, Some(0));
        assert_eq!(layout.z, Some(1));
        assert_eq!(layout.y, 2);
        assert_eq!(layout.x, 3);
    }

    #[test]
    fn scale_transform_is_read_per_level() {
        let ms = &reference_multiscale()[0];
        let (scale, translation) = transforms(&ms.datasets[0].coordinate_transformations, 4);
        assert!((scale[3] - 0.27381).abs() < 1e-6);
        assert_eq!(translation, vec![0.0; 4]);

        // Level 1 is close to but not exactly twice level 0, which is why the
        // viewer positions levels by scale rather than by a power of two.
        let (scale1, _) = transforms(&ms.datasets[1].coordinate_transformations, 4);
        assert!(scale1[3] > scale[3] * 1.99 && scale1[3] < scale[3] * 2.01);
        assert_ne!(scale1[3], scale[3] * 2.0);
    }

    #[test]
    fn inner_chunk_shape_is_found_in_the_sharding_codec() {
        let meta: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/array0_zarr_v3.json")).unwrap();
        let codecs = meta["codecs"].as_array().unwrap();
        let sharding = codecs
            .iter()
            .find(|c| c["name"] == "sharding_indexed")
            .unwrap();
        let inner: Vec<u64> =
            serde_json::from_value(sharding["configuration"]["chunk_shape"].clone()).unwrap();
        assert_eq!(inner, vec![3, 1, 128, 128]);
    }
}
