// Draws one image tile by mixing its channels, four to an array layer.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import bde::channel_mix::{ChannelMix, contribution, srgb_to_linear}

@group(2) @binding(0) var<uniform> channels: ChannelMix;
@group(2) @binding(1) var samples: texture_2d_array<f32>;
@group(2) @binding(2) var samples_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var colour = vec3<f32>(0.0);
    let layers = (channels.count + 3u) / 4u;
    for (var layer = 0u; layer < layers; layer++) {
        let texel = textureSample(samples, samples_sampler, in.uv, layer);
        for (var component = 0u; component < 4u; component++) {
            let index = layer * 4u + component;
            if index < channels.count {
                colour += contribution(channels.colours[index], channels.windows[index], texel[component]);
            }
        }
    }
    return vec4<f32>(srgb_to_linear(min(colour, vec3<f32>(1.0))) * channels.tint.rgb, 1.0);
}
