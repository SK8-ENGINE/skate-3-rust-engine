//! Naga validation for the retail shaders, with no GPU and no game app.
//!
//! Worth having because the interesting failure modes here are static: a slot
//! record read under non-uniform control flow, a binding number that drifted out
//! of step with `AsBindGroup`, or a struct whose WGSL layout stopped matching the
//! bytes Rust encodes. All of those are compile errors that Naga will name, and
//! none of them need a window.
//!
//! Bevy's own vertex interfaces are used for real, from the vendored sources.
//! Lighting resources and functions the retail shaders only *call* are interface
//! fixtures — the point is to type-check our code, not re-validate Bevy's.
use naga_oil::compose::{
    ComposableModuleDescriptor, Composer, NagaModuleDescriptor, ShaderDefValue,
};
use std::collections::HashMap;

/// Composes one of our shaders against the Bevy interface and validates it.
fn validate(source: &str, extras: &[&str]) -> naga::Module {
    let mut defs = HashMap::from([("MATERIAL_BIND_GROUP".into(), ShaderDefValue::UInt(3))]);
    for &name in [
        "VERTEX_UVS_A",
        "VERTEX_UVS_B",
        "VERTEX_TANGENTS",
        "VERTEX_COLORS",
        "VERTEX_NORMALS",
        "VERTEX_OUTPUT_INSTANCE_INDEX",
    ]
    .iter()
    .chain(extras)
    {
        defs.insert(name.into(), ShaderDefValue::Bool(true));
    }
    let mut composer = Composer::default().with_capabilities(naga::valid::Capabilities::all());
    let fixtures = [
        (
            "forward",
            include_str!("../../../vendor/bevy_pbr/src/render/forward_io.wgsl").to_string(),
        ),
        (
            "prepass",
            include_str!("../../../vendor/bevy_pbr/src/prepass/prepass_io.wgsl").to_string(),
        ),
        (
            "mesh",
            "#define_import_path bevy_pbr::mesh_bindings\n\
             struct Mesh { material_and_lightmap_bind_group_slot: u32 }\n\
             @group(2) @binding(0) var<storage> mesh: array<Mesh>;"
                .to_string(),
        ),
        (
            "frame",
            "#define_import_path bevy_pbr::mesh_view_bindings\n\
             struct View {view_from_world:mat4x4<f32>,clip_from_world:mat4x4<f32>,viewport:vec4<f32>,world_position:vec3<f32>,padding:f32}\n\
             struct Light {flags:u32}\n\
             struct Lights {n_directional_lights:u32,directional_lights:array<Light,10>}\n\
             @group(0) @binding(0) var<uniform> view:View;\n\
             @group(0) @binding(1) var<storage> lights:Lights;"
                .to_string(),
        ),
        (
            "shadows",
            "#define_import_path bevy_pbr::shadows\n\
             fn fetch_directional_shadow(id:u32,p:vec4<f32>,n:vec3<f32>,z:f32)->f32 {return 1.0;}"
                .to_string(),
        ),
        (
            "motion",
            "#define_import_path bevy_pbr::pbr_prepass_functions\n\
             fn calculate_motion_vector(p:vec4<f32>,q:vec4<f32>)->vec2<f32> {return vec2<f32>(0.0);}"
                .to_string(),
        ),
        (
            "transformations",
            "#define_import_path bevy_pbr::view_transformations\n\
             fn position_world_to_clip(p:vec3<f32>)->vec4<f32> {return vec4<f32>(p,1.0);}"
                .to_string(),
        ),
        (
            "mesh_functions",
            "#define_import_path bevy_pbr::mesh_functions\n\
             fn get_world_from_local(i:u32)->mat4x4<f32> {return mat4x4<f32>();}\n\
             fn mesh_position_local_to_world(m:mat4x4<f32>,p:vec4<f32>)->vec4<f32> {return m*p;}\n\
             fn mesh_normal_local_to_world(n:vec3<f32>,i:u32)->vec3<f32> {return n;}\n\
             fn mesh_tangent_local_to_world(m:mat4x4<f32>,t:vec4<f32>,i:u32)->vec4<f32> {return t;}"
                .to_string(),
        ),
        (
            "bindings",
            include_str!("retail_material_bindings.wgsl").to_string(),
        ),
        (
            "character_common",
            include_str!("retail_character_common.wgsl").to_string(),
        ),
    ];
    for (path, source) in fixtures {
        if let Err(error) = composer.add_composable_module(ComposableModuleDescriptor {
            source: &source,
            file_path: path,
            shader_defs: defs.clone(),
            ..Default::default()
        }) {
            panic!("{}", error.emit_to_string(&composer));
        }
    }
    let module = composer
        .make_naga_module(NagaModuleDescriptor {
            source,
            file_path: "retail",
            shader_defs: defs,
            ..Default::default()
        })
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&composer)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    module
}

/// The world shader is the one at risk: `slot` is a per-vertex attribute, so
/// every family branch is non-uniform control flow and implicit-derivative
/// sampling inside one is invalid WGSL. If this passes, the explicit gradients
/// are complete.
#[test]
fn world_shader_validates_under_non_uniform_material_slots() {
    validate(include_str!("retail_world.wgsl"), &[]);
}

/// The dome is one draw with no material table, so what is worth checking is the
/// binding numbers: they are written out by hand here and have to stay in step
/// with `SkyMaterial`'s `AsBindGroup` derive.
#[test]
fn sky_shader_validates() {
    validate(include_str!("retail_sky.wgsl"), &[]);
}

/// The shadow pass is the only place a second vertex layout is in play, and a
/// mismatch there aborts pipeline creation rather than degrading. Cover each
/// combination Bevy can ask for: cutting classes sample a page, and directional
/// cascades emulate unclipped depth on adapters without depth clip control.
#[test]
fn depth_shader_validates_in_every_shadow_configuration() {
    for extras in [
        &[][..],
        &["WORLD_ALPHA_CUTOFF"],
        &["UNCLIPPED_DEPTH_ORTHO_EMULATION"],
        &["WORLD_ALPHA_CUTOFF", "UNCLIPPED_DEPTH_ORTHO_EMULATION"],
    ] {
        validate(include_str!("retail_depth.wgsl"), extras);
    }
}

/// Each vertex input location of a shader's `vertex` entry point, against a
/// description of the type it expects to receive there.
fn vertex_locations(module: &naga::Module) -> HashMap<u32, String> {
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.name == "vertex")
        .expect("shader must own its vertex entry point");
    let mut found = HashMap::new();
    for argument in &entry.function.arguments {
        if let Some(naga::Binding::Location { location, .. }) = argument.binding {
            found.insert(location, format!("{:?}", module.types[argument.ty].inner));
        } else if let naga::TypeInner::Struct { members, .. } = &module.types[argument.ty].inner {
            for member in members {
                if let Some(naga::Binding::Location { location, .. }) = member.binding {
                    found.insert(location, format!("{:?}", module.types[member.ty].inner));
                }
            }
        }
    }
    found
}

/// The shadow pass reads the same vertex buffer as the main pass, so the two
/// shaders have to agree with `WorldMaterial::specialize` about which attribute
/// each location carries. Disagreeing is what made `prepass_pipeline` fail to
/// build: Bevy's own prepass shaders put `COLOR` on location 7.
#[test]
fn depth_shader_vertex_locations_match_the_world_shader() {
    let world = vertex_locations(&validate(include_str!("retail_world.wgsl"), &[]));
    let depth = vertex_locations(&validate(include_str!("retail_depth.wgsl"), &[]));
    for (location, ty) in &depth {
        assert_eq!(
            world.get(location),
            Some(ty),
            "location {location} differs from the world shader's vertex layout"
        );
    }
}

#[test]
fn character_shaders_validate() {
    validate(include_str!("retail_character.wgsl"), &[]);
    validate(include_str!("retail_character_depth.wgsl"), &[]);
    validate(
        include_str!("retail_character_depth.wgsl"),
        &[
            "PREPASS_FRAGMENT",
            "NORMAL_PREPASS",
            "NORMAL_PREPASS_OR_DEFERRED_PREPASS",
            "MOTION_VECTOR_PREPASS",
            "UNCLIPPED_DEPTH_ORTHO_EMULATION",
        ],
    );
}

/// The vertex entry point declares locations 0..5, and `specialize` pins the
/// mesh attributes to those same locations. A mismatch is silent corruption
/// rather than an error, so assert the shader's own view of its inputs.
#[test]
fn world_vertex_inputs_match_the_pinned_attribute_locations() {
    let module = validate(include_str!("retail_world.wgsl"), &[]);
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.name == "vertex")
        .expect("world shader must own its vertex entry point");
    let mut locations: Vec<u32> = entry
        .function
        .arguments
        .iter()
        .filter_map(|argument| match argument.binding {
            Some(naga::Binding::Location { location, .. }) => Some(location),
            _ => None,
        })
        .collect();
    // A single struct argument carries the locations on its members instead.
    if locations.is_empty() {
        for argument in &entry.function.arguments {
            if let naga::TypeInner::Struct { members, .. } = &module.types[argument.ty].inner {
                for member in members {
                    if let Some(naga::Binding::Location { location, .. }) = member.binding {
                        locations.push(location);
                    }
                }
            }
        }
    }
    locations.sort_unstable();
    assert_eq!(locations, [0, 1, 2, 3, 4, 5, 6]);
}
