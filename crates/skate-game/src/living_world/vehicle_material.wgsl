// Retail car shaders vehicle_defaultPS / vehicle_glass_defaultPS (TU3 shaders_final.big), from the microcode.
// Port notes: docs/hails-additions/26h-living-world-traffic.md#car-materials
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings as frame}
#import bevy_pbr::shadows::fetch_directional_shadow
#import skate_character_lighting::{character_normal, character_albedo}

struct VehicleParams {
    // xyz: g_vLightDir (towards the light), w: scene exposure (the character lighting's light).
    light: vec4<f32>,
    // i_colorize_red / i_colorize_blue (secondary / chassis palette colour, times a mod gain).
    red: vec4<f32>,
    blue: vec4<f32>,
    // x: glass program, y: normal map present, z: i_params.x (fade), w: environment cube present.
    options: vec4<f32>,
    // rgb: the ambient shader literal, w: the mask literal (G^2 below it = paint mask texel).
    literals: vec4<f32>,
    // m_params (c14..c19 = rows 0..5) of the material's vault row.
    rows: array<vec4<f32>, 9>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: VehicleParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var normal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var environment: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var environment_sampler: sampler;

@fragment
fn fragment(i: VertexOutput) -> @location(0) vec4<f32> {
    let glass = p.options.x > 0.0;
    let tex = textureSample(diffuse, diffuse_sampler, i.uv);
    // Retail lights in squared space: lin = diffuse^2.
    let lin = character_albedo(tex.rgb);
    var albedo = lin;
    if glass {
        // vehicle_glass_defaultPS has no diffuse map: the albedo is m_params[4].rgb.
        albedo = p.rows[4].rgb;
    } else if lin.g < p.literals.w {
        albedo = lin.r * p.red.rgb + lin.b * p.blue.rgb;
    }
    let mapped = character_normal(i.world_position, i.world_normal, i.uv, textureSample(normal_map, normal_sampler, i.uv).xy);
    let vn = select(normalize(i.world_normal), mapped, p.options.y > 0.0);
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
    // Shadow term: the shadow lookups, zero facing away from the light.
    shadow *= select(0.0, 1.0, ndl >= 0.0);
    let key = max(ndl, 0.0) * p.rows[1].rgb * shadow;
    let ambient = p.literals.rgb * p.rows[5].w;
    // Phong highlight of the key light, power m_params[2].w.
    let kr = p.light.xyz - 2.0 * ndl * vn;
    let spec = pow(saturate(dot(vd, -kr)), p.rows[2].w) * shadow;
    var env = vec3<f32>(0.0);
    if p.options.w > 0.0 {
        let rv = vd - 2.0 * vn * dot(vd, vn);
        env = character_albedo(textureSample(environment, environment_sampler, vec3<f32>(-rv.x, -rv.y, rv.z)).rgb);
    }
    let fres = pow(max(1.0 - saturate(dot(vn, vd)), 1e-6), p.rows[1].w);
    var refl = (spec * p.rows[2].rgb + env) * fres;
    var alpha = p.options.z;
    if glass {
        alpha *= p.rows[4].w;
    } else {
        // The diffuse alpha, squared, masks the reflection on the body.
        refl *= tex.a * tex.a;
    }
    let lit = albedo * (ambient + key) + refl;
    return vec4<f32>(lit * p.rows[0].y * p.light.w, alpha);
}
