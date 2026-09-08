//! Character SH/key/rim shader, independent of the world's additive lights.
use crate::retail_irradiance::Irradiance;
use bevy::{
    asset::embedded_asset,
    camera::visibility::RenderLayers,
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
            .add_systems(Update, (bind, shadow_views, update).chain());
    }
}
#[derive(Deserialize)]
struct MaterialData {
    shader: String,
    params: Vec<[f32; 4]>,
    specular: Option<String>,
    coverage: Option<String>,
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
    display_sh: Option<[Vec4; 9]>,
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
    #[texture(7)]
    #[sampler(8)]
    coverage: Option<Handle<Image>>,
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
    spawn_shadow_sources(&mut commands, light.truncate());
    commands.insert_resource(Lighting {
        data,
        probes,
        light,
        display_sh: None,
    });
}
fn spawn_shadow_sources(commands: &mut Commands, light: Vec3) {
    // World + skater casters, sampled only by the character shader.
    commands.spawn((
        Name::new("Character shadow visibility"),
        DirectionalLight {
            illuminance: 0.,
            shadows_enabled: true,
            affects_lightmapped_mesh_diffuse: false,
            ..default()
        },
        Transform::default().looking_to(-light, Vec3::Y),
        bevy::light::CascadeShadowConfigBuilder {
            maximum_distance: 100.,
            first_cascade_far_bound: 10.,
            ..default()
        }
        .build(),
    ));
    // Only skinned player pieces inhabit layer 31. The world receiver samples
    // this separate map so baked building/terrain shadows are not re-applied.
    commands.spawn((
        Name::new("Player shadow onto baked world"),
        DirectionalLight {
            illuminance: 0.,
            shadows_enabled: true,
            affects_lightmapped_mesh_diffuse: true,
            // Receiver-only map: no self-shadow acne to hide with large bias.
            shadow_depth_bias: 0.002,
            shadow_normal_bias: 0.0,
            ..default()
        },
        RenderLayers::layer(31),
        Transform::default().looking_to(-light, Vec3::Y),
        bevy::light::CascadeShadowConfigBuilder {
            num_cascades: 1,
            maximum_distance: 24.,
            ..default()
        }
        .build(),
    ));
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
                    if data.coverage.is_some() { -1. } else { alpha_cutoff },
                    if data.coverage.is_some() { 2. } else { f32::from(data.shader == "character.hair") },
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
            coverage: data.coverage.as_ref().map(|path| server.load_with_settings(
                path.clone(), |settings: &mut bevy::image::ImageLoaderSettings| { settings.is_srgb = false; })),
            alpha: if data.coverage.is_some() { AlphaMode::Blend } else { m.alpha_mode },
        });
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert((
                MeshMaterial3d(material),
                RenderLayers::from_layers(&[0, 31]),
            ));
    }
}
fn shadow_views(
    mut commands: Commands,
    lighting: Option<Res<Lighting>>,
    cameras: Query<(Entity, Option<&RenderLayers>), With<Camera3d>>,
) {
    if lighting.is_none() {
        return;
    }
    for (entity, layers) in &cameras {
        let layers = layers.cloned().unwrap_or_default();
        if !layers.intersects(&RenderLayers::layer(31)) {
            commands.entity(entity).insert(layers.with(31));
        }
    }
}
fn update(
    lighting: Option<ResMut<Lighting>>,
    root: Query<&Transform, With<crate::world::PlayerRoot>>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
    mut shadow: ResMut<crate::retail_render::ShadowState>,
    time: Res<Time>,
) {
    let (Some(mut lighting), Ok(root)) = (lighting, root.single()) else {
        return;
    };
    let fallback = lighting
        .data
        .default_sh
        .map(|v| Vec3::from_array(v).extend(0.));
    let sh = lighting.probes.sample(root.translation, fallback);
    let displayed = lighting.display_sh.map_or(sh, |old| {
        let weight = 1. - (-time.delta_secs().clamp(0., 0.05) / 0.35).exp();
        std::array::from_fn(|i| old[i].lerp(sh[i], weight))
    });
    for (_, material) in materials.iter_mut() {
        material.params.sh = displayed;
    }
    lighting.display_sh = Some(displayed);
    // Adapter floor: the local probe's direction-independent ambient term.
    // The native per-frame c8 shadow-colour controller remains unrecovered.
    shadow.approach(sh[0].truncate(), time.delta_secs());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_receiver_source_excludes_world_casters_and_emits_no_light() {
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        spawn_shadow_sources(&mut Commands::new(&mut queue, &world), Vec3::Y);
        queue.apply(&mut world);
        let player = RenderLayers::from_layers(&[0, 31]);
        let terrain = RenderLayers::default();
        let mut receivers = 0;
        let mut character_sources = 0;
        for (light, layers, cascades) in world
            .query::<(&DirectionalLight, Option<&RenderLayers>, &bevy::light::CascadeShadowConfig)>()
            .iter(&world)
        {
            let layers = layers.cloned().unwrap_or_default();
            assert_eq!(light.illuminance, 0.);
            assert!(light.shadows_enabled);
            assert!(layers.intersects(&player));
            if light.affects_lightmapped_mesh_diffuse {
                assert!(!layers.intersects(&terrain));
                assert_eq!(cascades.bounds, vec![24.]);
                assert_eq!(light.shadow_normal_bias, 0.);
                assert!(light.shadow_depth_bias < 0.005);
                receivers += 1;
            } else {
                assert!(layers.intersects(&terrain));
                character_sources += 1;
            }
        }
        assert_eq!((receivers, character_sources), (1, 1));
    }
}
