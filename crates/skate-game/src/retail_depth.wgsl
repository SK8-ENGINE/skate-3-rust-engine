// Shadow-map (prepass) stages for merged world geometry.
//
// This pass cannot use Bevy's default prepass shaders. Those declare Bevy's own
// prepass attribute locations — where `COLOR` lands on location 7 rather than
// the main pass's 5 — while `WorldMaterial::specialize` installs one layout for
// every pipeline it is asked to build, because `material_index` is a vertex
// attribute the default shaders know nothing about. Supplying both stages here
// keeps the shader and that layout describing the same vertex.
#import bevy_pbr::{mesh_functions, view_transformations::position_world_to_clip}
#import skate_retail::material_bindings as bindings

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) material_index: u32,
    @location(6) tangent: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) material_index: u32,
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    @location(2) unclipped_depth: f32,
#endif
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local, vec4<f32>(v.position, 1.0));
    out.clip_position = position_world_to_clip(world_position.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    // Casters behind the cascade's near plane still occlude, so carry the true
    // depth and clamp the clipped one, as the reference prepass does.
    out.unclipped_depth = out.clip_position.z;
    out.clip_position.z = min(out.clip_position.z, 1.0);
#endif
    out.uv = v.uv;
    out.material_index = v.material_index;
    return out;
}

@fragment
fn fragment(i: VertexOutput)
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    -> @builtin(frag_depth) f32
#endif
{
#ifdef WORLD_ALPHA_CUTOFF
    // Only the classes that can reject a texel pay for the fetch, so fences and
    // foliage cast their silhouette instead of their quad.
    let slot = i.material_index;
    let alpha = bindings::sample_diffuse(slot, i.uv, bindings::gradients(i.uv)).a;
    if alpha < bindings::params[slot].mode.z { discard; }
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    return i.unclipped_depth;
#endif
}
