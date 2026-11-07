//! Retail challenges in play: the own the spot challenges from `private/challenges.json` (setup challenges.py) run
//! through `skate_core::challenges::ots` (docs/hails-additions/42-challenges.md).
//!
//! A run arms its volumes, feeds the state machine the player's trigger events, the banked sequences and 60 Hz timer
//! ticks, and carries out its commands. Points follow retail (research b841): a landed, non-bail sequence that passed
//! through the spot volume adds its score; otherwise nothing counts, and every sequence clears the spot.
//! Start: the sign-up prompt inside the challenge's sign-up sphere (D-pad Up, no confirm popup; research b843), or
//! `SKATE_CHALLENGE=<id>` on that challenge's map.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use crate::hud_challenge::{ChallengeHudState, Component, DpoSlot, ObjectiveRow};
use skate_core::challenges::ots::{Command, Event, HudComponent, Ots, Params, TargetText};
use skate_core::challenges::Completion;
use skate_data::trigger_volumes::TriggerVolumeRecord;
use std::collections::{BTreeMap, BTreeSet};
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
    /// Retail `eChallengeTypes` (own the spot 9), inherited through the bank row's parents.
    #[serde(default)]
    pub challenge_type: Option<i32>,
    /// The challenge parameters (research b847; absent in an export from before them: the retail defaults below).
    #[serde(default)]
    pub out_of_bounds_time: Option<i32>,
    #[serde(default)]
    pub owned_display_time: Option<f32>,
    #[serde(default)]
    pub killed_display_time: Option<f32>,
    /// The central message id shown on leaving the challenge boundary (`CentralMessageHALIDReturnToChallengeArea`).
    #[serde(default)]
    pub return_to_area_message: Option<String>,
    /// The owned it / killed it objective rows as data (absent in an older export: the native check runs).
    #[serde(default)]
    pub objectives: Option<crate::challenge_objectives::ObjectiveSets>,
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

/// The local skater signed up for a challenge (retail 0x905C8249; serialisable: the authority starts the run).
#[derive(Message, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChallengeSignUp {
    pub body: String,
    pub challenge: String,
}

/// eChallengeTypes of own the spot.
const OWN_THE_SPOT_TYPE: i32 = 9;

/// The action overlay state of a challenge's sign-up (82665E88, research b849): one own the spot (eChallengeTypes 9)
/// shows the OTS state 5 ("Activate Spot"), any other challenge the Challenge state 1. An export from before the
/// type keys on the family.
pub(crate) fn sign_up_state(c: &Challenge) -> i32 {
    let ots = c.challenge_type.map_or(c.family == "own_the_spots", |t| t == OWN_THE_SPOT_TYPE);
    if ots { 5 } else { 1 }
}

/// eChallengeTypes of create-a-spot: listed last, labelled by `CREATE_A_SPOT_LABEL` (research b851).
const CREATE_A_SPOT_TYPE: i32 = 35;
const CREATE_A_SPOT_LABEL: &str = "#CreateASpot Needs A Name!";
/// The overlay text and the pick popup's title with several challenges in range (82665DA0 / 826660F0, b851).
pub(crate) const MULTIPLE_CHALLENGES: &str = "ID_SIGNUP_MULTIPLIESIGNUP_SIGNUPHUD";
/// At most this many challenges in the pick popup (826660F0).
const PICK_MAX: usize = 4;
pub(crate) const PICK_POPUP: &str = "engine:challenge.pick";
/// A press counts only once the prompt has shown this long: retail's show sets 15 component ticks (82665CD0, research
/// b854), counted at the console's 30 a second.
pub(crate) const REPRESS_DELAY_SECONDS: f32 = 15.0 / 30.0;

/// Every challenge whose sign-up sphere (`sign_up_radius` around `location`) holds `point`, in retail's list order
/// (82D29C38, b851): create-a-spot ones last in query order, the others before them in reverse query order.
pub(crate) fn in_sign_up<'a>(candidates: impl Iterator<Item = &'a Challenge>, point: [f32; 3]) -> Vec<&'a Challenge> {
    let found: Vec<&Challenge> = candidates.filter(|c| distance(point, c.location.matrix[3]) <= c.sign_up_radius).collect();
    let spot = |c: &&Challenge| c.challenge_type == Some(CREATE_A_SPOT_TYPE);
    found.iter().rev().filter(|c| !spot(c)).chain(found.iter().filter(|c| spot(c))).copied().collect()
}

/// The pick popup for several challenges in range: "Multiple Challenges", an empty body, one entry per challenge
/// (its Title id), B cancels (826660F0, b851).
pub(crate) fn pick_popup(list: &[Challenge]) -> skate_core::fe_popup::PopupRequest {
    let label = |c: &Challenge| if c.challenge_type == Some(CREATE_A_SPOT_TYPE) {
        CREATE_A_SPOT_LABEL.to_owned()
    } else {
        c.title_id.clone().or_else(|| c.title.clone()).unwrap_or_else(|| c.id.clone())
    };
    skate_core::fe_popup::PopupRequest {
        title: MULTIPLE_CHALLENGES.into(),
        description: "#".into(),
        options: list.iter().take(PICK_MAX).map(label).collect(),
        cancelable: true,
        callback: PICK_POPUP.into(),
        ..Default::default()
    }
}

fn distance(a: [f32; 3], b: [f32; 4]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The HUD text ids (research b845): the DPO titles (target, personal best) and the objective row per target.
pub(crate) const DPO_TARGET_TITLE: &str = "ID_COMMON_TARGET";
pub(crate) const DPO_PERSONAL_BEST_TITLE: &str = "ID_DPO_PERSONALBEST";
pub(crate) const NEW_PERSONAL_BEST: &str = "ID_LEADERBOARD_NEW_PERSONAL_BEST";
/// The central message style for `HUD2.eMessageColour.Green` (White 0, Red 1, Green 2, 8284DC78; the binding
/// 82850618 passes the colour through as the style; research b847).
const GREEN_STYLE: i32 = 2;

pub(crate) fn objective_text(text: TargetText) -> &'static str {
    match text {
        TargetText::BeatOwnSpot => "ID_CHALLENGE_OBJECTIVE_BEAT_OWNSPOT",
        TargetText::BeatKillSpot => "ID_CHALLENGE_OBJECTIVE_BEAT_KILLSPOT",
        TargetText::BeatPersonalScore => "ID_CHALLENGE_OBJECTIVE_BEAT_PERSONALSCORE",
    }
}

fn hud_component(c: HudComponent) -> Option<Component> {
    match c {
        // Own the spot never starts the countdown (b844).
        HudComponent::Countdown => None,
        HudComponent::Timer => Some(Component::Timer),
        HudComponent::Objective => Some(Component::Objective),
        HudComponent::Dpo => Some(Component::Dpo),
        HudComponent::OwnedIt => Some(Component::OwnedIt),
        HudComponent::KilledIt => Some(Component::KilledIt),
    }
}

/// `UpdateHud`: DPO 0 = score / target, DPO 1 = score / personal best, objective row 0 = the target text (b845).
pub(crate) fn update_hud(hud: &mut ChallengeHudState, score: i32, target: i32, personal_best: i32, text: TargetText) {
    hud.dpo[0] = DpoSlot { numerator: score.to_string(), denominator: target.to_string(), title: DPO_TARGET_TITLE.into(), highlight: hud.dpo[0].highlight };
    hud.dpo[1] = DpoSlot { numerator: score.to_string(), denominator: personal_best.to_string(), title: DPO_PERSONAL_BEST_TITLE.into(), highlight: hud.dpo[1].highlight };
    hud.two_dpos = true;
    hud.objectives = vec![ObjectiveRow { state: 3, kind: 0, title: objective_text(text).into(), value: String::new() }];
}

/// The framework vault's `default` row (research b847), for an export from before the parameters.
const OUT_OF_BOUNDS_TIME: i32 = 5;
const OWNED_DISPLAY_TIME: f32 = 3.1;
const KILLED_DISPLAY_TIME: f32 = 3.3;
const RETURN_TO_AREA: &str = "ID_CHALLENGE_CENTRAL_MESSAGE_OBJECTIVE_RETURNTOCHAL";

fn params(c: &Challenge) -> Result<Params, String> {
    Ok(Params {
        boundary: u64::from_str_radix(&c.boundary.id, 16).map_err(|e| format!("{}: boundary id {e}", c.id))?,
        owned_points: c.owned_points,
        killed_points: c.killed_points,
        out_of_bounds_time: c.out_of_bounds_time.unwrap_or(OUT_OF_BOUNDS_TIME) as f32,
        owned_display_time: c.owned_display_time.unwrap_or(OWNED_DISPLAY_TIME),
        killed_display_time: c.killed_display_time.unwrap_or(KILLED_DISPLAY_TIME),
    })
}

/// The objectives of a running own the spot (retail objective group, research b839 / b841).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Objectives {
    pub requirement: i32,
    pub points: i32,
    pub spot_entered: bool,
    pub evaluating: bool,
    /// The objective group from the exported rows; when present it decides instead of the native check.
    pub data: Option<crate::challenge_objectives::DataObjectives>,
}

impl Objectives {
    /// A banked sequence: the event it raises, if any. Mode 5: the spot clears after every sequence.
    pub fn bank(&mut self, score: i32, wiped_out: bool) -> Option<Event> {
        if let Some(data) = self.data.as_mut() {
            self.spot_entered = false;
            if !self.evaluating {
                return None;
            }
            let banked = data.bank(score, wiped_out, self.requirement);
            self.points = banked.points;
            // The exported group lists only the spot row; retail's points objective (group points >= requirement,
            // `global_points_objective`, b840) is not in it, so its gate is applied here.
            return if banked.all_complete && banked.points >= self.requirement {
                Some(Event::AllObjectivesComplete { points: banked.points })
            } else if banked.completed {
                Some(Event::NotEnoughPoints { points: banked.points })
            } else {
                None
            };
        }
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
        let requirement = self.requirement;
        if let Some(data) = self.data.as_mut() {
            data.reset(requirement);
        }
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

/// What mods change (`sdk.challenges`, modding/challenges.rs): hidden challenges, value overrides by (mod, challenge),
/// an abort request, and the events queued for `on_event`.
#[derive(Default)]
pub(crate) struct ModControl {
    pub disabled: BTreeMap<String, BTreeSet<String>>,
    pub overrides: BTreeMap<(String, String), skate_mods::ChallengeOptions>,
    pub abort: bool,
    pub events: Vec<serde_json::Value>,
}

/// Events kept for mods between two mod ticks; older ones drop first.
const MOD_EVENTS_MAX: usize = 64;

impl ModControl {
    pub(crate) fn enabled(&self, id: &str) -> bool {
        self.disabled.get(id).is_none_or(BTreeSet::is_empty)
    }

    /// The challenge with every mod's overrides on top of the setup data, in mod id order.
    pub(crate) fn apply(&self, mut c: Challenge) -> Challenge {
        for o in self.overrides.iter().filter(|((_, id), _)| *id == c.id).map(|(_, o)| o) {
            c.owned_points = o.owned_points.unwrap_or(c.owned_points);
            c.killed_points = o.killed_points.unwrap_or(c.killed_points);
            c.out_of_bounds_time = o.out_of_bounds_time.or(c.out_of_bounds_time);
            c.owned_display_time = o.owned_display_time.or(c.owned_display_time);
            c.killed_display_time = o.killed_display_time.or(c.killed_display_time);
        }
        c
    }

    pub(crate) fn event(&mut self, event: serde_json::Value) {
        if self.events.len() >= MOD_EVENTS_MAX {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    /// Everything one mod set, when it stops.
    pub(crate) fn clear_owner(&mut self, owner: &str) {
        self.disabled.values_mut().for_each(|o| { o.remove(owner); });
        self.disabled.retain(|_, o| !o.is_empty());
        self.overrides.retain(|(o, _), _| o != owner);
    }
}

#[derive(Resource, Default)]
pub(crate) struct Challenges {
    pub retail: Vec<Challenge>,
    pub loaded: bool,
    pub generation: Option<u64>,
    pub run: Option<Run>,
    /// Per challenge id: best completion, high score and discovery status (`settings/challenges.json`).
    pub records: crate::challenge_records::Records,
    /// Commands from the start, carried out by the next update.
    pending: Vec<Command>,
    /// The challenge whose sign-up prompt shows, and the input batch last read for its press.
    signing_up: Vec<String>,
    /// The challenges offered in the open pick popup, by entry.
    picking: Option<Vec<String>>,
    /// Seconds the prompt has shown (reset each time it appears), for the re-press delay.
    prompt_age: f32,
    last_batch: u64,
    /// `--verify` runs: `SKATE_VERIFY_CHALLENGE=<id>` places the skater at that challenge's sign-up and signs up
    /// once its prompt has shown for a moment (the frames it has shown so far).
    verify: Option<(String, u32)>,
    pub mods: ModControl,
    /// The career's activated challenges while gating is on (`career.rs`); `None` = every challenge is available.
    pub gate: Option<BTreeSet<String>>,
    /// The tutorial rows (research b858) and the one running; `verify_tutorial` = `SKATE_VERIFY_TUTORIAL` for test runs.
    pub tutorials: Vec<crate::tutorial_run::TutorialData>,
    pub tutorial: Option<crate::tutorial_run::TutorialRun>,
    tutorial_carry: f32,
    verify_tutorial: Option<String>,
    /// The challenge trick groups (b863), read from the stock collections when the first tutorial starts.
    trick_groups: Option<crate::challenge_objectives::TrickGroups>,
}

impl Challenges {
    pub(crate) fn on_map<'a>(&'a self, map: Option<&'a Path>) -> impl Iterator<Item = &'a Challenge> + 'a {
        self.retail.iter().filter(move |c| map.is_some_and(|p| crate::teleport_menu::same_map(p, &c.map)))
    }

    /// Discoverable and open to sign-up: no mod hid it, and the career activated it when gating is on.
    pub(crate) fn available(&self, id: &str) -> bool {
        self.mods.enabled(id) && self.gate.as_ref().is_none_or(|g| g.contains(id))
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
    mut hud: ResMut<ChallengeHudState>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
) {
    if challenges.generation == Some(map.generation) {
        return;
    }
    if challenges.generation.is_none() && config.verification_capture.is_some() {
        challenges.verify = std::env::var("SKATE_VERIFY_CHALLENGE").ok().map(|id| (id, 0));
        challenges.verify_tutorial = std::env::var("SKATE_VERIFY_TUTORIAL").ok();
    }
    challenges.generation = Some(map.generation);
    if !challenges.loaded {
        challenges.loaded = true;
        let root = config.asset_root.parent().unwrap_or(&config.asset_root);
        challenges.records = crate::challenge_records::Records::load(root.join("settings/challenges.json"));
        challenges.retail = load(&config.asset_root).unwrap_or_else(|e| {
            info!("SKATE_CHALLENGES none ({e})");
            Vec::new()
        });
        challenges.tutorials = crate::tutorial_run::load(&config.asset_root);
        info!("SKATE_CHALLENGES tutorials={}", challenges.tutorials.len());
    }
    if let Some(t) = challenges.tutorial.take() {
        info!("SKATE_TUTORIAL {} dropped by the map change", t.data.id);
    }
    if let Some(run) = challenges.run.take() {
        info!("SKATE_CHALLENGE {} dropped by the map change", run.challenge.id);
        *hud = ChallengeHudState::default();
    }
    let count = challenges.on_map(map.path.as_deref()).count();
    info!("SKATE_CHALLENGES map={} own_the_spot={count}", map.name);
    // Discovery (8255BD38, research b850): entering a challenge's boundary volume marks it discovered, so every
    // challenge boundary on the map is armed, not only a running one's.
    let boundaries: Vec<TriggerVolumeRecord> = challenges.on_map(map.path.as_deref()).map(|c| c.boundary.volume.clone()).collect();
    for b in &boundaries {
        if volumes.get(&b.id).is_none() {
            volumes.map.push(crate::trigger_volumes::TriggerVolume::from_record(b));
        }
    }
    if let Some((id, _)) = challenges.verify.clone() {
        if let Some(c) = challenges.on_map(map.path.as_deref()).find(|c| c.id == id) {
            info!("SKATE_VERIFY_CHALLENGE {id} start at its sign-up {:?}", &c.location.matrix[3][..3]);
            if let Err(e) = skater.travel_to(c.location.matrix) {
                warn!("SKATE_VERIFY_CHALLENGE {e}");
            }
        }
    }
    let Ok(wanted) = std::env::var("SKATE_CHALLENGE") else { return };
    let Some(c) = challenges.on_map(map.path.as_deref()).find(|c| c.id == wanted).cloned() else {
        info!("SKATE_CHALLENGE {wanted} is not on {}", map.name);
        return;
    };
    start_run(&mut challenges, &mut volumes, c);
}

fn start_run(challenges: &mut Challenges, volumes: &mut crate::trigger_volumes::TriggerVolumes, c: Challenge) {
    let c = challenges.mods.apply(c);
    match params(&c) {
        Ok(p) => {
            arm_volumes(volumes, &c);
            let saved = challenges.records.get(&c.id);
            let (best, intro_seen) = (saved.high_score, saved.discovery == crate::challenge_records::SEEN_INTRO);
            info!("SKATE_CHALLENGE start {} owned={} killed={} best={best} record={:?} intro_seen={intro_seen}", c.id,
                c.owned_points, c.killed_points, saved.completion);
            challenges.mods.event(serde_json::json!({"name": "challenge_started", "challenge": c.id}));
            let mut ots = Ots::new(p, best, intro_seen);
            challenges.pending.extend(ots.start());
            // Own the spot runs its owned it rows (the killed it set has the same shape; which group retail swaps
            // in for the kill target is not read [inferred]).
            let volumes = [&c.boundary, &c.scoring, &c.spot].iter()
                .filter_map(|v| Some((v.name.clone(), u64::from_str_radix(&v.id, 16).ok()?))).collect();
            let data = c.objectives.as_ref()
                .and_then(|o| crate::challenge_objectives::DataObjectives::new(&c.id, &o.owned, volumes, Default::default()));
            let objectives = Objectives { data, ..Default::default() };
            challenges.run = Some(Run { ots, challenge: c, objectives, tick_carry: 0., in_discovery: None });
        }
        Err(e) => warn!("SKATE_CHALLENGE {e}"),
    }
}

/// The sign-up prompt (research b849 / b850): the SignUpManager's "signup" element (HUD
/// `cChallengeSignUpManager-DPadUp`) lists the challenges whose sign-up sphere holds the player; with no run going the
/// action overlay shows the challenge's sign-up state with its title; D-pad Up signs up at once (no confirm popup,
/// no sound on that path).
/// A teleporter prompt on the same spot keeps the overlay and the button.
#[allow(clippy::too_many_arguments)]
fn sign_up(
    mut commands: Commands,
    mut challenges: ResMut<Challenges>,
    map: Res<crate::map_transition::CurrentMap>,
    skater: Res<crate::physics::SkaterRuntime>,
    input: Res<crate::input::ControllerInput>,
    mut popups: ResMut<crate::popup_host::PopupMods>,
    mut signed: MessageWriter<ChallengeSignUp>,
    mut discovered: MessageWriter<crate::career::ChallengeDiscovered>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
    lines: Query<Entity, With<SignUpLine>>,
    mut entered: MessageReader<crate::trigger_volumes::TriggerEntered>,
    time: Res<Time>,
) {
    let gameplay = crate::graphics_menu::gameplay_active(menu) && !replay.active;
    let feet = crate::trigger_volumes::player_points(&skater)[0];
    let boundaries: Vec<String> = entered.read().filter(|e| e.body == crate::trigger_volumes::PLAYER).map(|e| e.volume.clone()).collect();
    if !boundaries.is_empty() {
        let found: Vec<String> = challenges
            .on_map(map.path.as_deref())
            .filter(|c| boundaries.contains(&c.boundary.id) && challenges.mods.enabled(&c.id))
            .filter(|c| challenges.records.get(&c.id).discovery == crate::challenge_records::NOT_DISCOVERED)
            .map(|c| c.id.clone())
            .collect();
        for id in &found {
            info!("SKATE_CHALLENGE discovered {id}");
            challenges.records.update(id, |s| s.discovery = crate::challenge_records::DISCOVERED);
            challenges.mods.event(serde_json::json!({"name": "challenge_discovered", "challenge": id}));
            discovered.write(crate::career::ChallengeDiscovered { challenge: id.clone() });
        }
        if !found.is_empty() {
            challenges.records.save_in_background();
        }
    }
    // The pick popup's answer: the challenge offered at that entry (kept by id, so a change of what is in range
    // while it is open cannot start another one); B or a cleared popup starts nothing.
    let choices: Vec<i32> = popups.engine_choices.iter().filter(|(c, _)| c == PICK_POPUP).map(|&(_, i)| i).collect();
    popups.engine_choices.retain(|(c, _)| c != PICK_POPUP);
    if let (Some(&choice), Some(offered)) = (choices.first(), challenges.picking.take()) {
        match usize::try_from(choice).ok().and_then(|i| offered.get(i)) {
            Some(id) => {
                info!("SKATE_CHALLENGE sign-up {id} (picked)");
                signed.write(ChallengeSignUp { body: crate::trigger_volumes::PLAYER.into(), challenge: id.clone() });
            }
            None => info!("SKATE_CHALLENGE pick cancelled"),
        }
    }
    if !popups.stage_ready {
        challenges.picking = None;
    }
    let near: Vec<Challenge> = if gameplay && challenges.run.is_none() && popups.signup_title.is_none() && challenges.picking.is_none() {
        in_sign_up(challenges.on_map(map.path.as_deref()).filter(|c| challenges.available(&c.id)), feet).into_iter().cloned().collect()
    } else {
        Vec::new()
    };
    let ids: Vec<String> = near.iter().map(|c| c.id.clone()).collect();
    if ids != challenges.signing_up {
        info!("SKATE_CHALLENGE sign-up prompt {ids:?}");
        if challenges.signing_up.is_empty() {
            challenges.prompt_age = 0.;
        }
        challenges.signing_up = ids.clone();
    } else {
        challenges.prompt_age += time.delta_secs();
    }
    let (modifier, _, _) = input.session_marker_actions();
    let mut pressed = !near.is_empty() && !modifier && input.dpad_up_pressed() && challenges.last_batch != input.consumed_batches
        && challenges.prompt_age >= REPRESS_DELAY_SECONDS;
    challenges.last_batch = input.consumed_batches;
    if let Some((id, frames)) = challenges.verify.as_mut().filter(|(id, _)| near.iter().any(|c| c.id == *id)) {
        if *frames != u32::MAX {
            *frames += 1;
            if *frames == 30 {
                *frames = u32::MAX;
                info!("SKATE_VERIFY_CHALLENGE {id} pressed");
                pressed = true;
            }
        }
    }
    if pressed {
        match near.as_slice() {
            [c] => {
                info!("SKATE_CHALLENGE sign-up {}", c.id);
                signed.write(ChallengeSignUp { body: crate::trigger_volumes::PLAYER.into(), challenge: c.id.clone() });
            }
            // Without the menu stage there is no popup to pick from: the first entry starts, as a pick would.
            [c, ..] if !popups.stage_ready => {
                info!("SKATE_CHALLENGE sign-up {} (first of {}, no popup stage)", c.id, near.len());
                signed.write(ChallengeSignUp { body: crate::trigger_volumes::PLAYER.into(), challenge: c.id.clone() });
            }
            list => {
                info!("SKATE_CHALLENGE pick popup {ids:?}");
                challenges.picking = Some(list.iter().take(PICK_MAX).map(|c| c.id.clone()).collect());
                popups.engine_requests.push(pick_popup(list));
            }
        }
    }
    // The overlay (82665DA0): one challenge shows its own state and title, several the Challenge state 1 with
    // "Multiple Challenges".
    let shown = near.first().filter(|_| !pressed);
    if let Some(c) = shown {
        popups.signup_state = if near.len() == 1 { sign_up_state(c) } else { 1 };
        popups.signup_title = Some(if near.len() == 1 {
            c.title.clone().or_else(|| c.title_id.clone()).unwrap_or_else(|| c.id.clone())
        } else {
            MULTIPLE_CHALLENGES.into()
        });
    }
    let line = lines.iter().next();
    match (shown.as_ref().filter(|_| !popups.signup_ready), line) {
        (Some(c), None) => {
            let title = if near.len() > 1 { "Multiple Challenges".to_owned() } else { c.title.clone().unwrap_or_else(|| c.id.clone()) };
            commands.spawn((SignUpLine, GlobalZIndex(5), Node { position_type: PositionType::Absolute, bottom: percent(18),
                width: percent(100), justify_content: JustifyContent::Center, ..default() }))
                .with_child((Text::new(format!("D-pad Up: sign up, {title}")), TextFont { font_size: 24., ..default() },
                    TextColor(Color::WHITE)));
        }
        (None, Some(e)) => commands.entity(e).despawn(),
        _ => {}
    }
}

#[derive(Component)]
struct SignUpLine;

/// One frame of a run: events in, timers, commands out.
#[allow(clippy::too_many_arguments)]
fn update(
    mut challenges: ResMut<Challenges>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
    mut volumes: ResMut<crate::trigger_volumes::TriggerVolumes>,
    mut signed: MessageReader<ChallengeSignUp>,
    map: Res<crate::map_transition::CurrentMap>,
    mut hud: ResMut<ChallengeHudState>,
    mut central: MessageWriter<crate::hud_central::CentralMessage>,
    mut entered: MessageReader<crate::trigger_volumes::TriggerEntered>,
    mut exited: MessageReader<crate::trigger_volumes::TriggerExited>,
    time: Res<Time>,
    mut career: MessageWriter<crate::career::ChallengeResult>,
    mut markers: MessageReader<crate::ui_audio::SessionMarkerEvent>,
    config: Res<crate::config::Config>,
    pad: Option<Res<crate::input::ControllerInput>>,
    physics: Option<Res<crate::physics::GamePhysics>>,
) {
    use crate::ui_audio::SessionMarkerAction;
    let marker_flags: Vec<bool> = markers.read().filter_map(|m| match m.action {
        SessionMarkerAction::Returned => Some(true),
        SessionMarkerAction::Placed => Some(false),
        _ => None,
    }).collect();
    let mut sign_ups: Vec<String> = signed.read().filter(|s| s.body == crate::trigger_volumes::PLAYER).map(|s| s.challenge.clone()).collect();
    sign_ups.extend(challenges.verify_tutorial.take());
    for id in sign_ups {
        let found = challenges.on_map(map.path.as_deref()).find(|c| c.id == id).cloned();
        let tutorial = challenges.tutorials.iter()
            .find(|t| t.id == id && map.path.as_deref().is_some_and(|p| crate::teleport_menu::same_map(p, &t.map))).cloned();
        let idle = challenges.run.is_none() && challenges.tutorial.is_none();
        match (found, tutorial) {
            (Some(c), _) if idle => start_run(&mut challenges, &mut volumes, c),
            (None, Some(t)) if idle => {
                info!("SKATE_TUTORIAL start {}", t.id);
                let groups = challenges.trick_groups.get_or_insert_with(|| {
                    let groups = skate_data::collections::Collections::load(&config.asset_root)
                        .map(|c| skate_data::objective_program::trick_groups(&c)).unwrap_or_default();
                    info!("SKATE_TUTORIAL trick groups={}", groups.len());
                    std::sync::Arc::new(groups)
                }).clone();
                challenges.tutorial = Some(crate::tutorial_run::TutorialRun::new(t, groups));
            }
            _ => info!("SKATE_CHALLENGE sign-up {id} ignored"),
        }
    }
    let banked = skater.scoring.take_banked();
    let announced = skater.scoring.take_announced();
    let entered: Vec<String> = entered.read().filter(|e| e.body == crate::trigger_volumes::PLAYER).map(|e| e.volume.clone()).collect();
    let exited: Vec<String> = exited.read().filter(|e| e.body == crate::trigger_volumes::PLAYER).map(|e| e.volume.clone()).collect();
    if let Some(mut tutorial) = challenges.tutorial.take() {
        let ids = |list: &[String]| list.iter().filter_map(|i| u64::from_str_radix(i, 16).ok()).collect::<Vec<_>>();
        let map_volumes: Vec<(String, u64)> =
            volumes.map.iter().filter_map(|v| Some((v.name.clone(), u64::from_str_radix(&v.id, 16).ok()?))).collect();
        challenges.tutorial_carry += time.delta_secs() * skate_core::challenges::timers::TICKS_PER_SECOND;
        let ticks = challenges.tutorial_carry as u32;
        challenges.tutorial_carry -= ticks as f32;
        let (entered_ids, exited_ids) = (ids(&entered), ids(&exited));
        let lines: Vec<(i32, bool)> = banked.iter().map(|s| (s.score, s.wiped_out)).collect();
        let feet = crate::trigger_volumes::player_points(&skater)[0];
        let raw = pad.as_ref().map(|p| p.raw_input()).unwrap_or_default();
        let input = crate::tutorial_run::Inputs { volumes: &map_volumes, entered: &entered_ids, exited: &exited_ids,
            banked: &lines, ticks, teleport_pending: skater.teleport_pending(),
            distance: distance(feet, tutorial.data.start.matrix[3]), state_id: skater.physical_state_id(),
            in_sequence: skater.scoring.sequence_active(), markers: &marker_flags, tricks: &announced,
            right_stick_y: raw.right[1], triggers: raw.triggers, manual: skater.scoring.manual(),
            pushes: skater.animation_input.take_pushes(), pump_acceleration: skater.ground.pumping.pump_acceleration,
            pump_user: skater.ground.pumping.intentional_pumping != 0,
            grind_rail: (400..=405).contains(&skater.physical_state_id())
                .then_some(skater.player_input.grind.rail_owner).flatten()
                .and_then(|owner| physics.as_ref().and_then(|p| p.rail_guids(owner))) };
        let actions = if std::mem::take(&mut challenges.mods.abort) { tutorial.abandon() } else { tutorial.step(&input) };
        let mut finished = false;
        for action in actions {
            match action {
                crate::tutorial_run::Action::Teleport { matrix, offboard } => {
                    if let Err(e) = skater.travel_ex(matrix, None, !offboard) {
                        warn!("SKATE_TUTORIAL teleport {e}");
                    }
                }
                crate::tutorial_run::Action::Finished(completion) => {
                    career.write(crate::career::ChallengeResult { challenge: tutorial.data.id.clone(), completion, reactivate: false });
                    finished = true;
                }
            }
        }
        if !finished {
            challenges.tutorial = Some(tutorial);
        }
    }
    let mut commands = std::mem::take(&mut challenges.pending);
    let abort = std::mem::take(&mut challenges.mods.abort);
    let mut mod_events = Vec::new();
    let Some(run) = challenges.run.as_mut() else { return };
    if abort {
        info!("SKATE_CHALLENGE {} abandoned by a mod", run.challenge.id);
        commands.extend(run.ots.handle(Event::Abandon));
    }
    let volume_event = |id: &str, enter: bool| {
        u64::from_str_radix(id, 16).ok().map(|volume| if enter { Event::TriggerEnter { volume } } else { Event::TriggerExit { volume } })
    };
    let ours = [&run.challenge.boundary, &run.challenge.scoring, &run.challenge.spot];
    for (id, verb) in exited.iter().map(|i| (i, "exit")).chain(entered.iter().map(|i| (i, "enter"))) {
        if let Some(v) = ours.iter().find(|v| v.id == *id) {
            info!("SKATE_CHALLENGE {} {verb} {}", run.challenge.id, v.name);
        }
    }
    for id in &exited {
        if let (Some(data), Ok(v)) = (run.objectives.data.as_mut(), u64::from_str_radix(id, 16)) {
            data.exit(v);
        }
        if let Some(e) = volume_event(id, false) {
            commands.extend(run.ots.handle(e));
        }
    }
    for id in &entered {
        if *id == run.challenge.spot.id {
            run.objectives.spot_entered = true;
        }
        if let (Some(data), Ok(v)) = (run.objectives.data.as_mut(), u64::from_str_radix(id, 16)) {
            data.enter(v);
        }
        if let Some(e) = volume_event(id, true) {
            commands.extend(run.ots.handle(e));
        }
    }
    let feet = crate::trigger_volumes::player_points(&skater)[0];
    let distance = distance(feet, run.challenge.location.matrix[3]);
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
    let (mut intro_seen, mut high_score) = (false, None);
    let mut i = 0;
    while i < commands.len() {
        let command = commands[i].clone();
        i += 1;
        match command {
            Command::Teleport => {
                info!("SKATE_CHALLENGE {} teleport to {} {:?} from {feet:?}", run.challenge.id, run.challenge.start.locator,
                    &run.challenge.start.matrix[3][..3]);
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
            Command::MarkIntroSeen => intro_seen = true,
            Command::SetHighScore(points) => high_score = Some(points),
            Command::ShowHud(c, shown) => {
                if let Some(c) = hud_component(c) {
                    hud.show(c, shown);
                }
            }
            Command::StartTimerHud { .. } => hud.show(Component::Timer, true),
            Command::UpdateHud { score, target, personal_best, text } => {
                info!("SKATE_CHALLENGE {} hud score={score} target={target} best={personal_best} {text:?}", run.challenge.id);
                update_hud(&mut hud, score, target, personal_best, text);
            }
            Command::NewPersonalBestMessage => {
                mod_events.push(serde_json::json!({"name": "challenge_personal_best", "challenge": run.challenge.id,
                    "points": run.ots.personal_best()}));
                central.write(crate::hud_central::CentralMessage { style: GREEN_STYLE, ..crate::hud_central::CentralMessage::new(NEW_PERSONAL_BEST) });
            }
            // Retail ShowModifiedCentralMessage (82850770, research b852): the second central slot, off-white, 6 s,
            // not cleared when the player is back in the area.
            Command::ReturnToAreaMessage => {
                let text = run.challenge.return_to_area_message.clone().unwrap_or_else(|| RETURN_TO_AREA.into());
                central.write(crate::hud_central::CentralMessage::modified(text));
            }
            other => info!("SKATE_CHALLENGE {} {other:?}", run.challenge.id),
        }
    }
    if hud.is_shown(Component::Timer) {
        let (text, flag) = skate_core::challenges::timers::hud_text(run.ots.out_of_bounds_ticks().unwrap_or(0));
        if hud.timer_text != text || hud.timer_flag != flag {
            hud.timer_text = text;
            hud.timer_flag = flag;
        }
    }
    let (id, best) = (run.challenge.id.clone(), run.ots.personal_best());
    // Retail writes the high score as the graph asks (SetHighScore 827D1108, the graph compares first) and the tier
    // through CompleteChallenge (82C1D878, moves up only); SeenIntro through SetDiscoveryStatus (research b846).
    let records = &mut challenges.records;
    let mut changed = intro_seen && records.update(&id, |s| s.discovery = crate::challenge_records::SEEN_INTRO);
    if let Some(points) = high_score {
        changed |= records.update(&id, |s| s.high_score = points);
    }
    if let Some(completion) = finished {
        changed |= records.update(&id, |s| {
            let mut r = s.record();
            r.complete(completion);
            s.set_record(r);
        });
        let record = records.get(&id).record();
        info!("SKATE_CHALLENGE end {id} completion={completion:?} best={best} record={:?} high_score={}",
            record.completion.unwrap_or(Completion::Abandoned), record.high_score);
        mod_events.push(serde_json::json!({"name": "challenge_ended", "challenge": id, "points": best,
            "completion": completion_name(completion)}));
        career.write(crate::career::ChallengeResult { challenge: id.clone(), completion, reactivate: true });
        *hud = ChallengeHudState::default();
        challenges.run = None;
    }
    for e in mod_events {
        challenges.mods.event(e);
    }
    if changed {
        challenges.records.save_in_background();
    }
}

/// The completion as mods see it (`challenge_ended`, the `sdk.challenges` records).
pub(crate) fn completion_name(c: Completion) -> &'static str {
    match c {
        Completion::Abandoned => "abandoned",
        Completion::Owned => "owned",
        Completion::Killed => "killed",
    }
}

/// Ordering for the career (career.rs reads the results `update` writes).
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CareerResultSet;

/// The Challenges opt-in (living_world/opt_ins.rs): off by default while the challenges are being built.
pub(crate) fn opted_in(living: Option<Res<crate::living_world::LivingWorldSettings>>) -> bool {
    living.is_some_and(|l| l.user_opt_ins.challenges)
}

/// The opt-in switched off: the run, its HUD and the prompt go; switching back on arms the map again.
fn switch_off(
    mut commands: Commands,
    mut challenges: ResMut<Challenges>,
    mut hud: ResMut<ChallengeHudState>,
    lines: Query<Entity, With<SignUpLine>>,
) {
    if challenges.generation.is_none() {
        return;
    }
    if let Some(run) = challenges.run.take() {
        info!("SKATE_CHALLENGE {} dropped: challenges switched off", run.challenge.id);
    }
    *hud = ChallengeHudState::default();
    challenges.signing_up.clear();
    challenges.picking = None;
    challenges.generation = None;
    challenges.pending.clear();
    for e in &lines {
        commands.entity(e).despawn();
    }
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Challenges>()
        .init_resource::<ChallengeHudState>()
        .add_message::<ChallengeSignUp>()
        .add_systems(PreUpdate, arm.after(crate::map_transition::MapTransitionSet).run_if(opted_in))
        .add_systems(Update, switch_off.run_if(not(opted_in)))
        .add_systems(Update, (sign_up.after(crate::teleporters::prompt).before(crate::retail_menu_stage::MenuStageSet), update.after(crate::trigger_volumes::TriggerSet).in_set(CareerResultSet)).chain().run_if(opted_in));
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

    fn at(id: &str, x: f32, radius: f32) -> Challenge {
        let v = Volume { name: "v".into(), id: "1".into(), volume: serde_json::from_value(serde_json::json!({"id": "1", "name": "v",
            "shape": {"kind": "box", "center": [0., 0., 0.], "axes": [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                      "half_extents": [1., 1., 1.], "fatness": 0.}})).unwrap() };
        let m = |x: f32| Start { locator: "l".into(), matrix: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [x, 0., 0., 1.]] };
        Challenge { id: id.into(), family: "own_the_spots".into(), map: "DownTown".into(), title_id: None, title: None,
            boundary: v.clone(), scoring: v.clone(), spot: v, start: m(0.), location: m(x), sign_up_radius: radius,
            discovery_radius: 100., owned_points: 1000, killed_points: 1500,
            challenge_type: Some(9), out_of_bounds_time: None, owned_display_time: None, killed_display_time: None,
            return_to_area_message: None, objectives: None }
    }

    #[test]
    fn own_the_spot_signs_up_in_the_ots_overlay_state() {
        let mut c = at("a", 0., 10.);
        assert_eq!(sign_up_state(&c), 5);
        c.challenge_type = Some(3);
        assert_eq!(sign_up_state(&c), 1);
        c.challenge_type = None;
        assert_eq!(sign_up_state(&c), 5, "family fallback");
    }

    #[test]
    fn every_sign_up_sphere_holding_the_skater_lists_in_retail_order() {
        let mut spot = at("s", 6., 10.);
        spot.challenge_type = Some(CREATE_A_SPOT_TYPE);
        let list = [at("a", 0., 10.), spot, at("b", 12., 10.), at("c", 40., 10.)];
        let ids = |p: [f32; 3]| in_sign_up(list.iter(), p).into_iter().map(|c| c.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids([9., 0., 0.]), ["b", "a", "s"], "others reversed, create-a-spot last (b851)");
        assert_eq!(ids([20., 0., 0.]), ["b"]);
        assert!(ids([25., 0., 0.]).is_empty());
    }

    #[test]
    fn several_in_range_offer_up_to_four_by_title() {
        let mut list: Vec<Challenge> = (0..6).map(|i| at(&format!("c{i}"), 0., 10.)).collect();
        list[0].title_id = Some("ID_C0_TITLE".into());
        list[1].challenge_type = Some(CREATE_A_SPOT_TYPE);
        let p = pick_popup(&list);
        assert_eq!((p.title.as_str(), p.description.as_str(), p.cancelable, p.callback.as_str()),
            (MULTIPLE_CHALLENGES, "#", true, PICK_POPUP));
        assert_eq!(p.options, ["ID_C0_TITLE", CREATE_A_SPOT_LABEL, "c2", "c3"]);
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
        let p = params(&loaded[0]).unwrap();
        assert_eq!(p.boundary, 0x0008_B0CD_03E3_8706);
        assert_eq!((p.out_of_bounds_time, p.owned_display_time, p.killed_display_time), (5.0, 3.1, 3.3), "retail defaults");
    }
}
