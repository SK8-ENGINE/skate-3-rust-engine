//! Follow the leader in play (research b1006; doc 42 "Follow the leader"): a row from `private/challenges.json`
//! `follow_the_leaders` runs through `skate_core::challenges::film_run` (Kind::Follow), its owned objectives from the
//! exported rows (`challenge_objectives.rs`), the leader (competitor 1) riding the challenge's lines (`ch_<id>`) on the
//! contest rider code with the follower band. Start: the sign-up prompt at the row's sign-up sphere (challenges.rs),
//! the career's first challenge, or `SKATE_FTL=<id>` on that row's map.
//! The killed it group runs next to the owned one (make_objectives builds both lists, b875); no retail FTL row has one.
use crate::challenge_objectives::{DataObjectives, ObjectiveData};
use crate::hud_challenge::{ChallengeHudState, Component};
use bevy::prelude::*;
use serde::Deserialize;
use skate_core::challenges::film_run::{Command, Event, FilmRun, HudComponent, Kind, Message, Params, State};
use skate_core::challenges::objectives::Event as GroupEvent;
use skate_core::challenges::race_ai::{follower_band, shaped_speed, BandTuning, FollowerBand};
use skate_core::challenges::timers::TICKS_PER_SECOND;
use skate_core::challenges::{Completion, Flow};

/// The failure text when the row's own is not exported (b1006 §0.3).
const FAILURE_TEXT: &str = "ID_CHALLENGE_FTL_FAILURE";
const AUTOPILOT_DIALOG_SECONDS: f32 = 3.0;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct FtlStart {
    pub locator: String,
    pub matrix: [[f32; 4]; 4],
    /// Child n of the start locator: the player 0, the leader 1, a second leader 2.
    #[serde(default)]
    pub slots: Vec<[[f32; 4]; 4]>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct FtlObjectives {
    #[serde(default)]
    pub owned: Vec<ObjectiveData>,
    #[serde(default)]
    pub killed: Vec<ObjectiveData>,
}

fn yes() -> bool {
    true
}


/// The autopilot's gap behind the leader (m): inside it the brain holds the leader's speed or less. OURS, a test aid.
const FOLLOW_GAP: f32 = 8.0;

/// The line the autopilot may take next: the one after `current` on the leader's `path`; the leader's latest when
/// the brain is on it or off the path; the first one before the brain has a line.
fn follow_line(path: &[[u8; 16]], current: Option<[u8; 16]>) -> Option<[u8; 16]> {
    match current.and_then(|c| path.iter().rposition(|l| *l == c)) {
        Some(i) => path.get(i + 1).or(path.last()).copied(),
        None => current.map_or(path.first(), |_| path.last()).copied(),
    }
}

/// The autopilot never brakes below this for the gap (m/s): a leader standing at its start does not stop it.
const FOLLOW_MIN_SPEED: f32 = 4.0;

/// The autopilot's speed cap for a gap of `behind` meters behind the leader (negative: ahead of it) at the leader's
/// `speed`: none past the gap, the leader's speed easing down to 75 % at no gap, 60 % once ahead.
fn follow_cap(behind: f32, speed: f32) -> Option<f32> {
    let cap = if behind >= FOLLOW_GAP {
        return None;
    } else if behind <= 0.0 {
        0.6 * speed
    } else {
        speed * (0.75 + 0.25 * behind / FOLLOW_GAP)
    };
    Some(cap.max(FOLLOW_MIN_SPEED))
}

/// One row of the objectives_shared graph as `challenges.py` exports it: `follow_the_leaders` and `films` (spot films
/// and anywhere films share the export, b999 / b1003).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct FtlData {
    pub id: String,
    /// `follow_the_leaders`, `films` or `anywhere_films` (absent in an export from before films: follow the leader).
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub title_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(flatten)]
    pub sign_up: crate::challenges::SignUpFields,
    pub start: FtlStart,
    #[serde(default)]
    pub end_volume: Option<crate::challenges::Volume>,
    #[serde(default)]
    pub boundary: Option<crate::challenges::Volume>,
    #[serde(default)]
    pub max_follow_distance: Option<f32>,
    #[serde(default)]
    pub countdown_delay: Option<f32>,
    /// Film row values (b999 §3): PrefilmTime, TotalFilmTime (simul play only), FilmTimerLowLimit, OutOfBoundsTime and
    /// the banner times; absent = the family default in `Params::retail`.
    #[serde(default)]
    pub prefilm_time: Option<f32>,
    #[serde(default)]
    pub total_film_time: Option<f32>,
    #[serde(default)]
    pub film_low_limit: Option<i32>,
    #[serde(default)]
    pub out_of_bounds_time: Option<f32>,
    #[serde(default)]
    pub owned_display_time: Option<f32>,
    #[serde(default)]
    pub killed_display_time: Option<f32>,
    #[serde(default = "yes")]
    pub allow_player_bail: bool,
    #[serde(default)]
    pub allow_objective_failure: bool,
    #[serde(default)]
    pub spawn_offboard: bool,
    #[serde(default)]
    pub ai_paths: crate::contest_host::RoundPathsJson,
    #[serde(default)]
    pub competitors: Vec<crate::race_host::CompetitorRow>,
    #[serde(default)]
    pub objectives: FtlObjectives,
    /// The volumes the objectives test (IsInVolume / EnteredVolume), from the challenge's hull: a film's spot volumes
    /// are not among the map's trigger volumes, so the run arms them.
    #[serde(default)]
    pub objective_volumes: Vec<crate::challenges::Volume>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    follow_the_leaders: Vec<serde_json::Value>,
    #[serde(default)]
    films: Vec<serde_json::Value>,
}

/// Every follow the leader and film row. Rows are read one by one, so a row this host cannot run yet (an anywhere
/// film: no start, its objectives not exported, b961 §1) is left out with a log line instead of hiding the others.
pub(crate) fn load(assets: &std::path::Path) -> Vec<FtlData> {
    let Some(file) = std::fs::read(assets.join("private/challenges.json")).ok().and_then(|b| serde_json::from_slice::<File>(&b).ok()) else {
        return Vec::new();
    };
    let mut skipped = Vec::new();
    let rows: Vec<FtlData> = file.follow_the_leaders.into_iter().chain(file.films).filter_map(|v| {
        let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("?").to_owned();
        serde_json::from_value(v).map_err(|_| skipped.push(id)).ok()
    }).collect();
    if !skipped.is_empty() {
        info!("SKATE_FTL rows not runnable here yet (no start; anywhere films NOT RETAIL YET): {skipped:?}");
    }
    rows
}

fn volume_id(v: &Option<crate::challenges::Volume>) -> Option<u64> {
    v.as_ref().and_then(|v| u64::from_str_radix(&v.id, 16).ok())
}

/// The graph's challenge type for a row's family.
pub(crate) fn kind(d: &FtlData) -> Kind {
    match d.family.as_deref() {
        Some("films") => Kind::Film,
        Some("anywhere_films") => Kind::AnywhereFilm,
        _ => Kind::Follow,
    }
}

/// The row's values over the family defaults (b1006 §3, b999 §3).
pub(crate) fn params(d: &FtlData) -> Params {
    let kind = kind(d);
    let base = Params::retail(kind);
    Params {
        // follow the leader always has its leader; films ride alone
        competitors: if kind == Kind::Follow { d.competitors.len().max(1) } else { d.competitors.len() } as u32,
        boundary: volume_id(&d.boundary),
        ftl_end_volume: volume_id(&d.end_volume),
        countdown_delay: d.countdown_delay.unwrap_or(base.countdown_delay),
        prefilm_time: d.prefilm_time.unwrap_or(base.prefilm_time),
        total_film_time: d.total_film_time.unwrap_or(base.total_film_time),
        film_low_limit: d.film_low_limit.map_or(base.film_low_limit, |v| v as f32),
        out_of_bounds_time: d.out_of_bounds_time.unwrap_or(base.out_of_bounds_time),
        owned_display_time: d.owned_display_time.unwrap_or(base.owned_display_time),
        killed_display_time: d.killed_display_time.unwrap_or(base.killed_display_time),
        allow_player_bail: d.allow_player_bail,
        allow_objective_failure: d.allow_objective_failure,
        ..base
    }
}

/// One objective group of the run: the owned it list or the killed it list.
pub(crate) struct FtlGroup {
    killed: bool,
    objectives: DataObjectives,
}

/// The run's groups from the row: the owned list (required), then the killed list when the row has one. None when
/// the owned list does not load; a killed list that does not load is left out (logged by DataObjectives).
fn load_groups(data: &FtlData, volumes: Vec<(String, u64)>) -> Option<Vec<FtlGroup>> {
    let owned = DataObjectives::new(&data.id, &data.objectives.owned, volumes.clone(), Default::default())?;
    let mut groups = vec![FtlGroup { killed: false, objectives: owned }];
    if let Some(killed) = DataObjectives::new(&data.id, &data.objectives.killed, volumes, Default::default()) {
        groups.push(FtlGroup { killed: true, objectives: killed });
    }
    Some(groups)
}

pub(crate) struct FtlState {
    pub data: FtlData,
    run: FilmRun,
    groups: Vec<FtlGroup>,
    evaluating: bool,
    leader: Option<crate::contest_ai_host::ContestAi>,
    awaiting_teleport: bool,
    /// The leader band (None = switched off).
    band: Option<FollowerBand>,
    carry: f32,
    /// Fractional 60 Hz ticks toward the next objective evaluation.
    eval_carry: f32,
    /// The first leader's position last frame and its smoothed speed (m/s), for the autopilot's follow gap.
    leader_last: Option<[f32; 3]>,
    leader_speed: f32,
    /// The lines the first leader has taken this run, in order: the autopilot takes them in the same order.
    leader_path: Vec<[u8; 16]>,
    last_state: u32,
    failure_text: Option<String>,
    dialog_open: bool,
    dialog_seconds: f32,
    /// Leaders already counted inside the end volume.
    leaders_in_end: Vec<u32>,
    /// The competitors with a follow indicator (Leader.AddFollowIndicator), drawn as a trail behind their body.
    follow_indicators: Vec<u32>,
    meter: Option<skate_core::challenges::distance_meter::DistanceMeter>,
    /// Commands for the next update (the start's, made in arm).
    queued: Vec<Command>,
    /// What the player does, for the objective groups (shared with the tutorials).
    feed: crate::challenge_objectives::ObjectiveFeed,
    /// The autopilot's area lines were looked up for this run (films).
    area_checked: bool,
}

#[derive(Resource, Default)]
pub(crate) struct FollowTheLeader {
    pub rows: Vec<FtlData>,
    loaded: bool,
    generation: Option<u64>,
    pub run: Option<FtlState>,
    /// The last finished run's result line (tests, the autopilot).
    pub result: Option<String>,
}

impl FollowTheLeader {
    /// The rows on `map` with a sign-up sphere, for the sign-up prompt (challenges.rs).
    pub(crate) fn offers<'a>(&'a self, map: Option<&'a std::path::Path>) -> impl Iterator<Item = crate::challenges::Offer> + 'a {
        self.rows.iter().filter_map(move |r| r.sign_up.offer(&r.id, r.family.as_deref().unwrap_or("follow_the_leaders"), r.map.as_deref(),
            r.title_id.as_ref(), r.title.as_ref(), map))
    }

    /// The player is held from the start until the countdown releases it (PinPlayer / UnpinAI(player)).
    pub(crate) fn player_pinned(&self) -> bool {
        self.run.as_ref().is_some_and(|r| matches!(r.run.state(), State::PinPlayer | State::MakeObjectives | State::PrefilmDisplay
            | State::NisOverview | State::Setup | State::WaitForAssetLoad | State::SpawnPlayer | State::CheckAi | State::Wait | State::Countdown))
    }
}

/// A new map drops the old map's run and starts `SKATE_FTL`; a sign-up (the prompt at the row's sign-up sphere, the
/// career's first challenge) starts its row when nothing else runs.
fn arm(mut ftl: ResMut<FollowTheLeader>, config: Res<crate::config::Config>, map: Res<crate::map_transition::CurrentMap>,
    mut volumes: ResMut<crate::trigger_volumes::TriggerVolumes>, mut population: Option<ResMut<crate::living_world::PopulationState>>,
    mut signed: MessageReader<crate::challenges::ChallengeSignUp>, challenges: Option<Res<crate::challenges::Challenges>>,
    mut skater: ResMut<crate::physics::SkaterRuntime>) {
    let mut wanted = Vec::new();
    if ftl.generation != Some(map.generation) {
        ftl.generation = Some(map.generation);
        if !ftl.loaded {
            ftl.loaded = true;
            ftl.rows = load(&config.asset_root);
            info!("SKATE_FTL rows={}", ftl.rows.len());
        }
        if let Some(mut run) = ftl.run.take() {
            info!("SKATE_FTL {} dropped by the map change", run.data.id);
            if let (Some(ai), Some(pop)) = (run.leader.as_mut(), population.as_deref_mut()) {
                ai.remove(pop);
            }
        }
        wanted.extend(std::env::var("SKATE_FTL").ok());
        wanted.extend(std::env::var("SKATE_FILM").ok());
        // `--verify` runs: SKATE_VERIFY_CHALLENGE places the skater at the row's sign-up, as for own the spot.
        let verify = config.verification_capture.as_ref().and_then(|_| std::env::var("SKATE_VERIFY_CHALLENGE").ok());
        let path = map.path.as_deref();
        if let Some(o) = verify.and_then(|id| ftl.offers(path).find(|o| o.id == id)) {
            let at = ftl.rows.iter().find(|r| r.id == o.id).and_then(|r| r.sign_up.location.as_ref()).map(|l| l.matrix);
            info!("SKATE_VERIFY_CHALLENGE {} start at its sign-up {:?}", o.id, &o.location[..3]);
            if let Err(e) = at.map_or(Ok(()), |m| skater.travel_to(m)) {
                warn!("SKATE_VERIFY_CHALLENGE {e}");
            }
        }
    }
    let busy = challenges.as_deref().is_some_and(|c| c.run.is_some() || c.tutorial.is_some());
    for s in signed.read().filter(|s| s.body == crate::trigger_volumes::PLAYER) {
        if !ftl.rows.iter().any(|r| r.id == s.challenge) {
            continue;
        }
        if busy || ftl.run.is_some() {
            info!("SKATE_FTL sign-up {} ignored: a challenge is running", s.challenge);
            continue;
        }
        info!("SKATE_FTL sign-up {}", s.challenge);
        wanted.push(s.challenge.clone());
    }
    if let Some(id) = wanted.first() {
        start(&mut ftl, id, &config, &map, &mut volumes);
    }
}

fn start(ftl: &mut FollowTheLeader, wanted: &str, config: &crate::config::Config, map: &crate::map_transition::CurrentMap,
    volumes: &mut crate::trigger_volumes::TriggerVolumes) {
    let Some(data) = ftl.rows.iter().find(|r| r.id == wanted).cloned() else {
        info!("SKATE_FTL {wanted} not found");
        return;
    };
    if data.map.as_deref().is_some_and(|m| map.path.as_deref().is_some_and(|p| !crate::teleport_menu::same_map(p, m))) {
        info!("SKATE_FTL {wanted} is not on {}", map.name);
        return;
    }
    for v in [&data.end_volume, &data.boundary].into_iter().flatten().chain(&data.objective_volumes) {
        if volumes.get(&v.id).is_none() {
            volumes.map.push(crate::trigger_volumes::TriggerVolume::from_record(&v.volume));
        }
    }
    info!("SKATE_FTL {wanted}: {} objective volumes armed", data.objective_volumes.len());
    let map_volumes: Vec<(String, u64)> =
        volumes.map.iter().filter_map(|v| Some((v.name.clone(), u64::from_str_radix(&v.id, 16).ok()?))).collect();
    let Some(groups) = load_groups(&data, map_volumes) else {
        warn!("SKATE_FTL {wanted}: its owned objectives do not load; not started");
        return;
    };
    let rows: Vec<String> = data.competitors.iter().map(|c| c.character.clone()).collect();
    // films ride alone: only a row with competitors (follow the leader) loads its riders
    let leader = if rows.is_empty() { None } else {
        crate::contest_ai_host::ContestAi::load(&config.asset_root, &data.id, &rows, crate::race_ai_host::start_seed(map.generation))
    };
    let mut run = FilmRun::new(params(&data));
    info!("SKATE_FTL start {} at {} end={:?} max={:?} leaders={rows:?}", data.id, data.start.locator, data.end_volume.as_ref().map(|v| &v.name), data.max_follow_distance);
    let commands = run.start();
    info!("SKATE_FTL {wanted}: {} objective groups (killed it: {})", groups.len(), groups.iter().any(|g| g.killed));
    let mut state = FtlState { data, run, groups, evaluating: false, leader, awaiting_teleport: false, band: Some(FollowerBand::default()),
        carry: 0.0, eval_carry: 0.0, leader_last: None, leader_speed: 0.0, leader_path: Vec::new(), last_state: 0, failure_text: None, dialog_open: false, dialog_seconds: 0.0, leaders_in_end: Vec::new(), follow_indicators: Vec::new(), meter: None, queued: Vec::new(), feed: Default::default(), area_checked: false };
    // The first commands run on the next update with the frame's resources.
    state.pending(commands);
    ftl.run = Some(state);
}

impl FtlState {
    fn pending(&mut self, commands: Vec<Command>) {
        self.queued.extend(commands);
    }
}

/// The objective HUD's rows for a group (b844 / b947): the objectives with DisplayInHUD, in row order, each with its
/// title text id (the movie localizes it; `#OBJECTIVE NOT VALID!` without one, as retail's 8264D130) and its state
/// as the movie reads it: 0 failed, 1 complete, 2 not active yet, 3 active.
fn objective_rows(rows: &[ObjectiveData], states: &[skate_core::challenges::objectives::State]) -> Vec<crate::hud_challenge::ObjectiveRow> {
    use skate_core::challenges::objectives::State as S;
    rows.iter().zip(states).filter(|(r, _)| r.display_in_hud).map(|(r, s)| crate::hud_challenge::ObjectiveRow {
        state: match s {
            S::Failed => 0,
            S::Complete => 1,
            S::Inactive | S::Armed => 2,
            S::Active => 3,
        },
        kind: 0,
        title: r.title_id.clone().unwrap_or_else(|| "#OBJECTIVE NOT VALID!".into()),
        value: String::new(),
    }).collect()
}

fn hud_component(c: HudComponent) -> Option<Component> {
    match c {
        HudComponent::Timer => Some(Component::Timer),
        HudComponent::Objective => Some(Component::Objective),
        HudComponent::OwnedIt => Some(Component::OwnedIt),
        HudComponent::KilledIt => Some(Component::KilledIt),
        HudComponent::Countdown => Some(Component::Countdown),
        HudComponent::DistanceMeter => Some(Component::DistanceMeter),
    }
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut ftl: ResMut<FollowTheLeader>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
    (mut entered, mut exited, mut central, mut career): (
        MessageReader<crate::trigger_volumes::TriggerEntered>,
        MessageReader<crate::trigger_volumes::TriggerExited>,
        MessageWriter<crate::hud_central::CentralMessage>,
        MessageWriter<crate::career::ChallengeResult>,
    ),
    mut hud: ResMut<ChallengeHudState>,
    mut end_dialog: Option<ResMut<crate::end_dialog_host::EndDialogScreen>>,
    mut autopilot: Option<ResMut<crate::autopilot::Autopilot>>,
    (time, menus, mut follow, mut presenter): (Res<Time>, Option<Res<crate::retail_menus::RetailMenus>>, ResMut<crate::visual_indicators::FollowIndicators>, Option<ResMut<crate::presentation_run::Presenter>>),
    (challenges, pad, physics): (Option<Res<crate::challenges::Challenges>>, Option<Res<crate::input::ControllerInput>>, Option<Res<crate::physics::GamePhysics>>),
    (mut population, index, npc_transforms, mut clocks, mut commands_ecs, replays, mut brains): (
        Option<ResMut<crate::living_world::PopulationState>>,
        Option<Res<crate::living_world::npc_skaters::NpcSkaterIndex>>,
        Query<&Transform, With<crate::living_world::npc_skaters::NpcSkater>>,
        Query<&mut crate::living_world::npc_skaters::NpcRaceClock>,
        Commands,
        Query<&crate::living_world::npc_skaters::NpcReplay>,
        Query<&mut crate::living_world::npc_sim::NpcSim, With<crate::autopilot::Possessed>>,
    ),
) {
    let entered: Vec<u64> = entered.read().filter(|e| e.body == crate::trigger_volumes::PLAYER)
        .filter_map(|e| u64::from_str_radix(&e.volume, 16).ok()).collect();
    let exited: Vec<u64> = exited.read().filter(|e| e.body == crate::trigger_volumes::PLAYER)
        .filter_map(|e| u64::from_str_radix(&e.volume, 16).ok()).collect();
    let Some(state) = ftl.run.as_mut() else {
        if !follow.0.is_empty() {
            follow.0.clear();
        }
        if let Some(a) = autopilot.as_mut().filter(|a| a.follow.is_some() || a.area.is_some()) {
            a.follow = None;
            a.area = None;
        }
        for mut b in &mut brains {
            b.speed_cap = None;
        }
        return;
    };
    let id = state.data.id.clone();
    let mut commands = std::mem::take(&mut state.queued);
    // The end options dialog: its pick feeds Continue / Retry; the autopilot continues after a short look.
    if state.dialog_open {
        use skate_core::challenges::end_dialog::Action;
        state.dialog_seconds += time.delta_secs();
        if autopilot.as_ref().is_some_and(|a| a.requested) && state.dialog_seconds > AUTOPILOT_DIALOG_SECONDS {
            if let Some(d) = end_dialog.as_deref_mut().filter(|d| d.choice.is_none() && d.open) {
                info!("SKATE_FTL {id} end options: the autopilot continues");
                d.choice = Some(Action::Continue);
                d.open = false;
            }
        }
        match end_dialog.as_deref_mut() {
            Some(d) => {
                let event = match d.choice.take() {
                    Some(Action::Continue) => Some(Event::Continue),
                    Some(Action::Retry { .. }) => Some(Event::Retry),
                    _ => None,
                };
                if let Some(event) = event {
                    d.open = false;
                    state.dialog_open = false;
                    commands.extend(state.run.handle(event));
                }
            }
            None => {
                state.dialog_open = false;
                commands.extend(state.run.handle(Event::Continue));
            }
        }
    }
    // Positions: the player's hips (as the contests), each leader's body.
    let player = crate::trigger_volumes::player_points(&skater)[2];
    let leaders: Vec<(u32, Option<[f32; 3]>, Option<[f32; 3]>)> = match (state.leader.as_ref(), index.as_deref()) {
        (Some(ai), Some(index)) => ai.riders.iter().map(|r| {
            let t = r.npc.and_then(|n| index.0.get(&n)).and_then(|e| npc_transforms.get(*e).ok());
            (r.id, t.map(|t| [t.translation.x, t.translation.y + 1.0, t.translation.z]), t.map(|t| { let f = t.forward(); [f.x, f.y, f.z] }))
        }).collect(),
        _ => Vec::new(),
    };
    // A film with a boundary: the autopilot (a test aid) keeps to the lines that run through it, so a test run stays
    // where the objectives are instead of riding off and being sent back to the start.
    if let (Some(a), Some(b), Some(pop)) = (autopilot.as_mut(), state.data.boundary.as_ref(), population.as_deref()) {
        if state.leader.is_none() && a.area.is_none() && !state.area_checked {
            state.area_checked = true;
            // the lines through the objectives' own volumes (the spots that complete them), else through the boundary
            let lines = crate::living_world::npc_skaters::npc_lines(pop);
            let through = |boxes: &[skate_core::triggers::OrientedBox]| -> Vec<[u8; 16]> {
                lines.values().filter(|l| l.nodes.iter().any(|n| boxes.iter().any(|b| b.contains_point(n.position)))).map(|l| l.id).collect()
            };
            let spots: Vec<_> = state.data.objective_volumes.iter().map(|v| v.volume.shape.oriented_box()).collect();
            // first the lines that do a trick (a grind, a flip) inside one: the objectives ask for a trick at the spot
            let tricking: Vec<[u8; 16]> = lines.values().filter(|l| {
                let mut open = false;
                l.nodes.iter().any(|n| {
                    use skate_core::living_world::replay::node_events::{END_TRICK, START_TRICK};
                    open = match n.event { START_TRICK => true, END_TRICK => false, _ => open };
                    open && spots.iter().any(|b| b.contains_point(n.position))
                })
            }).map(|l| l.id).collect();
            let mut inside = if tricking.is_empty() { through(&spots) } else { tricking.clone() };
            if !tricking.is_empty() {
                info!("SKATE_FTL {id} autopilot area: {} lines trick inside the objective volumes", tricking.len());
            }
            let what = if inside.is_empty() {
                inside = through(&[b.volume.shape.oriented_box()]);
                b.name.clone()
            } else {
                format!("{} objective volumes", spots.len())
            };
            info!("SKATE_FTL {id} autopilot area: {} lines through {what}", inside.len());
            if !inside.is_empty() {
                a.area = Some(std::sync::Arc::new(inside));
            }
        }
    }
    // The autopilot (a test aid) takes the first leader's lines in the leader's order: the line the leader took after
    // the one the brain is on (after a bail it keeps to the path instead of walking to where the leader is now).
    if let Some(a) = autopilot.as_mut() {
        let line_of = |id: Option<skate_core::living_world::LivingWorldId>| id.and_then(|n| index.as_deref().and_then(|i| i.0.get(&n).copied()))
            .and_then(|e| replays.get(e).ok()).map(|r| r.cursor.line);
        if let Some(l) = line_of(state.leader.as_ref().and_then(|ai| ai.riders.first()).and_then(|r| r.npc)) {
            if state.leader_path.last() != Some(&l) {
                state.leader_path.push(l);
            }
        }
        let line = follow_line(&state.leader_path, line_of(a.npc_id()));
        if a.follow != line {
            a.follow = line;
        }
    }
    // ...and keeps a gap: it may not pass the leader (OURS, a test aid; FOLLOW_GAP).
    if let Some((_, Some(at), Some(fwd))) = leaders.first() {
        let dt = time.delta_secs().max(1e-3);
        if let Some(last) = state.leader_last {
            let v = ((at[0] - last[0]).powi(2) + (at[2] - last[2]).powi(2)).sqrt() / dt;
            if v < 30.0 {
                state.leader_speed += (v - state.leader_speed) * (dt * 2.0).min(1.0);
            }
        }
        state.leader_last = Some(*at);
        let behind = (at[0] - player[0]) * fwd[0] + (at[2] - player[2]) * fwd[2];
        let cap = follow_cap(behind, state.leader_speed);
        for mut b in &mut brains {
            if b.speed_cap != cap {
                b.speed_cap = cap;
            }
        }
    }
    let mut distances = [None; 4];
    for (n, at, _) in &leaders {
        if let (Some(at), Some(slot)) = (at, distances.get_mut(*n as usize - 1)) {
            *slot = Some(((at[0] - player[0]).powi(2) + (at[1] - player[1]).powi(2) + (at[2] - player[2]).powi(2)).sqrt());
        }
    }
    // The leader's body entering the end volume (triggerentercollision with its competitor index, b1006 §0.6).
    let running = state.run.state() == State::ChallengeRunning;
    if let Some(end) = state.data.end_volume.as_ref().filter(|_| running) {
        let shape = end.volume.shape.oriented_box();
        for (n, at, _) in &leaders {
            let inside = at.is_some_and(|p| shape.contains_point(p));
            if inside && !state.leaders_in_end.contains(n) {
                state.leaders_in_end.push(*n);
                info!("SKATE_FTL {id} leader {n} reached the end volume");
                if let Some(v) = volume_id(&state.data.end_volume) {
                    commands.extend(state.run.handle(Event::TriggerEnter { volume: v, competitor: *n }));
                }
            }
        }
    }
    // The player's volumes: the run (end volume, boundary) and the objectives.
    for &v in &exited {
        commands.extend(state.run.handle(Event::TriggerExit { volume: v }));
    }
    for &v in &entered {
        commands.extend(state.run.handle(Event::TriggerEnter { volume: v, competitor: 0 }));
    }
    let state_id = skater.physical_state_id();
    if state_id == 300 && state.last_state != 300 {
        commands.extend(state.run.handle(Event::Wipeout));
    }
    state.last_state = state_id;
    // The groups get what the tutorial's get (the shared feed): skater facts, the frame's volumes, air, pushes,
    // tricks, markers and banked sequences, and one evaluation per 60 Hz challenge tick, so a row's own counters (the
    // speech rows' `Delay` of 60 evaluations) keep their time. [inferred: retail's evaluation rate is open, b1006]
    state.eval_carry += time.delta_secs() * TICKS_PER_SECOND;
    let ticks = state.eval_carry as u32;
    state.eval_carry -= state.eval_carry.floor();
    let shared = challenges.as_deref();
    let banked: Vec<(i32, bool)> = shared.map(|c| c.banked_this_frame.iter().map(|s| (s.score, s.wiped_out)).collect()).unwrap_or_default();
    let raw = pad.as_ref().map(|p| p.raw_input()).unwrap_or_default();
    let feed_input = crate::challenge_objectives::FeedInput {
        entered: &entered, exited: &exited, banked: &banked, ticks, state_id, in_sequence: skater.scoring.sequence_active(),
        markers: shared.map_or(&[][..], |c| &c.markers_this_frame), tricks: shared.map_or(&[][..], |c| &c.tricks_done_this_frame),
        began: shared.map_or(&[][..], |c| &c.tricks_began_this_frame), right_stick_y: raw.right[1], triggers: raw.triggers,
        manual: skater.scoring.manual(), pushes: shared.map_or(0, |c| c.pushes_this_frame),
        pump_acceleration: skater.ground.pumping.pump_acceleration, pump_user: skater.ground.pumping.intentional_pumping != 0,
        grind_rail: (400..=405).contains(&state_id).then_some(skater.player_input.grind.rail_owner).flatten()
            .and_then(|owner| physics.as_ref().and_then(|p| p.rail_guids(owner))),
        time_to_grind: {
            let air = &skater.player_input.physical.air;
            if air.flag_443 != 0 { air.collision_time_180 - air.time_in_state_176 } else { skate_core::challenges::api::NO_GRIND_TIME }
        },
        board_adjust: skater.animation_input.fields.board_adjust,
        wipeout_over: skater.player_input.physical.skeleton.over_599 != 0,
        bail_pose: shared.and_then(|c| c.last_bail_pose),
        competitor_distances: distances,
    };
    let killed_of: Vec<bool> = state.groups.iter().map(|g| g.killed).collect();
    let mut groups: Vec<&mut DataObjectives> = state.groups.iter_mut().map(|g| &mut g.objectives).collect();
    let fed = state.feed.step(&mut groups, &feed_input, state.evaluating);
    if state.evaluating {
        let events: Vec<(bool, GroupEvent)> = fed.into_iter().map(|(i, e)| (killed_of[i], e)).collect();
        for g in &mut state.groups {
            let o = &mut g.objectives;
            for (competitor, on) in o.follow_indicator.drain(..) {
                // retail removes any entry for the id first, so adding twice keeps one (82D4F470)
                state.follow_indicators.retain(|&c| c != competitor);
                if on {
                    state.follow_indicators.push(competitor);
                }
                info!("SKATE_FTL {id} follow indicator {competitor} {on}");
            }
        }
        // the followed competitors' bodies get the trail (by their living-world serial)
        let serials: Vec<u32> = state.leader.as_ref().map(|ai| ai.riders.iter()
            .filter(|r| state.follow_indicators.contains(&r.id)).filter_map(|r| r.npc.map(|n| n.serial)).collect()).unwrap_or_default();
        if follow.0 != serials {
            follow.0 = serials;
        }
        for (killed, e) in events {
            let rows = if killed { &state.data.objectives.killed } else { &state.data.objectives.owned };
            match e {
                GroupEvent::Completed(i) => {
                    info!("SKATE_FTL {id} objective {i} complete (killed it group: {killed}) {}", rows.get(i).map_or("", |o| o.key.as_str()));
                }
                GroupEvent::AllComplete => {
                    info!("SKATE_FTL {id} all objectives complete (killed it group: {killed})");
                    commands.extend(state.run.handle(Event::AllObjectivesComplete { killed }));
                }
                GroupEvent::FailureCondition { objective, row } => {
                    let text = rows.get(objective).and_then(|o| o.inline_failure(row)).and_then(|f| f.text.clone());
                    info!("SKATE_FTL {id} objective {objective} (killed it group: {killed}) failure row {row} ({text:?}) distances {distances:?}");
                    state.failure_text = Some(text.unwrap_or_else(|| FAILURE_TEXT.into()));
                    commands.extend(state.run.handle(Event::ObjectiveFailed));
                }
                // A hint row plays its presentation only (82C755B8, b942): the leader's speech lines and the start tip.
                GroupEvent::Hint { objective, row } => {
                    let events = rows.get(objective).and_then(|o| o.hints.get(row)).map(|h| h.presentation.events()).unwrap_or_default();
                    if let (Some(p), false) = (presenter.as_deref_mut(), events.is_empty()) {
                        let group = format!("{id}/{}obj{objective}/hint{row}", if killed { "killed/" } else { "" });
                        let started = p.manager.start(skate_core::presentation::Group { id: group.clone(), owner: id.clone(), events });
                        info!("SKATE_FTL {id} hint row {row} of objective {objective}: presentation {group} {started:?}");
                    }
                }
                _ => {}
            }
        }
    }
    if state.awaiting_teleport && !skater.teleport_pending() {
        state.awaiting_teleport = false;
        commands.extend(state.run.handle(Event::TeleportFinished));
    }
    state.carry += time.delta_secs() * TICKS_PER_SECOND;
    while state.carry >= 1.0 {
        state.carry -= 1.0;
        commands.extend(state.run.tick());
    }
    let mut finished = None;
    let mut guard = 0;
    loop {
        let mut queue: std::collections::VecDeque<Command> = std::mem::take(&mut commands).into();
        while let Some(c) = queue.pop_front() {
            guard += 1;
            match c {
                Command::CreateHud { .. } => queue.extend(state.run.handle(Event::HudLoaded)),
                Command::StartPresentation => queue.extend(state.run.handle(Event::PresentationFinished)),
                Command::SpawnAi | Command::RespawnAiAtStart => {
                    if let (Some(ai), Some(pop)) = (state.leader.as_mut(), population.as_deref_mut()) {
                        ai.place(&state.data.start.matrix, &state.data.start.slots, &state.data.ai_paths.paths(), pop);
                    }
                }
                Command::PinAi(true) => state.leader.iter_mut().for_each(|a| a.stop()),
                Command::PinAi(false) | Command::StartAiPaths => state.leader.iter_mut().for_each(|a| a.go()),
                // PinAI(leader): the leader that reached the end (competitor 1, b1006 §0.6).
                Command::PinLeader => {
                    if let Some(r) = state.leader.as_mut().and_then(|a| a.riders.first_mut()) {
                        r.moving = false;
                    }
                }
                // retail repositions twice in one frame after a second out-of-bounds (the stale flag, b999 §2); the
                // player is already on the way to the start, so the second one is that same move
                Command::RepositionPlayer if state.awaiting_teleport && skater.teleport_pending() => {
                    info!("SKATE_FTL {id} already on the way to the start");
                }
                Command::RepositionPlayer => {
                    let at = state.data.start.slots.first().copied().unwrap_or(state.data.start.matrix);
                    info!("SKATE_FTL {id} player to the start {:?}", &at[3][..3]);
                    if let Err(e) = skater.travel_ex(at, None, !state.data.spawn_offboard) {
                        warn!("SKATE_FTL teleport {e}");
                    }
                    if let Some(a) = autopilot.as_mut() {
                        a.host_teleported = true;
                    }
                    state.awaiting_teleport = true;
                }
                Command::Evaluation(on) => state.evaluating = on,
                // every objective active (the films' flow, b934): not only the first one, which left a film's other
                // objectives inactive however often their spots were skated (desktop run 2026-11-13)
                Command::ActivateStartObjectives => {
                    for g in &mut state.groups {
                        g.objectives.activate_flow();
                        info!("SKATE_FTL {id} objectives start (killed it group: {}): {:?}", g.killed, g.objectives.states());
                    }
                }
                Command::ResetObjectives => {
                    // a restart resets and the countdown activates later; a re-arm while running (a film's wipeout,
                    // objective failure or session marker, b999 §2) starts the objectives over at once
                    let running = state.run.state() == State::ChallengeRunning;
                    if !running {
                        state.leaders_in_end.clear();
                        state.leader_path.clear();
                        state.failure_text = None;
                    }
                    for g in &mut state.groups {
                        g.objectives.reset_only();
                        if running {
                            g.objectives.activate_flow();
                        }
                    }
                }
                Command::ShowHud(c, shown) => {
                    // Countdown entry: SetupDistanceHUDData(max = the row's FTLMaxFollowDistance, current) with the show.
                    // OURS: a row without a max gets -1 (never red).
                    if matches!(c, HudComponent::DistanceMeter) && shown {
                        let max = state.data.max_follow_distance.unwrap_or(-1.0);
                        state.meter = Some(skate_core::challenges::distance_meter::DistanceMeter::setup(max, distances[0].unwrap_or(0.0)));
                    }
                    match hud_component(c) {
                        Some(c) => hud.show(c, shown),
                        None => info!("SKATE_FTL {id} ShowHud({c:?}, {shown}) NOT RETAIL YET"),
                    }
                }
                Command::StartTimerHud { .. } => hud.show(Component::Timer, true),
                Command::CentralMessage(m) => {
                    let text = match m {
                        Message::ObjectiveFailure => state.failure_text.clone().unwrap_or_else(|| FAILURE_TEXT.into()),
                        // a film's boundary: the same retail text as own the spot's (ReturnToChallengeArea)
                        Message::ReturnToArea => crate::challenges::RETURN_TO_AREA.into(),
                        other => {
                            info!("SKATE_FTL {id} central message {other:?} NOT RETAIL YET");
                            continue;
                        }
                    };
                    central.write(crate::hud_central::CentralMessage::new(text));
                }
                Command::PlayHighlightReel => queue.extend(state.run.handle(Event::HighlightReelComplete)),
                Command::OpenContinueRetry => {
                    let killed = state.run.result() == Flow::Kill;
                    let won = killed || state.run.result() == Flow::Win;
                    info!("SKATE_FTL {id} end options won={won} killed={killed}");
                    match end_dialog.as_deref_mut().filter(|d| d.available) {
                        Some(d) => {
                            d.dialog.passed = won;
                            d.dialog.killed = killed;
                            d.title = state.data.title_id.clone();
                            d.open = true;
                            state.dialog_open = true;
                            state.dialog_seconds = 0.0;
                        }
                        None => queue.extend(state.run.handle(Event::Continue)),
                    }
                }
                Command::Complete(c) => finished = Some(c),
                other => info!("SKATE_FTL {id} {other:?}"),
            }
        }
        // The waits the machine polls: assets (none to load), the leader's existence (spawned at once or never).
        match state.run.state() {
            State::WaitForAssetLoad => commands.extend(state.run.handle(Event::AssetsLoaded)),
            State::CheckAi => {
                let exists = state.leader.as_ref().is_some_and(|a| a.riders.iter().any(|r| r.npc.is_some()));
                if !exists {
                    warn!("SKATE_FTL {id}: no leader could spawn (lines or look missing); abandoned");
                }
                commands.extend(state.run.handle(if exists { Event::AiExist(true) } else { Event::Abandon }));
            }
            _ => {}
        }
        if commands.is_empty() || guard > 256 {
            break;
        }
    }
    // The follower band on each leader while it rides (b969).
    if let Some(ai) = state.leader.as_mut() {
        for r in &mut ai.riders {
            let band = leaders.iter().find(|(n, ..)| *n == r.id).and_then(|(_, at, fwd)| Some((at.as_ref()?, fwd.as_ref()?)));
            r.scale = match (state.band.as_ref(), band) {
                (Some(b), Some((at, fwd))) => shaped_speed(1.0, 0.0, follower_band(*at, *fwd, player, b), &BandTuning::default()),
                _ => 1.0,
            };
        }
        if let (Some(index), Some(pop)) = (index.as_deref(), population.as_deref()) {
            ai.step(pop.world.tick(), index, &mut clocks, &mut commands_ecs);
        }
    }
    if hud.is_shown(Component::Timer) {
        let (text, flag) = skate_core::challenges::timers::hud_text(state.run.ftl_ticks().unwrap_or(0));
        if hud.timer_text != text || hud.timer_flag != flag {
            hud.timer_text = text;
            hud.timer_flag = flag;
        }
    }
    // The objective HUD (SetupObjectiveHUDData on the first owned group, b875): one row per objective shown on the
    // HUD, its title text id and its state as the objective movie reads it. NOT RETAIL YET: a replay of an owned
    // challenge shows the first killed group instead.
    if hud.is_shown(Component::Objective) {
        let states = state.groups.iter().find(|g| !g.killed).map(|g| g.objectives.states()).unwrap_or_default();
        let rows = objective_rows(&state.data.objectives.owned, &states);
        if hud.objectives != rows {
            hud.objectives = rows;
        }
    }
    if let Some(meter) = state.meter.as_mut().filter(|_| hud.is_shown(Component::DistanceMeter)) {
        let tuning = skate_core::challenges::distance_meter::MeterTuning::default();
        meter.advance(time.delta_secs(), distances[0].unwrap_or(meter.shown), &tuning);
        let now = crate::hud_challenge::DistanceHud {
            metres: meter.shown,
            imperial: menus.as_deref().is_some_and(crate::retail_menus::imperial),
            flag: meter.flag(&tuning),
        };
        if hud.distance != now {
            hud.distance = now;
        }
    }
    if let Some(c) = finished {
        let mut done = ftl.run.take().expect("the run");
        if let (Some(ai), Some(pop)) = (done.leader.as_mut(), population.as_deref_mut()) {
            ai.remove(pop);
        }
        info!("SKATE_FTL end {id} completion={c:?}");
        ftl.result = Some(format!("ftl={id} completion={c:?}"));
        for h in [Component::Timer, Component::Countdown, Component::OwnedIt, Component::KilledIt, Component::DistanceMeter] {
            hud.show(h, false);
        }
        if c != Completion::Abandoned || done.run.result() == Flow::Win {
            career.write(crate::career::ChallengeResult { challenge: id, completion: c, reactivate: false });
        }
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<FollowTheLeader>()
        .add_systems(PreUpdate, arm.after(crate::map_transition::MapTransitionSet).run_if(crate::challenges::opted_in))
        .add_systems(Update, update.after(crate::challenges::CareerResultSet).run_if(crate::challenges::opted_in));
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::challenges::objectives::Message as GroupMessage;

    #[test]
    fn the_autopilot_keeps_a_gap_and_never_passes_the_leader() {
        assert_eq!(follow_cap(12.0, 8.0), None);
        assert_eq!(follow_cap(FOLLOW_GAP, 8.0), None);
        assert_eq!(follow_cap(4.0, 8.0), Some(7.0));
        assert_eq!(follow_cap(0.0, 8.0), Some(4.8));
        assert_eq!(follow_cap(-3.0, 8.0), Some(4.8));
        assert_eq!(follow_cap(2.0, 0.0), Some(FOLLOW_MIN_SPEED), "a leader still at its start does not stop it");
    }

    #[test]
    fn the_autopilot_takes_the_leaders_lines_in_order() {
        let (a, b, c) = ([1u8; 16], [2u8; 16], [3u8; 16]);
        let path = [a, b, c];
        assert_eq!(follow_line(&path, Some(a)), Some(b), "after a bail on a: b, not where the leader is now");
        assert_eq!(follow_line(&path, Some(c)), Some(c));
        assert_eq!(follow_line(&path, None), Some(a));
        assert_eq!(follow_line(&path, Some([9u8; 16])), Some(c));
        assert_eq!(follow_line(&[], Some(a)), None);
    }

    fn volume_row(key: &str, volume: &str) -> ObjectiveData {
        let mut triggers = vec![false; 14];
        triggers[12] = true;
        ObjectiveData { key: key.into(), activation: 0, triggers, points_valid: None,
            condition: serde_json::json!({"locals": 0, "body": [{"return": {"api": "ObjectiveTrigger.IsInVolume",
                "args": [{"lit": "1"}, {"lit": format!("DIST_DownTown|{volume}")}]}}]}),
            failures: Vec::new(), failures_inline: Vec::new(), failure_first: false, hints: Vec::new(), title_id: None, title_arg: None, display_in_hud: true }
    }

    #[test]
    fn a_killed_it_list_runs_as_its_own_group_and_completes_on_its_own() {
        let mut row: FtlData = serde_json::from_value(serde_json::json!({
            "id": "ftl_test", "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}
        })).unwrap();
        let volumes = vec![("ftl_end".to_string(), 7), ("ftl_kill".to_string(), 9)];
        row.objectives.owned = vec![volume_row("owned_0", "ftl_end")];
        assert_eq!(load_groups(&row, volumes.clone()).map(|g| g.len()), Some(1), "retail rows: the owned list only");
        row.objectives.killed = vec![volume_row("killed_0", "ftl_kill")];
        let mut groups = load_groups(&row, volumes).unwrap();
        assert_eq!(groups.iter().map(|g| g.killed).collect::<Vec<_>>(), [false, true]);
        let mut done = Vec::new();
        for g in &mut groups {
            g.objectives.activate_first();
            g.objectives.enter(9);
            let events = g.objectives.handle(GroupMessage::Tick { gated: false });
            if events.contains(&GroupEvent::AllComplete) {
                done.push(g.killed);
            }
        }
        assert_eq!(done, [true], "inside the killed volume only the killed it group completes");
        row.objectives.owned.clear();
        assert!(load_groups(&row, vec![]).is_none(), "no owned list: not started");
    }

    #[test]
    fn rows_with_a_sign_up_sphere_on_this_map_are_offered_to_the_prompt() {
        let row = |id: &str, map: &str, sphere: bool| -> FtlData {
            let mut v = serde_json::json!({"id": id, "map": map, "title_id": "ID_T", "challenge_type": 23,
                "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}});
            if sphere {
                v["location"] = serde_json::json!({"locator": "l", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[5,6,7,1]]});
                v["sign_up_radius"] = serde_json::json!(3.0);
            }
            serde_json::from_value(v).unwrap()
        };
        let ftl = FollowTheLeader { rows: vec![row("a", "university", true), row("b", "university", false), row("c", "downtown", true)], ..Default::default() };
        let offers: Vec<_> = ftl.offers(Some(std::path::Path::new("maps/University.skate"))).collect();
        assert_eq!(offers.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["a"]);
        assert_eq!((offers[0].location, offers[0].sign_up_radius, offers[0].challenge_type), ([5., 6., 7., 1.], 3.0, Some(23)));
        assert_eq!(crate::challenges::sign_up_state(&offers[0]), 1, "the Challenge overlay state, not own the spot's");
        assert_eq!(ftl.offers(None).count(), 0);
    }

    #[test]
    fn a_films_objectives_all_start_active_not_only_the_first() {
        use skate_core::challenges::objectives::State as S;
        // film_univ_02's owned rows: activation 0, 5 (on a sequence), 0
        let mut rows = vec![volume_row("film_univ_02_0", "a"), volume_row("film_univ_02_1", "b"), volume_row("film_univ_02_2", "c")];
        rows[1].activation = 5;
        let mut d = DataObjectives::new("film_univ_02", &rows, vec![("a".into(), 1), ("b".into(), 2), ("c".into(), 3)], Default::default()).unwrap();
        d.activate_flow();
        assert_eq!(d.states(), [S::Active, S::Active, S::Active], "the films' flow mode 0 (b934)");
    }

    #[test]
    fn the_objective_hud_lists_the_shown_objectives_with_their_titles_and_states() {
        use skate_core::challenges::objectives::State as S;
        let mut a = volume_row("film_indu_01_0", "x");
        a.title_id = Some("ID_CHALLENGE_FILM_INDU_01_OWNED01".into());
        let mut hidden = volume_row("film_indu_01_6", "x");
        hidden.display_in_hud = false;
        let mut b = volume_row("film_indu_01_1", "x");
        b.title_id = Some("ID_CHALLENGE_FILM_INDU_01_OWNED02".into());
        let rows = objective_rows(&[a, hidden, b], &[S::Complete, S::Active, S::Active]);
        let got: Vec<(i32, &str)> = rows.iter().map(|r| (r.state, r.title.as_str())).collect();
        assert_eq!(got, [(1, "ID_CHALLENGE_FILM_INDU_01_OWNED01"), (3, "ID_CHALLENGE_FILM_INDU_01_OWNED02")],
            "DisplayInHUD false leaves the row out (film_indu_01_6, b947)");
    }

    /// The exported film_univ_02 (SKATE_TEST_ASSETS = an assets folder with private/challenges.json): a clean line
    /// that grinds inside an objective's volumes completes that objective and its HUD row, through the same feed and
    /// rows the host uses; a line that grinds outside them or bails completes nothing.
    #[test]
    fn film_univ_02_grind_objectives_complete_from_the_exported_rows() {
        use crate::challenge_objectives::{FeedInput, ObjectiveFeed};
        use skate_core::challenges::objectives::State as S;
        let Some(assets) = std::env::var_os("SKATE_TEST_ASSETS") else { test_skip!("set SKATE_TEST_ASSETS") };
        let Some(data) = load(std::path::Path::new(&assets)).into_iter().find(|r| r.id == "film_univ_02") else {
            test_skip!("no film_univ_02 in the export")
        };
        let volumes: Vec<(String, u64)> =
            data.objective_volumes.iter().filter_map(|v| Some((v.name.clone(), u64::from_str_radix(&v.id, 16).ok()?))).collect();
        let id = |name: &str| volumes.iter().find(|(n, _)| n == name).map(|(_, i)| *i).unwrap_or_else(|| panic!("no volume {name}"));
        // (volume to grind in, the owned objective it completes)
        let cases = [("film_univ_02_grind_volume_2", Some(2)), ("film_univ_02_gap_01_grd_volume_3", Some(0)), ("film_univ_02_gap_02_volume_1", None)];
        for (volume, expected) in cases {
            for bail in [false, true] {
                let mut groups = load_groups(&data, volumes.clone()).expect("the rows load");
                for g in &mut groups {
                    g.objectives.activate_flow();
                }
                let mut feed = ObjectiveFeed::default();
                let mut completed = Vec::new();
                let mut frame = |entered: &[u64], exited: &[u64], state_id: u32, in_sequence: bool, banked: &[(i32, bool)]| {
                    let input = FeedInput { entered, exited, banked, ticks: 1, state_id, in_sequence, ..Default::default() };
                    let mut refs: Vec<&mut DataObjectives> = groups.iter_mut().map(|g| &mut g.objectives).collect();
                    for (i, e) in feed.step(&mut refs, &input, true) {
                        if let GroupEvent::Completed(o) = e {
                            completed.push((i, o));
                        }
                    }
                };
                let v = id(volume);
                frame(&[], &[], 100, true, &[]);
                frame(&[v], &[], 200, true, &[]);
                for _ in 0..20 {
                    frame(&[], &[], 401, true, &[]);
                }
                frame(&[], &[v], 200, true, &[]);
                frame(&[], &[], if bail { 300 } else { 100 }, true, &[]);
                frame(&[], &[], 100, false, &[(800, bail)]);
                let owned: Vec<usize> = completed.iter().filter(|(g, _)| *g == 0).map(|(_, o)| *o).collect();
                let want: Vec<usize> = if bail { Vec::new() } else { expected.into_iter().collect() };
                assert_eq!(owned, want, "grinding in {volume}, bail {bail}");
                let rows = objective_rows(&data.objectives.owned, &groups[0].objectives.states());
                let done: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.state == 1).map(|(i, _)| i).collect();
                assert_eq!(done, want, "the HUD rows show it ({volume}, bail {bail})");
                assert!(groups[0].objectives.states().iter().all(|s| *s != S::Failed));
            }
        }
    }

    #[test]
    fn a_film_row_runs_on_the_same_host_without_a_leader_or_a_film_clock() {
        let row: FtlData = serde_json::from_value(serde_json::json!({
            "id": "film_univ_02", "family": "films", "map": "university",
            "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]},
            "total_film_time": 180.0, "film_low_limit": 5, "prefilm_time": 0.0, "allow_player_bail": false
        })).unwrap();
        let p = params(&row);
        assert_eq!((p.kind, p.competitors, p.allow_player_bail), (Kind::Film, 0, false), "films ride alone and re-arm on a bail");
        assert_eq!((p.total_film_time, p.film_low_limit, p.prefilm_time, p.simul_play), (180.0, 5.0, 0.0, false));
        let old: FtlData = serde_json::from_value(serde_json::json!({
            "id": "ftl_test", "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}})).unwrap();
        assert_eq!(kind(&old), Kind::Follow, "an export from before films is follow the leader");
        let anywhere = serde_json::json!({"id": "film_anyw_01", "family": "anywhere_films", "start": null});
        assert!(serde_json::from_value::<FtlData>(anywhere).is_err(), "anywhere films are left out until they are hosted");
    }

    #[test]
    fn rows_become_follow_params_with_the_end_volume_and_their_competitors() {
        let row: FtlData = serde_json::from_value(serde_json::json!({
            "id": "ftl_test", "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]},
            "max_follow_distance": 100.0, "competitors": [{"character": "reda_ftl"}, {"character": "second"}]
        })).unwrap();
        let p = params(&row);
        assert_eq!(p.kind, Kind::Follow);
        assert_eq!(p.competitors, 2);
        assert!(p.allow_player_bail && !p.allow_objective_failure, "family defaults (b1006 §0.3 / §0.5)");
        assert_eq!(p.ftl_window, 10.0);
    }
}
