// Draws each point as a screen-space quad.
//
// Point clouds cannot be drawn with point topology and a size, because the
// hardware fixes points at one pixel. Every point is therefore four vertices
// sharing a position, each carrying a unit corner offset that this shader
// expands in clip space — so the size is a uniform and can change without
// touching the mesh.

#import bevy_sprite::{
    mesh2d_functions::{get_world_from_local, mesh2d_position_local_to_clip},
    mesh2d_view_bindings::view,
}

struct PointSettings {
    // Diameter in device pixels.
    size: f32,
    // Multiplies every point's colour, carrying the transparency setting.
    tint: vec4<f32>,
};

@group(2) @binding(0) var<uniform> settings: PointSettings;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) corner: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) corner: vec2<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    let world_from_local = get_world_from_local(vertex.instance_index);
    var clip = mesh2d_position_local_to_clip(world_from_local, vec4<f32>(vertex.position, 1.0));

    // Offset in clip space so the quad keeps its size on screen however far the
    // view is zoomed. Scaling by w keeps it right under any projection.
    //
    // The corner is a unit offset, so this is the half extent. Clip space spans
    // 2 across a viewport, meaning one clip unit is half the viewport in
    // pixels; dividing the diameter by the viewport therefore gives a half
    // extent of size/2 pixels, and a point `size` pixels across.
    let viewport = view.viewport.zw;
    let offset = vertex.corner * settings.size / viewport;
    clip = vec4<f32>(clip.xy + offset * clip.w, clip.zw);

    out.clip_position = clip;
    out.color = vertex.color * settings.tint;
    out.corner = vertex.corner;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Round the corners off into a disc. Below about three pixels the quad is
    // too small for the cut to read, and clipping it only loses brightness.
    if (settings.size >= 3.0 && dot(in.corner, in.corner) > 1.0) {
        discard;
    }
    return in.color;
}
