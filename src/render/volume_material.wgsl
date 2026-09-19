// Draws a stack of slices as a volume, by marching each pixel's view ray
// through a 3D texture.
//
// What is kept along the ray is the brightest sample, not a blend: a
// maximum-intensity projection. It is the usual way to look at fluorescence in
// depth, and it needs no order — the brightest point on a ray is the same from
// either end — so the box is one draw with nothing to sort.
//
// The box is drawn by its back faces. Front faces vanish once the camera is
// inside the box, and a back face is behind every point the ray can enter by,
// so marching from the camera — or from where the ray enters, if that is
// further — covers the whole of the volume in view.

#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::view,
}
#import bde::channel_mix::{ChannelMix, contribution, srgb_to_linear}

struct VolumeSettings {
    // Corners of the box, in world space. The box is never rotated, so these
    // are all a ray needs to find where it enters and leaves.
    box_min: vec4<f32>,
    box_max: vec4<f32>,
    // The part read finely, or an inside-out box when there is none.
    detail_min: vec4<f32>,
    detail_max: vec4<f32>,
    // World distance between samples, and a ceiling on how many there are.
    step: f32,
    max_steps: u32,
};

@group(2) @binding(0) var<uniform> settings: VolumeSettings;
@group(2) @binding(1) var voxels: texture_3d<f32>;
@group(2) @binding(2) var voxel_sampler: sampler;
@group(2) @binding(3) var detail: texture_3d<f32>;
@group(2) @binding(4) var detail_sampler: sampler;
@group(2) @binding(5) var<uniform> channels: ChannelMix;

// A sample's channels mixed into color, as a tile would paint them. A 3D
// texture holds four channels, so a volume mixes at most four.
fn mixed(texel: vec4<f32>) -> vec3<f32> {
    var color = vec3<f32>(0.0);
    for (var index = 0u; index < min(channels.count, 4u); index++) {
        color += contribution(channels.colors[index], channels.windows[index], texel[index]);
    }
    return srgb_to_linear(min(color, vec3<f32>(1.0)));
}

// Where a point lies in a box as texture coordinates. Texture rows run down
// the image and layers from the first slice, which sits at the front of the
// box; world y and z run the other way.
fn texture_coordinates(point: vec3<f32>, low: vec3<f32>, high: vec3<f32>) -> vec3<f32> {
    let t = (point - low) / (high - low);
    return vec3<f32>(t.x, 1.0 - t.y, 1.0 - t.z);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let origin = view.world_position;
    let direction = normalize(in.world_position.xyz - origin);

    // Where the ray crosses each pair of faces; the box is the stretch where
    // it is inside all three.
    let inverse = 1.0 / direction;
    let a = (settings.box_min.xyz - origin) * inverse;
    let b = (settings.box_max.xyz - origin) * inverse;
    let near = max(max(max(min(a.x, b.x), min(a.y, b.y)), min(a.z, b.z)), 0.0);
    let far = min(min(max(a.x, b.x), max(a.y, b.y)), max(a.z, b.z));
    if far <= near {
        discard;
    }

    let length = far - near;
    let count = min(u32(ceil(length / settings.step)), settings.max_steps);
    let step = length / f32(max(count, 1u));

    var brightest = vec3<f32>(0.0);
    for (var i = 0u; i < count; i++) {
        let point = origin + direction * (near + (f32(i) + 0.5) * step);
        var texel: vec4<f32>;
        if all(point >= settings.detail_min.xyz) && all(point <= settings.detail_max.xyz) {
            let uvw = texture_coordinates(point, settings.detail_min.xyz, settings.detail_max.xyz);
            texel = textureSampleLevel(detail, detail_sampler, uvw, 0.0);
        } else {
            let uvw = texture_coordinates(point, settings.box_min.xyz, settings.box_max.xyz);
            texel = textureSampleLevel(voxels, voxel_sampler, uvw, 0.0);
        }
        brightest = max(brightest, mixed(texel));
    }

    // Covers what is behind it by as much as it is bright, so dark tissue lets
    // the frame show through and the brightest signal hides it. Premultiplied:
    // the color already carries its coverage.
    let coverage = max(max(brightest.r, brightest.g), brightest.b);
    if coverage <= 0.0 {
        discard;
    }
    return vec4<f32>(brightest * channels.tint.rgb, coverage);
}
