//! AI racers in play (research b969, b972, b978, b987, b992): the race host's commands place, pin, release and remove
//! the racers (race_shared's AI calls); every world tick their positions feed the tracker, their split and finish
//! volumes their progress, and the rubber band their cursor clock. Racers ride their race's challenge lines.
//! Details: docs/hails-additions/42-challenges.md#change-race-ai-racers-challengesrace_airs
use crate::living_world::npc_sim::NpcSim;
use crate::living_world::npc_skaters::{NpcRaceClock, NpcSkaterIndex, FRAMES_PER_TICK};
use crate::living_world::PopulationState;
use crate::race_host::Heat;
use bevy::prelude::*;
use skate_core::challenges::race::RaceTracker;
use skate_core::challenges::race_ai::{self, BandTuning, RacerAi};
use skate_core::living_world::replay::{choose_line_from, BranchContext, ChainConfig, ReplayLine};
use skate_core::living_world::LivingWorldId;
use std::collections::BTreeMap;

pub(crate) struct Racer {
    pub ai: RacerAi,
    /// The ai_characters row and the look (characters_marquee key) the NPC wears.
    pub row: String,
    pub look: String,
    /// The look's display name text id (`characters.<look>.name_id`, e.g. ID_COMMON_DANNY_WAY): the leaderboard name.
    pub name_id: String,
    pub npc: Option<LivingWorldId>,
    leg: usize,
    split: usize,
    carry: f32,
}

pub(crate) struct RaceAi {
    pub racers: Vec<Racer>,
    lines: BTreeMap<[u8; 16], ReplayLine>,
    pub band: BandTuning,
    last_tick: Option<u64>,
}

/// ai_characters row -> its look (the characters_marquee key) and the look's display name id, from the export
/// (`skater_profiles.json`).
fn looks(asset_root: &std::path::Path) -> BTreeMap<String, (String, String)> {
    let Ok(bytes) = std::fs::read(asset_root.join("private/living_world/skater_profiles.json")) else { return BTreeMap::new() };
    let Ok(doc) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return BTreeMap::new() };
    doc["ai_characters"].as_object().into_iter().flatten()
        .filter_map(|(k, v)| v["look"].as_str().map(|l| {
            let name = doc["characters"][l]["name_id"].as_str().unwrap_or(l).to_owned();
            (k.clone(), (l.to_owned(), name))
        })).collect()
}

fn heading(m: &[[f32; 4]; 4]) -> f32 {
    m[2][0].atan2(m[2][2])
}

impl RaceAi {
    /// The racers of `race` (its competitors, shuffled as every career race does) on the set `ch_<race id>`. None when
    /// the race has no AI or its lines are not exported.
    pub(crate) fn load(asset_root: &std::path::Path, race: &str, competitors: &[String], seed: u64) -> Option<Self> {
        if competitors.is_empty() {
            return None;
        }
        let set = format!("ch_{race}");
        let Some(lines) = crate::living_world::challenge_lines(asset_root, &set) else {
            warn!("SKATE_RACE {race}: no AI lines ({set} not exported; refresh the game files)");
            return None;
        };
        let looks = looks(asset_root);
        let mut rng = skate_core::living_world::rng::Rng::new(seed);
        let racers = race_ai::lineup(competitors, true, || rng.next_u32()).into_iter()
            .filter_map(|c| {
                let row = c.character?;
                let (look, name_id) = looks.get(&row).cloned().unwrap_or_else(|| (row.clone(), row.clone()));
                Some(Racer { ai: RacerAi::new(c.id), row, look, name_id, npc: None, leg: 0, split: 0, carry: 0.0 })
            })
            .collect::<Vec<_>>();
        info!("SKATE_RACE {race}: {} AI racers {:?} on {} lines", racers.len(),
            racers.iter().map(|r| (r.ai.id, r.row.as_str(), r.look.as_str())).collect::<Vec<_>>(), lines.len());
        Some(Self { racers, lines, band: BandTuning::default(), last_tick: None })
    }

    /// setup_heat (TeleportToStart): each racer at its start slot on the line that starts nearest it (b972 §4), pinned.
    /// The racers placed before it count as riders on their lines (the branch score's crowding term), so racers on
    /// neighbouring slots spread over the lines. A racer already out is replaced (RepositionAI). The racers ride only
    /// this set (the challenge path filter, b972 §3).
    pub(crate) fn place(&mut self, heat: &Heat, population: &mut PopulationState) {
        crate::living_world::merge_npc_lines(population, &self.lines, true);
        population.scripted_lines = Some(std::sync::Arc::new(self.lines.keys().copied().collect()));
        let mut placed: Vec<([u8; 16], u32)> = Vec::new();
        let mut in_use: Vec<[u8; 16]> = Vec::new();
        for r in &mut self.racers {
            if let Some(id) = r.npc.take() {
                population.despawn_scripted(id);
            }
            let slot = heat.start.slot(r.ai.id as usize);
            let point = [slot[3][0], slot[3][1], slot[3][2]];
            let ctx = BranchContext { position: point, forward: [slot[2][0], slot[2][1], slot[2][2]], speed: 0.0, players: &[], others: &placed,
                in_use: &in_use, preferred_skill: -1, online: false, chain: ChainConfig::retail(), tricks: Default::default(), body: None,
                valid_lines: None };
            let Some((line, _)) = choose_line_from(&self.lines, point, &ctx) else {
                warn!("SKATE_RACE racer {} ({}): no line near slot {:?}", r.ai.id, r.row, point);
                continue;
            };
            placed.push((line, 0));
            in_use.push(line);
            // Repositioned to its own slot (setup_heat), not the line start: two racers can share a line.
            r.npc = population.spawn_scripted(line, &r.look, r.ai.id as u8, point, heading(&slot));
            r.ai = RacerAi::new(r.ai.id);
            r.ai.pinned = true;
            r.ai.start_lap_paths(1);
            (r.leg, r.split, r.carry) = (0, 0, 0.0);
        }
    }

    /// The racers join the tracker (AddPlayerToRace).
    pub(crate) fn add_to(&self, tracker: &mut RaceTracker) {
        for r in &self.racers {
            tracker.add_racer(r.ai.id, None);
        }
    }

    /// run_heat (RaceStart): UnpinAI + StartPath.
    pub(crate) fn go(&mut self) {
        self.racers.iter_mut().filter(|r| !r.ai.finished).for_each(|r| r.ai.go());
    }

    /// end_heat (RaceStop): pin all.
    pub(crate) fn stop(&mut self) {
        self.racers.iter_mut().for_each(|r| r.ai.pinned = true);
    }

    /// complete / cleanup: UnspawnAI, and their lines go.
    pub(crate) fn remove(&mut self, population: &mut PopulationState) {
        for r in &mut self.racers {
            if let Some(id) = r.npc.take() {
                population.despawn_scripted(id);
            }
        }
        crate::living_world::merge_npc_lines(population, &self.lines, false);
        population.scripted_lines = None;
    }

    /// Each racer's position: its physics body's deck when simulated, else its line sample.
    pub(crate) fn positions(&self, index: &NpcSkaterIndex, bodies: &Query<(&Transform, Option<&NpcSim>)>) -> BTreeMap<u32, [f32; 3]> {
        self.racers.iter()
            .filter_map(|r| {
                let (t, sim) = bodies.get(*index.0.get(&r.npc?)?).ok()?;
                Some((r.ai.id, sim.map_or(t.translation.to_array(), NpcSim::deck_position)))
            })
            .collect()
    }

    /// One update: clocks (held while pinned, band while moving) for the world ticks since the last update, then split
    /// and finish volumes. Returns how many racers finished (each one is an OpponentFinished for the run).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step(&mut self, heat: &Heat, volumes: &crate::trigger_volumes::TriggerVolumes, positions: &BTreeMap<u32, [f32; 3]>,
        tracker: &mut RaceTracker, player: u32, elapsed: f32, world_tick: u64, index: &NpcSkaterIndex,
        clocks: &mut Query<&mut NpcRaceClock>, commands: &mut Commands) -> usize {
        let ticks = self.last_tick.map_or(0, |t| world_tick.saturating_sub(t));
        self.last_tick = Some(world_tick);
        let player_eta = tracker.eta(player);
        let mut finished = 0;
        for r in &mut self.racers {
            let Some(entity) = r.npc.and_then(|id| index.0.get(&id).copied()) else { continue };
            let frames = (ticks * FRAMES_PER_TICK) as f32;
            match clocks.get_mut(entity) {
                Ok(mut clock) => {
                    clock.moving = r.ai.moving();
                    clock.scale = 1.0;
                    if r.ai.moving() {
                        let factor = race_ai::eta_band(player_eta, tracker.eta(r.ai.id), false, &self.band);
                        clock.scale = race_ai::shaped_speed(1.0, 0.0, factor, &self.band);
                        r.carry += frames * self.band.band_gain * factor;
                        let whole = r.carry.trunc();
                        r.carry -= whole;
                        clock.extra += whole as i64;
                    } else {
                        clock.held += ticks * FRAMES_PER_TICK;
                    }
                }
                Err(_) => {
                    commands.entity(entity).insert(NpcRaceClock::default());
                }
            }
            if !r.ai.moving() {
                continue;
            }
            let Some(&p) = positions.get(&r.ai.id) else { continue };
            let Some(leg) = heat.legs.get(r.leg) else { continue };
            let hit = leg.splits.iter().enumerate().skip(r.split)
                .filter(|(_, v)| volumes.get(&v.id).is_some_and(|tv| tv.shape.contains_point(p)))
                .map(|(i, _)| i + 1).last();
            let Some(i) = hit else { continue };
            r.split = i;
            tracker.set_progress(r.ai.id, r.leg, i);
            if i < leg.splits.len() {
                continue;
            }
            if r.leg + 1 < heat.legs.len() {
                r.leg += 1;
                r.split = 0;
                r.ai.start_lap_paths(r.leg as i32 + 1);
                continue;
            }
            tracker.finish(r.ai.id, elapsed);
            r.ai.finish();
            finished += 1;
            info!("SKATE_RACE racer {} ({}) finished {elapsed:.2}", r.ai.id, r.row);
        }
        finished
    }
}
