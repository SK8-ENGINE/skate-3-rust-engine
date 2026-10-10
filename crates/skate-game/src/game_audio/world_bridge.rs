//! The bridge from the engine-facing world audio components (`crate::world_audio`) to the hosts'
//! internal seams (`world_sources::WorldOwners`, `npc_skaters::NpcSkaters`), and the read-back
//! (`WorldAudioInstance`, `WorldAudioStats`). Runs before `native::mixmap_frame`; the read-back
//! after both hosts. Inert (returns at once, touches nothing) while no component exists and
//! nothing is held.
//!
//! What the bridge fills where the engine leaves a field open (retail rules from the recomp gap
//! runs G1 / G2, spec `world-audio-hookin` §7.3):
//! - velocity: [`AudioVelocity`] or the transform's change over the frame;
//! - vehicle speed: |velocity|; acceleration (`+144`): the speed change per second;
//! - ped `+68` footsteps on: the 3 nearest peds within 50 m; `+148` / `+156`: the 3-D distance to
//!   the listener / 20 m; materials: 0 (retail's pavements);
//! - NPC skaters: the list order (remote players first, then `list_order`, then spawn order).
//!
//! Remote multiplayer players (non-retail extension, user decision 2026-10-03) get an
//! [`NpcSkaterAudio`] with a lite state ([`AudioState::rolling`]) from their root transform and
//! the ground's audio material under them ([`ground_material`]).
use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use skate_audio::world::traffic::{EngineRecord, VehicleState};

use skate_audio::world::speech_manager::Speaker;

use super::world_sources::{PED_LIST_RADIUS, WorldHeld, WorldOwners};

/// The security type bit (`S+96 == 64`).
const SECURITY: u32 = skate_audio::world::speech_manager::kind::SECURITY_GUARD;
use crate::world_audio::*;

/// Retail's footsteps-on rule: the first 3 of the nearest-first ped list (`S+68`, gap run G2;
/// `aud_speech/default` field `53364DFA09A499DD`).
pub(crate) const FOOTSTEP_PEDS: usize = 3;
/// The model's far threshold for regular peds (`S+156`, `aud_characteristics` `A27215A909135B62`).
pub(crate) const PED_FAR_THRESHOLD: f32 = 20.0;

/// The bridge's publish step: publishers that write components each frame (the mod host, the
/// ghost) run before it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct WorldAudioPublish;

/// A ghost NPC skater (dev / mods, spec §3.10): replays recorded audio states
/// ([`super::state_replay::ghost_states`]) as its [`NpcSkaterAudio`] state, looping, 60 rows per
/// second of game time, the entity following the board.
#[derive(Component, Clone, Debug)]
pub(crate) struct GhostSkater {
    pub(crate) states: Vec<AudioState>,
    /// The loose-board state per row (the board slide).
    pub(crate) loose: Vec<u8>,
    /// Rows played (fractional).
    pub(crate) at: f64,
}

impl GhostSkater {
    /// Load a state log's window as a ghost (see `ghost_states`).
    pub(crate) fn from_log(text: &str, from: f32, seconds: f32, anchor: [f32; 3]) -> Result<Self, String> {
        let (states, loose) = super::state_replay::ghost_window(text, from, seconds, anchor)?;
        if states.is_empty() {
            return Err("empty window".into());
        }
        Ok(Self { states, loose, at: 0.0 })
    }
}

/// Advance the ghosts: the row for this frame, its one-step push pulse OR-ed over the rows the
/// frame passed (as `skate_events::latch_pulses` does for the local player).
fn ghost_step(time: Res<Time>, mut ghosts: Query<(&mut GhostSkater, &mut NpcSkaterAudio, &mut Transform, &mut GlobalTransform)>) {
    for (mut ghost, mut npc, mut t, mut g) in &mut ghosts {
        let n = ghost.states.len();
        let before = ghost.at as usize;
        ghost.at += f64::from(time.delta_secs()) * 60.0;
        let now = ghost.at as usize;
        let mut s = ghost.states[now % n];
        s.push_trigger |= (before + 1..now).any(|i| ghost.states[i % n].push_trigger);
        if ghost.at >= (n * 1000) as f64 {
            ghost.at -= (n * 1000) as f64;
        }
        t.translation = Vec3::from_array(s.board_position);
        *g = GlobalTransform::from(*t);
        npc.state = Some(s);
        npc.loose_board = ghost.loose.get(now % n).copied().unwrap_or(0);
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<LivingWorldAudio>()
        .init_resource::<WorldAudioStats>()
        .init_resource::<Bridge>()
        .init_resource::<super::mod_world::OwnWorldOwners>()
        .add_message::<PedSpeechEvent>()
        .add_message::<PedTazerEvent>()
        .add_message::<PedBodyFallEvent>()
        .add_message::<NpcSkaterReactionEvent>()
        .add_message::<AnnouncerSpeechEvent>()
        .add_message::<VehicleHorn>()
        .add_message::<VehicleAlarm>()
        .add_systems(Update, (tag_remote_players, ghost_step, publish.in_set(WorldAudioPublish)).chain().before(super::native::mixmap_frame).after(crate::app::FrameSet::Animation))
        .add_systems(Update, read_back.after(super::world_sources::frame).after(super::npc_skaters::frame).after(super::world_speech::frame));
}

/// The bridge's memory between frames.
#[derive(Resource, Default)]
pub(crate) struct Bridge {
    /// Last frame's position and speed per entity (velocity / acceleration from the change).
    last: HashMap<Entity, ([f32; 3], f32)>,
    /// Horn holds from [`VehicleHorn`] / [`VehicleAlarm`]: (state, seconds left).
    horns: HashMap<Entity, (HornState, f32)>,
    /// Spawn order for skaters without a list order.
    order: HashMap<Entity, u32>,
    next_order: u32,
    /// Engine records by name, and the names already reported unknown.
    engines: HashMap<String, EngineRecord>,
    unknown: HashSet<String>,
    /// Entities that carry a `WorldAudioInstance` now.
    tagged: HashSet<Entity>,
    /// Something was published last frame (the owners must be cleared once when it stops).
    active: bool,
    /// Ped voices without a model in the install (reported once).
    unknown_models: HashSet<u32>,
    /// Speech values to set next frame (a repeated value goes through 0 first).
    speech_next: Vec<(Entity, i32)>,
    /// Tazer holds from [`PedTazerEvent`]: seconds left.
    tazers: HashMap<Entity, f32>,
    /// Body-fall keys from [`PedBodyFallEvent`]: the queue, the key on now (0 = the gap) and the
    /// seconds it has left.
    falls: HashMap<Entity, (std::collections::VecDeque<f32>, f32, f32)>,
    /// NPC skater reactions from [`NpcSkaterReactionEvent`]: the set and the seconds it has left.
    reactions: HashMap<Entity, (SkaterReactions, f32)>,
    /// The ped tuning's tazer hold (`world_tuning.ped_objects.tazer_seconds`), read once.
    tazer_seconds: Option<f32>,
    /// The summary log: seconds since the last line, and the running counts then.
    log_timer: f32,
    log_counts: (u64, u64, u64),
}

impl Bridge {
    /// A runtime tuning write changed the world tuning (`tuning.rs`): the cached engine records and
    /// the tazer hold are read again.
    pub(crate) fn retune(&mut self) {
        self.engines.clear();
        self.unknown.clear();
        self.tazer_seconds = None;
    }
}

/// The component's held reactions with an event's raised on top.
fn merge_reactions(held: SkaterReactions, event: SkaterReactions) -> SkaterReactions {
    let mut r = held;
    if event.slam {
        (r.slam, r.slam_by) = (true, event.slam_by);
    }
    if event.slam_b {
        (r.slam_b, r.slam_b_by) = (true, event.slam_b_by);
    }
    if event.trick {
        (r.trick, r.trick_by) = (true, event.trick_by);
    }
    r.crash |= event.crash;
    r.chase |= event.chase;
    r
}

/// The ground's audio material under a point (`material_of_tag` of the first surface a 1.5 m line
/// down from 0.5 m above it hits; `NO_MATERIAL` when none): the same stock line query the wheel
/// lines use.
pub(crate) fn ground_material(physics: &crate::physics::GamePhysics, at: Vec3) -> u32 {
    use skate_core::math::Vector3;
    let start = Vector3::new(at.x, at.y + 0.5, at.z);
    let end = Vector3::new(at.x, at.y - 1.0, at.z);
    match physics.world().query_thin_line(start, end) {
        Ok(Some(hit)) => skate_audio::player::state::material_of_tag(hit.tag & 0x7F),
        _ => skate_audio::player::state::NO_MATERIAL,
    }
}

fn velocity(bridge: &Bridge, e: Entity, at: [f32; 3], given: Option<&AudioVelocity>, dt: f32) -> [f32; 3] {
    if let Some(v) = given {
        return v.0.to_array();
    }
    match bridge.last.get(&e) {
        Some((last, _)) if dt > 0.0 => std::array::from_fn(|i| (at[i] - last[i]) / dt),
        _ => [0.0; 3],
    }
}

fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The +Z axis of a transform (the game's forward).
fn forward(t: &GlobalTransform) -> [f32; 3] {
    let z = t.affine().matrix3.z_axis;
    let n = z.length();
    if n > 1e-6 { (z / n).to_array() } else { [0.0, 0.0, 1.0] }
}

/// Remote multiplayer players take the NPC skater instance(s) (non-retail extension): tag their
/// roots once; [`publish`] fills their lite state.
fn tag_remote_players(mut commands: Commands, remotes: Query<Entity, (With<crate::multiplayer::appearance::RemoteCharacter>, Without<NpcSkaterAudio>)>) {
    for e in &remotes {
        commands.entity(e).insert(NpcSkaterAudio { remote: true, ..Default::default() });
    }
}

impl Bridge {
    /// The car alarm's time left on a vehicle (None: not sounding).
    pub(crate) fn alarm_left(&self, vehicle: Entity) -> Option<f32> {
        self.horns.get(&vehicle).filter(|h| h.0 == HornState::Alarm).map(|h| h.1)
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn publish(
    mut bridge: ResMut<Bridge>,
    (mut owners, mut own): (ResMut<WorldOwners>, ResMut<super::mod_world::OwnWorldOwners>),
    mut skaters: ResMut<super::npc_skaters::NpcSkaters>,
    (living, alarm_rule): (Res<LivingWorldAudio>, Option<Res<CarAlarmRule>>),
    library: Option<Res<super::Library>>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    time: Res<Time>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    vehicles: Query<(Entity, &GlobalTransform, &TrafficAudio, Option<&AudioVelocity>, Has<crate::world_audio::OwnAudioInstance>)>,
    mut peds: Query<(Entity, &GlobalTransform, &mut PedAudio, Option<&AudioVelocity>, Has<crate::world_audio::OwnAudioInstance>)>,
    mut npcs: Query<(Entity, &GlobalTransform, &mut NpcSkaterAudio, Option<&AudioVelocity>)>,
    mut speech: MessageReader<PedSpeechEvent>,
    mut horns: MessageReader<VehicleHorn>,
    mut alarms: MessageReader<VehicleAlarm>,
    mut zaps: MessageReader<PedTazerEvent>,
    (mut falls, mut reacts): (MessageReader<PedBodyFallEvent>, MessageReader<NpcSkaterReactionEvent>),
) {
    let any = !vehicles.is_empty() || !peds.is_empty() || !npcs.is_empty();
    if owners.expected != living.expected {
        owners.expected = living.expected;
    }
    let photo = living.photo_flag || living.mod_photo_flag;
    if owners.photo_flag != photo {
        owners.photo_flag = photo;
    }
    if !any && !bridge.active && speech.is_empty() && horns.is_empty() && alarms.is_empty() && zaps.is_empty() && falls.is_empty() && reacts.is_empty() && bridge.speech_next.is_empty() && bridge.reactions.is_empty() {
        return;
    }
    let bridge = &mut *bridge;
    let dt = time.delta_secs();
    // PedestrianSpeech requests a line when the value changes: a repeat of the current value goes
    // through 0 for one frame first (a state graph re-entering its state does the same).
    for (e, value) in std::mem::take(&mut bridge.speech_next) {
        if let Ok((_, _, mut ped, _, _)) = peds.get_mut(e) {
            ped.speech_value = value;
        }
    }
    for e in speech.read() {
        if let Ok((_, _, mut ped, _, _)) = peds.get_mut(e.ped) {
            if ped.speech_value == e.value.0 && e.value.0 != 0 {
                ped.speech_value = 0;
                bridge.speech_next.push((e.ped, e.value.0));
            } else {
                ped.speech_value = e.value.0;
            }
        }
    }
    for h in horns.read() {
        bridge.horns.insert(h.vehicle, (HornState::Honk(h.kind.clamp(1, 5)), h.seconds.max(0.0)));
    }
    // The alarm's time: the rule's (retail 8 s), whole console frames past it (`AlarmTuning::hold_seconds`).
    let alarm_hold = alarm_rule.as_deref().map_or_else(|| AlarmTuning::default().hold_seconds(), |r| r.tuning().hold_seconds());
    for a in alarms.read() {
        bridge.horns.insert(a.vehicle, (HornState::Alarm, alarm_hold));
    }
    let tazer_seconds = *bridge.tazer_seconds.get_or_insert_with(|| library.as_deref().map_or_else(|| skate_audio::world::peds::PedObjectTuning::default().tazer_seconds, |l| l.world_tuning().ped_objects().tazer_seconds));
    for z in zaps.read() {
        bridge.tazers.insert(z.ped, z.seconds.unwrap_or(tazer_seconds).max(0.0));
    }
    for f in falls.read() {
        if f.kind != 0.0 && f.kind.is_finite() {
            bridge.falls.entry(f.ped).or_default().0.push_back(f.kind);
        }
    }
    // Each key is on for one console frame, then 0 for one frame (so a repeated key is a change).
    let step = skate_audio::mixmap::cadence::CONSOLE_DT;
    for (queue, now, left) in bridge.falls.values_mut() {
        *left -= dt;
        if *left <= 0.0 {
            if *now != 0.0 {
                *now = 0.0;
                *left = step;
            } else if let Some(k) = queue.pop_front() {
                *now = k;
                *left = step;
            }
        }
    }
    bridge.falls.retain(|e, (q, now, left)| peds.contains(*e) && (!q.is_empty() || *now != 0.0 || *left > 0.0));
    // A reaction is held for one console frame (the AI's bytes are per frame).
    for (_, left) in bridge.reactions.values_mut() {
        *left -= dt;
    }
    bridge.reactions.retain(|e, (_, left)| *left > 0.0 && npcs.contains(*e));
    for r in reacts.read() {
        let (set, left) = bridge.reactions.entry(r.skater).or_insert((SkaterReactions::default(), 0.0));
        *set = r.reaction.raise(*set, r.by);
        *left = step;
    }
    for left in bridge.tazers.values_mut() {
        *left -= dt;
    }
    bridge.tazers.retain(|e, left| *left > 0.0 && peds.contains(*e));
    for (_, left) in bridge.horns.values_mut() {
        *left -= dt;
    }
    bridge.horns.retain(|e, (_, left)| *left > 0.0 && vehicles.contains(*e));
    let camera = listener.single().map(|t| t.translation()).unwrap_or(Vec3::ZERO);

    // Vehicles (own-instance objects to `mod_world`'s host, doc 16 L3).
    owners.vehicles.clear();
    if !own.0.vehicles.is_empty() {
        own.0.vehicles.clear();
    }
    let mut seen = Vec::with_capacity(vehicles.iter().len());
    for (e, t, car, given, own_instance) in &vehicles {
        let at = t.translation().to_array();
        let v = velocity(bridge, e, at, given, dt);
        let speed = car.speed.unwrap_or_else(|| length(v)).max(0.0);
        let last_speed = bridge.last.get(&e).map_or(speed, |l| l.1);
        let load = car.load.unwrap_or(if dt > 0.0 { (speed - last_speed) / dt } else { 0.0 });
        let record = engine_record(bridge, library.as_deref(), &car.engine);
        let horn = bridge.horns.get(&e).map_or(car.horn, |h| h.0);
        let target = if own_instance { &mut own.0.vehicles } else { &mut owners.vehicles };
        target.insert(
            e.to_bits(),
            VehicleState { position: at, velocity: v, direction: forward(t), speed, load, horn: horn.word(), skid: i32::from(car.skidding), engine: record },
        );
        seen.push((e, at, speed));
    }

    // Peds: the nearest-first list within 50 m gives the footsteps-on rule.
    owners.peds.clear();
    if !own.0.peds.is_empty() {
        own.0.peds.clear();
    }
    let mut list: Vec<(f32, Entity)> = peds.iter().map(|(e, t, _, _, _)| (t.translation().distance(camera), e)).filter(|(d, _)| *d < PED_LIST_RADIUS).collect();
    list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.to_bits().cmp(&b.1.to_bits())));
    let nearest: HashSet<Entity> = list.iter().take(FOOTSTEP_PEDS).map(|x| x.1).collect();
    let tuning = library.as_deref().map(super::Library::world_tuning);
    for (e, t, ped, given, own_instance) in &peds {
        let at = t.translation().to_array();
        let v = velocity(bridge, e, at, given, dt);
        let distance = t.translation().distance(camera);
        // The model's `aud_characteristics` fields (the voice = the model, gap run G2).
        let model = ped.voice.and_then(|voice| {
            let m = tuning.and_then(|t| t.ped_model(voice));
            if m.is_none() && bridge.unknown_models.insert(voice) {
                warn!("AUDIO_WORLD ped voice {voice}: no aud_characteristics model in the install (defaults; rerun setup)");
            }
            m
        });
        let security = ped.close_range.unwrap_or(model.is_some_and(|m| m.kind == SECURITY));
        let far = model.map_or(PED_FAR_THRESHOLD, |m| if m.far > 0.0 { m.far } else { PED_FAR_THRESHOLD });
        let (measure, limit) = ped.speech_distance.unwrap_or((distance, far));
        let shoe = ped.shoe_class.unwrap_or(model.map_or(2, |m| if m.shoe_class == 0 { 2 } else { m.shoe_class }));
        let speaker = match (ped.voice, model) {
            (Some(voice), Some(m)) => Speaker { index: voice, kind: m.kind, variant: m.variant, partner: 0, word5: 0, word6: m.gender },
            (Some(voice), None) => Speaker { index: voice, ..Default::default() },
            _ => Speaker::default(),
        };
        let target = if own_instance { &mut own.0.peds } else { &mut owners.peds };
        target.insert(
            e.to_bits(),
            skate_audio::world::peds::PedState {
                position: at,
                velocity: v,
                speed: length(v),
                feet: ped.feet_down,
                footsteps: ped.footsteps_on.unwrap_or_else(|| nearest.contains(&e)),
                materials: ped.foot_materials.unwrap_or([0, 0]),
                speech_value: ped.speech_value,
                class: i32::from(shoe.clamp(1, 5)),
                weight: i32::from(ped.weight.clamp(1, 5)),
                close: security,
                speech_measure: measure,
                speech_limit: limit,
                voice: ped.voice.unwrap_or(0),
                speaker,
                level_select: skate_audio::world::speech_player::PedLevelSelect { security, ..Default::default() },
                tazing: ped.tazing || bridge.tazers.contains_key(&e),
                body_fall: bridge.falls.get(&e).map_or(ped.body_fall, |f| f.1),
            },
        );
        seen.push((e, at, 0.0));
    }

    // NPC / remote skaters, in list order.
    skaters.skaters.clear();
    let mut list: Vec<(bool, u32, u32, u64, AudioState, u32, u8, SkaterReactions)> = Vec::new();
    for (e, t, mut npc, given) in &mut npcs {
        let at = t.translation().to_array();
        if npc.remote {
            let v = velocity(bridge, e, at, given, dt);
            let f = forward(t);
            let material = physics.as_deref().map_or(skate_audio::player::state::NO_MATERIAL, |p| ground_material(p, t.translation()));
            npc.state = Some(AudioState::rolling(&LiteSkater { position: at, velocity: v, heading: f[0].atan2(f[2]), material, dt: dt.max(1e-4), ..Default::default() }));
        }
        seen.push((e, at, 0.0));
        let Some(state) = npc.state else { continue };
        let spawn = *bridge.order.entry(e).or_insert_with(|| {
            bridge.next_order += 1;
            bridge.next_order
        });
        let reactions = bridge.reactions.get(&e).map_or(npc.reactions, |(set, _)| merge_reactions(npc.reactions, *set));
        list.push((!npc.remote, if npc.list_order == 0 { u32::MAX } else { npc.list_order }, spawn, e.to_bits(), state, npc.voice.unwrap_or(0), npc.loose_board, reactions));
    }
    list.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
    skaters.skaters.extend(list.into_iter().map(|(_, _, _, id, state, voice, loose_board, reactions)| skate_audio::world::skaters::NpcSkaterAudioState { id, state, voice, loose_board: u32::from(loose_board.min(2)), reactions }));

    bridge.last.clear();
    for (e, at, speed) in seen {
        bridge.last.insert(e, (at, speed));
    }
    bridge.order.retain(|e, _| npcs.contains(*e));
    bridge.active = any;
}

fn engine_record(bridge: &mut Bridge, library: Option<&super::Library>, name: &str) -> EngineRecord {
    if let Some(r) = bridge.engines.get(name) {
        return *r;
    }
    let Some(library) = library else { return EngineRecord::default() };
    let tuning = library.world_tuning();
    let lower = name.to_ascii_lowercase();
    // A record name, or a living-world model mapped to its record (the setup's attribute chain).
    let record = tuning.engine(&lower).or_else(|| tuning.traffic_model(&lower).and_then(|r| tuning.engine(r)));
    let record = match record {
        Some(r) => r,
        None => {
            if bridge.unknown.insert(name.to_owned()) {
                warn!("AUDIO_WORLD unknown traffic engine record {name:?}: the default record (silent)");
            }
            tuning.engine("default").unwrap_or_default()
        }
    };
    bridge.engines.insert(name.to_owned(), record);
    record
}

/// `WorldAudioInstance` on the holders, and the stats.
#[allow(clippy::too_many_arguments)]
fn read_back(
    time: Res<Time>,
    mut commands: Commands,
    mut bridge: ResMut<Bridge>,
    held: Res<WorldHeld>,
    owners: Res<WorldOwners>,
    own: Res<super::mod_world::OwnWorldOwners>,
    skaters: Res<super::npc_skaters::NpcSkaters>,
    native: Option<Res<super::native::Native>>,
    mut stats: ResMut<WorldAudioStats>,
    instances: Query<&WorldAudioInstance>,
) {
    if !bridge.active && bridge.tagged.is_empty() {
        return;
    }
    let mut now: HashMap<Entity, WorldAudioInstance> = HashMap::new();
    for (list, slot, own) in [(&held.traffic, WorldAudioSlot::Traffic, false), (&held.peds, WorldAudioSlot::Ped, false), (&held.skaters, WorldAudioSlot::PlayerSlot, false), (&held.own_traffic, WorldAudioSlot::Traffic, true), (&held.own_peds, WorldAudioSlot::Ped, true)] {
        for &(id, instance) in list {
            now.insert(Entity::from_bits(id), WorldAudioInstance { slot, instance, own });
        }
    }
    for e in bridge.tagged.clone() {
        if !now.contains_key(&e) {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_remove::<WorldAudioInstance>();
            }
            bridge.tagged.remove(&e);
        }
    }
    for (e, tag) in now {
        if instances.get(e).ok() != Some(&tag) {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_insert(tag);
                bridge.tagged.insert(e);
            }
        }
    }
    let world = native.as_deref().map_or(super::native::WorldInstances::RETAIL, |n| n.world);
    let next = WorldAudioStats {
        vehicles: owners.vehicles.len(),
        peds: owners.peds.len(),
        skaters: skaters.skaters.len(),
        traffic_held: held.traffic.len(),
        peds_held: held.peds.len(),
        skaters_held: held.skaters.len(),
        instances: (world.traffic, world.peds, world.npc),
        more_audible: world != super::native::WorldInstances::RETAIL,
        speech_lines: held.speech_lines,
        waiting: held.waiting_traffic.iter().chain(&held.waiting_peds).map(|&id| Entity::from_bits(id)).collect(),
        own_vehicles: own.0.vehicles.len(),
        own_peds: own.0.peds.len(),
        own_traffic_held: held.own_traffic.len(),
        own_peds_held: held.own_peds.len(),
        own_instances: (super::mod_world::TRAFFIC, super::mod_world::PEDS),
        own_waiting: held.own_waiting_traffic.iter().chain(&held.own_waiting_peds).map(|&id| Entity::from_bits(id)).collect(),
    };
    // One summary line per second while anything is published (log-based checks; the test mod).
    if bridge.active {
        bridge.log_timer += time.delta_secs();
        if bridge.log_timer >= 1.0 {
            bridge.log_timer = 0.0;
            let footsteps = held.peds.iter().filter(|(id, _)| owners.peds.get(id).is_some_and(|p| p.footsteps)).count();
            let (posts, npc, speech) = bridge.log_counts;
            info!(
                "WORLD_AUDIO cars {}/audible {}, peds {}/{} (footsteps {}), skaters {}/{}, posts +{} (npc +{}), speech lines +{}",
                next.vehicles,
                next.traffic_held,
                next.peds,
                next.peds_held,
                footsteps,
                next.skaters,
                next.skaters_held,
                held.posts.saturating_sub(posts),
                held.npc_posts.saturating_sub(npc),
                held.speech_lines.saturating_sub(speech)
            );
            if next.own_vehicles + next.own_peds > 0 {
                info!(
                    "WORLD_AUDIO own instances: cars {}/audible {}, peds {}/{}, waiting {} (of {} / {})",
                    next.own_vehicles,
                    next.own_traffic_held,
                    next.own_peds,
                    next.own_peds_held,
                    next.own_waiting.len(),
                    next.own_instances.0,
                    next.own_instances.1
                );
            }
            bridge.log_counts = (held.posts, held.npc_posts, held.speech_lines);
        }
    } else {
        bridge.log_timer = 0.0;
    }
    if *stats != next {
        *stats = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<WorldOwners>().init_resource::<super::super::npc_skaters::NpcSkaters>().init_resource::<WorldHeld>();
        app.init_resource::<LivingWorldAudio>().init_resource::<WorldAudioStats>().init_resource::<Bridge>().init_resource::<super::super::mod_world::OwnWorldOwners>();
        app.add_message::<PedSpeechEvent>().add_message::<VehicleHorn>().add_message::<VehicleAlarm>().add_message::<PedTazerEvent>().add_message::<PedBodyFallEvent>().add_message::<NpcSkaterReactionEvent>();
        app.add_systems(Update, (publish, read_back).chain());
        app.world_mut().spawn((super::super::GameAudioListener, Transform::default(), GlobalTransform::default()));
        app
    }

    fn at(x: f32, z: f32) -> (Transform, GlobalTransform) {
        let t = Transform::from_xyz(x, 0.0, z);
        (t, GlobalTransform::from(t))
    }

    /// Doc 16 L3: a car or ped with `OwnAudioInstance` is published to the own-instance host
    /// (`mod_world::OwnWorldOwners`), never to retail's pools; its read-back tag says `own`.
    #[test]
    fn own_instance_objects_go_to_their_own_host() {
        let mut app = app();
        let mine = app.world_mut().spawn((TrafficAudio::new("c04_taxi01"), crate::world_audio::OwnAudioInstance, at(0.0, 10.0))).id();
        let ped = app.world_mut().spawn((PedAudio::default(), crate::world_audio::OwnAudioInstance, at(0.0, 5.0))).id();
        let retail = app.world_mut().spawn((TrafficAudio::new("c04_taxi01"), at(0.0, 12.0))).id();
        app.update();
        let owners = app.world().resource::<WorldOwners>();
        let own = &app.world().resource::<super::super::mod_world::OwnWorldOwners>().0;
        assert!(owners.vehicles.contains_key(&retail.to_bits()) && !owners.vehicles.contains_key(&mine.to_bits()) && owners.peds.is_empty());
        assert!(own.vehicles.contains_key(&mine.to_bits()) && own.peds.contains_key(&ped.to_bits()) && !own.vehicles.contains_key(&retail.to_bits()));
        app.world_mut().resource_mut::<WorldHeld>().own_traffic = vec![(mine.to_bits(), 3)];
        app.update();
        assert_eq!(app.world().get::<WorldAudioInstance>(mine), Some(&WorldAudioInstance { slot: WorldAudioSlot::Traffic, instance: 3, own: true }));
    }

    /// Components become owners (ids = entity bits), events hold their states for the right
    /// time, despawning releases, and the read-back tags the holders.
    #[test]
    fn components_become_owners_and_events_hold_states() {
        let mut app = app();
        let car = app.world_mut().spawn((TrafficAudio::new("c04_taxi01"), at(0.0, 10.0), AudioVelocity(Vec3::new(0.0, 0.0, 8.0)))).id();
        let peds: Vec<Entity> = (0..5).map(|i| app.world_mut().spawn((PedAudio { voice: Some(59), ..Default::default() }, at(1.0, 2.0 + i as f32))).id()).collect();
        let far = app.world_mut().spawn((PedAudio::default(), at(0.0, 60.0))).id();
        let npc = app.world_mut().spawn((NpcSkaterAudio { state: Some(AudioState::default()), ..Default::default() }, at(3.0, 3.0))).id();
        let remote_like = app.world_mut().spawn((NpcSkaterAudio { state: Some(AudioState::default()), remote: true, ..Default::default() }, at(4.0, 3.0))).id();
        app.update();
        {
            let owners = app.world().resource::<WorldOwners>();
            let v = owners.vehicles[&car.to_bits()];
            assert_eq!(v.velocity, [0.0, 0.0, 8.0]);
            assert_eq!(v.speed, 8.0);
            assert_eq!(v.direction, [0.0, 0.0, 1.0]);
            assert_eq!(owners.peds.len(), 6);
            // The 3 nearest within 50 m step; the others (and the one beyond 50 m) don't.
            let on: Vec<bool> = peds.iter().map(|p| owners.peds[&p.to_bits()].footsteps).collect();
            assert_eq!(on, [true, true, true, false, false]);
            assert!(!owners.peds[&far.to_bits()].footsteps);
            assert_eq!(owners.peds[&far.to_bits()].speech_limit, PED_FAR_THRESHOLD);
            // Remote players first in the list order.
            let s = app.world().resource::<super::super::npc_skaters::NpcSkaters>();
            assert_eq!(s.skaters.iter().map(|s| s.id).collect::<Vec<_>>(), vec![remote_like.to_bits(), npc.to_bits()]);
        }
        // Events.
        app.world_mut().write_message(VehicleAlarm { vehicle: car });
        app.world_mut().write_message(PedSpeechEvent { ped: peds[0], value: SpeechValue::WARN, topic: None });
        app.update();
        assert_eq!(app.world().resource::<WorldOwners>().vehicles[&car.to_bits()].horn, 6);
        assert_eq!(app.world().get::<PedAudio>(peds[0]).unwrap().speech_value, 53);
        app.world_mut().write_message(VehicleHorn { vehicle: car, kind: 3, seconds: 0.0 });
        app.update();
        assert_eq!(app.world().resource::<WorldOwners>().vehicles[&car.to_bits()].horn, 0, "a zero-length horn ends at once");
        // Read-back.
        app.world_mut().resource_mut::<WorldHeld>().traffic = vec![(car.to_bits(), 2)];
        app.update();
        assert_eq!(app.world().get::<WorldAudioInstance>(car), Some(&WorldAudioInstance { slot: WorldAudioSlot::Traffic, instance: 2, own: false }));
        app.world_mut().resource_mut::<WorldHeld>().traffic.clear();
        app.update();
        assert!(app.world().get::<WorldAudioInstance>(car).is_none());
        // Despawn = release: the owner disappears.
        app.world_mut().despawn(car);
        app.update();
        assert!(app.world().resource::<WorldOwners>().vehicles.is_empty());
        for p in peds.into_iter().chain([far, npc, remote_like]) {
            app.world_mut().despawn(p);
        }
        app.update();
        assert!(app.world().resource::<WorldOwners>().peds.is_empty());
        assert!(app.world().resource::<super::super::npc_skaters::NpcSkaters>().skaters.is_empty());
    }

    /// No component, nothing held: the bridge does not touch the owners (no change detection).
    #[test]
    fn inert_without_components() {
        let mut app = app();
        app.update();
        app.update();
        assert!(app.world().resource::<WorldOwners>().vehicles.is_empty());
        assert!(!app.world().resource::<Bridge>().active);
    }
}
