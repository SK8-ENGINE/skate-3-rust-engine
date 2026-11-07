//! The career in play (career M1 step 6; doc 43): loads `private/progression.json` and the player's
//! `settings/profiles/local/career.json` (importing `settings/challenges.json` once, without pay), takes challenge
//! results from `challenges.rs`, runs the career (pay order, actions, graphs) and saves. Presentation the engine does
//! not have yet (Skate 101 cards, yes / no popups, career photos) is answered at once and logged NOT RETAIL YET
//! (user decision 2026-11-06). Runs only with the Challenges opt-in.
use bevy::prelude::*;
use skate_core::career::graph::Request;
use skate_core::career::Career;
use skate_core::challenges::Completion;
use skate_data::career_save::{CareerFile, ChallengeEntry};
use skate_data::progression::Loaded;
use std::path::PathBuf;

/// A challenge run ended (written by `challenges.rs`).
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) struct ChallengeResult {
    pub challenge: String,
    pub completion: Completion,
    /// CompleteChallenge's re-activate flag (research b918): own the spot true, tutorials their Persistent field.
    pub reactivate: bool,
}

/// The player discovered a challenge (written by `challenges.rs`): retail activates it in the career (8255BD38,
/// research b910), so discovery opens the sign-up gate rather than waiting for it.
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) struct ChallengeDiscovered {
    pub challenge: String,
}

/// The Skate Profile progression page built from the career; the menu stage layers mods on top when `generation`
/// changes.
#[derive(Resource, Default)]
pub(crate) struct CareerProfile {
    pub data: skate_core::offline_profile::ProfileData,
    pub generation: u64,
}

/// Tier and achievement rows, and the profile's achievements (own file, kept across a new career).
#[derive(Default)]
pub(crate) struct Awards {
    pub tiers: std::collections::BTreeMap<String, skate_core::career::team::TierRow>,
    pub rows: std::collections::BTreeMap<String, skate_core::career::achievements::AchievementRow>,
    pub achievements: skate_core::career::achievements::Achievements,
    pub file: skate_data::career_save::AchievementsFile,
    pub path: Option<PathBuf>,
    /// sdk.career events for the mods (newest 64 kept).
    pub events: Vec<serde_json::Value>,
    /// Discovery statuses the graphs set (Discovery.SetDiscoveryStatus), applied to the challenge records.
    pub discovery: Vec<(String, String)>,
}

impl Awards {
    pub(crate) fn event(&mut self, event: serde_json::Value) {
        if self.events.len() >= 64 {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    pub(crate) fn grant(&mut self, name: &str) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        if !self.achievements.grant(name, self.rows.get(name), now) {
            return;
        }
        info!("SKATE_CAREER achievement {name} id={}", self.rows.get(name).map_or(0, |r| r.id));
        self.event(serde_json::json!({"name": "career_achievement", "achievement": name}));
        self.file.granted = self.achievements.granted.clone();
        if let Some(path) = self.path.clone() {
            write_in_background(path, self.file.to_json());
        }
    }

    /// Plays presentation events: GrantAchievement (type 28) grants; the rest are NOT RETAIL YET and only logged.
    fn play(&mut self, events: impl IntoIterator<Item = skate_core::career::graph::Event>) {
        for e in events {
            match (e.kind.as_str(), &e.args[0]) {
                ("GrantAchievement", Some(name)) => self.grant(name),
                _ => info!("SKATE_CAREER event {} {:?} NOT RETAIL YET", e.kind, e.args),
            }
        }
    }
}

fn write_in_background(path: PathBuf, text: String) {
    std::thread::spawn(move || {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let part = path.with_extension("json.part");
        if std::fs::write(&part, text).is_ok() {
            let _ = std::fs::rename(&part, &path);
        }
    });
}

/// The tier and achievement names the graphs use (row keys are stored hashed).
fn graph_names(graphs: &std::collections::BTreeMap<String, skate_core::career::graph::Graph>) -> (Vec<String>, Vec<String>) {
    use skate_core::career::graph::Op;
    fn walk(ops: &[Op], tiers: &mut Vec<String>, achievements: &mut Vec<String>) {
        for op in ops {
            match op {
                Op::BrandTier(t) => tiers.push(t.clone()),
                Op::GrantAchievement(a) => achievements.push(a.clone()),
                Op::If { then, otherwise, .. } => {
                    walk(then, tiers, achievements);
                    walk(otherwise, tiers, achievements);
                }
                _ => {}
            }
        }
    }
    let (mut tiers, mut achievements): (Vec<String>, Vec<String>) = ((1..5).map(|i| format!("040{i}_team_photo_teammate_{i}")).collect(), Vec::new());
    for state in graphs.values().flat_map(|g| g.states.values()) {
        state.on.values().for_each(|ops| walk(ops, &mut tiers, &mut achievements));
        achievements.extend(state.events.iter().filter(|e| e.kind == "GrantAchievement").filter_map(|e| e.args[0].clone()));
    }
    tiers.sort();
    tiers.dedup();
    achievements.sort();
    achievements.dedup();
    (tiers, achievements)
}

#[derive(Resource, Default)]
pub(crate) struct CareerState {
    pub awards: Awards,
    pub summary: Vec<skate_data::career_summary::SummaryRow>,
    pub data: Option<Loaded>,
    pub career: Career,
    pub file: CareerFile,
    pub path: Option<PathBuf>,
    pub tried: bool,
    pub loading: Option<std::thread::JoinHandle<Result<Prepared, String>>>,
    pub waiting: Vec<ChallengeResult>,
    pub discovered: Vec<String>,
    /// The setup export as read, before mod overlays.
    pub base: Option<serde_json::Value>,
    /// Running mods (id, content fingerprint) last synced, and their progression overlays in mod id order.
    pub overlay_keys: Vec<(String, u64)>,
    pub overlays: Vec<(String, Vec<serde_json::Value>)>,
    pub overlays_changed: bool,
    pub dirty: bool,
}

fn completion_text(c: Completion) -> Option<&'static str> {
    match c {
        Completion::Owned => Some("owned"),
        Completion::Killed => Some("killed"),
        Completion::Abandoned => None,
    }
}

fn completion_of(text: Option<&str>) -> Option<Completion> {
    match text {
        Some("owned") => Some(Completion::Owned),
        Some("killed") => Some(Completion::Killed),
        _ => None,
    }
}

impl CareerState {
    /// The file's counters, statuses, actions and flags into the career.
    fn adopt(&mut self) {
        let f = &self.file;
        let c = &mut self.career;
        c.counters.sales_owned = f.counters.sales_owned;
        c.counters.sales_killed = f.counters.sales_killed;
        c.counters.sales_bonus = f.counters.sales_bonus;
        c.counters.sales_set = f.counters.sales_set;
        c.counters.money = f.counters.money;
        c.counters.money_earned = f.counters.money_earned;
        c.status = f.challenges.iter().filter_map(|(id, e)| completion_of(e.completion.as_deref()).map(|x| (id.clone(), x))).collect();
        c.tracker.armed = f.actions.armed.clone();
        c.tracker.fired = f.actions.fired.clone();
        c.runner.flags = f.flags.clone();
        c.pending.milestones = f.rewards.milestones_pending;
        c.pending.teammates = f.rewards.teammates_pending;
        for (i, joined) in f.team.members.iter().enumerate().take(5) {
            c.team.members[i] = *joined || i == 0;
        }
        c.team.tiers = f.team.tiers.iter().cloned().collect();
        c.unlocks.granted = f.unlocks.iter().map(|u| ((u.kind, u.name.clone()), u.new)).collect();
        c.active = f.active.iter().cloned().collect();
    }

    /// The career back into the file (other keys of the file stay).
    fn capture(&mut self) {
        let c = &self.career;
        let f = &mut self.file;
        f.counters.sales_owned = c.counters.sales_owned;
        f.counters.sales_killed = c.counters.sales_killed;
        f.counters.sales_bonus = c.counters.sales_bonus;
        f.counters.sales_set = c.counters.sales_set;
        f.counters.money = c.counters.money;
        f.counters.money_earned = c.counters.money_earned;
        for (id, completion) in &c.status {
            let entry = f.challenges.entry(id.clone()).or_insert_with(ChallengeEntry::default);
            entry.completion = completion_text(*completion).map(str::to_owned);
        }
        f.actions.armed = c.tracker.armed.clone();
        f.actions.fired = c.tracker.fired.clone();
        f.flags = c.runner.flags.clone();
        f.rewards.milestones_pending = c.pending.milestones;
        f.rewards.teammates_pending = c.pending.teammates;
        f.team.members = c.team.members.to_vec();
        f.team.tiers = c.team.tiers.iter().cloned().collect();
        f.unlocks = c.unlocks.granted.iter().map(|((kind, name), new)| skate_data::career_save::UnlockEntry {
            kind: *kind, name: name.clone(), new: *new, ..Default::default() }).collect();
        f.active = c.active.iter().cloned().collect();
    }

    fn save_in_background(&self) {
        if let Some(path) = self.path.clone() {
            write_in_background(path, self.file.to_json());
        }
    }
}

/// Logs the requests, answers the presentation ones at once (M1), plays or defers presentation events, grants
/// achievements, and repeats until nothing waits.
fn settle(career: &mut Career, awards: &mut Awards, data: &Loaded, mut requests: Vec<Request>) {
    use skate_core::career::graph::EventMode;
    let (progression, graphs) = (&data.progression, &data.graphs);
    let mut guard = 0;
    while !requests.is_empty() && guard < 256 {
        guard += 1;
        let mut next = Vec::new();
        for r in requests {
            match &r {
                Request::UnlockReward { reward, big } => grant_reward(career, awards, data, reward, *big),
                Request::GrantAchievement(name) => awards.grant(name),
                Request::SetDiscovery { challenge, status } => awards.discovery.push((challenge.clone(), status.clone())),
                Request::MilestoneEarned => awards.event(serde_json::json!({"name": "career_milestone"})),
                Request::UnlockTeammate => awards.event(serde_json::json!({"name": "career_teammate"})),
                Request::BrandTier(tier) => awards.event(serde_json::json!({"name": "career_tier", "tier": tier})),
                Request::Events { graph, state, mode } => {
                    let events = graphs.get(graph).and_then(|g| g.states.get(state)).map(|s| s.events.clone()).unwrap_or_default();
                    match mode {
                        EventMode::Execute => awards.play(events),
                        EventMode::Defer => career.deferred.extend(events),
                        EventMode::None => {}
                    }
                }
                _ => {}
            }
            let reply = match &r {
                Request::Show101 { .. } => Some(("hudcomplete", None)),
                Request::ShowYesNo { .. } => Some(("hudyes", None)),
                Request::LoadCareerPhoto { request, .. } => Some(("challengeteambrandimagecomplete", Some(*request))),
                _ => None,
            };
            info!("SKATE_CAREER {r:?}{}", if reply.is_some() { " NOT RETAIL YET: answered at once" } else { "" });
            if let Some((message, request)) = reply {
                next.extend(career.deliver(message, request, progression, graphs));
            }
        }
        next.extend(career.update(progression, graphs));
        requests = next;
    }
}

/// Save entries that point at ids the loaded data does not have (a mod's content after the mod is gone) are
/// removed; everything that works without the mod stays. Mod flags (`<mod id>:<name>`) go when their mod stops.
fn drop_unknown(state: &mut CareerState, data: &Loaded) {
    let c = &mut state.career;
    let actions = &data.progression.actions;
    let mut dropped: Vec<String> = c.tracker.armed.iter().chain(&c.tracker.fired).filter(|a| !actions.contains_key(*a)).cloned().collect();
    c.tracker.armed.retain(|a| actions.contains_key(a));
    c.tracker.fired.retain(|a| actions.contains_key(a));
    dropped.extend(c.runner.queue.iter().filter(|g| !data.graphs.contains_key(*g)).cloned());
    c.runner.queue.retain(|g| data.graphs.contains_key(g));
    if c.runner.running.as_ref().is_some_and(|r| !data.graphs.contains_key(&r.graph)) {
        dropped.extend(c.runner.running.take().map(|r| r.graph));
    }
    if !state.awards.tiers.is_empty() {
        dropped.extend(c.team.tiers.iter().filter(|t| !state.awards.tiers.contains_key(*t)).cloned());
        c.team.tiers.retain(|t| state.awards.tiers.contains_key(t));
    }
    if !dropped.is_empty() {
        info!("SKATE_CAREER dropped unknown save entries {dropped:?}");
        state.dirty = true;
    }
}

/// Everything the career reads from disk, parsed off the main thread (the collections file is ~10 MB).
pub(crate) struct Prepared {
    base: serde_json::Value,
    data: Loaded,
    existing: Option<CareerFile>,
    old_challenges: Option<String>,
    path: PathBuf,
    summary: Vec<skate_data::career_summary::SummaryRow>,
    tiers: std::collections::BTreeMap<String, skate_core::career::team::TierRow>,
    rows: std::collections::BTreeMap<String, skate_core::career::achievements::AchievementRow>,
    achievements: skate_data::career_save::AchievementsFile,
    achievements_path: PathBuf,
}

fn read(asset_root: PathBuf) -> Result<Prepared, String> {
    let root = asset_root.parent().unwrap_or(&asset_root).to_path_buf();
    let base: serde_json::Value = std::fs::read_to_string(asset_root.join("private/progression.json")).map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))?;
    let data = skate_data::progression::parse_value(&base)?;
    let path = root.join("settings/profiles/local/career.json");
    let existing = std::fs::read_to_string(&path).ok().and_then(|t| CareerFile::parse(&t).ok());
    let old_challenges = existing.is_none().then(|| std::fs::read_to_string(root.join("settings/challenges.json")).ok()).flatten();
    let (mut summary, mut tiers, mut rows) = Default::default();
    match skate_data::collections::Collections::load(&asset_root) {
        Ok(c) => {
            use skate_data::career_summary as cs;
            let (tier_names, achievement_names) = graph_names(&data.graphs);
            summary = cs::summary_rows(&c);
            tiers = cs::tier_rows(&c, &tier_names);
            rows = cs::achievement_rows(&c, &achievement_names);
        }
        Err(e) => info!("SKATE_CAREER no summary, tier or achievement rows ({e})"),
    }
    let achievements_path = root.join("settings/profiles/local/achievements.json");
    let achievements = std::fs::read_to_string(&achievements_path).ok()
        .and_then(|t| skate_data::career_save::AchievementsFile::parse(&t).ok()).unwrap_or_default();
    Ok(Prepared { base, data, existing, old_challenges, path, summary, tiers, rows, achievements, achievements_path })
}

/// The export with the running mods' overlays merged (b887 §8): each mod's overlay is kept only if the data still
/// parses with it (a broken overlay is logged and left out). Then save entries the new data does not know go.
fn rebuild(state: &mut CareerState, data: &mut Loaded) {
    state.overlays_changed = false;
    let Some(base) = state.base.as_ref() else { return };
    let mut merged = base.clone();
    let mut used = Vec::new();
    for (id, list) in &state.overlays {
        let mut candidate = merged.clone();
        list.iter().for_each(|o| skate_data::progression::merge(&mut candidate, o));
        match skate_data::progression::parse_value(&candidate) {
            Ok(_) => {
                merged = candidate;
                used.push(id.clone());
            }
            Err(e) => warn!("SKATE_CAREER mod {id} progression overlay left out: {e}"),
        }
    }
    match skate_data::progression::parse_value(&merged) {
        Ok(new) => {
            *data = new;
            info!("SKATE_CAREER data rebuilt with overlays {used:?}");
        }
        Err(e) => warn!("SKATE_CAREER rebuild failed: {e}"),
    }
    drop_unknown(state, data);
    state.dirty = true;
}

/// The read career into the state: import, adopt, clean up, defaults, load replay, start.
fn apply(state: &mut CareerState, p: Prepared) {
    let Prepared { base, mut data, existing, old_challenges, path, summary, tiers, rows, achievements, achievements_path } = p;
    state.base = Some(base);
    if !state.overlays.is_empty() {
        rebuild(state, &mut data);
    }
    let new_career = existing.is_none();
    state.file = existing.unwrap_or_default();
    if let Some(old) = old_challenges {
        if let Ok(n) = state.file.import_challenge_records(&old, "settings/challenges.json") {
            info!("SKATE_CAREER imported {n} challenge records (statuses only, no pay)");
        }
    }
    state.path = Some(path);
    state.summary = summary;
    state.awards.tiers = tiers;
    state.awards.rows = rows;
    state.awards.achievements.granted = achievements.granted.clone();
    state.awards.file = achievements;
    state.awards.path = Some(achievements_path);
    state.adopt();
    drop_unknown(state, &data);
    let tiers = state.awards.tiers.clone();
    state.career.team.apply_defaults(&tiers);
    info!("SKATE_CAREER loaded board_sales={} armed={} fired={} new={new_career} tiers={} achievements={}",
        state.career.counters.board_sales(), state.career.tracker.armed.len(), state.career.tracker.fired.len(),
        state.career.team.tiers.len(), state.awards.achievements.granted.len());
    // The load replay (827CE7B0): milestone achievements whose board sales are already reached.
    let sales = state.career.counters.board_sales();
    for (name, threshold) in skate_core::career::achievements::load_replay(&data.progression, &data.graphs) {
        if sales >= threshold {
            state.awards.grant(&name);
        }
    }
    let requests = state.career.start(&data.progression, &data.graphs);
    let s = &mut *state;
    settle(&mut s.career, &mut s.awards, &data, requests);
    if std::env::var_os("SKATE_CAREER_SKIP_START").is_some_and(|v| v == "1") {
        skip_start(state, &data);
    }
    state.data = Some(data);
    state.capture();
    state.save_in_background();
}

/// The crossbar's GetProgressionData slots as menu facts (b873 §1.1), written only when they change.
fn publish_facts(career: &Career, data: &Loaded, menus: &mut crate::retail_menus::RetailMenus) {
    use crate::crossbar_host::progression_facts as f;
    let values = skate_core::career::milestones::facts(career, &data.progression);
    for (key, value) in [f::MILESTONE, f::COUNT, f::START, f::GOAL, f::BOARD_SALES, f::EXTRA].into_iter().zip(values) {
        if menus.context.facts.get(key) != Some(&value) {
            menus.context.facts.insert(key.into(), value);
        }
    }
}

/// NOT RETAIL YET: retail shows the reward summary and milestone / teammate cards and clears the pending counts when
/// they close (b893); without those screens the earned rewards count as presented at the end of the frame.
fn present_stub(career: &mut Career, awards: &mut Awards) -> bool {
    let p = career.pending;
    let summary = !career.unlocks.summary_big.is_empty() || !career.unlocks.summary_other.is_empty();
    if p.milestones == 0 && p.teammates == 0 && career.deferred.is_empty() && !summary {
        return false;
    }
    let (big, other) = career.unlocks.take_summary();
    info!("SKATE_CAREER NOT RETAIL YET summary/cards milestones={} teammates={} deferred={} big={big:?} other={other:?}",
        p.milestones, p.teammates, career.deferred.len());
    career.pending = Default::default();
    awards.play(std::mem::take(&mut career.deferred));
    true
}

/// Skip the career start (b887 U1, user ok 2026-11-06): each crossbar gate challenge with no status yet completes as
/// owned through the normal result path (pay, checks, graphs), as if the player had done the start chain.
pub(crate) fn skip_start(state: &mut CareerState, data: &Loaded) -> usize {
    let gates: Vec<String> = data.progression.config.crossbar_gates.iter()
        .filter(|g| !state.career.status.contains_key(*g)).cloned().collect();
    for gate in &gates {
        // The actions waiting for this gate (normally armed by the start chain's graphs) are armed first, so its
        // completion fires them and their graphs run (photo_univ_05's arms the milestones, b888).
        let waiting: Vec<String> = data.progression.actions.iter()
            .filter(|(id, a)| a.conditions.iter().any(|c| matches!(c, skate_core::career::Condition::ChallengeDone(g) if g == gate))
                && !state.career.tracker.armed.contains(*id) && !state.career.tracker.fired.contains(*id))
            .map(|(id, _)| id.clone()).collect();
        for id in &waiting {
            let effects = state.career.arm(id, &data.progression);
            apply_effects(state, data, effects);
        }
        info!("SKATE_CAREER skip start: {gate} owned (armed first: {waiting:?})");
        let effects = state.career.challenge_result(gate, Completion::Owned, &data.progression);
        state.career.settle_active(gate, Completion::Owned, false);
        apply_effects(state, data, effects);
    }
    gates.len()
}

/// A progression reward row into the unlock store (kind and name from `progression.json` `rewards`) and the
/// reward summary list; a reward id with no row is logged and skipped.
pub(crate) fn grant_reward(career: &mut Career, awards: &mut Awards, data: &Loaded, reward: &str, big: bool) {
    let Some((kind, name)) = data.rewards.get(reward) else {
        info!("SKATE_CAREER reward {reward} has no reward row");
        return;
    };
    if name.is_empty() {
        // A card with nothing to unlock (milestone, photo cards): only the reward summary lists it.
        let list = if big { &mut career.unlocks.summary_big } else { &mut career.unlocks.summary_other };
        list.push(reward.to_owned());
        return;
    }
    if career.unlocks.reward(reward, *kind, name, big) {
        info!("SKATE_CAREER unlock kind={kind} {name} (reward {reward}, big={big})");
        awards.event(serde_json::json!({"name": "career_unlock", "kind": kind, "unlock": name, "reward": reward}));
    }
}

/// Logs career effects, tells the mods, starts the fired actions' graphs and settles their requests (a challenge
/// result or an sdk.career change).
pub(crate) fn apply_effects(state: &mut CareerState, data: &Loaded, effects: Vec<skate_core::career::Effect>) {
    use skate_core::career::Effect;
    for e in &effects {
        info!("SKATE_CAREER {e:?}");
        let event = match e {
            Effect::Paid { challenge, tier, money, sales, rewards } => {
                for reward in rewards {
                    grant_reward(&mut state.career, &mut state.awards, data, reward, false);
                }
                serde_json::json!({"name": "career_paid", "challenge": challenge, "tier": completion_text(*tier),
                    "money": money, "sales": sales, "rewards": rewards})
            }
            Effect::ActionFired { action, graph } => serde_json::json!({"name": "career_action_fired", "action": action, "graph": graph}),
            Effect::StatusChanged { .. } => continue,
        };
        state.awards.event(event);
    }
    let requests = state.career.drive(effects, &data.progression, &data.graphs);
    settle(&mut state.career, &mut state.awards, data, requests);
    info!("SKATE_CAREER board_sales={} money={}", state.career.counters.board_sales(), state.career.counters.money);
}

/// Unlock-gated summary rows show once their (kind, name hash) is granted (82503AE0 / 82503D50). DLC packs bring
/// their own rows, so those appear with the DLC.
fn publish_profile(state: &CareerState, data: &Loaded, profile: &mut CareerProfile) {
    let unlocks = &state.career.unlocks;
    let (groups, overall) = skate_data::career_summary::summary_groups(&state.summary, &data.progression,
        |[kind, hash]| unlocks.granted_hash(kind, hash));
    let mut page = std::mem::take(&mut profile.data);
    skate_core::career::profile::fill(&mut page, &groups, [&overall[0], &overall[1]], &state.career, &data.progression);
    profile.data = page;
    profile.generation += 1;
}

/// Challenge gating follows the unlock store's switch: with `unlock_all` off only the career's activated
/// challenges are available (retail); on (the default) all are.
fn publish_gate(career: &Career, challenges: &mut crate::challenges::Challenges) {
    let gate = (!career.unlocks.unlock_all).then(|| career.active.clone());
    if challenges.gate != gate {
        challenges.gate = gate;
    }
}

#[allow(clippy::too_many_arguments)]
fn update(mut res: ResMut<CareerState>, config: Res<crate::config::Config>, mut results: MessageReader<ChallengeResult>,
          mut discovered: MessageReader<ChallengeDiscovered>,
          menus: Option<ResMut<crate::retail_menus::RetailMenus>>, mut profile: ResMut<CareerProfile>,
          challenges: Option<ResMut<crate::challenges::Challenges>>) {
    let state: &mut CareerState = &mut res;
    if !state.tried {
        state.tried = true;
        let asset_root = config.asset_root.clone();
        state.loading = Some(std::thread::spawn(move || read(asset_root)));
    }
    // Results that arrive while the career loads wait for it (none are lost).
    state.waiting.extend(results.read().cloned());
    state.discovered.extend(discovered.read().map(|d| d.challenge.clone()));
    let mut first = false;
    if state.loading.as_ref().is_some_and(|h| h.is_finished()) {
        match state.loading.take().map(|h| h.join()) {
            Some(Ok(Ok(prepared))) => {
                apply(state, prepared);
                first = true;
            }
            Some(Ok(Err(e))) => info!("SKATE_CAREER none ({e})"),
            _ => warn!("SKATE_CAREER load thread failed"),
        }
    }
    let Some(mut data) = state.data.take() else {
        if state.loading.is_none() {
            state.waiting.clear();
            state.discovered.clear();
        }
        return;
    };
    if state.overlays_changed {
        rebuild(state, &mut data);
    }
    let mut changed = false;
    for id in std::mem::take(&mut state.discovered) {
        if state.career.activate(&id) {
            info!("SKATE_CAREER activated {id} (discovered)");
            changed = true;
        }
    }
    for r in std::mem::take(&mut state.waiting) {
        let effects = state.career.challenge_result(&r.challenge, r.completion, &data.progression);
        state.career.settle_active(&r.challenge, r.completion, r.reactivate);
        apply_effects(state, &data, effects);
        changed = true;
    }
    changed |= std::mem::take(&mut state.dirty);
    let requests = state.career.update(&data.progression, &data.graphs);
    if !requests.is_empty() {
        settle(&mut state.career, &mut state.awards, &data, requests);
        changed = true;
    }
    changed |= present_stub(&mut state.career, &mut state.awards);
    if let Some(mut menus) = menus.filter(|_| changed || first) {
        publish_facts(&state.career, &data, &mut menus);
    }
    let mut challenges = challenges;
    if let Some(challenges) = challenges.as_deref_mut() {
        for (id, status) in std::mem::take(&mut state.awards.discovery) {
            let value = match status.as_str() {
                "Discovered" => crate::challenge_records::DISCOVERED,
                "Undiscovered" | "NotDiscovered" => crate::challenge_records::NOT_DISCOVERED,
                other => {
                    warn!("SKATE_CAREER unknown discovery status {other} for {id}");
                    continue;
                }
            };
            info!("SKATE_CAREER discovery {id} = {status}");
            challenges.records.update(&id, |s| s.discovery = value);
        }
    }
    if changed || first {
        publish_profile(&state, &data, &mut profile);
        if let Some(challenges) = challenges.as_deref_mut() {
            publish_gate(&state.career, challenges);
        }
    }
    state.data = Some(data);
    if changed {
        state.capture();
        state.save_in_background();
    }
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<CareerState>()
        .init_resource::<CareerProfile>()
        .add_message::<ChallengeResult>()
        .add_message::<ChallengeDiscovered>()
        .add_systems(Update, update.after(crate::challenges::CareerResultSet).run_if(crate::challenges::opted_in));
}
