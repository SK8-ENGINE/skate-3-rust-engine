//! Original APT HUD rendered independently of the world's resolution scale.
use crate::{apt_scene, config::Config, hud_central, hud_minimap, hud_runtime, physics::SkaterRuntime};
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
pub(crate) struct ColorTransform {
    multiply: Vec4,
    add: Vec4,
    /// x = 1: apply the transform in gamma space (hud_render.wgsl; the retail menus and the HUD).
    mode: Vec4,
}
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct HudMaterial {
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
pub(crate) struct Slot {
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
    /// The central message HUD (`hud2/hudcentral`), drawn above the trick display; `None` in an install from before
    /// it was converted (Refresh Game Files adds it).
    central: Option<Central>,
    /// The minimap frame (`hud2/hudminimap`); `None` in an install from before it was converted.
    minimap: Option<MiniMap>,
}
/// The minimap frame movie and its draw slots.
struct MiniMap {
    runtime: hud_minimap::Runtime,
    shapes: apt_scene::Shapes,
    slots: Vec<Slot>,
    failed: bool,
}
/// Below the central messages, above the trick display.
const MINIMAP_Z: f32 = 29.0;
/// The north needle retail draws over the minimap (82665848, `north_pointer.Texture` of icons/minimap, 16 x 128), its
/// texture key here and its centre: the movie's needle clip `mNorth` (b568). The texture lives in the menu movie set.
const NEEDLE_TEXTURE: &str = "minimap:north_pointer";
/// The minimap icon pack's textures the HUD loads (menu set `texture/<name>`), keyed `minimap:<name>`.
const MINIMAP_TEXTURES: [&str; 2] = ["north_pointer", "mini_skater"];
/// The pointer quad in map units (+-16 x +-100, 0x8232F9C0..0x8232F9FF; b571), scaled like the icons.
const NEEDLE_UNITS: [f32; 2] = [32.0, 200.0];

/// The needle quad turned degrees clockwise on screen.
fn needle_draw(degrees: f32) -> apt_scene::Draw {
    // Map units turned, then the map matrix's scale and centre (skate_core::minimap::IconRules, traced).
    let rules = skate_core::minimap::IconRules::default();
    let (s, c) = degrees.to_radians().sin_cos();
    let [hw, hh] = [NEEDLE_UNITS[0] / 2.0, NEEDLE_UNITS[1] / 2.0];
    let corner = |x: f32, y: f32, u: f32, v: f32| apt_scene::Vertex {
        position: [rules.centre[0] + (x * c - y * s) * rules.scale[0], rules.centre[1] + (x * s + y * c) * rules.scale[1]],
        uv: [u, v],
    };
    let [a, b, d, e] = [corner(-hw, -hh, 0.0, 0.0), corner(hw, -hh, 1.0, 0.0), corner(hw, hh, 1.0, 1.0), corner(-hw, hh, 0.0, 1.0)];
    apt_scene::Draw { texture: NEEDLE_TEXTURE.into(), vertices: vec![a.clone(), b, d.clone(), a, d, e], multiply: [1.0; 4], add: [0.0; 4], fill: apt_scene::Fill::TextureClamped }
}
/// The central message movie and its draw slots.
struct Central {
    runtime: hud_central::Runtime,
    source: serde_json::Value,
    shapes: apt_scene::Shapes,
    sounds: Vec<String>,
    slots: Vec<Slot>,
    failed: bool,
}
/// Above the trick display's draws (`index * 0.01` from 0), below the retail menus (`MENU_Z` 50).
const CENTRAL_Z: f32 = 30.0;
/// The `fe` record names (`private/audio/audio_manifest.json`), registered in the movie as `FE_SOUND_<NAME>`.
fn frontend_sounds(asset_root: &std::path::Path) -> Vec<String> {
    let manifest: serde_json::Value = std::fs::read(asset_root.join("private/audio/audio_manifest.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut sounds: Vec<String> = manifest["frontend"]["sounds"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter_map(|s| s["name"].as_str().map(str::to_owned))
        .collect();
    sounds.sort_unstable();
    sounds
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
        app.init_resource::<hud_minimap::MinimapHeading>().init_resource::<hud_minimap::MinimapIcons>();
        app.add_message::<hud_central::CentralMessage>().add_plugins((
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
            (resize_target, reset, hud_minimap::track_heading, hud_minimap::called_skater_icons, render)
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
        let central = match std::fs::read(root.join("runtime/hudcentral.json")) {
            Err(_) => {
                info!("Central message HUD not installed ({}): Refresh Game Files adds it", root.display());
                None
            }
            Ok(bytes) => {
                let source: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                let sounds = frontend_sounds(&config.asset_root);
                let runtime = hud_central::Runtime::load(&source, &sounds)?;
                let shapes: apt_scene::Shapes = serde_json::from_value(source["shapes"].clone()).map_err(|e| e.to_string())?;
                for texture in shapes.values().flatten().filter_map(|s| s.texture.as_ref()) {
                    files.insert(texture.rgba.clone(), [texture.width, texture.height]);
                }
                for font in runtime.bindings.movie.text_assets.fonts.values() {
                    files.insert(font.texture.clone(), font.size);
                    if let Some(fg) = &font.foreground {
                        files.insert(fg.texture.clone(), fg.size);
                    }
                }
                Some(Central { runtime, source, shapes, sounds, slots: Vec::new(), failed: false })
            }
        };
        let minimap = match std::fs::read(root.join("runtime/hudminimap.json")) {
            Err(_) => {
                info!("Minimap HUD not installed ({}): Refresh Game Files adds it", root.display());
                None
            }
            // Optional: a minimap that does not load leaves the rest of the HUD running.
            Ok(bytes) => match (|| -> Result<MiniMap, String> {
                let source: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                let runtime = hud_minimap::Runtime::load(&source)?;
                let shapes: apt_scene::Shapes = serde_json::from_value(source["shapes"].clone()).map_err(|e| e.to_string())?;
                Ok(MiniMap { runtime, shapes, slots: Vec::new(), failed: false })
            })() {
                Ok(m) => {
                    for texture in m.shapes.values().flatten().filter_map(|s| s.texture.as_ref()) {
                        files.insert(texture.rgba.clone(), [texture.width, texture.height]);
                    }
                    Some(m)
                }
                Err(error) => {
                    error!("Minimap HUD could not load: {error}");
                    None
                }
            },
        };
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
        // The needle and icon textures from the menu movie set's minimap icons (manifest `images`: texture/<name>).
        if minimap.is_some() {
            let set = crate::retail_menu_movies::set_root(&config);
            let manifest = std::fs::read(set.join("manifest.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).unwrap_or_default();
            for name in MINIMAP_TEXTURES {
                let e = &manifest["images"][format!("texture/{name}")];
                match (e["rgba"].as_str(), e["width"].as_u64(), e["height"].as_u64()) {
                    (Some(rgba), Some(w), Some(h)) => match std::fs::read(set.join(rgba)) {
                        Ok(bytes) if bytes.len() == (w * h * 4) as usize => {
                            let image = Image::new(Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, TextureDimension::D2, bytes, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
                            textures.insert(format!("minimap:{name}"), images.add(image));
                        }
                        _ => warn!("Minimap texture {rgba} unreadable"),
                    },
                    _ => warn!("Minimap: no texture/{name} in the menu set (Refresh Game Files)"),
                }
            }
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
            central,
            minimap,
        })
    })();
    match result {
        Ok(mut hud) => {
            let target = images.add(Image::new_target_texture(
                window.physical_width().max(1),
                window.physical_height().max(1),
                // Display values, blended as the console's front end does (`hud_render.wgsl`).
                TextureFormat::Rgba8Unorm,
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
                    TextureFormat::Rgba8Unorm,
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
    if let Some(central) = hud.central.as_mut() {
        match hud_central::Runtime::load(&central.source, &central.sounds) {
            Ok(runtime) => {
                central.runtime = runtime;
                central.failed = false;
            }
            Err(error) => {
                error!("Central message HUD reset: {error}");
                central.failed = true;
            }
        }
    }
}
fn advance(
    hud: Option<ResMut<Hud>>,
    skater: Res<SkaterRuntime>,
    map: Res<crate::map_transition::CurrentMap>,
    mut messages: MessageReader<hud_central::CentralMessage>,
    mut sounds: MessageWriter<crate::ui_audio::FrontendSound>,
    time: Res<Time>,
) {
    let Some(mut hud) = hud else {
        messages.clear();
        return;
    };
    if hud.generation == map.generation {
        if let Some(central) = hud.central.as_mut().filter(|c| !c.failed) {
            let result = (|| -> Result<(), String> {
                for message in messages.read() {
                    central.runtime.show(message)?;
                    info!(
                        "HUD_CENTRAL message {:?} text={:?} style={} audio_hook={} seconds={:.1}",
                        message.text, central.runtime.message_text(), message.style, message.audio_hook,
                        message.seconds.unwrap_or(hud_central::DISPLAY_SECONDS)
                    );
                }
                if central.runtime.tick(time.delta_secs())? {
                    info!("HUD_CENTRAL hidden {:?}", central.runtime.message_text());
                }
                central.runtime.update()
            })();
            for name in central.runtime.bindings.sounds.drain(..) {
                sounds.write(crate::ui_audio::FrontendSound::named(&name));
            }
            if let Err(error) = result {
                error!("Central message HUD stopped: {error}");
                central.failed = true;
            }
        }
    }
    messages.clear();
    if let Some(m) = hud.minimap.as_mut().filter(|m| !m.failed) {
        if let Err(error) = m.runtime.update() {
            error!("Minimap HUD stopped: {error}");
            m.failed = true;
        }
    }
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
/// One central message's life on screen, for the HUD_CENTRAL lines.
#[derive(Clone, Copy, Default)]
struct CentralWatch {
    showing: bool,
    frames: u32,
    drawn: u32,
    hidden: u32,
    peak: usize,
}

fn render(
    commands: Commands,
    hud: Option<ResMut<Hud>>,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<HudMaterial>>,
    fe_hud: Option<Res<crate::fe_backdrop::FeHud>>,
    overlay: Option<Res<crate::graphics_menu::Menu>>,
    menus: Option<Res<crate::retail_menus::RetailMenus>>,
    mut was_showing: Local<CentralWatch>,
    heading: Res<hud_minimap::MinimapHeading>,
    icons: Res<hud_minimap::MinimapIcons>,
) {
    let started = std::time::Instant::now();
    render_hud(commands, hud, meshes, materials, fe_hud, overlay, menus, &mut was_showing, &heading, &icons);
    crate::frame_timing::hitch::add_phase(crate::frame_timing::hitch::PHASE_HUD_RENDER, started);
}
fn render_hud(
    mut commands: Commands,
    hud: Option<ResMut<Hud>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HudMaterial>>,
    fe_hud: Option<Res<crate::fe_backdrop::FeHud>>,
    overlay: Option<Res<crate::graphics_menu::Menu>>,
    menus: Option<Res<crate::retail_menus::RetailMenus>>,
    was_showing: &mut CentralWatch,
    heading: &hud_minimap::MinimapHeading,
    icons: &hud_minimap::MinimapIcons,
) {
    let Some(mut hud) = hud else {
        return;
    };
    // HUD hidden under an FE screen that hides it, the PC overlay, or HUD Off (+164); appliers NOT RETAIL YET.
    let hud_off = menus
        .as_deref()
        .and_then(|m| crate::retail_menus::setting(m, "ID_GAMESETTINGS_HUD_TOGGLE"))
        .is_some_and(|v| skate_core::menu_values::number(&v) == 0.0);
    let hidden = fe_hud.as_ref().is_some_and(|f| !f.hud) || overlay.as_ref().is_some_and(|m| m.overlay_visible()) || hud_off;
    let draws = if hud.failed || hidden {
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
    let central_draws = match hud.central.as_mut() {
        Some(central) if !central.failed && !hidden => {
            match apt_scene::draw(&central.runtime.bindings.movie, &central.runtime.vm, &central.shapes) {
                Ok(draws) => draws,
                Err(error) => {
                    error!("Central message HUD geometry: {error}");
                    central.failed = true;
                    Vec::new()
                }
            }
        }
        _ => Vec::new(),
    };
    // Laptop session 2026-10-20: a message posted after a replay never showed; say why on its first frame, and on
    // its hide how many frames drew it (the first frame draws nothing yet, so it alone cannot tell).
    let showing = hud.central.as_ref().is_some_and(|c| c.runtime.showing());
    if showing && !was_showing.showing {
        info!(
            "HUD_CENTRAL draw draws={} hud_draws={} fe_hud={} overlay={} hud_off={} failed={}",
            central_draws.len(), draws.len(), fe_hud.as_ref().is_none_or(|f| f.hud), overlay.as_ref().is_some_and(|m| m.overlay_visible()),
            hud_off, hud.central.as_ref().is_some_and(|c| c.failed)
        );
        *was_showing = CentralWatch { showing: true, ..CentralWatch::default() };
    }
    if showing {
        was_showing.frames += 1;
        was_showing.drawn += u32::from(!central_draws.is_empty());
        was_showing.hidden += u32::from(hidden);
        was_showing.peak = was_showing.peak.max(central_draws.len());
    } else if was_showing.showing {
        let w = *was_showing;
        info!("HUD_CENTRAL shown frames={} drawn={} hidden={} peak_draws={}", w.frames, w.drawn, w.hidden, w.peak);
        *was_showing = CentralWatch::default();
    }
    // The minimap (82665278): off with the HUD, else the Game Settings Minimap row (+158, retail default on); our
    // pause menu is Free Play (game mode 3), where the row decides.
    let setting = menus
        .as_deref()
        .and_then(|m| crate::retail_menus::setting(m, "ID_GAMESETTINGS_MINIMAP_TOGGLE"))
        .is_none_or(|v| skate_core::menu_values::number(&v) != 0.0);
    let minimap_draws = match hud.minimap.as_mut() {
        Some(m) if !m.failed => {
            // A minimap element hidden by retail's hide message (ours: a mod, `sdk.minimap.show`) stays hidden.
            let shown = skate_core::minimap::visible(!hidden, 3, setting) && icons.element_shown();
            let changed = m.runtime.shown() != shown;
            // The needle turns with the heading-up map (82665848). NOT RETAIL YET: world north taken as +z.
            let north = skate_core::minimap::needle_degrees(&heading.needle);
            match m.runtime.set_shown(shown).and_then(|_| m.runtime.set_north(north)).and_then(|_| if shown { apt_scene::draw(&m.runtime.bindings.movie, &m.runtime.vm, &m.shapes).map_err(|e| e.to_string()) } else { Ok(Vec::new()) }) {
                Ok(draws) => {
                    if changed {
                        info!("HUD_MINIMAP {} draws={}", if shown { "shown" } else { "hidden" }, draws.len());
                    }
                    draws
                }
                Err(error) => {
                    error!("Minimap HUD: {error}");
                    m.failed = true;
                    Vec::new()
                }
            }
        }
        _ => Vec::new(),
    };
    let mut minimap_draws = minimap_draws;
    if !minimap_draws.is_empty() {
        // The icon groups in order (826656F8 -> 8268CB40), then the needle on top (82665848).
        let rules = skate_core::minimap::IconRules::default();
        for icon in icons.icons() {
            let key = format!("minimap:{}", icon.texture);
            let Some((c, a)) = hud.textures.contains_key(&key).then(|| skate_core::minimap::place(icon, heading.player, &heading.heading, &rules)).flatten() else { continue };
            let v = |i: usize, u: f32, w: f32| apt_scene::Vertex { position: c[i], uv: [u, w] };
            let [r, g, b, alpha] = icon.colour;
            minimap_draws.push(apt_scene::Draw { texture: key, vertices: vec![v(0, 0.0, 0.0), v(1, 1.0, 0.0), v(2, 1.0, 1.0), v(0, 0.0, 0.0), v(2, 1.0, 1.0), v(3, 0.0, 1.0)], multiply: [r, g, b, alpha * a], add: [0.0; 4], fill: apt_scene::Fill::TextureClamped });
        }
        if hud.textures.contains_key(NEEDLE_TEXTURE) {
            minimap_draws.push(needle_draw(skate_core::minimap::needle_degrees(&heading.needle)));
        }
    }
    let hud = &mut *hud;
    apply_draws(
        &mut commands,
        &mut hud.slots,
        &hud.textures,
        &draws,
        &mut meshes,
        &mut materials,
    );
    if let Some(central) = hud.central.as_mut() {
        let textures = &hud.textures;
        render_draws(&mut commands, &mut meshes, &mut materials, &mut central.slots, &central_draws, |t| textures.get(t).cloned(), CENTRAL_Z, true);
    }
    if let Some(m) = hud.minimap.as_mut() {
        let textures = &hud.textures;
        render_draws(&mut commands, &mut meshes, &mut materials, &mut m.slots, &minimap_draws, |t| textures.get(t).cloned(), MINIMAP_Z, true);
    }
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
fn hud_material(draw: &apt_scene::Draw, atlas: Handle<Image>, gamma: bool) -> HudMaterial {
    HudMaterial {
        color: ColorTransform {
            multiply: draw.multiply.into(),
            add: draw.add.into(),
            mode: Vec4::new(f32::from(u8::from(gamma)), 0., 0., 0.),
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
        && bits(old.color.mode.to_array(), new.color.mode.to_array())
        && old.atlas == new.atlas
}
/// Write draws into retained slots only when they differ (no asset churn); bit-identical to replacing them.
fn apply_draws(
    commands: &mut Commands,
    slots: &mut Vec<Slot>,
    textures: &BTreeMap<String, Handle<Image>>,
    draws: &[apt_scene::Draw],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<HudMaterial>,
) {
    apply_draws_with(commands, slots, |t| textures.get(t).cloned(), draws, meshes, materials, 0.0, true);
}

/// Draws APT geometry into the HUD's offscreen layer (31), one retained slot per draw. `z_base` stacks another
/// movie set (the retail menus) above the HUD; `gamma` applies colour transforms in gamma space.
pub(crate) fn render_draws(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<HudMaterial>,
    slots: &mut Vec<Slot>,
    draws: &[apt_scene::Draw],
    texture_of: impl Fn(&str) -> Option<Handle<Image>>,
    z_base: f32,
    gamma: bool,
) {
    apply_draws_with(commands, slots, texture_of, draws, meshes, materials, z_base, gamma);
}

#[allow(clippy::too_many_arguments)]
fn apply_draws_with(
    commands: &mut Commands,
    slots: &mut Vec<Slot>,
    texture_of: impl Fn(&str) -> Option<Handle<Image>>,
    draws: &[apt_scene::Draw],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<HudMaterial>,
    z_base: f32,
    gamma: bool,
) {
    for (index, draw) in draws.iter().enumerate() {
        let Some(texture) = texture_of(&draw.texture) else {
            continue;
        };
        let material = hud_material(draw, texture, gamma);
        if index == slots.len() {
            let mesh = meshes.add(hud_mesh(draw));
            let material = materials.add(material);
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., z_base + index as f32 * 0.01),
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
                    mode: Vec4::new(1., 0., 0., 0.),
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

    /// Scripted airs: the new HUD writer leaves every slot identical to the old one; prints churn and cost.
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
