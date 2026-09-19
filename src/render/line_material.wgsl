// Draws outlines as quads widened in clip space.
//
// A stroke is measured in world units, which zoomed out can be far thinner
// than a pixel. Each segment's vertices carry a unit push across and past the
// segment, and this shader scales it to the wider of the stroke as drawn and a
// floor in screen pixels — so the mesh never changes as the view does.

#import bevy_sprite::{
    mesh2d_functions::{get_world_from_local, mesh2d_position_local_to_clip},
    mesh2d_view_bindings::view,
}

struct LineSettings {
    // The narrowest a stroke is drawn, in device pixels.
    min_stroke_px: f32,
    // The shortest a dash is drawn, in device pixels.
    min_dash_px: f32,
    // Multiplies every line's color, carrying the transparency setting.
    tint: vec4<f32>,
};

@group(2) @binding(0) var<uniform> settings: LineSettings;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) offset: vec2<f32>,
    // Distance along, stroke width and dash length, in world units.
    @location(3) style: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Distance along the outline, in device pixels at this zoom.
    @location(1) along_px: f32,
    // Length of a dash in device pixels, or zero for a solid line.
    @location(2) dash_px: f32,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    let world_from_local = get_world_from_local(vertex.instance_index);
    var clip = mesh2d_position_local_to_clip(world_from_local, vec4<f32>(vertex.position, 1.0));

    // Clip space spans 2 across the viewport, so a world unit covers this many
    // device pixels. The projection is orthographic and uniform, so one axis
    // stands for both.
    let px_per_world = view.clip_from_world[0][0] * view.viewport.z * 0.5;

    let stroke_px = max(vertex.style.y * px_per_world, settings.min_stroke_px);
    // The offset is a unit push, so half the stroke either side. Dividing by
    // the viewport turns pixels into clip units, which span two across it.
    let viewport = view.viewport.zw;
    clip = vec4<f32>(clip.xy + vertex.offset * stroke_px / viewport * clip.w, clip.zw);

    out.clip_position = clip;
    out.color = vertex.color * settings.tint;
    out.along_px = vertex.style.x * px_per_world;
    out.dash_px = select(0.0, max(vertex.style.z * px_per_world, settings.min_dash_px),
                         vertex.style.z > 0.0);
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Dashes and gaps of equal length, cut from the distance along.
    if (in.dash_px > 0.0 && fract(in.along_px / (2.0 * in.dash_px)) > 0.5) {
        discard;
    }
    return in.color;
}
