// Draws each point as a screen-space quad.
//
// Point clouds cannot be drawn with point topology and a size, because the
// hardware fixes points at one pixel. Every point is therefore four vertices
// sharing a position, each carrying a unit corner offset that this shader
// expands in clip space — so the size is a uniform and can change without
// touching the mesh.
//
// The same trick carries the highlight: a vertex knows the value its point is
// coloured by, and the uniform names one of those values to draw large. Hovering
// a cell therefore enlarges every other cell sharing its value without a single
// vertex being rewritten.

#import bevy_sprite::{
    mesh2d_functions::{get_world_from_local, mesh2d_position_local_to_clip},
    mesh2d_view_bindings::view,
}

struct PointSettings {
    // Diameter in device pixels.
    size: f32,
    // The category drawn large, or a value no category can take.
    highlight: u32,
    // How much larger, as a multiple of `size`.
    highlight_scale: f32,
    // Multiplies every point's colour, carrying the transparency setting.
    tint: vec4<f32>,
};

@group(2) @binding(0) var<uniform> settings: PointSettings;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    // Corner bits above the point's category. See `ATTRIBUTE_CORNER`.
    @location(2) packed: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) corner: vec2<f32>,
    // The size this point actually drew at, which the highlight can change.
    @location(2) diameter: f32,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    // Bit 16 and 17 are the corner's signs; the low half is the category.
    let corner = vec2<f32>(
        f32((vertex.packed >> 16u) & 1u) * 2.0 - 1.0,
        f32((vertex.packed >> 17u) & 1u) * 2.0 - 1.0,
    );
    let category = vertex.packed & 0xFFFFu;

    let world_from_local = get_world_from_local(vertex.instance_index);
    var clip = mesh2d_position_local_to_clip(world_from_local, vec4<f32>(vertex.position, 1.0));

    let diameter = select(settings.size, settings.size * settings.highlight_scale,
                          category == settings.highlight);

    // Offset in clip space so the quad keeps its size on screen however far the
    // view is zoomed. Scaling by w keeps it right under any projection.
    //
    // The corner is a unit offset, so this is the half extent. Clip space spans
    // 2 across a viewport, meaning one clip unit is half the viewport in
    // pixels; dividing the diameter by the viewport therefore gives a half
    // extent of size/2 pixels, and a point `size` pixels across.
    let viewport = view.viewport.zw;
    let offset = corner * diameter / viewport;
    clip = vec4<f32>(clip.xy + offset * clip.w, clip.zw);

    out.clip_position = clip;
    out.color = vertex.color * settings.tint;
    out.corner = corner;
    out.diameter = diameter;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Round the corners off into a disc. Below about three pixels the quad is
    // too small for the cut to read, and clipping it only loses brightness.
    // Measured against the size this point drew at, so a highlighted point stays
    // round even when its neighbours are too small to be cut.
    if (in.diameter >= 3.0 && dot(in.corner, in.corner) > 1.0) {
        discard;
    }
    return in.color;
}
