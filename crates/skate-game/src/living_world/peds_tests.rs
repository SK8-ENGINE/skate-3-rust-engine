//! Headless, seeded tests of the ped body (doc 26, peds M2): spawn records become peds with the
//! seeded look, they animate and move from the record and the tick alone (any frame rate), they
//! publish foot plants, despawn cleans up, mod overrides apply. No window, no assets (one
//! data-gated test loads the real bank and tables).

use super::peds::*;
use super::*;
use skate_core::animation::output::Sqt;
use skate_core::living_world::peds::anim::{ClipWindow, IDENTITY, PedClip, RemapClip, names};
use skate_core::living_world::peds::{Locomotion, PedAnimPlayer, PedAnimSet, PedCatalog, PedEntity, PedModel, PedOverrides, PedRig};
use std::collections::BTreeMap;
use std::sync::Arc;

fn sqt(t: [f32; 3]) -> Sqt {
    Sqt { scale: [1.0; 4], rotation: [0.0, 0.0, 0.0, 1.0], translation: [t[0], t[1], t[2], 1.0] }
}

fn clip(name: &str, frames: usize, speed: f32, looping: bool, windows: Vec<ClipWindow>) -> PedClip {
    let len = (frames - 1) as f32 / 30.0;
    PedClip {
        name: name.into(),
        fps: 30.0,
        frames: (0..frames).map(|f| vec![sqt([0.0, 0.0, speed * f as f32 / 30.0]), IDENTITY, IDENTITY]).collect(),
        looping,
        loop_rotation: [0.0, 0.0, 0.0, 1.0],
        loop_translation: [0.0, 0.0, speed * len],
        windows,
    }
}

fn data() -> PedData {
    let w = |c: &str, b: f32, e: f32| ClipWindow { channel: c.into(), begin: b, end: e, value: 1.0 };
    let clips: BTreeMap<String, PedClip> = [
        clip("IDLE", 31, 0.0, true, vec![w("LEFTTOEDOWN", 0.0, 1.0), w("RIGHTTOEDOWN", 0.0, 1.0)]),
        clip("WALK", 34, 1.3, true, vec![w("LEFTTOEDOWN", 0.25, 0.75), w("RIGHTTOEDOWN", 0.0, 0.3), w("RIGHTTOEDOWN", 0.76, 1.0)]),
        clip("START", 28, 0.8, false, vec![]),
        clip("STOP", 30, 0.5, false, vec![]),
        clip("TURN", 43, 0.0, false, vec![]),
        clip("HIT", 31, 0.0, false, vec![]),
        clip("GROUND", 21, 0.0, true, vec![]),
    ]
    .into_iter()
    .map(|c| (c.name.clone(), c))
    .collect();
    let r = |c: &str| vec![RemapClip { clip: c.into(), windows: vec![] }];
    let mut set = PedAnimSet::default();
    for (n, c) in [(names::IDLE, "IDLE"), (names::WALK, "WALK"), (names::START, "START"), (names::STOP, "STOP"), (names::TURN_180, "TURN")] {
        set.entries.insert(n.into(), r(c));
    }
    for n in skate_core::living_world::peds::skater_contact::REACTION_ANIMS {
        set.entries.insert((*n).into(), r(if n.contains("GroundCyc") { "GROUND" } else { "HIT" }));
    }
    let mut catalog = PedCatalog::default();
    catalog.categories.insert("aletown".into(), vec!["jock02".into(), "bum02".into()]);
    catalog.categories.insert("empty".into(), vec![]);
    for (e, m, recipe, voice) in [("jock02", "jock02", "male_jock_2", 55), ("bum02", "bum02", "male_bum_2", 60)] {
        catalog.entities.insert(e.into(), PedEntity { model: Some(m.into()), anim_set: Some("default".into()) });
        catalog.models.insert(m.into(), PedModel { recipe: recipe.into(), voice: Some(voice), ..Default::default() });
    }
    let rig = PedRig {
        names: vec!["TRAJECTORY".into(), "HIPS".into(), "HEADEND".into()],
        parents: vec![-1, 0, 1],
        mirrors: vec![0, 1, -1],
        reference: vec![IDENTITY, sqt([0.0, 0.92, 0.0]), IDENTITY],
        animated: vec![true, true, false],
    };
    let mut d = PedData::default();
    d.catalog = Arc::new(catalog);
    d.anim_sets = Arc::new([("default".to_string(), set)].into_iter().collect());
    d.rig = Arc::new(rig);
    d.clips = Arc::new(clips);
    d
}

fn record(serial: u32, category: &str, seed: u64, tick: u64) -> SpawnRecord {
    SpawnRecord {
        id: LivingWorldId { kind: Kind::Pedestrian, serial },
        tick,
        position: [serial as f32 * 3.0, 0.0, 10.0],
        heading: 0.5,
        seed,
        initial: false,
        choice: SpawnChoice::Census { record: "aletown".into(), category: category.into() },
    }
}

#[derive(Resource, Default)]
struct Seen(Vec<PedEvent>);

fn collect(mut ev: MessageReader<PedEvent>, mut seen: ResMut<Seen>) {
    seen.0.extend(ev.read().cloned());
}

fn app(hz: f32) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let settings = LivingWorldSettings { seed: 3, ..LivingWorldSettings::default() };
    let mut state = PopulationState::default();
    state.install("Test", 1, &settings, LoadedData { config: PopulationConfig::retail(), census: None, skaters: None, roads: None, vehicles: None, npc: Default::default(), status: "peds test".into() });
    app.insert_resource(settings)
        .insert_resource(state)
        .insert_resource(data())
        .init_resource::<LivingWorldObservers>()
        .init_resource::<PedLooks>()
        .init_resource::<PedIndex>()
        .init_resource::<PedRejected>()
        .init_resource::<PedNavSettings>()
        .init_resource::<PedObstacles>()
        // Registered by the plugin next to PedObstacles (PED_OBSTACLE logging).
        .init_resource::<PedObstacleTrace>()
        .init_resource::<Seen>()
        .add_message::<LivingWorldSpawn>()
        .add_message::<LivingWorldDespawn>()
        .add_message::<PedEvent>()
        .add_systems(Update, (step_population, apply_ped_records, release_rejected, advance_peds, collect).chain());
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / hz)));
    app.update();
    app
}

fn run(app: &mut App, seconds: f32, hz: f32) {
    for _ in 0..(seconds * hz).round() as u32 {
        app.update();
    }
}

fn spawn_at_tick(app: &mut App, r: SpawnRecord) {
    app.world_mut().write_message(LivingWorldSpawn(r));
}

fn peds(app: &mut App) -> Vec<(LivingWorldId, String, Vec3, f32, Locomotion, u64, [bool; 2], Option<u32>)> {
    let mut q = app.world_mut().query::<(&Pedestrian, &PedBody, &crate::world_audio::PedAudio)>();
    let mut v: Vec<_> = q.iter(app.world()).map(|(p, b, a)| (p.id, p.recipe.clone(), b.position, b.heading, b.player.state, b.ticks, a.feet_down, a.voice)).collect();
    v.sort_by_key(|x| x.0);
    v
}

#[test]
fn living_world_peds_spawn_with_the_seeded_look_and_despawn_cleanly() {
    let mut a = app(60.0);
    let tick = a.world().resource::<PopulationState>().world.tick();
    for i in 1..=6 {
        spawn_at_tick(&mut a, record(i, "aletown", 100 + i as u64, tick));
    }
    spawn_at_tick(&mut a, record(7, "empty", 1, tick));
    run(&mut a, 0.5, 60.0);
    let list = peds(&mut a);
    assert_eq!(list.len(), 6);
    let d = data();
    for (id, recipe, pos, ..) in &list {
        let look = d.catalog.choose("aletown", 100 + id.serial as u64, &PedOverrides::default()).unwrap();
        assert_eq!(recipe, &look.recipe, "look = f(category, seed)");
        assert!(pos.is_finite());
    }
    assert!(list.iter().all(|p| p.7 == Some(55) || p.7 == Some(60)), "voice from the model record");
    let seen = &a.world().resource::<Seen>().0;
    assert!(seen.iter().any(|e| matches!(e, PedEvent::Rejected { category, .. } if category == "empty")));
    assert_eq!(seen.iter().filter(|e| matches!(e, PedEvent::Spawned { .. })).count(), 6);
    // Despawn: the entity and its index entry go.
    let id = list[0].0;
    a.world_mut().write_message(LivingWorldDespawn(DespawnRecord { id, tick, reason: DespawnReason::Distance }));
    run(&mut a, 0.1, 60.0);
    assert_eq!(peds(&mut a).len(), 5);
    assert!(!a.world().resource::<PedIndex>().0.contains_key(&id));
    assert!(a.world().resource::<Seen>().0.iter().any(|e| matches!(e, PedEvent::Despawned { id: d, .. } if *d == id)));
}

#[test]
fn living_world_ped_motion_follows_from_the_record_and_tick_at_any_frame_rate() {
    let mut results = Vec::new();
    for hz in [60.0, 144.0] {
        let mut a = app(hz);
        let tick = a.world().resource::<PopulationState>().world.tick();
        spawn_at_tick(&mut a, record(1, "aletown", 77, tick));
        run(&mut a, 30.0, hz);
        let tick_now = a.world().resource::<PopulationState>().world.tick();
        let p = peds(&mut a).remove(0);
        // Rebuild from the record: same player, same number of console ticks.
        assert_eq!(p.5, tick_now - tick, "{hz} Hz");
        results.push((p.2, p.3, p.4, p.5));
    }
    // Same tick count = identical state (the two rates end on different ticks; compare a rebuild).
    let d = data();
    for (pos, heading, state, ticks) in results {
        let set = &d.anim_sets["default"];
        let mut body = PedBody {
            player: PedAnimPlayer::new(set, 77).unwrap(),
            path: skate_core::living_world::peds::anim::TestPath::new(77), nav: Default::default(), blocked: 0.0,
            position: Vec3::new(3.0, 0.0, 10.0),
            velocity: Vec3::ZERO,
            heading: 0.5,
            ticks: 0,
            feet_down: [false; 2],
            body_fall: 0.0,
            taunt: Default::default(),
            plugin_motion: Default::default(),
        };
        for _ in 0..ticks {
            body.player.intent = body.path.intent(tick_seconds(60.0), body.player.state);
            let out = body.player.step(tick_seconds(60.0), set, &d);
            body.position += Quat::from_rotation_y(body.heading) * Vec3::from_array(out.root.translation);
            body.heading += out.root.yaw;
        }
        assert_eq!((body.position, body.heading, body.player.state), (pos, heading, state));
        // It walked: the test path moves it a few metres from the spawn point.
        assert!(pos.distance(Vec3::new(3.0, 0.0, 10.0)) > 1.0, "moved {pos}");
    }
}

#[test]
fn living_world_peds_publish_foot_plants_while_walking() {
    let mut a = app(60.0);
    let tick = a.world().resource::<PopulationState>().world.tick();
    spawn_at_tick(&mut a, record(1, "aletown", 5, tick));
    let mut feet = std::collections::BTreeSet::new();
    let mut states = std::collections::BTreeSet::new();
    for _ in 0..(20 * 60) {
        a.update();
        let p = peds(&mut a).remove(0);
        feet.insert(p.6);
        states.insert(p.4);
    }
    assert!(states.contains(&Locomotion::Walk), "{states:?}");
    assert!(feet.contains(&[true, false]) && feet.contains(&[false, true]) && feet.contains(&[true, true]), "{feet:?}");
    assert!(a.world().resource::<Seen>().0.iter().any(|e| matches!(e, PedEvent::State { state: Locomotion::Walk, .. })));
}

#[test]
fn living_world_ped_mod_overrides_and_render_helpers() {
    let mut a = app(60.0);
    {
        let mut looks = a.world_mut().resource_mut::<PedLooks>();
        looks.overrides.category_entities.insert("aletown".into(), vec!["bum02".into()]);
        looks.glb.insert("male_bum_2".into(), "mods/x/bum.glb".into());
    }
    let tick = a.world().resource::<PopulationState>().world.tick();
    for i in 1..=4 {
        spawn_at_tick(&mut a, record(i, "aletown", i as u64, tick));
    }
    run(&mut a, 0.2, 60.0);
    assert!(peds(&mut a).iter().all(|p| p.1 == "male_bum_2"));
    let looks = a.world().resource::<PedLooks>().clone();
    assert_eq!(glb_path(&looks, "male_bum_2"), "mods/x/bum.glb");
    assert_eq!(glb_path(&PedLooks::default(), "male_jock_2"), "private/living_world/models/male_jock_2.glb");
    // LOD hysteresis on the 45 / 55 pair.
    assert_eq!(lod_for(40.0, 1, [45.0, 55.0]), 0);
    assert_eq!(lod_for(50.0, 0, [45.0, 55.0]), 0);
    assert_eq!(lod_for(50.0, 1, [45.0, 55.0]), 1);
    assert_eq!(lod_for(60.0, 0, [45.0, 55.0]), 1);
    // Followers keep the bind offset to their parent.
    let rig = data().rig;
    let bind: BTreeMap<usize, Mat4> = [(1, Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0))), (2, Mat4::from_translation(Vec3::new(0.0, 1.6, 0.1)))].into_iter().collect();
    let f = follower_offsets(&rig, &bind);
    assert_eq!(f.len(), 1);
    assert!((f[0].2.w_axis.truncate() - Vec3::new(0.0, 0.6, 0.1)).length() < 1e-6);
    let d = data();
    let body = PedBody {
        player: PedAnimPlayer::new(&d.anim_sets["default"], 1).unwrap(),
        path: skate_core::living_world::peds::anim::TestPath::new(1), nav: Default::default(), blocked: 0.0,
        position: Vec3::ZERO,
        velocity: Vec3::ZERO,
        heading: 0.0,
        ticks: 0,
        feet_down: [false; 2],
        body_fall: 0.0,
        taunt: Default::default(),
        plugin_motion: Default::default(),
    };
    let g = ped_globals(&d.rig, &body, &d, 0.0, &f).unwrap();
    assert!((g[1].w_axis.y - 0.92).abs() < 1e-5);
    assert!((g[2].w_axis.truncate() - Vec3::new(0.0, 1.52, 0.1)).length() < 1e-5, "{:?}", g[2].w_axis);
}

/// fix10 (peds rendered warped): the skin matrix is `global x basis x inverse(GLB bind)`, with the
/// basis read from the GLB's bind frames. Retail ped GLBs keep the rig's reference frames (basis
/// identity, data test `ped_glb_bind_frames_are_the_rig_reference_frames`); before the fix the
/// skater's bone-local basis was applied to them, twisting the torso -90 and the legs +90 degrees
/// about their own axes (180 apart at the hips: the pinched waist).
#[test]
fn ped_skin_rests_in_the_reference_pose_and_never_twists_bones() {
    let skater_basis = Mat4::from_cols(Vec4::X, -Vec4::Z, Vec4::Y, Vec4::W);
    // Reference globals with authored bone frames like the rig's: spine x up, leg x down.
    let spine = Mat4::from_rotation_translation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), Vec3::new(0.0, 1.0, 0.0));
    let leg = Mat4::from_rotation_translation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(0.2), Vec3::new(0.1, 0.9, 0.0));
    let reference = vec![Mat4::IDENTITY, spine, leg];
    let skin = |joint: Mat4, bind: Mat4| joint * skater_basis * bind.inverse(); // `pose_transforms` adds the skater basis
    let angle = |m: Mat4| 2.0 * Quat::from_mat4(&m).normalize().w.abs().min(1.0).acos().to_degrees();
    for (label, convention) in [("retail ped GLB", Mat4::IDENTITY), ("skater-style GLB", skater_basis)] {
        let glb_bind: BTreeMap<usize, Mat4> = [(1, reference[1] * convention), (2, reference[2] * convention)].into_iter().collect();
        let basis = ped_bone_basis(&reference, &glb_bind);
        assert!(basis.abs_diff_eq(convention, 1e-6), "{label}: basis {basis:?}");
        // The reference pose skins to the bind mesh (identity skin) on every bone.
        let joints = ped_joint_globals(&reference, basis);
        for i in [1, 2] {
            let m = skin(joints[i], glb_bind[&i]);
            assert!(m.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{label} bone {i}: {m:?}");
        }
        // A 30 degree knee-style bend on the leg only: the leg skin turns 30 degrees, the torso 0.
        let bent = vec![reference[0], reference[1], reference[2] * Mat4::from_rotation_y(30f32.to_radians())];
        let joints = ped_joint_globals(&bent, basis);
        assert!(angle(skin(joints[1], glb_bind[&1])) < 1e-2);
        assert!((angle(skin(joints[2], glb_bind[&2])) - 30.0).abs() < 1e-2, "{label}: {}", angle(skin(joints[2], glb_bind[&2])));
    }
    // The pre-fix path (skater basis forced on a retail ped GLB): every bone turned 90 degrees.
    let bind = reference[1];
    assert!((angle(skin(reference[1], bind)) - 90.0).abs() < 1e-2);
}

#[test]
fn living_world_ped_data_loads_from_the_export() {
    let Some(raw) = std::env::var_os("SKATE3_ASSET_ROOT") else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT (an asset root with stock data and living_world)");
        return;
    };
    let Some(root) = std::env::split_paths(&raw).find(|r| r.join(skate_data::ped_anim::PED_BANK).exists() && r.join("private/living_world/tables.json").exists()) else {
        eprintln!("skipped: no single root holds both the ped bank and living_world/tables.json");
        return;
    };
    let d = PedData::load(&root);
    eprintln!("{}", d.status);
    assert!(d.ready(), "{}", d.status);
    // The hand prop attach bone (b87 / b89; right hand: every carry channel is *RH).
    assert_eq!(d.rig.names.iter().position(|n| n.eq_ignore_ascii_case("RightHandProp")), Some(26));
    assert!(d.clips.len() > 50);
    let look = d.catalog.choose("aletown", 1, &PedOverrides::default()).unwrap();
    let set = &d.anim_sets[&look.anim_set];
    let body = PedBody { player: PedAnimPlayer::new(set, 1).unwrap(), path: skate_core::living_world::peds::anim::TestPath::new(1), nav: Default::default(), blocked: 0.0, position: Vec3::ZERO, velocity: Vec3::ZERO, heading: 0.0, ticks: 0, feet_down: [false; 2], body_fall: 0.0, taunt: Default::default(), plugin_motion: Default::default() };
    let g = ped_globals(&d.rig, &body, &d, 0.0, &[]).unwrap();
    assert!((0.8..1.1).contains(&g[1].w_axis.y), "hips height {}", g[1].w_axis.y);
    // The hand prop bone hangs off the right hand, a few centimetres away (rig reference local).
    assert_eq!(d.rig.parents[26], 12);
    let gap = g[26].w_axis.truncate().distance(g[12].w_axis.truncate());
    eprintln!("RIGHTHANDPROP to RIGHTHAND {gap:.3} m");
    assert!(gap < 0.2, "hand prop bone {gap} m from the hand");
}

/// Visual check helper (no window): with `SKATE_PED_POSE_DUMP=<file.json>` and the data roots,
/// write native model-space matrices (all 50 rig bones, column-major) for
/// `.claude/skills/living-world/tools/render_glb.py --pose`: for the `default` set and up to two
/// more sets (with a model each), poses mid-blend into start / walk / stop / idle / both turns, mid
/// walk, mid stop, idle and mid right / mirrored left turn. Each pose carries its label, set and model.
#[test]
fn living_world_ped_pose_dump_for_a_render_check() {
    use skate_core::living_world::peds::anim::Intent;
    let (Some(out), Some(raw)) = (std::env::var_os("SKATE_PED_POSE_DUMP"), std::env::var_os("SKATE3_ASSET_ROOT")) else { return };
    let Some(root) = std::env::split_paths(&raw).find(|r| r.join(skate_data::ped_anim::PED_BANK).exists() && r.join("private/living_world/tables.json").exists()) else { return };
    let d = PedData::load(&root);
    // Up to three looks with distinct anim sets ("default" first).
    let mut looks: Vec<(String, String)> = Vec::new();
    for category in d.catalog.categories.keys() {
        for seed in 0..40u64 {
            if let Some(look) = d.catalog.choose(category, seed, &PedOverrides::default()) {
                let Some((set, _)) = d.playable_set(&look.anim_set) else { continue };
                if !looks.iter().any(|(s, _)| s == set) && looks.len() < 3 {
                    looks.push((set.clone(), look.model.clone()));
                }
            }
        }
    }
    if let Some(i) = looks.iter().position(|(s, _)| s == "default") {
        looks.swap(0, i);
    }
    // Pose 0: the bank's reference pose (`PEDESTRIAN_RIG_TPOSE`, zero clip delta), the frame the
    // skinned mesh must rest in.
    let reference: Vec<Mat4> = skate_core::living_world::peds::anim::PedEvaluator::globals(&d.rig, &{ let mut r = d.rig.reference.clone(); r[0] = skate_core::living_world::peds::anim::IDENTITY; r }).into_iter().map(crate::animation::native_matrix).collect();
    let mut poses = vec![serde_json::json!({"label": "reference", "set": "", "model": "", "clip": "", "time": 0.0, "weight": 1.0, "mirrored": false,
        "bones": reference.iter().map(|m| m.to_cols_array().to_vec()).collect::<Vec<_>>()})];
    for (set_name, model) in &looks {
        let set = &d.anim_sets[set_name];
        let mut body = PedBody { player: PedAnimPlayer::new(set, 1).unwrap(), path: skate_core::living_world::peds::anim::TestPath::new(1), nav: Default::default(), blocked: 0.0, position: Vec3::ZERO, velocity: Vec3::ZERO, heading: 0.0, ticks: 0, feet_down: [false; 2], body_fall: 0.0, taunt: Default::default(), plugin_motion: Default::default() };
        // Scripted intents: idle 0.5 s, walk to 4 s, stop, turn right, then turn left.
        let mut captures: Vec<(u32, String)> = Vec::new();
        let mut turned = 0;
        let mut idle_since = 0u32;
        for t in 0..900u32 {
            let secs = t as f32 / 60.0;
            let state = body.player.state;
            body.player.intent = match state {
                Locomotion::Idle if (30..240).contains(&t) => Intent::Walk,
                Locomotion::Idle if t >= 240 && t - idle_since > 40 && turned == 0 => Intent::TurnRight,
                Locomotion::Idle if t >= 240 && t - idle_since > 40 && turned == 1 => Intent::TurnLeft,
                Locomotion::Start | Locomotion::Walk if secs < 4.0 => Intent::Walk,
                _ => Intent::Idle,
            };
            let o = body.player.step(tick_seconds(60.0), set, &d);
            if let Some(entered) = o.entered {
                let clip_len = d.clips.get(body.player.current_clip()).map_or(0.0, |c| c.length());
                let blend = match entered {
                    Locomotion::Start => skate_core::living_world::peds::anim::timing::START_BLEND,
                    Locomotion::Walk => skate_core::living_world::peds::anim::timing::WALK_BLEND,
                    Locomotion::Idle => skate_core::living_world::peds::anim::timing::IDLE_BLEND,
                    _ => skate_core::living_world::peds::anim::timing::TRANSITION_BLEND,
                };
                captures.push((t + ((blend * 0.5) * 60.0).round() as u32, format!("blend_into_{}", entered.name())));
                if entered != Locomotion::Start {
                    captures.push((t + ((clip_len * 0.5).min(1.5) * 60.0).round() as u32, format!("mid_{}", entered.name())));
                }
                if matches!(entered, Locomotion::TurnRight) {
                    turned = 1;
                }
                if matches!(entered, Locomotion::TurnLeft) {
                    turned = 2;
                }
                if entered == Locomotion::Idle {
                    idle_since = t;
                }
            }
            for (_, label) in captures.iter().filter(|(at, _)| *at == t) {
                let g = ped_globals(&d.rig, &body, &d, 0.0, &[]).unwrap();
                poses.push(serde_json::json!({"label": label, "set": set_name, "model": model, "clip": body.player.current_clip(),
                    "time": body.player.current_time(), "weight": body.player.blend_weight(), "mirrored": body.player.mirrored(),
                    "bones": g.iter().map(|m| m.to_cols_array().to_vec()).collect::<Vec<_>>()}));
            }
        }
    }
    // Symmetry of the reference pose under each mirror mode (0 = plain reflection everywhere).
    for mode in [0u32, 1] {
        let mut m = d.rig.reference.clone();
        skate_core::animation::pose_mirror::mirror(&mut m, &d.rig.parents, &d.rig.mirrors, mode).unwrap();
        let worst = m.iter().zip(&d.rig.reference).enumerate().filter(|(i, _)| *i > 0).map(|(i, (a, b))| {
            let dot: f32 = (0..4).map(|k| a.rotation[k] * b.rotation[k]).sum::<f32>().abs().min(1.0);
            (i, 2.0 * dot.acos().to_degrees())
        }).fold((0, 0.0f32), |w, x| if x.1 > w.1 { x } else { w });
        eprintln!("mirror mode {mode}: reference worst bone {} ({}) off by {:.2} deg", worst.0, d.rig.names[worst.0], worst.1);
    }
    let doc = serde_json::json!({"names": d.rig.names, "parents": d.rig.parents, "animated": d.rig.animated, "poses": poses});
    std::fs::write(out, serde_json::to_string(&doc).unwrap()).unwrap();
}

/// A flat 40 x 40 m navmesh of 2 m squares with a 6 x 6 m hole in the middle (M3).
fn plaza() -> skate_core::living_world::peds::NavMeshInput {
    use skate_core::living_world::peds::NavPolyInput;
    let hole = |i: i32, j: i32| (8..11).contains(&i) && (8..11).contains(&j);
    let index = |i: i32, j: i32| if (0..20).contains(&i) && (0..20).contains(&j) && !hole(i, j) { Some((j * 20 + i) as u32) } else { None };
    let mut polygons = Vec::new();
    for j in 0..20 {
        for i in 0..20 {
            let (x, z) = (i as f32 * 2.0, j as f32 * 2.0);
            let walkable = !hole(i, j);
            polygons.push(NavPolyInput {
                verts: vec![[x, 0.0, z], [x + 2.0, 0.0, z], [x + 2.0, 0.0, z + 2.0], [x, 0.0, z + 2.0]],
                neighbours: if walkable { vec![index(i, j - 1), index(i + 1, j), index(i, j + 1), index(i - 1, j)] } else { vec![None; 4] },
                area: if walkable { 0x11 } else { 0xF1 },
            });
        }
    }
    skate_core::living_world::peds::NavMeshInput { agent: [0.12, 0.35, 0.2, 1.6], polygons }
}

#[test]
fn living_world_peds_wander_on_the_navmesh_deterministically() {
    let mut finals = Vec::new();
    for _ in 0..2 {
        let mut a = app(60.0);
        {
            let mut d = a.world_mut().resource_mut::<PedData>();
            let input = plaza();
            d.nav = Some(Arc::new(skate_core::living_world::peds::NavMesh::build(&input, Default::default())));
            d.nav_input = Some(Arc::new(input));
        }
        let tick = a.world().resource::<PopulationState>().world.tick();
        for i in 1..=4 {
            let mut r = record(i, "aletown", 200 + i as u64, tick);
            r.position = [4.0 + 8.0 * i as f32, 0.0, 4.0];
            spawn_at_tick(&mut a, r);
        }
        let mut targets = 0;
        for _ in 0..(60 * 40) {
            a.update();
            let mut q = a.world_mut().query::<&PedBody>();
            let bodies: Vec<PedBody> = q.iter(a.world()).cloned().collect();
            let mesh = a.world().resource::<PedData>().nav.clone().unwrap();
            for b in &bodies {
                assert!(mesh.locate(b.position.to_array()).is_some(), "off the navmesh at {}", b.position);
                for o in &bodies {
                    let d = (b.position - o.position).length();
                    assert!(d == 0.0 || d >= 0.7 - 1e-3, "peds closer than twice the agent radius: {d}");
                }
            }
            targets = bodies.iter().map(|b| b.nav.targets_chosen).sum::<u32>();
        }
        assert!(targets >= 4, "every ped chose a wander target ({targets})");
        let list = peds(&mut a);
        assert!(list.iter().filter(|p| p.2.distance(Vec3::new(4.0 + 8.0 * p.0.serial as f32, 0.0, 4.0)) > 3.0).count() >= 3, "they walked: {list:?}");
        finals.push(list);
    }
    assert_eq!(finals[0], finals[1], "same records, same walk");
}

#[test]
fn living_world_ped_navmesh_loads_from_the_export() {
    let Some(raw) = std::env::var_os("SKATE3_ASSET_ROOT") else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT (an asset root with living_world/navmesh.bin)");
        return;
    };
    let Some(root) = std::env::split_paths(&raw).find(|r| r.join(skate_data::ped_nav::NAVMESH).exists()) else {
        eprintln!("skipped: no root holds living_world/navmesh.bin");
        return;
    };
    for (district, polygons) in [("DownTown", 36443), ("Industrial", 8693), ("University", 18147)] {
        let mut d = PedData::default();
        d.load_nav(&root, district, &Default::default());
        assert_eq!(d.nav.as_ref().map(|m| m.polys.len()), Some(polygons), "{district}: {}", d.status);
    }
    let mut d = PedData::default();
    d.load_nav(&root, "SomePark", &Default::default());
    assert!(d.nav.is_none() && d.status.contains("no navmesh"));
}

#[test]
fn living_world_peds_spawn_with_a_draw_fade_that_starts_transparent() {
    // Retail `sub_827C1188`: a spawned ped fades in (and fades out by camera distance before the
    // census cull); the opacity lives in the shared `NpcFade` so the NPC fade drawing shows it.
    let mut a = app(60.0);
    let tick = a.world().resource::<PopulationState>().world.tick();
    spawn_at_tick(&mut a, record(1, "aletown", 5, tick));
    run(&mut a, 0.1, 60.0);
    let mut q = a.world_mut().query::<(&Pedestrian, &super::npc_skaters::NpcFade)>();
    let fades: Vec<f32> = q.iter(a.world()).map(|(_, f)| f.alpha).collect();
    assert_eq!(fades, vec![0.0], "one ped, spawned transparent");
    assert_eq!(a.world().resource::<LivingWorldSettings>().ped_fade, skate_core::living_world::peds::PedFadeConfig::default());
}

/// Fix 11: props (and mod bodies) are navigation obstacles. Peds on the plaza with a field of bins
/// and a bench never step into one, and the walk is the same twice.
#[test]
fn living_world_peds_walk_round_props_deterministically() {
    use skate_core::living_world::peds::{NavObstacles, ObstacleInput, ObstacleParams};
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut props: Vec<ObstacleInput> = (0..12)
        .map(|k| ObstacleInput { id: k, center: [6.0 + (k % 4) as f32 * 8.0, 0.5, 9.0 + (k / 4) as f32 * 10.0], axes, half_extents: [0.35, 0.5, 0.35], velocity: [0.0; 3], inactive: false, held: false })
        .collect();
    props.push(ObstacleInput { id: 50, center: [20.0, 0.4, 7.0], axes, half_extents: [1.6, 0.4, 0.5], velocity: [0.0; 3], inactive: false, held: false });
    let mut finals = Vec::new();
    for _ in 0..2 {
        let mut a = app(60.0);
        {
            let mut d = a.world_mut().resource_mut::<PedData>();
            let input = plaza();
            d.nav = Some(Arc::new(skate_core::living_world::peds::NavMesh::build(&input, Default::default())));
            d.nav_input = Some(Arc::new(input));
        }
        let mut o = NavObstacles::new(ObstacleParams::default());
        o.update(&props);
        assert_eq!(o.cut_count(), props.len());
        a.world_mut().insert_resource(PedObstacles(o));
        let tick = a.world().resource::<PopulationState>().world.tick();
        for i in 1..=4 {
            let mut r = record(i, "aletown", 200 + i as u64, tick);
            r.position = [2.0 + 8.0 * i as f32, 0.0, 4.0];
            spawn_at_tick(&mut a, r);
        }
        for _ in 0..(60 * 40) {
            a.update();
            let obstacles = a.world().resource::<PedObstacles>().0.clone();
            let mut q = a.world_mut().query::<&PedBody>();
            for b in q.iter(a.world()) {
                for s in obstacles.states.values() {
                    let f = s.cut.unwrap();
                    assert!(!f.contains(b.position.to_array(), 0.0), "ped inside a prop at {}", b.position);
                }
            }
        }
        let list = peds(&mut a);
        assert!(list.iter().filter(|p| p.2.distance(Vec3::new(2.0 + 8.0 * p.0.serial as f32, 0.0, 4.0)) > 3.0).count() >= 3, "they walked: {list:?}");
        finals.push(list);
    }
    assert_eq!(finals[0], finals[1], "same records and props, same walk");
}

/// Fix 11: prop boxes and mod bodies become obstacle inputs with stable ids (mod bodies above
/// 2^40), the attached mod body counts as carried; the Lua tuning path sets and resets the rules.
#[test]
fn living_world_ped_obstacle_inputs_and_mod_tuning() {
    let solids = vec![(7u64, [1.0, 0.0, 2.0], [2.0, 1.0, 4.0], [0.0, 0.0, 0.0], false), (9u64, [0.0; 3], [1.0; 3], [0.0; 3], true)];
    let inputs = obstacle_inputs(None, &solids);
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].id, MOD_BODY_OBSTACLE_BASE | 7);
    assert_eq!(inputs[0].center, [1.5, 0.5, 3.0]);
    assert_eq!(inputs[0].half_extents, [0.5, 0.5, 1.0]);
    assert!(!inputs[0].inactive && !inputs[0].held && inputs[1].held && !inputs[1].inactive, "the attached body is held, not switched off");

    let mut world = World::new();
    world.insert_resource(LivingWorldSettings::default());
    let patch = serde_json::json!({"ped_obstacles": {"enabled": false, "moving_speed": 1.0}});
    crate::modding::world_tuning::set(&mut world, "mod_a", "living_world", Some(patch)).unwrap();
    let s = world.resource::<LivingWorldSettings>().ped_obstacles.clone();
    assert!(!s.enabled && s.moving_speed == 1.0 && s.min_half_extent == 0.2);
    assert!(s.held_is_obstacle && s.moving_solid, "retail / stand-in defaults kept");
    crate::modding::world_tuning::set(&mut world, "mod_a", "living_world", Some(serde_json::json!({"ped_obstacles": {"held_is_obstacle": false, "moving_solid": false}}))).unwrap();
    let s = world.resource::<LivingWorldSettings>().ped_obstacles.clone();
    assert!(!s.held_is_obstacle && !s.moving_solid);
    assert!(crate::modding::world_tuning::set(&mut world, "mod_a", "living_world", Some(serde_json::json!({"ped_obstacles": {"moving_speed": "fast"}}))).is_err());
    crate::modding::world_tuning::clear_owner(&mut world, "mod_a");
    assert_eq!(world.resource::<LivingWorldSettings>().ped_obstacles, skate_core::living_world::peds::ObstacleParams::default(), "mod disable restores retail");
}

#[test]
fn living_world_ped_tint_applies_to_ped_body_materials_only() {
    // material type from the export (retail recipe XML)
    assert!(ped_material_colorized(Some("Rostral_x"), Some(r#"{"shader":"pedestrian_high_stamp"}"#)));
    assert!(ped_material_colorized(Some("Hair_x"), Some(r#"{"shader":"pedestrian_high_stamp"}"#)));
    assert!(ped_material_colorized(Some("Rostral_x"), Some(r#"{"shader":"pedestrian_low"}"#)));
    assert!(!ped_material_colorized(Some("Hair_x"), Some(r#"{"shader":"marquee_hair"}"#)));
    assert!(!ped_material_colorized(Some("Accessory_x"), Some(r#"{"shader":"marquee_cloth"}"#)));
    assert!(!ped_material_colorized(Some("Rostral_x"), Some(r#"{"other":1}"#)));
    // exports without the type: the body slot
    assert!(ped_material_colorized(Some("Rostral_000030fd03e38817"), None));
    assert!(!ped_material_colorized(Some("Hair_000030fc03e38817"), None));
    assert!(!ped_material_colorized(None, None));
}

/// Floating peds (2026-10-06): the render ground probe took the first hit from one agent height
/// (1.6 m) above the navmesh, so overhead geometry lifted peds into the air. Count, over the
/// DownTown navmesh polygon centres, how far the drawn height would sit above the navmesh with
/// the old window and with the step-height window.
#[test]
#[ignore = "requires an installed DownTown map and living_world navmesh (SKATE3_ASSET_ROOT, SKATE3_MAP)"]
fn ped_render_ground_does_not_lift_onto_overhead_geometry() {
    let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let physics = crate::physics::GamePhysics::load_with_difficulty(&root, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    let mut d = PedData::default();
    d.load_nav(&root, "DownTown", &Default::default());
    let mesh = d.nav.clone().expect("DownTown navmesh");
    let (step, height) = (mesh.agent[2].max(0.05), mesh.agent[3].max(0.5));
    let (mut old_lifted, mut new_lifted, mut probed) = (0, 0, 0);
    for poly in &mesh.polys {
        let n = poly.verts.len() as f32;
        let c = poly.verts.iter().fold([0.0f32; 3], |a, v| [a[0] + v[0] / n, a[1] + v[1] / n, a[2] + v[2] / n]);
        let at = Vec3::new(c[0], c[1], c[2]);
        let old = ground(Some(&physics), at, height, height);
        let new = ground(Some(&physics), at, step, height);
        probed += 1;
        if old.is_some_and(|y| y - at.y > 0.3) { old_lifted += 1; }
        if new.is_some_and(|y| y - at.y > 0.3) { new_lifted += 1; }
    }
    eprintln!("PED_RENDER_GROUND polygons={probed} lifted_over_0.3m old={old_lifted} new={new_lifted} step={step} height={height}");
    assert_eq!(new_lifted, 0, "peds would still be drawn above the navmesh");
}

/// A ped body standing at `at` (no animation needed for the contact).
fn standing_body(at: Vec3) -> PedBody {
    let d = data();
    PedBody {
        player: PedAnimPlayer::new(&d.anim_sets["default"], 5).unwrap(),
        path: skate_core::living_world::peds::anim::TestPath::new(5),
        nav: Default::default(),
        blocked: 0.0,
        position: at,
        velocity: Vec3::ZERO,
        heading: 0.0,
        ticks: 0,
        feet_down: [false; 2],
        body_fall: 0.0,
        taunt: Default::default(),
        plugin_motion: Default::default(),
    }
}

#[test]
fn living_world_cars_push_peds_out_of_the_way_and_report_the_contact() {
    use super::vehicle_contacts::resolve;
    use skate_core::living_world::peds::{CarBox, NavMesh, VehicleContactParams};
    let mesh = NavMesh::build(&plaza(), Default::default());
    // A 1.8 x 1.5 x 4.4 m car at (5, 0, 5) facing +z at 8 m/s; peds: one 0.25 m into the front
    // bumper zone, one clear of the car.
    let car = CarBox { center: [5.0, 0.75, 5.0], forward: [0.0, 1.0], half: [0.9, 0.75, 2.2] };
    let cars = [(77u64, car, Vec3::new(0.0, 0.0, 8.0))];
    let run = |params: VehicleContactParams| {
        let (mut a, mut b) = (standing_body(Vec3::new(5.0, 0.0, 7.3)), standing_body(Vec3::new(9.0, 0.0, 5.0)));
        let mut peds = [(1u64, &mut a), (2u64, &mut b)];
        let events = resolve(&params, 40, Some(&mesh), &cars, &mut peds);
        (events, a.position, b.position)
    };
    // Retail: an IVehicle contact only moves the ped with its pushed body (no knock-down).
    let (events, a, b) = run(VehicleContactParams::default());
    assert_eq!(events.len(), 1, "{events:?}");
    let e = &events[0];
    assert_eq!((e.car, e.ped, e.tick, e.reaction.as_str()), (77, 1, 40, "push"));
    assert!((e.depth - 0.25).abs() < 1e-4, "agent radius 0.35 [data] minus 0.1 m gap");
    assert!((e.car_speed - 8.0).abs() < 1e-5 && (e.closing - 8.0).abs() < 1e-5);
    assert!((a - Vec3::new(5.0, 0.0, 7.55)).length() < 1e-4, "pushed off the bumper: {a}");
    assert_eq!(b, Vec3::new(9.0, 0.0, 5.0), "a ped clear of the car is untouched");
    // Same inputs, same result (deterministic), and the event serialises for a host.
    let (again, a2, _) = run(VehicleContactParams::default());
    assert_eq!((again, a2), (events.clone(), a));
    let wire = serde_json::to_string(&events[0]).unwrap();
    assert_eq!(serde_json::from_str::<super::vehicle_contacts::VehicleContactEvent>(&wire).unwrap(), events[0]);
    // Mod options: report only (no push), or no contact at all.
    let (events, a, _) = run(VehicleContactParams { enabled: true, push: false });
    assert_eq!((events.len(), events[0].reaction.as_str(), a), (1, "reported", Vec3::new(5.0, 0.0, 7.3)));
    let (events, a, _) = run(VehicleContactParams { enabled: false, push: true });
    assert!(events.is_empty() && a == Vec3::new(5.0, 0.0, 7.3));
}

#[test]
fn living_world_car_box_matches_the_car_proxy() {
    use super::vehicle_contacts::car_box;
    let car = super::vehicles::TrafficCar {
        id: LivingWorldId { kind: Kind::Vehicle, serial: 3 },
        entity: String::new(),
        model: String::new(),
        glb: String::new(),
        chassis_id: String::new(),
        secondary_id: String::new(),
        chassis: [1.0; 4],
        secondary: [1.0; 4],
        engine: String::new(),
        paint_gain: 1.0,
        wheel_radius: 0.3,
        bounds: [[-1.0, 0.0, -2.0], [1.0, 1.4, 2.6]],
        spawn_tick: 0,
    };
    let pose = Transform::from_translation(Vec3::new(10.0, 2.0, 3.0)).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
    let b = car_box(&car, &pose);
    // Facing +x after a quarter turn; the bounds' 0.3 m forward offset moves the centre along +x.
    assert!((b.forward[0] - 1.0).abs() < 1e-5 && b.forward[1].abs() < 1e-5);
    assert!((b.center[0] - 10.3).abs() < 1e-5 && (b.center[1] - 2.7).abs() < 1e-5 && (b.center[2] - 3.0).abs() < 1e-5);
    assert_eq!(b.half, [1.0, 0.7, 2.3]);
}

/// A skater running into a ped (retail `sub_82E38FB8` kind 5, doc 26 "Skater hits peds"): fast =
/// knock-down (fall, ground, get-up), slow = standing stumble; the ped reacts once, then walks on.
#[test]
fn living_world_a_skater_knocks_a_ped_down_or_makes_it_stumble() {
    use skate_core::living_world::peds::skater_contact::ReactionKind;
    use skate_core::living_world::Observer;
    for (speed, expected) in [(6.0f32, ReactionKind::Knockdown), (1.0, ReactionKind::Standing)] {
        let mut a = app(60.0);
        spawn_at_tick(&mut a, record(1, "aletown", 9, 0));
        run(&mut a, 0.2, 60.0);
        let at = peds(&mut a)[0].2;
        a.world_mut().resource_mut::<LivingWorldObservers>().observers = vec![Observer { position: [at.x - 0.5, at.y, at.z], velocity: [speed, 0.0, 0.0] }];
        run(&mut a, 0.1, 60.0);
        let hits: Vec<_> = a.world().resource::<Seen>().0.iter().filter_map(|e| if let PedEvent::Hit { kind, closing, .. } = e { Some((*kind, *closing)) } else { None }).collect();
        assert_eq!(hits.len(), 1, "one reaction while touching: {hits:?}");
        assert_eq!(hits[0].0, expected, "at {speed} m/s");
        assert_eq!(peds(&mut a)[0].4, Locomotion::Reaction);
        // The skater leaves; the reaction ends and the ped is back to locomotion.
        a.world_mut().resource_mut::<LivingWorldObservers>().observers.clear();
        run(&mut a, 5.0, 60.0);
        assert_ne!(peds(&mut a)[0].4, Locomotion::Reaction);
    }
}

/// Ped behaviour runtime step 1 (data-gated): the stock ped AI and motion graphs load, bind and
/// compile on the shared graph runtime; the AI graph runs on the ped brain host, wanders on its
/// own and leaves wander for flee when a flee want is raised.
#[test]
fn living_world_ped_ai_graph_runs_on_the_brain() {
    use skate_core::graph::controller::Controller;
    use skate_core::living_world::peds::brain::{motion, BrainHost, BrainSettings, PedBrain};
    let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from).filter(|r| r.join(super::ped_graph::AI_GRAPH).exists()) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let ai = super::ped_graph::PedGraph::load(&root, super::ped_graph::AI_GRAPH).expect("ped AI graph");
    crate::graph_runtime::load_graph(&root, super::ped_graph::MOTION_GRAPH).expect("ped motion graph");
    let pending = ai.pending();
    eprintln!("ped AI graph: {} behaviours, {} conditions, pending {:?}", ai.behaviors.len(), ai.conditions.len(), pending);
    let program = &ai.graph.runtime.program;
    let mut controller = Controller::new(program.topology.states.len());
    let mut brain = PedBrain::default();
    let settings = BrainSettings::default();
    let skater = std::cell::Cell::new([5.0f32, 0.0, 0.0]);
    let at = |_: u64| Some(skater.get());
    let state_name = |c: &Controller| c.frame.current.map(|s| ai.graph.binding.states[s].name.clone());
    for _ in 0..30 {
        brain.tick_timers(1.0 / 30.0);
        let mut host = BrainHost { behaviors: &ai.behaviors, conditions: &ai.conditions, brain: &mut brain, settings: &settings, position: [0.0; 3], heading: 0.0, skater: None, target_position: &at, chase: Default::default() };
        controller.update(program, 1.0 / 30.0, &mut host);
    }
    eprintln!("idle state {:?}, intent {:?}", state_name(&controller), brain.motion_intent);
    assert_eq!(brain.motion_intent, Some(motion::WANDER), "an idle ped wanders");
    brain.set_want("flee", 1);
    for _ in 0..30 {
        brain.tick_timers(1.0 / 30.0);
        let mut host = BrainHost { behaviors: &ai.behaviors, conditions: &ai.conditions, brain: &mut brain, settings: &settings, position: [0.0; 3], heading: 0.0, skater: None, target_position: &at, chase: Default::default() };
        controller.update(program, 1.0 / 30.0, &mut host);
    }
    eprintln!("flee state {:?}, intent {:?}, flee_from {:?}", state_name(&controller), brain.motion_intent, brain.flee_from);
    assert_eq!(brain.motion_intent, Some(motion::FLEE), "a flee want makes it flee");
    // Farther than 15 m from the skater: the flee ends (DistanceToWantTarget greater 15).
    skater.set([20.0, 0.0, 0.0]);
    for _ in 0..30 {
        let mut host = BrainHost { behaviors: &ai.behaviors, conditions: &ai.conditions, brain: &mut brain, settings: &settings, position: [0.0; 3], heading: 0.0, skater: None, target_position: &at, chase: Default::default() };
        controller.update(program, 1.0 / 30.0, &mut host);
    }
    eprintln!("after flee state {:?}, intent {:?}, wants {:?}", state_name(&controller), brain.motion_intent, brain.wants);
    assert_eq!(brain.flee_from, None);
    // A warn want: the ped stops and faces the skater (StopAndFaceWantTarget / watching).
    skater.set([3.0, 0.0, 0.0]);
    brain.set_want("warn", 1);
    let mut faced = false;
    for _ in 0..30 {
        brain.tick_timers(1.0 / 30.0);
        let mut host = BrainHost { behaviors: &ai.behaviors, conditions: &ai.conditions, brain: &mut brain, settings: &settings, position: [0.0; 3], heading: 0.0, skater: Some(([3.0, 0.0, 0.0], [0.0; 3])), target_position: &at, chase: Default::default() };
        controller.update(program, 1.0 / 30.0, &mut host);
        faced |= brain.face == Some([3.0, 0.0, 0.0]) && brain.speed_suggestion == Some(0.0);
    }
    eprintln!("warn state {:?}, face {:?}, speed {:?}, wants {:?}", state_name(&controller), brain.face, brain.speed_suggestion, brain.wants);
    assert!(faced, "warning stops and faces the skater");
}

/// Zombie mode on the stock ped graph (data-gated, b95): a wandering ped enters ZombieFollow (`pedestrian_zombiefollow.xml`),
/// plays the `zombie` anim set, drops its hand prop and heads for the player; with the mode off it wanders again.
#[test]
fn living_world_zombie_mode_runs_zombie_follow_on_the_stock_graph() {
    use skate_core::graph::controller::Controller;
    use skate_core::living_world::peds::brain::{motion, BrainHost, BrainSettings, PedBrain};
    let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from).filter(|r| r.join(super::ped_graph::AI_GRAPH).exists()) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let ai = super::ped_graph::PedGraph::load(&root, super::ped_graph::AI_GRAPH).expect("ped AI graph");
    let program = &ai.graph.runtime.program;
    let mut controller = Controller::new(program.topology.states.len());
    let mut brain = PedBrain { rng: Some(skate_core::living_world::Rng::new(5)), ..Default::default() };
    brain.hand_prop.holding = true;
    let settings = BrainSettings::default();
    let none = |_: u64| None;
    let player = [30.0f32, 0.0, 0.0];
    let state_name = |c: &Controller| c.frame.current.map(|s| ai.graph.binding.states[s].name.clone());
    let run = |controller: &mut Controller, brain: &mut PedBrain, zombie: bool| {
        brain.zombie = zombie;
        for _ in 0..30 {
            brain.tick_timers(1.0 / 30.0);
            let mut host = BrainHost { behaviors: &ai.behaviors, conditions: &ai.conditions, brain, settings: &settings, position: [0.0; 3], heading: 0.0, skater: Some((player, [0.0; 3])), target_position: &none, chase: Default::default() };
            controller.update(program, 1.0 / 30.0, &mut host);
        }
    };
    run(&mut controller, &mut brain, false);
    assert_eq!(brain.motion_intent, Some(motion::WANDER));
    run(&mut controller, &mut brain, true);
    eprintln!("zombie state {:?}, intent {:?}, goal {:?}, speed {:?}, anim {:?}", state_name(&controller), brain.motion_intent, brain.zombie_goal, brain.speed_suggestion, brain.anim_override);
    assert_eq!((brain.motion_intent, brain.zombie_goal, brain.speed_suggestion), (Some(motion::ZOMBIE_FOLLOW), Some(player), Some(8.0)));
    assert_eq!(brain.anim_override.as_deref(), Some("zombie"));
    assert!(!brain.hand_prop.holding, "DropHandProp on entry");
    run(&mut controller, &mut brain, false);
    eprintln!("after state {:?}, intent {:?}", state_name(&controller), brain.motion_intent);
    assert_eq!((brain.motion_intent, brain.anim_override.as_deref()), (Some(motion::WANDER), None));
}

/// Mood system on the stock tables (data-gated): an adult male hit by the skater does nothing on
/// the first hit, warns on the second (Nth 2) and chases on the third (Nth 3).
#[test]
fn living_world_ped_mood_tables_warn_then_chase_on_collisions() {
    use skate_core::living_world::peds::mood::{MoodContext, MoodEvent, MoodStore};
    let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from).filter(|r| r.join("private/living_world/tables.json").exists()) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let bytes = std::fs::read(root.join("private/living_world/tables.json")).unwrap();
    let tables = super::ped_mood::parse(&bytes).expect("mood tables");
    let sets = super::ped_mood::reaction_sets(&bytes);
    eprintln!("{} categories, {} results, {} reaction sets, {} entity types", tables.categories.len(), tables.results.len(), tables.reactions.len(), sets.len());
    assert_eq!(tables.results.len(), 88);
    assert_eq!(sets.get("adult_male01").map(String::as_str), Some("adult_male"));
    assert_eq!(tables.categories["presence"].magnitude, 0.5);
    assert_eq!(tables.categories["collision"].magnitude, 0.0);
    const PED: u64 = 1;
    const PLAYER: u64 = 2;
    let entity = |id: u64| match id {
        PED => Some(("adult_male01".to_string(), [0.0; 3])),
        PLAYER => Some(("skater".to_string(), [1.0, 0.0, 0.0])),
        _ => None,
    };
    let ctx = MoodContext { ped: PED, ped_type: "adult_male", position: [0.0; 3], entity: &entity, zombie: false, busy: &|_| false };
    let mut store = MoodStore::default();
    let mut rng = skate_core::living_world::Rng::new(5);
    let hit = || MoodEvent { category: "collision".into(), instigator: Some(PLAYER), second: Some(PED), position: [0.0; 3] };
    store.post(hit(), 0.0);
    let first = tables.produce(&mut store, &ctx, false, &|_| 0, &|_| false, &mut rng);
    eprintln!("first hit: {first:?}");
    assert!(first.is_none_or(|r| !r.wants.iter().any(|w| w.want == "warn" || w.want == "angrychase")));
    store.post(hit(), 0.0);
    let second = tables.produce(&mut store, &ctx, false, &|_| 0, &|_| false, &mut rng).expect("a reaction to the second hit");
    eprintln!("second hit: {second:?}");
    assert_eq!(second.result, "skatercollisionwarn");
    assert!(second.wants.iter().any(|w| w.want == "warn" && w.target == PLAYER));
    store.tick(10.0, &|_| 30.0);
    store.post(hit(), 0.0);
    let third = tables.produce(&mut store, &ctx, false, &|_| 1, &|_| false, &mut rng).expect("a reaction to the third hit");
    eprintln!("third hit: {third:?}");
    assert_eq!(third.result, "skatercollisionchase");
    // A bystander records nearbycollision, not collision (the "ped itself" prerequisite).
    let other = MoodEvent { second: Some(77), ..hit() };
    let mut bystander = MoodStore::default();
    for _ in 0..3 {
        bystander.post(other.clone(), 0.0);
    }
    let r = tables.produce(&mut bystander, &ctx, false, &|_| 0, &|_| false, &mut rng);
    assert!(r.as_ref().is_none_or(|r| r.result != "skatercollisionwarn" && r.result != "skatercollisionchase"), "{r:?}");
}

/// The stock plugin graphs (conversation, sit, lookat, spectate) load on the same runtime as the
/// ped AI graph (b23); prints what each still needs.
#[test]
fn living_world_ped_plugin_graphs_load() {
    let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from).filter(|r| r.join(super::ped_graph::AI_GRAPH).exists()) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    for name in ["conversation", "sit", "lookat", "actortracker_spectate"] {
        let path = format!("private/stock/data/state/livingworldentities/pedestrian/plugin/{name}.stategraph");
        let g = super::ped_graph::PedGraph::load(&root, &path).unwrap_or_else(|e| panic!("{name}: {e}"));
        eprintln!("plugin {name}: {} behaviours, {} conditions, pending {:?}", g.behaviors.len(), g.conditions.len(), g.pending());
    }
}

/// Where the placed plugin waypoints sit against the navmesh: the horizontal gap from each waypoint to its located
/// navmesh point (a waypoint off the mesh cannot be reached by the route; the template's slide covers 1 m).
#[test]
#[ignore = "requires the installed living-world export (SKATE3_ASSET_ROOT)"]
fn placed_plugin_waypoints_against_the_navmesh() {
    let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let plugins = super::ped_plugins::PluginData::load(&root).unwrap();
    for (district, props) in &plugins.placed {
        let mut d = PedData::default();
        d.load_nav(&root, district, &Default::default());
        let Some(mesh) = d.nav.clone() else { continue };
        for p in props {
            for w in &p.waypoints {
                let gap = mesh.locate(w.position).map(|n| ((n.position[0] - w.position[0]).powi(2) + (n.position[2] - w.position[2]).powi(2)).sqrt());
                eprintln!("PLUGIN_WP {district} {} {:016X} at [{:.2}, {:.2}, {:.2}] mesh gap {:?}", p.class, p.id, w.position[0], w.position[1], w.position[2], gap.map(|g| (g * 100.0).round() / 100.0));
            }
        }
    }
}
