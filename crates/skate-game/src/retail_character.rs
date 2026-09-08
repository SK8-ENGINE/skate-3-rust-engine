//! Character SH/key/rim shader, independent of the world's additive lights.
use crate::retail_irradiance::Irradiance;
use bevy::{
    asset::embedded_asset,
    gltf::GltfMaterialName,
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use serde::Deserialize;
use std::collections::HashMap;

pub(crate) struct CharacterLightingPlugin;
impl Plugin for CharacterLightingPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "retail_character.wgsl");
        embedded_asset!(app, "retail_character_depth.wgsl");
        app.add_plugins(MaterialPlugin::<CharacterMaterial>::default())
            .add_systems(Startup, load)
            .add_systems(Update, (bind, update).chain());
    }
}
#[derive(Deserialize)]
struct MaterialData {
    shader: String,
    params: Vec<[f32; 4]>,
    specular: Option<String>,
}
#[derive(Deserialize)]
struct LightingData {
    materials: HashMap<String, MaterialData>,
    default_sh: [[f32; 3]; 9],
}
#[derive(Resource)]
struct Lighting {
    data: LightingData,
    probes: Irradiance,
    light: Vec4,
}
#[derive(Clone, ShaderType)]
struct CharacterParams {
    light: Vec4,
    tint: Vec4,
    // normal map present, dedicated specular mask, alpha cutoff, hair family
    options: Vec4,
    rows: [Vec4; 9],
    sh: [Vec4; 9],
}
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct CharacterMaterial {
    #[uniform(0)]
    params: CharacterParams,
    #[texture(1)]
    #[sampler(2)]
    diffuse: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    normal: Option<Handle<Image>>,
    #[texture(5)]
    #[sampler(6)]
    mask: Option<Handle<Image>>,
    alpha: AlphaMode,
}
impl Material for CharacterMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://skate3rust/retail_character.wgsl".into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://skate3rust/retail_character_depth.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }
}
fn load(mut commands: Commands, config: Res<crate::config::Config>) {
    let Some(map_path) = &config.map_path else {
        return;
    };
    let path = config.asset_root.join("private/character-lighting.json");
    let data = match std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|b| serde_json::from_slice::<LightingData>(&b).map_err(|e| e.to_string()))
    {
        Ok(data) => data,
        Err(e) => {
            warn!("SKATE_CHARACTER_LIGHTING: {e}");
            return;
        }
    };
    let name = map_path.file_stem().unwrap_or_default().to_string_lossy();
    let overlay = config
        .asset_root
        .join("private/native-lighting")
        .join(format!("{name}.irradiance"));
    let probe_path = if overlay.is_file() {
        overlay
    } else {
        map_path.with_extension("irradiance")
    };
    let probes = match std::fs::read(probe_path)
        .map_err(|e| e.to_string())
        .and_then(|b| Irradiance::parse(&b))
    {
        Ok(probes) => probes,
        Err(e) => {
            warn!("SKATE_CHARACTER_LIGHTING: spatial data unavailable: {e}");
            return;
        }
    };
    let sky_path = config
        .asset_root
        .join("private/native-skies")
        .join(format!("{name}.json"));
    let sky: serde_json::Value = match std::fs::read(sky_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
    {
        Some(sky) => sky,
        None => {
            warn!("SKATE_CHARACTER_LIGHTING: missing authored light direction");
            return;
        }
    };
    let Some(sun) = sky["environment"]["sun_direction"].as_array() else {
        return;
    };
    if sun.len() != 3 {
        return;
    }
    let mut light = Vec4::new(0., 0., 0., 2.5); // Existing retail scene exposure.
    for i in 0..3 {
        let Some(v) = sun[i].as_f64() else {
            return;
        };
        light[i] = v as f32;
    }
    info!("SKATE_CHARACTER_LIGHTING: loaded authored {name} irradiance and character parameters");
    // Shadow visibility only. World shaders do not sample this map; zero lux
    // also prevents additive energy on any remaining StandardMaterials.
    commands.spawn((
        Name::new("Character shadow visibility"),
        DirectionalLight {
            illuminance: 0.,
            shadows_enabled: true,
            ..default()
        },
        Transform::default().looking_to(-light.truncate(), Vec3::Y),
        bevy::light::CascadeShadowConfigBuilder {
            maximum_distance: 100.,
            first_cascade_far_bound: 10.,
            ..default()
        }
        .build(),
    ));
    commands.insert_resource(Lighting {
        data,
        probes,
        light,
    });
}
fn bind(
    mut commands: Commands,
    lighting: Option<Res<Lighting>>,
    source: Res<Assets<StandardMaterial>>,
    server: Res<AssetServer>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
    entities: Query<(Entity, &GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(lighting) = lighting else {
        return;
    };
    for (entity, name, handle) in &entities {
        let Some(data) = lighting.data.materials.get(&name.0) else {
            continue;
        };
        if data.params.len() != 9 {
            continue;
        }
        let Some(m) = source.get(&handle.0) else {
            continue;
        };
        let alpha_cutoff = if let AlphaMode::Mask(cutoff) = m.alpha_mode {
            cutoff
        } else {
            -1.
        };
        let material = materials.add(CharacterMaterial {
            params: CharacterParams {
                light: lighting.light,
                tint: Vec4::from_array(m.base_color.to_linear().to_f32_array()),
                options: Vec4::new(
                    f32::from(m.normal_map_texture.is_some()),
                    f32::from(data.specular.is_some()),
                    alpha_cutoff,
                    f32::from(data.shader == "character.hair"),
                ),
                rows: std::array::from_fn(|i| Vec4::from_array(data.params[i])),
                sh: lighting
                    .data
                    .default_sh
                    .map(|v| Vec3::from_array(v).extend(0.)),
            },
            diffuse: m.base_color_texture.clone(),
            normal: m.normal_map_texture.clone(),
            mask: data.specular.as_ref().map(|path| {
                server.load_with_settings(
                    path.clone(),
                    |settings: &mut bevy::image::ImageLoaderSettings| {
                        settings.is_srgb = false;
                    },
                )
            }),
            alpha: m.alpha_mode,
        });
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(material));
    }
}
fn update(
    lighting: Option<ResMut<Lighting>>,
    root: Query<&Transform, With<crate::world::PlayerRoot>>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
) {
    let (Some(mut lighting), Ok(root)) = (lighting, root.single()) else {
        return;
    };
    let fallback = lighting
        .data
        .default_sh
        .map(|v| Vec3::from_array(v).extend(0.));
    let sh = lighting.probes.sample(root.translation, fallback);
    for (_, material) in materials.iter_mut() {
        material.params.sh = sh;
    }
}
