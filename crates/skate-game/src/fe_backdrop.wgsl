#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
// x, y = blur direction, z = radius in pixels; tint.rgb multiplies the result.
struct Params { blur: vec4<f32>, tint: vec4<f32> }
@group(0) @binding(2) var<uniform> params: Params;

// One axis of the backdrop blur (a 17-tap gaussian over the radius), then the tint. The kernel shape is not
// retail's (its blur shader is not ported); the strength and the tint are.
@fragment
fn fragment(i: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let texel = params.blur.xy / vec2<f32>(textureDimensions(source));
    let radius = params.blur.z;
    var c = textureSampleLevel(source, source_sampler, i.uv, 0.0);
    if radius > 0.01 {
        let sigma = max(radius * 0.5, 0.5);
        var sum = vec4<f32>(0.0);
        var total = 0.0;
        for (var k = -8; k <= 8; k++) {
            let x = f32(k) * radius / 8.0;
            let w = exp(-x * x / (2.0 * sigma * sigma));
            sum += textureSampleLevel(source, source_sampler, i.uv + texel * x, 0.0) * w;
            total += w;
        }
        c = sum / total;
    }
    return vec4<f32>(c.rgb * params.tint.rgb, c.a);
}
