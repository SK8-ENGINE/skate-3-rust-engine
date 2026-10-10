# 26d: Living world: NPC skaters: stance and riding direction

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## NPC skaters riding backwards (fix 23), 2026-10-08

**Problem.** User (test 6, 2026-10-05): "near the end of the session the skater randomly changed directions he was
skating while still facing the original direction and skated backwards, grinded a ledge and then skated away
backwards." Test 7: "Skating backward npcs is still a thing, you know that." The readout of test 6 showed the NPC
switching lines (...9b79 node 57, ...9a7c node 102, ...9b79 node 87, ...9ab6 node 8) and staying backwards.

**Root cause.** Two parts.
1. The fix 16 rule (`begin_switch`): at a switch, when the new line's recorded skater faced more than 90 deg away
   from the drawn skater, the cursor toggled `facing_flipped` and rode the whole new line turned 180 deg. Once the
   drawn skater was backwards (a recorded fakie or switch stretch), every later switch kept it backwards on lines
   recorded forward. Not retail: an own heuristic.
2. The puppet root was the recorded skater quaternion. [data] On the exported lines (DownTown, Industrial,
   University; 1,620 lines, 142,042 nodes moving at 1 m/s or more) that frame faces against the travel (more than
   135 deg) on 28,102 nodes, while retail's path frame (below) faces against it (more than 90 deg) on only 11,598.
   The two frames agree within 30 deg on 111,240 nodes and are about 180 deg apart on 24,251 (13,864 of them on
   board-flipped nodes, 10,387 on clear ones), in 69 lines for most of their length and 382 lines in stretches:
   the recorder rode switch (body turned round, board rolling nose first). The puppet has one stance, so those
   stretches were drawn riding backwards even without a switch.

**Evidence (retail).**
- [code] `sub_8246C7F8` (line end, called from `sub_8246D3C0` on the last node): `sub_82458968` lists up to 16
  lines, one is taken as is, several go through `sub_8246C1C8`; then it stores the line id (`+592/+600`), calls
  `sub_82468AC8`, stores the node (`+816`) and the line pointer (`+800`, `sub_824587E8`). No facing, stance or flip
  state is written at the switch. The branch choice `sub_8246BEE0` likewise stores line and node. (Corrected
  2026-10-08: retail does keep a facing state, the flip at controller `+927`; the switch only leaves it alone. See
  "Fix 23 corrected" below.)
- [code] `sub_8246D560` (controller update) calls `sub_8246D3C0`, then rebuilds the path frame at controller `+144`
  every update from the current line and node: `sub_8245A1A0` on node 0, else `sub_824734A8` (interpolated between
  nodes), both through `sub_82453A58`: the node's board orientation (`sub_82453970`, node `+0x18`) with its X and Z
  rows negated (turned 180 deg about up) when flags `+0x28` bit 0 (`m_IsBoardFlipped`) is set. The frame depends on
  the current node only, never on an earlier line.
- [data] That path frame faces the travel on 92 % of moving nodes (fakie on the rest), the recorded skater frame on
  80 %. The node holds no switch or stance flag (flags bits 0..3: board flipped, crouched, airborne, off board).
- [code] The one skater-frame reader decoded so far, `sub_8246B1F8` (node copy at controller `+712`, called from
  `sub_8246A700`), takes the board frame's yaw (`sub_82453970`, no flip turn) minus the skater frame's yaw
  (`sub_82453B70`, node `+28`), wraps it to +-180 deg and folds it by 180 deg into +-90 deg (constants read as pi,
  2 pi and pi / 2 from the fold pattern, not from data), then stores it (`+856`, `+860`, flag `+931`): the half turn
  between body and board is dropped there. Not yet read: the other readers `sub_82453C58`, `sub_82454648`,
  `sub_8245A018`, `sub_82454B28`, the consumers of `+856/+860/+931`, and how the body and stance follow the path
  frame.

**Change.** Part 1 follows the retail code. Part 2 (`drawn_skater`) is NOT RETAIL YET: it is backed by the data and
by one decoded retail reader (below), not by a full port of how retail's skater body follows the path frame.
- `skate-core::living_world::replay`: `path_frame(node)` ports `sub_82453A58`. `drawn_skater(line, i)` (NOT RETAIL
  YET): the recorded
  skater frame (pitch, roll, air attitude) turned 180 deg about its own up axis when its forward is more than
  90 deg (yaw) from the path frame's forward; airborne nodes take the turn of the last grounded node before them (a
  shove-it spins the board in the air; only the landing node carries the final flip bit). `sample` and the switch
  blend draw `drawn_skater`, so a switch-stance recorder is drawn riding forward and a fakie recorder fakie, like
  retail's target frame.
- `ChainConfig::keep_facing` (the fix 16 rule) defaults to `false` (not retail: retail never compares the old and new line at a switch; its facing state is the
  latched flip `+927`, see "Fix 23 corrected" below); it stays as a mod
  option (`sdk.world.set_tuning('living_world', {skater_line_chain = {keep_facing = true}})`, validated, first writer
  wins, read back, reset to `false` on mod disable). Docs in `sdk/skate.lua` and `skate-mods` say it is not retail.
- Logging: `NPC_SKATER_BACKWARDS #id character line node heading velocity_yaw off deg m/s recorded_fakie
  facing_flipped phase pos` (warn, always on, at most one line per NPC every 2 s) when the drawn heading is more
  than 135 deg from the velocity yaw at 1 m/s or more (`facing_check`, `facing_diagnostic`: diagnostic constants,
  not retail values). `recorded_fakie true` means the line's own path frame opposes travel there (retail fakie). The
  `SKATE_LIVING_WORLD_DEBUG` readout now also prints `heading` and `velocity_yaw`.
- Multiplayer: everything is a pure function of the lines, the node and the branch records; a client mirroring the
  records draws the same frames (tested). No new state.

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`;
`crates/skate-game/src/living_world/npc_skaters.rs` (`backwards_line`, `log_backwards`, readout), `npc_tests.rs`,
`peds_tests.rs` (test app gets `PedObstacleTrace`), `crates/skate-game/src/modding/world_tuning.rs`;
`crates/skate-mods/src/world_tuning.rs`; `crates/skate-data/tests/living_world_data.rs`; `sdk/skate.lua`.

**Verification.**
- skate-core `living_world` 118 passed. New: `npc_skater_never_rides_a_forward_recorded_line_backwards_across_switches`
  (forward line, chain onto a line recorded fakie then reverting, chain onto a forward line: never backwards outside
  the recorded fakie stretch once the blend settles, line 3 forward, client mirror identical; the fix 16 option
  reproduces the bug on more than 300 frames), `path_frame_turns_the_board_when_the_node_is_board_flipped_like_sub_82453a58`,
  `drawn_skater_faces_the_retail_path_frame_direction` (switch stance drawn forward, flip bit, air keeps the grounded
  turn, fakie drawn fakie), `facing_check_flags_heading_against_velocity`.
- skate-game `living_world world_tuning` 58 passed (1 ignored). New headless
  `living_world_npc_skaters_never_ride_backwards_across_line_switches`: 40 s, every chain joins a line recorded the
  other way round, switch-stance stretches on the rest; 0 forward-recorded frames ridden backwards, recorded fakie
  reported as `recorded_fakie true`, deterministic; the fix 16 option rides more than 100 such frames backwards.
- skate-mods `world_tuning` 3 passed.
- Data-gated (`SKATE3_ASSET_ROOT`): `npc_skater_keeps_its_facing_across_switches_on_the_exported_lines`, 212 seeded
  rides, 882 switches, 474,412 judged settled frames: forward-recorded frames drawn backwards 353 (retail rule) vs
  70,740 with the raw skater frame before this change and 16,026 with the fix 16 option; spins at a switch 29 (was
  130 without fix 16). All 8 data tests pass.
- Not seen in game yet (to playtest).

**Open questions.**
- How retail's full skater consumes the path frame (`AIPhysicsInput` filled in `sub_82463C08`, `sub_82464000`,
  `sub_82464448`, `sub_82465578`, input struct at controller `+8`, e.g. `+6007` from controller bytes `+796/+797`):
  whether it reverts or 180s to match a path frame that flips at a switch. The 29 remaining switch spins are switches
  onto a line whose path frame is fakie; retail's target frame flips there too.
- The 353 remaining frames (breakdown on the exported lines): 236 in the air (recorded spins against the travel, 166
  of them before a fakie landing), 105 on ground segments where the turn decision changes between two nodes (96 of
  them entering a recorded fakie stretch, the nlerp passing 90 deg), 12 other.
- Switch riders are drawn riding forward in the character's stance; retail shows switch with its own clips and stance
  mirroring. Port the rest of the skater-frame readers before calling `drawn_skater` retail.
- Follow-up (in this order): read `sub_82453C58`, `sub_82454648`, `sub_8245A018`, `sub_82454B28` and the consumers of
  controller `+856/+860/+931`; port how the body and stance follow the path frame; then the switch-stance mirror for
  the puppet (switch and fakie clips, stance mirroring).
- To playtest: follow an NPC skater through several line switches in DownTown; watch for backwards riding, spins at a
  switch, switch riders drawn forward; check the log for `NPC_SKATER_BACKWARDS` with `recorded_fakie false`.
- Retail plays fakie and switch clips and mirrors for stance; the replay puppet plays forward regular clips.

### Fix 23 corrected (2026-10-08)

**Problem.** Fix 23 said "retail keeps no facing state" and replaced the fix 16 rule with our own per-node fold
(`drawn_skater`: the recorded skater frame turned wherever it faces more than 90 deg from the path frame). The retail
parity review (`.local/research/retail-parity-review-2026-10-08.md`, item 6) found both wrong: retail keeps one latched
flip, and its target is the whole recorded skater frame, not a per-node fold against the board frame.

**Retail evidence** (re-verified in the recomp source before porting).
- [code] `sub_8246D560` rebuilds controller `+144..+207` (path frame) and `+208..+271` (recorded skater frame
  interpolated between the two nodes: `sub_8245A088` -> `sub_82454CD0` -> `sub_82454B28`; `sub_82454B28` reads the
  nodes' skater quaternions at node `+28` and slerps with shortest-arc sign selection).
- [code] `sub_8246B358` (AI input): loads the target from controller `+208`; when byte `+927` is set it negates rows 0
  and 2 (`vxor` with the sign mask: 180 deg about up). The yaw error (`sub_824536C8`) goes to `sub_82471188`, which
  reads the `ai_skater` tunable `DD8843F793462295` (2.0 deg dead zone) and `281F55D7BB965ADC` (10.0 deg full steer):
  steer = clamp((|e| - 2) / 8, 0, 1), sign opposite to the error.
- [code] `sub_8246A700`: `+928` = the skater state object's `+438` (set by `sub_82DB6EC0` while the player state id is
  in 200..299, inferred: riding). Only on the rising edge of `+928` it reads the current node's skater frame
  (`sub_82453B70` on controller `+608`) and the character matrix row 2 (`+400`), and stores `+927` = 1 when their 3D
  dot product is below 0 (`vcmpgtfp` against 0.0), else 0. `sub_8246C7F8` / `sub_8246BEE0` never touch `+927`, so it
  is held across branches and chains.
- [code] New in this pass: the spawn `sub_8245C548` builds the character's frame with `sub_82453C58` (node world
  frame: the skater frame on off-board nodes, the path frame on the others) and passes it to `sub_8245DA78`. So the
  first latch compares the recorded skater frame against the path frame at the spawn node.

**Change.**
- `skate-core::living_world::replay`: `FacingRule { RidingEntry, PerNode }`. `RidingEntry` is the retail rule:
  `target_skater` (slerp of the recorded skater frames, `slerp` with shortest-arc sign; the nlerp threshold 0.9995 is
  ours, retail's is not decoded), turned 180 deg (`turn_about_up`) while `LineCursor::flip`. The cursor latches
  `flip` with `flip_test` (retail's dot test) at spawn against `node_world_frame` and on every riding entry
  (`node_riding`: landing = airborne -> grounded node, back on the board = off board -> on board) against the
  drawn skater of the previous frame, and holds it across branches and chains. `riding` is the edge detector. Both
  are plain public fields, a pure function of the lines, the spawn and the branch records (a mirroring client derives
  the same values; tested).
- **Default drawing stays the fix 23 per-node fold (`PerNode`), NOT RETAIL YET** (decision of the main session
  until the user decides). Reason, measured: drawing the retail target without retail's fakie / switch clips and
  stance mirror shows every stretch retail rides fakie or switch (character forward against travel) as riding
  backwards, the bug the user reported. `riding_entry` is a mod option and the target default once the stance mirror
  lands. The cursor latches the flip under either rule.
- `steer_input` + `ChainConfig::steer_dead_zone_deg` / `steer_full_deg` (2 / 10 deg): data with retail defaults for
  the simulated tier. The replay tier has no steering; nothing calls it there.
- Mods: `skater_line_chain {facing_rule = 'per_node' | 'riding_entry', steer_dead_zone_deg, steer_full_deg}`
  (validated, first writer wins, read back, reset on mod disable; tested). `keep_facing` (fix 16) stays a mod option,
  off.
- Log: `NPC_SKATER_BACKWARDS` now also prints `flip`.

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`;
`crates/skate-game/src/living_world/npc_skaters.rs`, `npc_tests.rs`, `crates/skate-game/src/modding/world_tuning.rs`;
`crates/skate-mods/src/world_tuning.rs`; `crates/skate-data/tests/living_world_data.rs`; `sdk/skate.lua`.

**Verification.**
- New skate-core tests (the rule's math, not measured output): `retail_flip_latches_at_spawn_against_the_node_world_frame`
  (switch stance at the spawn node sets the flip, drawn = recorded frame turned; forward node, air and off-board
  spawns do not latch; a recorded revert while riding turns the drawn body with it),
  `retail_flip_is_held_across_a_switch_and_relatched_on_landing` (held across a ground chain; a chain onto a line
  starting in the air relatches on landing against the body: set when the landing comes inside the 0.2 s blend,
  clear when the body already shows the new line; a mirroring client agrees frame by frame),
  `slerp_takes_the_short_arc_and_the_steer_ramp_matches_the_ai_skater_defaults`.
- Data-gated `npc_skater_facing_rules_on_the_exported_lines` (212 seeded 40 s rides, 882 switches, 474,412 judged
  settled frames; frames drawn more than 135 deg against travel where the path frame does not oppose travel, i.e.
  `NPC_SKATER_BACKWARDS` with `recorded_fakie false`; spins after a switch):

  | Rule | Backwards frames | Spins |
  |---|---|---|
  | `per_node` (fix 23, default, NOT RETAIL YET) | 353 | 29 |
  | `per_node` + `keep_facing` (fix 16, mod option) | 16,026 | 3 |
  | `riding_entry` (retail, mod option) | 84,776 | 128 |

  The `per_node` and fix 16 numbers equal the fix 23 numbers above (behaviour unchanged by default). Under the retail
  rule the flip was set at spawn on 19 of 212 rides and changed 2 times later. The counts are reported, not tuned.

**Open questions.**
- **Stance mirror (next task).** Retail draws the frames counted above for `riding_entry` with fakie riding clips and
  a mirrored stance; the replay puppet plays one forward regular clip set. What exists already: the player's
  animation host carries the same bits (`skate-game` `skater_animation.rs`: `state.fakie()` / `state.mirrored()`,
  leading foot `sub_82592B68` = (natural stance == relative stance) xor fakie, `Initialize82B97E38` mirror bits,
  `IsRidingGoofy` from PhysOutAnimation 157/158; `graph_host/motion_channels.rs` `FakieHead`
  (`FakieHeadChannel82BAC778`, channel `B_FAKIE_CHANNEL`)). Not read: what sets the AI skater's fakie / relative
  stance bits (the physics side of `sub_8246B358` -> `AIPhysicsInput`), and which pro natural stance each NPC has.
  Port: per replay sample derive fakie = the drawn retail target's forward against the board's riding direction
  (path frame), mirrored from the character's natural stance; feed both to the puppet's clip pick / playback context
  (`is_mirrored`) and the fakie channel. Estimate: one agent run (about 60 min) to read the AI fakie/stance source
  and wire the two bits into the puppet with tests, a second for the clip set if the stock clips lack fakie variants.
  Then switch the default to `riding_entry`.
- The `node_riding` mapping (air and off board leave riding) is inferred; whether retail's grind / manual states are
  inside 200..299 is not read.
- Retail's nlerp threshold inside `sub_82454B28` is not decoded.

### NPC skater fakie drawing (stance port, 2026-10-08)

**Problem.** Under retail's facing rule (`riding_entry`, "Fix 23 corrected") the puppet drew every stretch where the
recorded skater frame faces against the travel with the forward riding clip, which reads as riding backwards. Retail
draws those stretches riding fakie. The open question was what sets the AI skater's fakie and stance bits.

**Retail evidence** (recomp source).
- [code] The AI input side sets no stance bit. `sub_8246B358` turns the target into analog steer only:
  `sub_824536C8` (yaw error) -> `sub_82471188` (steer slot) and, when `+930` / `+931`, `sub_82471008` (two floats
  through the slot accessor `sub_82471818`). The AI drives the ordinary character, so its stance bits come from the
  character's own animation, like the player's.
- [code] `sub_8246ACF0` calls `sub_8246B358` (the flip-applying target steer) only while the state object's `+438`
  (riding) is set; otherwise it sends `sub_82471070` with controller `+824`. So the latched flip `+927` is not applied
  off the board.
- [code] The fakie bit (SkaterAnim flags `0x20000000`) is written by the motion graph's `UpdateRidingFakie`
  (`UpdateRidingFakie82BB2330`, already ported as `skate_core::animation::riding_fakie`): on the ground and outside a
  trick it sets when `dot(velocity, board axis) < -0.5` above `highSpeedThreshold`, or above `lowSpeedThreshold` for
  longer than `timeSlowlyRollingBackwardsThreshold`; it clears in the air and off the board, holds during tricks and
  stays clear for `timeFromTeleportThreshold` after a teleport. [data] The stock motion graph has one such node:
  1.0 m/s, 0.5 m/s, 0.2 s, 1.0 s (checked by a data-gated test).
- [code] What the bit draws: `FakieHeadChannel82BAC778` starts channel `fakie` with `B_FAKIE_CHANNEL` (blend in / out
  0.3 s, transition 0.1 s) on the rising edge and ends it when the bit clears, setting `torso` toward 1.0 while
  manualing, 0.0 while power sliding, else 0.5. The riding clip below it stays the same. [data] `B_FAKIE_CHANNEL` is a
  phase blend of `FAKIE_MANUAL_CHANNEL_CYC`, `FAKIE_CHANNEL_CYC`, `FAKIE_HEAD_CHANNEL_CYC`; at `torso` 0.5 it plays
  `FAKIE_CHANNEL_CYC`, which turns 19 of the 36 skeleton bones (spine, neck, head, arms). So the stock set has no
  separate fakie riding clips: retail draws fakie as the body turned round plus this channel, which the puppet now does.
- [code] Natural stance: `GetCACSettings` (`sub_82590B50`) returns natural stance = 1 (goofy) when byte `+120` of the
  character's 168-byte CAS record is 0, else 0; `Initialize82B97E38` / `set_customisation` turn it into the mirror
  bits. Switch (relative stance) and the mirror bit are toggled by the `switch` / `mirrored` attributes of trick clips
  (`apply_stance_events`). Which CAS record an AI pro uses was not found.

**Change.**
- `skate-core::living_world::replay`: `LineCursor::fakie` runs retail's `riding_fakie::State` every 60 Hz step on the
  body as drawn (board axis = the drawn root's +Z, the puppet's board is part of its rig; velocity = segment velocity;
  ground = riding node, trick = open trick span on the ground). `fakie_since` / `fakie_previous_since` drive
  `fakie_channel_weight` (linear 0.3 s fade in and out, `ChannelPlayback`). `ReplaySample::fakie`,
  `FacingCheck::drawn_fakie`. `ChainConfig::fakie` holds the thresholds (`retail::FAKIE`, the stock values).
- `rule_skater` under `riding_entry` no longer turns off-board nodes (`sub_8246ACF0` above); stepping on or off the
  board slerps between the per-node targets.
- Puppet (`npc_skaters.rs`): while the channel weight is above 0, the stock `B_FAKIE_CHANNEL` tree is built from the
  stock metadata (`graph_host::motion::tree_commands`, `torso` 0.5) and blended over the layered riding pose with
  `PoseCommand::ChannelBlend` at the channel weight, like `MotionChannels::evaluate`. `NpcPuppetClip::fakie` shows it.
- `NPC_SKATER_BACKWARDS` prints `drawn_fakie`.
- Mods: `skater_line_chain {fakie_high_speed, fakie_low_speed, fakie_slow_seconds, fakie_spawn_seconds}` (validated,
  first writer wins, read back, reset on disable; a very high speed turns the fakie drawing off) and
  `skater_clips["fakie_channel"]` (the overlaid stock tree; a tree that does not build is left out).
- Multiplayer: the bit, its clocks and its change frames are plain cursor fields, a pure function of the lines, the
  spawn, the branch records and the tuning; a mirroring client steps the same cursor and gets the same bits (no
  serialisation added; a snapshot would carry them as plain values).

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`, `crates/skate-core/src/animation/riding_fakie.rs`;
`crates/skate-game/src/living_world/npc_skaters.rs`, `npc_tests.rs`, `crates/skate-game/src/graph_host/motion.rs`,
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-game/src/physics/prop_dynamics.rs` (test literal);
`crates/skate-mods/src/world_tuning.rs`; `crates/skate-data/tests/living_world_data.rs`; `sdk/skate.lua`.

**Verification.**
- `retail_fakie_bit_follows_the_drawn_body_against_travel` (skate-core): set when the drawn body turns against travel,
  not inside the spawn window, cleared in the air, set again after the landing, 0.3 s fades, deterministic, never set
  under `per_node`, a mod can turn it off.
- `living_world_npc_fakie_rule_and_channel_match_the_stock_graph` (skate-game, data-gated): the stock graph's
  `UpdateRidingFakie` equals `retail::FAKIE`; the channel tree builds, its clips evaluate, it changes the riding
  pose, weight 0 leaves it unchanged; the mod key replaces the tree.
- Headless `living_world_npc_skaters_never_ride_backwards_across_line_switches` (retail rule): 1191 switch-stance
  samples drawn against travel, all 1191 drawn fakie; no forward-recorded line ridden backwards outside them.
- Data test `npc_skater_facing_rules_on_the_exported_lines` (212 rides, 882 switches, 474,412 judged frames), frames
  drawn more than 135 deg against travel where the path frame does not oppose travel, split by drawn stance:

  | Rule | Backwards | Drawn fakie | Air after a fakie takeoff | Air | Off board | Ground trick | First 1 s | Other | Spins |
  |---|---|---|---|---|---|---|---|---|---|
  | `riding_entry` (retail) | 85,525 | 72,825 | 10,188 | 305 | 2,009 | 78 | 112 | 8 | 128 |
  | `per_node` (fix 23) | 353 | 9 | 72 | 164 | 103 | 5 | 0 | 0 | 29 |
  | `per_node` + `keep_facing` (fix 16) | 16,026 | 12,628 | 2,412 | 253 | 653 | 80 | 0 | 0 | 3 |

  Under the retail rule 97.1 % of the frames against travel are drawn the way retail draws them (fakie on the ground
  with the fakie channel, or an air that took off fakie, where retail's bit is clear too). The counts are reported,
  not tuned.

**NOT RETAIL YET.**
- The mirror bits: the AI pros' natural stance (which CAS record, byte `+120`) is not decoded, so every NPC keeps the
  default goofy stance; switch / mirrored trick events are not collected by the puppet. This is left / right only; it
  does not change which way the body faces.
- The category mapping of the fakie rule (riding node = ground; grinds and manuals are trick spans, retail allows
  grind state 503), board axis = drawn root +Z, ground speed = horizontal segment speed; `torso` fixed at the riding
  value (no manual / powerslide state in the replay).
- Off-board nodes draw the recorded frame (retail sends `sub_82471070` there; decoded in the next subsection, a
  walking-character steer): 2,009 frames.
- Spins after a switch (128 vs 29): retail's body yaw is emergent from the character physics (next subsection); the
  puppet turns within the 0.2 s switch blend.

**Default switched to `riding_entry`** (separate commit). With the fakie bit and channel ported, 97.1 % of the frames
the retail rule draws against travel are drawn the way retail draws them, so the retail rule is the default;
`per_node` (fix 23) and `keep_facing` (fix 16) stay mod options. What this does not cover is listed above: the mirror
bits (left / right only), off-board frames, and the spins after a switch that a turn rate would soften. Reverting the
default is one line (`ChainConfig::retail().facing_rule`).

### NPC skater turn rate, pro stance and off-board steer (research, 2026-10-08)

**Problem.** Two gaps left by the stance port: the drawn body spins after a switch under the retail facing rule (128
switches turn the body, 29 under `per_node`), and every NPC uses the default goofy stance. A third: off-board frames
are drawn as recorded.

**Evidence: the turn rate is not a tunable [code + data].**
- `sub_82471188` (on board) computes steer = clamp((|e| - dead zone) / (full - dead zone), 0, 1), sign opposite to e,
  with the `ai_skater` tunables `DD8843F793462295` = 2.0 deg and `281F55D7BB965ADC` = 10.0 deg
  (`skater_profiles.json`, `ai_skater.default`). It then writes that one value through `sub_82471818` (a hashed
  name-to-slot lookup) into three input channels of the character: `0x830BFD74`, `0x830BE600`, `0x830BE1E0`. Their
  names come from the static initialisers `sub_82F84BE0` / `sub_82F84A30` / `sub_82F84BC8`: `Turn` (`0x820DB088`),
  `BodySpin` (`0x820DAF88`) and `KickTurn` (`0x820DB07C`).
- So the AI drives the same channels as the player's stick (`Turn` is the intention of `sub_825999F0`,
  `skate_core::input::steering_intentions`). On the ground the body yaw follows from steering tilt `sub_82D92440`
  (`skate_core::riding::steering::calculate_tilt`), truck targets `sub_82C040F0` and `SetTruckDriveFrames`
  `sub_82C0B9C0` (`skate_core::physics::truck_frames`), and the rigid-body wheel solve. There is no turn rate or yaw
  response constant on the AI side to port into the replay puppet; the rate is whatever the board physics produces.
- Decision (main, 2026-10-08): no fitted turn rate for the puppet. The 0.2 s switch blend stays, labelled NOT RETAIL
  YET in `ChainConfig::blend_seconds`. The faithful fix is the simulated NPC tier (PR #52 checklist item), where the
  AI steer drives the ported riding physics.

**Evidence: off-board steer `sub_82471070` [code + data].** `sub_8246ACF0` calls it with controller `+824` when the
state object's `+438` (riding) is clear. Steer = clamp(|e| / full, 0, 1), sign opposite to e, no dead zone, with
`ai_skater` tunable `FED24A60BD8606F7` = 18.0 deg; the same three channels (`Turn`, `BodySpin`, `KickTurn`). `+824` is
written by `sub_8246AA00`: a yaw error (`sub_824536C8`) toward a look-ahead target (`sub_82468FC0`, `sub_82592A00`,
`sub_82592990`, avoidance `sub_82467FC8`), stored raw or as (e + previous) * 0.5 (`0x8209975C` = 0.5) depending on
`+828` / `+933` / `+945`. The off-board body is the walking character, so its yaw is the Move Object controller's
response (`skate_core::player::offboard::move_object`), again physics, not a puppet rate. The off-board flip is never
applied (`+927` is only read by `sub_8246B358`), which the replay already does. Off-board frames stay drawn as
recorded (2,009 frames), NOT RETAIL YET until the simulated tier.

**Evidence: pro natural stance, not found (about 25 min) [code + data].** Checked:
- `GetCACSettings` `sub_82590B50`: record = table at `0x83067060` (`+8` pointer) + index * 168; byte `+120` == 0 gives
  goofy. Its only callers are `sub_82590DC0` (the `Actor` constructor, allocation tag `Actor` at `0x82224AA8`; index =
  its `r5` argument), `sub_825922C0` (no direct caller, a vtable entry) and `sub_825947C0` (from `sub_827D8450`).
  `sub_82590DC0` is reached from `sub_82598600` / `sub_82598748`, which are reached only from `sub_82597A70` /
  `sub_82597AE0`, both called indirectly. Which index an AI pro's actor gets was not traced.
- `marquee.big` pro recipes (`data/content/recipe/marquee/<pro>.recipe` + `.xml`): geometry and materials only, no
  stance field.
- `skater_profiles.json` `characters` (layout bits `+8` / `+9` / `+16` / `+17` and the hashed fields) and
  `ai_skater_profiles` (bool / int fields): no field separates the pros into two stance groups consistently; the
  varying ones (`+16`, `6DBAF7AD6CDE6A16`, `782E7345CDF39DFD`) put pros who ride the same stance in real life into
  both groups (`+16`: eric_koston false, andrew_reynolds true; `6DBAF7AD6CDE6A16`: koston true, dyrdek false), so
  they are not stance. (Real-life stance is only a sanity check here, not retail evidence.)
- `createacharacter.big`, `db.big` names: no per-pro CAS or stance entries.
Next step: trace (recomp hook on `sub_82590B50`) the index passed for spawned AI pros, then read the 168-byte table
it points at; the table is filled at runtime, so this needs a recomp run. Until then every NPC keeps the default
goofy stance (left / right only).

**Change.** Documentation and the `ChainConfig::blend_seconds` NOT RETAIL YET note only; no behaviour change, so the
counts above are unchanged (riding_entry: 128 spins after a switch, per_node 29, keep_facing 3).

**Open questions.** The AI pro CAS index; whether a simulated-tier NPC reproduces retail's switch behaviour (it should,
since the steer and the physics are both ported); `+930` / `+931` gating of `sub_82471008`.

### NPC skater natural stance (port, 2026-10-08)

**Problem.** Every NPC skater was drawn in the stock rig's stance (goofy), whatever pro it was. Retail gives each NPC
the natural stance of its character record.

**Evidence [code + data].** A recomp trace (hook on the CAS getter, ten idle runs, no skating: PCU Library, Mega-Park,
Industrial Ghetto Spot, then 13 Downtown, eight University and nine more Industrial locations; 426 NPC skater spawns,
0 malformed lines, 0 conflicts) resolved the open AI pro CAS index above:
- `GetCACSettings` `sub_82590B50` indexes the table at `[0x83067060 + 8]` (168 bytes per entry) by **slot**, not by
  character: slot 0 = the player, slots 1 to 3 = the NPC skaters. Every NPC spawn refills its slot from its character
  record, then the actor ctor `sub_82590DC0` (slot in `r5`) resolves the record's 64-bit id to its name
  (`sub_824581C0`) and builds the character (`sub_82B973C8`).
- Stance: the getter writes `goofy = (byte +120 == 0)`; the ctor passes it to `Initialize82B97E38`, which stores it at
  character `+228` (and `+232`); virtual `+128` (`sub_82B97168`) returns it, and retail's Lua bindings
  `GetIfSkaterIsRegularStance` (`sub_8284B910`) and `IsRegular` / `IsGoofy` answer "regular" when it is 0. So +120 = 1
  regular, 0 goofy. User-confirmed: the player's created skater (CAC_female) reads +120 = 0 and rides goofy in game.
- Per record (+120), 36 records: regular = andrew_reynolds, attiba_jefferson, brayden_szafranski, chris_cole,
  dan_drehobl, danny_way, darren_navarette, jason_dill, john_rattray, pj_ladd, ryan_smith, seb, terry_kennedy,
  teammate_01, teammate_02, teammate_04, z_kook_1; goofy = benny_fairfax, chris_haslam, cuz, deerman_of_darkwoods,
  dennis_busenitz, eric_koston, jerry_hsu, joey_brezinski, john_cardiel, josh_kalis, mark_appleyard, mike_carroll,
  pat_duffy, ray_barbee, rob_dyrdek, ryan_gallant, slappy, teammate_03, lizard_king (inferred). "Inferred": the slot was refilled without a new read, so the value is the previous
  occupant's (the same in every one of their spawns). Record ids and the run of each record are in the table file.
  These are retail's record values, which need not match the real skaters' stances.
- Drawing: for a regular skater `Initialize82B97E38` sets the orientation and mirror bits (`0xC000_0000`), so every bind
  pose adds `BOARD_BACKWARDS` / `BOARD_BACKWARDS_IK` and mirrors (mode 2), channels included (`add_bind_pose`, the
  player's `SkaterAnimation::set_customisation`). The physical board follows the animated board
  (`publish_deck_angles`, `board_flipped = bit31 ^ mirror`, false for both stances).

**Change.**
- `skate_core::living_world::stance`: the table as our own data file `npc_natural_stance.tsv` (record name, record id,
  regular / goofy / unknown, measured / inferred / unseen; measured values, no game records), `resolve` (mod override by
  record id or record name, then the table, then the default goofy = the old behaviour).
- `NpcSkater::stance`: set once at spawn from the character key's record (`skater_profiles.json` `recipe`, e.g.
  `deerman` -> `deerman_of_darkwoods`). A function of the spawn record, so a client derives the host's value.
- Puppet (`puppet_pose_in_stance`): goofy commands unchanged; regular closes the layered pose and the fakie channel tree
  each with the bind pose tail (reference pose, `BOARD_BACKWARDS`, `BOARD_BACKWARDS_IK`, mirror 2) before the channel
  blend, like the player's base tree and `fakie` channel. The fakie bit itself does not depend on the stance (board
  axis), as in retail (`UpdateRidingFakie` reads the board velocity).
- Root: the regular bind pose turns the drawn board round in root space, so a regular puppet's root turns half a turn
  (`stance_root_turn`): the board keeps the heading the facing and fakie rules gave it, and the left foot leads. This
  follows from retail's board publication above; the replay frame is ours (NOT RETAIL YET together with the replay
  tier's facing rule).
- Mods: `sdk.world.set_tuning('living_world', {skater_stance = {["CD56C7FE01EBE665"] = "regular", deerman_of_darkwoods
  = "goofy"}})`; read at spawn (live NPCs keep theirs, like retail), cleared on mod disable. The bound-look log line
  prints the stance.

**Verification.** skate-core `living_world::stance` 4 tests (table counts 35 measured / 1 inferred / 8 unseen, the bit
per record, overrides by id / name, bad tables rejected); skate-game `living_world_npc_skaters_take_the_natural_stance_of_their_record`
(spawned stance per record, overrides, same spawns with and without overrides, reset), `npc_skater_stance_set_merge_and_reset`
(mod patch merge, read, reset), `living_world_npc_bind_pose_tail_per_stance`, data-gated
`living_world_npc_regular_puppet_is_mirrored` (goofy pose identical to before; regular feet are the mirrored other foot
within 1 mm; after the root turn the front truck is where the goofy one is and the left foot leads). Totals: skate-core
785 + 2 known; skate-mods lib 102; skate-game bin 580 + 1 known; skate-data `living_world_data` 8/8 before and after
(skate-data unchanged).

**Open questions.**
- The record class `91BCA6693EFC9AA7` (stance byte +120, male flag +121, style +124 / +144) is not in our setup export;
  until the file holding it is found the stance is our measured table, not a setup export.
- 8 free-roam pool records did not spawn in the runs (colin_mckay, lucas_puig, michael_burnette,
  community_skater_01 to 05): unknown, drawn goofy. [data] The community records carry the vault gate "offline" and
  [code] the choice skips community ids unless allowed (online uploaded profiles; EA's servers are gone), so offline
  runs cannot spawn them. [data] Spot choice follows the ambient line masks (path +72, bit = profile pro index):
  bit 26 (ryan_smith) is on most University lines where bit 51 (the shared index of andrew_reynolds, lucas_puig,
  michael_burnette, ray_barbee, teammates) is not, which is where ryan_smith first spawned (Campus Entrance to Peterson
  Pavilion); bits 17 (darren_navarette), 24 (colin_mckay) and 51 are set on the same lines everywhere, so for those
  there is no favoured spot and only more idle time helps. Untried: skate.Park, DLC areas, Slappy's Car Lot.
- Switch / mirrored toggles from trick clip attributes: ported, see the next section.

### NPC skater trick-clip stance toggles (port, 2026-10-08)

**Problem.** A trick clip can flip the skater's stance bits (board turned round under the body, body mirrored,
relative stance switch). The replay puppet kept its natural stance bits for good, so after a shove-it the next clips
snapped the board back to its old heading.

**Evidence.**
- [code] Every actor's animation step `sub_82593230` (the player and the NPC skaters alike; the SkaterAnim object at
  actor `+1804 - 14960`) advances the tree (virtual `+32`, `+124`), then calls `sub_82B98980`. That function queries the
  current tree's attributes (virtual `+96`) for three names (`0x830BFAB4`, `0x830C0694`, `0x830C02A4`:
  `animboardbackward`, `mirrored`, `switch`) and toggles flags `+15180` bit 31, bit 30 and the relative stance
  `+15196` (0 / 1) once per frame each is present. No other condition: an NPC's trick clips toggle its bits exactly
  like the player's. The bits bake into every tree built afterwards (`add_bind_pose`: `BOARD_BACKWARDS` /
  `BOARD_BACKWARDS_IK` for bit 31, mirror mode 2 for bit 30); a tree keeps the bits it was built with.
- [data] Stock banks (3,324 clips): `ANIMBOARDBACKWARD` is a point event on 99 clips (the shove-it, varial, hardflip and
  inward heelflip air clips, late shove-its, dark-catch and underflip outs, grab varials); `MIRRORED` and `SWITCH`
  appear together on 9 clips only, the switch riding clips `R_SWITCH_RIDE_*` and the bail dismounts
  `BR_DISMOUNT_*_INTO_BR_AIR`. Of the trick clips the replay puppet plays (recorded trick slots), 16 slots carry the
  board event (pop shove-it, fs pop shove-it, hardflip, inward heelflip, varial kickflip / heelflip, nollie variants)
  and none carries `MIRRORED` / `SWITCH`. So on NPCs the visible toggle is the board bit.

**Change.**
- `skate_core::living_world::stance`: `StanceFlags` (board backward, mirrored, switch; plain data with `to_bits` /
  `from_bits`), `StanceFlags::natural` (regular = bits 31 and 30, `Initialize82B97E38`), `StanceFlags::apply` (the
  `82B98980` toggles), `StanceEvents` (the three attribute names, mod renames, empty = off) and `attribute_in_window`
  (the clip clock's collected-attribute test, checked against `ClipClock::attribute_status`).
- The player's `AnimationState::apply_stance_events` now calls the same `StanceFlags::apply` (one implementation for
  player and NPCs, as in retail); a test compares it bit for bit with the previous code over flag words, relative
  stance values (including out-of-range ones) and every attribute subset.
- NPCs: `NpcStanceTrack` per NPC, updated on the fixed step after `advance` (`track_stance`): the newest puppet layer's
  clip (`+` sequences part by part) is checked over the window it advanced; a new layer records the bits it was built
  with. The puppet (`puppet_pose_in_flags`) closes each layer with the tail of its own bits once the layers' bits differ
  (a crossfade between two trees, each with its own bind pose); with equal bits the commands are exactly the earlier
  ones (goofy: unchanged; regular: the natural stance port). The root's half turn follows the newest layer's mirror bit
  (`flags_root_turn`): the board bit alone keeps the body where it is and the next layer's `BOARD_BACKWARDS` keeps the
  board turned as the clip left it. Every toggle is logged (`LIVING_WORLD npc stance #serial character clip t: old ->
  new`).
- Multiplayer: the bits are a function of the cursor, the clip data and the tuning, so a client derives the host's
  bits; no new events.
- Mods: `sdk.world.set_tuning('living_world', {skater_stance_events = {board_backward = "animboardbackward", mirrored =
  "mirrored", switch = "switch"}})` renames the attribute that fires a toggle (a mod's own clip attribute) or turns it
  off with `""`; a mod's trick clip (`skater_clips["trick.<name>"]`) fires its own events. Cleared on mod disable.

**Files.** `crates/skate-core/src/living_world/stance.rs`, `crates/skate-game/src/skater_animation/state.rs`,
`crates/skate-game/src/living_world/npc_skaters.rs`, `crates/skate-game/src/living_world/mod.rs`,
`crates/skate-game/src/living_world/npc_tests.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-mods/src/api.lua`, `crates/skate-data/src/animation_metadata.rs`
(`AnimationMetadata::clips`).

**Verification.** skate-core `stance_flags_toggle_like_82b98980`, `attribute_window_matches_the_clip_clock`; skate-game
`shared_stance_events_match_the_player_reference`, `living_world_npc_stance_track_follows_trick_clip_events` (one toggle
per clip, layers keep their start bits, sequences, mod off, bounded memory), data-gated
`living_world_npc_trick_clips_toggle_the_board_bit` (the stock facts above; uniform bits = the earlier poses; mixed
bits differ; weight 1 = the newest layer alone), the natural stance test extended (every NPC tracks its natural bits;
no events without the banks), `npc_skater_stance_events_set_merge_and_reset`; skate-mods patch validation. Totals:
skate-core 787 + 2 known; skate-mods lib 102; skate-data lib 34; skate-game bin 584 + 1 known.

**Fakie rule after a shove-it (checked against retail, 2026-10-08).** Was open: does the board bit change when the
skater counts as riding fakie? Retail says no, so the NPC rule was already retail's; the change is evidence, helpers
and tests.
- [code] The riding-fakie rule (`UpdateRidingFakie82BB2330`) projects the velocity on PhysOutSkeleton `+0`.
  `Fill82BE1AE8` stores row 2 of `GetEffectiveRoot82BE3650` there; that function copies Skeleton `+11920`
  (animation to world) and negates rows 0 and 2 only when Processed `+2476` bit 2 is set (`rlwinm 29,29`), which is
  animation packet `+10370`, the mirror bit 30. Bit 31 is not read.
- [code] `board_flipped = bit31 ^ bit30` (`GetPhysUpdateData82B985E8`, packet `+10368`) reaches physics as Processed
  `+2468` bit 20: the deck's effective frame (`GetEffectiveTransform82C01BF8` negates X / Z), the push foot frame, deck
  angles, steering and manual entry. The effective deck frame turns the shoved board back, so its forward stays the
  body's. After a shove-it the fakie input is the same as before it.
- Port: `StanceFlags::board_flipped` (bit 31 xor bit 30) and `StanceFlags::fakie_board_axis` (`82BE3650`: the root Z
  negated iff mirrored). The cursor's fakie rule uses the drawn frame; the puppet root is that frame turned half a turn
  iff mirrored, so its effective root is the drawn frame for any stance bits. The NPC stance log now also prints
  `board_flipped` and the fakie bit. No new tuning field (retail has no value to expose); the existing
  `skater_stance_events` still drives the bits and is reset on mod disable. Deterministic, no new events.
- Tests: skate-core `board_flipped_and_fakie_axis_match_the_player_reference` (the byte equals the player's
  `publish_evaluated` for all 8 bit combinations; `riding_fakie::State` gives the same results with the natural bits
  and after a shove-it, both stances, forward and backward travel); skate-game
  `living_world_npc_fakie_axis_after_a_shove_it_is_retails` (after a shove-it clip `board_flipped` = bit 31 xor
  bit 30 and is set; the fakie axis of the puppet root equals the drawn frame for the natural and the toggled bits).
  skate-core lib 788 + 2 known; skate-game `living_world` 59 pass, 1 ignored.

**Open questions (NOT RETAIL YET).**
- Puppet clip times are the replay's, not retail's tree clock (air clips are not matched to air time), so the event can
  fire a little earlier or later than in retail, and a trick cut short by the replay before its event point does not
  toggle (as in retail when its tree is replaced first).
- The root turn during a crossfade between layers with different mirror bits snaps with the newest layer (no NPC trick
  clip toggles the mirror bit, so it does not happen with stock clips).
- The switch bit is tracked but changes nothing on the puppet (retail uses it for tree selection and physics, which the
  replay tier does not run).
