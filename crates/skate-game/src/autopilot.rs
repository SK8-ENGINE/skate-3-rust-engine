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
}

impl Autopilot {
    pub(crate) fn from_env() -> Self {
        let on = std::env::var("SKATE_AUTOPILOT").is_ok_and(|v| v == "1");
        let character = std::env::var("SKATE_AUTOPILOT_CHARACTER").unwrap_or_else(|_| DEFAULT_CHARACTER.to_owned());
        let line = std::env::var("SKATE_AUTOPILOT_LINE").ok().and_then(|h| parse_line(&h));
        Self { requested: on, character, line, ..default() }
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
    mut possessed: Query<(Has<Possessed>, &mut crate::living_world::npc_skaters::NpcReplay)>,
) {
    let (Some(mut autopilot), Some(mut population), Some(index), Some(physics), Some(mut skater), Some(mut controls), Some(input)) =
        (autopilot, population, index, physics, skater, controls, input) else { return };
    if !autopilot.requested && autopilot.npc.is_none() {
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
            let given = autopilot.line.filter(|l| {
                let known = lines.contains_key(l);
                if !known {
                    warn!("AUTOPILOT line {} is not on this map; taking the nearest", hex(l));
                }
                known
            });
            let pick = match given {
                Some(l) => nearest_node(lines.get(&l).into_iter(), position, forward, f32::INFINITY),
                None => nearest_node(lines.values(), position, forward, PICK_RADIUS),
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
            autopilot.start = Some((position, heading));
            autopilot.quiet = false;
            info!("AUTOPILOT engaged: brain {character} on line {} node {node} at {position:?} ({speed:.1} m/s)", hex(&line));
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
    if !clock.moving {
        clock.moving = true;
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
) {
    let (Some(mut autopilot), Some(mut physics), Some(mut skater), Some(mut controls), Some(mut camera)) =
        (autopilot, physics, skater, controls, camera) else { return };
    let Ok((mut sim, replay, visibility, audio)) = sims.single_mut() else {
        if autopilot.swapped {
            error!("AUTOPILOT the possessed NPC lost its sim while it held the player's body; physics stops");
            physics.failed = true;
            autopilot.swapped = false;
        }
        autopilot.driving = false;
        return;
    };
    if autopilot.swapped {
        let deck = sim.deck_position();
        npc_sim::swap_player(&mut sim, &mut physics, &mut skater, &mut controls, &mut camera);
        autopilot.swapped = false;
        *calls += 1;
        // Every 2 s: where the brain has the player (logs must diagnose).
        if *calls % 120 == 1 {
            let v = physics.board.bodies()[skate_core::physics::board::BodyId::Deck.index()].rates.linear_velocity;
            info!("AUTOPILOT at [{:.1}, {:.1}, {:.1}] {:.1} m/s state {:?} line {} node {}", deck[0], deck[1], deck[2],
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
