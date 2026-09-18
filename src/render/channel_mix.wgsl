// Mixing channels into colour, shared by everything drawn from unmixed
// channels: image tiles and volumes.
//
// Each channel is stored as its intensity, and painted in its colour by as
// much as that intensity reaches through its window. Mixing here rather than
// when the data is read is what makes showing, hiding and brightening a
// channel instant — only the uniform changes.

#define_import_path bde::channel_mix

struct ChannelMix {
    // Colour of each channel; w is 1 when it is shown and 0 when not.
    colours: array<vec4<f32>, 16>,
    // Each channel's window, as stored intensities: x where it starts to
    // show, y where it is at full colour.
    windows: array<vec4<f32>, 16>,
    count: u32,
    // One more than the index of the channel that cuts the tile out, or 0.
    mask: u32,
    // Multiplies the mixed colour, carrying the source's transparency.
    tint: vec4<f32>,
};

// What one channel adds, at a stored intensity of `value`.
fn contribution(colour: vec4<f32>, window: vec4<f32>, value: f32) -> vec3<f32> {
    let span = window.y - window.x;
    if colour.w == 0.0 || span <= 0.0 {
        return vec3<f32>(0.0);
    }
    return colour.rgb * clamp((value - window.x) / span, 0.0, 1.0);
}

// Channels mix in the encoded space they were always composited in, where
// halfway through a window looks half as bright; the target wants linear.
fn srgb_to_linear(colour: vec3<f32>) -> vec3<f32> {
    let low = colour / 12.92;
    let high = pow((colour + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, colour <= vec3<f32>(0.04045));
}
