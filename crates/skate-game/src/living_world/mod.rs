//! Living world, engine side (doc `docs/hails-additions/26b-living-world-population.md`, milestone 2: the
//! population core). Runs `skate_core::living_world` around the local player at the console
//! cadence and publishes spawn / despawn decisions as messages. Consumers: the replay-tier NPC
//! skaters ([`npc_skaters`], milestone 3); peds and cars come with their milestones.
//!
//! Data: the setup group `livingworld` export (`private/living_world/`): `tables.json`
//! (census caps, ranges), `<District>.census.bin`, `skater_profiles.json`,
//! `skater_paths/<District>.bin`. Without it the world stays empty.
//!
//! Multiplayer seams (no networking here): [`NetRole`] decides who runs the population.
//! Standalone and Host run the rules; a Client never does and only mirrors records it is given
//! ([`PopulationState::apply_records`]). Records serialise ([`WireRecord`]). Retail default:
//! nothing ambient spawns in an online session (culling still runs).
//!
//! Mod surface (doc 26 "Modding"): [`LivingWorldSettings`] is the one place settings and mods
//! change the living world; mods patch it through `sdk.world.set_tuning('living_world', ...)`
//! (`modding::world_tuning`), and a mod that stops is undone with
//! [`LivingWorldSettings::reset_mod_overrides`].
// Messages and records for consumers the engine does not have yet.
#![allow(dead_code)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use skate_core::living_world::skaters::SkaterData;
use skate_core::living_world::{
    CensusMap, Decision, DespawnReason, DespawnRecord, FreePlay, Kind, LivingWorld, LivingWorldId, Observer, PopulationConfig, SpawnChoice,
    SpawnRecord, TickInputs,
};
use std::path::Path;

pub(crate) mod dmo_stream;
pub(crate) mod npc_avoid;
pub(crate) mod npc_sim;
pub(crate) mod npc_skaters;
pub(crate) mod ped_graph;
pub(crate) mod ped_mood;
pub(crate) mod ped_hand_props;
pub(crate) mod ped_plugins;
pub(crate) mod peds;
pub(crate) mod vehicle_contacts;
pub(crate) mod vehicles;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
#[cfg(test)]
#[path = "npc_tests.rs"]
mod npc_tests;
#[cfg(test)]
#[path = "peds_tests.rs"]
mod peds_tests;
#[cfg(test)]
#[path = "vehicles_tests.rs"]
mod vehicles_tests;

/// Who runs the population decision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum NetRole {
    /// Single player (and, with the retail default, every online session: nothing spawns).
    #[default]
    Standalone,
    /// Future: decides for everyone and sends the records.
    Host,
    /// Future: never decides; mirrors the host's records.
    Client,
}

/// Per-kind switch and density (1.0 = retail).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct KindSetting {
    pub enabled: bool,
    pub density: f32,
}

impl Default for KindSetting {
    fn default() -> Self {
        Self { enabled: true, density: 1.0 }
    }
}

/// Settings and mod overrides. Defaults = retail.
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct LivingWorldSettings {
    pub enabled: bool,
    pub skaters: KindSetting,
    pub pedestrians: KindSetting,
    pub vehicles: KindSetting,
    /// Desired ambient NPC skaters offline (retail 3).
    pub ambient_skaters: u32,
    /// NPC skater spawn fade in / leave fade (retail 1 s in, 1 s out, removed below 0.2). A mod
    /// may change it; `LivingWorldSettings::default()` restores retail.
    pub skater_fade: skate_core::living_world::leave_fade::LeaveFadeConfig,
    /// What an NPC skater does at the end of its line (retail: continue on an unused line whose
    /// start is within 4 m, `sub_8246C7F8`; radius 0 = fade out at every line end). A mod may
    /// change it; `LivingWorldSettings::default()` restores retail.
    pub skater_line_chain: skate_core::living_world::replay::ChainConfig,
    /// Ped draw fade by camera distance and spawn fade in (retail: the model's 45 / 55 m pair,
    /// 1 s in; `skate_core::living_world::peds::fade`). A mod may change or disable it.
    pub ped_fade: skate_core::living_world::peds::PedFadeConfig,
    /// Props and mod bodies as ped navigation obstacles (fix 11; retail DynamicObject NavPower
    /// obstacles: 0.2 m minimum half extent, no cut above 0.4 m/s, re-cut after 0.25 x the
    /// smallest half extent; `skate_core::living_world::peds::obstacles`). A mod may change or
    /// disable it; `LivingWorldSettings::default()` restores retail.
    pub ped_obstacles: skate_core::living_world::peds::ObstacleParams,
    /// Traffic cars touching peds (retail: on, the ped is pushed out of the car and walks on; no
    /// knock-down: `skate_core::living_world::peds::vehicle_contact`). A mod may switch the
    /// detection or the push off; `LivingWorldSettings::default()` restores retail.
    pub ped_vehicle_contact: skate_core::living_world::peds::VehicleContactParams,
    /// NPC skaters push dynamic props like the player's board and body (fix 19; retail NPC skaters
    /// are full skaters). A mod may switch it off; `LivingWorldSettings::default()` restores retail.
    pub npc_skater_props: npc_skaters::NpcSkaterPropContact,
    /// NPC draw distance (QoL, not retail; `skate_core::living_world::draw_distance`): every
    /// population range (census circles, ped draw fade, NPC skater ranges, cars) x this, caps x
    /// its square. 1.0 = retail. The value in effect: the settings menu writes the player's choice
    /// here and into [`Self::user_npc_draw_distance`]; a mod may set it, and
    /// [`Self::reset_mod_overrides`] puts the player's choice back. Owned by the population
    /// authority (it decides what exists); a client never feeds its own value into the rules.
    pub npc_draw_distance: f32,
    /// Mod overrides of the NPC skater puppet clip, keyed by the stable phase id
    /// (`ReplayPhase::name`) or `<phase>.<style>`; empty = the shipped picks
    /// (`npc_skaters::puppet_clip`). Cleared by [`Self::reset_mod_overrides`].
    pub skater_clips: std::collections::BTreeMap<String, String>,
    /// Mod overrides of the NPC skater puppet crossfade time (s) into a phase's clip, keyed by the
    /// phase id or `default`; empty = the stock graph's default transition time
    /// (`npc_skaters::RETAIL_BLEND_SECONDS`). Cleared by [`Self::reset_mod_overrides`].
    pub skater_blend_seconds: std::collections::BTreeMap<String, f32>,
    /// Mod overrides of an NPC skater's natural stance, keyed by character record id (16 hex
    /// digits) or record name; empty = the retail table (`skate_core::living_world::stance`). Read
    /// at spawn (retail sets it once from the record). Cleared by [`Self::reset_mod_overrides`].
    pub skater_stance: std::collections::BTreeMap<String, skate_core::living_world::stance::NaturalStance>,
    /// Mod renames of the clip attributes that toggle an NPC skater's stance bits, keyed by
    /// `board_backward` / `mirrored` / `switch` (`skate_core::living_world::stance::StanceEvents`);
    /// empty value = that toggle off; empty map = retail (`animboardbackward` / `mirrored` /
    /// `switch`). Cleared by [`Self::reset_mod_overrides`].
    pub skater_stance_events: std::collections::BTreeMap<String, String>,
    /// The trick an NPC skater does at a recorded ollie / flip slot (retail: re-picked from the
    /// character's profile table when the recorded air is long enough;
    /// `skate_core::living_world::npc_tricks`). A mod may change the mode and the gate windows;
    /// `LivingWorldSettings::default()` restores retail.
    pub npc_tricks: npc_skaters::NpcTrickSettings,
    /// Mod trick tables per character key or `ai_skater_profiles` name (a key wins over a profile
    /// name; an absent table keeps the disc's); empty = the disc's tables. Read at each slot.
    /// Cleared by [`Self::reset_mod_overrides`].
    pub skater_trick_profiles: std::collections::BTreeMap<String, npc_skaters::NpcTrickTables>,
    /// Simulated NPC skaters near the player (full physics driven by their AI record; doc 26 M7).
    /// Off by default until play-tested (`SKATE_NPC_SIM=1`); a mod may switch it and change the
    /// radius and count. `LivingWorldSettings::default()` restores the default.
    pub npc_simulated: npc_sim::SimulatedTierSettings,
    /// The NPC skaters' retail obstacle avoider (`skate_core::living_world::avoid`, doc 26 "NPC
    /// skater obstacle avoider"); retail values by default, mod-tunable (`npc_avoid`).
    pub npc_avoid: skate_core::living_world::avoid::AvoidSettings,
    /// The ped behaviour runtime: the stock ped AI graph on each ped's brain (doc 26 "Ped
    /// behaviour runtime"); retail values, mod-tunable (`ped_brain`).
    pub ped_brain: PedBrainSettings,
    /// Mod horn values per traffic driver record (`livingworld_vehicle_drivers` name, or `all` first), read at
    /// spawn (`traffic_horn`); empty = the disc's values.
    pub traffic_horn: std::collections::BTreeMap<String, skate_mods::world_tuning::TrafficHornPatch>,
    /// The player's menu choice (saved in `settings/graphics.json`), restored when a mod's
    /// override is undone.
    pub user_npc_draw_distance: f32,
    /// The player picked "None" for NPC draw distance (QoL, not retail): no NPC skaters, peds or
    /// cars spawn and the live ones despawn (reason Disabled). Kept across mod resets like the
    /// draw distance; the ranges stay retail.
    pub user_npcs_off: bool,
    /// Free Play options (mode 3); `None` = career free roam (no scaling). The Free Play menu is
    /// a later milestone.
    pub free_play: Option<FreePlay>,
    /// The zombie cheat (later milestone; peds without cap, no traffic, no NPC skaters).
    pub zombie: bool,
    /// DMO streaming of the placed props (doc 27 "DMO streaming"; on as in retail, `SKATE_DMO_STREAM=0` turns it off).
    pub dmo_stream: dmo_stream::DmoStreamGameSettings,
    pub net_role: NetRole,
    /// Session seed (0 = derive from the map name).
    pub seed: u64,
    /// Log a population summary every 5 s (`SKATE_LIVING_WORLD_DEBUG=1`).
    pub debug: bool,
}

impl Default for LivingWorldSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            skaters: KindSetting::default(),
            pedestrians: KindSetting::default(),
            vehicles: KindSetting::default(),
            ambient_skaters: skate_core::living_world::config::retail::SKATER_DESIRED,
            skater_fade: skate_core::living_world::leave_fade::LeaveFadeConfig::retail(),
            skater_line_chain: skate_core::living_world::replay::ChainConfig::retail(),
            ped_fade: skate_core::living_world::peds::PedFadeConfig::default(),
            ped_obstacles: skate_core::living_world::peds::ObstacleParams::default(),
            ped_vehicle_contact: skate_core::living_world::peds::VehicleContactParams::default(),
            npc_skater_props: npc_skaters::NpcSkaterPropContact::default(),
            npc_draw_distance: skate_core::living_world::DrawDistance::RETAIL,
            user_npc_draw_distance: skate_core::living_world::DrawDistance::RETAIL,
            user_npcs_off: false,
            skater_clips: Default::default(),
            skater_blend_seconds: Default::default(),
            skater_stance: Default::default(),
            skater_stance_events: Default::default(),
            npc_tricks: Default::default(),
            skater_trick_profiles: Default::default(),
            npc_simulated: Default::default(),
            npc_avoid: Default::default(),
            ped_brain: Default::default(),
            traffic_horn: Default::default(),
            free_play: None,
            // The zombie cheat; no cheat screen yet: `SKATE_ZOMBIE=1` or the mod value `zombie`.
            zombie: std::env::var("SKATE_ZOMBIE").ok().as_deref() == Some("1"),
            dmo_stream: Default::default(),
            net_role: NetRole::Standalone,
            seed: 0,
            debug: false,
        }
    }
}

impl LivingWorldSettings {
    fn from_env() -> Self {
        let flag = |k: &str| std::env::var(k).ok();
        Self {
            enabled: flag("SKATE_LIVING_WORLD").as_deref() != Some("0"),
            debug: flag("SKATE_LIVING_WORLD_DEBUG").as_deref() == Some("1"),
            ..Self::default()
        }
    }

    /// The draw distance in effect (sanitised: non-finite or non-positive = retail, clamped).
    pub(crate) fn draw_distance(&self) -> skate_core::living_world::DrawDistance {
        skate_core::living_world::DrawDistance::new(self.npc_draw_distance)
    }

    /// The settings menu: the player's choice, in effect at once.
    pub(crate) fn set_user_draw_distance(&mut self, multiplier: f32) {
        self.user_npcs_off = multiplier == skate_core::living_world::DrawDistance::NONE;
        let m = skate_core::living_world::DrawDistance::new(multiplier).multiplier();
        self.user_npc_draw_distance = m;
        self.npc_draw_distance = m;
    }

    /// Undo every mod override (mod disabled): retail values for everything a mod may change,
    /// keeping the player's own choices (menu draw distance) and the session flags (enabled,
    /// debug, seed, network role).
    pub(crate) fn reset_mod_overrides(&mut self) {
        *self = Self {
            enabled: self.enabled,
            debug: self.debug,
            seed: self.seed,
            net_role: self.net_role,
            npc_draw_distance: self.user_npc_draw_distance,
            user_npc_draw_distance: self.user_npc_draw_distance,
            user_npcs_off: self.user_npcs_off,
            ..Self::default()
        };
    }

    /// Write the settings into the core config (code defaults and data ranges stay).
    pub(crate) fn apply(&self, config: &mut PopulationConfig) {
        config.draw_distance = self.draw_distance().multiplier();
        let on = self.enabled && !self.user_npcs_off;
        config.skaters.enabled = on && self.skaters.enabled;
        config.skaters.desired = (self.ambient_skaters as f32 * self.skaters.density.max(0.0)).round() as u32;
        config.skaters.leave_fade = self.skater_fade;
        config.skaters.line_chain = self.skater_line_chain;
        config.pedestrians.enabled = on && self.pedestrians.enabled;
        config.pedestrians.density = self.pedestrians.density;
        config.vehicles.enabled = on && self.vehicles.enabled;
        config.vehicles.density = self.vehicles.density;
    }
}

/// A spawn decision for the engine (bodies, rendering, audio publishers consume it).
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) struct LivingWorldSpawn(pub SpawnRecord);

/// A despawn decision.
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) struct LivingWorldDespawn(pub DespawnRecord);

/// Serialisable form of a decision (what a host would send; mod events use the same fields).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum WireRecord {
    Spawn { kind: String, id: u64, tick: u64, position: [f32; 3], heading: f32, seed: u64, initial: bool, choice: WireChoice },
    Despawn { kind: String, id: u64, tick: u64, reason: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum WireChoice {
    Census { record: String, category: String },
    Skater { line: String, character: String, slot: u8 },
    /// Census car (milestone V2): stable names plus the lane (retail segment id, lane, distance).
    Vehicle { record: String, category: String, entity: String, model: String, chassis: u32, secondary: u32, segment: u64, lane: u8, distance: f32 },
}

fn reason_name(r: DespawnReason) -> &'static str {
    match r {
        DespawnReason::Distance => "distance",
        DespawnReason::Excess => "excess",
        DespawnReason::FreePlayOff => "free_play_off",
        DespawnReason::Disabled => "disabled",
        DespawnReason::External => "external",
    }
}

impl WireRecord {
    pub(crate) fn from_decision(d: &Decision) -> Self {
        match d {
            Decision::Spawn(s) => WireRecord::Spawn {
                kind: s.id.kind.name().into(),
                id: s.id.to_u64(),
                tick: s.tick,
                position: s.position,
                heading: s.heading,
                seed: s.seed,
                initial: s.initial,
                choice: match &s.choice {
                    SpawnChoice::Census { record, category } => WireChoice::Census { record: record.clone(), category: category.clone() },
                    SpawnChoice::Vehicle { record, category, entity, model, chassis, secondary, segment, lane, distance } => WireChoice::Vehicle {
                        record: record.clone(),
                        category: category.clone(),
                        entity: entity.clone(),
                        model: model.clone(),
                        chassis: *chassis,
                        secondary: *secondary,
                        segment: *segment,
                        lane: *lane,
                        distance: *distance,
                    },
                    SpawnChoice::Skater { line, character, slot } => {
                        WireChoice::Skater { line: line.iter().map(|b| format!("{b:02x}")).collect(), character: character.clone(), slot: *slot }
                    }
                },
            },
            Decision::Despawn(r) => WireRecord::Despawn { kind: r.id.kind.name().into(), id: r.id.to_u64(), tick: r.tick, reason: reason_name(r.reason).into() },
        }
    }

    pub(crate) fn to_decision(&self) -> Option<Decision> {
        Some(match self {
            WireRecord::Spawn { id, tick, position, heading, seed, initial, choice, .. } => Decision::Spawn(SpawnRecord {
                id: LivingWorldId::from_u64(*id)?,
                tick: *tick,
                position: *position,
                heading: *heading,
                seed: *seed,
                initial: *initial,
                choice: match choice {
                    WireChoice::Census { record, category } => SpawnChoice::Census { record: record.clone(), category: category.clone() },
                    WireChoice::Vehicle { record, category, entity, model, chassis, secondary, segment, lane, distance } => SpawnChoice::Vehicle {
                        record: record.clone(),
                        category: category.clone(),
                        entity: entity.clone(),
                        model: model.clone(),
                        chassis: *chassis,
                        secondary: *secondary,
                        segment: *segment,
                        lane: *lane,
                        distance: *distance,
                    },
                    WireChoice::Skater { line, character, slot } => {
                        let mut id = [0u8; 16];
                        for (i, b) in id.iter_mut().enumerate() {
                            *b = u8::from_str_radix(line.get(2 * i..2 * i + 2)?, 16).ok()?;
                        }
                        SpawnChoice::Skater { line: id, character: character.clone(), slot: *slot }
                    }
                },
            }),
            WireRecord::Despawn { id, tick, reason, .. } => Decision::Despawn(DespawnRecord {
                id: LivingWorldId::from_u64(*id)?,
                tick: *tick,
                reason: match reason.as_str() {
                    "distance" => DespawnReason::Distance,
                    "excess" => DespawnReason::Excess,
                    "free_play_off" => DespawnReason::FreePlayOff,
                    "disabled" => DespawnReason::Disabled,
                    _ => DespawnReason::External,
                },
            }),
        })
    }
}

/// The population of the current world plus the loaded data.
#[derive(Resource)]
pub(crate) struct PopulationState {
    pub world: LivingWorld,
    pub census: Option<CensusMap>,
    pub skaters: Option<SkaterData>,
    /// Road network and vehicle entities of the loaded world (milestone V2: cars are placed on
    /// lanes; none = no cars).
    pub roads: Option<skate_core::living_world::traffic::RoadNetwork>,
    pub vehicles: Option<skate_core::living_world::VehicleCatalog>,
    /// Replay-tier lines and voices of the loaded district (milestone 3).
    pub npc: npc_skaters::NpcData,
    /// The ranges from `tables.json` (re-applied after settings changes).
    data_config: PopulationConfig,
    /// (map name, generation) the data was loaded for.
    loaded_for: Option<(String, u64)>,
    pub status: String,
    pub spawned: u64,
    pub despawned: u64,
    last_report: u64,
}

impl Default for PopulationState {
    fn default() -> Self {
        Self {
            world: LivingWorld::new(PopulationConfig::retail(), 0),
            census: None,
            skaters: None,
            roads: None,
            vehicles: None,
            npc: npc_skaters::NpcData::default(),
            data_config: PopulationConfig::retail(),
            loaded_for: None,
            status: "no living-world data".into(),
            spawned: 0,
            despawned: 0,
            last_report: 0,
        }
    }
}

impl PopulationState {
    /// A new world: fresh population (seeded), new data.
    pub(crate) fn install(&mut self, map: &str, generation: u64, settings: &LivingWorldSettings, data: LoadedData) {
        let seed = if settings.seed != 0 { settings.seed } else { skate_core::living_world::rng::derive(0x5345_4544, &[name_hash(map), generation]) };
        self.data_config = data.config;
        self.world = LivingWorld::new(self.data_config.clone(), seed);
        self.census = data.census;
        self.skaters = data.skaters;
        self.roads = data.roads;
        self.vehicles = data.vehicles;
        self.npc = data.npc;
        self.status = data.status;
        self.loaded_for = Some((map.to_string(), generation));
    }

    /// Client side: mirror records from a host (no rules, no RNG). Transport is not built.
    pub(crate) fn apply_records(&mut self, records: &[WireRecord]) -> Vec<Decision> {
        let decisions: Vec<Decision> = records.iter().filter_map(WireRecord::to_decision).collect();
        for d in &decisions {
            self.world.apply(d);
        }
        decisions
    }
}

fn name_hash(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Data for one world.
pub(crate) struct LoadedData {
    pub config: PopulationConfig,
    pub census: Option<CensusMap>,
    pub skaters: Option<SkaterData>,
    pub roads: Option<skate_core::living_world::traffic::RoadNetwork>,
    pub vehicles: Option<skate_core::living_world::VehicleCatalog>,
    pub npc: npc_skaters::NpcData,
    pub status: String,
}

/// Read the export for a district (map name = district name for the retail cities).
pub(crate) fn load_data(asset_root: &Path, district: &str) -> LoadedData {
    let dir = asset_root.join("private/living_world");
    let mut config = PopulationConfig::retail();
    let mut status = Vec::new();
    let tables = std::fs::read(dir.join("tables.json")).ok().and_then(|b| match skate_data::living_world::LivingWorldTables::parse(&b) {
        Ok(t) => Some(t),
        Err(e) => {
            status.push(e.to_string());
            None
        }
    });
    let census = tables.as_ref().and_then(|t| {
        t.apply_to(&mut config);
        let bytes = std::fs::read(dir.join(format!("{district}.census.bin"))).ok()?;
        match skate_data::living_world::parse_census_grid(&bytes) {
            Ok(g) => Some(t.census_map(vec![g])),
            Err(e) => {
                status.push(e.to_string());
                None
            }
        }
    });
    let mut npc = npc_skaters::NpcData::default();
    let skaters = (|| {
        let profiles = std::fs::read(dir.join("skater_profiles.json")).ok()?;
        let characters = skate_data::living_world::skater_characters(&profiles, &[]).ok()?;
        npc.voices = npc_voices(&profiles);
        npc.records = npc_records(&profiles);
        npc.tricks = std::sync::Arc::new(skate_data::living_world::skater_trick_profiles(&profiles).unwrap_or_default());
        let pack = std::fs::read(dir.join("skater_paths").join(format!("{district}.bin"))).ok()?;
        let tiles = skate_data::aipath::parse_pack(&pack).ok()?;
        let (paths, _) = skate_data::aipath::district_paths(&tiles).ok()?;
        let lines = skate_data::living_world::skater_lines(paths.iter().map(|p| &p.path));
        npc.lines = std::sync::Arc::new(paths.iter().filter(|p| p.path.id.is_ambient()).map(|p| (p.path.id.0, skate_data::living_world::replay_line(&p.path))).collect());
        Some(SkaterData { lines, characters })
    })();
    status.insert(
        0,
        format!(
            "{district}: census {}, skater lines {}",
            census.as_ref().map_or("none".to_string(), |c| format!("{} records", c.records.len())),
            skaters.as_ref().map_or(0, |s| s.lines.len())
        ),
    );
    // Roads and vehicle entities (milestone V2). The file holds every district, but the districts
    // are separate worlds overlapping in x / z (doc 26, "Cars flying off"): only the loaded
    // district's roads join the network, or cars land on another district's road in the air.
    let roads = std::fs::read(dir.join("roads.bin")).ok().and_then(|b| {
        let built = skate_data::roads::RoadGraph::parse(&b).map_err(|e| e.to_string()).and_then(|g| match g.district_traffic_input(district) {
            Some(input) => skate_core::living_world::traffic::RoadNetwork::build(&input).map(Some).map_err(|e| e.to_string()),
            None => Ok(None),
        });
        built.map_err(|e| status.push(format!("roads: {e}"))).ok().flatten()
    });
    let vehicles = std::fs::read(dir.join("vehicles.json")).ok().and_then(|b| skate_data::living_world::vehicle_catalog(&b).map_err(|e| status.push(e.to_string())).ok());
    if let Some(first) = status.first_mut() {
        first.push_str(&format!(", roads {}, vehicle entities {}", roads.as_ref().map_or(0, |r| r.segments.len()), vehicles.as_ref().map_or(0, |v| v.entities.len())));
    }
    LoadedData { config, census, skaters, roads, vehicles, npc, status: status.join("; ") }
}

/// `characters_marquee` voice ids by character key (`skater_profiles.json` `characters.*.voice`).
pub(crate) fn npc_voices(profiles: &[u8]) -> std::collections::BTreeMap<String, u32> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(profiles) else { return Default::default() };
    v["characters"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, c)| c["voice"].as_u64().map(|x| (k.clone(), x as u32)))
        .collect()
}

/// `characters_marquee` character record names by character key (`skater_profiles.json`
/// `characters.*.recipe`, e.g. `deerman` -> `deerman_of_darkwoods`): the record the natural stance
/// is read from (`skate_core::living_world::stance`).
pub(crate) fn npc_records(profiles: &[u8]) -> std::collections::BTreeMap<String, String> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(profiles) else { return Default::default() };
    v["characters"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, c)| c["recipe"].as_str().filter(|r| !r.is_empty()).map(|r| (k.clone(), r.to_owned())))
        .collect()
}

/// The observers this frame: the local skater (deck position and velocity). Remote players join
/// here once the host role exists.
#[derive(Resource, Default, Clone)]
pub(crate) struct LivingWorldObservers {
    pub observers: Vec<Observer>,
    /// Skater slots held by players (local + remote).
    pub player_slots: u32,
    pub online: bool,
}

/// Speed squared at or below which the census focus velocity is zero (retail `sub_826BE870`
/// compares |v|^2 against `0x8209BE90` = 1e-4 [code]).
const FOCUS_STILL_SPEED2: f32 = 1e-4;

/// The census focus of one player: the player's character, not the board (retail
/// `sub_826BDB50` -> `sub_826BE7D0` / `sub_826BE870` read the focused skater's `+52` component
/// position and its velocity [code], `.local/research/npc/fix3-pedcull.md`). `position` /
/// `velocity` are the skater's physical centre of mass and its velocity, valid on board, walking,
/// in the air and in a bail; the deck stays wherever the board was left. Non-finite values pass
/// through: the census skips such a focus (`population.rs` guard).
pub(crate) fn player_focus(position: [f32; 3], velocity: [f32; 3]) -> Observer {
    let still = velocity.iter().map(|v| v * v).sum::<f32>() <= FOCUS_STILL_SPEED2;
    Observer { position, velocity: if still { [0.0; 3] } else { velocity } }
}

/// The local player's focus: the character (position, velocity) when a skater is loaded, else
/// the board (tools and tests without a skater). The board is never preferred over the
/// character: walking away from a left board keeps the population around the player.
pub(crate) fn local_focus(character: Option<([f32; 3], [f32; 3])>, deck: ([f32; 3], [f32; 3])) -> Observer {
    let (position, velocity) = character.unwrap_or(deck);
    player_focus(position, velocity)
}

/// Whether a periodic debug line is due at world tick `tick` (every `period` ticks since `last`).
/// A new world (map reload, respawn into another generation) restarts the tick at 0, so a `last`
/// ahead of the tick belongs to the old world and is restarted with it.
pub(crate) fn report_due(tick: u64, last: &mut u64, period: u64) -> bool {
    if tick < *last {
        *last = 0;
    }
    if tick < *last + period {
        return false;
    }
    *last = tick;
    true
}

fn gather_observers(
    physics: Res<crate::physics::GamePhysics>,
    skater: Option<Res<crate::physics::SkaterRuntime>>,
    multiplayer: Option<Res<crate::multiplayer::Multiplayer>>,
    mut out: ResMut<LivingWorldObservers>,
) {
    use skate_core::physics::board::BodyId;
    // The player's character (on board, walking or bailing): the skeleton's physical centre of
    // mass (Skeleton16144/16160) and its velocity (Skeleton16176), as published each tick into
    // the reckoning fields (`physics/render_pose.rs`).
    let character = skater.as_ref().map(|s| {
        let f = &s.animated_skeleton.board_frames;
        ([f.centre_of_mass[0], f.centre_of_mass[1], f.centre_of_mass[2]], [f.com_velocity[0], f.com_velocity[1], f.com_velocity[2]])
    });
    let deck = physics.board.bodies()[BodyId::Deck.index()].rates;
    let v = |x: skate_core::math::Vector3| [x.x, x.y, x.z];
    let observer = local_focus(character, (v(deck.position), v(deck.linear_velocity)));
    if observer.position.iter().chain(&observer.velocity).any(|x| !x.is_finite()) {
        // The census skips a broken focus (no cull, no spawn) instead of emptying the world.
        warn!("LIVING_WORLD census focus is not finite (position {:?}, velocity {:?}); population held", observer.position, observer.velocity);
    }
    out.observers = vec![observer];
    let online = multiplayer.as_ref().is_some_and(|m| m.active());
    out.online = online;
    out.player_slots = if online { multiplayer.map_or(1, |m| m.player_ids().len().max(1) as u32) } else { 1 };
}

fn load_for_map(
    config: Res<crate::config::Config>,
    map: Res<crate::map_transition::CurrentMap>,
    settings: Res<LivingWorldSettings>,
    mut state: ResMut<PopulationState>,
    mut despawns: MessageWriter<LivingWorldDespawn>,
    audio: Option<ResMut<crate::world_audio::LivingWorldAudio>>,
) {
    let key = (map.name.clone(), map.generation);
    if state.loaded_for.as_ref() == Some(&key) {
        return;
    }
    for d in state.world.reset_world() {
        if let Decision::Despawn(r) = d {
            despawns.write(LivingWorldDespawn(r));
        }
    }
    let data = load_data(&config.asset_root, &map.name);
    info!("LIVING_WORLD data {}", data.status);
    // NPC skaters will publish board audio and speech here: let the world banks decode early.
    if let Some(mut audio) = audio {
        audio.expected = settings.enabled && !data.npc.lines.is_empty();
    }
    state.install(&map.name, map.generation, &settings, data);
}

/// One fixed step: settings into the config, then the console ticks that are due.
pub(crate) fn step_population(
    time: Res<Time>,
    settings: Res<LivingWorldSettings>,
    observers: Res<LivingWorldObservers>,
    mut state: ResMut<PopulationState>,
    mut spawns: MessageWriter<LivingWorldSpawn>,
    mut despawns: MessageWriter<LivingWorldDespawn>,
) {
    if settings.net_role == NetRole::Client {
        return;
    }
    let state = &mut *state;
    let mut config = state.data_config.clone();
    settings.apply(&mut config);
    state.world.config = config;
    let inputs = TickInputs {
        observers: &observers.observers,
        census: state.census.as_ref(),
        skater_world: state.skaters.as_ref().map(|s| s as &dyn skate_core::living_world::SkaterWorld),
        roads: state.roads.as_ref(),
        vehicles: state.vehicles.as_ref(),
        online: observers.online,
        zombie: settings.zombie,
        free_play: settings.free_play,
        player_slots: observers.player_slots.max(1),
        scripted_skaters: 0,
        ambient_skater_override: None,
        world_ready: true,
    };
    let decisions = state.world.advance(time.delta_secs_f64(), &inputs);
    for d in decisions {
        match d {
            Decision::Spawn(s) => {
                state.spawned += 1;
                spawns.write(LivingWorldSpawn(s));
            }
            Decision::Despawn(r) => {
                state.despawned += 1;
                despawns.write(LivingWorldDespawn(r));
            }
        }
    }
    // Every 5 s of the 60 Hz world tick (restarted with each new world).
    if settings.debug && report_due(state.world.tick(), &mut state.last_report, 300) {
        let focus: Vec<String> = observers.observers.iter().map(|o| format!("[{:.1}, {:.1}, {:.1}] {:.1} m/s", o.position[0], o.position[1], o.position[2], o.velocity.iter().map(|v| v * v).sum::<f32>().sqrt())).collect();
        info!("{} focus {}", readout(state), focus.join(" / "));
    }
}

/// One-line population summary (debug log; the overlay milestone can show the same text).
pub(crate) fn readout(state: &PopulationState) -> String {
    let w = &state.world;
    format!(
        "LIVING_WORLD tick {} skaters {} peds {} vehicles {} pool {:?} spawned {} despawned {} ({})",
        w.tick(),
        w.count(Kind::Skater),
        w.count(Kind::Pedestrian),
        w.count(Kind::Vehicle),
        w.skater_pool(),
        state.spawned,
        state.despawned,
        state.status
    )
}

pub(crate) struct LivingWorldPlugin;

impl Plugin for LivingWorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LivingWorldSettings::from_env())
            .init_resource::<PopulationState>()
            .init_resource::<LivingWorldObservers>()
            .init_resource::<dmo_stream::DmoStreamState>()
            .add_message::<LivingWorldSpawn>()
            .add_message::<LivingWorldDespawn>()
            .add_systems(
                FixedUpdate,
                (load_for_map, gather_observers, step_population, dmo_stream::stream_dmos).chain().after(crate::app::SimulationSet::Physics),
            );
        npc_skaters::install(app);
        peds::install(app);
        vehicles::install(app);
        vehicle_contacts::install(app);
    }
}

/// The ped behaviour runtime's switch and values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PedBrainSettings {
    pub enabled: bool,
    /// The mood system raises wants from the stock mood tables.
    pub mood: bool,
    /// Where a fleeing ped runs (retail values).
    pub flee: skate_core::living_world::peds::flee::FleeParams,
    /// RunFromHonker's goal and speeds (`826A1358`).
    pub run_from_honker: skate_core::living_world::peds::honk::RunFromHonkerParams,
    pub values: skate_core::living_world::peds::brain::BrainSettings,
}

impl Default for PedBrainSettings {
    fn default() -> Self {
        Self { enabled: true, mood: true, flee: Default::default(), run_from_honker: Default::default(), values: Default::default() }
    }
}
