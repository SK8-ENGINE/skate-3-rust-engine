//! GPU-free validation of the original retail material shaders.
//! Bevy vertex interfaces are real; unrelated lighting functions/resources use
//! interface fixtures. Runtime pipeline/device validation remains a user check.
use naga_oil::compose::{Composer, ComposableModuleDescriptor, NagaModuleDescriptor, ShaderDefValue};
use std::collections::HashMap;

#[test]
fn retail_material_uses_original_bindings() {
    use bevy::render::render_resource::AsBindGroup;
    assert!(super::RetailWorldMaterial::bindless_descriptor().is_none());
    assert!(super::RetailWorldMaterial::bindless_slot_count().is_none());
}

fn validate(bindless: bool, prepass: bool, extras: &[&str]) {
    let mut defs = HashMap::from([("MATERIAL_BIND_GROUP".into(),ShaderDefValue::UInt(3))]);
    for &name in ["VERTEX_UVS_A","VERTEX_UVS_B","VERTEX_TANGENTS","VERTEX_COLORS","VERTEX_NORMALS","VERTEX_OUTPUT_INSTANCE_INDEX"].iter().chain(extras) {
        defs.insert(name.into(),ShaderDefValue::Bool(true));
    }
    if bindless { defs.insert("BINDLESS".into(),ShaderDefValue::Bool(true)); }
    let mut composer = Composer::default().with_capabilities(naga::valid::Capabilities::all());
    let fixtures = [
        ("forward",include_str!("../../../vendor/bevy_pbr/src/render/forward_io.wgsl")),
        ("prepass",include_str!("../../../vendor/bevy_pbr/src/prepass/prepass_io.wgsl")),
        ("mesh", "#define_import_path bevy_pbr::mesh_bindings\nstruct Mesh { material_and_lightmap_bind_group_slot: u32 }\n@group(2) @binding(0) var<storage> mesh: array<Mesh>;"),
        ("frame", "#define_import_path bevy_pbr::mesh_view_bindings\nstruct View {view_from_world:mat4x4<f32>,viewport:vec4<f32>,world_position:vec3<f32>,padding:f32}\nstruct Light {flags:u32}\nstruct Lights {n_directional_lights:u32,directional_lights:array<Light,10>}\n@group(0) @binding(0) var<uniform> view:View;\n@group(0) @binding(1) var<storage> lights:Lights;"),
        ("shadows", "#define_import_path bevy_pbr::shadows\nfn fetch_directional_shadow(id:u32,p:vec4<f32>,n:vec3<f32>,z:f32)->f32 {return 1.0;}"),
        ("motion", "#define_import_path bevy_pbr::pbr_prepass_functions\nfn calculate_motion_vector(p:vec4<f32>,q:vec4<f32>)->vec2<f32> {return vec2<f32>(0.0);}"),
    ];
    for (path,source) in fixtures {
        if let Err(error) = composer.add_composable_module(ComposableModuleDescriptor {source,file_path:path,shader_defs:defs.clone(),..Default::default()}) {
            panic!("{}",error.emit_to_string(&composer));
        }
    }
    let source = if prepass {include_str!("retail_depth.wgsl")} else {include_str!("retail_world.wgsl")};
    let module = composer.make_naga_module(NagaModuleDescriptor {source,file_path:"retail",shader_defs:defs,..Default::default()})
        .unwrap_or_else(|e| panic!("{}",e.emit_to_string(&composer)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
}

#[test]
fn original_material_shaders_validate() {
    for bindless in [false] {
        validate(bindless,false,&[]);
        validate(bindless,true,&[]);
        validate(bindless,true,&["PREPASS_FRAGMENT","NORMAL_PREPASS","NORMAL_PREPASS_OR_DEFERRED_PREPASS","MOTION_VECTOR_PREPASS","UNCLIPPED_DEPTH_ORTHO_EMULATION"]);
    }
}
