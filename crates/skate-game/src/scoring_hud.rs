//! Original APT HUD rendered independently of the world's resolution scale.
use crate::{apt_scene, config::Config, hud_runtime, physics::SkaterRuntime};
use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, PrimitiveTopology, ShaderType, TextureDimension, TextureFormat,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin, MeshMaterial2d},
};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Copy, Debug, ShaderType)]
struct ColorTransform {
    multiply: Vec4,
    add: Vec4,
}
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct HudMaterial {
    #[uniform(0)]
    color: ColorTransform,
    #[texture(1)]
    #[sampler(2)]
    atlas: Handle<Image>,
}
impl Material2d for HudMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://skate3rust/hud_render.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}
struct Slot {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<HudMaterial>,
}
#[derive(Resource)]
struct Hud {
    runtime: hud_runtime::Runtime,
    source: serde_json::Value,
    shapes: apt_scene::Shapes,
    textures: BTreeMap<String, Handle<Image>>,
    slots: Vec<Slot>,
    generation: u64,
    failed: bool,
}
pub(crate) struct ScoringHudPlugin;
impl Plugin for ScoringHudPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "hud_render.wgsl");
        app.add_plugins(Material2dPlugin::<HudMaterial>::default())
            .add_systems(
                PostStartup,
                setup.after(crate::graphics_menu::PresentationSetup),
            )
            .add_systems(
                FixedUpdate,
                advance
                    .after(crate::app::SimulationSet::Physics)
                    .run_if(crate::graphics_menu::gameplay_active),
            )
            .add_systems(
                Update,
                (reset, render).chain().after(crate::app::FrameSet::Physics),
            );
    }
}
fn setup(
    mut commands: Commands,
    config: Res<Config>,
    skater: Res<SkaterRuntime>,
    cameras: Query<Entity, With<IsDefaultUiCamera>>,
    mut images: ResMut<Assets<Image>>,
) {
    // The startup dependency also applies the presentation system's deferred
    // camera spawn before this query. Without it, setup silently lost the HUD.
    let Ok(output) = cameras.single() else {
        error!("Original HUD requires the presentation camera");
        return;
    };
    let root = std::env::var_os("SKATE_SCORING_HUD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| config.asset_root.join("private/hud"));
    let result = (|| -> Result<Hud, String> {
        let source: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("runtime/trickdisplay.json"))
                .map_err(|e| format!("{}: {e}", root.display()))?,
        )
        .map_err(|e| e.to_string())?;
        let runtime = hud_runtime::Runtime::load(&source, skater.scoring.hud_input())?;
        let shapes: apt_scene::Shapes =
            serde_json::from_value(source["shapes"].clone()).map_err(|e| e.to_string())?;
        let mut files = BTreeMap::new();
        for shape in shapes.values().flatten() {
            files.insert(
                shape.texture.rgba.clone(),
                [shape.texture.width, shape.texture.height],
            );
        }
        for font in runtime.bindings.movie.text_assets.fonts.values() {
            files.insert(font.texture.clone(), font.size);
        }
        let mut textures = BTreeMap::new();
        for (path, size) in files {
            let bytes = std::fs::read(root.join(&path)).map_err(|e| format!("HUD {path}: {e}"))?;
            if bytes.len() != size[0] as usize * size[1] as usize * 4 {
                return Err(format!("Invalid HUD texture size {path}"));
            }
            let image = Image::new(
                Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                bytes,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            textures.insert(path, images.add(image));
        }
        Ok(Hud {
            runtime,
            source,
            shapes,
            textures,
            slots: Vec::new(),
            generation: 0,
            failed: false,
        })
    })();
    match result {
        Ok(hud) => {
            let target = images.add(Image::new_target_texture(
                1280,
                720,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
            commands.spawn((
                Camera2d,
                Camera {
                    order: -1,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(target.clone().into()),
                RenderLayers::layer(31),
                Msaa::Off,
            ));
            commands.spawn((
                ImageNode::new(target),
                UiTargetCamera(output),
                GlobalZIndex(1),
                Pickable::IGNORE,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
            ));
            commands.insert_resource(hud);
            info!("Original scoring HUD loaded");
        }
        Err(error) => error!("Original scoring HUD could not load: {error}"),
    }
}
fn reset(
    hud: Option<ResMut<Hud>>,
    skater: Res<SkaterRuntime>,
    map: Res<crate::map_transition::CurrentMap>,
) {
    let Some(mut hud) = hud else {
        return;
    };
    if hud.generation == map.generation {
        return;
    }
    match hud_runtime::Runtime::load(&hud.source, skater.scoring.hud_input()) {
        Ok(runtime) => {
            hud.runtime = runtime;
            hud.generation = map.generation;
            hud.failed = false;
        }
        Err(error) => {
            error!("Original scoring HUD reset: {error}");
            hud.failed = true;
        }
    }
}
fn advance(
    hud: Option<ResMut<Hud>>,
    skater: Res<SkaterRuntime>,
    map: Res<crate::map_transition::CurrentMap>,
) {
    let Some(mut hud) = hud else {
        return;
    };
    if hud.failed || hud.generation != map.generation {
        return;
    }
    if let Err(error) = hud.runtime.update(
        skater.scoring.hud_input(),
        skater.scoring.new_trick,
        skater.scoring.modified_trick,
        skater.scoring.close_tricks,
    ) {
        error!("Original scoring HUD stopped: {error}");
        hud.failed = true;
    }
}
fn render(
    mut commands: Commands,
    hud: Option<ResMut<Hud>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HudMaterial>>,
) {
    let Some(mut hud) = hud else {
        return;
    };
    let draws = if hud.failed {
        Vec::new()
    } else {
        match apt_scene::draw(&hud.runtime.bindings.movie, &hud.runtime.vm, &hud.shapes) {
            Ok(draws) => draws,
            Err(error) => {
                error!("Original HUD geometry: {error}");
                hud.failed = true;
                Vec::new()
            }
        }
    };
    for (index, draw) in draws.iter().enumerate() {
        let Some(texture) = hud.textures.get(&draw.texture).cloned() else {
            continue;
        };
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            draw.vertices
                .iter()
                .map(|v| [v.position[0] - 640., 360. - v.position[1], 0.])
                .collect::<Vec<_>>(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            draw.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            vec![[0., 0., 1.]; draw.vertices.len()],
        );
        let material = HudMaterial {
            color: ColorTransform {
                multiply: draw.multiply.into(),
                add: draw.add.into(),
            },
            atlas: texture,
        };
        if index == hud.slots.len() {
            let mesh = meshes.add(mesh);
            let material = materials.add(material);
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., index as f32 * 0.01),
                    RenderLayers::layer(31),
                ))
                .id();
            hud.slots.push(Slot {
                entity,
                mesh,
                material,
            });
        } else {
            let slot = &hud.slots[index];
            if let Some(old) = meshes.get_mut(&slot.mesh) {
                *old = mesh;
            }
            if let Some(old) = materials.get_mut(&slot.material) {
                *old = material;
            }
            commands.entity(slot.entity).insert(Visibility::Visible);
        }
    }
    for slot in &hud.slots[draws.len()..] {
        commands.entity(slot.entity).insert(Visibility::Hidden);
    }
}
