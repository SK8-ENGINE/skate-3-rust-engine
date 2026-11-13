#import bevy_ui::ui_vertex_output::UiVertexOutput
@group(1) @binding(0) var hud: texture_2d<f32>;
@group(1) @binding(1) var hud_sampler: sampler;
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), c > vec3(0.04045));
}
// The layer holds display values multiplied by coverage (blended on display values, see `hud_render.wgsl`): undo the
// coverage, convert to linear, multiply again for the premultiplied blend onto the frame.
@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let t = textureSample(hud, hud_sampler, in.uv);
    if t.a <= 0.0 {
        return vec4(0.0);
    }
    return vec4(to_linear(clamp(t.rgb / t.a, vec3(0.0), vec3(1.0))) * t.a, t.a);
}
