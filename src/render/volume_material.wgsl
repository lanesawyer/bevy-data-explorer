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

struct VolumeSettings {
    // Corners of the box, in world space. The box is never rotated, so these
    // are all a ray needs to find where it enters and leaves.
    box_min: vec4<f32>,
    box_max: vec4<f32>,
    // World distance between samples, and a ceiling on how many there are.
    step: f32,
    max_steps: u32,
};

@group(2) @binding(0) var<uniform> settings: VolumeSettings;
@group(2) @binding(1) var voxels: texture_3d<f32>;
@group(2) @binding(2) var voxel_sampler: sampler;

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

    let size = settings.box_max.xyz - settings.box_min.xyz;
    let length = far - near;
    let count = min(u32(ceil(length / settings.step)), settings.max_steps);
    let step = length / f32(max(count, 1u));

    var brightest = vec3<f32>(0.0);
    for (var i = 0u; i < count; i++) {
        let point = origin + direction * (near + (f32(i) + 0.5) * step);
        // Texture rows run down the image and layers from the first slice,
        // which sits at the front of the box; world y and z run the other way.
        let t = (point - settings.box_min.xyz) / size;
        let uvw = vec3<f32>(t.x, 1.0 - t.y, 1.0 - t.z);
        brightest = max(brightest, textureSampleLevel(voxels, voxel_sampler, uvw, 0.0).rgb);
    }

    // Covers what is behind it by as much as it is bright, so dark tissue lets
    // the frame show through and the brightest signal hides it. Premultiplied:
    // the colour already carries its coverage.
    let coverage = max(max(brightest.r, brightest.g), brightest.b);
    if coverage <= 0.0 {
        discard;
    }
    return vec4<f32>(brightest, coverage);
}
