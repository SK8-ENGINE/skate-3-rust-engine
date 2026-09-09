#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings as frame}
#import skate_character_lighting::{CharacterParams, shade_character}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: CharacterParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var normal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var mask_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var mask_sampler: sampler;

@fragment
fn fragment(i: VertexOutput) -> @location(0) vec4<f32> {
    let albedo = textureSample(diffuse,diffuse_sampler,i.uv);
    // GLTF diffuse is sampled as sRGB; recover retail's gamma-two decode.
    let gamma = select(albedo.rgb*12.92, 1.055*pow(max(albedo.rgb,vec3<f32>(0.0)),vec3<f32>(1.0/2.4))-0.055, albedo.rgb>vec3<f32>(0.0031308));
    let d = gamma*gamma*p.tint.rgb;
    let n = normalize(i.world_normal);
    let rpos = i.world_position.xyz-frame::view.world_position;
    let vd = -normalize(rpos);
    let dp1=dpdx(rpos); let dp2=dpdy(rpos);
    let du1=dpdx(i.uv); let du2=dpdy(i.uv);
    var tt=cross(dp2,n)*du1.x+cross(n,dp1)*du2.x;
    var bb=cross(dp2,n)*du1.y+cross(n,dp1)*du2.y;
    tt *= -inverseSqrt(max(dot(tt,tt),1e-12));
    bb *= inverseSqrt(max(dot(bb,bb),1e-12));
    var vn=n;
    if p.options.x>0.0 {
        // Installed normal PNGs already unpack DXT5nm into RGB.
        let nm=textureSample(normal_map,normal_sampler,i.uv).xy*2.0-1.0;
        vn=normalize(nm.x*tt+nm.y*bb+n*sqrt(saturate(1.0-dot(nm,nm))));
    }
    var smask=albedo.a;
    if p.options.y>0.0 { smask=textureSample(mask_map,mask_sampler,i.uv).r; }
    return shade_character(p, vn, i.world_position, d, smask, albedo.a);
}
