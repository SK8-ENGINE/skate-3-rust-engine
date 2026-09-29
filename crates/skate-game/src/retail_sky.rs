//! Authored camera-relative sky dome, its radial sun gradient, and the fog frame
//! the world shader reads.
//!
//! The dome itself is a conventional one-draw material: a single mesh with a
//! single panorama, so none of the slabbing machinery in `retail_render` applies.
//! What *does* have to reach the world is the authored environment — sun
//! direction and fog ramp. Those live in the per-material storage buffer, which
//! is encoded once while the material table is built, so the sky package is
//! loaded *before* the world and its environment is threaded through
//! `MaterialTable::build` rather than patched into materials afterwards.
use bevy::{
    asset::{AssetPath, RenderAssetUsages, embedded_path},
    prelude::*,
    render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType},
    shader::ShaderRef,
};

use crate::map_render::{AssetSink, SceneCommands};

/// What the world shader needs from the sky package.
///
/// The defaults are the no-sky case: the same sun vector the material table used
/// before skies were authored, and a fog ramp that evaluates to zero fog for
/// every distance (`saturate(d*0 + 0)`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct SkyEnvironment {
    pub sun_direction: Vec4,
    pub fog_ramp: Vec4,
    pub fog_color: Vec4,
}

impl Default for SkyEnvironment {
    fn default() -> Self {
        Self {
            sun_direction: Vec3::new(4., 7., 4.).normalize().extend(0.),
            fog_ramp: Vec4::new(0., 0., 1., 0.),
            fog_color: Vec4::ZERO,
        }
    }
}

#[derive(Clone, Debug, ShaderType)]
pub(crate) struct SkyParams {
    /// anchor height, scene exposure, sky multiplier, sun angular scale (0 disables)
    pub settings: Vec4,
    pub sun_direction: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct SkyMaterial {
    #[uniform(0)]
    pub params: SkyParams,
    #[texture(1)]
    #[sampler(2)]
    pub panorama: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub sun: Option<Handle<Image>>,
}

impl Material for SkyMaterial {
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn vertex_shader() -> ShaderRef {
        AssetPath::from(embedded_path!("retail_sky.wgsl"))
            .with_source("embedded")
            .into()
    }

    fn fragment_shader() -> ShaderRef {
        Self::vertex_shader()
    }

    fn specialize(
        _: &bevy::pbr::MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &bevy::mesh::MeshVertexBufferLayoutRef,
        _: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        // The camera sits inside the dome, and the dome is pushed to the far
        // plane in the vertex shader, so it must neither cull nor occlude.
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = false;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Authored package
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct FogFrame {
    ramp: [f32; 4],
    colour: [f32; 4],
}

#[derive(serde::Deserialize)]
struct Environment {
    anchor_height: f32,
    sun_direction: [f32; 3],
    sun_scale: f32,
    multiplier: f32,
    location_chain: Vec<String>,
    #[serde(default)]
    fog_frame: Option<FogFrame>,
}

#[derive(serde::Deserialize)]
struct Sky {
    width: u32,
    height: u32,
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    #[serde(default)]
    sun_width: u32,
    #[serde(default)]
    sun_height: u32,
    environment: Option<Environment>,
}

/// A validated sky package, decoded off the main thread with the rest of the map.
pub(crate) struct SkyPackage {
    sky: Sky,
    rgba: Vec<u8>,
    sun: Option<Vec<u8>>,
}

impl SkyPackage {
    pub(crate) fn load(root: &std::path::Path, name: &str) -> Result<Self, String> {
        if name.is_empty() || name.contains(['/', '\\', ':']) || matches!(name, "." | "..") {
            return Err("Invalid retail sky name".into());
        }
        let base = root.join("private/native-skies");
        let read = |suffix: &str| {
            std::fs::read(base.join(format!("{name}{suffix}"))).map_err(|e| e.to_string())
        };
        let sky: Sky = serde_json::from_slice(&read(".json")?).map_err(|e| e.to_string())?;
        let rgba = read(".rgba")?;
        if sky.width == 0
            || sky.height == 0
            || u64::from(sky.width) * u64::from(sky.height) * 4 != rgba.len() as u64
            || sky.positions.is_empty()
            || sky.positions.len() != sky.uvs.len()
            || sky.indices.is_empty()
            || sky.indices.len() % 3 != 0
            || sky
                .indices
                .iter()
                .any(|&i| i as usize >= sky.positions.len())
            || sky
                .positions
                .iter()
                .flatten()
                .chain(sky.uvs.iter().flatten())
                .any(|v| !v.is_finite())
        {
            return Err("Invalid retail sky dimensions/geometry".into());
        }
        let sun = if let Some(env) = &sky.environment {
            let direction = Vec3::from_array(env.sun_direction);
            if !env.anchor_height.is_finite()
                || !direction.is_finite()
                || !(0.99..=1.01).contains(&direction.length_squared())
                || !env.sun_scale.is_finite()
                || env.sun_scale <= 0.
                || !env.multiplier.is_finite()
                || env.multiplier < 0.
                || sky.sun_width == 0
                || sky.sun_height != 16
            {
                return Err("Invalid authored sky environment".into());
            }
            if let Some(fog) = &env.fog_frame {
                if !fog.ramp.iter().chain(fog.colour.iter()).all(|v| v.is_finite())
                    || fog.ramp[0] < 0.
                    || fog.ramp[2] <= 0.
                    || !(-1. ..=0.).contains(&fog.colour[3])
                {
                    return Err("Invalid authored fog frame".into());
                }
            }
            let bytes = read(".sun.rgba")?;
            if u64::from(sky.sun_width) * u64::from(sky.sun_height) * 4 != bytes.len() as u64 {
                return Err("Invalid sun-gradient payload".into());
            }
            Some(bytes)
        } else {
            None
        };
        Ok(Self { sky, rgba, sun })
    }

    /// The world-facing half of the package. Legacy packages carry no
    /// environment block, which leaves the world on the defaults.
    pub(crate) fn environment(&self) -> SkyEnvironment {
        let Some(env) = &self.sky.environment else {
            return SkyEnvironment::default();
        };
        let mut environment = SkyEnvironment {
            // Only the tangent-sign terms in the world shader read this vector;
            // it adds no directional light energy and no shadow source.
            sun_direction: Vec3::from_array(env.sun_direction).extend(0.),
            ..default()
        };
        if let Some(fog) = &env.fog_frame {
            environment.fog_ramp = Vec4::from_array(fog.ramp);
            environment.fog_color = Vec4::from_array(fog.colour);
        }
        environment
    }

    pub(crate) fn spawn(
        self,
        commands: &mut SceneCommands,
        meshes: &mut impl AssetSink<Mesh>,
        images: &mut impl AssetSink<Image>,
        materials: &mut impl AssetSink<SkyMaterial>,
    ) {
        // Legacy sky packages retain their original parameters. The scene
        // exposure is the baseline the tone pass divides back out.
        let mut params = SkyParams {
            settings: Vec4::new(165., 2.5, 1., 0.),
            sun_direction: Vec4::ZERO,
        };
        if let Some(env) = &self.sky.environment {
            params.settings = Vec4::new(env.anchor_height, 2.5, env.multiplier, env.sun_scale);
            params.sun_direction = Vec3::from_array(env.sun_direction).extend(0.);
            info!(
                "SKATE_SKY_READY location={:?} anchor={} sun_scale={} multiplier={} fog={} authored_direction={:?}",
                env.location_chain,
                env.anchor_height,
                env.sun_scale,
                env.multiplier,
                env.fog_frame.is_some(),
                env.sun_direction
            );
        } else {
            warn!(
                "Retail sky has legacy metadata; reconvert skies for authored parameters, sun and fog"
            );
        }
        let sun = self
            .sun
            .map(|bytes| image(bytes, self.sky.sun_width, self.sky.sun_height, false))
            .map(|sun| images.add(sun));
        let panorama = images.add(image(self.rgba, self.sky.width, self.sky.height, true));
        let mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.sky.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.sky.uvs)
        .with_inserted_indices(bevy::mesh::Indices::U32(self.sky.indices));
        commands.spawn((
            Name::new("Retail sky dome"),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(SkyMaterial {
                params,
                panorama,
                sun,
            })),
            Transform::default(),
            // The dome follows the camera in the vertex shader, so its authored
            // bounds say nothing about where it will be drawn.
            bevy::camera::visibility::NoFrustumCulling,
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
        ));
    }
}

fn image(rgba: Vec<u8>, width: u32, height: u32, repeat: bool) -> Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        // Squared in the shader, matching the world path's approximate decode.
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    let mut sampler = bevy::image::ImageSamplerDescriptor::linear();
    if repeat {
        // The panorama wraps in azimuth only.
        sampler.address_mode_u = bevy::image::ImageAddressMode::Repeat;
    }
    image.sampler = bevy::image::ImageSampler::Descriptor(sampler);
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(fog: Option<[[f32; 4]; 2]>) -> SkyPackage {
        SkyPackage {
            sky: Sky {
                width: 1,
                height: 1,
                positions: vec![[0.; 3]],
                uvs: vec![[0.; 2]],
                indices: vec![0, 0, 0],
                sun_width: 8,
                sun_height: 16,
                environment: Some(Environment {
                    anchor_height: 165.,
                    sun_direction: [0., 1., 0.],
                    sun_scale: 0.5,
                    multiplier: 1.,
                    location_chain: vec![],
                    fog_frame: fog.map(|[ramp, colour]| FogFrame { ramp, colour }),
                }),
            },
            rgba: vec![0; 4],
            sun: None,
        }
    }

    /// A map with no authored fog must evaluate to *no* fog rather than to a
    /// black ramp: `fog_ramp.z` is an exponent the world shader raises the ramp
    /// to, and zero there would collapse every distance to full fog.
    #[test]
    fn the_default_environment_disables_fog() {
        let default = SkyEnvironment::default();
        assert_eq!(default.fog_ramp, Vec4::new(0., 0., 1., 0.));
        assert_eq!(default.fog_color, Vec4::ZERO);
        assert!((default.sun_direction.xyz().length() - 1.).abs() < 1e-5);
        assert_eq!(default.sun_direction.w, 0.);
        assert_eq!(package(None).environment().fog_ramp, default.fog_ramp);
    }

    #[test]
    fn an_authored_fog_frame_reaches_the_world() {
        let environment = package(Some([[0.001, -0.2, 1.5, 0.], [0.3, 0.4, 0.5, -0.8]]))
            .environment();
        assert_eq!(environment.fog_ramp, Vec4::new(0.001, -0.2, 1.5, 0.));
        assert_eq!(environment.fog_color, Vec4::new(0.3, 0.4, 0.5, -0.8));
        assert_eq!(environment.sun_direction, Vec4::new(0., 1., 0., 0.));
    }
}
