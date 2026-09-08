//! Retail world shading. See docs/retail-renderer.md for provenance and gaps.
use bevy::{
    asset::embedded_asset,
    prelude::*,
    render::{
        extract_component::ExtractComponent,
        render_resource::{AsBindGroup, ShaderType},
    },
    shader::ShaderRef,
};
use std::collections::BTreeMap;

pub(crate) struct RetailRenderPlugin;
impl Plugin for RetailRenderPlugin {
    fn build(&self, app: &mut App) {
        shadow::install(app);
        exposure::install(app);
        if std::env::var_os("SKATE_WEATHERING_COMPARE").is_some_and(|v| v == "1") {
            app.add_systems(Update, compare_weathering);
        }
        app.add_plugins(crate::retail_character::CharacterLightingPlugin);
        if std::env::var_os("SKATE_DEBUG_FOLIAGE").is_some_and(|v| v == "1") {
            eprintln!("SKATE_FOLIAGE_DEBUG: solid cyan tree-wall cards, magenta other foliage; alpha rejection disabled for foliage only");
        }
        embedded_asset!(app, "retail_world.wgsl");
        embedded_asset!(app, "retail_tone.wgsl");
        embedded_asset!(app, "retail_depth.wgsl");
        embedded_asset!(app, "retail_sky.wgsl");
        app.add_plugins((
            MaterialPlugin::<RetailWorldMaterial>::default(),
            MaterialPlugin::<RetailSkyMaterial>::default(),
        ));
    }
}

// Diagnostic only: mutate materials once per keypress, never continuously.
// Separate bits preserve the original texture-presence flags for restoration.
fn compare_weathering(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: Local<u32>,
    mut materials: ResMut<Assets<RetailWorldMaterial>>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    if !keys.just_pressed(KeyCode::F8) { return; }
    *mode = (*mode + 1) % 4;
    let label = match *mode {
        1 => "Repeating grime OFF; decals ON",
        2 => "Repeating grime ON; decals OFF",
        3 => "Repeating grime OFF; decals OFF",
        _ => "Authored grime and decals ON",
    };
    for (_, material) in materials.iter_mut() {
        let family = material.params.mode.x as u32;
        if !(1..=8).contains(&family) { continue; }
        let flags = material.params.mode.y as u32;
        material.params.mode.y = ((flags & !768) | (*mode << 8)) as f32;
    }
    for mut window in &mut windows {
        window.title = format!("Skate 3 - {label} [F8: next comparison]");
    }
    info!("WEATHERING_COMPARE: {label}");
}

#[path = "retail_exposure.rs"]
mod exposure;

#[path = "retail_shadow.rs"]
mod shadow;
pub(crate) use shadow::ShadowState;

#[derive(Resource)]
pub(crate) struct RetailScene(pub bool);

#[path = "retail_sky.rs"]
mod sky;
pub(crate) use sky::{spawn_sky, RetailSkyMaterial};

#[path = "retail_backdrop.rs"]
mod backdrop;
pub(crate) use backdrop::spawn_backdrop;

#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default)]
pub(crate) struct RetailTone {
    pub enabled: Vec4,
}

#[derive(Clone, Debug, ShaderType)]
pub(crate) struct WorldParams {
    // family, texture flags, alpha cutoff (-1 for opaque), exposure
    pub mode: Vec4,
    // Diagnostic solid foliage colour; w=0 keeps retail shading.
    pub foliage_debug: Vec4,
    // macro UV scale, opacity, detail UV scale, material multiplier
    pub surface: Vec4,
    // tree LM scale/floor/tint, proxy multiplier: reference day capture
    pub family: Vec4,
    pub fog_ramp: Vec4,
    pub fog_color: Vec4,
    pub shadow_color: Vec4,
    pub sun_direction: Vec4,
    pub water: [Vec4; 4],
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(RetailKey)]
pub(crate) struct RetailWorldMaterial {
    #[uniform(0)]
    pub params: WorldParams,
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub lightmap: Option<Handle<Image>>,
    #[texture(5)]
    #[sampler(6)]
    pub normal: Option<Handle<Image>>,
    #[texture(7)]
    #[sampler(8)]
    pub detail: Option<Handle<Image>>,
    #[texture(9)]
    #[sampler(10)]
    pub macro_map: Option<Handle<Image>>,
    #[texture(11)]
    #[sampler(12)]
    pub decal: Option<Handle<Image>>,
    #[texture(13)]
    #[sampler(14)]
    pub specular: Option<Handle<Image>>,
    #[texture(15, dimension = "cube")]
    pub environment: Option<Handle<Image>>,
    #[storage(16, read_only)]
    pub shadow_state: Handle<bevy::render::storage::ShaderStorageBuffer>,
    pub alpha: AlphaMode,
    pub two_sided: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RetailKey {
    two_sided: bool,
}
impl From<&RetailWorldMaterial> for RetailKey {
    fn from(m: &RetailWorldMaterial) -> Self {
        Self {
            two_sided: m.two_sided,
        }
    }
}
impl Material for RetailWorldMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://skate3rust/retail_world.wgsl".into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://skate3rust/retail_depth.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }
    fn specialize(
        _: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        // Retail foliage is two-sided; opaque world triangles retain winding.
        descriptor.primitive.cull_mode = if key.bind_group_data.two_sided {
            None
        } else {
            Some(bevy::render::render_resource::Face::Back)
        };
        Ok(())
    }
}

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
    pub fn parse(bytes: &[u8]) -> Option<Self> {
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
        let family = r.u32()?;
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
    pub fn scalar(&self, name: &str) -> Option<f32> {
        self.parameters
            .get(name)?
            .first()?
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
    }
    pub fn supported(&self, tuning: &MaterialTuning) -> bool {
        (1..=13).contains(&self.family) || match self.family {
            14 | 32 => tuning.rows.get(&self.shader).is_some_and(|r| !r.is_empty()),
            31 => tuning.pca_available && tuning.rows.get(&self.shader).is_some_and(|r| r.len() == 3),
            30 => tuning.rows.get(&self.shader).is_some_and(|r| r.len() == 4),
            _ => false,
        }
    }
    pub fn build(
        &self,
        m: &skate_data::skate_map::Material,
        tuning: &MaterialTuning,
        texture: &mut impl FnMut(u32, u8) -> Option<Handle<Image>>,
    ) -> RetailWorldMaterial {
        let mut fetch = |role: &str, fallback: u32, clamp: bool| {
            let b = self.bindings.get(role);
            texture(
                b.map_or(fallback, |b| b.texture),
                if clamp || b.is_some_and(|b| b.u == 1 && b.v == 1) {
                    4
                } else {
                    3
                },
            )
        };
        let diffuse = fetch("diffuse", m.textures[0], false);
        let lightmap = fetch("lightmap", m.textures[1], true);
        let normal = fetch("normal", m.textures[2], false);
        let detail = fetch(if self.family == 31 { "normal2" } else { "detail" }, 0, false);
        let macro_map = fetch("macrooverlay", 0, false);
        let decal = fetch("decal", 0, self.family == 3);
        let specular = fetch("specular", 0, false);
        let environment = texture(self.bindings.get("environment").map_or(0, |b| b.texture), 5);
        let macro_scale = self.scalar("macroOverlayUVScale").unwrap_or(0.);
        let macro_opacity = self.scalar("macroOverlayOpacity").unwrap_or(0.);
        let detail_scale = self.scalar("detailNormalUVScale").unwrap_or(0.);
        let flags = u32::from(normal.is_some())
            | (u32::from(detail.is_some() && detail_scale > 0.) << 1)
            | (u32::from(macro_map.is_some() && macro_scale > 0. && (macro_opacity > 0. || self.family == 31)) << 2)
            | (u32::from(decal.is_some()) << 3)
            | (u32::from(specular.is_some()) << 4)
            | (u32::from(lightmap.is_some()) << 5)
            | (u32::from(environment.is_some()) << 6)
            | (u32::from(detail.is_some()) << 7);
        // scene.hlsl's retail world ALPHAREF is 30, not the portable 0.5.
        let cutoff = 30. / 255.;
        let debug_foliage = matches!(self.family, 9 | 10)
            && std::env::var_os("SKATE_DEBUG_FOLIAGE").is_some_and(|v| v == "1");
        let tree_wall = m.retail_definition.as_deref().is_some_and(|bytes| {
            bytes.windows(b"TreeWall".len()).any(|s| s == b"TreeWall")
        });
        let alpha = match (debug_foliage, m.alpha_mode) {
            (true, _) => AlphaMode::Opaque,
            (_, 1) => AlphaMode::Mask(cutoff),
            (_, 2) => AlphaMode::Blend,
            _ => AlphaMode::Opaque,
        };
        let alpha = if self.family == 32 || (self.family == 30 && self.shader.ends_with("alpha")) {
            AlphaMode::Blend
        } else { alpha };
        let mut water = [Vec4::ZERO; 4];
        if let Some(rows) = tuning.rows.get(&self.shader) {
            for (to, from) in water.iter_mut().zip(rows) { *to = Vec4::from_array(*from); }
        }
        if self.family == 14 {
            water[1] = Vec4::new(self.scalar("uAnimationSpeed").unwrap_or(0.), self.scalar("vAnimationSpeed").unwrap_or(0.), 0., 0.);
        }
        RetailWorldMaterial {
            params: WorldParams {
                mode: Vec4::new(
                    self.family as f32,
                    flags as f32,
                    if m.alpha_mode == 1 && !debug_foliage { cutoff } else { -1. },
                    2.5,
                ),
                foliage_debug: if !debug_foliage {
                    Vec4::ZERO
                } else if tree_wall {
                    Vec4::new(0., 1., 1., 1.)
                } else {
                    Vec4::new(1., 0., 1., 1.)
                },
                surface: Vec4::new(macro_scale, macro_opacity, detail_scale, 1.),
                family: Vec4::new(0.3435, 0.02, 1., 0.45),
                fog_ramp: Vec4::new(0., 0., 1., 0.),
                fog_color: Vec4::ZERO,
                shadow_color: Vec4::ZERO,
                sun_direction: Vec3::new(4., 7., 4.).normalize().extend(0.),
                water,
            },
            diffuse,
            lightmap,
            normal,
            detail,
            macro_map,
            decal,
            specular,
            environment,
            shadow_state: shadow::BUFFER,
            alpha,
            two_sided: self.flags & 4 != 0,
        }
    }
}

/// Native renderer box-filters decoded UNORM cube faces down to 1x1.
/// The same chain prevents aliasing of repeating world maps at distance.
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
                                sum += level[((y * 2 + dy).min(h - 1) * w
                                    + (x * 2 + dx).min(w - 1))
                                    * 4
                                    + c] as u32;
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


#[derive(Default)]
pub(crate) struct MaterialTuning {
    rows: BTreeMap<String, Vec<[f32; 4]>>,
    pca_available: bool,
}
impl MaterialTuning {
    pub(crate) fn load(root: &std::path::Path) -> Self {
        let path = root.join("private/render-parameters.json");
        match std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<BTreeMap<String, Vec<[f32; 4]>>>(&b).ok()) {
            Some(rows) if rows.values().flatten().flatten().all(|x| x.is_finite()) => Self { rows, pca_available: shadow::pca_available(root) },
            _ => { warn!("Retail water/scroll tuning unavailable: {}", path.display()); Self::default() }
        }
    }
}
