#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<storage, read> exposure: vec4<f32>;
// The retail colour grade (postfx_visualfxPS): near rows 0..2, far rows 3..5 (rgb . row.xyz + row.w), the depth
// weights (mult.x, mult.y, add.x, add.y) and (camera near, 0, 0, 0). The base record is the identity.
@group(0) @binding(3) var depth: texture_depth_2d;
@group(0) @binding(4) var<uniform> grade: array<vec4<f32>, 8>;

fn graded(c: vec3<f32>, base: u32) -> vec3<f32> {
    let v = vec4<f32>(c, 1.0);
    return vec3<f32>(dot(grade[base], v), dot(grade[base + 1u], v), dot(grade[base + 2u], v));
}

@fragment
fn fragment(i: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(source,source_sampler,i.uv);
    let xe = max(c.rgb*exposure.x/2.5,vec3<f32>(0.0));
    let t = saturate(1.0-xe);
    let tm = max(xe*0.25+0.75,vec3<f32>(1.0))-t*t;
    let curve = sqrt(max(tm*0.5,vec3<f32>(0.0)))*1.41;
    // The colour matrix after the curve, before the clamp: near and far mixed linearly in view depth (reverse-Z
    // infinite perspective: view depth = near / depth; depth 0 = far away).
    let size = vec2<f32>(textureDimensions(depth));
    let d = textureLoad(depth, vec2<i32>(clamp(i.uv * size, vec2<f32>(0.0), size - 1.0)), 0);
    let z = select(1.0e9, grade[7].x / d, d > 0.0);
    let wa = saturate(z * grade[6].x + grade[6].z);
    let wb = saturate(z * grade[6].y + grade[6].w);
    let gamma = saturate(wa * graded(curve, 0u) + wb * graded(curve, 3u));
    // Bevy's final output attachment performs sRGB encoding. The retail
    // curve already includes gamma; invert sRGB here to avoid encoding twice.
    let linear = select(gamma/12.92,pow((gamma+0.055)/1.055,vec3<f32>(2.4)),gamma>vec3<f32>(0.04045));
    // The reference's final tone pass is opaque. Material coverage has
    // already been resolved; carrying it into the presentation blit would
    // composite foliage a second time against the window's clear colour.
    return vec4<f32>(linear,1.0);
}
