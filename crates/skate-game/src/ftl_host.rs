//! Follow the leader in play (research b1006; doc 42 "Follow the leader"): a row from `private/challenges.json`
//! `follow_the_leaders` runs through `skate_core::challenges::film_run` (Kind::Follow), its owned objectives from the
//! exported rows (`challenge_objectives.rs`), the leader (competitor 1) riding the challenge's lines (`ch_<id>`) on the
//! contest rider code with the follower band. Start: `SKATE_FTL=<id>` on that row's map.
//! NOT RETAIL YET: the follow indicator trail (logged), speech, the killed objectives.
use crate::challenge_objectives::{DataObjectives, ObjectiveData};
use crate::hud_challenge::{ChallengeHudState, Component};
use bevy::prelude::*;
use serde::Deserialize;
use skate_core::challenges::api::SkaterFacts;
use skate_core::challenges::film_run::{Command, Event, FilmRun, HudComponent, Kind, Message, Params, State};
use skate_core::challenges::objectives::{Event as GroupEvent, Message as GroupMessage};
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

/// At most this many objective evaluations catch up in one frame (a long frame does not replay seconds of them).
const MAX_EVALUATIONS_PER_FRAME: u32 = 4;

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

/// One `follow_the_leaders` row as `challenges.py` exports it.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct FtlData {
    pub id: String,
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub title_id: Option<String>,
    pub start: FtlStart,
    #[serde(default)]
    pub end_volume: Option<crate::challenges::Volume>,
    #[serde(default)]
    pub boundary: Option<crate::challenges::Volume>,
    #[serde(default)]
    pub max_follow_distance: Option<f32>,
    #[serde(default)]
    pub countdown_delay: Option<f32>,
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
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    follow_the_leaders: Vec<FtlData>,
}

pub(crate) fn load(assets: &std::path::Path) -> Vec<FtlData> {
    std::fs::read(assets.join("private/challenges.json")).ok()
        .and_then(|b| serde_json::from_slice::<File>(&b).ok()).map(|f| f.follow_the_leaders).unwrap_or_default()
}

fn volume_id(v: &Option<crate::challenges::Volume>) -> Option<u64> {
    v.as_ref().and_then(|v| u64::from_str_radix(&v.id, 16).ok())
}

/// The row's values over the family defaults (b1006 §3).
pub(crate) fn params(d: &FtlData) -> Params {
    let base = Params::retail(Kind::Follow);
    Params {
        competitors: d.competitors.len().max(1) as u32,
        boundary: volume_id(&d.boundary),
        ftl_end_volume: volume_id(&d.end_volume),
        countdown_delay: d.countdown_delay.unwrap_or(base.countdown_delay),
        allow_player_bail: d.allow_player_bail,
        allow_objective_failure: d.allow_objective_failure,
        ..base
    }
}

pub(crate) struct FtlState {
    pub data: FtlData,
    run: FilmRun,
    objectives: Option<DataObjectives>,
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
    /// The player is held from the start until the countdown releases it (PinPlayer / UnpinAI(player)).
    pub(crate) fn player_pinned(&self) -> bool {
        self.run.as_ref().is_some_and(|r| matches!(r.run.state(), State::PinPlayer | State::MakeObjectives | State::PrefilmDisplay
            | State::NisOverview | State::Setup | State::WaitForAssetLoad | State::SpawnPlayer | State::CheckAi | State::Wait | State::Countdown))
    }
}

fn arm(mut ftl: ResMut<FollowTheLeader>, config: Res<crate::config::Config>, map: Res<crate::map_transition::CurrentMap>,
    mut volumes: ResMut<crate::trigger_volumes::TriggerVolumes>, mut population: Option<ResMut<crate::living_world::PopulationState>>) {
    if ftl.generation == Some(map.generation) {
        return;
    }
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
    let Ok(wanted) = std::env::var("SKATE_FTL") else { return };
    let Some(data) = ftl.rows.iter().find(|r| r.id == wanted).cloned() else {
        info!("SKATE_FTL {wanted} not found");
        return;
    };
    if data.map.as_deref().is_some_and(|m| map.path.as_deref().is_some_and(|p| !crate::teleport_menu::same_map(p, m))) {
        info!("SKATE_FTL {wanted} is not on {}", map.name);
        return;
    }
    for v in [&data.end_volume, &data.boundary].into_iter().flatten() {
        if volumes.get(&v.id).is_none() {
            volumes.map.push(crate::trigger_volumes::TriggerVolume::from_record(&v.volume));
        }
    }
    let map_volumes: Vec<(String, u64)> =
        volumes.map.iter().filter_map(|v| Some((v.name.clone(), u64::from_str_radix(&v.id, 16).ok()?))).collect();
    let objectives = DataObjectives::new(&data.id, &data.objectives.owned, map_volumes, Default::default());
    if objectives.is_none() {
        warn!("SKATE_FTL {wanted}: its owned objectives do not load; not started");
        return;
    }
    if !data.objectives.killed.is_empty() {
        info!("SKATE_FTL {wanted}: {} killed objective rows NOT RETAIL YET (not run)", data.objectives.killed.len());
    }
    let rows: Vec<String> = data.competitors.iter().map(|c| c.character.clone()).collect();
    let leader = crate::contest_ai_host::ContestAi::load(&config.asset_root, &data.id, &rows, crate::race_ai_host::start_seed(map.generation));
    let mut run = FilmRun::new(params(&data));
    info!("SKATE_FTL start {} at {} end={:?} max={:?} leaders={rows:?}", data.id, data.start.locator, data.end_volume.as_ref().map(|v| &v.name), data.max_follow_distance);
    let commands = run.start();
    let mut state = FtlState { data, run, objectives, evaluating: false, leader, awaiting_teleport: false, band: Some(FollowerBand::default()),
        carry: 0.0, eval_carry: 0.0, leader_last: None, leader_speed: 0.0, leader_path: Vec::new(), last_state: 0, failure_text: None, dialog_open: false, dialog_seconds: 0.0, leaders_in_end: Vec::new(), follow_indicators: Vec::new(), meter: None, queued: Vec::new() };
    // The first commands run on the next update with the frame's resources.
    state.pending(commands);
    ftl.run = Some(state);
}

impl FtlState {
    fn pending(&mut self, commands: Vec<Command>) {
        self.queued.extend(commands);
    }
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
        if let Some(a) = autopilot.as_mut().filter(|a| a.follow.is_some()) {
            a.follow = None;
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
    let off = crate::tutorial_run::off_board(state_id);
    let in_air = (200..=202).contains(&state_id);
    if let Some(o) = state.objectives.as_mut() {
        o.skater = SkaterFacts {
            wiping_out: state_id == 300,
            off_board: off,
            grinding: (400..=405).contains(&state_id),
            on_board: !off && state_id != 300 && state_id < 700,
            on_ground: (100..=105).contains(&state_id),
            in_air,
            competitor_distances: distances,
            ..SkaterFacts::default()
        };
    }
    state.last_state = state_id;
    if state.evaluating {
        let mut events = Vec::new();
        // The objectives evaluate once per 60 Hz challenge tick, whatever the frame rate: a row's own counters (the
        // speech rows' `Delay` of 60 evaluations) keep their time. [inferred: retail's evaluation rate is open, b1006]
        state.eval_carry += time.delta_secs() * TICKS_PER_SECOND;
        let evaluations = (state.eval_carry as u32).min(MAX_EVALUATIONS_PER_FRAME);
        state.eval_carry -= state.eval_carry.floor();
        if let Some(o) = state.objectives.as_mut() {
            for &v in &exited {
                events.extend(o.handle(GroupMessage::TriggerExit { volume: v }));
            }
            for &v in &entered {
                events.extend(o.handle(GroupMessage::TriggerEnter { volume: v }));
            }
            for _ in 0..evaluations {
                events.extend(o.handle(GroupMessage::Tick { gated: false }));
            }
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
        for e in events {
            match e {
                GroupEvent::AllComplete => commands.extend(state.run.handle(Event::AllObjectivesComplete { killed: false })),
                GroupEvent::FailureCondition { objective, row } => {
                    let text = state.data.objectives.owned.get(objective).and_then(|o| o.inline_failure(row)).and_then(|f| f.text.clone());
                    info!("SKATE_FTL {id} objective {objective} failure row {row} ({text:?}) distances {distances:?}");
                    state.failure_text = Some(text.unwrap_or_else(|| FAILURE_TEXT.into()));
                    commands.extend(state.run.handle(Event::ObjectiveFailed));
                }
                // A hint row plays its presentation only (82C755B8, b942): the leader's speech lines and the start tip.
                GroupEvent::Hint { objective, row } => {
                    let events = state.data.objectives.owned.get(objective).and_then(|o| o.hints.get(row)).map(|h| h.presentation.events()).unwrap_or_default();
                    if let (Some(p), false) = (presenter.as_deref_mut(), events.is_empty()) {
                        let group = format!("{id}/obj{objective}/hint{row}");
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
                Command::ActivateStartObjectives => {
                    if let Some(o) = state.objectives.as_mut() {
                        o.activate_first();
                    }
                }
                Command::ResetObjectives => {
                    state.leaders_in_end.clear();
                    state.leader_path.clear();
                    state.failure_text = None;
                    if let Some(o) = state.objectives.as_mut() {
                        o.reset_only();
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
                        other => {
                            info!("SKATE_FTL {id} central message {other:?} NOT RETAIL YET");
                            continue;
                        }
                    };
                    central.write(crate::hud_central::CentralMessage::new(text));
                }
                Command::PlayHighlightReel => queue.extend(state.run.handle(Event::HighlightReelComplete)),
                Command::OpenContinueRetry => {
                    let won = state.run.result() == Flow::Win;
                    info!("SKATE_FTL {id} end options won={won}");
                    match end_dialog.as_deref_mut().filter(|d| d.available) {
                        Some(d) => {
                            d.dialog.passed = won;
                            d.dialog.killed = false;
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
