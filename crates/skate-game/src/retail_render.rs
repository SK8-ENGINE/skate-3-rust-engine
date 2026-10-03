//! Retail world materials for merged geometry.
//!
//! The old renderer gave every material its own bind group, so a material was a
//! draw. Here the material table is GPU data: `params` and `slots` are storage
//! arrays indexed by the `.skate` per-vertex material index, so one draw covers
//! every material in its slab (RFC 1 D1/D2).
//!
//! Only *textures* are slabbed. A `texture_2d_array` has a finite layer count
//! and every layer must share dimensions, so textures are grouped into pages by
//! size and a slab is sealed when a page fills. Parameters have no such limit,
//! which is why they are not what defines a slab.
use bevy::{
    asset::{AssetPath, RenderAssetUsages, embedded_asset, embedded_path, uuid_handle},
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::extract_component::ExtractComponent,
    render::{
        render_resource::{
            AsBindGroup, Extent3d, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
            TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
        },
        storage::ShaderStorageBuffer,
    },
    shader::ShaderRef,
};
use skate_data::skate_map::SkateMap;
use std::collections::{BTreeMap, HashMap};

use crate::map_render::AssetSink;

/// Pages per slab. A `texture_2d_array` needs uniform layer dimensions, and
/// retail packages use more than a handful of sizes, so a single page would
/// force wholesale rescaling. Eight keeps the sampled-texture count inside the
/// 16-per-stage floor while covering the observed size spread.
pub(crate) const PAGE_CLASSES: usize = 8;

/// Conservative `max_texture_array_layers`. The scene is staged off the main
/// thread with no render device in hand, so this is a constant rather than a
/// queried limit; every desktop target we care about allows at least this many.
const MAX_PAGE_LAYERS: usize = 2048;

/// scene.hlsl's retail world ALPHAREF is 30, not the portable 0.5.
const ALPHA_REF: f32 = 30. / 255.;

/// Shared per-frame state: shadow floor, animation clock and authored ocean PCA.
/// Map-independent, so it is a fixed handle rather than a staged asset.
pub(crate) const FRAME_STATE: Handle<ShaderStorageBuffer> =
    uuid_handle!("7f3a1c58-9d21-4e0b-9a44-2c6b8e51d7a3");

// ---------------------------------------------------------------------------
// GPU data
// ---------------------------------------------------------------------------

/// Per-material shading constants. Byte layout must match `WorldParams` in
/// `retail_material_bindings.wgsl`: thirteen `vec4<f32>`, 208 bytes.
#[derive(Clone, Debug, Default)]
pub(crate) struct WorldParams {
    /// family, texture flags, alpha cutoff (-1 for opaque), exposure
    pub mode: Vec4,
    /// Diagnostic solid foliage colour; w=0 keeps retail shading.
    pub foliage_debug: Vec4,
    /// macro UV scale, opacity, detail UV scale, material multiplier
    pub surface: Vec4,
    /// tree LM scale/floor/tint, proxy multiplier: reference day capture
    pub family: Vec4,
    pub fog_ramp: Vec4,
    pub fog_color: Vec4,
    pub shadow_color: Vec4,
    pub sun_direction: Vec4,
    /// Wear-only visual tuning; artwork retains unit opacity.
    pub decal: Vec4,
    pub water: [Vec4; 4],
}

impl WorldParams {
    pub(crate) const SIZE: usize = 13 * 16;

    fn encode(&self, out: &mut Vec<u8>) {
        for row in [
            self.mode,
            self.foliage_debug,
            self.surface,
            self.family,
            self.fog_ramp,
            self.fog_color,
            self.shadow_color,
            self.sun_direction,
            self.decal,
            self.water[0],
            self.water[1],
            self.water[2],
            self.water[3],
        ] {
            for component in row.to_array() {
                out.extend_from_slice(&component.to_le_bytes());
            }
        }
    }
}

/// Where each texture channel of one material lives, as `MaterialSlots` in the
/// shader: eight `u32`, 32 bytes.
///
/// Each channel packs `clamp << 31 | class << 16 | layer`. `ABSENT` is all ones,
/// whose class reads back above `PAGE_CLASSES` and so lands in the shader's
/// default branch; callers still gate on the presence bits in `mode.y`.
#[derive(Clone, Copy, Debug)]
struct MaterialSlots([u32; 8]);

const ABSENT: u32 = 0xffff_ffff;

impl Default for MaterialSlots {
    fn default() -> Self {
        Self([ABSENT; 8])
    }
}

impl MaterialSlots {
    const SIZE: usize = 32;

    fn encode(&self, out: &mut Vec<u8>) {
        for slot in self.0 {
            out.extend_from_slice(&slot.to_le_bytes());
        }
    }
}

fn pack_slot(class: usize, layer: usize, clamp: bool) -> u32 {
    debug_assert!(class < PAGE_CLASSES && layer < MAX_PAGE_LAYERS);
    (u32::from(clamp) << 31) | ((class as u32) << 16) | layer as u32
}

/// Shared frame state, matching `FrameState` in the bindings module: 144 bytes.
///
/// `shadow.w` gates every dynamic-shadow read in the world shader. Shadows are
/// out of scope for v1 (RFC 1 D5), so it stays zero and the shader never touches
/// the cascade bindings; re-enabling them is a write to this field.
#[derive(Resource, Clone, Default)]
pub(crate) struct FrameStateData {
    pub shadow: Vec4,
    pub clock: Vec4,
    pub pca: [Vec4; 7],
}

impl FrameStateData {
    const SIZE: usize = 9 * 16;

    /// Eases the shadow floor towards the local probe's ambient term. Carried
    /// over with the character lighting that feeds it.
    pub(crate) fn approach(&mut self, target: Vec3, dt: f32) {
        let target = target.clamp(Vec3::ZERO, Vec3::ONE);
        let value = if self.shadow.w == 0. {
            target
        } else {
            // Adapter smoothing, not a recovered native constant. Cap a hitch's
            // contribution so one long frame cannot cause a darkness step.
            self.shadow
                .truncate()
                .lerp(target, 1. - (-dt.clamp(0., 0.05) / 0.35).exp())
        };
        self.shadow = value.extend(1.);
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::SIZE);
        for row in std::iter::once(self.shadow)
            .chain(std::iter::once(self.clock))
            .chain(self.pca)
        {
            for component in row.to_array() {
                out.extend_from_slice(&component.to_le_bytes());
            }
        }
        out
    }
}

/// Marks a camera that wants the retail tone curve rather than Bevy's.
///
/// `retail_exposure` meters this view and runs the tone pass on it. The world
/// shader emits linear radiance premultiplied by the baseline exposure of 2.5,
/// which the tone curve divides back out, so a camera without this marker
/// renders that unmapped signal straight into an 8-bit surface and clips
/// everything bright to white.
#[derive(Component, ExtractComponent, Clone, Copy, Default, Debug)]
pub(crate) struct RetailTone;

// ---------------------------------------------------------------------------
// Render classes
// ---------------------------------------------------------------------------

/// Pipeline state that cannot vary per vertex, and therefore cannot be merged
/// into one draw.
///
/// Alpha testing is a class, even though the cutoff itself lives in `mode.z`
/// per material. A shader that can `discard` cannot resolve depth before it
/// runs, so the hardware stops writing depth early and nothing rejects the
/// fragments hidden behind it. Sharing one draw between opaque and alpha-tested
/// geometry therefore imposed alpha-tested overdraw on the whole world: the
/// opaque pass shaded 2.16x the pixels it kept. Keeping the classes apart lets
/// the opaque variant compile without `discard` and reject hidden fragments
/// before shading them. The split only costs a draw where one slab holds both
/// kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum RenderClass {
    Opaque,
    OpaqueTwoSided,
    Cutout,
    CutoutTwoSided,
    Blended,
    BlendedTwoSided,
}

impl RenderClass {
    /// `cutout` only separates the opaque classes. A blended material resolves
    /// coverage by blending, and its cutoff is honoured by the same shader
    /// variant either way, so splitting it would buy nothing.
    fn new(blended: bool, cutout: bool, two_sided: bool) -> Self {
        match (blended, cutout, two_sided) {
            (true, _, false) => Self::Blended,
            (true, _, true) => Self::BlendedTwoSided,
            (false, true, false) => Self::Cutout,
            (false, true, true) => Self::CutoutTwoSided,
            (false, false, false) => Self::Opaque,
            (false, false, true) => Self::OpaqueTwoSided,
        }
    }

    fn two_sided(self) -> bool {
        matches!(
            self,
            Self::OpaqueTwoSided | Self::CutoutTwoSided | Self::BlendedTwoSided
        )
    }

    /// Whether this class's shader variant keeps the `discard`, and so gives up
    /// early depth writes.
    fn discards(self) -> bool {
        !matches!(self, Self::Opaque | Self::OpaqueTwoSided)
    }

    fn alpha_mode(self) -> AlphaMode {
        match self {
            // Genuinely opaque: no cutoff reaches the shader, so Bevy must not
            // set MAY_DISCARD either.
            Self::Opaque | Self::OpaqueTwoSided => AlphaMode::Opaque,
            // `Mask` so Bevy sets MAY_DISCARD; the threshold is per-material in
            // `mode.z`.
            Self::Cutout | Self::CutoutTwoSided => AlphaMode::Mask(ALPHA_REF),
            Self::Blended | Self::BlendedTwoSided => AlphaMode::Blend,
        }
    }

    pub(crate) const ALL: [Self; 6] = [
        Self::Opaque,
        Self::OpaqueTwoSided,
        Self::Cutout,
        Self::CutoutTwoSided,
        Self::Blended,
        Self::BlendedTwoSided,
    ];
}

// ---------------------------------------------------------------------------
// Material asset
// ---------------------------------------------------------------------------

/// One slab's worth of world materials, plus the pipeline state of one class.
///
/// Every field except `class` is shared between the classes of a slab, so the
/// four class variants cost four bind groups, not four copies of the data.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(WorldMaterialKey)]
pub(crate) struct WorldMaterial {
    #[storage(0, read_only)]
    pub params: Handle<ShaderStorageBuffer>,
    #[storage(1, read_only)]
    pub slots: Handle<ShaderStorageBuffer>,
    #[storage(2, read_only)]
    pub frame_state: Handle<ShaderStorageBuffer>,
    #[texture(3, dimension = "2d_array")]
    #[sampler(11)]
    pub page_0: Handle<Image>,
    #[texture(4, dimension = "2d_array")]
    pub page_1: Handle<Image>,
    #[texture(5, dimension = "2d_array")]
    pub page_2: Handle<Image>,
    #[texture(6, dimension = "2d_array")]
    pub page_3: Handle<Image>,
    #[texture(7, dimension = "2d_array")]
    pub page_4: Handle<Image>,
    #[texture(8, dimension = "2d_array")]
    pub page_5: Handle<Image>,
    #[texture(9, dimension = "2d_array")]
    pub page_6: Handle<Image>,
    #[texture(10, dimension = "2d_array")]
    pub page_7: Handle<Image>,
    #[texture(12, dimension = "cube_array")]
    pub cubes: Handle<Image>,
    pub class: RenderClass,
}

impl WorldMaterial {
    fn with_pages(
        params: Handle<ShaderStorageBuffer>,
        slots: Handle<ShaderStorageBuffer>,
        pages: &[Handle<Image>; PAGE_CLASSES],
        cubes: Handle<Image>,
        class: RenderClass,
    ) -> Self {
        Self {
            params,
            slots,
            frame_state: FRAME_STATE,
            page_0: pages[0].clone(),
            page_1: pages[1].clone(),
            page_2: pages[2].clone(),
            page_3: pages[3].clone(),
            page_4: pages[4].clone(),
            page_5: pages[5].clone(),
            page_6: pages[6].clone(),
            page_7: pages[7].clone(),
            cubes,
            class,
        }
    }
}

/// Only the state `specialize` needs: face culling, and whether the fragment
/// shader keeps its `discard`.
///
/// Bevy's own pipeline key carries the alpha mode, but it selects blend state
/// rather than shader defs, so the `discard` has to be keyed here.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct WorldMaterialKey {
    two_sided: bool,
    discards: bool,
}

impl From<&WorldMaterial> for WorldMaterialKey {
    fn from(material: &WorldMaterial) -> Self {
        Self {
            two_sided: material.class.two_sided(),
            discards: material.class.discards(),
        }
    }
}

// Use the same path derivation as embedded_asset!: alternate binary targets have
// a different crate namespace even though they share these source files.
fn retail_shader(path: &str) -> ShaderRef {
    AssetPath::from(embedded_path!(path))
        .with_source("embedded")
        .into()
}

impl Material for WorldMaterial {
    fn vertex_shader() -> ShaderRef {
        retail_shader("retail_world.wgsl")
    }

    fn fragment_shader() -> ShaderRef {
        retail_shader("retail_world.wgsl")
    }

    // Shadow maps are built from the prepass, and `specialize` below declares one
    // vertex layout for every pipeline. Both prepass stages are ours so that the
    // layout and the shader agree about where `material_index` lives.
    fn prepass_vertex_shader() -> ShaderRef {
        retail_shader("retail_depth.wgsl")
    }

    fn prepass_fragment_shader() -> ShaderRef {
        retail_shader("retail_depth.wgsl")
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.class.alpha_mode()
    }

    fn specialize(
        _: &bevy::pbr::MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The material index is a vertex attribute, so the layout is ours to
        // declare rather than Bevy's mesh pipeline's.
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(3),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(4),
            crate::skate_world::ATTRIBUTE_MATERIAL_INDEX.at_shader_location(5),
            Mesh::ATTRIBUTE_TANGENT.at_shader_location(6),
        ])?];
        // Retail foliage is two-sided; opaque world triangles retain winding.
        descriptor.primitive.cull_mode = if key.bind_group_data.two_sided {
            None
        } else {
            Some(Face::Back)
        };
        // Only the classes that can actually reject a texel compile the
        // `discard`. Without it the hardware resolves depth before the shader
        // runs, so hidden fragments never reach it.
        if key.bind_group_data.discards
            && let Some(fragment) = &mut descriptor.fragment
        {
            fragment.shader_defs.push("WORLD_ALPHA_CUTOFF".into());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Material definitions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Binding {
    pub texture: u32,
    pub uv: u32,
    pub u: u32,
    pub v: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Definition {
    pub shader: String,
    pub family: u32,
    pub flags: u32,
    pub bindings: BTreeMap<String, Binding>,
    pub parameters: BTreeMap<String, Vec<String>>,
}

impl Definition {
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        struct Reader<'a>(&'a [u8]);
        impl<'a> Reader<'a> {
            fn take(&mut self, n: usize) -> Option<&'a [u8]> {
                let v = self.0.get(..n)?;
                self.0 = &self.0[n..];
                Some(v)
            }
            fn u32(&mut self) -> Option<u32> {
                Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
            }
            fn text(&mut self) -> Option<String> {
                let n = self.u32()? as usize;
                String::from_utf8(self.take(n)?.to_vec()).ok()
            }
        }
        let mut r = Reader(bytes);
        r.take(16)?;
        let shader = r.text()?;
        let stored_family = r.u32()?;
        // Older map packages retained the complete bindings but classified
        // non-flowing water as unknown. Upgrade without re-exporting geometry.
        let family = if matches!(
            shader.as_str(),
            "water.default" | "water.alpha" | "water.skatepark"
        ) {
            33
        } else {
            stored_family
        };
        let flags = r.u32()?;
        let mut bindings = BTreeMap::new();
        for _ in 0..r.u32()? {
            bindings.insert(
                r.text()?,
                Binding {
                    texture: r.u32()?,
                    uv: r.u32()?,
                    u: r.u32()?,
                    v: r.u32()?,
                },
            );
        }
        let mut parameters = BTreeMap::new();
        for _ in 0..r.u32()? {
            let name = r.text()?;
            let values = (0..r.u32()?)
                .map(|_| r.text())
                .collect::<Option<Vec<_>>>()?;
            // Names/GUIDs do not affect shading or batching.
            if !matches!(name.as_str(), "Name" | "AttribulatorMaterialName") {
                parameters.insert(name, values);
            }
        }
        r.text()?;
        if !r.0.is_empty() {
            return None;
        }
        Some(Self {
            shader,
            family,
            flags,
            bindings,
            parameters,
        })
    }

    /// A definition-free material: diffuse and lightmap out of the portable
    /// `.skate` fields. Custom maps and Skate 2 era exports land here.
    fn portable() -> Self {
        Self {
            shader: String::new(),
            family: 1,
            flags: 0,
            bindings: BTreeMap::new(),
            parameters: BTreeMap::new(),
        }
    }

    pub(crate) fn scalar(&self, name: &str) -> Option<f32> {
        self.parameters
            .get(name)?
            .first()?
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
    }

    pub(crate) fn supported(&self, tuning: &MaterialTuning) -> bool {
        (1..=13).contains(&self.family)
            || match self.family {
                14 | 32 => tuning.rows.get(&self.shader).is_some_and(|r| !r.is_empty()),
                31 => {
                    tuning.pca_available
                        && tuning.rows.get(&self.shader).is_some_and(|r| r.len() == 3)
                }
                30 => tuning.rows.get(&self.shader).is_some_and(|r| r.len() == 4),
                33 => {
                    tuning.pca_available
                        && self.bindings.contains_key("normal")
                        && self.bindings.contains_key("normal2")
                        && tuning.rows.get(&self.shader).is_some_and(|r| r.len() == 4)
                }
                _ => false,
            }
    }
}

// The source labels distinguish weathering from graphics. This is an explicit
// visual tuning choice, not a recovered native material constant. Keep arrows,
// logos, paint, scratches and edge wear at their authored alpha.
fn stain_opacity(texture_label: &str) -> f32 {
    let label = texture_label.to_ascii_lowercase();
    if [
        "grime",
        "grunge",
        "stain",
        "oildirt",
        "drainage",
        "ground_decals",
    ]
    .iter()
    .any(|word| label.contains(word))
    {
        0.35
    } else {
        1.
    }
}

#[derive(Default)]
pub(crate) struct MaterialTuning {
    rows: BTreeMap<String, Vec<[f32; 4]>>,
    pca_available: bool,
}

impl MaterialTuning {
    pub(crate) fn load(root: &std::path::Path) -> Self {
        let path = root.join("private/render-parameters.json");
        match std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<BTreeMap<String, Vec<[f32; 4]>>>(&b).ok())
        {
            Some(rows) if rows.values().flatten().flatten().all(|x| x.is_finite()) => Self {
                rows,
                pca_available: read_pca(root).is_some(),
            },
            _ => {
                warn!(
                    "Retail water/scroll tuning unavailable: {}",
                    path.display()
                );
                Self::default()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Texture pages
// ---------------------------------------------------------------------------

/// How one channel wants its texture sampled. Mirrors the reference role codes:
/// repeat with mips, clamped without, clamped with.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Usage {
    clamp: bool,
    mips: bool,
}

impl Usage {
    const REPEAT: Self = Self {
        clamp: false,
        mips: true,
    };
    const CLAMP: Self = Self {
        clamp: true,
        mips: false,
    };
    const CLAMP_MIPS: Self = Self {
        clamp: true,
        mips: true,
    };
}

/// Deduplicates textures by content so identical pixels share one layer.
/// Returns a lookup from 1-based source id to canonical 1-based id, with 0
/// meaning "no texture" in both directions.
fn canonical_texture_ids(textures: &[skate_data::skate_map::Texture]) -> Vec<u32> {
    let mut seen: HashMap<(u32, u32, u32, &[u8]), u32> = HashMap::new();
    let mut ids = vec![0; textures.len() + 1];
    for (index, texture) in textures.iter().enumerate() {
        let id = index as u32 + 1;
        ids[index + 1] = *seen
            .entry((
                texture.width,
                texture.height,
                texture.color_space,
                texture.rgba.as_slice(),
            ))
            .or_insert(id);
    }
    ids
}

/// A page under construction: one texture size, one growing list of layers.
struct Page {
    width: u32,
    height: u32,
    layers: Vec<u32>,
    mips: bool,
}

/// Box-filters decoded UNORM faces down to 1x1. The same chain the native
/// renderer used, and what keeps repeating world maps from aliasing at distance.
pub(crate) fn mip_chain(rgba: &[u8], width: u32, height: u32, layers: u32) -> (Vec<u8>, u32) {
    let count = 32 - width.max(height).leading_zeros();
    let mut bytes = Vec::with_capacity(rgba.len() * 4 / 3 + 64);
    for face in rgba
        .chunks_exact((width * height * 4) as usize)
        .take(layers as usize)
    {
        let mut level = face.to_vec();
        let (mut w, mut h) = (width as usize, height as usize);
        bytes.extend_from_slice(&level);
        while w > 1 || h > 1 {
            let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
            let mut next = vec![0; nw * nh * 4];
            for y in 0..nh {
                for x in 0..nw {
                    for c in 0..4 {
                        let mut sum = 0u32;
                        for dy in 0..2 {
                            for dx in 0..2 {
                                sum += level
                                    [((y * 2 + dy).min(h - 1) * w + (x * 2 + dx).min(w - 1)) * 4 + c]
                                    as u32;
                            }
                        }
                        next[(y * nw + x) * 4 + c] = ((sum + 2) / 4) as u8;
                    }
                }
            }
            bytes.extend_from_slice(&next);
            level = next;
            w = nw;
            h = nh;
        }
    }
    (bytes, count)
}

/// Area-average resample, used only for textures whose size did not earn a page.
fn resample(rgba: &[u8], width: u32, height: u32, target_w: u32, target_h: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (tw, th) = (target_w as usize, target_h as usize);
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th {
        let (y0, y1) = (y * h / th, (((y + 1) * h + th - 1) / th).min(h).max(y * h / th + 1));
        for x in 0..tw {
            let (x0, x1) = (x * w / tw, (((x + 1) * w + tw - 1) / tw).min(w).max(x * w / tw + 1));
            for c in 0..4 {
                let mut sum = 0u32;
                let mut n = 0u32;
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        sum += rgba[(sy * w + sx) * 4 + c] as u32;
                        n += 1;
                    }
                }
                out[(y * tw + x) * 4 + c] = (sum / n.max(1)) as u8;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Material table
// ---------------------------------------------------------------------------

/// What the scene builder needs to know about one source material.
#[derive(Clone, Copy)]
pub(crate) struct MaterialEntry {
    /// Slab owning this material's textures.
    pub slab: u16,
    pub class: RenderClass,
    /// Index into the slab's `params`/`slots` arrays, i.e. the value written into
    /// the per-vertex material attribute.
    index: u32,
}

pub(crate) struct MaterialTable {
    /// One slot per source material index; `None` where the material has no
    /// renderable geometry contribution.
    entries: Vec<Option<MaterialEntry>>,
    materials: HashMap<(u16, RenderClass), Handle<WorldMaterial>>,
    slabs: u16,
}

impl MaterialTable {
    pub(crate) fn entry(&self, source: usize) -> Option<MaterialEntry> {
        *self.entries.get(source)?
    }

    /// The per-vertex material attribute value. Unrenderable materials never
    /// reach a merged mesh, so zero is only a defensive default.
    pub(crate) fn slab_index(&self, source: usize) -> u32 {
        self.entry(source).map_or(0, |entry| entry.index)
    }

    pub(crate) fn material(&self, slab: u16, class: RenderClass) -> Handle<WorldMaterial> {
        self.materials
            .get(&(slab, class))
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn slab_count(&self) -> usize {
        usize::from(self.slabs)
    }

    pub(crate) fn build(
        map: &SkateMap,
        tuning: &MaterialTuning,
        sky: &crate::retail_sky::SkyEnvironment,
        materials: &mut impl AssetSink<WorldMaterial>,
        images: &mut impl AssetSink<Image>,
        buffers: &mut impl AssetSink<ShaderStorageBuffer>,
    ) -> Self {
        let _span = info_span!("build_material_table").entered();
        let canonical = canonical_texture_ids(&map.textures);

        // Pass 1: decode every material into a definition plus the channel
        // texture ids it wants, so page sizing can see the whole demand.
        let mut requests: Vec<Option<Request>> = Vec::with_capacity(map.materials.len());
        let mut unsupported = 0usize;
        for material in &map.materials {
            let definition = material
                .retail_definition
                .as_deref()
                .and_then(Definition::parse);
            let mut definition = definition.unwrap_or_else(Definition::portable);
            if !definition.supported(tuning) {
                // A hole is worse than approximate shading, and there is no
                // second world material path to fall back to (RFC 1 D8), so an
                // unsupported family renders as plain diffuse plus lightmap.
                unsupported += 1;
                definition.family = 1;
            }
            requests.push(Some(Request::new(
                material,
                &definition,
                tuning,
                sky,
                &canonical,
                map,
            )));
        }
        if unsupported > 0 {
            warn!(
                "{unsupported} of {} world materials use an unsupported shader family and render as family 1",
                map.materials.len()
            );
        }

        // Pass 2: choose page sizes from the actual demand, most-used first.
        let mut demand: HashMap<(u32, u32), usize> = HashMap::new();
        for request in requests.iter().flatten() {
            for channel in request.channels.iter().flatten() {
                if channel.cube {
                    continue;
                }
                let texture = &map.textures[channel.id as usize - 1];
                *demand.entry((texture.width, texture.height)).or_default() += 1;
            }
        }
        let mut sizes: Vec<((u32, u32), usize)> = demand.into_iter().collect();
        sizes.sort_unstable_by_key(|&(size, count)| (std::cmp::Reverse(count), size));
        let page_sizes: Vec<(u32, u32)> = sizes
            .iter()
            .take(PAGE_CLASSES)
            .map(|&(size, _)| size)
            .collect();
        let rescaled: usize = sizes.iter().skip(PAGE_CLASSES).map(|&(_, n)| n).sum();
        if rescaled > 0 {
            warn!(
                "{rescaled} world textures fall outside the {PAGE_CLASSES} most common sizes and were rescaled to the nearest page"
            );
        }

        // Pass 3: pack materials into slabs, sealing a slab when a page fills.
        let mut table = Self {
            entries: vec![None; map.materials.len()],
            materials: HashMap::new(),
            slabs: 0,
        };
        let mut slab = Slab::new(&page_sizes);
        for (source, request) in requests.iter().enumerate() {
            let Some(request) = request else { continue };
            if !slab.fits(request, map) {
                table.seal(slab, map, materials, images, buffers);
                slab = Slab::new(&page_sizes);
            }
            // `seal` is what increments the count, so the slab being filled is
            // always the next index.
            let entry = slab.insert(request, map, table.slabs);
            table.entries[source] = Some(entry);
        }
        table.seal(slab, map, materials, images, buffers);
        table
    }

    fn seal(
        &mut self,
        slab: Slab,
        map: &SkateMap,
        materials: &mut impl AssetSink<WorldMaterial>,
        images: &mut impl AssetSink<Image>,
        buffers: &mut impl AssetSink<ShaderStorageBuffer>,
    ) {
        if slab.params.is_empty() {
            return;
        }
        let index = self.slabs;
        self.slabs += 1;

        let mut params_bytes = Vec::with_capacity(slab.params.len() * WorldParams::SIZE);
        for params in &slab.params {
            params.encode(&mut params_bytes);
        }
        let mut slots_bytes = Vec::with_capacity(slab.slots.len() * MaterialSlots::SIZE);
        for slots in &slab.slots {
            slots.encode(&mut slots_bytes);
        }
        // `From<T>` goes through `encase`, which will not take a pre-encoded byte
        // blob, so the buffers are constructed from raw bytes instead.
        let params = buffers.add(ShaderStorageBuffer::new(
            &params_bytes,
            RenderAssetUsages::RENDER_WORLD,
        ));
        let slots = buffers.add(ShaderStorageBuffer::new(
            &slots_bytes,
            RenderAssetUsages::RENDER_WORLD,
        ));

        let pages = std::array::from_fn(|class| slab.upload_page(class, map, images));
        let cubes = slab.upload_cubes(map, images);
        for class in RenderClass::ALL {
            let material = materials.add(WorldMaterial::with_pages(
                params.clone(),
                slots.clone(),
                &pages,
                cubes.clone(),
                class,
            ));
            self.materials.insert((index, class), material);
        }
    }
}

/// One material's decoded shading inputs, before page assignment.
struct Request {
    params: WorldParams,
    class: RenderClass,
    /// diffuse, lightmap, normal, detail, macro, decal, specular, environment
    channels: [Option<Channel>; 8],
}

#[derive(Clone, Copy)]
struct Channel {
    /// Canonical 1-based texture id.
    id: u32,
    usage: Usage,
    cube: bool,
}

impl Request {
    fn new(
        material: &skate_data::skate_map::Material,
        definition: &Definition,
        tuning: &MaterialTuning,
        sky: &crate::retail_sky::SkyEnvironment,
        canonical: &[u32],
        map: &SkateMap,
    ) -> Self {
        let lookup = |id: u32, cube: bool| -> Option<u32> {
            let id = *canonical.get(id as usize)?;
            if id == 0 {
                return None;
            }
            let texture = &map.textures[id as usize - 1];
            // Older .skate exports contain only face zero. Never treat those as
            // a cube: six stacked faces is the only valid encoding.
            if cube && texture.height != texture.width * 6 {
                return None;
            }
            Some(id)
        };
        let fetch = |role: &str, fallback: u32, force_clamp: bool| -> Option<Channel> {
            let binding = definition.bindings.get(role);
            let id = lookup(binding.map_or(fallback, |b| b.texture), false)?;
            let clamped = force_clamp || binding.is_some_and(|b| b.u == 1 && b.v == 1);
            let usage = match (clamped, role) {
                (false, _) => Usage::REPEAT,
                (true, "decal") => Usage::CLAMP_MIPS,
                (true, _) => Usage::CLAMP,
            };
            Some(Channel {
                id,
                usage,
                cube: false,
            })
        };
        let diffuse = fetch("diffuse", material.textures[0], false);
        let lightmap = fetch("lightmap", material.textures[1], true);
        let normal = fetch("normal", material.textures[2], false);
        let detail = fetch(
            if matches!(definition.family, 31 | 33) {
                "normal2"
            } else {
                "detail"
            },
            0,
            false,
        );
        let macro_map = fetch("macrooverlay", 0, false);
        let decal = fetch("decal", 0, definition.family == 3);
        let specular = fetch("specular", 0, false);
        let environment = definition
            .bindings
            .get("environment")
            .and_then(|b| lookup(b.texture, true))
            .map(|id| Channel {
                id,
                usage: Usage::CLAMP_MIPS,
                cube: true,
            });

        let macro_scale = definition.scalar("macroOverlayUVScale").unwrap_or(0.);
        let macro_opacity = definition.scalar("macroOverlayOpacity").unwrap_or(0.);
        let detail_scale = definition.scalar("detailNormalUVScale").unwrap_or(0.);
        let flags = u32::from(normal.is_some())
            | (u32::from(detail.is_some() && detail_scale > 0.) << 1)
            | (u32::from(
                macro_map.is_some()
                    && macro_scale > 0.
                    && (macro_opacity > 0. || definition.family == 31),
            ) << 2)
            | (u32::from(decal.is_some()) << 3)
            | (u32::from(specular.is_some()) << 4)
            | (u32::from(lightmap.is_some()) << 5)
            | (u32::from(environment.is_some()) << 6)
            | (u32::from(detail.is_some()) << 7);

        let blended = material.alpha_mode == 2
            || definition.family == 32
            || (matches!(definition.family, 30 | 33) && definition.shader.ends_with("alpha"));
        // The same condition sets `mode.z` below. A material without it has a
        // cutoff of -1, which no sampled alpha can fall under, so its class must
        // not compile the `discard`.
        let cutout = material.alpha_mode == 1;
        let class = RenderClass::new(blended, cutout, definition.flags & 4 != 0);

        let mut water = [Vec4::ZERO; 4];
        if let Some(rows) = tuning.rows.get(&definition.shader) {
            for (to, from) in water.iter_mut().zip(rows) {
                *to = Vec4::from_array(*from);
            }
        }
        if definition.family == 14 {
            water[1] = Vec4::new(
                definition.scalar("uAnimationSpeed").unwrap_or(0.),
                definition.scalar("vAnimationSpeed").unwrap_or(0.),
                0.,
                0.,
            );
        }

        Self {
            params: WorldParams {
                mode: Vec4::new(
                    definition.family as f32,
                    flags as f32,
                    if cutout { ALPHA_REF } else { -1. },
                    2.5,
                ),
                foliage_debug: Vec4::ZERO,
                surface: Vec4::new(macro_scale, macro_opacity, detail_scale, 1.),
                family: Vec4::new(0.3435, 0.02, 1., 0.45),
                // Authored by the map's sky package; the defaults evaluate to no
                // fog at all, so a map without one is unchanged.
                fog_ramp: sky.fog_ramp,
                fog_color: sky.fog_color,
                shadow_color: Vec4::ZERO,
                sun_direction: sky.sun_direction,
                decal: Vec4::new(
                    stain_opacity(
                        definition
                            .parameters
                            .get("decal")
                            .and_then(|v| v.first())
                            .map(String::as_str)
                            .unwrap_or(""),
                    ),
                    0.,
                    0.,
                    0.,
                ),
                water,
            },
            class,
            channels: [
                diffuse,
                lightmap,
                normal,
                detail,
                macro_map,
                decal,
                specular,
                environment,
            ],
        }
    }
}

/// Accumulates one slab: its pages, its cube array and its GPU arrays.
struct Slab {
    pages: Vec<Page>,
    /// Canonical texture id to (class, layer) within this slab.
    placed: HashMap<u32, (usize, usize)>,
    cubes: Vec<u32>,
    cube_layer: HashMap<u32, usize>,
    params: Vec<WorldParams>,
    slots: Vec<MaterialSlots>,
}

impl Slab {
    fn new(page_sizes: &[(u32, u32)]) -> Self {
        Self {
            pages: page_sizes
                .iter()
                .map(|&(width, height)| Page {
                    width,
                    height,
                    layers: Vec::new(),
                    mips: false,
                })
                .collect(),
            placed: HashMap::new(),
            cubes: Vec::new(),
            cube_layer: HashMap::new(),
            params: Vec::new(),
            slots: Vec::new(),
        }
    }

    /// The page a texture of this size belongs to: exact match, else the closest
    /// by area so a rescale changes resolution as little as possible.
    fn class_for(&self, width: u32, height: u32) -> Option<usize> {
        if self.pages.is_empty() {
            return None;
        }
        if let Some(index) = self
            .pages
            .iter()
            .position(|p| p.width == width && p.height == height)
        {
            return Some(index);
        }
        let area = u64::from(width) * u64::from(height);
        self.pages
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| {
                (u64::from(p.width) * u64::from(p.height)).abs_diff(area)
            })
            .map(|(index, _)| index)
    }

    /// Layer capacity is the only reason to seal a slab: the page *sizes* are
    /// chosen once for the whole map and every slab shares them.
    fn fits(&self, request: &Request, map: &SkateMap) -> bool {
        let mut needed = [0usize; PAGE_CLASSES];
        let mut counted: Vec<u32> = Vec::new();
        for channel in request.channels.iter().flatten() {
            if channel.cube || self.placed.contains_key(&channel.id) || counted.contains(&channel.id)
            {
                continue;
            }
            counted.push(channel.id);
            let texture = &map.textures[channel.id as usize - 1];
            if let Some(class) = self.class_for(texture.width, texture.height) {
                needed[class] += 1;
            }
        }
        self.pages
            .iter()
            .enumerate()
            .all(|(class, page)| page.layers.len() + needed[class] <= MAX_PAGE_LAYERS)
    }

    fn insert(&mut self, request: &Request, map: &SkateMap, slab: u16) -> MaterialEntry {
        let index = self.params.len() as u32;
        let mut slots = MaterialSlots::default();
        for (channel_index, channel) in request.channels.iter().enumerate() {
            let Some(channel) = channel else { continue };
            if channel.cube {
                let layer = match self.cube_layer.get(&channel.id) {
                    Some(&layer) => layer,
                    None => {
                        let layer = self.cubes.len();
                        self.cubes.push(channel.id);
                        self.cube_layer.insert(channel.id, layer);
                        layer
                    }
                };
                // Cubes have their own binding, so the class field is unused.
                slots.0[channel_index] = pack_slot(0, layer, true);
                continue;
            }
            let texture = &map.textures[channel.id as usize - 1];
            let Some(class) = self.class_for(texture.width, texture.height) else {
                continue;
            };
            let layer = match self.placed.get(&channel.id) {
                Some(&(_, layer)) => layer,
                None => {
                    let page = &mut self.pages[class];
                    page.layers.push(channel.id);
                    let layer = page.layers.len() - 1;
                    self.placed.insert(channel.id, (class, layer));
                    layer
                }
            };
            self.pages[class].mips |= channel.usage.mips;
            slots.0[channel_index] = pack_slot(class, layer, channel.usage.clamp);
        }
        self.params.push(request.params.clone());
        self.slots.push(slots);
        MaterialEntry {
            slab,
            class: request.class,
            index,
        }
    }

    fn upload_page(
        &self,
        class: usize,
        map: &SkateMap,
        images: &mut impl AssetSink<Image>,
    ) -> Handle<Image> {
        let Some(page) = self.pages.get(class).filter(|p| !p.layers.is_empty()) else {
            return placeholder_page(images);
        };
        let mut bytes = Vec::new();
        let mut levels = 1;
        for &id in &page.layers {
            let texture = &map.textures[id as usize - 1];
            let owned;
            let rgba = if texture.width == page.width && texture.height == page.height {
                &texture.rgba
            } else {
                owned = resample(
                    &texture.rgba,
                    texture.width,
                    texture.height,
                    page.width,
                    page.height,
                );
                &owned
            };
            if page.mips {
                let (chain, count) = mip_chain(rgba, page.width, page.height, 1);
                levels = count;
                bytes.extend_from_slice(&chain);
            } else {
                bytes.extend_from_slice(rgba);
            }
        }
        images.add(array_image(
            page.width,
            page.height,
            page.layers.len() as u32,
            levels,
            bytes,
            TextureViewDimension::D2Array,
        ))
    }

    fn upload_cubes(&self, map: &SkateMap, images: &mut impl AssetSink<Image>) -> Handle<Image> {
        let Some(&first) = self.cubes.first() else {
            return placeholder_cube(images);
        };
        let size = map.textures[first as usize - 1].width;
        let mut bytes = Vec::new();
        let mut levels = 1;
        let mut faces = 0;
        for &id in &self.cubes {
            let texture = &map.textures[id as usize - 1];
            if texture.width != size {
                // A cube array shares one face size. Mismatches are rare enough
                // to drop rather than rescale; the slot still reads as present,
                // so log it instead of silently changing reflections.
                warn!(
                    "environment cube '{}' is {}px in a {}px array; reflection omitted",
                    texture.name, texture.width, size
                );
                continue;
            }
            let (chain, count) = mip_chain(&texture.rgba, size, size, 6);
            levels = count;
            bytes.extend_from_slice(&chain);
            faces += 6;
        }
        if faces == 0 {
            return placeholder_cube(images);
        }
        images.add(array_image(
            size,
            size,
            faces,
            levels,
            bytes,
            TextureViewDimension::CubeArray,
        ))
    }
}

fn array_image(
    width: u32,
    height: u32,
    layers: u32,
    levels: u32,
    bytes: Vec<u8>,
    dimension: TextureViewDimension,
) -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width,
            height,
            depth_or_array_layers: layers,
        },
        TextureDimension::D2,
        // Retail world textures are read raw and squared in the shader, which is
        // the approximate sRGB decode the native renderer used. Uploading them
        // as sRGB would apply the decode twice.
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(bytes);
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(dimension),
        ..default()
    });
    // One sampler serves every page; per-channel clamping is a bit in the slot
    // record, applied to the coordinates in the shader.
    let mut sampler = ImageSamplerDescriptor::linear();
    sampler.address_mode_u = ImageAddressMode::Repeat;
    sampler.address_mode_v = ImageAddressMode::Repeat;
    image.sampler = ImageSampler::Descriptor(sampler);
    image
}

/// Unused bindings still need a resource of the right shape.
fn placeholder_page(images: &mut impl AssetSink<Image>) -> Handle<Image> {
    images.add(array_image(
        1,
        1,
        1,
        1,
        vec![0, 0, 0, 255],
        TextureViewDimension::D2Array,
    ))
}

fn placeholder_cube(images: &mut impl AssetSink<Image>) -> Handle<Image> {
    images.add(array_image(
        1,
        1,
        6,
        1,
        vec![0, 0, 0, 255].repeat(6),
        TextureViewDimension::CubeArray,
    ))
}

// ---------------------------------------------------------------------------
// Scene detection and plugin
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub(crate) struct RetailScene(pub bool);

impl RetailScene {
    /// Package evidence, never the editable map name. Older Skate 2 exports can
    /// retain native provenance/sky/collision without material definitions.
    pub(crate) fn for_map(map: &SkateMap) -> bool {
        map.materials.iter().any(|m| m.retail_definition.is_some())
            || map
                .extensions
                .iter()
                .any(|e| matches!(&e.tag, b"WMET" | b"RWCM" | b"SKYB"))
            || map.geometry.collision.iter().any(|c| c.native_edges.is_some())
            || map.rails.iter().any(|r| r.native.is_some())
    }
}

#[derive(serde::Deserialize, Resource)]
struct OceanPca {
    hz: f32,
    frames: Vec<[[f32; 4]; 7]>,
}

fn read_pca(root: &std::path::Path) -> Option<OceanPca> {
    let pca = std::fs::read(root.join("private/ocean-pca.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<OceanPca>(&b).ok())?;
    (pca.hz == 30.
        && pca.frames.len() == 30
        && pca.frames.iter().flatten().flatten().all(|v| v.is_finite()))
    .then_some(pca)
}

fn load_pca(mut commands: Commands, config: Res<crate::config::Config>) {
    if let Some(pca) = read_pca(&config.asset_root) {
        info!("RETAIL_OCEAN: loaded 30 authored PCA frames");
        commands.insert_resource(pca);
    }
}

fn initialize_frame_state(mut buffers: ResMut<Assets<ShaderStorageBuffer>>) {
    if let Err(error) = buffers.insert(
        FRAME_STATE.id(),
        ShaderStorageBuffer::new(
            &FrameStateData::default().encode(),
            RenderAssetUsages::RENDER_WORLD,
        ),
    ) {
        // Without this buffer every world bind group is unsatisfiable, so the
        // whole map would render black. Fail loudly.
        error!("could not install the shared frame state buffer: {error}");
    }
}

/// Advances the animation clock and ocean PCA frame. `shadow` stays zero while
/// shadows are out of scope (RFC 1 D5), which keeps the shader off the cascade
/// bindings entirely.
fn advance_frame_state(
    mut state: ResMut<FrameStateData>,
    mut buffers: ResMut<Assets<ShaderStorageBuffer>>,
    time: Res<Time>,
    pca: Option<Res<OceanPca>>,
) {
    state.clock.x = time.elapsed_secs();
    if let Some(pca) = pca {
        let frame = ((time.elapsed_secs_f64() * f64::from(pca.hz)) as usize) % pca.frames.len();
        state.pca = pca.frames[frame].map(Vec4::from_array);
        state.clock.y = 1.;
    }
    if let Some(buffer) = buffers.get_mut(&FRAME_STATE) {
        buffer.data = Some(state.encode());
    }
}

#[cfg(test)]
#[path = "retail_shader_tests.rs"]
mod shader_tests;

pub(crate) struct RetailRenderPlugin;

impl Plugin for RetailRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "retail_world.wgsl");
        embedded_asset!(app, "retail_depth.wgsl");
        embedded_asset!(app, "retail_sky.wgsl");
        bevy::shader::load_shader_library!(app, "retail_material_bindings.wgsl");
        app.init_resource::<FrameStateData>()
            .add_plugins((
                MaterialPlugin::<WorldMaterial>::default(),
                MaterialPlugin::<crate::retail_sky::SkyMaterial>::default(),
                crate::retail_character::CharacterLightingPlugin,
                crate::retail_exposure::RetailExposurePlugin,
            ))
            .add_systems(Startup, (initialize_frame_state, load_pca))
            .add_systems(Update, advance_frame_state);
    }
}

pub(crate) fn world_changed(
    mut messages: MessageReader<crate::map_transition::WorldChanged>,
) -> bool {
    messages.read().count() != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The per-vertex material index only works if no vertex is shared between
    /// triangles of different materials. `main` never had to care: it split
    /// geometry by material, so each partition re-indexed its own vertices.
    /// Merged geometry keeps the source indices, and `@interpolate(flat)` reads
    /// one provoking vertex, so a shared vertex would shade a whole triangle
    /// with a neighbour's material — every symptom of shuffled textures.
    #[test]
    #[ignore = "Requires private installed assets; diagnostic only"]
    fn no_vertex_is_shared_between_materials() {
        let path = std::path::PathBuf::from(
            std::env::var("SKATE_BUDGET_MAP").expect("SKATE_BUDGET_MAP"),
        );
        let map = skate_data::skate_map::SkateMap::load(&path).unwrap();
        let mut conflicts = 0usize;
        let mut corner_disagreements = 0usize;
        for tri in map.geometry.indices.chunks_exact(3) {
            let materials = [
                map.geometry.vertices[tri[0] as usize].material,
                map.geometry.vertices[tri[1] as usize].material,
                map.geometry.vertices[tri[2] as usize].material,
            ];
            if materials[0] != materials[1] || materials[1] != materials[2] {
                corner_disagreements += 1;
            }
        }
        // A vertex referenced by triangles whose corner-0 materials differ.
        let mut owner: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        for tri in map.geometry.indices.chunks_exact(3) {
            let material = map.geometry.vertices[tri[0] as usize].material;
            for &index in tri {
                match owner.get(&index) {
                    Some(&existing) if existing != material => conflicts += 1,
                    _ => {
                        owner.insert(index, material);
                    }
                }
            }
        }
        eprintln!(
            "VERTEX_SHARING triangles={} vertices={} corner_disagreements={} cross_material_refs={}",
            map.geometry.indices.len() / 3,
            map.geometry.vertices.len(),
            corner_disagreements,
            conflicts
        );
        assert_eq!(
            corner_disagreements, 0,
            "a triangle spans materials, so corner 0 is not authoritative"
        );
        assert_eq!(
            conflicts, 0,
            "vertices are shared across materials; flat interpolation cannot carry \
             a per-vertex material index without splitting them"
        );
    }

    /// `Vertex::material` is one-based with zero meaning "no material". Reading
    /// it as a direct index shades every surface with the *next* material's
    /// textures, which renders cleanly at full frame rate and is invisible to
    /// draw-count and shader-validation tests - it just puts the wrong texture
    /// on almost every object. The one-based reading is what makes the highest
    /// index reach `materials.len()` rather than stop one short.
    #[test]
    #[ignore = "Requires private installed assets"]
    fn vertex_material_indices_are_one_based() {
        let path = std::path::PathBuf::from(
            std::env::var("SKATE_BUDGET_MAP").expect("SKATE_BUDGET_MAP"),
        );
        let map = skate_data::skate_map::SkateMap::load(&path).unwrap();
        let highest = map
            .geometry
            .vertices
            .iter()
            .map(|v| v.material)
            .max()
            .expect("geometry has vertices");
        let lowest = map
            .geometry
            .vertices
            .iter()
            .map(|v| v.material)
            .filter(|&m| m != 0)
            .min()
            .expect("geometry references a material");
        eprintln!(
            "MATERIAL_BASE materials={} lowest={lowest} highest={highest}",
            map.materials.len()
        );
        assert_eq!(
            highest as usize,
            map.materials.len(),
            "the highest material index should reach materials.len() under a \
             one-based reading; a zero-based reading would stop one short"
        );
        assert_eq!(lowest, 1, "one-based indices start at 1");
    }

    /// Reports what `Binding::texture` actually contains, because the reference
    /// delegated that decision to a caller-supplied closure and the two callers
    /// disagreed: one treats the value as a global 1-based texture id, the other
    /// as a slot into the material's own five-entry list. Guessing wrong
    /// substitutes one real texture for another, which renders cleanly and looks
    /// like the textures were shuffled.
    #[test]
    #[ignore = "Requires private installed assets; diagnostic only"]
    fn binding_texture_ids_are_global_not_material_local() {
        let path = std::path::PathBuf::from(
            std::env::var("SKATE_BUDGET_MAP").expect("SKATE_BUDGET_MAP"),
        );
        let map = skate_data::skate_map::SkateMap::load(&path).unwrap();
        let mut histogram: BTreeMap<u32, usize> = BTreeMap::new();
        let mut roles: BTreeMap<String, (u32, u32, usize)> = BTreeMap::new();
        let mut with_definition = 0usize;
        let mut agrees_with_local = 0usize;
        let mut agrees_with_global = 0usize;
        for material in &map.materials {
            let Some(definition) = material
                .retail_definition
                .as_deref()
                .and_then(Definition::parse)
            else {
                continue;
            };
            with_definition += 1;
            for (role, binding) in &definition.bindings {
                *histogram.entry(binding.texture.min(64)).or_default() += 1;
                let entry = roles.entry(role.clone()).or_insert((u32::MAX, 0, 0));
                entry.0 = entry.0.min(binding.texture);
                entry.1 = entry.1.max(binding.texture);
                entry.2 += 1;
            }
            // If ids are material-local, the diffuse binding indexes
            // `material.textures`; if global, it is already a texture id.
            if let Some(diffuse) = definition.bindings.get("diffuse") {
                if (diffuse.texture as usize) < material.textures.len()
                    && material.textures[diffuse.texture as usize] != 0
                {
                    agrees_with_local += 1;
                }
                if diffuse.texture as usize <= map.textures.len() && diffuse.texture != 0 {
                    agrees_with_global += 1;
                }
            }
        }
        eprintln!(
            "BINDING_IDS materials={} with_definition={} textures={} diffuse_plausible_local={} diffuse_plausible_global={}",
            map.materials.len(),
            with_definition,
            map.textures.len(),
            agrees_with_local,
            agrees_with_global
        );
        for (role, (min, max, count)) in &roles {
            eprintln!("BINDING_ROLE {role} min={min} max={max} count={count}");
        }
        let capped: usize = histogram.iter().filter(|(k, _)| **k >= 64).map(|(_, v)| *v).sum();
        eprintln!(
            "BINDING_SMALL_IDS below_five={} at_or_above_64={}",
            histogram.iter().filter(|(k, _)| **k < 5).map(|(_, v)| *v).sum::<usize>(),
            capped
        );
        assert_eq!(
            agrees_with_local, 0,
            "binding ids are not slots into the material's own texture list"
        );
        assert_eq!(
            agrees_with_global, with_definition,
            "every diffuse binding should be a valid one-based global texture id"
        );
    }

    #[test]
    fn gpu_struct_sizes_match_the_shader() {
        let mut bytes = Vec::new();
        WorldParams::default().encode(&mut bytes);
        assert_eq!(bytes.len(), WorldParams::SIZE);
        assert_eq!(WorldParams::SIZE, 208);

        let mut bytes = Vec::new();
        MaterialSlots::default().encode(&mut bytes);
        assert_eq!(bytes.len(), MaterialSlots::SIZE);

        assert_eq!(FrameStateData::default().encode().len(), FrameStateData::SIZE);
    }

    #[test]
    fn slot_packing_round_trips() {
        let packed = pack_slot(5, 1234, true);
        assert_eq!((packed >> 16) & 0xff, 5);
        assert_eq!(packed & 0xffff, 1234);
        assert_ne!(packed & 0x8000_0000, 0);
        assert_ne!(packed, ABSENT);
        assert_eq!((ABSENT >> 16) & 0xff, 0xff);
    }

    #[test]
    fn identical_textures_share_one_layer() {
        let texture = |name: &str, color_space, pixel| skate_data::skate_map::Texture {
            name: name.into(),
            width: 1,
            height: 1,
            color_space,
            rgba: vec![pixel, 0, 0, 255],
        };
        let textures = vec![
            texture("first", 1, 30),
            texture("duplicate", 1, 30),
            texture("linear", 0, 30),
            texture("different", 1, 31),
        ];
        assert_eq!(canonical_texture_ids(&textures), [0, 1, 1, 3, 4]);
    }

    #[test]
    fn weathering_tuning_preserves_artwork() {
        for name in [
            "decal_WEAR_WaterStain_01",
            "subway_grunge03",
            "decal_GrimePuddle",
            "OT_Ground_decals",
        ] {
            assert_eq!(stain_opacity(name), 0.35);
        }
        for name in [
            "decal_Graphic_SP_UN_Shark_01",
            "decal_other_sp_arrowramps_01",
            "decal_Wear_GL_UN_MPedge_01",
            "",
        ] {
            assert_eq!(stain_opacity(name), 1.);
        }
    }

    #[test]
    fn resample_averages_rather_than_dropping_texels() {
        // 2x2 white/black checker to 1x1 must average, not pick a corner.
        let rgba = vec![
            255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255,
        ];
        assert_eq!(resample(&rgba, 2, 2, 1, 1), vec![127, 127, 127, 255]);
    }

    #[test]
    fn classes_split_on_culling_blending_and_alpha_testing() {
        assert_eq!(
            RenderClass::new(false, false, false),
            RenderClass::Opaque,
            "no blending, no cutoff and back-face culling is the plain case"
        );
        assert_eq!(RenderClass::new(false, true, false), RenderClass::Cutout);
        assert_eq!(
            RenderClass::new(true, true, true),
            RenderClass::BlendedTwoSided,
            "blending subsumes the cutoff rather than splitting again"
        );
        assert!(RenderClass::OpaqueTwoSided.two_sided());
        assert!(RenderClass::CutoutTwoSided.two_sided());
        assert!(!RenderClass::Cutout.two_sided());
        assert_eq!(RenderClass::ALL.len(), 6);
    }

    /// The whole point of the split: only the opaque classes may drop the
    /// `discard`, and they must not claim an alpha mode that makes Bevy set
    /// MAY_DISCARD for them anyway.
    #[test]
    fn only_opaque_classes_give_up_the_discard() {
        for class in RenderClass::ALL {
            let opaque = matches!(class, RenderClass::Opaque | RenderClass::OpaqueTwoSided);
            assert_eq!(class.discards(), !opaque, "{class:?}");
            assert_eq!(
                class.alpha_mode() == AlphaMode::Opaque,
                opaque,
                "{class:?} must report Opaque exactly when it drops the discard"
            );
        }
    }
}
