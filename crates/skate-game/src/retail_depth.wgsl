#import bevy_pbr::prepass_io::VertexOutput
#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput
#endif
#ifdef MOTION_VECTOR_PREPASS
#import bevy_pbr::pbr_prepass_functions::calculate_motion_vector
#endif
struct WorldParams { mode: vec4<f32>, surface: vec4<f32>, family: vec4<f32>, fog_ramp: vec4<f32>, fog_color: vec4<f32>, shadow_color: vec4<f32>, sun_direction: vec4<f32> }
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: WorldParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@fragment
fn fragment(i: VertexOutput)
#ifdef PREPASS_FRAGMENT
    -> FragmentOutput
#endif
{
    if textureSample(diffuse,diffuse_sampler,i.uv).a < p.mode.z { discard; }
#ifdef PREPASS_FRAGMENT
    var out: FragmentOutput;
#ifdef NORMAL_PREPASS
    out.normal = vec4<f32>(normalize(i.world_normal)*0.5+0.5,1.0);
#endif
#ifdef MOTION_VECTOR_PREPASS
    out.motion_vector = calculate_motion_vector(i.world_position,i.previous_world_position);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = i.unclipped_depth;
#endif
    return out;
#endif
}
