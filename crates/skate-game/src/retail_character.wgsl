#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings as frame}
#import bevy_pbr::shadows::fetch_directional_shadow
struct CharacterParams {
    light: vec4<f32>, tint: vec4<f32>, options: vec4<f32>,
    rows: array<vec4<f32>,9>, sh: array<vec4<f32>,9>,
}
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
    if p.options.w>0.0 {
        // Native hair response. The installed GLB supplies the selected dye
        // and composited strand coverage; live CAC colour rows are not present.
        let ndl=saturate(dot(n,p.light.xyz));
        let fres=pow(1.0-saturate(dot(n,vd)),max(p.rows[2].w,1.0));
        let hl=p.rows[1].rgb*(saturate(ndl*0.75+0.25)+p.rows[5].w*0.25)
            +p.rows[6].rgb*fres*saturate(ndl*1.75+0.25);
        if albedo.a<p.options.z { discard; }
        return vec4<f32>(d*hl*p.light.w,albedo.a*p.tint.a);
    }
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
    let s=p.rows[7].y;
    let v=vn*s;
    let irr=saturate(p.sh[0].rgb+v.x*p.sh[1].rgb+v.y*p.sh[2].rgb+v.z*p.sh[3].rgb
        +v.x*v.z*p.sh[4].rgb+v.z*v.y*p.sh[5].rgb+v.y*v.x*p.sh[6].rgb
        +(3.0*v.z*v.z-1.0)*p.sh[7].rgb+(v.x*v.x-v.y*v.y)*p.sh[8].rgb);
    let ndl=dot(vn,p.light.xyz);
    let view_z=(frame::view.view_from_world*i.world_position).z;
    var shadow=1.0;
    for (var light_id=0u; light_id<frame::lights.n_directional_lights; light_id+=1u) {
        // All-caster visibility light does not affect lightmapped world diffuse.
        if (frame::lights.directional_lights[light_id].flags & 5u)==1u {
            shadow=fetch_directional_shadow(light_id,i.world_position,vn,view_z);
            break;
        }
    }
    var lit=p.rows[1].rgb*saturate(ndl)*shadow+irr*p.rows[5].w;
    let fb=1.0-saturate(dot(vn,vd));
    let kfres=pow(max(fb,1e-6),p.rows[1].w);
    let rfres=pow(max(fb,1e-6),p.rows[0].z);
    let rd=normalize(p.light.xyz*vec3<f32>(-1.,0.2,-1.)-vd);
    lit+=saturate(rfres*saturate(dot(vn,rd)))*p.rows[6].rgb;
    let kr=p.light.xyz-2.0*ndl*vn;
    let ks=pow(saturate(dot(vd,-kr)),p.rows[2].w)*select(0.0,1.0,ndl>=0.0)*shadow;
    let rr=rd-2.0*dot(vn,rd)*vn;
    let rs=pow(saturate(dot(vd,-rr)),p.rows[3].w)*rfres;
    let spec=saturate((ks*p.rows[2].rgb*kfres+rs*p.rows[3].rgb)*smask*smask);
    if albedo.a<p.options.z { discard; }
    return vec4<f32>((d*lit+spec)*p.rows[0].y*p.light.w,albedo.a*p.tint.a);
}
