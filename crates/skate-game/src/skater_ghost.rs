//! Skater fade-in after every placement ("ghost"), retail sub_82594488 / sub_825926F8
//! (`skate_core::player::ghost`).
//!
//! - **Placement** (`FixedUpdate`, after the physics tick): every completed teleport of the
//!   simulated skater (checkpoint respawn, menu / map / mod teleports, session marker return; all
//!   finish in the one reset at `physics/frame.rs`, counter `respawn::Runtime::placements`) and a
//!   newly loaded skater (spawn, map change) become a [`SkaterPlaced`] message keyed by the stable
//!   player id. Serialisable, so a networked host could send the same events; no networking here.
//! - **Fade** (`FixedUpdate`): [`SkaterGhosts`] holds one [`GhostFade`] per player id; a
//!   placement restarts it, then each fixed tick publishes the opacity and advances the timer by
//!   the fixed dt (time-based, any tick rate gives the same curve).
//! - **Draw** (`Update`): while the local player's opacity is below 1 every mesh under
//!   [`PlayerRoot`] (body, hair, clothes, board parts) draws a blended copy of its material with
//!   the alpha scaled; at 1.0 the original material handle goes back, so solid frames draw
//!   exactly the materials they drew before this feature.
//! - **Mods**: world tuning domain `ghost` {enabled, fade_in_seconds, hold_alpha} writes
//!   [`GhostTuning`] (retail defaults, reset on mod stop).
//!
//! NOT RETAIL YET: the 0.68 hold condition (state byte 71) is undecoded and never set here; the
//! render-side smoother sub_8278C4D8 (15/s up, 5/s down toward the published opacity) is not
//! ported (the 1 s ramp is slower than it, so the drawn opacity follows the published one). NPC
//! skaters do not use this path: their own spawn fade-in is `living_world::npc_skaters::NpcFade`.
use bevy::prelude::*;
use skate_core::player::ghost::{GhostFade, GhostSettings};
use std::collections::BTreeMap;

use crate::physics::SkaterRuntime;
use crate::physics::respawn::LOCAL_PLAYER_ID;
use crate::world::PlayerRoot;

/// One placement of a skater (retail sub_825926F8), emitted on the fixed tick.
#[derive(Message, Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SkaterPlaced {
    pub player_id: u32,
    /// The skater's placement count after this one (0 = spawned / loaded).
    pub placement: u32,
}

/// Fade tuning a mod changes (`sdk.world.set_tuning('ghost', ...)`); `default()` = retail.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct GhostTuning(pub GhostSettings);

/// Per stable player id fade state.
#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct SkaterGhosts {
    pub fades: BTreeMap<u32, GhostFade>,
}

impl SkaterGhosts {
    /// Opacity the player draws with now (1 = solid; unknown players are solid).
    pub(crate) fn opacity(&self, player_id: u32) -> f32 {
        self.fades.get(&player_id).map_or(1.0, |f| f.opacity)
    }
}

pub(crate) struct GhostPlugin;
impl Plugin for GhostPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GhostTuning>()
            .init_resource::<SkaterGhosts>()
            .add_message::<SkaterPlaced>()
            .add_systems(
                FixedUpdate,
                (emit_placements, step_fades).chain().after(crate::app::SimulationSet::Physics),
            )
            .add_systems(
                Update,
                (
                    present::<StandardMaterial>,
                    present::<crate::customiser_material::SkaterMaterial>,
                    present::<crate::retail_character::CharacterMaterial>,
                )
                    .after(crate::retail_character::CharacterUpdateSet),
            );
    }
}

/// Last placement count seen per player id.
#[derive(Default)]
pub(crate) struct Seen(Option<u32>);

/// Turn the simulated skater's placement counter (and a newly loaded skater) into messages.
pub(crate) fn emit_placements(
    skater: Option<Res<SkaterRuntime>>,
    mut seen: Local<Seen>,
    mut out: MessageWriter<SkaterPlaced>,
) {
    let Some(skater) = skater else {
        seen.0 = None;
        return;
    };
    let count = skater.placements();
    if skater.is_added() || seen.0.is_none() {
        // Constructor sub_82590DC0 starts the timer at 0: a new skater fades in.
        out.write(SkaterPlaced { player_id: LOCAL_PLAYER_ID, placement: count });
    } else if seen.0 != Some(count) {
        out.write(SkaterPlaced { player_id: LOCAL_PLAYER_ID, placement: count });
    }
    seen.0 = Some(count);
}

/// Restart fades on placements, then one sub_82594488 step per player on the fixed dt.
pub(crate) fn step_fades(
    mut placed: MessageReader<SkaterPlaced>,
    tuning: Res<GhostTuning>,
    time: Res<Time>,
    mut ghosts: ResMut<SkaterGhosts>,
) {
    for p in placed.read() {
        ghosts.fades.entry(p.player_id).or_default().place();
    }
    let dt = time.delta_secs();
    for fade in ghosts.fades.values_mut() {
        // NOT RETAIL YET: hold condition (state byte 71) undecoded, kept off.
        fade.step(dt, &tuning.0, false);
    }
}

/// A material the ghost fade can draw see-through.
pub(crate) trait GhostMaterial: Material + Clone {
    /// Set this copy's alpha fields from `original` scaled by `opacity` (only alpha fields, so
    /// lighting written into the copy meanwhile stays).
    fn ghost(&mut self, original: &Self, opacity: f32);
}

fn blended(mode: AlphaMode) -> AlphaMode {
    match mode {
        AlphaMode::Opaque | AlphaMode::Mask(_) | AlphaMode::AlphaToCoverage => AlphaMode::Blend,
        other => other,
    }
}

impl GhostMaterial for StandardMaterial {
    fn ghost(&mut self, original: &Self, opacity: f32) {
        self.alpha_mode = blended(original.alpha_mode);
        self.base_color.set_alpha(original.base_color.alpha() * opacity);
    }
}

impl GhostMaterial for crate::customiser_material::SkaterMaterial {
    fn ghost(&mut self, original: &Self, opacity: f32) {
        self.base.ghost(&original.base, opacity);
    }
}

/// The mesh's own material while it draws a faded copy.
#[derive(Component)]
pub(crate) struct GhostSaved<M: Material> {
    pub original: Handle<M>,
    copy: Handle<M>,
    opacity: f32,
}

pub(crate) fn present<M: GhostMaterial>(
    mut commands: Commands,
    ghosts: Res<SkaterGhosts>,
    roots: Query<Entity, With<PlayerRoot>>,
    children: Query<&Children>,
    mut meshes: Query<(&mut MeshMaterial3d<M>, Option<&mut GhostSaved<M>>)>,
    orphans: Query<Entity, (With<GhostSaved<M>>, Without<MeshMaterial3d<M>>)>,
    materials: Option<ResMut<Assets<M>>>,
) {
    let Some(mut materials) = materials else { return };
    // A material swapped out by its owner (retail binding, outfit change) drops the saved state.
    for e in &orphans {
        commands.entity(e).try_remove::<GhostSaved<M>>();
    }
    let opacity = ghosts.opacity(LOCAL_PLAYER_ID).clamp(0.0, 1.0);
    for root in &roots {
        for mesh in children.iter_descendants(root) {
            let Ok((mut handle, saved)) = meshes.get_mut(mesh) else { continue };
            let ours = saved.as_ref().is_some_and(|s| s.copy.id() == handle.0.id());
            if opacity >= 1.0 {
                if let Some(s) = saved {
                    if ours {
                        handle.0 = s.original.clone();
                    }
                    commands.entity(mesh).try_remove::<GhostSaved<M>>();
                }
                continue;
            }
            match saved {
                Some(mut s) if ours => {
                    if s.opacity != opacity {
                        let Some(original) = materials.get(&s.original).cloned() else { continue };
                        if let Some(copy) = materials.get_mut(&s.copy) {
                            copy.ghost(&original, opacity);
                        }
                        s.opacity = opacity;
                    }
                }
                _ => {
                    let Some(original) = materials.get(&handle.0).cloned() else { continue };
                    let mut copy = original.clone();
                    copy.ghost(&original, opacity);
                    let copy = materials.add(copy);
                    commands.entity(mesh).try_insert(GhostSaved { original: handle.0.clone(), copy: copy.clone(), opacity });
                    handle.0 = copy;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<SkaterGhosts>()
            .add_systems(Update, present::<StandardMaterial>);
        app
    }

    fn set_opacity(app: &mut App, opacity: f32) {
        app.world_mut().resource_mut::<SkaterGhosts>().fades.insert(
            LOCAL_PLAYER_ID,
            GhostFade { timer: 0.0, opacity },
        );
    }

    #[test]
    fn faded_frames_blend_a_copy_and_solid_frames_restore_the_original() {
        let mut app = app();
        let authored = StandardMaterial { base_color: Color::srgba(0.2, 0.4, 0.6, 0.8), ..default() };
        let original = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(authored.clone());
        let root = app.world_mut().spawn(PlayerRoot).id();
        let mesh = app.world_mut().spawn((MeshMaterial3d(original.clone()), ChildOf(root))).id();
        let other = app.world_mut().spawn(MeshMaterial3d(original.clone())).id();

        // Solid: nothing changes, no saved state.
        set_opacity(&mut app, 1.0);
        app.update();
        assert_eq!(app.world().get::<MeshMaterial3d<StandardMaterial>>(mesh).unwrap().0, original);
        assert!(app.world().get::<GhostSaved<StandardMaterial>>(mesh).is_none());

        set_opacity(&mut app, 0.25);
        app.update();
        let copy = app.world().get::<MeshMaterial3d<StandardMaterial>>(mesh).unwrap().0.clone();
        assert_ne!(copy, original);
        let assets = app.world().resource::<Assets<StandardMaterial>>();
        let m = assets.get(&copy).unwrap();
        assert_eq!(m.alpha_mode, AlphaMode::Blend);
        assert!((m.base_color.alpha() - 0.8 * 0.25).abs() < 1e-6);
        let o = assets.get(&original).unwrap();
        assert_eq!(o.alpha_mode, authored.alpha_mode, "original untouched");
        assert_eq!(o.base_color, authored.base_color, "original untouched");
        assert_eq!(app.world().get::<MeshMaterial3d<StandardMaterial>>(other).unwrap().0, original, "not under the player");

        set_opacity(&mut app, 0.5);
        app.update();
        let assets = app.world().resource::<Assets<StandardMaterial>>();
        assert!((assets.get(&copy).unwrap().base_color.alpha() - 0.4).abs() < 1e-6, "copy reused");

        set_opacity(&mut app, 1.0);
        app.update();
        assert_eq!(app.world().get::<MeshMaterial3d<StandardMaterial>>(mesh).unwrap().0, original, "solid draws the original");
        assert!(app.world().get::<GhostSaved<StandardMaterial>>(mesh).is_none());
    }

    #[test]
    fn every_placement_kind_restarts_the_fade() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<GhostTuning>()
            .init_resource::<SkaterGhosts>()
            .add_message::<SkaterPlaced>()
            .add_systems(Update, step_fades);
        let tick = |app: &mut App, placed: Option<u32>| {
            if let Some(n) = placed {
                app.world_mut().write_message(SkaterPlaced { player_id: LOCAL_PLAYER_ID, placement: n });
            }
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f64(1.0 / 60.0));
            app.world_mut().run_schedule(Update);
            app.world().resource::<SkaterGhosts>().opacity(LOCAL_PLAYER_ID)
        };
        // Spawn (placement 0), then checkpoint respawn, menu teleport, marker return: all funnel
        // into the one placement counter, so each is a placement message.
        for placement in 0..4 {
            assert_eq!(tick(&mut app, Some(placement)), 0.0, "placement {placement} draws at 0");
            let mut a = 0.0;
            for _ in 0..30 {
                a = tick(&mut app, None);
            }
            assert!(a > 0.4 && a < 0.6, "half way: {a}");
            for _ in 0..40 {
                a = tick(&mut app, None);
            }
            assert_eq!(a, 1.0);
        }
        // Mod override: disabled = solid right after a placement.
        app.world_mut().resource_mut::<GhostTuning>().0.enabled = false;
        assert_eq!(tick(&mut app, Some(9)), 1.0);
        let json = serde_json::to_string(&SkaterPlaced { player_id: 3, placement: 7 }).unwrap();
        assert_eq!(serde_json::from_str::<SkaterPlaced>(&json).unwrap(), SkaterPlaced { player_id: 3, placement: 7 });
    }
}
