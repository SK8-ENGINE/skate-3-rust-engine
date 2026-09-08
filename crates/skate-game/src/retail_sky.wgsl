#import bevy_pbr::{forward_io::Vertex, mesh_view_bindings::view}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> settings: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var panorama: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var panorama_sampler: sampler;
struct SkyOut { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex
fn vertex(v: Vertex) -> SkyOut {
    var out: SkyOut;
    let world = v.position + vec3<f32>(view.world_position.x,settings.x,view.world_position.z);
    out.position = view.clip_from_world*vec4<f32>(world,1.0);
    // Reverse-Z far plane: the authored dome is larger than the world camera's far distance.
    out.position.z = 0.0000001*out.position.w;
    out.uv = v.uv;
    return out;
}
@fragment
fn fragment(i: SkyOut) -> @location(0) vec4<f32> {
    let d = textureSample(panorama,panorama_sampler,i.uv).rgb;
    return vec4<f32>(d*d*settings.y*settings.z,1.0);
}
