//! AI contest riders (ask 599, step 1): the contest's competitors ride its challenge lines (`ch_<contest id>`), pinned
//! from setup_round through the countdown, released at run_round (AI.UnpinAI + StartPath), pinned at end_round and
//! removed when the contest ends. Their scores come in step 2. Details: docs/hails-additions/42-challenges.md#contests
use crate::living_world::npc_skaters::{NpcRaceClock, NpcSkaterIndex, FRAMES_PER_TICK};
use crate::living_world::PopulationState;
use bevy::prelude::*;
use skate_core::living_world::replay::{choose_line_from, BranchContext, ChainConfig, ReplayLine};
use skate_core::living_world::LivingWorldId;
use std::collections::BTreeMap;

pub(crate) struct Rider {
    /// The competitor index in the run (1.. in the contest's competitor order; 0 is the player).
    pub id: u32,
    /// The ai_characters row, the look the NPC wears and its display name id.
    pub row: String,
    pub look: String,
    pub name_id: String,
    /// The start slot (child of each round's start locator) rolled for this rider when the contest starts; the player
    /// keeps slot 0.
    pub slot: u32,
    pub npc: Option<LivingWorldId>,
    pub moving: bool,
}

pub(crate) struct ContestAi {
    pub riders: Vec<Rider>,
    lines: BTreeMap<[u8; 16], ReplayLine>,
    last_tick: Option<u64>,
}

impl ContestAi {
    /// The riders of `contest` (its competitor rows, in order) on the set `ch_<contest>`; None without competitors or
    /// exported lines. Each rider's start slot is rolled from `seed` with the race line-up shuffle (Fisher-Yates over
    /// slots 1..N, the player on 0; b992), once per contest.
    pub(crate) fn load(asset_root: &std::path::Path, contest: &str, competitors: &[String], seed: u64) -> Option<Self> {
        if competitors.is_empty() {
            return None;
        }
        let set = format!("ch_{contest}");
        let Some(lines) = crate::living_world::challenge_lines(asset_root, &set) else {
            warn!("SKATE_CONTEST {contest}: no AI lines ({set} not exported); the competitors do not ride");
            return None;
        };
        let looks = crate::race_ai_host::looks(asset_root);
        let mut rng = skate_core::living_world::rng::Rng::new(seed);
        let order: Vec<String> = (0..competitors.len()).map(|i| i.to_string()).collect();
        let lineup = skate_core::challenges::race_ai::lineup(&order, true, || rng.next_u32());
        let slots: Vec<u32> = (0..competitors.len())
            .map(|i| lineup.iter().find(|c| c.character.as_deref() == Some(order[i].as_str())).map_or(i as u32 + 1, |c| c.id))
            .collect();
        let riders = competitors.iter().enumerate().map(|(i, row)| {
            let (look, name_id) = looks.get(row).cloned().unwrap_or_else(|| (row.clone(), row.clone()));
            Rider { id: i as u32 + 1, row: row.clone(), look, name_id, slot: slots[i], npc: None, moving: false }
        }).collect::<Vec<_>>();
        info!("SKATE_CONTEST {contest}: {} AI riders (id, row, slot) {:?} on {} lines", riders.len(),
            riders.iter().map(|r| (r.id, r.row.as_str(), r.slot)).collect::<Vec<_>>(), lines.len());
        Some(Self { riders, lines, last_tick: None })
    }

    /// setup_round: AI.FilterPaths(CurRound.Paths), then AI.RepositionAI(id, StartLocation, id) + PinAI: rider n stands
    /// on slot n of the start locator (`slots`, child n; the round start when the locator has fewer) on the line the
    /// path follower takes from there (`82458968` mode 1: a filtered start within the chain radius by branch score,
    /// else the nearest filtered start; b972 §4), the riders placed before it counting as crowding.
    pub(crate) fn place(&mut self, start: &[[f32; 4]; 4], slots: &[[[f32; 4]; 4]], paths: &skate_core::challenges::path_filter::RoundPaths,
        population: &mut PopulationState) {
        crate::living_world::merge_npc_lines(population, &self.lines, true);
        let valid = paths.valid_lines(self.lines.keys());
        if let Some(v) = &valid {
            info!("SKATE_CONTEST round paths: {} names -> {} of {} lines", paths.names.len(), v.len(), self.lines.len());
        }
        population.scripted_lines = Some(std::sync::Arc::new(valid.clone().unwrap_or_else(|| self.lines.keys().copied().collect())));
        let chain = ChainConfig { fallback: true, ..ChainConfig::retail() };
        let mut placed: Vec<([u8; 16], u32)> = Vec::new();
        let mut in_use: Vec<[u8; 16]> = Vec::new();
        for r in &mut self.riders {
            if let Some(id) = r.npc.take() {
                population.despawn_scripted(id);
            }
            let slot = slots.get(r.slot as usize).unwrap_or(start);
            let point = [slot[3][0], slot[3][1], slot[3][2]];
            let forward = [slot[2][0], slot[2][1], slot[2][2]];
            let ctx = BranchContext { position: point, forward, speed: 0.0, players: &[], others: &placed, in_use: &in_use,
                preferred_skill: -1, online: false, chain, tricks: Default::default(), body: None, valid_lines: valid.as_deref() };
            let Some((line, _)) = choose_line_from(&self.lines, point, &ctx) else {
                warn!("SKATE_CONTEST rider {} ({}): no line for its start slot {point:?}; not spawned", r.id, r.row);
                continue;
            };
            // Repositioned to its own slot, not the line start (two riders can share a line), like the racers.
            let (at, heading) = (point, forward[0].atan2(forward[2]));
            placed.push((line, 0));
            in_use.push(line);
            r.npc = population.spawn_scripted(line, &r.look, r.id as u8, at, heading);
            r.moving = false;
            let hex: String = line.iter().map(|b| format!("{b:02x}")).collect();
            info!("SKATE_CONTEST rider {} ({}): slot {} npc {:?} line {hex} at {at:?}", r.id, r.row, r.slot, r.npc);
        }
    }

    /// run_round: UnpinAI + StartPath.
    pub(crate) fn go(&mut self) {
        self.riders.iter_mut().for_each(|r| r.moving = true);
    }

    /// end_round: PinAI.
    pub(crate) fn stop(&mut self) {
        self.riders.iter_mut().for_each(|r| r.moving = false);
    }

    /// The contest ends (or the map changes): UnspawnAI, and their lines go.
    pub(crate) fn remove(&mut self, population: &mut PopulationState) {
        for r in &mut self.riders {
            if let Some(id) = r.npc.take() {
                population.despawn_scripted(id);
            }
        }
        crate::living_world::merge_npc_lines(population, &self.lines, false);
        population.scripted_lines = None;
    }

    /// One update: a pinned rider's clock is held for the world ticks since the last update, a released one runs.
    pub(crate) fn step(&mut self, world_tick: u64, index: &NpcSkaterIndex, clocks: &mut Query<&mut NpcRaceClock>, commands: &mut Commands) {
        let ticks = self.last_tick.map_or(0, |t| world_tick.saturating_sub(t));
        self.last_tick = Some(world_tick);
        for r in &self.riders {
            let Some(entity) = r.npc.and_then(|id| index.0.get(&id).copied()) else { continue };
            match clocks.get_mut(entity) {
                Ok(mut clock) => {
                    clock.moving = r.moving;
                    clock.scale = 1.0;
                    if !r.moving {
                        clock.held += ticks * FRAMES_PER_TICK;
                    }
                }
                Err(_) => {
                    commands.entity(entity).insert(NpcRaceClock::default());
                }
            }
        }
    }
}
