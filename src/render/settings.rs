//! A source's display settings — how opaque it is, how large its points are,
//! which category is picked out — pushed into whatever draws it.
//!
//! Works off the render layer rather than asking each format plugin to apply
//! them, so a new format is faded without writing any code for it. Each system
//! writes when a setting changes and to geometry just spawned, because tiles
//! and octree nodes stream in continuously and anything spawned since the last
//! change would otherwise appear at the defaults.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::app::schedule::Stage;
use crate::render::channels::ChannelTileMaterial;
use crate::render::lines::LineMaterial;
use crate::render::points::{PointMaterial, SourceHighlight, SourcePointSize};
use crate::render::volume::VolumeMaterial;
use crate::source::DataSource;
use crate::source::volume::SourceVolume;

/// How opaque a source's geometry is drawn, on its own entity so that two
/// sources can be faded independently.
#[derive(Component, Clone, Copy)]
pub struct SourceOpacity(pub f32);

impl Default for SourceOpacity {
    fn default() -> Self {
        SourceOpacity(1.0)
    }
}

/// The tint that fades geometry to `opacity`.
///
/// Fading dims the colour rather than lowering alpha, because alpha compounds
/// with overdraw. A dense point cloud stacks dozens of points on a pixel, and
/// `1 - (1 - a)^n` reaches 99% by eight layers, so an alpha of 0.5 left the
/// sections looking untouched and nothing appeared to happen until roughly 0.1.
/// Dimming fades a layer uniformly however many times it overdraws, and against
/// a dark background looks the same as a single transparent layer would.
///
/// The factor is applied in sRGB so the slider reads perceptually: halfway
/// along looks half as bright.
fn fade_tint(opacity: f32) -> Color {
    let f = opacity.clamp(0.0, 1.0);
    Color::srgb(f, f, f)
}

/// [`fade_tint`] as the `vec4` the shaders multiply by.
fn fade_uniform(opacity: f32) -> Vec4 {
    let tint = fade_tint(opacity).to_linear();
    Vec4::new(tint.red, tint.green, tint.blue, tint.alpha)
}

/// Fade a source's sprites and plain meshes to its opacity.
fn apply_opacity(
    sources: Query<(&DataSource, Ref<SourceOpacity>)>,
    mut sprites: Query<(&RenderLayers, &mut Sprite)>,
    meshes: Query<(&RenderLayers, Ref<MeshMaterial2d<ColorMaterial>>)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (source, opacity) in &sources {
        // New geometry already draws at full opacity, and leaving it alone
        // leaves its own colour alone.
        let fresh = |added: bool| added && opacity.0 < 1.0;
        let layer = RenderLayers::layer(source.layer);
        let tint = fade_tint(opacity.0);

        for (layers, mut sprite) in &mut sprites {
            if *layers == layer && (opacity.is_changed() || fresh(sprite.is_added())) {
                sprite.color = tint;
            }
        }
        for (layers, material) in &meshes {
            if *layers == layer
                && (opacity.is_changed() || fresh(material.is_added()))
                && let Some(mut material) = materials.get_mut(&material.0)
            {
                material.color = tint;
            }
        }
    }
}

/// Push a source's point size, fade and highlight into the materials drawing it.
fn apply_point_settings(
    sources: Query<(
        &DataSource,
        Ref<SourcePointSize>,
        Option<Ref<SourceOpacity>>,
        Option<Ref<SourceHighlight>>,
    )>,
    meshes: Query<(&RenderLayers, Ref<MeshMaterial2d<PointMaterial>>)>,
    mut materials: ResMut<Assets<PointMaterial>>,
) {
    for (source, size, opacity, highlight) in &sources {
        let changed = size.is_changed()
            || opacity.as_ref().is_some_and(DetectChanges::is_changed)
            || highlight.as_ref().is_some_and(DetectChanges::is_changed);
        let layer = RenderLayers::layer(source.layer);
        let tint = fade_uniform(opacity.map_or(1.0, |o| o.0));
        let highlight = highlight.map_or(crate::render::points::HIGHLIGHT_NONE, |h| h.uniform());

        for (layers, material) in &meshes {
            if *layers == layer
                && (changed || material.is_added())
                && let Some(mut material) = materials.get_mut(&material.0)
            {
                material.settings.size = size.0;
                material.settings.highlight = highlight;
                material.settings.tint = tint;
            }
        }
    }
}

/// Push a source's opacity into the outlines drawing it.
///
/// Lowers alpha rather than dimming, unlike every other kind of geometry: an
/// outline barely overdraws itself, so the reason dimming exists does not
/// apply, and an outline layered over a pale slide should let the slide show
/// through rather than turn darker.
fn apply_line_opacity(
    sources: Query<(&DataSource, Ref<SourceOpacity>)>,
    meshes: Query<(&RenderLayers, Ref<MeshMaterial2d<LineMaterial>>)>,
    mut materials: ResMut<Assets<LineMaterial>>,
) {
    for (source, opacity) in &sources {
        let layer = RenderLayers::layer(source.layer);
        for (layers, material) in &meshes {
            if *layers != layer || !(opacity.is_changed() || material.is_added()) {
                continue;
            }
            if let Some(material) = materials.get_mut(&material.0).as_mut() {
                material.settings.tint = Vec4::new(1.0, 1.0, 1.0, opacity.0.clamp(0.0, 1.0));
            }
        }
    }
}

/// Push a source's opacity into whatever mixes its channels: its image tiles,
/// on the source's own layer, and its volume, on the volume's.
///
/// Dims, as every other fade here does, by the tint the shaders multiply the
/// mixed colour by. Mixing owns the rest of the uniform, so only the tint is
/// written.
fn apply_channel_opacity(
    sources: Query<(&DataSource, Ref<SourceOpacity>, Option<&SourceVolume>)>,
    tiles: Query<(&RenderLayers, Ref<MeshMaterial2d<ChannelTileMaterial>>)>,
    volumes: Query<(&RenderLayers, Ref<MeshMaterial2d<VolumeMaterial>>)>,
    mut tile_materials: ResMut<Assets<ChannelTileMaterial>>,
    mut volume_materials: ResMut<Assets<VolumeMaterial>>,
) {
    for (source, opacity, volume) in &sources {
        let tint = fade_uniform(opacity.0);
        let flat = RenderLayers::layer(source.layer);
        for (layers, material) in &tiles {
            if *layers != flat || !(opacity.is_changed() || material.is_added()) {
                continue;
            }
            if let Some(mut material) = tile_materials.get_mut(&material.0)
                && material.mix.tint != tint
            {
                material.mix.tint = tint;
            }
        }
        let Some(volume) = volume else { continue };
        let deep = RenderLayers::layer(volume.layer);
        for (layers, material) in &volumes {
            if *layers != deep || !(opacity.is_changed() || material.is_added()) {
                continue;
            }
            if let Some(mut material) = volume_materials.get_mut(&material.0)
                && material.mix.tint != tint
            {
                material.mix.tint = tint;
            }
        }
    }
}

pub struct SourceSettingsPlugin;

impl Plugin for SourceSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                apply_opacity,
                apply_point_settings,
                apply_line_opacity,
                apply_channel_opacity,
            )
                .in_set(Stage::ControlsApply),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brightness(colour: Color) -> f32 {
        colour.to_srgba().red
    }

    #[test]
    fn full_opacity_leaves_the_colour_untouched() {
        // The tint multiplies the source colour, so white is a no-op.
        assert_eq!(brightness(fade_tint(1.0)), 1.0);
        assert_eq!(fade_tint(1.0).alpha(), 1.0);
    }

    #[test]
    fn fading_dims_rather_than_going_transparent() {
        // Alpha stays at one at every setting: transparency compounds with
        // overdraw, which is the bug this replaced.
        for opacity in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_eq!(fade_tint(opacity).alpha(), 1.0);
            assert_eq!(fade_uniform(opacity).w, 1.0);
        }
    }

    #[test]
    fn the_slider_reads_perceptually() {
        // Halfway along the slider should look about half as bright, which is
        // a factor of one half in sRGB rather than in linear light.
        assert!((brightness(fade_tint(0.5)) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn brightness_rises_with_opacity() {
        let mut previous = -1.0;
        for step in 0..=10 {
            let value = brightness(fade_tint(step as f32 / 10.0));
            assert!(value > previous, "step {step} did not brighten");
            previous = value;
        }
    }

    #[test]
    fn out_of_range_values_clamp() {
        assert_eq!(brightness(fade_tint(-1.0)), 0.0);
        assert_eq!(brightness(fade_tint(4.0)), 1.0);
    }
}
