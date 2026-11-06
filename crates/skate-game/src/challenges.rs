//! Retail challenges in play: the own the spot challenges from `private/challenges.json` (setup challenges.py) run
//! through `skate_core::challenges::ots` (docs/hails-additions/42-challenges.md).
//!
//! A run arms its volumes, feeds the state machine the player's trigger events, the banked sequences and 60 Hz timer
//! ticks, and carries out its commands. Points follow retail (research b841): a landed, non-bail sequence that passed
//! through the spot volume adds its score; otherwise nothing counts, and every sequence clears the spot.
//! Start: `SKATE_CHALLENGE=<id>` on that challenge's map (the retail sign-up flow is not ported yet).
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use skate_core::challenges::ots::{Command, Event, Ots, Params};
use skate_core::challenges::{Completion, Record};
use skate_data::trigger_volumes::TriggerVolumeRecord;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Volume {
    pub name: String,
    pub id: String,
    pub volume: TriggerVolumeRecord,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Start {
    pub locator: String,
    pub matrix: [[f32; 4]; 4],
}

/// One challenge as setup wrote it; `id` is the retail key (`ots_dwtn_01`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Challenge {
    pub id: String,
    pub family: String,
    pub map: String,
    #[serde(default)]
    pub title_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    pub boundary: Volume,
    pub scoring: Volume,
    pub spot: Volume,
    pub start: Start,
    /// The challenge locator (retail Location): centre of the sign-up and discovery spheres (research b842 / b843).
    pub location: Start,
    pub sign_up_radius: f32,
    pub discovery_radius: f32,
    pub owned_points: i32,
    pub killed_points: i32,
}

#[derive(Deserialize)]
struct File {
    format: String,
    version: u32,
    challenges: Vec<Challenge>,
}

pub(crate) fn load(assets: &Path) -> Result<Vec<Challenge>, String> {
    let path = assets.join("private/challenges.json");
    let file: File = serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if file.format != "skate3rust.challenges" || file.version != 1 {
        return Err(format!("{}: unsupported {} v{}", path.display(), file.format, file.version));
    }
    Ok(file.challenges)
}

/// Placeholders until the bank fields are located (doc 42 open questions); not retail values.
const OUT_OF_BOUNDS_TIME: f32 = 10.0;
const BANNER_TIME: f32 = 3.0;

fn params(c: &Challenge) -> Result<Params, String> {
    Ok(Params {
        boundary: u64::from_str_radix(&c.boundary.id, 16).map_err(|e| format!("{}: boundary id {e}", c.id))?,
        owned_points: c.owned_points,
        killed_points: c.killed_points,
        out_of_bounds_time: OUT_OF_BOUNDS_TIME,
        owned_display_time: BANNER_TIME,
        killed_display_time: BANNER_TIME,
    })
}

/// The objectives of a running own the spot (retail objective group, research b839 / b841).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Objectives {
    pub requirement: i32,
    pub points: i32,
    pub spot_entered: bool,
    pub evaluating: bool,
}

impl Objectives {
    /// A banked sequence: the event it raises, if any. Mode 5: the spot clears after every sequence.
    pub fn bank(&mut self, score: i32, wiped_out: bool) -> Option<Event> {
        let counted = self.evaluating && !wiped_out && self.spot_entered;
        self.spot_entered = false;
        if !counted {
            return None;
        }
        self.points += score;
        Some(if self.points >= self.requirement {
            Event::AllObjectivesComplete { points: self.points }
        } else {
            Event::NotEnoughPoints { points: self.points }
        })
    }

    pub fn reset(&mut self) {
        self.points = 0;
        self.spot_entered = false;
    }
}

pub(crate) struct Run {
    pub challenge: Challenge,
    pub ots: Ots,
    pub objectives: Objectives,
    tick_carry: f32,
    /// Inside the discovery sphere last frame (retail 8255BEB8 diffs it per frame); None before the first look.
    in_discovery: Option<bool>,
}

#[derive(Resource, Default)]
pub(crate) struct Challenges {
    pub retail: Vec<Challenge>,
    pub loaded: bool,
    pub generation: Option<u64>,
    pub run: Option<Run>,
    /// Per challenge id: best completion and high score (kept for the session; the save is not ported yet).
    pub records: BTreeMap<String, Record>,
    /// Commands from the start, carried out by the next update.
    pending: Vec<Command>,
}

impl Challenges {
    pub(crate) fn on_map<'a>(&'a self, map: Option<&'a Path>) -> impl Iterator<Item = &'a Challenge> + 'a {
        self.retail.iter().filter(move |c| map.is_some_and(|p| crate::teleport_menu::same_map(p, &c.map)))
    }
}

fn arm_volumes(volumes: &mut crate::trigger_volumes::TriggerVolumes, c: &Challenge) {
    for v in [&c.boundary, &c.scoring, &c.spot] {
        if volumes.get(&v.id).is_none() {
            volumes.map.push(crate::trigger_volumes::TriggerVolume::from_record(&v.volume));
        }
    }
}

/// A new map: read the challenges once, drop a run from the old map, start `SKATE_CHALLENGE` if it is on this one.
fn arm(
    mut challenges: ResMut<Challenges>,
    config: Res<crate::config::Config>,
    map: Res<crate::map_transition::CurrentMap>,
    mut volumes: ResMut<crate::trigger_volumes::TriggerVolumes>,
) {
    if challenges.generation == Some(map.generation) {
        return;
    }
    challenges.generation = Some(map.generation);
    if !challenges.loaded {
        challenges.loaded = true;
        challenges.retail = load(&config.asset_root).unwrap_or_else(|e| {
            info!("SKATE_CHALLENGES none ({e})");
            Vec::new()
        });
    }
    if let Some(run) = challenges.run.take() {
        info!("SKATE_CHALLENGE {} dropped by the map change", run.challenge.id);
    }
    let count = challenges.on_map(map.path.as_deref()).count();
    info!("SKATE_CHALLENGES map={} own_the_spot={count}", map.name);
    let Ok(wanted) = std::env::var("SKATE_CHALLENGE") else { return };
    let Some(c) = challenges.on_map(map.path.as_deref()).find(|c| c.id == wanted).cloned() else {
        info!("SKATE_CHALLENGE {wanted} is not on {}", map.name);
        return;
    };
    match params(&c) {
        Ok(p) => {
            arm_volumes(&mut volumes, &c);
            let best = challenges.records.get(&c.id).map_or(0, |r| r.high_score);
            info!("SKATE_CHALLENGE start {} owned={} killed={} best={best}", c.id, c.owned_points, c.killed_points);
            challenges.run = Some(Run { ots: Ots::new(p, best, false), challenge: c, objectives: Objectives::default(), tick_carry: 0., in_discovery: None });
            let commands = challenges.run.as_mut().map(|r| r.ots.start()).unwrap_or_default();
            challenges.pending.extend(commands);
        }
        Err(e) => warn!("SKATE_CHALLENGE {e}"),
    }
}

/// One frame of a run: events in, timers, commands out.
#[allow(clippy::too_many_arguments)]
fn update(
    mut challenges: ResMut<Challenges>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
    mut entered: MessageReader<crate::trigger_volumes::TriggerEntered>,
    mut exited: MessageReader<crate::trigger_volumes::TriggerExited>,
    time: Res<Time>,
) {
    let banked = skater.scoring.take_banked();
    let entered: Vec<String> = entered.read().filter(|e| e.body == crate::trigger_volumes::PLAYER).map(|e| e.volume.clone()).collect();
    let exited: Vec<String> = exited.read().filter(|e| e.body == crate::trigger_volumes::PLAYER).map(|e| e.volume.clone()).collect();
    let mut commands = std::mem::take(&mut challenges.pending);
    let Some(run) = challenges.run.as_mut() else { return };
    let volume_event = |id: &str, enter: bool| {
        u64::from_str_radix(id, 16).ok().map(|volume| if enter { Event::TriggerEnter { volume } } else { Event::TriggerExit { volume } })
    };
    for id in &exited {
        if let Some(e) = volume_event(id, false) {
            commands.extend(run.ots.handle(e));
        }
    }
    for id in &entered {
        if *id == run.challenge.spot.id {
            run.objectives.spot_entered = true;
        }
        if let Some(e) = volume_event(id, true) {
            commands.extend(run.ots.handle(e));
        }
    }
    let feet = crate::trigger_volumes::player_points(&skater)[0];
    let centre = run.challenge.location.matrix[3];
    let distance = ((feet[0] - centre[0]).powi(2) + (feet[1] - centre[1]).powi(2) + (feet[2] - centre[2]).powi(2)).sqrt();
    let inside = distance <= run.challenge.discovery_radius;
    if run.in_discovery == Some(true) && !inside {
        info!("SKATE_CHALLENGE {} left the discovery sphere ({distance:.1} m)", run.challenge.id);
        commands.extend(run.ots.handle(Event::ExitDiscovery));
    }
    run.in_discovery = Some(inside);
    for s in banked {
        if let Some(e) = run.objectives.bank(s.score, s.wiped_out) {
            info!("SKATE_CHALLENGE sequence score={} points={} requirement={}", s.score, run.objectives.points, run.objectives.requirement);
            commands.extend(run.ots.handle(e));
        }
    }
    run.tick_carry += time.delta_secs() * skate_core::challenges::timers::TICKS_PER_SECOND;
    while run.tick_carry >= 1. {
        run.tick_carry -= 1.;
        commands.extend(run.ots.tick());
    }
    let mut finished = None;
    let mut i = 0;
    while i < commands.len() {
        let command = commands[i].clone();
        i += 1;
        match command {
            Command::Teleport => {
                if let Err(e) = skater.travel_ex(run.challenge.start.matrix, None, true) {
                    warn!("SKATE_CHALLENGE teleport {e}");
                }
            }
            // The challenge HUD movies are exported; until they run, the HUD counts as loaded at once.
            Command::CreateHud => commands.extend(run.ots.handle(Event::HudLoaded)),
            // The highlight reel is not ported: it completes at once (the run returns sooner than retail).
            Command::PlayHighlightReel => commands.extend(run.ots.handle(Event::HighlightReelComplete)),
            Command::Evaluation(on) => run.objectives.evaluating = on,
            Command::ResetObjectives => run.objectives.reset(),
            Command::SetPointRequirement(points) => run.objectives.requirement = points,
            Command::Complete(c) => finished = Some(c),
            other => info!("SKATE_CHALLENGE {} {other:?}", run.challenge.id),
        }
    }
    if let Some(completion) = finished {
        let run = challenges.run.take().expect("a run");
        let record = challenges.records.entry(run.challenge.id.clone()).or_default();
        record.complete(completion);
        record.high_score = record.high_score.max(run.ots.personal_best());
        info!("SKATE_CHALLENGE end {} completion={completion:?} best={} record={:?}", run.challenge.id,
            run.ots.personal_best(), record.completion.unwrap_or(Completion::Abandoned));
    }
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Challenges>()
        .add_systems(PreUpdate, arm.after(crate::map_transition::MapTransitionSet))
        .add_systems(Update, update.after(crate::trigger_volumes::TriggerSet));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_landed_line_through_the_spot_counts() {
        let mut o = Objectives { requirement: 1000, evaluating: true, ..default() };
        assert_eq!(o.bank(5000, false), None);
        o.spot_entered = true;
        assert_eq!(o.bank(5000, true), None);
        assert!(!o.spot_entered);
        o.spot_entered = true;
        assert_eq!(o.bank(400, false), Some(Event::NotEnoughPoints { points: 400 }));
        o.reset();
        o.spot_entered = true;
        assert_eq!(o.bank(1200, false), Some(Event::AllObjectivesComplete { points: 1200 }));
    }

    #[test]
    fn nothing_counts_while_scoring_is_off() {
        let mut o = Objectives { requirement: 1000, spot_entered: true, ..default() };
        assert_eq!(o.bank(5000, false), None);
    }

    #[test]
    fn the_export_loads() {
        let dir = std::env::temp_dir().join(format!("skate-challenges-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("private")).unwrap();
        let volume = serde_json::json!({"id": "0008b0cd03e38706", "name": "b",
            "shape": {"kind": "box", "center": [0., 0., 0.], "axes": [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                      "half_extents": [1., 1., 1.], "fatness": 0.}});
        let v = serde_json::json!({"name": "b", "id": "0008b0cd03e38706", "volume": volume});
        let file = serde_json::json!({"format": "skate3rust.challenges", "version": 1, "challenges": [{
            "id": "ots_dwtn_01", "family": "own_the_spots", "map": "DownTown", "boundary": v, "scoring": v, "spot": v,
            "start": {"locator": "s", "matrix": [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]]},
            "location": {"locator": "l", "matrix": [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]]},
            "sign_up_radius": 10., "discovery_radius": 100., "owned_points": 1000, "killed_points": 1500}]});
        std::fs::write(dir.join("private/challenges.json"), file.to_string()).unwrap();
        let loaded = load(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(loaded[0].id, "ots_dwtn_01");
        assert_eq!(params(&loaded[0]).unwrap().boundary, 0x0008_B0CD_03E3_8706);
    }
}
