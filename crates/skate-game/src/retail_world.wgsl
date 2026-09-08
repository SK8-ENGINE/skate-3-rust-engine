#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings as frame}
#import bevy_pbr::shadows::fetch_directional_shadow

struct WorldParams {
    mode: vec4<f32>, foliage_debug: vec4<f32>, surface: vec4<f32>, family: vec4<f32>,
    fog_ramp: vec4<f32>, fog_color: vec4<f32>, shadow_color: vec4<f32>, sun_direction: vec4<f32>, decal: vec4<f32>, water: array<vec4<f32>, 4>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: WorldParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var lightmap: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var lm_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var normal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var detail_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var detail_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var macro_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var macro_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var decal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var decal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var specular_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var specular_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(15) var environment_map: texture_cube<f32>;

struct FrameState { shadow: vec4<f32>, clock: vec4<f32>, pca: array<vec4<f32>, 7> }
@group(#{MATERIAL_BIND_GROUP}) @binding(16) var<storage, read> frame_state: FrameState;

// Textures and base UVs both have V flipped by the exporter. Scale in the
// authored coordinate system, then return to the flipped texture rows.
fn scaled_uv(uv: vec2<f32>, scale: f32) -> vec2<f32> {
    return vec2<f32>(uv.x*scale, 1.0-(1.0-uv.y)*scale);
}

@fragment
fn fragment(i: VertexOutput) -> @location(0) vec4<f32> {
    let fam = u32(p.mode.x);
    let flags = u32(p.mode.y);
    var diffuse_uv=i.uv;
    if fam==14u { diffuse_uv+=fract(frame_state.clock.x*p.water[1].xy*vec2<f32>(1.0,-1.0)); }
    let a = textureSample(diffuse, diffuse_sampler, diffuse_uv);
    let lm = textureSampleLevel(lightmap, lm_sampler, i.uv_b, 0.0).rgb;
    // Sample before alpha rejection: implicit derivatives must be uniform.
    var nm = vec3<f32>(0.5,0.5,1.0);
    var detail = vec2<f32>(0.5);
    var overlay_sample = vec3<f32>(0.5);
    var art = vec4<f32>(0.0);
    var masks = vec3<f32>(0.0);
    if (flags & 1u) != 0u && (fam <= 6u || fam == 13u) { nm = textureSample(normal_map,normal_sampler,i.uv).rgb; }
    if (flags & 2u) != 0u && fam != 2u {
        detail = textureSample(detail_map,detail_sampler,scaled_uv(i.uv,p.surface.z)).rg;
    }
    if fam == 5u || fam == 6u || fam == 13u {
        // The reference folds the reflective material's constant detail texel.
        if (flags & 128u) != 0u { detail = textureLoad(detail_map,vec2<i32>(0),0).rg; }
    }
    if (flags & 4u) != 0u { overlay_sample = textureSample(macro_map,macro_sampler,scaled_uv(i.uv,p.surface.x)).rgb; }
    if (flags & 8u) != 0u && (fam == 3u || fam == 4u) { art = textureSample(decal_map,decal_sampler,i.color.xy); }
    if (flags & 16u) != 0u { masks = textureSample(specular_map,specular_sampler,i.uv).rgb; }
    var wn = normalize(i.world_normal);
    let dp1 = dpdx(i.world_position.xyz);
    let dp2 = dpdy(i.world_position.xyz);
    // Exporter flips V and texture rows; recover original UV derivatives.
    let du1 = dpdx(i.uv * vec2<f32>(1.0,-1.0));
    let du2 = dpdy(i.uv * vec2<f32>(1.0,-1.0));
    let dp2p = cross(dp2,wn);
    let dp1p = cross(wn,dp1);
    var kt = dp2p * du1.x + dp1p * du2.x;
    var kb = dp2p * du1.y + dp1p * du2.y;
    kt *= -inverseSqrt(max(dot(kt,kt),1e-12));
    kb *= inverseSqrt(max(dot(kb,kb),1e-12));
#ifdef VERTEX_TANGENTS
    if dot(i.world_tangent.xyz,i.world_tangent.xyz) > 0.01 {
        kt = normalize(i.world_tangent.xyz);
        kb = normalize(cross(wn,kt)) * i.world_tangent.w;
    }
#endif
    let rpos = i.world_position.xyz - frame::view.world_position;
    let vd = -normalize(rpos);
    // Authored render-location direction for the tangent-space sign terms.
    // This does not add directional light energy or a shadow source.
    let sun = p.sun_direction.xyz;
    var d = a.rgb*a.rgb;
    var alpha = 1.0;
    var lin = vec3<f32>(0.0);
    var baked = lm*lm;
    if frame_state.shadow.w>0.0 && (fam<=8u || fam==13u) {
        let view_z=(frame::view.view_from_world*i.world_position).z;
        for (var light_id=0u; light_id<frame::lights.n_directional_lights; light_id+=1u) {
            // The lightmapped receiver source contains only player/board casters.
            if (frame::lights.directional_lights[light_id].flags & 5u)==5u {
                let visibility=fetch_directional_shadow(light_id,i.world_position,wn,view_z);
                baked=min(baked,vec3<f32>(visibility)+frame_state.shadow.rgb);
                break;
            }
        }
    }
    if fam==14u {
        lin=d*p.water[0].y;
    } else if fam==32u {
        lin=d*p.water[0].x;
        alpha=saturate((i.world_position.y-p.water[0].z)/max(p.water[0].y-p.water[0].z,1e-4));
    } else if fam==31u {
        let raw_uv=vec2<f32>(i.uv.x,1.0-i.uv.y);
        let uv=raw_uv*p.water[1].w;
        let sample_uv=vec2<f32>(uv.x,1.0-uv.y);
        let c0=textureSample(normal_map,normal_sampler,sample_uv)*2.0-1.0;
        let c1=textureSample(detail_map,detail_sampler,sample_uv)*2.0-1.0;
        let pca=(vec3<f32>(dot(c0,frame_state.pca[1])+dot(c1,frame_state.pca[2]),
            dot(c0,frame_state.pca[3])+dot(c1,frame_state.pca[4]),
            dot(c0,frame_state.pca[5])+dot(c1,frame_state.pca[6]))+frame_state.pca[0].xyz)*2.0-1.0;
        var overlay=1.0;
        if (flags & 4u)!=0u {
            let uv_overlay=raw_uv*p.surface.x;
            overlay=textureSample(macro_map,macro_sampler,vec2<f32>(uv_overlay.x,1.0-uv_overlay.y)).r;
        }
        let tonedown=2.0*p.water[1].z*saturate(dot(c1,frame_state.pca[6])+overlay);
        let nt=normalize(max(abs(normalize(mix(vec3<f32>(0.0,0.0,1.0),pca,tonedown))),vec3<f32>(0.001)));
        let n=nt.xzy;
        let rv=vd-2.0*n*dot(vd,n);
        var cube=vec3<f32>(0.0);
        if (flags & 64u)!=0u { cube=textureSampleBias(environment_map,diffuse_sampler,vec3<f32>(-rv.x,-rv.y,rv.z),log2(frame::view.viewport.w/640.0)).rgb; }
        let olm=textureSampleLevel(lightmap,lm_sampler,i.uv_b+0.01*nt.xy*vec2<f32>(1.0,-1.0),0.0).rgb;
        let fres=0.25+0.75*pow(1.0-abs(dot(n,vd)),5.0);
        let u=normalize(vec3<f32>(-n.z,0.0,n.x));
        let v=cross(n,u);
        let ax=p.water[2].z/p.water[2].x;
        let ay=p.water[2].z/p.water[2].y;
        let h=normalize(sun+vd);
        let eu=dot(h,u)/ax;
        let ev=dot(h,v)/ay;
        let den=sqrt(abs(dot(n,sun)*dot(n,vd)))*12.566371*ax*ay;
        let ward=exp(-2.0*(eu*eu+ev*ev)/(1.0+dot(h,n)))/max(den,1e-6);
        lin=(cube*olm*olm*fres+ward*p.water[0].rgb)*p.water[1].y;
        alpha=1.0;
    } else if fam==30u {
        let t=frame_state.clock.x;
        // Convert to original UVs for scale/scroll, then back to flipped rows.
        let raw_uv=vec2<f32>(i.uv.x,1.0-i.uv.y);
        let uv1=raw_uv*p.water[2].xy+p.water[1].xy*t;
        let uv2=raw_uv*p.water[2].zw+p.water[1].zw*t;
        let n1=textureSample(normal_map,normal_sampler,vec2<f32>(uv1.x,1.0-uv1.y)).rgb;
        let n2=textureSample(normal_map,normal_sampler,vec2<f32>(uv2.x,1.0-uv2.y)).rgb;
        let vn=normalize((2.0*n1+2.0*n2-2.0)*p.water[0].xzw);
        let water_n=normalize(vn.x*kt+vn.y*kb+vn.z*wn);
        let wlm=textureSampleLevel(lightmap,lm_sampler,i.uv_b+0.01*vn.xz*vec2<f32>(1.0,-1.0),0.0).rgb;
        var lml=wlm*wlm;
        if frame_state.shadow.w>0.0 {
            let vz=(frame::view.view_from_world*i.world_position).z;
            for(var id=0u;id<frame::lights.n_directional_lights;id+=1u) {
                if (frame::lights.directional_lights[id].flags & 5u)==5u {
                    lml=min(lml,vec3<f32>(fetch_directional_shadow(id,i.world_position,wn,vz))+frame_state.shadow.rgb); break;
                }
            }
        }
        let kd=dot(water_n,vec3<f32>(0.58*sign(sun.x),0.62*sign(sun.y),0.39))*2.39562;
        var wm=vec2<f32>(0.0);
        if (flags & 16u)!=0u { wm=saturate(textureSample(specular_map,specular_sampler,i.uv).xz-p.water[3].y); }
        let reflected_light=2.0*water_n*dot(water_n,sun)-sun;
        let ks=pow(max(saturate(dot(vd,reflected_light)),1e-6),p.water[3].z);
        var spec=ks*wm.x*vec3<f32>(2.1,1.8,1.5)*saturate(lml.g-0.1);
        if (flags & 64u)!=0u {
            let rv=vd-2.0*water_n*dot(vd,water_n);
            let cube=textureSampleBias(environment_map,diffuse_sampler,vec3<f32>(-rv.x,-rv.y,rv.z),log2(frame::view.viewport.w/640.0)).rgb;
            let lum=0.3*saturate(4.0*wm.y-2.6);
            spec+=cube*(lml.g+lum*(1.0-lml.g))*wm.y*1.5;
        }
        lin=(lml*kd*d+spec)*p.water[0].y;
        alpha=max(spec.g,p.water[3].w);
    } else if fam == 9u || fam == 10u {
        lin = d * max(lm*lm,vec3<f32>(p.family.y)) * p.family.x;
        if fam == 9u { lin *= p.family.z; }
        alpha = a.a;
    } else if fam == 11u || fam == 12u {
        lin = d;
        if fam == 11u { lin *= p.family.w; }
    } else {
        if (fam == 3u || fam == 4u) && (flags & 8u) != 0u && (flags & 512u) == 0u { d = mix(d,art.rgb*art.rgb,art.a*p.decal.x); }
        if (flags & 4u) != 0u && fam < 13u && (flags & 256u) == 0u { d *= saturate((overlay_sample-0.5)*p.surface.y+0.5); }
        var kd = 0.93429;
        if (fam <= 6u || fam == 13u) && (flags & 1u) != 0u {
            var dxy = vec2<f32>(0.5);
            if (flags & 2u) != 0u && fam != 2u { dxy = detail; }
            if (fam == 5u || fam == 6u || fam == 13u) && (flags & 128u) != 0u { dxy = detail; }
            let raw = vec3<f32>(nm.xy*2.0+dxy*2.0-2.0,nm.z*2.0-1.0);
            var vnd = raw;
            if fam != 2u && fam != 6u { vnd = raw * inverseSqrt(max(dot(raw,raw),1e-12)); }
            var tt = kt;
            var bb = kb;
            if fam == 5u || fam == 6u || fam == 13u {
                let up = vec3<f32>(0.0,1.0,0.0)-wn*wn.y;
                if length(up) > 0.05 {
                    let b2 = normalize(up);
                    let t2 = cross(b2,wn);
                    tt = t2*select(-1.0,1.0,dot(t2,kt)>=0.0);
                    bb = b2*select(-1.0,1.0,dot(b2,kb)>=0.0);
                }
            }
            wn = normalize(raw.x*tt+raw.y*bb+wn*max(raw.z,0.05));
            kd = (vnd.x*0.58*sign(dot(kt,sun))+vnd.y*0.62*sign(dot(kb,sun))+vnd.z*0.39)*2.39562;
        }
        var lml = baked;
        lin = lml*kd*d;
        if fam == 13u { lin = lml*d*a.a; }
        if (flags & 16u) != 0u {
            let lit = vec3<f32>(-0.14,0.5,0.9);
            let reflected = lit-2.0*wn*dot(wn,lit);
            let bp = saturate(dot(vd,-reflected));
            let ks = pow(max(bp,1e-6),10.0+290.0*masks.y);
            lin += ks*vec3<f32>(2.1,1.8,1.5)*lml.g*masks.x;
        }
        if (fam == 5u || fam == 6u || fam == 13u) && (flags & 64u) != 0u {
            let rv = vd-2.0*wn*dot(vd,wn);
            let dir = vec3<f32>(-rv.x,-rv.y,rv.z);
            let cube = textureSampleBias(environment_map,diffuse_sampler,dir,log2(frame::view.viewport.w/640.0)).rgb;
            let rl = 0.3*saturate(4.0*masks.z-2.6);
            let lum = lml.g+rl*(1.0-lml.g);
            lin += cube*lum*masks.z*1.5;
        }
        if fam >= 7u { alpha = a.a; }
        if fam == 13u { alpha *= alpha; }
    }
    var f = saturate(length(rpos)*p.fog_ramp.x+p.fog_ramp.y);
    if p.fog_ramp.z != 1.0 { f = pow(max(f,1e-6),p.fog_ramp.z); }
    var fog_a = 1.0+p.fog_color.a*f;
    if fam <= 8u || fam == 13u { fog_a *= p.surface.w; }
    var xe = max((lin*fog_a+p.fog_color.rgb*f)*p.mode.w,vec3<f32>(0.0));
    // Reduced curve is the full curve with the linear input capped at one.
    if fam == 8u { xe = min(xe,vec3<f32>(1.0)); }
    if p.foliage_debug.w != 0.0 { return vec4<f32>(p.foliage_debug.rgb, 1.0); }
    if a.a < p.mode.z { discard; }
    return vec4<f32>(xe,alpha);
}
