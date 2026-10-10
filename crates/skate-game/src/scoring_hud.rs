//! Original APT HUD rendered independently of the world's resolution scale.
use crate::{apt_scene, config::Config, hud_runtime, physics::SkaterRuntime};
use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendState, Extent3d, PrimitiveTopology, RenderPipelineDescriptor, ShaderType,
        TextureDimension, TextureFormat,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin, MeshMaterial2d},
    ui_render::UiMaterialPlugin,
    window::PrimaryWindow,
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
/// The offscreen target already contains RGB multiplied by coverage. Applying
/// ImageNode's straight-alpha blend again suppresses the original soft glow.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct HudComposite {
    #[texture(0)]
    #[sampler(1)]
    image: Handle<Image>,
}
impl UiMaterial for HudComposite {
    fn fragment_shader() -> ShaderRef {
        "embedded://skate3rust/hud_composite.wgsl".into()
    }
    fn specialize(descriptor: &mut RenderPipelineDescriptor, _: UiMaterialKey<Self>) {
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
            }
        }
    }
}
struct Slot {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<HudMaterial>,
    /// The `Visibility` this module last inserted; `None` until the first insert (the spawn
    /// leaves the required-component default, `Inherited`).
    visibility: Option<Visibility>,
}
#[derive(Resource)]
struct Hud {
    target: Handle<Image>,
    composite: Handle<HudComposite>,
    rebind_after_resize: bool,
    runtime: hud_runtime::Runtime,
    source: serde_json::Value,
    shapes: apt_scene::Shapes,
    textures: BTreeMap<String, Handle<Image>>,
    slots: Vec<Slot>,
    generation: u64,
    failed: bool,
}
/// Same string the scoring HUD draws: language table first, then a readable fallback.
pub(crate) fn display_trick(world: &World, label: &str) -> String {
    if label.is_empty() {
        return String::new();
    }
    localize_trick(
        label,
        world
            .get_resource::<Hud>()
            .map(|hud| &hud.runtime.bindings.movie.text_assets),
    )
}

pub(crate) use crate::apt_text::localize_trick;


pub(crate) struct ScoringHudPlugin;
impl Plugin for ScoringHudPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "hud_render.wgsl");
        embedded_asset!(app, "hud_composite.wgsl");
        app.add_plugins((
            Material2dPlugin::<HudMaterial>::default(),
            UiMaterialPlugin::<HudComposite>::default(),
        ))
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
            (resize_target, reset, render)
                .chain()
                .after(crate::app::FrameSet::Physics),
        );
    }
}
fn setup(
    mut commands: Commands,
    config: Res<Config>,
    skater: Res<SkaterRuntime>,
    cameras: Query<Entity, With<IsDefaultUiCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut composites: ResMut<Assets<HudComposite>>,
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
        for texture in shapes.values().flatten().filter_map(|s| s.texture.as_ref()) {
            files.insert(texture.rgba.clone(), [texture.width, texture.height]);
        }
        for font in runtime.bindings.movie.text_assets.fonts.values() {
            files.insert(font.texture.clone(), font.size);
            if let Some(fg) = &font.foreground {
                files.insert(fg.texture.clone(), fg.size);
            }
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
            target: Handle::default(),
            composite: Handle::default(),
            rebind_after_resize: false,
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
        Ok(mut hud) => {
            let target = images.add(Image::new_target_texture(
                window.physical_width().max(1),
                window.physical_height().max(1),
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
            hud.target = target.clone();
            commands.spawn((
                Camera2d,
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed {
                        width: 1280.,
                        height: 720.,
                    },
                    ..OrthographicProjection::default_2d()
                }),
                Camera {
                    order: -1,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(target.clone().into()),
                RenderLayers::layer(31),
                Msaa::Off,
            ));
            hud.composite = composites.add(HudComposite { image: target });
            commands.spawn((
                MaterialNode(hud.composite.clone()),
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
            info!("Original scoring HUD loaded from {}", root.display());
        }
        Err(error) => error!(
            "Original scoring HUD could not load from {}: {error}. See docs/hud-installation.md",
            root.display()
        ),
    }
}
// Rasterize at output pixel resolution; retain the original 1280x720 APT
// coordinate space. This avoids a second enlargement of every glyph/glow.
fn resize_target(
    hud: Option<ResMut<Hud>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut composites: ResMut<Assets<HudComposite>>,
) {
    let Some(mut hud) = hud else {
        return;
    };
    // Bevy 0.18's UI material preparation has no dependency on GpuImage
    // preparation. Retry once on the following frame, when the replacement
    // image is available regardless of preparation order during the resize.
    if hud.rebind_after_resize {
        if let Some(material) = composites.get_mut(&hud.composite) {
            material.image = hud.target.clone();
        }
    }
    let size = Extent3d {
        width: window.physical_width().max(1),
        height: window.physical_height().max(1),
        depth_or_array_layers: 1,
    };
    hud.rebind_after_resize = resize_image(
        &mut images,
        &mut composites,
        &hud.target,
        &hud.composite,
        size,
    );
}
fn resize_image(
    images: &mut Assets<Image>,
    composites: &mut Assets<HudComposite>,
    target: &Handle<Image>,
    composite: &Handle<HudComposite>,
    size: Extent3d,
) -> bool {
    if images
        .get(target)
        .is_some_and(|image| image.texture_descriptor.size != size)
    {
        // Assets::get_mut emits Modified even without a write. Doing that
        // every frame recreates the GPU target behind the compositor's
        // cached bind group. Only invalidate the image on a real resize.
        if let Some(image) = images.get_mut(target) {
            image.resize(size);
        }
        // The UI material retains its bind group. A real target resize must
        // reprepare it so it samples the replacement GPU texture view.
        if let Some(material) = composites.get_mut(composite) {
            material.image = target.clone();
        }
        return true;
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_target_changes_only_on_resize_and_refreshes_composite() {
        // Asset scheduling only: no render plugin, window or gameplay systems.
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<HudComposite>();
        let target =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    1280,
                    720,
                    TextureFormat::Rgba8UnormSrgb,
                    None,
                ));
        let composite = app
            .world_mut()
            .resource_mut::<Assets<HudComposite>>()
            .add(HudComposite {
                image: target.clone(),
            });
        app.update();
        app.world_mut()
            .resource_mut::<Messages<AssetEvent<Image>>>()
            .clear();
        app.world_mut()
            .resource_mut::<Messages<AssetEvent<HudComposite>>>()
            .clear();
        for (width, height, changed) in
            [(1280, 720, false), (1920, 1080, true), (1920, 1080, false)]
        {
            app.world_mut()
                .resource_scope(|world, mut images: Mut<Assets<Image>>| {
                    let mut composites = world.resource_mut::<Assets<HudComposite>>();
                    resize_image(
                        &mut images,
                        &mut composites,
                        &target,
                        &composite,
                        Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                    );
                });
            app.update();
            let images: Vec<_> = app
                .world_mut()
                .resource_mut::<Messages<AssetEvent<Image>>>()
                .drain()
                .collect();
            let materials: Vec<_> = app
                .world_mut()
                .resource_mut::<Messages<AssetEvent<HudComposite>>>()
                .drain()
                .collect();
            assert_eq!(
                images
                    .iter()
                    .any(|e| matches!(e, AssetEvent::Modified { id } if *id == target.id())),
                changed
            );
            assert_eq!(
                materials
                    .iter()
                    .any(|e| matches!(e, AssetEvent::Modified { id } if *id == composite.id())),
                changed
            );
        }
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
    let started = std::time::Instant::now();
    let updated = hud.runtime.update(
        skater.scoring.hud_input(),
        skater.scoring.new_trick,
        skater.scoring.modified_trick,
        skater.scoring.close_tricks,
    );
    crate::frame_timing::hitch::add_phase(crate::frame_timing::hitch::PHASE_HUD_ADVANCE, started);
    if let Err(error) = updated {
        error!("Original scoring HUD stopped: {error}");
        hud.failed = true;
    }
}
fn render(
    commands: Commands,
    hud: Option<ResMut<Hud>>,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<HudMaterial>>,
) {
    let started = std::time::Instant::now();
    render_draws(commands, hud, meshes, materials);
    crate::frame_timing::hitch::add_phase(crate::frame_timing::hitch::PHASE_HUD_RENDER, started);
}
fn render_draws(
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
    let hud = &mut *hud;
    apply_draws(
        &mut commands,
        &mut hud.slots,
        &hud.textures,
        &draws,
        &mut meshes,
        &mut materials,
    );
}
fn hud_position(v: &apt_scene::Vertex) -> [f32; 3] {
    [v.position[0] - 640., 360. - v.position[1], 0.]
}
fn hud_mesh(draw: &apt_scene::Draw) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        draw.vertices.iter().map(hud_position).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        draw.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vec![[0., 0., 1.]; draw.vertices.len()],
    );
    mesh
}
fn hud_material(draw: &apt_scene::Draw, atlas: Handle<Image>) -> HudMaterial {
    HudMaterial {
        color: ColorTransform {
            multiply: draw.multiply.into(),
            add: draw.add.into(),
        },
        atlas,
    }
}
fn bits<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
    a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits())
}
/// True when `mesh` already holds exactly what [`hud_mesh`] would build for `draw` (bit for bit),
/// so replacing it would change nothing but still emit `AssetEvent::Modified`.
fn mesh_matches(mesh: &Mesh, draw: &apt_scene::Draw) -> bool {
    use bevy::mesh::VertexAttributeValues as V;
    let n = draw.vertices.len();
    let (Some(V::Float32x3(positions)), Some(V::Float32x2(uvs)), Some(V::Float32x3(normals))) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_UV_0),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
    ) else {
        return false;
    };
    mesh.primitive_topology() == PrimitiveTopology::TriangleList
        && mesh.indices().is_none()
        && mesh.attributes().count() == 3
        && positions.len() == n
        && uvs.len() == n
        && normals.len() == n
        && normals.iter().all(|&v| bits(v, [0., 0., 1.]))
        && draw
            .vertices
            .iter()
            .zip(positions.iter().zip(uvs))
            .all(|(v, (&p, &uv))| bits(p, hud_position(v)) && bits(uv, v.uv))
}
fn material_matches(old: &HudMaterial, new: &HudMaterial) -> bool {
    bits(old.color.multiply.to_array(), new.color.multiply.to_array())
        && bits(old.color.add.to_array(), new.color.add.to_array())
        && old.atlas == new.atlas
}
/// Write this frame's draws into the retained slots. A slot's mesh, material and visibility are
/// written only when they differ from what it already holds: `Assets::get_mut` marks the asset
/// modified even without a write, which re-extracts the mesh, re-uploads its vertices and rebuilds
/// the material's bind group in the render world, every frame, for every glyph on screen. The
/// resulting assets and components are bit-identical to replacing them every frame.
fn apply_draws(
    commands: &mut Commands,
    slots: &mut Vec<Slot>,
    textures: &BTreeMap<String, Handle<Image>>,
    draws: &[apt_scene::Draw],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<HudMaterial>,
) {
    for (index, draw) in draws.iter().enumerate() {
        let Some(texture) = textures.get(&draw.texture).cloned() else {
            continue;
        };
        let material = hud_material(draw, texture);
        if index == slots.len() {
            let mesh = meshes.add(hud_mesh(draw));
            let material = materials.add(material);
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., index as f32 * 0.01),
                    RenderLayers::layer(31),
                ))
                .id();
            slots.push(Slot {
                entity,
                mesh,
                material,
                visibility: None,
            });
        } else {
            let slot = &mut slots[index];
            if !meshes
                .get(&slot.mesh)
                .is_some_and(|m| mesh_matches(m, draw))
            {
                if let Some(old) = meshes.get_mut(&slot.mesh) {
                    *old = hud_mesh(draw);
                }
            }
            if !materials
                .get(&slot.material)
                .is_some_and(|m| material_matches(m, &material))
            {
                if let Some(old) = materials.get_mut(&slot.material) {
                    *old = material;
                }
            }
            set_visibility(commands, slot, Visibility::Visible);
        }
    }
    for slot in &mut slots[draws.len()..] {
        set_visibility(commands, slot, Visibility::Hidden);
    }
}
fn set_visibility(commands: &mut Commands, slot: &mut Slot, visibility: Visibility) {
    if slot.visibility != Some(visibility) {
        commands.entity(slot.entity).insert(visibility);
        slot.visibility = Some(visibility);
    }
}
#[cfg(test)]
mod draw_tests {
    //! The change-aware HUD writer against the writer it replaced (kept below as the reference),
    //! over a scripted trick sequence through the real scoring runtime and HUD movie.
    use super::*;
    use crate::scoring_runtime::{Frame, Runtime as Scoring};
    use skate_core::physics::filtered_state::FilteredCategory;
    use std::time::{Duration, Instant};

    /// The writer before 2026-10-08, verbatim: replaces every slot's mesh and material and
    /// inserts every slot's visibility each frame.
    fn apply_draws_reference(
        commands: &mut Commands,
        slots: &mut Vec<Slot>,
        textures: &BTreeMap<String, Handle<Image>>,
        draws: &[apt_scene::Draw],
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<HudMaterial>,
    ) {
        for (index, draw) in draws.iter().enumerate() {
            let Some(texture) = textures.get(&draw.texture).cloned() else {
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
            if index == slots.len() {
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
                slots.push(Slot {
                    entity,
                    mesh,
                    material,
                    visibility: None,
                });
            } else {
                let slot = &slots[index];
                if let Some(old) = meshes.get_mut(&slot.mesh) {
                    *old = mesh;
                }
                if let Some(old) = materials.get_mut(&slot.material) {
                    *old = material;
                }
                commands.entity(slot.entity).insert(Visibility::Visible);
            }
        }
        for slot in &slots[draws.len()..] {
            commands.entity(slot.entity).insert(Visibility::Hidden);
        }
    }

    #[derive(Resource)]
    struct Rig {
        reference: bool,
        slots: Vec<Slot>,
        textures: BTreeMap<String, Handle<Image>>,
        draws: Vec<apt_scene::Draw>,
        took: Duration,
    }
    fn rig_system(
        mut commands: Commands,
        mut rig: ResMut<Rig>,
        mut meshes: ResMut<Assets<Mesh>>,
        mut materials: ResMut<Assets<HudMaterial>>,
    ) {
        let rig = &mut *rig;
        let draws = std::mem::take(&mut rig.draws);
        let started = Instant::now();
        if rig.reference {
            apply_draws_reference(
                &mut commands,
                &mut rig.slots,
                &rig.textures,
                &draws,
                &mut meshes,
                &mut materials,
            );
        } else {
            apply_draws(
                &mut commands,
                &mut rig.slots,
                &rig.textures,
                &draws,
                &mut meshes,
                &mut materials,
            );
        }
        rig.took = started.elapsed();
    }
    fn rig(reference: bool, texture_paths: &[String]) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<Mesh>()
            .init_asset::<HudMaterial>();
        let textures = texture_paths
            .iter()
            .map(|p| {
                let handle = app
                    .world_mut()
                    .resource_mut::<Assets<Image>>()
                    .add(Image::default());
                (p.clone(), handle)
            })
            .collect();
        app.insert_resource(Rig {
            reference,
            slots: Vec::new(),
            textures,
            draws: Vec::new(),
            took: Duration::ZERO,
        })
        .add_systems(Update, rig_system);
        app
    }
    /// Everything the render world can see of the HUD: per slot the entity, its components and
    /// the bits of its mesh and material.
    fn snapshot(app: &App) -> Vec<String> {
        let world = app.world();
        let rig = world.resource::<Rig>();
        let meshes = world.resource::<Assets<Mesh>>();
        let materials = world.resource::<Assets<HudMaterial>>();
        let atlas_name = |h: &Handle<Image>| {
            rig.textures
                .iter()
                .find(|(_, t)| *t == h)
                .map(|(p, _)| p.clone())
        };
        rig.slots
            .iter()
            .map(|slot| {
                let e = world.entity(slot.entity);
                let mesh = meshes.get(&slot.mesh).unwrap();
                let attrs: Vec<_> = mesh
                    .attributes()
                    .map(|(a, v)| (format!("{:?}", a.id), v.get_bytes().to_vec()))
                    .collect();
                let m = materials.get(&slot.material).unwrap();
                let color: Vec<u32> = m
                    .color
                    .multiply
                    .to_array()
                    .iter()
                    .chain(&m.color.add.to_array())
                    .map(|f| f.to_bits())
                    .collect();
                format!(
                    "{:?} vis={:?} z={} layers={:?} mesh2d={:?} mat2d={:?} topo={:?} idx={} usage={:?} attrs={attrs:?} color={color:?} atlas={:?}",
                    slot.entity,
                    e.get::<Visibility>(),
                    e.get::<Transform>().unwrap().translation.z.to_bits(),
                    e.get::<RenderLayers>(),
                    e.get::<Mesh2d>().map(|m| m.0.id() == slot.mesh.id()),
                    e.get::<MeshMaterial2d<HudMaterial>>().map(|m| m.0.id() == slot.material.id()),
                    mesh.primitive_topology(),
                    mesh.indices().is_some(),
                    mesh.asset_usage,
                    atlas_name(&m.atlas),
                )
            })
            .collect()
    }
    /// (mesh Modified, material Modified) events since the last call.
    fn modified(app: &mut App) -> (usize, usize) {
        let m = app
            .world_mut()
            .resource_mut::<Messages<AssetEvent<Mesh>>>()
            .drain()
            .filter(|e| matches!(e, AssetEvent::Modified { .. }))
            .count();
        let mat = app
            .world_mut()
            .resource_mut::<Messages<AssetEvent<HudMaterial>>>()
            .drain()
            .filter(|e| matches!(e, AssetEvent::Modified { .. }))
            .count();
        (m, mat)
    }
    fn frame(
        tick: u32,
        category: FilteredCategory,
        descriptor: Option<skate_core::animation::output::attributes::AttributeName>,
    ) -> Frame {
        let identity = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        Frame {
            tick,
            dt: 1. / 60.,
            category,
            state: if category == FilteredCategory::Air {
                200
            } else {
                100
            },
            descriptor,
            grind_id: -1,
            flags: 0,
            position: [0., 1., tick as f32 / 60.],
            velocity: [0., 0., 1.],
            forward: [0., 0., 1.],
            switch: false,
            fakie: false,
            regular: true,
            player_basis: identity,
            board_basis: identity,
            reckoning_up: [0., 1., 0.],
            body_flip: false,
            front_flip: false,
            suspend_air: false,
            landing: Default::default(),
            teleported: false,
            reverting: false,
        }
    }
    fn pct(sorted: &[f64], p: f64) -> f64 {
        sorted[((sorted.len() - 1) as f64 * p).round() as usize]
    }

    /// Scripted large airs with three flips each, a multiplier and a bail, the HUD movie driven
    /// by the real scoring runtime. Asserts that the new writer leaves every slot (entity
    /// components, mesh bytes, material bits) identical to the old writer after every frame, and
    /// prints the asset churn and writer cost of both.
    /// `SKATE3_ASSET_ROOT=<assets> cargo test --release --bin skate3rust -- --ignored --nocapture hud_writer`
    #[test]
    #[ignore = "requires private authored scoring and HUD data via SKATE3_ASSET_ROOT"]
    fn hud_writer_is_identical_and_skips_unchanged_assets() {
        let root = PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("asset root"));
        let mut scoring =
            Scoring::load(&skate_data::collections::Collections::load(&root).unwrap()).unwrap();
        let source: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("private/hud/runtime/trickdisplay.json")).unwrap(),
        )
        .unwrap();
        let mut hud = hud_runtime::Runtime::load(&source, scoring.hud_input()).unwrap();
        let shapes: apt_scene::Shapes = serde_json::from_value(source["shapes"].clone()).unwrap();
        let mut paths: Vec<String> = shapes
            .values()
            .flatten()
            .filter_map(|s| s.texture.as_ref().map(|t| t.rgba.clone()))
            .collect();
        paths.extend(
            hud.bindings
                .movie
                .text_assets
                .fonts
                .values()
                .map(|f| f.texture.clone()),
        );
        paths.sort();
        paths.dedup();
        let tricks: Vec<_> = [
            "kickflip",
            "heelflip",
            "hardflip",
            "varial_kickflip",
            "360_flip",
            "tre_flip",
            "melon",
            "n_kickflip",
            "tailgrab_airwalk",
        ]
        .iter()
        .filter_map(|id| {
            scoring
                .data
                .definitions
                .iter()
                .find(|d| d.identifier == *id)
                .map(|d| d.encoded_name)
        })
        .collect();
        assert!(tricks.len() >= 3, "trick fixtures missing");
        let mut new = rig(false, &paths);
        let mut old = rig(true, &paths);
        let (mut tick, mut frames, mut max_draws) = (0u32, 0usize, 0usize);
        let (mut churn_old, mut churn_new) = ([0usize; 2], [0usize; 2]);
        let (mut worst_old, mut worst_new) = (0usize, 0usize);
        let (mut cost_old, mut cost_new, mut cost_scene, mut cost_advance) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for attempt in 0..10usize {
            if attempt == 1 {
                scoring.session.combo.multiplier = 3.0;
                scoring.session.combo.timer.points = scoring.data.combo_capacity;
                scoring.session.line.points = scoring.data.line_capacity;
            }
            // 2.5 s of air with three flips in a row, then 1.5 s on the ground; attempt 6 bails.
            for offset in 0..240usize {
                let category = if offset < 150 {
                    FilteredCategory::Air
                } else if attempt == 6 && offset < 170 {
                    FilteredCategory::Wipeout
                } else {
                    FilteredCategory::Ground
                };
                let descriptor =
                    (offset < 150).then(|| tricks[(attempt + offset / 50) % tricks.len()]);
                tick += 1;
                scoring.advance(frame(tick, category, descriptor)).unwrap();
                let started = Instant::now();
                hud.update(
                    scoring.hud_input(),
                    scoring.new_trick,
                    scoring.modified_trick,
                    scoring.close_tricks,
                )
                .unwrap();
                cost_advance.push(started.elapsed().as_secs_f64() * 1e6);
                let started = Instant::now();
                let draws = apt_scene::draw(&hud.bindings.movie, &hud.vm, &shapes).unwrap();
                cost_scene.push(started.elapsed().as_secs_f64() * 1e6);
                max_draws = max_draws.max(draws.len());
                new.world_mut().resource_mut::<Rig>().draws = draws;
                old.world_mut().resource_mut::<Rig>().draws =
                    apt_scene::draw(&hud.bindings.movie, &hud.vm, &shapes).unwrap();
                new.update();
                old.update();
                frames += 1;
                let (nm, nmat) = modified(&mut new);
                let (om, omat) = modified(&mut old);
                churn_new[0] += nm;
                churn_new[1] += nmat;
                churn_old[0] += om;
                churn_old[1] += omat;
                worst_new = worst_new.max(nm + nmat);
                worst_old = worst_old.max(om + omat);
                cost_new.push(new.world().resource::<Rig>().took.as_secs_f64() * 1e6);
                cost_old.push(old.world().resource::<Rig>().took.as_secs_f64() * 1e6);
                let (a, b) = (snapshot(&new), snapshot(&old));
                assert_eq!(a.len(), b.len(), "slot count differs at frame {frames}");
                for (i, (x, y)) in a.iter().zip(&b).enumerate() {
                    assert_eq!(x, y, "slot {i} differs at frame {frames}");
                }
            }
        }
        for v in [
            &mut cost_old,
            &mut cost_new,
            &mut cost_scene,
            &mut cost_advance,
        ] {
            v.sort_by(f64::total_cmp);
        }
        let line = |name: &str, v: &[f64]| {
            eprintln!(
                "{name}: p50 {:.1} us, p99 {:.1} us, max {:.1} us",
                pct(v, 0.5),
                pct(v, 0.99),
                v[v.len() - 1]
            )
        };
        eprintln!(
            "frames {frames}, max draws {max_draws}, slots {}",
            new.world().resource::<Rig>().slots.len()
        );
        eprintln!(
            "Modified per frame: old mesh {:.1} material {:.1} (worst frame {worst_old}), new mesh {:.1} material {:.1} (worst frame {worst_new})",
            churn_old[0] as f64 / frames as f64,
            churn_old[1] as f64 / frames as f64,
            churn_new[0] as f64 / frames as f64,
            churn_new[1] as f64 / frames as f64,
        );
        line("hud_runtime::update (hud_advance)", &cost_advance);
        line("apt_scene::draw", &cost_scene);
        line("writer old", &cost_old);
        line("writer new", &cost_new);
        assert!(churn_new[0] + churn_new[1] < churn_old[0] + churn_old[1]);
    }
}
