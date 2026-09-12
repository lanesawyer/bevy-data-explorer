//! Rendering for point clouds.
//!
//! Points are drawn as screen-space quads rather than with point topology,
//! because the hardware fixes point primitives at a single pixel and gives no
//! way to size them. Each point becomes four vertices sharing a position, with
//! a unit corner offset that the shader expands, so point size is a uniform and
//! costs nothing to change.

use bevy::asset::RenderAssetUsages;
use bevy::asset::{Asset, embedded_asset};
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::VertexFormat;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};

/// Unit offset of a vertex from its point's centre, expanded by the shader.
pub const ATTRIBUTE_CORNER: MeshVertexAttribute =
    MeshVertexAttribute::new("Vertex_Corner", 0x9c0d_7e11, VertexFormat::Float32x2);

/// Default diameter of a point, in device pixels.
pub const DEFAULT_POINT_PX: f32 = 1.5;
/// Range the point size control offers.
///
/// The floor is below a pixel on purpose: zoomed out, a dense cloud stacks many
/// points on every pixel, and thinning them past one is what stops the whole
/// thing turning into a solid mass.
pub const MIN_POINT_PX: f32 = 0.5;
pub const MAX_POINT_PX: f32 = 12.0;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct PointMaterial {
    #[uniform(0)]
    pub settings: PointSettings,
}

#[derive(Clone, Copy, ShaderType)]
pub struct PointSettings {
    pub size: f32,
    pub tint: Vec4,
}

impl Default for PointMaterial {
    fn default() -> Self {
        PointMaterial {
            settings: PointSettings {
                size: DEFAULT_POINT_PX,
                tint: Vec4::ONE,
            },
        }
    }
}

impl Material2d for PointMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Path("embedded://bevy_data_explorer/point_material.wgsl".into())
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path("embedded://bevy_data_explorer/point_material.wgsl".into())
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(1),
            ATTRIBUTE_CORNER.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        Ok(())
    }
}

/// How large a source draws its points, in logical pixels.
///
/// Only point sources carry this. The sidebar offers the control when the
/// selected source has one, so an image never shows a setting that means
/// nothing to it.
#[derive(Component, Clone, Copy)]
pub struct SourcePointSize(pub f32);

impl Default for SourcePointSize {
    fn default() -> Self {
        SourcePointSize(DEFAULT_POINT_PX)
    }
}

pub struct PointRenderPlugin;

impl Plugin for PointRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "point_material.wgsl");
        app.add_plugins(Material2dPlugin::<PointMaterial>::default());
    }
}

/// The four corners of a point's quad, and the two triangles covering it.
const CORNERS: [[f32; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
const TRIANGLES: [u32; 6] = [0, 1, 2, 0, 2, 3];

/// Build a mesh of quads, one per point.
pub fn build_point_mesh(positions: &[Vec2], colours: &[[f32; 4]]) -> Mesh {
    let count = positions.len().min(colours.len());
    let mut vertices = Vec::with_capacity(count * 4);
    let mut colour_data = Vec::with_capacity(count * 4);
    let mut corners = Vec::with_capacity(count * 4);
    let mut indices = Vec::with_capacity(count * 6);

    for (index, (point, colour)) in positions.iter().zip(colours).enumerate() {
        // World y is negated for display, matching every other source.
        let centre = [point.x, -point.y, 0.0];
        for corner in CORNERS {
            vertices.push(centre);
            colour_data.push(*colour);
            corners.push(corner);
        }
        let base = (index * 4) as u32;
        indices.extend(TRIANGLES.map(|offset| base + offset));
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colour_data);
    mesh.insert_attribute(ATTRIBUTE_CORNER, corners);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_point_becomes_a_quad() {
        let mesh = build_point_mesh(&[Vec2::ZERO, Vec2::new(1.0, 2.0)], &[[1.0; 4]; 2]);
        assert_eq!(mesh.count_vertices(), 8);
        assert_eq!(mesh.indices().unwrap().len(), 12);
    }

    #[test]
    fn the_corners_cover_a_unit_square() {
        // The shader treats these as a unit offset and the fragment stage cuts
        // a disc from them, so they have to reach the full extent in each axis.
        let xs: Vec<f32> = CORNERS.iter().map(|c| c[0]).collect();
        let ys: Vec<f32> = CORNERS.iter().map(|c| c[1]).collect();
        assert_eq!(xs.iter().cloned().fold(f32::MAX, f32::min), -1.0);
        assert_eq!(xs.iter().cloned().fold(f32::MIN, f32::max), 1.0);
        assert_eq!(ys.iter().cloned().fold(f32::MAX, f32::min), -1.0);
        assert_eq!(ys.iter().cloned().fold(f32::MIN, f32::max), 1.0);
    }

    #[test]
    fn the_triangles_use_each_corner() {
        let used: std::collections::HashSet<u32> = TRIANGLES.iter().copied().collect();
        assert_eq!(used.len(), 4, "a corner of the quad would be missing");
    }

    #[test]
    fn points_are_placed_with_y_running_downward() {
        // Display space negates y, as the image and section views do.
        let mesh = build_point_mesh(&[Vec2::new(3.0, 5.0)], &[[1.0; 4]]);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("expected float positions");
        };
        assert_eq!(positions[0], [3.0, -5.0, 0.0]);
    }

    #[test]
    fn the_default_size_sits_inside_the_range_offered() {
        assert!(MIN_POINT_PX <= DEFAULT_POINT_PX && DEFAULT_POINT_PX <= MAX_POINT_PX);
    }

    #[test]
    fn the_smallest_setting_is_thinner_than_a_pixel() {
        // Zoomed out over a dense cloud, a whole pixel a point is already a
        // solid mass, so the range has to reach below one.
        assert!(MIN_POINT_PX < 1.0);
    }

    #[test]
    fn a_quad_costs_four_vertices_per_point() {
        // Sizing points means giving each one geometry, which is four times the
        // vertices a point list needed. Worth keeping in view against the point
        // budgets the streamers enforce.
        let mesh = build_point_mesh(&[Vec2::ZERO; 100], &[[1.0; 4]; 100]);
        assert_eq!(mesh.count_vertices(), 400);
    }
}
