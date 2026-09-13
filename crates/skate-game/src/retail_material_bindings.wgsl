#define_import_path skate_retail::material_bindings

// Material data for a whole texture slab, indexed by a per-vertex material
// index rather than a per-draw bind group slot. That is the entire point of the
// rewrite: one draw can cover thousands of materials (RFC 1 §1).
//
// Two consequences shape this file.
//
// 1. `slot` is non-uniform within a draw, so every texture read uses explicit
//    gradients. Implicit-derivative sampling (`textureSample`) inside a branch
//    keyed on `slot` is invalid WGSL and Naga rejects it.
// 2. Textures live in `texture_2d_array` pages grouped by size class, selected
//    with a `switch`. Array layers are whole textures, so tiling still works —
//    an atlas would bleed across neighbours on repeat.

struct WorldParams {
    mode: vec4<f32>, foliage_debug: vec4<f32>, surface: vec4<f32>, family: vec4<f32>,
    fog_ramp: vec4<f32>, fog_color: vec4<f32>, shadow_color: vec4<f32>, sun_direction: vec4<f32>, decal: vec4<f32>, water: array<vec4<f32>, 4>,
}
struct FrameState { shadow: vec4<f32>, clock: vec4<f32>, pca: array<vec4<f32>, 7> }

/// Where each texture channel of one material lives.
///
/// Each field packs `(size_class << 16) | layer`. `ABSENT` means the channel has
/// no texture; callers must gate on the presence bits in `mode.y` exactly as
/// before, since a missing channel has no defined layer to sample.
struct MaterialSlots {
    diffuse: u32,
    lightmap: u32,
    normal_map: u32,
    detail_map: u32,
    macro_map: u32,
    decal_map: u32,
    specular_map: u32,
    environment: u32,
}

const ABSENT: u32 = 0xffffffffu;

// One page per distinct texture size present in the map. A `texture_2d_array`
// requires every layer to share dimensions, and retail packages use more than a
// handful of sizes, so four pages would force rescaling. Eight covers the
// observed spread and still leaves the sampled-texture count well inside
// `max_sampled_textures_per_shader_stage` (16 on the strictest tier we target).
const PAGE_CLASSES: u32 = 8u;

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage, read> params: array<WorldParams>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> slots: array<MaterialSlots>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> frame_state_buffer: FrameState;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var page_0: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var page_1: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var page_2: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var page_3: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var page_4: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var page_5: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var page_6: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var page_7: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var repeat_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var cubes: texture_cube_array<f32>;
fn page_class(packed: u32) -> u32 { return (packed >> 16u) & 0xffu; }
fn page_layer(packed: u32) -> u32 { return packed & 0xffffu; }

/// Clamped channels — lightmaps, decals and authored clamp-addressed diffuse —
/// saturate their coordinates instead of using a second sampler. `AsBindGroup`
/// derives a sampler from an image's own descriptor, so a clamp sampler would
/// need a second carrier image; the difference from `ClampToEdge` is confined to
/// the outer half-texel, whereas leaving these on repeat would tile a decal
/// across its whole surface.
fn page_uv(packed: u32, uv: vec2<f32>) -> vec2<f32> {
    return select(uv, saturate(uv), (packed & 0x80000000u) != 0u);
}

/// Textures and base UVs both have V flipped by the exporter. Scale in the
/// authored coordinate system, then return to the flipped texture rows.
fn scaled_uv(uv: vec2<f32>, scale: f32) -> vec2<f32> {
    return vec2<f32>(uv.x*scale, 1.0-(1.0-uv.y)*scale);
}

/// Gradients for a UV set, carried from uniform control flow into the branches.
///
/// A derived UV is an affine function of the base UV, so its gradients are the
/// same transform applied to the base gradients — which is why the caller can
/// build these before knowing which family branch will run.
struct Gradients { ddx: vec2<f32>, ddy: vec2<f32> }

fn gradients(uv: vec2<f32>) -> Gradients {
    return Gradients(dpdx(uv), dpdy(uv));
}

fn gradients_scaled(g: Gradients, scale: f32) -> Gradients {
    return Gradients(g.ddx*scale, g.ddy*scale);
}

// Keep resource-array access at the sampling sites. Passing a selected resource
// into a shading function crashes the tested Vulkan pipeline compiler.
fn sample_page(packed: u32, uv_in: vec2<f32>, g: Gradients) -> vec4<f32> {
    let layer = page_layer(packed);
    let uv = page_uv(packed, uv_in);
    switch page_class(packed) {
        case 0u: { return textureSampleGrad(page_0,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 1u: { return textureSampleGrad(page_1,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 2u: { return textureSampleGrad(page_2,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 3u: { return textureSampleGrad(page_3,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 4u: { return textureSampleGrad(page_4,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 5u: { return textureSampleGrad(page_5,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 6u: { return textureSampleGrad(page_6,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 7u: { return textureSampleGrad(page_7,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        default: { return vec4<f32>(0.0,0.0,0.0,1.0); }
    }
}

/// Level-sampled and clamped: lightmaps are unique per surface, never tiled, and
/// the reference always reads mip 0.
fn sample_page_level(packed: u32, uv_in: vec2<f32>, level: f32) -> vec4<f32> {
    let layer = page_layer(packed);
    let uv = page_uv(packed, uv_in);
    switch page_class(packed) {
        case 0u: { return textureSampleLevel(page_0,repeat_sampler,uv,layer,level); }
        case 1u: { return textureSampleLevel(page_1,repeat_sampler,uv,layer,level); }
        case 2u: { return textureSampleLevel(page_2,repeat_sampler,uv,layer,level); }
        case 3u: { return textureSampleLevel(page_3,repeat_sampler,uv,layer,level); }
        case 4u: { return textureSampleLevel(page_4,repeat_sampler,uv,layer,level); }
        case 5u: { return textureSampleLevel(page_5,repeat_sampler,uv,layer,level); }
        case 6u: { return textureSampleLevel(page_6,repeat_sampler,uv,layer,level); }
        case 7u: { return textureSampleLevel(page_7,repeat_sampler,uv,layer,level); }
        default: { return vec4<f32>(1.0); }
    }
}

fn load_page(packed: u32, uv: vec2<i32>, level: i32) -> vec4<f32> {
    let layer = i32(page_layer(packed));
    switch page_class(packed) {
        case 0u: { return textureLoad(page_0,uv,layer,level); }
        case 1u: { return textureLoad(page_1,uv,layer,level); }
        case 2u: { return textureLoad(page_2,uv,layer,level); }
        case 3u: { return textureLoad(page_3,uv,layer,level); }
        case 4u: { return textureLoad(page_4,uv,layer,level); }
        case 5u: { return textureLoad(page_5,uv,layer,level); }
        case 6u: { return textureLoad(page_6,uv,layer,level); }
        case 7u: { return textureLoad(page_7,uv,layer,level); }
        default: { return vec4<f32>(0.5); }
    }
}

fn page_dimensions(packed: u32) -> vec2<u32> {
    switch page_class(packed) {
        case 0u: { return textureDimensions(page_0); }
        case 1u: { return textureDimensions(page_1); }
        case 2u: { return textureDimensions(page_2); }
        case 3u: { return textureDimensions(page_3); }
        case 4u: { return textureDimensions(page_4); }
        case 5u: { return textureDimensions(page_5); }
        case 6u: { return textureDimensions(page_6); }
        case 7u: { return textureDimensions(page_7); }
        default: { return vec2<u32>(1u); }
    }
}

// --- Channel accessors, matching the reference surface -----------------------

fn frame_state() -> FrameState { return frame_state_buffer; }

fn sample_diffuse(slot: u32, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(slots[slot].diffuse, uv, g);
}

fn sample_normal_map(slot: u32, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(slots[slot].normal_map, uv, g);
}

fn sample_detail_map(slot: u32, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(slots[slot].detail_map, uv, g);
}

fn sample_macro_map(slot: u32, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(slots[slot].macro_map, uv, g);
}

fn sample_decal_map(slot: u32, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(slots[slot].decal_map, uv, g);
}

fn sample_specular_map(slot: u32, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(slots[slot].specular_map, uv, g);
}

fn sample_lightmap(slot: u32, uv: vec2<f32>, level: f32) -> vec4<f32> {
    return sample_page_level(slots[slot].lightmap, uv, level);
}

fn load_detail(slot: u32, uv: vec2<i32>, level: i32) -> vec4<f32> {
    return load_page(slots[slot].detail_map, uv, level);
}

fn lightmap_dimensions(slot: u32) -> vec2<u32> {
    return page_dimensions(slots[slot].lightmap);
}

/// `textureSampleBias` in the reference; bias cannot be used under non-uniform
/// control flow, so the caller's mip bias becomes a gradient scale instead.
/// `exp2(bias)` is the mip-ratio the bias represented.
fn sample_environment(slot: u32, direction: vec3<f32>, bias: f32) -> vec4<f32> {
    let packed = slots[slot].environment;
    if packed == ABSENT { return vec4<f32>(0.0); }
    let scale = exp2(bias);
    let ddx = dpdx(direction)*scale;
    let ddy = dpdy(direction)*scale;
    return textureSampleGrad(cubes,repeat_sampler,direction,page_layer(packed),ddx,ddy);
}
