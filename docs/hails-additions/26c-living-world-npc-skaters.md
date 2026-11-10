# 26c: Living world: NPC skaters: replay tier and animation

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## Change (milestone 3: NPC skaters, replay tier)

Ambient NPC skaters are visible in free roam: the population's skater spawn records become skaters that ride the
retail recorded lines kinematically (the "replay tier"; the simulated tier with the full skater physics follows).

What retail does, checked in the code for this milestone (TU3, title update 3):
- An NPC is a full skater steered along its line by the `PathController` (ctor `sub_824685F0`). At a node that
  carries a branch group the controller picks where to continue (`sub_8246BEE0`): stay on the line, or a branch
  target that exists, is not in use and is valid. The lowest score of `sub_8246C230` wins, the first on ties; there
  is no random draw and the branch record's f32 is not read [code]. A candidate is rejected when the line has no
  node after it, when the next node lies 50 deg or more off the skater's forward (`0x822F91B0`), or when an airborne
  or event node lies within one node (`sub_8246C4F8`). Score = angle x 572.958 + (offline) 1024 per other AI skater
  on the same line within 5 nodes (`sub_82456A38`) + min(30 x the player's distance to the line, 1500)
  (`sub_8246C5D8`) + 1000 for lines with flag bits 0..2 all set + a skill term. A taken branch starts at the target
  node nearest the skater among the 3 before it (`sub_82455BB0`) [code].
- Line nodes carry the skater and board orientation as quaternions x, y, z, w with +Z forward [data: 81 % of
  moving nodes]; branch groups never sit on a line's last node [data].

Change:
- `skate-core::living_world::replay`: the line cursor (node index + frame in the segment at the 60 Hz recording
  rate; any engine rate gives the same cursor), pose / velocity / heading / orientations / node flags and events,
  the phase (rolling, crouched, air, air trick, ground trick, off board), the retail branch choice, and a mirror mode
  that replays recorded branch decisions. `skate-data::living_world::replay_line` converts a decoded line.
- `skate-game::living_world::npc_skaters`: one entity per skater spawn record (`NpcSkater` with the stable
  `LivingWorldId`, character, slot, seed, voice; `NpcReplay` with the cursor). The cursor stays at
  2 x (population tick - spawn tick) recording frames, so an NPC's state follows from its spawn record, the tick and
  its branch records. Its position goes back into the population (culls, the 5 m rule). Collision: a kinematic
  capsule and board box join the skater solve through the network proxies (like a mod's solid), so the player bumps
  into NPCs. Look: the character's native roster GLB (the customiser's and online players' files), a mod override
  (`NpcSkaterLooks`), else the stock skater; bound to the stock skeleton like a remote player. Puppet animation: one
  stock clip per phase from the player's evaluator; the root takes the recorded position and skater orientation.
  The clip time is the time since the phase began; a looping clip (`_CYC` in its name) wraps by its length,
  `(frames - 1) / fps` like the player's `ClipClock`, others hold their last frame. Fix (2026-10-05): the time used
  to run past the clip end unwrapped and the sampler clamps there, so after about one clip length every NPC froze on
  the clip's last frame ("Skate npc animations are not playing"). Each NPC carries `NpcPuppetClip` (phase, clip,
  time, posed). Mod entry: `sdk.world.set_tuning('living_world', {skater_clips = {rolling = '...',
  ['rolling.Aggressive'] = '...'}})` keyed by the stable phase ids (`ReplayPhase::name`); an override that does not
  evaluate falls back to the shipped pick; cleared on mod disable (`reset_mod_overrides`). Tests:
  `living_world_npc_puppet_clip_attached_and_playing_per_phase` (headless), `..._looping_clips_wrap_and_others_hold`,
  data-gated `..._puppet_clips_play_past_their_first_loop`, `npc_skater_clips_set_merge_and_reset`.
  Audio: `NpcSkaterAudio` with a lite state (speed, air, ground trick as a grind) and the character's voice, so
  #32's NPC board sounds and speech play. Events: `NpcSkaterEvent` (spawned, despawned, node event, branch, line
  end). Debug: `SKATE_LIVING_WORLD_DEBUG=1` logs the NPC count and the nearest NPC (distance, line id, node, phase,
  speed) every 5 s.

Replay-tier simplifications until the simulated tier (documented, not retail): positions and timing come from the
recording, not from steering (`AIPhysicsInput`); the branch is evaluated once when the node is reached; the
obstacle-list rejection is not modelled; NPCs do not react to the player (the proxy is infinite mass, no bails);
tricks show one clip per phase (a grind, slide or manual is one 50-50 clip; the recording does not say which), the
stock graphs do not run; the end of a line with no branch taken despawns the NPC (retail behaviour not decoded).

Files: `crates/skate-core/src/living_world/{replay,replay_tests,mod,clock,population}.rs`,
`crates/skate-data/src/living_world.rs`, `crates/skate-data/tests/living_world_data.rs`,
`crates/skate-game/src/living_world/{npc_skaters,npc_tests,mod,tests}.rs`.

Verification (milestone 3):
- `cargo test -p skate-core --release --locked living_world::replay`: 7 tests (60 Hz timing and interpolation,
  frame-rate independence at 30 / 60 / 64 / 144 / 240 Hz, phases from flags and trick events, line end, every term
  of the branch score against the code constants, branch taken / stay / in-use target, client mirror equals host,
  orientation order).
- `SKATE3_ASSET_ROOT=<export> cargo test -p skate-data --release --locked --test living_world_data`: every one of
  the 1,691 lines rides end to end with the retail branch choice (1,331 branches taken, average ride 14.6 s; recomp
  NPC median life 13 to 28 s), every branch target resolves, no frame moves more than 3 m, deterministic.
- `cargo test -p skate-game --release --locked --bin skate3rust -- living_world`: 9 tests; new: NPCs ride their
  lines and each position equals a cursor rebuilt from the spawn record and the tick, 3 NPCs max, line ends despawn,
  audio velocity and voice published, same result at 60 and 144 Hz, NPC entities go with the population, proxy /
  audio state / clip table; with `SKATE3_ASSET_ROOT` every puppet clip evaluates on the stock banks.

Multiplayer: an NPC is reproducible from its spawn record, the console tick and its branch records
(`Decider::Mirror`); a client never decides (`NetRole::Client` mirrors). No transport.

Moddability: lines are keyed by retail id in a shared map a content overlay can extend or patch; looks by character
key (`NpcSkaterLooks`); NPC events are messages; spawn / despawn go through the population (stable ids). The
`sdk.living_world` NPC surface is designed below (open items) and comes with the mod milestone.

## NPC skaters fading out near the player (line end), 2026-10-05

**Problem.** The user reported that NPC skaters still faded out far too soon and too close to the player, suspected the
distance was measured from the spawn point rather than from the player, and said they should keep skating
around.

**Cause.** The replay tier faded every skater 1 s before the end of its recorded line when no branch group was
ahead, wherever that was. [data] No line on the disc has a branch group on its last node (760 of 760 DownTown lines,
all 1,691), 426 DownTown lines have none at all, and the median line lasts 9 s (DownTown p10/50/90 4.3 / 9.0 /
18.7 s), so most skaters vanished after a few seconds, often close to the player. The fade itself is retail, but it
belongs to a timed controller state entered on a skater component flag (`sub_8246EF78`, byte `+59`; leaving it
cancels the fade), not to the line end.

**Evidence (retail code, TU3 recompilation).**
- [code] `sub_8246D3C0` (PathController node update): when the node index reaches the last node it calls
  `sub_8246C7F8` with the end position; otherwise a changed node goes to the branch choice `sub_8246BEE0`.
- [code] `sub_8246C7F8`: `sub_82458968(path manager, position, ..., 16 entries, mode 1)` lists lines; one result
  is taken as it is, several go through the branch chooser `sub_8246C1C8` (score `sub_8246C230`, lowest wins,
  index 0 when all are rejected); the controller takes the line id, node and path (`+592`, `+816`, `+800`).
- [code] `sub_82458968` mode 1: walks every loaded line (path hash map), skips lines in use (`sub_82458860`),
  takes the **start node** (node 0) and keeps lines whose start is within the radius; with none, it falls back to
  the nearest valid start at any distance (a nearer invalid one wins only beyond 6 m, `0x822F94F4` = 36).
- [data] Radius: `ai_skater` `default` field `Hash_4F87E7A70DA11691` = 4.0 m (read by `sub_8246C7F8`).
- [code] Ambient skaters otherwise leave by the population cull `sub_8245D520` (3-D distance to the player > 120 m,
  height > 1000 m) and the per-skater check `sub_8245A9B8` (fading state below opacity 0.2; character wanted
  elsewhere while over budget). [recomp] measured lives 13-28 s median, despawn distance median 115-123 m.
- [data] 742 of 760 DownTown lines (406 / 423 Industrial, 484 / 508 University) have another line's start within
  4 m of their end; median end-to-start distance 0.1 m (tool `tools/claude-skills/living-world/tools/line_ends.py`).

**Change.** At the last node the cursor continues on the next line exactly like that: unused lines whose start
lies within `ChainConfig::radius` (retail 4 m) of the end, at most 16 in id order (deterministic; retail walks a
hash map), one taken directly, several scored by the branch chooser, start at node 0. The decision is a
`BranchRecord` from the last node, so clients mirror it. The leave fade now starts only at a dead end (no start
within the radius), which the replay tier cannot ride past (retail's full skater would steer to the far start);
the fade in, the 1 s fade and the 0.2 removal are unchanged. So skaters keep riding until the 120 m cull.
Moddable: `SkaterConfig::line_chain`, `LivingWorldSettings::skater_line_chain`,
`sdk.world.set_tuning('living_world', {skater_line_chain = {radius, max_candidates}})` (radius 0 = fade at every
line end), reset on mod disable. The debug readout of the NPC skaters now uses `report_due` (it went silent after
a map reload).

**Files.** `crates/skate-core/src/living_world/replay.rs` (`ChainConfig`, `choose_next_line`,
`LineSource::for_each_line`, `LineCursor::chain`), `leave_fade.rs` (fade only at a dead end), `config.rs`
(`line_chain`), `replay_tests.rs`; `crates/skate-game/src/living_world/npc_skaters.rs`, `mod.rs` (settings),
`npc_tests.rs`; `crates/skate-game/src/modding/world_tuning.rs`; `crates/skate-mods/src/world_tuning.rs`,
`api.lua`; `sdk/skate.lua`; `crates/skate-data/tests/living_world_data.rs`.

**Verification.**
- skate-core: `npc_skater_chains_to_a_line_starting_within_4_m_like_sub_8246c7f8` (chains on the frame the end is
  reached, onto node 0, mirrored by a client; in-use and out-of-radius lines skipped; radius 8 / 0 from data),
  `line_end_choice_scores_like_the_branch_chooser_and_falls_back_to_the_first`,
  `npc_skater_keeps_riding_a_seeded_line_network_for_minutes` (seeds 1, 7, 1234: 5 minutes, never ends, two runs
  identical), `npc_fades_out_only_at_a_dead_end_and_goes_below_alpha_0_2`.
- skate-game: `living_world_npc_skaters_chain_lines_and_fade_only_at_a_dead_end` (no fade while a next line
  exists, chains at the line end, leaves by the distance cull; radius 0 gives the retail fade at every end),
  `..._ride_their_lines_from_the_spawn_record` (cursor rebuilt from the host's records), tuning test extended.
- skate-data (data-gated, exported lines): 1,691 rides, 19,948 branches and chains, 440 dead ends within 2 minutes
  (before: more than 1,600 ended within their first line).

**Line validity per character (2026-10-29, research b159 / b675).** Retail's `sub_82456970` (the spawn
`sub_8245C018` / `sub_8245B068`, branches and the line-end search) wants the line's allowed-skater bit for the pro
index AND a capability bit: the runtime AI profile's bytes +0x31 / +0x30 / +0x32 (`sub_8245FFD8`: `ai_skater_profiles`
`Hash_E01079D1A094B0C0` / `Hash_AACEBDA7CA734319` / `Hash_C36E34B4C619E868`, all true via `default`) against line flag
bits 0 / 1 / 2. Our reader hard-coded false, and `fits` used OR; now `skater_characters` reads the three fields and
`fits` is the AND rule, used by the spawn and (cached per character, `BranchContext::valid_lines`) by branches and
chains. With stock data every line has a flag bit, so the rule is the pro bit (DownTown: 659..760 of 760 lines per
character). Open: the controller exclusion (ctrl+0xA0). Retail's fallback to a
far start (the full skater steers there) is not possible in the replay tier: 440 of 1,691 solo rides still meet a
dead end within 2 minutes and fade there. What the timed state on byte `+59` is (likely a bail) is unconfirmed.
Not seen in game yet.

## NPC skaters snapping between animations, 2026-10-05

**Problem.** The user reported that NPC skaters looked better but still snapped between animations, with no
interpolation between them.

**Cause.** The replay tier shows one stock clip per replay phase and swapped clips on the frame the phase changed
(rolling to air, air to rolling, ground trick, off board, also when a branch or line chain changes the phase). The
cursor only knew the current phase, so there was nothing to blend from.

**Evidence (retail).** Ambient skaters are full skaters on the same motion graph as the player (design doc 26), so
a clip change is the graph's transition: [code] Blend82B96058 (`playback_transition`, already ported for the
player) blends the outgoing tree into the incoming one with weight `elapsed / time` clamped to 0..1 and eased
`3w^2 - 2w^3` when `time` is 0.05 s (`0x3d4ccccd`) or more, linear below; the outgoing tree keeps playing
(matching mode 0). [data] `time` comes from each `PlayAnimation`; without one it is the literal at `0x82099280`,
0.2 s. The looping idle states the puppet clips stand for enter with 0.2 s (`air.xml` `B_AIR_CYC`,
`offboard.xml` stand `BLENDSEC`, `T_handplantair.xml` `IA_IDLE_N_N_0_CYC`). Across the stock motion graph the
times are 0.1 (119), 0.2 (89 plus 80 without a time), 0.05 (39), 0.3 (26), 0.15 (10, landings), up to 0.85 s.
No separate animation path for far AI skaters was found (no LOD animation strings besides `PhysicalPlayerHiLOD`).

**Change.**
- `skate-core::animation::playback_transition::transition_weight(elapsed, seconds)`: the player's transition
  weight, moved out of `PlaybackTransition::weight` unchanged so the NPC puppet uses the same code.
- `skate-core::living_world::replay`: the cursor keeps the phase it left and when it began; `ReplaySample` carries
  `previous_phase` and `previous_phase_frames`. Both follow from the cursor alone (a client mirrors them).
- `npc_skaters::present_pose`: while `transition_weight(phase_frames / 60, seconds) < 1` the pose is
  `[outgoing clip at its own time, incoming clip, Blend {weight}, RIG_TPOSE, Add]` (the player's SQT blend);
  same clip on both sides = no blend. A mod clip that does not evaluate falls back to the shipped pick on both
  sides. `NpcPuppetClip::blend` shows the crossfade in effect.
- Moddable: `LivingWorldSettings::skater_blend_seconds` (phase id or `default` -> s, empty = 0.2 s), Lua
  `sdk.world.set_tuning('living_world', {skater_blend_seconds = {default = 0.3, air = 0.1}})`, validated
  (known phase or `default`, 0..10 s), first writer wins per key, readable via `world_tuning`, cleared by
  `reset_mod_overrides()` on mod disable. Mod clips blend the same way.

**Files.** `crates/skate-core/src/animation/playback_transition.rs`, `crates/skate-core/src/living_world/replay.rs`,
`replay_tests.rs`, `crates/skate-game/src/living_world/npc_skaters.rs`, `npc_tests.rs`, `mod.rs`,
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/world_tuning.rs`, `api.lua`, `sdk/skate.lua`.

**Verification.** skate-core `living_world`: 105 passed (new: the cursor keeps the previous phase; the weight
curve is 0 / 0.15625 / 0.5 / 0.84375 / 1 at 0 / 0.05 / 0.1 / 0.15 / 0.2 s, linear below 0.05 s); skate-core
`animation::` 44 passed (player transitions unchanged). skate-game: `..._puppet_blend_follows_the_transition_curve`,
`npc_skater_blend_seconds_set_merge_and_reset`, and data-gated `..._puppet_clip_change_has_no_pose_jump` (every
pair of puppet clips: the biggest per-frame joint move across the change stays within the clips' own motion plus
one curve step of the gap; worst old snap 0.94, worst blended frame step 0.13). Not seen in game yet.

**Open.** Per-transition times: the replay tier does not run the graph, so every phase change uses the stock
default 0.2 s (retail landings use 0.15 s, grind entries 0.3 s, many `INTO` clips 0.1 s; the simulated tier gets
them from the graph). A phase change during a running blend starts a new blend from the previous phase's clip
alone (retail nests the running transition). Jitter between 60 Hz ticks is a separate cause (fix 12 report).

## NPC skaters jittery while skating, 2026-10-05

**Problem.** User: "The skater npcs look decent but a little jittery when they skate."

**Cause.** Three presentation faults of the replay tier, none in the line data:
1. The root was placed between ticks, but the clip time and the crossfade weight advanced in whole 60 Hz frames
   (`phase_frames / 60`), so at 144 Hz the body pose repeated frames under a smoothly moving root.
2. The render extrapolated past the current tick with a look-ahead cursor that never branches
   (`Decider::Stay`), while the player is drawn from the previous to the current fixed state. At a branch or chain
   the look-ahead rode the old line, then the next tick corrected it.
3. A branch or chain moved the line under the skater at once: on the exported lines (212 seeded rides, 881
   switches) the gap is p50 0.46 m, p90 2.8 m, p99 6.7 m, max 16 m (branch targets far from the branch node).

**Evidence (retail).** Retail has no snap and no blend time here: [code] at a line end `sub_8246D3C0` ->
`sub_8246C7F8` only stores the new line id and node (`+592/+600`, `+816`, fix 9), and the branch choice
`sub_8246BEE0` likewise only picks a line and node; the full skater keeps its physical position and its
`AIPhysicsInput` steers onto the new line (not traced in the recomp for this fix). The orientation inside a segment is
already nlerped between the two nodes' decoded quaternions (continuous at nodes), unchanged.

**Change.**
- `skate-core::living_world::replay`: `LineSwitch` (frame, drawn offset, drawn skater and board orientation at
  the switch, including a blend still running); the drawn root decays from it onto the new line with the graph
  transition curve (`transition_weight`) over `ChainConfig::blend_seconds` (default `SWITCH_BLEND_SECONDS` = the
  stock graph's default transition time 0.2 s, an engine stand-in for retail's steering; 0 = cut).
  `ReplaySample::sub_frame` (render fraction). `LineCursor::render_sample(lines, records, frames_ahead)`: steps
  whole frames with the recorded branch decisions (`Decider::Mirror`), never guessing, then samples the fraction.
  All of it follows from the cursor, the branch records and the fraction (a client draws the same pose).
- `npc_skaters`: `NpcReplay::previous` (cursor one world tick back, taken before the tick's last step);
  `present_pose` draws `previous.render_sample(branches, fraction)` like the player's previous-to-current
  interpolation; clip time and crossfade use `(frames + sub_frame) / 60` (`puppet_blend_at`). The cursor's
  `switch_blend_seconds` is set from the tuning each step.
- Moddable: `sdk.world.set_tuning('living_world', {skater_line_chain = {blend_seconds = 0.5}})` (0..10 s,
  validated, first writer wins, read back, reset on mod disable with the rest of `skater_line_chain`).

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`,
`crates/skate-game/src/living_world/npc_skaters.rs`, `npc_tests.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-data/tests/living_world_data.rs`, `sdk/skate.lua`.

**Verification.** skate-core `living_world` 107 passed (new: `npc_skater_render_is_smooth_across_branches_and_chains`
renders a ride with a 0.8 m / 30 deg branch and a 1.5 m chain at 144 Hz: every root step stays below the line speed
plus one smoothstep step of the gap, the turn likewise, clip time grows by exactly 1/144 s, the cut ride jumps,
identical when mirrored; `npc_skater_render_is_smooth_on_a_seeded_line_network`, seeds 1 / 7 / 1234). skate-game
`living_world world_tuning` 50 passed (new: `living_world_npc_skaters_render_smoothly_between_ticks_and_across_chains`,
40 s at 144 Hz, worst root step below 0.075 m, clip time on the render clock; tuning test covers `blend_seconds`).
skate-mods `world_tuning` 3 passed. Data-gated `npc_skater_render_is_smooth_on_the_exported_lines` (pm3 export):
worst render step while a switch blends 0.75 m (the 16 m gap), 15.9 m cut; elsewhere 0.36 m. Not seen in game yet.

**Open.** Large branch gaps (p99 6.7 m) still slide fast over 0.2 s; retail would steer (and hand over to the nav
mesh beyond its 4.5 / 6 m corridor). A gap-scaled blend time or the simulated tier would cover it; mods can raise
`blend_seconds` now. The 8-bit node quaternions (about 0.45 deg steps) are interpolated as recorded, not smoothed.

## NPC skaters switching the side they stand on, 2026-10-05

**Problem.** The user found NPC skaters much better and smoother, but they still switched the side they stand on
their boards at random. In the user's capture an NPC rolling on flat ground
turns its whole body round in under 0.1 s while the board keeps rolling the same way.

**Root cause.** The replay tier draws the recorded skater orientation as the puppet root. The lines are recorded
human runs, and some recorders rode fakie at places (the skater frame faces against the travel direction): DownTown
34 of 760 lines start fakie and 40 end fakie, and branch targets also land inside fakie stretches. At a branch or a
chain the NPC took the new line's recorded facing, so the 0.2 s switch blend turned it 180 deg. On the exported
lines (212 seeded 40 s rides) 130 of 882 switches spun the skater round, about one per ride, which reads as random.

**Evidence (retail).** [code] A switch stores only the new line and node (`sub_8246D3C0` -> `sub_8246C7F8`
`+816`, `sub_8246BEE0`; fix 9 and 14); the full skater keeps its own body, so its facing and stance carry onto the
new line and only a performed trick or revert turns it. [code] The path frame retail builds from a node is the board
orientation (node `+0x18`, `sub_82453970`) turned 180 deg about its up axis when the node's `m_IsBoardFlipped` bit
(flags `+0x28` bit 0) is set: `sub_82453A58` and the interpolating `sub_824734A8` (called from the controller update
`sub_8246D560` into controller `+272`) negate the frame's X and Z rows. [data] With that turn the board frame matches
the recorded skater frame (flag clear: 20,195 of 21,191 moving nodes within 22 deg; flag set: 11,679 of 12,653 at
180 deg). Stance (regular or goofy) is not stored per node.

**Change.** `skate-core::living_world::replay`: `LineCursor::facing_flipped`; at a branch or chain, when the new
line's recorded skater faces more than 90 deg away (about +Y) from the drawn skater, the cursor toggles it and rides
the line turned 180 deg about the skater's own up axis (`turn_about_up`, `q * (0, 1, 0, 0)`, retail's board-flip
turn), applied to the skater and the board orientation in `sample` and in the switch blend. Recorded turns on a line
(180s, reverts) still play, from the facing the skater has. A pure function of the lines and the branch records (a
client mirroring the records derives the same flip). Moddable: `ChainConfig::keep_facing` (retail `true`),
`sdk.world.set_tuning('living_world', {skater_line_chain = {keep_facing = false}})` takes the recorded facing
(validated, first writer wins, read back, reset on mod disable); the cursor copies it each step like the blend time.

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`,
`crates/skate-game/src/living_world/npc_skaters.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-data/tests/living_world_data.rs`, `sdk/skate.lua`.

**Verification.** skate-core `living_world` 108 passed (new `npc_skater_keeps_its_facing_across_a_line_switch`: a
chain onto a line recorded fakie keeps facing +Z on every frame, the line's recorded revert still turns it,
mirrored client identical, `keep_facing = false` spins as before). skate-game `living_world world_tuning` 50
passed (tuning test covers `keep_facing`), skate-mods `world_tuning` 3 passed. Data-gated
`npc_skater_keeps_its_facing_across_switches_on_the_exported_lines`: 212 rides, 882 switches, 130 kept by a flip,
0 spins (130 without `keep_facing`), deterministic. Not seen in game yet.

**Superseded (fix 23, 2026-10-08):** the flip is now a mod option, off by default; see "NPC skaters riding
backwards" below.

**Open.** The replay puppet plays one forward clip per phase; retail plays fakie riding clips while fakie and the
character's stance (regular or goofy) mirrors them. Recorded in-line facing changes on the ground without air (about
12 in DownTown) still turn the root within a node, where retail performs a revert. Both belong to the simulated tier.

## NPC skaters popping between animations, tricks not playing, 2026-10-05

**Problem.** The user reported (after fixes 12 to 19) that NPC skaters still popped between animations: one ends and
the next is already partly played, so the limbs jump between poses. Trick animations seemed missing: on a jump
the trick sound plays, but neither the board nor the limbs do the trick.

**Root cause.**
1. The puppet crossfade (fix 12) only knew ONE previous phase. On the shipped lines 13,183 of 27,464 phase changes
   come before a 0.2 s blend could finish [data] (crouch phases are 0 to 2 frames, a takeoff about 10, an air trick
   about 13). Each such change restarted the blend from the previous clip alone, dropping the running blend: the body
   jumped to that clip's pose in one frame (measured up to 0.83 m per joint on a jump).
2. Every recorded jump opens its trick span on the ground (most common sequence rolling > ground trick > air trick >
   air [data]), and the ground trick phase showed the 50-50 grind clip: the takeoff flashed a grind pose.
3. The node's trick slot was never read for animation: the air trick phase played an air idle clip
   (`IA_IDLE_LO_N_0_CYC`, the board turns 3 deg), so no flip, no board trick.

**Retail.**
- [code] The trick slot is the 40 B extended record's i16 at +0x24, an `EScorableID`: `sub_8246A2E0` (0x8246A4A0)
  loads `*(node+0x20)`, then `lhz 36(ext)`, `extsh`, and treats `> -1 && < 332` as a trick (332 = the scorable
  table 0x820862A8, `skate_core::scoring::catalog`). On the shipped lines: ollie 1925, gnd_bsgrab 829, kickflip 734,
  bsgrab 409, fsgrab 365, nollie 322, n_heelflip 322, heelflip 264, then grinds [data].
- [code] Ambient skaters are full skaters on the player's motion graph (doc 26 section 1); a graph transition keeps
  the running transition as its outgoing tree (Blend82B96058, fix 12), so blends nest.
- [data] `MotionGraphIncludes/Tricks/Tricks.xml` pairs each `TRICK_NAME` with an `ANIM_NAME`; `T_Trick.xml` /
  `T_Ollie.xml` / `T_Kickflip.xml` play `$ANIM_NAME$_G` on takeoff (`time="0.05"`), then `$ANIM_NAME$_A` with
  `transType="sequence"` (follows the ground clip, no blend; `time="0.1"` when entered from the air); the kickflip
  family continues with `$ANIM_CYC_NAME$1..3` and `$ANIM_OUT_NAME$n`. The `B_*` names are authored trees in the stock
  banks (phase blends over `TRICKHEIGHT`, selectors over `PROSKATER`), e.g. `B_KICKFLIP_IN_A` = phase blend of
  `KICKFLIP_IN_LOW_A` / `KICKFLIP_IN_HIGH_A`. The board is the rig's `Skateboard_Root`, so the trick clip moves body
  and board together (kickflip air sequence: board turns 169 deg).
- [video] Retail (RPCS3, `2026-10-05 13-01-52.mp4` 3:23.6, a varial heelflip from the same trick trees): the board
  turns under a continuous body over about 0.4 s (a local screenshot).
  NPC skaters in that recording are too far from the camera to read limb detail.

**Change.**
- `skate-core` `living_world/replay.rs`: `ReplayLine::{open_trick_at, node_trick}`, the cursor keeps the span's trick
  id and a 6-entry `PhaseEntry` history (phase, start frame, trick), `LineCursor::phase_history()`,
  `render_cursor()` (the cursor `render_sample` samples). `ReplaySample` unchanged.
- `skate-game` `living_world/npc_skaters.rs`: `puppet_layers` builds the pose from every phase whose blend still runs
  (oldest first, each blended over the result, stops at the first fully blended one; consecutive phases on one clip,
  or a clip continuing a `<ground>+<air>` sequence, are one layer from the older start); `puppet_layers_pose`
  evaluates them (`PoseCommand::Blend` chain); `puppet_layer_clip` picks per phase: a trick span with a trick
  animation plays `<base>_G+<air sequence>` from the ground (0.05 s in) or the air sequence alone from the air (0.1 s
  in), the air after an air trick continues it, everything else the phase clip as before; `retail_trick_anim`
  (EScorableID -> `B_<NAME>` per Tricks.xml; kickflip / heelflip and nollie forms `B_<NAME>_IN`; numbered, late,
  underflip, dark-catch variants their base flip or the ollie; grabs and other air tricks the ollie takeoff; grinds,
  slides, manuals, powerslides, reverts and handplants none), `trick_air_sequence`, `stock_tree_leaf` (selector
  default, a `TRICKHEIGHT` phase blend the child with the highest value, other phase blends the first child),
  `sequence_part` (`+` parts back to back).
- Moddable: `skater_clips["trick.<scorable name>"]` = an animation base (or a plain clip), `skater_blend_seconds`
  keys `trick_takeoff` / `trick_air` (`skate-mods` validation, `sdk/skate.lua`); reset on mod disable with the rest
  of `LivingWorldSettings`. Multiplayer: the pose is a function of the cursor (lines, branch records, tick), no new
  records.

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`; `crates/skate-game/src/living_world/npc_skaters.rs`,
`npc_tests.rs`; `crates/skate-mods/src/world_tuning.rs`; `crates/skate-data/tests/living_world_data.rs`; `sdk/skate.lua`.

**Verification.** skate-core `--lib living_world` 114 passed (new `replay_cursor_keeps_a_phase_history_with_the_span_trick`),
`--lib animation::` 44 passed; skate-game `--bin skate3rust -- living_world world_tuning` 56 passed (new
`living_world_npc_puppet_layers_nest_running_blends`, `..._trick_slots_pick_the_stock_trick_animation`, data-gated
`..._trick_clips_exist_and_flip_the_board`: every mapped tree resolves to a stock clip, kickflip board 169 deg vs 3 deg,
and `..._trick_jump_has_no_pose_pop`: on recorded jump timelines the pose at a phase change leaves the showing pose by
0.008 m, the one-previous-clip blend by 0.83 m); skate-mods `world_tuning` 3 passed; skate-data `living_world_data`
8 passed (new `npc_skater_trick_slots_and_phase_changes_on_the_exported_lines`). Not seen in game yet.

**Trick height (2026-10-29, research b690):** retail's AI perform step
(`sub_82469448`, the only direct caller of `sub_82470DD8`) always posts TrickHeight 1.0 normalized (`0x8231A844`) for
trick categories 1 / 2 [code], and `PhaseBlend` sorts its children by the value each reports, so the AI plays the
highest child, e.g. `KICKFLIP_IN_HIGH_A` and the `T_HI_*` cycles. The replay-tier puppet's `stock_tree_leaf` now picks
that child from the clips' own `TRICKHEIGHT` attribute (the sim tier already posts 1.0, `ai_signals.rs`). A blend
whose child is a subtree keeps the first child: the ollie (`B_OLLIE_A` = `OLLIE_LOW_A` 0.0 + the `B_OLLIE_WALL_A`
subtree) still plays `OLLIE_LOW_A` (open: the subtree's value). Data-gated tests
`living_world_npc_trick_clips_exist_and_flip_the_board`, `living_world_npc_trick_jump_has_no_pose_pop`.

**Next (research b691, names checked in the installed
`MotionGraph_OnBoard.stategraph`):** each stock grab / grind / slide / manual state names its score (`ScoringGrabs
grabName=` / `ScoringTrick trick=`) next to the animations it plays, so an index score name -> INTO / CYC / OUT built
from the loaded graph gives retail's clips with no hand table (e.g. fsgrab `GR_GRAB_N_FS_0_INTO` / `BLEND_FS_TWEAK2` /
`GR_GRAB_N_FS_0_OUT`; fs_overcrook plays `B_BS_GRINDCROOKS`, so no name rule). Ground grabs (gnd_bsgrab, 829 slots)
are the most visible miss. Grind / slide trees are TWIST phase blends over DISTTOCOG and need attribute trees
(`tree_commands`, like the fakie channel). Not ported yet. Inputs (research b692): the AI posts each grab's
`scoring_trick` intent tuples by name (`sub_824696B8` -> `sub_82470ED0`; ours already does on the sim tier,
`ai_signals.rs` `slot_signals`): FS / BS variants as unit TweakX / TweakY, mute / seatbelt as BoardAdjustAngle /
Mag; `BLEND_FS_TWEAK2` is a 2-D blend space over TWEAK_X / TWEAK_Y, the BS / mute / stale / dbl tweaks phase blends;
the graph eases them in (0.116 / 0.133 s, board adjust 0.25 / 0.08). Open before porting: by the action graph data a
tweak-only AI nosegrab would play a BS grab tweaked down (only BoardAdjust enters the nose / tail / crail states); a
data-gated graph_host test feeding the nosegrab schedule would settle it.

**Grabs and grinds ported (2026-10-29).** A grab slot (catalog category 4) and a grind / slide slot (category 5)
now play the trees of the stock motion graph state that scores them:
- Index ([Score clips](#score-clips)): every `ScoringGrabs` / `ScoringTrick` name of the loaded motion graph -> its
  INTO / CYC / OUT trees (312 names; every category 4 / 5 id the lines use).
- Which grab the AI reaches ([NPC grab scores](#npc-grab-scores)): the open question above is settled for our graph
  port. `air_playback.rs` `ai_nosegrab_schedule_through_stock_graphs` posts the nosegrab schedule through
  `slot_signals` into an airborne player: the graphs go to `GR_GRAB_N_BS_0_INTO` then `BLEND_BS_TWEAK`, score
  `bsgrab_down`, never a nosegrab clip. `ai_grab_schedules_through_stock_graphs` runs every category 4 schedule the
  same way; 33 grabs score another grab (nosegrab -> bsgrab_down = the BS japan, tailgrab -> bsgrab_up = melon,
  stale / crail -> FS tweaks, rocketair -> dblgrab, seatbelt -> bsgrab, the airwalks -> christ_air). The puppet's
  table is asserted against that run.
- Clips: air grabs take off with the ollie's `_G`, then INTO + CYC as a sequence (CYC loops); started in the air,
  INTO + CYC 0.1 s in; ground grabs (gnd_*, coffin) on the ground play their own INTO + CYC (0.1 s,
  `GroundGrabs/T_Grab.xml`); grinds / slides their CYC tree, 0.3 s in (`T_Grind.xml`). Blend spaces and phase blends
  take the child nearest the score's unit tweak (`_up` (0, 1) ... base (0, 0)); grinds the neutral TWIST child and
  the DISTTOCOG low end (fs_50_50 still `G_5050_FS_LOW_0_CYC`; others no longer fall back to it).
- Mods: `skater_clips["trick.<name>"]` still overrides; a graph mod changes the index (built from the loaded graph,
  resource `NpcScoreClips`). Data-gated tests `living_world_npc_score_clips_index_the_stock_graph`,
  `living_world_npc_grab_and_grind_slots_play_their_scored_trees`.

**Open.** Tweaks and grind twist are the settled vertex, not the graph's eased blend (attribute trees through
`tree_commands`: next step); the release OUT clips are not played; a mirrored NPC's hands are not swapped (the action
graph's `IsMirrored` branch of `T_Grab.xml`); the ground part of an air grab slot before takeoff shows the ollie
(retail posts the ground grab inputs there too: unchecked); manuals keep the phase clip; spins (`spins` byte) and
landing clips (`B_LAND_*`, 0.15 s) are not played; the per-skater `PROSKATER` selector takes the default branch.

## Silent landings (NPC skater sounds filled the voice cap), 2026-10-07

**Problem.** The user reported random quiet landings (two silent ones at the Carvetron, several in all),
and after the next session one more silent landing near its end.

**Root cause.** The landing logic plays every touchdown (a headless replay of the last 95 s of the user's 46-minute
session starts the touchdown voices on every landing). The in-game mixer, however, was full: it sat at its 96-voice cap
(`Mixer::max_voices`) and refused new opens without an error, so a touchdown whose voices were all refused was silent.
The voices leaked from NPC skaters: when an NPC skater lost its audio instance the host dropped its `NpcSkater` without
releasing its Splice sounds (`world::skaters::NpcSkater::deactivate` assumed "the Splice one-shots end by
themselves"). A Splice sound is only stepped, and its mixer voice only released, by its owner's update or release, so
the dropped skater's touchdowns, pop, roll, taps, scuffs, hand-on-deck, grind on / off and clothing sounds kept their
`SplicePlayer` slots and mixer voices for good. The mixer kept finished voices until released and counted them against
the cap.

**Evidence.** Session log of 2026-10-07 (AUDIO_TIMING `block_voices`): max 96 in 790 of 2789 s, from about 9 minutes
in; 96 every second in the last ~105 s. The per-minute voice floor rose from 17 to 90, only while NPC skaters were
released (58 after 63 releases, flat while none were released, 90 after 143), while the world at the end held
`skaters 2/0`. 143 of the 621 landings fell in seconds at the cap. Full report:
a local research note (not in the repo).

**Change.**
1. Retail parity (retail's release stops every layer): `NpcSkater::release` releases every Splice sound the skater
   holds: `Contacts::release_all` (pop, roll, ollie, landing, touchdowns, second voice, manual landing, taps, scuffs,
   plant / lift, and `StepOn::release_all` for the hands on deck), `Grind::release_sounds` (grind on / off, queued
   starts dropped) and `Clothing::release_all` (stroke / plant foley). The NPC host calls it next to `stop_wheels`
   when a skater loses its instance and for every skater on a map change (`NpcHost::reset`). Release is per owner id
   and deterministic (multiplayer-ready). Peds (`PedObjects::release`) and mod voices (`ModVoices`: ended one-shots
   are forgotten, `stop_owner` on mod disable) already released everything; checked, unchanged.
2. Safety net in the mixer: at the cap, voices whose sample has ended (`done`; every owner query already reads them as
   gone) are freed before the open is refused (`Mixer::make_room`, the same de-click fold as a release). Below the cap
   nothing changes, so what plays is identical until the cap is reached. Retail does not keep finished voices
   allocated (aems-voice-graph-spec 6.6).
3. Diagnostics: `Mixer::refused_cap` and `Mixer::evicted`; the `AUDIO_TIMING` line shows them per second as
   `voices_refused=` / `voices_evicted=` (only when not 0), so a future session shows refusals directly.

**Moddability.** No new mod surface: mod-owned voices are already released on mod disable (`stop_owner`), mods' mute
rules (`Observed`) see the same starts as before, and a released NPC frees its sounds whatever mod content its banks
hold. The cap stays a plain `max_voices` field.

**Files.** `crates/skate-audio/src/world/skaters.rs`, `crates/skate-audio/src/player/{contacts,step_on,clothing,components}.rs`,
`crates/skate-audio/src/mixer.rs`, `crates/skate-audio/src/splice/mod.rs` (`sound_count`, diagnostics),
`crates/skate-game/src/game_audio/npc_skaters.rs`, `crates/skate-game/src/game_audio/timing.rs`.

**Verification.**
- New tests: `releasing_an_npc_skater_frees_its_splice_sounds_and_mixer_voices` (an NPC ollie and landing holds its
  sounds; after its release the Splice sound count and mixer voice count return to the start),
  `two_hundred_claim_release_cycles_keep_the_voice_count_bounded` (0 / 0 after every cycle, no refused open),
  `finished_voices_do_not_block_a_new_open_at_the_cap` (below the cap a finished voice stays as before; live voices
  still refuse at the cap; finished ones make room).
- `cargo test -p skate-audio --locked`: all pass. `cargo test -p skate-game --bin skate3rust --locked -- game_audio
  living_world`: 124 passed.
- Behaviour identity for the player: the e2e bench (`tools/audio-e2e/scenarios.py`, 12 scenarios, `E2E_FPS=60`)
  rendered before and after the change: all 48 outputs (audio, voices, body, deck) byte-identical.

**Open.** Confirm in the user's next session that `block_voices` stays flat and `voices_refused` stays absent. "No
landing noise when landing in manual" is a separate report (by design the kind-2 touch is skipped while in a manual);
check it against `sub_824BB330` if the user still hears it.

## NPC Skater Physics: hitches, skaters vanishing up close, path projection (2026-10-13)

### Problem
The user reported (2026-10-13) that skaters were only visible in the distance and disappeared up close,
with large hitches, and wanted it fixed rather than the opt-in switched off.
Sessions `hails-additions/logs/game-20261013-183803` and `-204749` (NPC Skater Physics opt-in on): 59 / 63 switches
replay -> physics, 26 / 43 bails; 35 of 39 switches hitched over 150 ms.

### Causes and changes
- **Hitches** (measured, `NPC_SKATER_SIM_LOAD`): every switch loaded a skater from disk: runtime 149 ms, controls
  31 ms, camera 37 ms. Now `NpcSimPool` keeps loaded skaters with their own physics context (filled up to the sim
  limit on the first enabled tick, refilled by hand-backs and despawns); a switch or a bail respawn places one with the
  player's teleport (`SkaterRuntime::travel`; retail places its AI skater object again). Its own context is kept (a
  fresh context restarted the clock and the camera refused the older subject tick). Earlier the same day: a blocked
  skater no longer restarts the sim every frame beyond the radius.
- **Start state** (research b109): sims started between nodes, from rest
  (the teleport reset zeroed the context velocity) and 0.1 m up. Now a sim starts only on a node with retail's spawn
  push (x 0.75, `82C04168`) carried through the teleport.
- **Path projection** (research b110, new skate-core
  `living_world/path_follow.rs`): the line cursor of a simulated skater followed the recording's clock, so the
  target and the trick events ran away from the skater (137 of 144 trick dispatches were more than 1 m off the line).
  Ported `sub_8246D560`: nearest node in a forward window (`sub_82455BB0`), two-segment projection with the 0.5
  hysteresis and the 0.25 clamp (`sub_824563D8` / `sub_8246C930`), lane test (widths / 50, `sub_8246CCC8`), rejoin
  commit within 0.5 m (`sub_8246E2B8`), cursor B one shaped recorded tick ahead (`sub_8246E3B8`), on path dropped
  beyond 1.0 m outside the lane (`sub_8246DB50`; no recorded trick then). The cursor now goes to B
  (`NpcReplay::follow_target`); replay resumes from there on hand-back. Values: `SimulatedTierSettings::path`
  (retail defaults, mod-overridable). Log `NPC_SKATER_PATH` (left / rejoined the path).
- Test knob `SKATE_NPC_SIM_RADIUS`; per-system living-world timing in `FRAME_HITCH` (`living_world=[...]`).

### Verification
- skate-core `living_world::path_follow::tests` (4); skate-core living_world 285 passed; skate-game living_world 87.
- Muted DownTown runs, radius 150 m, 150 s, up to 3 sims:

| build | disk loads | hitches over 100 ms | bails |
|---|---|---|---|
| before (user session 204749, radius 40 m) | one per switch | 82 | 43 |
| pool, node start, trick gate | 27 | 14 | 29 |
| + own context kept | 4 | 3 | 26 |
| + path projection | 3 | 2 (both at load) | 17 |

### Open (next steps)
- Remaining bails are mostly ground wipeouts while rolling (`PhysicsGround -> WipeoutGround`) and many path leaves
  (48 left / 31 rejoined in 150 s). Checked 2026-10-29 (research b674): the steering-target gate, the lost hand-over,
  the stuck tracker and the projected point were ported since (`ai_record.rs`, `path_follow.rs`, `ai_controller.rs`,
  `npc_sim.rs` `projected_point`). The node-change handler `sub_8246BEE0` was the gap: a simulated skater branched
  from the replay cursor with no gate, so it could switch line mid-grind, and rejoined at the node nearest the node,
  not its body. Now `BranchContext::body` (`BodyGate`, set by the sim each tick): no branch while airborne, walking,
  or in trick category 1 / 2 / 5 (`NO_BRANCH_CATEGORIES`, data); scored and rejoined from the projected body point.
  Test `a_simulated_skater_branches_only_through_the_gate`; the NPC goldens are unchanged (replay tier). Still open
  (b674): the upcoming-trick test (pc+796 / +797), retail's line-end
  test (A >= count-2 with t > 0.8), raising lost in the same tick, the skitch / bit 26 / online gates on the target.
  Done the same day: the stuck tracker measures from the animation root; on the board the projected point is the
  deck rigid body's position (`Reckoning+144`, its centre of mass), not the deck part's origin.
- A bail still hides the skater for 5 s (retail respawn delay); what the user sees as "vanishing".

## NPC Skater Physics: bails (2026-10-13 night)

### Problem
The user's decision (2026-10-13): fix it, the opt-in stays on (NPC Skater Physics skaters bail and vanish up close). Muted
DownTown baseline (radius 150 m, 150 s, player idle): 17 bails, 79 on/off-path switches.

### Causes and changes
- **Steering record off the path** (`sub_8246DB50`, verified 0x8246DB50..0x8246DD40): retail zeroes the record,
  seeds the flag word with `pc+922 << 25` and clears bits 31..26, and only builds the steering target
  (`sub_8246DE38`) when the skater is on the path and past the start nodes (`pc+920`, `pc+937`), or always without
  on-board steering. Ours wrote the steering target every tick, pulling a skater that had left its line back toward a
  point metres away. Now `ai_record::steers` / `ai_record::without_steering`: off the path the skater coasts on its
  own physics until the rejoin commit. 17 -> 4 bails.
- **Respawn counted twice:** the placing teleport is queued until the next physics step, so the old wipeout bit was
  still set on the next tick and started a second 5 s bail (2 of those 4). `NpcSim::placed` ignores it until the bit
  clears.
- **Line kept running during a bail:** the projection kept moving the cursor while the skater lay on the ground
  (node 6 -> 14, a trick dispatched mid-bail). During the respawn wait there is no projection now (the line waits, as
  the comment always said).
- **Stale first tick:** the first tick after a start or respawn projected the body's position from before the queued
  teleport (a pooled body: anywhere), moving the cursor nodes ahead ("left the path at node 1 (A 5+1.00, 79.25 m)").
  `NpcSim::teleport_pending` skips the projection until the teleport has been applied.
- **Respawn node** (`sub_8246ED68`, decoded and verified 0x8246ED68..0x8246EE24, picked when the bail starts by
  `sub_8246EE30`): the projection's search node `pc+816` plus 2, a remembered prop node ahead within 20 instead,
  clamped to the last node, then the first safe node (`sub_82455728`, our `ai_controller::safe_node`). Ours placed the
  skater where it fell, on a take-off: one skater respawned 24 times in a row at node 17, wiping out right after each
  placement. Now `ai_controller::respawn_node`. Not ported: game modes 19 / 20 (`sub_824681D8`) use
  `sub_824553D8(line, cursor A)`.
- **Lost hand-over** (research b115; verified the test's structure in
  `sub_82468DD8`): on a tick its search node did not advance, a skater that was off the path, more than 6.0 m from
  the search node and from both lane edge points, and slower than 14 m/s (`pc+808`) is lost; that raises the same
  reposition request (`ctrl+941`) mode 7 raises, so controller B walks it back (goal `pc+816` + 2, safe node). On B's
  hand-back retail's A activate (`sub_824689C0`) resets the controller off the path with the search at the node
  nearest the skater over the whole line (`sub_82455108`); the normal rejoin commit puts it back on. Ours re-attached
  on the path at B's goal node. Now `path_follow::lost`, `PathFollow::reattach`, `path_follow::nearest_node`; the
  reposition counts from `pc+816`. Log `NPC_SKATER_LOST`. Values in `PathTuning` (`lost_radius` 6.0,
  `lost_max_speed` 14.0; retail defaults, mod-overridable).
- **Projected point** (research b116, verified the state 500 /
  wipeout tests in `sub_82467FC8`): off the board (BipedGround) or wiping out retail projects the animation root
  (Skeleton +416), not the board; the speed `pc+808` (`sub_8246BCF8`) is that point's displacement, negative only
  above 0.01 m/s against the heading (the skeleton's forward off the board). Ours projected the deck always, so a
  skater walking back was measured where its board lay. Now `npc_sim::projected_point`; the hand-back's nearest node
  uses the skater too.
- **Getting back on the board** (research b117, verified the press in
  `sub_8246FA30`): retail's line controller A, not controller B, re-boards a skater: every path tick it presses the
  off-board toggle (`824712E0`: `NewToggleOffBoardState` + `ToggleOffBoardState` while off the board and not
  toggling, every 5 ticks) and then sets mode 2 ("get on") for the next tick; A's activate clears it. Ours had no
  toggle in A, so a skater handed back by controller B stood off its board for good. Now
  `ai_controller::OffBoardToggle` in the line tick (period from the controller B settings, retail 4).
- **Line ends and switches** (research b118): retail has no
  "finished" line for an AI skater: at the end it looks for the next line every tick (`sub_8246C7F8`), and every
  switch runs the reset `sub_82468AC8`, which leaves the skater off the path (rejoin commit again) but keeps the start
  gate `pc+937`. Ours handed a simulated skater at a line end back to the replay tier, which faded it out, and
  restarted the projection on the path after a switch. Then `LineCursor::retry_chain` each tick for simulated skaters
  (log `NPC_SKATER_CHAIN`; replaced 2026-10-30 by the chain inside `npc_sim::simulate` on the trigger tick, doc 26e "Line end trigger"), no line-end hand-back, and `PathFollow::switch_to` on a switch and on controller B's
  hand-back. The retry runs whenever the cursor sits on the last node (the projection holds it there, so it never
  became "finished" and never retried at first).
- **Line-end fallback** (research b122, `sub_82458968` mode 1): with no
  line start within 4 m, retail takes the nearest free start at any distance (a valid one over a nearer invalid one
  below 36 m^2; per-character validity is not modelled, so every line counts as valid). Ours ended the line, and the
  skater rode off, got lost, bailed and was respawned at the line end again: two skaters made 70 of 72 bails in a
  300 s run that way (each placement there launched them at 25 to 100 m/s, cause not found; trace
  `NPC_SKATER_PLACED` at debug level). Now `ChainConfig::fallback` (on for the physics tier only; the replay tier
  cannot ride the gap). Not ported yet: retail's end point (B past 0.8 of the second-to-last segment), junction scoring from the physics skater.
- **Ground steer** (research b121): retail also steers an AI skater with
  stick input every path tick on the ground, on or off the path (`sub_8246ACF0` -> `sub_8246AA00` /
  `sub_82471070`): the heading error to a point 45 ticks ahead on the line, moving toward the point 2 ticks ahead
  when the skater is 0.4 to 3 m off it, written to `Turn`, `BodySpin` and `KickTurn` (full at 18 degrees). Off the
  path this is what turns a skater back to its line. Ours had no stick input at all. Now skate-core
  `living_world::ground_steer` (tuning `SimulatedTierSettings::ground_steer`, retail defaults); `PathFollow` keeps
  the lateral ratio `pc+868`. Not mapped: the pending trick's category 9 gate.
- **Respawned cursor ran ahead:** the re-created cursor counts its own frames, but the follow target still held the
  old count, so the cursor ran up to 38 nodes ahead right after the respawn (then "lost" 50 m from the path). Now the
  target is reset with the cursor.
- **Airborne** (research b119, verified the writer in `sub_82DB6EC0`):
  the skater state byte `+438` that `sub_8246DB50` and the junction switch read is "an air state (200..299) with no
  wheel touching", not "riding". While airborne retail writes no steering target and does not drop the path. Now the
  sim passes it to `ai_record::steers` and `PathFollow::tick`. The replay tier's flip latch reads the same byte as
  "riding" (replay.rs `node_riding`); left as is tonight (research b120 first: it touches the facing rule behind the
  earlier backwards-riding fix).
- **Air steer** (research b123): in the air retail steers the character toward
  the recorded skater forward at the near look-ahead (`sub_8246B358`, reversed when the takeoff latch `pc+927` says
  the character faced against the node's recorded skater, `sub_8246A700`), with a 2 degree dead zone and full stick
  at 10 (`sub_82471188`), on the same three keys. Now skate-core `living_world::air_steer` (tuning
  `SimulatedTierSettings::air_steer`). Measured at takeoff: the character forward (animation root row 2) lines up with
  the deck forward (dot 0.9 to 1.0), so the input axis is right. **Was off by default from 2026-10-13 to 2026-10-25 (on since, see below):** muted A/B runs (240 s,
  same build, `SKATE_NPC_AIR_STEER=1` to turn it on) had more bails with it: radius 150 m 48 vs 19, radius 110 m 33
  vs 24. Candidates: the board adjust write it also makes (`BoardAdjustAngle` / `BoardAdjustMag`, `sub_82471008`, not
  ported), whether retail's AI input map keeps or clears an unwritten key between ticks, the slerp fraction
  (`sub_82459F68`). Ported anyway so the next step is a switch, not a rewrite.
  **On by default since 2026-10-25 (b616, ask 354):** with it off, sim skaters kept their takeoff yaw through every
  air, so recorded 180s and quarter pipe airs landed backwards (92 of 269 `NPC_SKATER_BACKWARDS` in laptop session
  game-20261025-171928 came within 2.5 s of an air), rode the line backwards, went LOST and were bailed by stuck
  stage 5. Retail calls the air steer on every airborne tick with no gate (call at `0x8246ADE4`). The board adjust
  write named above has since been ported (board yaw, on by default), so the A/B in the table is out of date and was
  re-run (see the table). Mod: `npc_simulated {air_steer, air_steer_dead_zone_deg, air_steer_full_deg}`; A/B knob
  `SKATE_NPC_AIR_STEER=0`. `NPC_SKATER_LAND` logs each sim landing's facing against the node's recorded skater
  (`aligned` = sign matches the takeoff latch); `NPC_SKATER_BACKWARDS` for sim skaters now counts ground frames on the
  board only and prints the state and the latch.
- `NPC_SKATER_BAIL` now says what the skater was doing: previous state, speed, node, cursor A, on path, distance to
  the path and position (logs must diagnose).

### Verification
- skate-core: `ai_record` (steering gate), `ai_controller` (respawn node), `path_follow` (lost radius / lane edges /
  speed gate / node advance / reattach / nearest node); living_world 290 passed.
- Muted DownTown runs (`tools/dev-bench/npcsim.sh`, radius 150 m, up to 3 sims, player idle):

| build | length | bails | hitches over 100 ms (excl. the two load frames) |
|---|---|---|---|
| before (8489da5e) | 150 s | 17 | 1 |
| steering gate | 150 s | 4 (2 double counts) | 1 |
| + respawn / teleport fixes, retail respawn node | 150 s | 2 | 2 |
| + lost hand-over | 300 s | 4 | 9 |
| + projected point | 300 s | 4 | 15 |
| + line controller re-boards (skaters ride again instead of standing) | 300 s | 55 | 16 |
| + line-end retry, switch reset, airborne gate, ground steer | 300 s | 72 (70 at two line ends) | 14 |
| + line-end fallback, retry on the last node | 300 s | 19 | 12 |
| + air steer (A/B, 240 s, radius 150 / 110) | 240 s | 48 / 33 on, 19 / 24 off | 7 / 4 on, 7 / 8 off |
| air steer re-run after the board yaw port (2026-10-25, b616) | 240 s | 6 / 7 on, 6 / 6 off; no stuck stage 5 | 2 / 2 on, 2 / 3 off |

The 2026-10-25 re-run also counted `NPC_SKATER_LAND`: on, 123 / 117 landings, all aligned with the recording; off,
10 / 7 of 120 / 116 landed against it. Sim `NPC_SKATER_BACKWARDS` (ground, on board) was 36 / 28 on vs 16 / 25 off,
mostly `riding_fakie` (27 / 19 on), which retail allows after a latched landing; one skater (#2 at radius 150) gave 9
around its own bail / respawn cycle. Logs: local test output of research b616.

The long frames left (100 to 165 ms) show no physics steps and nothing in the instrumented stages or the living
world systems: not the NPCs; open.

### Open
- `pc+922` (on-board steering) comes from an undecoded skater field (`[[rec]+32]+320`, `sub_824660A0`); ours stays
  set. A's second toggle writer `sub_8246FFF8` (node flag 0x10 while state 500) is not ported.
- On the board retail projects the board rigid body's position (SkateboardReckoning +144); ours the deck part (b116,
  field not mapped yet). `pc+808`'s scale `[0x8303EE08]` (taken as 1/dt = 60). `+1904` bit `0x04000000` is a scripted
  AI flag (UnpinAI / ClearBehavior), never set for ambient skaters; `G+608` and motion state 104 not read.
- Ported 2026-10-29: the lost test on a line-end chain's tick (`sub_8246D3C0` calls the chain `sub_8246C7F8`, then
  `sub_82468DD8` at 4.5 m, `0x821DDA68`, from the chained-to node whether or not the search node moves;
  `PathFollow::chained_to`, `PathTuning::line_end_lost_radius`, test
  `a_line_end_chain_tests_lost_at_four_and_a_half_metres`). A junction switch keeps the 6 m test after no node
  change. The node-change handler `sub_8246BEE0` is the body gate above.
- Next (research b694, decoded in Ghidra and the recomp asm): retail's chain
  `sub_8246C7F8` searches line starts within 4.0 m of the skater's body point and scores them with the skater's own
  heading and speed (`sub_8246C3C0`); ours searches from the line's last node and its last segment (`replay.rs`
  `chain`, `choose_next_line`). Retail tests the line end (`sub_8246D3C0`, the previous tick's A / t, 0.8) before
  the projection in the same tick, keeps the old line and retries when no line is found (ours finishes the cursor),
  and takes the nearest invalid start when the nearest valid one is 6 m or more away (`ChainConfig::fallback_valid_d2`
  is set but read nowhere). The reposition runs after the projection (`sub_8246FE38`), so a lost request is handled
  the same tick. Port plan in b694 sections 2 and 3; not started.

## NPC Skater Physics: bails right after a placement (2026-10-15)

**Problem.** Simulated NPC skaters wiped out about 0.45 s after every placement (switch to physics and every respawn), so
the user rarely saw one ride past (to the user the skaters seemed to crash as soon as they came into view).

**Evidence.**
- New log `WIPEOUT_REASONS` (`physics/player_state/selection.rs`): placement bails come from PhysicsGround with ground
  request 3 (deck angle error) and 18 (squash). They fire when the 0.4 s teleport wipeout cooldown runs out, so the bad
  state is there from the first tick. A review of 225 bails in 13 user sessions found the same in older builds at
  0.2 to 0.3 s, matching their cooldown (a local research note).
- New debug log `NPC_SKATER_STEER` (deck, line and velocity yaw per tick after a placement): on ticks 3 to 5 the deck
  tumbles (up y 0.82 to -0.26), then the skater is left 50 to 60 deg off its line at 3 m/s and takes off crooked
  (`NPC_SKATER_TAKEOFF char.deck 0.72`, `deck.travel -0.41`). Placement frames themselves are upright
  (`NPC_SKATER_PLACE_FRAME` up y 0.99), and the steer sign is right (flipped, it diverged faster).
- Every placement ran two resets: `SkaterRuntime::travel` queues the host teleport with the spawn push and the actor
  reply, which resets again one tick later and snaps the rolling board back (two `player teleport spawn` lines per
  placement). Retail places once (`825926F8`) and runs its hold branch (`824701F8`: no steer while the placed skater
  settles) (a local research note, code reading, not traced).

**Change.**
- `place` uses one reset, the host teleport with the spawn push (`SkaterRuntime::teleport_with_velocity`).
- A hold after each placement: no ground steer and no trick dispatch for `npc_simulated.placement_hold_seconds`
  (default 0.5 s; mod key, 0..=10).
  Changed 2026-10-17 (doc 26e "Handover hold and spawn push"): the hold is retail's counter (30 ticks after a reset
  placement, 8 after a bail respawn via `npc_simulated.respawn_hold_seconds`, one tick at a replay -> sim switch) and
  the push comes on its last tick.
- Crash fix (same day): `NpcSim::respawn_node` keeps its line; a walk-back hand back could chain the cursor to the next
  line during the respawn wait, and the old line's node index ran past the new line (`npc_avoid.rs:204`, index 103 of 70).

**Verification.** Muted 90 to 150 s runs at the PCU Library (`tools/dev-bench/npcsim.sh`, `MAP=University
TELEPORT=dist_university_library`, radius 150 m):

| placement | bails | within 1 s of a placement |
|---|---|---|
| two resets (before) | 16 in 90 s (21 placements) | 4 |
| actor reply only | 9 in 90 s (15) | 4 |
| host teleport only | 6 in 90 s (10) | 1 |
| host teleport + 0.5 s hold (shipped) | 8 in 150 s (17) | 0 |
| actor reply + 0.5 s hold | 12 in 150 s (17) | 4 |

**Second pass (same day): one respawn path, and a hold with no record target.**
- Every respawn wiped out again within 0.2 s (8 of 8 in a muted run). Two respawn systems ran per NPC: our 5 s timer
  placed it with a teleport while its wipeout was still running, and the skater's own wipeout-end respawn (the player's
  checkpoint respawn, `physics/respawn.rs` `request`) later threw it to an old checkpoint (41 m off its line, then a
  walk-back). Now the bail sets the chosen node as the skater's respawn destination
  (`SkaterRuntime::set_respawn_destination`); the wipeout-end teleport places it there through the actor reply (one reset,
  like the player); `npc_sim` only follows with the line state. A timer 10 s past the respawn delay stays as a logged
  fallback.
- During the hold the record carries no target (retail's hold branch: no steering), so the record no longer pulls the
  body while it settles (pose displacement, request 1, fired at 0.45 s before).
- Muted PCU runs, 180 s each (real wipeouts counted from `WIPEOUT_REASONS`, which include ones the old bail log hid):

| | wipeouts | checkpoint teleports | walk-backs | placements |
|---|---|---|---|---|
| host teleport + hold (first pass) | 29 | 14 | 12 | 20 |
| one respawn path | 15 | 2 | 2 | 16 |
| + no record target during the hold | 4 | 1 | 1 | 8 |

**Retail riding mode (recomp trace, 2026-10-15).** Hook `AIPC922` (category `aiskater`, `824660A0`, research-hooks
branch, committed): the path controller's `pc+922` (the hips ground test, which keeps the record's physics-riding bit
25) is 0 only on a controller's first sample and at its end, 1 for its whole ride (24 controllers, 360 samples,
a local data folder). Retail's ambient AI skaters ride with full physics, like ours; FollowPath (105) is not their
riding mode. `824660A0` writes `pc+922` every call (not a one-way latch).

**Third pass (same day, after the user's play-tests).**
- Vanishing: the user saw skaters vanish instantly, board and all, and never come back at the switch to physics. The
  render pose of a simulated skater is relative to its animation root (joints within ~2 m of 0, new log
  `NPC_SKATER_DRAWN`), but the draw set the puppet root to the world origin, so every simulated skater was drawn at the
  map origin. The puppet root now sits at the animation root (`npc_skaters.rs`, `NpcSim::animation_root`).
- Placement: one actor reply (retail `825926F8`, `SkaterRuntime::place_by_reply`) for the switch from replay too; the
  host teleport alone moved the board but not the animation root.
- Placement guard: the reply lands a tick or two later; the guard lifted after one tick, so the first projection used
  the pooled body (71 m off the line) and the skater was "lost" and walked off within 0.1 s (the user saw them often bail
  and then run off to a new line). It now lifts when the placement count changes (logged 30-tick limit).
- Ghost proxy: `push_proxies` built collision solids from every NPC's replay sample, simulated ones included, so a
  simulated skater rode with an invisible solid copy of itself on its line (contact bails, flings) and the player hit that
  copy. Simulated NPCs get no replay proxy now. Done since 7fb5dd0f (2026-10-16): each simulated NPC's own last-step
  body joins the proxies as a peer (`push_proxies`, `npc_sim_proxy_mark`), and each NPC's own solve leaves itself out
  (the player and the other simulated NPCs only); `npc_simulated.bump` / `SKATE_NPC_BUMP=0` turns it off.

**Open.**
- NOT RETAIL YET: retail places with the actor reply only and pushes from the hold branch; our reply-only variant
  bailed more. Needs a recomp trace of the branch `824701F8` takes after `8246E558` (pc+888 / pc+924 on the first ticks).
- Remaining bails are mostly in the air (KnownAir / PhysicsAir request 1, pose displacement) and spread over lines and
  nodes; long walk-backs were wiped out by the stuck monitor after ~10 s (new log `NPC_WALK_PROGRESS`; the log shows them walking until then, not standing).
- Logs added: `WIPEOUT_REASONS`, `GRIND_WIPEOUT`, `PED_DESPAWN at= focus= distance=`, `NPC_AVOID` (bad cursor),
  `NPC_SKATER_PLACE_FRAME`, `NPC_SKATER_STEER` (debug), `NPC_WALK_PROGRESS`.

## Simulated skater bodies share one animation source (load time, 2026-10-16)
- **Problem:** each simulated skater body read and decoded the animation banks from disk (`NPC_SKATER_SIM_LOAD`
  ~222 ms each in the 2026-10-16 sessions; the pool loads 3 at world start, ~670 ms inside the load hitch).
- **Change:** `NpcSimPool::source` keeps the first body's `AnimationSource`; later bodies are built on it
  (`GamePhysics::load_skater_in_context(.., source)`, `SkaterRuntime::load_for_world` also takes the source's banks
  for the metadata). `SkaterAnimation::load` is `from_source` over a fresh `AnimationSource::load`, the source is
  immutable (no interior mutability in `animation_pose.rs`), so a shared body is the same runtime.
- **Verification (optimisation rule: identical behaviour first):** data-gated
  `a_body_on_a_shared_animation_source_rides_like_a_fresh_load` (DownTown): a fresh and a shared body ride the same
  push for 240 ticks, deck transform, skeleton root and physical state equal every tick (bit for bit, `Debug`
  strings); load 161-166 ms from disk vs 64-66 ms on the shared source (2 runs). The bump test still gives NPC wipeout
  tick 26 and player tick 36. The log line now says `(shared animation source: true/false)`.
- **Open:** the remaining ~65 ms (collections, skeleton, controls and camera loads); the first body could use the
  player's source too (left out: a mod could change the player's clips).

## Simulated skaters send the full audio record (2026-10-16)
- **Problem (research b161):** both NPC tiers sent a cut-down audio state (`lite_state` through
  `AudioState::rolling`, about 16 of ~80 fields), so a simulated skater's bails, pushes, plants, pop strength, deck and
  body impacts, skids and seams never sounded, and the bail grunt (armed on the record's bail) never fired.
- **Retail [code]:** the bridge `sub_824B0DA8` fills an AI skater's record from its skater entry the same way as the
  player's; the player's sound objects then run on it (our `skate-audio/src/world/skaters.rs` already matches that).
- **Change:** after each simulated step `npc_sim::run_steps` swaps the NPC's context into its worker and calls
  `skate_events::skater_audio_state` (the hook the audio port left for this) with the NPC's runtime and its own
  `SkaterAudioMemory`; `NpcSim::audio_state` holds the record and `npc_skaters` sends it (the lite state only before
  the first step after a placement). A placement resets the memory.
- **Verification:** data-gated `a_simulated_npc_skaters_audio_record_is_its_own` (DownTown: the record's board is the
  NPC's deck, 4 wheels down, not the player's board); skate-game tests pass (known failure only). Not listened to yet.
- **Open (b161):** the replay tier still sends the lite state (its lines record no contacts); the engine never sets
  the reaction bits (slam, missed trick, positive, chase, own crash: entry +388 / +392 / +396, writer not found);
  the loose-board state is never sent for NPCs; the 30 m gate's position.

## NPC skater x player contacts in the session log (2026-10-18)

- **Problem:** laptop session 2026-10-18 22:01, an NPC skater hit the user and bailed (in
  Aletown, near the end). The log had no line for a skater touching another skater, so the bail could not be tied to
  the contact.
- **Change (log only, behaviour unchanged):** after each simulated NPC's step, `npc_sim::log_skater_contact` reads its
  skeleton's skater contact (`collision_feedback.other_skater` / `maximum_skater_force`, the values its wipeout check
  reads) and logs `NPC_SKATER_CONTACT #<serial> <character> with=<player|#serial> force speed team wiped at` once per
  contact (the same skater again after half a second apart counts as new). `team` = hit by the local player, which is
  when the bumped skater says its 8290 team line. `NPC_SKATER_BAIL` now ends with `skater contact <who> <s> ago` for a
  contact within the last second (else `none`). Names come from `npc_skaters::peer_name`.
- **Multiplayer:** the age counts simulation steps, not wall time, so every peer that runs the sim logs the same.
- **Verification:** test `contact_peers_have_readable_names`. Not play-tested.
- **Open:** only simulated NPCs (NPC Skater Physics opt-in) have a skeleton solve; replayed NPC skaters' contacts are
  not logged. The player's own side of the contact is not logged separately.

## DLC map lines and opt-in pumping (2026-11-09)

- DLC map lines [data]: the DLC map packs carry AI skater lines in the same `cSim_*` region arenas as the city
  districts (AIPATHDATA, RW type 0x00EB0014): Art Gallery (`DIST_AGPark`, 59 lines), Danny Way's Hawaiian Dream
  (`DLC_DW_MegaCompund`, 32), Sanatorium (`DIST_SanitariumDLC`, 41), Maloof Money Cup (`DIST_MaloofMoneyCupDLC`, 28);
  BackLot, DownTown Skatepark Night and Zen have none, and no base disc park (Stadium / MegaPark included) has any.
  The export read only the disc's `worldDIST_*.big`, so these maps had no NPC skaters (laptop
  autopilot-races-20261109-0812, spot1). Now `dlc.district_bigs` lists each DLC map's stream folder and the line,
  census and road readers (`living_world_skaters.district_tiles`, `living_world.district_assets`) read them too; the
  district key is the map name (the folder without `DIST_`). Setup passes its DLC folder to the living world stage
  (`stage_job.py --dlc`), and the livingworld group's fingerprint follows the DLC map packs. The DLC maps have no
  census layers on the disc (no peds / car census there); NPC skaters spawn from their lines.
- Pumping [code, research `.local/research/npc-auto-pump.md`]: retail AI skaters never post `AutoPumpAngle` /
  `AutoPumpMag`; only the player's pad listener does (`sub_825999F0`, from the left stick), and an AI crouches only where
  its line's node is flagged crouched (`sub_824696B8`). OURS, opt-in (`npc_simulated.auto_pump`, off = retail): an NPC
  skater on the ground going down a line slope of -0.04 or steeper at 1.5 m/s or more, not pushing, posts
  `AutoPumpAngle` 0 and `AutoPumpMag` 1 for 40 ticks then 0 for 10 (`autopilot_brain::pump_magnitude`, `PumpRules`), so
  the stock crouch node crouches and pumps as for a player. The player autopilot always pumps. `NPC_SKATER_SPEED`
  counts `pump ticks` next to `push ticks`.

## Code reference: living_world/npc_skaters.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/npc_skaters.rs`; each comment there links its section here.

### NPC skater replay tier

NPC skaters, replay tier (doc 26, milestone 3): turns the population's skater spawn / despawn records into visible skaters that ride their recorded lines.

- **Entity** per spawn record: [`NpcSkater`] (stable `LivingWorldId`, character key, slot, seed, voice) + [`NpcReplay`] (the `skate_core::living_world::replay` cursor) + `Transform`. The id map [`NpcSkaterIndex`] finds it again for the despawn.
- **Motion** (`FixedUpdate`, after the population step): each cursor is kept at `population tick - spawn tick` recording frames (the 60 Hz lines at the retail 60 Hz world tick, `skate_core::living_world::clock`), so the state is a function of the spawn record and the tick; branch decisions use the retail score with the players and the other NPCs and are kept as records a client would mirror. The NPC's position goes back into the population (culls, the 5 m rule). At the end of a line it continues on an unused line starting within 4 m, picked by the same retail score (`replay::choose_next_line`, `sub_8246C7F8`; fix 9), so it keeps riding until the population's 120 m cull removes it. A new NPC fades in over its first second; only at a dead end (no line starts near the end) does it fade out and leave once its opacity is below 0.2 (`skate_core::living_world::leave_fade`; [`NpcFade`]).
- **Collision**: a kinematic proxy (capsule for the body, box for the board, infinite mass, the cursor's velocity) joins `physics::network::Proxies` like a mod's solid, so the player bumps into it. NPCs never react (replay tier).
- **Look**: the character's native roster GLB (`CustomModels::online_native_path`, the same files the customiser and online players use), or a mod's GLB from [`NpcSkaterLooks`], else the stock skater. Bound to the stock skeleton like a remote player (`AnimationStatus`).
- **Puppet animation** (`Update`): one stock clip per [`ReplayPhase`] ([`puppet_clip`]), evaluated with the player's evaluator; root = the recorded position and skater orientation. The stock graphs are not run (simplification until the simulated tier). The clip time is the time since the phase began; a looping clip (`_CYC` in its name) wraps by its length like the player's `ClipClock`, others hold their last frame. Each NPC carries [`NpcPuppetClip`] (the clip and time in effect). A mod may override the clip per phase id or `<phase>.<style>` (`LivingWorldSettings::skater_clips`, `sdk.world.set_tuning('living_world', {skater_clips})`); an override that does not evaluate falls back to the shipped pick. A phase change (also one a branch or line chain brings) crossfades from the previous phase's clip, which keeps playing, with the player's graph transition curve (`playback_transition::transition_weight`, Blend82B96058: smoothstep over the transition time) over [`RETAIL_BLEND_SECONDS`] (the stock graph's default transition time); mods set it per phase (`skater_blend_seconds`). The weight is a function of the cursor's phase frames, so a client derives the same pose. Render (fix 14): drawn from the cursor one tick back ([`NpcReplay::previous`]) towards the current one by the fixed-step fraction, like the player, with the recorded branches (`LineCursor::render_sample`); clip and blend times include the sub-frame; after a branch or chain the root blends onto the new line (`replay::LineSwitch`, `skater_line_chain.blend_seconds`). Fix 21: the crossfade nests like a retail transition (Blend82B96058 keeps the running transition as the outgoing tree): the cursor remembers the last `replay::PHASE_HISTORY` phases and the pose is built from every phase whose blend still runs ([`puppet_layers`]), so a phase change during a blend (half of all changes on the shipped lines) no longer restarts from one clip. A recorded trick slot (`EScorableID`) plays its stock trick animation on body and board (the board is the rig's `Skateboard_Root`): `<anim>_G` on the ground, `<anim>_A` once airborne, like `T_Trick.xml` ([`retail_trick_anim`]; mods: `skater_clips["trick.<id name>"]`).
- **Fade** (`Update`, [`present_fade`]): while [`NpcFade::alpha`] is below 1 every mesh under the NPC (body and board) draws with a per-NPC blended copy of its material; at 1 the shared materials come back and the copies are freed, so a solid NPC costs what it did before.
- **Audio**: `NpcSkaterAudio` with a lite state from the cursor (position, velocity, air, ground trick as a grind) and the character's voice; #32's host picks the one audible NPC by retail's rule.
- **Events** for engine systems and the planned `sdk.living_world`: [`NpcSkaterEvent`].

Multiplayer: nothing here decides a spawn; the entity is rebuilt from a `SpawnRecord`, the tick and [`BranchRecord`]s alone (`Decider::Mirror` on a client).

### Retail blend seconds

Transition time (s) into a phase's clip: a `PlayAnimation` without a `time` attribute transitions over the literal at `0x82099280` = 0.2 s ([code], read in `graph_host::motion_nodes::transition`), and the stock motion graph's looping idle states the puppet clips stand for enter with 0.2 s ([data] `air.xml` `B_AIR_CYC` time 0.2, `offboard.xml` stand `BLENDSEC` 0.2, `T_handplantair.xml` `IA_IDLE_N_N_0_CYC` time 0.2).

### Retail trick anim

The stock trick animation base a recorded trick slot plays (`<base>_G` on the ground, `<base>_A` in the air), from the trick's `EScorableID` ([code] node ext +0x24, see `ReplayLine::node_trick`; names `skate_core::scoring::catalog`, table 0x820862A8). Pairs are [data] `MotionGraphIncludes/Tricks/Tricks.xml` (`TRICK_NAME` -> `ANIM_NAME`): the flips and shuvits by name, kickflip / heelflip and their nollie forms use `B_<NAME>_IN`; numbered, late and underflip variants use their base flip; a grab or any other air trick leaves the ground with the ollie (`B_OLLIE`, retail grabs are performed out of an ollie). Ground-only tricks (manuals, powerslides, reverts, grinds, slides; catalog classes 0 and 1) and handplants (class 6) have none and keep the phase clip.

### Puppet layer clip

The clip a remembered phase shows and its transition time (`older` = the phase before it): a trick span with a trick animation plays `<base>_G` then `<base>_A` from the ground (0.05 s in), or `<base>_A` alone when it starts in the air (0.1 s in); the air after an air trick keeps the clip (one layer); everything else the phase's puppet clip with its blend time. A clip that does not evaluate (`evaluates` false) falls back to the shipped phase pick; trick names go through `stock` (authored tree name -> playable clip, [`stock_tree_leaf`]).

### Failed tick leaves queries
Problem (2026-10-29 session, DownTown): after one NPC's physics tick failed (an unsupported stock motion graph condition, `IsArmChannelPlaying`, in the off-board graph), every NPC skater flipped between the physics and replay tiers each tick and jittered (`physics error, back to replay: Wheel queries were started twice without result publication`, 623 times). The failed tick had started its wheel and probe query batches before the animation tick, and a failed body goes back to the pool for reuse; the new-actor reset kept the unpublished batches, so every skater handed that body failed at its first step and returned it to the pool again.
Change: `GamePhysics::abandon_context_queries` (riding outputs `abandon_queries`, probes `abandon`) runs when a pooled body becomes a new NPC (`npc_sim::spawn_sim`), so a failed tick stays confined to the NPC it happened to. The two conditions the stock motion graph still lacked are ported as well (both in the drop-in: `TryToLeave` and `OutOfDropIn`; the player hit the first on foot 2026-10-29 and the game ended): `IsArmChannelPlaying` (`82BA84F0`) is true while a GestureBoth, GestureLeft, GestureRight or Shove channel exists (keys `830BE040` / `830BE084` / `830BFCC0` / `830BE010`, names read from their static initialisers and the image strings); `IsDropInWithoutTransition` (`82BA4268`) reads grinds bundle byte 325, the tipslide state byte 98 that `82D420A0` sets when it places the skater straight into the drop-in (FillOut `82D42788`). Test `stock_motion_graph_has_no_unsupported_operations` (data-gated) keeps the stock graph free of unsupported operations. Open: retail's gesture hand selection (`82BAB258`) checks only the Shove and WipeoutPushOff channels; ours also checks RetrieveBoard.

### Score clips

`graph_host::motion::stock_score_clips`: each `ScoringGrabs` (`grabName` and its `up` / `left` / `down` / `right`
names, with that direction) or `ScoringTrick` (`trick`) behaviour of the loaded motion graph, keyed by the lower-case
name, gives the trees of the state that owns it: the state's own `PlayAnimation` (grinds, slides); when that state is
a `Cyc` / `Cycle`, the `Into` / `Out` siblings too (air grabs, board adjust grabs); a state without its own tree uses
its `<X>Into` / `<X>Cyc` / `<X>Out` children (ground grabs). First in graph order wins, except that a transition state
(`Into` / `Out`) names a score only when no other state does: a grind's grab `Into` also scores the grind
(`T_GrindGrabs.xml`), which otherwise gave every grind its grab's INTO clip.

### NPC grab scores

`npc_skaters::RETAIL_GRAB_SCORES` / `retail_grab_score`: the grab score the stock graphs reach for each category 4
schedule (see the 2026-10-29 paragraph above), ground grabs keeping their own score on the ground;
`scored_trick_clip` turns it into the clip sequence through the index and `stock_tree_leaf_at` (blend space / phase
blend -> nearest child by its clip's attribute value; sub-tree children by rank).

### NPC grind height

Problem: the grabs port step 3 played every grind and slide on its LOW clip (the board at the bottom of the `DISTTOCOG` range, a deep crouch). Retail rests on the HI clips.

Retail [data + our port of the TU3 behaviour, research b700, values checked 2026-10-30]: `Grinds/Grinds.xml` runs `ControlGrindCrouch` every tick, which eases `disttocog` toward `min(bound(1 - Crouch, min, max), bound(1 - physical grind crouch, min, max))` at `grind_disttocog_speed` (`skate-core/src/animation/grind_control.rs` `crouch`). `anim_motion/grind_height` in the setup data: `min_grind_disttocog` 0.6, `max_grind_disttocog` 0.85, speed 0.6 /s. The AI posts no Crouch during a trick (`ai_signals.rs`, `in_trick` gate), so with no crouch the value rests at 0.85: `G_5050_FS_HI_0_CYC` (clip DISTTOCOG 0.859; LOW is 0.684).

Ours: `scored_trick_clip` picks the grind tree's child at the rest height, read from the loaded grind settings (`MotionHost::grind_rest_disttocog`, the setup data's `max_grind_disttocog`), so a data change moves both tiers. Mod key `living_world.npc_grind_disttocog` (0..1; unset = the data value), reset on mod disable. The entry ease from the landing height is not modelled (the puppet has no landing height; the clip starts at the rest). Twist stays 0 (ours: retail takes the physical body angle at entry, which a replay puppet does not have).

Open: the AI's physical grind crouch (`crouch_132`) is assumed 0; whether any grind schedule posts Crouch or GrindBalanceX is not checked.

Test: `npc_tests::living_world_npc_grab_and_grind_slots_play_their_scored_trees` (data-gated): fs_50_50 -> `G_5050_FS_HI_0_CYC`; `world_tuning` valid_patch checks for the key.

Research b703 (scoring data of the disc and all 10 DLC vaults): no grind schedule posts Crouch, GrindBalanceX or PhysGrindStabilityNudge (12 of the 54 grind records post only PhysGrindUpDown and Left / RightGroundGrab for the whole slot), so the intent side rests at the max and the twist keeps its entry value. The physical side (`physics/grind/advance.rs`: |deck z . grind normal| x 0.4) only lowers it with more than about 22 degrees of deck tilt; the puppet has no deck, so that part is not modelled.

### NPC manuals

Problem: an NPC manual (category 8, recorded as a GroundTrick phase) had no scored clip and fell back to the GroundTrick stand-in, a crouched 50-50 grind (`G_5050_FS_LOW_0_CYC`).

Retail [data, stock graph; research b703]: `ground.xml` Manual -> `Manual/NoseManual.xml`: `B_NOSE_MANUAL_INTO` (0.1 s), then rolling forwards (`PhysicsSpeedCompare alongSkateZ greater 0.5`) `B_NOSE_MANUAL` (0.3 s) with `SetManualAngle`; `Manual/TailManual.xml`: `B_TAIL_MANUAL` with `SetManualAngle` only rolling backwards (`less -0.5`), else `B_TAIL_MANUAL_LOW` (0.3 s). The AI holds `Manual` = +/- 0.5 (`ai_signals` `manual_value`, + nose, - tail); `anim_motion/manual` `manual_balance` maps 0.5 to 0, so `MANUAL_ANGLE` rests at 0. `B_NOSE_MANUAL` blends over `MANUAL_ANGLE`; `B_NOSE_MANUAL_INTO` and `B_TAIL_MANUAL_LOW` over `DISTTOCOG`, which riding's Crouching keeps at its top with no crouch.

Ours: `scored_trick_clip` category 8: nosemanual -> Into + cycle as a sequence (blend 0.1 s), tailmanual -> `B_TAIL_MANUAL_LOW` (0.3 s), each through the leaf picker with `MANUAL_ANGLE` from the loaded balance graph (`MotionHost::manual_rest_angle`, so a data change reaches NPCs) and `DISTTOCOG` at the top. Rolling forwards is assumed (an NPC riding fakie would take the other branch: not modelled); the manual deck-pitch crouch (`MaxCrouchFromDeckAngle`) is not modelled; the manual's OUT is left to the next phase's blend.

Test: the same data-gated test: nosemanual -> `M_NOSEIDLE_N_0_INTO+...`, tailmanual a single clip, neither a 50-50.

### NPC grab tweaks

Problem: the grabs port step 3 showed an air grab's `Cyc` as one clip, the blend space vertex nearest the score's unit direction, so a grab snapped to its end pose and never eased.

Retail [data, stock graph; research b700 / b704, read 2026-10-30]: an air grab's `Cyc` state (`GrabsTweaks/T_FSBSGrab.xml`, `T_MuteStaleGrab.xml`, `DBLGrab.xml`) runs `FilterMotionGraphIntent` on TweakX and TweakY (`blend 0.116`, `blendOut 0.133`, `clampVel 0.1`, per graph update), then `TweakProject` (FS / BS: abs(x) + abs(y) clamped to 1) or a raw `SetBlend` (mute, stale, DBL) into the tree's `tweak_x` / `tweak_y`. The input is the AI's schedule (`sub_82470ED0`, `slot_signals`), copied AG -> MG with a 0 default and TweakX negated when mirrored (`ActionGraphIncludes/air.xml` Grabbing, `mirrorFilter="negate"`). Ground grabs have no tweak.

Ours: `graph_host::motion::stock_score_clips` keeps each score's Cyc filter settings and projection (`ScoreClips::tweak`, read from the loaded graph, so a graph mod changes NPC easing like the player's). `npc_skaters::grab_tree` gives an air grab layer its Cyc tree once the puppet's sequence reaches the last part; `grab_tweak` re-runs the player's filter (`skate_core::animation::intent_filter::State`) from Begin over every 60 Hz tick since the Cyc entry with the schedule value at that slot tick, so there is no stored state (a scrub or a late-joining peer gets the same pose). The slot tick comes from the cursor's new `trick_since` (the frame the span's START_TRICK was crossed, on `LineCursor` and each `PhaseEntry`). `present_pose` negates x for a mirrored layer and builds the tree with `graph_host::motion::named_tree_commands_into` (one reused tree per name, attributes set each frame); the pose plays the tree in place of the leaf clip.

Open: the Cyc entry is taken as the Into's end (retail's AG / MG entry delay is not measured); the Into -> Cyc step is a cut (retail blends 0.1 s).

Tests: `npc_tests::an_npc_grab_tweak_eases_from_the_cyc_entry_like_the_player` (b704 sample values, schedule windows, the projection); data-gated `living_world_npc_score_clips_index_the_stock_graph` (filter settings from the graph, every eased Cyc holds the leaf the puppet picks, the tweak moves the BS pose). The NPC goldens are unchanged (`npc_present_pose_and_advance_match_golden`, `npc_present_pose_posed_matches_golden`).

### NPC grab playback speed

Problem: the puppet played every sequence part at its clip's own rate, but retail's grab `Into` states set `playBackSpeed` (FS / BS 2.0, mute / stale 1.25, DBL 3.0; Out FS / BS / DBL 2.0), so NPC grabs reached their Cyc late.

Retail [data, stock graph; our port `skate-core/src/animation/clip_clock.rs` `set_speed`: length = (frames - 1) / (fps x speed); research b708]. Ours: `ScoreClips::into_speed` / `out_speed` from the state's `PlayAnimation playBackSpeed` (read from the loaded graph); a sequence part carries it as `NAME*speed` (`npc_skaters::part_speed`, `with_speed`), and `sequence_part`, `sequence_attributes` (stance events) and the grab tree's Cyc entry all scale the part's length and time by it.

Test: `npc_tests::a_grab_landed_open_holds_or_continues_and_never_takes_off_again` (parsing, a 2x part's event window), data-gated `living_world_npc_score_clips_index_the_stock_graph` and `living_world_npc_grab_and_grind_slots_play_their_scored_trees` (`GR_GRAB_N_FS_0_INTO*2+...`).

### NPC grab touchdown

Problem: a grab span still open when the NPC touched down started a new GroundTrick layer whose clip was the takeoff sequence (`B_OLLIE_G+Into+Cyc`), so the NPC replayed its ollie on landing.

Retail [data, stock graph; research b709 / b710, verified `T_FSBSGrab.xml` Cyc transitions and our AG-before-MG order `skater_animation.rs`]: the FS / BS / DBL Cyc's high-priority transitions are WipeOut, then the ground grab hold `OnGround...Grabbing.<X>.<X>Cyc`, then `Land.GrabLanding.<X>`. The AG posts `GroundFSGrab` / `GroundBSGrab` on the first ground tick before the MG picks, so a slot still open at touchdown plays the ground hold (`GroundGrabs/FSGrab.xml` `BLEND_FSGRAB`, time 0.2). Mute / stale have neither (normal landing).

Ours: `puppet_layer_clip` marks a GroundTrick right after the same span's AirTrick as landed; `scored_trick_clip` (`landed`) gives the FS / BS / DBL family's ground grab Cyc with its `cyc_seconds` blend; otherwise the air clip continues in one layer. Never a second takeoff.

Grab landing [data, `Landing.xml` `Land.GrabLanding.<X>`; ported 2026-10-30]: a span that ended before touchdown with the Cyc still active (no Out yet, past the Into) lands into `L_FSGRAB` / `L_BSGRAB` / `L_DBLGRAB` (time 0.1); `Land` leaves for OnBoard on `WillExpire 0.1`. Ours: `ScoreClips::landing` (tree, blend, expire, read from the Cyc's transition target in the loaded graph); `npc_puppet_layers_into` finds a touchdown layer's grab span in the phase history (so the landing outlives the grab layer's blend-out), plays the landing in its place, then blends the touchdown layer back in with the time it came back with.

WipeOut (Cyc: off air or trucks / deck in contact and `HasTweak greaterEqual 0.4`) is not ported for the replay puppet: the survey `npc_tests::living_world_npc_grab_touchdowns_on_the_shipped_lines` (every ambient line of the three districts through the puppet's cursor and timing) finds 0 of 645 air grab touchdowns at or above 0.4 (612 released before touchdown, 17 during the Into, 9 grab landings, 6 ground holds). Simulated NPCs run the real graph and bail through it. The ground hold's own Out (`B_FSGRAB_OUT`) is left to the next phase's blend.

### NPC grab release

Problem: the puppet held an air grab's Cyc until landing; retail plays the grab's Out.

Retail [data, stock graph; research b708]: when the slot ends (END_TRICK) the grab intent goes and the Cyc's filters ease out (`blendOut`, no input); the Cyc leaves for `Out` on the update after the filtered tweak drops below the transition's level (`HasTweak less="0.2"`, the x / y magnitude, for FS / BS / DBL; `HasFilteredMotionGraphIntent TweakY abs less="0.2"` for mute / stale). Out blends in over its `time` (0.1 s) at its `playBackSpeed`.

Ours: `ScoreClips::tweak.release` and `out_seconds` from the loaded graph; `grab_tweak` / `grab_release_tick` stop the schedule input at the span's end (the first newer phase outside it) and find the release tick; `npc_puppet_layers_into` inserts the Out as its own layer after the grab, when it began before the next layer (a later landing blends over it). Pure function of the cursor history and the schedule.

After the Out: the Out leaves for InAir on its `WillExpire` (0.01 s before its clip ends, `ScoreClips::out_expire` from the graph) and the puppet blends in the Air phase clip (the puppet's own Air clip and blend). End to end on the shipped lines: `living_world_npc_grab_landings_and_air_after_out_show_on_the_shipped_lines` (data-gated, loads a skater runtime).

Open: blend vs blendOut after the slot end is inferred (blendOut: the AG copy is gone); `B_MUTE_OUT`'s `Magnitude` is taken as 0.

Test: `npc_tests::an_npc_grab_releases_into_its_out_once_the_tweak_eases_below_the_level` (eases out after the slot, release tick, no rule = no Out, mute / stale TweakY only); data-gated checks of the parsed rules in `living_world_npc_score_clips_index_the_stock_graph`.

### Trick air sequence

The air part of a trick animation base: `<base>_A`, and for the kickflip family (`T_Kickflip.xml`: `LeftGround` plays `$ANIM_NAME$_A`, then as sequences `$ANIM_CYC_NAME$1..3` per extra flip and `$ANIM_OUT_NAME$<n>`) `B_<FLIP>_IN_A`, `B_<FLIP>_CYC1..n`, `B_<FLIP>_OUT<n>` with n the trick's flip count (`kickflip2` = 2; 1 without a digit). Parts are joined with `+`: the puppet plays them back to back (`transType="sequence"`, no blend).

### NPC stance flags

An NPC skater's stance bits (retail SkaterAnim flags bits 31 / 30 and the relative stance, [`StanceFlags`]): the natural stance's start bits, toggled by the stance events of the clip its newest layer plays, like retail's `82593230` -> `82B98980` for every actor. Each layer keeps the bits current when it began (retail bakes them into the tree's bind pose at construction). Simulation state, updated on the fixed step from the cursor and the clip data only ([`track_stance`]), so a client derives the host's bits; plain data for a snapshot.

### NPC fade

Spawn fade in (retail `sub_825926F8`: 0 to 1 over the first second), then the leave fade (retail `sub_8246EA90` / `sub_8245A9B8`, removal below opacity 0.2): only once the cursor has ended at a dead end (no line starts within the chain radius), holding the last pose while fading. The clock is the spawn-relative frame (`target`), which keeps running after the cursor stops, so clients derive the same alpha from the same spawn record and tick.

### NPC skaters vs props

NPC skaters against dynamic props (DMOs), doc 26 fix 19. Retail NPC skaters are full skaters (`AIController` on the player's skater physics, [code] notes `npc-skaters-re.md` section 0), so their board and body hit a DMO like the player's: the prop step pushes it by the prop's own tuning (`props` domain: push mass, transfer, board / body caps). Our NPC is a replay puppet, so it does not slow down or bail; the prop is pushed out of its line. A mod may switch it off (`sdk.world.set_tuning("living_world", {npc_skater_props = {enabled = false}})`).

### NPC prop volumes

The prop-step push volumes of one NPC: the proxy's body capsule and its board (as a capsule along the deck), at the recorded velocity, in the same places as [`proxy`]. Both count as a board hit (the prop's `board_push_speed` cap): the lower body-bump cap is for the player's walking body, which the prop's triangles stop; a puppet is not stopped, and in retail the whole skater rides into the prop at riding speed. Actor id = the proxy's solid id (stable, the prop's authority owner).

### Simulated NPC draw root

A simulated NPC skater is drawn from its own physics pose. That pose is relative to the skater's animation root (joints within ~2 m of 0, NPC_SKATER_DRAWN), so the puppet root sits at the animation root in the world; with the root at the origin every simulated skater was drawn at the map origin. A pose already in world space (root joint at the skater) keeps the identity root.

### Flags root turn

The puppet root's extra turn for the stance bits of the newest layer. The regular bind pose ([`bind_pose_tail`]) draws the board turned round (`BOARD_BACKWARDS`, nose along root -Z) and the body mirrored; retail's physical board follows the animated board (`publish_deck_angles` with `board_flipped = bit31 ^ mirror`, false for both natural stances), so a regular character's frame is half a turn from its board's. The replay root is the frame the goofy puppet rides nose first with (the facing rule and the fakie rule work on that board axis), so a mirrored puppet turns its root half a turn: the board keeps its world heading and the left foot leads. The turn follows the mirror bit alone: a trick clip that toggles only `animboardbackward` (the board turned under the body) ends with the board turned in the clip, and the next layer's `BOARD_BACKWARDS` keeps it turned without moving the body.

### Puppet pose in flags

The puppet pose with each layer in the stance bits it was built with (`flags[i]` for `layers[i]`; retail bakes the bits into a tree's bind pose at construction, so a transition blends two trees each closed with its own tail). While every layer shares the bits, the layers blend first and one tail closes the result (with no bits set: the commands every NPC drew before the stance port; with the regular bits: the natural stance port); once a newer layer's bits differ, every layer is closed with its own tail before it blends in. The channel tree (fakie) is closed with the newest layer's bits, like the player's channel tree built with the current flags.

### NPC backwards line

The `NPC_SKATER_BACKWARDS` line for one NPC sample, `None` when it is not riding backwards (`skate_core::living_world::replay::facing_check`: drawn heading more than 135 deg from the velocity yaw at 1 m/s or more; a diagnostic threshold). `recorded_fakie` says the line's own retail path frame opposes travel there (the recorder rode fakie: retail's target frame does too); `flip` is retail's latched switch / fakie flip (controller `+927`); `facing_flipped` is the fix 16 mod option's turn.

## Code reference: skate-core living_world/replay.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/replay.rs`; each comment there links its section here.

### Line cursor model

NPC skaters, replay tier: the line cursor (doc 26, milestone 3).

Retail runs every NPC skater as a full skater steered along a recorded human line (`AIController` + `PathController`, ctor `sub_824685F0`, vtable `0x822FBA1C`). The replay tier is the engine's cheap stand-in until the simulated tier exists: the NPC follows its line kinematically, node by node, at the recording rate (60 Hz, [data]: node distance / frames = per-tick displacement over 389k nodes). What is ported 1:1 from the code is the **branch choice** at a node that carries a branch group ([code] `sub_8246BEE0`, chooser `sub_8246C1C8`, candidate score `sub_8246C230`):

- candidates: index 0 = stay on the current line at the current node, then every branch target line that exists, passes `sub_82458860` (the AI's path-kind filter and the 0x200 object overlap; no in-use test, b715, caller lr 8246C0AC inside `sub_8246BEE0`; corrected 2026-11-08) and is valid (`sub_82456970`);
- each candidate is rejected (score -1, never chosen) when the line has no node after the candidate node (`sub_8246C3C0`), when the direction from the skater to that next node is 50 deg or more off the skater's forward (0.872665 rad, `0x822F91B0`), or when a node within one of the candidate node is airborne (flag 0x04) or carries an event (`sub_8246C4F8`; skipped at node 0); otherwise
- score = |angle| x 572.958 (`0x822F9354`, tenths of a degree) + 400 x speed (`0x822F95F0` = -400, the same for every candidate) + offline: 1024 x the other AI skaters on the same line within 5 nodes (`sub_82456A38`) + min(30 x the nearest distance from the player to every second node from the candidate node on, 1500) (`sub_8246C5D8`, `0x820D4924` = 30) + 1000 when the line's flags have bits 0, 1 and 2 all set + the skill term (|path skill - preferred| x 250 + 100 when both are set and differ);
- the lowest score wins, the first on ties; when every candidate is rejected or the stay wins, nothing changes. No random draw is involved. The branch record's f32 is not read there.
- a taken branch starts at the target node nearest to the skater among `target - 3 ..= target` (`sub_82455BB0`).

Replay-tier simplifications (documented in doc 26; the simulated tier replaces them): positions, orientation and timing come straight from the recording instead of the steering (`AIPhysicsInput`); the branch is evaluated once when the cursor reaches the group's node (retail evaluates while the controller sits on it); the obstacle-list rejection of `sub_8246C4F8` is not modelled (no obstacles yet).

**Line end** ([code] `sub_8246D3C0` -> `sub_8246C7F8` -> `sub_82458968`, fix 9): no line on the disc has a branch group on its last node; instead, when the controller reaches the last node, retail looks for unused lines whose start node is within 4 m of the end ([data] `ai_skater` tunable) and picks one with the branch chooser ([`choose_next_line`]), so an ambient skater keeps riding from line to line until the population's distance cull (120 m from the player, `sub_8245D520`) removes it. [data] 742 of the 760 DownTown lines have another line's start within 4 m of their end. Only when none is near (retail would steer the full skater to the nearest start at any distance) does the replay tier report [`CursorEvent::Finished`].

**Presentation** (fix 14): a branch or chain moves the line under the skater (a chain up to the 4 m radius). Retail's full skater steers onto the new line and never jumps; the replay tier keeps where the skater was drawn at the switch ([`LineSwitch`]) and decays that offset (position and orientation) with the graph transition curve over [`ChainConfig::blend_seconds`]. Samples carry the render sub-frame ([`ReplaySample::sub_frame`]) so clip and blend times move between ticks, and [`LineCursor::render_sample`] interpolates from the cursor one tick back with the recorded branch decisions (the player's previous-to-current scheme), never guessing a branch.

**Facing** (fix 23 corrected): retail's AI target frame is the recorded skater frame slerped between nodes, turned 180 deg while the latched flip ([`LineCursor::flip`], controller `+927`) is set; the flip is latched only on entering riding and held across switches ([`FacingRule::RidingEntry`]). Retail's full skater steers its body toward that target ([`steer_input`], 2 / 10 deg) and shows fakie / switch with its own clips and stance mirror. The cursor always latches the flip; the puppet draws the retail target only under the default `riding_entry` rule; the cursor runs retail's riding-fakie rule on the drawn body ([`LineCursor::fakie`]) and the puppet overlays the stock fakie channel like retail, so a body against its travel is drawn riding fakie. The fix 23 per-node fold ([`FacingRule::PerNode`], not retail) stays a mod option.

Multiplayer: between branches the cursor is a pure function of (line, start node, frames); a branch decision is a record ([`BranchRecord`]) a client mirrors ([`LineCursor::step`] with a mirroring decider), so a client reproduces the host's NPC from its spawn record, the frame count and the branch records alone.

### Facing rule riding entry

Retail (default): the target frame is the recorded SKATER frame slerped between the two nodes ([code] controller `+208`, `sub_8246D560` -> `sub_8245A088` -> `sub_82454CD0` -> `sub_82454B28`), turned 180 deg about its up axis while the cursor's latched [`LineCursor::flip`] is set ([code] `sub_8246B358`: rows 0 and 2 of `+208` negated while controller `+927`). The flip is latched only at takeoff ([code] `sub_8246A700`, rising edge of `+928`, the airborne byte) and held across landings, branches and chains. The default (`riding_entry`): a body drawn against its travel on the ground is drawn riding fakie ([`LineCursor::fakie`], the stock fakie channel), as retail draws it.

### Switch blend seconds

Seconds over which the drawn root (position and orientation) moves from where it was on the old line onto the new one after a branch or a chain (`LineSwitch`). Retail has no such value: its full skater never jumps, it steers onto the new line (`sub_8246D3C0` / `sub_8246C7F8` only store the new line and node, `AIPhysicsInput` steers). The replay tier stands in with `SWITCH_BLEND_SECONDS`. 0 = cut.

NOT RETAIL YET (switch spins): retail has no turn rate to port here. The AI steer of `sub_82471188` (on board) and `sub_82471070` (off board) is written to three named input channels of the normal character, `Turn`, `BodySpin` and `KickTurn` ([code] slots `0x830BFD74` / `0x830BE600` / `0x830BE1E0`, names from static inits `sub_82F84BE0` / `sub_82F84A30` / `sub_82F84BC8`), so the body yaw comes out of the player chain: steering tilt `sub_82D92440` -> truck targets `sub_82C040F0` / `sub_82C0B9C0` -> the rigid-body wheel solve. It is emergent, not a tunable; the faithful fix is the simulated NPC tier. With `FacingRule::RidingEntry` the puppet turns a switch's facing change within this blend (128 spins after a switch on the exported lines, 29 under `FacingRule::PerNode`).

### Keep facing option

Mod option, not retail (default `false`): carry the drawn facing across a branch or chain by riding the new line turned 180 deg (`LineCursor::facing_flipped`, the fix 16 rule). Retail's facing state is the latched flip (`FacingRule::RidingEntry`, controller `+927`), which a switch leaves alone (a switch stores only line and node, [code] `sub_8246C7F8` `+592/+600/+816`, `sub_8246BEE0`); it never compares the old and new line at a switch. Kept on, this turn carries on across later lines and the skater can ride a forward-recorded line backwards.

### Node airborne

Whether a node counts as airborne for the flip latch. Retail latches on the rising edge of controller `+928` = the skater state object's `+438`, which [code] `sub_82DB6EC0` (store at 0x82DB7654) sets while the player state id is in `200..300` and the wheel count is 0: in the air on the board (b119, b354; ground, grinds, wipeout and off board give 0). The replay tier has no player state, so this maps it to the recorded node flags. [data, b355] AIRBORNE marks free flight on the disc lines (median -12.5 m/s^2 over 12,049 runs; every grind start is on the ground; a grind or manual ending in the air ends inside the run); OFF_BOARD must win (756 nodes carry both, retail's `+438` is 0 off the board). TU3 has no live recorder, so what the dev recorder tested is unknown.

### Per-node facing rule

NOT RETAIL (fix 23 rule, kept as the `FacingRule::PerNode` mod option; the default is `FacingRule::RidingEntry`). Whether the replay draws node `i`'s recorded skater frame turned 180 deg about its up axis: its forward lies more than 90 deg (yaw) from the forward of the retail path frame (`path_frame`, the riding direction retail's controller steers by). [data] On the exported lines (3 districts, 142,042 moving nodes) the recorded skater frame and the path frame agree within 30 deg on 111,240 nodes and are about 180 deg apart on 24,251 (switch stance: the body turned round while the board rolls nose first); the skater frame faces against the travel on 28,102 nodes, the path frame on 11,598 (fakie). Airborne nodes keep the turn of the last grounded node before them: a shove-it spins the board about its up axis in the air, and only its landing node carries the final `m_IsBoardFlipped` state.

Retail does not do this: the review in doc 26 "Fix 23 corrected" found that retail's AI target is the recorded skater frame slerped between nodes, turned as a whole by one latched flip (controller `+927`, `FacingRule::RidingEntry`), not folded per node against the path frame. The node holds no switch or stance flag (flags bits 0..3 = board flipped, crouched, airborne, off board [data]); `sub_8246B1F8` folds the body-board yaw into +-90 deg for a separate input (`+856/+860`). Retail plays switch with its own clips and stance mirroring; the replay puppet has one stance, so this rule draws a switch rider as riding forward in the character's stance.

### Choose next line

The next line at the end of `current` (retail [code] `sub_8246D3C0`: once the controller's node is the last one it calls `sub_8246C7F8` with the end position):
- candidates (`sub_82458968` mode 1): every loaded line that is not in use (`sub_82458860`; the skater's own line counts as in use) whose **start node** (node 0) lies within `ctx.chain.radius` of the end position, at most `ctx.chain.max_candidates`, in id order (retail walks its path hash map; the order only matters for ties and the cap);
- one candidate is taken as it is; several go through the branch chooser `sub_8246C1C8` (score `sub_8246C230` at node 0, see [`branch_score`]): lowest score wins, the first on ties, and the first candidate when every one is rejected (the chooser starts at index 0);
- the new line starts at node 0.

`None` = no start node within the radius. Retail then falls back to the nearest start node at any distance and the full skater steers to it (`sub_82458968`, the 6 m valid/invalid rule); the replay tier cannot ride that gap, so the caller ends the line (and the NPC fades out). Line validity per character (`sub_82456970`) is not modelled here, like in [`choose_branch`].

### Latched flip

Retail's latched switch / fakie flip (controller `+927`, [`FacingRule::RidingEntry`]): the drawn skater is the recorded skater frame turned 180 deg about up while set. Latched by [`flip_test`] only at takeoff ([`node_airborne`] rising, [code] `sub_8246A700`; also a spawn in the air, and a spawn riding as a stand-in: retail holds 0 there until the first takeoff), held across landings, branches and chains. A pure function of the lines, the spawn and the branch records (deterministic; a snapshot carries it as a plain bool).

### Riding fakie bit

Retail's riding-fakie bit of the drawn body (SkaterAnim flags `0x20000000`, set by the motion graph's `UpdateRidingFakie82BB2330` [code], [`crate::animation::riding_fakie`]): on the ground, outside a trick, the travel runs against the board's forward (`dot(velocity, board axis) < -0.5`) above the high speed, or above the low speed for longer than the slow time; cleared in the air and off the board, held during a trick, never set in the first `after_teleport_seconds` after spawn. The board axis is the drawn root's +Z: retail reads the effective root (`GetEffectiveRoot82BE3650`, root Z negated iff the mirror bit, [`crate::living_world::stance::StanceFlags::fakie_board_axis`]), which for the puppet root (drawn frame turned half a turn iff mirrored) is the drawn frame for any stance bits; the board bit 31 (a shove-it) is not an input. Updated every step after the facing rule, so it describes the body as drawn; a pure function of the lines, the spawn and the branch records.

### Cursor spawn

Spawn on a line at a node (retail spawns at node 0, `sub_8245DA78`).

Retail places the character with the node world frame ([code] `sub_8245C548` -> `sub_82453C58` -> `sub_8245DA78`, [`node_world_frame`]); when the spawn node is airborne, the first controller update sees `+928` rise and latches the flip against that frame ([`flip_test`]): set when the recorded skater frame faces away from the path frame. A riding spawn latches the same test as a stand-in (retail: 0 until the first takeoff).

### Current trick and phases

The phases this cursor entered, newest first, with the 60 Hz frames since each began (the puppet's nested crossfade and trick clips; fix 21). A pure function of the cursor. The trick of the open trick span (`-1` outside a span or without one): the recorded trick, or the one the trick choice started ([`super::npc_tricks`]).

### Begin switch

At a branch or chain from the current node of `old` to `to_node` of `next`: keep where the skater is drawn now (including a switch blend still running) so the root moves onto the new line instead of jumping. Deterministic: from the lines and the record alone.

Facing, retail: nothing else. The switch stores line and node only (`sub_8246C7F8`, `sub_8246BEE0`), so the latched [`LineCursor::flip`] (`+927`) is held and the new line's recorded skater frame is drawn turned by it; the root blend above stands in for the full skater steering onto it. With [`LineCursor::keep_facing`] (mod option, not retail; fix 16) the skater keeps the way it faces. When the new line's recorded skater faces more than 90 deg away from the drawn one about +Y (its recorder rode the other way round there: fakie against forward), the cursor flips [`LineCursor::facing_flipped`] so the line is ridden turned 180 deg about the skater's up axis, the same turn retail applies to a node's board frame when it is flagged `m_IsBoardFlipped` ([code] `sub_82453A58`, `sub_824734A8`: negate the frame's X and Z rows).

### Flip only in the air

[code] `sub_8246ACF0` calls the flip-applying target steer (`sub_8246B358`) only while the state object's `+438` (airborne, b354) is set; otherwise it sends `sub_82471070` with controller `+824` and the flip is not applied. Off-board nodes therefore show the recorded frame as recorded (walking forward). On the ground the puppet keeps the flip as a stand-in (retail's ground target is the path frame, b353; no ground physics in the replay tier).

## Code reference: skate-data aipath.rs

Detail moved out of the code comments of `crates/skate-data/src/aipath.rs`; each comment there links its section here.

### AIPATHDATA layout

Recorded NPC skater lines: the retail AIPATHDATA object (RenderWare type
0x00EB0014) and the living-world skater path pack the setup export writes.

Retail free roam streams these objects with the district `cSim_*.xsf` tiles;
`Sk8::AIPath::ThePathManager` keeps every path by its 16-byte id, and the AI
skater manager (`sub_8245C548`, spawn) picks lines whose start node lies in
its spawn ring. A path that crosses several tiles is stored, byte-identical,
in every tile it touches: the disc holds 3,891 path copies of 1,691 unique
paths (DownTown, University, Industrial; the park districts have none).

Layout (big-endian; every `m_p*` offset is relative to the start of the
struct that contains it). Field names follow the Skate 2 symbols as
documented by DumbadsSkate3ModdingTools (Ethanw05; credits there to SunJay,
Dumbad, RenderWareGavin, Tuukkas); every field was re-checked against the
user's disc ([data], the local NPC skaters research note, section 3):

```text
AIPATHDATA  +0 u32 path count, +4 u32 offset of the path array (0x10), +8 8 pad
tAIPath (96 B)
  +0x00 vec4 bbox min, +0x10 vec4 bbox max (w = 1)
  +0x20 16 B m_ID (byte 0 = 0 for ambient paths, bytes 1..5 district tag)
  +0x30 m_pNodes, +0x34 node count, +0x38 m_pExtData (0 = none)
  +0x3C m_pBranchGroup, +0x40 group count
  +0x44 u32 m_BitFlags, +0x48 u64 m_AllowedSkaters, +0x50 i32 m_SkillLevel,
  +0x54 3 x i32 extra (0)
tAIPathNode (44 B)
  +0x00 vec3 position, +0x0C vec3 direction (displacement since the previous
  node), +0x18 board orientation, +0x1C skater orientation (4 x u8, biased
  quaternion), +0x20 m_pExtData (node-relative, 0 = none), +0x24 u8 frames
  since the previous node (60 Hz), +0x25 u8 width left, +0x26 u8 width right
  (255 = unbounded), +0x27 u8 event type, +0x28 u8 flags, +0x29 3 B extra
tAIPathNodeExtData (40 B)
  +0x00 vec3 trajectory start position, +0x0C vec3 trajectory start
  velocity, +0x18 vec3 trajectory offset, +0x24 i16 trick index (-1 none),
  +0x26 i8 180-degree air spin count, +0x27 u8 flags
tAIPathBranchGroup (12 B): +0 offset of its branches (group-relative),
  +4 branch count, +8 node index on this path
tAIPathBranch (24 B): +0 16 B target path id, +16 u32 target node index,
  +20 f32 position along the target
```

Bytes in, no file I/O: the setup export (tools/asset_pipeline/
living_world_skaters.py) writes one pack per district, and a mod can author
extra paths in the same retail layout.

### Skater path pack layout

Pack magic. Layout (little-endian, our own format; the blobs inside stay the
retail big-endian bytes, verbatim):
```text
+0  8 B  "LWSKPTH\0"
+8  u32  version (1)
+12 u32  tile count
+16 tile table, 48 B each: u64 asset id, u32 blob offset (from file start),
    u32 blob length, 32 B tile stream name (NUL padded, e.g. cSim_-150_-150_high)
then the AIPATHDATA blobs, each 16-byte aligned
```

## Code reference: living_world/leave_fade.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/leave_fade.rs`; each comment there links its section here.

### Leave fade

How an ambient NPC skater appears and leaves: it fades in over its first second, and at the end it fades out and the skater manager removes it once the fade is far enough along, instead of popping in or vanishing on the spot.

Retail [code, TU3]:
- The skater's render object (`skater+1804`, a sub-object at `+14960` of the 15328-byte object built by `sub_82B973C8`, vtable `0x8231E170`) keeps an opacity at `+244`: vfunc 68 sets it, vfunc 76 (`sub_82B97190`) returns it. Default 1.0.
- `sub_82594488(skater, dt)` drives it from two timers: `skater+1868` (fade in, opacity = clamp(t, 0, 1)) and `skater+1872` (fade out, opacity = 1 - clamp(t, 0, 1)); both grow by dt every tick, so a fade takes 1 s. `+1872` = FLT_MAX means "not fading out" (`sub_8246EF78`).
- `sub_8246EA90` (ambient skater controller) starts the fade out (`+1872 = 0`, controller byte `+29 = 1`) once the time in its timed state is more than `duration - 1 s`: the fade ends exactly when the state does.
- The manager's per-skater check `sub_8245A9B8` removes a skater whose controller is fading (`+29`) once the opacity (vfunc 76) is below 0.2 (constant at `0x82099280`), 0.8 s into the fade. A recomp trace of six bails saw no such removal: each was re-placed at the end of the fade ([Bail fade](#bail-fade), research b715).
- `sub_825926F8` (skater spawn setup) sets `+1868 = 0`, so every spawned skater fades in from opacity 0 to 1 over 1 s. `sub_82594488` checks the fade in first: while `+1868 < 1` the opacity is the fade-in value even if a fade out was started (held at 0.5 while a component byte `+71` is set; not ported, the replay tier has no such component).

That timed state is entered on a skater component flag (`sub_8246EF78`, byte `+59`), not at a line end: at a line end retail chains to the next line (`replay::choose_next_line`, fix 9), so an ambient skater keeps riding until the 120 m population cull. The replay tier fades only at a dead end (no line starts within the chain radius): the fade starts there with the last pose held, the NPC is removed below `despawn_alpha`. Pure and deterministic (a function of the cursor frame), so a client derives the same value.

### Leave fade start

Start the fade when the cursor has ended: its line is over and no other line starts within the chain radius (`replay::choose_next_line`). Returns true on the frame it starts.

Retail (fix 9): the 1 s fade and the removal at opacity 0.2 belong to a timed state the controller enters on a skater component flag (`sub_8246EF78`, byte `+59`; leaving it cancels the fade), not to the line end: at a line end retail chains to the next line (`sub_8246C7F8`). The replay tier has no such state; its only stand-in is a dead end the replay cannot ride past, where the fade starts with the last pose held.

### Leave fade despawn

The manager removes a fading skater below the threshold (`sub_8245A9B8`). Checked once the fade in is over: retail's timed state lasts at least 1.5 s (`sub_8246EE30`), so its fade out never starts while the fade in is still below 0.5, and a skater is never removed mid fade in; our lines can be shorter, so the rule is stated explicitly.

### Bail fade

Problem: a simulated NPC skater that bailed stayed solid on the ground until its respawn popped it onto its line.

Retail [trace + code; research b715, the skate3recomp hooks NPCLATCH / NPCBAIL / NPCREMOVE, research b705/fire_trace.tsv`]: six ambient (kind 0) bails at Aletown all ran the same chain: the bail latch (`8246EF78`) rises; the bail wait (`8246EA90`) starts the fade at 4.0 s (`pc+0x1D` set, the `+1872` timer from 0); at 5.0 s (the 5.0 s ambient delay) it places the skater again (`8246E558`, lr `0x8246ECE8`) with the timer at 0.983, and the latch falls with the post-bail countdown at 8. No removal followed any bail (`8245DCE0` never ran from the manager's opacity branch, lr `0x8245AC6C`), so the reading above that the manager removes a fading skater below 0.2 does not happen for a bail: it is faded out and re-placed. The recomp runs the retail code but is not retail; this matches the bail wait's own order (fade start, then the place).

Ours: `LeaveFadeConfig::bail_alpha` (solid until `bail_fade_lead_seconds` before the respawn, then `1 - t / fade_seconds`); `npc_sim::simulate` counts ticks since the bail and sets `NpcSim::bail_alpha`, back to 1 at the respawn; `advance` multiplies the NPC's drawn alpha by it. Mod keys `skater_fade.bail_fade` (retail true) and `skater_fade.bail_fade_lead_seconds` (1.0); the fade length is `skater_fade.fade_seconds`.

Test: `replay_tests::a_bailed_skater_fades_out_over_the_last_second_before_its_respawn`.

## Code reference: skater_ghost.rs

Detail moved out of the code comments of `crates/skate-game/src/skater_ghost.rs`; each comment there links its section here.

### Skater ghost fade

Skater fade-in after every placement ("ghost"), retail sub_82594488 / sub_825926F8 (`skate_core::player::ghost`).

- **Placement** (`FixedUpdate`, after the physics tick): every completed teleport of the simulated skater (checkpoint respawn, menu / map / mod teleports, session marker return; all finish in the one reset at `physics/frame.rs`, counter `respawn::Runtime::placements`) and a newly loaded skater (spawn, map change) become a [`SkaterPlaced`] message keyed by the stable player id. Serialisable, so a networked host could send the same events; no networking here.
- **Fade** (`FixedUpdate`): [`SkaterGhosts`] holds one [`GhostFade`] per player id; a placement restarts it, then each fixed tick publishes the opacity and advances the timer by the fixed dt (time-based, any tick rate gives the same curve).
- **Draw** (`Update`): while the local player's opacity is below 1 every mesh under [`PlayerRoot`] (body, hair, clothes, board parts) draws a blended copy of its material with the alpha scaled; at 1.0 the original material handle goes back, so solid frames draw exactly the materials they drew before this feature.
- **Mods**: world tuning domain `ghost` {enabled, fade_in_seconds, hold_alpha} writes [`GhostTuning`] (retail defaults, reset on mod stop).

NOT RETAIL YET: the 0.68 hold condition (state byte 71) is undecoded and never set here; the render-side smoother sub_8278C4D8 (15/s up, 5/s down toward the published opacity) is not ported (the 1 s ramp is slower than it, so the drawn opacity follows the published one). NPC skaters do not use this path: their own spawn fade-in is `living_world::npc_skaters::NpcFade`.

## Code reference: player/ghost.rs

Detail moved out of the code comments of `crates/skate-core/src/player/ghost.rs`; each comment there links its section here.

### Ghost fade rule

Skater fade-in after every placement ("ghost"), TU3 sub_82594488.

Retail keeps a fade-in timer in seconds per skater placement component (owner +1868). The place-skater handler sub_825926F8 sets it to 0 (and counts the placement, +1864); the component constructor sub_82590DC0 starts it at 0 too, so a fresh skater fades in. Each tick sub_82594488(owner, dt) publishes the opacity (record +190 = opacity * 255, read by the character shaders as `i_params.x`) and then adds dt:
- timer < 1: opacity = clamp(timer, 0, 1), and the timer stays put while state byte 71 is set and timer > 0.68 (0x82099764),
- else a fade-out timer (+1872, constructor FLT_MAX = off) would apply; nothing in our port starts it,
- else 1.0.

**Default off.** The skater must be fully faded in when the screen shows it. Our respawns and same-map teleports have no screen cover to hide the 1 s ramp, so `GhostSettings::default()` has `enabled: false` and a placed skater draws solid at once. This follows the user's account of retail play; it differs from the earlier reading that the ramp is partly visible in retail (the trace and the RPCS3 video in [09-water.md](09-water.md)). A mod can turn the ramp on with `ghost.enabled`.

Generalised for mods: opacity = clamp(timer / fade_in_seconds, 0, 1) and the hold compares the opacity with `hold_alpha` (retail 1.0 s and 0.68, where both forms are the same). Time-based, so the curve is the same at any tick rate.

NOT RETAIL YET: who sets state byte 71 (the 0.68 hold) is undecoded; hosts pass `hold = false`.

## Code reference: skate-game/src/living_world/npc_tests.rs

Test descriptions moved out of the code comments of `crates/skate-game/src/living_world/npc_tests.rs`; each test there links its section here.

### Test fix 23 facing rules

Fix 23 (NPC skaters riding backwards), headless: every line chains onto the line starting at its end (line i -> i + 6), and lines 6..11, 18..23, ... were recorded fakie for their first 50 nodes (skater and board frame facing -x while travelling +x), so every chain joins a line the other way round from the one it left; the other lines hold a switch-stance stretch (nodes 150..250: skater frame turned round, board forward). Retail rule (fix 23 corrected, `FacingRule::RidingEntry`, mod option until the stance mirror lands): the recorded skater frame, turned only while the flip latched at a riding entry is set (clear here: every spawn node's skater frame agrees with its path frame), held across switches. With it no NPC is drawn against its travel outside a recorded fakie or switch-stance stretch once the switch blend is over; the switch-stance stretch is drawn as recorded, body against travel (the puppet has no stance mirror: NOT RETAIL YET), and `NPC_SKATER_BACKWARDS` reports the recorded fakie stretches as such. The fix 23 rule (`per_node`, the default, NOT RETAIL YET) draws the switch stance forward; the fix 16 mod option (`keep_facing`) reproduces the reported bug: forward-recorded lines ridden backwards.

### Test nested blends

Data-gated (fix 21): the recorded jump pattern of the shipped lines (rolling, a trick span opening on the ground, airborne, the span closing in the air, landing; phases shorter than the 0.2 s blend, as half of all changes on the lines are) is drawn without a pose jump: at every phase change the drawn pose continues the one showing (the new clip enters at weight 0 over the running blend), where restarting the blend from one previous clip (fix 12) jumped.

### Test trick choice

Trick choice (M5, `skate_core::living_world::npc_tricks`): every line gets an ollie slot with 80 frames of air. The host re-picks it from the character's profile table (retail mode), sends a `Trick` event per slot and keeps the record; a client cursor mirroring the branch and trick records shows the same trick. A mod's `recorded` mode keeps the line's ollie; the reset restores the profile pick.

## Code reference: skate-core/src/living_world/replay_tests.rs

Test descriptions moved out of the code comments of `crates/skate-core/src/living_world/replay_tests.rs`; each test there links its section here.

### Test keep facing option

Fix 16 rule, now a mod option (`keep_facing = true`, not retail): the skater keeps the way it faces across a chain or branch. Line 2 was recorded riding the other way round (its skater frame faces -Z while it travels +Z, fakie), and then turns forward again with a recorded revert. With the option the NPC keeps facing +Z there and the recorded revert turns it into fakie (the source of the backwards riding, see the next test). Retail default (`keep_facing = false`): the recorded facing, the path frame of the node. Deterministic: a client mirroring the record derives the same flip.

### Test retail facing over switches

Fix 23 (NPC skaters riding backwards), now under the retail rule (fix 23 corrected): a switch stores line `+592/+600` and node `+816` only ([code] `sub_8246C7F8`) and leaves the latched flip alone; spawned forward, the flip is clear on every line here. Ride: line 1 forward, chain onto line 2 (recorded fakie for 15 nodes, then a recorded revert to forward), chain onto line 3 (forward). With the retail rule the drawn skater never faces against its travel outside a recorded fakie stretch, on any frame once the switch blend is over, and it rides line 3 forward. The fix 16 option reproduces the bug: it carries the turn on and rides the forward-recorded part of line 2 and all of line 3 backwards. Host and a mirroring client agree frame by frame.

### Test per-node facing

Fix 23 rule, now the NOT RETAIL [`FacingRule::PerNode`] mod option: the drawn skater faces the path frame's riding direction per node. A recorder riding switch (skater frame turned round, board rolling nose first, flag clear) is drawn forward; a board-flipped node (board turned round, flag set, skater forward) stays forward; a recorded fakie node (path frame against travel) is drawn fakie. Airborne nodes keep the last grounded turn, so a shove-it spinning the board in the air does not turn the skater. A cursor ride over a switch stretch is never drawn against its travel.

### Test spawn latch

Retail facing rule (fix 23 corrected, [`FacingRule::RidingEntry`], a mod option until the stance mirror lands): spawn latch. The character spawns with the node world frame ([code] `sub_8245C548` -> `sub_82453C58`, the path frame on a riding node) and the flip latches on that first riding entry ([code] `sub_8246A700`: `dot(recorded skater row 2, character row 2) < 0`). A recorder riding switch at the spawn node (skater frame turned round, board forward) sets the flip; the drawn skater is the recorded frame turned 180 deg about up (rows 0 and 2 negated, [code] `sub_8246B358`). Spawned in the air, retail's first update sees the takeoff edge (b354); off board, nothing is latched; on the ground the same test is a stand-in.
