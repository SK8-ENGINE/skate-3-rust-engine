//! Retail world shading. See docs/retail-renderer.md for provenance and gaps.
use bevy::{
    asset::embedded_asset,
    core_pipeline::{
        core_3d::graph::Node3d,
        fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin},
    },
    prelude::*,
    render::{
        extract_component::ExtractComponent,
        render_graph::{InternedRenderLabel, RenderLabel},
        render_resource::{AsBindGroup, ShaderType},
    },
    shader::ShaderRef,
};
use std::collections::BTreeMap;

pub(crate) struct RetailRenderPlugin;
impl Plugin for RetailRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "retail_world.wgsl");
        embedded_asset!(app, "retail_tone.wgsl");
        embedded_asset!(app, "retail_depth.wgsl");
        embedded_asset!(app, "retail_sky.wgsl");
        app.add_plugins((
            MaterialPlugin::<RetailWorldMaterial>::default(),
            MaterialPlugin::<RetailSkyMaterial>::default(),
            FullscreenMaterialPlugin::<RetailTone>::default(),
        ));
    }
}

#[derive(Resource)]
pub(crate) struct RetailScene(pub bool);

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct RetailSkyMaterial {
    #[uniform(0)]
    pub params: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Handle<Image>,
}
impl Material for RetailSkyMaterial {
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn vertex_shader() -> ShaderRef {
        "embedded://skate3rust/retail_sky.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        Self::vertex_shader()
    }
    fn specialize(
        _: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _: &bevy::mesh::MeshVertexBufferLayoutRef,
        _: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = false;
        }
        Ok(())
    }
}

pub(crate) fn spawn_sky(
    name: &str,
    root: &std::path::Path,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<RetailSkyMaterial>,
) {
    // Parks need their authored sky selection from the environment controller;
    // only these three district identities are currently resolved.
    if !matches!(name, "University" | "DownTown" | "Industrial") {
        return;
    }
    #[derive(serde::Deserialize)]
    struct Sky {
        width: u32,
        height: u32,
        positions: Vec<[f32; 3]>,
        uvs: Vec<[f32; 2]>,
        indices: Vec<u32>,
    }
    let load = || -> Result<(Sky, Vec<u8>), String> {
        let base = root.join("private/native-skies");
        let sky: Sky = serde_json::from_slice(
            &std::fs::read(base.join(format!("{name}.json"))).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let rgba = std::fs::read(base.join(format!("{name}.rgba"))).map_err(|e| e.to_string())?;
        if sky.width == 0
            || sky.height == 0
            || u64::from(sky.width) * u64::from(sky.height) * 4 != rgba.len() as u64
            || sky.positions.len() != sky.uvs.len()
            || sky
                .indices
                .iter()
                .any(|&i| i as usize >= sky.positions.len())
        {
            return Err("Invalid retail sky dimensions/geometry".into());
        }
        Ok((sky, rgba))
    };
    let (sky, rgba) = match load() {
        Ok(v) => v,
        Err(e) => {
            warn!("Retail sky unavailable: {e}");
            return;
        }
    };
    let mut image = Image::new(
        bevy::render::render_resource::Extent3d {
            width: sky.width,
            height: sky.height,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        rgba,
        bevy::render::render_resource::TextureFormat::Rgba8Unorm,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    let mut sampler = bevy::image::ImageSamplerDescriptor::linear();
    sampler.address_mode_u = bevy::image::ImageAddressMode::Repeat;
    image.sampler = bevy::image::ImageSampler::Descriptor(sampler);
    let mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, sky.positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, sky.uvs)
    .with_inserted_indices(bevy::mesh::Indices::U32(sky.indices));
    commands.spawn((
        Name::new("Retail sky dome"),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(RetailSkyMaterial {
            params: Vec4::new(165., 2.5, 1., 0.),
            diffuse: images.add(image),
        })),
        Transform::default(),
        bevy::camera::visibility::NoFrustumCulling,
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
    ));
}

#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default)]
pub(crate) struct RetailTone {
    pub enabled: Vec4,
}
impl FullscreenMaterial for RetailTone {
    fn fragment_shader() -> ShaderRef {
        "embedded://skate3rust/retail_tone.wgsl".into()
    }
    fn node_edges() -> Vec<InternedRenderLabel> {
        vec![
            Node3d::Tonemapping.intern(),
            Self::node_label().intern(),
            Node3d::EndMainPassPostProcessing.intern(),
        ]
    }
}

#[derive(Clone, Debug, ShaderType)]
pub(crate) struct WorldParams {
    // family, texture flags, alpha cutoff (-1 for opaque), exposure
    pub mode: Vec4,
    // macro UV scale, opacity, detail UV scale, material multiplier
    pub surface: Vec4,
    // tree LM scale/floor/tint, proxy multiplier: reference day capture
    pub family: Vec4,
    pub fog_ramp: Vec4,
    pub fog_color: Vec4,
    pub shadow_color: Vec4,
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
        let _shader = r.text()?;
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
    pub fn supported(&self) -> bool {
        (1..=12).contains(&self.family) || self.family == 13
    }
    pub fn build(
        &self,
        m: &skate_data::skate_map::Material,
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
        let detail = fetch("detail", 0, false);
        let macro_map = fetch("macrooverlay", 0, false);
        let decal = fetch("decal", 0, self.family == 3);
        let specular = fetch("specular", 0, false);
        let environment = texture(self.bindings.get("environment").map_or(0, |b| b.texture), 5);
        let macro_scale = self.scalar("macroOverlayUVScale").unwrap_or(0.);
        let macro_opacity = self.scalar("macroOverlayOpacity").unwrap_or(0.);
        let detail_scale = self.scalar("detailNormalUVScale").unwrap_or(0.);
        let flags = u32::from(normal.is_some())
            | (u32::from(detail.is_some() && detail_scale > 0.) << 1)
            | (u32::from(macro_map.is_some() && macro_scale > 0. && macro_opacity > 0.) << 2)
            | (u32::from(decal.is_some()) << 3)
            | (u32::from(specular.is_some()) << 4)
            | (u32::from(lightmap.is_some()) << 5)
            | (u32::from(environment.is_some()) << 6)
            | (u32::from(detail.is_some()) << 7);
        // scene.hlsl's retail world ALPHAREF is 30, not the portable 0.5.
        let cutoff = 30. / 255.;
        let alpha = match m.alpha_mode {
            1 => AlphaMode::Mask(cutoff),
            2 => AlphaMode::Blend,
            _ => AlphaMode::Opaque,
        };
        RetailWorldMaterial {
            params: WorldParams {
                mode: Vec4::new(
                    self.family as f32,
                    flags as f32,
                    if m.alpha_mode == 1 { cutoff } else { -1. },
                    2.5,
                ),
                surface: Vec4::new(macro_scale, macro_opacity, detail_scale, 1.),
                family: Vec4::new(0.3435, 0.02, 1., 0.45),
                fog_ramp: Vec4::new(0., 0., 1., 0.),
                fog_color: Vec4::ZERO,
                shadow_color: Vec4::ZERO,
            },
            diffuse,
            lightmap,
            normal,
            detail,
            macro_map,
            decal,
            specular,
            environment,
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
