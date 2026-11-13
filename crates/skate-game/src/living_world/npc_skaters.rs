//! NPC skaters, replay tier: spawn records -> visible skaters riding their recorded lines.
//! Details: docs/hails-additions/26c-living-world-npc-skaters.md#npc-skater-replay-tier

use super::{LivingWorldDespawn, LivingWorldObservers, LivingWorldSettings, LivingWorldSpawn, NetRole, PopulationState};
use crate::world_audio::{AudioState, AudioVelocity, LiteSkater, NpcSkaterAudio};
use bevy::prelude::*;
use skate_core::living_world::replay::{BranchContext, BranchRecord, CursorEvent, Decider, LineCursor, PhaseEntry, ReplayLine, ReplayPhase, ReplaySample, TrickRecord};
use skate_core::living_world::npc_tricks::{TrickContext, TrickProfile};
use skate_core::living_world::leave_fade::LeaveFade;
use skate_core::living_world::stance::{StanceEvents, StanceFlags};
use skate_core::living_world::{DespawnReason, Kind, LivingWorldId, SpawnChoice};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Recording frames per world tick: the lines are recorded at 60 Hz ([data], `RECORDING_HZ`) and
/// the retail world tick that runs the skater manager is the 60 Hz fixed step
/// (`skate_core::living_world::clock::RETAIL_TICK_HZ`) [code], so one frame per tick.
pub(crate) const FRAMES_PER_TICK: u64 = 1;

/// Lines and per-character data of the loaded district, shared with the population.
#[derive(Clone, Default)]
pub(crate) struct NpcData {
    pub lines: Arc<BTreeMap<[u8; 16], ReplayLine>>,
    /// `characters_marquee` voice per character key (`skater_profiles.json`).
    pub voices: BTreeMap<String, u32>,
    /// Character record name per character key (`skater_profiles.json` `recipe`); a key without
    /// one is its own record name.
    pub records: BTreeMap<String, String>,
    /// The AI profiles' trick tables and each character's profile (`skater_profiles.json`).
    pub tricks: Arc<skate_data::living_world::SkaterTrickProfiles>,
}

/// The NPC skater trick choice in effect (`skate_core::living_world::npc_tricks`; retail: mode
/// profile, gate windows 300 / 50 frames).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct NpcTrickSettings {
    pub mode: skate_core::living_world::npc_tricks::TrickMode,
    pub params: skate_core::living_world::npc_tricks::TrickParams,
}

/// A mod's trick tables for a character or profile; `None` keeps the disc's table.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NpcTrickTables {
    pub regular: Option<Vec<(i16, f32)>>,
    pub nollie: Option<Vec<(i16, f32)>>,
}

/// `a` is `b` or a sequence `<x>+b` (no allocation; b620).
fn sequence_ends_in(a: &str, b: &str) -> bool {
    a.len() > b.len() && a.ends_with(b) && a.as_bytes()[a.len() - b.len() - 1] == b'+'
}

/// The trick tables an NPC uses: the disc's table of its `aiprofile` (else `default`), each table
/// replaced by a mod's for the character key, else for the profile name.
pub(crate) fn npc_trick_profile<'a>(data: &'a skate_data::living_world::SkaterTrickProfiles, overrides: &BTreeMap<String, NpcTrickTables>, character: &str) -> Option<std::borrow::Cow<'a, TrickProfile>> {
    let base = data.for_character(character);
    if overrides.is_empty() {
        return base.map(std::borrow::Cow::Borrowed);
    }
    let profile_name = data.character_profile.get(character).map_or("default", String::as_str);
    let by_key = overrides.get(character);
    let by_profile = overrides.get(profile_name);
    let pick = |f: fn(&NpcTrickTables) -> &Option<Vec<(i16, f32)>>, disc: Option<&Vec<(i16, f32)>>| {
        by_key.and_then(|o| f(o).clone()).or_else(|| by_profile.and_then(|o| f(o).clone())).or_else(|| disc.cloned())
    };
    let regular = pick(|o| &o.regular, base.as_ref().map(|b| &b.regular));
    let nollie = pick(|o| &o.nollie, base.as_ref().map(|b| &b.nollie));
    (regular.is_some() || nollie.is_some()).then(|| std::borrow::Cow::Owned(TrickProfile { regular: regular.unwrap_or_default(), nollie: nollie.unwrap_or_default() }))
}

/// A trick id for logs: the catalog name, `none` for -1.
pub(crate) fn trick_name(trick: i16) -> String {
    usize::try_from(trick).ok().and_then(|i| skate_core::scoring::catalog::IDENTIFIERS.get(i)).map_or_else(|| if trick < 0 { "none".to_owned() } else { trick.to_string() }, |t| t.0.to_owned())
}

/// The shipped natural stance table (`skate_core::living_world::stance::RETAIL_TABLE`), parsed once.
pub(crate) fn stance_table() -> &'static [skate_core::living_world::stance::StanceRow] {
    static TABLE: std::sync::OnceLock<Vec<skate_core::living_world::stance::StanceRow>> = std::sync::OnceLock::new();
    TABLE.get_or_init(skate_core::living_world::stance::retail_table)
}

/// An NPC's natural stance at spawn (82590DC0 -> Initialize82B97E38): record, mod override, table, default.
pub(crate) fn npc_stance(records: &BTreeMap<String, String>, overrides: &BTreeMap<String, skate_core::living_world::stance::NaturalStance>, character: &str) -> skate_core::living_world::stance::NaturalStance {
    let record = records.get(character).map_or(character, String::as_str);
    skate_core::living_world::stance::resolve(stance_table(), overrides, record)
}

/// One replay-tier NPC skater.
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct NpcSkater {
    pub id: LivingWorldId,
    pub character: String,
    pub slot: u8,
    pub seed: u64,
    pub voice: Option<u32>,
    pub spawn_tick: u64,
    pub start_line: [u8; 16],
    /// Natural stance, set once at spawn ([`npc_stance`]); regular draws the puppet mirrored.
    pub stance: skate_core::living_world::stance::NaturalStance,
}

#[derive(Component, Clone, Debug)]
pub(crate) struct NpcReplay {
    pub cursor: LineCursor,
    /// Branch decisions so far (what a host would send).
    pub branches: Vec<BranchRecord>,
    /// Trick choices so far (what a host would send; a client applies them).
    pub tricks: Vec<TrickRecord>,
    pub last: Option<ReplaySample>,
    /// The cursor one world tick back: the render draws from it towards `cursor` by the fixed-step
    /// fraction (the player's previous-to-current interpolation), so it never guesses a branch.
    pub previous: Option<LineCursor>,
    /// The obstacle avoider's state: cursor lag and lateral offset ([`super::npc_avoid`]).
    pub avoid: super::npc_avoid::NpcAvoid,
    /// While simulated: the cursor frame cursor B is at (`skate_core::living_world::path_follow`, retail's projection),
    /// used instead of the recording's clock. Wire state a client needs with `branches` / `tricks` to follow a simulated
    /// NPC's cursor (its clock, and `LineCursor::hold_at_end`).
    pub follow_target: Option<u64>,
    /// The simulated body's branch gate and point, set by the sim each tick (`None` in the replay tier).
    pub body_gate: Option<skate_core::living_world::replay::BodyGate>,
}

/// The NPC's leave fade and its opacity now (1 = solid). Engine systems and mods read `alpha` to
/// draw the fade; the removal follows from it (`skate_core::living_world::leave_fade`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct NpcFade {
    pub fade: LeaveFade,
    pub alpha: f32,
}

impl Default for NpcFade {
    fn default() -> Self {
        Self { fade: LeaveFade::default(), alpha: 1.0 }
    }
}

/// A challenge AI racer's clock (race_ai_host): frames it was held (PinAI) and frames the rubber band added or took
/// away; the cursor's target frame is the spawn clock minus `held` plus `extra` (replay tier). `moving` = released
/// (UnpinAI); `scale` = the speed shape's multiplier for the band (`race_ai::shaped_speed` of 1), which the physics
/// tier applies to its push speed (0 = not set yet).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct NpcRaceClock {
    pub held: u64,
    pub extra: i64,
    pub moving: bool,
    pub scale: f32,
    /// The racer's start slot (position, heading): it is placed there as a full skater and held while pinned (b972 §4).
    pub start: Option<([f32; 3], f32)>,
}

impl NpcRaceClock {
    pub(crate) fn speed_scale(&self) -> f32 {
        if self.scale > 0.0 { self.scale } else { 1.0 }
    }
}

/// The lines a challenge AI skater may take at a branch or line end (its challenge's set, sorted), instead of the
/// lines valid for its character.
#[derive(Component, Clone, Debug)]
pub(crate) struct NpcLineFilter(pub Arc<Vec<[u8; 16]>>);

/// LivingWorldId -> entity.
#[derive(Resource, Default)]
pub(crate) struct NpcSkaterIndex(pub BTreeMap<LivingWorldId, Entity>);

/// Looks of the save skater slots 1.. (the career's `cac.json`): a teammate stand-in whose slot has one joins the pool
/// (`SkaterData::filled_slots`) and wears it, assembled from the customizer's parts. Written by the career.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct SaveSlotLooks(pub BTreeMap<u32, serde_json::Value>);

/// Mod / engine look overrides: character key -> GLB asset path. Empty = retail looks.
#[derive(Resource, Default, Clone)]
pub(crate) struct NpcSkaterLooks(pub BTreeMap<String, String>);

/// The model an NPC skater of `character` loads: a mod's look, else the online roster's, else the stock skater
/// (`present_looks` and the living world preload).
pub(crate) fn look_path(looks: &NpcSkaterLooks, models: &crate::custom_models::CustomModels, character: &str) -> String {
    looks.0.get(character).cloned().or_else(|| models.online_native_path(character)).unwrap_or_else(|| "private/skater.glb".to_owned())
}

/// What happened to an NPC skater (engine systems, mods, speech).
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) enum NpcSkaterEvent {
    Spawned { id: LivingWorldId, character: String, line: [u8; 16] },
    Despawned { id: LivingWorldId, reason: DespawnReason },
    Node { id: LivingWorldId, line: [u8; 16], node: u32, event: u8, flags: u8 },
    Branch { id: LivingWorldId, record: BranchRecord },
    /// The trick chosen at a start-trick node (`record.recorded` = the line's own).
    Trick { id: LivingWorldId, record: TrickRecord },
    LineEnd { id: LivingWorldId },
    /// A simulated NPC skater bailed; it is placed back on its line after `respawn_seconds`.
    Bail { id: LivingWorldId, respawn_seconds: f32 },
    /// The bail respawn placed it back on its line at `node`.
    Respawned { id: LivingWorldId, node: u32 },
    /// Mode 7: controller B (NavMeshController) starts walking the skater back to a line node, or hands back
    /// to the line controller there (`started` false).
    WalkBack { id: LivingWorldId, node: u32, started: bool },
    /// The obstacle avoider changed mode (target: obstacle kind and id).
    Avoid { id: LivingWorldId, mode: skate_core::living_world::avoid::AvoidMode, target: Option<(skate_core::living_world::avoid::ObstacleKind, u64)> },
    /// A simulated NPC skater banked a sequence (retail cMsgSequenceComplete from that skater's own score state:
    /// 82DF18B0 -> 82DF0878 run for every skater, b1009 §0): its score and whether it ended in a bail.
    SequenceComplete { id: LivingWorldId, score: i32, wiped_out: bool, tricks: [u16; crate::scoring_runtime::SEQUENCE_CATEGORIES], flipped: bool },
    /// A simulated NPC skater started a sequence (retail cMsgSequenceBegin from its own score state).
    SequenceBegin { id: LivingWorldId },
}

/// The stock clip a replay NPC shows per phase ([data]: names checked against the decoded stock
/// clip list, `crate::living_world` data-gated test). Riding uses the pro's style like the
/// customizer (`native_animation_style`).
pub(crate) fn puppet_clip(phase: ReplayPhase, style: &str) -> &'static str {
    match phase {
        ReplayPhase::Rolling => match style {
            "Aggressive" => "R_IDLE_RIDE_AGGR_0_CYC",
            "Loose" => "R_IDLE_RIDE_LOOSE_0_CYC",
            _ => "R_IDLE_RIDE_N_0_CYC",
        },
        ReplayPhase::Crouched => "R_IDLE_LCOM_000",
        ReplayPhase::Air => "IA_IDLE_N_N_0_CYC",
        ReplayPhase::AirTrick => "IA_IDLE_LO_N_0_CYC",
        ReplayPhase::GroundTrick => "G_5050_FS_LOW_0_CYC",
        ReplayPhase::OffBoard => "BR_STAND_0_CYC",
    }
}

/// The clip in effect: a mod override for `<phase>.<style>`, then for `<phase>`, else the shipped
/// pick ([`puppet_clip`]).
pub(crate) fn resolve_puppet_clip<'a>(overrides: &'a BTreeMap<String, String>, phase: ReplayPhase, style: &str) -> &'a str {
    if overrides.is_empty() {
        return puppet_clip(phase, style);
    }
    overrides
        .get(&format!("{}.{style}", phase.name()))
        .or_else(|| overrides.get(phase.name()))
        .map_or_else(|| puppet_clip(phase, style), String::as_str)
}

/// Stock naming: cyclic clips carry `_CYC` (they loop); others play once and hold.
pub(crate) fn clip_loops(clip: &str) -> bool {
    clip.to_ascii_uppercase().contains("_CYC")
}

/// Sample time of `clip` after `time` s in its phase: wrapped by `length` when the clip loops.
pub(crate) fn puppet_clip_time(clip: &str, time: f32, length: f32) -> f32 {
    if clip_loops(clip) && length > 0.0 { time.rem_euclid(length) } else { time.max(0.0) }
}

/// Transition time into a phase clip: the 0.2 s literal at 0x82099280 and the stock idle states: docs/hails-additions/26c-living-world-npc-skaters.md#retail-blend-seconds
pub(crate) const RETAIL_BLEND_SECONDS: f32 = 0.2;

/// Crossfade time into `to`'s clip: a mod's value for the phase id, then for `default`, else
/// [`RETAIL_BLEND_SECONDS`].
pub(crate) fn blend_seconds(overrides: &BTreeMap<String, f32>, to: ReplayPhase) -> f32 {
    overrides.get(to.name()).or_else(|| overrides.get("default")).copied().unwrap_or(RETAIL_BLEND_SECONDS)
}

/// Takeoff transition into a trick's ground clip: [data] `T_Trick.xml` / `T_Ollie.xml`
/// `Takeoff/FromAntic` and `FromManual` `PlayAnimation anim="$ANIM_NAME$_G" time="0.05"`.
pub(crate) const RETAIL_TRICK_TAKEOFF_SECONDS: f32 = 0.05;
/// Into a trick's air clip without its ground clip before it (a trick started in the air):
/// [data] `T_Trick.xml` `GrindOutAssist/LeftGround` `time="0.1"`. After the ground clip the air
/// clip follows as a sequence (`LeftGround/Default` `transType="sequence"`: no blend, 0 s).
pub(crate) const RETAIL_TRICK_AIR_SECONDS: f32 = 0.1;

/// Stock trick animation base for a recorded `EScorableID` (Tricks.xml pairs): docs/hails-additions/26c-living-world-npc-skaters.md#retail-trick-anim
pub(crate) fn retail_trick_anim(trick: i16) -> Option<String> {
    let (name, class, _) = *skate_core::scoring::catalog::IDENTIFIERS.get(usize::try_from(trick).ok()?)?;
    /// `TRICK_NAME`s of `Tricks.xml` with a `T_Trick` / `T_TrickWithUnderflip` /
    /// `T_TrickWithDarkCatch` include (`ANIM_NAME` = `B_<NAME>`), each also as `N_<NAME>`.
    const FLIPS: [&str; 12] = ["popshuvit", "fspopshuvit", "varialkickflip", "varialheelflip", "hardflip", "inwardheelflip", "360popshuvit", "fs360popshuvit", "360flip", "laserflip", "360hardflip", "360inwardheelflip"];
    match class {
        3 => {
            let base = name.trim_end_matches("_underflip").trim_end_matches("_darkcatch").trim_end_matches(|c: char| c.is_ascii_digit());
            let plain = base.strip_prefix("n_").unwrap_or(base);
            Some(if matches!(plain, "kickflip" | "heelflip") {
                // `T_Kickflip.xml` includes: `ANIM_NAME="B_<NAME>_IN"`.
                format!("B_{}_IN", base.to_ascii_uppercase())
            } else if FLIPS.contains(&plain) {
                format!("B_{}", base.to_ascii_uppercase())
            } else if base.starts_with("n_") {
                "B_NOLLIE".to_owned()
            } else {
                // Late flips, dark catches out of other tricks: the takeoff is an ollie.
                "B_OLLIE".to_owned()
            })
        }
        4 if name == "nollie" => Some("B_NOLLIE".to_owned()),
        2 | 4 => Some("B_OLLIE".to_owned()),
        _ => None,
    }
}

/// The trick animation in effect for a recorded trick: a mod's `skater_clips["trick.<name>"]`
/// (the catalog's identifier, e.g. `trick.kickflip`; value = an animation base without `_G`/`_A`),
/// else [`retail_trick_anim`].
pub(crate) fn resolve_trick_anim(overrides: &BTreeMap<String, String>, trick: i16) -> Option<String> {
    let name = skate_core::scoring::catalog::IDENTIFIERS.get(usize::try_from(trick).ok()?)?.0;
    overrides.get(&format!("trick.{name}")).cloned().or_else(|| retail_trick_anim(trick))
}

/// Clip and transition time a remembered phase shows (trick `_G` / `_A`, else the phase clip): docs/hails-additions/26c-living-world-npc-skaters.md#puppet-layer-clip
pub(crate) fn puppet_layer_clip(
    clips: &BTreeMap<String, String>,
    blends: &BTreeMap<String, f32>,
    style: &str,
    evaluates: &dyn Fn(&str) -> bool,
    stock: &dyn Fn(&str) -> Option<String>,
    scored: &dyn Fn(PhaseEntry, bool) -> Option<(String, f32)>,
    e: PhaseEntry,
    older: Option<PhaseEntry>,
) -> (String, f32) {
    let phase_clip = || {
        let clip = resolve_puppet_clip(clips, e.phase, style);
        let clip = if evaluates(clip) { clip } else { puppet_clip(e.phase, style) };
        (clip.to_owned(), blend_seconds(blends, e.phase))
    };
    let tuned = |key: &str, retail: f32| blends.get(key).copied().unwrap_or(retail);
    // Each `+` part through `stock` (authored tree name -> clip); `None` when any part is missing.
    let trick_clip = |entry: PhaseEntry, suffix: &str| {
        let base = resolve_trick_anim(clips, entry.trick)?;
        let names = if suffix == "_A" { trick_air_sequence(&base, entry.trick) } else { format!("{base}{suffix}") };
        names.split('+').map(stock).collect::<Option<Vec<_>>>().map(|v| v.join("+"))
    };
    // A grab / grind slot plays the trees of the stock graph state it scores, unless a mod names its animation.
    let scored = |entry: PhaseEntry, landed: bool| {
        let name = skate_core::scoring::catalog::IDENTIFIERS.get(usize::try_from(entry.trick).ok()?)?.0;
        if !clips.is_empty() && clips.contains_key(&format!("trick.{name}")) { None } else { scored(entry, landed) }
    };
    // The span's air phase before this ground one: the trick landed still open (no new takeoff).
    let air_before = older.filter(|o| e.phase == ReplayPhase::GroundTrick && o.phase == ReplayPhase::AirTrick && o.trick == e.trick && o.trick_since == e.trick_since);
    if let Some(c) = match e.phase {
        // Landed with the span open: the scored hold, else the air clip continues (one layer).
        ReplayPhase::GroundTrick if air_before.is_some() => scored(e, true).or_else(|| air_before.and_then(|o| scored(o, false)).map(|(c, _)| (c, 0.0))),
        ReplayPhase::GroundTrick | ReplayPhase::AirTrick => scored(e, false),
        ReplayPhase::Air => older.filter(|o| o.phase == ReplayPhase::AirTrick).and_then(|o| scored(o, false)).map(|(c, _)| (c, 0.0)),
        _ => None,
    } {
        return c;
    }
    match e.phase {
        // The takeoff: `_G`, then the air part as a sequence (`LeftGround/Default`
        // `transType="sequence"` starts it when the ground clip ends, not when the recorder's
        // airborne flag comes). [`puppet_layers`] continues this clip through the air phases.
        ReplayPhase::GroundTrick => match (trick_clip(e, "_G"), trick_clip(e, "_A")) {
            (Some(g), Some(a)) => (format!("{g}+{a}"), tuned("trick_takeoff", RETAIL_TRICK_TAKEOFF_SECONDS)),
            _ => phase_clip(),
        },
        // Without its takeoff before it (a trick started in the air): the air part, 0.1 s in.
        ReplayPhase::AirTrick => trick_clip(e, "_A").map_or_else(phase_clip, |c| (c, tuned("trick_air", RETAIL_TRICK_AIR_SECONDS))),
        // The air after an air trick keeps the trick's clip (merged into its layer).
        ReplayPhase::Air => older.filter(|o| o.phase == ReplayPhase::AirTrick).and_then(|o| trick_clip(o, "_A")).map_or_else(phase_clip, |c| (c, 0.0)),
        _ => phase_clip(),
    }
}

/// Air part of a trick animation base (`_A`, kickflip family IN / CYC / OUT sequence): docs/hails-additions/26c-living-world-npc-skaters.md#trick-air-sequence
pub(crate) fn trick_air_sequence(base: &str, trick: i16) -> String {
    let Some(flip) = base.strip_suffix("_IN") else { return format!("{base}_A") };
    let name = usize::try_from(trick).ok().and_then(|t| skate_core::scoring::catalog::IDENTIFIERS.get(t)).map_or("", |e| e.0);
    let digits = name.trim_end_matches("_underflip").trim_end_matches("_darkcatch");
    let n: usize = digits.chars().last().and_then(|c| c.to_digit(10)).map_or(1, |d| d as usize).clamp(1, 4);
    let mut parts = vec![format!("{base}_A")];
    parts.extend((1..=n.min(3)).map(|i| format!("{flip}_CYC{i}")));
    parts.push(format!("{flip}_OUT{n}"));
    parts.join("+")
}

/// The clip a stock authored tree plays for an NPC: a clip is itself, a selector its default
/// child, a `TRICKHEIGHT` phase blend the child retail's AI height picks (the perform step always posts TrickHeight
/// 1.0 normalized, `sub_82469448` -> `sub_82470DD8`, research b690: the highest child value), any other phase blend
/// its first child. Blend and selection spaces: none.
pub(crate) fn stock_tree_leaf(meta: &skate_data::animation_metadata::AnimationMetadata, name: &str) -> Option<String> {
    use skate_data::animation_metadata::TreeMetadata;
    let mut name = name.to_owned();
    for _ in 0..8 {
        name = match meta.tree(&name).ok()? {
            TreeMetadata::Clip(c) => return Some(c.name.clone()),
            TreeMetadata::Selector(s) => s.default.clone(),
            TreeMetadata::PhaseBlend(p) if p.parameter.eq_ignore_ascii_case("TRICKHEIGHT") => highest_child(meta, &p.children, &p.parameter).or_else(|| p.children.first().cloned())?,
            TreeMetadata::PhaseBlend(p) => p.children.first()?.clone(),
            _ => return None,
        };
    }
    None
}

/// The phase blend child with the highest `parameter` value (`PhaseBlend` sorts its children by the value each child
/// reports, `phase_blend.rs`); `None` when a child is not a clip or carries no value.
fn highest_child(meta: &skate_data::animation_metadata::AnimationMetadata, children: &[String], parameter: &str) -> Option<String> {
    use skate_data::animation_metadata::TreeMetadata;
    let mut best: Option<(f32, &String)> = None;
    for child in children {
        let TreeMetadata::Clip(c) = meta.tree(child).ok()? else { return None };
        let value = f32::from_bits(*c.attributes.iter().find(|a| a.name.eq_ignore_ascii_case(parameter))?.payload_words.first()?);
        if best.is_none_or(|(v, _)| value > v) {
            best = Some((value, child));
        }
    }
    best.map(|(_, c)| c.clone())
}

/// [`stock_tree_leaf`] with attribute targets: a blend space, or a phase blend over a `targets` parameter, takes the
/// child nearest the target ([`nearest_child`]); everything else as [`stock_tree_leaf`].
pub(crate) fn stock_tree_leaf_at(meta: &skate_data::animation_metadata::AnimationMetadata, name: &str, targets: &[(&str, f32)]) -> Option<String> {
    use skate_data::animation_metadata::TreeMetadata;
    let target = |p: &str| targets.iter().find(|(n, _)| n.eq_ignore_ascii_case(p)).map(|t| t.1);
    let mut name = name.to_owned();
    for _ in 0..8 {
        let (parameters, children): (Vec<&str>, &[String]) = match meta.tree(&name).ok()? {
            TreeMetadata::BlendSpace(b) => (b.parameters.iter().map(String::as_str).collect(), &b.children),
            TreeMetadata::PhaseBlend(p) if target(&p.parameter).is_some() => (vec![p.parameter.as_str()], &p.children),
            _ => return stock_tree_leaf(meta, &name),
        };
        let goal: Vec<f32> = parameters.iter().map(|p| target(p).unwrap_or(0.0)).collect();
        name = nearest_child(meta, children, &parameters, &goal)?;
    }
    None
}

/// The child whose clip values of `parameters` lie nearest `goal`; when a child carries none (a sub-tree), the first
/// parameter picks by rank: the middle child for 0, the first below it, the last above.
fn nearest_child(meta: &skate_data::animation_metadata::AnimationMetadata, children: &[String], parameters: &[&str], goal: &[f32]) -> Option<String> {
    use skate_data::animation_metadata::TreeMetadata;
    let value = |child: &str, p: &str| match meta.tree(child).ok()? {
        TreeMetadata::Clip(c) => Some(f32::from_bits(*c.attributes.iter().find(|a| a.name.eq_ignore_ascii_case(p))?.payload_words.first()?)),
        _ => None,
    };
    let mut best: Option<(f32, &String)> = None;
    for child in children {
        let Some(d) = parameters.iter().zip(goal).map(|(p, g)| value(child, p).map(|v| (v - g).powi(2))).sum::<Option<f32>>() else {
            best = None;
            break;
        };
        if best.is_none_or(|(b, _)| d < b) {
            best = Some((d, child));
        }
    }
    if let Some((_, c)) = best {
        return Some(c.clone());
    }
    let g = *goal.first()?;
    let i = if g == 0.0 { children.len() / 2 } else if g < 0.0 { 0 } else { children.len().checked_sub(1)? };
    children.get(i).cloned()
}

/// Grab slots the stock graphs score as another grab: the AI posts tweaks and a grab hand (board adjust only for
/// mute / seatbelt), so e.g. nosegrab plays the BS grab tweaked down. Proven against the stock graphs by
/// `air_playback.rs` `ai_grab_schedules_through_stock_graphs`: docs/hails-additions/26c-living-world-npc-skaters.md#npc-grab-scores
pub(crate) const RETAIL_GRAB_SCORES: [(&str, &str); 33] = [
    ("gnd_bsgrab", "bsgrab"), ("gnd_dblgrab", "dblgrab"), ("gnd_fsgrab", "fsgrab"),
    ("mutegrab_down", "bsgrab_down"), ("mutegrab_left", "bsgrab_left"), ("mutegrab_up", "bsgrab_up"),
    ("nosegrab", "bsgrab_down"), ("nosegrab_left", "bsgrab_left"), ("nosegrab_right", "bsgrab_down"),
    ("rocketair", "dblgrab"),
    ("stalegrab", "fsgrab_right"), ("stalegrab_down", "fsgrab_down"), ("stalegrab_left", "fsgrab_left"), ("stalegrab_up", "fsgrab_up"),
    ("tailgrab", "bsgrab_up"), ("tailgrab_left", "bsgrab_left"), ("tailgrab_right", "bsgrab_up"),
    ("fs_onefoot_right", "fsgrab"), ("fs_backfoot", "fsgrab"), ("bs_onefoot_front", "bsgrab"), ("bs_onefoot_behind", "bsgrab"),
    ("nosegrab_onefoot_front", "bs_onefoot_front"), ("nosegrab_onefoot_behind", "bs_onefoot_behind"),
    ("tailgrab_onefoot_front", "bs_onefoot_behind"), ("tailgrab_onefoot_behind", "bs_onefoot_front"),
    ("tailgrab_airwalk", "christ_air"), ("nosegrab_airwalk", "christ_air"),
    ("crailgrab", "fsgrab_down"), ("crailgrab_left", "fsgrab_left"), ("crailgrab_right", "fsgrab_down"),
    ("seatbeltgrab", "bsgrab"), ("seatbeltgrab_left", "bsgrab"), ("seatbeltgrab_right", "bsgrab"),
];

/// Grabs the ground graph plays (`GroundGrabs/*.xml`): on the ground they keep their own score.
const GROUND_GRABS: [&str; 4] = ["gnd_bsgrab", "gnd_dblgrab", "gnd_fsgrab", "coffin"];

/// The score a grab slot reaches: on the ground a ground grab's own, else [`RETAIL_GRAB_SCORES`], else its own name.
pub(crate) fn retail_grab_score(name: &str, on_ground: bool) -> &str {
    if on_ground && GROUND_GRABS.contains(&name) {
        return name;
    }
    RETAIL_GRAB_SCORES.iter().find(|(n, _)| *n == name).map_or(name, |(_, s)| s)
}

/// Clip sequence and blend time of a grab (category 4) or grind / slide (category 5) slot in `e`'s phase, from the
/// stock motion graph's score index (`leaf`: tree + attribute targets -> playable clip):
/// docs/hails-additions/26c-living-world-npc-skaters.md#npc-grab-scores
pub(crate) fn scored_trick_clip(
    scores: &BTreeMap<String, crate::graph_host::motion::ScoreClips>,
    leaf: &dyn Fn(&str, &[(&str, f32)]) -> Option<String>,
    grind_disttocog: f32,
    manual_angle: &dyn Fn(f32) -> f32,
    e: PhaseEntry,
    landed: bool,
) -> Option<(String, f32)> {
    let (name, _, category) = *skate_core::scoring::catalog::IDENTIFIERS.get(usize::try_from(e.trick).ok()?)?;
    match (category, e.phase) {
        // An air grab still open at touchdown: the FS / BS / DBL Cyc takes the ground grab hold
        // (`T_FSBSGrab.xml` / `DBLGrab.xml` Cyc -> `OnGround...Grabbing.<X>.<X>Cyc`, the AI posting Ground<X>Grab on the
        // first ground tick, research b710); mute / stale have none: docs/hails-additions/26c-living-world-npc-skaters.md#npc-grab-touchdown
        (4, ReplayPhase::GroundTrick) if landed => {
            let air = retail_grab_score(name, false);
            let family = ["fsgrab", "bsgrab", "dblgrab"].into_iter().find(|f| air.starts_with(f))?;
            let c = scores.get(&format!("gnd_{family}"))?;
            Some((leaf(&c.cyc, &[("ANGLE", 0.0)])?, c.cyc_seconds?))
        }
        (4, ReplayPhase::GroundTrick | ReplayPhase::AirTrick) => {
            let on_ground = e.phase == ReplayPhase::GroundTrick && GROUND_GRABS.contains(&name);
            let c = scores.get(retail_grab_score(name, on_ground))?;
            // ScoringGrabs' direction windows (`select_grab_score`): the unit tweak of the score's direction.
            let (x, y) = match c.direction {
                Some("up") => (0.0, 1.0),
                Some("left") => (-1.0, 0.0),
                Some("down") => (0.0, -1.0),
                Some("right") => (1.0, 0.0),
                _ => (0.0, 0.0),
            };
            let targets = [("TWEAK_X", x), ("TWEAK_Y", y), ("ANGLE", 0.0)];
            // The Into at its state's `playBackSpeed` ([`part_speed`]).
            let into = match c.into.as_deref() {
                Some(t) => Some(with_speed(leaf(t, &targets)?, c.into_speed)),
                None => None,
            };
            let grab = into.into_iter().chain(Some(leaf(&c.cyc, &targets)?)).collect::<Vec<_>>().join("+");
            Some(match e.phase {
                // `GroundGrabs/T_Grab.xml` Into `time="0.1"`.
                ReplayPhase::GroundTrick if on_ground => (grab, 0.1),
                // An air grab's takeoff is the ollie's `_G`; the grab follows as a sequence.
                ReplayPhase::GroundTrick => (format!("{}+{grab}", leaf("B_OLLIE_G", &[])?), RETAIL_TRICK_TAKEOFF_SECONDS),
                _ => (grab, RETAIL_TRICK_AIR_SECONDS),
            })
        }
        // `T_Grind.xml` `PlayAnimation time="0.3"`; neutral twist (ours), the uncrouched rest height:
        // docs/hails-additions/26c-living-world-npc-skaters.md#npc-grind-height
        (5, ReplayPhase::GroundTrick) => Some((leaf(&scores.get(name)?.cyc, &[("TWIST", 0.0), ("DISTTOCOG", grind_disttocog)])?, 0.3)),
        // Manuals rolling forwards (`Manual/NoseManual.xml` Into 0.1 then `B_NOSE_MANUAL` 0.3 with SetManualAngle;
        // `TailManual.xml` `B_TAIL_MANUAL_LOW` 0.3 unless rolling backwards): docs/hails-additions/26c-living-world-npc-skaters.md#npc-manuals
        (8, ReplayPhase::GroundTrick) => {
            let value = skate_core::living_world::ai_signals::SignalSettings::default().manual_value;
            let targets = [("MANUAL_ANGLE", manual_angle(value)), ("DISTTOCOG", 1.0e6), ("ANGLE", 0.0)];
            match name {
                "nosemanual" => Some((format!("{}+{}", leaf("B_NOSE_MANUAL_INTO", &targets)?, leaf("B_NOSE_MANUAL", &targets)?), 0.1)),
                "tailmanual" => Some((leaf("B_TAIL_MANUAL_LOW", &targets)?, 0.3)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The stock motion graph's score index ([`crate::graph_host::motion::stock_score_clips`]), built once from the loaded
/// graph so a graph mod changes NPC grabs like the player's; empty = the shipped picks.
#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct NpcScoreClips(pub BTreeMap<String, crate::graph_host::motion::ScoreClips>);

/// One clip of the puppet's nested crossfade: `weight` is how far it has blended in over
/// everything older (the oldest layer is the base, its weight is unused).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PuppetLayer {
    pub clip: String,
    /// Seconds since the clip began (its phase start, or the first of the phases it continues).
    pub time: f32,
    pub weight: f32,
    /// Cursor frame the layer began (its phase entry's `since`): the layer's identity, which
    /// keys the stance bits it was built with ([`NpcStanceTrack`]). 0 where no cursor drives it.
    pub since: u64,
    /// An air grab's `Cyc` tree, played in place of the sequence's last clip ([`grab_tree`]).
    pub tree: Option<PuppetTree>,
}

/// An air grab layer's `Cyc` tweak tree: docs/hails-additions/26c-living-world-npc-skaters.md#npc-grab-tweaks
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PuppetTree {
    pub name: String,
    /// Seconds into the tree.
    pub time: f32,
    /// `tweak_x` / `tweak_y` unmirrored (a mirrored layer negates x: `air.xml` TweakX `mirrorFilter="negate"`).
    pub tweak: [f32; 2],
}

/// Ticks [`grab_tweak`] eases over at most (the filter has long settled).
const TWEAK_TICKS: u64 = 600;

/// The `Cyc` state's tweaks `ticks` (fractional) after its entry: TweakX / TweakY from the slot `schedule` at slot tick
/// `entry_tick + m`, eased per 60 Hz tick by the state's `FilterMotionGraphIntent`s from Begin (no stored state, so a
/// scrub or a late peer gets the same value), then `TweakProject`'s clamp. From slot tick `end` (the slot is over) the
/// AG copy is gone and the filter eases out: docs/hails-additions/26c-living-world-npc-skaters.md#npc-grab-tweaks
pub(crate) fn grab_tweak(filter: &crate::graph_host::motion::TweakFilter, schedule: &[skate_core::living_world::ai_signals::IntentTuple], entry_tick: i64, ticks: f32, end: Option<i64>) -> [f32; 2] {
    let ticks = ticks.max(0.0);
    let n = (ticks.floor() as u64).min(TWEAK_TICKS);
    let mut steps = tweak_steps(filter, schedule, entry_tick, end);
    let now = steps.nth(n as usize).unwrap_or([0.0; 2]);
    let next = steps.next().unwrap_or(now);
    let a = if n == TWEAK_TICKS { 0.0 } else { ticks.fract() };
    let mut v = [now[0] + (next[0] - now[0]) * a, now[1] + (next[1] - now[1]) * a];
    let total = v[0].abs() + v[1].abs();
    if filter.project && total > 1.0 {
        v = [v[0] / total, v[1] / total];
    }
    v
}

/// The filtered (TweakX, TweakY) after each Cyc tick 0, 1, ... (Begin, then one update per tick; see [`grab_tweak`]).
fn tweak_steps<'a>(filter: &'a crate::graph_host::motion::TweakFilter, schedule: &'a [skate_core::living_world::ai_signals::IntentTuple], entry_tick: i64, end: Option<i64>) -> impl Iterator<Item = [f32; 2]> + 'a {
    use skate_core::animation::intent_filter::State;
    // `slot_signals`: the first 6 tuples, present from start to end (end < 0 = open); the AG copy defaults to 0.
    let input = move |name: &str, t: i64| {
        if end.is_some_and(|e| t >= e) {
            return None;
        }
        Some(schedule.iter().take(6).filter(|e| skate_core::input::skater_intent::name(e.intent) == Some(name) && t >= i64::from(e.start) && (e.end < 0 || t <= i64::from(e.end))).last().map_or(0.0, |e| e.value))
    };
    let (mut x, mut y) = (State::default(), State::default());
    x.begin(&filter.x);
    y.begin(&filter.y);
    (0..=TWEAK_TICKS + 1).map(move |m| {
        let t = entry_tick + m as i64;
        [x.update(&filter.x, input("TweakX", t), 1.0 / 60.0, (false, false)), y.update(&filter.y, input("TweakY", t), 1.0 / 60.0, (false, false))]
    })
}

/// The Cyc tick the Out begins: the first update after the slot's `end` whose filtered tweak is below the release
/// level ([`crate::graph_host::motion::Release`]) moves the graph on the next update (research b708).
pub(crate) fn grab_release_tick(filter: &crate::graph_host::motion::TweakFilter, schedule: &[skate_core::living_world::ai_signals::IntentTuple], entry_tick: i64, end: i64) -> Option<u64> {
    let r = filter.release?;
    let from = (end - entry_tick).max(0) as usize;
    tweak_steps(filter, schedule, entry_tick, Some(end)).enumerate().skip(from).find(|(_, [x, y])| if r.y_only { y.abs() < r.below } else { x.hypot(*y) < r.below }).map(|(m, _)| m as u64 + 1)
}

/// The tweak tree of an air grab layer the score index plays (no mod clip): the `Cyc` tree once the sequence reaches
/// its last part, with [`grab_tweak`] from the Cyc entry (the Into's end).
/// What follows an air grab layer: the Out and the air pose after it (inserted after the grab while they began before
/// the next layer), or at a touchdown with the Cyc still active the grab landing in place of the touchdown layer (with
/// the seconds it plays before that layer takes over).
#[derive(Default)]
struct GrabFollow {
    after: Vec<PuppetLayer>,
    landing: Option<(PuppetLayer, f32)>,
}

fn grab_tree(clips: &BTreeMap<String, String>, scores: &NpcScoreClips, skater: &crate::physics::SkaterRuntime, history: &[(PhaseEntry, f32)], l: &PuppetLayer, air: (&str, f32)) -> Option<(PuppetTree, GrabFollow)> {
    let i = history.iter().position(|(e, _)| e.since == l.since)?;
    let e = match history[i].0 {
        e if e.phase == ReplayPhase::Air => history.get(i + 1).map(|h| h.0).filter(|o| o.phase == ReplayPhase::AirTrick)?,
        e => e,
    };
    let (name, _, category) = *skate_core::scoring::catalog::IDENTIFIERS.get(usize::try_from(e.trick).ok()?)?;
    if category != 4 || !matches!(e.phase, ReplayPhase::GroundTrick | ReplayPhase::AirTrick) || (!clips.is_empty() && clips.contains_key(&format!("trick.{name}"))) {
        return None;
    }
    let c = scores.0.get(retail_grab_score(name, e.phase == ReplayPhase::GroundTrick && GROUND_GRABS.contains(&name)))?;
    let filter = c.tweak.as_ref()?;
    let (body, last) = l.clip.rsplit_once('+').unwrap_or(("", &l.clip));
    let evaluator = &skater.animation.evaluator;
    let prefix = if body.is_empty() { 0.0 } else { body.split('+').map(|p| part_speed(p)).map(|(p, speed)| evaluator.clip_length(p).ok().map(|l| l / speed)).sum::<Option<f32>>()? };
    let last = part_speed(last).0;
    let time = l.time - prefix;
    if time < 0.0 || !tree_holds(skater.animation.motion.animation.metadata(), &c.cyc, last, 4) {
        return None;
    }
    let schedule = skater.scoring.data.by_id(usize::try_from(e.trick).ok()?).map_or(&[][..], |d| d.intents.as_slice());
    let entry = (prefix * 60.0).ceil();
    let entry_tick = l.since as i64 + entry as i64 - e.trick_since as i64;
    // The span's end (END_TRICK: the slot and the grab intent end), from the first newer phase outside it.
    let end = history[..i].iter().rev().map(|h| h.0).find(|h| h.since > e.since && !matches!(h.phase, ReplayPhase::AirTrick | ReplayPhase::GroundTrick));
    let end_tick = end.map(|h| h.since as i64 - e.trick_since as i64);
    let tree = PuppetTree { name: c.cyc.clone(), time, tweak: grab_tweak(filter, schedule, entry_tick, l.time * 60.0 - entry, end_tick) };
    let meta = skater.animation.motion.animation.metadata();
    let weight = skate_core::animation::playback_transition::transition_weight;
    let mut follow = GrabFollow::default();
    // Released in the air: the Out at its speed once the eased tweak drops below the release level, then the air pose
    // (Out -> InAir on `WillExpire`).
    let release = end.filter(|h| h.phase == ReplayPhase::Air).and_then(|_| Some(entry + grab_release_tick(filter, schedule, entry_tick, end_tick?)? as f32));
    if let Some(start) = release {
        let out_time = (l.time * 60.0 - start) / 60.0;
        let out = c.out.as_deref().and_then(|o| stock_tree_leaf_at(meta, o, &[("MAGNITUDE", 0.0)]));
        if let (true, Some(out), Some(blend)) = (out_time >= 0.0, out, c.out_seconds) {
            let length = skater.animation.evaluator.clip_length(&out).ok().map(|n| n / c.out_speed.unwrap_or(1.0));
            follow.after.push(PuppetLayer { clip: with_speed(out, c.out_speed), time: out_time, weight: weight(out_time, blend), since: l.since + start as u64, tree: None });
            if let (Some(length), Some(expire)) = (length, c.out_expire) {
                let air_time = out_time - (length - expire).max(0.0);
                if air_time >= 0.0 {
                    let since = l.since + start as u64 + ((length - expire).max(0.0) * 60.0) as u64;
                    follow.after.push(PuppetLayer { clip: air.0.to_owned(), time: air_time, weight: weight(air_time, air.1), since, tree: None });
                }
            }
        }
    }
    // Touchdown with the Cyc still active (the span ended, no Out yet, past the Into): the grab landing; a tweak at the
    // WipeOut level never reaches touchdown on the shipped lines (survey `living_world_npc_grab_touchdowns_on_the_shipped_lines`).
    let touch = history[..i].iter().rev().find(|h| h.0.since > e.since && matches!(h.0.phase, ReplayPhase::Rolling | ReplayPhase::Crouched));
    if let (Some(&(t, seconds)), Some(g), Some(_)) = (touch, c.landing.as_ref(), end) {
        let at = t.since as f32 - l.since as f32;
        if at >= entry && release.is_none_or(|r| r > at) {
            if let Some(clip) = stock_tree_leaf_at(meta, &g.tree, &[]) {
                let length = skater.animation.evaluator.clip_length(&clip).unwrap_or(0.0);
                follow.landing = Some((PuppetLayer { clip, time: seconds, weight: weight(seconds, g.seconds), since: t.since, tree: None }, (length - g.expire).max(0.0)));
            }
        }
    }
    Some((tree, follow))
}

/// Whether `clip` is `tree` or one of its blend children (`depth` levels down).
pub(crate) fn tree_holds(meta: &skate_data::animation_metadata::AnimationMetadata, tree: &str, clip: &str, depth: u8) -> bool {
    use skate_data::animation_metadata::TreeMetadata;
    if tree.eq_ignore_ascii_case(clip) {
        return true;
    }
    let children = match meta.tree(tree) {
        Ok(TreeMetadata::BlendSpace(b)) if depth > 0 => b.children.clone(),
        Ok(TreeMetadata::PhaseBlend(p)) if depth > 0 => p.children.clone(),
        _ => return false,
    };
    children.iter().any(|c| tree_holds(meta, c, clip, depth - 1))
}

/// The puppet's layers, oldest first, from the phase history (same clip = one layer). Pure.
pub(crate) fn puppet_layers(history: &[(PhaseEntry, f32)], resolve: impl Fn(PhaseEntry, Option<PhaseEntry>) -> (String, f32)) -> Vec<PuppetLayer> {
    let mut out = Vec::new();
    puppet_layers_into(history, resolve, &mut out);
    out
}

/// [`puppet_layers`] into `out` (cleared first); each history entry is resolved once, in order.
pub(crate) fn puppet_layers_into(history: &[(PhaseEntry, f32)], resolve: impl Fn(PhaseEntry, Option<PhaseEntry>) -> (String, f32), out: &mut Vec<PuppetLayer>) {
    out.clear();
    let item = |i: usize| {
        let (e, t) = history[i];
        let (clip, seconds) = resolve(e, history.get(i + 1).map(|h| h.0));
        (clip, seconds, t, e.since)
    };
    let mut next = (!history.is_empty()).then(|| item(0));
    let mut j = 0;
    while let Some(mut current) = next.take() {
        // An older phase whose clip is this one, or a sequence ending in it (`<ground>+<air>`
        // before `<air>`), is the same playing clip: continue it from the older start.
        while j + 1 < history.len() {
            let older = item(j + 1);
            j += 1;
            if older.0 == current.0 || sequence_ends_in(&older.0, &current.0) {
                current = older;
            } else {
                next = Some(older);
                break;
            }
        }
        let (clip, seconds, time, since) = current;
        let weight = skate_core::animation::playback_transition::transition_weight(time, seconds);
        out.push(PuppetLayer { clip, time, weight, since, tree: None });
        if weight >= 1.0 {
            break;
        }
    }
    out.reverse();
}

/// An NPC's crossfade out of the previous phase's clip (the outgoing tree of a graph transition).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PuppetBlend {
    pub from: String,
    /// Time since the previous phase began: the outgoing clip keeps playing (transition matching 0).
    pub from_time: f32,
    /// Weight of the incoming clip (0 = all `from`, 1 = done).
    pub weight: f32,
}

/// [`puppet_blend`] at fractional times (s since each phase began): the render passes
/// `(frames + sub_frame) / 60`, so the weight and the outgoing clip move between ticks.
pub(crate) fn puppet_blend_at(from: &str, from_time: f32, to: &str, to_time: f32, seconds: f32) -> Option<PuppetBlend> {
    if from == to {
        return None;
    }
    let weight = skate_core::animation::playback_transition::transition_weight(to_time, seconds);
    (weight < 1.0).then(|| PuppetBlend { from: from.to_owned(), from_time, weight })
}

/// The crossfade in effect `to_frames` 60 Hz frames into the new phase, `None` once it is done or
/// when both phases show the same clip (nothing to blend).
pub(crate) fn puppet_blend(from: &str, from_frames: u64, to: &str, to_frames: u64, seconds: f32) -> Option<PuppetBlend> {
    puppet_blend_at(from, from_frames as f32 / 60.0, to, to_frames as f32 / 60.0, seconds)
}

/// The clip one NPC shows now and the time since its phase began (render side, rewritten every
/// frame from the replay; deterministic from the replay cursor).
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct NpcPuppetClip {
    pub phase: ReplayPhase,
    pub clip: String,
    pub time: f32,
    /// The crossfade from the previous layer's clip while it runs.
    pub blend: Option<PuppetBlend>,
    /// Every clip in the pose, oldest first ([`puppet_layers`]); the last one is `clip`.
    pub layers: Vec<PuppetLayer>,
    /// Whether the last frame posed the skeleton from it (false until the look is bound).
    pub posed: bool,
    /// Retail's fakie overlay while it shows ([`fakie_channel_layer`]): the channel tree, its time
    /// and its channel weight.
    pub fakie: Option<PuppetLayer>,
}

/// `tweak_x` / `tweak_y` (the names `TweakProject` binds, TU3 factory 82BC9510).
fn tweak_attributes(v: [f32; 2]) -> [skate_core::animation::playback_parameters::SettableAttribute; 2] {
    use skate_core::animation::{playback_parameters::SettableAttribute, skeleton_input::name::encode};
    [SettableAttribute { name: encode(b"tweak_x"), value: v[0], normalized: false, sequence_id: -1 }, SettableAttribute { name: encode(b"tweak_y"), value: v[1], normalized: false, sequence_id: -1 }]
}

/// Fakie channel tree (FakieHeadChannel82BAC778, blend 0.3 s, transition 0.1 s); mod-replaceable.
pub(crate) const FAKIE_CHANNEL: &str = "B_FAKIE_CHANNEL";

/// The fakie channel tree in effect: a mod's `skater_clips["fakie_channel"]`, else [`FAKIE_CHANNEL`].
pub(crate) fn resolve_fakie_channel(overrides: &BTreeMap<String, String>) -> &str {
    overrides.get("fakie_channel").map_or(FAKIE_CHANNEL, String::as_str)
}

/// Fakie channel `torso` while riding (82BAC778: manual 1.0, powerslide 0.0, else 0.5; replay: always 0.5).
pub(crate) const FAKIE_TORSO_RIDING: f32 = 0.5;

/// Fakie channel layer of a cursor (`None` while its weight is 0).
pub(crate) fn fakie_channel_layer(cursor: &LineCursor, alpha: f32, tree: &str) -> Option<PuppetLayer> {
    let weight = cursor.fakie_channel_weight(alpha);
    if weight <= 0.0 {
        return None;
    }
    let began = if cursor.fakie { cursor.fakie_since } else { cursor.fakie_previous_since };
    let time = (cursor.frames - began.min(cursor.frames)) as f32 / 60.0 + alpha.clamp(0.0, 1.0) / 60.0;
    Some(PuppetLayer { clip: tree.to_owned(), time, weight, since: began, tree: None })
}

/// The fakie channel tree's parameters: `torso` = [`FAKIE_TORSO_RIDING`] (pass to
/// `graph_host::motion::tree_commands` with the layer's tree and time).
pub(crate) fn fakie_channel_attributes() -> [skate_core::animation::playback_parameters::SettableAttribute; 1] {
    [skate_core::animation::playback_parameters::SettableAttribute {
        name: skate_core::animation::skeleton_input::name::encode(b"torso"),
        value: FAKIE_TORSO_RIDING,
        normalized: false,
        sequence_id: -1,
    }]
}

/// The puppet's layers for a cursor (mod clips and blend times, shipped pick as fallback).
pub(crate) fn npc_puppet_layers(settings: &LivingWorldSettings, scores: &NpcScoreClips, style: &str, skater: Option<&crate::physics::SkaterRuntime>, cursor: &LineCursor, sub_frame: f32) -> Vec<PuppetLayer> {
    let mut out = Vec::new();
    npc_puppet_layers_into(settings, scores, style, skater, cursor, sub_frame, &mut out);
    out
}

/// [`npc_puppet_layers`] into `out` (cleared first).
pub(crate) fn npc_puppet_layers_into(settings: &LivingWorldSettings, scores: &NpcScoreClips, style: &str, skater: Option<&crate::physics::SkaterRuntime>, cursor: &LineCursor, sub_frame: f32, out: &mut Vec<PuppetLayer>) {
    let evaluates = |clip: &str| skater.is_none_or(|k| k.animation.evaluator.clip_length(clip).is_ok());
    let meta = skater.map(|k| k.animation.motion.animation.metadata());
    let stock = |name: &str| match meta {
        Some(m) => stock_tree_leaf(m, name).filter(|c| evaluates(c)),
        None => Some(name.to_owned()),
    };
    let leaf = |name: &str, targets: &[(&str, f32)]| match meta {
        Some(m) => stock_tree_leaf_at(m, name, targets).filter(|c| evaluates(c)),
        None => Some(name.to_owned()),
    };
    // No skater runtime (headless): the top of the range, where retail rests.
    let grind = settings.npc_grind_disttocog.or_else(|| skater.map(|k| k.animation.motion.grind_rest_disttocog())).unwrap_or(1.0e6);
    let manual = |m: f32| skater.map_or(0.0, |k| k.animation.motion.manual_rest_angle(m));
    let scored = |e: PhaseEntry, landed: bool| scored_trick_clip(&scores.0, &leaf, grind, &manual, e, landed);
    let resolve = |e: PhaseEntry, older: Option<PhaseEntry>| puppet_layer_clip(&settings.skater_clips, &settings.skater_blend_seconds, style, &evaluates, &stock, &scored, e, older);
    let mut entries = cursor.phase_history().map(|(e, frames)| (e, (frames as f32 + sub_frame) / 60.0));
    let Some(first) = entries.next() else {
        return puppet_layers_into(&[], resolve, out);
    };
    let mut history = [first; skate_core::living_world::replay::PHASE_HISTORY];
    let mut n = 1;
    for entry in entries {
        history[n] = entry;
        n += 1;
    }
    puppet_layers_into(&history[..n], &resolve, out);
    if let Some(k) = skater {
        // The air pose after a grab's Out: the Air phase clip at its blend.
        let air = resolve_puppet_clip(&settings.skater_clips, ReplayPhase::Air, style);
        let air = (if evaluates(air) { air } else { puppet_clip(ReplayPhase::Air, style) }, blend_seconds(&settings.skater_blend_seconds, ReplayPhase::Air));
        let mut i = 0;
        while i < out.len() {
            let (tree, follow) = grab_tree(&settings.skater_clips, scores, k, &history[..n], &out[i], air).unzip();
            out[i].tree = tree;
            let follow = follow.unwrap_or_default();
            // The Out / air pose between the grab and the next layer, when they began before it (else the grab was held).
            let next = out.get(i + 1).map(|x| x.since);
            for layer in follow.after.into_iter().filter(|r| next.is_none_or(|s| s > r.since)) {
                i += 1;
                out.insert(i, layer);
            }
            i += 1;
        }
        // A touchdown layer whose span's grab still held its Cyc: the grab landing until Land leaves for OnBoard, then
        // the touchdown layer blends back in (found from the history, so it outlives the grab layer's blend-out).
        let mut j = 0;
        while j < out.len() {
            let landed = history[..n].iter().position(|h| h.0.since == out[j].since && out[j].tree.is_none() && matches!(h.0.phase, ReplayPhase::Rolling | ReplayPhase::Crouched)).and_then(|ti| {
                let t = history[ti].0;
                let gi = (ti + 1..n).filter(|&g| history[g].0.trick_since == t.trick_since && matches!(history[g].0.phase, ReplayPhase::GroundTrick | ReplayPhase::AirTrick)).last()?;
                let (clip, _) = resolve(history[gi].0, history.get(gi + 1).map(|h| h.0));
                let grab = PuppetLayer { clip, time: history[gi].1, weight: 1.0, since: history[gi].0.since, tree: None };
                grab_tree(&settings.skater_clips, scores, k, &history[..n], &grab, air)?.1.landing.filter(|(l, _)| l.since == t.since)
            });
            if let Some((landing, plays)) = landed {
                let back = landing.time - plays;
                let seconds = history[..n].iter().find(|h| h.0.since == out[j].since).map_or(RETAIL_BLEND_SECONDS, |h| blend_seconds(&settings.skater_blend_seconds, h.0.phase));
                let weight = skate_core::animation::playback_transition::transition_weight(back.max(0.0), seconds);
                if back < 0.0 {
                    out[j] = landing;
                } else if weight < 1.0 {
                    let ground = std::mem::replace(&mut out[j], landing);
                    j += 1;
                    out.insert(j, PuppetLayer { time: back, weight, ..ground });
                } else {
                    // Fully back: the landing is gone; the touchdown layer keeps the time it came back with.
                    out[j].time = back;
                }
            }
            j += 1;
        }
    }
}

/// Layer starts an NPC remembers (its puppet shows at most a few layers at once).
const STANCE_STARTS: usize = 8;

/// NPC stance bits (SkaterAnim bits 31 / 30, 82593230 -> 82B98980), per layer: docs/hails-additions/26c-living-world-npc-skaters.md#npc-stance-flags
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct NpcStanceTrack {
    /// Current bits (what a tree built now would get).
    pub flags: StanceFlags,
    /// The newest layer (clip, start frame) and its clip time at the last step.
    pub top: Option<(String, u64, f32)>,
    /// Layer start frame -> the bits it was built with, oldest first (at most [`STANCE_STARTS`]).
    pub starts: Vec<(u64, StanceFlags)>,
    /// The natural stance's start bits (layers older than every remembered start).
    pub natural: StanceFlags,
}

impl NpcStanceTrack {
    pub fn new(stance: skate_core::living_world::stance::NaturalStance) -> Self {
        let natural = StanceFlags::natural(stance);
        Self { flags: natural, top: None, starts: Vec::new(), natural }
    }

    /// The bits the layer that began at `since` was built with: its own start, else the newest
    /// start before it, else the natural bits.
    pub fn flags_for(&self, since: u64) -> StanceFlags {
        self.starts.iter().rev().find(|(s, _)| *s <= since).map_or(self.natural, |(_, f)| *f)
    }

    /// One fixed step: a new layer records the current bits; stance events in the window toggle them.
    pub fn step(&mut self, top: &PuppetLayer, events: &StanceEvents, attributes: impl Fn(&str, f32, f32) -> Vec<String>) -> bool {
        let fresh = !matches!(&self.top, Some((clip, since, _)) if *clip == top.clip && *since == top.since);
        let previous_time = match &self.top {
            Some((_, _, t)) if !fresh => *t,
            _ => 0.0,
        };
        if fresh {
            if self.starts.last().is_none_or(|(s, _)| *s != top.since) {
                self.starts.push((top.since, self.flags));
            }
            if self.starts.len() > STANCE_STARTS {
                self.starts.remove(0);
            }
        }
        self.top = Some((top.clip.clone(), top.since, top.time));
        let names = attributes(&top.clip, previous_time, top.time);
        let before = self.flags;
        // Names compare as the engine's attribute names (`encode`: the banks spell them in
        // upper case, `ANIMBOARDBACKWARD`), like the player's `apply_stance_events`.
        use skate_core::animation::skeleton_input::name::encode;
        self.flags.apply(events, |n| names.iter().any(|x| encode(x.as_bytes()) == encode(n.as_bytes())));
        before != self.flags
    }
}

/// Attribute names of a `+`-joined clip over `previous_time..time` (parts back to back).
pub(crate) fn sequence_attributes<N: AsRef<str>>(clip: &str, previous_time: f32, time: f32, clip_data: impl Fn(&str) -> Option<(f32, Vec<(N, f32, f32)>)>) -> Vec<String> {
    let mut out = Vec::new();
    let mut offset = 0.0f32;
    let mut parts = clip.split('+').peekable();
    while let Some(part) = parts.next() {
        let last = parts.peek().is_none();
        let (name, speed) = part_speed(part);
        let Some((length, attributes)) = clip_data(name) else { break };
        let end = offset + length / speed;
        let reached = time >= offset;
        let left = !last && previous_time >= end;
        if reached && !left {
            let p = ((previous_time - offset) * speed).clamp(0.0, length);
            let t = ((time - offset) * speed).clamp(0.0, length);
            out.extend(attributes.iter().filter(|(_, b, e)| skate_core::living_world::stance::attribute_in_window(*b, *e, length, p, t)).map(|(n, ..)| n.as_ref().to_owned()));
        }
        if !reached {
            break;
        }
        offset = end;
    }
    out
}

/// Fixed step after [`advance`]: each NPC's stance bits from the clip its newest layer plays
/// ([`NpcStanceTrack::step`]); the clip data come from the stock banks of the skater runtime
/// (without it no clip carries events and the bits stay natural). Logs every change.
pub(crate) fn track_stance(
    settings: Res<LivingWorldSettings>,
    scores: Option<Res<NpcScoreClips>>,
    skater: Option<Res<crate::physics::SkaterRuntime>>,
    state: Res<PopulationState>,
    mut npcs: Query<(&NpcSkater, &NpcReplay, Option<&NpcPuppet>, &mut NpcStanceTrack)>,
) {
    let lines = npc_lines(&state);
    let no_scores = NpcScoreClips::default();
    let scores = scores.as_deref().unwrap_or(&no_scores);
    let events = StanceEvents::with_overrides(&settings.skater_stance_events);
    let skater = skater.as_deref();
    let clip_data = |part: &str| -> Option<(f32, Vec<(&str, f32, f32)>)> {
        let k = skater?;
        let length = k.animation.evaluator.clip_length(part).ok()?;
        let meta = k.animation.motion.animation.metadata().clip(part).ok()?;
        Some((length, meta.attributes.iter().map(|a| (a.name.as_str(), f32::from_bits(a.begin_bits), f32::from_bits(a.end_bits))).collect()))
    };
    for (npc, replay, puppet, mut track) in &mut npcs {
        let style = puppet.map_or_else(|| crate::custom_models::native_animation_style(&npc.character), |p| p.style);
        let mut layers = npc_puppet_layers(&settings, scores, style, skater, &replay.cursor, 0.0);
        // Before the first remembered phase: the phase's own clip (as `present_pose` draws it).
        if layers.is_empty() {
            let Some(s) = replay.cursor.sample(&*lines, 0.0) else { continue };
            let clip = resolve_puppet_clip(&settings.skater_clips, s.phase, style);
            let clip = if skater.is_none_or(|k| k.animation.evaluator.clip_length(clip).is_ok()) { clip } else { puppet_clip(s.phase, style) };
            layers.push(PuppetLayer { clip: clip.to_owned(), time: s.phase_frames as f32 / 60.0, weight: 1.0, since: replay.cursor.frames.saturating_sub(s.phase_frames), tree: None });
        }
        let Some(top) = layers.last() else { continue };
        let before = track.flags;
        if track.step(top, &events, |clip, p, t| sequence_attributes(clip, p, t, &clip_data)) {
            info!("LIVING_WORLD npc stance #{} {} clip {} t {:.2}: {:?} -> {:?} board_flipped {} fakie {}", npc.id.serial, npc.character, top.clip, top.time, before, track.flags, track.flags.board_flipped(), replay.cursor.fakie);
        }
    }
}

pub(crate) const PUPPET_CLIPS: [&str; 8] =
    ["R_IDLE_RIDE_N_0_CYC", "R_IDLE_RIDE_AGGR_0_CYC", "R_IDLE_RIDE_LOOSE_0_CYC", "R_IDLE_LCOM_000", "IA_IDLE_N_N_0_CYC", "IA_IDLE_LO_N_0_CYC", "G_5050_FS_LOW_0_CYC", "BR_STAND_0_CYC"];

pub(crate) fn npc_lines(state: &PopulationState) -> Arc<BTreeMap<[u8; 16], ReplayLine>> {
    state.npc.lines.clone()
}

/// Spawn and despawn NPC entities from the population's records.
pub(crate) fn apply_records(
    mut commands: Commands,
    mut spawns: MessageReader<LivingWorldSpawn>,
    mut despawns: MessageReader<LivingWorldDespawn>,
    state: Res<PopulationState>,
    settings: Res<LivingWorldSettings>,
    mut index: ResMut<NpcSkaterIndex>,
    mut events: MessageWriter<NpcSkaterEvent>,
) {
    for LivingWorldDespawn(r) in despawns.read() {
        if r.id.kind != Kind::Skater {
            continue;
        }
        if let Some(e) = index.0.remove(&r.id) {
            // A simulated skater's loaded body goes back to the pool first (loading one costs ~220 ms).
            commands.queue(move |world: &mut World| super::npc_sim::pool_from(world, e));
            crate::replay::retire(&mut commands, e);
            events.write(NpcSkaterEvent::Despawned { id: r.id, reason: r.reason });
            info!("NPC_SKATER_DESPAWN #{} reason {:?} tick {} left {}", r.id.serial, r.reason, r.tick, index.0.len());
        }
    }
    let lines = npc_lines(&state);
    for LivingWorldSpawn(s) in spawns.read() {
        let SpawnChoice::Skater { line, character, slot } = &s.choice else { continue };
        if index.0.contains_key(&s.id) {
            continue;
        }
        let cursor = LineCursor::spawn(&*lines, *line, 0);
        let npc = NpcSkater {
            id: s.id,
            character: character.clone(),
            slot: *slot,
            seed: s.seed,
            voice: state.npc.voices.get(character).copied(),
            spawn_tick: s.tick,
            start_line: *line,
            stance: npc_stance(&state.npc.records, &settings.skater_stance, character),
        };
        let sample = cursor.sample(&*lines, 0.0);
        let at = sample.as_ref().map_or(Vec3::from_array(s.position), |x| Vec3::from_array(x.position));
        let e = commands
            .spawn((
                Name::new(format!("NPC skater {} ({character})", s.id.serial)),
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(s.heading)),
                Visibility::Inherited,
                NpcReplay { cursor, branches: Vec::new(), tricks: Vec::new(), last: sample, previous: None, avoid: Default::default(), follow_target: None, body_gate: None },
                NpcFade { alpha: if state.world.is_scripted(s.id) { 1.0 } else { state.world.config.skaters.leave_fade.fade_in_alpha(0) }, ..NpcFade::default() },
                NpcSkaterAudio { list_order: u32::from(*slot), voice: npc.voice, ..Default::default() },
                NpcStanceTrack::new(npc.stance),
                npc,
            ))
            .id();
        // A challenge AI skater gets its clock (pin, band; it stays in the replay tier) and its line filter at once.
        if state.world.is_scripted(s.id) {
            commands.entity(e).insert(NpcRaceClock { start: Some((s.position, s.heading)), ..Default::default() });
            info!("NPC_SKATER_SPAWN #{} race start slot at [{:.1}, {:.1}, {:.1}]", s.id.serial, s.position[0], s.position[1], s.position[2]);
            if let Some(lines) = &state.scripted_lines {
                commands.entity(e).insert(NpcLineFilter(lines.clone()));
            }
        }
        index.0.insert(s.id, e);
        events.write(NpcSkaterEvent::Spawned { id: s.id, character: character.clone(), line: *line });
        info!(
            "NPC_SKATER_SPAWN #{} {character} line {} at [{:.1}, {:.1}, {:.1}] tick {} live {}",
            s.id.serial,
            line.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            at.x,
            at.y,
            at.z,
            s.tick,
            index.0.len()
        );
    }
}

/// Buffers [`advance`] reuses across NPCs and ticks.
#[derive(Default)]
pub(crate) struct AdvanceScratch {
    players: Vec<[f32; 3]>,
    order: Vec<(LivingWorldId, [u8; 16], u32)>,
    others: Vec<([u8; 16], u32)>,
    in_use: Vec<[u8; 16]>,
    out: Vec<CursorEvent>,
    finished: Vec<(LivingWorldId, Entity)>,
    /// The cache when no [`NpcValidLines`] resource is installed (harnesses that add the systems alone).
    own_valid: NpcValidLines,
}

/// Per character the lines valid for it (`line_valid`, sorted), for the loaded line count: filled by [`advance`], read
/// by the simulated skaters' line end in `npc_sim::simulate` (same tick, after it).
#[derive(Resource, Default)]
pub(crate) struct NpcValidLines {
    pub per_character: std::collections::HashMap<String, Vec<[u8; 16]>>,
    pub line_count: usize,
}

/// Every other NPC's (line, node) and the lines in use, by serial order.
pub(crate) fn fill_others(order: &[(LivingWorldId, [u8; 16], u32)], id: LivingWorldId, others: &mut Vec<([u8; 16], u32)>, in_use: &mut Vec<[u8; 16]>) {
    others.clear();
    others.extend(order.iter().filter(|o| o.0 != id).map(|o| (o.1, o.2)));
    in_use.clear();
    in_use.extend(others.iter().map(|o| o.0));
}

/// Advance every cursor to its tick, take branches, end finished lines, publish position and audio.
#[allow(clippy::too_many_arguments)]
pub(crate) fn advance(
    mut commands: Commands,
    settings: Res<LivingWorldSettings>,
    observers: Res<LivingWorldObservers>,
    mut state: ResMut<PopulationState>,
    mut index: ResMut<NpcSkaterIndex>,
    mut npcs: Query<(Entity, &NpcSkater, &mut NpcReplay, &mut Transform, &mut NpcSkaterAudio, &mut NpcFade, Option<&super::npc_sim::NpcSim>, Option<&NpcRaceClock>, Option<&NpcLineFilter>)>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    mut despawns: MessageWriter<LivingWorldDespawn>,
    mut events: MessageWriter<NpcSkaterEvent>,
    (mut scratch, mut valid_lines): (Local<AdvanceScratch>, Option<ResMut<NpcValidLines>>),
) {
    let _span = super::timing::span(super::timing::NPC_ADVANCE);
    let AdvanceScratch { players, order, others, in_use, out, finished, own_valid } = &mut *scratch;
    let NpcValidLines { per_character: valid, line_count: valid_count } = match valid_lines.as_deref_mut() {
        Some(v) => v,
        None => own_valid,
    };
    let lines = npc_lines(&state);
    let tick = state.world.tick();
    players.clear();
    players.extend(observers.observers.iter().map(|o| o.position));
    let mirror = settings.net_role == NetRole::Client;
    // Others' (line, node) and lines in use, by serial order (deterministic).
    order.clear();
    order.extend(npcs.iter().map(|(_, n, r, ..)| (n.id, r.cursor.line, r.cursor.node)));
    order.sort_by_key(|x| x.0);
    finished.clear();
    let mut sorted: Vec<_> = npcs.iter_mut().collect();
    sorted.sort_by_key(|(_, n, ..)| n.id);
    let fade_cfg = state.world.config.skaters.leave_fade;
    let chain = state.world.config.skaters.line_chain;
    // Branches and the line-end search take only lines valid for the character (`sub_82456970`, `fits`), cached per
    // character for the loaded line count.
    if let Some(w) = state.skaters.as_ref() {
        if *valid_count != w.lines.len() {
            valid.clear();
            *valid_count = w.lines.len();
        }
        for (_, npc, ..) in &sorted {
            if valid.contains_key(&npc.character) {
                continue;
            }
            if let Some(c) = w.characters.iter().find(|c| c.key == npc.character) {
                use skate_core::living_world::SkaterWorld;
                let mut ids: Vec<[u8; 16]> = w.lines.iter().filter(|l| w.fits(l, c)).map(|l| l.id).collect();
                ids.sort_unstable();
                info!("NPC_SKATER_LINES {} valid {} of {}", npc.character, ids.len(), w.lines.len());
                valid.insert(npc.character.clone(), ids);
            }
        }
    }
    for (e, npc, mut replay, mut transform, mut audio, mut fade, sim, race, filter) in sorted {
        let mut elapsed = tick.saturating_sub(npc.spawn_tick) * FRAMES_PER_TICK;
        if let Some(clock) = race {
            elapsed = elapsed.saturating_sub(clock.held).saturating_add_signed(clock.extra);
        }
        // The obstacle avoider holds the cursor back (`npc_avoid`); the fade keeps the spawn clock.
        let target = replay.follow_target.unwrap_or_else(|| elapsed.saturating_sub(replay.avoid.held_frames()));
        out.clear();
        // The switch blend time (branch / chain root blend) is a tuning value, the same on a client.
        replay.cursor.switch_blend_seconds = chain.blend_seconds;
        // Keep the facing across switches (fix 16 rule, mod option, retail off); a tuning value.
        replay.cursor.keep_facing = chain.keep_facing;
        // Facing rule (retail riding-entry flip by default; the fix 23 per-node fold as a mod option).
        replay.cursor.facing_rule = chain.facing_rule;
        // Retail riding-fakie thresholds (data, stock graph values by default).
        replay.cursor.fakie_settings = chain.fakie;
        // A simulated skater holds at a dead end (host and client alike, from the replicated `follow_target`).
        replay.cursor.hold_at_end = replay.follow_target.is_some();
        // A challenge skater's path filter hides the other lines from its line search (82458860 skips them); the
        // per-character mask stays the valid / invalid split of the fallback (82456970).
        let valid_lines = valid.get(&npc.character).map(Vec::as_slice);
        let filtered = filter.map(|f| skate_core::living_world::replay::FilteredLines { inner: &*lines, ids: f.0.as_slice() });
        let search: &dyn skate_core::living_world::replay::LineSource = match &filtered {
            Some(f) => f,
            None => &*lines,
        };
        // The NPC's trick tables (disc profile, mod overrides), read at its trick slots.
        let trick_profile = npc_trick_profile(&state.npc.tricks, &settings.skater_trick_profiles, &npc.character);
        // A simulated skater's line end chains in `npc_sim::simulate` on its trigger tick (b701 / b712).
        // The loop only moves this NPC's own `order` entry, which `others` leaves out (b620).
        fill_others(order, npc.id, others, in_use);
        // A client applies a host record at the cursor's frame even on a tick it takes no step (a restart while the
        // host's clock stands still).
        if mirror {
            let r = &mut *replay;
            r.cursor.apply_records(&*lines, &Decider::Mirror(&r.branches, &r.tricks), out);
        }
        while replay.cursor.frames < target && !replay.cursor.finished {
            if replay.cursor.frames + FRAMES_PER_TICK >= target {
                replay.previous = Some(replay.cursor.clone());
            }
            let s = replay.cursor.sample(&*lines, 0.0);
            let (position, forward, speed) = s.as_ref().map_or(([0.0; 3], [0.0, 0.0, 1.0], 0.0), |s| (s.position, s.velocity, length(s.velocity)));
            let tricks = TrickContext { mode: settings.npc_tricks.mode, params: settings.npc_tricks.params, profile: trick_profile.as_deref(), seed: npc.seed, slot_tricks: state.npc.tricks.slot_tricks_for(&npc.character) };
            // A simulated skater branches from its body, through retail's gate (set by the sim each tick, b674).
            let body = replay.body_gate.filter(|_| replay.follow_target.is_some());
            let ctx = BranchContext { position, forward, speed, players, others, in_use, preferred_skill: -1, online: observers.online, chain, tricks, body, valid_lines };
            let r = &mut *replay;
            let mut decider = if mirror { Decider::Mirror(&r.branches, &r.tricks) } else { Decider::Decide(ctx) };
            let before = r.cursor.frames;
            r.cursor.step(search, &mut decider, out);
            // A simulated skater held on its last node does not advance: no busy loop.
            if r.cursor.frames == before && !r.cursor.finished {
                break;
            }
            if let Some(o) = order.iter_mut().find(|o| o.0 == npc.id) {
                o.1 = replay.cursor.line;
                o.2 = replay.cursor.node;
            }
        }
        for ev in out.drain(..) {
            match ev {
                CursorEvent::Node { line, node, event, flags } => {
                    // A racer at a junction: was the body gate open, and where could it go (the race_dwtn_02 dead end
                    // at node 26 of line ..00 / ..02: docs/hails-additions/42-challenges.md).
                    if race.is_some() {
                        if let Some(group) = lines.get(&line).and_then(|l| l.group_at(node)) {
                            let hex = |id: &[u8; 16]| id.iter().map(|b| format!("{b:02x}")).collect::<String>();
                            let targets: Vec<String> = group.branches.iter().map(|b| format!("{}@{}", hex(&b.target), b.target_node)).collect();
                            let gate = replay.body_gate.as_ref().map_or("none (replay tier)".to_string(), |g| format!("may_branch {}", g.may_branch));
                            info!("NPC_RACE_JUNCTION #{} {} line {} node {node}: gate {gate}; targets {}", npc.id.serial, npc.character, hex(&line), targets.join(", "));
                        }
                    }
                    if event != 0 {
                        events.write(NpcSkaterEvent::Node { id: npc.id, line, node, event, flags });
                    }
                }
                CursorEvent::Branch(record) => {
                    if race.is_some() {
                        info!("NPC_RACE_JUNCTION #{} {} branched at node {} -> node {}", npc.id.serial, npc.character, record.from_node, record.to_node);
                    }
                    if !mirror {
                        replay.branches.push(record.clone());
                    }
                    events.write(NpcSkaterEvent::Branch { id: npc.id, record });
                }
                CursorEvent::Trick(record) => {
                    info!(
                        "NPC_SKATER_TRICK #{} {} node {} frame {} recorded {} chosen {} mode {}",
                        npc.id.serial,
                        npc.character,
                        record.node,
                        record.frame,
                        trick_name(record.recorded),
                        trick_name(record.chosen),
                        settings.npc_tricks.mode.name()
                    );
                    if !mirror {
                        replay.tricks.push(record.clone());
                    }
                    events.write(NpcSkaterEvent::Trick { id: npc.id, record });
                }
                CursorEvent::Finished => {
                    events.write(NpcSkaterEvent::LineEnd { id: npc.id });
                }
            }
        }
        // Spawn fade in (825926F8), leave fade at a dead end (8246EA90 / 8245A9B8): docs/hails-additions/26c-living-world-npc-skaters.md#npc-fade
        // A challenge racer never takes the ambient leave fade: its path filter gives it the next line of its set at a
        // line end (b972 §3; laptop run 2026-11-07: a racer faded out at its first dead end before the race was half run).
        if race.is_none() {
            fade.fade.update(&replay.cursor, &*lines, &fade_cfg);
        }
        // A bailed simulated skater fades out before its respawn (b715). A challenge racer shows whole from its spawn on
        // its slot: no spawn fade-in (its clock is held while pinned, so the fade-in ran only after the start; laptop
        // 2026-11-08: see-through racers until several pushes into the race).
        let spawn_fade = if race.is_some() { 1.0 } else { fade.fade.alpha(elapsed, &fade_cfg) };
        let alpha = spawn_fade * sim.map_or(1.0, |s| s.bail_alpha);
        if fade.alpha != alpha {
            fade.alpha = alpha;
        }
        if fade.fade.should_despawn(elapsed, &fade_cfg) {
            finished.push((npc.id, e));
        }
        let Some(s) = replay.cursor.sample(&*lines, 0.0) else { continue };
        state.world.update_position(npc.id, s.position);
        transform.translation = Vec3::from_array(s.position) + Vec3::from_array(super::npc_avoid::right_of(s.velocity)) * replay.avoid.offset;
        transform.rotation = root_rotation(&s);
        let material = physics.as_deref().map_or(skate_audio::player::state::NO_MATERIAL, |p| crate::game_audio::world_bridge::ground_material(p, transform.translation));
        audio.voice = npc.voice;
        // A simulated NPC sounds from its physics body (its line waits at the node while it bails: a grind kept
        // sounding after a bail off the rail, user 2026-10-15); a replay-tier one from its line sample.
        audio.state = Some(match sim {
            // The full record of its last step; the lite state only before its first step after a placement.
            Some(sim) => match sim.audio_state() {
                Some(state) => state.clone(),
                None => {
                    let deck = sim.deck_position();
                    let material = physics.as_deref().map_or(skate_audio::player::state::NO_MATERIAL, |p| crate::game_audio::world_bridge::ground_material(p, Vec3::from_array(deck)));
                    sim.lite_state(material)
                }
            },
            None => lite_state(&s, material),
        });
        commands.entity(e).insert(AudioVelocity(Vec3::from_array(s.velocity)));
        replay.last = Some(s);
    }
    // Faded out (opacity below the threshold): the NPC leaves. Hosts decide this; a client waits
    // for the host's despawn record.
    if !mirror {
        for (id, e) in finished.drain(..) {
            if let Some(skate_core::living_world::Decision::Despawn(r)) = state.world.despawn(id, DespawnReason::External) {
                state.despawned += 1;
                despawns.write(LivingWorldDespawn(r));
            }
            if index.0.remove(&id).is_some() {
                // A simulated skater's body back to the pool, as `apply_records` does (that path no longer finds it in
                // the index): dropped, the next switch to physics loaded one on the game thread (140 ms, 2026-10-23).
                commands.queue(move |world: &mut World| super::npc_sim::pool_from(world, e));
                crate::replay::retire(&mut commands, e);
                events.write(NpcSkaterEvent::Despawned { id, reason: DespawnReason::External });
                info!("NPC_SKATER_DESPAWN #{} reason faded out left {}", id.serial, index.0.len());
            }
        }
    }
}

fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Root rotation of the puppet: the recorded skater orientation (x, y, z, w, +Z forward [data]).
pub(crate) fn root_rotation(s: &ReplaySample) -> Quat {
    let [x, y, z, w] = s.skater;
    let q = Quat::from_xyzw(x, y, z, w);
    if q.is_finite() && q.length_squared() > 0.5 {
        q.normalize()
    } else {
        Quat::from_rotation_y(s.heading)
    }
}

/// The lite audio state of a replay NPC (like a remote player's, plus air and ground tricks).
pub(crate) fn lite_state(s: &ReplaySample, material: u32) -> AudioState {
    let airborne = matches!(s.phase, ReplayPhase::Air | ReplayPhase::AirTrick);
    let off = s.phase == ReplayPhase::OffBoard;
    AudioState::rolling(&LiteSkater {
        position: s.position,
        velocity: s.velocity,
        heading: s.heading,
        wheels: if off { [false; 4] } else { [true; 4] },
        material: if off { skate_audio::player::state::NO_MATERIAL } else { material },
        grinding: s.phase == ReplayPhase::GroundTrick,
        grind_material: if s.phase == ReplayPhase::GroundTrick { material } else { skate_audio::player::state::NO_MATERIAL },
        airborne,
        air_time: if airborne { s.phase_frames as f32 / 60.0 } else { 0.0 },
        dt: (1.0 / skate_core::living_world::clock::RETAIL_TICK_HZ) as f32,
    })
}

/// Solid ids of NPC proxies: a tag in the top bits keeps them apart from mod bodies.
pub(crate) const PROXY_ID_TAG: u64 = 0x4E50_0000_0000_0000;

/// The board half of an NPC proxy carries this bit on top of the body's id.
pub(crate) const PROXY_BOARD_BIT: u64 = 1 << 47;

/// The proxy peer id of a simulated NPC's body (the actor its contacts report).
pub(crate) fn sim_peer(id: LivingWorldId) -> u64 {
    PROXY_ID_TAG | (1 << 46) | id.to_u64()
}

/// The proxy peer id of the player's body in a simulated NPC's solve.
pub(crate) const PLAYER_PEER: u64 = PROXY_ID_TAG | (1 << 45);

/// A contact's skater peer for the log: `player`, `#<serial>` for a simulated NPC, else the raw id.
pub(crate) fn peer_name(peer: u64) -> String {
    let peer = peer & !PROXY_BOARD_BIT;
    if peer == PLAYER_PEER {
        return "player".into();
    }
    let sim = PROXY_ID_TAG | (1 << 46);
    match (peer & sim == sim).then(|| LivingWorldId::from_u64(peer & !sim)).flatten() {
        Some(id) => format!("#{}", id.serial),
        None => format!("{peer:#x}"),
    }
}

/// Retail collision groups: skeleton parts 5 (82BD4A30 at 0x82BD53D0), board 4.
pub(crate) const PROXY_BODY_GROUP: u32 = 5;
pub(crate) const PROXY_BOARD_GROUP: u32 = 4;

/// The kinematic collision proxies of one NPC (body capsule in the skater group, board box in
/// the board group), world space.
pub(crate) fn proxy(id: LivingWorldId, s: &ReplaySample) -> [skate_dynamics::SolidBody; 2] {
    use skate_dynamics::rapier3d::prelude::{Pose, Rotation, SharedShape, Vector};
    let [x, y, z, w] = s.skater;
    let rotation = Rotation::from_xyzw(x, y, z, w).normalize();
    let at = |local: [f32; 3]| {
        let q = Quat::from_xyzw(x, y, z, w).normalize();
        Vec3::from_array(s.position) + q * Vec3::from_array(local)
    };
    let p = |v: Vec3| Vector::new(v.x, v.y, v.z);
    // Body: a 0.25 m capsule from 0.25 to 1.55 m above the deck; board: 0.8 x 0.1 x 0.2 m.
    let body = SharedShape::capsule_y(0.65, 0.25);
    let board = SharedShape::cuboid(0.1, 0.05, 0.4);
    let com = at([0.0, 0.9, 0.0]);
    let deck = at([0.0, 0.08, 0.0]);
    let solid = |id: u64, group: u32, centre: Vec3, collider: skate_dynamics::SolidCollider| skate_dynamics::SolidBody {
        id,
        pose: Pose::from_parts(p(Vec3::from_array(s.position)), rotation),
        center_of_mass: p(centre),
        inertia_rotation: rotation,
        inverse_mass: 0.0,
        inverse_inertia: Vector::new(0.0, 0.0, 0.0),
        linvel: Vector::new(s.velocity[0], s.velocity[1], s.velocity[2]),
        angvel: Vector::new(0.0, 0.0, 0.0),
        contact_group: group,
        colliders: vec![collider],
    };
    let base = PROXY_ID_TAG | id.to_u64();
    [
        solid(base, PROXY_BODY_GROUP, com, skate_dynamics::SolidCollider { shape: body, pose: Pose::from_parts(p(com), rotation), friction: 0.5 }),
        solid(base | PROXY_BOARD_BIT, PROXY_BOARD_GROUP, deck, skate_dynamics::SolidCollider { shape: board, pose: Pose::from_parts(p(deck), rotation), friction: 0.5 }),
    ]
}

/// NPC skaters against DMO props (fix 19; mod switch `npc_skater_props`): docs/hails-additions/26c-living-world-npc-skaters.md#npc-skaters-vs-props
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NpcSkaterPropContact {
    /// Retail on.
    pub enabled: bool,
}

impl Default for NpcSkaterPropContact {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Prop-step push volumes of one NPC (body capsule and board), both board hits: docs/hails-additions/26c-living-world-npc-skaters.md#npc-prop-volumes
pub(crate) fn prop_volumes(id: LivingWorldId, s: &ReplaySample) -> [(u64, skate_core::physics::board_world::BoardWorldVolume); 2] {
    use skate_core::math::Vector3;
    use skate_core::physics::board_step::CollisionBody;
    use skate_core::physics::board_world::BoardWorldVolume;
    use skate_core::physics::world_contact::ContactPrimitive;
    let [x, y, z, w] = s.skater;
    let q = Quat::from_xyzw(x, y, z, w);
    let q = if q.is_finite() && q.length_squared() > 0.5 { q.normalize() } else { Quat::from_rotation_y(s.heading) };
    let v = |a: Vec3| Vector3::new(a.x, a.y, a.z);
    let at = |local: [f32; 3]| v(Vec3::from_array(s.position) + q * Vec3::from_array(local));
    let material = skate_core::physics::contact::RetailContactMaterial { static_friction: 0.5, dynamic_friction: 0.5, restitution: 0.0 };
    let velocity = Vector3::new(s.velocity[0], s.velocity[1], s.velocity[2]);
    let actor = PROXY_ID_TAG | id.to_u64();
    let hit = CollisionBody::Board(skate_core::physics::board::BodyId::Deck);
    let body = BoardWorldVolume {
        collision_group: 4,
        body: hit,
        primitive: ContactPrimitive::Capsule { center: at([0.0, 0.9, 0.0]), axis: v(q * Vec3::Y), half_length: 0.65, radius: 0.25 },
        motion: skate_core::physics::board_world::VolumeMotion { linear_velocity: velocity, ..Default::default() },
        material,
    };
    let board = BoardWorldVolume {
        collision_group: 4,
        body: hit,
        // The proxy's 0.8 x 0.1 x 0.2 m board box as a capsule along the deck (0.4 m to each end).
        primitive: ContactPrimitive::Capsule { center: at([0.0, 0.08, 0.0]), axis: v(q * Vec3::Z), half_length: 0.3, radius: 0.1 },
        motion: skate_core::physics::board_world::VolumeMotion { linear_velocity: velocity, ..Default::default() },
        material,
    };
    [(actor, body), (actor, board)]
}

/// Add the NPC proxies to the skater solve (after the network proxies were rebuilt), and their
/// push volumes to the prop step ([`prop_volumes`]).
pub(crate) fn push_proxies(
    npcs: Query<(&NpcSkater, &NpcReplay), Without<super::npc_sim::NpcSim>>,
    sims: Query<(&NpcSkater, &super::npc_sim::NpcSim)>,
    net: Option<Res<crate::multiplayer::Multiplayer>>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    skater: Res<crate::physics::SkaterRuntime>,
    replay: Res<crate::replay::Replay>,
    settings: Res<LivingWorldSettings>,
) {
    // Replay-tier NPCs only. A simulated NPC has its own physics body; a proxy from its replay sample was an invisible
    // solid copy of it on the line that the simulated body collided with (contact bails and flings, 2026-10-15).
    physics.actor_prop_volumes.clear();
    physics.npc_sim_proxy_mark = None;
    if replay.active {
        return;
    }
    let mut proxies = std::mem::take(&mut physics.network_proxies);
    let mut list: Vec<_> = npcs.iter().filter_map(|(n, r)| r.last.as_ref().map(|s| (n.id, s))).collect();
    list.sort_by_key(|x| x.0);
    for &(id, s) in &list {
        for solid in proxy(id, s) {
            proxies.append_solid(solid, &physics, &skater, false);
        }
    }
    // Simulated NPCs: their own body from their last step, the whole skeleton and board like a multiplayer peer's, so
    // the player and they bump and knock each other down through each one's own skeleton contact and wipeout check
    // (retail: both are full skaters, group 5; research m7, b125). Appended last: their own solves swap them out.
    if let Some(net) = net.as_ref().filter(|_| settings.npc_simulated.bump) {
        physics.npc_sim_proxy_mark = Some(proxies.mark(&skater));
        let mut bodies: Vec<_> = sims.iter().filter_map(|(n, sim)| sim.last_body().map(|b| (n.id, b))).collect();
        bodies.sort_by_key(|x| x.0);
        if let Some(at) = skate_net::prediction::CollisionPrediction::at(0.0) {
            for (id, body) in bodies {
                proxies.append(sim_peer(id), body, &net.schema, &physics, &skater, at);
            }
        }
    }
    physics.network_proxies = proxies;
    if settings.npc_skater_props.enabled {
        let volumes = list.iter().flat_map(|&(id, s)| prop_volumes(id, s)).collect();
        physics.actor_prop_volumes = volumes;
    }
}

/// The look and the puppet pose of one NPC (render side).
#[derive(Component, Default)]
pub(crate) struct NpcPuppet {
    pub(crate) scene: Option<Entity>,
    pub(crate) bindings: Option<crate::animation::AnimationStatus>,
    pub(crate) style: &'static str,
}

impl NpcPuppet {
    #[cfg(test)]
    pub(crate) fn for_test(bindings: crate::animation::AnimationStatus, style: &'static str) -> Self {
        Self { scene: None, bindings: Some(bindings), style }
    }

    /// The bound joints, for the replay recorder.
    pub(crate) fn joints(&self) -> Vec<Entity> {
        self.joints_iter().collect()
    }

    pub(crate) fn joints_iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.bindings.iter().flat_map(|b| b.joints_iter())
    }
}

/// Load the look and bind it to the stock skeleton (like a remote player's look).
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_looks(
    mut commands: Commands,
    server: Res<AssetServer>,
    looks: Res<NpcSkaterLooks>,
    models: Res<crate::custom_models::CustomModels>,
    skater: Res<crate::physics::SkaterRuntime>,
    mut npcs: Query<(Entity, &NpcSkater, Option<&mut NpcPuppet>), Without<super::npc_outfits::NpcOutfit>>,
    skins: Query<(Entity, &bevy::mesh::skinning::SkinnedMesh)>,
    nodes: Query<(&Name, &Transform)>,
    parents: Query<&ChildOf>,
    instances: Query<&bevy::scene::SceneInstance>,
    spawner: Res<SceneSpawner>,
) {
    for (e, npc, puppet) in &mut npcs {
        let Some(mut puppet) = puppet else {
            let path = look_path(&looks, &models, &npc.character);
            let scene = commands.spawn((SceneRoot(server.load(GltfAssetLabel::Scene(0).from_asset(path))), Transform::default(), Visibility::Hidden, ChildOf(e))).id();
            commands.entity(e).insert(NpcPuppet { scene: Some(scene), bindings: None, style: crate::custom_models::native_animation_style(&npc.character) });
            continue;
        };
        let Some(scene) = puppet.scene else { continue };
        if puppet.bindings.is_some() || !instances.get(scene).is_ok_and(|i| spawner.instance_is_ready(**i)) {
            continue;
        }
        let _event = super::timing::event(super::timing::EV_NPC_BIND);
        match crate::animation::AnimationStatus::for_scene(scene, &skater.animation.evaluator.frames.bone_names, &skins, &nodes, &parents) {
            Ok(b) => {
                for (mesh, _) in &skins {
                    if parents.iter_ancestors(mesh).any(|p| p == scene) {
                        commands.entity(mesh).insert((bevy::camera::visibility::NoFrustumCulling, bevy::camera::visibility::RenderLayers::from_layers(&[0, 28])));
                    }
                }
                commands.entity(scene).insert(Visibility::Inherited);
                info!("LIVING_WORLD npc look bound #{} {} stance {}", npc.id.serial, npc.character, npc.stance.name());
                puppet.bindings = Some(b);
            }
            Err(err) => {
                warn!("NPC skater look rejected ({}): {err}", npc.character);
                commands.entity(scene).despawn();
                puppet.scene = None;
            }
        }
    }
}

/// Place the root between fixed steps, pick the phase's clip and pose the skeleton from it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_pose(
    mut commands: Commands,
    skater: Option<Res<crate::physics::SkaterRuntime>>,
    settings: Res<LivingWorldSettings>,
    scores: Option<Res<NpcScoreClips>>,
    state: Res<PopulationState>,
    fixed: Res<Time<Fixed>>,
    mut npcs: Query<(Entity, &NpcSkater, &NpcReplay, Option<&NpcPuppet>, Option<&mut NpcPuppetClip>, &mut Transform, Option<&NpcStanceTrack>, Option<&super::npc_sim::NpcSim>)>,
    mut joints: Query<&mut Transform, Without<NpcReplay>>,
    mut scratch: Local<NpcPoseScratch>,
) {
    let NpcPoseScratch { layers, layer_flags, channel, fakie_tree, grab_trees, pose, transforms, before, pose_now, pose_before } = &mut *scratch;
    let no_scores = NpcScoreClips::default();
    let scores = scores.as_deref().unwrap_or(&no_scores);
    if skater.as_ref().is_none_or(|s| s.is_added()) {
        *fakie_tree = None;
        grab_trees.clear();
    }
    let lines = npc_lines(&state);
    // Render interpolation: the fraction of the next world tick, drawn from one tick back (fix 14).
    let hz = state.world.clock().hz;
    let ahead = (state.world.clock().overstep() + fixed.overstep_fraction() as f64 * fixed.timestep().as_secs_f64() * hz).clamp(0.0, 1.0) as f32
        * FRAMES_PER_TICK as f32;
    for (e, npc, replay, puppet, current, mut root, stance, sim) in &mut npcs {
        // A simulated NPC is drawn from its physics pose, relative to its animation root: docs/hails-additions/26c-living-world-npc-skaters.md#simulated-npc-draw-root
        if let Some(sim) = sim {
            let root_of = |world: skate_core::animation::output::NativeMatrix, pose: &[skate_core::animation::output::NativeMatrix]| {
                let pose_root = pose.first().map(|m| [m[3][0], m[3][1], m[3][2]]).unwrap_or([0.0; 3]);
                let gap = (0..3).map(|i| (pose_root[i] - world[3][i]).powi(2)).sum::<f32>().sqrt();
                if gap > 2.0 { Transform::from_matrix(crate::animation::native_matrix(world)) } else { Transform::IDENTITY }
            };
            // Frame blending between the previous and the current physics tick (the player's presentation does the
            // same); not across a placement, which would smear the skater across the map.
            let current = root_of(sim.animation_root(), sim.render_pose());
            let blending = sim.previous_pose.len() == sim.render_pose().len() && sim.previous_placements == sim.placements();
            let alpha = fixed.overstep_fraction();
            *root = if blending { crate::presentation::blend(root_of(sim.previous_root, &sim.previous_pose), current, alpha) } else { current };
            if let Some(bindings) = puppet.and_then(|p| p.bindings.as_ref()) {
                pose_now.clear();
                pose_now.extend(sim.render_pose().iter().copied().map(crate::animation::native_matrix));
                bindings.pose_transforms_into(pose_now, transforms);
                before.clear();
                if blending {
                    pose_before.clear();
                    pose_before.extend(sim.previous_pose.iter().copied().map(crate::animation::native_matrix));
                    bindings.pose_transforms_into(pose_before, before);
                }
                for (i, &(joint, local)) in transforms.iter().enumerate() {
                    let shown = match before.get(i) {
                        Some((j, previous)) if *j == joint => crate::presentation::blend(*previous, local, alpha),
                        _ => local,
                    };
                    if let Ok(mut t) = joints.get_mut(joint) {
                        *t = shown;
                    }
                }
            }
            continue;
        }
        let (cursor, frac) = match &replay.previous {
            Some(previous) if !replay.cursor.finished => previous.render_cursor(&*lines, &replay.branches, &replay.tricks, ahead),
            _ => (replay.cursor.clone(), 0.0),
        };
        let sample = cursor.sample(&*lines, frac);
        let Some(s) = sample.or_else(|| replay.last.clone()) else { continue };
        // The avoider's lateral offset, interpolated like the cursor.
        let offset = replay.avoid.offset_at(ahead / FRAMES_PER_TICK as f32);
        root.translation = Vec3::from_array(s.position) + Vec3::from_array(super::npc_avoid::right_of(s.velocity)) * offset;
        let style = puppet.map_or_else(|| crate::custom_models::native_animation_style(&npc.character), |p| p.style);
        // A mod's clip that does not evaluate falls back to the shipped pick (also the outgoing
        // clip of a crossfade).
        let usable = |phase: ReplayPhase| {
            let clip = resolve_puppet_clip(&settings.skater_clips, phase, style);
            match skater.as_deref() {
                Some(k) if k.animation.evaluator.clip_length(clip).is_err() => puppet_clip(phase, style),
                _ => clip,
            }
        };
        // Clip and blend time include the render sub-frame (no 60 Hz stair steps under a smooth root).
        npc_puppet_layers_into(&settings, scores, style, skater.as_deref(), &cursor, s.sub_frame, layers);
        if layers.is_empty() {
            layers.push(PuppetLayer { clip: usable(s.phase).to_owned(), time: (s.phase_frames as f32 + s.sub_frame) / 60.0, weight: 1.0, since: cursor.frames.saturating_sub(s.phase_frames), tree: None });
        }
        // The stance bits each layer was built with (natural stance, toggled by trick clips).
        let natural = StanceFlags::natural(npc.stance);
        layer_flags.clear();
        layer_flags.extend(layers.iter().map(|l| stance.map_or(natural, |t| t.flags_for(l.since))));
        let top_flags = *layer_flags.last().expect("one layer");
        root.rotation = root_rotation(&s) * flags_root_turn(top_flags);
        let top = layers.last().expect("one layer");
        let (clip, time) = (top.clip.as_str(), top.time);
        let blend = (layers.len() >= 2).then(|| {
            let from = &layers[layers.len() - 2];
            (from.clip.as_str(), from.time, top.weight)
        });
        let fakie = fakie_channel_layer(&cursor, frac, resolve_fakie_channel(&settings.skater_clips));
        let bound = skater.as_deref().zip(puppet.and_then(|p| p.bindings.as_ref()));
        let posed = bound.and_then(|(skater, bindings)| {
            // A channel tree that does not build (a mod's bad name) is left out.
            let weight = fakie.as_ref().and_then(|f| {
                crate::graph_host::motion::cached_tree_commands_into(&skater.animation.motion.animation, fakie_tree, &f.clip, &fakie_channel_attributes(), f.time, channel).ok().map(|_| f.weight)
            });
            pose.trees.resize_with(layers.len(), Vec::new);
            for ((l, flags), out) in layers.iter().zip(layer_flags.iter()).zip(pose.trees.iter_mut()) {
                out.clear();
                if let Some(t) = &l.tree {
                    let x = if flags.mirrored { -t.tweak[0] } else { t.tweak[0] };
                    if crate::graph_host::motion::named_tree_commands_into(&skater.animation.motion.animation, grab_trees, &t.name, &tweak_attributes([x, t.tweak[1]]), t.time, out).is_err() {
                        out.clear();
                    }
                }
            }
            if !puppet_pose_in_flags_into(&skater.animation.evaluator, layers, layer_flags, weight.map(|w| (channel.as_slice(), w)), pose) {
                return None;
            }
            bindings.pose_transforms_into(&pose.globals, transforms);
            for &(joint, local) in transforms.iter() {
                if let Ok(mut t) = joints.get_mut(joint) {
                    *t = local;
                }
            }
            Some(())
        });
        let posed = posed.is_some();
        match current {
            // Written in place only on a difference (same value and change ticks as building the next clip).
            Some(mut c) => {
                let same = c.phase == s.phase && c.clip == clip && c.time == time && c.blend.as_ref().map(|b| (b.from.as_str(), b.from_time, b.weight)) == blend && c.layers == *layers && c.posed == posed && c.fakie == fakie;
                if !same {
                    let c = &mut *c;
                    c.phase = s.phase;
                    c.clip.clear();
                    c.clip.push_str(clip);
                    c.time = time;
                    c.blend = blend.map(|(from, from_time, weight)| PuppetBlend { from: from.to_owned(), from_time, weight });
                    c.layers.clone_from(layers);
                    c.posed = posed;
                    c.fakie = fakie;
                }
            }
            None => {
                let blend = blend.map(|(from, from_time, weight)| PuppetBlend { from: from.to_owned(), from_time, weight });
                commands.entity(e).insert(NpcPuppetClip { phase: s.phase, clip: clip.to_owned(), time, blend, layers: layers.clone(), posed, fakie });
            }
        }
    }
}

/// Buffers [`present_pose`] reuses across NPCs and frames.
#[derive(Default)]
pub(crate) struct NpcPoseScratch {
    layers: Vec<PuppetLayer>,
    layer_flags: Vec<StanceFlags>,
    channel: Vec<skate_core::animation::playback_tree::PoseCommand>,
    fakie_tree: Option<crate::graph_host::motion::TreeCache>,
    grab_trees: Vec<crate::graph_host::motion::TreeCache>,
    pose: PoseScratch,
    transforms: Vec<(Entity, Transform)>,
    before: Vec<(Entity, Transform)>,
    pose_now: Vec<Mat4>,
    pose_before: Vec<Mat4>,
}

/// A material an NPC fade can blend: its own base alpha and how a blended copy takes a new one.
pub(crate) trait FadeMaterial: Material + Clone {
    fn fade_base(&self) -> f32;
    /// Blend the copy at `alpha` (the base alpha times the fade).
    fn set_fade(&mut self, alpha: f32);
    /// The source draws opaque: its fade is the output alpha alone, not times its texture's alpha (opaque retail
    /// textures keep other data in alpha; without this, clothes and boards vanished while skin showed). Sent as a
    /// negative tag value (vendor/README.md "Fade alpha in MeshTag").
    fn fade_opaque(&self) -> bool {
        false
    }
}

impl FadeMaterial for StandardMaterial {
    fn fade_base(&self) -> f32 {
        self.base_color.alpha()
    }
    fn set_fade(&mut self, alpha: f32) {
        self.alpha_mode = AlphaMode::Blend;
        self.base_color.set_alpha(alpha);
    }
    fn fade_opaque(&self) -> bool {
        matches!(self.alpha_mode, AlphaMode::Opaque)
    }
}

/// Retail's ped program draws its fade as the output alpha (`i_params.x`).
impl FadeMaterial for super::ped_material::PedMaterial {
    fn fade_base(&self) -> f32 {
        self.params.options.y
    }
    fn set_fade(&mut self, alpha: f32) {
        self.alpha = AlphaMode::Blend;
        self.params.options.y = alpha;
    }
}

/// A fade alpha as the `MeshTag` the PBR and ped shaders read (`bitcast<f32>(tag ^ 0x80000000)`; tag 0 = not fading):
/// the same f32 the copy's material held before. -0.0 (which would be tag 0) draws as +0.0, the same blend.
pub(crate) fn fade_tag(alpha: f32) -> u32 {
    let bits = alpha.to_bits() ^ 0x8000_0000;
    if bits == 0 { 0x8000_0000 } else { bits }
}

/// Blended material copies of one NPC while it fades (spawn fade in, leave fade). Absent while the
/// NPC is solid. Dropping it (despawn, mod disable, map change) frees the copies: they are only
/// held here.
#[derive(Component)]
pub(crate) struct NpcFadeMaterials<M: FadeMaterial = StandardMaterial> {
    /// Mesh entity and its own (shared) material, put back at alpha 1.
    pub originals: Vec<(Entity, Handle<M>)>,
    /// Source material -> its blended copy and the source's own base alpha (negative when the source is opaque).
    pub copies: BTreeMap<AssetId<M>, (Handle<M>, f32)>,
    /// Alpha the copies were last set to (NaN = not yet).
    pub alpha: f32,
}

impl<M: FadeMaterial> Default for NpcFadeMaterials<M> {
    fn default() -> Self {
        Self { originals: Vec::new(), copies: BTreeMap::new(), alpha: f32::NAN }
    }
}

/// Draw [`NpcFade::alpha`] with per-NPC blended material copies below 1 (render only), for each material type.
pub(crate) fn present_fade<M: FadeMaterial>(
    mut commands: Commands,
    mut npcs: Query<(Entity, &NpcFade, Option<&mut NpcFadeMaterials<M>>)>,
    children: Query<&Children>,
    mut meshes: Query<&mut MeshMaterial3d<M>>,
    mut tags: Query<&mut bevy::mesh::MeshTag>,
    materials: Option<ResMut<Assets<M>>>,
) {
    let Some(mut materials) = materials else { return };
    for (e, fade, state) in &mut npcs {
        let alpha = fade.alpha.clamp(0.0, 1.0);
        if alpha >= 1.0 {
            if let Some(state) = state {
                for (mesh, original) in &state.originals {
                    if let Ok(mut m) = meshes.get_mut(*mesh) {
                        m.0 = original.clone();
                    }
                    if tags.contains(*mesh) {
                        commands.entity(*mesh).remove::<bevy::mesh::MeshTag>();
                    }
                }
                commands.entity(e).remove::<NpcFadeMaterials<M>>();
            }
            continue;
        }
        let mut fresh = None;
        let state: &mut NpcFadeMaterials<M> = match state {
            Some(s) => s.into_inner(),
            None => fresh.insert(NpcFadeMaterials::default()),
        };
        let mut swapped = false;
        for mesh in children.iter_descendants(e) {
            let Ok(mut m) = meshes.get_mut(mesh) else { continue };
            let current = m.0.id();
            if state.copies.values().any(|(h, _)| h.id() == current) {
                continue;
            }
            let copy = match state.copies.get(&current) {
                Some((h, _)) => h.clone(),
                None => {
                    let Some(source) = materials.get(current) else { continue };
                    let _event = super::timing::event(super::timing::EV_FADE_COPY);
                    let mut blended = source.clone();
                    let base = source.fade_base();
                    // Signed: negative = opaque source, so the tag below says "the fade alone".
                    let signed = if source.fade_opaque() { -base } else { base };
                    blended.set_fade(base * alpha);
                    let h = materials.add(blended);
                    state.copies.insert(current, (h.clone(), signed));
                    h
                }
            };
            state.originals.retain(|(x, _)| *x != mesh);
            state.originals.push((mesh, m.0.clone()));
            m.0 = copy;
            swapped = true;
        }
        // The alpha goes to each mesh's MeshTag, not into the copy: a material write rebuilt its bind group every frame
        // (doc 34 "Fades: MeshTag").
        if swapped || state.alpha != alpha {
            for (mesh, original) in &state.originals {
                let Some((_, base)) = state.copies.get(&original.id()) else { continue };
                let tag = bevy::mesh::MeshTag(fade_tag(*base * alpha));
                match tags.get_mut(*mesh) {
                    Ok(mut t) => *t = tag,
                    Err(_) => {
                        commands.entity(*mesh).insert(tag);
                    }
                }
            }
            state.alpha = alpha;
        }
        if let Some(s) = fresh {
            commands.entity(e).insert(s);
        }
    }
}

/// Global (model-space) bone matrices of a stock clip `time` s into its phase (wrapped when the
/// clip loops), root trajectory held at the origin (the cursor moves the root).
pub(crate) fn evaluator_pose(evaluator: &crate::animation_pose::PoseEvaluator, clip: &str, time: f32) -> Option<Vec<Mat4>> {
    puppet_blend_pose(evaluator, clip, time, None)
}

/// [`evaluator_pose`] with an optional crossfade: like a graph transition, the outgoing and the
/// incoming clip are evaluated and blended with `PoseCommand::Blend` (the player's SQT blend,
/// `pose_blend::blend_sample`), then the reference pose is added to the result.
pub(crate) fn puppet_blend_pose(evaluator: &crate::animation_pose::PoseEvaluator, clip: &str, time: f32, blend: Option<&PuppetBlend>) -> Option<Vec<Mat4>> {
    let mut layers = Vec::with_capacity(2);
    if let Some(b) = blend {
        layers.push(PuppetLayer { clip: b.from.clone(), time: b.from_time, weight: 1.0, since: 0, tree: None });
    }
    layers.push(PuppetLayer { clip: clip.to_owned(), time, weight: blend.map_or(1.0, |b| b.weight), since: 0, tree: None });
    puppet_layers_pose(evaluator, &layers)
}

/// [`puppet_blend_pose`] for nested layers, oldest first (SQT blend like a graph transition).
pub(crate) fn puppet_layers_pose(evaluator: &crate::animation_pose::PoseEvaluator, layers: &[PuppetLayer]) -> Option<Vec<Mat4>> {
    puppet_pose_with_channel(evaluator, layers, None)
}

/// [`puppet_layers_pose`] with a channel overlay on top of the layers (the fakie channel): the
/// channel tree's pose commands blended over the layered pose with `PoseCommand::ChannelBlend`
/// (its per-bone channel weights, like `MotionChannels::evaluate`) at the channel's weight.
pub(crate) fn puppet_pose_with_channel(evaluator: &crate::animation_pose::PoseEvaluator, layers: &[PuppetLayer], channel: Option<(&[skate_core::animation::playback_tree::PoseCommand], f32)>) -> Option<Vec<Mat4>> {
    puppet_pose_in_stance(evaluator, layers, channel, false)
}

/// The puppet root's extra turn for a natural stance (`flags_root_turn` of its start bits).
pub(crate) fn stance_root_turn(stance: skate_core::living_world::stance::NaturalStance) -> Quat {
    flags_root_turn(StanceFlags::natural(stance))
}

/// Puppet root extra turn for the newest layer's stance bits (mirror bit alone): docs/hails-additions/26c-living-world-npc-skaters.md#flags-root-turn
pub(crate) fn flags_root_turn(flags: StanceFlags) -> Quat {
    if flags.mirrored { Quat::from_rotation_y(std::f32::consts::PI) } else { Quat::IDENTITY }
}

/// [`stance_tail`] for a natural stance's start bits (`mirrored` = regular).
pub(crate) fn bind_pose_tail(mirrored: bool) -> Vec<skate_core::animation::playback_tree::PoseCommand> {
    stance_tail(StanceFlags { board_backward: mirrored, mirrored, switch: false })
}

/// Bind pose commands retail appends to every tree (bit 31 BOARD_BACKWARDS, bit 30 mirror mode 2).
pub(crate) fn stance_tail(flags: StanceFlags) -> Vec<skate_core::animation::playback_tree::PoseCommand> {
    let mut out = Vec::new();
    stance_tail_into(flags, &mut out);
    out
}

/// [`stance_tail`] appended to `out`.
fn stance_tail_into(flags: StanceFlags, out: &mut Vec<skate_core::animation::playback_tree::PoseCommand>) {
    use skate_core::animation::playback_tree::PoseCommand;
    out.extend([PoseCommand::Pose { name: "RIG_TPOSE".into() }, PoseCommand::Add { motion_is_a: true }]);
    if flags.board_backward {
        out.extend([
            PoseCommand::Pose { name: "BOARD_BACKWARDS".into() },
            PoseCommand::Add { motion_is_a: false },
            PoseCommand::Pose { name: "BOARD_BACKWARDS_IK".into() },
            PoseCommand::Add { motion_is_a: true },
        ]);
    }
    if flags.mirrored {
        out.push(PoseCommand::Mirror { trajectory_mode: 2 });
    }
}

/// [`puppet_pose_with_channel`] in a natural stance (`mirrored` = regular): every layer with the
/// stance's start bits ([`puppet_pose_in_flags`]).
pub(crate) fn puppet_pose_in_stance(evaluator: &crate::animation_pose::PoseEvaluator, layers: &[PuppetLayer], channel: Option<(&[skate_core::animation::playback_tree::PoseCommand], f32)>, mirrored: bool) -> Option<Vec<Mat4>> {
    let flags = StanceFlags { board_backward: mirrored, mirrored, switch: false };
    puppet_pose_in_flags(evaluator, layers, &vec![flags; layers.len()], channel)
}

/// Puppet pose with each layer in its own stance bits: docs/hails-additions/26c-living-world-npc-skaters.md#puppet-pose-in-flags
pub(crate) fn puppet_pose_in_flags(evaluator: &crate::animation_pose::PoseEvaluator, layers: &[PuppetLayer], flags: &[StanceFlags], channel: Option<(&[skate_core::animation::playback_tree::PoseCommand], f32)>) -> Option<Vec<Mat4>> {
    let mut scratch = PoseScratch::default();
    puppet_pose_in_flags_into(evaluator, layers, flags, channel, &mut scratch).then_some(scratch.globals)
}

/// Reused buffers of [`puppet_pose_in_flags_into`].
#[derive(Default)]
pub(crate) struct PoseScratch {
    commands: Vec<skate_core::animation::playback_tree::PoseCommand>,
    /// Per layer: the pose commands of its tree ([`PuppetLayer::tree`]), played in place of its last clip; empty = none.
    pub trees: Vec<Vec<skate_core::animation::playback_tree::PoseCommand>>,
    locals: Vec<Mat4>,
    pub globals: Vec<Mat4>,
}

/// [`puppet_pose_in_flags`] into `scratch.globals`; false where it gives `None`.
pub(crate) fn puppet_pose_in_flags_into(evaluator: &crate::animation_pose::PoseEvaluator, layers: &[PuppetLayer], flags: &[StanceFlags], channel: Option<(&[skate_core::animation::playback_tree::PoseCommand], f32)>, scratch: &mut PoseScratch) -> bool {
    use skate_core::animation::playback_tree::PoseCommand;
    let PoseScratch { commands, trees, locals, globals } = scratch;
    // Pushes layer `i`'s pose: its tree in its last part, else the clip playing.
    let sample = |commands: &mut Vec<PoseCommand>, i: usize, l: &PuppetLayer| -> bool {
        let Some((clip, time)) = sequence_part(evaluator, &l.clip, l.time) else { return false };
        if let Some(tree) = trees.get(i).filter(|t| !t.is_empty() && l.clip.rsplit('+').next().map(|p| part_speed(p).0) == Some(clip)) {
            commands.extend_from_slice(tree);
            return true;
        }
        let Ok(length) = evaluator.clip_length(clip) else { return false };
        let time = puppet_clip_time(clip, time, length);
        commands.push(PoseCommand::Clip { name: clip.to_owned(), previous_time: time, time, loops: 0 });
        true
    };
    let flag = |i: usize| flags.get(i).copied().unwrap_or_default();
    let Some((top, older)) = layers.split_last() else { return false };
    let top_flags = flag(layers.len() - 1);
    let mixed = (0..layers.len()).any(|i| flag(i) != top_flags);
    commands.clear();
    commands.reserve(layers.len() * 8 + 8);
    // Per-layer tails once the bits differ (each layer closed before it blends in).
    let closed = |commands: &mut Vec<PoseCommand>, i: usize| {
        if mixed {
            stance_tail_into(flag(i), commands);
        }
    };
    for (i, l) in older.iter().enumerate() {
        let base = commands.is_empty();
        if sample(commands, i, l) {
            closed(commands, i);
            if !base {
                commands.push(PoseCommand::Blend { weight: l.weight });
            }
        }
    }
    let base = commands.is_empty();
    if !sample(commands, layers.len() - 1, top) {
        return false;
    }
    closed(commands, layers.len() - 1);
    if !base {
        commands.push(PoseCommand::Blend { weight: top.weight });
    }
    // Shared bits: one tail for the blended layers. A tail that mirrors (or turns the board)
    // closes before the channel tree, which gets its own (as the player's channel tree).
    let shaped = top_flags.board_backward || top_flags.mirrored;
    if !mixed && shaped {
        stance_tail_into(top_flags, commands);
    }
    if let Some((tree, weight)) = channel.filter(|c| !c.0.is_empty() && c.1 > 0.0) {
        commands.extend_from_slice(tree);
        if mixed || shaped {
            stance_tail_into(top_flags, commands);
        }
        commands.push(PoseCommand::ChannelBlend { weight, use_channels_from_weights: false });
    }
    if !mixed && !shaped {
        stance_tail_into(top_flags, commands);
    }
    let Ok(pose) = evaluator.evaluate(commands) else { return false };
    locals.clear();
    locals.extend(pose.iter().copied().map(skate_core::animation::output::sqt_to_matrix).map(crate::animation::native_matrix));
    let parents = &evaluator.frames.parents;
    globals.clear();
    globals.reserve(locals.len());
    for (i, local) in locals.iter().enumerate() {
        let g = match parents.get(i).copied() {
            Some(p) if p >= 0 && (p as usize) < i => globals[p as usize] * *local,
            _ => *local,
        };
        globals.push(g);
    }
    true
}

/// The part of a `+`-joined clip sequence playing `time` s in and the clip time inside it: parts play
/// back to back at their [`part_speed`], the last one holds (or loops when it is a `_CYC` clip). A plain clip is itself.
pub(crate) fn sequence_part<'a>(evaluator: &crate::animation_pose::PoseEvaluator, clip: &'a str, time: f32) -> Option<(&'a str, f32)> {
    let mut parts = clip.split('+').peekable();
    let mut t = time.max(0.0);
    while let Some(part) = parts.next() {
        let (name, speed) = part_speed(part);
        if parts.peek().is_none() {
            return Some((name, t * speed));
        }
        let len = evaluator.clip_length(name).ok()? / speed;
        if t < len {
            return Some((name, t * speed));
        }
        t -= len;
    }
    None
}

/// A sequence part's clip and playback speed: `NAME*2` plays `NAME` at twice its rate (a state's `playBackSpeed`, the
/// clip clock's length (frames - 1) / (fps x speed)); a plain name plays at 1: docs/hails-additions/26c-living-world-npc-skaters.md#npc-grab-playback-speed
pub(crate) fn part_speed(part: &str) -> (&str, f32) {
    match part.rsplit_once('*').map(|(name, s)| (name, s.parse::<f32>())) {
        Some((name, Ok(speed))) if speed.is_finite() && speed > 0.0 => (name, speed),
        _ => (part, 1.0),
    }
}

/// `clip` as a sequence part played at `speed` (`None` or 1 = the plain name).
pub(crate) fn with_speed(clip: String, speed: Option<f32>) -> String {
    match speed {
        Some(s) if s != 1.0 => format!("{clip}*{s}"),
        _ => clip,
    }
}

/// One-line NPC summary for the debug readout: count, the nearest NPC (distance, line, phase).
pub(crate) fn npc_readout(npcs: &[(LivingWorldId, String, Option<ReplaySample>)], player: Option<[f32; 3]>) -> String {
    let nearest = player.and_then(|p| {
        npcs.iter()
            .filter_map(|(id, c, s)| s.as_ref().map(|s| (id, c, s, length([s.position[0] - p[0], s.position[1] - p[1], s.position[2] - p[2]]))))
            .min_by(|a, b| a.3.total_cmp(&b.3))
    });
    match nearest {
        Some((id, c, s, d)) => format!(
            "npc skaters {} nearest #{} {c} {:.0} m line {} node {} {} {:.1} m/s heading {:.0} velocity_yaw {:.0}",
            npcs.len(),
            id.serial,
            d,
            s.line.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            s.node,
            s.phase.name(),
            length(s.velocity),
            {
                let f = skate_core::living_world::replay::rotate(s.skater, [0.0, 0.0, 1.0]);
                f[0].atan2(f[2]).to_degrees()
            },
            s.velocity[0].atan2(s.velocity[2]).to_degrees()
        ),
        None => format!("npc skaters {}", npcs.len()),
    }
}

/// World ticks between two `NPC_SKATER_BACKWARDS` lines for one NPC (2 s at 30 ticks/s).
pub(crate) const BACKWARDS_LOG_TICKS: u64 = 60;

/// `NPC_SKATER_BACKWARDS` line for one NPC sample (135 deg diagnostic threshold): docs/hails-additions/26c-living-world-npc-skaters.md#npc-backwards-line
pub(crate) fn backwards_line(id: LivingWorldId, character: &str, line: &ReplayLine, cursor: &LineCursor, s: &ReplaySample) -> Option<String> {
    let c = skate_core::living_world::replay::facing_check(line, s)?;
    if !c.backwards {
        return None;
    }
    Some(format!(
        "NPC_SKATER_BACKWARDS #{} {character} line {} node {} heading {:.0} velocity_yaw {:.0} off {:.0} deg {:.1} m/s recorded_fakie {} drawn_fakie {} flip {} facing_flipped {} phase {} pos {:.1} {:.1} {:.1}",
        id.serial,
        s.line.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        s.node,
        c.heading_yaw.to_degrees(),
        c.velocity_yaw.to_degrees(),
        c.angle.to_degrees(),
        length(s.velocity),
        c.recorded_fakie,
        c.drawn_fakie,
        cursor.flip,
        cursor.facing_flipped,
        s.phase.name(),
        s.position[0],
        s.position[1],
        s.position[2],
    ))
}

/// Logs NPC skaters whose drawn heading opposes their travel (always on, rate limited per NPC to
/// one line per [`BACKWARDS_LOG_TICKS`]), so a reported "rides backwards" can be found in the log.
pub(crate) fn log_backwards(state: Res<PopulationState>, npcs: Query<(&NpcSkater, &NpcReplay, Option<&super::npc_sim::NpcSim>)>, mut last: Local<BTreeMap<LivingWorldId, u64>>) {
    let tick = state.world.tick();
    let lines = npc_lines(&state);
    last.retain(|id, _| npcs.iter().any(|(n, _, _)| n.id == *id));
    for (npc, replay, sim) in &npcs {
        // Rate limit first: it does not depend on the text, so only a due NPC formats a line.
        if !last.get(&npc.id).is_none_or(|t| tick < *t || tick >= t + BACKWARDS_LOG_TICKS) {
            continue;
        }
        // A simulated NPC is drawn from its body, never from the cursor's sample (b352).
        let text = if let Some(sim) = sim {
            super::npc_sim::backwards_line(npc.id, &npc.character, sim, lines.get(&replay.cursor.line), &replay.cursor)
        } else {
            let (Some(s), Some(line)) = (replay.last.as_ref(), lines.get(&replay.cursor.line)) else { continue };
            if s.line != replay.cursor.line {
                continue;
            }
            backwards_line(npc.id, &npc.character, line, &replay.cursor, s)
        };
        let Some(text) = text else { continue };
        last.insert(npc.id, tick);
        warn!("{text}");
    }
}

/// The debug readout system (`SKATE_LIVING_WORLD_DEBUG=1`, every 5 s with the population line).
pub(crate) fn log_readout(settings: Res<LivingWorldSettings>, state: Res<PopulationState>, observers: Res<LivingWorldObservers>, npcs: Query<(&NpcSkater, &NpcReplay)>, mut last: Local<u64>) {
    if !settings.debug || !super::report_due(state.world.tick(), &mut last, 150) {
        return;
    }
    let list: Vec<_> = npcs.iter().map(|(n, r)| (n.id, n.character.clone(), r.last.clone())).collect();
    info!("LIVING_WORLD {}", npc_readout(&list, observers.observers.first().map(|o| o.position)));
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<NpcSkaterIndex>()
        .init_resource::<super::npc_sim::NpcSimPool>()
        .init_resource::<NpcValidLines>()
        .init_resource::<NpcSkaterLooks>()
        .init_resource::<SaveSlotLooks>()
        .init_resource::<super::npc_avoid::AvoidTrack>()
        .add_message::<NpcSkaterEvent>()
        // After the ped data load: avoid and simulate read its navmesh, and unordered they saw the old map's (or
        // none) on a map's first tick depending on which the executor ran first.
        .add_systems(FixedUpdate, (crate::autopilot::engage, apply_records, super::npc_avoid::avoid, advance, crate::autopilot::possess_in, super::npc_sim::simulate, crate::autopilot::possess_out, super::npc_sim::reactions, track_stance, log_backwards, log_readout).chain().after(super::step_population).after(super::peds::load_ped_data).distributive_run_if(crate::replay::world_live))
        .add_systems(
            FixedUpdate,
            push_proxies.after(crate::multiplayer::prepare).after(crate::app::SimulationSet::Controls).before(crate::app::SimulationSet::Physics),
        )
        .add_systems(Update, (super::npc_outfits::present_outfits, present_looks, present_pose, present_fade::<StandardMaterial>, present_fade::<super::ped_material::PedMaterial>).chain().after(crate::app::FrameSet::Animation));
}

#[cfg(test)]
mod peer_name_tests {
    use super::*;

    /// b620: the allocation-free sequence test gives the old `ends_with(format!("+{b}"))` result for every pair.
    #[test]
    fn sequence_ends_in_matches_the_format_reference() {
        let names = ["", "+", "a", "b", "ab", "a+b", "+b", "b+", "a+a", "x+ab", "ab+b", "kick+", "ollie+kickflip", "kickflip", "é+b", "aé", "é"];
        for a in names {
            for b in names {
                assert_eq!(sequence_ends_in(a, b), a.ends_with(&format!("+{b}")), "{a:?} {b:?}");
            }
        }
    }

    /// NPC_SKATER_CONTACT names the player, a simulated NPC by serial (body or board half), anything else raw.
    #[test]
    fn contact_peers_have_readable_names() {
        assert_eq!(peer_name(PLAYER_PEER), "player");
        let id = LivingWorldId { kind: skate_core::living_world::Kind::Skater, serial: 167 };
        assert_eq!(peer_name(sim_peer(id)), "#167");
        assert_eq!(peer_name(sim_peer(id) | PROXY_BOARD_BIT), "#167");
        assert_eq!(peer_name(42), "0x2a");
    }
}