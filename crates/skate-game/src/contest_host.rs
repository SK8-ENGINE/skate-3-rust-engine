//! Contests in play (street / vert contests, demos), player first: a contest from `private/challenges.json` `contests`
//! runs through `skate_core::challenges::contest_run`. Start: the sign-up prompt at the contest's sign-up sphere
//! (challenges.rs), or `SKATE_CONTEST=<id>` on that contest's map. Details: docs/hails-additions/42-challenges.md#contests
use bevy::prelude::*;
use serde::Deserialize;
use crate::hud_challenge::{ChallengeHudState, Component, LeaderboardHud, LeaderboardRow, LEADERBOARD_NO_VARIANT,
    LEADERBOARD_RANK_NAME_FLOATSCORE, LEADERBOARD_RANK_NAME_SCORE};
use skate_core::challenges::contest_run::{Command, ContestRun, Event, Params, Round, RunType, State};
use skate_core::challenges::timers::TICKS_PER_SECOND;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ContestRound {
    #[serde(default)]
    pub run_type: i32,
    /// eContestType (the round's scorer: Sequence 0 .. Grind_Only 16).
    #[serde(default)]
    pub variant: i32,
    #[serde(default)]
    pub round_time: i32,
    #[serde(default = "one")]
    pub ai_adjustment: f32,
    #[serde(default)]
    pub cumulative: bool,
    #[serde(default)]
    pub cumulative_min: i32,
    #[serde(default = "one_i")]
    pub solo_runs: i32,
    #[serde(default)]
    pub solo_ai_min: i32,
    #[serde(default)]
    pub solo_ai_max: i32,
    #[serde(default)]
    pub demo_target: Option<i32>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub start_matrix: Option<[[f32; 4]; 4]>,
    /// Competitor n's start (child n of the start locator; the player is 0): setup_round's
    /// AI.RepositionAI(id, StartLocation, id).
    #[serde(default)]
    pub start_slots: Vec<[[f32; 4]; 4]>,
    /// ResultsScreenHALIDCol1..3: the results tables' column titles.
    #[serde(default)]
    pub result_columns: Vec<String>,
    /// The round's `Paths` (AI.FilterPaths): which lines its riders may take.
    #[serde(default)]
    pub ai_paths: RoundPathsJson,
    /// RoundBoundary (inBoundary) and SpotVolumes (hitSpotVol), with their shapes.
    #[serde(default)]
    pub boundary_volume: Option<crate::challenges::Volume>,
    #[serde(default)]
    pub spot_volume_shapes: Vec<crate::challenges::Volume>,
}

/// `ai_paths` as exported: `{names, all_paths, param}`, or a bare name list (exports before 2026-11-11, which did not
/// keep the FilterDisabled flag; taken as filter on).
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum RoundPathsJson {
    Full {
        #[serde(default)]
        names: Vec<String>,
        #[serde(default = "yes")]
        all_paths: bool,
        #[serde(default)]
        param: u32,
    },
    Names(Vec<String>),
}

impl Default for RoundPathsJson {
    fn default() -> Self {
        Self::Names(Vec::new())
    }
}

fn yes() -> bool {
    true
}

impl RoundPathsJson {
    pub(crate) fn paths(&self) -> skate_core::challenges::path_filter::RoundPaths {
        use skate_core::challenges::path_filter::RoundPaths;
        match self {
            Self::Full { names, all_paths, param } => RoundPaths { names: names.clone(), all_paths: *all_paths, param: *param },
            Self::Names(names) => RoundPaths { names: names.clone(), all_paths: names.is_empty(), param: 0 },
        }
    }
}

fn one() -> f32 {
    1.0
}
fn one_i() -> i32 {
    1
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Contest {
    pub id: String,
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub demo: bool,
    /// Mission.GetTitle on the results dialog: the localized title from setup, else its text id.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub title_id: Option<String>,
    #[serde(default)]
    pub competitors: Vec<crate::race_host::CompetitorRow>,
    #[serde(default)]
    pub music_playlist: Option<String>,
    #[serde(flatten)]
    pub sign_up: crate::challenges::SignUpFields,
    pub rounds: Vec<ContestRound>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    contests: Vec<Contest>,
}

pub(crate) fn load(assets: &std::path::Path) -> Result<Vec<Contest>, String> {
    let path = assets.join("private/challenges.json");
    let file: File = serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(file.contests)
}

/// The contest's values for the run (eRunType 0 normal, 1 SoloRun, 3 TimedRunSingle; 4 TimedRunMulti and 5 run as
/// normal rounds: NOT RETAIL YET).
pub(crate) fn params(contest: &Contest, seed: u32) -> Params {
    let rounds = contest.rounds.iter().map(|r| Round {
        round_time: r.round_time.max(1) as f32,
        run_type: match r.run_type {
            1 => RunType::SoloRun { runs: r.solo_runs.max(1) as u32, ai_min: r.solo_ai_min as f32, ai_max: r.solo_ai_max as f32 },
            3 => RunType::TimedRunSingle,
            _ => RunType::Normal,
        },
        cumulative: r.cumulative,
        cumulative_min: r.cumulative_min as f32,
        ai_adjustment: r.ai_adjustment,
        demo_target: r.demo_target.map(|t| t as f32),
    }).collect();
    let mut p = Params::new(rounds, if contest.demo { 0 } else { contest.competitors.len() });
    p.demo = contest.demo;
    p.seed = seed;
    p
}

/// setup_round's UseIntScore (contest_shared setup_round.stateenter@742-785): Sequence and its trick types (0..6),
/// Air_Spin 12, Best_Line 14, Air_Only 15, Grind_Only 16 score in whole points (layout RankNameScore); the distance,
/// height, time, length and speed variants in floats (RankNameFloatscore). Enum values by the image's string order
/// (b877; the same order gives eRunType's values).
pub(crate) fn uses_int_score(variant: i32) -> bool {
    matches!(variant, 0..=6 | 12 | 14..=16)
}

/// The leaderboard rows' places: round score descending, a tie keeps the place above (end_round's PointSorter).
fn round_ranks(scores: &[f32]) -> Vec<i32> {
    scores.iter().map(|s| 1 + scores.iter().filter(|o| *o > s).count() as i32).collect()
}

/// The AI competitors' scores until AI riders exist. NOT RETAIL (retail's AI riders land their own tricks, native
/// code not read; research b1009 §8): one second before a normal round ends each AI scores the player's round score
/// times a seeded factor in this range. A mod can replace the range.
#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct AiScoreStandIn {
    pub min: f32,
    pub max: f32,
}

impl Default for AiScoreStandIn {
    fn default() -> Self {
        Self { min: 0.7, max: 1.3 }
    }
}

pub(crate) struct ContestState {
    pub contest: Contest,
    pub run: ContestRun,
    carry: f32,
    rng: u32,
    ai_scored: bool,
    /// The leaderboard names: the player's, then each competitor's look name id.
    names: Vec<String>,
    go_shown: f32,
    /// The AI competitors riding the contest's lines (none for a demo or without exported lines).
    ai: Option<crate::contest_ai_host::ContestAi>,
    /// The round results dialog is open (the run waits for its Continue).
    dialog_open: bool,
    dialog_seconds: f32,
    /// Per competitor (player 0): run_round's volume state, reset at setup_round.
    gates: Vec<skate_core::challenges::contest_run::Gates>,
    /// Per competitor: inside the round boundary, and which spot volumes hold it (bit per volume), last tick.
    inside: Vec<(bool, u64)>,
    wrong: skate_core::challenges::contest_run::WrongTricks,
    player_sequence: bool,
}

#[derive(Resource, Default)]
pub(crate) struct Contests {
    pub contests: Vec<Contest>,
    loaded: bool,
    generation: Option<u64>,
    pub run: Option<ContestState>,
    /// Mission music triggers with the contest's playlist key (`game_audio::music_zones`).
    pub music: Vec<(u8, Option<String>)>,
    /// The last finished contest's result line (tests, the autopilot).
    pub result: Option<String>,
    pending: Vec<Command>,
}

impl Contests {
    /// The contests and demos on `map` with a sign-up sphere, for the sign-up prompt (challenges.rs).
    pub(crate) fn offers<'a>(&'a self, map: Option<&'a std::path::Path>) -> impl Iterator<Item = crate::challenges::Offer> + 'a {
        self.contests.iter().filter_map(move |c| c.sign_up.offer(&c.id, if c.demo { "demos" } else { "contests" }, c.map.as_deref(),
            c.title_id.as_ref(), c.title.as_ref(), map))
    }
}

/// A new map: read the contests once, drop a run from the old map, start `SKATE_CONTEST` if it is on this one; a
/// sign-up (the prompt at the contest's sign-up sphere) starts its contest when nothing else runs.
fn arm(mut contests: ResMut<Contests>, config: Res<crate::config::Config>, map: Res<crate::map_transition::CurrentMap>,
    mut population: Option<ResMut<crate::living_world::PopulationState>>,
    mut signed: MessageReader<crate::challenges::ChallengeSignUp>, challenges: Option<Res<crate::challenges::Challenges>>) {
    let mut wanted = None;
    if contests.generation != Some(map.generation) {
        contests.generation = Some(map.generation);
        if !contests.loaded {
            contests.loaded = true;
            contests.contests = load(&config.asset_root).unwrap_or_else(|e| {
                info!("SKATE_CONTESTS none ({e})");
                Vec::new()
            });
        }
        if let Some(mut run) = contests.run.take() {
            info!("SKATE_CONTEST {} dropped by the map change", run.contest.id);
            if let (Some(ai), Some(pop)) = (run.ai.as_mut(), population.as_deref_mut()) {
                ai.remove(pop);
            }
        }
        wanted = std::env::var("SKATE_CONTEST").ok();
    }
    let busy = contests.run.is_some() || challenges.as_deref().is_some_and(|c| c.run.is_some() || c.tutorial.is_some());
    for s in signed.read().filter(|s| s.body == crate::trigger_volumes::PLAYER && contests.contests.iter().any(|c| c.id == s.challenge)) {
        if busy {
            info!("SKATE_CONTEST sign-up {} ignored: a challenge is running", s.challenge);
        } else {
            info!("SKATE_CONTEST sign-up {}", s.challenge);
            wanted.get_or_insert_with(|| s.challenge.clone());
        }
    }
    let Some(wanted) = wanted else { return };
    let Some(contest) = contests.contests.iter().find(|c| c.id == wanted).cloned() else {
        info!("SKATE_CONTEST {wanted} not found");
        return;
    };
    if contest.map.as_deref().is_some_and(|m| map.path.as_deref().is_some_and(|p| !crate::teleport_menu::same_map(p, m))) {
        info!("SKATE_CONTEST {wanted} is not on {}", map.name);
        return;
    }
    let seed = (map.generation as u32).wrapping_mul(2_654_435_761) | 1;
    let mut run = ContestRun::new(params(&contest, seed));
    info!("SKATE_CONTEST start {} rounds={} ai={} demo={}", contest.id, contest.rounds.len(), contest.competitors.len(), contest.demo);
    let commands = run.start();
    let looks = crate::race_ai_host::looks(&config.asset_root);
    let mut names = vec![format!("#{}", crate::multiplayer::local_player_name(&config.asset_root))];
    names.extend(contest.competitors.iter().map(|c| looks.get(&c.character).map_or_else(|| format!("#{}", c.character), |l| l.1.clone())));
    let ai = if contest.demo { None } else {
        let rows: Vec<String> = contest.competitors.iter().map(|c| c.character.clone()).collect();
        crate::contest_ai_host::ContestAi::load(&config.asset_root, &contest.id, &rows, crate::race_ai_host::start_seed(map.generation))
    };
    let n = contest.competitors.len() + 1;
    contests.run = Some(ContestState { contest, run, carry: 0.0, rng: seed, ai_scored: false, names, go_shown: 0.0, ai, dialog_open: false, dialog_seconds: 0.0,
        gates: vec![Default::default(); n], inside: vec![(true, 0); n], wrong: Default::default(), player_sequence: false });
    contests.pending = commands;
}

impl Contests {
    /// The player is pinned at its start slot: setup_round pins every competitor, run_round unpins them and end_round
    /// pins them again until the contest's cleanup (contest_shared, b877).
    pub(crate) fn player_pinned(&self) -> bool {
        self.run.as_ref().is_some_and(|r| !matches!(r.run.state(), State::RunRound | State::EndChallenge | State::EndOptions | State::Complete))
    }

    fn take_pending(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.pending)
    }
}

fn update(
    mut contests: ResMut<Contests>,
    challenges: Res<crate::challenges::Challenges>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
    stand_in: Res<AiScoreStandIn>,
    (mut npc_events, npc_transforms, mut central): (
        MessageReader<crate::living_world::npc_skaters::NpcSkaterEvent>,
        Query<&Transform, With<crate::living_world::npc_skaters::NpcSkater>>,
        MessageWriter<crate::hud_central::CentralMessage>,
    ),
    mut hud: ResMut<ChallengeHudState>,
    mut end_dialog: Option<ResMut<crate::end_dialog_host::EndDialogScreen>>,
    mut autopilot: Option<ResMut<crate::autopilot::Autopilot>>,
    time: Res<Time>,
    (mut population, index, mut clocks, mut commands_ecs): (
        Option<ResMut<crate::living_world::PopulationState>>,
        Option<Res<crate::living_world::npc_skaters::NpcSkaterIndex>>,
        Query<&mut crate::living_world::npc_skaters::NpcRaceClock>,
        Commands,
    ),
) {
    let mut commands = contests.take_pending();
    let Some(state) = contests.run.as_mut() else { return };
    if state.dialog_open {
        use skate_core::challenges::end_dialog::Action;
        state.dialog_seconds += time.delta_secs();
        // The autopilot has no pad: it continues for the player after a short look (ours, test runs).
        if autopilot.as_ref().is_some_and(|a| a.requested) && state.dialog_seconds > AUTOPILOT_DIALOG_SECONDS {
            if let Some(d) = end_dialog.as_deref_mut().filter(|d| d.choice.is_none() && d.open) {
                info!("SKATE_CONTEST {} round results: the autopilot continues", state.contest.id);
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
                    (d.results, d.other) = (None, None);
                    state.dialog_open = false;
                    commands.extend(state.run.handle(event));
                }
            }
            None => state.dialog_open = false,
        }
    }
    // run_round's volume and sequence handlers, then sequencecomplete with its gates (b1009 §2): every competitor's
    // banked line (the player's from the scoring runtime, a rider's own NpcSkaterEvent) is gated, then
    // UpdateScore(CIndex, points) (the run applies AIScoreAdjustment). A bailed line scores nothing.
    use crate::living_world::npc_skaters::NpcSkaterEvent;
    use skate_core::challenges::contest_run::{gate_line, wrong_type_modifier, Gate, LineFacts};
    let round = state.contest.rounds.get(state.run.round().saturating_sub(1));
    let variant = round.map_or(0, |r| r.variant);
    let running = state.run.state() == State::RunRound;
    let npc_list: Vec<NpcSkaterEvent> = npc_events.read().cloned().collect();
    let rider_of = |state: &ContestState, id| state.ai.as_ref().and_then(|ai| ai.riders.iter().find(|r| r.npc == Some(id))).map(|r| r.id as usize);
    // Positions (hips height; [inferred]: retail tests the skater's own trigger body): the player, then each rider.
    let mut points: Vec<Option<[f32; 3]>> = vec![None; state.gates.len()];
    if let Some(p) = points.get_mut(0) {
        *p = Some(crate::trigger_volumes::player_points(&skater)[2]);
    }
    if let (Some(ai), Some(index)) = (state.ai.as_ref(), index.as_deref()) {
        for r in &ai.riders {
            let at = r.npc.and_then(|id| index.0.get(&id)).and_then(|e| npc_transforms.get(*e).ok()).map(|t| t.translation);
            if let (Some(t), Some(p)) = (at, points.get_mut(r.id as usize)) {
                *p = Some([t.x, t.y + 1.0, t.z]);
            }
        }
    }
    if let (true, Some(round)) = (running, round) {
        let boundary = round.boundary_volume.as_ref().map(|v| v.volume.shape.oriented_box());
        let spots: Vec<_> = round.spot_volume_shapes.iter().map(|v| v.volume.shape.oriented_box()).collect();
        for (c, at) in points.iter().enumerate() {
            let Some(at) = *at else { continue };
            let (was_in, was_mask) = state.inside[c];
            if let Some(b) = &boundary {
                let now_in = b.contains_point(at);
                if now_in != was_in {
                    state.gates[c].boundary(now_in);
                    if c == 0 {
                        info!("SKATE_CONTEST player {} the round boundary", if now_in { "entered" } else { "left" });
                    }
                }
                state.inside[c].0 = now_in;
            }
            let mut mask = 0u64;
            for (i, s) in spots.iter().enumerate().take(64) {
                if s.contains_point(at) {
                    mask |= 1 << i;
                }
            }
            if variant != skate_core::challenges::contest_run::variant::BEST_LINE {
                for _ in 0..(mask & !was_mask).count_ones() {
                    state.gates[c].enter_spot();
                }
                for _ in 0..(was_mask & !mask).count_ones() {
                    state.gates[c].exit_spot();
                }
            }
            state.inside[c].1 = mask;
        }
    }
    // sequencebegin: the player's scoring runtime opening a sequence, a rider's SequenceBegin.
    let player_active = skater.scoring.sequence_active();
    if running && player_active && !state.player_sequence {
        state.gates[0].sequence_begin(variant);
    }
    state.player_sequence = player_active;
    // The lines: (competitor, points, tricks, flipped).
    let mut lines: Vec<(usize, f32, [u16; crate::scoring_runtime::SEQUENCE_CATEGORIES], bool)> =
        challenges.banked_this_frame.iter().filter(|s| !s.wiped_out).map(|s| (0, s.score as f32, s.tricks, s.flipped)).collect();
    for e in &npc_list {
        match e {
            NpcSkaterEvent::SequenceBegin { id } if running => {
                if let Some(c) = rider_of(state, *id).filter(|c| *c < state.gates.len()) {
                    state.gates[c].sequence_begin(variant);
                }
            }
            NpcSkaterEvent::SequenceComplete { id, score, wiped_out: false, tricks, flipped } => {
                if let Some(c) = rider_of(state, *id) {
                    lines.push((c, *score as f32, *tricks, *flipped));
                }
            }
            _ => {}
        }
    }
    let spot_count = round.map_or(0, |r| r.spot_volume_shapes.len());
    for (c, raw, tricks, flipped) in lines {
        let gates = state.gates.get(c).copied().unwrap_or_default();
        let gate = gate_line(variant, spot_count, &gates, LineFacts { tricks: &tricks[..], flipped });
        let points = if gate == Gate::Scored { raw } else { 0.0 };
        if c == 0 && running && state.wrong.line(gate, points) {
            info!("SKATE_CONTEST wrong trick type warning (ID_CONTEST_WRONG_TYPE {}, speech 307)", wrong_type_modifier(variant));
            // NOT RETAIL YET: what the modifier does to the text (82850770's second argument) and speech line 307.
            central.write(crate::hud_central::CentralMessage::modified("ID_CONTEST_WRONG_TYPE"));
        }
        if c > 0 || gate != Gate::Scored {
            info!("SKATE_CONTEST competitor {c} line {raw} -> {points} ({gate:?})");
        }
        commands.extend(state.run.handle(Event::Score { competitor: c, points }));
    }
    if state.run.state() == State::RunRound && !state.ai_scored && state.run.round_ticks().is_some_and(|t| t <= TICKS_PER_SECOND as i32) {
        state.ai_scored = true;
        let player = state.run.round_scores()[0];
        // The stand-in only for competitors without a riding rider (no exported line set, or not spawned).
        let riding: Vec<usize> = state.ai.as_ref().map_or_else(Vec::new, |ai| ai.riders.iter().filter(|r| r.npc.is_some()).map(|r| r.id as usize).collect());
        for c in (1..state.run.round_scores().len()).filter(|c| !riding.contains(c)) {
            state.rng ^= state.rng << 13;
            state.rng ^= state.rng >> 17;
            state.rng ^= state.rng << 5;
            let f = stand_in.min + (stand_in.max - stand_in.min) * (state.rng as f32 / u32::MAX as f32);
            commands.extend(state.run.handle(Event::Score { competitor: c, points: player * f }));
        }
    }
    state.carry += time.delta_secs() * TICKS_PER_SECOND;
    while state.carry >= 1.0 {
        state.carry -= 1.0;
        commands.extend(state.run.tick());
    }
    let id = state.contest.id.clone();
    let playlist = state.contest.music_playlist.clone();
    let mut music = Vec::new();
    let mut finished = None;
    let mut queue: std::collections::VecDeque<Command> = commands.into();
    while let Some(c) = queue.pop_front() {
        match c {
            Command::MusicTrigger(code) => music.push((code, playlist.clone())),
            Command::TeleportToStart { round } => {
                // setup_round @173-224: every competitor starts the round in bounds, in no spot volume.
                state.gates.iter_mut().for_each(|g| *g = Default::default());
                state.inside.iter_mut().for_each(|i| *i = (true, 0));
                if let Some(m) = state.contest.rounds.get(round.saturating_sub(1)).and_then(|r| r.start_matrix) {
                    let name = state.contest.rounds.get(round.saturating_sub(1)).and_then(|r| r.start.clone()).unwrap_or_default();
                    info!("SKATE_CONTEST {id} round {round} start {name} {:?}", &m[3][..3]);
                    let variant = state.contest.rounds.get(round.saturating_sub(1)).map_or(0, |r| r.variant);
                    leaderboard_setup(&mut hud, &state.names, variant);
                    if let (Some(ai), Some(pop)) = (state.ai.as_mut(), population.as_deref_mut()) {
                        let r = state.contest.rounds.get(round.saturating_sub(1));
                        let paths = r.map(|r| r.ai_paths.paths()).unwrap_or_default();
                        ai.place(&m, r.map_or(&[][..], |r| &r.start_slots[..]), &paths, pop);
                    }
                    // RepositionAI(id, StartLocation, id) puts the player (competitor 0) on slot 0 of the locator, like the
                    // race heats (b992); the locator itself can sit under the ground (street_dwtn_01 round 2: 1.3 m
                    // below the plaza, the player fell through). The locator when the round has no slots.
                    let player = state.contest.rounds.get(round.saturating_sub(1)).and_then(|r| r.start_slots.first().copied()).unwrap_or(m);
                    info!("SKATE_CONTEST {id} round {round} player slot 0 {:?}", &player[3][..3]);
                    if let Err(e) = skater.travel_ex(player, None, true) {
                        warn!("SKATE_CONTEST teleport {e}");
                    }
                    if let Some(a) = autopilot.as_mut() {
                        a.host_teleported = true;
                    }
                }
            }
            Command::RoundStart { round, run, ticks } => {
                state.ai_scored = false;
                state.go_shown = 0.0;
                hud.show(Component::Timer, true);
                if let Some(ai) = state.ai.as_mut() {
                    ai.go();
                }
                info!("SKATE_CONTEST {id} round {round} run {run} start {} s", ticks as f32 / TICKS_PER_SECOND);
            }
            Command::Scores { round, total } => {
                info!("SKATE_CONTEST {id} scores round={round:?} total={total:?}");
                leaderboard_scores(&mut hud, &round);
            }
            Command::StartCountdownHud { .. } => {}
            // end_round: HUD off (setup_round shows the leaderboard again).
            Command::RoundStop => {
                hud.show(Component::Timer, false);
                hud.show(Component::Leaderboard, false);
                if let Some(ai) = state.ai.as_mut() {
                    ai.stop();
                }
            }
            // results_screen: the round results (Next) and the contest standings (Continue / Previous) in the end
            // dialog; without its movies the run continues at once.
            Command::RoundResults { round } => {
                info!("SKATE_CONTEST {id} round {round} results");
                match end_dialog.as_deref_mut().filter(|d| d.available) {
                    Some(d) => {
                        open_round_results(state, d, round);
                        state.dialog_open = true;
                        state.dialog_seconds = 0.0;
                    }
                    None => queue.extend(state.run.handle(Event::Continue)),
                }
            }
            // end_options: the final standings with Continue / Retry (Retry / Quit after a loss) in the end dialog.
            Command::EndOptions { won } => {
                info!("SKATE_CONTEST {id} end won={won}");
                match end_dialog.as_deref_mut().filter(|d| d.available) {
                    Some(d) => {
                        open_end_options(state, d, won);
                        state.dialog_open = true;
                        state.dialog_seconds = 0.0;
                    }
                    None => queue.extend(state.run.handle(Event::Continue)),
                }
            }
            Command::Complete(c) => finished = Some(c),
            other => info!("SKATE_CONTEST {id} {other:?}"),
        }
    }
    if let (Some(ai), Some(index), Some(pop)) = (state.ai.as_mut(), index.as_deref(), population.as_deref()) {
        ai.step(pop.world.tick(), index, &mut clocks, &mut commands_ecs);
    }
    contest_hud(state, &mut hud, time.delta_secs());
    let totals = state.run.totals().to_vec();
    contests.music.extend(music);
    if finished.is_some() {
        if let (Some(ai), Some(pop)) = (contests.run.as_mut().and_then(|r| r.ai.as_mut()), population.as_deref_mut()) {
            ai.remove(pop);
        }
    }
    if let Some(c) = finished {
        info!("SKATE_CONTEST end {id} completion={c:?} totals={totals:?}");
        contests.result = Some(format!("contest={id} completion={c:?} totals={totals:?}"));
        contests.music.push((12, playlist));
        contests.run = None;
        for c in [Component::Countdown, Component::Timer, Component::Leaderboard] {
            hud.show(c, false);
        }
    }
}

const AUTOPILOT_DIALOG_SECONDS: f32 = 3.0;

/// results_screen.stateenter@220-450 (single player, no elimination, no teams): block FinalResults = the standings by
/// total (title CONTEST_STANDINGS, Continue / Previous), block RoundResults = this round by round score (title
/// ROUND_RESULTS, Next; HighlightReel only with highlights, none recorded yet), shown first; layout by UseIntScore,
/// columns from the round row; OpenContinueRetryDialog(true).
fn open_round_results(state: &ContestState, d: &mut crate::end_dialog_host::EndDialogScreen, round: usize) {
    use skate_core::challenges::end_dialog::{CONTINUE, NEXT, NONE, PREVIOUS};
    use skate_core::challenges::results::{contest_results, ContestScore, MENU_FINAL_RESULTS, MENU_ROUND_RESULTS};
    let row = state.contest.rounds.get(round.saturating_sub(1));
    let int = uses_int_score(row.map_or(0, |r| r.variant));
    let columns = row.map(|r| r.result_columns.clone()).unwrap_or_default();
    let scores = |values: Vec<f32>| -> Vec<ContestScore> {
        values.into_iter().enumerate().map(|(i, score)| ContestScore { id: i as u32, name: state.names.get(i).cloned().unwrap_or_default(),
            local: i == 0, score }).collect()
    };
    let round_view = contest_results(&scores(state.run.round_scores().to_vec()), int, "ID_CHALLENGE_COMMON_ROUND_RESULTS", &columns, MENU_ROUND_RESULTS);
    let totals = state.run.totals().iter().map(|t| *t as f32).collect();
    let final_view = contest_results(&scores(totals), true, "ID_CHALLENGE_COMMON_CONTEST_STANDINGS", &columns, MENU_FINAL_RESULTS);
    d.dialog.passed = true;
    d.dialog.killed = false;
    d.dialog.slots = [NEXT, NONE, NONE, NONE];
    d.other = Some(([CONTINUE, PREVIOUS, NONE, NONE], final_view));
    d.results = Some(round_view);
    d.title = state.contest.title.as_ref().map(|t| format!("#{t}")).or_else(|| state.contest.title_id.clone());
    d.open = true;
    d.choice = None;
    // The dialog can still be open from the last pick (same movie, no reopen this frame): refill it (SetState 6).
    d.refill = true;
}

/// end_options.stateenter (single player, no elimination, no teams; contest_shared @0-80, 192-212, 399-440, 659-757,
/// read 2026-11-11): block FinalResults, slots Continue_Retry / Retry_Quit / ChangeDifficulty after a loss, title
/// CONTEST_STANDINGS, rows by total (BestTrickNonElimInt), the last round's columns, no rows for a demo;
/// OpenContinueRetryDialog(won). NOT RETAIL YET: a win with an OutroNIS plays the NIS first (not ported).
fn open_end_options(state: &ContestState, d: &mut crate::end_dialog_host::EndDialogScreen, won: bool) {
    use skate_core::challenges::end_dialog::{CHANGE_DIFFICULTY, CONTINUE_RETRY, NONE, RETRY_QUIT};
    use skate_core::challenges::results::{contest_results, ContestScore, MENU_FINAL_RESULTS};
    let columns = state.contest.rounds.last().map(|r| r.result_columns.clone()).unwrap_or_default();
    let scores: Vec<ContestScore> = if state.contest.demo { Vec::new() } else {
        state.run.totals().iter().enumerate().map(|(i, t)| ContestScore { id: i as u32,
            name: state.names.get(i).cloned().unwrap_or_default(), local: i == 0, score: *t as f32 }).collect()
    };
    d.dialog.passed = won;
    d.dialog.killed = false;
    d.dialog.slots = [CONTINUE_RETRY, RETRY_QUIT, if won { NONE } else { CHANGE_DIFFICULTY }, NONE];
    d.other = None;
    d.results = Some(contest_results(&scores, true, "ID_CHALLENGE_COMMON_CONTEST_STANDINGS", &columns, MENU_FINAL_RESULTS));
    d.title = state.contest.title.as_ref().map(|t| format!("#{t}")).or_else(|| state.contest.title_id.clone());
    d.open = true;
    d.choice = None;
    // The dialog can still be open from the last pick (same movie, no reopen this frame): refill it (SetState 6).
    d.refill = true;
}

/// setup_round's LeaderboardHUDReset + Init (layout by UseIntScore) with every round score 0 and rank 1.
fn leaderboard_setup(hud: &mut ChallengeHudState, names: &[String], variant: i32) {
    let int = uses_int_score(variant);
    let rows = names.iter().enumerate().map(|(i, n)| LeaderboardRow { id: i as u32, rank: 1, score: 0.0,
        score_text: if int { "0".into() } else { "0.00".into() }, name: n.clone(), highlight: i == 0 }).collect();
    hud.leaderboard = LeaderboardHud { layout: if int { LEADERBOARD_RANK_NAME_SCORE } else { LEADERBOARD_RANK_NAME_FLOATSCORE },
        title: String::new(), variant: LEADERBOARD_NO_VARIANT.into(), rows };
    hud.show(Component::Leaderboard, true);
}

/// LeaderboardHUDSetRank / SetInt / SetFloat after a score change. NOT RETAIL YET: the float text format (two
/// decimals) is a guess until the movie's float path is read.
fn leaderboard_scores(hud: &mut ChallengeHudState, round: &[f32]) {
    let int = hud.leaderboard.layout == LEADERBOARD_RANK_NAME_SCORE;
    let ranks = round_ranks(round);
    let mut rows = hud.leaderboard.rows.clone();
    for row in &mut rows {
        let i = row.id as usize;
        let (Some(score), Some(rank)) = (round.get(i), ranks.get(i)) else { continue };
        row.rank = *rank;
        row.score = *score;
        row.score_text = if int { format!("{}", *score as i64) } else { format!("{score:.2}") };
    }
    if rows != hud.leaderboard.rows {
        hud.leaderboard.rows = rows;
    }
}

/// The 3-2-1-GO (the countdown state's 3 s HUD countdown, GO for a second into the round) and the round timer.
fn contest_hud(state: &mut ContestState, hud: &mut ChallengeHudState, dt: f32) {
    let seconds = match state.run.countdown_hud_ticks() {
        Some(t) => Some((t.max(0) + TICKS_PER_SECOND as i32 - 1) / TICKS_PER_SECOND as i32),
        None if state.run.state() == State::RunRound && state.go_shown < 1.0 && hud.is_shown(Component::Countdown) => {
            state.go_shown += dt;
            Some(0)
        }
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
    if hud.is_shown(Component::Timer) {
        let (text, flag) = skate_core::challenges::timers::hud_text(state.run.round_ticks().unwrap_or(0));
        if hud.timer_text != text || hud.timer_flag != flag {
            hud.timer_text = text;
            hud.timer_flag = flag;
        }
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<Contests>()
        .init_resource::<AiScoreStandIn>()
        .add_systems(PreUpdate, arm.after(crate::map_transition::MapTransitionSet).run_if(crate::challenges::opted_in))
        .add_systems(Update, update.after(crate::challenges::CareerResultSet).run_if(crate::challenges::opted_in));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_score_variants_and_tied_places_follow_setup_round() {
        assert!([0, 3, 6, 12, 14, 15, 16].iter().all(|v| uses_int_score(*v)));
        assert!([7, 8, 9, 10, 11, 13].iter().all(|v| !uses_int_score(*v)));
        assert_eq!(round_ranks(&[500.0, 900.0, 500.0, 0.0]), [2, 1, 2, 4]);
    }

    #[test]
    fn rows_become_params_with_solo_runs_and_timed_runs() {
        let row = |run_type, solo_runs| ContestRound { run_type, variant: 0, round_time: 90, ai_adjustment: 0.75, cumulative: true, cumulative_min: 0,
            solo_runs, solo_ai_min: 3500, solo_ai_max: 2000, demo_target: None, start: None, start_matrix: None, start_slots: Vec::new(), result_columns: Vec::new(), ai_paths: Default::default(), boundary_volume: None, spot_volume_shapes: Vec::new() };
        let c = Contest { id: "street_univ_01".into(), map: None, demo: false, competitors: vec![crate::race_host::CompetitorRow { character: "a".into(), kind: 0 }; 3],
            music_playlist: None, title: None, title_id: None, sign_up: Default::default(), rounds: vec![row(1, 1), row(3, 1), row(4, 1)] };
        let p = params(&c, 7);
        assert_eq!(p.ai, 3);
        assert_eq!(p.rounds[0].run_type, RunType::SoloRun { runs: 1, ai_min: 3500.0, ai_max: 2000.0 });
        assert_eq!((p.rounds[1].run_type, p.rounds[2].run_type), (RunType::TimedRunSingle, RunType::Normal));
        assert_eq!((p.rounds[0].round_time, p.rounds[0].ai_adjustment, p.rounds[0].cumulative), (90.0, 0.75, true));
    }
}
