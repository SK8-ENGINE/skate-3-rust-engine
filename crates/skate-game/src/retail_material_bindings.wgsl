#define_import_path skate_retail::material_bindings
struct WorldParams {
    mode: vec4<f32>, foliage_debug: vec4<f32>, surface: vec4<f32>, family: vec4<f32>,
    fog_ramp: vec4<f32>, fog_color: vec4<f32>, shadow_color: vec4<f32>, sun_direction: vec4<f32>, decal: vec4<f32>, water: array<vec4<f32>, 4>,
}
struct FrameState { shadow: vec4<f32>, clock: vec4<f32>, pca: array<vec4<f32>, 7> }

#ifdef BINDLESS
struct MaterialIndices {
    params: u32,
    diffuse: u32,
    diffuse_sampler: u32,
    lightmap: u32,
    lm_sampler: u32,
    normal_map: u32,
    normal_sampler: u32,
    detail_map: u32,
    detail_sampler: u32,
    macro_map: u32,
    macro_sampler: u32,
    decal_map: u32,
    decal_sampler: u32,
    specular_map: u32,
    specular_sampler: u32,
    environment_map: u32,
    frame_state: u32,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage> indices: array<MaterialIndices>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var samplers: binding_array<sampler>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var textures: binding_array<texture_2d<f32>>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var cubes: binding_array<texture_cube<f32>>;
@group(#{MATERIAL_BIND_GROUP}) @binding(17) var<storage> params: array<WorldParams>;
@group(#{MATERIAL_BIND_GROUP}) @binding(18) var<storage> frames: binding_array<FrameState>;
#else
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

@group(#{MATERIAL_BIND_GROUP}) @binding(16) var<storage, read> frame_state: FrameState;

#endif
