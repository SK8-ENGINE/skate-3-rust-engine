#import bevy_sprite::mesh2d_vertex_output::VertexOutput
struct ColorTransform { multiply: vec4<f32>, add: vec4<f32>, mode: vec4<f32> }
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> color: ColorTransform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var atlas_sampler: sampler;

fn to_gamma(c: vec3<f32>) -> vec3<f32> {
    return select(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, c > vec3(0.0031308));
}
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), c > vec3(0.04045));
}

// Blend in display (gamma) values like retail's APT draw state (827E4AF0 / 825D4490); mode.x picks the path: docs/hails-additions/31-retail-menus.md#hud-blend-space
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = textureSample(atlas, atlas_sampler, in.uv);
    if color.mode.x < 0.5 {
        let l = clamp(t * color.multiply + color.add, vec4(0.0), vec4(1.0));
        return vec4(to_gamma(l.rgb), l.a);
    }
    return clamp(vec4(to_gamma(t.rgb), t.a) * color.multiply + color.add, vec4(0.0), vec4(1.0));
}
