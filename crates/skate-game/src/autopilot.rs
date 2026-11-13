//! Player autopilot: the player's skater driven by the NPC skater brain (ask 546; doc 42 "Player autopilot"). Not
//! retail: a testing aid for challenges and the career, and a mod surface. The possessed NPC (`Possessed`) is a scripted
//! NPC skater whose simulated body is the player's own: `possess_in` swaps the player's physics context, runtime,
//! controls and camera into its `NpcSim` right before `npc_sim::simulate`, `possess_out` swaps them back right after,
//! so the brain and its step run unchanged on the player, and scoring, challenges and the career see a normal player.
use crate::living_world::npc_sim::{self, NpcSim};
use crate::living_world::npc_skaters::{NpcRaceClock, NpcSkaterIndex};
use crate::world_audio::NpcSkaterAudio;
use crate::living_world::PopulationState;
use crate::physics::{GamePhysics, PlayerControls, SkaterRuntime};
use bevy::prelude::*;
use skate_core::living_world::replay::{LineCursor, ReplayLine};
use skate_core::living_world::LivingWorldId;

/// The NPC skater whose brain drives the player.
#[derive(Component)]
pub(crate) struct Possessed;

/// The brain's character when none is given: its AI profile and trick tables (SKATE_AUTOPILOT_CHARACTER overrides).
pub(crate) const DEFAULT_CHARACTER: &str = "danny_way";

#[derive(Resource, Default)]
pub(crate) struct Autopilot {
    /// Wanted on (dev: SKATE_AUTOPILOT=1).
    pub requested: bool,
    /// The brain's character (a look key: its AI profile and trick tables).
    pub character: String,
    npc: Option<LivingWorldId>,
    /// Where the player stood at engaging (the possessed NPC's start slot: it gets its body there at once, pinned).
    start: Option<([f32; 3], f32)>,
    /// The node of the start line nearest the player (the possessed NPC's cursor starts there, not at node 0).
    start_node: u32,
    /// "No line near" was logged (once per wait).
    waiting_logged: bool,
    /// The player's body is inside the possessed NPC's sim (between `possess_in` and `possess_out`).
    swapped: bool,
    /// The brain stepped the player last tick: the player's own controls and physics step stand down.
    driving: bool,
    /// Pad input seen since engaging (any input after a quiet tick releases the player).
    quiet: bool,
    /// The line to start on instead of the nearest (dev: SKATE_AUTOPILOT_LINE=<32 hex digits>).
    pub line: Option<[u8; 16]>,
    /// Lines to ride in turn in free roam (dev: SKATE_AUTOPILOT_LINES=<id>,<id>,...): when the brain leaves the current
    /// one it is teleported to the next one's start; the list loops. Ours, a test aid.
    pub lines: Vec<[u8; 16]>,
    list_at: usize,
    /// Race starts seen (`Races::placements`): a new one restarts the brain at the start slot, on the race's lines.
    race_placements: u32,
    /// Ticks to wait before engaging again, so the start slot teleport lands first.
    restart_in: u32,
    /// Held at its spot (a race before GO): the body is pinned, the brain does not ride.
    hold: bool,
    /// The player's deck last tick, to see a teleport (a jump of more than `TELEPORT_JUMP` in one tick).
    last_deck: Option<[f32; 3]>,
    /// Bails per line (line, tick of each bail) and the lines it gave up on (ours: learning from bails); cleared by a
    /// teleport or a race start.
    bails: Vec<([u8; 16], u64)>,
    avoid: Vec<[u8; 16]>,
    was_bailed: bool,
    ticks: u64,
    /// Where it was when it last made progress (position, tick): no `CRAWL_DISTANCE` in `CRAWL_TICKS` is a wedge.
    crawl_anchor: Option<([f32; 3], u64)>,
    /// The line of the skater the brain follows (a follow the leader host sets it every frame to the leader's line): it
    /// starts on that line, and at a line end takes only it, so it rides behind the leader instead of walking to
    /// another line of the challenge's set. Ours, a test aid.
    pub(crate) follow: Option<[u8; 16]>,
    /// The follow line last put on the possessed NPC as its line filter.
    applied_follow: Option<[u8; 16]>,
    /// The lines a challenge with an area (a film's boundary) keeps the brain on: it starts on one and chains only
    /// to them, so a test run stays where the objectives are. Ours, a test aid; `follow` wins over it.
    pub(crate) area: Option<std::sync::Arc<Vec<[u8; 16]>>>,
    applied_area: Option<std::sync::Arc<Vec<[u8; 16]>>>,
    /// A host teleported the player (a contest round start): pick a line again where it arrives. The deck jump test
    /// misses these while the brain drives (its step resets `last_deck`).
    pub(crate) host_teleported: bool,
}

impl Autopilot {
    pub(crate) fn from_env() -> Self {
        let on = std::env::var("SKATE_AUTOPILOT").is_ok_and(|v| v == "1");
        let character = std::env::var("SKATE_AUTOPILOT_CHARACTER").unwrap_or_else(|_| DEFAULT_CHARACTER.to_owned());
        let lines: Vec<[u8; 16]> = std::env::var("SKATE_AUTOPILOT_LINES").ok().map(|v| v.split(',').filter_map(parse_line).collect()).unwrap_or_default();
        let line = lines.first().copied().or_else(|| std::env::var("SKATE_AUTOPILOT_LINE").ok().and_then(|h| parse_line(&h)));
        Self { requested: on, character, line, lines, ..default() }
    }

    /// The possessed NPC that drives the player, while engaged.
    pub(crate) fn npc_id(&self) -> Option<LivingWorldId> {
        self.npc
    }

    /// The brain steps the player: the player's own controls and physics step skip this tick.
    pub(crate) fn driving(&self) -> bool {
        self.driving
    }

    /// The possessed NPC's sim holds the player's body right now (its body must not go back to the pool).
    pub(crate) fn holds_player(&self) -> bool {
        self.swapped
    }
}

/// True when the autopilot drives the player (the player's controls and physics step stand down).
pub(crate) fn driving(autopilot: Option<&Autopilot>) -> bool {
    autopilot.is_some_and(Autopilot::driving)
}

/// Engage and release: spawns the possessed NPC on the line nearest the player, and on release (not wanted any more,
/// or the pad used) despawns it and hands the player back to the pad.
#[allow(clippy::too_many_arguments)]
pub(crate) fn engage(
    mut commands: Commands,
    autopilot: Option<ResMut<Autopilot>>,
    population: Option<ResMut<PopulationState>>,
    index: Option<Res<NpcSkaterIndex>>,
    physics: Option<Res<GamePhysics>>,
    skater: Option<ResMut<SkaterRuntime>>,
    controls: Option<ResMut<PlayerControls>>,
    input: Option<Res<crate::input::PublishedTickInput>>,
    races: Option<ResMut<crate::race_host::Races>>,
    mut possessed: Query<(Has<Possessed>, &mut crate::living_world::npc_skaters::NpcReplay)>,
    contests: Option<Res<crate::contest_host::Contests>>,
    ftl: Option<Res<crate::ftl_host::FollowTheLeader>>,
) {
    let (Some(mut autopilot), Some(mut population), Some(index), Some(physics), Some(mut skater), Some(mut controls), Some(input)) =
        (autopilot, population, index, physics, skater, controls, input) else { return };
    if !autopilot.requested && autopilot.npc.is_none() {
        return;
    }
    // A contest pins the player from setup_round through the countdown and again from end_round (retail PinAI).
    autopilot.hold = crate::input::player_pinned(contests.as_deref(), races.as_deref(), ftl.as_deref());
    // Races (ours): the brain restarts at the start slot on every race start, waits for GO, and logs the result.
    if let Some(mut races) = races {
        if races.placements != autopilot.race_placements {
            autopilot.race_placements = races.placements;
            autopilot.restart_in = RESTART_TICKS;
            autopilot.avoid.clear();
            autopilot.bails.clear();
            info!("AUTOPILOT race start: restarting at the start slot");
        }
        if let Some(result) = races.result.take() {
            info!("AUTOPILOT_RESULT {result} brain={}", autopilot.character);
            // The race's lines go with it: pick up an ambient line again.
            autopilot.restart_in = RESTART_TICKS;
        }
    }
    // Any teleport (--teleport, the teleport menu, a challenge start): pick a line again where the player arrived.
    let deck = physics.board.bodies()[skate_core::physics::board::BodyId::Deck.index()].rates.position;
    let deck = [deck.x, deck.y, deck.z];
    if std::mem::take(&mut autopilot.host_teleported) && autopilot.npc.is_some() && autopilot.restart_in == 0 {
        autopilot.restart_in = RESTART_TICKS;
        autopilot.avoid.clear();
        autopilot.bails.clear();
        info!("AUTOPILOT a host teleported the player: picking a line again there");
    }
    if let Some(last) = autopilot.last_deck.replace(deck) {
        let moved = ((deck[0] - last[0]).powi(2) + (deck[1] - last[1]).powi(2) + (deck[2] - last[2]).powi(2)).sqrt();
        if moved > TELEPORT_JUMP && autopilot.npc.is_some() && autopilot.restart_in == 0 {
            autopilot.restart_in = RESTART_TICKS;
            autopilot.avoid.clear();
            autopilot.bails.clear();
            info!("AUTOPILOT the player moved {moved:.1} m in a tick (a teleport): picking a line again there");
        }
    }
    if autopilot.restart_in > 0 {
        if autopilot.swapped {
            return;
        }
        if let Some(id) = autopilot.npc.take() {
            population.despawn_scripted(id);
            autopilot.start = None;
            autopilot.driving = false;
            controls.ai_driven = false;
            controls.action_intents.clear();
            controls.named_intents.clear();
            skater.ai_physics = None;
        }
        autopilot.restart_in -= 1;
        return;
    }
    let pad = input.0.actions().values().iter().any(|v| v.abs() > 0.3);
    if autopilot.npc.is_some() {
        if !pad {
            autopilot.quiet = true;
        } else if autopilot.quiet && autopilot.requested {
            info!("AUTOPILOT released by the pad");
            autopilot.requested = false;
        }
    }
    // The possessed brain's line while a line list runs (free roam only).
    let on_line = autopilot.npc.filter(|_| !autopilot.lines.is_empty() && population.scripted_lines.is_none())
        .and_then(|id| index.0.get(&id).and_then(|&e| possessed.get(e).ok()).filter(|(p, _)| *p).map(|(_, r)| r.cursor.line));
    match (autopilot.requested, autopilot.npc) {
        (true, None) => {
            use skate_core::physics::board::BodyId;
            let deck = physics.board.bodies()[BodyId::Deck.index()].rates;
            let (p, v) = (deck.position, deck.linear_velocity);
            let (position, velocity) = ([p.x, p.y, p.z], [v.x, v.y, v.z]);
            let speed = (velocity[0] * velocity[0] + velocity[2] * velocity[2]).sqrt();
            let heading = if speed > 0.5 { velocity[0].atan2(velocity[2]) } else { player_heading(&physics) };
            let forward = [heading.sin(), 0.0, heading.cos()];
            let lines = crate::living_world::npc_skaters::npc_lines(&population);
            // In a challenge with its own lines (a race) only those count, as for its AI racers.
            let scripted = population.scripted_lines.clone();
            let avoid = autopilot.avoid.clone();
            let area = autopilot.area.clone();
            let allowed = |id: &[u8; 16]| scripted.as_ref().is_none_or(|s| s.contains(id)) && !avoid.contains(id)
                && area.as_ref().is_none_or(|a| a.contains(id));
            let given = autopilot.line.filter(|l| {
                let known = lines.contains_key(l);
                if !known {
                    warn!("AUTOPILOT line {} is not on this map; taking the nearest", hex(l));
                }
                known
            });
            // In a race, the line starting nearest its slot, as the AI racers pick (`choose_line_from`, b972 §4): the
            // nearest node of any race line could lie 5 m off and the brain walked to it on foot at GO.
            let race_pick = scripted.as_ref().and_then(|ids| {
                let race_lines: std::collections::BTreeMap<[u8; 16], ReplayLine> =
                    ids.iter().filter_map(|id| lines.get(id).map(|l| (*id, l.clone()))).collect();
                let ctx = skate_core::living_world::replay::BranchContext { position, forward, speed: 0.0, players: &[], others: &[],
                    in_use: &[], preferred_skill: -1, online: false, chain: skate_core::living_world::replay::ChainConfig::retail(),
                    tricks: Default::default(), body: None, valid_lines: None };
                skate_core::living_world::replay::choose_line_from(&race_lines, position, &ctx)
            });
            // Following a skater: its line, from the node nearest the player.
            let follow_pick = autopilot.follow.filter(|l| lines.contains_key(l))
                .and_then(|l| nearest_node(lines.get(&l).into_iter(), position, forward, f32::INFINITY));
            let pick = match (given, follow_pick.or(race_pick)) {
                (Some(l), _) => nearest_node(lines.get(&l).into_iter(), position, forward, f32::INFINITY),
                (None, Some(p)) => Some(p),
                (None, None) => nearest_node(lines.values().filter(|l| allowed(&l.id)), position, forward, PICK_RADIUS),
            };
            let Some((line, node)) = pick else {
                if !autopilot.waiting_logged {
                    autopilot.waiting_logged = true;
                    warn!("AUTOPILOT no AI line within {PICK_RADIUS} m of the player at {position:?}; waiting");
                }
                return;
            };
            autopilot.waiting_logged = false;
            autopilot.start_node = node;
            let character = autopilot.character.clone();
            autopilot.npc = population.spawn_scripted(line, &character, 0, position, heading);
            autopilot.applied_follow = None;
            autopilot.applied_area = None;
            autopilot.start = Some((position, heading));
            autopilot.quiet = false;
            info!("AUTOPILOT engaged: brain {character} on line {} node {node} at {position:?} ({speed:.1} m/s)", hex(&line));
        }
        (true, Some(_)) if on_line.is_some() => {
            // A line list: the brain left the current line, so on to the next one's start.
            let current = autopilot.lines[autopilot.list_at];
            if on_line.is_some_and(|l| l != current) && !autopilot.swapped {
                autopilot.list_at = (autopilot.list_at + 1) % autopilot.lines.len();
                let next = autopilot.lines[autopilot.list_at];
                let lines = crate::living_world::npc_skaters::npc_lines(&population);
                match lines.get(&next).and_then(|l| l.nodes.first()) {
                    Some(n) => {
                        let h = n.step[0].atan2(n.step[2]);
                        let (s, c) = h.sin_cos();
                        let m = [[c, 0.0, -s, 0.0], [0.0, 1.0, 0.0, 0.0], [s, 0.0, c, 0.0], [n.position[0], n.position[1], n.position[2], 1.0]];
                        if let Err(e) = skater.travel_ex(m, None, true) {
                            warn!("AUTOPILOT_LINES teleport {e}");
                        }
                        autopilot.line = Some(next);
                        autopilot.restart_in = RESTART_TICKS;
                        info!("AUTOPILOT_LINES left {}: line {} of {} {} at its start {:?}", hex(&current), autopilot.list_at + 1, autopilot.lines.len(), hex(&next), n.position);
                    }
                    None => warn!("AUTOPILOT_LINES line {} is not on this map", hex(&next)),
                }
            }
        }
        (true, Some(id)) => {
            if let Some(&e) = index.0.get(&id) {
                if let Ok((false, mut replay)) = possessed.get_mut(e) {
                    let lines = crate::living_world::npc_skaters::npc_lines(&population);
                    replay.cursor = LineCursor::spawn(&*lines, replay.cursor.line, autopilot.start_node);
                    // Pinned at the player's spot like a racer at its slot: it takes a body at once (any distance, any
                    // node) and its line waits; `possess_in` releases it once it holds the player.
                    commands.entity(e).insert((Possessed, NpcRaceClock { moving: false, scale: 1.0, start: autopilot.start, ..default() }));
                }
                if autopilot.follow != autopilot.applied_follow || (autopilot.follow.is_none() && autopilot.area != autopilot.applied_area) {
                    match (autopilot.follow, autopilot.area.clone()) {
                        (Some(line), _) => {
                            info!("AUTOPILOT follows line {}", hex(&line));
                            commands.entity(e).insert(crate::living_world::npc_skaters::NpcLineFilter(std::sync::Arc::new(vec![line])));
                        }
                        (None, Some(area)) => {
                            info!("AUTOPILOT keeps to the challenge area: {} lines", area.len());
                            commands.entity(e).insert(crate::living_world::npc_skaters::NpcLineFilter(area));
                        }
                        (None, None) => {
                            commands.entity(e).remove::<crate::living_world::npc_skaters::NpcLineFilter>();
                        }
                    }
                    autopilot.applied_follow = autopilot.follow;
                    autopilot.applied_area = autopilot.area.clone();
                }
            }
        }
        (false, Some(id)) if !autopilot.swapped => {
            population.despawn_scripted(id);
            autopilot.npc = None;
            autopilot.start = None;
            autopilot.driving = false;
            controls.ai_driven = false;
            controls.action_intents.clear();
            controls.named_intents.clear();
            skater.ai_physics = None;
            info!("AUTOPILOT off: the pad drives the player");
        }
        _ => {}
    }
}

/// How far from the player a line may be to pick it up (ours: the nearest node of any line).
const PICK_RADIUS: f32 = 30.0;
/// Ticks between a race start's slot teleport and engaging there again.
const RESTART_TICKS: u32 = 10;
/// More than this (m) in one tick is a teleport, not riding (14 m/s is 0.23 m per tick).
const TELEPORT_JUMP: f32 = 10.0;
/// Bails on one line within this many ticks (60 s at 60 Hz) that make it leave the line.
const BAIL_WINDOW_TICKS: u64 = 3600;
const BAILS_TO_LEAVE: usize = 2;
/// A wedge: less than this far (m) in this many ticks (6 s at 60 Hz) while riding.
const CRAWL_DISTANCE: f32 = 3.0;
const CRAWL_TICKS: u64 = 360;

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// OURS (not retail; retail starts AI skaters at a line start): the line and node nearest `position`, a node whose
/// recorded direction points away from `forward` counting 10 m farther, within `radius`.
fn nearest_node<'a>(lines: impl Iterator<Item = &'a ReplayLine>, position: [f32; 3], forward: [f32; 3], radius: f32) -> Option<([u8; 16], u32)> {
    let mut best: Option<(f32, [u8; 16], u32)> = None;
    for line in lines {
        // The last two nodes leave no room to ride.
        for (i, n) in line.nodes.iter().enumerate().take(line.nodes.len().saturating_sub(2)) {
            let d = [n.position[0] - position[0], n.position[1] - position[1], n.position[2] - position[2]];
            let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if dist > radius {
                continue;
            }
            let along = n.step[0] * forward[0] + n.step[2] * forward[2];
            let score = dist + if along < 0.0 { 10.0 } else { 0.0 };
            if best.is_none_or(|b| score < b.0) {
                best = Some((score, line.id, i as u32));
            }
        }
    }
    best.map(|(_, line, node)| (line, node))
}

fn player_heading(physics: &GamePhysics) -> f32 {
    let c = physics.board.part_transforms()[skate_core::physics::board::BodyId::Deck.index()].basis.columns[2];
    c[0].atan2(c[2])
}

fn parse_line(text: &str) -> Option<[u8; 16]> {
    let t = text.trim();
    if t.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&t[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

/// Right before `simulate`: the player's body into the possessed NPC's sim.
pub(crate) fn possess_in(
    autopilot: Option<ResMut<Autopilot>>,
    physics: Option<ResMut<GamePhysics>>,
    skater: Option<ResMut<SkaterRuntime>>,
    controls: Option<ResMut<PlayerControls>>,
    camera: Option<ResMut<crate::camera::CameraRuntime>>,
    mut sims: Query<(&mut NpcSim, &mut NpcRaceClock), With<Possessed>>,
) {
    let (Some(mut autopilot), Some(mut physics), Some(mut skater), Some(mut controls), Some(mut camera)) =
        (autopilot, physics, skater, controls, camera) else { return };
    let Ok((mut sim, mut clock)) = sims.single_mut() else { return };
    if clock.moving == autopilot.hold {
        clock.moving = !autopilot.hold;
    }
    // Held (a race before GO): its line clock waits too, as the AI racers' does (else at GO the line is meters ahead).
    if autopilot.hold {
        clock.held += crate::living_world::npc_skaters::FRAMES_PER_TICK;
    }
    sim.own_speed = true;
    // One arming tick: from the next tick the player's own step stands down (else the player steps twice once).
    if !autopilot.driving {
        autopilot.driving = true;
        info!("AUTOPILOT the brain takes the player from the next tick");
        return;
    }
    npc_sim::swap_player(&mut sim, &mut physics, &mut skater, &mut controls, &mut camera);
    autopilot.swapped = true;
}

/// Right after `simulate`: the player's body back; the possessed NPC stays unseen and unheard (the player is drawn and
/// heard as always).
pub(crate) fn possess_out(
    autopilot: Option<ResMut<Autopilot>>,
    physics: Option<ResMut<GamePhysics>>,
    skater: Option<ResMut<SkaterRuntime>>,
    controls: Option<ResMut<PlayerControls>>,
    camera: Option<ResMut<crate::camera::CameraRuntime>>,
    mut sims: Query<(&mut NpcSim, &crate::living_world::npc_skaters::NpcReplay, Option<&mut Visibility>, Option<&mut NpcSkaterAudio>), With<Possessed>>,
    mut calls: Local<u64>,
    population: Option<Res<PopulationState>>,
) {
    let (Some(mut autopilot), Some(mut physics), Some(mut skater), Some(mut controls), Some(mut camera)) =
        (autopilot, physics, skater, controls, camera) else { return };
    let races_running = population.is_some_and(|p| p.scripted_lines.is_some());
    let Ok((mut sim, replay, visibility, audio)) = sims.single_mut() else {
        if autopilot.swapped {
            error!("AUTOPILOT the possessed NPC lost its sim while it held the player's body; physics stops");
            physics.failed = true;
            autopilot.swapped = false;
        }
        autopilot.driving = false;
        return;
    };
    // Learning from bails (ours): two bails on one line within a minute and it leaves that line for another near
    // one; not in a race (`scripted`: its course is all it may ride).
    autopilot.ticks += 1;
    let bailed = sim.bailed();
    if bailed && !autopilot.was_bailed && !autopilot.hold {
        let (line, now) = (replay.cursor.line, autopilot.ticks);
        autopilot.bails.retain(|(_, t)| now - t < BAIL_WINDOW_TICKS);
        autopilot.bails.push((line, now));
        let repeats = autopilot.bails.iter().filter(|(l, _)| *l == line).count();
        if repeats >= BAILS_TO_LEAVE && !races_running && !autopilot.avoid.contains(&line) {
            autopilot.avoid.push(line);
            autopilot.restart_in = RESTART_TICKS;
            info!("AUTOPILOT bailed {repeats} times on line {} within a minute: leaving it for another line", hex(&line));
        }
    }
    autopilot.was_bailed = bailed;
    // Course correction (ours): wedged on a curb or ledge (under CRAWL_DISTANCE moved in CRAWL_TICKS, riding, not
    // bailed or held) and it gives that spot up for another line nearby (in a race, the nearest node of the course).
    let at = sim.deck_position();
    let now = autopilot.ticks;
    match autopilot.crawl_anchor {
        Some((p, _)) if bailed || autopilot.hold || dist(p, at) > CRAWL_DISTANCE => autopilot.crawl_anchor = Some((at, now)),
        Some((_, since)) if now - since > CRAWL_TICKS && autopilot.restart_in == 0 => {
            let line = replay.cursor.line;
            if !races_running && !autopilot.avoid.contains(&line) {
                autopilot.avoid.push(line);
            }
            autopilot.restart_in = RESTART_TICKS;
            autopilot.crawl_anchor = None;
            info!("AUTOPILOT wedged: under {CRAWL_DISTANCE} m in {} s on line {} node {}: picking a line again", CRAWL_TICKS / 60, hex(&line), replay.cursor.node);
        }
        None => autopilot.crawl_anchor = Some((at, now)),
        _ => {}
    }
    if autopilot.swapped {
        let deck = sim.deck_position();
        npc_sim::swap_player(&mut sim, &mut physics, &mut skater, &mut controls, &mut camera);
        autopilot.swapped = false;
        // The brain's own moves (a bail respawn) are not teleports.
        let p = physics.board.bodies()[skate_core::physics::board::BodyId::Deck.index()].rates.position;
        autopilot.last_deck = Some([p.x, p.y, p.z]);
        *calls += 1;
        // Every 2 s: where the brain has the player (logs must diagnose).
        if *calls % 120 == 1 {
            let v = physics.board.bodies()[skate_core::physics::board::BodyId::Deck.index()].rates.linear_velocity;
            info!("AUTOPILOT deck at [{:.1}, {:.1}, {:.1}] {:.1} m/s state {:?} line {} node {}", deck[0], deck[1], deck[2],
                (v.x * v.x + v.z * v.z).sqrt(), skater.player_state.current(), hex(&replay.cursor.line), replay.cursor.node);
        }
    }
    if let Some(mut v) = visibility.filter(|v| **v != Visibility::Hidden) {
        *v = Visibility::Hidden;
    }
    if let Some(mut a) = audio {
        a.state = None;
    }
}

pub(crate) struct AutopilotPlugin;
impl Plugin for AutopilotPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Autopilot::from_env());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::living_world::replay::ReplayNode;

    fn line(id: u8, points: &[[f32; 3]], step: [f32; 3]) -> ReplayLine {
        let nodes = points.iter().map(|&position| ReplayNode { position, step, board: [128; 4], skater: [128; 4], frames: 10,
            event: 0, flags: 0, jump: None, width: [0; 2] }).collect();
        ReplayLine { id: [id; 16], flags: 0, skill: -1, nodes, jumps: Vec::new(), groups: Vec::new() }
    }

    #[test]
    fn the_pick_is_the_nearest_node_heading_the_players_way() {
        let row = |z: f32| (0..6).map(|i| [i as f32 * 5.0, 0.0, z]).collect::<Vec<_>>();
        // Line 1 runs +x 2 m away, line 2 runs -x 1 m away: facing +x the nearer one counts 10 m farther.
        let lines = [line(1, &row(2.0), [0.1, 0.0, 0.0]), line(2, &row(-1.0), [-0.1, 0.0, 0.0])];
        assert_eq!(nearest_node(lines.iter(), [10.0, 0.0, 0.0], [1.0, 0.0, 0.0], PICK_RADIUS), Some(([1; 16], 2)));
        assert_eq!(nearest_node(lines.iter(), [10.0, 0.0, 0.0], [-1.0, 0.0, 0.0], PICK_RADIUS), Some(([2; 16], 2)));
        // The last two nodes are never picked, and nothing past the radius.
        assert_eq!(nearest_node(lines.iter(), [25.0, 0.0, 2.0], [1.0, 0.0, 0.0], PICK_RADIUS), Some(([1; 16], 3)));
        assert_eq!(nearest_node(lines.iter(), [500.0, 0.0, 0.0], [1.0, 0.0, 0.0], PICK_RADIUS), None);
    }

    #[test]
    fn a_line_id_parses_from_32_hex_digits() {
        let id = parse_line("006477746e001fa7e83266024b4ca82b").unwrap();
        assert_eq!(hex(&id), "006477746e001fa7e83266024b4ca82b");
        assert_eq!(parse_line("0064"), None);
        assert_eq!(parse_line("zz6477746e001fa7e83266024b4ca82b"), None);
    }
}
