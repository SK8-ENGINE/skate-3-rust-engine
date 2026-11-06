# 42. Challenges (own the spot first)

Branch `world/challenges` (from `world/replay`; becomes its own upstream PR once #52 has landed). Status: research done,
setup export, the challenge HUD movies and the own the spot state machine (skate-core) landed; the engine side (runner, timers, objectives, HUD, completion) is next.

## Problem

Retail Skate 3 has about 490 challenges (races, own the spots, films, hall of meat and more). The engine had none: the
Challenge Map could not list them, the minimap challenge icon groups had no source, and the only challenge data the
setup exported was the teleport family (doc 24, `teleporters.py`).

## How retail stores and runs a challenge

- **Bank:** `db.big` `data/db/challengebanks/main` holds the challenge rows (class challenge_global_data). Each row
  inherits its family's defaults through its parent chain (`ots_dwtn_01` < `own_the_spots`). The row key string is the
  stable id; the same key names the local data vault (`challenge_local_data/<key>`), the hull
  (`missions.big data/content/missions/<key>/`) and the boundary polygon (`miscload.big data/boundary/<key>.xml`).
- **Logic:** compiled Lua 5.1 state graphs (`db.big data/db/challenge_stategraph/<graph>.bin`, 41 graphs, one chunk
  per message handler). States change through `ChangeState("name")`, deferred to the next queue flush; the old state
  gets stateexit, the new one stateenter. Messages are delivered at once to every registered challenge, current
  state's handlers first, then the global state's. Timers count whole 1/60 s ticks and post timerexpired.
- **Result:** the challenge's own script decides the tier and calls `CompleteChallenge(challenge, result)` with
  Abandoned 0, Owned 1, Killed 2. A saved tier only ever moves up.
- **Own the spot:** when every objective of the owned group is complete, accumulated points >= the killed it group's
  PointRequirement give killed it, else owned it. The owned it / killed it banners are the `challenge/ownthespot` and
  `challenge/killedit` movies (they take no text or score).

Port decision (user, 2026-11-06): each family is re-implemented in Rust with the retail graph as the specification
and the retail values as data; no retail Lua interpreter. Mods add challenges through the Lua SDK API 2.

## Change: the setup export (`tools/asset_pipeline/challenges.py`)

Writes `assets/private/challenges.json` (format `skate3rust.challenges`, version 1) for the families in `FAMILIES`
(today `own_the_spots`), from the disc and the DLC packs:

| Field | Source |
|---|---|
| `id`, `family`, `map`, `district` | bank row key, parent chain, the boundary's district |
| `title_id`, `title`, `description_id` | bank row text ids (resolved title when the text tables are available) |
| `boundary` | OTSTriggerBoundary (bank) -> the volume in the hull |
| `scoring` | OTSScoringBoundary (local data) -> the volume in the hull |
| `spot` | the spot objective's completion condition (Lua text in the bank, `ObjectiveTrigger.EnteredVolume("<id>", "DISTRICT\|name")`, research b840) -> the volume in the hull: a line must pass through it to count |
| `start` | StartLocation (local data) -> the 0x00EB0009 locator in the hull, else the district's global locators |
| `location`, `sign_up_radius`, `discovery_radius` | Location (the challenge locator, global locators) and the family's GlobalActivateRadius (10 m sign-up sphere) / DiscoveryActivateRadius (100 m: leaving it abandons a run, research b843) |
| `owned_points`, `killed_points` | PointRequirement of the ObjectivesOwned / ObjectivesKilledIt groups |

Details:
- Objective group rows whose key string is not in `names.txt` keep their hash as the key (`Hash_<16 hex>`); the
  killed it groups are such rows, so the lookup accepts both.
- `map_volumes.arena_volumes` / `district_volumes` gained `strict` (default on, so the map `.triggers` export is
  unchanged). The challenge export reads hulls with it off: `ots_indu_02`'s boundary box is about 20 m larger than its
  stored bounds on two axes (centre matching), so it is kept and flagged `bounds_mismatch`. Which of the two retail
  tests (box, bounds or both) is open; the engine port decides from retail code.
- Kept DLC maps keep their challenge rows across a refresh without the DLC folder (`retained.py`).
- Result on the disc: 16 own the spots on 3 maps; `ots_ind_01` is left out (no StartLocation, a leftover beside
  `ots_indu_01..05`). Export time 0.9 s; two runs give identical output.

## Change: the challenge HUD movies (`tools/prepare_hud.py`)

`MOVIES` gained the own the spot HUD components and banners, written beside the existing HUD movies as
`private/hud/runtime/<file>`: `hud2/hudtimer`, `hud2/hudcountdown`, `hud2/hudobjective2`, `hud2/dpo`,
`challenge/ownthespot`, `challenge/killedit` (FE templates per research b834; the banners per b836 / b837).
The three earlier movies (trickdisplay, hudcentral, hudminimap) come out byte-identical to an installed copy.

## Change: the own the spot state machine (`crates/skate-core/src/challenges/`)

Engine independent (no ECS), deterministic, plain data: `ots::Ots` takes engine events (HUD loaded, trigger enter /
exit, all objectives complete / not enough points with the points, wipeout / over, discovery exit, highlight reel
complete, abandon / terminate / restart) and returns commands (HUD show / update, timer HUD, messages, teleport, reset
trigger volumes, reset objectives, evaluation on / off, point requirement, high score, play reel, complete with a
`Completion`). It follows the retail graph (research b838) and runner rules (b832):
- a state change is queued and applied after the message; the new state's enter runs then; current state handlers
  run before the global ones;
- timers (`challenges::timers`) hold trunc(seconds * 60) ticks and lose one per 60 Hz tick;
- the target is the owned score until the personal best reaches it, then the killed it score, then the personal
  best; a win completes the challenge only when that tier moves up, otherwise the challenge loops back to the start;
- not enough points resets the objectives and freezes scoring 0.5 s (no fail); a bail pauses scoring and resets on
  wipeout over; leaving the boundary starts the out-of-bounds countdown (expiry abandons), coming back cancels it;
- the killed it / owned it banner timers read the two display times crossed, as retail does;
- `Record` keeps the best completion, which only moves up (retail 82C1D878).
Not ported yet: SimulPlay, online scores, the intro presentation, highlight groups. Open: where the bank keeps the
two display times and the out-of-bounds time (not under those names' hashes; the family row has unnamed floats 10,
100 and 250).

## Change: own the spot in play (`crates/skate-game/src/challenges.rs`)

- Loads `private/challenges.json` once; on each map, a run started with `SKATE_CHALLENGE=<id>` (dev start until the
  retail sign-up flow is ported) arms the challenge's three volumes, starts `ots::Ots` with the session's best score,
  and each frame feeds it the player's trigger enter / exit events, the banked sequences and 60 Hz timer ticks.
- Points (retail rule, research b841): retail posts `cMsgSequenceComplete` (SequenceScore, Wipedout,
  UsedSessionMarker) when a sequence ends. Our scoring runtime now queues every banked sequence
  (`scoring_runtime::BankedSequence`, at the bank point beside `SCORING_BANK`); a sequence counts only when scoring
  is on, it did not end in a bail, and the player entered the spot volume during it; the spot clears after every
  sequence (activation mode 5). The group total against the current target gives all objectives complete or not
  enough points.
- Commands: teleport to the start locator (`travel_ex`, on board), objectives reset / evaluation / requirement, the
  completion recorded per challenge for the session (the tier only moves up). HUD commands are logged until the
  HUD movies run; the HUD counts as loaded at once and the highlight reel completes at once.
- Leaving the discovery sphere (distance from the challenge locator > `discovery_radius`, checked every frame as
  retail 8255BEB8 does) sends exit discovery, which abandons the run.
- Placeholders, not retail values: the out-of-bounds time (10 s; the family's 10 / 100 / 250 floats are radii) and both banner times (3 s), until the bank fields
  are located.

## Files

- `tools/asset_pipeline/challenges.py` (new), `tools/asset_pipeline/test_challenges.py` (new)
- `tools/asset_pipeline/install.py` (the export step, after teleporters)
- `tools/asset_pipeline/map_volumes.py` (`strict`), `tools/asset_pipeline/retained.py`, `test_retained.py`
- `tools/prepare_hud.py` (the six challenge movies)
- `crates/skate-core/src/challenges/` (mod, timers, ots, ots/tests), `crates/skate-core/src/lib.rs`
- `crates/skate-game/src/challenges.rs` (new), `scoring_runtime.rs` (banked sequences), `main.rs`, `app.rs`

## Verification

- `python -m unittest tools.asset_pipeline.test_challenges tools.asset_pipeline.test_map_volumes
  tools.asset_pipeline.test_retained`: OK.
- Export against the disc: `ots_dwtn_01` start (-132.75, 30.98, 143.53), owned 1000, killed it 1500, boundary
  `ots_dwtn_01_challengeboundary_01`, spot `ots_dwtn_01_scoreboundary_01`.

## Moddability

Challenge ids are the retail key strings, so a mod can override one challenge's values or add its own challenge with
its own volumes, start and points through the content layer; the engine side will expose the same fields to the Lua
SDK API 2 and remove a mod's challenges when it is disabled.

## Open questions

- Who opens the owned it / killed it screens (FE templates 26 / 27): not found statically; a recomp trace on
  OpenScreen during an own-the-spot win is planned.
- The display time fields and the remaining unnamed bank fields.
- Box versus stored bounds for the trigger test (`ots_indu_02`).
- Per-challenge high score layout in the retail save.
