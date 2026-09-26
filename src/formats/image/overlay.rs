//! Several stores on one grid read as one image: the channels of one specimen
//! written a store apiece, as a Neuroglancer state overlays them.
//!
//! The first store is the image; the rest are its members, whose channels
//! follow its own. A tile or a volume is read from each at the same place
//! and their channels stacked, so everything past the reader mixes them as
//! one store's.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use super::dataset::{
    ChannelSamples, Dataset, Level, TileSource, VolumeRegion, read_tile, read_volume,
};
use crate::render::channels::{MAX_CHANNELS, VOLUME_CHANNELS};

impl Dataset {
    /// One image of `first` and `others`, their channels one after another.
    ///
    /// Only stores that line up pixel for pixel: the same levels, each the
    /// same size and scale, the same slices and the same kind of sample. A
    /// tile is then the same tile of every one of them, which is what lets
    /// their channels be read side by side and mixed as one store's are.
    pub fn overlay(mut first: Dataset, others: Vec<Dataset>) -> Result<Dataset, String> {
        for other in &others {
            first.lines_up_with(other)?;
        }
        let channels =
            first.channels.len() + others.iter().map(|it| it.channels.len()).sum::<usize>();
        if channels > MAX_CHANNELS {
            return Err(format!(
                "{channels} channels between them, more than the {MAX_CHANNELS} one image mixes"
            ));
        }
        for other in others {
            first.channels.extend(other.channels.iter().cloned());
            first.members.push(Arc::new(other));
        }
        Ok(first)
    }

    /// Why `other` cannot be read tile for tile beside this, if it cannot.
    fn lines_up_with(&self, other: &Dataset) -> Result<(), String> {
        let differs =
            |what: &str| Err(format!("{} and {} differ in {what}", self.name, other.name));
        if self.levels.len() != other.levels.len() {
            return differs("how many levels they have");
        }
        if self.depth() != other.depth() {
            return differs("how many slices they have");
        }
        if self.sample_scale != other.sample_scale {
            return differs("the kind of sample they store");
        }
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0);
        for (mine, theirs) in self.levels.iter().zip(&other.levels) {
            if mine.width != theirs.width
                || mine.height != theirs.height
                || mine.tile_px != theirs.tile_px
                || mine.array.shape().len() != theirs.array.shape().len()
                || !close(mine.scale_x, theirs.scale_x)
                || !close(mine.scale_y, theirs.scale_y)
                || !close(mine.origin_x, theirs.origin_x)
                || !close(mine.origin_y, theirs.origin_y)
                || self.level_depth(mine) != other.level_depth(theirs)
            {
                return differs(&format!("level {}", mine.index));
            }
        }
        Ok(())
    }

    /// How many slices `level` holds.
    fn level_depth(&self, level: &Level) -> u64 {
        self.layout.z.map_or(1, |axis| level.array.shape()[axis])
    }
}

impl ChannelSamples {
    /// The channels of `parts` one after another, as if one store held them,
    /// each part holding its own `count` channels.
    ///
    /// Every part covers the same texels. A tile keeps four channels to a
    /// layer, as many layers as it needs; a volume's layers are its slices,
    /// so it keeps the first four channels, one to a component.
    pub(super) fn stacked(parts: &[(ChannelSamples, usize)], volume: bool) -> Self {
        let first = &parts[0].0;
        let total: usize = parts.iter().map(|(_, count)| count).sum();
        let (width, height) = (first.width as usize, first.height as usize);
        let (layers, texels) = if volume {
            let slices = first.layers as usize;
            (slices, width * height * slices)
        } else {
            (total.div_ceil(4).max(1), width * height)
        };
        let place = |channel: usize| {
            if volume {
                (channel < VOLUME_CHANNELS).then_some((0, channel))
            } else {
                Some((channel / 4, channel % 4))
            }
        };
        let mut out = ChannelSamples::zeroed(width, height, layers);
        let mut next = 0;
        for (samples, count) in parts {
            for channel in 0..*count {
                if let (Some((from_layer, from)), Some((to_layer, to))) =
                    (place(channel), place(next))
                {
                    for texel in 0..texels {
                        let value = samples.get(from_layer * texels + texel, from);
                        out.put(to_layer * texels + texel, to, value);
                    }
                }
                next += 1;
            }
        }
        out
    }
}

/// `own`, this store's tile, with every member's tile at the same place
/// stacked after it.
pub(super) async fn beside_members_tile(
    dataset: &Dataset,
    level: &Level,
    own: ChannelSamples,
    ty: u64,
    tx: u64,
    full_z: u64,
) -> Result<ChannelSamples, String> {
    if dataset.members.is_empty() {
        return Ok(own);
    }

    // Every member's tile at the same place, read together. A member is read
    // from its array: the shard decoder above is positioned on this store's
    // shard, not on theirs.
    let reads = dataset.members.iter().map(|member| {
        let level = &member.levels[level.index];
        Box::pin(read_tile(member, level, TileSource::Array, ty, tx, full_z))
    });
    let mut parts = vec![(own, dataset.own_channels())];
    for (member, read) in dataset
        .members
        .iter()
        .zip(futures::future::join_all(reads).await)
    {
        let samples = read?.ok_or_else(|| format!("{} has no tile ({ty},{tx})", member.name))?;
        parts.push((samples, member.channels.len()));
    }
    Ok(ChannelSamples::stacked(&parts, false))
}

/// `own`, this store's volume, with every member's stacked after it.
pub(super) async fn beside_members_volume(
    dataset: &Dataset,
    region: VolumeRegion,
    own: ChannelSamples,
) -> Result<ChannelSamples, String> {
    if dataset.members.is_empty() {
        return Ok(own);
    }
    // A member's slices are counted into a progress of their own: the status
    // line follows this store's, which the members keep pace with.
    let ignored = AtomicU64::new(0);
    let mut parts = vec![(own, dataset.own_channels())];
    for member in &dataset.members {
        let samples = Box::pin(read_volume(member, region, &ignored)).await?;
        parts.push((samples, member.channels.len()));
    }
    Ok(ChannelSamples::stacked(&parts, true))
}
