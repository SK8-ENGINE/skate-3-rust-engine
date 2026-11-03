// Retail ped body shader livingworld_stamp_defaultPS (TU3 shaders_final.big), from the microcode (research b614).
// Port notes: docs/hails-additions/26f-living-world-peds.md#ped-material-the-retail-shadow-side-2026-10-25-user-asks-352
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings as frame, mesh_bindings::mesh}
#import bevy_pbr::shadows::fetch_directional_shadow
#import skate_character_lighting::{character_normal, character_albedo}

struct PedParams {
    // xyz: g_vLightDir (towards the light), w: scene exposure (the character lighting's light).
    light: vec4<f32>,
    // x: normal map present, y: i_params.x (fade alpha).
    options: vec4<f32>,
    // rgb: the ambient shader literal.
    literals: vec4<f32>,
    // m_params (c14..c20 = rows 0..6) of the material's vault row.
    rows: array<vec4<f32>, 9>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: PedParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var normal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var normal_sampler: sampler;

@fragment
fn fragment(i: VertexOutput) -> @location(0) vec4<f32> {
    // The diffuse carries the ped's tint pair already (the colorize rule, baked per pair).
    let albedo = character_albedo(textureSample(diffuse, diffuse_sampler, i.uv).rgb);
    let mapped = character_normal(i.world_position, i.world_normal, i.uv, textureSample(normal_map, normal_sampler, i.uv).xy);
    let vn = select(normalize(i.world_normal), mapped, p.options.x > 0.0);
    let vd = -normalize(i.world_position.xyz - frame::view.world_position);
    let ndl = dot(vn, p.light.xyz);
    let view_z = (frame::view.view_from_world * i.world_position).z;
    var shadow = 1.0;
    for (var light_id = 0u; light_id < frame::lights.n_directional_lights; light_id += 1u) {
        if (frame::lights.directional_lights[light_id].flags & 5u) == 1u {
            shadow = fetch_directional_shadow(light_id, i.world_position, vn, view_z);
            break;
        }
    }
    shadow *= select(0.0, 1.0, ndl >= 0.0);
    let key = max(ndl, 0.0) * p.rows[1].rgb * shadow;
    // The whole shadow side: one constant per material, plus a view rim (power m_params[0].z, colour m_params[6]).
    let nv = saturate(dot(vn, vd));
    let rim = pow(max(1.0 - nv, 1e-6), p.rows[0].z);
    let ambient = p.literals.rgb * p.rows[5].w + rim * p.rows[6].rgb;
    // Phong highlight of the key light (power m_params[2].w, colour m_params[2].rgb, fresnel power m_params[1].w).
    let kr = p.light.xyz - 2.0 * ndl * vn;
    let spec = pow(saturate(dot(vd, -kr)), p.rows[2].w) * shadow * p.rows[2].rgb * pow(max(1.0 - nv, 1e-6), p.rows[1].w);
    let lit = albedo * (key + ambient) + spec;
    // The fade alpha per instance in MeshTag while a fade runs (npc_skaters::present_fade), else i_params.x.
    let fade_tag = mesh[i.instance_index].tag;
    let alpha = select(p.options.y, bitcast<f32>(fade_tag ^ 0x80000000u), fade_tag != 0u);
    return vec4<f32>(lit * p.rows[0].y * p.light.w, alpha);
}
