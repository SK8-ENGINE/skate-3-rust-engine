//! The Hall of Meat challenges in play (doc 42 "Hall of Meat"): the `hall_of_meats` rows of challenges.json run on
//! skate-core's `HomRun` (the retail hom_challenge graph, research b1005). The rows share the films' row reader and
//! objective groups (`ftl_host.rs`); the run lives in the `FollowTheLeader` resource, so the sign-up prompt, the
//! player pin and the "a challenge runs" checks cover it. The bail score comes from `career_stats_host.rs`
//! (`Challenges::last_bail_score`, final at bail close).

use crate::challenge_objectives::{FeedInput, ObjectiveFeed};
use crate::ftl_host::{FollowTheLeader, FtlData, FtlGroup};
use crate::hud_challenge::{ChallengeHudState, Component};
use bevy::prelude::*;
use skate_core::challenges::hom_run::{Command, Event, HomRun, HudComponent, Params};
use skate_core::challenges::objectives::Event as GroupEvent;
use skate_core::challenges::timers::TICKS_PER_SECOND;
use skate_core::challenges::Completion;

pub(crate) struct HomState {
    pub data: FtlData,
    run: HomRun,
    groups: Vec<FtlGroup>,
    evaluating: bool,
    /// PinPlayer: held from the setup until the countdown releases it.
    pub pinned: bool,
    awaiting_teleport: bool,
    carry: f32,
    eval_carry: f32,
    feed: ObjectiveFeed,
    last_state: u32,
    bails_seen: u32,
    queued: Vec<Command>,
    dialog_open: bool,
}

fn volume_id(v: &Option<crate::challenges::Volume>) -> Option<u64> {
    v.as_ref().and_then(|v| u64::from_str_radix(&v.id, 16).ok())
}

/// The row's values over the graph's defaults. NOT RETAIL YET: the stored result (the replay rule ShowKilledItObj)
/// is not read from the career yet, so every run is a first attempt.
pub(crate) fn params(d: &FtlData) -> Params {
    let r = Params::retail();
    Params {
        time_limit: d.time_limit.filter(|t| *t > 0.0),
        disable_time_limit_for_single_player: d.disable_time_limit_for_single_player,
        boundary: volume_id(&d.boundary),
        has_killed_group: !d.objectives.killed.is_empty(),
        disable_hom: d.disable_hom,
        countdown_delay: d.countdown_delay.unwrap_or(r.countdown_delay),
        out_of_bounds_time: d.out_of_bounds_time.unwrap_or(r.out_of_bounds_time),
        owned_display_time: d.owned_display_time.unwrap_or(r.owned_display_time),
        killed_display_time: d.killed_display_time.unwrap_or(r.killed_display_time),
        ..r
    }
}

/// Loads the rows with the map; `SKATE_HOM=<id>` (tests) or a sign-up starts one when nothing else runs.
fn arm(mut ftl: ResMut<FollowTheLeader>, config: Res<crate::config::Config>, map: Res<crate::map_transition::CurrentMap>,
    mut volumes: ResMut<crate::trigger_volumes::TriggerVolumes>, mut signed: MessageReader<crate::challenges::ChallengeSignUp>,
    challenges: Option<Res<crate::challenges::Challenges>>, mut generation: Local<Option<u64>>) {
    let mut wanted = Vec::new();
    if *generation != Some(map.generation) {
        *generation = Some(map.generation);
        if ftl.hom_rows.is_empty() {
            ftl.hom_rows = crate::ftl_host::load_hall_of_meat(&config.asset_root);
            info!("SKATE_HOM rows={}", ftl.hom_rows.len());
        }
        if let Some(run) = ftl.hom.take() {
            info!("SKATE_HOM {} dropped by the map change", run.data.id);
        }
        wanted.extend(std::env::var("SKATE_HOM").ok());
    }
    let busy = challenges.as_deref().is_some_and(|c| c.run.is_some() || c.tutorial.is_some()) || ftl.run.is_some() || ftl.hom.is_some();
    for s in signed.read().filter(|s| s.body == crate::trigger_volumes::PLAYER) {
        if !ftl.hom_rows.iter().any(|r| r.id == s.challenge) {
            continue;
        }
        if busy {
            info!("SKATE_HOM sign-up {} ignored: a challenge is running", s.challenge);
            continue;
        }
        info!("SKATE_HOM sign-up {}", s.challenge);
        wanted.push(s.challenge.clone());
    }
    let Some(id) = wanted.first() else { return };
    let Some(data) = ftl.hom_rows.iter().find(|r| &r.id == id).cloned() else {
        info!("SKATE_HOM {id} not found");
        return;
    };
    if data.map.as_deref().is_some_and(|m| map.path.as_deref().is_some_and(|p| !crate::teleport_menu::same_map(p, m))) {
        info!("SKATE_HOM {id} is not on {}", map.name);
        return;
    }
    for v in data.boundary.iter().chain(&data.objective_volumes) {
        if volumes.get(&v.id).is_none() {
            volumes.map.push(crate::trigger_volumes::TriggerVolume::from_record(&v.volume));
        }
    }
    let map_volumes: Vec<(String, u64)> =
        volumes.map.iter().filter_map(|v| Some((v.name.clone(), u64::from_str_radix(&v.id, 16).ok()?))).collect();
    let Some(groups) = crate::ftl_host::load_groups(&data, map_volumes) else {
        warn!("SKATE_HOM {id}: its owned objectives do not load; not started");
        return;
    };
    let mut run = HomRun::new(params(&data));
    let queued = run.start();
    info!("SKATE_HOM start {id} at {} groups={} limit={:?} boundary={:?}", data.start.locator, groups.len(),
        data.time_limit, data.boundary.as_ref().map(|v| &v.name));
    let bails_seen = challenges.as_deref().map_or(0, |c| c.bails_closed);
    ftl.hom = Some(HomState { data, run, groups, evaluating: false, pinned: false, awaiting_teleport: false, carry: 0.0,
        eval_carry: 0.0, feed: ObjectiveFeed::default(), last_state: 0, bails_seen, queued, dialog_open: false });
}

fn hud_component(c: HudComponent) -> Option<Component> {
    match c {
        HudComponent::Objective => Some(Component::Objective),
        HudComponent::Timer => Some(Component::Timer),
        HudComponent::Countdown => Some(Component::Countdown),
        HudComponent::OwnedIt => Some(Component::OwnedIt),
        HudComponent::KilledIt => Some(Component::KilledIt),
        // the minimap and trick display are the free-skate HUD's; the DPO bar is not hosted (NOT RETAIL YET)
        HudComponent::MiniMap | HudComponent::TrickDisplay | HudComponent::Dpo => None,
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
    time: Res<Time>,
    challenges: Option<Res<crate::challenges::Challenges>>,
    mut autopilot: Option<ResMut<crate::autopilot::Autopilot>>,
) {
    let entered: Vec<u64> = entered.read().filter(|e| e.body == crate::trigger_volumes::PLAYER)
        .filter_map(|e| u64::from_str_radix(&e.volume, 16).ok()).collect();
    let exited: Vec<u64> = exited.read().filter(|e| e.body == crate::trigger_volumes::PLAYER)
        .filter_map(|e| u64::from_str_radix(&e.volume, 16).ok()).collect();
    let Some(state) = ftl.hom.as_mut() else { return };
    let id = state.data.id.clone();
    let shared = challenges.as_deref();
    let mut commands = std::mem::take(&mut state.queued);
    // The end options dialog: its pick is Continue or Retry.
    if state.dialog_open {
        use skate_core::challenges::end_dialog::Action;
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
    // The player's volumes (the boundary) and the bail: Wipeout as it opens, WipeoutOver with its score once the
    // career host closed it (the score is final at close, b880 §1.1).
    for &v in &exited {
        commands.extend(state.run.handle(Event::TriggerExit { volume: v }));
    }
    for &v in &entered {
        commands.extend(state.run.handle(Event::TriggerEnter { volume: v }));
    }
    let state_id = skater.physical_state_id();
    if state_id == 300 && state.last_state != 300 {
        commands.extend(state.run.handle(Event::Wipeout));
    }
    state.last_state = state_id;
    if let Some(c) = shared.filter(|c| c.bails_closed != state.bails_seen) {
        state.bails_seen = c.bails_closed;
        let score = c.last_bail_score.map_or(0, |s| s.total);
        info!("SKATE_HOM {id} bail closed: score {score}");
        commands.extend(state.run.handle(Event::WipeoutOver { score: score as i32 }));
    }
    if shared.is_some_and(|c| c.markers_this_frame.contains(&true)) {
        commands.extend(state.run.handle(Event::UseMarker));
    }
    // The objective groups: the shared feed, one evaluation per 60 Hz tick.
    state.eval_carry += time.delta_secs() * TICKS_PER_SECOND;
    let ticks = state.eval_carry as u32;
    state.eval_carry -= state.eval_carry.floor();
    let banked: Vec<(i32, bool)> = shared.map(|c| c.banked_this_frame.iter().map(|s| (s.score, s.wiped_out)).collect()).unwrap_or_default();
    let input = FeedInput {
        entered: &entered, exited: &exited, banked: &banked, ticks, state_id, in_sequence: skater.scoring.sequence_active(),
        markers: shared.map_or(&[][..], |c| &c.markers_this_frame), tricks: shared.map_or(&[][..], |c| &c.tricks_done_this_frame),
        began: shared.map_or(&[][..], |c| &c.tricks_began_this_frame), manual: skater.scoring.manual(),
        pushes: shared.map_or(0, |c| c.pushes_this_frame),
        wipeout_over: skater.player_input.physical.skeleton.over_599 != 0,
        bail_pose: shared.and_then(|c| c.last_bail_pose), bail_score: shared.and_then(|c| c.last_bail_score).map_or(0, |s| s.total),
        bail: shared.map(|c| c.last_bail).unwrap_or_default(),
        ..Default::default()
    };
    let killed_of: Vec<bool> = state.groups.iter().map(|g| g.killed).collect();
    let mut refs: Vec<&mut crate::challenge_objectives::DataObjectives> = state.groups.iter_mut().map(|g| &mut g.objectives).collect();
    let fed = state.feed.step(&mut refs, &input, state.evaluating);
    for (i, e) in fed {
        let killed = killed_of[i];
        match e {
            GroupEvent::Completed(o) => {
                info!("SKATE_HOM {id} objective {o} complete (killed it group: {killed})");
                commands.extend(state.run.handle(Event::ObjectiveComplete));
            }
            GroupEvent::AllComplete => {
                info!("SKATE_HOM {id} all objectives complete (killed it group: {killed})");
                commands.extend(state.run.handle(Event::AllObjectivesComplete { killed }));
            }
            _ => {}
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
    let mut queue: std::collections::VecDeque<Command> = commands.into();
    while let Some(c) = queue.pop_front() {
        guard += 1;
        if guard > 256 {
            warn!("SKATE_HOM {id}: command loop cut at 256");
            break;
        }
        match c {
            Command::PinPlayer(on) => state.pinned = on,
            Command::CreateHud => queue.extend(state.run.handle(Event::HudLoaded)),
            Command::EnterNis => queue.extend(state.run.handle(Event::NisComplete)),
            Command::StartPresentation(_) => queue.extend(state.run.handle(Event::PresentationFinished)),
            Command::RepositionToStart => {
                let at = state.data.start.slots.first().copied().unwrap_or(state.data.start.matrix);
                info!("SKATE_HOM {id} player to the start {:?}", &at[3][..3]);
                if let Err(e) = skater.travel_ex(at, None, !state.data.spawn_offboard) {
                    warn!("SKATE_HOM teleport {e}");
                }
                // the test autopilot re-plans from the start instead of steering back to where it was
                if let Some(a) = autopilot.as_mut() {
                    a.host_teleported = true;
                }
                state.awaiting_teleport = true;
            }
            Command::ResetObjectives => {
                for g in &mut state.groups {
                    g.objectives.reset_only();
                }
            }
            Command::Evaluation(on) => {
                // the run's start activates every objective (hom_challenge's flow mode 0, as the films', b934)
                if on && !state.evaluating {
                    for g in &mut state.groups {
                        g.objectives.activate_flow();
                    }
                }
                state.evaluating = on;
            }
            Command::ShowHud(c, shown) => match hud_component(c) {
                Some(c) => hud.show(c, shown),
                None => info!("SKATE_HOM {id} ShowHud({c:?}, {shown}) not hosted"),
            },
            Command::StartCountdownHud { .. } => hud.show(Component::Countdown, true),
            Command::StartTimerHud { .. } => hud.show(Component::Timer, true),
            Command::ReturnToChallengeArea => {
                central.write(crate::hud_central::CentralMessage::new(crate::challenges::RETURN_TO_AREA.to_string()));
            }
            Command::PlayHighlightReel => queue.extend(state.run.handle(Event::HighlightReelComplete)),
            Command::OpenContinueRetry => {
                let result = state.run.competitor().result;
                let (won, killed) = (result != Completion::Abandoned, result == Completion::Killed);
                info!("SKATE_HOM {id} end options won={won} killed={killed} best bail {}", state.run.competitor().highest_score);
                match end_dialog.as_deref_mut().filter(|d| d.available) {
                    Some(d) => {
                        d.dialog.passed = won;
                        d.dialog.killed = killed;
                        d.title = state.data.title_id.clone();
                        d.open = true;
                        state.dialog_open = true;
                    }
                    None => queue.extend(state.run.handle(Event::Continue)),
                }
            }
            Command::EndTeleport => {
                if let Some(at) = state.data.sign_up.location.as_ref().map(|l| l.matrix) {
                    if let Err(e) = skater.travel_ex(at, None, true) {
                        warn!("SKATE_HOM end teleport {e}");
                    }
                }
            }
            Command::Complete(c) => finished = Some(c),
            // NOT RETAIL YET (logged): the homscoring HUD group, the highlight reel, the DPO bar, ScoredLow's text,
            // the session marker set at the countdown, the pause menu switches.
            other => info!("SKATE_HOM {id} {other:?}"),
        }
    }
    if hud.is_shown(Component::Timer) {
        let (text, flag) = skate_core::challenges::timers::hud_text(state.run.time_left_ticks().unwrap_or(0));
        if hud.timer_text != text || hud.timer_flag != flag {
            hud.timer_text = text;
            hud.timer_flag = flag;
        }
    }
    if hud.is_shown(Component::Objective) {
        let states = state.groups.iter().find(|g| !g.killed).map(|g| g.objectives.states()).unwrap_or_default();
        let rows = crate::ftl_host::objective_rows(&state.data.objectives.owned, &states);
        if hud.objectives != rows {
            hud.objectives = rows;
        }
    }
    if let Some(c) = finished {
        let done = ftl.hom.take().expect("the run");
        info!("SKATE_HOM end {id} completion={c:?} best bail {}", done.run.competitor().highest_score);
        ftl.result = Some(format!("hom={id} completion={c:?}"));
        for h in [Component::Timer, Component::Countdown, Component::OwnedIt, Component::KilledIt, Component::Objective] {
            hud.show(h, false);
        }
        if c != Completion::Abandoned {
            career.write(crate::career::ChallengeResult { challenge: id, completion: c, reactivate: false });
        }
    }
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(PreUpdate, arm.after(crate::map_transition::MapTransitionSet).run_if(crate::challenges::opted_in))
        .add_systems(Update, update.after(crate::challenges::CareerResultSet).run_if(crate::challenges::opted_in));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(json: serde_json::Value) -> FtlData {
        let mut base = serde_json::json!({
            "id": "hom_dwtn_01_sp", "family": "hall_of_meats", "map": "downtown",
            "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}
        });
        base.as_object_mut().unwrap().extend(json.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn a_hall_of_meat_row_becomes_its_run_values() {
        let p = params(&row(serde_json::json!({"time_limit": 180.0, "disable_time_limit_for_single_player": true,
            "countdown_delay": 1.5, "owned_display_time": 3.1})));
        assert_eq!((p.time_limit, p.disable_time_limit_for_single_player, p.time_limited()), (Some(180.0), true, false),
            "single player counts up on a row that disables its limit");
        let timed = params(&row(serde_json::json!({"time_limit": 90.0})));
        assert!(timed.time_limited());
        assert_eq!(params(&row(serde_json::json!({"time_limit": 0.0}))).time_limit, None, "TimeLimit 0 is no limit");
        assert!(!params(&row(serde_json::json!({}))).has_killed_group);
    }
}
