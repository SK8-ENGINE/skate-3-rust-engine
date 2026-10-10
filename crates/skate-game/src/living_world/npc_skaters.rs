//! NPC skaters, replay tier (doc 26, milestone 3): turns the population's skater spawn / despawn
//! records into visible skaters that ride their recorded lines.
//!
//! - **Entity** per spawn record: [`NpcSkater`] (stable `LivingWorldId`, character key, slot,
//!   seed, voice) + [`NpcReplay`] (the `skate_core::living_world::replay` cursor) + `Transform`.
//!   The id map [`NpcSkaterIndex`] finds it again for the despawn.
//! - **Motion** (`FixedUpdate`, after the population step): each cursor is kept at
//!   `population tick - spawn tick` recording frames (the 60 Hz lines at the retail 60 Hz world
//!   tick, `skate_core::living_world::clock`), so the state is a function of the spawn record and the tick; branch decisions use the
//!   retail score with the players and the other NPCs and are kept as records a client would
//!   mirror. The NPC's position goes back into the population (culls, the 5 m rule). At the end
//!   of a line it continues on an unused line starting within 4 m, picked by the same retail
//!   score (`replay::choose_next_line`, `sub_8246C7F8`; fix 9), so it keeps riding until the
//!   population's 120 m cull removes it. A new NPC fades in over its first second; only at a dead
//!   end (no line starts near the end) does it fade out and leave once its opacity is below 0.2
//!   (`skate_core::living_world::leave_fade`; [`NpcFade`]).
//! - **Collision**: a kinematic proxy (capsule for the body, box for the board, infinite mass, the
//!   cursor's velocity) joins `physics::network::Proxies` like a mod's solid, so the player bumps
//!   into it. NPCs never react (replay tier).
//! - **Look**: the character's native roster GLB (`CustomModels::online_native_path`, the same
//!   files the customiser and online players use), or a mod's GLB from [`NpcSkaterLooks`], else
//!   the stock skater. Bound to the stock skeleton like a remote player (`AnimationStatus`).
//! - **Puppet animation** (`Update`): one stock clip per [`ReplayPhase`] ([`puppet_clip`]),
//!   evaluated with the player's evaluator; root = the recorded position and skater orientation.
//!   The stock graphs are not run (simplification until the simulated tier). The clip time is the
//!   time since the phase began; a looping clip (`_CYC` in its name) wraps by its length like the
//!   player's `ClipClock`, others hold their last frame. Each NPC carries [`NpcPuppetClip`] (the
//!   clip and time in effect). A mod may override the clip per phase id or `<phase>.<style>`
//!   (`LivingWorldSettings::skater_clips`, `sdk.world.set_tuning('living_world', {skater_clips})`);
//!   an override that does not evaluate falls back to the shipped pick. A phase change (also one a
//!   branch or line chain brings) crossfades from the previous phase's clip, which keeps playing,
//!   with the player's graph transition curve (`playback_transition::transition_weight`,
//!   Blend82B96058: smoothstep over the transition time) over [`RETAIL_BLEND_SECONDS`] (the stock
//!   graph's default transition time); mods set it per phase (`skater_blend_seconds`). The
//!   weight is a function of the cursor's phase frames, so a client derives the same pose.
//!   Render (fix 14): drawn from the cursor one tick back ([`NpcReplay::previous`]) towards the
//!   current one by the fixed-step fraction, like the player, with the recorded branches
//!   (`LineCursor::render_sample`); clip and blend times include the sub-frame; after a branch or
//!   chain the root blends onto the new line (`replay::LineSwitch`, `skater_line_chain.blend_seconds`).
//!   Fix 21: the crossfade nests like a retail transition (Blend82B96058 keeps the running
//!   transition as the outgoing tree): the cursor remembers the last `replay::PHASE_HISTORY` phases and
//!   the pose is built from every phase whose blend still runs ([`puppet_layers`]), so a phase
//!   change during a blend (half of all changes on the shipped lines) no longer restarts from one
//!   clip. A recorded trick slot (`EScorableID`) plays its stock trick animation on body and board
//!   (the board is the rig's `Skateboard_Root`): `<anim>_G` on the ground, `<anim>_A` once
//!   airborne, like `T_Trick.xml` ([`retail_trick_anim`]; mods: `skater_clips["trick.<id name>"]`).
//! - **Fade** (`Update`, [`present_fade`]): while [`NpcFade::alpha`] is below 1 every mesh under
//!   the NPC (body and board) draws with a per-NPC blended copy of its material; at 1 the shared
//!   materials come back and the copies are freed, so a solid NPC costs what it did before.
//! - **Audio**: [`NpcSkaterAudio`](crate::world_audio::NpcSkaterAudio) with a lite state from the
//!   cursor (position, velocity, air, ground trick as a grind) and the character's voice; #32's
//!   host picks the one audible NPC by retail's rule.
//! - **Events** for engine systems and the planned `sdk.living_world`: [`NpcSkaterEvent`].
//!
//! Multiplayer: nothing here decides a spawn; the entity is rebuilt from a `SpawnRecord`, the tick
//! and [`BranchRecord`]s alone (`Decider::Mirror` on a client).

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

/// The trick tables an NPC uses: the disc's table of its `aiprofile` (else `default`), each table
/// replaced by a mod's for the character key, else for the profile name.
pub(crate) fn npc_trick_profile(data: &skate_data::living_world::SkaterTrickProfiles, overrides: &BTreeMap<String, NpcTrickTables>, character: &str) -> Option<TrickProfile> {
    let base = data.for_character(character).cloned();
    if overrides.is_empty() {
        return base;
    }
    let profile_name = data.character_profile.get(character).map_or("default", String::as_str);
    let by_key = overrides.get(character);
    let by_profile = overrides.get(profile_name);
    let pick = |f: fn(&NpcTrickTables) -> &Option<Vec<(i16, f32)>>, disc: Option<&Vec<(i16, f32)>>| {
        by_key.and_then(|o| f(o).clone()).or_else(|| by_profile.and_then(|o| f(o).clone())).or_else(|| disc.cloned())
    };
    let regular = pick(|o| &o.regular, base.as_ref().map(|b| &b.regular));
    let nollie = pick(|o| &o.nollie, base.as_ref().map(|b| &b.nollie));
    (regular.is_some() || nollie.is_some()).then(|| TrickProfile { regular: regular.unwrap_or_default(), nollie: nollie.unwrap_or_default() })
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

/// An NPC's natural stance at spawn (retail: once, from its character record, `82590DC0` ->
/// `Initialize82B97E38`): the record of the character key, a mod override, the table, else the
/// engine default. A function of the spawn record's character and the overrides, so a client
/// derives the host's value.
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

/// LivingWorldId -> entity.
#[derive(Resource, Default)]
pub(crate) struct NpcSkaterIndex(pub BTreeMap<LivingWorldId, Entity>);

/// Mod / engine look overrides: character key -> GLB asset path. Empty = retail looks.
#[derive(Resource, Default, Clone)]
pub(crate) struct NpcSkaterLooks(pub BTreeMap<String, String>);

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
}

/// The stock clip a replay NPC shows per phase ([data]: names checked against the decoded stock
/// clip list, `crate::living_world` data-gated test). Riding uses the pro's style like the
/// customiser (`native_animation_style`).
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

/// Transition time (s) into a phase's clip: a `PlayAnimation` without a `time` attribute
/// transitions over the literal at `0x82099280` = 0.2 s ([code], read in
/// `graph_host::motion_nodes::transition`), and the stock motion graph's looping idle states the
/// puppet clips stand for enter with 0.2 s ([data] `air.xml` `B_AIR_CYC` time 0.2,
/// `offboard.xml` stand `BLENDSEC` 0.2, `T_handplantair.xml` `IA_IDLE_N_N_0_CYC` time 0.2).
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

/// The stock trick animation base a recorded trick slot plays (`<base>_G` on the ground,
/// `<base>_A` in the air), from the trick's `EScorableID` ([code] node ext +0x24, see
/// `ReplayLine::node_trick`; names `skate_core::scoring::catalog`, table 0x820862A8). Pairs are
/// [data] `MotionGraphIncludes/Tricks/Tricks.xml` (`TRICK_NAME` -> `ANIM_NAME`): the flips and
/// shuvits by name, kickflip / heelflip and their nollie forms use `B_<NAME>_IN`; numbered,
/// late and underflip variants use their base flip; a grab or any other air trick leaves the
/// ground with the ollie (`B_OLLIE`, retail grabs are performed out of an ollie). Ground-only
/// tricks (manuals, powerslides, reverts, grinds, slides; catalog classes 0 and 1) and
/// handplants (class 6) have none and keep the phase clip.
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

/// The clip a remembered phase shows and its transition time (`older` = the phase before it):
/// a trick span with a trick animation plays `<base>_G` then `<base>_A` from the ground (0.05 s
/// in), or `<base>_A` alone when it starts in the air (0.1 s in); the air after an air trick keeps
/// the clip (one layer); everything else the phase's puppet clip with its blend time. A clip
/// that does not evaluate (`evaluates` false) falls back to the shipped phase pick; trick names
/// go through `stock` (authored tree name -> playable clip, [`stock_tree_leaf`]).
pub(crate) fn puppet_layer_clip(
    clips: &BTreeMap<String, String>,
    blends: &BTreeMap<String, f32>,
    style: &str,
    evaluates: &dyn Fn(&str) -> bool,
    stock: &dyn Fn(&str) -> Option<String>,
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

/// The air part of a trick animation base: `<base>_A`, and for the kickflip family (`T_Kickflip.xml`:
/// `LeftGround` plays `$ANIM_NAME$_A`, then as sequences `$ANIM_CYC_NAME$1..3` per extra flip and
/// `$ANIM_OUT_NAME$<n>`) `B_<FLIP>_IN_A`, `B_<FLIP>_CYC1..n`, `B_<FLIP>_OUT<n>` with n the
/// trick's flip count (`kickflip2` = 2; 1 without a digit). Parts are joined with `+`: the
/// puppet plays them back to back (`transType="sequence"`, no blend).
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
/// child, a phase blend its first child (the low end of its parameter, e.g. `TRICKHEIGHT` 0:
/// the replay does not carry the player's trick height). Blend and selection spaces: none.
pub(crate) fn stock_tree_leaf(meta: &skate_data::animation_metadata::AnimationMetadata, name: &str) -> Option<String> {
    use skate_data::animation_metadata::TreeMetadata;
    let mut name = name.to_owned();
    for _ in 0..8 {
        name = match meta.tree(&name).ok()? {
            TreeMetadata::Clip(c) => return Some(c.name.clone()),
            TreeMetadata::Selector(s) => s.default.clone(),
            TreeMetadata::PhaseBlend(p) => p.children.first()?.clone(),
            _ => return None,
        };
    }
    None
}

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
}

/// The puppet's layers, oldest first, from the cursor's phases (`history`: newest first, seconds
/// since each began). `resolve(entry, older)` gives a phase's clip and its transition time.
/// Consecutive phases showing the same clip are one layer (the clip keeps playing from the older
/// start: a trick's air clip runs on after its span closes). Layers stop at the first one fully
/// blended in: older ones no longer show. Pure: same history, same layers.
pub(crate) fn puppet_layers(history: &[(PhaseEntry, f32)], resolve: impl Fn(PhaseEntry, Option<PhaseEntry>) -> (String, f32)) -> Vec<PuppetLayer> {
    let resolved: Vec<(String, f32, f32, u64)> = history
        .iter()
        .enumerate()
        .map(|(i, (e, t))| {
            let (clip, seconds) = resolve(*e, history.get(i + 1).map(|h| h.0));
            (clip, seconds, *t, e.since)
        })
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < resolved.len() {
        // An older phase whose clip is this one, or a sequence ending in it (`<ground>+<air>`
        // before `<air>`), is the same playing clip: continue it from the older start.
        let mut j = i;
        while j + 1 < resolved.len() && (resolved[j + 1].0 == resolved[j].0 || resolved[j + 1].0.ends_with(&format!("+{}", resolved[j].0))) {
            j += 1;
        }
        let (ref clip, seconds, time, since) = resolved[j];
        let weight = skate_core::animation::playback_transition::transition_weight(time, seconds);
        out.push(PuppetLayer { clip: clip.clone(), time, weight, since });
        if weight >= 1.0 {
            break;
        }
        i = j + 1;
    }
    out.reverse();
    out
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

/// The stock tree retail overlays while the skater rides fakie: [code] `FakieHeadChannel82BAC778`
/// starts channel `"fakie"` with `B_FAKIE_CHANNEL` (blend in / out 0.3 s, transition 0.1 s) on
/// the rising edge of the riding-fakie bit and ends it when the bit clears; the riding clip below
/// it is unchanged. A mod may replace it (`skater_clips["fakie_channel"]`).
pub(crate) const FAKIE_CHANNEL: &str = "B_FAKIE_CHANNEL";

/// The fakie channel tree in effect: a mod's `skater_clips["fakie_channel"]`, else [`FAKIE_CHANNEL`].
pub(crate) fn resolve_fakie_channel(overrides: &BTreeMap<String, String>) -> &str {
    overrides.get("fakie_channel").map_or(FAKIE_CHANNEL, String::as_str)
}

/// The `torso` value the fakie channel holds while riding: [code] `FakieHeadChannel82BAC778`
/// targets 1.0 while manualing, 0.0 while power sliding, else 0.5 (and starts at its target).
/// The replay has no manual / powerslide state, so the puppet always uses the riding value (NOT
/// RETAIL YET for recorded manuals and powerslides).
pub(crate) const FAKIE_TORSO_RIDING: f32 = 0.5;

/// The fakie channel layer of a cursor `alpha` of the next frame: `clip` = the channel tree
/// (`B_FAKIE_CHANNEL`, a stock phase blend on `torso`), its time since the bit set (the channel
/// keeps that clock while it fades out) and its weight ([`LineCursor::fakie_channel_weight`]);
/// `None` while the weight is 0.
pub(crate) fn fakie_channel_layer(cursor: &LineCursor, alpha: f32, tree: &str) -> Option<PuppetLayer> {
    let weight = cursor.fakie_channel_weight(alpha);
    if weight <= 0.0 {
        return None;
    }
    let began = if cursor.fakie { cursor.fakie_since } else { cursor.fakie_previous_since };
    let time = (cursor.frames - began.min(cursor.frames)) as f32 / 60.0 + alpha.clamp(0.0, 1.0) / 60.0;
    Some(PuppetLayer { clip: tree.to_owned(), time, weight, since: began })
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

/// The puppet's layers for a cursor (`sub_frame` = the render fraction, 0 on a fixed step): the
/// phase history resolved to clips ([`puppet_layer_clip`] with the mods' clips and blend times,
/// the shipped pick where a clip does not evaluate) and folded into layers ([`puppet_layers`]).
/// Empty before the first phase. Without the skater runtime every clip counts as playable.
pub(crate) fn npc_puppet_layers(settings: &LivingWorldSettings, style: &str, skater: Option<&crate::physics::SkaterRuntime>, cursor: &LineCursor, sub_frame: f32) -> Vec<PuppetLayer> {
    let evaluates = |clip: &str| skater.is_none_or(|k| k.animation.evaluator.clip_length(clip).is_ok());
    let meta = skater.map(|k| k.animation.motion.animation.metadata());
    let stock = |name: &str| match meta {
        Some(m) => stock_tree_leaf(m, name).filter(|c| evaluates(c)),
        None => Some(name.to_owned()),
    };
    let resolve = |e: PhaseEntry, older: Option<PhaseEntry>| puppet_layer_clip(&settings.skater_clips, &settings.skater_blend_seconds, style, &evaluates, &stock, e, older);
    let history: Vec<(PhaseEntry, f32)> = cursor.phase_history().map(|(e, frames)| (e, (frames as f32 + sub_frame) / 60.0)).collect();
    puppet_layers(&history, resolve)
}

/// Layer starts an NPC remembers (its puppet shows at most a few layers at once).
const STANCE_STARTS: usize = 8;

/// An NPC skater's stance bits (retail SkaterAnim flags bits 31 / 30 and the relative stance,
/// [`StanceFlags`]): the natural stance's start bits, toggled by the stance events of the clip its
/// newest layer plays, like retail's `82593230` -> `82B98980` for every actor. Each layer keeps
/// the bits current when it began (retail bakes them into the tree's bind pose at construction).
/// Simulation state, updated on the fixed step from the cursor and the clip data only
/// ([`track_stance`]), so a client derives the host's bits; plain data for a snapshot.
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

    /// One fixed step with the newest layer `top`: a new layer (clip or start changed) records the
    /// current bits as its own and plays from its clip start; then every stance event in the
    /// clip's attributes collected over the advanced window (`attributes(clip, previous_time,
    /// time)`, names) toggles its bit ([`StanceFlags::apply`]). Returns whether a bit changed.
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

/// The attribute names of a (`+`-joined) clip collected over `previous_time..time` s of its
/// playback ([`skate_core::living_world::stance::attribute_in_window`] per part: parts play back
/// to back, a part entered in the window plays from its start, the last part holds at its end).
/// `clip_data(part)` = the part's length (s) and its attributes (name, begin, end; normalised).
pub(crate) fn sequence_attributes(clip: &str, previous_time: f32, time: f32, clip_data: impl Fn(&str) -> Option<(f32, Vec<(String, f32, f32)>)>) -> Vec<String> {
    let mut out = Vec::new();
    let mut offset = 0.0f32;
    let mut parts = clip.split('+').peekable();
    while let Some(part) = parts.next() {
        let last = parts.peek().is_none();
        let Some((length, attributes)) = clip_data(part) else { break };
        let end = offset + length;
        let reached = time >= offset;
        let left = !last && previous_time >= end;
        if reached && !left {
            let p = (previous_time - offset).clamp(0.0, length);
            let t = (time - offset).clamp(0.0, length);
            out.extend(attributes.iter().filter(|(_, b, e)| skate_core::living_world::stance::attribute_in_window(*b, *e, length, p, t)).map(|(n, ..)| n.clone()));
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
    skater: Option<Res<crate::physics::SkaterRuntime>>,
    state: Res<PopulationState>,
    mut npcs: Query<(&NpcSkater, &NpcReplay, Option<&NpcPuppet>, &mut NpcStanceTrack)>,
) {
    let lines = npc_lines(&state);
    let events = StanceEvents::with_overrides(&settings.skater_stance_events);
    let skater = skater.as_deref();
    let clip_data = |part: &str| -> Option<(f32, Vec<(String, f32, f32)>)> {
        let k = skater?;
        let length = k.animation.evaluator.clip_length(part).ok()?;
        let meta = k.animation.motion.animation.metadata().clip(part).ok()?;
        Some((length, meta.attributes.iter().map(|a| (a.name.clone(), f32::from_bits(a.begin_bits), f32::from_bits(a.end_bits))).collect()))
    };
    for (npc, replay, puppet, mut track) in &mut npcs {
        let style = puppet.map_or_else(|| crate::custom_models::native_animation_style(&npc.character), |p| p.style);
        let mut layers = npc_puppet_layers(&settings, style, skater, &replay.cursor, 0.0);
        // Before the first remembered phase: the phase's own clip (as `present_pose` draws it).
        if layers.is_empty() {
            let Some(s) = replay.cursor.sample(&*lines, 0.0) else { continue };
            let clip = resolve_puppet_clip(&settings.skater_clips, s.phase, style);
            let clip = if skater.is_none_or(|k| k.animation.evaluator.clip_length(clip).is_ok()) { clip } else { puppet_clip(s.phase, style) };
            layers.push(PuppetLayer { clip: clip.to_owned(), time: s.phase_frames as f32 / 60.0, weight: 1.0, since: replay.cursor.frames.saturating_sub(s.phase_frames) });
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
            commands.entity(e).despawn();
            events.write(NpcSkaterEvent::Despawned { id: r.id, reason: r.reason });
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
                NpcReplay { cursor, branches: Vec::new(), tricks: Vec::new(), last: sample, previous: None, avoid: Default::default() },
                NpcFade { alpha: state.world.config.skaters.leave_fade.fade_in_alpha(0), ..NpcFade::default() },
                NpcSkaterAudio { list_order: u32::from(*slot), voice: npc.voice, ..Default::default() },
                NpcStanceTrack::new(npc.stance),
                npc,
            ))
            .id();
        index.0.insert(s.id, e);
        events.write(NpcSkaterEvent::Spawned { id: s.id, character: character.clone(), line: *line });
    }
}

/// Advance every cursor to its tick, take branches, end finished lines, publish position and audio.
#[allow(clippy::too_many_arguments)]
pub(crate) fn advance(
    mut commands: Commands,
    settings: Res<LivingWorldSettings>,
    observers: Res<LivingWorldObservers>,
    mut state: ResMut<PopulationState>,
    mut index: ResMut<NpcSkaterIndex>,
    mut npcs: Query<(Entity, &NpcSkater, &mut NpcReplay, &mut Transform, &mut NpcSkaterAudio, &mut NpcFade)>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    mut despawns: MessageWriter<LivingWorldDespawn>,
    mut events: MessageWriter<NpcSkaterEvent>,
) {
    let lines = npc_lines(&state);
    let tick = state.world.tick();
    let players: Vec<[f32; 3]> = observers.observers.iter().map(|o| o.position).collect();
    let mirror = settings.net_role == NetRole::Client;
    // Others' (line, node) and lines in use, by serial order (deterministic).
    let mut order: Vec<(LivingWorldId, [u8; 16], u32)> = npcs.iter().map(|(_, n, r, ..)| (n.id, r.cursor.line, r.cursor.node)).collect();
    order.sort_by_key(|x| x.0);
    let mut finished = Vec::new();
    let mut sorted: Vec<_> = npcs.iter_mut().collect();
    sorted.sort_by_key(|(_, n, ..)| n.id);
    let fade_cfg = state.world.config.skaters.leave_fade;
    let chain = state.world.config.skaters.line_chain;
    for (e, npc, mut replay, mut transform, mut audio, mut fade) in sorted {
        let elapsed = tick.saturating_sub(npc.spawn_tick) * FRAMES_PER_TICK;
        // The obstacle avoider holds the cursor back (`npc_avoid`); the fade keeps the spawn clock.
        let target = elapsed.saturating_sub(replay.avoid.held_frames());
        let mut out = Vec::new();
        // The switch blend time (branch / chain root blend) is a tuning value, the same on a client.
        replay.cursor.switch_blend_seconds = chain.blend_seconds;
        // Keep the facing across switches (fix 16 rule, mod option, retail off); a tuning value.
        replay.cursor.keep_facing = chain.keep_facing;
        // Facing rule (retail riding-entry flip by default; the fix 23 per-node fold as a mod option).
        replay.cursor.facing_rule = chain.facing_rule;
        // Retail riding-fakie thresholds (data, stock graph values by default).
        replay.cursor.fakie_settings = chain.fakie;
        // The NPC's trick tables (disc profile, mod overrides), read at its trick slots.
        let trick_profile = npc_trick_profile(&state.npc.tricks, &settings.skater_trick_profiles, &npc.character);
        while replay.cursor.frames < target && !replay.cursor.finished {
            if replay.cursor.frames + FRAMES_PER_TICK >= target {
                replay.previous = Some(replay.cursor.clone());
            }
            let s = replay.cursor.sample(&*lines, 0.0);
            let others: Vec<([u8; 16], u32)> = order.iter().filter(|o| o.0 != npc.id).map(|o| (o.1, o.2)).collect();
            let in_use: Vec<[u8; 16]> = others.iter().map(|o| o.0).collect();
            let (position, forward, speed) = s.as_ref().map_or(([0.0; 3], [0.0, 0.0, 1.0], 0.0), |s| (s.position, s.velocity, length(s.velocity)));
            let tricks = TrickContext { mode: settings.npc_tricks.mode, params: settings.npc_tricks.params, profile: trick_profile.as_ref(), seed: npc.seed };
            let ctx = BranchContext { position, forward, speed, players: &players, others: &others, in_use: &in_use, preferred_skill: -1, online: observers.online, chain, tricks };
            let records = replay.branches.clone();
            let trick_records = replay.tricks.clone();
            let mut decider = if mirror { Decider::Mirror(&records, &trick_records) } else { Decider::Decide(ctx) };
            replay.cursor.step(&*lines, &mut decider, &mut out);
            if let Some(o) = order.iter_mut().find(|o| o.0 == npc.id) {
                o.1 = replay.cursor.line;
                o.2 = replay.cursor.node;
            }
        }
        for ev in out {
            match ev {
                CursorEvent::Node { line, node, event, flags } => {
                    if event != 0 {
                        events.write(NpcSkaterEvent::Node { id: npc.id, line, node, event, flags });
                    }
                }
                CursorEvent::Branch(record) => {
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
        // Spawn fade in (retail `sub_825926F8`: 0 to 1 over the first second), then the leave fade
        // (retail `sub_8246EA90` / `sub_8245A9B8`, removal below opacity 0.2): only once the
        // cursor has ended at a dead end (no line starts within the chain radius), holding the
        // last pose while fading. The clock is the spawn-relative frame (`target`), which keeps
        // running after the cursor stops, so clients derive the same alpha from the same spawn
        // record and tick.
        fade.fade.update(&replay.cursor, &*lines, &fade_cfg);
        let alpha = fade.fade.alpha(elapsed, &fade_cfg);
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
        audio.state = Some(lite_state(&s, material));
        commands.entity(e).insert(AudioVelocity(Vec3::from_array(s.velocity)));
        replay.last = Some(s);
    }
    // Faded out (opacity below the threshold): the NPC leaves. Hosts decide this; a client waits
    // for the host's despawn record.
    if !mirror {
        for (id, e) in finished {
            if let Some(skate_core::living_world::Decision::Despawn(r)) = state.world.despawn(id, DespawnReason::External) {
                state.despawned += 1;
                despawns.write(LivingWorldDespawn(r));
            }
            if index.0.remove(&id).is_some() {
                commands.entity(e).despawn();
                events.write(NpcSkaterEvent::Despawned { id, reason: DespawnReason::External });
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

/// Retail collision groups: skater skeleton parts 5 (the skeleton contact pass scales a group-5
/// force by `SkaterSkeletonScalar` and raises the player's thresholds by
/// `SkaterSkaterThresholdScalar`, `sub_82BD4A30` at `0x82BD53D0`), board 4 (ignored by that
/// pass). The multiplayer peers' proxies use the same split (`physics/network.rs`).
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

/// NPC skaters against dynamic props (DMOs), doc 26 fix 19. Retail NPC skaters are full skaters
/// (`AIController` on the player's skater physics, [code] notes `npc-skaters-re.md` section 0),
/// so their board and body hit a DMO like the player's: the prop step pushes it by the prop's own
/// tuning (`props` domain: push mass, transfer, board / body caps). Our NPC is a replay puppet, so
/// it does not slow down or bail; the prop is pushed out of its line. A mod may switch it off
/// (`sdk.world.set_tuning("living_world", {npc_skater_props = {enabled = false}})`).
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

/// The prop-step push volumes of one NPC: the proxy's body capsule and its board (as a capsule
/// along the deck), at the recorded velocity, in the same places as [`proxy`]. Both count as a
/// board hit (the prop's `board_push_speed` cap): the lower body-bump cap is for the player's
/// walking body, which the prop's triangles stop; a puppet is not stopped, and in retail the
/// whole skater rides into the prop at riding speed. Actor id = the proxy's solid id (stable, the
/// prop's authority owner).
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
    npcs: Query<(&NpcSkater, &NpcReplay)>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    skater: Res<crate::physics::SkaterRuntime>,
    replay: Res<crate::replay::Replay>,
    settings: Res<LivingWorldSettings>,
) {
    physics.actor_prop_volumes.clear();
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
    physics.network_proxies = proxies;
    if settings.npc_skater_props.enabled {
        let volumes = list.iter().flat_map(|&(id, s)| prop_volumes(id, s)).collect();
        physics.actor_prop_volumes = volumes;
    }
}

/// The look and the puppet pose of one NPC (render side).
#[derive(Component, Default)]
pub(crate) struct NpcPuppet {
    scene: Option<Entity>,
    bindings: Option<crate::animation::AnimationStatus>,
    style: &'static str,
}

/// Load the look and bind it to the stock skeleton (like a remote player's look).
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_looks(
    mut commands: Commands,
    server: Res<AssetServer>,
    looks: Res<NpcSkaterLooks>,
    models: Res<crate::custom_models::CustomModels>,
    skater: Res<crate::physics::SkaterRuntime>,
    mut npcs: Query<(Entity, &NpcSkater, Option<&mut NpcPuppet>)>,
    skins: Query<(Entity, &bevy::mesh::skinning::SkinnedMesh)>,
    nodes: Query<(&Name, &Transform)>,
    parents: Query<&ChildOf>,
    instances: Query<&bevy::scene::SceneInstance>,
    spawner: Res<SceneSpawner>,
) {
    for (e, npc, puppet) in &mut npcs {
        let Some(mut puppet) = puppet else {
            let path = looks
                .0
                .get(&npc.character)
                .cloned()
                .or_else(|| models.online_native_path(&npc.character))
                .unwrap_or_else(|| "private/skater.glb".to_owned());
            let scene = commands.spawn((SceneRoot(server.load(GltfAssetLabel::Scene(0).from_asset(path))), Transform::default(), Visibility::Hidden, ChildOf(e))).id();
            commands.entity(e).insert(NpcPuppet { scene: Some(scene), bindings: None, style: crate::custom_models::native_animation_style(&npc.character) });
            continue;
        };
        let Some(scene) = puppet.scene else { continue };
        if puppet.bindings.is_some() || !instances.get(scene).is_ok_and(|i| spawner.instance_is_ready(**i)) {
            continue;
        }
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
    state: Res<PopulationState>,
    fixed: Res<Time<Fixed>>,
    mut npcs: Query<(Entity, &NpcSkater, &NpcReplay, Option<&NpcPuppet>, Option<&mut NpcPuppetClip>, &mut Transform, Option<&NpcStanceTrack>, Option<&super::npc_sim::NpcSim>)>,
    mut joints: Query<&mut Transform, Without<NpcReplay>>,
) {
    let lines = npc_lines(&state);
    // Render interpolation: fraction of the next world tick (FRAMES_PER_TICK recording frames)
    // already elapsed. Like the player (`physics.rs` / `animation.rs`: previous -> current by the
    // fixed-step alpha) the NPC is drawn from the cursor one tick back towards the current one, with
    // the branch records already made, so the drawn pose is a function of the cursor, the records
    // and this fraction (fix 14; before it extrapolated past the current tick with `Decider::Stay`).
    let hz = state.world.clock().hz;
    let ahead = (state.world.clock().overstep() + fixed.overstep_fraction() as f64 * fixed.timestep().as_secs_f64() * hz).clamp(0.0, 1.0) as f32
        * FRAMES_PER_TICK as f32;
    for (e, npc, replay, puppet, current, mut root, stance, sim) in &mut npcs {
        // A simulated NPC skater is drawn from its own physics pose, like the player: its
        // render pose is in world space, so the puppet root sits at the origin.
        if let Some(sim) = sim {
            *root = Transform::IDENTITY;
            if let Some(bindings) = puppet.and_then(|p| p.bindings.as_ref()) {
                let pose: Vec<Mat4> = sim.render_pose().iter().copied().map(crate::animation::native_matrix).collect();
                for (joint, local) in bindings.pose_transforms(&pose) {
                    if let Ok(mut t) = joints.get_mut(joint) {
                        *t = local;
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
        let mut layers = npc_puppet_layers(&settings, style, skater.as_deref(), &cursor, s.sub_frame);
        if layers.is_empty() {
            layers.push(PuppetLayer { clip: usable(s.phase).to_owned(), time: (s.phase_frames as f32 + s.sub_frame) / 60.0, weight: 1.0, since: cursor.frames.saturating_sub(s.phase_frames) });
        }
        // The stance bits each layer was built with (natural stance, toggled by trick clips).
        let natural = StanceFlags::natural(npc.stance);
        let layer_flags: Vec<StanceFlags> = layers.iter().map(|l| stance.map_or(natural, |t| t.flags_for(l.since))).collect();
        let top_flags = *layer_flags.last().expect("one layer");
        root.rotation = root_rotation(&s) * flags_root_turn(top_flags);
        let top = layers.last().cloned().expect("one layer");
        let clip = top.clip.as_str();
        let time = top.time;
        let blend = (layers.len() >= 2).then(|| {
            let from = &layers[layers.len() - 2];
            PuppetBlend { from: from.clip.clone(), from_time: from.time, weight: top.weight }
        });
        let fakie = fakie_channel_layer(&cursor, frac, resolve_fakie_channel(&settings.skater_clips));
        let bound = skater.as_deref().zip(puppet.and_then(|p| p.bindings.as_ref()));
        let posed = bound.and_then(|(skater, bindings)| {
            // A channel tree that does not build (a mod's bad name) is left out.
            let channel = fakie.as_ref().and_then(|f| {
                crate::graph_host::motion::tree_commands(&skater.animation.motion.animation, &f.clip, &fakie_channel_attributes(), f.time).ok().map(|c| (c, f.weight))
            });
            let globals = puppet_pose_in_flags(&skater.animation.evaluator, &layers, &layer_flags, channel.as_ref().map(|(c, w)| (c.as_slice(), *w)))?;
            for (joint, local) in bindings.pose_transforms(&globals) {
                if let Ok(mut t) = joints.get_mut(joint) {
                    *t = local;
                }
            }
            Some(())
        });
        let next = NpcPuppetClip { phase: s.phase, clip: clip.to_owned(), time, blend, layers, posed: posed.is_some(), fakie };
        match current {
            Some(mut c) => {
                if *c != next {
                    *c = next;
                }
            }
            None => {
                commands.entity(e).insert(next);
            }
        }
    }
}

/// Blended material copies of one NPC while it fades (spawn fade in, leave fade). Absent while the
/// NPC is solid. Dropping it (despawn, mod disable, map change) frees the copies: they are only
/// held here.
#[derive(Component, Default)]
pub(crate) struct NpcFadeMaterials {
    /// Mesh entity and its own (shared) material, put back at alpha 1.
    pub originals: Vec<(Entity, Handle<StandardMaterial>)>,
    /// Source material -> its blended copy and the source's own base alpha.
    pub copies: BTreeMap<AssetId<StandardMaterial>, (Handle<StandardMaterial>, f32)>,
    /// Alpha the copies were last set to (NaN = not yet).
    pub alpha: f32,
}

/// Draw [`NpcFade::alpha`]: below 1, swap every mesh under the NPC (body, board, anything a mod
/// parents to it) to a blended copy of its material at the source alpha times the fade; at 1,
/// restore the shared materials and drop the copies. Meshes that load mid fade are picked up on
/// the next frame. Generic: works on any entity carrying [`NpcFade`] (NPC skaters; peds may use it
/// for their camera-distance fade). Render only: the alpha itself is simulation state.
pub(crate) fn present_fade(
    mut commands: Commands,
    mut npcs: Query<(Entity, &NpcFade, Option<&mut NpcFadeMaterials>)>,
    children: Query<&Children>,
    mut meshes: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, fade, state) in &mut npcs {
        let alpha = fade.alpha.clamp(0.0, 1.0);
        if alpha >= 1.0 {
            if let Some(state) = state {
                for (mesh, original) in &state.originals {
                    if let Ok(mut m) = meshes.get_mut(*mesh) {
                        m.0 = original.clone();
                    }
                }
                commands.entity(e).remove::<NpcFadeMaterials>();
            }
            continue;
        }
        let mut fresh = None;
        let state: &mut NpcFadeMaterials = match state {
            Some(s) => s.into_inner(),
            None => fresh.insert(NpcFadeMaterials { alpha: f32::NAN, ..Default::default() }),
        };
        let mut added = false;
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
                    let mut blended = source.clone();
                    let base = source.base_color.alpha();
                    blended.alpha_mode = AlphaMode::Blend;
                    blended.base_color.set_alpha(base * alpha);
                    let h = materials.add(blended);
                    state.copies.insert(current, (h.clone(), base));
                    added = true;
                    h
                }
            };
            state.originals.retain(|(x, _)| *x != mesh);
            state.originals.push((mesh, m.0.clone()));
            m.0 = copy;
        }
        if added || state.alpha != alpha {
            for (h, base) in state.copies.values() {
                if let Some(mat) = materials.get_mut(h.id()) {
                    mat.base_color.set_alpha(*base * alpha);
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
        layers.push(PuppetLayer { clip: b.from.clone(), time: b.from_time, weight: 1.0, since: 0 });
    }
    layers.push(PuppetLayer { clip: clip.to_owned(), time, weight: blend.map_or(1.0, |b| b.weight), since: 0 });
    puppet_layers_pose(evaluator, &layers)
}

/// [`puppet_blend_pose`] for nested layers (oldest first): the base clip, then each newer clip
/// blended over the result by its weight (`PoseCommand::Blend`, the player's SQT blend), like a
/// graph transition whose outgoing tree is the running transition. A layer whose clip does not
/// evaluate is skipped (an outgoing one) or fails the pose (the newest one).
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

/// The puppet root's extra turn for the stance bits of the newest layer. The regular bind pose
/// ([`bind_pose_tail`]) draws the board turned round (`BOARD_BACKWARDS`, nose along root -Z) and
/// the body mirrored; retail's physical board follows the animated board (`publish_deck_angles`
/// with `board_flipped = bit31 ^ mirror`, false for both natural stances), so a regular
/// character's frame is half a turn from its board's. The replay root is the frame the goofy
/// puppet rides nose first with (the facing rule and the fakie rule work on that board axis), so
/// a mirrored puppet turns its root half a turn: the board keeps its world heading and the left
/// foot leads. The turn follows the mirror bit alone: a trick clip that toggles only
/// `animboardbackward` (the board turned under the body) ends with the board turned in the clip,
/// and the next layer's `BOARD_BACKWARDS` keeps it turned without moving the body.
pub(crate) fn flags_root_turn(flags: StanceFlags) -> Quat {
    if flags.mirrored { Quat::from_rotation_y(std::f32::consts::PI) } else { Quat::IDENTITY }
}

/// [`stance_tail`] for a natural stance's start bits (`mirrored` = regular).
pub(crate) fn bind_pose_tail(mirrored: bool) -> Vec<skate_core::animation::playback_tree::PoseCommand> {
    stance_tail(StanceFlags { board_backward: mirrored, mirrored, switch: false })
}

/// The commands retail's bind pose appends to every tree it plays (`MotionAnimation::add_bind_pose`,
/// the player's path, from the SkaterAnim flags at the tree's construction): the reference pose
/// added, `BOARD_BACKWARDS` / `BOARD_BACKWARDS_IK` for bit 31, a mode 2 mirror for bit 30. The
/// switch bit adds nothing.
pub(crate) fn stance_tail(flags: StanceFlags) -> Vec<skate_core::animation::playback_tree::PoseCommand> {
    use skate_core::animation::playback_tree::PoseCommand;
    let mut out = vec![PoseCommand::Pose { name: "RIG_TPOSE".into() }, PoseCommand::Add { motion_is_a: true }];
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
    out
}

/// [`puppet_pose_with_channel`] in a natural stance (`mirrored` = regular): every layer with the
/// stance's start bits ([`puppet_pose_in_flags`]).
pub(crate) fn puppet_pose_in_stance(evaluator: &crate::animation_pose::PoseEvaluator, layers: &[PuppetLayer], channel: Option<(&[skate_core::animation::playback_tree::PoseCommand], f32)>, mirrored: bool) -> Option<Vec<Mat4>> {
    let flags = StanceFlags { board_backward: mirrored, mirrored, switch: false };
    puppet_pose_in_flags(evaluator, layers, &vec![flags; layers.len()], channel)
}

/// The puppet pose with each layer in the stance bits it was built with (`flags[i]` for
/// `layers[i]`; retail bakes the bits into a tree's bind pose at construction, so a transition
/// blends two trees each closed with its own tail). While every layer shares the bits, the
/// layers blend first and one tail closes the result (with no bits set: the commands every NPC
/// drew before the stance port; with the regular bits: the natural stance port); once a newer
/// layer's bits differ, every layer is closed with its own tail before it blends in. The channel
/// tree (fakie) is closed with the newest layer's bits, like the player's channel tree built with
/// the current flags.
pub(crate) fn puppet_pose_in_flags(evaluator: &crate::animation_pose::PoseEvaluator, layers: &[PuppetLayer], flags: &[StanceFlags], channel: Option<(&[skate_core::animation::playback_tree::PoseCommand], f32)>) -> Option<Vec<Mat4>> {
    use skate_core::animation::playback_tree::PoseCommand;
    let sample = |clip: &str, time: f32| -> Option<PoseCommand> {
        let (clip, time) = sequence_part(evaluator, clip, time)?;
        let time = puppet_clip_time(clip, time, evaluator.clip_length(clip).ok()?);
        Some(PoseCommand::Clip { name: clip.to_owned(), previous_time: time, time, loops: 0 })
    };
    let flag = |i: usize| flags.get(i).copied().unwrap_or_default();
    let (top, older) = layers.split_last()?;
    let top_flags = flag(layers.len() - 1);
    let mixed = (0..layers.len()).any(|i| flag(i) != top_flags);
    let mut commands = Vec::with_capacity(layers.len() * 8 + 8);
    // Per-layer tails once the bits differ (each layer closed before it blends in).
    let closed = |commands: &mut Vec<PoseCommand>, i: usize| {
        if mixed {
            commands.extend(stance_tail(flag(i)));
        }
    };
    for (i, l) in older.iter().enumerate() {
        if let Some(c) = sample(&l.clip, l.time) {
            let base = commands.is_empty();
            commands.push(c);
            closed(&mut commands, i);
            if !base {
                commands.push(PoseCommand::Blend { weight: l.weight });
            }
        }
    }
    let to = sample(&top.clip, top.time)?;
    let base = commands.is_empty();
    commands.push(to);
    closed(&mut commands, layers.len() - 1);
    if !base {
        commands.push(PoseCommand::Blend { weight: top.weight });
    }
    // Shared bits: one tail for the blended layers. A tail that mirrors (or turns the board)
    // closes before the channel tree, which gets its own (as the player's channel tree).
    let shaped = top_flags.board_backward || top_flags.mirrored;
    if !mixed && shaped {
        commands.extend(stance_tail(top_flags));
    }
    if let Some((tree, weight)) = channel.filter(|c| !c.0.is_empty() && c.1 > 0.0) {
        commands.extend_from_slice(tree);
        if mixed || shaped {
            commands.extend(stance_tail(top_flags));
        }
        commands.push(PoseCommand::ChannelBlend { weight, use_channels_from_weights: false });
    }
    if !mixed && !shaped {
        commands.extend(stance_tail(top_flags));
    }
    let pose = evaluator.evaluate(&commands).ok()?;
    let locals: Vec<Mat4> = pose.iter().copied().map(skate_core::animation::output::sqt_to_matrix).map(crate::animation::native_matrix).collect();
    let parents = &evaluator.frames.parents;
    let mut globals: Vec<Mat4> = Vec::with_capacity(locals.len());
    for (i, local) in locals.iter().enumerate() {
        let g = match parents.get(i).copied() {
            Some(p) if p >= 0 && (p as usize) < i => globals[p as usize] * *local,
            _ => *local,
        };
        globals.push(g);
    }
    Some(globals)
}

/// The part of a `+`-joined clip sequence playing `time` s in and the time inside it: parts play
/// back to back, the last one holds (or loops when it is a `_CYC` clip). A plain clip is itself.
pub(crate) fn sequence_part<'a>(evaluator: &crate::animation_pose::PoseEvaluator, clip: &'a str, time: f32) -> Option<(&'a str, f32)> {
    let mut parts = clip.split('+').peekable();
    let mut t = time.max(0.0);
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return Some((part, t));
        }
        let len = evaluator.clip_length(part).ok()?;
        if t < len {
            return Some((part, t));
        }
        t -= len;
    }
    None
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

/// The `NPC_SKATER_BACKWARDS` line for one NPC sample, `None` when it is not riding backwards
/// (`skate_core::living_world::replay::facing_check`: drawn heading more than 135 deg from the
/// velocity yaw at 1 m/s or more; a diagnostic threshold). `recorded_fakie` says the line's own
/// retail path frame opposes travel there (the recorder rode fakie: retail's target frame does
/// too); `flip` is retail's latched switch / fakie flip (controller `+927`); `facing_flipped` is
/// the fix 16 mod option's turn.
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
pub(crate) fn log_backwards(state: Res<PopulationState>, npcs: Query<(&NpcSkater, &NpcReplay)>, mut last: Local<BTreeMap<LivingWorldId, u64>>) {
    let tick = state.world.tick();
    let lines = npc_lines(&state);
    last.retain(|id, _| npcs.iter().any(|(n, _)| n.id == *id));
    for (npc, replay) in &npcs {
        let (Some(s), Some(line)) = (replay.last.as_ref(), lines.get(&replay.cursor.line)) else { continue };
        if s.line != replay.cursor.line {
            continue;
        }
        let Some(text) = backwards_line(npc.id, &npc.character, line, &replay.cursor, s) else { continue };
        let due = last.get(&npc.id).is_none_or(|t| tick < *t || tick >= t + BACKWARDS_LOG_TICKS);
        if due {
            last.insert(npc.id, tick);
            warn!("{text}");
        }
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
        .init_resource::<NpcSkaterLooks>()
        .init_resource::<super::npc_avoid::AvoidTrack>()
        .add_message::<NpcSkaterEvent>()
        .add_systems(FixedUpdate, (apply_records, super::npc_avoid::avoid, advance, super::npc_sim::simulate, track_stance, log_backwards, log_readout).chain().after(super::step_population))
        .add_systems(
            FixedUpdate,
            push_proxies.after(crate::multiplayer::prepare).after(crate::app::SimulationSet::Controls).before(crate::app::SimulationSet::Physics),
        )
        .add_systems(Update, (present_looks, present_pose, present_fade).chain().after(crate::app::FrameSet::Animation));
}
