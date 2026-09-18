//! Rendering for volumes: a stack of slices drawn in depth.
//!
//! A volume is one box whose shader marches each pixel's view ray through a 3D
//! texture of the composited stack; see `volume_material.wgsl`. It is a 2D mesh
//! for the same reason a frame is a 2D camera: a frame looking at a volume keeps
//! its own camera and trades its projection for a perspective one, so the grid's
//! draw order and its one clearing camera are untouched by a frame going 3D.

use bevy::asset::{Asset, AssetPath, embedded_asset, embedded_path};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendState, Face, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError,
};

use super::channels::ChannelMix;
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};

/// Samples a ray may take, however long it is.
///
/// The step is half a voxel, so this is enough for a volume about a thousand
/// voxels through its diagonal before rays start skipping voxels; the cost is
/// per pixel of the frame, which is why there is a ceiling at all.
pub const MAX_RAY_STEPS: u32 = 1024;

/// A volume: the whole stack coarsely, and optionally part of it finely.
///
/// The detail is one box inside the whole, read at a finer level for what the
/// frames looking at the volume can see. Rays sample it wherever they are
/// inside that box and the whole everywhere else, so zooming in sharpens what
/// is in view without the rest of the specimen going missing. With no detail
/// the whole is bound in its place, behind an empty box no ray is ever inside.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct VolumeMaterial {
    #[uniform(0)]
    pub settings: VolumeSettings,
    #[texture(1, dimension = "3d")]
    #[sampler(2)]
    pub voxels: Handle<Image>,
    #[texture(3, dimension = "3d")]
    #[sampler(4)]
    pub detail: Handle<Image>,
    /// How the stored channels are mixed, the same as the source's tiles.
    #[uniform(5)]
    pub mix: ChannelMix,
}

#[derive(Clone, Copy, ShaderType)]
pub struct VolumeSettings {
    pub box_min: Vec4,
    pub box_max: Vec4,
    pub detail_min: Vec4,
    pub detail_max: Vec4,
    pub step: f32,
    pub max_steps: u32,
}

/// Half the finest voxel of a texture of `dimensions` filling `min` to `max`.
fn half_voxel(min: Vec3, max: Vec3, dimensions: UVec3) -> f32 {
    let voxel = (max - min) / dimensions.max(UVec3::ONE).as_vec3();
    (voxel.min_element() * 0.5).max(f32::MIN_POSITIVE)
}

impl VolumeMaterial {
    /// A volume filling the box from `min` to `max`, sampled every half voxel.
    pub fn new(
        voxels: Handle<Image>,
        min: Vec3,
        max: Vec3,
        dimensions: UVec3,
        mix: ChannelMix,
    ) -> Self {
        VolumeMaterial {
            mix,
            settings: VolumeSettings {
                box_min: min.extend(1.0),
                box_max: max.extend(1.0),
                // Inside out, so no point is ever within it.
                detail_min: Vec4::ONE,
                detail_max: Vec4::ZERO,
                step: half_voxel(min, max, dimensions),
                max_steps: MAX_RAY_STEPS,
            },
            detail: voxels.clone(),
            voxels,
        }
    }

    /// Draw `detail` inside the box from `min` to `max`, and step finely enough
    /// to see it.
    ///
    /// Detail is always read from a finer level than the whole, so its voxels
    /// set the step. The ceiling on steps is what keeps a ray that runs far
    /// outside the detail from costing more than it may.
    pub fn set_detail(&mut self, detail: Handle<Image>, min: Vec3, max: Vec3, dimensions: UVec3) {
        self.settings.detail_min = min.extend(1.0);
        self.settings.detail_max = max.extend(1.0);
        self.settings.step = half_voxel(min, max, dimensions);
        self.detail = detail;
    }
}

/// Where `embedded_asset!` below put the shader. Derived rather than spelled
/// out, for the reason given beside the point shader's.
fn shader() -> ShaderRef {
    ShaderRef::Path(
        AssetPath::from_path_buf(embedded_path!("volume_material.wgsl")).with_source("embedded"),
    )
}

impl Material2d for VolumeMaterial {
    fn fragment_shader() -> ShaderRef {
        shader()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Back faces only: each pixel's ray is marched once, and still from
        // inside the box. The 2D pipeline draws both, which would march twice
        // and add the two together.
        descriptor.primitive.cull_mode = Some(Face::Front);
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
            }
        }
        Ok(())
    }
}

pub struct VolumeRenderPlugin;

impl Plugin for VolumeRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "volume_material.wgsl");
        app.add_plugins(Material2dPlugin::<VolumeMaterial>::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_step_by_half_the_finest_voxel() {
        // Sections 0.1 apart under pixels 0.045 across: the step follows the
        // pixels, or a ray would skip every other one.
        let material = VolumeMaterial::new(
            Handle::default(),
            Vec3::ZERO,
            Vec3::new(14.0, 10.5, 14.2),
            UVec3::new(312, 234, 142),
            ChannelMix::default(),
        );
        let pixel = 14.0 / 312.0;
        assert!((material.settings.step - pixel * 0.5).abs() < 1e-5);
    }

    #[test]
    fn detail_is_drawn_only_inside_its_box() {
        let mut material = VolumeMaterial::new(
            Handle::default(),
            Vec3::ZERO,
            Vec3::splat(10.0),
            UVec3::splat(100),
            ChannelMix::default(),
        );
        // No detail yet: the box is inside out, so nothing is ever in it.
        let s = material.settings;
        assert!(s.detail_min.truncate().cmpgt(s.detail_max.truncate()).any());

        material.set_detail(
            Handle::default(),
            Vec3::splat(4.0),
            Vec3::splat(5.0),
            UVec3::splat(100),
        );
        let s = material.settings;
        assert_eq!(s.detail_min.truncate(), Vec3::splat(4.0));
        // Stepping follows the detail's voxels, a hundredth of the box's.
        assert!((s.step - 0.005).abs() < 1e-6);
    }
}
