//! FE 62's skater preview (retail MarqueeViewer, research b575..b580, b585): the highlighted skater stands on a stage
//! above the map playing MARQUEE_STAND, the camera in front. Details: docs/hails-additions/31-retail-menus.md#fe-62-preview
use crate::{animation::AnimationStatus, spawn_skater_host::SpawnSkaterRequest, world::PlayerRoot};
use bevy::prelude::*;
use skate_core::spawn_skater::{Action, Skater};
use skate_data::scene_clip::SceneClip;
use std::sync::Arc;

/// The stage (vault `freeskate` layout, b576 / b578, placement traced b585), data so a mod can move it.
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct MarqueeStage {
    /// World position of the character's feet and its heading (degrees about +Y).
    pub position: Vec3,
    pub heading_degrees: f32,
    /// Camera eye and forward (world), as the game camera got them (b585 trace, `MQSET`).
    pub eye: Vec3,
    pub forward: Vec3,
    pub fov_degrees: f32,
    /// `stock/data/anim/<bank>` and the clip (b579: loops by its own flag).
    pub bank: String,
    pub clip: String,
    /// The set around the stage, `private/hud/scene/<set>` (setup's export of DIST_CreateSakter, modelled in place).
    pub set: String,
}

impl Default for MarqueeStage {
    fn default() -> Self {
        Self {
            position: Vec3::new(-1.0, 200.0, -7.05),
            heading_degrees: -100.0,
            eye: Vec3::new(-4.0, 200.48, -7.4),
            forward: Vec3::new(0.989, 0.127, -0.076),
            fov_degrees: 60.0,
            bank: "sk83_marquee_viewer.abin".into(),
            clip: "MARQUEE_STAND".into(),
            set: "MarqueeStage.skate".into(),
        }
    }
}

/// Set by the retail menu stage: FE 62 is the top screen in a mode that previews (Free Play, Demo; not Call Skater).
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct MarqueeView {
    pub open: bool,
}

#[derive(Resource, Default)]
struct Wanted(Option<Skater>);

#[derive(Default)]
struct Viewer {
    active: bool,
    /// The player's character before the preview (`CustomModels::active`), put back on close.
    saved: Option<Option<String>>,
    shown: Option<Skater>,
    clip: Option<Option<Arc<SceneClip>>>,
    bindings: Option<AnimationStatus>,
    /// Bound after the swap for `shown` finished, with the skinned mesh count then (outfit parts spawn later).
    bound_for: Option<Skater>,
    skins: usize,
    time: f32,
    fov: Option<f32>,
    set: Option<Option<Arc<skate_data::skate_map::SkateMap>>>,
    stage: Vec<Entity>,
}

pub(crate) struct MarqueePreviewPlugin;
impl Plugin for MarqueePreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MarqueeStage>().init_resource::<MarqueeView>().init_resource::<Wanted>().add_systems(
            Update,
            (wanted, drive).chain().after(crate::app::FrameSet::Animation).after(crate::camera::present)
                .before(crate::app::FrameSet::Verification),
        );
    }
}

/// FlowMessage 0x8D (82705E88): the highlighted skater.
fn wanted(mut requests: MessageReader<SpawnSkaterRequest>, mut wanted: ResMut<Wanted>) {
    for SpawnSkaterRequest(action) in requests.read() {
        if let Action::Preview(skater) = action {
            wanted.0 = Some(skater.clone());
        }
    }
}

fn drive(world: &mut World, mut viewer: Local<Viewer>) {
    let open = world.resource::<MarqueeView>().open;
    let Some(player) = world.query_filtered::<Entity, With<PlayerRoot>>().iter(world).next() else { return };
    // The first highlight is posted in the frame FE 62 opens, before the stage reports it open: keep it.
    if !open {
        if viewer.active {
            close(world, &mut viewer);
            world.resource_mut::<Wanted>().0 = None;
        }
        return;
    }
    let Some(models) = world.get_resource::<crate::custom_models::CustomModels>() else { return };
    if !viewer.active {
        viewer.active = true;
        viewer.saved = Some(models.active.clone());
        info!("MARQUEE_VIEWER open (player {:?})", models.active);
        open_stage(world, &mut viewer);
    }
    let stage = world.resource::<MarqueeStage>().clone();
    // A new highlight swaps the character (retail unloads the external character and streams the previewed one).
    let wanted = world.resource::<Wanted>().0.clone();
    if wanted.is_some() && wanted != viewer.shown {
        let id = match &wanted {
            Some(Skater::Character(id)) => Some(id.as_str()),
            _ => None,
        };
        let mut models = world.resource_mut::<crate::custom_models::CustomModels>();
        if !models.request_preview(id) {
            warn!("MARQUEE_VIEWER no character {id:?}");
        }
        info!("MARQUEE_VIEWER load {wanted:?}");
        viewer.shown = wanted;
        viewer.bindings = None;
        viewer.bound_for = None;
        viewer.time = 0.0;
    }
    // Every open frame, the swap frames included: the gameplay camera (`camera::present`) would show the map.
    stage_camera(world, &mut viewer, &stage);
    if world.resource::<crate::custom_models::CustomModels>().swapping() || viewer.shown.is_none() {
        return;
    }
    let clip = viewer.clip.get_or_insert_with(|| {
        let path = world.resource::<crate::config::Config>().asset_root.join("private/stock/data/anim").join(&stage.bank);
        std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| SceneClip::parse_named(b, Some(&stage.clip)))
            .inspect_err(|e| warn!("MARQUEE_VIEWER clip {} in {}: {e}", stage.clip, path.display()))
            .ok().map(Arc::new)
    }).clone();
    let Some(clip) = clip else { return };
    let skins = world.query::<&bevy::mesh::skinning::SkinnedMesh>().iter(world).count();
    if viewer.bound_for != viewer.shown || viewer.skins != skins {
        viewer.bindings = crate::trick_guide_scene::bind(world, player, &clip);
        viewer.bound_for = viewer.shown.clone();
        viewer.skins = skins;
        info!("MARQUEE_VIEWER show {:?}: {} joints bound", viewer.shown, viewer.bindings.as_ref().map_or(0, |b| b.online_bindings().len()));
    }
    viewer.time = (viewer.time + world.resource::<Time<Real>>().delta_secs()) % clip.duration().max(1e-3);
    let Some(globals) = crate::trick_guide_scene::pose(&clip, viewer.time) else { return };
    // The character's world transform is translate(position) * rotY(heading) (b578), drawn through the player rig.
    let root = world.get::<GlobalTransform>(player).copied().unwrap_or_default().to_matrix();
    let place = root.inverse() * Mat4::from_rotation_translation(facing(stage.heading_degrees), stage.position);
    if let Some(bindings) = &viewer.bindings {
        let matrices: Vec<Mat4> = globals.iter().map(|g| place * crate::animation::native_matrix(*g)).collect();
        for (entity, transform) in bindings.pose_transforms(&matrices) {
            if let Some(mut node) = world.get_mut::<Transform>(entity) {
                *node = transform;
            }
        }
    }
}

fn stage_camera(world: &mut World, viewer: &mut Viewer, stage: &MarqueeStage) {
    let camera = world.query_filtered::<Entity, With<crate::camera::GameplayCamera>>().iter(world).next();
    if let Some(camera) = camera {
        if let Some(mut transform) = world.get_mut::<Transform>(camera) {
            *transform = Transform::from_translation(stage.eye).looking_to(stage.forward, Vec3::Y);
        }
        if let Some(mut projection) = world.get_mut::<Projection>(camera) {
            if let Projection::Perspective(p) = &mut *projection {
                viewer.fov.get_or_insert(p.fov);
                p.fov = stage.fov_degrees.to_radians();
            }
        }
    }
}

/// The set the viewer's streamer move shows (retail streams the stage in with the camera move, b575).
fn open_stage(world: &mut World, viewer: &mut Viewer) {
    let asset_root = world.resource::<crate::config::Config>().asset_root.clone();
    let name = world.resource::<MarqueeStage>().set.clone();
    let set = viewer.set.get_or_insert_with(|| {
        let path = asset_root.join("private/hud/scene").join(&name);
        std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| skate_data::skate_map::SkateMap::parse_render_only(&b))
            .inspect_err(|e| warn!("MARQUEE_VIEWER stage {} unavailable (run setup's refresh): {e}", path.display()))
            .ok().map(Arc::new)
    }).clone();
    let Some(set) = set else { return };
    // Lit like the map around it (no render location of its own found).
    let environment = world.get_resource::<crate::map_transition::CurrentMap>()
        .and_then(|map| crate::retail_sky::SkyPackage::load(&asset_root, &map.name).ok())
        .map(|s| s.environment()).unwrap_or_default();
    viewer.stage = crate::trick_guide_scene::spawn_set(world, &set, &environment, None);
}

/// Retail's heading on our axes: the clip's forward is +X, a quarter turn from the heading's zero (in game
/// 2026-10-29: -100 degrees as is turned the character side on).
fn facing(heading_degrees: f32) -> Quat {
    Quat::from_rotation_y((heading_degrees - 90.0).to_radians())
}

/// Shutdown (0x2E60BEA0, b575 / b578): the player's own character comes back unless FE 62 made a real pick; the
/// gameplay presentation rewrites the rig and camera on its next frame.
fn close(world: &mut World, viewer: &mut Viewer) {
    if let Some(fov) = viewer.fov.take() {
        let camera = world.query_filtered::<Entity, With<crate::camera::GameplayCamera>>().iter(world).next();
        if let Some(mut projection) = camera.and_then(|c| world.get_mut::<Projection>(c)) {
            if let Projection::Perspective(p) = &mut *projection {
                p.fov = fov;
            }
        }
    }
    let saved = viewer.saved.take().flatten();
    if let Some(mut models) = world.get_resource_mut::<crate::custom_models::CustomModels>() {
        if models.previewing() {
            models.request_preview(saved.as_deref());
        }
    }
    for entity in std::mem::take(&mut viewer.stage) {
        if let Ok(e) = world.get_entity_mut(entity) {
            e.despawn();
        }
    }
    info!("MARQUEE_VIEWER closed (back to {saved:?})");
    *viewer = Viewer { clip: viewer.clip.take(), set: viewer.set.take(), ..default() };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail's heading turns the clip's forward (+X) toward the camera eye (b578 left the sign open; the vault's
    /// -100 degrees and the traced eye agree within 4 degrees).
    #[test]
    fn the_retail_stage_faces_the_character_to_the_camera() {
        let s = MarqueeStage::default();
        let forward = facing(s.heading_degrees) * Vec3::X;
        let to_eye = ((s.eye - s.position) * Vec3::new(1.0, 0.0, 1.0)).normalize();
        assert!(forward.angle_between(to_eye).to_degrees() < 5.0, "{forward} vs {to_eye}");
    }
}
