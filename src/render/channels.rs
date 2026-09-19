//! Drawing data kept as separate channels, mixed into color on the GPU.
//!
//! An image's channels used to be composited into RGBA as each tile was read,
//! which baked in which channels were shown and how bright: changing either
//! meant reading every visible tile again, a second or two of waiting for a
//! slider. Now a tile keeps each channel's intensity and a shader mixes them
//! from a [`ChannelMix`] uniform, so a change of channels rewrites a few
//! hundred bytes per tile and shows on the next frame. Volumes mix the same
//! way, from the same WGSL.

use bevy::asset::RenderAssetUsages;
use bevy::asset::{Asset, AssetPath, embedded_asset, embedded_path};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::shader::{ShaderRef, load_shader_library};
use bevy::sprite_render::{Material2d, Material2dPlugin};

/// Channels a tile can mix, four to each layer of its texture. Matches the
/// array lengths in `channel_mix.wgsl`.
pub const MAX_CHANNELS: usize = 16;

/// Channels a volume can mix. A 3D texture has no layers to hold more, so a
/// volume keeps its first four; a flat frame of the same image shows them all.
pub const VOLUME_CHANNELS: usize = 4;

/// One channel as the mix needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixChannel {
    pub color: [f32; 3],
    /// Window over the stored intensities: where it starts to show, and where
    /// it reaches full color.
    pub window: (f32, f32),
    pub shown: bool,
}

/// How channels are mixed into color, as the shaders read it.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct ChannelMix {
    pub colors: [Vec4; MAX_CHANNELS],
    pub windows: [Vec4; MAX_CHANNELS],
    pub count: u32,
    pub tint: Vec4,
}

impl Default for ChannelMix {
    fn default() -> Self {
        ChannelMix {
            colors: [Vec4::ZERO; MAX_CHANNELS],
            windows: [Vec4::ZERO; MAX_CHANNELS],
            count: 0,
            tint: Vec4::ONE,
        }
    }
}

impl ChannelMix {
    /// Mix `channels`, keeping the tint this mix already has — that belongs
    /// to the source's transparency, not to its channels.
    pub fn set_channels(&mut self, channels: &[MixChannel]) {
        self.colors = [Vec4::ZERO; MAX_CHANNELS];
        self.windows = [Vec4::ZERO; MAX_CHANNELS];
        for (index, channel) in channels.iter().take(MAX_CHANNELS).enumerate() {
            let [r, g, b] = channel.color;
            self.colors[index] = Vec4::new(r, g, b, if channel.shown { 1.0 } else { 0.0 });
            self.windows[index] = Vec4::new(channel.window.0, channel.window.1, 0.0, 0.0);
        }
        self.count = channels.len().min(MAX_CHANNELS) as u32;
    }

    pub fn of(channels: &[MixChannel]) -> Self {
        let mut mix = ChannelMix::default();
        mix.set_channels(channels);
        mix
    }
}

/// A tile drawn by mixing its channels.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct ChannelTileMaterial {
    #[uniform(0)]
    pub mix: ChannelMix,
    #[texture(1, dimension = "2d_array")]
    #[sampler(2)]
    pub samples: Handle<Image>,
}

fn shader() -> ShaderRef {
    ShaderRef::Path(
        AssetPath::from_path_buf(embedded_path!("channel_tile.wgsl")).with_source("embedded"),
    )
}

impl Material2d for ChannelTileMaterial {
    fn fragment_shader() -> ShaderRef {
        shader()
    }
}

/// Channel intensities as a texture: half floats, four channels to a texel.
///
/// Half floats because tiles are sampled filtered and 16-bit normalised
/// formats are an optional GPU feature; a half keeps three significant
/// figures at any magnitude, which holds a window as narrow as a fiftieth of
/// the stored range. `layers` are array layers for a tile, and slices when
/// `dimension` is 3D.
pub fn channel_texture(
    width: u32,
    height: u32,
    layers: u32,
    data: Vec<u8>,
    dimension: TextureDimension,
) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: layers,
        },
        dimension,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Tiles are drawn as tiles always were: crisp pixels past 1:1, smooth when
    // shrunk. A volume is filtered both ways, so a ray between two sections
    // sees a blend of them rather than a staircase.
    let magnify = if dimension == TextureDimension::D3 {
        ImageFilterMode::Linear
    } else {
        ImageFilterMode::Nearest
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: magnify,
        min_filter: ImageFilterMode::Linear,
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        ..default()
    });
    // One layer would otherwise be viewed as a plain 2D texture, which the
    // shader's array binding rejects.
    if dimension == TextureDimension::D2 {
        image.texture_view_descriptor = Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        });
    }
    image
}

pub struct ChannelRenderPlugin;

impl Plugin for ChannelRenderPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "channel_mix.wgsl");
        embedded_asset!(app, "channel_tile.wgsl");
        app.add_plugins(Material2dPlugin::<ChannelTileMaterial>::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(shown: bool) -> MixChannel {
        MixChannel {
            color: [1.0, 0.5, 0.0],
            window: (0.0, 0.25),
            shown,
        }
    }

    #[test]
    fn a_hidden_channel_is_kept_but_painted_in_nothing() {
        let mix = ChannelMix::of(&[channel(true), channel(false)]);
        assert_eq!(mix.count, 2);
        assert_eq!(mix.colors[0].w, 1.0);
        assert_eq!(mix.colors[1].w, 0.0);
        // Its window is still there for when it is shown again.
        assert_eq!(mix.windows[1].y, 0.25);
    }

    #[test]
    fn changing_channels_leaves_the_transparency_alone() {
        let mut mix = ChannelMix::of(&[channel(true)]);
        mix.tint = Vec4::splat(0.5);
        mix.set_channels(&[channel(false), channel(true), channel(true)]);
        assert_eq!(mix.tint, Vec4::splat(0.5));
        assert_eq!(mix.count, 3);
    }

    #[test]
    fn more_channels_than_a_tile_holds_are_dropped_not_overrun() {
        let many = vec![channel(true); MAX_CHANNELS + 5];
        assert_eq!(ChannelMix::of(&many).count as usize, MAX_CHANNELS);
    }

    #[test]
    fn a_single_layer_tile_is_still_an_array() {
        let image = channel_texture(2, 2, 1, vec![0; 2 * 2 * 8], TextureDimension::D2);
        let view = image.texture_view_descriptor.expect("an explicit view");
        assert_eq!(view.dimension, Some(TextureViewDimension::D2Array));
        assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba16Float);
    }
}
