//! Races in play, player first: a race from `private/challenges.json` `races` runs through
//! `skate_core::challenges::race_run` with the race tracker and its AI racers (`race_ai_host`). Start:
//! `SKATE_RACE=<id>` on that race's map. No race HUD movies or sign-up yet. Details: docs/hails-additions/42-challenges.md#change-races-in-play-race_hostrs
use crate::challenges::{Start, Volume};
use crate::hud_challenge::{ChallengeHudState, Component};
use bevy::prelude::*;
use serde::Deserialize;
use skate_core::challenges::race::{Course, RaceTracker, RaceTuning};
use skate_core::challenges::race_run::{Banner, Command, Event, Params, RaceRun};
use skate_core::challenges::timers::TICKS_PER_SECOND;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct StartSlots {
    #[serde(flatten)]
    pub start: Start,
    /// Competitor n's start (the player is 0; research b989 / b992).
    #[serde(default)]
    pub slots: Vec<[[f32; 4]; 4]>,
}

impl StartSlots {
    pub(crate) fn slot(&self, n: usize) -> [[f32; 4]; 4] {
        self.slots.get(n).copied().unwrap_or(self.start.matrix)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Leg {
    #[serde(default)]
    pub end_time_bonus: i32,
    pub splits: Vec<Volume>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Heat {
    pub start: StartSlots,
    pub time_limit: Option<i32>,
    pub time_limit_warning: Option<i32>,
    pub killed_it_time: Option<f32>,
    pub legs: Vec<Leg>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Race {
    pub id: String,
    pub map: String,
    pub heats: Vec<Heat>,
    /// The single player competitors (b987): ai_characters rows; kind 1 (a random pick from a pool) is not used by
    /// any disc race and is skipped.
    #[serde(default)]
    pub competitors: Vec<CompetitorRow>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CompetitorRow {
    pub character: String,
    #[serde(default)]
    pub kind: u32,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    races: Vec<Race>,
}

pub(crate) fn load(assets: &std::path::Path) -> Result<Vec<Race>, String> {
    let path = assets.join("private/challenges.json");
    let file: File = serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(file.races)
}

fn centre(v: &Volume) -> [f32; 3] {
    v.volume.shape.oriented_box().center
}

fn split_id(v: &Volume) -> Option<u64> {
    u64::from_str_radix(&v.id, 16).ok()
}

/// The race's first heat as run parameters (the disc races have one heat each).
pub(crate) fn params(race: &Race) -> Result<Params, String> {
    let heat = race.heats.first().ok_or("no heat")?;
    let splits: Vec<Vec<u64>> = heat.legs.iter().map(|l| l.splits.iter().filter_map(split_id).collect()).collect();
    if splits.is_empty() || splits.iter().any(|s| s.is_empty()) {
        return Err(format!("{}: a leg without split triggers", race.id));
    }
    let mut p = Params::new(splits, heat.time_limit.unwrap_or(0) as f32, heat.time_limit_warning.unwrap_or(0) as f32,
        heat.killed_it_time.unwrap_or(0.0));
    p.end_time_bonus = heat.legs.iter().map(|l| l.end_time_bonus as f32).collect();
    Ok(p)
}

pub(crate) struct RaceState {
    pub race: Race,
    pub run: RaceRun,
    pub tracker: RaceTracker,
    pub ai: Option<crate::race_ai_host::RaceAi>,
    tick_carry: f32,
}

#[derive(Resource, Default)]
pub(crate) struct Races {
    pub races: Vec<Race>,
    loaded: bool,
    generation: Option<u64>,
    pub run: Option<RaceState>,
}

const PLAYER: u32 = 0;
const RACE_FINISHED: &str = "ID_RACE_FINISHED";
const CHALLENGE_FAILED: &str = "ID_MISSION_CHALLENGE_FAILED";

fn tracker_for(heat: &Heat, tuning: RaceTuning) -> RaceTracker {
    let start = heat.start.slot(0)[3];
    let course = Course::new([start[0], start[1], start[2]], &heat.legs.iter().map(|l| l.splits.iter().map(centre).collect()).collect::<Vec<_>>());
    let mut tracker = RaceTracker::new(course, tuning);
    tracker.add_racer(PLAYER, None);
    tracker
}

fn start(races: &mut Races, volumes: &mut crate::trigger_volumes::TriggerVolumes, best: f32, race: Race, assets: &std::path::Path, seed: u64)
    -> Vec<Command> {
    let p = match params(&race) {
        Ok(p) => p,
        Err(e) => {
            warn!("SKATE_RACE {e}");
            return Vec::new();
        }
    };
    for v in race.heats.iter().flat_map(|h| h.legs.iter()).flat_map(|l| l.splits.iter()) {
        if volumes.get(&v.id).is_none() {
            volumes.map.push(crate::trigger_volumes::TriggerVolume::from_record(&v.volume));
        }
    }
    info!("SKATE_RACE start {} splits={:?} limit={} killed_it={} best={best}", race.id,
        p.splits.iter().map(Vec::len).collect::<Vec<_>>(), p.time_limit, p.killed_it_time);
    let tracker = tracker_for(&race.heats[0], RaceTuning::default());
    let names: Vec<String> = race.competitors.iter().filter(|c| c.kind == 0).map(|c| c.character.clone()).collect();
    let ai = crate::race_ai_host::RaceAi::load(assets, &race.id, &names, seed);
    let mut run = RaceRun::new(p, best);
    let commands = run.start();
    races.run = Some(RaceState { race, run, tracker, ai, tick_carry: 0.0 });
    commands
}

/// A new map: read the races once, drop a run from the old map, start `SKATE_RACE` if it is on this one.
fn arm(
    mut races: ResMut<Races>,
    mut challenges: ResMut<crate::challenges::Challenges>,
    config: Res<crate::config::Config>,
    map: Res<crate::map_transition::CurrentMap>,
    mut volumes: ResMut<crate::trigger_volumes::TriggerVolumes>,
    mut hud: ResMut<ChallengeHudState>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
    mut population: Option<ResMut<crate::living_world::PopulationState>>,
) {
    if races.generation == Some(map.generation) {
        return;
    }
    races.generation = Some(map.generation);
    if !races.loaded {
        races.loaded = true;
        races.races = load(&config.asset_root).unwrap_or_else(|e| {
            info!("SKATE_RACES none ({e})");
            Vec::new()
        });
    }
    if let Some(run) = races.run.take() {
        info!("SKATE_RACE {} dropped by the map change", run.race.id);
        *hud = ChallengeHudState::default();
    }
    let on_map: Vec<Race> = races.races.iter()
        .filter(|r| map.path.as_deref().is_some_and(|p| crate::teleport_menu::same_map(p, &r.map))).cloned().collect();
    info!("SKATE_RACES map={} races={}", map.name, on_map.len());
    let Ok(wanted) = std::env::var("SKATE_RACE") else { return };
    let Some(race) = on_map.into_iter().find(|r| r.id == wanted) else {
        info!("SKATE_RACE {wanted} is not on {}", map.name);
        return;
    };
    let best = challenges.records.get(&race.id).best_time.unwrap_or(0.0);
    // The line-up shuffle follows the session's population seed (one authority picks it; b987 §3).
    let seed = population.as_ref().map_or(0, |p| skate_core::living_world::rng::derive(p.world.seed(), &[map.generation]));
    let commands = start(&mut races, &mut volumes, best, race, &config.asset_root, seed);
    apply(&mut races, &mut challenges, &mut skater, &mut hud, None, population.as_deref_mut(), commands);
}

fn update(
    mut races: ResMut<Races>,
    mut challenges: ResMut<crate::challenges::Challenges>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
    mut hud: ResMut<ChallengeHudState>,
    mut central: MessageWriter<crate::hud_central::CentralMessage>,
    mut entered: MessageReader<crate::trigger_volumes::TriggerEntered>,
    mut career: MessageWriter<crate::career::ChallengeResult>,
    time: Res<Time>,
    (mut population, index, transforms, mut clocks, volumes, mut commands_ecs): (
        Option<ResMut<crate::living_world::PopulationState>>,
        Option<Res<crate::living_world::npc_skaters::NpcSkaterIndex>>,
        Query<&Transform>,
        Query<&mut crate::living_world::npc_skaters::NpcRaceClock>,
        Res<crate::trigger_volumes::TriggerVolumes>,
        Commands,
    ),
) {
    let entered: Vec<u64> = entered.read().filter(|e| e.body == crate::trigger_volumes::PLAYER)
        .filter_map(|e| u64::from_str_radix(&e.volume, 16).ok()).collect();
    let Some(state) = races.run.as_mut() else { return };
    let mut commands = Vec::new();
    for volume in entered {
        commands.extend(state.run.handle(Event::TriggerEnter { volume }));
    }
    state.tick_carry += time.delta_secs() * TICKS_PER_SECOND;
    let mut ticked = false;
    while state.tick_carry >= 1.0 {
        state.tick_carry -= 1.0;
        commands.extend(state.run.tick());
        ticked = true;
    }
    if ticked {
        let feet = crate::trigger_volumes::player_points(&skater)[0];
        let elapsed = state.run.elapsed();
        let mut positions = match (state.ai.as_ref(), index.as_deref()) {
            (Some(ai), Some(index)) => ai.positions(index, &transforms),
            _ => Default::default(),
        };
        positions.insert(PLAYER, feet);
        state.tracker.update(|id| positions.get(&id).copied(), |_| elapsed);
        if let (Some(ai), Some(index), Some(pop)) = (state.ai.as_mut(), index.as_deref(), population.as_deref()) {
            let finished = ai.step(&state.race.heats[0], &volumes, &positions, &mut state.tracker, PLAYER, elapsed, pop.world.tick(), index,
                &mut clocks, &mut commands_ecs);
            for _ in 0..finished {
                commands.extend(state.run.handle(Event::OpponentFinished));
            }
        }
    }
    let messages = apply(&mut races, &mut challenges, &mut skater, &mut hud, Some(&mut career), population.as_deref_mut(), commands);
    for m in messages {
        central.write(m);
    }
    if let Some(state) = races.run.as_ref() {
        if hud.is_shown(Component::Timer) {
            let (text, flag) = skate_core::challenges::timers::hud_text(state.run.limit_ticks().unwrap_or(0));
            if hud.timer_text != text || hud.timer_flag != flag {
                hud.timer_text = text;
                hud.timer_flag = flag;
            }
        }
    }
}

/// Carries out the run's commands; returns the central messages to show.
fn apply(
    races: &mut Races,
    challenges: &mut crate::challenges::Challenges,
    skater: &mut crate::physics::SkaterRuntime,
    hud: &mut ChallengeHudState,
    mut career: Option<&mut MessageWriter<crate::career::ChallengeResult>>,
    mut population: Option<&mut crate::living_world::PopulationState>,
    mut commands: Vec<Command>,
) -> Vec<crate::hud_central::CentralMessage> {
    let mut messages = Vec::new();
    let Some(state) = races.run.as_mut() else { return messages };
    let (mut finished, mut changed) = (None, false);
    let mut i = 0;
    while i < commands.len() {
        let command = commands[i].clone();
        i += 1;
        let id = state.race.id.clone();
        match command {
            Command::TeleportToStart => {
                let slot = state.race.heats[0].start.slot(PLAYER as usize);
                info!("SKATE_RACE {id} teleport to slot 0 {:?}", &slot[3][..3]);
                if let Err(e) = skater.travel_ex(slot, None, true) {
                    warn!("SKATE_RACE teleport {e}");
                }
                if let (Some(ai), Some(pop)) = (state.ai.as_mut(), population.as_deref_mut()) {
                    ai.place(&state.race.heats[0], pop);
                }
            }
            Command::RaceReset => {
                state.tracker = tracker_for(&state.race.heats[0], state.tracker.tuning);
                if let Some(ai) = state.ai.as_ref() {
                    ai.add_to(&mut state.tracker);
                }
            }
            Command::RaceStart => {
                if let Some(ai) = state.ai.as_mut() {
                    ai.go();
                }
            }
            Command::RaceStop => {
                if let Some(ai) = state.ai.as_mut() {
                    ai.stop();
                }
            }
            Command::RaceSetProgress { leg, split } => {
                info!("SKATE_RACE {id} split leg={leg} split={split}");
                state.tracker.set_progress(PLAYER, leg, split);
            }
            Command::RaceFinishPlayer { time } => {
                info!("SKATE_RACE {id} finish time={time:.2} rank={}", state.tracker.rank(PLAYER));
                state.tracker.finish(PLAYER, time);
            }
            Command::StartTimerHud { .. } => hud.show(Component::Timer, true),
            Command::RaceFinishedMessage => messages.push(crate::hud_central::CentralMessage::new(RACE_FINISHED)),
            Command::TimeUpMessage => messages.push(crate::hud_central::CentralMessage::new(CHALLENGE_FAILED)),
            Command::Banner(Banner::OwnedIt, shown) => hud.show(Component::OwnedIt, shown),
            Command::Banner(Banner::KilledIt, shown) => hud.show(Component::KilledIt, shown),
            Command::Banner(Banner::Failed, _) => {}
            Command::RemoveGates | Command::Leaderboard(false) => hud.show(Component::Timer, false),
            Command::SetBestTime(t) => {
                info!("SKATE_RACE {id} best time {t:.2}");
                changed |= challenges.records.update(&id, |s| s.best_time = Some(t));
            }
            // The results screen is not ported: Continue at once (as the end dialog does without its movie).
            Command::Results { time, place } => {
                info!("SKATE_RACE {id} results time={time:?} place={place:?}: continue");
                commands.extend(state.run.handle(Event::Continue));
            }
            Command::Complete(c) => finished = Some(c),
            other => info!("SKATE_RACE {id} {other:?}"),
        }
    }
    if let Some(completion) = finished {
        let id = state.race.id.clone();
        changed |= challenges.records.update(&id, |s| {
            let mut r = s.record();
            r.complete(completion);
            s.set_record(r);
        });
        info!("SKATE_RACE end {id} completion={completion:?} best_time={:?}", challenges.records.get(&id).best_time);
        challenges.mods.event(serde_json::json!({"name": "challenge_ended", "challenge": id,
            "completion": crate::challenges::completion_name(completion)}));
        if let Some(career) = career.as_mut() {
            career.write(crate::career::ChallengeResult { challenge: id, completion, reactivate: false });
        }
        *hud = ChallengeHudState::default();
        if let (Some(mut run), Some(pop)) = (races.run.take(), population.as_deref_mut()) {
            if let Some(ai) = run.ai.as_mut() {
                ai.remove(pop);
            }
        }
        races.run = None;
    }
    if changed {
        challenges.records.save_in_background();
    }
    messages
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Races>()
        .add_systems(PreUpdate, arm.after(crate::map_transition::MapTransitionSet).run_if(crate::challenges::opted_in))
        .add_systems(Update, update.after(crate::trigger_volumes::TriggerSet).in_set(crate::challenges::CareerResultSet)
            .run_if(crate::challenges::opted_in));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disc_shaped_race_row_loads_and_gives_run_params() {
        let json = r#"{"races": [{"id": "race_x", "map": "university", "heats": [{
            "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[5,0,0,1]],
                      "slots": [[[1,0,0,0],[0,1,0,0],[0,0,1,0],[6,0,0,1]]]},
            "time_limit": 180, "time_limit_warning": 30, "killed_it_time": 90.0,
            "legs": [{"end_time_bonus": 0, "splits": [{"name": "fin", "id": "00000000000000ab", "volume": {
                "id": "00000000000000ab", "name": "fin", "group": "challenge",
                "shape": {"kind": "box", "center": [10, 0, 0], "half_extents": [1, 1, 1]}}}]}]}]}]}"#;
        let file: File = serde_json::from_str(json).unwrap();
        let race = &file.races[0];
        let p = params(race).unwrap();
        assert_eq!(p.splits, vec![vec![0xab]]);
        assert_eq!((p.time_limit, p.time_limit_warning, p.killed_it_time), (180.0, 30.0, 90.0));
        assert_eq!(race.heats[0].start.slot(0)[3], [6.0, 0.0, 0.0, 1.0]);
        assert_eq!(race.heats[0].start.slot(3)[3], [5.0, 0.0, 0.0, 1.0], "past the slots: the locator");
        let tracker = tracker_for(&race.heats[0], RaceTuning::default());
        assert_eq!(tracker.course.legs[0].points, vec![[6.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
    }
}
