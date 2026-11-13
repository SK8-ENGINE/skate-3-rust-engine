#import bevy_pbr::{mesh_functions, view_transformations::position_world_to_clip}
#import bevy_pbr::mesh_view_bindings as frame
#import bevy_pbr::shadows::fetch_directional_shadow
#import skate_retail::material_bindings as bindings

// Merged world geometry: one draw covers many materials, each vertex naming its
// own. `material_index` must be `@interpolate(flat)` — interpolating it corrupts
// material lookup across triangle interiors without any visible error.
//
// The fragment body below is a direct port. The only edits are `slot` coming from
// the vertex attribute rather than the mesh instance, and explicit gradients
// replacing implicit derivatives (see RFC 1 §7). The family math is unchanged.

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) material_index: u32,
    // Zero where the map authored no frame; the fragment falls back to
    // screen-space derivatives per vertex rather than per mesh.
    @location(6) tangent: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) @interpolate(flat) material_index: u32,
    @location(6) world_tangent: vec4<f32>,
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let sway = bindings::flag_sway(v.material_index, v.color);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local, vec4<f32>(v.position + sway, 1.0));
    out.clip_position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(v.normal, v.instance_index);
    out.uv = v.uv;
    out.uv_b = v.uv_b;
    out.color = v.color;
    out.material_index = v.material_index;
    // Leaves a zero tangent zero, which the fragment tests for.
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local, v.tangent, v.instance_index);
    return out;
}

@fragment
fn fragment(i: VertexOutput) -> @location(0) vec4<f32> {
    let slot = i.material_index;
    let p = bindings::params[slot];
    let frame_state = bindings::frame_state();
    let fam = u32(p.mode.x);
    let flags = u32(p.mode.y);

    // Gradients are built here, in uniform control flow, and threaded into the
    // branches below. Every derived UV is an affine transform of one of these,
    // so its gradients are the same transform of theirs.
    let g = bindings::gradients(i.uv);
    let g_decal = bindings::gradients(i.color.xy);
    let g_detail = bindings::gradients_scaled(g, p.surface.z);
    let g_macro = bindings::gradients_scaled(g, p.surface.x);
    let g_b = bindings::gradients(i.uv_b);

    var diffuse_uv=i.uv;
    var g_diffuse=g;
    // The scroll offset is constant across the primitive, so gradients are unchanged.
    if fam==14u { diffuse_uv+=fract(frame_state.clock.x*p.water[1].xy*vec2<f32>(1.0,-1.0)); }
    // trafficlight_one/two VS: lamp slot floor(4 * uvA.z) picks a status component; above 1 use the "on" UV set.
    if fam==19u || fam==20u {
        let lamp=min(u32(floor(4.0*i.color.x)),3u);
        if frame_state.traffic_lights[fam-19u][lamp]>1.0 { diffuse_uv=i.uv_b; g_diffuse=g_b; }
    }
    var a = bindings::sample_diffuse(slot, diffuse_uv, g_diffuse);
    if (flags & 1024u) != 0u && frame_state.video_feed.x > 0.5 { a = bindings::sample_video_feed(diffuse_uv, g_diffuse); }
    var lm = bindings::sample_lightmap(slot, i.uv_b, 0.0).rgb;
    // Trees, proxy, incandescent and ocean programs take one tap; so does the
    // unknown-shader fallback (0), whose retail programs vary.
    if (fam >= 1u && fam <= 8u) || fam == 13u || fam == 16u || fam == 17u { lm = bindings::sample_lightmap_box(slot, i.uv_b); }
    // Sample before alpha rejection: gradients must stay uniform.
    var nm = vec3<f32>(0.5,0.5,1.0);
    var detail = vec2<f32>(0.5);
    var overlay_sample = vec3<f32>(0.5);
    var art = vec4<f32>(0.0);
    var masks = vec3<f32>(0.0);
    if (flags & 1u) != 0u && (fam <= 6u || fam == 13u || fam == 15u || (fam >= 50u && fam <= 52u)) { nm = bindings::sample_normal_map(slot,i.uv,g).rgb; }
    if (flags & 2u) != 0u && fam != 2u {
        detail = bindings::sample_detail_map(slot,bindings::scaled_uv(i.uv,p.surface.z),g_detail).rg;
    }
    if fam == 5u || fam == 6u || fam == 13u {
        // The reference folds the reflective material's constant detail texel.
        if (flags & 128u) != 0u { detail = bindings::load_detail(slot,vec2<i32>(0),0).rg; }
    }
    if (flags & 4u) != 0u { overlay_sample = bindings::sample_macro_map(slot,bindings::scaled_uv(i.uv,p.surface.x),g_macro).rgb; }
    if (flags & 8u) != 0u && (fam == 3u || fam == 4u || fam == 51u) { art = bindings::sample_decal_map(slot,i.color.xy,g_decal); }
    if (flags & 16u) != 0u { masks = bindings::sample_specular_map(slot,i.uv,g).rgb; }
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
    // Prefer the authored frame where the map supplied one. The derivative basis
    // above is only a stand-in, and it disagrees with the authored orientation on
    // surfaces whose UVs were laid out by hand.
    if dot(i.world_tangent.xyz,i.world_tangent.xyz) > 0.01 {
        kt = normalize(i.world_tangent.xyz);
        kb = normalize(cross(wn,kt)) * i.world_tangent.w;
    }
    let rpos = i.world_position.xyz - frame::view.world_position;
    let vd = -normalize(rpos);
    // Authored render-location direction for the tangent-space sign terms.
    // This does not add directional light energy or a shadow source.
    let sun = p.sun_direction.xyz;
    var d = a.rgb*a.rgb;
    var alpha = 1.0;
    var lin = vec3<f32>(0.0);
    var baked = lm*lm;
    if frame_state.shadow.w>0.0 && (fam<=8u || fam==13u || fam==16u || fam==17u) {
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
        let g_pca=bindings::gradients_scaled(g,p.water[1].w);
        let c0=bindings::sample_normal_map(slot,sample_uv,g_pca)*2.0-1.0;
        let c1=bindings::sample_detail_map(slot,sample_uv,g_pca)*2.0-1.0;
        let pca=(vec3<f32>(dot(c0,frame_state.pca[1])+dot(c1,frame_state.pca[2]),
            dot(c0,frame_state.pca[3])+dot(c1,frame_state.pca[4]),
            dot(c0,frame_state.pca[5])+dot(c1,frame_state.pca[6]))+frame_state.pca[0].xyz)*2.0-1.0;
        var overlay=1.0;
        if (flags & 4u)!=0u {
            let uv_overlay=raw_uv*p.surface.x;
            overlay=bindings::sample_macro_map(slot,vec2<f32>(uv_overlay.x,1.0-uv_overlay.y),g_macro).r;
        }
        let tonedown=2.0*p.water[1].z*saturate(dot(c1,frame_state.pca[6])+overlay);
        var nt=normalize(max(abs(normalize(mix(vec3<f32>(0.0,0.0,1.0),pca,tonedown))),vec3<f32>(0.001)));
        // Water look (water_look.rs, not retail): calm open-sea swells, shading only (the ocean mesh is far too
        // coarse to move). Three sines, each at its deep-water speed for its length; each fades once a pixel covers a
        // good part of its wavelength, so the far sea does not shimmer. Amplitude 0 (retail) skips it.
        let swell=frame_state.ocean_swell;
        if swell.x>0.0 {
            let px=max(length(dp1.xz),length(dp2.xz));
            var slope=vec2<f32>(0.0);
            let dirs=array<vec2<f32>,3>(vec2<f32>(0.8,0.6),vec2<f32>(0.97,-0.24),vec2<f32>(0.42,0.91));
            let lens=array<f32,3>(1.0,0.61,1.37);
            let amps=array<f32,3>(1.0,0.55,0.45);
            for (var w=0u; w<3u; w++) {
                let len=swell.y*lens[w];
                let k=6.2831853/len;
                let speed=swell.z*sqrt(lens[w]);
                let fade=1.0-smoothstep(0.1,0.35,px/len);
                slope+=dirs[w]*(amps[w]*fade*cos(k*(dot(dirs[w],i.world_position.xz)-speed*frame_state.clock.x)));
            }
            nt=normalize(nt+vec3<f32>(-slope*swell.x,0.0));
        }
        let n=nt.xzy;
        let rv=vd-2.0*n*dot(vd,n);
        var cube=vec3<f32>(0.0);
        if (flags & 64u)!=0u { cube=bindings::sample_environment(slot,vec3<f32>(-rv.x,-rv.y,rv.z),log2(frame::view.viewport.w/640.0)).rgb; }
        let olm=bindings::sample_lightmap(slot,i.uv_b+0.01*nt.xy*vec2<f32>(1.0,-1.0),0.0).rgb;
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
    } else if fam==30u || fam==33u {
        // Native PCA update time (retail_render::WaterAnimation), or the water
        // look's seamless clock (retail_render::water_time).
        let t=frame_state.clock.z;
        // Water look layer (water_look.rs, project choices); retail values
        // (1, 0, 0, 0 and tint 1) leave the retail program unchanged.
        let look=select(vec4<f32>(1.0,0.0,0.0,0.0),frame_state.water_look,fam==33u);
        // Flowing water (family 30, University's fountains) gets the look's ripple scale (water_tint.w), blur and
        // tint when it is on; 0 = the retail flowingwater_defaultPS.
        // Steep flowing water (falls) fades back to the retail program over the lip; the look's dense ripples and blur flatten it: docs/hails-additions/09-water.md#better-water-falls-keep-retail-flow
        let steep=1.0-smoothstep(0.55,0.8,abs(normalize(i.world_normal).y));
        let flow=select(0.0,1.0-steep,fam==30u && frame_state.water_tint.w>0.0);
        let lookw=select(flow,1.0,fam==33u);
        // Water look fountains (frame_state.fountain, not retail): flowing water scrolls on the cascade clock
        // (fountain.w), and the scroll follows the ripple scale so the surface keeps retail's speed.
        let cascade=fam==30u && frame_state.fountain.x>0.0;
        let tc=select(t,frame_state.fountain.w,cascade);
        // Convert to original UVs for scale/scroll, then back to flipped rows.
        let raw_uv=vec2<f32>(i.uv.x,1.0-i.uv.y);
        // water_defaultPS 11..13: c20 * c21.x, times the look's ripple scale.
        let uv_scale=select(mix(1.0,frame_state.water_tint.w,flow),p.water[3].x*look.x,fam==33u);
        let sk=select(1.0,uv_scale,cascade);
        let uv1=raw_uv*p.water[2].xy*uv_scale+p.water[1].xy*tc*sk;
        let uv2=raw_uv*p.water[2].zw*uv_scale+p.water[1].zw*tc*sk;
        // The double V flip cancels, so per-axis scaling is all that carries over.
        let s1=p.water[2].xy*uv_scale;
        let s2=p.water[2].zw*uv_scale;
        let g1=bindings::Gradients(g.ddx*s1,g.ddy*s1);
        let g2=bindings::Gradients(g.ddx*s2,g.ddy*s2);
        let n1=bindings::sample_normal_map(slot,vec2<f32>(uv1.x,1.0-uv1.y),g1);
        let n2=bindings::sample_normal_map(slot,vec2<f32>(uv2.x,1.0-uv2.y),g2);
        var vn=normalize((2.0*n1.rgb+2.0*n2.rgb-2.0)*p.water[0].xzw);
        var water_n=normalize(vn.x*kt+vn.y*kb+vn.z*wn);
        var sample_uv=i.uv;
        if fam==33u {
            let c1=bindings::sample_detail_map(slot,vec2<f32>(uv1.x,1.0-uv1.y),g1)*2.0-1.0;
            let c2=bindings::sample_detail_map(slot,vec2<f32>(uv2.x,1.0-uv2.y),g2)*2.0-1.0;
            let a1=n1*2.0-1.0;
            let a2=n2*2.0-1.0;
            // water_defaultPS instructions 22..54: the native mean is XYZ,
            // weights are R/G/B pairs. FrameState stores the ocean's R/B/G
            // arrangement; recover those original registers here. `pca_slow`
            // is the animation at the look's wave rate (equal to `pca` at
            // the retail rate).
            let mean=frame_state.pca_slow[0].xzy;
            let pca1=vec3<f32>(dot(a1,frame_state.pca_slow[1])+dot(c1,frame_state.pca_slow[2])+mean.x,
                dot(a1,frame_state.pca_slow[5])+dot(c1,frame_state.pca_slow[6])+mean.y,
                dot(a1,frame_state.pca_slow[3])+dot(c1,frame_state.pca_slow[4])+mean.z);
            let pca2=vec3<f32>(dot(a2,frame_state.pca_slow[1])+dot(c2,frame_state.pca_slow[2])+mean.x,
                dot(a2,frame_state.pca_slow[5])+dot(c2,frame_state.pca_slow[6])+mean.y,
                dot(a2,frame_state.pca_slow[3])+dot(c2,frame_state.pca_slow[4])+mean.z);
            let first=vec3<f32>((pca2.x*2.0-1.0)*p.water[0].z,
                1.0+2.0*(pca2.y-1.0)*p.water[0].z,(pca2.z*2.0-1.0)*p.water[0].z);
            let second=vec3<f32>((pca1.x*2.0-1.0)*p.water[0].w,
                1.0+2.0*(pca1.y-1.0)*p.water[0].w,(pca1.z*2.0-1.0)*p.water[0].w);
            // Instructions 47..54 and 69..71 normalize separate reflection
            // and refraction vectors. There is no third wave or area blend.
            water_n=first*inverseSqrt(max(dot(first,first),1e-12));
            if look.z>0.5 || look.w>0.5 {
                // Water look (not retail): extra rotated PCA sample and swells, calmer on small bodies: docs/hails-additions/35-world-shading-parity.md#water-look-extra-layers
                var third=first;
                if look.z>0.5 {
                    let k=0.613;
                    let uv3=mat2x2<f32>(0.8,0.6,-0.6,0.8)*(raw_uv*p.water[2].zw*uv_scale*k)+p.water[1].wz*t*k+vec2<f32>(0.37,0.71);
                    let s3=s2*k;
                    let g3=bindings::Gradients(g.ddx*s3,g.ddy*s3);
                    let a3=bindings::sample_normal_map(slot,vec2<f32>(uv3.x,1.0-uv3.y),g3)*2.0-1.0;
                    let c3=bindings::sample_detail_map(slot,vec2<f32>(uv3.x,1.0-uv3.y),g3)*2.0-1.0;
                    let pca3=vec3<f32>(dot(a3,frame_state.pca_slow[1])+dot(c3,frame_state.pca_slow[2])+mean.x,
                        dot(a3,frame_state.pca_slow[5])+dot(c3,frame_state.pca_slow[6])+mean.y,
                        dot(a3,frame_state.pca_slow[3])+dot(c3,frame_state.pca_slow[4])+mean.z);
                    third=vec3<f32>((pca3.x*2.0-1.0)*p.water[0].z,
                        1.0+2.0*(pca3.y-1.0)*p.water[0].z,(pca3.z*2.0-1.0)*p.water[0].z);
                }
                let up=vec3<f32>(0.0,1.0,0.0);
                let size=select(1.0,saturate(log2(max(p.decal.y,1.0)/100.0)/log2(40.0)),look.w>0.5 && p.decal.y>0.0);
                let swell=select(0.0,1.0,look.z>0.5);
                water_n=normalize(2.0*up+(first+third-2.0*up)*(0.6+0.4*size)+(second-up)*2.0*size*size*swell);
            }
            let refract=second*inverseSqrt(max(dot(second,second),1e-12));
            sample_uv+=0.02*refract.xz*vec2<f32>(1.0,-1.0);
            d=bindings::sample_diffuse(slot,sample_uv,g).rgb;
            d*=d;
            vn=water_n;
        }
        // Water look fountains: flat basins take the donor's large-water program; cascades keep their flow: docs/hails-additions/35-world-shading-parity.md#fountain-shading
        var mult=p.water[0].y;
        var mask_share=1.0;
        var spec_floor=p.water[3].y;
        var spec_power=p.water[3].z;
        var foam=0.0;
        if cascade {
            foam=steep*frame_state.fountain.z*saturate(n1.b*2.0-1.0+0.5*(n2.r-0.5));
            if steep<1.0 && p.fountain[0].y>0.0 {
                let q=p.fountain;
                let tile=frame_state.fountain.x;
                let ratio=q[2].xy/max(q[2].zw,vec2<f32>(1e-6));
                let fine=i.world_position.xz/tile;
                let fu1=fine*ratio+q[1].xy*t;
                let fu2=fine+q[1].zw*t;
                let gf1=bindings::Gradients(dp1.xz*ratio/tile,dp2.xz*ratio/tile);
                let gf2=bindings::Gradients(dp1.xz/tile,dp2.xz/tile);
                let b1=bindings::sample_macro_map(slot,fu1,gf1)*2.0-1.0;
                let b2=bindings::sample_macro_map(slot,fu2,gf2)*2.0-1.0;
                let e1=bindings::sample_detail_map(slot,fu1,gf1)*2.0-1.0;
                let e2=bindings::sample_detail_map(slot,fu2,gf2)*2.0-1.0;
                let mean=frame_state.pca_slow[0].xzy;
                let pb1=vec3<f32>(dot(b1,frame_state.pca_slow[1])+dot(e1,frame_state.pca_slow[2])+mean.x,
                    dot(b1,frame_state.pca_slow[5])+dot(e1,frame_state.pca_slow[6])+mean.y,
                    dot(b1,frame_state.pca_slow[3])+dot(e1,frame_state.pca_slow[4])+mean.z);
                let pb2=vec3<f32>(dot(b2,frame_state.pca_slow[1])+dot(e2,frame_state.pca_slow[2])+mean.x,
                    dot(b2,frame_state.pca_slow[5])+dot(e2,frame_state.pca_slow[6])+mean.y,
                    dot(b2,frame_state.pca_slow[3])+dot(e2,frame_state.pca_slow[4])+mean.z);
                let first=vec3<f32>((pb2.x*2.0-1.0)*q[0].z,1.0+2.0*(pb2.y-1.0)*q[0].z,(pb2.z*2.0-1.0)*q[0].z);
                let second=vec3<f32>((pb1.x*2.0-1.0)*q[0].w,1.0+2.0*(pb1.y-1.0)*q[0].w,(pb1.z*2.0-1.0)*q[0].w);
                let up=vec3<f32>(0.0,1.0,0.0);
                // Body calming as on the large water, but never below the look's fountain floor.
                let size=select(1.0,max(frame_state.fountain.y,saturate(log2(max(p.decal.y,1.0)/100.0)/log2(40.0))),frame_state.water_look.w>0.5);
                let calm_n=normalize(2.0*up+2.0*(first-up)*(0.6+0.4*size)+(second-up)*2.0*size*size);
                // The look's tilt gain (fountain_ripple.x): a flat basin colour shows the waves only through the lighting.
                let basin_n=normalize(vec3<f32>(calm_n.x*frame_state.fountain_ripple.x,calm_n.y,calm_n.z*frame_state.fountain_ripple.x));
                // The basin takes the look's colour (fountain_body), not the donor's diffuse; the donor gives the waves only.
                d=mix(frame_state.fountain_body.rgb,d,steep);
                mask_share=mix(frame_state.fountain_body.w,1.0,steep);
                water_n=normalize(mix(basin_n,water_n,steep));
                vn=mix(basin_n,vn,steep);
                mult=mix(q[0].y,mult,steep);
                spec_floor=mix(q[3].y,spec_floor,steep);
                spec_power=mix(q[3].z,spec_power,steep);
            }
        }
        let water_lm_uv=i.uv_b+0.01*vn.xz*vec2<f32>(1.0,-1.0);
        // water_defaultPS 96..99 and flowingwater_defaultPS 60..63, 67..70.
        let wlm=bindings::sample_lightmap_box(slot,water_lm_uv);
        var lml=wlm*wlm;
        if frame_state.shadow.w>0.0 {
            let vz=(frame::view.view_from_world*i.world_position).z;
            for(var id=0u;id<frame::lights.n_directional_lights;id+=1u) {
                if (frame::lights.directional_lights[id].flags & 5u)==5u {
                    // water_defaultPS / flowingwater_defaultPS use the same RGB floor as
                    // the other world receivers (their lightmap sits in G,B,R registers,
                    // so the literal reads 0.09, 0.13, 0.05 in register order).
                    lml=min(lml,vec3<f32>(fetch_directional_shadow(id,i.world_position,wn,vz))+frame_state.shadow.rgb); break;
                }
            }
        }
        let kd=dot(water_n,vec3<f32>(0.58*sign(sun.x),0.62*sign(sun.y),0.39))*2.39562;
        var wm=vec2<f32>(0.0);
        if (flags & 16u)!=0u { wm=saturate(bindings::sample_specular_map(slot,sample_uv,g).xz-spec_floor)*mask_share; }
        let reflected_light=2.0*water_n*dot(water_n,sun)-sun;
        let ks=pow(max(saturate(dot(vd,reflected_light)),1e-6),spec_power);
        var spec=ks*wm.x*vec3<f32>(2.1,1.8,1.5)*saturate(lml.g-0.1);
        if (flags & 64u)!=0u {
            let rv=vd-2.0*water_n*dot(vd,water_n);
            // Instruction 95 uses the environment fetch without an added
            // mip bias or color multiplier; the water look adds both (family
            // 33: an even steel-blue reflection as in the Aletown canal
            // footage, where the sharp cube read grey with bright/dark bands).
            // Fountain water (flat basins included) keeps a sharp reflection: the blur greyed it next to the falls (09-water.md#better-water-falls-keep-retail-flow).
            let blur=frame_state.water_look.y*select(lookw,0.0,cascade);
            let cube=bindings::sample_environment(slot,vec3<f32>(-rv.x,-rv.y,rv.z),log2(frame::view.viewport.w/640.0)+blur).rgb;
            let tint=mix(vec3<f32>(1.0),frame_state.water_tint.rgb,lookw);
            let lum=0.3*saturate(4.0*wm.y-2.6);
            spec+=cube*tint*(lml.g+lum*(1.0-lml.g))*wm.y*1.5;
        }
        lin=(lml*kd*d+spec)*mult+foam*lml*0.6;
        alpha=max(max(spec.g,p.water[3].w),foam);
    } else if fam == 9u || fam == 10u {
        // tree_defaultPS / treeanimate_defaultPS 6..7: max(lm, g_ViewDotLight.y) * g_ViewDotLight.x.
        lin = d * max(lm*lm,vec3<f32>(frame_state.view_dot_light.y)) * frame_state.view_dot_light.x;
        if fam == 9u { lin *= p.family.z; }
        alpha = a.a;
    } else if fam == 21u {
        // vertexanimate_defaultPS 5..8, 18: lightmap^2 * g_ViewDotLight.x * diffuse^2;
        // one lightmap tap, no shadow, no m_params; alpha = diffuse alpha.
        lin = d*(lm*lm)*frame_state.view_dot_light.x;
        alpha = a.a;
    } else if fam == 19u || fam == 20u {
        // trafficlight_*_defaultPS 3, 13: unsquared diffuse * m_params; alpha out
        // (masked by the RGB-only blend state 26).
        lin = a.rgb;
        alpha = a.a;
    } else if fam == 11u || fam == 12u || fam == 18u {
        lin = d;
        if fam == 11u { lin *= p.family.w; }
        // transparentincandescent_defaultPS 14: output alpha = diffuse alpha.
        if fam == 18u { alpha = a.a; }
    } else if fam >= 50u && fam <= 52u {
        // environmentpark{diffuse,decal,alphatest}_defaultPS: the skate.Park pieces (research b442, slot numbers of
        // the diffuse program; doc 39 "Park shading"). Tint and the two shadow colours: ParkDraw (b443).
        let park = bindings::material_park;
        // 14..28: 2 * normal - 1, NOT normalised before the frame (diffuse); decal adds the detail normal
        // (2 * detail - 2); alphatest is shaped like dynamicobject: detail added, normalised.
        var nt = nm*2.0-1.0;
        if fam != 50u && (flags & 2u) != 0u { nt = vec3<f32>(nm.xy*2.0+detail*2.0-2.0,nm.z*2.0-1.0); }
        if fam == 52u { nt = nt*inverseSqrt(max(dot(nt,nt),1e-12)); }
        let n = normalize(nt.x*kt+nt.y*kb+nt.z*wn);
        // 57..81: the tangent-frame pseudo light, as dynamicobject's.
        let kd = (0.58*sign(dot(kt,sun))*nt.x+0.62*sign(dot(kb,sun))*nt.y+0.39*nt.z)*2.39562;
        let nl = dot(n,sun);
        var csm = 1.0;
        if frame_state.shadow.w>0.0 {
            let view_z=(frame::view.view_from_world*i.world_position).z;
            for (var light_id=0u; light_id<frame::lights.n_directional_lights; light_id+=1u) {
                if (frame::lights.directional_lights[light_id].flags & 5u)==5u {
                    csm=fetch_directional_shadow(light_id,i.world_position,wn,view_z);
                    break;
                }
            }
        }
        // Park shadows and projected lightmap (b439 / b440 / b444): docs/hails-additions/35-world-shading-parity.md#park-projected-lightmap
        var plm = vec3<f32>(1.0);
        let lb = park.lightmap_bounds;
        if lb.y != lb.x && lb.w != lb.z && (flags & 32u) != 0u {
            let w = i.world_position.xyz;
            let dx = lb.y-lb.x;
            let dz = lb.w-lb.z;
            let ly = select(sun.y,1e-4,abs(sun.y)<1e-4);
            let u = (w.z-lb.z)/dz+w.y*sun.z/(ly*abs(dz));
            let v = (w.x-lb.x)/dx-w.y*sun.x/(ly*abs(dx));
            plm = bindings::sample_lightmap(slot,vec2<f32>(u,1.0-v),0.0).rgb;
        }
        let occ = min(min(mix(park.dynamic_shadow.rgb,vec3<f32>(1.0),csm),
            mix(park.exponential_shadow.rgb,vec3<f32>(1.0),saturate(10.0*nl)*csm)),plm);
        // 15, 19: diffuse^2 (decal: lerp to decal^2 by decal alpha first) times the tint.
        if fam == 51u && (flags & 8u) != 0u { d = mix(d,art.rgb*art.rgb,art.a); }
        d *= park.tint.rgb;
        var light = kd*occ;
        if fam == 52u {
            // alphatest: dynamicobject's direct + counter + ambient (m_params[1]), occ in place of its shadow.
            let counter = p.water[1].w*saturate(dot(n,vec3<f32>(-sun.x,sun.y,-sun.z)));
            light = kd*(saturate(nl)*occ+vec3<f32>(counter)+p.water[1].xyz);
        }
        lin = light*d;
        if (flags & 16u) != 0u {
            // The world literal light reflected about n, power 10 + 290 * specular.g, not shadowed (alphatest:
            // times occ, one channel; which one is open, green taken as the world path does).
            let lit = vec3<f32>(-0.14,0.5,0.9);
            let reflected = lit-2.0*n*dot(n,lit);
            let ks = pow(max(saturate(dot(vd,-reflected)),1e-6),10.0+290.0*masks.y);
            var s = ks*vec3<f32>(2.1,1.8,1.5)*masks.x;
            if fam == 52u { s *= occ.g; }
            lin += s;
        }
        // alphatest 112: oC0.w = the raw diffuse alpha; the others are opaque.
        alpha = select(1.0,a.a,fam == 52u);
    } else if fam == 15u {        // dynamicobject_defaultPS (props). Instruction numbers are the shader's
        // ALU slots; see doc 27 "D9". c9 g_vLightDir is the authored sun
        // direction, c14/c15 m_params are p.water[0]/p.water[1].
        // 12, 20..24: detail normal added in tangent space.
        var dxy = vec2<f32>(0.5);
        if (flags & 2u) != 0u { dxy = detail; }
        let raw = vec3<f32>(nm.xy*2.0+dxy*2.0-2.0,nm.z*2.0-1.0);
        // 25..28: normalised before both the frame transform and the kd term.
        let vnd = raw*inverseSqrt(max(dot(raw,raw),1e-12));
        // 29..34: world normal from the interpolated frame.
        let n = normalize(vnd.x*kt+vnd.y*kb+vnd.z*wn);
        // 35..37: sun term and its back-face step.
        let nl = dot(n,sun);
        // 58..60, 72..81: signs of the sun in the unperturbed tangent frame.
        let s = vec2<f32>(0.58*sign(dot(kt,sun)),0.62*sign(dot(kb,sun)));
        // 86..88: tangent-space pseudo light, scaled by 2.3956 at 96.
        let kd_raw = vnd.x*s.x+vnd.y*s.y+vnd.z*0.39;
        let kd = kd_raw*2.39562;
        // 53..56, 66, 70..82: dynamic shadow map (retail: 4-tap CSM atlas).
        var csm = 1.0;
        if frame_state.shadow.w>0.0 {
            let view_z=(frame::view.view_from_world*i.world_position).z;
            for (var light_id=0u; light_id<frame::lights.n_directional_lights; light_id+=1u) {
                if (frame::lights.directional_lights[light_id].flags & 5u)==5u {
                    csm=fetch_directional_shadow(light_id,i.world_position,wn,view_z);
                    break;
                }
            }
        }
        // Retail also reads the static world shadow map (floored at g_CSMSelfBias.w); no world shadow pass yet: lit.
        let world_visibility = 1.0;
        let shade = min(select(0.0,1.0,nl>=0.0)*csm,world_visibility);
        // 57, 64, 68, 88: counter light (sun mirrored about the vertical),
        // weighted by m_params[1].w.
        let counter = p.water[1].w*saturate(dot(n,vec3<f32>(-sun.x,sun.y,-sun.z)));
        // 86, 92..95, 97: direct + counter + ambient (m_params[1].rgb).
        let light = vec3<f32>(saturate(nl)*shade+counter)+p.water[1].xyz;
        lin = kd*light*d;
        if (flags & 16u) != 0u {
            // 61..63, 70, 81, 83: view direction in the tangent frame.
            let vt = normalize(vec3<f32>(dot(vd,kt),dot(vd,kb),dot(vd,wn)));
            // 89..91: the pseudo light reflected about the detail normal.
            let reflected = vec3<f32>(s,0.39)-2.0*kd_raw*vnd;
            // 66, 71, 92..95: power 10 + 290 * specular.g.
            let ks = pow(max(saturate(dot(vt,-reflected)),1e-6),10.0+290.0*masks.y);
            // 96, 98..100: shadowed, masked by specular.r.
            lin += ks*vec3<f32>(2.1,1.8,1.5)*shade*masks.x;
        }
        // Alpha-tested props test the diffuse alpha (WORLD_ALPHA_CUTOFF).
        alpha = 1.0;
    } else {
        if (fam == 3u || fam == 4u) && (flags & 8u) != 0u && (flags & 512u) == 0u { d = mix(d,art.rgb*art.rgb,art.a*p.decal.x*frame_state.clock.w); }
        if (flags & 4u) != 0u && fam < 13u && (flags & 256u) == 0u { d *= saturate((overlay_sample-0.5)*p.surface.y+0.5); }
        // Flat-normal kd (0.39 * 2.39562) of the normal-mapped programs. Families
        // 7 / 8 (alphatestdefaultenvironment_defaultPS 27-34, environmentdiffuse_defaultPS
        // 26-34) have no kd term: lin = min(lightmap^2, vis + floor) * diffuse^2 * m_params[0].y.
        var kd = select(0.93429, 1.0, fam == 7u || fam == 8u);
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
        // advertisement_defaultPS 32: max with g_ViewDotLight.z; 33: no kd.
        if fam == 17u { lml = max(lml,vec3<f32>(frame_state.view_dot_light.z)); kd = 1.0; }
        lin = lml*kd*d;
        // transparentenvironment_defaultPS 39, 43: diffuse^2 * lightmap * alpha, no kd.
        if fam == 13u || fam == 16u { lin = lml*d*a.a; }
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
            let cube = bindings::sample_environment(slot,dir,log2(frame::view.viewport.w/640.0)).rgb;
            let rl = 0.3*saturate(4.0*masks.z-2.6);
            let lum = lml.g+rl*(1.0-lml.g);
            lin += cube*lum*masks.z*1.5;
        }
        if fam >= 7u { alpha = a.a; }
        // transparentenvironment_defaultPS 53: oC0.w = alpha * alpha.
        if fam == 13u || fam == 16u { alpha *= alpha; }
    }
    var f = saturate(length(rpos)*p.fog_ramp.x+p.fog_ramp.y);
    if p.fog_ramp.z != 1.0 { f = pow(max(f,1e-6),p.fog_ramp.z); }
    var fog_a = 1.0+p.fog_color.a*f;
    if fam <= 8u || fam == 12u || fam == 13u || (fam >= 16u && fam <= 20u) { fog_a *= p.surface.w; }
    // dynamicobject_defaultPS 98, 101: m_params[0].y scales the fogged light.
    // environmentpark_*PS: the same m_params[0].y fog scale (research b442).
    if fam == 15u || (fam >= 50u && fam <= 52u) { fog_a *= p.water[0].y; }
    var xe = max((lin*fog_a+p.fog_color.rgb*f)*p.mode.w,vec3<f32>(0.0));
    // Reduced curve is the full curve with the linear input capped at one.
    // advertisement_defaultPS 36..41 use the same reduced curve.
    if fam == 8u || fam == 17u { xe = min(xe,vec3<f32>(1.0)); }
    if p.foliage_debug.w != 0.0 { return vec4<f32>(p.foliage_debug.rgb, 1.0); }
    // Compiled only for the classes whose materials carry a cutoff. The opaque
    // classes leave it out so the hardware can write depth before shading and
    // reject what is hidden; see RenderClass.
#ifdef WORLD_ALPHA_CUTOFF
    // The retail fence state tests the output alpha (diffuse alpha squared).
    if select(a.a, alpha, fam == 16u) < p.mode.z { discard; }
#endif
    // material_park.tint.w: opacity of a fading prop's own blended copy (1 everywhere else).
    return vec4<f32>(xe,alpha*bindings::material_park.tint.w);
}
