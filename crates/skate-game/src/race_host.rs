//! Races in play, player first: a race from `private/challenges.json` `races` runs through
//! `skate_core::challenges::race_run` with the race tracker and its AI racers (`race_ai_host`). Start:
//! `SKATE_RACE=<id>` on that race's map. No race HUD movies or sign-up yet. Details: docs/hails-additions/42-challenges.md#change-races-in-play-race_hostrs
use crate::challenges::{Start, Volume};
use crate::hud_challenge::{ChallengeHudState, Component, DpoSlot, DpoValue, LeaderboardHud, LeaderboardRow, LEADERBOARD_NO_VARIANT, LEADERBOARD_RANK_NAME_TIME};
use skate_core::challenges::dpo::{self, DpoFormat};
use bevy::prelude::*;
use serde::Deserialize;
use skate_core::challenges::race::{Course, RaceTracker, RaceTuning};
use skate_core::challenges::race_run::{Banner, Command, Event, Params, RaceRun, State};
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
    /// The leg's gates in course order (minimap icons, ribbons; not splits).
    #[serde(default)]
    pub gates: Vec<Gate>,
}

/// A race gate: its trigger volume, and its ribbon (`ribbon_indicator` + `ribbon_spline_colour` row names, or
/// `Hash_<16 hex>`) as exported (research race-gate-indicators.md).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Gate {
    #[serde(flatten)]
    pub volume: Volume,
    #[serde(default)]
    pub ribbon: Option<GateRibbon>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct GateRibbon {
    pub indicator: String,
    #[serde(default)]
    pub colour: Option<String>,
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
    /// The race's title text id (the end dialog's Mission.GetTitle).
    #[serde(default)]
    pub title_id: Option<String>,
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
    // The AI competitors (kind 0, as `start` spawns them) and the player.
    p.competitors = race.competitors.iter().filter(|c| c.kind == 0).count() as u32 + 1;
    Ok(p)
}

pub(crate) struct RaceState {
    pub race: Race,
    pub run: RaceRun,
    pub tracker: RaceTracker,
    pub ai: Option<crate::race_ai_host::RaceAi>,
    tick_carry: f32,
    /// The local player's leaderboard name (`settings/player.json`).
    player_name: String,
    /// Seconds since the leaderboard rows were last refreshed.
    leaderboard_age: f32,
    /// The results screen's time and place, for the autopilot's result line.
    results: String,
    /// The leg whose gates show (ShowGates .. RemoveGates), how many of its gates lie behind the player, and whether
    /// the minimap group needs a refresh.
    gate_leg: Option<usize>,
    gates_passed: usize,
    gates_dirty: bool,
    /// The end dialog's results (build_results) and its pass flag (ShowWinHUD), waiting for `update` to open it; and
    /// whether the dialog is open for this race (its choice comes back as a race event).
    end_results: Option<(skate_core::challenges::results::ResultsView, bool)>,
    dialog_open: bool,
    /// Seconds the dialog has been open (the autopilot answers it after `AUTOPILOT_DIALOG_SECONDS`).
    dialog_seconds: f32,
    killed: bool,
}

#[derive(Resource, Default)]
pub(crate) struct Races {
    pub races: Vec<Race>,
    loaded: bool,
    generation: Option<u64>,
    pub run: Option<RaceState>,
    /// For the player autopilot (`autopilot.rs`): starts placed (TeleportToStart), the heat running (RaceStart to
    /// RaceStop), and the last finished race's result line.
    pub placements: u32,
    pub started: bool,
    pub result: Option<String>,
    /// The fe_map_icons records (read at the first race start) and the gate icon rules (retail defaults).
    map_icons: Option<std::collections::BTreeMap<String, skate_data::challenge_map::MapIcon>>,
    gate_icons: skate_core::challenges::race::GateIcons,
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
    if races.map_icons.is_none() {
        races.map_icons = Some(skate_data::collections::Collections::load(assets).map(|c| skate_data::challenge_map::map_icons(&c)).unwrap_or_default());
    }
    for v in race.heats.iter().flat_map(|h| h.legs.iter()).flat_map(|l| l.splits.iter().chain(l.gates.iter().map(|g| &g.volume))) {
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
    let player_name = crate::multiplayer::local_player_name(assets);
    races.run = Some(RaceState { race, run, tracker, ai, tick_carry: 0.0, player_name, leaderboard_age: 0.0, results: String::new(), gate_leg: None, gates_passed: 0, gates_dirty: false, end_results: None, dialog_open: false, dialog_seconds: 0.0, killed: false });
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
        Query<(&Transform, Option<&crate::living_world::npc_sim::NpcSim>)>,
        Query<&mut crate::living_world::npc_skaters::NpcRaceClock>,
        Res<crate::trigger_volumes::TriggerVolumes>,
        Commands,
    ),
    mut minimap: Option<ResMut<crate::hud_minimap::MinimapIcons>>,
    mut ribbons: Option<ResMut<crate::visual_indicators::RaceIndicators>>,
    mut end_dialog: Option<ResMut<crate::end_dialog_host::EndDialogScreen>>,
    autopilot: Option<Res<crate::autopilot::Autopilot>>,
) {
    let entered: Vec<u64> = entered.read().filter(|e| e.body == crate::trigger_volumes::PLAYER)
        .filter_map(|e| u64::from_str_radix(&e.volume, 16).ok()).collect();
    let Some(state) = races.run.as_mut() else { return };
    // Gate hits: the furthest gate of the shown leg the player entered (a skipped gate counts as passed).
    if let Some(gates) = state.gate_leg.and_then(|l| state.race.heats[0].legs.get(l)).map(|l| &l.gates) {
        for volume in &entered {
            if let Some(i) = gates.iter().position(|g| split_id(&g.volume) == Some(*volume)) {
                if i + 1 > state.gates_passed {
                    state.gates_passed = i + 1;
                    state.gates_dirty = true;
                }
            }
        }
    }
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
    // The end dialog: open it with the results, or continue at once without its movies; the player's choice comes back.
    if let Some(state) = races.run.as_mut() {
        if let Some((view, passed)) = state.end_results.take() {
            match end_dialog.as_deref_mut().filter(|d| d.available) {
                Some(d) => {
                    use skate_core::challenges::end_dialog::{CHANGE_DIFFICULTY, CONTINUE_RETRY, NONE, RETRY_QUIT};
                    d.dialog.passed = passed;
                    d.dialog.killed = state.killed;
                    // Slots: Continue_Retry, Retry_Quit, ChangeDifficulty after a loss (build_results@446-467).
                    d.dialog.slots = [CONTINUE_RETRY, RETRY_QUIT, if passed { NONE } else { CHANGE_DIFFICULTY }, NONE];
                    d.title = state.race.title_id.clone();
                    d.results = Some(view);
                    d.open = true;
                    d.choice = None;
                    state.dialog_open = true;
                    state.dialog_seconds = 0.0;
                    info!("SKATE_RACE {} end dialog {:?}", state.race.id, d.movie());
                }
                None => commands.extend(state.run.handle(Event::Continue)),
            }
        } else if state.dialog_open {
            use skate_core::challenges::end_dialog::Action;
            // The autopilot has no pad: it continues for the player after a short look at the dialog (ours, test runs).
            state.dialog_seconds += time.delta_secs();
            if autopilot.as_ref().is_some_and(|a| a.requested) && state.dialog_seconds > AUTOPILOT_DIALOG_SECONDS {
                if let Some(d) = end_dialog.as_deref_mut().filter(|d| d.choice.is_none()) {
                    info!("SKATE_RACE {} end dialog: the autopilot continues", state.race.id);
                    d.choice = Some(Action::Continue);
                    d.open = false;
                }
            }
            match end_dialog.as_deref_mut().and_then(|d| d.choice.take()) {
                Some(Action::Continue) => {
                    state.dialog_open = false;
                    commands.extend(state.run.handle(Event::Continue));
                }
                Some(Action::Retry { .. }) => {
                    state.dialog_open = false;
                    commands.extend(state.run.handle(Event::Retry));
                }
                _ => {}
            }
            if !state.dialog_open {
                if let Some(d) = end_dialog.as_deref_mut() {
                    d.results = None;
                }
            }
        }
    }
    let messages = apply(&mut races, &mut challenges, &mut skater, &mut hud, Some(&mut career), population.as_deref_mut(), commands);
    if races.run.as_ref().is_some_and(|s| s.gates_dirty) || races.run.is_none() {
        if let Some(ribbons) = ribbons.as_deref_mut() {
            gate_ribbons(&races, ribbons);
        }
    }
    if let Some(minimap) = minimap.as_deref_mut() {
        gate_icons(&mut races, minimap);
    }
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
        countdown_hud(&state.run, &mut hud);
    }
    if let Some(state) = races.run.as_mut() {
        state.leaderboard_age += time.delta_secs();
        if hud.is_shown(Component::Leaderboard) && state.leaderboard_age >= state.tracker.tuning.hud_refresh_seconds {
            state.leaderboard_age = 0.0;
            leaderboard_rows(&state.tracker, state.run.elapsed(), &mut hud);
        }
    }
}

/// The race's gate ribbons (`VisualIndicators.AddTriggerIndicator` at the countdown, research
/// race-gate-ribbon-anchor.md): every gate of the shown leg with its own ribbon, fitted to the gate box; the finish with
/// the final ribbon (one leg: red) or the leg finish one (green), replacing a gate on the same volume (keyed by volume,
/// the last add wins). Passed gates keep theirs (their Time_Bonus is 0). Cleared with the gates and at the race end.
fn gate_ribbons(races: &Races, ribbons: &mut crate::visual_indicators::RaceIndicators) {
    const GATE_INDICATOR: &str = "Hash_624E29026A97C02E";
    let leg = races.run.as_ref().and_then(|s| Some((s, s.gate_leg?))).and_then(|(s, l)| Some((s, l, s.race.heats[0].legs.get(l)?)));
    let Some((state, leg_index, leg)) = leg else {
        if !ribbons.0.is_empty() {
            ribbons.0.clear();
        }
        return;
    };
    let placement = |v: &Volume, key: &str, colour: Option<String>| {
        let b = v.volume.shape.oriented_box();
        let a = b.axes;
        crate::visual_indicators::Placement { key: key.to_owned(), locator: String::new(),
            matrix: [[a[0][0], a[0][1], a[0][2], 0.0], [a[1][0], a[1][1], a[1][2], 0.0], [a[2][0], a[2][1], a[2][2], 0.0],
                [b.center[0], b.center[1], b.center[2], 1.0]],
            style: None, colour, always: true, fit: Some(b.half_extents) }
    };
    let mut by_volume: Vec<(String, crate::visual_indicators::Placement)> = leg.gates.iter()
        .filter_map(|g| g.ribbon.as_ref().map(|r| (g.volume.id.clone(), placement(&g.volume, &r.indicator, r.colour.clone()))))
        .collect();
    if let Some(finish) = leg.splits.last() {
        let last_leg = leg_index + 1 >= state.race.heats[0].legs.len();
        let key = leg.gates.iter().find_map(|g| g.ribbon.as_ref()).map_or(GATE_INDICATOR.to_owned(), |r| r.indicator.clone());
        by_volume.retain(|(id, _)| *id != finish.id);
        by_volume.push((finish.id.clone(), placement(finish, &key, Some(if last_leg { "red" } else { "green" }.to_owned()))));
    }
    ribbons.0 = by_volume.into_iter().map(|(_, p)| p).collect();
    info!("RACE_RIBBONS leg {leg_index}: {} ribbons", ribbons.0.len());
}

/// The race's minimap group (`race`, retail's MiniMapManager group C): the next gates of the shown leg and the finish
/// (`skate_core::challenges::race::visible_gates`); gone when the gates are removed or the race ends.
fn gate_icons(races: &mut Races, minimap: &mut crate::hud_minimap::MinimapIcons) {
    const GROUP: &str = "race";
    let Some(state) = races.run.as_mut() else {
        minimap.remove(GROUP);
        return;
    };
    if !std::mem::take(&mut state.gates_dirty) {
        return;
    }
    let Some(leg) = state.gate_leg.and_then(|l| state.race.heats[0].legs.get(l)) else {
        minimap.remove(GROUP);
        return;
    };
    let records = races.map_icons.as_ref();
    let rules = &races.gate_icons;
    let icon = |id: String, record: &str, v: &Volume| {
        let r = records.and_then(|m| skate_data::challenge_map::map_icon(m, record))?;
        let c = centre(v);
        Some(skate_core::minimap::Icon { id, texture: r.texture.clone(), position: [c[0], c[2]], angle: 0.0, scale: 1.0, fade: true,
            max_distance: r.max_distance, colour: [1.0; 4] })
    };
    let (window, finish) = skate_core::challenges::race::visible_gates(state.gates_passed, leg.gates.len(), rules.max_icons);
    let mut icons: Vec<_> = window.filter_map(|i| icon(format!("{GROUP}:gate{i}"), &rules.gate, &leg.gates[i].volume)).collect();
    if finish {
        icons.extend(leg.splits.last().and_then(|v| icon(format!("{GROUP}:finish"), &rules.checkpoint, v)));
    }
    info!("RACE_MINIMAP gates passed {} of {}: {} icons (records {})", state.gates_passed, leg.gates.len(), icons.len(), records.map_or(0, |m| m.len()));
    minimap.set(GROUP, icons);
}

/// How long the end dialog shows before the autopilot continues (ours).
const AUTOPILOT_DIALOG_SECONDS: f32 = 3.0;

/// C.PlaceHigherToWin: the places that win the race (retail 1; end_heat@213-244).
const PLACE_TO_WIN: u32 = 1;

/// end_heat's RoundStandings input: the player and every AI racer, with finish times and estimates (names: the
/// player's leaderboard name, the racers' name ids).
fn standings(state: &RaceState) -> Vec<skate_core::challenges::results::Standing> {
    let entry = |id: u32, name: String, local: bool| {
        let r = state.tracker.racer(id);
        skate_core::challenges::results::Standing { id, name, local, finish: r.and_then(|r| r.finish_time), eta: r.map_or(0.0, |r| r.eta) }
    };
    let mut out = vec![entry(PLAYER, format!("#{}", state.player_name), true)];
    if let Some(ai) = state.ai.as_ref() {
        out.extend(ai.racers.iter().map(|r| entry(r.ai.id, r.name_id.clone(), false)));
    }
    out
}

/// A row's time: a finished racer's finish time, the leader's running race time, everyone else's gap behind the leader
/// (retail as the user describes it, 2026-11-09).
fn row_time(rank: i32, finish_time: Option<f32>, gap: f32, elapsed: f32) -> f32 {
    finish_time.unwrap_or(if rank == 1 { elapsed } else { gap })
}

/// A row's time text: a gap gets a leading `+` (retail's row text forces the sign for unfinished non-leader rows,
/// `8265DFD8` -> `sub_8265DDF8` flag 1); the leader's race time and finish times have none.
fn row_text(rank: i32, finished: bool, time: f32) -> String {
    let text = skate_core::challenges::dpo::time_text(time);
    if finished || rank == 1 { text } else { format!("+{text}") }
}

/// The tracker's standings into the leaderboard rows: rank and [`row_time`] (82560170 pushes rank and gap, b973 §6).
/// NOT RETAIL YET: the row's time text (data-block native 8265E510's sprintf) is not decoded; it uses the DPO time
/// text (b997 §4).
fn leaderboard_rows(tracker: &RaceTracker, elapsed: f32, hud: &mut ChallengeHudState) {
    let mut next = hud.leaderboard.rows.clone();
    for row in &mut next {
        let Some(r) = tracker.racer(row.id) else { continue };
        if r.rank > 0 {
            row.rank = r.rank;
        }
        let score = row_time(r.rank, r.finish_time, r.gap, elapsed);
        row.score = score;
        row.score_text = row_text(r.rank, r.finish_time.is_some(), score);
    }
    if next != hud.leaderboard.rows {
        hud.leaderboard.rows = next;
    }
}

/// The 3-2-1-GO (b995 item 4): the whole seconds left on the run's HUD countdown, GO (0) from the start until
/// `go_display_time` into the race, then hidden. Writes only on a change.
fn countdown_hud(run: &RaceRun, hud: &mut ChallengeHudState) {
    let seconds = match run.countdown_hud_ticks() {
        Some(ticks) => Some((ticks.max(0) + TICKS_PER_SECOND as i32 - 1) / TICKS_PER_SECOND as i32),
        None if hud.is_shown(Component::Countdown) && run.state() == State::RunHeat && run.elapsed() < run.params().go_display_time => Some(0),
        None => None,
    };
    match seconds {
        Some(s) => {
            if hud.countdown != s {
                hud.countdown = s;
            }
            if !hud.is_shown(Component::Countdown) {
                hud.show(Component::Countdown, true);
            }
        }
        None if hud.is_shown(Component::Countdown) => hud.show(Component::Countdown, false),
        None => {}
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
    commands: Vec<Command>,
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
                races.placements += 1;
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
                races.started = true;
                if let Some(ai) = state.ai.as_mut() {
                    ai.go();
                }
            }
            Command::RaceStop => {
                races.started = false;
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
            // setup_heat's DPOs (b995 items 2a / 2b): the KilledIt time first when the heat has one, then the position.
            Command::DpoKilledItTime(t) => {
                hud.dpo[0] = DpoSlot { title: "ID_CHALLENGE_RACE_KILLED_IT_TIME".into(), icon: dpo::icon::KILLED_IT,
                    value: Some(DpoValue { format: DpoFormat::Time.value(), numerator: 0.0, denominator: t }), ..Default::default() };
                hud.two_dpos = true;
            }
            Command::DpoPosition { of } => {
                let slot = usize::from(hud.two_dpos);
                hud.dpo[slot] = DpoSlot { title: "ID_DPO_POSITION".into(), icon: dpo::icon::CHALLENGE,
                    value: Some(DpoValue { format: DpoFormat::Ints.value(), numerator: 1.0, denominator: of as f32 }), ..Default::default() };
                hud.show(Component::Dpo, true);
            }
            Command::RacePassTimerUpdate => {
                let place = state.tracker.rank(PLAYER);
                let slot = usize::from(hud.two_dpos);
                if let Some(v) = hud.dpo[slot].value.as_mut().filter(|_| place > 0) {
                    if v.numerator != place as f32 {
                        v.numerator = place as f32;
                    }
                }
            }
            Command::DpoHide => {
                hud.show(Component::Dpo, false);
                hud.two_dpos = false;
            }
            Command::RaceFinishedMessage => messages.push(crate::hud_central::CentralMessage::new(RACE_FINISHED)),
            Command::TimeUpMessage => messages.push(crate::hud_central::CentralMessage::new(CHALLENGE_FAILED)),
            Command::Banner(Banner::OwnedIt, shown) => hud.show(Component::OwnedIt, shown),
            Command::Banner(Banner::KilledIt, shown) => {
                state.killed |= shown;
                hud.show(Component::KilledIt, shown);
            }
            Command::Banner(Banner::Failed, _) => {}
            Command::RemoveGates => {
                hud.show(Component::Timer, false);
                (state.gate_leg, state.gates_dirty) = (None, true);
            }
            Command::ShowGates { leg } => (state.gate_leg, state.gates_passed, state.gates_dirty) = (Some(leg), 0, true),
            Command::Leaderboard(false) => {
                hud.show(Component::Timer, false);
                hud.show(Component::Leaderboard, false);
            }
            // setup_heat: the standings, one row per racer, shown until cleanup_heat (b995 item 1).
            Command::Leaderboard(true) => {
                let mut rows = vec![LeaderboardRow { id: PLAYER, rank: 1, score: -1.0, name: format!("#{}", state.player_name), highlight: true,
                    ..Default::default() }];
                if let Some(ai) = state.ai.as_ref() {
                    rows.extend(ai.racers.iter().map(|r| LeaderboardRow { id: r.ai.id, rank: r.ai.id as i32 + 1, score: -1.0,
                        name: r.name_id.clone(), ..Default::default() }));
                }
                hud.leaderboard = LeaderboardHud { layout: LEADERBOARD_RANK_NAME_TIME, title: String::new(),
                    variant: LEADERBOARD_NO_VARIANT.into(), rows };
                hud.show(Component::Leaderboard, true);
                state.leaderboard_age = 0.0;
            }
            Command::SetBestTime(t) => {
                info!("SKATE_RACE {id} best time {t:.2}");
                changed |= challenges.records.update(&id, |s| s.best_time = Some(t));
            }
            // The results screen is not ported: Continue at once (as the end dialog does without its movie).
            Command::Results { time, place } => {
                info!("SKATE_RACE {id} results time={time:?} place={place:?}: continue");
                // The record's completion is 0 for a lost race as well as an abandoned one (retail has no "lost" value).
                let outcome = match (time, place) {
                    (None, _) => "time up",
                    (Some(_), Some(1)) => "won",
                    _ => "lost",
                };
                state.results = format!("outcome={outcome} time={} place={}", time.map_or("none".into(), |t| format!("{t:.2}")), place.map_or("none".into(), |p| p.to_string()));
                // build_results (research race-results-rows.md): every racer, finish order, then the end dialog
                // (opened by `update`; without the dialog movies the run continues at once).
                let standings = standings(state);
                let view = skate_core::challenges::results::race_results(&standings, state.run.elapsed(), false, PLACE_TO_WIN,
                    &skate_core::challenges::results::ResultsRules::default());
                let passed = place.is_some_and(|p| p <= PLACE_TO_WIN) && time.is_some();
                info!("SKATE_RACE {id} results table: {} rows, self rank {}, passed {passed}", view.rows.len(), view.self_rank());
                state.end_results = Some((view, passed));
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
        races.started = false;
        races.result = Some(format!("race={id} completion={} {}", crate::challenges::completion_name(completion),
            races.run.as_ref().map_or(String::new(), |r| r.results.clone())));
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

    /// The countdown HUD shows 3, 2, 1 while the run's HUD countdown runs, GO (0) from the start for the GO display time,
    /// then hides; nothing during the countdown delay.
    #[test]
    fn the_leader_row_shows_the_race_time_the_others_their_gap() {
        assert_eq!(row_time(1, None, 0.0, 42.5), 42.5);
        assert_eq!(row_time(2, None, 8.47, 42.5), 8.47);
        assert_eq!(row_time(1, Some(70.38), 0.0, 75.0), 70.38);
        assert_eq!(row_time(3, Some(81.0), 5.0, 90.0), 81.0);
        assert!(row_text(2, false, 8.47).starts_with('+'));
        assert!(!row_text(1, false, 42.5).starts_with('+'));
        assert!(!row_text(2, true, 81.0).starts_with('+'));
    }

    #[test]
    fn the_countdown_hud_counts_three_two_one_go_then_hides() {
        let mut run = RaceRun::new(Params::new(vec![vec![1]], 120.0, 30.0, 0.0), 0.0);
        run.start();
        let mut hud = ChallengeHudState::default();
        let mut seen = Vec::new();
        for _ in 0..(60 * 7) {
            run.tick();
            countdown_hud(&run, &mut hud);
            let now = hud.is_shown(Component::Countdown).then_some(hud.countdown);
            if seen.last() != Some(&now) {
                seen.push(now);
            }
        }
        assert_eq!(seen, vec![None, Some(3), Some(2), Some(1), Some(0), None]);
    }
}
