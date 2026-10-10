# 26b: Living world: Data and the population core

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## Change (milestone 1: data)

- **Setup group `livingworld`** (optional; own fingerprint; the existing groups' fingerprints are unchanged, so an
  existing install only runs the new group). On a content error it removes `living_world/`, writes
  `living_world-availability.json` and the world runs empty; setup reports it. One vault conversion feeds both
  exporters (about 40 s on the disc, 53 MB):
  - `tables.json` (29 living-world vault classes, inheritance applied, readable names where known plus a
    `field_names` map to the `Hash_*` names), census grids per district (4 m cells) and `census.json`,
    `roads.json` / `roads.bin` (segments; road objects kept verbatim; milestone V0 decodes the rest and corrects the count to 76),
    `waypoints.json` (74 groups, 394 waypoints), `navmesh.json` (inventory only), `models.json` and
    `models/<recipe>.glb` (all 77 recipes from a binary `.recipe` reader; each GLB with the model's skeleton, both
    LODs, all parts and textures). Format details: [`living-world/peds-data.md`](living-world/peds-data.md).
  - `skater_paths/<District>.bin` (the retail line blobs unchanged in a small container, deduplicated by line id like
    retail's path manager), `skater_paths/index.json`, `skater_profiles.json` (193 profiles, 87 characters, the
    42-character pool). Format details: [`living-world/skaters-data.md`](living-world/skaters-data.md).
- **`skate-data::aipath`**: the line format, the pack container and the per-district dedupe.
- **`native_roster`**: the NPC pool (`npc_pool`) and teammate binding (`bind_teammates`): the disc has no look for
  `teammate_NN` (retail reads it from the save), so a recruited teammate binds to a customiser look in
  `settings/living_world_teammates.json`; unbound = not recruited = never spawns.

Files: `crates/skate-data/src/aipath.rs`, `crates/skate-data/tests/aipath_data.rs`,
`tools/asset_pipeline/{living_world,living_world_models,living_world_skaters}.py` (+ tests), `versions.py`,
`install.py`, `group_receipts.py`, `asset_exports.py`, `native_roster.py`, `tools/test_setup_assets.py`.

## Change (milestone 2: population core)

One census engine for every ambient kind, shared by NPC skaters, pedestrians and vehicles (props and dynamic
objects later). Pure rules in `skate-core::living_world` (no ECS, no I/O), readers in `skate-data::living_world`,
a minimal Bevy plugin in `skate-game::living_world` that emits spawn / despawn decisions as messages. No bodies or
rendering yet (later milestones consume the messages).

What retail does, checked in the code for this milestone (tags [code] / [data]; addresses TU3):
- **Census tick** `sub_826B71F0` runs one living-world type per console tick in rotation (peds 0, vehicles 1, DMOs
  2, props 3), each a cull pass then a spawn pass [code]. So each census kind gets a pass every 4 ticks.
- **Census circle**: the `livingworld_census_ranges` sets lerped by the player's speed `|v| x 3.6` km/h
  (`sub_826B7D60`, `0x822F8628`); peds 50-60 m ring / 70 m cull at 45 km/h, 50-80 / 90 with offset 20 at 80 km/h;
  vehicles 80-100 / 110 [data]. Cull = 3-D squared distance to the circle centre (`sub_826BA8B0`) [code].
- **Spawn pass** `sub_826B9940` (peds) / `sub_826B9B90` (vehicles): gated by the manager byte, `0x83082929` and
  `IsOnline`; 2 attempts / at most 1 spawn; initial populate 6000 attempts / 600 spawns in an 8-80 m ring
  (`0x82099250`, `0x820E5748`) [code]. Ring point (`sub_82E17508`): direction from two uniform draws in
  [-0.5, 0.5) normalised, radius uniform in [inner, outer] (linear), heading = u32 x 2pi / 2^32 (`0x822F9598`) [code].
- **Cap at the point** `sub_826B8A28`: the record painted in the census layer at the spawn point, max population x
  density (Free Play scale, truncated; skipped in zombie mode). **An unpainted point has no record and cap 0** for
  peds and vehicles (the lookup only resolves a record when the layer query hits) [code]; this corrects the
  milestone-1 note "retail falls back to `default`", which only holds for the other census types.
- **Budget and category** `sub_826B8B88`: cap 0 = no spawn (even in zombie mode); count >= cap = no spawn unless the
  zombie cheat is on; category roll `rand() % 100 + 1` against the cumulative weight x 100 (`0x820ED57C`), no hit =
  no spawn (DownTown traffic weights sum to 0.875) [code, data]. Ped pool 31 [code].
- **Free Play** (mode 3): `sub_826B7010` clamps `+336` / `+332` to 0..1 as the ped / vehicle density; below 1.19e-7
  the whole kind is culled at once [code]. A.I. Skaters `+340` off: no spawns (`sub_8245C548`) and the per-skater
  checks despawn every ambient NPC (`sub_8245A9B8`, the `r25` gate) [code].
- **Ambient skaters** `sub_8245BA28`: 60-tick cycle (cull 0, pool 15, spawn 30, checks 3-12 / 18-27 / 33-57),
  desired 3 (0 online), AI cap 5, 7 slots shared with players, spawn at the start node of an unused line 60-90 m away
  (`0x821FF080`), cull 120 m 3-D or 1000 m height (`0x82256FE0`, `0x82256FE8`) or excess [code]. Line scorer
  `sub_8245C018` (lowest wins): reject when any skater is within 5 m (`d^2 < 25`, `0x8209994C`); nearest skater
  closer than 10 m (`d^2 < 100`): 600 below 0.5 m, else `(10 - d) x 300 x 600` (`0x821963E4`, `0x822F92C0` =
  0x4395FFFF, `0x820994A8`); plus `rand() % 400`; online both terms are skipped [code]. Character scorer
  `sub_8245B068`: 500 if it fits a nearby line + 5 per fitting line + `rand() % 160`, highest wins [code].

Change:
- `skate-core::living_world`: `LivingWorld` (one session: config, seed, console clock, three kinds),
  `PopulationConfig` (code constants as defaults, `config::retail` with labels and addresses; census ranges and
  caps only from data), `census` (circle lerp, grid lookup, ring point, cap, category roll, cull), `skaters`
  (`SkaterWorld` trait, line and character scorers, pool, slots), `population` (rosters, `Scorer` trait,
  `pick_lowest` / `pick_highest`, the census pass), `clock` (retail's fixed 60 Hz world step from any engine step; first written as 30 Hz, corrected in
  milestone V2: the census and the ambient skater manager run on the 60 Hz world step, `sub_8285C928` ->
  `sub_82859E70` bit 0 -> `sub_8245A7E8` / `sub_826BDB50`, fixed 1/60 s at `0x820849C8` [code]; the lights in the
  same update took 480 ticks for 8 s [trace]; the 60-tick skater cycle is 1 s, each census type passes 15 times
  per second, the replay tier advances one 60 Hz recording frame per tick), `rng`
  (seeded PCG32, derived sub-seeds; no global state).
- `skate-data::living_world`: `parse_census_grid`, `LivingWorldTables` (census records resolved through the
  category groups, ranges, `apply_to` the config), `skater_characters` (free-roam pool; unrecruited teammates
  left out), `skater_lines` (ambient lines, start node, heading).
- `skate-game::living_world` (`LivingWorldPlugin`): `LivingWorldSettings` (per kind enabled / density, ambient
  skater count, Free Play options, zombie, net role, seed, debug log; defaults = retail), `PopulationState`,
  `LivingWorldObservers`; `FixedUpdate` after physics: load the district's data on map change, gather the local
  skater's deck position / velocity and the multiplayer state, run the due console ticks, write
  `LivingWorldSpawn` / `LivingWorldDespawn`. `SKATE_LIVING_WORLD=0` turns it off, `SKATE_LIVING_WORLD_DEBUG=1`
  logs a summary every 5 s.

Multiplayer seams (user, 2026-10-04: "build everything with multiplayer support, just dont add the multiplayer
yet, we can have it ready for that to be added however."): a decision is a pure function of (config, seed, tick,
observers, slot budget); it runs in one place (`NetRole` Standalone / Host; a Client never decides and mirrors
records with `PopulationState::apply_records`). The core takes a list of observers (culls use all of them; the
spawn pass rotates through them; one observer = retail). Every entity has a stable `LivingWorldId` (kind + serial,
never reused in a session); records carry kind, id, tick, position, heading, the entity's own seed and the choice
(census record + category, or line id + character + slot) and serialise as `WireRecord`. Rosters are `BTreeMap`s;
no frame-time dependence. No transport or replication code. Retail default stays: nothing ambient spawns online.

Moddability: every rule value is a public config field (code defaults, data ranges), settings are one resource,
ids are stable, decisions are messages any system (or a mod bridge) can read. `sdk.living_world` population
surface (planned, not built in this milestone): `set_density(kind, scale)`, `set_enabled(kind, bool)`,
`set_ambient_skaters(n)`, census record / range patches through `living_world.json` (content overlay), events
`spawned` / `despawned` {kind, id, reason, record, category | line, character}; on mod disable the settings return
to `LivingWorldSettings::default()` and mod-spawned entities are despawned (`DespawnReason::External`).

Files: `crates/skate-core/src/living_world/{mod,census,clock,config,population,rng,skaters,tests}.rs`,
`crates/skate-core/src/lib.rs`, `crates/skate-data/src/living_world.rs`, `crates/skate-data/src/lib.rs`,
`crates/skate-data/tests/living_world_data.rs`, `crates/skate-game/src/living_world/{mod,tests}.rs`,
`crates/skate-game/src/{main,app}.rs`.

Verification (milestone 2):
- `cargo test -p skate-core --release --locked living_world`: 24 tests (circle lerp, ring, caps, category roll,
  initial populate, one spawn per pass in rotation, cull radii, unpainted = none, vehicles 80-100 / 110 / 30, Free
  Play scale and zero removes all at once, online spawns nothing but culls, zombie, skaters 3 / phase 30 /
  60-90 m / 5 m rule / 120 m cull / Free Play off / online excess / shared slots, same inputs same stream, ids
  unique and a client mirror equals the host).
- `cargo test -p skate-data --release --locked --lib living_world` (2) and, with `SKATE3_ASSET_ROOT`,
  `--test living_world_data` (4): caps and ranges exact on the export, the three grids parse, a DownTown run keeps
  15 peds max and 30 cars with every spawn in its ring, 38 pool characters and 1,691 lines.
- `cargo test -p skate-game --release --locked --bin skate3rust -- living_world::` (4): a fake player drives a
  path in a headless app; counts, radii, determinism at 60 and 144 Hz, wire round trip, client role, online,
  settings.


## NPCs vanishing in view (leave fade, census centre), 2026-10-05

**Problem.** The user: "They also (peds and skaters) keep dissapearing randomly while still in sight, which i
don't remember happening often in retail."

**Root cause.** (1) Replay NPC skaters were removed on the spot at the end of a line with no branch, a
placeholder, often in plain view. (2) The census circle centre moved along the horizontal velocity; retail uses
the 3-D velocity, so on slopes our cull circle sat higher or lower than retail's.

**Evidence** [code, TU3 recompilation, reference only]:
- `sub_8245A9B8` (manager per-skater check) removes a skater whose controller has its fading byte (`+104` object,
  `+29`) set once `skater+1804` vfunc 76 is below 0.2 (constant at `0x82099280`).
- `skater+1804` is a sub-object at `+14960` of the object built by `sub_82B973C8` (vtable `0x8231E170`): vfunc
  76 `sub_82B97190` returns the opacity at `+244`, vfunc 68 sets it; default 1.0.
- `sub_82594488(skater, dt)` sets the opacity from two timers growing by dt: `+1868` (fade in, `clamp(t,0,1)`)
  and `+1872` (fade out, `1 - clamp(t,0,1)`), so a fade lasts 1 s.
- `sub_8246EA90` starts the fade out (`+1872 = 0`, byte `+29 = 1`) when the time in the controller's timed state
  passes `duration - 1 s`; `sub_8246EE30` sets that duration to `clamp(5 s + rest of the current line, 1.5 s,
  7.9 s)`; `sub_8246EF78` cancels the fade (`+1872 = FLT_MAX`) when the state ends. Retail skaters therefore
  fade for 0.8 s and are removed at opacity 0.2, never popping out at full opacity.
- Census centre: `sub_826B7530` normalises the 3-D velocity (see `.local/research/npc/fix1-cull.md`).

**Change.** New `skate_core::living_world::leave_fade` (`LeaveFadeConfig { fade_seconds: 1.0, despawn_alpha: 0.2
}` as data-driven retail defaults in `SkaterConfig::leave_fade`, `LeaveFade`, `frames_to_line_end`). The replay
NPC starts fading 1 s before a line end with no branch group ahead and is removed below 0.2; the opacity is
published as the `NpcFade` component (alpha) for rendering and mods. Deterministic (a function of the spawn
record and the tick), host decides the removal, clients mirror. `CensusRange::around` moves the centre along
the 3-D velocity.

**Files.** `crates/skate-core/src/living_world/{leave_fade.rs, mod.rs, config.rs, census.rs, tests.rs,
replay_tests.rs}`, `crates/skate-game/src/living_world/{npc_skaters.rs, npc_tests.rs, mod.rs}`.

**Verification.** New tests: `census_centre_follows_the_3d_velocity_like_sub_826b7530`,
`npc_fades_out_over_the_last_second_and_goes_below_alpha_0_2`,
`leave_fade_waits_while_a_branch_is_ahead_and_is_data_driven` (skate-core),
`living_world_npc_skaters_fade_out_before_their_line_ends` (skate-game). All living_world tests pass.

**Drawing the fade and the spawn fade in (follow-up, same day).** Evidence [code]: `sub_825926F8` (skater spawn
setup) sets `+1868 = 0`, so a new skater fades in from 0 to 1 over 1 s; `sub_82594488` checks the fade in first,
so it wins over a fade out. Change:
- `LeaveFadeConfig::fade_in_seconds` (retail 1.0) with `fade_in_alpha`; `LeaveFade::alpha(frames since spawn)`
  is the fade in while it runs, then the leave fade. Removal is only checked once the fade in is over (retail's
  timed state lasts at least 1.5 s, so its fade out cannot start before the fade in is at 0.5; our lines can be
  shorter, so the rule is explicit).
- Mod entry: `LivingWorldSettings::skater_fade` (retail default, applied into `SkaterConfig::leave_fade` every
  step; `LivingWorldSettings::default()` restores retail).
- `present_fade` (skate-game, `Update`): while `NpcFade::alpha < 1`, every mesh under the entity (body, board,
  anything parented to it) draws with a per-entity `AlphaMode::Blend` copy of its material at source alpha times
  the fade (`NpcFadeMaterials`, like the vehicle glass copies). At 1 the shared materials are put back and the
  component (the only owner of the copies) is removed, so a solid NPC costs what it did before; a despawn or mod
  disable drops the component and frees the copies. Generic over any entity with `NpcFade`, so peds can use it
  for their distance fade.

Verification: `npc_fades_in_over_its_first_second_like_sub_825926f8` (skate-core: curve, fade in wins, no removal
during it, data-driven) and `living_world_npc_skaters_fade_in_from_transparent_with_blended_copies` (skate-game:
alpha < 0.05 at spawn, every frame equals the core curve, 1.0 at 1 s, the mesh uses a blended copy at the fade's
alpha while fading and the shared material again at 1, copies freed). `cargo test --locked -p skate-game
living_world` 27/27, skate-core living_world 88/88, `cargo build --locked` (dev) ok. Not yet seen in game.

**Open.** Blending the whole character can show its own back faces through itself during the 1 s fades (no
depth write in `AlphaMode::Blend`), and alpha-masked source materials lose their cutout while fading; retail's
fade shader is not decoded. The 0.5 hold of the fade in (component byte `+71`) is not ported. What the controller's timed state is in retail
(entered on a skater component byte `+59`) and the early fade when a speed-like value is below 1.0 (byte `+604`,
float `+576`) are not decoded. Peds: the cull component's vfunc +4 and the census focus object (player or
camera) are still open; no camera test was found in the ped cull itself.

**Peds (follow-up, same day).** Problem: the same report for pedestrians. Root cause: retail peds fade out by
camera distance (opaque to 45 m, gone at 55 m) and are culled at 70 m (slow circle; 90 m with a 20 m forward
offset fast), so the cull is never seen. We read the model's 45 / 55 m pair as an LOD switch and drew every ped
solid up to the 70 m cull, which then popped it in view. Evidence [code, TU3 recompilation, reference only]:
- `sub_827C1188` (per render instance, every frame): pair `Hash_73B6874C7B46C7C6` of the model record (peds 45 /
  55 m [data]) against the camera distance gives opacity `1 - clamp((d - near) / (far - near), 0, 1)`; the pair
  `Hash_9FCFDBEA56BA4733` (65 / 75) is used only when its third float is larger (`sub_827C1870`), and both are 0
  for peds [data]. A spawn fade in `+576` grows by 1/30 per console frame (`0x8232BA4C`, about 1 s); the drawn
  opacity is the smaller one, and below 1 the instance draws blended (`+580`).
- `sub_826BA8B0` (ped cull) is a pure 3-D squared-distance test (`>=` for the ped pass). Entities flagged
  census-owned (`*(ent+2020)+240` bit 0x80, set at spawn from the spawn descriptor byte `+97`) are removed;
  the others only pass the "beyond" flag to their `ISecurityGuard` component (lookup key `sub_8269D780`, which
  returns the string `ISecurityGuard` at `0x82307BC0`; vfunc +4), so that branch is the security guards, not a
  visibility rule.
- The census focus (`sub_826BDB50` calls `sub_826B71F0` with its own vfuncs +124 / +128, `sub_826BE7D0` /
  `sub_826BE870`) is the focused skater from the player manager `*(0x83085480)` (= world `+196`) vfunc 16:
  position from its `+52` component, velocity from vfunc 24 (zeroed below |v|^2 = 1e-4, `0x8209BE90`). With no
  skater it falls back to a vector at world `+292` `+5312`. Not the camera.

Change: new `skate_core::living_world::peds::fade` (`PedFadeConfig { distance: [45, 55], fade_in_seconds: 1.0,
enabled }`, `draw_alpha`); a model record's own pair wins (mods set it per model), `LivingWorldSettings::ped_fade`
is the mod entry. Each ped gets the shared `NpcFade` (alpha 0 at spawn); `present_ped_pose` sets the alpha from
the camera distance and the time since the spawn tick (deterministic) and hides the ped's scene at 0; the generic
`present_fade` draws the blend. The cull radii and the LOD placeholder are unchanged.
Files: `crates/skate-core/src/living_world/peds/{fade.rs, mod.rs, choice.rs}`,
`crates/skate-game/src/living_world/{peds.rs, mod.rs, peds_tests.rs}`.
Verification: `peds_are_invisible_before_the_census_cull_can_remove_them`,
`ped_fade_follows_the_model_pair_like_sub_827c1188`,
`ped_spawn_fades_in_over_a_second_and_mods_can_change_or_disable_it` (skate-core) and
`living_world_peds_spawn_with_a_draw_fade_that_starts_transparent` (skate-game); skate-core living_world 91/91,
skate-game living_world 28/28. Not yet seen in game. Open: the `ISecurityGuard` vfunc +4 body (what a guard does
when beyond), the census gate on the focus skater (`+52` vfunc 4, `+1904` bit 0x8) and the near-range override
(`+612` / `+616`) in `sub_827C1188`; our observer is still the deck, retail's is the skater's `+52` position.

## NPC draw distance (QoL, not retail)

**Problem.** User request, 2026-10-05: "Lets add an option in the settings to extend the distance of culling (non
retail QoL feature". Decisions: one option "NPC draw distance" in the graphics settings, Retail (default) / 1.5x /
2x / 3x, for peds, NPC skaters and cars together, keeping the density (caps scale with the covered area, so 2x
range = up to 4x NPCs). Retail has no such option; this is an engine QoL setting, not a retail claim.
User request, 2026-10-07: a step so no peds spawn; decision (user): a "None" step on this option, first in the
list, which turns off NPC skaters, peds and cars together (`DrawDistance::NONE` = 0; `LivingWorldSettings::
user_npcs_off` sets every kind's `enabled` false, so live NPCs despawn with reason Disabled; ranges stay retail;
kept across mod resets like the draw distance; saved with the other graphics settings). Test: the draw distance
test in `living_world/tests.rs` (config off, kept on mod reset, a running population spawns 0 / 0 / 0).

**Change.**
- `skate-core::living_world::draw_distance::DrawDistance` (new): one multiplier `m` on top of the retail
  data-driven values, never written into them. Distances x m: both census circles of peds and cars (spawn ring
  inner / outer, cull radius, forward offset; speed keys stay), the initial populate ring (8-80 m), the ambient
  skater spawn ring (60-90 m) and cull (120 m). Counts x m^2: census caps (through the density, so the cap at a
  point is `max_population x density x m^2`), the entity pools (peds 31, vehicles 15 and the initial 15), the
  attempts / spawns per pass and of the initial populate, the skater desired count, AI cap, character pool,
  candidate line count and the ambient skater slots (slot 0 stays the player's). Not scaled: speed keys, the
  5 m / 10 m skater spacing, the 1000 m height cull, all fades in time, the census rotation and cycle phases.
  Values that are not finite or not positive mean retail; the rest is clamped to 0.25..4.
- `PopulationConfig::draw_distance` (1.0 = retail). `LivingWorld::step` builds a scaled copy only when the
  multiplier is not 1; at 1x it runs `self.config` itself, so retail is the unchanged code path.
- Ped draw fade (`peds::ped_draw_alpha`): the model's 45 / 55 m pair (or the configured default) x m, so the
  fade still ends before the scaled census cull (70 m near edge x m). The ped LOD switch stays at the retail
  distances (far peds keep the cheaper LOD). NPC skaters fade by time (1 s), so only their ranges scale. Cars
  have no draw fade; their census cull scales like the peds'.
- Settings: `LivingWorldSettings::npc_draw_distance` (in effect; a mod reads and sets it) and
  `user_npc_draw_distance` (the player's menu choice). `apply` writes the sanitised value into the core config
  every step. `reset_mod_overrides()` (mod disabled) restores every mod-changeable field to retail and the draw
  distance to the player's own choice, keeping the session flags.
- Menu: GRAPHICS row "NPC draw distance" (Left / Right or Enter cycles Retail, 1.5x, 2x, 3x), saved in
  `settings/graphics.json` (`npc_draw_distance`, missing or unknown = Retail) and applied at start. The row
  reads "2x  (not retail)", and changing it shows: "NPC draw distance is a QoL option, not retail: peds, NPC
  skaters and cars appear farther out, with more of them to keep the density. Costs frame time."

**Multiplayer.** The population authority (standalone or a future host) owns the multiplier: it decides what
exists for everyone, and the records carry positions, not ranges. A per-client draw distance must stay out of
the authority's simulation: a client would only hide or fade what it was sent beyond its own range (render
side, like the ped fade), never feed its value into `PopulationConfig`. A Client role runs no rules today, so
its setting only changes its ped fade. No networking was added.

**Files.** `crates/skate-core/src/living_world/{draw_distance.rs (new), config.rs, mod.rs, population.rs,
tests.rs}`, `crates/skate-game/src/living_world/{mod.rs, peds.rs, tests.rs}`, `crates/skate-game/src/graphics_menu.rs`.
No vehicle file changed (cars read the scaled census circle in the core).

**Verification.**
- 1x behaviour-identical: the decision stream (every spawn / despawn record with position, heading, seed,
  choice, plus the final counts) of the core's seeded 100 s run (player circling at 8 m/s, skaters, peds, cars;
  seeds 1234, 99, 7) was dumped before the change and after it: byte-identical (421,816 bytes, same SHA-256).
  Kept as tests: `draw_distance_retail_runs_the_unchanged_config` (no scaled copy at 1x; explicit 1.0 gives the
  same stream as the default) and, in skate-game, the same through the settings path.
- `draw_distance_2x_doubles_every_range_and_quadruples_the_caps`: every radius 2x, density / pools / budgets
  4x, skaters 120 / 180 / 240 m with 12 desired; a still player gets 60 peds instead of 15, all spawns inside
  the scaled ring; the retail config is untouched.
- `draw_distance_keeps_the_fade_before_the_cull_at_every_step`: at 1x, 1.5x, 2x and 3x the scaled ped fade
  ends before the near edge of either cull circle (camera up to 10 m nearer), and every kind spawns inside its
  cull.
- skate-game: `living_world_npc_draw_distance_setting_scales_population_and_mods_reset_to_the_players_choice`,
  `living_world_ped_fade_scales_with_the_draw_distance`.
- Results: skate-core living_world 95/95, skate-game living_world 30/30, `cargo build --locked` (dev) ok with
  no new warnings. Not yet seen in game.

**Open.** The frame cost at 2x / 3x (up to 4x / 9x NPCs) is not measured. Retail pools (31 peds, 15 cars) are
memory limits on the console; scaled by area here, which is a choice of this option. Skater lines are only
loaded for the current district, so at 3x the skater ring (180-270 m) may find few lines.

## Startup freeze from the first-play fixes, 2026-10-05

**Problem.** A release build with the fixes above (snapshot of the uncommitted tree) froze at startup with no
rendering; Windows ended it as a hung window. The build without the fixes started.

**Cause: not the code.** The freeze run happened while another worktree compiled `skate-game` (three rustc
processes at about 4 GB each) on the same HDD that holds the repo, the assets and the target dirs; the disk sat at
100 %. Evidence: the map parse took 8165 ms in the frozen run against 2097 / 2460 ms in normal runs, audio init
frames were about 20 s apart, and the process used little CPU (waiting on I/O, not spinning). Reading the diff
found nothing that blocks before the living-world load (no locks, waits, ordering cycles; the new systems only
act once the population exists).

**Verification.** The same exe, run muted once with no build running: `LIVING_WORLD data DownTown` 28 s after the
first log line (baseline build 42 s), then traffic, peds and NPC skater lines, `tick 600` reached. `roads 46`
instead of 76 is the district road filter ("Cars flying off"). No code change. Lesson: do not judge a startup
while a build runs on the same disk.

## Population follows the board, not the player, 2026-10-05

**Problem.** In the user's session (log `game-20261005-114026`), at 17:45:02 every car and NPC skater disappeared
and all 15 peds respawned out of earshot. That lasted until 17:45:15, when the board was back in hand. The video
shows the player walking without the board. FPS fell from about 280 to about 40 over the same 13 s.

**Cause.** `gather_observers` built the census focus from the deck body. When the player walks off the board and
leaves it behind (or the board is far away), the census culled everything around the player and populated around
the board. Retail census focus is the focused skater: `sub_826BDB50` -> `sub_826BE7D0` (position, skater `+52`
component vfunc 20) and `sub_826BE870` (velocity, zeroed when |v|^2 <= 1e-4 at `0x8209BE90`) [code], see "NPCs
vanishing in view".

**Evidence.** WORLD_AUDIO lines 17:45:02 to 17:45:15: `cars 0`, `skaters 0`, `peds 15/0`. REPORT_META shows
`physical:BipedGround` the whole time. The jump from 9 cars to 0 within one second means the focus moved more than
the 110 m car cull radius in under a second, which walking cannot do.

**Change.** The focus is now the player's character: the skeleton's physical centre of mass and its velocity
(`board_frames.centre_of_mass` / `com_velocity`, the Skeleton16144/16176 values that `render_pose.rs` publishes
into the reckoning fields). These are valid on board, walking, in the air and in a bail. The deck is only the
fallback when no skater is loaded. Near-zero velocity is zeroed as retail does. The non-finite guard is unchanged.
The observer list stays one entry per player. Peds' draw fade and the NPC draw distance keep using the camera,
as in retail (`sub_827C1188` fades by camera distance).
Also fixed: after a map reload or respawn into a new generation, the world tick restarts at 0 but the debug
readouts kept the old world's last-report tick, so `LIVING_WORLD tick ..`, the peds readout and the traffic summary
never logged again. A shared `report_due` restarts with the world. The population line now also prints the focus
position and speed.

**FPS drop (not proven).** The fixed physics step kept 60 ticks/s through the drop (REPORT_META physics_tick
2513 -> 3306), so the cost went to frame time. The log has no deck position and no spawn counts, because the
summary was silenced by the reload bug above. Likely causes: (a) spawn churn, if the deck was moving fast (flung
or falling), so that peds spawned around it fell behind the cull every pass and respawned with fresh looks; or
(b) the deck itself costing physics time far from the player. With the focus on the player, (a) cannot happen
from a left board. The next debug session will show it (`focus` and `spawned/despawned` in the population line).

**Files.** `crates/skate-game/src/living_world/mod.rs` (`player_focus`, `local_focus`, `report_due`,
`gather_observers`, population readout), `peds.rs` and `vehicles.rs` (readout timing), `tests.rs` (3 tests).
`npc_skaters.rs` `log_readout` has the same reload bug and still needs `report_due` (left for the NPC skater work).

**Verification.** `cargo test --locked -p skate-game --bin skate3rust living_world`: 37 passed, including
`living_world_off_board_player_keeps_the_population_around_the_player_not_the_board` (a player walks away from a
deck 500 m off; every ped and car stays within cull range of the player and nothing ever spawns near the deck),
`living_world_focus_is_the_character_and_still_velocity_is_zero`, and
`living_world_debug_summaries_restart_with_a_new_world`. Not seen in game yet.

## Frame drop with the board thrown away (hidden board scanned the whole map), 2026-10-05

- **Problem:** throwing the board and walking away dropped the frame rate to 4 to 20 FPS; calling the board back
  fixed it at once (user play test, 2026-10-05). The living world was not the cause: the log shows the population
  steady and physics at 60 ticks per second.
- **Root cause:** beyond `MaxDistance` (30 m) the board controller hides the board (state 3: parked 1000 m above the
  player, every board collision volume disabled). The solve keeps only enabled volumes and called
  `BoardWorld::query_primitives` with an empty list; with no volumes the search box was `None`, and
  `candidate_ranges(None)` means every triangle, so each tick walked all of the map's collision triangles for an
  empty result. The cost jumps once at the hide distance; it does not grow with distance.
- **Evidence:** a headless test on the real DownTown map (stock graphs, pad-driven: step off, drop, walk away)
  measured 2 to 3.5 ms per tick with the board dropped and 27 to 29 ms once hidden, 22 ms of it in that query.
- **Change:** `query_primitives` returns right after its buffer reset when the volume list is empty. Pure
  optimisation: the old loop gave the same empty result and the same buffer state. It covers every caller (board,
  skeleton, prop world). The bug is in shared physics and exists on `main`; it is fixed in this PR because walking
  away from the board is part of the living world's tests.
- **Files:** `crates/skate-core/src/physics/board_world.rs`, `crates/skate-core/src/physics/board_world/tests.rs`,
  `crates/skate-game/src/physics/board_away_tests.rs` (new), `crates/skate-game/src/physics.rs` (test module).
- **Verification:** `empty_volume_query_is_empty_and_resets_the_previous_result` passes with and without the early
  return (identical behaviour); the ignored data-gated `hidden_board_tick_costs_no_more_than_a_dropped_board`
  passes with the fix (2.73 ms hidden vs 2.66 ms dropped) and fails without it (29.3 ms). Not yet confirmed in game.
- **Open:** in one run a board lying 24 m away cost about 1 ms more per tick than at 0 to 10 m, which looked tied
  to where it landed rather than distance; not investigated.
