//! Every drawn actor in the replay is recorded as drawn and played back (b364, 827BF090); mods via [`ReplayActor`].
//! Details: docs/hails-additions/37-instant-replay.md#replay-world-actors
use super::Replay;
use bevy::prelude::*;
use skate_core::replay_buffer::{ActorPose, JointPose};
use std::collections::HashMap;

/// Id tags in the top byte, after `LivingWorldId::to_u64` (0 skater, 1 pedestrian, 2 vehicle).
pub(crate) const PROP_TAG: u64 = 3 << 56;

/// A recorded actor: a stable id (network and mod safe) for its track.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReplayActor {
    pub id: u64,
}

/// The joints recorded with the actor's root, in a fixed order (their local transforms).
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub(crate) struct ReplayJoints(pub Vec<Entity>);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<LiveState>().add_systems(
        PostUpdate,
        (tag_actors, refresh_joints, record, drop_ghosts, play_back, apply_view).chain().before(bevy::transform::TransformSystems::Propagate),
    );
}

fn tag_actors(
    mut commands: Commands,
    npcs: Query<(Entity, &crate::living_world::npc_skaters::NpcSkater), Without<ReplayActor>>,
    peds: Query<(Entity, &crate::living_world::peds::Pedestrian), Without<ReplayActor>>,
    cars: Query<(Entity, &crate::living_world::vehicles::TrafficCar), Without<ReplayActor>>,
    props: Query<(Entity, &crate::skate_world::PropInstance), Without<ReplayActor>>,
    held: Query<(&crate::living_world::peds::Pedestrian, &crate::living_world::ped_hand_props::HeldHandProp)>,
    actors: Query<(), With<ReplayActor>>,
) {
    // A ped's held prop is drawn from the ped's live hand every frame; recorded as its own actor it plays back with the
    // ped instead of floating at the frozen live hand (desktop session 2026-10-20). One id per prop object (b453:
    // retail records each prop DMO by its own id), so a second prop on the same ped gets its own track.
    for (_, prop) in &held {
        if actors.get(prop.entity).is_err() {
            if let Ok(mut e) = commands.get_entity(prop.entity) {
                e.insert(ReplayActor { id: HAND_PROP_TAG | (prop.entity.to_bits() & HAND_PROP_ID_MASK) });
            }
        }
    }
    let ids = npcs.iter().map(|(e, n)| (e, n.id.to_u64()))
        .chain(peds.iter().map(|(e, p)| (e, p.id.to_u64())))
        .chain(cars.iter().map(|(e, c)| (e, c.id.to_u64())))
        .chain(props.iter().filter(|(e, _)| !held.iter().any(|(_, h)| h.entity == *e)).map(|(e, p)| (e, PROP_TAG | u64::from(p.id))));
    for (entity, id) in ids {
        commands.entity(entity).insert(ReplayActor { id });
    }
}

/// Joint lists from each kind's bindings (they change when a look loads).
fn refresh_joints(
    mut commands: Commands,
    npcs: Query<(Entity, &crate::living_world::npc_skaters::NpcPuppet, Option<&ReplayJoints>)>,
    peds: Query<(Entity, &crate::living_world::peds::PedPuppet, Option<&ReplayJoints>)>,
    cars: Query<(Entity, &crate::living_world::vehicles::CarLook, Option<&ReplayJoints>)>,
) {
    // Compared without collecting; a list is built only when it changed (b620).
    let mut sync = |entity: Entity, joints: &mut dyn Iterator<Item = Entity>, current: Option<&ReplayJoints>, again: &dyn Fn() -> Vec<Entity>| {
        if current.is_none_or(|c| !c.0.iter().copied().eq(joints)) {
            commands.entity(entity).insert(ReplayJoints(again()));
        }
    };
    for (e, p, j) in &npcs {
        sync(e, &mut p.joints_iter(), j, &|| p.joints());
    }
    for (e, p, j) in &peds {
        sync(e, &mut p.bindings.iter().flat_map(|b| b.joints_iter()), j, &|| p.bindings.as_ref().map(|b| b.joints()).unwrap_or_default());
    }
    for (e, c, j) in &cars {
        sync(e, &mut c.wheel_joints_iter(), j, &|| c.wheel_joints());
    }
}

/// A despawned actor's drawn scene, kept hidden under its replay id until its track leaves the window, so playback
/// still draws actors that left the world inside the window (retail plays them back from its frames too).
#[derive(Component)]
pub(crate) struct ReplayGhost;

/// The live entity a ghost continues: a replayed one-shot naming it lands on the ghost (b670).
#[derive(Component, Clone, Copy)]
pub(crate) struct GhostOf(pub Entity);

/// At most this many ghosts are kept (the oldest-despawned are dropped first by their tracks leaving the window).
/// Traffic churns: 90 s of cars, peds and NPC skaters leaving can pass 64.
pub(crate) const MAX_GHOSTS: usize = 256;

/// Id tag of a ped's held hand prop (after `PROP_TAG`), with the prop entity's bits below it: one track per prop.
pub(crate) const HAND_PROP_TAG: u64 = 5 << 56;
const HAND_PROP_ID_MASK: u64 = (1 << 56) - 1;

/// Despawn an actor, keeping its drawn scene for the replay. Use instead of `despawn()` for anything that has a
/// [`ReplayActor`] and [`ReplayJoints`]; anything else (or past the ghost cap) is simply despawned.
pub(crate) fn retire(commands: &mut Commands, entity: Entity) {
    commands.queue(move |world: &mut World| {
        if let Some(ghost) = ghost_parts(world, entity) {
            let (actor, joints, transform, scene) = ghost;
            // The fade goes with the scene: playback writes the recorded alpha and `present_fade` draws it on the
            // ghost's meshes (a skater that faded out before leaving would otherwise stay invisible).
            use crate::living_world::npc_skaters::{NpcFade, NpcFadeMaterials};
            let fade = world.get::<NpcFade>(entity).copied();
            let materials = world.entity_mut(entity).take::<NpcFadeMaterials>();
            let ped_materials = world.entity_mut(entity).take::<NpcFadeMaterials<crate::living_world::ped_material::PedMaterial>>();
            let mut ghost = world.spawn((Name::new("replay ghost"), ReplayGhost, GhostOf(entity), actor, joints, transform, Visibility::Hidden));
            if let Some(fade) = fade {
                ghost.insert(fade);
            }
            if let Some(materials) = materials {
                ghost.insert(materials);
            }
            if let Some(materials) = ped_materials {
                ghost.insert(materials);
            }
            let ghost = ghost.id();
            world.entity_mut(scene).insert(ChildOf(ghost));
        }
        if let Ok(e) = world.get_entity_mut(entity) {
            e.despawn();
        }
    });
}

/// A jointless model actor leaving its recorded life keeps a hidden copy; a dropped prop starts a new track (b453).
pub(crate) fn keep_model(commands: &mut Commands, entity: Entity) {
    commands.queue(move |world: &mut World| {
        let Ok(e) = world.get_entity(entity) else { return };
        let (Some(actor), Some(scene), Some(transform)) =
            (e.get::<ReplayActor>().copied(), e.get::<SceneRoot>().cloned(), e.get::<Transform>().copied()) else { return };
        let parent = e.get::<ChildOf>().map(ChildOf::parent);
        if !world.get_resource::<Replay>().is_some_and(|r| r.world.contains(actor.id)) {
            return;
        }
        let mut ghost = world.spawn((Name::new("replay ghost prop"), ReplayGhost, actor, scene, transform, Visibility::Hidden));
        if let Some(parent) = parent {
            ghost.insert(ChildOf(parent));
        }
        info!("REPLAY_GHOST prop id={:#x}", actor.id);
        world.entity_mut(entity).remove::<ReplayActor>();
    });
}

/// The actor's id, joints, root transform and the child that holds the joints (its loaded scene).
fn ghost_parts(world: &mut World, entity: Entity) -> Option<(ReplayActor, ReplayJoints, Transform, Entity)> {
    let ghosts = world.query_filtered::<(), With<ReplayGhost>>().iter(world).count();
    let replay = world.get_resource::<Replay>()?;
    let e = world.get_entity(entity).ok()?;
    let actor = *e.get::<ReplayActor>()?;
    // Every refusal is logged: a missing ghost shows as an actor vanishing from the replay.
    let refuse = |why: &str| {
        info!("REPLAY_GHOST refused id={:#x} reason={why}", actor.id);
        None
    };
    let (Some(joints), Some(transform)) = (e.get::<ReplayJoints>().cloned(), e.get::<Transform>().copied()) else {
        return refuse("no joints");
    };
    if ghosts >= MAX_GHOSTS {
        return refuse("cap");
    }
    if !replay.world.contains(actor.id) {
        return refuse("not recorded");
    }
    let Some(mut child) = joints.0.first().copied() else { return refuse("no joints") };
    loop {
        let Some(parent) = world.get::<ChildOf>(child).map(|c| c.parent()) else { return refuse("scene not under the actor") };
        if parent == entity {
            info!("REPLAY_GHOST id={:#x} joints={} ghosts={}", actor.id, joints.0.len(), ghosts + 1);
            return Some((actor, joints, transform, child));
        }
        child = parent;
    }
}

/// Ghosts whose track left the window go (never during playback).
fn drop_ghosts(mut commands: Commands, replay: Res<Replay>, ghosts: Query<(Entity, &ReplayActor), With<ReplayGhost>>) {
    if replay.active {
        return;
    }
    for (entity, actor) in &ghosts {
        if !replay.world.contains(actor.id) {
            commands.entity(entity).despawn();
        }
    }
}

fn joint(t: &Transform) -> JointPose {
    JointPose { translation: t.translation.to_array(), rotation: t.rotation.to_array(), scale: t.scale.to_array() }
}

fn transform(p: &JointPose) -> Transform {
    Transform {
        translation: Vec3::from_array(p.translation),
        rotation: Quat::from_array(p.rotation),
        scale: Vec3::from_array(p.scale),
    }
}

/// One frame per new player frame (one per sim step), after every presenter drew this frame.
#[allow(clippy::type_complexity)]
pub(super) fn record(
    mut replay: ResMut<Replay>,
    mut ids: Local<HashMap<Entity, u64>>,
    mut gone: Local<Vec<u64>>,
    mut removed: RemovedComponents<ReplayActor>,
    actors: Query<(Entity, Ref<ReplayActor>, Ref<Transform>, Option<&ReplayJoints>, Option<Ref<Visibility>>), Without<ReplayGhost>>,
    joints: Query<&Transform>,
) {
    gone.extend(removed.read().filter_map(|e| ids.remove(&e)));
    if replay.active {
        return;
    }
    let Some(now) = replay.frames.back().map(|f| f.time) else { return };
    if replay.world_time == Some(now) {
        return;
    }
    replay.world_time = Some(now);
    replay.world.begin_frame(now);
    for id in gone.drain(..) {
        replay.world.end(id);
    }
    for (entity, actor, root, list, visibility) in &actors {
        // A jointless actor with no change keeps its last key; one without a track yet always records.
        let changed = actor.is_added() || root.is_changed() || visibility.as_ref().is_some_and(|v| v.is_changed())
            || !replay.world.contains(actor.id);
        if list.is_none() && !changed {
            continue;
        }
        ids.insert(entity, actor.id);
        let visibility = visibility.map(|v| *v);
        let pose = ActorPose {
            root: joint(&root),
            joints: list.map(|l| l.0.iter().filter_map(|j| joints.get(*j).ok()).map(joint).collect()).unwrap_or_default(),
            hidden: visibility == Some(Visibility::Hidden),
        };
        replay.world.record(actor.id, &pose);
    }
}

/// Live transforms and visibility of every actor the playback touched, put back on exit.
#[derive(Resource, Default)]
struct LiveState {
    saved: Option<HashMap<Entity, (Transform, Option<Visibility>)>>,
}

fn play_back(
    replay: Res<Replay>,
    mut live: ResMut<LiveState>,
    actors: Query<(Entity, &ReplayActor, Option<&ReplayJoints>)>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut commands: Commands,
) {
    if !replay.active {
        for (entity, (t, v)) in live.saved.take().into_iter().flatten() {
            if let Ok(mut current) = transforms.get_mut(entity) {
                *current = t;
            }
            match (v, visibility.get_mut(entity)) {
                (Some(v), Ok(mut current)) => *current = v,
                (None, Ok(_)) => {
                    commands.entity(entity).remove::<Visibility>();
                }
                _ => {}
            }
        }
        return;
    }
    let saved = live.saved.get_or_insert_with(HashMap::new);
    let keep = |saved: &mut HashMap<_, _>, entity: Entity, transforms: &Query<&mut Transform>, visibility: &Query<&mut Visibility>| {
        *saved.entry(entity).or_insert_with(|| {
            (transforms.get(entity).copied().unwrap_or_default(), visibility.get(entity).ok().copied())
        })
    };
    let time = replay.cursor;
    for (entity, actor, list) in &actors {
        let (_, live_visibility) = keep(saved, entity, &transforms, &visibility);
        let sample = replay.world.sample(actor.id, time);
        let shown = sample.as_ref().is_some_and(|p| !p.hidden) && replay.view.shows(actor.id >> 56);
        if let Ok(mut v) = visibility.get_mut(entity) {
            *v = if shown { live_visibility.filter(|v| *v != Visibility::Hidden).unwrap_or(Visibility::Inherited) } else { Visibility::Hidden };
        } else if !shown {
            commands.entity(entity).insert(Visibility::Hidden);
        }
        let Some(pose) = sample else { continue };
        if let Ok(mut t) = transforms.get_mut(entity) {
            *t = transform(&pose.root);
        }
        let Some(list) = list.filter(|l| l.0.len() == pose.joints.len()) else { continue };
        for (j, p) in list.0.iter().zip(&pose.joints) {
            keep(saved, *j, &transforms, &visibility);
            if let Ok(mut t) = transforms.get_mut(*j) {
                *t = transform(p);
            }
        }
    }
}

/// What [`apply_view`] changed, to put back.
#[derive(Default)]
struct ViewApplied {
    player: Option<(Entity, Option<Visibility>)>,
    map: Vec<(Entity, Option<Visibility>)>,
    clear: Option<ClearColor>,
}

/// The film pack's replay mode on the player and the world (b557, `827F9D40` / `827FEFD0`): "player visible" off hides
/// the player's root; greenscreen hides everything the map owns and the sky dome (not the lights, so the characters
/// stay lit) and clears to the greenscreen colour (retail fills (0, 1, 0, 1) before the scene, b558 `827FE8D8`). Put back when the mode changes or the replay closes.
fn apply_view(
    replay: Res<Replay>,
    mut state: Local<ViewApplied>,
    players: Query<Entity, With<crate::world::PlayerRoot>>,
    map: Query<Entity, (Or<(With<crate::map_render::MapEntity>, With<crate::retail_sky::SkyDome>)>, Without<ChildOf>, Without<DirectionalLight>, Without<PointLight>, Without<SpotLight>)>,
    mut visibility: Query<&mut Visibility>,
    clear: Option<ResMut<ClearColor>>,
    mut commands: Commands,
) {
    let Some(mut clear) = clear else { return };
    let view = if replay.active { replay.view } else { super::ReplayViewMode::default() };
    let mut hide = |entity: Entity, visibility: &mut Query<&mut Visibility>| -> Option<Visibility> {
        let old = visibility.get(entity).ok().copied();
        match visibility.get_mut(entity) {
            Ok(mut v) => *v = Visibility::Hidden,
            Err(_) => {
                commands.entity(entity).insert(Visibility::Hidden);
            }
        }
        old
    };
    if !view.player && state.player.is_none() {
        if let Some(entity) = players.iter().next() {
            state.player = Some((entity, hide(entity, &mut visibility)));
        }
    }
    if view.greenscreen && state.clear.is_none() {
        state.clear = Some(clear.clone());
        let [r, g, b] = view.greenscreen_colour;
        clear.0 = Color::srgb(r, g, b);
        state.map = map.iter().map(|e| (e, hide(e, &mut visibility))).collect();
    }
    let mut restore = |entity: Entity, old: Option<Visibility>, visibility: &mut Query<&mut Visibility>| match (old, visibility.get_mut(entity)) {
        (Some(old), Ok(mut v)) => *v = old,
        (None, Ok(_)) => {
            commands.entity(entity).remove::<Visibility>();
        }
        _ => {}
    };
    if view.player && let Some((entity, old)) = state.player.take() {
        restore(entity, old, &mut visibility);
    }
    if !view.greenscreen && let Some(old) = state.clear.take() {
        *clear = old;
        for (entity, old) in std::mem::take(&mut state.map) {
            restore(entity, old, &mut visibility);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Greenscreen hides the map (not its lights) and clears green; "player visible" off hides the player; both come
    /// back when the mode is off again (b557).
    #[test]
    fn replay_mode_hides_the_world_and_the_player_and_puts_them_back() {
        let mut app = App::new();
        app.init_resource::<Replay>().insert_resource(ClearColor(Color::BLACK)).add_systems(Update, apply_view);
        let world = app.world_mut();
        let player = world.spawn((crate::world::PlayerRoot, Visibility::Inherited)).id();
        let level = world.spawn((crate::map_render::MapEntity, Visibility::Visible)).id();
        let sky = world.spawn(crate::map_render::MapEntity).id();
        let sun = world.spawn((crate::map_render::MapEntity, DirectionalLight::default(), Visibility::Inherited)).id();
        let dome = world.spawn((crate::retail_sky::SkyDome, Visibility::Visible)).id();
        {
            let mut replay = world.resource_mut::<Replay>();
            replay.active = true;
            replay.view.toggle(0);
            replay.view.toggle(1);
        }
        app.update();
        let w = app.world();
        let vis = |e: Entity| w.get::<Visibility>(e).copied();
        assert_eq!((vis(player), vis(level), vis(sky), vis(sun)), (Some(Visibility::Hidden), Some(Visibility::Hidden), Some(Visibility::Hidden), Some(Visibility::Inherited)));
        assert_eq!(vis(dome), Some(Visibility::Hidden));
        assert_eq!(w.resource::<ClearColor>().0, Color::srgb(0.0, 1.0, 0.0));
        app.world_mut().resource_mut::<Replay>().view = super::super::ReplayViewMode::default();
        app.update();
        let w = app.world();
        let vis = |e: Entity| w.get::<Visibility>(e).copied();
        assert_eq!((vis(player), vis(level), vis(sky)), (Some(Visibility::Inherited), Some(Visibility::Visible), None));
        assert_eq!(vis(dome), Some(Visibility::Visible));
        assert_eq!(w.resource::<ClearColor>().0, Color::BLACK);
    }
}
