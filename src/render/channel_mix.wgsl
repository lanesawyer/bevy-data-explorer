// Mixing channels into color, shared by everything drawn from unmixed
// channels: image tiles and volumes.
//
// Each channel is stored as its intensity, and painted in its color by as
// much as that intensity reaches through its window. Mixing here rather than
// when the data is read is what makes showing, hiding and brightening a
// channel instant — only the uniform changes.

#define_import_path bde::channel_mix

struct ChannelMix {
    // Color of each channel; w is 1 when it is shown and 0 when not.
    colors: array<vec4<f32>, 16>,
    // Each channel's window, as stored intensities: x where it starts to
    // show, y where it is at full color.
    windows: array<vec4<f32>, 16>,
    count: u32,
    // Multiplies the mixed color, carrying the source's transparency.
    tint: vec4<f32>,
};

// What one channel adds, at a stored intensity of `value`.
fn contribution(color: vec4<f32>, window: vec4<f32>, value: f32) -> vec3<f32> {
    let span = window.y - window.x;
    if color.w == 0.0 || span <= 0.0 {
        return vec3<f32>(0.0);
    }
    return color.rgb * clamp((value - window.x) / span, 0.0, 1.0);
}

// Channels mix in the encoded space they were always composited in, where
// halfway through a window looks half as bright; the target wants linear.
fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    let low = color / 12.92;
    let high = pow((color + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, color <= vec3<f32>(0.04045));
}
