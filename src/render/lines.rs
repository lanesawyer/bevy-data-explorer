//! Rendering for outlines: annotations drawn over an image.
//!
//! A stroke is given in the units of what it outlines, and at a slide's
//! overview a ten pixel stroke is a hundredth of a screen pixel. So each
//! segment is a quad the shader widens in clip space to whichever is wider,
//! the stroke as drawn or a floor in screen pixels, which keeps an outline
//! legible zoomed out and true to its width zoomed in — without the mesh being
//! rebuilt as the view moves, and correct in two frames at different zooms.

use bevy::asset::RenderAssetUsages;
use bevy::asset::{Asset, AssetPath, embedded_asset, embedded_path};
use bevy::mesh::{
    Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology,
    VertexAttributeValues,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, VertexFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};

/// Which way a vertex is pushed out from its segment: across it by the normal,
/// and past its end by the tangent, both unit length. The shader scales them by
/// half the stroke.
///
/// Pushing past the ends squares each segment off, which covers the notch a
/// corner would otherwise leave between two segments at the widths these are
/// drawn at, for no join geometry.
pub const ATTRIBUTE_LINE_OFFSET: MeshVertexAttribute =
    MeshVertexAttribute::new("Vertex_LineOffset", 0x9c0d_7e21, VertexFormat::Float32x2);

/// Distance along the outline, stroke width and dash length, all in world
/// units. A dash length of zero is a solid line.
pub const ATTRIBUTE_LINE_STYLE: MeshVertexAttribute =
    MeshVertexAttribute::new("Vertex_LineStyle", 0x9c0d_7e22, VertexFormat::Float32x3);

/// Colour, as bytes.
pub const ATTRIBUTE_LINE_COLOR: MeshVertexAttribute =
    MeshVertexAttribute::new("Vertex_LineColor", 0x9c0d_7e23, VertexFormat::Unorm8x4);

/// The narrowest a stroke is drawn, in device pixels.
pub const MIN_STROKE_PX: f32 = 2.0;

/// The shortest a dash is drawn, in device pixels. Below this a dashed outline
/// reads as a faint solid one.
pub const MIN_DASH_PX: f32 = 6.0;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct LineMaterial {
    #[uniform(0)]
    pub settings: LineSettings,
}

#[derive(Clone, Copy, ShaderType)]
pub struct LineSettings {
    pub min_stroke_px: f32,
    pub min_dash_px: f32,
    /// Multiplies every line's colour, carrying the transparency setting.
    ///
    /// Unlike points this lowers alpha rather than dimming: an outline barely
    /// overdraws itself, and dimming one over a pale slide would darken it
    /// rather than let the slide show through.
    pub tint: Vec4,
}

impl Default for LineMaterial {
    fn default() -> Self {
        LineMaterial {
            settings: LineSettings {
                min_stroke_px: MIN_STROKE_PX,
                min_dash_px: MIN_DASH_PX,
                tint: Vec4::ONE,
            },
        }
    }
}

/// Where `embedded_asset!` below put the shader. Derived for the same reason
/// the point shader's is.
fn shader() -> ShaderRef {
    ShaderRef::Path(
        AssetPath::from_path_buf(embedded_path!("line_material.wgsl")).with_source("embedded"),
    )
}

impl Material2d for LineMaterial {
    fn vertex_shader() -> ShaderRef {
        shader()
    }

    fn fragment_shader() -> ShaderRef {
        shader()
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
            ATTRIBUTE_LINE_COLOR.at_shader_location(1),
            ATTRIBUTE_LINE_OFFSET.at_shader_location(2),
            ATTRIBUTE_LINE_STYLE.at_shader_location(3),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        Ok(())
    }
}

pub struct LineRenderPlugin;

impl Plugin for LineRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "line_material.wgsl");
        app.add_plugins(Material2dPlugin::<LineMaterial>::default());
    }
}

/// One outline to draw, in display coordinates.
pub struct Stroke<'a> {
    pub points: &'a [Vec2],
    pub closed: bool,
    pub colour: [f32; 4],
    pub width: f32,
    /// World length of a dash, or zero for a solid line.
    pub dash: f32,
}

/// Where each corner of a segment's quad sits: which side of the line, and
/// which end.
const QUAD: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
const TRIANGLES: [u32; 6] = [0, 1, 2, 0, 2, 3];

/// Build one mesh holding every stroke, a quad per segment, in the order given
/// so that a later stroke draws over an earlier one.
pub fn build_line_mesh<'a>(strokes: impl IntoIterator<Item = Stroke<'a>>) -> Mesh {
    let mut positions = Vec::new();
    let mut colours = Vec::new();
    let mut offsets = Vec::new();
    let mut styles = Vec::new();
    let mut indices = Vec::new();

    for stroke in strokes {
        let colour = stroke
            .colour
            .map(|channel| (channel.clamp(0.0, 1.0) * 255.0) as u8);
        let closing = stroke
            .closed
            .then(|| (stroke.points.last(), stroke.points.first()));
        let segments = stroke
            .points
            .windows(2)
            .map(|pair| (&pair[0], &pair[1]))
            .chain(closing.and_then(|(from, to)| from.zip(to)));

        let mut along = 0.0;
        for (from, to) in segments {
            let length = from.distance(*to);
            if length <= f32::EPSILON {
                continue;
            }
            let tangent = (*to - *from) / length;
            let normal = tangent.perp();
            let base = positions.len() as u32;
            for (side, end) in QUAD {
                let (point, distance) = if end < 0.0 {
                    (from, along)
                } else {
                    (to, along + length)
                };
                positions.push([point.x, point.y, 0.0]);
                colours.push(colour);
                offsets.push((normal * side + tangent * end).to_array());
                styles.push([distance, stroke.width, stroke.dash]);
            }
            indices.extend(TRIANGLES.map(|offset| base + offset));
            along += length;
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(
        ATTRIBUTE_LINE_COLOR,
        VertexAttributeValues::Unorm8x4(colours),
    );
    mesh.insert_attribute(ATTRIBUTE_LINE_OFFSET, offsets);
    mesh.insert_attribute(ATTRIBUTE_LINE_STYLE, styles);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Vec2> {
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ]
    }

    fn stroke(points: &[Vec2], closed: bool) -> Stroke<'_> {
        Stroke {
            points,
            closed,
            colour: [1.0; 4],
            width: 2.0,
            dash: 0.0,
        }
    }

    #[test]
    fn a_closed_outline_gets_the_segment_back_to_its_start() {
        let points = square();
        let open = build_line_mesh([stroke(&points, false)]);
        let closed = build_line_mesh([stroke(&points, true)]);
        assert_eq!(open.count_vertices(), 3 * 4);
        assert_eq!(closed.count_vertices(), 4 * 4);
        assert_eq!(closed.indices().unwrap().len(), 4 * 6);
    }

    #[test]
    fn a_repeated_point_draws_nothing_rather_than_a_nan() {
        // Converters write the closing point out as well as implying it, and a
        // zero-length segment has no direction to widen along.
        let points = [Vec2::ZERO, Vec2::ZERO, Vec2::X];
        let mesh = build_line_mesh([stroke(&points, false)]);
        assert_eq!(mesh.count_vertices(), 4);
        let Some(VertexAttributeValues::Float32x2(offsets)) = mesh.attribute(ATTRIBUTE_LINE_OFFSET)
        else {
            panic!("expected offsets");
        };
        assert!(offsets.iter().flatten().all(|v| v.is_finite()));
    }

    #[test]
    fn distance_along_carries_on_across_segments() {
        // Dashes are cut from it, so restarting at every corner would put a
        // dash at every corner.
        let points = square();
        let mesh = build_line_mesh([stroke(&points, true)]);
        let Some(VertexAttributeValues::Float32x3(styles)) = mesh.attribute(ATTRIBUTE_LINE_STYLE)
        else {
            panic!("expected styles");
        };
        let ends: Vec<f32> = styles.chunks(4).map(|quad| quad[2][0]).collect();
        assert_eq!(ends, vec![10.0, 20.0, 30.0, 40.0]);
    }

    #[test]
    fn every_corner_is_pushed_across_and_past_its_segment() {
        let signs: std::collections::HashSet<(i8, i8)> = QUAD
            .iter()
            .map(|(side, end)| (side.signum() as i8, end.signum() as i8))
            .collect();
        assert_eq!(signs.len(), 4);
    }
}
