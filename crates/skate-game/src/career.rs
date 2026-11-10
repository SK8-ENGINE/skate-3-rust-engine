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
    /// The names the TEAMMATE text type can show (824FEAB8: teammate v while v < the member count).
    pub teammates: Vec<String>,
}

/// What the reward summary and card screens show (`reward_summary_host.rs`), rebuilt when a screen chain opens.
#[derive(Resource, Default)]
pub(crate) struct RewardScreens {
    pub view: skate_core::career::presentation::SummaryView,
    pub generation: u64,
    /// The screen the stage shows (screen, card index); `accepted`: the stage took A on it.
    pub open: Option<(skate_core::career::presentation::Screen, u32)>,
    pub accepted: bool,
    /// The stage has every reward movie; without it each screen is accepted the frame it opens.
    pub available: bool,
}

fn summary_view(state: &CareerState, data: &Loaded) -> skate_core::career::presentation::SummaryView {
    use skate_core::career::presentation::{Card, SummaryView};
    let c = &state.career;
    let card = |r: &str| data.reward_text.get(r).map(|t| Card { title: t.title.clone(), image: t.image.clone(),
        description: t.description.clone() }).unwrap_or_else(|| Card { title: r.to_owned(), ..Default::default() });
    let mut view = SummaryView::build(&state.reward_tabs, &c.unlocks.summary_other, &c.unlocks.summary_big,
        |r| data.rewards.get(r).map(|(kind, _)| *kind), card);
    let facts = skate_core::career::milestones::facts(c, &data.progression);
    view.totals = c.summary;
    view.total_cash = c.counters.money;
    view.total_sales = c.counters.board_sales();
    view.prev_milestone = facts[2];
    view.next_milestone = facts[3];
    view
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

/// A teammate preset row and its look read from `SavedRecipe<id>.bin`.
pub(crate) struct Preset {
    pub row: skate_data::teammate_presets::PresetRow,
    pub profile: serde_json::Map<String, serde_json::Value>,
}

fn read_presets(asset_root: &std::path::Path, c: &skate_data::collections::Collections) -> Vec<Preset> {
    use skate_data::teammate_presets as tp;
    let names = tp::morph_order(c);
    tp::preset_rows(c).into_iter().filter_map(|row| match tp::load_profile(asset_root, &row, &names) {
        Ok(profile) => Some(Preset { row, profile }),
        Err(e) => {
            warn!("SKATE_CAREER preset {} left out: {e}", row.key);
            None
        }
    }).collect()
}

impl CareerState {
    /// Teammate n gets preset `key`'s look (retail 8257DC48 -> 827D5288 copies it into slot n).
    pub(crate) fn apply_preset(&mut self, n: usize, key: &str) -> Result<(), String> {
        if self.career.team.teammate_mut(n).is_none() {
            return Err(format!("no teammate {n}"));
        }
        let preset = self.presets.iter().find(|p| p.row.key == key).ok_or(format!("no preset {key:?}"))?;
        self.looks.set(n, Some(skate_data::cac_save::SlotLook { source: skate_data::cac_save::LookSource::Preset { id: key.to_owned() },
            profile: preset.profile.clone(), extra: Default::default() }));
        self.save_looks();
        Ok(())
    }
}

fn write_in_background(path: PathBuf, text: String) {
    let writing = crate::save_tag::Writing::begin();
    std::thread::spawn(move || {
        let _writing = writing;
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
    pub reward_tabs: Vec<skate_core::career::presentation::Tab>,
    pub data: Option<Loaded>,
    pub career: Career,
    pub file: CareerFile,
    pub path: Option<PathBuf>,
    pub tried: bool,
    pub loading: Option<std::thread::JoinHandle<Result<Prepared, String>>>,
    pub waiting: Vec<ChallengeResult>,
    pub discovered: Vec<String>,
    pub presentation: skate_core::career::presentation::Presentation,
    /// The setup export as read, before mod overlays.
    pub base: Option<serde_json::Value>,
    /// Running mods (id, content fingerprint) last synced, and their progression overlays in mod id order.
    pub overlay_keys: Vec<(String, u64)>,
    pub overlays: Vec<(String, Vec<serde_json::Value>)>,
    pub overlays_changed: bool,
    pub dirty: bool,
    /// The new career's world -> first challenge rule (data; a mod can replace it).
    pub first_challenge: skate_core::career::boot::FirstChallenge,
    /// Saved skater looks (`cac.json`, retail's CAC record): teammate n's look and spawn gate.
    pub looks: skate_data::cac_save::CacFile,
    pub looks_path: Option<PathBuf>,
    /// Bumped on every look change; the living world re-reads the looks when it moves.
    pub looks_generation: u64,
    /// Teammate presets (b986), by id; the list screen sorts them by localised name.
    pub presets: Vec<Preset>,
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
        if let Some(name) = &f.team.name {
            c.team.name = skate_core::career::team::clip_name(name);
        }
        c.team.logo = f.team.logo.clone();
        for (i, t) in c.team.teammates.iter_mut().enumerate() {
            match f.team.teammates.get(i) {
                Some(saved) => (t.joined, t.name) = (saved.joined, skate_core::career::team::clip_name(&saved.name)),
                None => t.joined = f.team.members.get(i + 1).copied().unwrap_or(false),
            }
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
        f.team.name = Some(c.team.name.clone());
        f.team.logo = c.team.logo.clone();
        f.team.teammates.resize_with(c.team.teammates.len(), Default::default);
        for (saved, t) in f.team.teammates.iter_mut().zip(&c.team.teammates) {
            (saved.joined, saved.name) = (t.joined, t.name.clone());
        }
        f.team.members = c.team.members();
        f.team.tiers = c.team.tiers.iter().cloned().collect();
        f.unlocks = c.unlocks.granted.iter().map(|((kind, name), new)| skate_data::career_save::UnlockEntry {
            kind: *kind, name: name.clone(), new: *new, ..Default::default() }).collect();
        f.active = c.active.iter().cloned().collect();
    }

    pub(crate) fn save_looks(&mut self) {
        self.looks_generation += 1;
        if let Some(path) = self.looks_path.clone() {
            write_in_background(path, self.looks.to_json());
        }
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
    reward_tabs: Vec<skate_core::career::presentation::Tab>,
    tiers: std::collections::BTreeMap<String, skate_core::career::team::TierRow>,
    rows: std::collections::BTreeMap<String, skate_core::career::achievements::AchievementRow>,
    achievements: skate_data::career_save::AchievementsFile,
    achievements_path: PathBuf,
    looks: skate_data::cac_save::CacFile,
    looks_path: PathBuf,
    presets: Vec<Preset>,
}

fn read(asset_root: PathBuf) -> Result<Prepared, String> {
    let root = asset_root.parent().unwrap_or(&asset_root).to_path_buf();
    let base: serde_json::Value = std::fs::read_to_string(asset_root.join("private/progression.json")).map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))?;
    let data = skate_data::progression::parse_value(&base)?;
    let path = root.join("settings/profiles/local/career.json");
    let existing = std::fs::read_to_string(&path).ok().and_then(|t| CareerFile::parse(&t).ok());
    let old_challenges = existing.is_none().then(|| std::fs::read_to_string(root.join("settings/challenges.json")).ok()).flatten();
    let (mut summary, mut tiers, mut rows, mut reward_tabs, mut presets) = Default::default();
    match skate_data::collections::Collections::load(&asset_root) {
        Ok(c) => {
            use skate_data::career_summary as cs;
            let (tier_names, achievement_names) = graph_names(&data.graphs);
            summary = cs::summary_rows(&c);
            reward_tabs = cs::reward_tabs(&c).into_iter()
                .map(|t| skate_core::career::presentation::Tab { id: t.key, title: t.title, list: t.list, kinds: t.kinds })
                .collect();
            tiers = cs::tier_rows(&c, &tier_names);
            rows = cs::achievement_rows(&c, &achievement_names);
            presets = read_presets(&asset_root, &c);
        }
        Err(e) => info!("SKATE_CAREER no summary, tier or achievement rows ({e})"),
    }
    let achievements_path = root.join("settings/profiles/local/achievements.json");
    let achievements = std::fs::read_to_string(&achievements_path).ok()
        .and_then(|t| skate_data::career_save::AchievementsFile::parse(&t).ok()).unwrap_or_default();
    let looks_path = root.join("settings/profiles/local/cac.json");
    let looks = match std::fs::read_to_string(&looks_path) {
        Ok(t) => skate_data::cac_save::CacFile::parse(&t).unwrap_or_else(|e| {
            warn!("SKATE_CAREER cac.json unreadable, kept as cac.json.bad; looks start empty ({e})");
            let _ = std::fs::copy(&looks_path, looks_path.with_extension("json.bad"));
            Default::default()
        }),
        Err(_) => Default::default(),
    };
    Ok(Prepared { base, data, existing, old_challenges, path, summary, reward_tabs, tiers, rows, achievements, achievements_path, looks, looks_path, presets })
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
    let Prepared { base, mut data, existing, old_challenges, path, summary, reward_tabs, tiers, rows, achievements, achievements_path, looks, looks_path, presets } = p;
    state.presets = presets;
    state.looks = looks;
    state.looks_generation += 1;
    state.looks_path = Some(looks_path);
    state.base = Some(base);
    if !state.overlays.is_empty() {
        rebuild(state, &mut data);
    }
    let new_career = existing.is_none();
    state.file = existing.unwrap_or_default();
    if new_career {
        // The profile reset sets "new career pending" (825009F0, research b976).
        state.file.boot.pending = true;
    }
    if let Some(old) = old_challenges {
        if let Ok(n) = state.file.import_challenge_records(&old, "settings/challenges.json") {
            info!("SKATE_CAREER imported {n} challenge records (statuses only, no pay)");
        }
    }
    state.path = Some(path);
    state.summary = summary;
    state.reward_tabs = reward_tabs;
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
    // Test hook: SKATE_TEAMMATE_PRESET=<n>:<preset key>[,...] gives teammate n that preset's look (saved in cac.json).
    for pair in std::env::var("SKATE_TEAMMATE_PRESET").unwrap_or_default().split(',').filter(|p| !p.is_empty()) {
        let result = pair.split_once(':').ok_or_else(|| "expected <n>:<key>".to_owned())
            .and_then(|(n, key)| state.apply_preset(n.trim().parse().map_err(|_| format!("bad teammate {n:?}"))?, key.trim()));
        info!("SKATE_CAREER teammate preset {pair} (SKATE_TEAMMATE_PRESET): {result:?}");
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

/// The reward presentation (docs/hails-additions/43-career.md#change-the-reward-presentation-careerpresentationrs):
/// the stage shows each screen and reports A (`RewardScreens`). NOT RETAIL YET: without the movies each screen is
/// accepted the frame it opens.
fn present(state: &mut CareerState, data: &Loaded, screens: &mut RewardScreens, runner_idle: bool) -> bool {
    use skate_core::career::presentation::Out;
    let unlocks = &state.career.unlocks;
    let summary = !state.career.summary.is_empty() || !unlocks.summary_big.is_empty() || !unlocks.summary_other.is_empty();
    let mut out = if runner_idle { state.presentation.runner_idle(summary) } else { Vec::new() };
    if std::mem::take(&mut screens.accepted) && screens.open.is_some() {
        out.extend(state.presentation.accept(state.career.pending));
    }
    let mut i = 0;
    while i < out.len() {
        let o = out[i];
        i += 1;
        match o {
            Out::Open(screen, index) => {
                if index == 0 {
                    screens.view = summary_view(state, data);
                    screens.generation += 1;
                }
                info!("SKATE_CAREER presentation open {screen:?} {index} big={:?} other={:?} totals={:?} pending={:?}{}",
                      state.career.unlocks.summary_big, state.career.unlocks.summary_other, state.career.summary,
                      state.career.pending, if screens.available { "" } else { " NOT RETAIL YET (no movie, accepted at once)" });
                if screens.available {
                    screens.open = Some((screen, index));
                } else {
                    out.extend(state.presentation.accept(state.career.pending));
                }
            }
            Out::Close(_) => {
                screens.open = None;
                info!("SKATE_CAREER presentation {o:?}");
            }
            Out::ClearOthers => {
                state.career.unlocks.summary_other.clear();
                state.career.summary = Default::default();
            }
            Out::ClearCards => {
                state.career.unlocks.summary_big.clear();
                state.career.pending = Default::default();
            }
            Out::PlayDeferred => state.awards.play(std::mem::take(&mut state.career.deferred)),
            Out::Started | Out::Done => info!("SKATE_CAREER presentation {o:?}"),
        }
    }
    i > 0
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
    let team = &state.career.team;
    profile.teammates = team.teammates.iter().take(team.count() - 1).map(|t| t.name.clone()).collect();
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
          challenges: Option<ResMut<crate::challenges::Challenges>>, mut screens: ResMut<RewardScreens>) {
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
        state.presentation.result(r.completion != Completion::Abandoned, false);
        apply_effects(state, &data, effects);
        changed = true;
    }
    changed |= std::mem::take(&mut state.dirty);
    let requests = state.career.update(&data.progression, &data.graphs);
    if !requests.is_empty() {
        settle(&mut state.career, &mut state.awards, &data, requests);
        changed = true;
    }
    let runner_idle = challenges.as_ref().is_none_or(|c| c.run.is_none() && c.tutorial.is_none());
    changed |= present(state, &data, &mut screens, runner_idle);
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

/// The looks of save skater slots 1.. for the living world (teammate stand-ins join the pool and wear them, b977).
fn publish_slot_looks(state: Res<CareerState>, looks: Option<ResMut<crate::living_world::npc_skaters::SaveSlotLooks>>, mut seen: Local<Option<u64>>) {
    let Some(mut looks) = looks else { return };
    if *seen == Some(state.looks_generation) {
        return;
    }
    *seen = Some(state.looks_generation);
    looks.0 = state.looks.slots.iter().enumerate().skip(1)
        .filter_map(|(i, s)| s.as_ref().map(|l| (i as u32, serde_json::Value::Object(l.profile.clone())))).collect();
    info!("SKATE_CAREER teammate looks in slots {:?}", looks.0.keys().collect::<Vec<_>>());
}

/// A new career's first world load queues its first challenge by the world and clears the pending flag (827CF960,
/// research b976 / b1000). Signing up starts it when the challenge is on that map (the tutorial on the skate school);
/// a challenge not ported yet is ignored there, as retail clears the flag either way.
fn queue_first_challenge(
    mut state: ResMut<CareerState>,
    map: Res<crate::map_transition::CurrentMap>,
    mut seen: Local<Option<u64>>,
    mut signed: MessageWriter<crate::challenges::ChallengeSignUp>,
) {
    if *seen == Some(map.generation) || state.data.is_none() || !state.file.boot.pending {
        return;
    }
    *seen = Some(map.generation);
    let worlds = skate_data::challenge_map::LocationRules::default();
    let world = worlds.0.iter()
        .find(|(label, _)| map.path.as_deref().is_some_and(|p| crate::teleport_menu::same_map(p, label)))
        .map(|(_, district)| district.as_str());
    let challenge = state.first_challenge.pick(world).to_owned();
    info!("SKATE_CAREER new career: first challenge {challenge} (world {world:?})");
    signed.write(crate::challenges::ChallengeSignUp { body: crate::trigger_volumes::PLAYER.into(), challenge });
    state.file.boot.pending = false;
    state.save_in_background();
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<CareerState>()
        .init_resource::<CareerProfile>()
        .init_resource::<RewardScreens>()
        .add_message::<ChallengeResult>()
        .add_message::<ChallengeDiscovered>()
        .add_systems(Update, update.after(crate::challenges::CareerResultSet).run_if(crate::challenges::opted_in))
        .add_systems(Update, queue_first_challenge.after(update).run_if(crate::challenges::opted_in))
        .add_systems(Update, publish_slot_looks.after(update).run_if(crate::challenges::opted_in));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_record_round_trips_and_old_saves_keep_their_joined_members() {
        let mut state = CareerState::default();
        state.file.team.members = vec![true, true, false, false, false];
        state.adopt();
        assert_eq!(state.career.team.members(), [true, true, false, false, false]);
        assert_eq!(state.career.team.name, "MyTeamName");

        state.career.team.name = "Shred Co".into();
        state.career.team.teammate_mut(2).unwrap().name = "Gerry".into();
        state.capture();
        let text = state.file.to_json();
        let mut again = CareerState::default();
        again.file = CareerFile::parse(&text).unwrap();
        again.adopt();
        assert_eq!(again.career.team.name, "Shred Co");
        assert_eq!(again.career.team.teammates[1].name, "Gerry");
        assert_eq!(again.career.team.teammates[1].character, "teammate_02");
        assert_eq!(again.career.team.members(), [true, true, false, false, false]);
    }
}