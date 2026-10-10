//! Headless, seeded tests of the replay-tier NPC skaters: a fake player drives past synthetic
//! lines, the population spawns NPCs, the NPC systems ride them; positions, despawns, audio and
//! determinism are checked. No window, no assets (one data-gated test reads the stock clips).

use super::npc_skaters::*;
use super::*;
use skate_core::living_world::replay::{LineCursor, ReplayLine, ReplayNode, ReplayPhase};
use skate_core::living_world::{SkaterCharacter, SkaterLine};
use std::collections::BTreeMap;
use std::sync::Arc;

const NODES: u32 = 300;
const FRAMES: u8 = 4;
const STEP: f32 = 0.5; // 0.5 m per 4 frames = 7.5 m/s

fn id(i: u32) -> [u8; 16] {
    let mut id = [0u8; 16];
    id[0] = i as u8;
    id[1] = 0xA1;
    id
}

/// Lines every 25 m beside the player's road (z = 70), each riding +x for 20 s.
fn data() -> LoadedData {
    let config = PopulationConfig::retail();
    let starts: Vec<[f32; 3]> = (0..80).map(|i| [-1000.0 + i as f32 * 25.0, 0.0, 70.0]).collect();
    let lines = starts
        .iter()
        .enumerate()
        .map(|(i, s)| SkaterLine { id: id(i as u32), start: *s, heading: std::f32::consts::FRAC_PI_2, valid: true, allowed_skaters: u64::MAX, flags: 4 })
        .collect();
    let replay: BTreeMap<[u8; 16], ReplayLine> = starts
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let nodes = (0..NODES)
                .map(|n| ReplayNode {
                    position: [s[0] + n as f32 * STEP, s[1], s[2]],
                    step: [STEP / FRAMES as f32, 0.0, 0.0],
                    // Heading +x: 90 deg about +y.
                    board: [128, 218, 128, 218],
                    skater: [128, 218, 128, 218],
                    frames: if n == 0 { 0 } else { FRAMES },
                    event: 0,
                    flags: if (100..110).contains(&n) { skate_core::living_world::replay::node_flags::AIRBORNE } else { 0 },
                    jump: None,
                    width: [50, 50],
                })
                .collect();
            (id(i as u32), ReplayLine { id: id(i as u32), flags: 4, skill: 0, nodes, jumps: vec![], groups: vec![] })
        })
        .collect();
    let characters = (0..6).map(|i| SkaterCharacter { key: format!("pro_{i}"), pro_index: Some(i), capabilities: [false; 3], community: false }).collect();
    let voices = (0..6).map(|i| (format!("pro_{i}"), 10 + i)).collect();
    LoadedData {
        config,
        census: None,
        skaters: Some(SkaterData { lines, characters }),
        roads: None,
        vehicles: None,
        // Two characters carry a stance-table record (regular / goofy); the rest are unmapped.
        npc: NpcData { lines: Arc::new(replay), voices, records: [("pro_0".to_owned(), "chris_cole".to_owned()), ("pro_1".to_owned(), "josh_kalis".to_owned())].into(), tricks: Default::default() },
        status: "npc test".into(),
    }
}

#[derive(Resource)]
struct Drive {
    t: f32,
    speed: f32,
}

/// A fixed player position instead of the drive (avoider tests).
#[derive(Resource, Default)]
struct Pin(Option<[f32; 3]>);

fn drive(time: Res<Time>, mut d: ResMut<Drive>, mut obs: ResMut<LivingWorldObservers>, pin: Option<Res<Pin>>) {
    if let Some(p) = pin.and_then(|p| p.0) {
        obs.observers = vec![Observer { position: p, velocity: [0.0; 3] }];
        obs.player_slots = 1;
        return;
    }
    d.t += time.delta_secs();
    let x = -900.0 + d.t * d.speed;
    obs.observers = vec![Observer { position: [x, 0.0, 0.0], velocity: [d.speed, 0.0, 0.0] }];
    obs.player_slots = 1;
}

#[derive(Resource, Default)]
struct Seen(Vec<NpcSkaterEvent>);

fn collect(mut ev: MessageReader<NpcSkaterEvent>, mut seen: ResMut<Seen>) {
    seen.0.extend(ev.read().cloned());
}

fn app(seed: u64) -> App {
    app_with(seed, data())
}

fn app_with(seed: u64, data: LoadedData) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let settings = LivingWorldSettings { seed, ..LivingWorldSettings::default() };
    let mut state = PopulationState::default();
    state.install("Test", 1, &settings, data);
    app.insert_resource(settings).insert_resource(state).init_resource::<LivingWorldObservers>().init_resource::<NpcSkaterIndex>().init_resource::<Seen>();
    app.insert_resource(Drive { t: 0.0, speed: 8.0 });
    app.add_message::<LivingWorldSpawn>().add_message::<LivingWorldDespawn>().add_message::<NpcSkaterEvent>();
    app.add_systems(Update, (drive, step_population, apply_records, advance, track_stance, collect).chain());
    app
}

fn run(app: &mut App, seconds: f32, hz: f32) {
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / hz)));
    app.update();
    for _ in 0..(seconds * hz) as u32 {
        app.update();
    }
}

/// (id, position, character, voice, audio velocity) of every NPC entity, by id.
fn npcs(app: &mut App) -> Vec<(LivingWorldId, [f32; 3], String, Option<u32>, Option<[f32; 3]>, u64, [u8; 16])> {
    let mut q = app.world_mut().query::<(&NpcSkater, &NpcReplay, &Transform, &crate::world_audio::NpcSkaterAudio)>();
    let mut v: Vec<_> = q
        .iter(app.world())
        .map(|(n, r, t, a)| (n.id, t.translation.to_array(), n.character.clone(), a.voice, a.state.as_ref().map(|s| s.board_velocity), r.cursor.frames, n.start_line))
        .collect();
    v.sort_by_key(|x| x.0);
    v
}

#[test]
fn living_world_npc_skaters_ride_their_lines_from_the_spawn_record() {
    let mut a = app(11);
    // The player outruns the NPCs (16 vs 7.5 m/s), so some fall behind past the 120 m cull.
    a.world_mut().resource_mut::<Drive>().speed = 16.0;
    let mut max = 0;
    let mut checked = 0;
    for _ in 0..40 {
        run(&mut a, 1.0, 60.0);
        let tick = a.world().resource::<PopulationState>().world.tick();
        let list = npcs(&mut a);
        let st = a.world().resource::<PopulationState>();
        // One entity per live population skater, same ids; positions written back.
        let live: Vec<_> = st.world.live(Kind::Skater).map(|l| (l.id, l.position)).collect();
        assert_eq!(live.iter().map(|l| l.0).collect::<Vec<_>>(), list.iter().map(|n| n.0).collect::<Vec<_>>());
        max = max.max(list.len());
        let lines = st.npc.lines.clone();
        let mut rq = a.world_mut().query::<(&NpcSkater, &NpcReplay)>();
        let records: BTreeMap<LivingWorldId, (Vec<skate_core::living_world::replay::BranchRecord>, Vec<skate_core::living_world::replay::TrickRecord>)> = rq.iter(a.world()).map(|(n, r)| (n.id, (r.branches.clone(), r.tricks.clone()))).collect();
        for (nid, pos, character, voice, vel, frames, start) in &list {
            // The state is a function of the spawn record, the tick and the host's line choices
            // (branches and line-end chains, fix 9): rebuild the cursor as a client would.
            let spawn_tick = a.world().resource::<Seen>().0.iter().find_map(|e| match e {
                NpcSkaterEvent::Spawned { id, .. } if id == nid => Some(()),
                _ => None,
            });
            assert!(spawn_tick.is_some());
            let mut c = LineCursor::spawn(&*lines, *start, 0);
            c.advance(*frames as u32, &*lines, &mut skate_core::living_world::replay::Decider::Mirror(&records[nid].0, &records[nid].1), &mut Vec::new());
            let s = c.sample(&*lines, 0.0).unwrap();
            assert_eq!(s.position, *pos, "npc {nid:?} at tick {tick}");
            assert_eq!(live.iter().find(|l| l.0 == *nid).unwrap().1, *pos);
            // Speed 7.5 m/s along +x, published to the audio.
            let v = vel.unwrap();
            assert!((v[0] - 7.5).abs() < 1e-3 && v[2].abs() < 1e-3, "{v:?}");
            assert_eq!(*voice, Some(10 + character[4..].parse::<u32>().unwrap()));
            checked += 1;
        }
    }
    assert_eq!(max, 3, "retail keeps 3 ambient NPC skaters");
    assert!(checked > 60);
    let seen = &a.world().resource::<Seen>().0;
    // Lines are 20 s long and each ends on the start of the line 150 m on: NPCs chain from line
    // to line (fix 9) and leave by the population's distance cull; air nodes raised no events
    // (flags only), entities are gone with their records.
    let despawned: Vec<_> = seen.iter().filter_map(|e| if let NpcSkaterEvent::Despawned { id, .. } = e { Some(*id) } else { None }).collect();
    assert!(!despawned.is_empty());
    let alive = npcs(&mut a);
    assert!(despawned.iter().all(|d| !alive.iter().any(|n| n.0 == *d)));
    assert_eq!(a.world().resource::<NpcSkaterIndex>().0.len(), alive.len());
}

/// Natural stance port: every NPC gets its stance once at spawn from its character record (the
/// retail table: `chris_cole` regular, `josh_kalis` goofy, unmapped records the goofy default); a
/// mod override by record name or id wins for later spawns and is gone after the reset.
#[test]
fn living_world_npc_skaters_take_the_natural_stance_of_their_record() {
    use skate_core::living_world::stance::NaturalStance as S;
    let stances = |a: &mut App| {
        let mut q = a.world_mut().query::<&NpcSkater>();
        q.iter(a.world()).map(|n| (n.character.clone(), n.stance)).collect::<Vec<_>>()
    };
    let mut seen = BTreeMap::new();
    let mut a = app(11);
    for _ in 0..30 {
        run(&mut a, 1.0, 60.0);
        for (c, s) in stances(&mut a) {
            assert_eq!(*seen.entry(c.clone()).or_insert(s), s, "{c}: the stance never changes");
        }
    }
    eprintln!("stances by character: {seen:?}");
    assert!(seen.len() >= 2, "several characters spawned");
    for (c, s) in &seen {
        assert_eq!(*s, if c == "pro_0" { S::Regular } else { S::Goofy }, "{c}");
    }
    // Overrides: by record name for the unmapped keys, by record id for josh_kalis (pro_1).
    let mut b = app(11);
    {
        let mut st = b.world_mut().resource_mut::<LivingWorldSettings>();
        for i in 2..6 {
            st.skater_stance.insert(format!("pro_{i}"), S::Regular);
        }
        st.skater_stance.insert("CD56C7FE01EBE665".into(), S::Regular);
        st.skater_stance.insert("chris_cole".into(), S::Goofy);
    }
    let mut overridden = BTreeMap::new();
    for _ in 0..30 {
        run(&mut b, 1.0, 60.0);
        overridden.extend(stances(&mut b));
    }
    assert_eq!(overridden.keys().collect::<Vec<_>>(), seen.keys().collect::<Vec<_>>(), "same spawns: the stance does not change population decisions");
    for (c, s) in &overridden {
        assert_eq!(*s, if c == "pro_0" { S::Goofy } else { S::Regular }, "{c} overridden");
    }
    b.world_mut().resource_mut::<LivingWorldSettings>().reset_mod_overrides();
    assert!(b.world().resource::<LivingWorldSettings>().skater_stance.is_empty());
    // Every NPC carries its stance bits from the natural stance; without the stock banks no clip
    // fires an event, so the bits stay natural.
    let mut q = b.world_mut().query::<(&NpcSkater, &NpcStanceTrack)>();
    let tracks: Vec<_> = q.iter(b.world()).map(|(n, t)| (n.stance, t.clone())).collect();
    assert!(!tracks.is_empty());
    for (s, t) in &tracks {
        let natural = skate_core::living_world::stance::StanceFlags::natural(*s);
        assert_eq!((t.natural, t.flags), (natural, natural));
    }
    // (an NPC spawned this tick has no phase yet)
    assert!(tracks.iter().any(|(_, t)| t.top.is_some()), "the tracker follows the newest layer");
    let records = b.world().resource::<PopulationState>().npc.records.clone();
    assert_eq!(npc_stance(&records, &BTreeMap::new(), "pro_0"), S::Regular, "reset = retail table");
}

#[test]
fn living_world_npc_skaters_same_at_any_engine_rate() {
    let mut x = app(23);
    run(&mut x, 15.0, 60.0);
    let mut y = app(23);
    run(&mut y, 15.0, 144.0);
    let tick = |a: &App| a.world().resource::<PopulationState>().world.tick();
    // 2160 steps of a rounded 1/144 s can end a hair short of tick 450: let it catch up.
    for _ in 0..4 {
        if tick(&y) < tick(&x) {
            y.update();
        }
    }
    assert_eq!(tick(&x), tick(&y));
    let (a, b) = (npcs(&mut x), npcs(&mut y));
    assert!(!a.is_empty());
    assert_eq!(a.iter().map(|n| (n.0, n.1, n.5)).collect::<Vec<_>>(), b.iter().map(|n| (n.0, n.1, n.5)).collect::<Vec<_>>());
}

#[test]
fn living_world_npc_skaters_despawn_with_the_population() {
    let mut a = app(5);
    run(&mut a, 10.0, 60.0);
    assert!(!npcs(&mut a).is_empty());
    a.world_mut().resource_mut::<LivingWorldSettings>().skaters.enabled = false;
    run(&mut a, 1.0, 60.0);
    assert!(npcs(&mut a).is_empty());
    assert!(a.world().resource::<NpcSkaterIndex>().0.is_empty());
    let mut q = a.world_mut().query::<&NpcSkater>();
    assert_eq!(q.iter(a.world()).count(), 0);
}

#[test]
fn living_world_npc_proxy_audio_and_clips() {
    let lines = data().npc.lines;
    let mut c = LineCursor::spawn(&*lines, id(0), 0);
    c.advance(4 * 102, &*lines, &mut skate_core::living_world::replay::Decider::Stay, &mut Vec::new());
    let s = c.sample(&*lines, 0.0).unwrap();
    assert_eq!(s.phase, ReplayPhase::Air);
    let st = lite_state(&s, 3);
    assert!(st.airborne && st.wheel_count == 0);
    let nid = LivingWorldId { kind: Kind::Skater, serial: 7 };
    let [body, board] = proxy(nid, &s);
    assert_eq!(body.id, PROXY_ID_TAG | nid.to_u64());
    assert_eq!(board.id, PROXY_ID_TAG | PROXY_BOARD_BIT | nid.to_u64());
    // Retail groups: skater skeleton 5 (the player's skater-contact scaling), board 4.
    assert_eq!((body.contact_group, board.contact_group), (5, 4));
    for p in [&body, &board] {
        assert_eq!(p.inverse_mass, 0.0);
        assert!((p.linvel.x - 7.5).abs() < 1e-3);
        assert_eq!(p.colliders.len(), 1);
    }
    // The skater orientation turns the model's +Z onto the travel direction (+x).
    let f = root_rotation(&s) * Vec3::Z;
    assert!(f.x > 0.99, "{f:?}");
    for phase in [ReplayPhase::Rolling, ReplayPhase::Crouched, ReplayPhase::Air, ReplayPhase::AirTrick, ReplayPhase::GroundTrick, ReplayPhase::OffBoard] {
        for style in ["Aggressive", "Loose", "DannyWay"] {
            assert!(PUPPET_CLIPS.contains(&puppet_clip(phase, style)));
        }
    }
    let r = npc_readout(&[(nid, "pro_1".into(), Some(s.clone()))], Some([s.position[0], 0.0, 0.0]));
    assert!(r.contains("npc skaters 1") && r.contains("70 m") && r.contains("air"), "{r}");
}

/// Data-gated: every puppet clip exists in the user's stock animation banks and evaluates.
#[test]
fn living_world_npc_puppet_clips_exist_in_the_stock_banks() {
    let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from).filter(|r| r.join("private/stock/data/anim/OnBoard.abin").exists()) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let evaluator = crate::animation_pose::PoseEvaluator::load(&root).unwrap();
    use skate_core::animation::playback_tree::PoseCommand;
    for clip in PUPPET_CLIPS {
        let pose = evaluator
            .evaluate(&[
                PoseCommand::Clip { name: clip.to_owned(), previous_time: 0.5, time: 0.5, loops: 0 },
                PoseCommand::Pose { name: "RIG_TPOSE".into() },
                PoseCommand::Add { motion_is_a: true },
            ])
            .unwrap_or_else(|e| panic!("{clip}: {e}"));
        assert_eq!(pose.len(), evaluator.frames.bone_names.len(), "{clip}");
    }
}

#[test]
fn living_world_npc_skaters_chain_lines_and_fade_only_at_a_dead_end() {
    // Retail (fix 9): at a line end the skater continues on an unused line starting within 4 m
    // (`sub_8246C7F8`); the fixture's line i ends on line i + 6's start, so NPCs keep riding and
    // nothing fades out while a next line exists. They leave by the 120 m cull instead (after
    // 40 s alongside them the player speeds up to 30 m/s and leaves them behind).
    let mut a = app(5);
    for frame in 0..(60 * 60) {
        if frame == 40 * 60 {
            a.world_mut().resource_mut::<Drive>().speed = 30.0;
        }
        run(&mut a, 0.0, 60.0);
        let mut q = a.world_mut().query::<(&NpcFade, &NpcReplay)>();
        for (f, r) in q.iter(a.world()) {
            assert!(f.fade.started.is_none() || r.cursor.finished, "fade out only at a dead end");
        }
    }
    let seen = &a.world().resource::<Seen>().0;
    let chained = seen.iter().filter(|e| matches!(e, NpcSkaterEvent::Branch { record, .. } if record.from_node == NODES - 1 && record.to_node == 0)).count();
    assert!(chained > 0, "NPCs continued on the next line at their line end");
    assert!(seen.iter().any(|e| matches!(e, NpcSkaterEvent::Despawned { reason: DespawnReason::Distance, .. })), "left by the distance cull");

    // A mod's radius 0 turns every line end into a dead end: the retail 1 s fade, removed below
    // opacity 0.2 (`sub_8246EA90` / `sub_8245A9B8`), never popping at full opacity.
    let mut b = app(5);
    b.world_mut().resource_mut::<LivingWorldSettings>().skater_line_chain.radius = 0.0;
    let mut faded = false;
    for _ in 0..(25 * 60) {
        run(&mut b, 0.0, 60.0);
        let mut q = b.world_mut().query::<&NpcFade>();
        for f in q.iter(b.world()) {
            assert!(f.alpha >= 0.0 && f.alpha <= 1.0);
            if f.fade.started.is_some() && f.alpha < 1.0 {
                faded = true;
                assert!(f.alpha >= 0.15, "removed once below 0.2: {}", f.alpha);
            }
        }
    }
    assert!(faded, "an NPC reached a dead end within 25 s");
    let seen = &b.world().resource::<Seen>().0;
    assert!(!seen.iter().any(|e| matches!(e, NpcSkaterEvent::Branch { record, .. } if record.from_node == NODES - 1)));
    assert!(seen.iter().any(|e| matches!(e, NpcSkaterEvent::Despawned { reason: DespawnReason::External, .. })));
}

/// Fix 23 (NPC skaters riding backwards, user test 6), headless: every line chains onto the line
/// starting at its end (line i -> i + 6), and lines 6..11, 18..23, ... were recorded fakie for their
/// first 50 nodes (skater and board frame facing -x while travelling +x), so every chain joins a
/// line the other way round from the one it left; the other lines hold a switch-stance stretch
/// (nodes 150..250: skater frame turned round, board forward). Retail rule (fix 23 corrected,
/// `FacingRule::RidingEntry`, mod option until the stance mirror lands): the recorded skater frame, turned only while the flip latched at a
/// riding entry is set (clear here: every spawn node's skater frame agrees with its path frame), held
/// across switches. With it no NPC is drawn against its travel outside a recorded fakie or
/// switch-stance stretch once the switch blend is over; the switch-stance stretch is drawn as
/// recorded, body against travel (the puppet has no stance mirror: NOT RETAIL YET), and
/// `NPC_SKATER_BACKWARDS` reports the recorded fakie stretches as such. The fix 23 rule (`per_node`,
/// the default, NOT RETAIL YET) draws the switch stance forward; the fix 16 mod option (`keep_facing`) reproduces the
/// reported bug: forward-recorded lines ridden backwards.
#[test]
fn living_world_npc_skaters_never_ride_backwards_across_line_switches() {
    const FAKIE: [u8; 4] = [128, 38, 128, 218]; // -90 deg about +y: +Z onto -x.
    use skate_core::living_world::replay::FacingRule;
    let ride = |keep_facing: bool, facing_rule: FacingRule| {
        let mut a = app(5);
        let mut d = data();
        let mut lines = (*d.npc.lines).clone();
        for (k, l) in lines.values_mut().enumerate() {
            if k % 12 >= 6 {
                for n in &mut l.nodes[..50] {
                    n.skater = FAKIE;
                    n.board = FAKIE;
                }
            } else {
                // Switch stance: the recorder's body turned round, the board rolling nose first.
                for n in &mut l.nodes[150..250] {
                    n.skater = FAKIE;
                }
            }
        }
        d.npc.lines = Arc::new(lines.clone());
        let settings = LivingWorldSettings { seed: 5, skater_line_chain: skate_core::living_world::replay::ChainConfig { keep_facing, facing_rule, ..Default::default() }, ..LivingWorldSettings::default() };
        let mut state = PopulationState::default();
        state.install("Test", 1, &settings, d);
        a.insert_resource(settings).insert_resource(state);
        let (mut forward_backwards, mut fakie_logged, mut checked, mut stance, mut stance_fakie) = (0usize, 0usize, 0usize, 0usize, 0usize);
        for _ in 0..(40 * 60) {
            run(&mut a, 0.0, 60.0);
            let mut q = a.world_mut().query::<(&NpcSkater, &NpcReplay)>();
            for (n, r) in q.iter(a.world()) {
                let Some(s) = r.last.as_ref() else { continue };
                let Some(line) = lines.get(&s.line) else { continue };
                if r.cursor.switch.is_some() {
                    continue;
                }
                checked += 1;
                if let Some(text) = backwards_line(n.id, &n.character, line, &r.cursor, s) {
                    assert!(text.starts_with("NPC_SKATER_BACKWARDS #") && text.contains("velocity_yaw"), "{text}");
                    if text.contains("recorded_fakie true") {
                        fakie_logged += 1;
                    } else if (149..=250).contains(&s.node) && lines.keys().position(|k| *k == s.line).is_some_and(|k| k % 12 < 6) {
                        stance += 1;
                        stance_fakie += usize::from(text.contains("drawn_fakie true"));
                    } else {
                        forward_backwards += 1;
                    }
                }
            }
        }
        let chains = a.world().resource::<Seen>().0.iter().filter(|e| matches!(e, NpcSkaterEvent::Branch { record, .. } if record.from_node == NODES - 1)).count();
        (forward_backwards, fakie_logged, checked, chains, stance, stance_fakie)
    };
    let (bad, fakie, checked, chains, stance, stance_fakie) = ride(false, FacingRule::RidingEntry);
    assert!(chains >= 2 && checked > 1000, "chains {chains}, checked {checked}");
    assert_eq!(bad, 0, "retail: no NPC rides a forward-recorded line backwards outside switch stance");
    assert!(fakie > 0, "the recorded fakie stretches are reported as recorded_fakie");
    println!("retail rule: {stance} switch-stance samples drawn against travel, {stance_fakie} of them drawn fakie (retail's fakie bit, fakie channel), {fakie} recorded fakie, {checked} checked");
    assert!(stance == 0 || stance_fakie > 0, "the switch-stance stretch drawn against travel gets retail's fakie bit");
    // Deterministic: the same run gives the same counts.
    assert_eq!(ride(false, FacingRule::RidingEntry), (bad, fakie, checked, chains, stance, stance_fakie));
    let (per_node_bad, _, _, _, per_node_stance, _) = ride(false, FacingRule::PerNode);
    assert_eq!((per_node_bad, per_node_stance), (0, 0), "the fix 23 option draws switch stance forward");
    let (old_bad, ..) = ride(true, FacingRule::PerNode);
    assert!(old_bad > 100, "the fix 16 option rides forward-recorded lines backwards ({old_bad})");
}

#[test]
fn living_world_npc_skaters_fade_in_from_transparent_with_blended_copies() {
    // Retail `sub_825926F8` / `sub_82594488`: opacity 0 at the spawn, 1 after 1 s. While fading the
    // NPC's meshes draw with blended per-NPC copies; at 1 the shared material comes back.
    let mut a = app(5);
    a.insert_resource(Assets::<StandardMaterial>::default());
    a.add_systems(Update, present_fade.after(advance));
    let shared = a.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: Color::srgba(1.0, 0.5, 0.25, 1.0), ..default() });
    let mut fades = Vec::new();
    let mut npc = None;
    // Run until the first NPC spawns, then give it a mesh child (the look, without assets).
    for _ in 0..(20 * 60) {
        run(&mut a, 0.0, 60.0);
        let index = &a.world().resource::<NpcSkaterIndex>().0;
        if let Some((nid, e)) = index.iter().next().map(|(k, v)| (*k, *v)) {
            npc = Some((nid, e));
            break;
        }
    }
    let (nid, e) = npc.expect("an NPC spawned");
    let spawn_tick = a.world().get::<NpcSkater>(e).unwrap().spawn_tick;
    let mesh = a.world_mut().spawn((MeshMaterial3d(shared.clone()), ChildOf(e))).id();
    let first = *a.world().get::<NpcFade>(e).unwrap();
    assert!(first.alpha < 0.05, "starts transparent: {}", first.alpha);
    for _ in 0..90 {
        run(&mut a, 0.0, 60.0);
        let Some(f) = a.world().get::<NpcFade>(e).copied() else { break };
        let tick = a.world().resource::<PopulationState>().world.tick();
        let used = a.world().get::<MeshMaterial3d<StandardMaterial>>(mesh).unwrap().0.clone();
        if f.alpha < 1.0 {
            // A blended copy at the fade's alpha, the shared material untouched.
            assert_ne!(used.id(), shared.id(), "tick {tick}");
            let mats = a.world().resource::<Assets<StandardMaterial>>();
            let m = mats.get(&used).unwrap();
            assert_eq!(m.alpha_mode, AlphaMode::Blend);
            assert!((m.base_color.alpha() - f.alpha).abs() < 1e-6);
            assert_eq!(mats.get(&shared).unwrap().alpha_mode, AlphaMode::Opaque);
            assert!(a.world().get::<NpcFadeMaterials>(e).is_some());
        } else {
            assert_eq!(used.id(), shared.id(), "shared material back at alpha 1");
            assert!(a.world().get::<NpcFadeMaterials>(e).is_none(), "copies freed");
        }
        fades.push((tick - spawn_tick, f.alpha));
    }
    // The curve is the core's fade in, a function of the spawn-relative tick.
    let cfg = skate_core::living_world::leave_fade::LeaveFadeConfig::retail();
    for (t, alpha) in &fades {
        assert_eq!(*alpha, cfg.fade_in_alpha(*t), "npc {nid:?} tick {t}");
    }
    assert!(fades.iter().any(|f| f.1 > 0.2 && f.1 < 0.8));
    let at_1s = fades.iter().find(|f| f.0 == 60).expect("still alive 1 s after the spawn");
    assert_eq!(at_1s.1, 1.0);
}

const ALL_PHASES: [ReplayPhase; 6] = [ReplayPhase::Rolling, ReplayPhase::Crouched, ReplayPhase::Air, ReplayPhase::AirTrick, ReplayPhase::GroundTrick, ReplayPhase::OffBoard];

/// Every NPC gets its phase's clip attached and the clip time runs with the phase (it used to be
/// sampled past the clip end, which froze looping clips on their last frame).
#[test]
fn living_world_npc_puppet_clip_attached_and_playing_per_phase() {
    let mut a = app(5);
    a.add_systems(Update, present_pose.after(collect));
    run(&mut a, 8.0, 60.0);
    let snapshot = |a: &mut App| {
        let mut q = a.world_mut().query::<(&NpcSkater, &NpcPuppetClip)>();
        q.iter(a.world()).map(|(n, c)| (n.id, (c.clone(), crate::custom_models::native_animation_style(&n.character)))).collect::<BTreeMap<_, _>>()
    };
    let before = snapshot(&mut a);
    let mut q = a.world_mut().query::<&NpcReplay>();
    assert!(!before.is_empty() && before.len() == q.iter(a.world()).count(), "every NPC carries its clip");
    for (c, style) in before.values() {
        assert_eq!(c.clip, puppet_clip(c.phase, style));
        assert!(!c.posed, "no skater runtime in the headless app: nothing posed");
    }
    run(&mut a, 0.5, 60.0);
    let after = snapshot(&mut a);
    let mut advanced = 0;
    for (id, (c, _)) in &after {
        if let Some((b, _)) = before.get(id).filter(|(b, _)| b.phase == c.phase && c.time > b.time) {
            assert!((c.time - b.time - 0.5).abs() < 0.1, "{} -> {}", b.time, c.time);
            advanced += 1;
        }
    }
    assert!(advanced > 0, "clip time runs with the phase");
    // A mod override per phase id (and per style) is picked up at once and dropped on reset.
    for phase in ALL_PHASES {
        let mut s = a.world_mut().resource_mut::<LivingWorldSettings>();
        s.skater_clips.clear();
        s.skater_clips.insert(phase.name().to_owned(), format!("MOD_{}_CYC", phase.name()));
        drop(s);
        a.update();
        let s = a.world().resource::<LivingWorldSettings>();
        assert_eq!(resolve_puppet_clip(&s.skater_clips, phase, "Loose"), format!("MOD_{}_CYC", phase.name()));
        for other in ALL_PHASES.into_iter().filter(|p| *p != phase) {
            assert_eq!(resolve_puppet_clip(&s.skater_clips, other, "Loose"), puppet_clip(other, "Loose"));
        }
        let mut q = a.world_mut().query::<&NpcPuppetClip>();
        for c in q.iter(a.world()).filter(|c| c.phase == phase) {
            assert_eq!(c.clip, format!("MOD_{}_CYC", phase.name()));
        }
    }
    let mut s = a.world_mut().resource_mut::<LivingWorldSettings>();
    s.skater_clips.insert("rolling.Loose".into(), "STYLE_CYC".into());
    assert_eq!(resolve_puppet_clip(&s.skater_clips, ReplayPhase::Rolling, "Loose"), "STYLE_CYC");
    s.reset_mod_overrides();
    assert!(s.skater_clips.is_empty());
}

#[test]
fn living_world_npc_looping_clips_wrap_and_others_hold() {
    assert!(clip_loops("R_IDLE_RIDE_N_0_CYC") && !clip_loops("R_IDLE_LCOM_000"));
    assert!((puppet_clip_time("R_IDLE_RIDE_N_0_CYC", 2.5, 1.0) - 0.5).abs() < 1e-6);
    assert_eq!(puppet_clip_time("R_IDLE_LCOM_000", 2.5, 1.0), 2.5);
    assert_eq!(puppet_clip_time("X_CYC", 2.5, 0.0), 2.5);
}

/// Data-gated: every phase's clip evaluates and plays (the pose changes over time and keeps
/// changing after the clip's first loop instead of freezing on its last frame).
#[test]
fn living_world_npc_puppet_clips_play_past_their_first_loop() {
    let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from).filter(|r| r.join("private/stock/data/anim/OnBoard.abin").exists()) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let evaluator = crate::animation_pose::PoseEvaluator::load(&root).unwrap();
    let diff = |a: &[Mat4], b: &[Mat4]| a.iter().zip(b).map(|(x, y)| (*x - *y).to_cols_array().iter().map(|v| v.abs()).fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
    for phase in ALL_PHASES {
        for style in ["Aggressive", "Loose", "DannyWay"] {
            let clip = puppet_clip(phase, style);
            let length = evaluator.clip_length(clip).unwrap();
            assert!(length > 0.0, "{clip}");
            let at = |t: f32| evaluator_pose(&evaluator, clip, t).unwrap_or_else(|| panic!("{clip} at {t}"));
            assert!(diff(&at(0.0), &at(length * 0.5)) > 1e-4, "{clip} does not move");
            if clip_loops(clip) {
                // Three loops in: same pose as in the first loop, and still moving.
                let t = length * 0.3;
                assert!(diff(&at(t), &at(t + 3.0 * length)) < 1e-3, "{clip} does not wrap");
                assert!(diff(&at(3.0 * length + 0.1 * length), &at(3.0 * length + 0.6 * length)) > 1e-4, "{clip} frozen after its first loop");
            }
        }
    }
}

/// Fix 12: a phase change crossfades instead of snapping. The weight is the player's transition
/// curve over the stock default 0.2 s, starts at 0 on the change frame (the outgoing clip alone,
/// so the first frame cannot jump), the outgoing clip keeps its own time, a mod's time applies
/// per phase or as `default`, and the same clip on both sides needs no blend.
#[test]
fn living_world_npc_puppet_blend_follows_the_transition_curve() {
    let s = RETAIL_BLEND_SECONDS;
    assert_eq!(s, 0.2);
    let b = |f: u64| puppet_blend("A_CYC", 40 + f, "B_CYC", f, s);
    assert_eq!(b(0).unwrap(), PuppetBlend { from: "A_CYC".into(), from_time: 40.0 / 60.0, weight: 0.0 });
    assert!((b(6).unwrap().weight - 0.5).abs() < 1e-6, "half way at 0.1 s");
    assert!((b(3).unwrap().weight - 0.156_25).abs() < 1e-6, "smoothstep at 0.05 s");
    assert!(b(11).is_some() && b(12).is_none(), "done after 0.2 s (12 frames)");
    assert!(puppet_blend("A_CYC", 5, "A_CYC", 0, s).is_none());
    assert!(puppet_blend("A_CYC", 5, "B_CYC", 0, 0.0).is_none(), "0 s = cut");
    // Deterministic: the same frames give the same blend.
    assert_eq!(b(4), b(4));
    let mut o = BTreeMap::new();
    assert_eq!(blend_seconds(&o, ReplayPhase::Air), 0.2);
    o.insert("default".to_owned(), 0.3);
    o.insert("air".to_owned(), 0.1);
    assert_eq!((blend_seconds(&o, ReplayPhase::Air), blend_seconds(&o, ReplayPhase::Rolling)), (0.1, 0.3));
    let mut settings = LivingWorldSettings::default();
    settings.skater_blend_seconds = o;
    settings.reset_mod_overrides();
    assert!(settings.skater_blend_seconds.is_empty());
}

/// Data-gated: no pose discontinuity at a clip change. For every pair of puppet clips the
/// biggest per-frame (60 Hz) joint move across the change stays within what the clips move by
/// themselves plus one curve step of the gap between the two poses, while the old snap moved by
/// the whole gap.
#[test]
fn living_world_npc_puppet_clip_change_has_no_pose_jump() {
    let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from).filter(|r| r.join("private/stock/data/anim/OnBoard.abin").exists()) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let evaluator = crate::animation_pose::PoseEvaluator::load(&root).unwrap();
    let step = |a: &[Mat4], b: &[Mat4]| a.iter().zip(b).map(|(x, y)| (x.w_axis - y.w_axis).truncate().length()).fold(0.0f32, f32::max);
    let (mut worst_snap, mut worst_jump) = (0.0f32, 0.0f32);
    for from in PUPPET_CLIPS {
        for to in PUPPET_CLIPS.into_iter().filter(|c| *c != from) {
            const BEFORE: u64 = 37;
            // The pose per render frame: the outgoing clip alone before the change, then the blend.
            let pose = |f: i64| -> Vec<Mat4> {
                if f < 0 {
                    return evaluator_pose(&evaluator, from, (BEFORE as i64 + f) as f32 / 60.0).unwrap();
                }
                let f = f as u64;
                let blend = puppet_blend(from, BEFORE + f, to, f, RETAIL_BLEND_SECONDS);
                puppet_blend_pose(&evaluator, to, f as f32 / 60.0, blend.as_ref()).unwrap()
            };
            let clip_motion = |clip: &str, at: u64| step(&evaluator_pose(&evaluator, clip, at as f32 / 60.0).unwrap(), &evaluator_pose(&evaluator, clip, (at + 1) as f32 / 60.0).unwrap());
            let own = (0..16).map(|f| clip_motion(from, BEFORE - 1 + f).max(clip_motion(to, f))).fold(0.0f32, f32::max);
            let gap = (0..16).map(|f| step(&evaluator_pose(&evaluator, from, (BEFORE + f) as f32 / 60.0).unwrap(), &evaluator_pose(&evaluator, to, f as f32 / 60.0).unwrap())).fold(0.0f32, f32::max);
            let snap = step(&pose(-1), &evaluator_pose(&evaluator, to, 0.0).unwrap());
            worst_snap = worst_snap.max(snap);
            // Max curve step at 60 Hz over 0.2 s: 1.5 / 0.2 / 60 = 0.125.
            let limit = own + 0.13 * gap + 1e-3;
            for f in -1..16 {
                let jump = step(&pose(f), &pose(f + 1));
                worst_jump = worst_jump.max(jump);
                assert!(jump <= limit, "{from} -> {to} frame {f}: {jump} > {limit} (own {own}, gap {gap}, snap {snap})");
            }
            // Ends on the incoming clip alone.
            assert!(step(&pose(12), &evaluator_pose(&evaluator, to, 12.0 / 60.0).unwrap()) < 1e-5, "{from} -> {to} not done at 0.2 s");
        }
    }
    eprintln!("clip change: worst snap {worst_snap}, worst blended frame step {worst_jump}");
    assert!(worst_snap > 0.05, "the clips differ enough for the test to mean something ({worst_snap})");
}

/// Fix 14 (jitter): drawn like `present_pose` at a 144 Hz render rate (the cursor one tick back,
/// interpolated by the world clock's fraction with the recorded branches), the NPC root moves
/// by the line speed per render frame, also across the line chains (0.5 m off the end here, blended
/// over `skater_line_chain.blend_seconds`), and the clip time grows with the render clock.
#[test]
fn living_world_npc_skaters_render_smoothly_between_ticks_and_across_chains() {
    let mut a = app(5);
    a.world_mut().resource_mut::<LivingWorldSettings>().skater_line_chain.blend_seconds = 0.3;
    a.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 144.0)));
    let mut last: BTreeMap<LivingWorldId, ([f32; 3], f32)> = BTreeMap::new();
    let (mut worst, mut worst_dt, mut checked, mut blended) = (0.0f32, 0.0f32, 0usize, 0usize);
    for _ in 0..(40 * 144) {
        a.update();
        let state = a.world().resource::<PopulationState>();
        let lines = state.npc.lines.clone();
        let frac = state.world.clock().overstep() as f32;
        let mut q = a.world_mut().query::<(&NpcSkater, &NpcReplay)>();
        let mut seen = BTreeMap::new();
        for (n, r) in q.iter(a.world()) {
            let Some(prev) = r.previous.as_ref().filter(|_| !r.cursor.finished) else { continue };
            assert_eq!(prev.frames + FRAMES_PER_TICK, r.cursor.frames, "previous is one tick back");
            assert_eq!(r.cursor.switch_blend_seconds, 0.3, "the tuning reaches the cursor");
            blended += r.cursor.switch.is_some() as usize;
            let s = prev.render_sample(&*lines, &r.branches, &r.tricks, frac).unwrap();
            let time = (s.phase_frames as f32 + s.sub_frame) / 60.0;
            if let Some((p, t)) = last.get(&n.id) {
                let d = ((s.position[0] - p[0]).powi(2) + (s.position[1] - p[1]).powi(2) + (s.position[2] - p[2]).powi(2)).sqrt();
                worst = worst.max(d);
                if s.previous_phase.is_none() || time > *t {
                    worst_dt = worst_dt.max((time - t - 1.0 / 144.0).abs());
                }
                checked += 1;
            }
            seen.insert(n.id, (s.position, time));
        }
        last = seen;
    }
    assert!(checked > 1000 && blended > 0, "{checked} frames, {blended} blended");
    // 7.5 m/s / 144 = 0.052 m, plus a 0.5 m gap over 0.3 s (1.5 x 0.5 / 0.3 / 144 = 0.017 m).
    assert!(worst < 0.075, "worst render step {worst} m");
    assert!(worst_dt < 2e-3, "clip time off the render clock by {worst_dt} s");
}

/// Fix 19: NPC skater prop-push volumes (stable actor id, board + body like the proxy) and the mod
/// switch `living_world.npc_skater_props` (retail on, mod disable restores it).
#[test]
fn living_world_npc_skater_prop_volumes_and_mod_switch() {
    use skate_core::physics::board_step::CollisionBody;
    let id = skate_core::living_world::LivingWorldId { kind: skate_core::living_world::Kind::Skater, serial: 3 };
    let q = Quat::from_rotation_y(0.5);
    let s = skate_core::living_world::replay::ReplaySample {
        line: [0; 16], node: 0, position: [1.0, 2.0, 3.0], velocity: [4.0, 0.0, 1.0], heading: 0.5,
        board: [q.x, q.y, q.z, q.w], skater: [q.x, q.y, q.z, q.w], flags: 0, phase: ReplayPhase::Rolling,
        jump: None, phase_frames: 0, previous_phase: None, previous_phase_frames: 0, sub_frame: 0.0, fakie: false,
    };
    let v = prop_volumes(id, &s);
    assert!(v.iter().all(|(actor, _)| *actor == PROXY_ID_TAG | id.to_u64()));
    assert!(v.iter().all(|(_, v)| matches!(v.body, CollisionBody::Board(_))), "a riding skater's hit, board cap");
    assert_eq!(v[1].1.motion.linear_velocity.x, s.velocity[0]);
    assert_eq!(format!("{v:?}"), format!("{:?}", prop_volumes(id, &s)), "deterministic");

    let mut world = World::new();
    world.insert_resource(LivingWorldSettings::default());
    assert!(world.resource::<LivingWorldSettings>().npc_skater_props.enabled, "retail on");
    crate::modding::world_tuning::set(&mut world, "mod_a", "living_world", Some(serde_json::json!({"npc_skater_props": {"enabled": false}}))).unwrap();
    assert!(!world.resource::<LivingWorldSettings>().npc_skater_props.enabled);
    assert!(crate::modding::world_tuning::set(&mut world, "mod_a", "living_world", Some(serde_json::json!({"npc_skater_props": {"radius": 1.0}}))).is_err());
    crate::modding::world_tuning::clear_owner(&mut world, "mod_a");
    assert!(world.resource::<LivingWorldSettings>().npc_skater_props.enabled, "mod disable restores retail");
}

/// Fix 21: a phase change while the previous blend still runs nests (the running blend is the
/// outgoing pose), consecutive phases on one clip are one layer, and layers stop at the first
/// fully blended one.
#[test]
fn living_world_npc_puppet_layers_nest_running_blends() {
    use skate_core::living_world::replay::PhaseEntry;
    let e = |phase, since| PhaseEntry { phase, since, trick: -1 };
    let clip = |e: PhaseEntry, _: Option<PhaseEntry>| (e.phase.name().to_owned(), 0.2);
    // Rolling long ago, crouched 0.1 s ago, air 0.05 s ago: three layers, oldest first.
    let h = [(e(ReplayPhase::Air, 100), 0.05), (e(ReplayPhase::Crouched, 97), 0.1), (e(ReplayPhase::Rolling, 0), 5.0)];
    let l = puppet_layers(&h, clip);
    assert_eq!(l.iter().map(|l| l.clip.as_str()).collect::<Vec<_>>(), ["rolling", "crouched", "air"]);
    assert_eq!(l[1].weight, 0.5);
    assert!((l[2].weight - 0.15625).abs() < 1e-6);
    // The newest one fully in: only it shows.
    let h = [(e(ReplayPhase::Air, 100), 0.3), (e(ReplayPhase::Crouched, 97), 0.35)];
    assert_eq!(puppet_layers(&h, clip).len(), 1);
    // Same clip twice in a row: one layer timed from the older start.
    let same = |_: PhaseEntry, _: Option<PhaseEntry>| ("X".to_owned(), 0.2);
    let l = puppet_layers(&[(e(ReplayPhase::Air, 100), 0.05), (e(ReplayPhase::AirTrick, 90), 0.15)], same);
    assert_eq!((l.len(), l[0].time), (1, 0.15));
}

/// Fix 21: recorded trick ids map to the stock trick animations (`Tricks.xml` pairs), grinds and
/// manuals keep the phase clip, a mod overrides per trick name.
#[test]
fn living_world_npc_trick_slots_pick_the_stock_trick_animation() {
    assert_eq!(retail_trick_anim(128).as_deref(), Some("B_OLLIE"));
    assert_eq!(retail_trick_anim(127).as_deref(), Some("B_NOLLIE"));
    assert_eq!(retail_trick_anim(96).as_deref(), Some("B_KICKFLIP_IN"));
    assert_eq!(retail_trick_anim(113).as_deref(), Some("B_N_HEELFLIP_IN"));
    assert_eq!(retail_trick_anim(98).as_deref(), Some("B_KICKFLIP_IN"), "kickflip3 uses its base flip");
    assert_eq!(retail_trick_anim(126).as_deref(), Some("B_VARIALKICKFLIP"));
    assert_eq!(retail_trick_anim(85).as_deref(), Some("B_360FLIP"));
    assert_eq!(retail_trick_anim(54).as_deref(), Some("B_OLLIE"), "a grab leaves the ground with an ollie");
    assert_eq!(retail_trick_anim(7), None, "bs_50_50 is a grind");
    assert_eq!(retail_trick_anim(2), None, "nosemanual");
    assert_eq!(retail_trick_anim(-1), None);
    assert_eq!(retail_trick_anim(400), None);
    let mut m = BTreeMap::new();
    m.insert("trick.kickflip".to_owned(), "B_HEELFLIP_IN".to_owned());
    assert_eq!(resolve_trick_anim(&m, 96).as_deref(), Some("B_HEELFLIP_IN"));
    assert_eq!(resolve_trick_anim(&m, 92).as_deref(), Some("B_HEELFLIP_IN"));
    assert_eq!(resolve_trick_anim(&m, 128).as_deref(), Some("B_OLLIE"));
}

fn stock_evaluator() -> Option<(crate::animation_pose::PoseEvaluator, skate_data::animation_metadata::AnimationMetadata)> {
    let root = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from).filter(|r| r.join("private/stock/data/anim/OnBoard.abin").exists())?;
    let meta = skate_data::animation_banks::AnimationBanks::load(&root).unwrap().metadata().unwrap();
    Some((crate::animation_pose::PoseEvaluator::load(&root).unwrap(), meta))
}

/// Data-gated (fix 21): every trick animation a recorded slot can pick exists as `_G` and `_A`
/// in the stock banks, and a kickflip's air clip turns the board (rig `Skateboard_Root`) over.
#[test]
fn living_world_npc_trick_clips_exist_and_flip_the_board() {
    let Some((evaluator, meta)) = stock_evaluator() else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let mut names: Vec<String> = (0..332i16)
        .filter_map(|t| retail_trick_anim(t).map(|b| [format!("{b}_G"), trick_air_sequence(&b, t)]))
        .flatten()
        .flat_map(|n| n.split('+').map(str::to_owned).collect::<Vec<_>>())
        .collect();
    names.sort();
    names.dedup();
    let mut missing = Vec::new();
    for name in &names {
        match stock_tree_leaf(&meta, name).map(|c| (evaluator.clip_length(&c), c)) {
            Some((Ok(len), clip)) => {
                eprintln!("{name} -> {clip}: {len:.3} s");
                assert!(evaluator_pose(&evaluator, &clip, len * 0.5).is_some(), "{clip}");
            }
            other => missing.push(format!("{name}: {other:?}")),
        }
    }
    assert!(missing.is_empty(), "missing trick clips: {missing:?}");
    let board = evaluator.frames.bone_names.iter().position(|n| n.eq_ignore_ascii_case("Skateboard_Root")).expect("board bone");
    let turn = |clip: &str, len: f32| {
        let start = Quat::from_mat4(&evaluator_pose(&evaluator, clip, 0.0).unwrap()[board]);
        (1..=30).map(|i| start.angle_between(Quat::from_mat4(&evaluator_pose(&evaluator, clip, len * i as f32 / 30.0).unwrap()[board]))).fold(0.0f32, f32::max)
    };
    let kickflip = trick_air_sequence("B_KICKFLIP_IN", 96).split('+').map(|n| stock_tree_leaf(&meta, n).unwrap()).collect::<Vec<_>>().join("+");
    let kickflip_len: f32 = kickflip.split('+').map(|c| evaluator.clip_length(c).unwrap()).sum();
    eprintln!("kickflip air: {kickflip} {kickflip_len:.3} s");
    let most = turn(&kickflip, kickflip_len);
    let idle = turn("IA_IDLE_LO_N_0_CYC", evaluator.clip_length("IA_IDLE_LO_N_0_CYC").unwrap());
    eprintln!("board turn: kickflip air {:.0} deg, old air-trick clip {:.0} deg", most.to_degrees(), idle.to_degrees());
    assert!(most.to_degrees() > 90.0, "the kickflip turns the board over ({} deg)", most.to_degrees());
}

/// Data-gated (fix 21): the recorded jump pattern of the shipped lines (rolling, a trick span
/// opening on the ground, airborne, the span closing in the air, landing; phases shorter than the
/// 0.2 s blend, as half of all changes on the lines are) is drawn without a pose jump: at every
/// phase change the drawn pose continues the one showing (the new clip enters at weight 0 over
/// the running blend), where restarting the blend from one previous clip (fix 12) jumped.
#[test]
fn living_world_npc_trick_jump_has_no_pose_pop() {
    use skate_core::living_world::replay::PhaseEntry;
    let Some((evaluator, meta)) = stock_evaluator() else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let stock = |n: &str| stock_tree_leaf(&meta, n).filter(|c| evaluator.clip_length(c).is_ok());
    let step = |a: &[Mat4], b: &[Mat4]| a.iter().zip(b).map(|(x, y)| (x.w_axis - y.w_axis).truncate().length()).fold(0.0f32, f32::max);
    let none_clips = BTreeMap::new();
    let none_blends = BTreeMap::new();
    let evaluates = |c: &str| evaluator.clip_length(c).is_ok();
    let mut worst = (0.0f32, 0.0f32, [0.0f32; 2]);
    for (trick, timeline) in [
        // ollie: crouch for 0 frames, takeoff 8, air trick 13, air 20, land.
        (128i16, [(ReplayPhase::Rolling, 0u64), (ReplayPhase::Crouched, 40), (ReplayPhase::GroundTrick, 40), (ReplayPhase::AirTrick, 48), (ReplayPhase::Air, 61), (ReplayPhase::Rolling, 81)]),
        (96, [(ReplayPhase::Rolling, 0), (ReplayPhase::Crouched, 40), (ReplayPhase::GroundTrick, 41), (ReplayPhase::AirTrick, 50), (ReplayPhase::Air, 62), (ReplayPhase::Rolling, 70)]),
        // a grind span (no trick clip): rolling, grind 10 frames, hop off.
        (7, [(ReplayPhase::Rolling, 0), (ReplayPhase::GroundTrick, 40), (ReplayPhase::AirTrick, 50), (ReplayPhase::Air, 54), (ReplayPhase::Rolling, 66), (ReplayPhase::Crouched, 70)]),
    ] {
        let entries: Vec<PhaseEntry> = timeline
            .iter()
            .map(|&(phase, since)| PhaseEntry { phase, since, trick: if matches!(phase, ReplayPhase::GroundTrick | ReplayPhase::AirTrick) { trick } else { -1 } })
            .collect();
        let resolve = |e: PhaseEntry, o: Option<PhaseEntry>| puppet_layer_clip(&none_clips, &none_blends, "Normal", &evaluates, &stock, e, o);
        // Phases begun up to frame `f` (`upto` = f) or before it (`upto` = f - 1: what showed
        // before a change at f, still playing at f).
        let history = |f: u64, upto: u64, keep: usize| -> Vec<(PhaseEntry, f32)> {
            entries.iter().rev().filter(|e| e.since <= upto).take(keep).map(|e| (*e, (f - e.since) as f32 / 60.0)).collect()
        };
        // New: nested layers. Old (fix 12): the newest clip over the previous phase's clip alone.
        let layers = |f: u64, upto: u64, old: bool| {
            let mut l = puppet_layers(&history(f, upto, if old { 2 } else { usize::MAX }), resolve);
            if old && l.len() > 2 {
                l.drain(..l.len() - 2);
            }
            if let Some(first) = l.first_mut().filter(|_| old) {
                first.weight = 1.0;
            }
            l
        };
        let layers_at = |f: u64| layers(f, f, false);
        for old in [false, true] {
            let mut prev = puppet_layers_pose(&evaluator, &layers(30, 30, old)).unwrap();
            for f in 31..100u64 {
                let pose = puppet_layers_pose(&evaluator, &layers(f, f, old)).unwrap();
                worst.2[old as usize] = worst.2[old as usize].max(step(&prev, &pose));
                // A pop: at a phase change, the drawn pose leaves the pose that was showing.
                if entries.iter().any(|e| e.since == f) {
                    let before = puppet_layers_pose(&evaluator, &layers(f, f - 1, old)).unwrap();
                    let pop = step(&before, &pose);
                    if pop > 1e-3 {
                        eprintln!("{} trick {trick} frame {f}: pop {pop:.3} m", if old { "old" } else { "new" });
                    }
                    if old {
                        worst.1 = worst.1.max(pop);
                    } else {
                        worst.0 = worst.0.max(pop);
                    }
                }
                prev = pose;
            }
        }
        // The trick clips play: the takeoff shows `_G`, the air `_A` (until it lands).
        if trick == 96 {
            const KICKFLIP: &str = "KICKFLIP_IN_LOW_G+KICKFLIP_IN_LOW_A+T_LOW_KICK_CYC1+T_KICKFLIP_LOW_4FLIPS_0_OUT1";
            assert_eq!(layers_at(45).last().unwrap().clip, KICKFLIP);
            assert_eq!(layers_at(55).last().unwrap().clip, KICKFLIP, "takeoff and air are one sequence");
            let air = layers_at(65);
            assert_eq!(air.last().unwrap().clip, KICKFLIP, "the air clip runs on after the span closes");
            assert_eq!(air.last().unwrap().time, 24.0 / 60.0);
        }
    }
    eprintln!(
        "pose pop at phase changes: nested {:.4} m, one previous clip {:.3} m; worst joint move per 60 Hz frame: nested {:.3} m, old {:.3} m",
        worst.0, worst.1, worst.2[0], worst.2[1]
    );
    // A clip entering at weight 0 leaves the pose within float noise of the SQT blend (< 1 cm).
    assert!(worst.0 < 0.01, "nested crossfade pops {} m", worst.0);
    assert!(worst.1 > 0.05, "the old blend popped on this timeline ({} m)", worst.1);
}


/// Data-gated (stance port): the puppet's fakie rule is the stock motion graph's
/// `UpdateRidingFakie` (every node carries the thresholds `ChainConfig::retail().fakie` uses), and
/// the fakie channel tree resolves to a playable clip that changes the riding pose when overlaid.
#[test]
fn living_world_npc_fakie_rule_and_channel_match_the_stock_graph() {
    let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from).filter(|r| r.join("private/stock/data/anim/OnBoard.abin").exists()) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let assets = skate_data::GameAssets::load(&root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(&root, &assets).unwrap();
    let settings = crate::graph_host::motion::stock_riding_fakie_settings(&graphs.motion).unwrap();
    eprintln!("stock UpdateRidingFakie: {settings:?}");
    assert!(!settings.is_empty(), "the stock motion graph has UpdateRidingFakie");
    for s in &settings {
        assert_eq!(*s, skate_core::living_world::replay::ChainConfig::retail().fakie, "retail thresholds");
    }
    let (evaluator, meta) = stock_evaluator().unwrap();
    {
        use skate_data::animation_metadata::TreeMetadata;
        let kind = match meta.tree(FAKIE_CHANNEL) {
            Ok(TreeMetadata::Clip(c)) => format!("clip {}", c.name),
            Ok(TreeMetadata::Selector(s)) => format!("selector on {} default {} of {:?}", s.parameter, s.default, s.children),
            Ok(TreeMetadata::PhaseBlend(p)) => format!("phase blend of {:?}", p.children),
            Ok(TreeMetadata::BlendSpace(_)) => "blend space".into(),
            Ok(TreeMetadata::SelectionSpace(_)) => "selection space".into(),
            Err(e) => format!("missing: {e}"),
        };
        eprintln!("{FAKIE_CHANNEL}: {kind}");
    }
    let animation = crate::graph_host::motion::metadata_animation(meta);
    let tree = crate::graph_host::motion::tree_commands(&animation, FAKIE_CHANNEL, &fakie_channel_attributes(), 0.5).expect("the fakie channel tree builds");
    let clips: Vec<String> = tree.iter().filter_map(|c| match c {
        skate_core::animation::playback_tree::PoseCommand::Clip { name, .. } => Some(name.clone()),
        _ => None,
    }).collect();
    eprintln!("{FAKIE_CHANNEL} at torso {FAKIE_TORSO_RIDING}: {} commands, clips {clips:?}", tree.len());
    assert!(clips.iter().all(|c| evaluator.clip_length(c).is_ok()), "every channel clip evaluates");
    let base = [PuppetLayer { clip: "R_IDLE_RIDE_N_0_CYC".into(), time: 0.5, weight: 1.0, since: 0 }];
    let plain = puppet_layers_pose(&evaluator, &base).unwrap();
    let fakie = puppet_pose_with_channel(&evaluator, &base, Some((&tree, 1.0))).unwrap();
    let moved: Vec<&str> = plain
        .iter()
        .zip(&fakie)
        .enumerate()
        .filter(|(_, (a, b))| Quat::from_mat4(a).angle_between(Quat::from_mat4(b)) > 0.02)
        .map(|(i, _)| evaluator.frames.bone_names[i].as_str())
        .collect();
    eprintln!("fakie channel turns {} of {} bones: {moved:?}", moved.len(), plain.len());
    assert!(!moved.is_empty(), "the overlay changes the pose");
    assert_eq!(puppet_pose_with_channel(&evaluator, &base, Some((&tree, 0.0))).unwrap(), plain, "weight 0 = no overlay");
    // The cursor side: the layer carries the tree name, the mod key replaces it.
    let mut clips = BTreeMap::new();
    assert_eq!(resolve_fakie_channel(&clips), FAKIE_CHANNEL);
    clips.insert("fakie_channel".to_owned(), "B_OTHER".to_owned());
    assert_eq!(resolve_fakie_channel(&clips), "B_OTHER");
}

/// The bind pose tail per stance: goofy = the reference pose only (the commands every NPC drew
/// before the stance port), regular = the player's orientation + mirror bits path.
#[test]
fn living_world_npc_bind_pose_tail_per_stance() {
    use skate_core::animation::playback_tree::PoseCommand as P;
    assert_eq!(bind_pose_tail(false), vec![P::Pose { name: "RIG_TPOSE".into() }, P::Add { motion_is_a: true }]);
    let regular = bind_pose_tail(true);
    assert_eq!(regular[..2], bind_pose_tail(false)[..]);
    assert_eq!(regular.last(), Some(&P::Mirror { trajectory_mode: 2 }));
    assert!(regular.contains(&P::Pose { name: "BOARD_BACKWARDS".into() }) && regular.contains(&P::Pose { name: "BOARD_BACKWARDS_IK".into() }));
}

/// Data-gated (natural stance port): a goofy NPC's puppet pose is unchanged by the port, a regular
/// NPC's is the player's mirrored bind pose (left and right swap sides), with and without the
/// fakie channel.
#[test]
fn living_world_npc_regular_puppet_is_mirrored() {
    let Some((evaluator, meta)) = stock_evaluator() else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let base = [PuppetLayer { clip: "R_IDLE_RIDE_N_0_CYC".into(), time: 0.5, weight: 1.0, since: 0 }];
    let goofy = puppet_pose_in_stance(&evaluator, &base, None, false).unwrap();
    assert_eq!(goofy, puppet_layers_pose(&evaluator, &base).unwrap(), "goofy = the pre-port pose");
    let regular = puppet_pose_in_stance(&evaluator, &base, None, true).unwrap();
    let names = &evaluator.frames.bone_names;
    let at = |pose: &[Mat4], name: &str| pose[names.iter().position(|n| n == name).unwrap_or_else(|| panic!("{name} in {names:?}"))].w_axis.truncate();
    let pair = names
        .iter()
        .find_map(|n| {
            let r = n.replacen("LEFT", "RIGHT", 1);
            (n.starts_with("LEFT") && n.contains("FOOT") && names.contains(&r)).then(|| (n.clone(), r))
        })
        .expect("a left / right foot pair");
    let (gl, gr) = (at(&goofy, &pair.0), at(&goofy, &pair.1));
    let (rl, rr) = (at(&regular, &pair.0), at(&regular, &pair.1));
    eprintln!("{} / {}: goofy {gl} / {gr}, regular {rl} / {rr}", pair.0, pair.1);
    // Mirror (mode 2, z reflected) with the bones swapped: each regular foot is the other goofy
    // foot reflected.
    let reflect = |v: Vec3| Vec3::new(v.x, v.y, -v.z);
    assert!(rl.distance(reflect(gr)) < 1e-3 && rr.distance(reflect(gl)) < 1e-3, "mirrored feet");
    // Drawn with the root turned half a turn (`stance_root_turn`): the board keeps its heading
    // (nose truck where the goofy one is) and the left foot leads along root +Z where goofy leads
    // with the right.
    let turn = Mat4::from_quat(stance_root_turn(skate_core::living_world::stance::NaturalStance::Regular));
    let drawn = |pose: &[Mat4], name: &str| turn.transform_point3(at(pose, name));
    assert!(drawn(&regular, "TRUCK_FRONT").distance(at(&goofy, "TRUCK_FRONT")) < 1e-3, "board heading kept");
    assert!(gr.z > gl.z, "goofy: right foot leads");
    assert!(drawn(&regular, &pair.0).z > drawn(&regular, &pair.1).z, "regular: left foot leads");
    assert_eq!(stance_root_turn(skate_core::living_world::stance::NaturalStance::Goofy), Quat::IDENTITY);
    // The fakie channel goes through the same tail (as the player's channel tree).
    let animation = crate::graph_host::motion::metadata_animation(meta);
    let tree = crate::graph_host::motion::tree_commands(&animation, FAKIE_CHANNEL, &fakie_channel_attributes(), 0.5).unwrap();
    let fakie_regular = puppet_pose_in_stance(&evaluator, &base, Some((&tree, 1.0)), true).unwrap();
    assert_ne!(fakie_regular, regular, "the overlay changes the regular pose");
    assert_eq!(puppet_pose_in_stance(&evaluator, &base, Some((&tree, 0.0)), true).unwrap(), regular, "weight 0 = no overlay");
}

/// Trick-clip stance toggles (retail `82593230` -> `82B98980` per actor): the newest layer's clip
/// events toggle the NPC's bits once (point events, collected over each step's window, across
/// `+` sequence parts); a layer keeps the bits it began with, newer layers get the toggled ones;
/// a mod's rename turns a toggle off; the start list stays bounded.
#[test]
fn living_world_npc_stance_track_follows_trick_clip_events() {
    use skate_core::living_world::stance::{NaturalStance, StanceEvents, StanceFlags};
    // Stock spelling (upper case) and a point event 0.3 into a 1 s clip, like `POPSHUVIT_HIGH_A`.
    let data = |part: &str| -> Option<(f32, Vec<(String, f32, f32)>)> {
        match part {
            "SHUV_A" => Some((1.0, vec![("ANIMBOARDBACKWARD".into(), 0.3, 0.3), ("OTHER".into(), -1.0, -1.0)])),
            "RIDE" => Some((0.5, vec![])),
            _ => None,
        }
    };
    let layer = |clip: &str, since: u64, time: f32| PuppetLayer { clip: clip.into(), time, weight: 1.0, since };
    let retail = StanceEvents::retail();
    for stance in [NaturalStance::Goofy, NaturalStance::Regular] {
        let natural = StanceFlags::natural(stance);
        let mut t = NpcStanceTrack::new(stance);
        let mut toggles = 0;
        for i in 0..40 {
            toggles += usize::from(t.step(&layer("SHUV_A", 100, i as f32 / 30.0), &retail, |c, p, n| sequence_attributes(c, p, n, data)));
        }
        assert_eq!(toggles, 1, "{stance:?}: one toggle over the clip");
        let flipped = StanceFlags { board_backward: !natural.board_backward, ..natural };
        assert_eq!(t.flags, flipped);
        assert_eq!(t.flags_for(100), natural, "the trick's own tree keeps its start bits");
        assert!(!t.step(&layer("RIDE", 140, 0.0), &retail, |c, p, n| sequence_attributes(c, p, n, data)));
        assert_eq!((t.flags_for(140), t.flags_for(100), t.flags_for(5)), (flipped, natural, natural), "newer layers built with the toggled bits");
        // A second shove-it turns the board back.
        for i in 0..40 {
            t.step(&layer("SHUV_A", 200, i as f32 / 30.0), &retail, |c, p, n| sequence_attributes(c, p, n, data));
        }
        assert_eq!(t.flags, natural);
    }
    // Sequences: the event of a later part fires when the window enters it, once.
    assert_eq!(sequence_attributes("RIDE+SHUV_A", 0.4, 0.85, data), vec!["ANIMBOARDBACKWARD".to_owned(), "OTHER".to_owned()]);
    assert_eq!(sequence_attributes("RIDE+SHUV_A", 0.85, 0.9, data), vec!["OTHER".to_owned()]);
    assert_eq!(sequence_attributes("RIDE+SHUV_A", 0.0, 0.1, data), Vec::<String>::new(), "later parts not reached");
    assert_eq!(sequence_attributes("SHUV_A", 1.0, 2.0, data), vec!["OTHER".to_owned()], "the last part holds: no repeat");
    // Mod: the board toggle renamed to nothing = off.
    let off = StanceEvents::with_overrides(&[("board_backward".to_owned(), String::new())].into());
    let mut t = NpcStanceTrack::new(NaturalStance::Goofy);
    for i in 0..40 {
        assert!(!t.step(&layer("SHUV_A", 1, i as f32 / 30.0), &off, |c, p, n| sequence_attributes(c, p, n, data)));
    }
    // Bounded memory: oldest starts drop off.
    let mut t = NpcStanceTrack::new(NaturalStance::Goofy);
    for since in 0..20 {
        t.step(&layer("RIDE", since, 0.0), &retail, |c, p, n| sequence_attributes(c, p, n, data));
    }
    assert!(t.starts.len() <= 8 && t.starts.last().unwrap().0 == 19);
}

/// The fakie rule after a shove-it: the NPC's board-flipped state is retail's bit 31 xor bit 30
/// (`82B985E8`), and the board axis retail's fakie rule reads (`GetEffectiveRoot82BE3650`: the
/// root's Z, negated iff mirrored) of the puppet root (drawn frame * [`flags_root_turn`]) is the
/// drawn frame the cursor's fakie rule uses, for the natural bits and after the shove-it alike.
#[test]
fn living_world_npc_fakie_axis_after_a_shove_it_is_retails() {
    use skate_core::living_world::stance::{NaturalStance, StanceEvents, StanceFlags};
    let data = |part: &str| -> Option<(f32, Vec<(String, f32, f32)>)> { (part == "SHUV_A").then(|| (1.0, vec![("ANIMBOARDBACKWARD".into(), 0.3, 0.3)])) };
    let drawn = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.1);
    let drawn_z = drawn * Vec3::Z;
    for stance in [NaturalStance::Goofy, NaturalStance::Regular] {
        let mut t = NpcStanceTrack::new(stance);
        assert!(!t.flags.board_flipped(), "{stance:?}: natural bits ride the board unflipped");
        for i in 0..40 {
            t.step(&PuppetLayer { clip: "SHUV_A".into(), time: i as f32 / 30.0, weight: 1.0, since: 1 }, &StanceEvents::retail(), |c, p, n| sequence_attributes(c, p, n, data));
        }
        assert_eq!(t.flags.board_flipped(), t.flags.board_backward ^ t.flags.mirrored);
        assert!(t.flags.board_flipped(), "{stance:?}: after the shove-it the board is flipped");
        for flags in [StanceFlags::natural(stance), t.flags] {
            let root_z = (drawn * flags_root_turn(flags)) * Vec3::Z;
            let axis = flags.fakie_board_axis([root_z.x, root_z.y, root_z.z, 0.0]);
            assert!(Vec3::new(axis[0], axis[1], axis[2]).distance(drawn_z) < 1e-5, "{stance:?} {flags:?}: fakie axis = the drawn frame");
        }
    }
}

/// Data-gated: in the stock banks the trick clips an NPC plays toggle only the board bit
/// (`ANIMBOARDBACKWARD` point events: shove-it, varial, hardflip, inward heelflip families);
/// `MIRRORED` / `SWITCH` sit on switch riding and bail dismount clips, which the puppet does not
/// play. Layers with mixed bits pose differently from uniform ones, uniform bits are the stance
/// pose unchanged.
#[test]
fn living_world_npc_trick_clips_toggle_the_board_bit() {
    use skate_core::living_world::stance::StanceFlags;
    let Some((evaluator, meta)) = stock_evaluator() else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let has = |clip: &str, name: &str| meta.clip(clip).is_ok_and(|c| c.attributes.iter().any(|a| a.name.eq_ignore_ascii_case(name)));
    let mut board = Vec::new();
    for t in 0..332i16 {
        let Some(base) = retail_trick_anim(t) else { continue };
        for part in [format!("{base}_G"), trick_air_sequence(&base, t)].iter().flat_map(|n| n.split('+').map(str::to_owned).collect::<Vec<_>>()) {
            let Some(clip) = stock_tree_leaf(&meta, &part) else { continue };
            assert!(!has(&clip, "MIRRORED") && !has(&clip, "SWITCH"), "{clip}: trick clips carry no mirror / switch events");
            if has(&clip, "ANIMBOARDBACKWARD") {
                board.push((t, clip));
            }
        }
    }
    board.sort();
    board.dedup();
    eprintln!("trick clips with the board event: {board:?}");
    assert!(!board.is_empty(), "some recorded tricks turn the board (shove-it family)");
    assert!(has("POPSHUVIT_HIGH_A", "ANIMBOARDBACKWARD") && has("R_SWITCH_RIDE_N_0_N", "MIRRORED") && has("R_SWITCH_RIDE_N_0_N", "SWITCH"));
    let a = PuppetLayer { clip: "R_IDLE_RIDE_N_0_CYC".into(), time: 0.5, weight: 1.0, since: 0 };
    let b = PuppetLayer { clip: "POPSHUVIT_HIGH_A".into(), time: 0.2, weight: 0.5, since: 10 };
    let layers = [a, b];
    let goofy = StanceFlags::default();
    let turned = StanceFlags { board_backward: true, ..goofy };
    assert_eq!(puppet_pose_in_flags(&evaluator, &layers, &[goofy, goofy], None), puppet_pose_in_stance(&evaluator, &layers, None, false), "uniform goofy = the stance pose");
    let regular = StanceFlags::natural(skate_core::living_world::stance::NaturalStance::Regular);
    assert_eq!(puppet_pose_in_flags(&evaluator, &layers, &[regular, regular], None), puppet_pose_in_stance(&evaluator, &layers, None, true), "uniform regular = the stance pose");
    let mixed = puppet_pose_in_flags(&evaluator, &layers, &[goofy, turned], None);
    assert!(mixed.is_some());
    assert_ne!(mixed, puppet_pose_in_stance(&evaluator, &layers, None, false), "the newer layer's board bit shows");
    // Fully blended in: the newest layer alone, with its own tail.
    let full = [layers[0].clone(), PuppetLayer { weight: 1.0, ..layers[1].clone() }];
    let alone = puppet_pose_in_flags(&evaluator, &full[1..], &[turned], None).unwrap();
    let blended = puppet_pose_in_flags(&evaluator, &full, &[goofy, turned], None).unwrap();
    let bone = evaluator.frames.bone_names.iter().position(|n| n.eq_ignore_ascii_case("Skateboard_Root")).expect("board bone");
    assert!(alone[bone].w_axis.distance(blended[bone].w_axis) < 1e-3, "weight 1 = the newest layer's pose");
    assert_eq!(flags_root_turn(turned), Quat::IDENTITY, "the board bit alone does not turn the body");
}

/// Trick choice (M5, `skate_core::living_world::npc_tricks`): every line gets an ollie slot with
/// 80 frames of air. The host re-picks it from the character's profile table (retail mode), sends
/// a `Trick` event per slot and keeps the record; a client cursor mirroring the branch and trick
/// records shows the same trick. A mod's `recorded` mode keeps the line's ollie; the reset
/// restores the profile pick.
#[test]
fn living_world_npc_skaters_pick_tricks_from_their_profile_and_clients_mirror_them() {
    use skate_core::living_world::npc_tricks::TrickProfile;
    use skate_core::living_world::replay::{node_events, node_flags, ReplayJump, TrickRecord};
    const OLLIE: i16 = 128;
    const KICKFLIP: i16 = 96;
    const HEELFLIP: i16 = 92;
    let with_slots = || {
        let mut d = data();
        let mut lines = (*d.npc.lines).clone();
        for l in lines.values_mut() {
            l.jumps = vec![ReplayJump { start_position: [0.0; 3], start_velocity: [0.0; 3], offset: [0.0; 3], trick: OLLIE, spins: 0, flags: 0 }];
            for (n, node) in l.nodes.iter_mut().enumerate() {
                node.flags = if (100..120).contains(&n) { node_flags::AIRBORNE } else { 0 };
            }
            l.nodes[100].event = node_events::START_TRICK;
            l.nodes[100].jump = Some(0);
            l.nodes[120].event = node_events::END_TRICK;
        }
        d.npc.lines = Arc::new(lines);
        let mut t = skate_data::living_world::SkaterTrickProfiles::default();
        t.profiles.insert("default".into(), TrickProfile { regular: vec![(KICKFLIP, 1.0), (HEELFLIP, 1.0)], nollie: vec![] });
        d.npc.tricks = Arc::new(t);
        d
    };
    let tricks = |a: &App| a.world().resource::<Seen>().0.iter().filter_map(|e| if let NpcSkaterEvent::Trick { record, .. } = e { Some(record.clone()) } else { None }).collect::<Vec<TrickRecord>>();
    let mut a = app_with(11, with_slots());
    run(&mut a, 20.0, 60.0);
    let seen = tricks(&a);
    assert!(seen.len() >= 3, "trick slots passed: {}", seen.len());
    assert!(seen.iter().all(|r| r.recorded == OLLIE && (r.chosen == KICKFLIP || r.chosen == HEELFLIP)), "{seen:?}");
    assert!(seen.iter().any(|r| r.chosen == KICKFLIP) && seen.iter().any(|r| r.chosen == HEELFLIP), "both table entries come up: {seen:?}");
    // A client rebuilding each cursor from the records shows the host's trick.
    let lines = a.world().resource::<PopulationState>().npc.lines.clone();
    let mut q = a.world_mut().query::<(&NpcSkater, &NpcReplay)>();
    for (n, r) in q.iter(a.world()) {
        let mut c = LineCursor::spawn(&*lines, n.start_line, 0);
        c.advance(r.cursor.frames as u32, &*lines, &mut skate_core::living_world::replay::Decider::Mirror(&r.branches, &r.tricks), &mut Vec::new());
        assert_eq!(c.current_trick(), r.cursor.current_trick(), "npc {:?}", n.id);
        assert_eq!(c.frames, r.cursor.frames);
    }
    // A mod's recorded mode: the line's ollie.
    let mut a = app_with(11, with_slots());
    a.world_mut().resource_mut::<LivingWorldSettings>().npc_tricks.mode = skate_core::living_world::npc_tricks::TrickMode::Recorded;
    run(&mut a, 20.0, 60.0);
    let seen = tricks(&a);
    assert!(!seen.is_empty() && seen.iter().all(|r| r.chosen == OLLIE), "{seen:?}");
    a.world_mut().resource_mut::<LivingWorldSettings>().reset_mod_overrides();
    assert_eq!(a.world().resource::<LivingWorldSettings>().npc_tricks, Default::default());
}

/// Retail obstacle avoider on the replay tier: a player standing just right of an NPC's line,
/// 3 m ahead, stops it (cap 0: the cursor is held back); once the player leaves, it rides on.
#[test]
fn living_world_npc_skater_stops_for_a_player_standing_on_its_line() {
    let mut a = app(11);
    a.init_resource::<super::npc_avoid::AvoidTrack>().init_resource::<Pin>();
    a.add_systems(Update, super::npc_avoid::avoid.after(apply_records).before(advance));
    let mut found = None;
    for _ in 0..30 {
        run(&mut a, 0.5, 60.0);
        if let Some(n) = npcs(&mut a).into_iter().next() {
            found = Some(n);
            break;
        }
    }
    let (id, p, ..) = found.expect("an NPC spawned");
    // Lines ride +x; right of +x is -z.
    a.world_mut().resource_mut::<Pin>().0 = Some([p[0] + 3.0, p[1], p[2] - 0.5]);
    run(&mut a, 3.0, 60.0);
    let held = npcs(&mut a).into_iter().find(|n| n.0 == id).expect("still live");
    assert!(held.1[0] - p[0] < 3.0, "stopped short of the player: moved {:.2} m", held.1[0] - p[0]);
    let events = std::mem::take(&mut a.world_mut().resource_mut::<Seen>().0);
    assert!(
        events.iter().any(|e| matches!(e, NpcSkaterEvent::Avoid { id: i, mode: skate_core::living_world::avoid::AvoidMode::SlowDown, .. } if *i == id)),
        "an npc_avoid slow_down event: {events:?}"
    );
    // The player leaves (behind the NPC, out of its cone): it rides on.
    a.world_mut().resource_mut::<Pin>().0 = Some([held.1[0] - 20.0, p[1], p[2] - 20.0]);
    run(&mut a, 2.0, 60.0);
    let after = npcs(&mut a).into_iter().find(|n| n.0 == id).expect("still live");
    assert!(after.1[0] - held.1[0] > 10.0, "rides on: moved {:.2} m", after.1[0] - held.1[0]);
}

/// Retail bail respawn delay of an ambient skater: 5 s, clamped to 1.5..7.9 s (`sub_8246EE30`).
#[test]
fn living_world_npc_respawn_delay_is_retails_and_clamped() {
    let mut s = super::npc_sim::SimulatedTierSettings::default();
    assert_eq!(s.respawn_ticks(), 300);
    s.respawn_seconds = 20.0;
    assert_eq!(s.respawn_ticks(), (7.9f32 * 60.0).round() as u32);
    s.respawn_seconds = 0.5;
    assert_eq!(s.respawn_ticks(), 90);
    // A mod may widen the clamp.
    s.respawn_min = 0.0;
    assert_eq!(s.respawn_ticks(), 30);
}
